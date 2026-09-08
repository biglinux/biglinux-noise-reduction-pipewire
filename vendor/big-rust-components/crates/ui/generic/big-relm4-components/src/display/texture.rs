// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Pixbuf / raw-RGBA → [`gtk::gdk::Texture`] helpers.
//!
//! Every BigLinux media surface (camera roll, file-manager preview, image
//! viewer, video poster) ends at a `gtk::Picture` fed a `gdk::Texture`. This is
//! the ONE canonical decode→texture conversion so apps don't re-roll the
//! pixbuf→`MemoryTexture` boilerplate (and its rowstride / lifetime hazards) at
//! every call site. GTK-only — no `image`-crate or media-runtime coupling.

use std::path::Path;

use gtk::prelude::*;
use relm4::gtk;

/// Decoded image as tightly packed RGBA8 plus its dimensions. `Send`, so it can
/// cross from a worker thread — where the `!Send` `Pixbuf` decode runs — back to
/// the GTK main thread, where [`rgba_to_texture`] wraps it in a `MemoryTexture`
/// (the one step that must run on the main thread). This is how a file manager
/// or gallery decodes thumbnails off the main loop without ever moving a
/// `Pixbuf`/`Texture` across threads.
pub struct RgbaImage {
    /// Tightly packed RGBA8 pixels: `width * height * 4` bytes, no row padding.
    pub rgba: Vec<u8>,
    /// Image width in pixels.
    pub width: i32,
    /// Image height in pixels.
    pub height: i32,
}

/// Pack a `Pixbuf` into tightly packed RGBA8 in [`gtk::gdk::MemoryTexture`] layout. No GTK
/// texture is created, so — unlike [`pixbuf_to_texture`] — this is safe to call
/// on a worker thread (it touches only the `Pixbuf` and a plain buffer).
///
/// Feeding GTK the pixbuf's padded rowstride (or borrowing its pixel bytes) is a
/// layout/lifetime mismatch that can corrupt the heap, so we copy row-by-row
/// into a packed owned buffer.
#[must_use]
pub fn pixbuf_to_rgba(pixbuf: &gtk::gdk_pixbuf::Pixbuf) -> Option<RgbaImage> {
    let rgba = if pixbuf.has_alpha() {
        pixbuf.clone()
    } else {
        pixbuf.add_alpha(false, 0, 0, 0).ok()?
    };
    let width = rgba.width();
    let height = rgba.height();
    if width <= 0 || height <= 0 {
        return None;
    }
    let row = usize::try_from(width).ok()?.checked_mul(4)?;
    let src_stride = usize::try_from(rgba.rowstride()).ok()?;
    let rows = usize::try_from(height).ok()?;
    let src = rgba.read_pixel_bytes();
    let mut packed = vec![0_u8; row.checked_mul(rows)?];
    for y in 0..rows {
        let s = y.checked_mul(src_stride)?;
        let d = y * row;
        packed
            .get_mut(d..d + row)?
            .copy_from_slice(src.get(s..s + row)?);
    }
    Some(RgbaImage {
        rgba: packed,
        width,
        height,
    })
}

/// Convert a `Pixbuf` into a [`gtk::gdk::Texture`]. Main-thread only
/// (`MemoryTexture::new` asserts it); for large or networked files decode off
/// the main loop with [`decode_rgba_at_scale`] + [`rgba_to_texture`] instead.
#[must_use]
pub fn pixbuf_to_texture(pixbuf: &gtk::gdk_pixbuf::Pixbuf) -> Option<gtk::gdk::Texture> {
    let img = pixbuf_to_rgba(pixbuf)?;
    Some(rgba_to_texture(img.rgba, img.width, img.height))
}

/// Decode an image file, scaled to fit `max_edge` on the longer side, into
/// [`RgbaImage`] (aspect preserved; `None` on decode failure). The `!Send`
/// `Pixbuf` never escapes, so this is the **worker-thread half** of an
/// off-main-loop decode: run it in [`crate::task::spawn_blocking_result`] and
/// build the texture in the `on_result` via [`rgba_to_texture`].
#[must_use]
pub fn decode_rgba_at_scale(path: &Path, max_edge: i32) -> Option<RgbaImage> {
    let pixbuf =
        gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(path, max_edge, max_edge, true).ok()?;
    pixbuf_to_rgba(&pixbuf)
}

/// Decode an image file, scaled to fit `max_edge`, into a texture. Aspect ratio
/// preserved; `None` on decode failure. Runs the decode inline on the calling
/// thread — call it off the main loop (e.g. via
/// [`crate::task::spawn_blocking_result`] with [`decode_rgba_at_scale`]) for
/// large or networked files.
#[must_use]
pub fn texture_from_file_at_scale(path: &Path, max_edge: i32) -> Option<gtk::gdk::Texture> {
    let img = decode_rgba_at_scale(path, max_edge)?;
    Some(rgba_to_texture(img.rgba, img.width, img.height))
}

/// Build a texture from already-decoded, tightly packed RGBA8 bytes (e.g. the
/// `image` crate's `to_rgba8()` output). `rgba.len()` must equal
/// `width * height * 4`.
#[must_use]
pub fn rgba_to_texture(rgba: Vec<u8>, width: i32, height: i32) -> gtk::gdk::Texture {
    let stride = usize::try_from(width).unwrap_or(0).saturating_mul(4);
    let bytes = gtk::glib::Bytes::from_owned(rgba);
    gtk::gdk::MemoryTexture::new(
        width,
        height,
        gtk::gdk::MemoryFormat::R8g8b8a8,
        &bytes,
        stride,
    )
    .upcast()
}
