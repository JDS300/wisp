// SPDX-License-Identifier: MIT
//! Glyph rasterisation into an RGBA buffer.
//!
//! Pure: no display, no windowing, no I/O beyond the embedded font. This is
//! deliberate -- rendering and compositing cannot be meaningfully unit-tested,
//! so as much decision-making as possible lives here instead.

use crate::backend::Frame;
use fontdue::{Font, FontSettings};

const FONT_BYTES: &[u8] = include_bytes!("../../../assets/DejaVuSansMono.ttf");

pub struct Renderer {
    font: Font,
    scale_px: f32,
}

/// Vertical gap between stacked lines, in pixels.
pub const LINE_GAP_PX: u32 = 4;

/// One row of text and the colour to draw it in (straight, not premultiplied).
pub struct Line {
    pub text: String,
    pub rgb: [u8; 3],
}

impl Renderer {
    /// `scale_px` is explicit and has no default. A 7-inch handheld panel and a
    /// 2560x1440 desktop are different legibility problems.
    pub fn new(scale_px: f32) -> Renderer {
        let font = Font::from_bytes(FONT_BYTES, FontSettings::default())
            .expect("vendored font must parse");
        Renderer { font, scale_px }
    }

    // main.rs now draws everything through `render_lines`, but `render` stays
    // as the crate's single-line, white-text entry point (exercised by the
    // text tests below), so it is not dead API even though nothing in the
    // current binary calls it.
    #[allow(dead_code)]
    pub fn render(&self, text: &str) -> Frame {
        self.render_one(text, [255, 255, 255])
    }

    /// The height every rendered line is padded to, from the font's own
    /// vertical metrics at this renderer's scale: ascent minus descent,
    /// rounded up. Falls back to the rendered ink height of `"Wg"` (an
    /// ascender and a descender together) for a font that reports no
    /// metrics. Used so every HUD row is the same height regardless of
    /// which glyphs it happens to contain.
    pub fn line_height(&self) -> u32 {
        match self.font.horizontal_line_metrics(self.scale_px) {
            Some(m) => (m.ascent - m.descent).ceil().max(1.0) as u32,
            None => {
                let (ascent, descent) = self.ink_extent("Wg");
                (ascent + descent).max(1) as u32
            }
        }
    }

    /// Ascent and descent, in pixels, of the tallest and lowest glyph in
    /// `text`, each clamped at zero. Metrics only -- no bitmaps -- so this
    /// is cheap enough for `line_height`'s fallback to call without
    /// recursing back into `render_one`.
    fn ink_extent(&self, text: &str) -> (i32, i32) {
        let metrics: Vec<_> = text.chars().map(|c| self.font.metrics(c, self.scale_px)).collect();
        let ascent = metrics.iter().map(|m| (m.ymin + m.height as i32).max(0)).max().unwrap_or(0);
        let descent = metrics.iter().map(|m| (-m.ymin).max(0)).max().unwrap_or(0);
        (ascent, descent)
    }

    /// Stack lines top to bottom, `LINE_GAP_PX` apart, left-aligned, the
    /// canvas as wide as the widest line. Every line is the same height
    /// (`line_height`, or its own ink if that is somehow taller).
    pub fn render_lines(&self, lines: &[Line]) -> Frame {
        let frames: Vec<Frame> = lines.iter().map(|l| self.render_one(&l.text, l.rgb)).collect();
        if frames.is_empty() {
            return Frame { width: 0, height: 0, rgba: Vec::new() };
        }
        let width = frames.iter().map(|f| f.width).max().unwrap_or(0);
        let height: u32 = frames.iter().map(|f| f.height).sum::<u32>() + LINE_GAP_PX * (frames.len() as u32 - 1);
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        let mut y0 = 0u32;
        for f in &frames {
            for y in 0..f.height {
                let src = ((y * f.width) * 4) as usize;
                let dst = (((y0 + y) * width) * 4) as usize;
                rgba[dst..dst + (f.width * 4) as usize].copy_from_slice(&f.rgba[src..src + (f.width * 4) as usize]);
            }
            y0 += f.height + LINE_GAP_PX;
        }
        Frame { width, height, rgba }
    }

    fn render_one(&self, text: &str, rgb: [u8; 3]) -> Frame {
        if text.is_empty() {
            return Frame { width: 0, height: 0, rgba: Vec::new() };
        }

        // Rasterise the whole string up front; the result is reused both to
        // measure the canvas and to place each glyph. Monospace, so advance
        // is uniform.
        let rasterised: Vec<_> = text
            .chars()
            .map(|c| self.font.rasterize(c, self.scale_px))
            .collect();

        let advance = rasterised
            .iter()
            .map(|(m, _)| m.advance_width.ceil() as u32)
            .max()
            .unwrap_or(1)
            .max(1);
        // Ascent and descent are tracked separately, each clamped at zero, so
        // that a descender (ymin < 0, e.g. 'g') grows the canvas downward
        // instead of pushing its own bottom rows past the bounds check.
        let max_ascent = rasterised
            .iter()
            .map(|(m, _)| (m.ymin + m.height as i32).max(0))
            .max()
            .unwrap_or(0);
        let max_descent = rasterised
            .iter()
            .map(|(m, _)| (-m.ymin).max(0))
            .max()
            .unwrap_or(0);
        let baseline = max_ascent;
        let ink_height = (max_ascent + max_descent).max(1) as u32;
        // Every row is padded to the same height regardless of its glyphs;
        // the padding lands below the ink (the canvas grows downward, the
        // baseline does not move), so a row without descenders simply has
        // blank pixels under it rather than sitting off-centre.
        let height = ink_height.max(self.line_height());
        let width = advance * rasterised.len() as u32;

        let mut rgba = vec![0u8; (width * height * 4) as usize];

        for (i, (metrics, bitmap)) in rasterised.iter().enumerate() {
            let pen_x = i as u32 * advance;
            for gy in 0..metrics.height {
                for gx in 0..metrics.width {
                    let coverage = bitmap[gy * metrics.width + gx];
                    if coverage == 0 {
                        continue;
                    }
                    let x = pen_x as i32 + metrics.xmin + gx as i32;
                    let y = baseline - metrics.height as i32 - metrics.ymin + gy as i32;
                    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
                        continue;
                    }
                    let o = ((y as u32 * width + x as u32) * 4) as usize;
                    // Premultiplied colour. X11 and Wayland both want premultiplied alpha.
                    rgba[o] = (rgb[0] as u32 * coverage as u32 / 255) as u8;
                    rgba[o + 1] = (rgb[1] as u32 * coverage as u32 / 255) as u8;
                    rgba[o + 2] = (rgb[2] as u32 * coverage as u32 / 255) as u8;
                    rgba[o + 3] = coverage;
                }
            }
        }

        Frame { width, height, rgba }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_non_empty_output_for_non_empty_input() {
        let r = Renderer::new(32.0);
        let frame = r.render("1234");
        assert!(frame.width > 0);
        assert!(frame.height > 0);
        assert_eq!(frame.rgba.len(), (frame.width * frame.height * 4) as usize);
    }

    #[test]
    fn some_pixels_are_actually_marked() {
        let r = Renderer::new(32.0);
        let frame = r.render("8");
        let lit = frame.rgba.chunks_exact(4).filter(|px| px[3] > 0).count();
        assert!(lit > 0, "rasterised glyph produced no opaque pixels");
    }

    #[test]
    fn empty_input_produces_an_empty_frame_rather_than_panicking() {
        let r = Renderer::new(32.0);
        let frame = r.render("");
        assert_eq!(frame.rgba.len(), (frame.width * frame.height * 4) as usize);
    }

    #[test]
    fn a_larger_scale_produces_a_taller_frame() {
        let small = Renderer::new(16.0).render("123");
        let large = Renderer::new(48.0).render("123");
        assert!(
            large.height > small.height,
            "scale must actually affect output: {} vs {}",
            small.height,
            large.height
        );
    }

    #[test]
    fn monospace_width_is_proportional_to_character_count() {
        let r = Renderer::new(32.0);
        let one = r.render("1").width;
        let four = r.render("1234").width;
        assert!(four > one * 3, "expected roughly 4x, got {one} then {four}");
    }

    #[test]
    fn a_descender_is_not_clipped_by_the_bounds_check() {
        let r = Renderer::new(32.0);
        let frame = r.render("g");
        let lit = frame.rgba.chunks_exact(4).filter(|px| px[3] > 0).count();

        let (_, bitmap) = r.font.rasterize('g', 32.0);
        let expected = bitmap.iter().filter(|&&c| c != 0).count();

        assert_eq!(
            lit, expected,
            "some of the glyph's coverage bytes were dropped by the canvas bounds check"
        );
    }

    #[test]
    fn lines_stack_vertically_and_take_the_widest_width() {
        let r = Renderer::new(32.0);
        let long = r.render("1234");
        let short = r.render("12");
        let frame = r.render_lines(&[
            Line { text: "12".to_string(), rgb: [255, 255, 255] },
            Line { text: "1234".to_string(), rgb: [255, 255, 255] },
        ]);
        assert_eq!(frame.width, long.width, "as wide as the widest line");
        // Rows are now uniform height (F1): both lines are all-digit, so
        // both are padded up to the same `line_height()` and the per-line
        // "short vs long" height difference the old comment described no
        // longer holds. short.height == long.height == r.line_height() here.
        assert_eq!(short.height, long.height, "digits alone are padded to the same row height");
        assert_eq!(frame.height, 2 * r.line_height() + LINE_GAP_PX);
        assert_eq!(frame.rgba.len(), (frame.width * frame.height * 4) as usize);
    }

    #[test]
    fn rows_are_the_same_height_regardless_of_descenders() {
        let r = Renderer::new(32.0);
        let no_descenders = r.render("1234");
        let all_descenders = r.render("gjpqy");
        assert_eq!(
            no_descenders.height, all_descenders.height,
            "every row is padded to line_height(), so descenders don't make a row taller"
        );
    }

    #[test]
    fn a_coloured_line_is_premultiplied_by_coverage() {
        let r = Renderer::new(32.0);
        let frame = r.render_lines(&[Line { text: "8".to_string(), rgb: [255, 0, 0] }]);
        let max_px = frame
            .rgba
            .chunks_exact(4)
            .max_by_key(|px| px[3])
            .unwrap();
        assert!(max_px[3] > 200, "a fully covered pixel exists");
        assert_eq!(max_px[0], max_px[3], "red channel equals alpha (premultiplied)");
        assert_eq!(max_px[1], 0);
        assert_eq!(max_px[2], 0);
        assert!(frame.rgba.chunks_exact(4).all(|px| px[0] <= px[3] && px[1] <= px[3] && px[2] <= px[3]));
    }

    #[test]
    fn an_empty_line_list_is_an_empty_frame() {
        let frame = Renderer::new(32.0).render_lines(&[]);
        assert_eq!((frame.width, frame.height), (0, 0));
    }
}
