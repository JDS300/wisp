// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/evdev.rs
//! Raw evdev key codes to `keys::Key`, without libxkbcommon.
//!
//! `wl_keyboard.key` carries the evdev code -- the X keycode minus 8 -- and
//! smithay-client-toolkit's keyboard helpers, which would turn it into a
//! keysym, sit behind its `xkbcommon` feature and bind the C library. That
//! would end the static musl build, so Spec 6 §4.5 takes the eight
//! position-dependent keys and documents them as positions.
//!
//! Pure: no Wayland type appears here, so the whole mapping and the chord's
//! modifier state machine are unit tests rather than a compositor.

use crate::backend::KeyEvent;
use crate::keys::Key;
use std::collections::HashSet;

/// Grave, the chord's own key. Not in `key_for_code`: on its own it is not
/// a `Key` this HUD has.
const GRAVE: u32 = 41;
/// Left and right control.
const CTRL: [u32; 2] = [29, 97];
/// Left and right shift. Also `Key::Shift` in their own right.
const SHIFT: [u32; 2] = [42, 54];

/// The `keys::Key` an evdev code stands for, ignoring modifiers.
/// `Key::Chord` is never returned: it is not one key.
pub fn key_for_code(code: u32) -> Option<Key> {
    match code {
        1 => Some(Key::Escape),
        15 => Some(Key::Tab),
        103 => Some(Key::Up),
        108 => Some(Key::Down),
        105 => Some(Key::Left),
        106 => Some(Key::Right),
        42 | 54 => Some(Key::Shift),
        26 => Some(Key::BracketLeft),
        27 => Some(Key::BracketRight),
        33 => Some(Key::F),
        35 => Some(Key::H),
        // `=` and the keypad's own `+`; `-` and the keypad's.
        13 | 78 => Some(Key::Plus),
        12 | 74 => Some(Key::Minus),
        _ => None,
    }
}

/// Turns the raw `wl_keyboard.key` stream into `KeyEvent`s, holding the one
/// piece of state the mapping needs: which modifiers are down.
#[derive(Debug, Default)]
pub struct ChordTracker {
    /// Modifier codes currently down.
    mods: HashSet<u32>,
    /// `Key`s this tracker has reported as pressed and not yet as released,
    /// so `release_all` can put the caller's held set back to empty when
    /// focus goes away and no more releases will arrive.
    down: HashSet<Key>,
}

impl ChordTracker {
    pub fn new() -> ChordTracker {
        ChordTracker::default()
    }

    fn ctrl_held(&self) -> bool {
        CTRL.iter().any(|code| self.mods.contains(code))
    }

    fn shift_held(&self) -> bool {
        SHIFT.iter().any(|code| self.mods.contains(code))
    }

    /// One raw event in, at most one `KeyEvent` out.
    pub fn feed(&mut self, code: u32, pressed: bool) -> Option<KeyEvent> {
        if CTRL.contains(&code) || SHIFT.contains(&code) {
            if pressed {
                self.mods.insert(code);
            } else {
                self.mods.remove(&code);
            }
        }

        let key = if code == GRAVE {
            // A release always reports, whatever the modifiers are doing by
            // then: the user lets ctrl and shift go before the grave as
            // often as not, and a chord that never released would stay in
            // the caller's held set for the rest of the run.
            if pressed {
                if self.ctrl_held() && self.shift_held() {
                    Key::Chord
                } else {
                    return None;
                }
            } else if self.down.contains(&Key::Chord) {
                Key::Chord
            } else {
                return None;
            }
        } else {
            key_for_code(code)?
        };

        if pressed {
            self.down.insert(key);
        } else {
            self.down.remove(&key);
        }
        Some(KeyEvent { key, pressed })
    }

    /// The modifier codes `wl_keyboard.enter` says are already down. Only
    /// modifiers are taken from it -- see the ruling below.
    pub fn sync_from_enter(&mut self, codes: &[u32]) -> Vec<KeyEvent> {
        // Modifiers only. The array lists everything logically down when
        // focus arrived, and when the chord is what took the keyboard that
        // includes grave -- feeding which would leave HUD mode on the frame
        // it was entered.
        let mut events = Vec::new();
        for &code in codes {
            if CTRL.contains(&code) || SHIFT.contains(&code) {
                self.mods.insert(code);
                if SHIFT.contains(&code) && self.down.insert(Key::Shift) {
                    events.push(KeyEvent { key: Key::Shift, pressed: true });
                }
            }
        }
        events
    }

    /// A release for everything still held, then forget all of it. For
    /// `wl_keyboard.leave`, after which no more releases will arrive.
    pub fn release_all(&mut self) -> Vec<KeyEvent> {
        let events = self
            .down
            .drain()
            .map(|key| KeyEvent { key, pressed: false })
            .collect();
        self.mods.clear();
        events
    }
}

/// The little-endian `u32` codes packed into `wl_keyboard.enter`'s `keys`
/// array. A trailing partial code is ignored.
pub fn codes_from_enter(keys: &[u8]) -> Vec<u32> {
    keys.chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(t: &mut ChordTracker, code: u32) -> Option<KeyEvent> {
        t.feed(code, true)
    }
    fn release(t: &mut ChordTracker, code: u32) -> Option<KeyEvent> {
        t.feed(code, false)
    }
    fn down(key: Key) -> Option<KeyEvent> {
        Some(KeyEvent { key, pressed: true })
    }
    fn up(key: Key) -> Option<KeyEvent> {
        Some(KeyEvent { key, pressed: false })
    }

    #[test]
    fn the_table_is_the_specs() {
        for (code, key) in [
            (1, Key::Escape),
            (15, Key::Tab),
            (103, Key::Up),
            (108, Key::Down),
            (105, Key::Left),
            (106, Key::Right),
            (42, Key::Shift),
            (54, Key::Shift),
            (26, Key::BracketLeft),
            (27, Key::BracketRight),
            (33, Key::F),
            (35, Key::H),
            (13, Key::Plus),
            (78, Key::Plus),
            (12, Key::Minus),
            (74, Key::Minus),
        ] {
            assert_eq!(key_for_code(code), Some(key), "evdev {code}");
        }
        assert_eq!(key_for_code(41), None, "grave alone is not a Key; the chord is not one key");
        assert_eq!(key_for_code(29), None, "nor is ctrl");
        assert_eq!(key_for_code(9999), None);
    }

    #[test]
    fn an_unmapped_code_produces_no_event() {
        let mut t = ChordTracker::new();
        assert_eq!(press(&mut t, 30), None, "the A key is not a HUD-mode key");
        assert_eq!(release(&mut t, 30), None);
    }

    #[test]
    fn shift_is_tracked_from_its_own_press_and_release() {
        let mut t = ChordTracker::new();
        assert_eq!(press(&mut t, 42), down(Key::Shift));
        assert_eq!(release(&mut t, 42), up(Key::Shift));
        assert_eq!(press(&mut t, 54), down(Key::Shift), "the right shift too");
    }

    #[test]
    fn the_chord_needs_ctrl_and_a_shift_held_with_grave() {
        let mut t = ChordTracker::new();
        assert_eq!(press(&mut t, 41), None, "grave alone");

        press(&mut t, 29);
        assert_eq!(press(&mut t, 41), None, "ctrl and grave, no shift");

        press(&mut t, 42);
        assert_eq!(press(&mut t, 41), down(Key::Chord));

        // Either physical key of either modifier.
        let mut t = ChordTracker::new();
        press(&mut t, 97);
        press(&mut t, 54);
        assert_eq!(press(&mut t, 41), down(Key::Chord));
    }

    #[test]
    fn releasing_grave_releases_the_chord_whatever_the_modifiers_are_doing() {
        let mut t = ChordTracker::new();
        press(&mut t, 29);
        press(&mut t, 42);
        assert_eq!(press(&mut t, 41), down(Key::Chord));
        // The user lets the modifiers go first, which is what actually happens.
        assert_eq!(release(&mut t, 42), up(Key::Shift));
        release(&mut t, 29);
        assert_eq!(release(&mut t, 41), up(Key::Chord), "or the chord sticks down for ever");
    }

    #[test]
    fn an_enter_contributes_modifiers_and_nothing_else() {
        let mut t = ChordTracker::new();
        // grave, ctrl and shift are all down at the moment focus arrives,
        // because the chord is what took the keyboard.
        let events = t.sync_from_enter(&[41, 29, 42, 103]);
        assert_eq!(events, vec![KeyEvent { key: Key::Shift, pressed: true }]);
        // And the grave in that list did not arm a chord that never happened:
        assert_eq!(release(&mut t, 41), None, "no Chord was ever pressed");
        // The modifiers it did take are real:
        assert_eq!(press(&mut t, 41), down(Key::Chord));
    }

    #[test]
    fn leaving_releases_everything_still_held() {
        let mut t = ChordTracker::new();
        press(&mut t, 42);
        press(&mut t, 106);
        let mut released = t.release_all();
        released.sort_by_key(|e| format!("{:?}", e.key));
        assert_eq!(
            released,
            vec![
                KeyEvent { key: Key::Right, pressed: false },
                KeyEvent { key: Key::Shift, pressed: false },
            ]
        );
        assert!(t.release_all().is_empty(), "and then there is nothing left to release");
        assert_eq!(press(&mut t, 41), None, "the modifiers were forgotten too");
    }

    #[test]
    fn an_enter_array_is_little_endian_u32s() {
        assert_eq!(codes_from_enter(&[41, 0, 0, 0, 29, 0, 0, 0]), vec![41, 29]);
        assert_eq!(codes_from_enter(&[]), Vec::<u32>::new());
        assert_eq!(codes_from_enter(&[1, 0, 0]), Vec::<u32>::new(), "a partial code is not a code");
        assert_eq!(codes_from_enter(&[103, 0, 0, 0, 9]), vec![103], "nor is a trailing one");
    }
}
