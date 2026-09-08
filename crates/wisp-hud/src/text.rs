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

impl Renderer {
    /// `scale_px` is explicit and has no default. A 7-inch handheld panel and a
    /// 2560x1440 desktop are different legibility problems.
    pub fn new(scale_px: f32) -> Renderer {
        let font = Font::from_bytes(FONT_BYTES, FontSettings::default())
            .expect("vendored font must parse");
        Renderer { font, scale_px }
    }

    pub fn render(&self, text: &str) -> Frame {
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
        let height = (max_ascent + max_descent).max(1) as u32;
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
                    let y = baseline as i32 - metrics.height as i32 - metrics.ymin + gy as i32;
                    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
                        continue;
                    }
                    let o = ((y as u32 * width + x as u32) * 4) as usize;
                    // Premultiplied white. X11 and Wayland both want premultiplied alpha.
                    rgba[o] = coverage;
                    rgba[o + 1] = coverage;
                    rgba[o + 2] = coverage;
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
}
