// SPDX-License-Identifier: MIT
//! Snapshot + layout -> block views, and the HUD-side session accumulation.
//!
//! Pure, like `theme.rs`: no display, no window, nothing but arithmetic and
//! sorting over the wire types and the config's layout. `paint.rs` turns a
//! `BlockView` into canvas calls; this file only decides what a block says.

use crate::backend::Rect;
use crate::draw::Rgba;
use crate::theme::{Theme, CRITICAL_SECS, WARNING_SECS};
use std::collections::HashMap;
use wisp_config::layout::{Anchor, Block, BlockKind, Layout, Segment, Shows};
use wisp_proto::{Confidence, Encounter, Snapshot, Timer};

#[derive(Debug, Clone, PartialEq)]
pub enum RowState {
    Normal,
    Estimated,
    Warning,
    Critical,
    Gone,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Number {
    pub text: String,
    pub bold: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub name: String,
    /// timers: the kind label (+ " · est"); meter rows have none.
    pub tag: Option<String>,
    /// meter: `[total, rate(bold), "N%"]`; timers: `[time]`.
    pub numbers: Vec<Number>,
    /// warning/critical/gone time colour; `None` otherwise.
    pub number_colour: Option<Rgba>,
    /// 0..=1; 0 for a `Gone` row.
    pub fill: f32,
    /// kind colour (timers), or damage/healing/you bar (meter).
    pub bar: Rgba,
    pub state: RowState,
    pub you: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub label: Option<String>,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlockView {
    /// Position in `layout.blocks`.
    pub index: usize,
    pub kind: BlockKind,
    pub hidden: bool,
    /// Placed; height follows the content (a hidden block: header + 2 rows).
    pub rect: Rect,
    /// "Damage · fight", "Healing · session", "Timers".
    pub title: String,
    /// "0:42" (fight clock) / "4" (timer count).
    pub right: String,
    /// HUD mode's name tag: "meter · damage · fight" / "timers".
    pub tag: String,
    pub groups: Vec<Group>,
}

/// HUD-side accumulation for `segment = session`: per-name (damage, healing)
/// totals folded per fight, and the total seconds those fights ran, plus
/// whatever fight is currently in progress (not yet folded).
#[derive(Debug, Default, Clone)]
pub struct Session {
    damage_totals: HashMap<String, u64>,
    healing_totals: HashMap<String, u64>,
    you_damage_total: u64,
    you_healing_total: u64,
    seconds_total: u64,
    current: Option<Encounter>,
}

impl Session {
    /// Call once per snapshot, before `build`. A fight is folded into the
    /// totals when the encounter goes `None` or its `duration_s` drops below
    /// the last seen value (a new fight started while the old one lingered).
    pub fn observe(&mut self, encounter: Option<&Encounter>) {
        match encounter {
            Some(e) => {
                if let Some(prev) = &self.current {
                    if e.duration_s < prev.duration_s {
                        let prev = prev.clone();
                        self.fold(&prev);
                    }
                }
                self.current = Some(e.clone());
            }
            None => {
                if let Some(prev) = self.current.take() {
                    self.fold(&prev);
                }
            }
        }
    }

    /// Folded seconds plus the fight in progress, if any.
    pub fn seconds(&self) -> u64 {
        self.seconds_total + self.current.as_ref().map(|c| c.duration_s).unwrap_or(0)
    }

    fn fold(&mut self, e: &Encounter) {
        self.seconds_total += e.duration_s;
        self.you_damage_total += e.you.damage;
        self.you_healing_total += e.you.healing;
        for r in &e.damage {
            *self.damage_totals.entry(r.name.clone()).or_insert(0) += r.amount;
        }
        for r in &e.healing {
            *self.healing_totals.entry(r.name.clone()).or_insert(0) += r.amount;
        }
    }
}

/// Height a block needs at this theme, for `rows` rows and `groups` group
/// labels (a meter block never has a target label, so it always passes 0).
pub fn block_height(theme: &Theme, kind: BlockKind, rows: u32, groups: u32) -> u32 {
    let header = theme.header_px.ceil() as u32 + 2 * theme.header_pad_y;
    let rows_h = rows * (theme.row_h + theme.row_gap);
    let labels_h = match kind {
        BlockKind::Timers => groups * (theme.group_gap + theme.target_px.ceil() as u32),
        BlockKind::Meter => 0,
    };
    header + rows_h + labels_h + theme.row_inset
}

/// Spec §4.5: the offset is measured from the anchor towards the screen's
/// centre; width and offsets scale with the theme, height is given (it
/// already came out of `block_height` at this theme).
pub fn place(block: &Block, theme: &Theme, height: u32, screen: (u32, u32)) -> Rect {
    let scaled = |v: i32| ((v as f32) * theme.scale).round() as i32;
    let width = ((block.width() as f32) * theme.scale).round() as u32;
    let ox = scaled(block.offset[0]);
    let oy = scaled(block.offset[1]);
    let (sw, sh) = (screen.0 as i32, screen.1 as i32);

    let x = match block.anchor {
        Anchor::TopLeft | Anchor::Left | Anchor::BottomLeft => ox,
        Anchor::Top | Anchor::Center | Anchor::Bottom => (sw - width as i32) / 2 + ox,
        Anchor::TopRight | Anchor::Right | Anchor::BottomRight => sw - width as i32 - ox,
    };
    let y = match block.anchor {
        Anchor::TopLeft | Anchor::Top | Anchor::TopRight => oy,
        Anchor::Left | Anchor::Center | Anchor::Right => (sh - height as i32) / 2 + oy,
        Anchor::BottomLeft | Anchor::Bottom | Anchor::BottomRight => sh - height as i32 - oy,
    };
    Rect::new(x, y, width, height)
}

pub fn build(snapshot: &Snapshot, layout: &Layout, theme: &Theme, session: &Session, screen: (u32, u32)) -> Vec<BlockView> {
    layout
        .blocks
        .iter()
        .enumerate()
        .map(|(index, block)| build_block(index, block, snapshot, theme, session, screen))
        .collect()
}

fn hud_tag(block: &Block) -> String {
    match block.kind {
        BlockKind::Meter => format!(
            "meter · {} · {}",
            match block.shows {
                Shows::Damage => "damage",
                Shows::Healing => "healing",
            },
            match block.segment {
                Segment::Fight => "fight",
                Segment::Session => "session",
            }
        ),
        BlockKind::Timers => "timers".to_string(),
    }
}

fn build_block(index: usize, block: &Block, snapshot: &Snapshot, theme: &Theme, session: &Session, screen: (u32, u32)) -> BlockView {
    let tag = hud_tag(block);
    if block.hidden {
        let height = block_height(theme, block.kind, 2, 0);
        let rect = place(block, theme, height, screen);
        return BlockView { index, kind: block.kind, hidden: true, rect, title: String::new(), right: String::new(), tag, groups: Vec::new() };
    }
    match block.kind {
        BlockKind::Meter => build_meter(index, block, snapshot, theme, session, screen, tag),
        BlockKind::Timers => build_timers(index, block, snapshot, theme, screen, tag),
    }
}

fn build_meter(
    index: usize,
    block: &Block,
    snapshot: &Snapshot,
    theme: &Theme,
    session: &Session,
    screen: (u32, u32),
    tag: String,
) -> BlockView {
    let title = format!(
        "{} · {}",
        match block.shows {
            Shows::Damage => "Damage",
            Shows::Healing => "Healing",
        },
        match block.segment {
            Segment::Fight => "fight",
            Segment::Session => "session",
        }
    );

    let Some(encounter) = &snapshot.encounter else {
        let height = block_height(theme, block.kind, 0, 0);
        let rect = place(block, theme, height, screen);
        return BlockView { index, kind: block.kind, hidden: false, rect, title, right: "-:--".to_string(), tag, groups: Vec::new() };
    };

    let (mut sources, you_total, you_rate) = match block.segment {
        Segment::Fight => {
            let src: Vec<(String, u64, u64)> = match block.shows {
                Shows::Damage => encounter.damage.iter().map(|r| (r.name.clone(), r.amount, r.per_s)).collect(),
                Shows::Healing => encounter.healing.iter().map(|r| (r.name.clone(), r.amount, r.per_s)).collect(),
            };
            let (you_total, you_rate) = match block.shows {
                Shows::Damage => (encounter.you.damage, encounter.you.dps),
                Shows::Healing => (encounter.you.healing, encounter.you.hps),
            };
            (src, you_total, you_rate)
        }
        Segment::Session => {
            let seconds = session.seconds().max(1) as f64;
            let (folded, current_rows, you_folded, you_current) = match block.shows {
                Shows::Damage => (&session.damage_totals, &encounter.damage, session.you_damage_total, encounter.you.damage),
                Shows::Healing => (&session.healing_totals, &encounter.healing, session.you_healing_total, encounter.you.healing),
            };
            let mut combined: HashMap<String, u64> = folded.clone();
            for r in current_rows.iter() {
                *combined.entry(r.name.clone()).or_insert(0) += r.amount;
            }
            let src: Vec<(String, u64, u64)> = combined
                .into_iter()
                .map(|(name, amount)| {
                    let rate = (amount as f64 / seconds).round() as u64;
                    (name, amount, rate)
                })
                .collect();
            let you_total = you_folded + you_current;
            let you_rate = (you_total as f64 / seconds).round() as u64;
            (src, you_total, you_rate)
        }
    };

    if you_total > 0 {
        sources.push(("You".to_string(), you_total, you_rate));
    }
    sources.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let sum: u64 = sources.iter().map(|s| s.1).sum();
    let top = sources.first().map(|s| s.1).unwrap_or(0);
    let cap = block.rows() as usize;

    let rows: Vec<Row> = sources
        .into_iter()
        .take(cap)
        .map(|(name, amount, rate)| {
            let share = if sum == 0 { 0 } else { (amount as f64 * 100.0 / sum as f64).round() as u64 };
            let fill = if top == 0 { 0.0 } else { amount as f32 / top as f32 };
            let you = name == "You";
            let bar = if you {
                theme.you_bar
            } else {
                match block.shows {
                    Shows::Damage => theme.damage_bar,
                    Shows::Healing => theme.healing_bar,
                }
            };
            Row {
                name,
                tag: None,
                numbers: vec![
                    Number { text: compact(amount), bold: false },
                    Number { text: rate.to_string(), bold: true },
                    Number { text: format!("{share}%"), bold: false },
                ],
                number_colour: None,
                fill,
                bar,
                state: RowState::Normal,
                you,
            }
        })
        .collect();

    let right = match block.segment {
        Segment::Fight => clock(encounter.duration_s),
        Segment::Session => clock(session.seconds()),
    };
    let height = block_height(theme, block.kind, rows.len() as u32, 0);
    let rect = place(block, theme, height, screen);
    let groups = vec![Group { label: None, rows }];

    BlockView { index, kind: block.kind, hidden: false, rect, title, right, tag, groups }
}

fn build_timers(index: usize, block: &Block, snapshot: &Snapshot, theme: &Theme, screen: (u32, u32), tag: String) -> BlockView {
    let cap = block.rows() as usize;
    let mut groups: Vec<Group> = Vec::new();
    let mut group_index: HashMap<String, usize> = HashMap::new();

    for t in snapshot.timers.iter().take(cap) {
        let idx = *group_index.entry(t.target.clone()).or_insert_with(|| {
            groups.push(Group { label: Some(t.target.clone()), rows: Vec::new() });
            groups.len() - 1
        });
        groups[idx].rows.push(build_timer_row(t, theme));
    }

    let rows_total: u32 = groups.iter().map(|g| g.rows.len() as u32).sum();
    let height = block_height(theme, block.kind, rows_total, groups.len() as u32);
    let rect = place(block, theme, height, screen);

    BlockView {
        index,
        kind: block.kind,
        hidden: false,
        rect,
        title: "Timers".to_string(),
        right: snapshot.timers.len().to_string(),
        tag,
        groups,
    }
}

fn build_timer_row(t: &Timer, theme: &Theme) -> Row {
    let name = if t.rank > 0 { format!("{} {}", t.spell, roman(t.rank)) } else { t.spell.clone() };
    let state = if t.remaining_ms < 0 {
        RowState::Gone
    } else if t.remaining_ms <= CRITICAL_SECS * 1000 {
        RowState::Critical
    } else if t.remaining_ms <= WARNING_SECS * 1000 {
        RowState::Warning
    } else if t.confidence == Confidence::Estimated {
        RowState::Estimated
    } else {
        RowState::Normal
    };

    let mut tag = Theme::kind_label(t.kind, t.damage_type).to_string();
    if state == RowState::Estimated {
        tag.push_str(" · est");
    }

    let number_colour = match state {
        RowState::Warning => Some(theme.warning),
        RowState::Critical | RowState::Gone => Some(theme.critical),
        RowState::Normal | RowState::Estimated => None,
    };

    let fill = if state == RowState::Gone {
        0.0
    } else {
        (t.remaining_ms as f32 / t.duration_ms as f32).clamp(0.0, 1.0)
    };

    Row {
        name,
        tag: Some(tag),
        numbers: vec![Number { text: remaining_text(t.remaining_ms), bold: false }],
        number_colour,
        fill,
        bar: theme.kind_colour(t.kind, t.damage_type),
        state,
        you: false,
    }
}

/// 999 -> "999", 18234 -> "18.2k", 1320500 -> "1.32M". Moved from `main.rs`,
/// unchanged.
pub fn compact(n: u64) -> String {
    if n < 10_000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        format!("{:.2}M", n as f64 / 1_000_000.0)
    }
}

/// "0:42", "12:05".
pub fn clock(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// "18s" under a minute, "1:40" at or over one, "gone" when negative (the
/// daemon's post-expiry hold).
pub fn remaining_text(ms: i64) -> String {
    if ms < 0 {
        return "gone".to_string();
    }
    let secs = ms / 1000;
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

/// Moved from `main.rs`, unchanged.
pub fn roman(rank: u8) -> &'static str {
    ["", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"].get(rank as usize).copied().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_proto::{Confidence, DamageType, MeterRow, Personal, TimerKind};

    fn timer(remaining_ms: i64, confidence: Confidence) -> Timer {
        Timer {
            target: "a jeering gargoyle".to_string(),
            spell: "Mesmerization".to_string(),
            rank: 6,
            kind: TimerKind::Mez,
            damage_type: None,
            remaining_ms,
            duration_ms: 38_000,
            confidence,
        }
    }

    fn fight(active: bool) -> Encounter {
        Encounter {
            active,
            duration_s: 42,
            you: Personal { damage: 18_234, dps: 434, taken: 2_210, taken_ps: 52, healing: 900, hps: 21, overheal: 120 },
            damage: vec![MeterRow { name: "Serenitee".to_string(), amount: 1_320_500, per_s: 286 }],
            healing: vec![MeterRow { name: "Misery".to_string(), amount: 3_100, per_s: 74 }],
        }
    }

    fn snap(encounter: Option<Encounter>) -> Snapshot {
        Snapshot { v: 5, seq: 1, ts: String::new(), log: None, lines_ingested: 0, session_kills: 7, timers: vec![], encounter }
    }

    fn theme() -> Theme {
        Theme::at(1.0)
    }

    fn meter(shows: Shows) -> Layout {
        let mut l = Layout::default_layout();
        l.blocks[0].shows = shows;
        l
    }

    #[test]
    fn you_are_one_row_sorted_in_place_and_highlighted() {
        let s = snap(Some(fight(true))); // you: damage 18_234 dps 434; Serenitee 1_320_500 @ 286
        let v = build(&s, &meter(Shows::Damage), &theme(), &Session::default(), (2560, 1440));
        let rows = &v[0].groups[0].rows;
        assert_eq!(rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["Serenitee", "You"]);
        assert!(rows[1].you && !rows[0].you);
        assert_eq!(rows[1].bar, theme().you_bar);
        assert_eq!(rows.iter().filter(|r| r.you).count(), 1);
        assert_eq!(
            rows[1].numbers,
            vec![
                Number { text: "18.2k".into(), bold: false },
                Number { text: "434".into(), bold: true },
                Number { text: "1%".into(), bold: false }
            ]
        );
        assert!((rows[1].fill - 18_234.0 / 1_320_500.0).abs() < 1e-6);
        assert_eq!(rows[0].fill, 1.0);
        assert_eq!((v[0].title.as_str(), v[0].right.as_str()), ("Damage · fight", "0:42"));
    }

    #[test]
    fn a_healing_block_shows_your_healing_once_and_a_zero_you_is_omitted() {
        let v = build(&snap(Some(fight(true))), &meter(Shows::Healing), &theme(), &Session::default(), (2560, 1440));
        let rows = &v[0].groups[0].rows;
        assert_eq!(rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["Misery", "You"]);
        assert_eq!(rows[0].bar, theme().healing_bar);
        let mut e = fight(true);
        e.you.healing = 0;
        e.you.hps = 0;
        let v = build(&snap(Some(e)), &meter(Shows::Healing), &theme(), &Session::default(), (2560, 1440));
        assert_eq!(v[0].groups[0].rows.len(), 1);
    }

    #[test]
    fn no_encounter_is_an_empty_block_with_a_dashed_clock() {
        let v = build(&snap(None), &Layout::default_layout(), &theme(), &Session::default(), (2560, 1440));
        assert!(v[0].groups.is_empty());
        assert_eq!(v[0].right, "-:--");
    }

    #[test]
    fn timers_group_by_target_in_order_with_states_and_colours() {
        let mut s = snap(None);
        let a = timer(18_000, Confidence::Measured); // gargoyle, Mesmerization VI, mez
        let mut b = timer(9_000, Confidence::Measured);
        b.target = "an elite gnoll shaman".into();
        b.spell = "Envenomed Bolt".into();
        b.rank = 0;
        b.kind = TimerKind::Dot;
        b.damage_type = Some(DamageType::Poison);
        let mut c = timer(41_000, Confidence::Estimated);
        c.spell = "Turgur's Insects".into();
        c.rank = 0;
        c.kind = TimerKind::Slow;
        let mut d = timer(-2_000, Confidence::Measured);
        d.target = "an elite gnoll shaman".into();
        d.spell = "Tashani".into();
        d.rank = 0;
        d.kind = TimerKind::Debuff;
        let mut e = timer(4_000, Confidence::Measured);
        e.target = "an elite gnoll shaman".into();
        e.spell = "Malaise".into();
        e.rank = 0;
        e.kind = TimerKind::Debuff;
        s.timers = vec![a, b, c, d, e];
        let v = build(&s, &Layout::default_layout(), &theme(), &Session::default(), (2560, 1440));
        let t = &v[1];
        assert_eq!(t.title, "Timers");
        assert_eq!(t.right, "5");
        assert_eq!(
            t.groups.iter().map(|g| g.label.clone().unwrap()).collect::<Vec<_>>(),
            ["a jeering gargoyle", "an elite gnoll shaman"]
        );
        let g0 = &t.groups[0].rows;
        let g1 = &t.groups[1].rows;
        assert_eq!(
            (g0[0].name.as_str(), g0[0].tag.as_deref(), g0[0].numbers[0].text.as_str()),
            ("Mesmerization VI", Some("mez"), "18s")
        );
        assert_eq!(g0[0].state, RowState::Normal);
        assert_eq!(g0[0].bar, Rgba::rgb(0xff79c6));
        assert_eq!((g0[1].tag.as_deref(), g0[1].state.clone()), (Some("slow · est"), RowState::Estimated));
        assert_eq!(
            (g1[0].tag.as_deref(), g1[0].state.clone(), g1[0].number_colour),
            (Some("poison"), RowState::Warning, Some(theme().warning))
        );
        assert_eq!((g1[1].numbers[0].text.as_str(), g1[1].state.clone(), g1[1].fill), ("gone", RowState::Gone, 0.0));
        assert_eq!((g1[2].state.clone(), g1[2].number_colour), (RowState::Critical, Some(theme().critical)));
        assert!((g0[0].fill - 18.0 / 38.0).abs() < 1e-6);
    }

    #[test]
    fn the_row_cap_counts_across_groups() {
        let mut s = snap(None);
        s.timers = (0..6)
            .map(|i| {
                let mut t = timer(1000 * (i + 1), Confidence::Measured);
                if i >= 3 {
                    t.target = "b".into();
                }
                t
            })
            .collect();
        let mut l = Layout::default_layout();
        l.blocks[1].rows = Some(4);
        let v = build(&s, &l, &theme(), &Session::default(), (2560, 1440));
        assert_eq!(v[1].groups.iter().map(|g| g.rows.len()).sum::<usize>(), 4);
        assert_eq!(v[1].right, "6");
    }

    #[test]
    fn placement_measures_from_the_anchor_towards_the_centre() {
        let th = theme();
        let mut b = Block::new(BlockKind::Meter);
        b.offset = [20, 120];
        b.width = Some(290);
        let h = 200;
        assert_eq!(place(&b, &th, h, (2560, 1440)), Rect::new(20, 120, 290, 200));
        b.anchor = Anchor::BottomRight;
        assert_eq!(place(&b, &th, h, (2560, 1440)), Rect::new(2560 - 290 - 20, 1440 - 200 - 120, 290, 200));
        b.anchor = Anchor::Center;
        b.offset = [0, 0];
        assert_eq!(place(&b, &th, h, (2560, 1440)), Rect::new((2560 - 290) / 2, (1440 - 200) / 2, 290, 200));
        b.anchor = Anchor::Top;
        b.offset = [10, 5];
        assert_eq!(place(&b, &th, h, (2560, 1440)), Rect::new((2560 - 290) / 2 + 10, 5, 290, 200));
        let th2 = Theme::at(1.5);
        b.anchor = Anchor::TopLeft;
        b.offset = [20, 120];
        assert_eq!(place(&b, &th2, h, (2560, 1440)), Rect::new(30, 180, 435, 200));
    }

    #[test]
    fn the_session_folds_fights_and_keeps_the_current_one() {
        let mut s = Session::default();
        let mut e = fight(true);
        e.duration_s = 10;
        e.you.damage = 1000;
        e.damage[0].amount = 5000;
        s.observe(Some(&e));
        e.duration_s = 20;
        e.you.damage = 2000;
        e.damage[0].amount = 9000;
        s.observe(Some(&e));
        s.observe(None); // fight 1 folded: you 2000, Serenitee 9000, 20 s
        let mut f = fight(true);
        f.duration_s = 5;
        f.you.damage = 100;
        f.damage[0].amount = 100;
        s.observe(Some(&f)); // fight 2 in progress
        assert_eq!(s.seconds(), 25);
        let mut l = Layout::default_layout();
        l.blocks[0].segment = Segment::Session;
        let v = build(&snap(Some(f)), &l, &theme(), &s, (2560, 1440));
        let rows = &v[0].groups[0].rows;
        assert_eq!((rows[0].name.as_str(), rows[0].numbers[0].text.as_str()), ("Serenitee", "9100"));
        assert_eq!(
            (rows[1].name.as_str(), rows[1].numbers[0].text.as_str(), rows[1].numbers[1].text.as_str()),
            ("You", "2100", "84")
        );
        assert_eq!(v[0].title, "Damage · session");
    }

    #[test]
    fn text_helpers() {
        assert_eq!(compact(999), "999");
        assert_eq!(compact(9_999), "9999");
        assert_eq!(compact(18_234), "18.2k");
        assert_eq!(compact(1_320_500), "1.32M");
        assert_eq!(clock(42), "0:42");
        assert_eq!(clock(725), "12:05");
        assert_eq!(remaining_text(18_400), "18s");
        assert_eq!(remaining_text(60_000), "1:00");
        assert_eq!(remaining_text(100_000), "1:40");
        assert_eq!(remaining_text(-1), "gone");
        assert_eq!(remaining_text(0), "0s");
        assert_eq!(Theme::kind_label(TimerKind::Dot, Some(DamageType::Unresistable)), "dot");
        assert_eq!(Theme::kind_label(TimerKind::Dot, Some(DamageType::Fire)), "fire");
        assert_eq!(theme().kind_colour(TimerKind::Dot, Some(DamageType::Chromatic)), Rgba::rgb(0x94a3b8));
    }
}
