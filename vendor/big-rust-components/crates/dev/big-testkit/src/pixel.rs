// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Pixel-region math over an already-captured PNG.
//!
//! Generalizes the transparency smoke: average the RGBA of a rectangle and
//! assert it is translucent or opaque. **pure** — decodes a provided PNG with
//! the workspace's canonical `gdk_pixbuf` decoder (no display, no new crate)
//! and does plain averaging.
//!
//! The screenshot *capture* (spectacle / weston-screenshooter) is the VM-only
//! follow-up wave; this module only reads a PNG a capture step produced.

use std::path::Path;

use relm4::gtk;

/// A rectangle of pixels, top-left origin, in image coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Left edge (px).
    pub x: i32,
    /// Top edge (px).
    pub y: i32,
    /// Width (px).
    pub w: i32,
    /// Height (px).
    pub h: i32,
}

impl Rect {
    /// A rectangle at `(x, y)` sized `w * h`.
    #[must_use]
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
}

/// Average `[r, g, b, a]` (each `0.0..=255.0`) over `rect` of the PNG at `png`.
///
/// `rect` is clamped to the image bounds. The pixbuf is promoted to RGBA first,
/// so the alpha channel is always present.
///
/// # Panics
///
/// Panics when the PNG cannot be decoded or `rect` covers no in-bounds pixel —
/// this is a test helper, so a bad fixture/rect should fail loudly.
#[must_use]
pub fn sample_region_rgba(png: &Path, rect: Rect) -> [f64; 4] {
    let decoded = gtk::gdk_pixbuf::Pixbuf::from_file(png)
        .unwrap_or_else(|error| panic!("decode {}: {error}", png.display()));
    let rgba = if decoded.has_alpha() {
        decoded
    } else {
        decoded
            .add_alpha(false, 0, 0, 0)
            .expect("promote pixbuf to RGBA")
    };

    let width = rgba.width();
    let height = rgba.height();
    let stride = usize::try_from(rgba.rowstride()).expect("non-negative rowstride");
    let channels = usize::try_from(rgba.n_channels()).expect("non-negative channel count");
    let pixels = rgba.read_pixel_bytes();

    let x0 = rect.x.max(0);
    let y0 = rect.y.max(0);
    let x1 = (rect.x + rect.w).min(width);
    let y1 = (rect.y + rect.h).min(height);

    let mut sums = [0.0_f64; 4];
    let mut count = 0_u64;
    for y in y0..y1 {
        let row = usize::try_from(y).expect("row index") * stride;
        for x in x0..x1 {
            let base = row + usize::try_from(x).expect("col index") * channels;
            for (channel, sum) in sums.iter_mut().enumerate() {
                *sum += f64::from(pixels[base + channel]);
            }
            count += 1;
        }
    }

    assert!(
        count > 0,
        "rect {rect:?} covers no pixel of {width}x{height}"
    );
    let denominator = count as f64;
    sums.map(|sum| sum / denominator)
}

/// Assert the average alpha over `rect` is below `max_alpha` (the region shows
/// through — a translucent surface).
///
/// # Panics
///
/// Panics when the region is more opaque than `max_alpha`.
pub fn assert_translucent(png: &Path, rect: Rect, max_alpha: f64) {
    let alpha = sample_region_rgba(png, rect)[3];
    assert!(
        alpha < max_alpha,
        "region {rect:?} of {} has average alpha {alpha:.1} ≥ {max_alpha:.1} — expected translucent",
        png.display()
    );
}

/// Assert the region is fully opaque (average alpha ≈ 255).
///
/// # Panics
///
/// Panics when the region is not fully opaque.
pub fn assert_opaque(png: &Path, rect: Rect) {
    let alpha = sample_region_rgba(png, rect)[3];
    assert!(
        alpha >= 254.5,
        "region {rect:?} of {} has average alpha {alpha:.1} < 255 — expected opaque",
        png.display()
    );
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/translucency.png")
    }

    // Fixture layout (4x4 RGBA): left half opaque red (255,0,0,255),
    // right half translucent blue (0,0,255,128).

    #[test]
    fn left_half_is_opaque_red() {
        let avg = sample_region_rgba(&fixture(), Rect::new(0, 0, 2, 4));
        assert!((avg[0] - 255.0).abs() < 0.5, "red channel {}", avg[0]);
        assert!(avg[2] < 0.5, "blue channel {}", avg[2]);
        assert_opaque(&fixture(), Rect::new(0, 0, 2, 4));
    }

    #[test]
    fn right_half_is_translucent_blue() {
        let avg = sample_region_rgba(&fixture(), Rect::new(2, 0, 2, 4));
        assert!((avg[3] - 128.0).abs() < 0.5, "alpha {}", avg[3]);
        assert_translucent(&fixture(), Rect::new(2, 0, 2, 4), 200.0);
    }
}
