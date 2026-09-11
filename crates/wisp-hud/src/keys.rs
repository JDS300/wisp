// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/keys.rs
//! HUD-mode key polling: parsing the config's chord, resolving it and the
//! fixed set of HUD-mode keys to X keycodes once, and reading their current
//! state with `QueryKeymap`.
//!
//! This connection never selects events and never touches focus -- it opens
//! its own `RustConnection`, calls `get_keyboard_mapping` once at open, and
//! after that does one `query_keymap` per poll. See the charter invariant:
//! the HUD never changes focus, and this is the one file that talks to the
//! keyboard at all.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

/// "ctrl+shift+grave": modifiers in any order, one final key named by its X
/// keysym name in lowercase (grave, f9, a, ...). Only `ctrl`, `shift`, `alt`
/// are modifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: String,
}

/// A token that is neither a modifier nor a key `keysym_for_name` knows, or
/// an entirely empty one (`"ctrl+"`, `""`), reported back verbatim so the
/// caller's refusal can name it. `(empty)` matches the house style the other
/// binaries already use for a value the user left blank.
const EMPTY_TOKEN: &str = "(empty)";

/// Parses a chord string. `Err` carries the token that could not be used --
/// an unknown key name, or `(empty)` for a blank one.
pub fn parse_chord(text: &str) -> Result<Chord, String> {
    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut key: Option<String> = None;

    for token in text.split('+') {
        if token.is_empty() {
            return Err(EMPTY_TOKEN.to_string());
        }
        match token.to_lowercase().as_str() {
            "ctrl" => ctrl = true,
            "shift" => shift = true,
            "alt" => alt = true,
            lower => {
                if keysym_for_name(lower).is_none() {
                    return Err(token.to_string());
                }
                key = Some(lower.to_string());
            }
        }
    }

    match key {
        Some(key) => Ok(Chord { ctrl, shift, alt, key }),
        None => Err(EMPTY_TOKEN.to_string()),
    }
}

/// The X keysym for one of `parse_chord`'s final-key names (not the modifier
/// words `ctrl`/`shift`/`alt`, which are not keys of their own here). Values
/// are keysymdef.h's, transcribed in the brief this module implements.
fn keysym_for_name(name: &str) -> Option<u32> {
    match name {
        "grave" => Some(0x0060),
        "minus" => Some(0x002d),
        "equal" => Some(0x003d),
        "bracketleft" => Some(0x005b),
        "bracketright" => Some(0x005d),
        "semicolon" => Some(0x003b),
        "apostrophe" => Some(0x0027),
        "comma" => Some(0x002c),
        "period" => Some(0x002e),
        "slash" => Some(0x002f),
        "backslash" => Some(0x005c),
        "space" => Some(0x0020),
        "escape" => Some(0xff1b),
        "tab" => Some(0xff09),
        "up" => Some(0xff52),
        "down" => Some(0xff54),
        "left" => Some(0xff51),
        "right" => Some(0xff53),
        "plus" => Some(0x002b),
        _ => {
            if let Some(rest) = name.strip_prefix('f') {
                if let Ok(n) = rest.parse::<u32>() {
                    if (1..=12).contains(&n) {
                        return Some(0xffbe + (n - 1));
                    }
                }
            }
            let mut chars = name.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else { return None };
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                Some(c as u32)
            } else {
                None
            }
        }
    }
}

const KEYSYM_CONTROL_L: u32 = 0xffe3;
const KEYSYM_CONTROL_R: u32 = 0xffe4;
const KEYSYM_SHIFT_L: u32 = 0xffe1;
const KEYSYM_SHIFT_R: u32 = 0xffe2;
const KEYSYM_ALT_L: u32 = 0xffe9;
const KEYSYM_ALT_R: u32 = 0xffea;

/// The keys HUD mode reads, resolved to keycodes once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Chord,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Tab,
    Shift,
    BracketLeft,
    BracketRight,
    F,
    Plus,
    Minus,
    H,
}

/// Every `Key` other than `Chord`, paired with the keysym(s) that fire it.
/// `Key::Shift` is either physical Shift key; every other entry is one
/// keysym.
fn direct_key_keysyms() -> [(Key, &'static [u32]); 13] {
    [
        (Key::Escape, &[0xff1b]),
        (Key::Up, &[0xff52]),
        (Key::Down, &[0xff54]),
        (Key::Left, &[0xff51]),
        (Key::Right, &[0xff53]),
        (Key::Tab, &[0xff09]),
        (Key::Shift, &[KEYSYM_SHIFT_L, KEYSYM_SHIFT_R]),
        (Key::BracketLeft, &[0x005b]),
        (Key::BracketRight, &[0x005d]),
        (Key::F, &[0x0066]),
        (Key::Plus, &[0x002b]),
        (Key::Minus, &[0x002d]),
        (Key::H, &[0x0068]),
    ]
}

pub struct Keyboard {
    conn: RustConnection,
    /// Every direct `Key`'s keycodes (any one down means the key is down).
    keys: HashMap<Key, Vec<u8>>,
    /// The chord's own modifiers and final key, each resolved to keycodes.
    chord: Chord,
    chord_ctrl: Vec<u8>,
    chord_shift: Vec<u8>,
    chord_alt: Vec<u8>,
    chord_key: Vec<u8>,
}

impl Keyboard {
    /// `None` when there is no X display to poll (the layer-shell case): HUD
    /// mode is unavailable.
    pub fn open(chord: &Chord) -> Option<Keyboard> {
        let (conn, _screen_num) = x11rb::connect(None).ok()?;
        let setup = conn.setup();
        let min = setup.min_keycode;
        let max = setup.max_keycode;
        let count = max.saturating_sub(min).saturating_add(1);
        let mapping = conn.get_keyboard_mapping(min, count).ok()?.reply().ok()?;
        let per = mapping.keysyms_per_keycode.max(1) as usize;

        let keycodes_for = |target: u32| -> Vec<u8> {
            mapping
                .keysyms
                .chunks(per)
                .enumerate()
                .filter(|(_, syms)| syms.contains(&target))
                .map(|(i, _)| min + i as u8)
                .collect()
        };
        let keycodes_for_any = |targets: &[u32]| -> Vec<u8> {
            targets.iter().flat_map(|&t| keycodes_for(t)).collect()
        };

        let mut keys = HashMap::new();
        for (key, syms) in direct_key_keysyms() {
            keys.insert(key, keycodes_for_any(syms));
        }

        let chord_ctrl = keycodes_for_any(&[KEYSYM_CONTROL_L, KEYSYM_CONTROL_R]);
        let chord_shift = keycodes_for_any(&[KEYSYM_SHIFT_L, KEYSYM_SHIFT_R]);
        let chord_alt = keycodes_for_any(&[KEYSYM_ALT_L, KEYSYM_ALT_R]);
        let chord_key = keysym_for_name(&chord.key).map(keycodes_for).unwrap_or_default();

        Some(Keyboard { conn, keys, chord: chord.clone(), chord_ctrl, chord_shift, chord_alt, chord_key })
    }

    /// One `XQueryKeymap`; the set of `Key`s currently held (`Chord` = all
    /// its configured parts down together).
    pub fn poll(&mut self) -> Result<HashSet<Key>, String> {
        let reply = self.conn.query_keymap().map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
        let down = |kc: u8| reply.keys[(kc / 8) as usize] & (1 << (kc % 8)) != 0;
        let any_down = |kcs: &[u8]| kcs.iter().any(|&kc| down(kc));

        let mut result = HashSet::new();
        for (&key, kcs) in &self.keys {
            if any_down(kcs) {
                result.insert(key);
            }
        }

        let ctrl_ok = !self.chord.ctrl || any_down(&self.chord_ctrl);
        let shift_ok = !self.chord.shift || any_down(&self.chord_shift);
        let alt_ok = !self.chord.alt || any_down(&self.chord_alt);
        if ctrl_ok && shift_ok && alt_ok && any_down(&self.chord_key) {
            result.insert(Key::Chord);
        }

        Ok(result)
    }
}

/// Edge detection with repeat: a key newly down fires once; while held, it
/// fires again after `repeat_after` and then every `repeat_every`.
///
/// `Key::Chord` is the one exception: it never repeats, however long it is
/// held. It toggles HUD mode (or saves and exits it), and a chord held past
/// `repeat_after` -- entirely plausible; it is usually two or three keys
/// pressed in sequence -- must not fire that action a second time on the
/// same physical press.
pub struct Edges {
    repeat_after: Duration,
    repeat_every: Duration,
    next_fire: HashMap<Key, Instant>,
}

impl Edges {
    pub fn new(repeat_after: Duration, repeat_every: Duration) -> Edges {
        Edges { repeat_after, repeat_every, next_fire: HashMap::new() }
    }

    pub fn update(&mut self, now: Instant, down: &HashSet<Key>) -> Vec<Key> {
        self.next_fire.retain(|key, _| down.contains(key));

        let mut fired = Vec::new();
        for &key in down {
            if key == Key::Chord {
                // Presence in the map means "already fired this hold"; the
                // `Instant` itself is never read for this key, since it
                // never reaches the repeat branch below.
                if let std::collections::hash_map::Entry::Vacant(e) = self.next_fire.entry(key) {
                    e.insert(now);
                    fired.push(key);
                }
                continue;
            }
            match self.next_fire.get_mut(&key) {
                None => {
                    self.next_fire.insert(key, now + self.repeat_after);
                    fired.push(key);
                }
                Some(next) if now >= *next => {
                    fired.push(key);
                    // From the fire that was due, not from `now`: a poll that
                    // ran late does not push later repeats out with it.
                    *next += self.repeat_every;
                }
                Some(_) => {}
            }
        }
        fired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_read_in_any_order_and_case() {
        assert_eq!(
            parse_chord("ctrl+shift+grave"),
            Ok(Chord { ctrl: true, shift: true, alt: false, key: "grave".to_string() })
        );
        assert_eq!(
            parse_chord("Shift+Ctrl+F9"),
            Ok(Chord { ctrl: true, shift: true, alt: false, key: "f9".to_string() })
        );
        assert_eq!(parse_chord("grave"), Ok(Chord { ctrl: false, shift: false, alt: false, key: "grave".to_string() }));
    }

    #[test]
    fn an_empty_token_is_named_as_empty() {
        assert_eq!(parse_chord("ctrl+"), Err(EMPTY_TOKEN.to_string()));
        assert_eq!(parse_chord(""), Err(EMPTY_TOKEN.to_string()));
    }

    #[test]
    fn an_unknown_token_is_named() {
        assert_eq!(parse_chord("super+g"), Err("super".to_string()));
    }

    #[test]
    fn keysym_table_matches_the_brief() {
        assert_eq!(keysym_for_name("a"), Some(0x61));
        assert_eq!(keysym_for_name("z"), Some(0x7a));
        assert_eq!(keysym_for_name("0"), Some(0x30));
        assert_eq!(keysym_for_name("9"), Some(0x39));
        assert_eq!(keysym_for_name("f1"), Some(0xffbe));
        assert_eq!(keysym_for_name("f12"), Some(0xffc9));
        assert_eq!(keysym_for_name("f13"), None);
        assert_eq!(keysym_for_name("grave"), Some(0x60));
        assert_eq!(keysym_for_name("space"), Some(0x20));
        assert_eq!(keysym_for_name("escape"), Some(0xff1b));
        assert_eq!(keysym_for_name("ctrl"), None, "a modifier word is not a key name");
        assert_eq!(keysym_for_name("nonsense"), None);
    }

    #[test]
    fn a_key_held_from_t0_fires_once_then_waits_then_repeats() {
        let mut edges = Edges::new(Duration::from_millis(400), Duration::from_millis(100));
        let base = Instant::now();
        let down: HashSet<Key> = [Key::H].into_iter().collect();

        assert_eq!(edges.update(base, &down), vec![Key::H], "fires once at t=0");
        assert_eq!(edges.update(base + Duration::from_millis(100), &down), Vec::<Key>::new(), "not at t=100ms");
        assert_eq!(edges.update(base + Duration::from_millis(399), &down), Vec::<Key>::new());
        assert_eq!(edges.update(base + Duration::from_millis(400), &down), vec![Key::H], "fires at t=400ms");
        assert_eq!(edges.update(base + Duration::from_millis(450), &down), Vec::<Key>::new());
        assert_eq!(edges.update(base + Duration::from_millis(500), &down), vec![Key::H], "then every 100ms");
        assert_eq!(edges.update(base + Duration::from_millis(600), &down), vec![Key::H]);
    }

    #[test]
    fn the_chord_never_repeats_while_held() {
        let mut edges = Edges::new(Duration::from_millis(400), Duration::from_millis(100));
        let base = Instant::now();
        let down: HashSet<Key> = [Key::Chord].into_iter().collect();
        let empty: HashSet<Key> = HashSet::new();

        assert_eq!(edges.update(base, &down), vec![Key::Chord]);
        assert_eq!(
            edges.update(base + Duration::from_millis(400), &down),
            Vec::<Key>::new(),
            "no repeat at what would be an ordinary key's repeat_after"
        );
        assert_eq!(
            edges.update(base + Duration::from_secs(2), &down),
            Vec::<Key>::new(),
            "held 2s, still no repeat"
        );

        assert_eq!(edges.update(base + Duration::from_millis(2050), &empty), Vec::<Key>::new(), "released");
        assert_eq!(
            edges.update(base + Duration::from_millis(2060), &down),
            vec![Key::Chord],
            "pressed again fires once more"
        );
    }

    #[test]
    fn releasing_and_pressing_again_fires_immediately() {
        let mut edges = Edges::new(Duration::from_millis(400), Duration::from_millis(100));
        let base = Instant::now();
        let down: HashSet<Key> = [Key::H].into_iter().collect();
        let empty: HashSet<Key> = HashSet::new();

        assert_eq!(edges.update(base, &down), vec![Key::H]);
        assert_eq!(edges.update(base + Duration::from_millis(50), &empty), Vec::<Key>::new(), "released");
        assert_eq!(edges.update(base + Duration::from_millis(60), &down), vec![Key::H], "pressed again, fires at once");
    }
}
