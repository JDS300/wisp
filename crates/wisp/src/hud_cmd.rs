// SPDX-License-Identifier: MIT
//! `wisp hud`: the terminal-side editor of the config file's layout.
//!
//! The HUD itself reloads the file live (a later task); this binary never
//! talks to it. Every verb goes through the same three steps `wisp config
//! set` uses -- load the text, edit the parsed model, `to_toml_checked` and
//! `write_atomic` it back -- so the HUD's own save and this command cannot
//! drift into two writers of one file. The two refusals that make the write
//! safe (a layout the file could not state, and text that does not read back
//! as what it meant to say) are `config_cmd`'s, shared for the same reason.

use std::fs;
use std::io;
use std::path::Path;

use wisp_config::config::Config;
use wisp_config::layout::{Block, BlockKind, Layout, Segment, Shows};
use wisp_config::paths::config_path;
use wisp_config::write::write_atomic;

use crate::args::HudCommand;

/// Every verb, dispatched to a report (`list`) or an edit-and-write
/// (everything else); [`HudCommand::Invalid`] is the one form that touches
/// neither the config file nor the loader, so it is answered here directly.
pub fn hud(command: HudCommand) -> i32 {
    match command {
        HudCommand::Invalid(usage) => {
            eprint!("{usage}");
            2
        }
        HudCommand::List => list(),
        edit_command => apply(edit_command),
    }
}

/// The config file's layout, one line per block, then the two `[hud]`
/// fields. A missing file is [`Layout::default_layout`], the same rule every
/// reader of the config uses.
///
/// A layout that did not parse is the one case `list` refuses rather than
/// reports: the fallback it would otherwise print is the default layout, not
/// the user's, and printing someone else's two blocks as if they were theirs
/// -- with nothing on stderr -- is worse than saying what is wrong with the
/// file. Exit 2, the same code every editing verb gives the same file.
fn list() -> i32 {
    let config = load_for_reading();
    if let Some(code) = crate::config_cmd::refuse_unreadable_layout(&config) {
        return code;
    }
    let layout = config.layout();
    for (index, block) in layout.blocks.iter().enumerate() {
        println!("{}", block_line(index, block));
    }
    println!("scale {}", layout.hud.scale);
    println!("chord {}", layout.hud.chord);
    0
}

/// One block's line: `{n}  {kind:<6} {shows:<7} {segment:<7} {anchor:<12}
/// {x:>5} {y:>5}  w{width} rows{rows}{ hidden}`. A timers block has no
/// `shows` or `segment` of its own, so both print as `-`.
fn block_line(index: usize, block: &Block) -> String {
    let kind = kind_word(block.kind);
    let (shows, segment) = match block.kind {
        BlockKind::Meter => (shows_word(block.shows), segment_word(block.segment)),
        BlockKind::Timers => ("-", "-"),
    };
    let anchor = anchor_word(block.anchor);
    let x = block.offset[0];
    let y = block.offset[1];
    let width = block.width();
    let rows = block.rows();
    let hidden = if block.hidden { " hidden" } else { "" };
    format!(
        "{index}  {kind:<6} {shows:<7} {segment:<7} {anchor:<12} {x:>5} {y:>5}  w{width} rows{rows}{hidden}"
    )
}

fn kind_word(kind: BlockKind) -> &'static str {
    match kind {
        BlockKind::Meter => "meter",
        BlockKind::Timers => "timers",
    }
}

fn shows_word(shows: Shows) -> &'static str {
    match shows {
        Shows::Damage => "damage",
        Shows::Healing => "healing",
    }
}

fn segment_word(segment: Segment) -> &'static str {
    match segment {
        Segment::Fight => "fight",
        Segment::Session => "session",
    }
}

/// The nine words [`wisp_config::layout::Anchor`]'s own
/// `serde(rename_all = "kebab-case")` writes, the printing half of
/// `args::parse_anchor`.
fn anchor_word(anchor: wisp_config::layout::Anchor) -> &'static str {
    use wisp_config::layout::Anchor;
    match anchor {
        Anchor::TopLeft => "top-left",
        Anchor::Top => "top",
        Anchor::TopRight => "top-right",
        Anchor::Left => "left",
        Anchor::Center => "center",
        Anchor::Right => "right",
        Anchor::BottomLeft => "bottom-left",
        Anchor::Bottom => "bottom",
        Anchor::BottomRight => "bottom-right",
    }
}

/// The config file for `list`: a missing file is the default layout, and an
/// unreadable one is reported once and treated the same way -- `list` never
/// writes, so there is nothing here for an unreadable file to put at risk.
fn load_for_reading() -> Config {
    let Ok(path) = config_path() else { return Config::default() };
    match Config::load(&path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("wisp: ignoring unreadable config {}: {e}", path.display());
            Config::default()
        }
    }
}

/// Every editing verb: resolve the path, read the existing text (a missing
/// file is empty, exactly as `wisp config set` treats it), edit the parsed
/// model, then `to_toml` and `write_atomic` it back.
fn apply(command: HudCommand) -> i32 {
    let path = match config_path() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("wisp: cannot resolve the config path: {e}");
            return 1;
        }
    };
    let existing = match read_existing(&path) {
        Ok(text) => text,
        Err(code) => return code,
    };
    let mut config = Config::parse(&existing);
    // Before the edit, not after: a layout that did not parse has already
    // fallen back to the default, so applying the verb would edit the default
    // and write that over the user's blocks.
    if let Some(code) = crate::config_cmd::refuse_unreadable_layout(&config) {
        return code;
    }
    if let Err(code) = edit(config.layout_mut(), command) {
        return code;
    }
    let text = match crate::config_cmd::checked_toml(&config) {
        Ok(text) => text,
        Err(code) => return code,
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!("wisp: cannot create {}: {e}", parent.display());
            return 1;
        }
    }
    match write_atomic(&path, &text) {
        Ok(()) => {
            println!("{}", path.display());
            0
        }
        Err(e) => {
            eprintln!("wisp: cannot write {}: {e}", path.display());
            1
        }
    }
}

/// The file's text, or empty for one that does not exist yet: `wisp hud` is
/// as able to create the file as `wisp config set` is. A file that exists but
/// cannot be read is not overwritten -- replacing bytes this process could
/// not read with a partial edit would lose every other setting in it.
fn read_existing(path: &Path) -> Result<String, i32> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => {
            eprintln!("wisp: ignoring unreadable config {}: {e}", path.display());
            Err(1)
        }
    }
}

/// Every verb but `list` and `Invalid`, applied to the loaded layout. `Err`
/// carries the exit code, having already printed what it refuses.
fn edit(layout: &mut Layout, command: HudCommand) -> Result<(), i32> {
    match command {
        HudCommand::Place { index, anchor, x, y } => {
            let block = block_mut(layout, index)?;
            block.anchor = anchor;
            block.offset = [x, y];
            Ok(())
        }
        HudCommand::Nudge { index, dx, dy } => {
            let block = block_mut(layout, index)?;
            block.offset[0] += dx;
            block.offset[1] += dy;
            Ok(())
        }
        HudCommand::Set { index, key, value } => {
            let block = block_mut(layout, index)?;
            set_key(block, &key, &value)
        }
        HudCommand::Add(kind) => {
            layout.blocks.push(Block::new(kind));
            Ok(())
        }
        HudCommand::Remove { index } => {
            if index >= layout.blocks.len() {
                return Err(no_block(index, layout.blocks.len()));
            }
            layout.blocks.remove(index);
            Ok(())
        }
        HudCommand::Scale(factor) => {
            layout.hud.scale = factor;
            Ok(())
        }
        // `hud()` answers both before this function is ever called.
        HudCommand::List | HudCommand::Invalid(_) => unreachable!("handled in hud()"),
    }
}

/// The block at `index`, or the refusal the spec names: `wisp hud: no block
/// {n} (have {len})`, exit 2.
fn block_mut(layout: &mut Layout, index: usize) -> Result<&mut Block, i32> {
    let len = layout.blocks.len();
    layout.blocks.get_mut(index).ok_or_else(|| no_block(index, len))
}

fn no_block(index: usize, len: usize) -> i32 {
    eprintln!("wisp hud: no block {index} (have {len})");
    2
}

/// `set <n> <key> <value>`'s key and value, validated here: only this
/// function knows the five keys and what each accepts, so only it can print
/// the usage's own `block keys:` line -- byte for byte the same line
/// `main::USAGE` ends with, so the two cannot name a different vocabulary.
fn set_key(block: &mut Block, key: &str, value: &str) -> Result<(), i32> {
    match key {
        "shows" => match value {
            "damage" => {
                block.shows = Shows::Damage;
                Ok(())
            }
            "healing" => {
                block.shows = Shows::Healing;
                Ok(())
            }
            _ => Err(bad_key()),
        },
        "segment" => match value {
            "fight" => {
                block.segment = Segment::Fight;
                Ok(())
            }
            "session" => {
                block.segment = Segment::Session;
                Ok(())
            }
            _ => Err(bad_key()),
        },
        "width" => match value.parse::<u32>() {
            Ok(width) => {
                block.width = Some(width);
                Ok(())
            }
            Err(_) => Err(bad_key()),
        },
        "rows" => match value.parse::<u32>() {
            Ok(rows) => {
                block.rows = Some(rows);
                Ok(())
            }
            Err(_) => Err(bad_key()),
        },
        "hidden" => match value {
            "true" => {
                block.hidden = true;
                Ok(())
            }
            "false" => {
                block.hidden = false;
                Ok(())
            }
            _ => Err(bad_key()),
        },
        _ => Err(bad_key()),
    }
}

fn bad_key() -> i32 {
    eprintln!("block keys: shows (damage|healing), segment (fight|session), width, rows, hidden (true|false)");
    2
}
