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

use crate::draw::{Face, Fonts, Rgba};
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
    // The advance a target group's label actually takes: `model.rs`'s
    // `block_height` and `paint.rs`'s `draw_panel` must read the very same
    // number here or one budgets a row position the other doesn't draw at
    // (beta.5 fix B). `at()` falls back to `target_px.ceil()` -- the old,
    // too-small budget -- for a `Theme` built with no `Fonts` at hand;
    // `with_fonts` overwrites it with the real ascent+descent DejaVu Sans
    // reports at `target_px`, and every real render path calls it.
    pub label_line_h: u32,
    pub nudge: i32,         // 4 (HUD mode arrow step; also the halo's outward offset)
    // 24 (HUD mode arrow step with Shift). `HudMode::handle` takes its own
    // unscaled 4/24 (main.rs's NUDGE/SHIFT_NUDGE) rather than these two --
    // the step has to feel the same size in key presses regardless of render
    // scale -- so only `nudge` (the halo's offset, a drawn size like any
    // other) is read by `paint.rs`. Kept here anyway for parity with `nudge`
    // and because it is still spec §4.2's own scaled value, for a future
    // reader that wants it.
    #[allow(dead_code)]
    pub shift_nudge: i32,
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
        let target_px = 11.5 * scale;
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
            target_px,
            kind_px: 10.5 * scale,
            // No `Fonts` at hand here -- `with_fonts` is the real answer and
            // every render path calls it; this fallback only keeps a
            // fonts-less `Theme::at` (most of this crate's own tests) usable.
            label_line_h: target_px.ceil() as u32,
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

    /// Recomputes `label_line_h` from `fonts`' real metrics at `target_px`,
    /// so `model::block_height`'s row budget and `draw_panel`'s actual line
    /// advance are the same number and can never drift apart again. Call
    /// once, right after loading the fonts (`main.rs` does; a test that
    /// paints a Timers block with group labels should too).
    pub fn with_fonts(mut self, fonts: &Fonts) -> Theme {
        let (ascent, descent) = crate::draw::line_metrics(fonts, Face::Sans, self.target_px);
        self.label_line_h = ascent + descent;
        self
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec §6's "Sizes" acceptance criterion, and the §4.2 table it comes
    /// from. The one criterion in §6 with no assertion anywhere: `model.rs`
    /// covers scale 1.5 only through a block *width* of 435, which is not
    /// what the criterion says, and a row height that quietly stopped
    /// scaling would pass every other test in the suite.
    #[test]
    fn the_spec_table_is_transcribed_at_scale_one() {
        let t = Theme::at(1.0);
        assert_eq!(t.scale, 1.0);
        assert_eq!(t.row_h, 24, "§4.2: a row is 24 px tall");
        assert_eq!(t.row_gap, 4, "§4.2: 4 px gap above");
        assert_eq!(t.row_inset, 4, "§4.2: 4 px inset from the panel edge");
        assert_eq!(t.pad_x, 8, "§4.2: 8 px horizontal text padding");
        assert_eq!(t.header_pad_y, 4, "§4.2: 4 px vertical header padding");
        assert_eq!(t.group_gap, 7, "§4.2: a target label sits 7 px above its first row");
        assert_eq!(t.radius, 3, "§4.2: 3 px panel corner radius");
        assert_eq!(t.bar_radius, 2, "§4.2: 2 px bar radius");
        assert_eq!(t.border, 1, "§4.2: 1 px panel border");
        assert_eq!(t.text_px, 13.0, "§4.2: 13 px row text");
        assert_eq!(t.number_px, 12.5, "§4.2: 12.5 px numbers");
        assert_eq!(t.header_px, 12.0, "§4.2: 12 px header text");
        assert_eq!(t.target_px, 11.5, "§4.2: 11.5 px target label");
        assert_eq!(t.kind_px, 10.5, "§4.2: 10.5 px kind label");
    }

    #[test]
    fn scale_multiplies_every_size() {
        // §6: "at `scale = 1.5` they are 36 and 6".
        let t = Theme::at(1.5);
        assert_eq!(t.row_h, 36);
        assert_eq!(t.row_gap, 6);
        // And the rest of the table with them, since `scale` multiplies
        // "every one of them" (§4.2) and nothing else may opt out.
        assert_eq!(t.pad_x, 12);
        assert_eq!(t.radius, 5, "3 * 1.5 rounds to 5");
        assert_eq!(t.border, 2, "1 * 1.5 rounds to 2");
        assert_eq!(t.text_px, 19.5);
        assert_eq!(t.nudge, 6);
        assert_eq!(t.shift_nudge, 36);
    }
}
