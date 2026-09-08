// SPDX-License-Identifier: MIT
//! The three parse rules of Spec 1. Pure functions over a single log line.
//!
//! Every rule here was verified against a real EverQuest Legends log
//! (`eqlog_Daggo_freeport.txt`, 1,440,036 lines). None is inferred from
//! another client -- Quarm, Live and Legends print differently.

/// `[Mon Aug 10 20:39:54 2026] ` -> `Mon Aug 10 20:39:54 2026`.
/// Returned verbatim: EverQuest writes naive local wall clock and Wisp does
/// not parse or relabel it.
pub fn timestamp_text(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('[')?;
    let end = rest.find(']')?;
    Some(&rest[..end])
}

fn body(line: &str) -> &str {
    match line.find("] ") {
        Some(i) => &line[i + 2..],
        None => line,
    }
}

/// The player's own kill. 3,722 in the reference fixture.
///
/// Deliberately narrow: `X has been slain by Y!` is a *third-party* kill and
/// must not match. It accounts for 3,065 of the 6,787 "slain" lines, so a
/// looser rule would be wrong by 45%.
///
/// The spec rule is `You have slain (.+)!` -- at least one character between
/// the prefix and the trailing `!`, so a mob-less "You have slain !" (were
/// one ever to appear) does not count as a kill.
pub fn own_kill(line: &str) -> bool {
    let b = body(line);
    match b.strip_prefix("You have slain ") {
        Some(rest) => rest.len() > 1 && rest.ends_with('!'),
        None => false,
    }
}

/// Start of a new play session. 39 in the reference fixture.
///
/// EQ Legends prints exactly this. Project Quarm prints "Welcome to
/// EverQuest!" and must not match -- see the charter's rule against
/// generalising between clients.
pub fn session_boundary(line: &str) -> bool {
    body(line) == "Welcome to EverQuest Legends!"
}

#[derive(Debug, Default, Clone)]
pub struct Counters {
    pub lines_ingested: u64,
    pub session_kills: u64,
    pub last_ts: String,
}

impl Counters {
    pub fn apply(&mut self, line: &str) {
        self.lines_ingested += 1;
        if let Some(ts) = timestamp_text(line) {
            self.last_ts.clear();
            self.last_ts.push_str(ts);
        }
        if session_boundary(line) {
            // A session resets what the session measures, not the lifetime
            // ingest total.
            self.session_kills = 0;
            return;
        }
        if own_kill(line) {
            self.session_kills += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN: &str = "[Mon Aug 10 20:39:54 2026] You have slain a spiderling!";
    const THIRD_PARTY: &str = "[Mon Aug 10 19:16:16 2026] Zantetsu has been slain by Guard Wytiffin!";
    const BOUNDARY: &str = "[Mon Aug 10 18:56:09 2026] Welcome to EverQuest Legends!";
    const CHATTER: &str = "[Mon Aug 10 20:36:03 2026] A large rat bites YOU for 4 points of damage.";

    #[test]
    fn own_kills_are_recognised() {
        assert!(own_kill(OWN));
    }

    #[test]
    fn third_party_kills_are_not_counted() {
        // 3,065 of 6,787 "slain" lines in the reference fixture. Counting these
        // would make session_kills wrong by 45%.
        assert!(!own_kill(THIRD_PARTY));
    }

    #[test]
    fn ordinary_lines_are_not_kills() {
        assert!(!own_kill(CHATTER));
        assert!(!own_kill(BOUNDARY));
    }

    #[test]
    fn a_mob_less_slain_line_does_not_match() {
        // The spec rule is `You have slain (.+)!` -- at least one character
        // between the prefix and the trailing `!`. Zero such lines exist in
        // the 1,440,036-line reference fixture, so this does not change the
        // verified kill count; it closes a gap the fixture never exercised.
        assert!(!own_kill("[Mon Aug 10 20:39:54 2026] You have slain !"));
    }

    #[test]
    fn the_session_boundary_is_recognised() {
        assert!(session_boundary(BOUNDARY));
        assert!(!session_boundary(OWN));
    }

    #[test]
    fn quarm_boundary_text_is_not_accepted() {
        // Project Quarm prints "Welcome to EverQuest!" -- a different client on a
        // different codebase. Wisp targets EQ Legends and must not match it.
        let quarm = "[Sat Nov 25 10:28:35 2023] Welcome to EverQuest!";
        assert!(!session_boundary(quarm));
    }

    #[test]
    fn the_timestamp_is_extracted_verbatim() {
        assert_eq!(timestamp_text(OWN), Some("Mon Aug 10 20:39:54 2026"));
        assert_eq!(timestamp_text("no bracket here"), None);
    }

    #[test]
    fn counters_accumulate_and_reset_on_a_boundary() {
        let mut c = Counters::default();
        c.apply(OWN);
        c.apply(OWN);
        c.apply(THIRD_PARTY);
        c.apply(CHATTER);
        assert_eq!(c.session_kills, 2);
        assert_eq!(c.lines_ingested, 4, "every line counts, kill or not");

        c.apply(BOUNDARY);
        assert_eq!(c.session_kills, 0, "a new session resets kills");
        assert_eq!(c.lines_ingested, 5, "but not the ingest total");
    }

    #[test]
    fn the_last_timestamp_is_carried() {
        let mut c = Counters::default();
        c.apply(OWN);
        assert_eq!(c.last_ts, "Mon Aug 10 20:39:54 2026");
    }
}
