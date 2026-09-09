// SPDX-License-Identifier: MIT
//! One line of combat or healing -> one event carrying an amount. Pure.
//!
//! Every shape here was read from the reference fixture. Nothing without an
//! amount is an event: misses, dodges and crit rates are not this spec's
//! business. A suffix such as ` (Critical)` after the full stop is dropped
//! before matching.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageKind {
    Melee,
    Spell,
    Dot,
    Shield,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source<'a> {
    You,
    Named(&'a str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CombatEvent<'a> {
    /// `source` dealt `amount` to `target`. The target is never you here.
    Damage { source: Source<'a>, target: &'a str, amount: u64, kind: DamageKind },
    /// `source` dealt `amount` to you.
    Taken { source: &'a str, amount: u64, kind: DamageKind },
    /// `actual` landed; `potential` is what the spell could have healed
    /// (equal to `actual` when the log printed one number).
    Heal { source: Source<'a>, target: &'a str, actual: u64, potential: u64, hot: bool },
    /// Your pet told you it is attacking; `pet` is its printed name.
    PetAnnounce { pet: &'a str },
    /// Zone change or session start.
    Boundary,
}

/// `You kick a rat for 17 points of damage. (Critical)` -> without the note.
fn strip_note(body: &str) -> &str {
    match body.rfind(". (") {
        Some(i) if body.ends_with(')') => &body[..i + 1],
        _ => body,
    }
}

fn amount(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn is_plain_word(w: &str) -> bool {
    !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `<source> <verb>s <target>` -> (source, target). The verb is the first
/// plain word (letters, digits, underscore) ending in `s` that has a source
/// before it and a target after it; a backtick name like `Innoruuk`s Chosen`
/// is not a plain word and stays part of the source. Matches the reference
/// replay's leftmost split.
fn split_at_verb(left: &str) -> Option<(&str, &str)> {
    for (p, _) in left.match_indices(' ') {
        if p == 0 {
            continue;
        }
        let rest = &left[p + 1..];
        let (word, after) = rest.split_once(' ')?;
        if word.ends_with('s') && is_plain_word(word) && !after.is_empty() {
            return Some((&left[..p], after));
        }
    }
    None
}

fn source_of(name: &str) -> Source<'_> {
    if name == "You" { Source::You } else { Source::Named(name) }
}

pub fn classify(body: &str) -> Option<CombatEvent<'_>> {
    use CombatEvent::*;
    use DamageKind::*;

    if body.ends_with(" Master.'") {
        for sep in [" tells you, 'Attacking ", " told you, 'Attacking "] {
            if let Some((pet, _)) = body.split_once(sep) {
                return Some(PetAnnounce { pet });
            }
        }
        return None;
    }
    if body.starts_with("You have entered ")
        || body.starts_with("LOADING, PLEASE WAIT")
        || body == "Welcome to EverQuest Legends!"
    {
        return Some(Boundary);
    }

    let b = strip_note(body);

    // Melee: `… for N points of damage.` -- on you, yours, or another source's.
    if let Some(head) = b.strip_suffix(" points of damage.") {
        let (left, n) = head.rsplit_once(" for ")?;
        let amount = amount(n)?;
        if let Some(src) = left.strip_suffix(" YOU") {
            // A special attack with a preposition before YOU, e.g.
            // "An icy terror frenzies on YOU for 13 points of damage.": strip
            // the trailing " on" too, so the verb is still the plain word
            // ending in 's' that it would be without the preposition.
            let src = src.strip_suffix(" on").unwrap_or(src);
            if let Some((source, verb)) = src.rsplit_once(' ') {
                if verb.ends_with('s') && is_plain_word(verb) {
                    return Some(Taken { source, amount, kind: Melee });
                }
            }
        }
        if let Some(rest) = left.strip_prefix("You ") {
            let (_verb, target) = rest.split_once(' ')?;
            return Some(Damage { source: Source::You, target, amount, kind: Melee });
        }
        let (source, target) = split_at_verb(left)?;
        return Some(Damage { source: Source::Named(source), target, amount, kind: Melee });
    }

    // Damage shield: `<target> is <verb> by YOUR|<source>'s <thing> for N points of non-melee damage.`
    if let Some(head) = b.strip_suffix(" points of non-melee damage.") {
        let (left, n) = head.rsplit_once(" for ")?;
        let amount = amount(n)?;
        let (target, rest) = left.split_once(" is ")?;
        let (_verb, by) = rest.split_once(" by ")?;
        if by.strip_prefix("YOUR ").is_some() {
            return Some(Damage { source: Source::You, target, amount, kind: Shield });
        }
        let (source, _thing) = by.split_once("'s ")?;
        return Some(Damage { source: Source::Named(source), target, amount, kind: Shield });
    }

    // Damage shield on you: `YOU are <verb> by <source>'s <thing> for N
    // points of non-melee damage!` -- ends in `!`, so this must be checked
    // before the `.`-suffix branches below.
    if let Some(head) = b.strip_suffix(" points of non-melee damage!") {
        let (left, n) = head.rsplit_once(" for ")?;
        let amount = amount(n)?;
        let rest = left.strip_prefix("YOU are ")?;
        let (_verb, by) = rest.split_once(" by ")?;
        let (source, _thing) = by.split_once("'s ")?;
        return Some(Taken { source, amount, kind: Shield });
    }

    let head = b.strip_suffix('.')?;

    // Spell: `… for N points of <type> damage by <Spell>.`
    if let Some((left, _spell)) = head.rsplit_once(" damage by ") {
        let (left, _ty) = left.rsplit_once(" points of ")?;
        let (who, n) = left.rsplit_once(" for ")?;
        let amount = amount(n)?;
        if let Some(target) = who.strip_prefix("You hit ") {
            return Some(Damage { source: Source::You, target, amount, kind: Spell });
        }
        if let Some(source) = who.strip_suffix(" hit you") {
            return Some(Taken { source, amount, kind: Spell });
        }
        let (source, target) = who.split_once(" hit ")?;
        return Some(Damage { source: Source::Named(source), target, amount, kind: Spell });
    }

    // DoT ticks: `You have taken N damage from <Spell> by <source>.`,
    // `<target> has taken N damage from your <Spell>.`,
    // `<target> has taken N damage from <Spell> by <source>.` Source is
    // everything after the *last* " by ", since a spell name may itself
    // carry an apostrophe (a possessive bard song title); a line with no
    // " by " at all names no source and is not an event.
    if let Some(rest) = head.strip_prefix("You have taken ") {
        let (n, rest) = rest.split_once(" damage from ")?;
        let (_spell, source) = rest.split_once(" by ")?;
        return Some(Taken { source, amount: amount(n)?, kind: Dot });
    }
    if let Some((target, rest)) = head.split_once(" has taken ") {
        if let Some((n, from)) = rest.split_once(" damage from ") {
            let amount = amount(n)?;
            if from.strip_prefix("your ").is_some() {
                return Some(Damage { source: Source::You, target, amount, kind: Dot });
            }
            let (_spell, source) = from.rsplit_once(" by ")?;
            return Some(Damage { source: Source::Named(source), target, amount, kind: Dot });
        }
    }

    // Heals: `<source> healed <target>[ over time] for N[ (M)] hit points[ by <Spell>].`
    if head.contains(" hit points") {
        let left = match head.rsplit_once(" hit points by ") {
            Some((l, _spell)) => l,
            None => head.strip_suffix(" hit points")?,
        };
        let (who, amt) = left.rsplit_once(" for ")?;
        let (actual, potential) = match amt.split_once(" (") {
            Some((a, p)) => (amount(a)?, amount(p.strip_suffix(')')?)?),
            None => {
                let a = amount(amt)?;
                (a, a)
            }
        };
        let (source, target) = who.split_once(" healed ")?;
        let (target, hot) = match target.strip_suffix(" over time") {
            Some(t) => (t, true),
            None => (target, false),
        };
        return Some(Heal { source: source_of(source), target, actual, potential, hot });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use CombatEvent::*;
    use DamageKind::*;

    fn n(s: &str) -> Source<'_> { Source::Named(s) }

    #[test]
    fn your_melee_spell_dot_and_shield() {
        assert_eq!(classify("You kick a spiderling for 17 points of damage."),
            Some(Damage { source: Source::You, target: "a spiderling", amount: 17, kind: Melee }));
        assert_eq!(classify("You kick a bixie drone for 32 points of damage. (Critical)"),
            Some(Damage { source: Source::You, target: "a bixie drone", amount: 32, kind: Melee }));
        assert_eq!(classify("You hit a large wood spider for 44 points of damage."),
            Some(Damage { source: Source::You, target: "a large wood spider", amount: 44, kind: Melee }),
            "a plain melee 'hit' has no spell and counts as melee");
        assert_eq!(classify("You hit a spite golem for 961 points of magic damage by Discordant Mind. (Critical)"),
            Some(Damage { source: Source::You, target: "a spite golem", amount: 961, kind: Spell }));
        assert_eq!(classify("Xicotl has taken 88 damage from your Gasping Embrace."),
            Some(Damage { source: Source::You, target: "Xicotl", amount: 88, kind: Dot }));
        assert_eq!(classify("An abhorrent is pierced by YOUR thorns for 15 points of non-melee damage."),
            Some(Damage { source: Source::You, target: "An abhorrent", amount: 15, kind: Shield }));
    }

    #[test]
    fn other_sources_melee_spell_dot_and_shield() {
        assert_eq!(classify("Zaiv slashes a gnoll scout for 21 points of damage."),
            Some(Damage { source: n("Zaiv"), target: "a gnoll scout", amount: 21, kind: Melee }));
        assert_eq!(classify("Innoruuk`s Chosen slashes Zaiv for 30 points of damage."),
            Some(Damage { source: n("Innoruuk`s Chosen"), target: "Zaiv", amount: 30, kind: Melee }),
            "the verb is the first plain word ending in s, so a backtick name stays whole");
        assert_eq!(classify("A gnoll scout hits Zaiv for 5 points of damage."),
            Some(Damage { source: n("A gnoll scout"), target: "Zaiv", amount: 5, kind: Melee }));
        assert_eq!(classify("Kebanab hit Guard Wytiffin for 250 points of magic damage by Life Leech."),
            Some(Damage { source: n("Kebanab"), target: "Guard Wytiffin", amount: 250, kind: Spell }));
        assert_eq!(classify("a rat has taken 12 damage from Envenomed Bolt by Serenitee."),
            Some(Damage { source: n("Serenitee"), target: "a rat", amount: 12, kind: Dot }));
        assert_eq!(classify("Guard Xyxax is burned by Skullgrinder's flames for 15 points of non-melee damage."),
            Some(Damage { source: n("Skullgrinder"), target: "Guard Xyxax", amount: 15, kind: Shield }));
        assert_eq!(classify("Jennie`s warder claws a rat for 40 points of damage."),
            Some(Damage { source: n("Jennie`s warder"), target: "a rat", amount: 40, kind: Melee }));
    }

    #[test]
    fn other_sources_dot_ticks_split_on_the_last_by_not_an_apostrophe() {
        // The possessive rule (`from <source>'s <Spell>`) matched apostrophes
        // inside bard song names and credited phantom players. The real
        // shape names the spell first, then the source after the last " by ",
        // so a spell name containing its own apostrophe (a possessive song
        // title) no longer confuses the split (11,614 lines in the fixture).
        assert_eq!(classify("A rat has taken 12 damage from Envenomed Bolt by Serenitee."),
            Some(Damage { source: n("Serenitee"), target: "A rat", amount: 12, kind: Dot }));
        assert_eq!(classify("a spectre has taken 102 damage from Selo's Chords of Cessation VII by Kiil."),
            Some(Damage { source: n("Kiil"), target: "a spectre", amount: 102, kind: Dot }),
            "the apostrophe in the spell name no longer matters");
        assert_eq!(classify("A revultant rat has taken 30 damage by Deadly Poison."),
            None, "no ` by <source>` after `damage`: the line carries no source and is ignored (1,192 lines)");
    }

    #[test]
    fn a_damage_shield_lands_on_you() {
        // `YOU are <verb> by <source>'s <thing> for N points of non-melee
        // damage!` -- note `YOU are` and the line ends in `!` (19,077 lines).
        assert_eq!(classify("YOU are burned by a revultant rat's flames for 20 points of non-melee damage!"),
            Some(Taken { source: "a revultant rat", amount: 20, kind: Shield }));
    }

    #[test]
    fn damage_taken_by_you() {
        assert_eq!(classify("A giant thicket rat bites YOU for 2 points of damage. (Riposte)"),
            Some(Taken { source: "A giant thicket rat", amount: 2, kind: Melee }));
        assert_eq!(classify("Princess Cherista hit you for 175 points of poison damage by Shock of Poison."),
            Some(Taken { source: "Princess Cherista", amount: 175, kind: Spell }));
        assert_eq!(classify("You have taken 30 damage from Deadly Poison by a revultant rat."),
            Some(Taken { source: "a revultant rat", amount: 30, kind: Dot }));
    }

    #[test]
    fn a_special_attack_with_a_preposition_before_you_is_taken_damage() {
        // "An icy terror frenzies on YOU for 13 points of damage." -- after
        // stripping " YOU", also strip a trailing " on"; the remaining verb
        // still ends in 's', so this is the `<verb>s YOU` shape after all
        // (fixture: 245 such lines, found while debugging the Spec 3 replay).
        assert_eq!(classify("An icy terror frenzies on YOU for 13 points of damage."),
            Some(Taken { source: "An icy terror", amount: 13, kind: Melee }));
    }

    #[test]
    fn heals_with_and_without_overheal_and_over_time() {
        assert_eq!(classify("You healed Vibtik for 128 (197) hit points by Valor."),
            Some(Heal { source: Source::You, target: "Vibtik", actual: 128, potential: 197, hot: false }));
        assert_eq!(classify("You healed Daggo over time for 102 hit points by Ethereal Cleansing."),
            Some(Heal { source: Source::You, target: "Daggo", actual: 102, potential: 102, hot: true }));
        assert_eq!(classify("Alpharius healed himself for 73 hit points."),
            Some(Heal { source: n("Alpharius"), target: "himself", actual: 73, potential: 73, hot: false }));
        assert_eq!(classify("Cauli healed himself for 0 (229) hit points by Armor of Protection."),
            Some(Heal { source: n("Cauli"), target: "himself", actual: 0, potential: 229, hot: false }));
        assert_eq!(classify("Penuche healed you for 535 (556) hit points by Symbol of Naltron."),
            Some(Heal { source: n("Penuche"), target: "you", actual: 535, potential: 556, hot: false }));
    }

    #[test]
    fn pet_announcements_and_boundaries() {
        assert_eq!(classify("A revultant rat told you, 'Attacking an abhorrent Master.'"), Some(PetAnnounce { pet: "A revultant rat" }));
        assert_eq!(classify("Jarann tells you, 'Attacking a rat Master.'"), Some(PetAnnounce { pet: "Jarann" }));
        assert_eq!(classify("Vibtik told you, 'I am unable to wake Brother Jentry, Master.'"), None, "only attack announcements name a pet");
        assert_eq!(classify("You have entered The Northern Desert of Ro."), Some(Boundary));
        assert_eq!(classify("LOADING, PLEASE WAIT..."), Some(Boundary));
        assert_eq!(classify("Welcome to EverQuest Legends!"), Some(Boundary));
    }

    #[test]
    fn lines_without_an_amount_are_nothing() {
        assert_eq!(classify("You try to punch a halfling skeleton, but miss!"), None);
        assert_eq!(classify("A large rat bites YOU for 4 points of damage"), None, "no full stop: not the shape");
        assert_eq!(classify("Barrin tells general2:1, 'for 100 points of damage.'"), None);
        assert_eq!(classify("Your Mesmerization spell has worn off of a rat."), None);
        assert_eq!(classify("You gain experience! (0.002%)"), None);
    }
}
