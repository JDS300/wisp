// SPDX-License-Identifier: MIT
//! The drawing layer: a software canvas that composites rects and text into
//! a premultiplied-alpha `Frame`, with dirty-rect tracking so a caller can
//! present only what changed. Pure, like `text.rs`: no display, no I/O
//! beyond the embedded fonts.
//!
//! Colours and sizes are always parameters here, never literals -- the
//! theme a later task writes is the only place either is spelled out.

use crate::backend::{Frame, Rect};

/// Straight-alpha colour: `[r, g, b, a]`, each 0-255, `rgb` *not*
/// premultiplied by `a`. `Rgba::rgb(0xe8edf2)` is opaque;
/// `Rgba::rgba(0x080a0e, 0.74)` takes an alpha fraction. Premultiplication
/// happens at the blend, never in the theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    pub const fn rgb(hex: u32) -> Rgba {
        Rgba([((hex >> 16) & 0xff) as u8, ((hex >> 8) & 0xff) as u8, (hex & 0xff) as u8, 255])
    }

    // The brief's interface names this constructor to match the type, the
    // usual convention everywhere else in Rust; clippy's heuristic for
    // "looks like a mis-capitalised `new`" doesn't apply here.
    #[allow(clippy::self_named_constructors)]
    pub fn rgba(hex: u32, alpha: f32) -> Rgba {
        let Rgba([r, g, b, _]) = Rgba::rgb(hex);
        Rgba([r, g, b, alpha_byte(alpha)])
    }

    /// Scales the existing alpha by `alpha` (a fraction); the colour itself
    /// is unchanged.
    pub fn with_alpha(self, alpha: f32) -> Rgba {
        let Rgba([r, g, b, a]) = self;
        Rgba([r, g, b, ((a as f32 * alpha).round().clamp(0.0, 255.0)) as u8])
    }
}

fn alpha_byte(alpha: f32) -> u8 {
    (alpha.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// One of the three embedded DejaVu faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    Sans,
    SansBold,
    // Not drawn anywhere by `paint.rs` today -- every row in Spec 5's
    // console is proportional or tabular-digit, never fixed-width prose --
    // but the embedded face is still loaded and measured by this crate's own
    // tests, so it stays part of `Fonts`'s public surface.
    #[allow(dead_code)]
    Mono,
}

const SANS_BYTES: &[u8] = include_bytes!("../../../assets/DejaVuSans.ttf");
const SANS_BOLD_BYTES: &[u8] = include_bytes!("../../../assets/DejaVuSans-Bold.ttf");
const MONO_BYTES: &[u8] = include_bytes!("../../../assets/DejaVuSansMono.ttf");

/// The three embedded faces, parsed once. Parsing walks the font's tables
/// and builds its glyph index, which is real work -- build one `Fonts` at
/// startup and pass `&Fonts` around; don't construct a fresh one per frame
/// or per draw call.
pub struct Fonts {
    sans: fontdue::Font,
    sans_bold: fontdue::Font,
    mono: fontdue::Font,
}

impl Fonts {
    pub fn embedded() -> Fonts {
        let parse = |bytes: &[u8]| {
            fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                .expect("vendored font must parse")
        };
        Fonts { sans: parse(SANS_BYTES), sans_bold: parse(SANS_BOLD_BYTES), mono: parse(MONO_BYTES) }
    }

    pub fn font(&self, face: Face) -> &fontdue::Font {
        match face {
            Face::Sans => &self.sans,
            Face::SansBold => &self.sans_bold,
            Face::Mono => &self.mono,
        }
    }
}

#[derive(Clone, Copy)]
pub struct TextStyle {
    pub face: Face,
    /// px, already scaled by the caller.
    pub size: f32,
    pub colour: Rgba,
    /// Every ASCII digit advances by the widest digit's advance, so columns
    /// of numbers align regardless of which digits they hold.
    pub tabular: bool,
}

/// The per-glyph advance to use for `c`: the tabular slot width for an ASCII
/// digit under a tabular style, its own rounded advance otherwise.
fn char_advance(font: &fontdue::Font, style: TextStyle, tab_width: u32, c: char) -> u32 {
    if style.tabular && c.is_ascii_digit() {
        tab_width
    } else {
        font.metrics(c, style.size).advance_width.round() as u32
    }
}

/// The advance every digit is padded to under a tabular style: the widest
/// of the ten digits' own advances.
fn tabular_width(fonts: &Fonts, face: Face, size: f32) -> u32 {
    let font = fonts.font(face);
    ('0'..='9').map(|d| font.metrics(d, size).advance_width.round() as u32).max().unwrap_or(0)
}

/// Advance of `text` in `style`, without drawing.
pub fn measure(fonts: &Fonts, text: &str, style: TextStyle) -> u32 {
    let font = fonts.font(style.face);
    let tab_width = if style.tabular { tabular_width(fonts, style.face, style.size) } else { 0 };
    text.chars().map(|c| char_advance(font, style, tab_width, c)).sum()
}

/// The longest prefix of `text` that fits in `max` px, with `…` appended
/// when truncated. Returns `text` itself, unchanged, when it already fits.
pub fn ellipsize(fonts: &Fonts, text: &str, style: TextStyle, max: u32) -> String {
    if measure(fonts, text, style) <= max {
        return text.to_string();
    }
    let font = fonts.font(style.face);
    let tab_width = if style.tabular { tabular_width(fonts, style.face, style.size) } else { 0 };
    let ellipsis_w = char_advance(font, style, tab_width, '…');
    let budget = max.saturating_sub(ellipsis_w);
    let mut out = String::new();
    let mut w = 0u32;
    for c in text.chars() {
        let cw = char_advance(font, style, tab_width, c);
        if w + cw > budget {
            break;
        }
        w += cw;
        out.push(c);
    }
    out.push('…');
    out
}

/// Line metrics for laying rows out: (ascent, descent) in px at `size`,
/// ceil'd. Falls back to the ink extent of `"Wg"` for a font that reports no
/// vertical metrics, the same fallback `text::Renderer::line_height` uses.
pub fn line_metrics(fonts: &Fonts, face: Face, size: f32) -> (u32, u32) {
    let font = fonts.font(face);
    match font.horizontal_line_metrics(size) {
        Some(m) => (m.ascent.ceil().max(0.0) as u32, (-m.descent).ceil().max(0.0) as u32),
        None => {
            let metrics: Vec<_> = ['W', 'g'].iter().map(|&c| font.metrics(c, size)).collect();
            let ascent = metrics.iter().map(|m| (m.ymin + m.height as i32).max(0)).max().unwrap_or(0);
            let descent = metrics.iter().map(|m| (-m.ymin).max(0)).max().unwrap_or(0);
            (ascent as u32, descent as u32)
        }
    }
}

/// A pixel at local coordinates `(local_x, local_y)` inside a `w`x`h` rect is
/// painted when its centre lies within the rect's rounded outline: outside
/// the four `radius`x`radius` corner boxes it's simply inside, inside one
/// it's inside only if its centre is within `radius` of that corner's
/// circle centre. No anti-aliasing on the curve.
fn in_rounded_rect(local_x: i32, local_y: i32, w: u32, h: u32, radius: i32) -> bool {
    let radius = radius.clamp(0, (w.min(h) / 2) as i32);
    if radius == 0 {
        return true;
    }
    let (w, h) = (w as i32, h as i32);
    let in_left = local_x < radius;
    let in_right = local_x >= w - radius;
    let in_top = local_y < radius;
    let in_bottom = local_y >= h - radius;
    if !(in_top || in_bottom) || !(in_left || in_right) {
        return true;
    }
    let cx = if in_left { radius } else { w - radius };
    let cy = if in_top { radius } else { h - radius };
    let dx = local_x as f32 + 0.5 - cx as f32;
    let dy = local_y as f32 + 0.5 - cy as f32;
    dx * dx + dy * dy <= (radius as f32) * (radius as f32)
}

/// A rough clockwise position along a `w`x`h` rect's `t`-px-thick ring,
/// starting at the top-left corner, for dashing. Not continuous across
/// corners -- only its parity under a dash length is used, and the theme
/// keeps dashes to thin, unobtrusive lines where that seam is invisible.
fn perimeter_position(local_x: i32, local_y: i32, w: i32, h: i32, t: i32) -> i32 {
    if local_y < t {
        local_x
    } else if local_x >= w - t {
        w + local_y
    } else if local_y >= h - t {
        w + h + (w - local_x)
    } else {
        2 * w + h + (h - local_y)
    }
}

/// The premultiplied, fully-opaque-coverage source pixel for `colour`.
fn premultiply(colour: Rgba) -> [u8; 4] {
    let Rgba([r, g, b, a]) = colour;
    let mul = |c: u8| (((c as u32) * (a as u32) + 127) / 255) as u8;
    [mul(r), mul(g), mul(b), a]
}

/// The premultiplied source pixel for `colour` at a glyph's `coverage`
/// (0-255): alpha is `colour`'s alpha scaled by `coverage / 255`, then the
/// result is premultiplied as usual.
fn glyph_src(colour: Rgba, coverage: u8) -> [u8; 4] {
    let Rgba([r, g, b, a]) = colour;
    let src_a = (((a as u32) * (coverage as u32)) + 127) / 255;
    let mul = |c: u8| (((c as u32) * src_a + 127) / 255) as u8;
    [mul(r), mul(g), mul(b), src_a as u8]
}

/// Source-over blend of premultiplied `src` onto premultiplied `dst`:
/// `out = src + dst * (1 - src_a)`, per channel (alpha included), rounded.
fn blend_over(dst: [u8; 4], src: [u8; 4]) -> [u8; 4] {
    let inv = 255 - src[3] as u32;
    let mut out = [0u8; 4];
    for k in 0..4 {
        let v = (src[k] as u32) * 255 + (dst[k] as u32) * inv;
        out[k] = ((v + 127) / 255) as u8;
    }
    out
}

/// A software canvas: a premultiplied-alpha RGBA buffer plus the rects
/// touched since the last [`Canvas::take_dirty`].
pub struct Canvas {
    frame: Frame,
    dirty: Vec<Rect>,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Canvas {
        Canvas {
            frame: Frame { width, height, rgba: vec![0u8; width as usize * height as usize * 4] },
            dirty: Vec::new(),
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.frame.width, self.frame.height)
    }

    /// Transparent black over `r`; marks it dirty.
    pub fn clear(&mut self, r: Rect) {
        let Some(clip) = Rect::full(&self.frame).intersect(r) else { return };
        for y in clip.y..clip.y + clip.h as i32 {
            for x in clip.x..clip.x + clip.w as i32 {
                self.set_pixel(x as u32, y as u32, [0, 0, 0, 0]);
            }
        }
        self.mark_dirty(clip);
    }

    /// Source-over blend of `colour` over `r`, corners rounded by `radius`
    /// px (0 = square).
    pub fn fill_rect(&mut self, r: Rect, colour: Rgba, radius: u32) {
        let Some(clip) = Rect::full(&self.frame).intersect(r) else { return };
        let src = premultiply(colour);
        for y in clip.y..clip.y + clip.h as i32 {
            for x in clip.x..clip.x + clip.w as i32 {
                let (local_x, local_y) = (x - r.x, y - r.y);
                if in_rounded_rect(local_x, local_y, r.w, r.h, radius as i32) {
                    self.blend_pixel(x as u32, y as u32, src);
                }
            }
        }
        self.mark_dirty(clip);
    }

    /// A `thickness`-px outline just inside `r`; `dash` = `Some(len)` draws
    /// len-on/len-off around the perimeter. The ring is `r` minus `r`
    /// inset by `thickness` on every side.
    pub fn stroke_rect(&mut self, r: Rect, colour: Rgba, thickness: u32, dash: Option<u32>) {
        let Some(clip) = Rect::full(&self.frame).intersect(r) else { return };
        let src = premultiply(colour);
        let inner = r.inset(thickness as i32);
        let (w, h, t) = (r.w as i32, r.h as i32, thickness as i32);
        for y in clip.y..clip.y + clip.h as i32 {
            for x in clip.x..clip.x + clip.w as i32 {
                if inner.contains(x, y) {
                    continue; // interior, not part of the ring
                }
                if let Some(len) = dash {
                    let len = len.max(1) as i32;
                    let pos = perimeter_position(x - r.x, y - r.y, w, h, t);
                    if (pos / len) % 2 != 0 {
                        continue;
                    }
                }
                self.blend_pixel(x as u32, y as u32, src);
            }
        }
        self.mark_dirty(clip);
    }

    /// Draws `text` with its left edge at `x` and baseline at `baseline`;
    /// returns the advance. Clips to the canvas. Marks the ink's bounding
    /// box dirty.
    pub fn text(&mut self, fonts: &Fonts, x: i32, baseline: i32, text: &str, style: TextStyle) -> u32 {
        let font = fonts.font(style.face);
        let tab_width = if style.tabular { tabular_width(fonts, style.face, style.size) } else { 0 };
        let mut pen_x = x;
        let mut ink: Option<Rect> = None;
        for c in text.chars() {
            let (metrics, bitmap) = font.rasterize(c, style.size);
            let own_advance = metrics.advance_width.round() as u32;
            let tabular_digit = style.tabular && c.is_ascii_digit();
            let glyph_x = if tabular_digit {
                pen_x + ((tab_width as i32 - own_advance as i32) / 2).max(0)
            } else {
                pen_x
            };
            for gy in 0..metrics.height {
                for gx in 0..metrics.width {
                    let coverage = bitmap[gy * metrics.width + gx];
                    if coverage == 0 {
                        continue;
                    }
                    let px = glyph_x + metrics.xmin + gx as i32;
                    let py = baseline - metrics.height as i32 - metrics.ymin + gy as i32;
                    if px < 0 || py < 0 || px as u32 >= self.frame.width || py as u32 >= self.frame.height {
                        continue;
                    }
                    self.blend_pixel(px as u32, py as u32, glyph_src(style.colour, coverage));
                    let touched = Rect::new(px, py, 1, 1);
                    ink = Some(match ink {
                        Some(r) => r.union(touched),
                        None => touched,
                    });
                }
            }
            pen_x += (if tabular_digit { tab_width } else { own_advance }) as i32;
        }
        if let Some(r) = ink {
            self.mark_dirty(r);
        }
        (pen_x - x) as u32
    }

    /// Straight-alpha readback of one pixel (un-premultiplied; alpha 0
    /// reads as `[0, 0, 0, 0]`). Only this crate's own tests read a `Canvas`
    /// back this way -- the real path out is `frame()`, uploaded by a
    /// backend and read back over X in `tests/readback.rs`.
    #[allow(dead_code)]
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let p = self.get_pixel(x, y);
        let a = p[3];
        if a == 0 {
            return [0, 0, 0, 0];
        }
        [
            (p[0] as u32 * 255 / a as u32) as u8,
            (p[1] as u32 * 255 / a as u32) as u8,
            (p[2] as u32 * 255 / a as u32) as u8,
            a,
        ]
    }

    /// The premultiplied buffer, borrowed.
    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    /// The rectangles touched since the last call, merged so no two
    /// overlap; then reset.
    pub fn take_dirty(&mut self) -> Vec<Rect> {
        let mut rects = std::mem::take(&mut self.dirty);
        loop {
            let mut merged_at = None;
            'search: for i in 0..rects.len() {
                for j in (i + 1)..rects.len() {
                    if rects[i].intersect(rects[j]).is_some() {
                        merged_at = Some((i, j));
                        break 'search;
                    }
                }
            }
            let Some((i, j)) = merged_at else { break };
            rects[i] = rects[i].union(rects[j]);
            rects.remove(j);
        }
        rects
    }

    fn mark_dirty(&mut self, r: Rect) {
        if r.w > 0 && r.h > 0 {
            self.dirty.push(r);
        }
    }

    fn index(&self, x: u32, y: u32) -> usize {
        ((y * self.frame.width + x) * 4) as usize
    }

    fn get_pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = self.index(x, y);
        [self.frame.rgba[i], self.frame.rgba[i + 1], self.frame.rgba[i + 2], self.frame.rgba[i + 3]]
    }

    fn set_pixel(&mut self, x: u32, y: u32, p: [u8; 4]) {
        let i = self.index(x, y);
        self.frame.rgba[i..i + 4].copy_from_slice(&p);
    }

    fn blend_pixel(&mut self, x: u32, y: u32, src: [u8; 4]) {
        let out = blend_over(self.get_pixel(x, y), src);
        self.set_pixel(x, y, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_rect_blends_source_over_and_reads_back_straight_alpha() {
        let mut c = Canvas::new(4, 4);
        c.fill_rect(Rect::new(0, 0, 4, 4), Rgba::rgb(0xffffff), 0);
        c.fill_rect(Rect::new(1, 1, 2, 2), Rgba::rgba(0x000000, 0.5), 0);
        assert_eq!(c.pixel(0, 0), [255, 255, 255, 255]);
        let p = c.pixel(1, 1);
        assert!((126..=129).contains(&p[0]) && p[3] == 255, "{p:?}");
        assert_eq!(Canvas::new(2, 2).pixel(0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn rounded_corners_leave_the_corner_pixel_clear() {
        let mut c = Canvas::new(8, 8);
        c.fill_rect(Rect::new(0, 0, 8, 8), Rgba::rgb(0xff0000), 3);
        assert_eq!(c.pixel(0, 0)[3], 0);
        assert_eq!(c.pixel(4, 0)[3], 255);
        assert_eq!(c.pixel(4, 4), [255, 0, 0, 255]);
    }

    #[test]
    fn text_inks_inside_its_box_and_tabular_digits_align() {
        let fonts = Fonts::embedded();
        let style = TextStyle { face: Face::Sans, size: 13.0, colour: Rgba::rgb(0xffffff), tabular: true };
        let mut c = Canvas::new(200, 40);
        let adv = c.text(&fonts, 4, 20, "981", style);
        assert!(adv > 0 && adv < 40, "{adv}");
        let inked = (0..200).flat_map(|x| (0..40).map(move |y| (x, y))).filter(|&(x, y)| c.pixel(x, y)[3] > 0).count();
        assert!(inked > 20, "{inked}");
        assert_eq!(measure(&fonts, "111", style), measure(&fonts, "999", style), "tabular");
        let prop = TextStyle { tabular: false, ..style };
        assert!(measure(&fonts, "111", prop) <= measure(&fonts, "999", prop));
        assert_eq!(measure(&fonts, "", style), 0);
    }

    #[test]
    fn ellipsize_fits_and_marks_truncation() {
        let fonts = Fonts::embedded();
        let style = TextStyle { face: Face::Sans, size: 13.0, colour: Rgba::rgb(0xffffff), tabular: false };
        let full = "a very long mob name indeed";
        let w = measure(&fonts, full, style);
        assert_eq!(ellipsize(&fonts, full, style, w), full);
        let cut = ellipsize(&fonts, full, style, w / 2);
        assert!(cut.ends_with('…') && cut.len() < full.len(), "{cut}");
        assert!(measure(&fonts, &cut, style) <= w / 2);
    }

    #[test]
    fn dirty_rects_merge_and_reset() {
        let mut c = Canvas::new(100, 100);
        c.fill_rect(Rect::new(0, 0, 10, 10), Rgba::rgb(0), 0);
        c.fill_rect(Rect::new(5, 5, 10, 10), Rgba::rgb(0), 0);
        c.fill_rect(Rect::new(50, 50, 10, 10), Rgba::rgb(0), 0);
        let d = c.take_dirty();
        assert_eq!(d.len(), 2, "{d:?}");
        assert!(d.contains(&Rect::new(0, 0, 15, 15)));
        assert!(c.take_dirty().is_empty());
    }

    #[test]
    fn rect_algebra() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 10, 10);
        assert_eq!(a.union(b), Rect::new(0, 0, 15, 15));
        assert_eq!(a.intersect(b), Some(Rect::new(5, 5, 5, 5)));
        assert_eq!(a.intersect(Rect::new(20, 20, 1, 1)), None);
        assert!(a.contains(9, 9) && !a.contains(10, 10));
        assert_eq!(a.inset(2), Rect::new(2, 2, 6, 6));
    }

    #[test]
    fn stroke_rect_draws_a_ring_and_leaves_the_interior_untouched() {
        let mut c = Canvas::new(10, 10);
        c.stroke_rect(Rect::new(0, 0, 10, 10), Rgba::rgb(0x00ff00), 2, None);
        assert_eq!(c.pixel(0, 0), [0, 255, 0, 255]);
        assert_eq!(c.pixel(5, 5), [0, 0, 0, 0], "interior untouched");

        let mut d = Canvas::new(10, 10);
        d.stroke_rect(Rect::new(0, 0, 10, 10), Rgba::rgb(0x00ff00), 2, Some(2));
        let inked = (0..10).flat_map(|x| (0..10).map(move |y| (x, y))).filter(|&(x, y)| d.pixel(x, y)[3] > 0).count();
        assert!(inked > 0, "a dashed stroke still paints some of the ring");
    }

    #[test]
    fn line_metrics_scale_with_size_and_are_non_negative() {
        let fonts = Fonts::embedded();
        let (ascent_small, descent_small) = line_metrics(&fonts, Face::Sans, 13.0);
        let (ascent_large, _descent_large) = line_metrics(&fonts, Face::Sans, 26.0);
        assert!(ascent_small > 0);
        assert!(ascent_large > ascent_small, "{ascent_small} vs {ascent_large}");
        let _ = descent_small;
    }

    #[test]
    fn all_three_faces_load_and_measure_something() {
        let fonts = Fonts::embedded();
        let style_for = |face| TextStyle { face, size: 13.0, colour: Rgba::rgb(0xffffff), tabular: false };
        assert!(measure(&fonts, "A", style_for(Face::Sans)) > 0);
        assert!(measure(&fonts, "A", style_for(Face::SansBold)) > 0);
        assert!(measure(&fonts, "A", style_for(Face::Mono)) > 0);
    }
}
