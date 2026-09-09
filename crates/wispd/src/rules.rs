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

pub fn body(line: &str) -> &str {
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

/// Rank numerals the log appends to an upgraded spell: `Mesmerization VI`.
const ROMAN: [(&str, u8); 10] = [
    ("I", 1), ("II", 2), ("III", 3), ("IV", 4), ("V", 5),
    ("VI", 6), ("VII", 7), ("VIII", 8), ("IX", 9), ("X", 10),
];

/// `Mesmerization VI` -> (`Mesmerization`, 6); `Venom of the Snake` -> (…, 0).
///
/// A trailing numeral is a rank only if what precedes it is a known spell;
/// the client data has one row per spell and no row for `Mesmerization VI`.
/// Names that legitimately end in numeral letters are the charter's trap,
/// and the table -- not this function -- decides them.
pub fn split_rank(text: &str, known: impl Fn(&str) -> bool) -> Option<(&str, u8)> {
    if known(text) {
        return Some((text, 0));
    }
    let (base, tail) = text.rsplit_once(' ')?;
    let rank = ROMAN.iter().find(|(r, _)| *r == tail).map(|(_, n)| *n)?;
    if known(base) {
        Some((base, rank))
    } else {
        None
    }
}

/// `Mon Aug 10 20:39:54 2026` -> seconds on an arbitrary naive scale.
///
/// Only differences between two values from the same log are meaningful.
/// No zone is applied and none is implied: EverQuest writes local wall
/// clock, and Wisp subtracts rather than relabels (spec §3).
pub fn parse_log_time(ts: &str) -> Option<i64> {
    let mut parts = ts.split_whitespace();
    let _weekday = parts.next()?;
    let month = match parts.next()? {
        "Jan" => 1, "Feb" => 2, "Mar" => 3, "Apr" => 4, "May" => 5, "Jun" => 6,
        "Jul" => 7, "Aug" => 8, "Sep" => 9, "Oct" => 10, "Nov" => 11, "Dec" => 12,
        _ => return None,
    };
    let day: u32 = parts.next()?.parse().ok()?;
    let mut hms = parts.next()?.split(':');
    let h: i64 = hms.next()?.parse().ok()?;
    let m: i64 = hms.next()?.parse().ok()?;
    let s: i64 = hms.next()?.parse().ok()?;
    if hms.next().is_some() {
        return None;
    }
    let year: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + h * 3600 + m * 60 + s)
}

/// Days since 1970-01-01 for a proleptic Gregorian date. Howard Hinnant's
/// algorithm; pure integer arithmetic, no calendar library.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// What one log line means to the timer state machine. Borrowed from the
/// line's body; `Other` carries the body for prose-landing matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event<'a> {
    CastBegin { spell_text: &'a str },
    Fizzle { spell: &'a str },
    Interrupted { spell: &'a str },
    Resisted { target: &'a str, spell: &'a str },
    DotTick { target: &'a str, spell: &'a str },
    WornOff { spell: &'a str, target: &'a str },
    Awakened { target: &'a str },
    Slain { target: &'a str },
    ZoneChange,
    Other(&'a str),
}

/// Classify a line body (timestamp already stripped). Every shape here was
/// read from the reference fixture; none is inferred from another client.
pub fn classify(body: &str) -> Event<'_> {
    if let Some(rest) = body.strip_prefix("You begin casting ") {
        if let Some(spell_text) = rest.strip_suffix('.') {
            return Event::CastBegin { spell_text };
        }
    }
    if let Some(rest) = body.strip_prefix("Your ") {
        if let Some(spell) = rest.strip_suffix(" spell fizzles!") {
            return Event::Fizzle { spell };
        }
        if let Some(spell) = rest.strip_suffix(" spell is interrupted.") {
            return Event::Interrupted { spell };
        }
        if let Some((spell, target)) = rest
            .strip_suffix('.')
            .and_then(|r| r.split_once(" spell has worn off of "))
        {
            return Event::WornOff { spell, target };
        }
    }
    if let Some((target, spell)) = body
        .strip_suffix('!')
        .and_then(|r| r.split_once(" resisted your "))
    {
        return Event::Resisted { target, spell };
    }
    if let Some((target, rest)) = body
        .strip_suffix('.')
        .and_then(|r| r.split_once(" has taken "))
    {
        if let Some((n, spell)) = rest.split_once(" damage from your ") {
            if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) {
                return Event::DotTick { target, spell };
            }
        }
    }
    if let Some((target, _who)) = body
        .strip_suffix('.')
        .and_then(|r| r.split_once(" has been awakened by "))
    {
        return Event::Awakened { target };
    }
    if let Some(rest) = body.strip_prefix("You have slain ") {
        if let Some(target) = rest.strip_suffix('!') {
            if !target.is_empty() {
                return Event::Slain { target };
            }
        }
    }
    if let Some((target, _who)) = body
        .strip_suffix('!')
        .and_then(|r| r.split_once(" has been slain by "))
    {
        return Event::Slain { target };
    }
    if body.starts_with("You have entered ") || body.starts_with("LOADING, PLEASE WAIT") {
        return Event::ZoneChange;
    }
    Event::Other(body)
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

    fn known(name: &str) -> bool {
        matches!(name, "Mesmerization" | "Togor's Insects" | "Venom of the Snake")
    }

    #[test]
    fn a_trailing_numeral_is_a_rank_only_when_the_base_name_is_known() {
        assert_eq!(split_rank("Mesmerization VI", known), Some(("Mesmerization", 6)));
        assert_eq!(split_rank("Togor's Insects X", known), Some(("Togor's Insects", 10)));
        assert_eq!(split_rank("Venom of the Snake", known), Some(("Venom of the Snake", 0)));
        assert_eq!(split_rank("Illusion: Human", known), None, "unknown spell");
        assert_eq!(split_rank("Mesmerization XI", known), None, "XI is not a rank we parse");
    }

    #[test]
    fn log_time_differences_are_exact() {
        let a = parse_log_time("Mon Aug 10 20:39:54 2026").unwrap();
        let b = parse_log_time("Mon Aug 10 20:40:32 2026").unwrap();
        assert_eq!(b - a, 38);
        let c = parse_log_time("Tue Aug 11 00:00:00 2026").unwrap();
        assert_eq!(c - a, 3 * 3600 + 20 * 60 + 6);
        let y = parse_log_time("Fri Jan  1 00:00:00 2027").unwrap();
        let x = parse_log_time("Thu Dec 31 23:59:59 2026").unwrap();
        assert_eq!(y - x, 1);
        assert_eq!(parse_log_time("not a time"), None);
        assert_eq!(parse_log_time("Mon Aug 10 20:39 2026"), None);
    }

    #[test]
    fn timer_events_classify_from_fixture_lines() {
        assert_eq!(classify("You begin casting Mesmerization VI."), Event::CastBegin { spell_text: "Mesmerization VI" });
        assert_eq!(classify("Your Shiftless Deeds spell fizzles!"), Event::Fizzle { spell: "Shiftless Deeds" });
        assert_eq!(classify("Your Mesmerization spell is interrupted."), Event::Interrupted { spell: "Mesmerization" });
        assert_eq!(classify("A spite golem resisted your Earthquake!"), Event::Resisted { target: "A spite golem", spell: "Earthquake" });
        assert_eq!(classify("Xicotl has taken 88 damage from your Gasping Embrace."), Event::DotTick { target: "Xicotl", spell: "Gasping Embrace" });
        assert_eq!(classify("Your Mesmerization spell has worn off of a flouting gargoyle."), Event::WornOff { spell: "Mesmerization", target: "a flouting gargoyle" });
        assert_eq!(classify("A Pickclaw guard has been awakened by Downslap."), Event::Awakened { target: "A Pickclaw guard" });
        assert_eq!(classify("You have slain a spiderling!"), Event::Slain { target: "a spiderling" });
        assert_eq!(classify("Zantetsu has been slain by Guard Wytiffin!"), Event::Slain { target: "Zantetsu" });
        assert_eq!(classify("You have entered The Northern Desert of Ro."), Event::ZoneChange);
        assert_eq!(classify("LOADING, PLEASE WAIT..."), Event::ZoneChange);
        assert_eq!(classify("a jeering gargoyle has been mesmerized."), Event::Other("a jeering gargoyle has been mesmerized."));
        assert_eq!(classify("A large rat bites YOU for 4 points of damage."), Event::Other("A large rat bites YOU for 4 points of damage."));
    }

    #[test]
    fn body_strips_the_timestamp() {
        assert_eq!(body("[Mon Aug 10 20:39:54 2026] You begin casting Pacify V."), "You begin casting Pacify V.");
        assert_eq!(body("no bracket"), "no bracket");
    }
}
