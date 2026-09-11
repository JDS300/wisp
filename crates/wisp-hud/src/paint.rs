// SPDX-License-Identifier: MIT
//! Block views to canvas calls, and the HUD-mode overlay. Pure like
//! `model.rs`: no window, no display, no input -- `views` and `previous`
//! come in, `Canvas` calls go out.

use crate::backend::Rect;
use crate::draw::{measure, Canvas, Face, Fonts, TextStyle};
use crate::model::{BlockView, Row, RowState};
use crate::theme::Theme;

/// HUD mode's own state: which block is selected, and the help line to draw.
pub struct HudModeView<'a> {
    pub selected: usize,
    pub help: &'a str,
}

pub const HELP: &str = "HUD mode   ↑↓←→ move   Tab next   [ ] shows   F fight/session   + - rows   H hide   Esc save & exit";

/// Clears each block's previous rect (passed in `previous`), then draws
/// every non-hidden block; in HUD mode also outlines, tags, ghosts and the
/// help strip.
///
/// Returns the rects the caller must pass back as `previous` next frame: not
/// the block rects, but everything this frame actually touched. HUD mode
/// draws well outside every block -- the outline 3 px out, the selected
/// block's halo 4 px out, the name tag entirely above, the help strip along
/// the bottom of the screen -- and none of that was in the erase set, so
/// nudging a block left the old ring and tag behind as smear, leaving HUD
/// mode left all of it on screen, and redrawing the 55 %-alpha outline over
/// pixels that were never cleared accumulated alpha until it was opaque.
#[must_use = "the returned rects are next frame's erase set; dropping them is the smear bug"]
pub fn paint(canvas: &mut Canvas, fonts: &Fonts, theme: &Theme, views: &[BlockView], previous: &[Rect], hud_mode: Option<HudModeView>) -> Vec<Rect> {
    for r in previous {
        canvas.clear(*r);
    }

    for view in views {
        if view.hidden {
            if hud_mode.is_some() {
                draw_ghost(canvas, fonts, theme, view);
            }
            continue;
        }
        draw_panel(canvas, fonts, theme, view);
    }

    let mut erase: Vec<Rect> = Vec::with_capacity(views.len() + 1);
    match &hud_mode {
        Some(hud) => {
            for view in views {
                // A hidden block's ghost is drawn inside its own rect and
                // carries no chrome, so the rect is the whole of it.
                if view.hidden {
                    erase.push(view.rect);
                    continue;
                }
                let chrome = draw_hud_outline(canvas, fonts, theme, view, view.index == hud.selected);
                erase.push(view.rect.union(chrome));
            }
            erase.push(draw_help(canvas, fonts, theme, hud.help));
        }
        // Outside HUD mode a hidden block draws nothing, so there is nothing
        // of its own to clear next frame; whatever it left behind is cleared
        // by *this* frame, out of the rects the last one returned.
        None => erase.extend(views.iter().filter(|view| !view.hidden).map(|view| view.rect)),
    }
    erase
}

fn style(face: Face, size: f32, colour: crate::draw::Rgba) -> TextStyle {
    TextStyle { face, size, colour, tabular: false }
}

/// The baseline that centres a `size`-px line of `face` vertically inside a
/// `rect_h`-tall row starting at `rect_y`, using the font's real metrics.
fn centered_baseline(fonts: &Fonts, face: Face, size: f32, rect_y: i32, rect_h: u32) -> i32 {
    let (ascent, descent) = crate::draw::line_metrics(fonts, face, size);
    let line_h = ascent + descent;
    let top = rect_y + (rect_h.saturating_sub(line_h) / 2) as i32;
    top + ascent as i32
}

fn header_height(theme: &Theme) -> u32 {
    theme.header_px.ceil() as u32 + 2 * theme.header_pad_y
}

fn draw_panel(canvas: &mut Canvas, fonts: &Fonts, theme: &Theme, view: &BlockView) {
    let rect = view.rect;
    canvas.fill_rect(rect, theme.panel, theme.radius);
    canvas.stroke_rect(rect, theme.panel_border, theme.border, None);

    let header_h = header_height(theme).min(rect.h);
    let header_style = style(Face::Sans, theme.header_px, theme.header_text);
    canvas.fill_rect(Rect::new(rect.x, rect.y, rect.w, header_h), theme.header, 0);
    let header_baseline = centered_baseline(fonts, Face::Sans, theme.header_px, rect.y, header_h);
    canvas.text(fonts, rect.x + theme.pad_x as i32, header_baseline, &view.title, header_style);
    let right_w = measure(fonts, &view.right, header_style);
    canvas.text(fonts, rect.x + rect.w as i32 - theme.pad_x as i32 - right_w as i32, header_baseline, &view.right, header_style);

    let mut y = rect.y + header_h as i32 + theme.row_inset as i32;
    for group in &view.groups {
        if let Some(label) = &group.label {
            y += theme.group_gap as i32;
            let label_style = style(Face::Sans, theme.target_px, theme.target_text);
            let (ascent, descent) = crate::draw::line_metrics(fonts, Face::Sans, theme.target_px);
            canvas.text(fonts, rect.x + theme.pad_x as i32, y + ascent as i32, label, label_style);
            y += (ascent + descent) as i32;
        }
        for row in &group.rows {
            let row_rect = Rect::new(rect.x, y, rect.w, theme.row_h);
            draw_row(canvas, fonts, theme, row_rect, row);
            y += (theme.row_h + theme.row_gap) as i32;
        }
    }
}

fn draw_row(canvas: &mut Canvas, fonts: &Fonts, theme: &Theme, rect: Rect, row: &Row) {
    if row.state != RowState::Gone {
        let bar_alpha = if row.you {
            theme.you_bar_alpha
        } else if row.state == RowState::Estimated {
            theme.estimated_bar_alpha
        } else {
            theme.bar_alpha
        };
        let bar_w = (rect.w as f32 * row.fill.clamp(0.0, 1.0)).round() as u32;
        canvas.fill_rect(Rect::new(rect.x, rect.y, bar_w, rect.h), row.bar.with_alpha(bar_alpha), theme.bar_radius);
    }
    draw_name_and_tag(canvas, fonts, theme, rect, row);
    draw_numbers(canvas, fonts, theme, rect, row);
}

/// The row's numbers as they'll be drawn: a timer's lone number, or a
/// meter's `total (rate, share)`. Used both to draw them and to budget how
/// much width is left for the name before it (§4.2: "name ellipsised when
/// it does not fit").
fn numbers_text(row: &Row) -> String {
    if row.numbers.len() >= 3 {
        format!("{} ({}, {})", row.numbers[0].text, row.numbers[1].text, row.numbers[2].text)
    } else if let Some(n) = row.numbers.first() {
        n.text.clone()
    } else {
        String::new()
    }
}

fn draw_name_and_tag(canvas: &mut Canvas, fonts: &Fonts, theme: &Theme, rect: Rect, row: &Row) {
    let gone = row.state == RowState::Gone;
    let estimated = row.state == RowState::Estimated;
    let text_alpha = if gone {
        theme.gone_text_alpha
    } else if estimated {
        theme.estimated_text_alpha
    } else {
        1.0
    };
    let name_face = if row.you { Face::SansBold } else { Face::Sans };
    let name_colour = theme.text.with_alpha(text_alpha);
    let name_style = style(name_face, theme.text_px, name_colour);
    let baseline = centered_baseline(fonts, name_face, theme.text_px, rect.y, rect.h);
    let x = rect.x + theme.pad_x as i32;

    // The name gets whatever's left of the row after its own left padding,
    // the tag (if any) and its gap, the numbers and their right padding.
    let numbers_w = measure(fonts, &numbers_text(row), style(Face::Sans, theme.number_px, name_colour));
    let tag_w = row
        .tag
        .as_ref()
        .map(|t| measure(fonts, t, style(Face::Sans, theme.kind_px, name_colour)) + theme.pad_x / 2)
        .unwrap_or(0);
    let reserved = theme.pad_x + numbers_w + theme.pad_x + tag_w;
    let budget = (rect.w).saturating_sub(reserved);
    let name_text = crate::draw::ellipsize(fonts, &row.name, name_style, budget);

    let name_w = canvas.text(fonts, x, baseline, &name_text, name_style);

    if gone {
        let mid_y = rect.y + rect.h as i32 / 2;
        canvas.fill_rect(Rect::new(x, mid_y, name_w, theme.border), name_colour, 0);
    }

    if let Some(tag) = &row.tag {
        let tag_style = style(Face::Sans, theme.kind_px, theme.kind_text.with_alpha(text_alpha));
        let tag_baseline = centered_baseline(fonts, Face::Sans, theme.kind_px, rect.y, rect.h);
        canvas.text(fonts, x + name_w as i32 + theme.pad_x as i32 / 2, tag_baseline, tag, tag_style);
    }
}

fn draw_numbers(canvas: &mut Canvas, fonts: &Fonts, theme: &Theme, rect: Rect, row: &Row) {
    let alpha = if row.state == RowState::Estimated { theme.estimated_text_alpha } else { 1.0 };
    let colour = row.number_colour.unwrap_or(theme.text).with_alpha(alpha);
    let baseline = centered_baseline(fonts, Face::Sans, theme.number_px, rect.y, rect.h);
    let right_edge = rect.x + rect.w as i32 - theme.pad_x as i32;

    if row.numbers.len() == 1 {
        let n = &row.numbers[0];
        let s = style(if n.bold { Face::SansBold } else { Face::Sans }, theme.number_px, colour);
        let w = measure(fonts, &n.text, s);
        canvas.text(fonts, right_edge - w as i32, baseline, &n.text, s);
        return;
    }

    // Meter row: `41.2k (981, 55%)` -- total, a space, `(`, the bold rate,
    // `, `, the share, `)`.
    let plain = style(Face::Sans, theme.number_px, colour);
    let bold = style(Face::SansBold, theme.number_px, colour);
    let pieces: [(&str, TextStyle); 6] = [
        (row.numbers[0].text.as_str(), plain),
        (" (", plain),
        (row.numbers[1].text.as_str(), bold),
        (", ", plain),
        (row.numbers[2].text.as_str(), plain),
        (")", plain),
    ];
    let total_w: u32 = pieces.iter().map(|&(t, s)| measure(fonts, t, s)).sum();
    let mut x = right_edge - total_w as i32;
    for (t, s) in pieces {
        x += canvas.text(fonts, x, baseline, t, s) as i32;
    }
}

fn draw_ghost(canvas: &mut Canvas, fonts: &Fonts, theme: &Theme, view: &BlockView) {
    let rect = view.rect;
    canvas.stroke_rect(rect, theme.ghost, theme.border, Some(theme.dash));
    let label = format!("{} (hidden)", view.tag);
    let s = style(Face::Sans, theme.text_px, theme.ghost_text);
    let w = measure(fonts, &label, s);
    let baseline = centered_baseline(fonts, Face::Sans, theme.text_px, rect.y, rect.h);
    let x = rect.x + (rect.w as i32 - w as i32) / 2;
    canvas.text(fonts, x, baseline, &label, s);
}

/// Draws one block's HUD-mode chrome and returns the region it covers.
///
/// The returned rect is the same whether or not this block is selected: Tab
/// moves the selection between frames, and the next frame clears using what
/// *this* one returned, so a rect that shrank when a block was deselected
/// would leave the halo of the frame before it on screen.
fn draw_hud_outline(canvas: &mut Canvas, fonts: &Fonts, theme: &Theme, view: &BlockView, selected: bool) -> Rect {
    let rect = view.rect;
    let outer = rect.inset(-(theme.outline_offset as i32));
    if selected {
        canvas.stroke_rect(outer, theme.outline_selected, theme.outline_selected_thickness, None);
        let halo_rect = rect.inset(-theme.nudge);
        canvas.stroke_rect(halo_rect, theme.halo, theme.outline_selected_thickness, None);
    } else {
        canvas.stroke_rect(outer, theme.outline, theme.border, Some(theme.dash));
    }

    let tag = view.tag.to_uppercase();
    let s = style(Face::Sans, theme.kind_px, theme.text);
    let (ascent, descent) = crate::draw::line_metrics(fonts, Face::Sans, theme.kind_px);
    let tag_h = ascent + descent + 2 * theme.header_pad_y;
    let tag_w = measure(fonts, &tag, s) + 2 * theme.pad_x;
    let tag_rect = Rect::new(outer.x, outer.y - tag_h as i32, tag_w, tag_h);
    canvas.fill_rect(tag_rect, theme.tag_bg, 0);
    canvas.text(fonts, tag_rect.x + theme.pad_x as i32, tag_rect.y + theme.header_pad_y as i32 + ascent as i32, &tag, s);

    // The outline sits `outline_offset` outside the block and the halo
    // `nudge` outside it, so the wider of the two bounds the ring; the tag
    // hangs above that.
    let ring = rect.inset(-(theme.outline_offset.max(theme.nudge.max(0) as u32) as i32));
    ring.union(tag_rect)
}

/// Draws the bottom-of-screen help strip and returns its rect, which is
/// nowhere near any block and so has to be erased on its own account.
fn draw_help(canvas: &mut Canvas, fonts: &Fonts, theme: &Theme, help: &str) -> Rect {
    let (w, h) = canvas.size();
    let s = style(Face::Sans, theme.text_px, theme.text);
    let text_w = measure(fonts, help, s);
    let strip_w = text_w + 2 * theme.pad_x;
    let strip_h = header_height(theme);
    let rect = Rect::new((w as i32 - strip_w as i32) / 2, h as i32 - strip_h as i32, strip_w, strip_h);
    canvas.fill_rect(rect, theme.panel, theme.radius);
    canvas.stroke_rect(rect, theme.outline, theme.border, None);
    let baseline = centered_baseline(fonts, Face::Sans, theme.text_px, rect.y, rect.h);
    canvas.text(fonts, rect.x + theme.pad_x as i32, baseline, help, s);
    rect
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::Rgba;
    use crate::model::{Group, Number};
    use wisp_config::layout::BlockKind;

    fn damage_view(rect: Rect) -> BlockView {
        BlockView {
            index: 0,
            kind: BlockKind::Meter,
            hidden: false,
            rect,
            title: "Damage · fight".to_string(),
            right: "0:42".to_string(),
            tag: "meter · damage · fight".to_string(),
            groups: vec![Group {
                label: None,
                rows: vec![Row {
                    name: "Serenitee".to_string(),
                    tag: None,
                    numbers: vec![
                        Number { text: "41.2k".to_string(), bold: false },
                        Number { text: "981".to_string(), bold: true },
                        Number { text: "55%".to_string(), bold: false },
                    ],
                    number_colour: None,
                    fill: 1.0,
                    bar: Rgba::rgb(0x6b7a8c),
                    state: RowState::Normal,
                    you: false,
                }],
            }],
        }
    }

    #[test]
    fn paints_the_panel_and_the_top_rows_bar() {
        let theme = Theme::at(1.0);
        let fonts = Fonts::embedded();
        let rect = Rect::new(20, 120, 290, 80);
        let view = damage_view(rect);
        let mut canvas = Canvas::new(400, 400);

        let _ = paint(&mut canvas, &fonts, &theme, std::slice::from_ref(&view), &[], None);

        // Inside the panel, below the header and past the row: plain panel
        // colour blended over transparent. The canvas's 8-bit premultiplied
        // round trip (draw.rs, frozen) loses at most 1 per channel even for
        // a single unblended fill, so each channel gets the same tolerance
        // `draw.rs`'s own tests use.
        let below_row = rect.y + rect.h as i32 - 2;
        let p = canvas.pixel((rect.x + 2) as u32, below_row as u32);
        assert!((7..=9).contains(&p[0]) && (9..=11).contains(&p[1]) && (13..=15).contains(&p[2]), "{p:?}");
        assert!((188..=190).contains(&p[3]), "{p:?}");

        // Inside the top row's (fully filled) bar: not the plain panel colour.
        let header_h = header_height(&theme);
        let row_y = rect.y + header_h as i32 + theme.row_inset as i32 + theme.row_h as i32 / 2;
        let bar_p = canvas.pixel((rect.x + 4) as u32, row_y as u32);
        assert_ne!(bar_p, p);
    }

    /// Every pixel on `canvas` with any alpha at all.
    fn inked(canvas: &Canvas, w: u32, h: u32) -> Vec<(i32, i32)> {
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|&(x, y)| canvas.pixel(x, y)[3] > 0)
            .map(|(x, y)| (x as i32, y as i32))
            .collect()
    }

    #[test]
    fn hud_mode_chrome_is_in_the_erase_set_and_leaves_nothing_behind() {
        // HUD mode draws well outside every block -- the dashed outline 3 px
        // out, the selected block's halo 4 px out, the name tag entirely
        // above it, the help strip along the bottom of the screen -- and none
        // of that used to be handed back for the next frame's clear. Nudging
        // left the old ring and tag as smear, exiting HUD mode left all of it
        // on screen, and redrawing a 55 %-alpha stroke onto pixels that were
        // never cleared blended towards opaque within a couple of seconds.
        let theme = Theme::at(1.0);
        let fonts = Fonts::embedded();
        let (w, h) = (400u32, 400u32);
        let rect = Rect::new(60, 120, 200, 80);
        let view = damage_view(rect);
        let mut canvas = Canvas::new(w, h);

        let erase = paint(
            &mut canvas,
            &fonts,
            &theme,
            std::slice::from_ref(&view),
            &[],
            Some(HudModeView { selected: 0, help: HELP }),
        );

        // Not a vacuous pass: the three pieces of chrome outside the block
        // are on the canvas, and each is inside some erase rect.
        let ring_x = rect.x - theme.nudge;
        let tag_y = rect.y - theme.outline_offset as i32 - 1;
        let strip_y = h as i32 - 1;
        for (x, y, what) in [
            (ring_x, rect.y + rect.h as i32 / 2, "the halo, 4 px outside the block"),
            (rect.x - theme.outline_offset as i32 + 1, tag_y, "the name tag, above the block"),
            (w as i32 / 2, strip_y, "the help strip, along the bottom of the screen"),
        ] {
            assert!(canvas.pixel(x as u32, y as u32)[3] > 0, "{what} was not drawn at ({x}, {y})");
            assert!(!rect.contains(x, y), "{what} is meant to be outside the block");
            assert!(erase.iter().any(|r| r.contains(x, y)), "{what} at ({x}, {y}) is not in {erase:?}");
        }

        // And nothing at all was drawn outside the erase set.
        for (x, y) in inked(&canvas, w, h) {
            assert!(erase.iter().any(|r| r.contains(x, y)), "({x}, {y}) was painted but is never cleared");
        }

        // Next frame, HUD mode off: the chrome's own pixels come back
        // transparent, and only the block itself is left.
        let erase = paint(&mut canvas, &fonts, &theme, std::slice::from_ref(&view), &erase, None);
        for (x, y, what) in [
            (ring_x, rect.y + rect.h as i32 / 2, "the halo"),
            (rect.x - theme.outline_offset as i32 + 1, tag_y, "the name tag"),
            (w as i32 / 2, strip_y, "the help strip"),
        ] {
            assert_eq!(canvas.pixel(x as u32, y as u32), [0, 0, 0, 0], "{what} is still on screen at ({x}, {y})");
        }
        for (x, y) in inked(&canvas, w, h) {
            assert!(rect.contains(x, y), "({x}, {y}) is outside the block and still painted");
        }
        assert_eq!(erase, vec![rect], "outside HUD mode the block's own rect is the whole erase set");
    }

    #[test]
    fn a_hidden_block_paints_only_in_hud_mode() {
        let theme = Theme::at(1.0);
        let fonts = Fonts::embedded();
        let rect = Rect::new(50, 50, 200, 80);
        let mut hidden = damage_view(rect);
        hidden.hidden = true;
        hidden.groups.clear();

        let painted_inside = |canvas: &Canvas| -> bool {
            (rect.x..rect.x + rect.w as i32)
                .any(|x| (rect.y..rect.y + rect.h as i32).any(|y| canvas.pixel(x as u32, y as u32)[3] > 0))
        };

        let mut canvas = Canvas::new(400, 400);
        let _ = paint(&mut canvas, &fonts, &theme, std::slice::from_ref(&hidden), &[], None);
        assert!(!painted_inside(&canvas), "a hidden block draws nothing outside HUD mode");

        let mut canvas = Canvas::new(400, 400);
        let _ = paint(&mut canvas, &fonts, &theme, std::slice::from_ref(&hidden), &[], Some(HudModeView { selected: 99, help: HELP }));
        assert!(painted_inside(&canvas), "a hidden block draws its ghost in HUD mode");
    }
}
