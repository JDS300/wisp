// SPDX-License-Identifier: MIT
//! The HUD's layout: blocks, anchors, offsets — the part of the config file
//! that only the HUD reads, and the reason the file became TOML.
//!
//! `Layout` and its parts are plain serde types over the config's `[hud]`
//! table and `[[block]]` array. Defaulting is pushed onto each field with
//! `#[serde(default)]` rather than a hand-written parser, so a config written
//! for a newer Wisp with a field this one does not know keeps its known
//! fields and a config missing a field entirely gets a sane one.

use serde::{Deserialize, Serialize};

/// Where a block sits relative to the display it is drawn on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Anchor {
    #[default]
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

/// What a block draws: a meter or a timers list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlockKind {
    Meter,
    Timers,
}

/// A meter's metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Shows {
    #[default]
    Damage,
    Healing,
}

/// The window a meter totals over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Segment {
    #[default]
    Fight,
    Session,
}

/// One `[[block]]` entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub kind: BlockKind,
    #[serde(default)]
    pub shows: Shows,
    #[serde(default)]
    pub segment: Segment,
    #[serde(default)]
    pub anchor: Anchor,
    #[serde(default)]
    pub offset: [i32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<u32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

impl Block {
    /// A block of `kind`, otherwise defaulted, offset `[0, 0]`.
    pub fn new(kind: BlockKind) -> Block {
        Block {
            kind,
            shows: Shows::default(),
            segment: Segment::default(),
            anchor: Anchor::default(),
            offset: [0, 0],
            width: None,
            rows: None,
            hidden: false,
        }
    }

    /// The explicit width, else 290 for a meter and 330 for timers (spec
    /// §4.6's example file).
    pub fn width(&self) -> u32 {
        self.width.unwrap_or(match self.kind {
            BlockKind::Meter => 290,
            BlockKind::Timers => 330,
        })
    }

    /// The explicit row count, else 8 for a meter and 12 for timers.
    pub fn rows(&self) -> u32 {
        self.rows.unwrap_or(match self.kind {
            BlockKind::Meter => 8,
            BlockKind::Timers => 12,
        })
    }
}

/// The chord that toggles the HUD when the config does not say otherwise.
pub const DEFAULT_CHORD: &str = "ctrl+shift+grave";

fn one() -> f32 {
    1.0
}

fn default_chord() -> String {
    DEFAULT_CHORD.to_string()
}

/// The `[hud]` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hud {
    #[serde(default = "one")]
    pub scale: f32,
    #[serde(default = "default_chord")]
    pub chord: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

impl Default for Hud {
    fn default() -> Hud {
        Hud { scale: 1.0, chord: DEFAULT_CHORD.to_string(), output: None }
    }
}

/// The HUD's whole layout: `[hud]` plus every `[[block]]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    #[serde(default)]
    pub hud: Hud,
    #[serde(default, rename = "block")]
    pub blocks: Vec<Block>,
}

impl Layout {
    /// One damage meter at top-left `[20, 120]` and one timers block at
    /// top-right `[20, 120]`: the spec's example file minus the hidden
    /// healing meter. What a config file that says nothing about the layout
    /// gets.
    pub fn default_layout() -> Layout {
        let mut meter = Block::new(BlockKind::Meter);
        meter.anchor = Anchor::TopLeft;
        meter.offset = [20, 120];

        let mut timers = Block::new(BlockKind::Timers);
        timers.anchor = Anchor::TopRight;
        timers.offset = [20, 120];

        Layout { hud: Hud::default(), blocks: vec![meter, timers] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spec_example_parses_and_round_trips() {
        let text = r#"
[hud]
scale = 1.0
chord = "ctrl+shift+grave"

[[block]]
kind = "meter"
shows = "damage"
segment = "fight"
anchor = "top-left"
offset = [20, 120]
width = 290
rows = 8

[[block]]
kind = "meter"
shows = "healing"
anchor = "top-left"
offset = [20, 400]
hidden = true

[[block]]
kind = "timers"
anchor = "top-right"
offset = [20, 120]
width = 330
rows = 12
"#;
        let l: Layout = toml::from_str(text).unwrap();
        assert_eq!(l.blocks.len(), 3);
        assert_eq!(l.blocks[1].shows, Shows::Healing);
        assert_eq!(l.blocks[1].segment, Segment::Fight, "defaulted");
        assert!(l.blocks[1].hidden);
        assert_eq!(l.blocks[1].width(), 290, "a meter's default width");
        assert_eq!(l.blocks[2].rows(), 12);
        let again: Layout = toml::from_str(&toml::to_string(&l).unwrap()).unwrap();
        assert_eq!(again, l);
    }

    #[test]
    fn defaults_are_the_spec_example_minus_the_hidden_meter() {
        let l = Layout::default_layout();
        assert_eq!(l.hud, Hud::default());
        assert_eq!(l.hud.chord, "ctrl+shift+grave");
        assert_eq!(l.blocks.len(), 2);
        assert_eq!(
            (l.blocks[0].kind, l.blocks[0].anchor, l.blocks[0].offset),
            (BlockKind::Meter, Anchor::TopLeft, [20, 120])
        );
        assert_eq!(
            (l.blocks[1].kind, l.blocks[1].anchor, l.blocks[1].offset),
            (BlockKind::Timers, Anchor::TopRight, [20, 120])
        );
        assert_eq!(Block::new(BlockKind::Timers).rows(), 12);
    }
}
