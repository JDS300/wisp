// SPDX-License-Identifier: MIT
//! `wisp status`: one snapshot, as text or as the line that arrived.
//!
//! The second client of the protocol, and the reason the snapshot reader lives
//! in `wisp-proto` rather than in the HUD: `socat - <socket>` was always a
//! complete diagnostic, and this is that with the JSON already taken apart.

use std::time::Duration;
use wisp_proto::client::connect;
use wisp_proto::{encode, Confidence, Encounter, MeterRow, Snapshot, Timer, TimerKind};

/// How long a connected socket is given to produce its first snapshot before
/// `status` gives up. The same figure `wisp run` gives a daemon to start
/// listening at all (`READY` in `run.rs`), reused here for the next thing that
/// can go quiet: a daemon that accepted the connection but is stuck or has
/// wedged before writing anything.
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(5);

/// Connect, read one snapshot, print it, and return the status `main` exits
/// with.
pub fn status(json: bool) -> i32 {
    let path = wisp_config::paths::socket_path();
    let mut stream = match connect(&path) {
        Ok(stream) => stream,
        // Nothing answered. Whether that is a daemon that never started or one
        // that has just died is not knowable from here, and the socket's path is
        // what the user needs either way.
        Err(_) => {
            eprintln!("wisp: no daemon is listening on {}", path.display());
            return 1;
        }
    };
    // Unlike `wisp-hud`, which legitimately waits as long as the daemon stays
    // quiet, this is a one-shot read: without a timeout, a daemon that
    // accepts the connection and then never writes would hang `wisp status`
    // forever.
    if let Err(e) = stream.set_read_timeout(Some(SNAPSHOT_TIMEOUT)) {
        eprintln!("wisp: could not set a read timeout on {}: {e}", path.display());
        return 1;
    }
    match stream.next_snapshot() {
        Some(Ok(snapshot)) => {
            if json {
                // The line that arrived, not a retelling of it: `encode` is the
                // function the daemon built the line with, so the fields, their
                // order and their compactness are the same. `SnapshotStream`
                // reads into a `Snapshot` and does not keep the bytes.
                print!("{}", encode(&snapshot));
            } else {
                print!("{}", text(&snapshot));
            }
            0
        }
        // A version this build does not speak, or JSON that is not a snapshot.
        // `next_snapshot` has already said which, on stderr, in its own words.
        Some(Err(e)) => {
            eprintln!("wisp: {e}");
            1
        }
        None if stream.timed_out() => {
            eprintln!(
                "wisp: {}: the daemon accepted the connection but sent no snapshot in 5 s",
                path.display()
            );
            1
        }
        None => {
            // A read error is reported by `next_snapshot` itself; only a clean
            // EOF needs saying here, so a failure is not followed by a second,
            // misleading line.
            if !stream.had_error() {
                eprintln!("wisp: the daemon closed the connection without a snapshot");
            }
            1
        }
    }
}

/// The text form: one labelled line per field, with timer and meter rows
/// indented two spaces beneath the heading they belong to.
fn text(snapshot: &Snapshot) -> String {
    let mut out = String::new();
    field(&mut out, "log time:", &snapshot.ts);
    field(&mut out, "log:", snapshot.log.as_deref().unwrap_or(""));
    field(&mut out, "lines:", &snapshot.lines_ingested.to_string());
    field(&mut out, "kills:", &snapshot.session_kills.to_string());
    // A count of 0 is followed by nothing at all: no heading, no placeholder, no
    // empty row.
    field(&mut out, "timers:", &snapshot.timers.len().to_string());
    for timer in &snapshot.timers {
        out.push_str(&timer_row(timer));
        out.push('\n');
    }
    match &snapshot.encounter {
        None => field(&mut out, "fight:", "none"),
        Some(e) => {
            field(
                &mut out,
                "fight:",
                &format!(
                    "{}, {}:{:02}",
                    if e.active { "active" } else { "lingering" },
                    e.duration_s / 60,
                    e.duration_s % 60
                ),
            );
            row(&mut out, "you", &personal(e));
            for r in &e.damage {
                row(&mut out, "damage", &meter(r));
            }
            for r in &e.healing {
                row(&mut out, "healing", &meter(r));
            }
        }
    }
    out
}

fn timer_row(timer: &Timer) -> String {
    // The rank numeral only when the cast line carried one, exactly as the HUD
    // draws it.
    let spell = match timer.rank {
        0 => timer.spell.clone(),
        rank => format!("{} {}", timer.spell, roman(rank)),
    };
    // `{:>2}` is a minimum, not a clamp: a freshly seeded timer for one of the
    // longer-capped spells runs to hours, and a report that truncated it to fit
    // a column would be the one place Wisp lies about a duration.
    let secs = timer.remaining_ms.div_euclid(1000);
    format!(
        "  {:<20} {:<18} {:>2}s   {}   {}",
        timer.target,
        spell,
        secs,
        confidence(timer.confidence),
        kind_label(timer)
    )
}

/// The wire's own lowercase kind word, e.g. `mez` or `dot`, plus the damage
/// type in parentheses when the timer carries one.
fn kind_label(timer: &Timer) -> String {
    let word = kind_word(timer.kind);
    match timer.damage_type {
        Some(dt) => format!("{word} ({})", dt.name()),
        None => word.to_string(),
    }
}

fn kind_word(kind: TimerKind) -> &'static str {
    match kind {
        TimerKind::Mez => "mez",
        TimerKind::Slow => "slow",
        TimerKind::Dot => "dot",
        TimerKind::Debuff => "debuff",
    }
}

fn personal(e: &Encounter) -> String {
    format!(
        "DPS {}   in {}/s   HPS {}   damage {}   taken {}   healed {}   overheal {}",
        e.you.dps,
        e.you.taken_ps,
        e.you.hps,
        compact(e.you.damage),
        e.you.taken,
        compact(e.you.healing),
        e.you.overheal
    )
}

fn meter(r: &MeterRow) -> String {
    format!("{} {} {}/s", r.name, compact(r.amount), r.per_s)
}

/// The wire's own lowercase spelling, which is what a `--json` reader sees.
fn confidence(value: Confidence) -> &'static str {
    match value {
        Confidence::Measured => "measured",
        Confidence::Estimated => "estimated",
    }
}

/// A labelled line.
fn field(out: &mut String, label: &str, value: &str) {
    out.push_str(&crate::labelled(label, value));
    out.push('\n');
}

/// An indented row: two spaces, then its own label padded to the column the
/// labelled lines use, so `you`, `damage` and `healing` line up under `fight:`.
fn row(out: &mut String, label: &str, rest: &str) {
    out.push_str(&format!("  {label:<9}{rest}"));
    out.push('\n');
}

/// 999 -> "999", 18234 -> "18.2k", 1320500 -> "1.32M".
///
/// The HUD's own rule, restated beside [`roman`] because `wisp-hud` is a binary
/// crate and cannot export either, and the protocol carries an amount as a bare
/// integer and a rank as a bare `u8`: there is nothing on the wire for either of
/// them to be printed as.
fn compact(n: u64) -> String {
    if n < 10_000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        format!("{:.2}M", n as f64 / 1_000_000.0)
    }
}

/// The rank numeral for I-X, and nothing above it: the client's own spell data
/// does not go further, and the HUD draws the name alone when there is no
/// numeral to draw.
fn roman(rank: u8) -> &'static str {
    ["", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"]
        .get(rank as usize)
        .copied()
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_proto::{
        Confidence, Encounter, MeterRow, Personal, Snapshot, Timer, TimerKind, PROTOCOL_VERSION,
    };

    /// The plan's own example, field for field, so the layout cannot drift by
    /// one space without a test saying so.
    fn snapshot() -> Snapshot {
        Snapshot {
            v: PROTOCOL_VERSION,
            seq: 1,
            ts: "Mon Aug 10 20:39:54 2026".to_string(),
            log: Some("eqlog_Daggo_freeport.txt".to_string()),
            lines_ingested: 10432,
            session_kills: 7,
            timers: vec![
                Timer {
                    target: "a jeering gargoyle".to_string(),
                    spell: "Mesmerization".to_string(),
                    rank: 6,
                    kind: TimerKind::Mez,
                    damage_type: None,
                    remaining_ms: 12_000,
                    duration_ms: 38_000,
                    confidence: Confidence::Measured,
                },
                Timer {
                    target: "Guard Drazden".to_string(),
                    spell: "Pacify".to_string(),
                    rank: 5,
                    kind: TimerKind::Debuff,
                    damage_type: None,
                    remaining_ms: 63_000,
                    duration_ms: 63_000,
                    confidence: Confidence::Estimated,
                },
            ],
            encounter: Some(Encounter {
                active: true,
                duration_s: 42,
                you: Personal {
                    damage: 18_234,
                    dps: 434,
                    taken: 2_210,
                    taken_ps: 52,
                    healing: 900,
                    hps: 21,
                    overheal: 120,
                },
                damage: vec![
                    MeterRow { name: "Serenitee".to_string(), amount: 12_010, per_s: 286 },
                    MeterRow { name: "Misery".to_string(), amount: 3_100, per_s: 74 },
                ],
                healing: vec![MeterRow { name: "Misery".to_string(), amount: 3_100, per_s: 74 }],
            }),
        }
    }

    const EVERY_FIELD: &str = "\
log time:  Mon Aug 10 20:39:54 2026
log:       eqlog_Daggo_freeport.txt
lines:     10432
kills:     7
timers:    2
  a jeering gargoyle   Mesmerization VI   12s   measured   mez
  Guard Drazden        Pacify V           63s   estimated   debuff
fight:     active, 0:42
  you      DPS 434   in 52/s   HPS 21   damage 18.2k   taken 2210   healed 900   overheal 120
  damage   Serenitee 12.0k 286/s
  damage   Misery 3100 74/s
  healing  Misery 3100 74/s
";

    #[test]
    fn status_text_names_every_field() {
        assert_eq!(text(&snapshot()), EVERY_FIELD);

        // A rank the log's own cast line did not carry prints no numeral.
        let mut s = snapshot();
        let mut rankless = s.timers[0].clone();
        rankless.rank = 0;
        s.timers = vec![rankless];
        assert_eq!(
            text(&s).lines().nth(5).unwrap(),
            "  a jeering gargoyle   Mesmerization      12s   measured   mez"
        );
    }

    #[test]
    fn the_log_line_is_the_file_name_and_is_empty_when_there_is_none() {
        let out = text(&snapshot());
        assert!(out.contains("log:       eqlog_Daggo_freeport.txt\n"), "{out}");
        assert_eq!(out.lines().nth(1).unwrap(), "log:       eqlog_Daggo_freeport.txt", "it sits under log time:");

        let mut s = snapshot();
        s.log = None;
        let out = text(&s);
        // `labelled` trims, so a daemon with no log leaves no trailing spaces.
        assert!(out.contains("\nlog:\n"), "{out:?}");
    }

    #[test]
    fn timer_row_shows_the_damage_type_only_when_present() {
        let mut dot = snapshot().timers[0].clone();
        dot.kind = TimerKind::Dot;
        dot.damage_type = Some(wisp_proto::DamageType::Poison);
        assert!(timer_row(&dot).contains("dot (poison)"), "{}", timer_row(&dot));

        let mez = snapshot().timers[0].clone();
        assert!(timer_row(&mez).ends_with("mez"), "{}", timer_row(&mez));
    }

    #[test]
    fn no_timers_prints_no_indented_lines() {
        let mut s = snapshot();
        s.timers = Vec::new();
        s.encounter = None;
        let out = text(&s);
        assert!(out.contains("timers:    0\n"), "{out}");
        assert!(out.contains("kills:     7\n"), "{out}");
        // "No indented lines" is the whole of it: a count of 0 is not followed
        // by a heading, a placeholder or an empty row.
        assert!(!out.lines().any(|line| line.starts_with(' ')), "{out}");
    }

    #[test]
    fn a_null_encounter_prints_fight_none() {
        let mut s = snapshot();
        s.encounter = None;
        let out = text(&s);
        assert!(out.contains("fight:     none\n"), "{out}");
        assert!(!out.contains("  you"), "{out}");
        assert!(!out.contains("  damage"), "{out}");
        assert!(!out.contains("  healing"), "{out}");
        // The timers are not part of the fight block and still print.
        assert!(out.contains("  a jeering gargoyle"), "{out}");

        // A fight that has ended but lingers says so rather than claiming to be
        // live: `active` is false while the panel is still on screen.
        let mut s = snapshot();
        s.encounter.as_mut().unwrap().active = false;
        let lingering = text(&s);
        assert!(lingering.contains("fight:     lingering, 0:42\n"), "{lingering}");
    }

    #[test]
    fn amounts_compact_as_the_hud_does() {
        assert_eq!(compact(0), "0");
        assert_eq!(compact(999), "999");
        assert_eq!(compact(9_999), "9999");
        assert_eq!(compact(18_234), "18.2k");
        assert_eq!(compact(999_949), "999.9k");
        assert_eq!(compact(1_320_500), "1.32M");
        // The rank numeral is the other helper the HUD has and cannot export:
        // the protocol carries a rank as a bare u8, so both live here beside
        // each other and both are checked.
        assert_eq!(roman(0), "");
        assert_eq!(roman(1), "I");
        assert_eq!(roman(6), "VI");
        assert_eq!(roman(10), "X");
        assert_eq!(roman(11), "", "nothing above X, exactly as the HUD draws it");
    }
}
