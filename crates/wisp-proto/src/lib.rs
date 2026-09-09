// SPDX-License-Identifier: MIT
//! Wire format shared by `wispd` and `wisp-hud`.
//!
//! Newline-delimited JSON over a Unix domain socket. Chosen for
//! debuggability: `socat - $XDG_RUNTIME_DIR/wisp/wispd.sock` is a complete
//! diagnostic tool, and the boundary can be exercised without a renderer.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Bumped whenever the snapshot shape changes incompatibly.
/// 1: Spec 1 counters. 2: Spec 2 adds `timers`.
pub const PROTOCOL_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimerKind {
    Mez,
    Dot,
    Debuff,
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
    /// May be negative during the post-expiry hold.
    pub remaining_ms: i64,
    pub duration_ms: u64,
    pub confidence: Confidence,
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
        }
    }

    fn mez() -> Timer {
        Timer {
            target: "a jeering gargoyle".to_string(),
            spell: "Mesmerization".to_string(),
            rank: 6,
            kind: TimerKind::Mez,
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
    fn a_v1_line_is_refused_by_version_not_by_shape() {
        let v1 = r#"{"v":1,"seq":1,"ts":"x","lines_ingested":0,"session_kills":0}"#;
        match decode(v1) {
            Err(ProtoError::Version { found: 1, expected: 2 }) => {}
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
