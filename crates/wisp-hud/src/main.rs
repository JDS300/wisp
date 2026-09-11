// SPDX-License-Identifier: MIT
// crates/wisp-hud/src/main.rs
mod backend;
#[allow(dead_code)] // Task 7 rewires the HUD onto this module; text.rs still draws it today.
mod draw;
mod text;

use backend::OverlayBackend;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use wisp_config::config::{Config, Key};
use wisp_probe::BackendKind;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // args_os, not args: OsStr-clean throughout, no String round-trip. Only
    // --backend and --scale need to become text at all, one being a backend
    // name and the other parsed as a float.
    let args: Vec<OsString> = std::env::args_os().collect();
    let backend_flag = or_refuse(flag_value(&args, "backend"));
    let scale_flag = or_refuse(flag_value(&args, "scale"));

    // The config is read once, and everything it has to say about itself is
    // said once here, before any value from it is acted on.
    let (config, warnings) = startup_config();
    for warning in &warnings {
        eprintln!("{warning}");
    }

    // DEFAULT_SCALE is the default only when neither the flag nor the file
    // says otherwise. A present-but-bad value is refused with a clear error and
    // exit 2, not silently swapped for the default -- and the message names
    // whichever of the two it came from.
    let scale = or_refuse(scale_of(resolve(scale_flag.as_deref(), config.get(Key::Scale))));

    // Neither the flag nor the file said, so detection decides, as before.
    let kind = or_refuse(backend_of(resolve(backend_flag.as_deref(), config.get(Key::Backend))))
        .unwrap_or_else(|| wisp_probe::detect().kind);
    eprintln!("wisp-hud: backend {kind:?}, scale {scale}px");

    let renderer = text::Renderer::new(scale);

    // Size the window from the renderer: the kill line, the personal line,
    // MAX_ROWS timer rows, then the full set of meter rows, all at their
    // widest, padded, so the HUD never clips at this scale.
    const PAD: u32 = 8;
    let widest = std::iter::once(text::Line { text: "999999 kills".to_string(), rgb: WHITE })
        .chain(std::iter::once(text::Line { text: "DPS 99999  in 9999/s  HPS 9999   99:59".to_string(), rgb: WHITE }))
        .chain((0..MAX_ROWS).map(|_| text::Line { text: format_row("W".repeat(TARGET_COLS).as_str(), &"W".repeat(SPELL_COLS), 9999), rgb: WHITE }))
        .chain((0..MAX_DAMAGE_ROWS + MAX_HEALING_ROWS).map(|_| text::Line {
            text: format!("{} {:>7} {:>5}/s  +", "W".repeat(NAME_COLS), "999.9k", 99999),
            rgb: WHITE,
        }))
        .collect::<Vec<_>>();
    let probe = renderer.render_lines(&widest);
    let (w, h) = (probe.width + 2 * PAD, probe.height + 2 * PAD);

    let mut surface: Box<dyn OverlayBackend> = match kind {
        BackendKind::GamescopeX11 => {
            Box::new(backend::gamescope_x11::GamescopeX11Backend::new(w, h))
        }
        BackendKind::WlrLayerShell => Box::new(backend::layer_shell::LayerShellBackend::new(w, h)),
        BackendKind::PlainWindow => Box::new(backend::plain_window::PlainWindowBackend::new(w, h)),
    };
    surface.attach()?;

    let path = wisp_config::paths::socket_path();
    let mut stream = wisp_proto::client::connect(&path)?;
    eprintln!("wisp-hud: connected to {}", path.display());

    while let Some(item) = stream.next_snapshot() {
        match item {
            Ok(snap) => {
                let frame = renderer.render_lines(&hud_lines(&snap));
                if let Err(e) = surface.present(&frame) {
                    eprintln!("wisp-hud: {e}");
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("wisp-hud: {e}");
                std::process::exit(1);
            }
        }
    }
    // next_snapshot already logged a read error, if that's why the loop
    // ended; only print "closed the connection" for the other case, a clean
    // EOF, so a read failure is not followed by a second, misleading line.
    if !stream.had_error() {
        eprintln!("wisp-hud: daemon closed the connection");
    }
    Ok(())
}

/// The scale to render at when neither the flag nor the config file says.
const DEFAULT_SCALE: f32 = 48.0;

/// Where a startup value was written down, so a bad one can be blamed
/// correctly: a mistyped flag and a typo in the config file are fixed in
/// different places, and the message has to say which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    Flag,
    Config,
}

/// A flag beats the file; the file beats the default. Returns the winning
/// text and where it came from, so a bad value can be blamed correctly.
///
/// `None` is "neither said", which is not a value: each reader turns it into
/// its own default, and only the reader knows what that is.
fn resolve(flag: Option<&str>, file: Option<&str>) -> Option<(String, Origin)> {
    match (flag, file) {
        (Some(value), _) => Some((value.to_string(), Origin::Flag)),
        (None, Some(value)) => Some((value.to_string(), Origin::Config)),
        (None, None) => None,
    }
}

/// A value refused at startup, before any window is opened: the line for
/// stderr and the code to exit with.
#[derive(Debug, PartialEq, Eq)]
struct Refusal {
    message: String,
    code: i32,
}

/// The two ways a refusal has to speak for a value that left nothing to print.
/// They are different mistakes: `(empty)` is a value the user wrote and left
/// blank (`--scale ""`, or `scale =` in the file), `(missing)` is a flag with
/// nothing after it at all.
const EMPTY: &str = "(empty)";
const MISSING: &str = "(missing)";

/// The refusal for a value that is present but unusable, naming its origin:
/// `--scale` for a flag, the config key for a file value. Both are exit 2, the
/// code for "you invoked me wrongly", so a config typo is as diagnosable as a
/// mistyped flag and neither is a crash.
///
/// An empty value renders as `(empty)`: interpolated raw it would leave the line
/// ending in `": "`, which reads as a message that lost its value rather than as
/// the value the user actually wrote.
fn refusal(origin: Origin, key: &str, value: &str) -> Refusal {
    let value = if value.is_empty() { EMPTY } else { value };
    let message = match origin {
        Origin::Flag => format!("wisp-hud: invalid --{key} value: {value}"),
        Origin::Config => format!("wisp-hud: invalid config {key}: {value}"),
    };
    Refusal { message, code: 2 }
}

/// Say the line and exit. Diverging, so a refusal arm stays an expression in
/// whatever `match` produces the value.
fn refuse(refused: Refusal) -> ! {
    eprintln!("{}", refused.message);
    std::process::exit(refused.code);
}

/// The value, or the refusal printed and acted on. Reads like `?` for a
/// refusal, which `?` cannot be: `main` returns a `Result`, and a refusal is
/// exit 2 with one specific line, not an error to be reported and unwound.
fn or_refuse<T>(result: Result<T, Refusal>) -> T {
    match result {
        Ok(value) => value,
        Err(refused) => refuse(refused),
    }
}

/// The value written after `--<key>`, or `None` when the flag is absent.
///
/// A flag with nothing after it and a value that is not valid UTF-8 are both
/// refusals, not an absent flag. Treating either as absent would run the default
/// and print it as chosen -- a scale or a backend the user did ask about, just
/// not one they can be given, which is worth two exit codes and a line rather
/// than a silent substitution.
fn flag_value(args: &[OsString], key: &str) -> Result<Option<String>, Refusal> {
    let flag = format!("--{key}");
    let Some(i) = args.iter().position(|a| *a == OsStr::new(&flag)) else {
        return Ok(None);
    };
    match args.get(i + 1) {
        None => Err(refusal(Origin::Flag, key, MISSING)),
        Some(value) => match value.to_str() {
            Some(text) => Ok(Some(text.to_string())),
            // Lossy so the line still shows something the user can recognise.
            // The refusal is about the value being unusable, not about its
            // encoding, and one U+FFFD per undecodable byte says that.
            None => Err(refusal(Origin::Flag, key, &value.to_string_lossy())),
        },
    }
}

/// The winning scale, or the refusal it earns. The text is parsed here rather
/// than in `wisp-config` because the message has to repeat the raw value and
/// say where it came from, and only the reader knows both.
fn scale_of(resolved: Option<(String, Origin)>) -> Result<f32, Refusal> {
    let Some((text, origin)) = resolved else {
        return Ok(DEFAULT_SCALE);
    };
    text.parse::<f32>().map_err(|_| refusal(origin, "scale", &text))
}

/// The winning backend, or the refusal it earns. `Ok(None)` is "neither said",
/// which is where `wisp_probe::detect()` comes in -- detection needs a display,
/// so it stays out of this function and out of the tests. The three names are
/// `BackendKind::parse`'s, in `wisp-probe`, so this crate never spells one and
/// `wisp doctor` cannot disagree with what the HUD is about to do.
fn backend_of(resolved: Option<(String, Origin)>) -> Result<Option<BackendKind>, Refusal> {
    let Some((text, origin)) = resolved else {
        return Ok(None);
    };
    match BackendKind::parse(&text) {
        Some(kind) => Ok(Some(kind)),
        None => Err(refusal(origin, "backend", &text)),
    }
}

/// The config at `path`, and the lines to say once about it. A key Wisp does
/// not have is reported and skipped; a file that cannot be read at all -- not
/// valid UTF-8, or no permission to open it -- is reported and treated as empty.
/// Neither is fatal: the HUD runs on its defaults with no config file, so it
/// runs on them with a broken one.
fn read_config(path: &Path) -> (Config, Vec<String>) {
    match Config::load(path) {
        Ok(config) => {
            let warnings = config
                .unknown()
                .iter()
                .map(|name| format!("wisp-hud: ignoring unknown config key: {name}"))
                .collect();
            (config, warnings)
        }
        Err(e) => (
            Config::default(),
            vec![format!("wisp-hud: ignoring unreadable config {}: {e}", path.display())],
        ),
    }
}

/// [`read_config`] at the path this environment resolves to. No `HOME` and no
/// absolute `XDG_CONFIG_HOME` means there is no file to read, which is the
/// situation of a user who has never written one: an empty config, and nothing
/// said about it.
fn startup_config() -> (Config, Vec<String>) {
    match wisp_config::paths::config_path() {
        Ok(path) => read_config(&path),
        Err(_) => (Config::default(), Vec::new()),
    }
}

use wisp_proto::{Confidence, Encounter, MeterRow, Snapshot, Timer, MAX_DAMAGE_ROWS, MAX_HEALING_ROWS};

/// Wisp's own presentation thresholds. Not derived from anything.
const WARNING_SECS: i64 = 10;
const CRITICAL_SECS: i64 = 5;
const MAX_ROWS: usize = 8;
const TARGET_COLS: usize = 20;
const SPELL_COLS: usize = 18;
const NAME_COLS: usize = 14;
const EMPTY_PERSONAL: &str = "DPS     -  in    -/s  HPS    -   -:--";

const WHITE: [u8; 3] = [255, 255, 255];
const DIM: [u8; 3] = [170, 170, 170];
const WARNING: [u8; 3] = [255, 200, 0];
const CRITICAL: [u8; 3] = [255, 70, 70];

fn roman(rank: u8) -> &'static str {
    ["", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"]
        .get(rank as usize)
        .copied()
        .unwrap_or("")
}

/// Truncate to `cols` characters, padding on the right so columns line up
/// in the monospace face.
fn fit(s: &str, cols: usize) -> String {
    let mut out: String = s.chars().take(cols).collect();
    while out.chars().count() < cols {
        out.push(' ');
    }
    out
}

fn format_row(target: &str, spell: &str, secs: i64) -> String {
    format!("{} {} {:>4}", fit(target, TARGET_COLS), fit(spell, SPELL_COLS), secs)
}

fn row_colour(t: &Timer) -> [u8; 3] {
    let secs = t.remaining_ms.div_euclid(1000);
    if secs <= CRITICAL_SECS {
        CRITICAL
    } else if secs <= WARNING_SECS {
        WARNING
    } else if t.confidence == Confidence::Estimated {
        DIM
    } else {
        WHITE
    }
}

/// 999 -> "999", 18234 -> "18.2k", 1320500 -> "1.32M". Fits the 7-column amount slot.
fn compact(n: u64) -> String {
    if n < 10_000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        format!("{:.2}M", n as f64 / 1_000_000.0)
    }
}

fn personal_line(e: &Encounter) -> String {
    format!(
        "DPS {:>5}  in {:>4}/s  HPS {:>4}   {}:{:02}",
        e.you.dps, e.you.taken_ps, e.you.hps, e.duration_s / 60, e.duration_s % 60
    )
}

fn meter_line(r: &MeterRow, heal: bool) -> text::Line {
    let text = format!("{} {:>7} {:>5}/s{}", fit(&r.name, NAME_COLS), compact(r.amount), r.per_s, if heal { "  +" } else { "" });
    text::Line { text, rgb: WHITE }
}

/// Kill count, personal line, timer rows, damage rows, healing rows.
fn hud_lines(snap: &Snapshot) -> Vec<text::Line> {
    let mut lines = vec![text::Line { text: format!("{} kills", snap.session_kills), rgb: WHITE }];
    match &snap.encounter {
        Some(e) => lines.push(text::Line { text: personal_line(e), rgb: if e.active { WHITE } else { DIM } }),
        None => lines.push(text::Line { text: EMPTY_PERSONAL.to_string(), rgb: DIM }),
    }
    for t in snap.timers.iter().take(MAX_ROWS) {
        let spell = if t.rank == 0 { t.spell.clone() } else { format!("{} {}", t.spell, roman(t.rank)) };
        // Clamped to what the `{:>4}` column (and the startup probe's width)
        // can hold: a freshly seeded timer for one of the longer-capped
        // spells can seed above 9999 s.
        let secs = t.remaining_ms.div_euclid(1000).clamp(0, 9999);
        lines.push(text::Line { text: format_row(&t.target, &spell, secs), rgb: row_colour(t) });
    }
    if let Some(e) = &snap.encounter {
        lines.extend(e.damage.iter().take(MAX_DAMAGE_ROWS).map(|r| meter_line(r, false)));
        lines.extend(e.healing.iter().take(MAX_HEALING_ROWS).map(|r| meter_line(r, true)));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_proto::TimerKind;

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

    #[test]
    fn colour_follows_the_thresholds_and_confidence() {
        assert_eq!(row_colour(&timer(30_000, Confidence::Measured)), WHITE);
        assert_eq!(row_colour(&timer(30_000, Confidence::Estimated)), DIM);
        assert_eq!(row_colour(&timer(10_000, Confidence::Measured)), WARNING);
        assert_eq!(row_colour(&timer(5_999, Confidence::Measured)), CRITICAL);
        assert_eq!(row_colour(&timer(-2_000, Confidence::Measured)), CRITICAL);
    }

    #[test]
    fn rows_are_fixed_width_and_the_rank_is_roman() {
        let snap = Snapshot {
            v: 3, seq: 1, ts: String::new(), lines_ingested: 0, session_kills: 7,
            timers: vec![timer(11_800, Confidence::Measured)],
            encounter: None,
        };
        let lines = hud_lines(&snap);
        assert_eq!(lines[0].text, "7 kills");
        assert_eq!(lines[1].text, EMPTY_PERSONAL);
        assert_eq!(lines[2].text, format!("{} {} {:>4}", fit("a jeering gargoyle", 20), fit("Mesmerization VI", 18), 11));
        assert_eq!(fit("a very long mob name indeed", 20).chars().count(), 20);

        // A timer seeded far above the four-digit column (512 eligible
        // spells seed above 9999 s) still renders as "9999", not a wider
        // number that would break the probe's fixed width.
        let snap = Snapshot {
            v: 3, seq: 1, ts: String::new(), lines_ingested: 0, session_kills: 7,
            timers: vec![timer(100_000_000, Confidence::Measured)],
            encounter: None,
        };
        let lines = hud_lines(&snap);
        assert_eq!(lines[2].text, format!("{} {} {:>4}", fit("a jeering gargoyle", 20), fit("Mesmerization VI", 18), 9999));
    }

    #[test]
    fn at_most_eight_rows_are_drawn() {
        let snap = Snapshot {
            v: 3, seq: 1, ts: String::new(), lines_ingested: 0, session_kills: 0,
            timers: (0..12).map(|i| timer(1000 * i, Confidence::Measured)).collect(),
            encounter: None,
        };
        assert_eq!(hud_lines(&snap).len(), 2 + MAX_ROWS);
    }

    use wisp_proto::{Encounter, MeterRow, Personal};

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
        Snapshot { v: 3, seq: 1, ts: String::new(), lines_ingested: 0, session_kills: 7, timers: vec![], encounter }
    }

    #[test]
    fn amounts_print_compactly() {
        assert_eq!(compact(999), "999");
        assert_eq!(compact(9_999), "9999");
        assert_eq!(compact(18_234), "18.2k");
        assert_eq!(compact(999_949), "999.9k");
        assert_eq!(compact(1_320_500), "1.32M");
    }

    #[test]
    fn the_personal_line_and_rows_are_laid_out_around_the_timers() {
        let lines = hud_lines(&snap(Some(fight(true))));
        assert_eq!(lines[0].text, "7 kills");
        assert_eq!(lines[1].text, "DPS   434  in   52/s  HPS   21   0:42");
        assert_eq!(lines[1].rgb, WHITE);
        assert_eq!(lines[2].text, format!("{} {:>7} {:>5}/s", fit("Serenitee", NAME_COLS), "1.32M", 286));
        assert_eq!(lines[2].rgb, WHITE);
        assert_eq!(lines[3].text, format!("{} {:>7} {:>5}/s  +", fit("Misery", NAME_COLS), "3100", 74));
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn a_lingering_fight_dims_the_personal_line() {
        let lines = hud_lines(&snap(Some(fight(false))));
        assert_eq!(lines[1].rgb, DIM);
    }

    #[test]
    fn no_fight_draws_the_empty_personal_line_and_no_rows() {
        let lines = hud_lines(&snap(None));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].text, "DPS     -  in    -/s  HPS    -   -:--");
        assert_eq!(lines[1].rgb, DIM);
    }

    #[test]
    fn timer_rows_sit_between_the_personal_line_and_the_meter_rows() {
        let mut s = snap(Some(fight(true)));
        s.timers = vec![timer(11_800, Confidence::Measured)];
        let lines = hud_lines(&s);
        assert!(lines[1].text.starts_with("DPS"));
        assert!(lines[2].text.contains("Mesmerization"));
        assert!(lines[3].text.starts_with(&fit("Serenitee", NAME_COLS)));
    }

    #[test]
    fn the_maximum_layout_is_one_kill_line_one_personal_line_eight_timers_five_damage_and_three_healing() {
        let mut e = fight(true);
        e.damage = (0..7).map(|i| MeterRow { name: format!("Player{i}"), amount: 1000 - i, per_s: 10 }).collect();
        e.healing = (0..5).map(|i| MeterRow { name: format!("Healer{i}"), amount: 100 - i, per_s: 5 }).collect();
        let mut s = snap(Some(e));
        s.timers = (0..12).map(|i| timer(1000 * i, Confidence::Measured)).collect();
        assert_eq!(hud_lines(&s).len(), 1 + 1 + MAX_ROWS + MAX_DAMAGE_ROWS + MAX_HEALING_ROWS);
    }

    use std::fs;
    use std::path::PathBuf;

    /// A file of its own per test, since the harness runs them in parallel
    /// inside one process.
    fn scratch_file(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("wisp-hud-{}-{tag}", std::process::id()))
    }

    #[test]
    fn resolve_reports_where_the_value_came_from() {
        // The origin travels with the text: a bad value has to blame the place
        // the user actually wrote it.
        assert_eq!(resolve(Some("32"), Some("16")), Some(("32".to_string(), Origin::Flag)));
        assert_eq!(resolve(Some("32"), None), Some(("32".to_string(), Origin::Flag)));
        assert_eq!(resolve(None, Some("16")), Some(("16".to_string(), Origin::Config)));
        assert_eq!(resolve(None, None), None);
    }

    #[test]
    fn a_scale_flag_beats_the_config_file() {
        let config = Config::parse("scale = 16\n");
        assert_eq!(scale_of(resolve(Some("32"), config.get(Key::Scale))), Ok(32.0));
    }

    #[test]
    fn the_config_file_is_used_when_there_is_no_flag() {
        let config = Config::parse("scale = 16\n");
        assert_eq!(scale_of(resolve(None, config.get(Key::Scale))), Ok(16.0));
    }

    #[test]
    fn neither_gives_the_default_of_48() {
        let config = Config::parse("# no scale in here\n");
        assert_eq!(config.get(Key::Scale), None);
        assert_eq!(scale_of(resolve(None, config.get(Key::Scale))), Ok(48.0));
        assert_eq!(DEFAULT_SCALE, 48.0);
    }

    #[test]
    fn a_bad_scale_flag_exits_2_naming_the_flag() {
        let config = Config::parse("scale = 16\n");
        let refused = scale_of(resolve(Some("not-a-number"), config.get(Key::Scale))).unwrap_err();
        assert_eq!(refused.code, 2);
        assert_eq!(refused.message, "wisp-hud: invalid --scale value: not-a-number");
        // The flag won, so the file's usable value is not what gets blamed.
        assert!(!refused.message.contains("config"), "{}", refused.message);
    }

    #[test]
    fn a_bad_scale_in_the_config_exits_2_naming_the_key() {
        let config = Config::parse("scale = not-a-number\n");
        let refused = scale_of(resolve(None, config.get(Key::Scale))).unwrap_err();
        assert_eq!(refused.code, 2, "the same exit code a bad flag earns");
        assert_eq!(refused.message, "wisp-hud: invalid config scale: not-a-number");
    }

    #[test]
    fn a_backend_flag_beats_the_config_file() {
        let config = Config::parse("backend = plain\n");
        assert_eq!(
            backend_of(resolve(Some("gamescope"), config.get(Key::Backend))),
            Ok(Some(BackendKind::GamescopeX11))
        );
        assert_eq!(backend_of(resolve(None, config.get(Key::Backend))), Ok(Some(BackendKind::PlainWindow)));
        // Neither said, so no text wins and detection decides. `detect()` needs
        // a display, which is why it is not part of this function.
        assert_eq!(backend_of(resolve(None, None)), Ok(None));
        // Which names are accepted at all is `BackendKind::parse`'s business and
        // is tested in wisp-probe; here the only question is which source wins.
    }

    #[test]
    fn an_unknown_backend_in_the_config_is_refused_like_an_unknown_flag() {
        let config = Config::parse("backend = nonsense\n");
        let from_file = backend_of(resolve(None, config.get(Key::Backend))).unwrap_err();
        let from_flag = backend_of(resolve(Some("nonsense"), None)).unwrap_err();
        assert_eq!(from_file.code, 2);
        assert_eq!(from_flag.code, 2);
        assert_eq!(from_file.message, "wisp-hud: invalid config backend: nonsense");
        assert_eq!(from_flag.message, "wisp-hud: invalid --backend value: nonsense");
    }

    #[test]
    fn an_empty_value_is_named_rather_than_printed_as_nothing() {
        // `scale =` in the file and `--scale ""` both arrive as an empty
        // string; interpolated raw, the message would end in ": " and read as
        // a value that went missing rather than the one the user wrote.
        let config = Config::parse("scale =\n");
        assert_eq!(config.get(Key::Scale), Some(""));
        assert_eq!(
            scale_of(resolve(None, config.get(Key::Scale))).unwrap_err().message,
            "wisp-hud: invalid config scale: (empty)"
        );
        assert_eq!(
            scale_of(resolve(Some(""), None)).unwrap_err().message,
            "wisp-hud: invalid --scale value: (empty)"
        );
        let config = Config::parse("backend =\n");
        assert_eq!(config.get(Key::Backend), Some(""));
        assert_eq!(
            backend_of(resolve(None, config.get(Key::Backend))).unwrap_err().message,
            "wisp-hud: invalid config backend: (empty)"
        );
        assert_eq!(
            backend_of(resolve(Some(""), None)).unwrap_err().message,
            "wisp-hud: invalid --backend value: (empty)"
        );
    }

    #[test]
    fn a_flag_reads_the_value_after_it_and_is_absent_without_it() {
        let args = vec![OsString::from("wisp-hud"), OsString::from("--scale"), OsString::from("32")];
        assert_eq!(flag_value(&args, "scale"), Ok(Some("32".to_string())));
        assert_eq!(flag_value(&args, "backend"), Ok(None), "a flag not given is absent");
        // The value is whatever follows the flag, so the next flag's own name is
        // read as a value and refused by the reader that cannot use it.
        let args = vec![OsString::from("--scale"), OsString::from("--backend")];
        assert_eq!(flag_value(&args, "scale"), Ok(Some("--backend".to_string())));
    }

    #[test]
    fn a_scale_flag_with_nothing_after_it_is_refused_as_missing() {
        // Not `(empty)` and not an absent flag: the user asked for a scale and
        // gave no value, which is its own mistake with its own message.
        let args = vec![OsString::from("wisp-hud"), OsString::from("--scale")];
        let refused = flag_value(&args, "scale").unwrap_err();
        assert_eq!(refused.code, 2);
        assert_eq!(refused.message, "wisp-hud: invalid --scale value: (missing)");
    }

    #[test]
    fn a_backend_flag_with_nothing_after_it_is_refused_as_missing() {
        let args = vec![OsString::from("wisp-hud"), OsString::from("--backend")];
        let refused = flag_value(&args, "backend").unwrap_err();
        assert_eq!(refused.code, 2);
        assert_eq!(refused.message, "wisp-hud: invalid --backend value: (missing)");
    }

    #[test]
    fn a_non_utf8_flag_value_is_refused_with_its_lossy_text() {
        use std::os::unix::ffi::OsStringExt;
        // Undecodable bytes are still a value the user wrote, so the refusal
        // shows what survives of it -- one U+FFFD per byte that did not decode.
        let args = vec![OsString::from("--scale"), OsString::from_vec(b"\xff\xfe".to_vec())];
        let refused = flag_value(&args, "scale").unwrap_err();
        assert_eq!(refused.code, 2, "exit 2 like any other unusable value");
        assert_eq!(refused.message, "wisp-hud: invalid --scale value: \u{fffd}\u{fffd}");
        let args = vec![OsString::from("--backend"), OsString::from_vec(b"pla\xff".to_vec())];
        let refused = flag_value(&args, "backend").unwrap_err();
        assert_eq!(refused.message, "wisp-hud: invalid --backend value: pla\u{fffd}");
    }

    #[test]
    fn an_unknown_config_key_is_reported_once_and_ignored() {
        let path = scratch_file("unknown-key");
        fs::write(&path, "nonsense = 1\nscale = 32\nnonsense = 2\nalso_unknown = 3\n").unwrap();
        let (config, warnings) = read_config(&path);
        assert_eq!(
            warnings,
            [
                "wisp-hud: ignoring unknown config key: nonsense".to_string(),
                "wisp-hud: ignoring unknown config key: also_unknown".to_string(),
            ]
        );
        assert_eq!(config.get(Key::Scale), Some("32"), "the keys Wisp does have are still read");
        assert_eq!(config.get(Key::Backend), None);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn an_unreadable_config_is_reported_once_with_the_exact_text_and_treated_as_empty() {
        let path = scratch_file("unreadable-config");
        // 0xff is not a valid UTF-8 leading byte, so `Config::load` cannot
        // return text. The values in the file are unusable, not merely absent.
        fs::write(&path, b"scale = \xff\xfe\nbackend = plain\n").unwrap();
        let err = Config::load(&path).expect_err("the file is not text");
        let (config, warnings) = read_config(&path);
        assert_eq!(
            warnings,
            vec![format!("wisp-hud: ignoring unreadable config {}: {err}", path.display())],
            "the real path and the real io::Error, once"
        );
        assert_eq!(config, Config::default());
        // Both keys fall back to their defaults: the file's `backend = plain`
        // is behind the same read that failed.
        assert_eq!(scale_of(resolve(None, config.get(Key::Scale))), Ok(DEFAULT_SCALE));
        assert_eq!(backend_of(resolve(None, config.get(Key::Backend))), Ok(None));
        let _ = fs::remove_file(&path);
    }
}
