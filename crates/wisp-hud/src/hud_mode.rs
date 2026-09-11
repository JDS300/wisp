// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/hud_mode.rs
//! Spec §4.7: the state machine that turns polled keys into layout edits.
//! Pure, like `model.rs`: no display, no window -- a key and a layout come
//! in, an edited layout (maybe) and an `Action` go out.

use wisp_config::layout::{Anchor, BlockKind, Layout, Segment, Shows};

use crate::keys::Key;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HudMode {
    pub active: bool,
    pub selected: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Nothing,
    Redraw,
    SaveAndExit,
}

/// Whether `anchor` measures its offset from the screen's right edge, so the
/// arrow that visually moves a block right/left has to invert the sign on
/// the way to `offset[0]`.
fn x_reversed(anchor: Anchor) -> bool {
    matches!(anchor, Anchor::TopRight | Anchor::Right | Anchor::BottomRight)
}

/// Same as `x_reversed`, for the bottom edge and `offset[1]`.
fn y_reversed(anchor: Anchor) -> bool {
    matches!(anchor, Anchor::BottomLeft | Anchor::Bottom | Anchor::BottomRight)
}

impl HudMode {
    /// Spec §4.7's table. `shift` is whether Shift is held (24 px steps).
    /// Layout edits happen in place; `nudge`/`shift_nudge` come from the
    /// theme (unscaled: 4 and 24).
    pub fn handle(&mut self, key: Key, shift: bool, layout: &mut Layout, nudge: i32, shift_nudge: i32) -> Action {
        if !self.active {
            return if key == Key::Chord {
                self.active = true;
                self.selected = 0;
                Action::Redraw
            } else {
                Action::Nothing
            };
        }

        if key == Key::Chord || key == Key::Escape {
            self.active = false;
            return Action::SaveAndExit;
        }

        if layout.blocks.is_empty() {
            return Action::Nothing;
        }
        self.selected = self.selected.min(layout.blocks.len() - 1);
        let step = if shift { shift_nudge } else { nudge };

        match key {
            Key::Left | Key::Right => {
                let block = &mut layout.blocks[self.selected];
                let signed = if x_reversed(block.anchor) { -step } else { step };
                if key == Key::Right {
                    block.offset[0] += signed;
                } else {
                    block.offset[0] -= signed;
                }
                Action::Redraw
            }
            Key::Up | Key::Down => {
                let block = &mut layout.blocks[self.selected];
                let signed = if y_reversed(block.anchor) { -step } else { step };
                if key == Key::Down {
                    block.offset[1] += signed;
                } else {
                    block.offset[1] -= signed;
                }
                Action::Redraw
            }
            Key::Tab => {
                let len = layout.blocks.len();
                self.selected = if shift { (self.selected + len - 1) % len } else { (self.selected + 1) % len };
                Action::Redraw
            }
            Key::BracketLeft | Key::BracketRight => {
                let block = &mut layout.blocks[self.selected];
                if block.kind == BlockKind::Meter {
                    block.shows = match block.shows {
                        Shows::Damage => Shows::Healing,
                        Shows::Healing => Shows::Damage,
                    };
                    Action::Redraw
                } else {
                    Action::Nothing
                }
            }
            Key::F => {
                let block = &mut layout.blocks[self.selected];
                if block.kind == BlockKind::Meter {
                    block.segment = match block.segment {
                        Segment::Fight => Segment::Session,
                        Segment::Session => Segment::Fight,
                    };
                    Action::Redraw
                } else {
                    Action::Nothing
                }
            }
            Key::Plus | Key::Minus => {
                let block = &mut layout.blocks[self.selected];
                let current = block.rows();
                let next = if key == Key::Plus { current + 1 } else { current.saturating_sub(1).max(1) };
                block.rows = Some(next);
                Action::Redraw
            }
            Key::H => {
                let block = &mut layout.blocks[self.selected];
                block.hidden = !block.hidden;
                Action::Redraw
            }
            _ => Action::Nothing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_config::layout::Block;

    fn meter() -> Layout {
        Layout { hud: Default::default(), blocks: vec![Block::new(BlockKind::Meter)] }
    }

    #[test]
    fn outside_hud_mode_only_the_chord_matters() {
        let mut hm = HudMode::default();
        let mut l = meter();
        assert_eq!(hm.handle(Key::H, false, &mut l, 4, 24), Action::Nothing);
        assert!(!l.blocks[0].hidden, "no edit happened");
        assert_eq!(hm.handle(Key::Chord, false, &mut l, 4, 24), Action::Redraw);
        assert!(hm.active);
        assert_eq!(hm.selected, 0);
    }

    #[test]
    fn the_chord_or_escape_exits_and_saves() {
        for key in [Key::Chord, Key::Escape] {
            let mut hm = HudMode { active: true, selected: 0 };
            let mut l = meter();
            assert_eq!(hm.handle(key, false, &mut l, 4, 24), Action::SaveAndExit);
            assert!(!hm.active);
        }
    }

    #[test]
    fn arrows_move_a_top_left_block_towards_the_arrow() {
        let mut hm = HudMode { active: true, selected: 0 };
        let mut l = meter();
        l.blocks[0].anchor = Anchor::TopLeft;
        l.blocks[0].offset = [10, 10];
        hm.handle(Key::Right, false, &mut l, 4, 24);
        assert_eq!(l.blocks[0].offset, [14, 10]);
        hm.handle(Key::Left, false, &mut l, 4, 24);
        assert_eq!(l.blocks[0].offset, [10, 10]);
        hm.handle(Key::Down, false, &mut l, 4, 24);
        assert_eq!(l.blocks[0].offset, [10, 14]);
        hm.handle(Key::Up, false, &mut l, 4, 24);
        assert_eq!(l.blocks[0].offset, [10, 10]);
        hm.handle(Key::Right, true, &mut l, 4, 24);
        assert_eq!(l.blocks[0].offset, [34, 10], "shift steps by shift_nudge");
    }

    #[test]
    fn arrows_move_a_bottom_right_block_towards_the_arrow() {
        let mut hm = HudMode { active: true, selected: 0 };
        let mut l = meter();
        l.blocks[0].anchor = Anchor::BottomRight;
        l.blocks[0].offset = [10, 10];
        hm.handle(Key::Right, false, &mut l, 4, 24);
        assert_eq!(l.blocks[0].offset[0], 6, "right decreases offset[0] for a right anchor");
        hm.handle(Key::Left, false, &mut l, 4, 24);
        assert_eq!(l.blocks[0].offset[0], 10);
        hm.handle(Key::Down, false, &mut l, 4, 24);
        assert_eq!(l.blocks[0].offset[1], 6, "down decreases offset[1] for a bottom anchor");
        hm.handle(Key::Up, false, &mut l, 4, 24);
        assert_eq!(l.blocks[0].offset[1], 10);
    }

    #[test]
    fn arrows_move_a_centered_block_like_top_left() {
        let mut hm = HudMode { active: true, selected: 0 };
        let mut l = meter();
        l.blocks[0].anchor = Anchor::Center;
        l.blocks[0].offset = [0, 0];
        hm.handle(Key::Right, false, &mut l, 4, 24);
        assert_eq!(l.blocks[0].offset[0], 4);
        hm.handle(Key::Down, false, &mut l, 4, 24);
        assert_eq!(l.blocks[0].offset[1], 4);
    }

    #[test]
    fn tab_wraps_forward_and_shift_tab_wraps_backward() {
        let mut hm = HudMode { active: true, selected: 0 };
        let mut l = Layout { hud: Default::default(), blocks: vec![Block::new(BlockKind::Meter), Block::new(BlockKind::Timers)] };
        assert_eq!(hm.handle(Key::Tab, false, &mut l, 4, 24), Action::Redraw);
        assert_eq!(hm.selected, 1);
        assert_eq!(hm.handle(Key::Tab, false, &mut l, 4, 24), Action::Redraw);
        assert_eq!(hm.selected, 0, "wraps forward");
        assert_eq!(hm.handle(Key::Tab, true, &mut l, 4, 24), Action::Redraw);
        assert_eq!(hm.selected, 1, "wraps backward");
    }

    #[test]
    fn brackets_toggle_shows_on_a_meter_and_are_a_no_op_on_timers() {
        let mut hm = HudMode { active: true, selected: 0 };
        let mut l = meter();
        assert_eq!(l.blocks[0].shows, Shows::Damage);
        assert_eq!(hm.handle(Key::BracketRight, false, &mut l, 4, 24), Action::Redraw);
        assert_eq!(l.blocks[0].shows, Shows::Healing);
        assert_eq!(hm.handle(Key::BracketLeft, false, &mut l, 4, 24), Action::Redraw);
        assert_eq!(l.blocks[0].shows, Shows::Damage);

        let mut l = Layout { hud: Default::default(), blocks: vec![Block::new(BlockKind::Timers)] };
        assert_eq!(hm.handle(Key::BracketLeft, false, &mut l, 4, 24), Action::Nothing);
    }

    #[test]
    fn f_toggles_segment_on_a_meter() {
        let mut hm = HudMode { active: true, selected: 0 };
        let mut l = meter();
        assert_eq!(l.blocks[0].segment, Segment::Fight);
        assert_eq!(hm.handle(Key::F, false, &mut l, 4, 24), Action::Redraw);
        assert_eq!(l.blocks[0].segment, Segment::Session);
    }

    #[test]
    fn plus_on_a_default_row_count_becomes_the_default_plus_one() {
        let mut hm = HudMode { active: true, selected: 0 };
        let mut l = meter();
        assert_eq!(l.blocks[0].rows, None);
        assert_eq!(hm.handle(Key::Plus, false, &mut l, 4, 24), Action::Redraw);
        assert_eq!(l.blocks[0].rows, Some(9), "8 is a meter's default");
    }

    #[test]
    fn minus_floors_at_one_row() {
        let mut hm = HudMode { active: true, selected: 0 };
        let mut l = meter();
        l.blocks[0].rows = Some(1);
        hm.handle(Key::Minus, false, &mut l, 4, 24);
        assert_eq!(l.blocks[0].rows, Some(1));
    }

    #[test]
    fn h_toggles_hidden() {
        let mut hm = HudMode { active: true, selected: 0 };
        let mut l = meter();
        assert_eq!(hm.handle(Key::H, false, &mut l, 4, 24), Action::Redraw);
        assert!(l.blocks[0].hidden);
        hm.handle(Key::H, false, &mut l, 4, 24);
        assert!(!l.blocks[0].hidden);
    }

    #[test]
    fn an_unhandled_key_does_nothing() {
        let mut hm = HudMode { active: true, selected: 0 };
        let mut l = meter();
        assert_eq!(hm.handle(Key::Shift, false, &mut l, 4, 24), Action::Nothing);
    }
}
