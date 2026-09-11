// SPDX-License-Identifier: MIT
//! Every size and colour `wisp-hud` draws with, transcribed once from spec
//! §4.2 ("The look — Console") at scale 1.0, plus the few implementation
//! sizes the paint step needs that §4.2 doesn't name a pixel value for (the
//! HUD-mode outline's offset and thickness, and the dash unit) -- those are
//! transcribed from this task's own brief instead, so `paint.rs` never
//! spells out a literal of its own either.
//!
//! `Theme::at(scale)` does the multiplying once; every other file just reads
//! fields.

use crate::draw::Rgba;
use wisp_proto::{DamageType, TimerKind};

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub scale: f32,
    // sizes, already multiplied by scale and rounded:
    pub row_h: u32,         // 24
    pub row_gap: u32,       // 4
    pub row_inset: u32,     // 4  (row's inset from the panel edge)
    pub pad_x: u32,         // 8
    pub header_pad_y: u32,  // 4
    pub group_gap: u32,     // 7  (above a target label)
    pub radius: u32,        // 3, the panel's corner radius
    pub bar_radius: u32,    // 2, the bar's corner radius
    pub border: u32,        // 1
    pub text_px: f32,       // 13
    pub number_px: f32,     // 12.5
    pub header_px: f32,     // 12
    pub target_px: f32,     // 11.5
    pub kind_px: f32,       // 10.5
    pub nudge: i32,         // 4 (HUD mode arrow step; also the halo's outward offset)
    pub shift_nudge: i32,   // 24 (HUD mode arrow step with Shift)
    // HUD-mode outline geometry (this task's brief, not §4.2's table):
    pub outline_offset: u32,            // 3, dashed/solid outline drawn this far outside a block
    pub outline_selected_thickness: u32, // 2, the selected block's solid outline
    pub dash: u32,                       // the dash unit passed to a dashed `stroke_rect`
    // colours:
    pub panel: Rgba,        // rgba(0x080a0e, 0.74)
    pub panel_border: Rgba, // rgba(0xffffff, 0.08)
    pub header: Rgba,       // rgba(0x000000, 0.35)
    pub header_text: Rgba,  // 0xc9d1d9
    pub text: Rgba,         // 0xe8edf2
    pub target_text: Rgba,  // 0xa8b3bf
    pub kind_text: Rgba,    // rgba(0xffffff, 0.55)
    pub damage_bar: Rgba,   // 0x6b7a8c
    pub healing_bar: Rgba,  // 0x7cd992
    pub you_bar: Rgba,      // 0x8fe3ff
    pub warning: Rgba,      // 0xffcc66
    pub critical: Rgba,     // 0xff5c5c
    pub bar_alpha: f32,             // 0.55
    pub you_bar_alpha: f32,         // 0.80
    pub estimated_text_alpha: f32,  // 0.60
    pub estimated_bar_alpha: f32,   // 0.28
    pub gone_text_alpha: f32,       // 0.50
    // HUD mode:
    pub outline: Rgba,          // rgba(0x8fe3ff, 0.55) dashed; selected: 0x8fe3ff solid 2 px
    pub outline_selected: Rgba,
    pub halo: Rgba,             // rgba(0x8fe3ff, 0.18), 4 px outside the selected block
    pub ghost: Rgba,            // rgba(0xffffff, 0.25) dashed, ghost text rgba(0xffffff, 0.45)
    pub ghost_text: Rgba,
    pub tag_bg: Rgba,           // rgba(0x080a0e, 0.85)
}

/// Wisp's own presentation thresholds (Spec 1), unchanged by Spec 5.
pub const WARNING_SECS: i64 = 10;
pub const CRITICAL_SECS: i64 = 5;

impl Theme {
    pub fn at(scale: f32) -> Theme {
        let size = |v: f32| (v * scale).round() as u32;
        let signed = |v: f32| (v * scale).round() as i32;
        Theme {
            scale,
            row_h: size(24.0),
            row_gap: size(4.0),
            row_inset: size(4.0),
            pad_x: size(8.0),
            header_pad_y: size(4.0),
            group_gap: size(7.0),
            radius: size(3.0),
            bar_radius: size(2.0),
            border: size(1.0),
            text_px: 13.0 * scale,
            number_px: 12.5 * scale,
            header_px: 12.0 * scale,
            target_px: 11.5 * scale,
            kind_px: 10.5 * scale,
            nudge: signed(4.0),
            shift_nudge: signed(24.0),
            outline_offset: size(3.0),
            outline_selected_thickness: size(2.0),
            dash: size(6.0),
            panel: Rgba::rgba(0x080a0e, 0.74),
            panel_border: Rgba::rgba(0xffffff, 0.08),
            header: Rgba::rgba(0x000000, 0.35),
            header_text: Rgba::rgb(0xc9d1d9),
            text: Rgba::rgb(0xe8edf2),
            target_text: Rgba::rgb(0xa8b3bf),
            kind_text: Rgba::rgba(0xffffff, 0.55),
            damage_bar: Rgba::rgb(0x6b7a8c),
            healing_bar: Rgba::rgb(0x7cd992),
            you_bar: Rgba::rgb(0x8fe3ff),
            warning: Rgba::rgb(0xffcc66),
            critical: Rgba::rgb(0xff5c5c),
            bar_alpha: 0.55,
            you_bar_alpha: 0.80,
            estimated_text_alpha: 0.60,
            estimated_bar_alpha: 0.28,
            gone_text_alpha: 0.50,
            outline: Rgba::rgba(0x8fe3ff, 0.55),
            outline_selected: Rgba::rgb(0x8fe3ff),
            halo: Rgba::rgba(0x8fe3ff, 0.18),
            ghost: Rgba::rgba(0xffffff, 0.25),
            ghost_text: Rgba::rgba(0xffffff, 0.45),
            tag_bg: Rgba::rgba(0x080a0e, 0.85),
        }
    }

    /// The kind palette (pastel), §4.2: a `dot`'s colour comes from its
    /// damage type, four of which (unresistable, chromatic, prismatic,
    /// physical) share the plain debuff grey.
    pub fn kind_colour(&self, kind: TimerKind, damage_type: Option<DamageType>) -> Rgba {
        match kind {
            TimerKind::Mez => Rgba::rgb(0xff79c6),
            TimerKind::Slow => Rgba::rgb(0x82aaff),
            TimerKind::Debuff => Rgba::rgb(0x94a3b8),
            TimerKind::Dot => match damage_type {
                Some(DamageType::Fire) => Rgba::rgb(0xff8a65),
                Some(DamageType::Cold) => Rgba::rgb(0x80deea),
                Some(DamageType::Poison) => Rgba::rgb(0x7cd992),
                Some(DamageType::Disease) => Rgba::rgb(0xc5c86a),
                Some(DamageType::Magic) => Rgba::rgb(0xb48cff),
                Some(DamageType::Corruption) => Rgba::rgb(0xd4a373),
                // Unresistable, Chromatic, Prismatic, Physical, and no damage
                // type at all: the plain debuff grey.
                _ => Rgba::rgb(0x94a3b8),
            },
        }
    }

    /// "mez", "slow", "debuff"; a dot: its damage type's name, except the
    /// four grey ones (see `kind_colour`) which read as plain "dot".
    pub fn kind_label(kind: TimerKind, damage_type: Option<DamageType>) -> &'static str {
        match kind {
            TimerKind::Mez => "mez",
            TimerKind::Slow => "slow",
            TimerKind::Debuff => "debuff",
            TimerKind::Dot => match damage_type {
                Some(DamageType::Unresistable) | Some(DamageType::Chromatic) | Some(DamageType::Prismatic)
                | Some(DamageType::Physical) | None => "dot",
                Some(other) => other.name(),
            },
        }
    }
}
