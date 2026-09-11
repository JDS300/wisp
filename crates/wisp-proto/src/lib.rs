// SPDX-License-Identifier: MIT
//! Wire format shared by `wispd` and `wisp-hud`.
//!
//! Newline-delimited JSON over a Unix domain socket. Chosen for
//! debuggability: `socat - $XDG_RUNTIME_DIR/wisp/wispd.sock` is a complete
//! diagnostic tool, and the boundary can be exercised without a renderer.

pub mod client;

use serde::{Deserialize, Serialize};
use std::fmt;

/// Bumped whenever the snapshot shape changes incompatibly.
/// 1: Spec 1 counters. 2: Spec 2 adds `timers`. 3: Spec 3 adds `encounter`.
/// 4: Spec 5 adds the `slow` kind and `damage_type` on a `dot`.
pub const PROTOCOL_VERSION: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimerKind {
    Mez,
    Slow,
    Dot,
    Debuff,
}

/// The client's resist type (field 29 of `spells_us.txt`), reported on a `dot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DamageType {
    Unresistable,
    Magic,
    Fire,
    Cold,
    Poison,
    Disease,
    Chromatic,
    Prismatic,
    Physical,
    Corruption,
}

impl DamageType {
    /// 0 unresistable, 1 magic, 2 fire, 3 cold, 4 poison, 5 disease, 6 chromatic,
    /// 7 prismatic, 8 physical, 9 corruption. Anything else is unresistable:
    /// a code the client added later is still a DoT, just one without a colour.
    pub fn from_resist_type(code: i64) -> DamageType {
        match code {
            1 => DamageType::Magic,
            2 => DamageType::Fire,
            3 => DamageType::Cold,
            4 => DamageType::Poison,
            5 => DamageType::Disease,
            6 => DamageType::Chromatic,
            7 => DamageType::Prismatic,
            8 => DamageType::Physical,
            9 => DamageType::Corruption,
            _ => DamageType::Unresistable,
        }
    }

    /// The lowercase wire word, for the HUD's kind label and `wisp status`.
    pub fn name(self) -> &'static str {
        match self {
            DamageType::Unresistable => "unresistable",
            DamageType::Magic => "magic",
            DamageType::Fire => "fire",
            DamageType::Cold => "cold",
            DamageType::Poison => "poison",
            DamageType::Disease => "disease",
            DamageType::Chromatic => "chromatic",
            DamageType::Prismatic => "prismatic",
            DamageType::Physical => "physical",
            DamageType::Corruption => "corruption",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// Duration is the median of measured cast-to-fade intervals.
    Measured,
    /// Duration is seeded from the client's cap and the rank; not yet observed.
    Estimated,
}

/// One active countdown on a mob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timer {
    /// The target as the landing line printed it, e.g. `a jeering gargoyle`.
    pub target: String,
    /// Base spell name without rank, as the client data names it.
    pub spell: String,
    /// 0 when the cast line carried no numeral.
    pub rank: u8,
    pub kind: TimerKind,
    /// Present only when `kind == Dot`. Absent on the wire otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub damage_type: Option<DamageType>,
    /// May be negative during the post-expiry hold.
    pub remaining_ms: i64,
    pub duration_ms: u64,
    pub confidence: Confidence,
}

/// Your own numbers for the current fight. Rates are per second of fight
/// duration, rounded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Personal {
    pub damage: u64,
    pub dps: u64,
    pub taken: u64,
    pub taken_ps: u64,
    pub healing: u64,
    pub hps: u64,
    pub overheal: u64,
}

/// One ranked row of a group meter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeterRow {
    /// The source as the log printed it; an owned warder or pet is folded
    /// into its owner. Never `you`: your numbers are the personal line.
    pub name: String,
    pub amount: u64,
    pub per_s: u64,
}

/// The row caps the producer (`wispd::encounter`) applies when it ranks a
/// fight's sources, and the consumer (`wisp-hud`) relies on when it sizes
/// its window and iterates a snapshot's rows.
pub const MAX_DAMAGE_ROWS: usize = 5;
pub const MAX_HEALING_ROWS: usize = 3;

/// The current fight, or the last one while it lingers. `active` is false
/// while lingering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Encounter {
    pub active: bool,
    pub duration_s: u64,
    pub you: Personal,
    /// At most 5 rows, amount descending; group members only, you are on
    /// the personal line.
    pub damage: Vec<MeterRow>,
    /// At most 3 rows, amount descending.
    pub healing: Vec<MeterRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Protocol version. Receivers refuse anything they do not recognise.
    pub v: u32,
    /// Monotonically increasing per daemon run. Lets a client spot gaps.
    pub seq: u64,
    /// Raw timestamp text from the last consumed log line, e.g.
    /// `Mon Aug 10 20:39:54 2026`. Never parsed, never relabelled.
    pub ts: String,
    pub lines_ingested: u64,
    pub session_kills: u64,
    /// Active timers, soonest expiry first, at most 16. Absent on v1 lines.
    #[serde(default)]
    pub timers: Vec<Timer>,
    /// The current or lingering fight; `null` when there is none. Absent on
    /// v1 and v2 lines.
    #[serde(default)]
    pub encounter: Option<Encounter>,
}

#[derive(Debug)]
pub enum ProtoError {
    Version { found: u32, expected: u32 },
    Json(serde_json::Error),
}

impl fmt::Display for ProtoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtoError::Version { found, expected } => write!(
                f,
                "unsupported protocol version {found}; this build speaks {expected}"
            ),
            ProtoError::Json(e) => write!(f, "malformed snapshot: {e}"),
        }
    }
}

impl std::error::Error for ProtoError {}

impl From<serde_json::Error> for ProtoError {
    fn from(e: serde_json::Error) -> Self {
        ProtoError::Json(e)
    }
}

/// Serialise one snapshot as a single NDJSON line, newline included.
pub fn encode(snapshot: &Snapshot) -> String {
    let mut line = serde_json::to_string(snapshot).expect("Snapshot is always serialisable");
    line.push('\n');
    line
}

/// Parse one NDJSON line. Refuses unknown protocol versions loudly rather
/// than guessing at a shape it does not understand.
pub fn decode(line: &str) -> Result<Snapshot, ProtoError> {
    let snapshot: Snapshot = serde_json::from_str(line.trim_end_matches('\n'))?;
    if snapshot.v != PROTOCOL_VERSION {
        return Err(ProtoError::Version {
            found: snapshot.v,
            expected: PROTOCOL_VERSION,
        });
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Snapshot {
        Snapshot {
            v: PROTOCOL_VERSION,
            seq: 42,
            ts: "Mon Aug 10 20:39:54 2026".to_string(),
            lines_ingested: 10432,
            session_kills: 7,
            timers: Vec::new(),
            encounter: None,
        }
    }

    fn fight() -> Encounter {
        Encounter {
            active: true,
            duration_s: 42,
            you: Personal { damage: 18_234, dps: 434, taken: 2_210, taken_ps: 52, healing: 900, hps: 21, overheal: 120 },
            damage: vec![
                MeterRow { name: "Serenitee".to_string(), amount: 12_010, per_s: 286 },
                MeterRow { name: "Misery".to_string(), amount: 3_100, per_s: 74 },
            ],
            healing: vec![MeterRow { name: "Misery".to_string(), amount: 3_100, per_s: 74 }],
        }
    }

    #[test]
    fn an_encounter_round_trips() {
        let mut s = sample();
        s.encounter = Some(fight());
        let decoded = decode(&encode(&s)).unwrap();
        assert_eq!(decoded, s);
        assert_eq!(decoded.encounter.unwrap().damage[1].name, "Misery");
    }

    #[test]
    fn a_missing_encounter_decodes_as_none_and_none_encodes_as_null() {
        let line = r#"{"v":4,"seq":1,"ts":"x","lines_ingested":0,"session_kills":0,"timers":[]}"#;
        assert_eq!(decode(line).unwrap().encounter, None);
        assert!(encode(&sample()).contains(r#""encounter":null"#));
    }

    #[test]
    fn a_v2_line_is_refused_by_version() {
        let v2 = r#"{"v":2,"seq":1,"ts":"x","lines_ingested":0,"session_kills":0,"timers":[]}"#;
        match decode(v2) {
            Err(ProtoError::Version { found: 2, expected: 4 }) => {}
            other => panic!("expected a version error, got {other:?}"),
        }
    }

    fn mez() -> Timer {
        Timer {
            target: "a jeering gargoyle".to_string(),
            spell: "Mesmerization".to_string(),
            rank: 6,
            kind: TimerKind::Mez,
            damage_type: None,
            remaining_ms: 11_800,
            duration_ms: 38_000,
            confidence: Confidence::Measured,
        }
    }

    #[test]
    fn timers_round_trip() {
        let mut s = sample();
        s.timers = vec![mez()];
        let decoded = decode(&encode(&s)).unwrap();
        assert_eq!(decoded, s);
        assert_eq!(decoded.timers[0].kind, TimerKind::Mez);
    }

    #[test]
    fn timer_kinds_and_confidence_serialise_lowercase() {
        let line = encode(&Snapshot { timers: vec![mez()], ..sample() });
        assert!(line.contains(r#""kind":"mez""#), "{line}");
        assert!(line.contains(r#""confidence":"measured""#), "{line}");
    }

    #[test]
    fn a_dot_carries_its_damage_type_and_a_mez_carries_none() {
        let mut dot = mez();
        dot.spell = "Envenomed Bolt".to_string();
        dot.kind = TimerKind::Dot;
        dot.damage_type = Some(DamageType::Poison);
        let mut s = sample();
        s.timers = vec![dot, mez()];
        let line = encode(&s);
        assert!(line.contains(r#""kind":"dot","damage_type":"poison""#), "{line}");
        assert_eq!(line.matches("damage_type").count(), 1, "the mez has no key at all: {line}");
        let back = decode(&line).unwrap();
        assert_eq!(back.timers[0].damage_type, Some(DamageType::Poison));
        assert_eq!(back.timers[1].damage_type, None);
    }

    #[test]
    fn a_v4_line_without_the_field_decodes_to_none_and_v3_is_refused() {
        let line = r#"{"v":4,"seq":1,"ts":"","lines_ingested":0,"session_kills":0,"timers":[{"target":"a rat","spell":"Slow","rank":0,"kind":"slow","remaining_ms":1000,"duration_ms":2000,"confidence":"measured"}]}"#;
        let s = decode(line).unwrap();
        assert_eq!(s.timers[0].kind, TimerKind::Slow);
        assert_eq!(s.timers[0].damage_type, None);
        let v3 = line.replacen(r#""v":4"#, r#""v":3"#, 1);
        assert!(matches!(decode(&v3), Err(ProtoError::Version { found: 3, expected: 4 })));
    }

    #[test]
    fn resist_type_codes_map_to_damage_types() {
        assert_eq!(DamageType::from_resist_type(0), DamageType::Unresistable);
        assert_eq!(DamageType::from_resist_type(1), DamageType::Magic);
        assert_eq!(DamageType::from_resist_type(4), DamageType::Poison);
        assert_eq!(DamageType::from_resist_type(5), DamageType::Disease);
        assert_eq!(DamageType::from_resist_type(9), DamageType::Corruption);
        assert_eq!(DamageType::from_resist_type(42), DamageType::Unresistable);
        assert_eq!(DamageType::from_resist_type(-1), DamageType::Unresistable);
        for (t, word) in [(DamageType::Magic, "magic"), (DamageType::Corruption, "corruption"), (DamageType::Unresistable, "unresistable")] {
            assert_eq!(t.name(), word);
            assert_eq!(serde_json::to_string(&t).unwrap(), format!("\"{word}\""));
        }
    }

    #[test]
    fn a_v1_line_is_refused_by_version_not_by_shape() {
        let v1 = r#"{"v":1,"seq":1,"ts":"x","lines_ingested":0,"session_kills":0}"#;
        match decode(v1) {
            Err(ProtoError::Version { found: 1, expected: 4 }) => {}
            other => panic!("expected a version error, got {other:?}"),
        }
    }

    #[test]
    fn round_trips() {
        let s = sample();
        assert_eq!(decode(&encode(&s)).unwrap(), s);
    }

    #[test]
    fn encoded_line_ends_with_newline_and_has_no_interior_newline() {
        let line = encode(&sample());
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1);
    }

    #[test]
    fn rejects_unknown_protocol_version() {
        let line = r#"{"v":99,"seq":1,"ts":"x","lines_ingested":0,"session_kills":0}"#;
        match decode(line) {
            Err(ProtoError::Version { found, expected }) => {
                assert_eq!(found, 99);
                assert_eq!(expected, PROTOCOL_VERSION);
            }
            other => panic!("expected a version error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(matches!(decode("not json"), Err(ProtoError::Json(_))));
    }

    #[test]
    fn timestamp_is_carried_verbatim() {
        // EverQuest writes naive local wall clock. We never parse or relabel it.
        let s = sample();
        let decoded = decode(&encode(&s)).unwrap();
        assert_eq!(decoded.ts, "Mon Aug 10 20:39:54 2026");
    }
}
