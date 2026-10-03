//! Grabbing the icon bitmaps while the native flyout is still on screen.
//!
//! The icons only exist as pixels inside the flyout, so the flow is: read the
//! icon rectangles with UI Automation, grab that part of the desktop with GDI,
//! cut the individual icons out, and key their background away so they sit on
//! our own panel instead of showing a square of Windows' flyout.

use std::collections::HashMap;

use anyhow::Context;
use base64::Engine as _;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HDC, HBITMAP,
};
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::UI::WindowsAndMessaging::PW_RENDERFULLCONTENT;

/// A rectangle of the desktop, top-down, 32 bits per pixel, BGRA.
pub struct ScreenBitmap {
    pub width: i32,
    pub height: i32,
    pub bgra: Vec<u8>,
}

impl ScreenBitmap {
    fn pixel(&self, x: i32, y: i32) -> [u8; 3] {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return [0, 0, 0];
        }
        let offset = ((y * self.width + x) * 4) as usize;
        [
            self.bgra[offset],
            self.bgra[offset + 1],
            self.bgra[offset + 2],
        ]
    }
}

/// Renders a window into a bitmap without it having to be on screen.
///
/// `PW_RENDERFULLCONTENT` is what makes this work for content drawn by
/// DirectComposition, which is exactly what the flyout is. That is what lets the
/// flyout be parked off screen and still read.
pub fn print_window(handle: HWND, width: i32, height: i32) -> anyhow::Result<ScreenBitmap> {
    anyhow::ensure!(width > 0 && height > 0, "empty capture rectangle");

    unsafe {
        let screen = GetDC(None);
        anyhow::ensure!(!screen.is_invalid(), "GetDC returned no device context");
        let memory = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let previous = SelectObject(memory, bitmap.into());

        let result = (|| -> anyhow::Result<ScreenBitmap> {
            anyhow::ensure!(
                PrintWindow(handle, memory, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)).as_bool(),
                "PrintWindow failed"
            );
            extract(memory, bitmap, width, height)
        })();

        SelectObject(memory, previous);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        result
    }
}

/// Reads a compatible bitmap out as top-down 32-bit BGRA.
unsafe fn extract(
    memory: HDC,
    bitmap: HBITMAP,
    width: i32,
    height: i32,
) -> anyhow::Result<ScreenBitmap> {
    let mut info = BITMAPINFO::default();
    info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = width;
    info.bmiHeader.biHeight = -height; // negative: rows top-down
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB.0;

    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    let lines = GetDIBits(
        memory,
        bitmap,
        0,
        height as u32,
        Some(pixels.as_mut_ptr() as *mut std::ffi::c_void),
        &mut info,
        DIB_RGB_COLORS,
    );
    anyhow::ensure!(lines != 0, "GetDIBits returned no scan lines");

    Ok(ScreenBitmap {
        width,
        height,
        bgra: pixels,
    })
}

/// The centres of the flyout's icon cells, read from the drawn grid.
///
/// UI Automation reports icon rectangles that can still be moving while the
/// flyout's opening animation runs, and a rectangle half a cell off crops the
/// wrong square — visibly, as a square of background with the icon pushed down.
/// The drawn grid cannot lie, so the cells are found by projecting "not the
/// background colour" onto each axis and cutting the result into bands.
///
/// Only the flyout's own surface is measured. The window carries a dark shadow
/// ring around that surface, and the ring would otherwise be read as an extra
/// row of icons along two edges.
pub fn grid_centres(bitmap: &ScreenBitmap) -> Vec<(i32, i32)> {
    let background = dominant_colour(bitmap);
    let Some(surface) = surface_bounds(bitmap, background) else {
        return Vec::new();
    };

    let inked = |x: i32, y: i32| -> bool {
        if x < surface.0 || y < surface.1 || x > surface.2 || y > surface.3 {
            return false;
        }
        let at = ((y as usize) * (bitmap.width as usize) + x as usize) * 4;
        let difference = (bitmap.bgra[at] as i32 - background[0] as i32).abs()
            + (bitmap.bgra[at + 1] as i32 - background[1] as i32).abs()
            + (bitmap.bgra[at + 2] as i32 - background[2] as i32).abs();
        difference > INK_THRESHOLD
    };

    let columns = bands(surface.2 - surface.0, |offset| {
        let x = surface.0 + offset;
        (surface.1..=surface.3).filter(|y| inked(x, *y)).count()
    });
    let rows = bands(surface.3 - surface.1, |offset| {
        let y = surface.1 + offset;
        (surface.0..=surface.2).filter(|x| inked(*x, y)).count()
    });

    // Row-major, which is the order the shell lists its icons in.
    let mut centres = Vec::with_capacity(columns.len() * rows.len());
    for row in &rows {
        for column in &columns {
            centres.push((surface.0 + column, surface.1 + row));
        }
    }
    centres
}

/// The rectangle covered by the flyout's own surface, as opposed to the shadow
/// painted around it.
fn surface_bounds(bitmap: &ScreenBitmap, background: [u8; 3]) -> Option<(i32, i32, i32, i32)> {
    let mut bounds: Option<(i32, i32, i32, i32)> = None;
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            let at = ((y as usize) * (bitmap.width as usize) + x as usize) * 4;
            let difference = (bitmap.bgra[at] as i32 - background[0] as i32).abs()
                + (bitmap.bgra[at + 1] as i32 - background[1] as i32).abs()
                + (bitmap.bgra[at + 2] as i32 - background[2] as i32).abs();
            if difference <= SURFACE_TOLERANCE {
                bounds = Some(match bounds {
                    None => (x, y, x, y),
                    Some((left, top, right, bottom)) => (
                        left.min(x),
                        top.min(y),
                        right.max(x),
                        bottom.max(y),
                    ),
                });
            }
        }
    }
    // Pull the edges in by a pixel or two so the surface's own rounded corner and
    // its antialiased rim cannot register as ink.
    bounds.and_then(|(left, top, right, bottom)| {
        let inset = 3;
        (right - left > inset * 4 && bottom - top > inset * 4)
            .then_some((left + inset, top + inset, right - inset, bottom - inset))
    })
}

/// The centre of every band of ink along one axis, snapped onto an even lattice
/// so that a band which was merged with its neighbour or missed because its icon
/// was too pale still ends up in its proper place.
fn bands(length: i32, ink: impl Fn(i32) -> usize) -> Vec<i32> {
    let counts: Vec<usize> = (0..length).map(&ink).collect();
    let peak = counts.iter().copied().max().unwrap_or(0);
    if peak == 0 {
        return Vec::new();
    }
    let cut = ((peak as f64) * 0.15).ceil() as usize;

    let mut centres = Vec::new();
    let mut start: Option<i32> = None;
    for (position, count) in counts.iter().enumerate() {
        let position = position as i32;
        if *count >= cut {
            if start.is_none() {
                start = Some(position);
            }
        } else if let Some(from) = start.take() {
            centres.push((from + position - 1) / 2);
        }
    }
    if let Some(from) = start {
        centres.push((from + length - 1) / 2);
    }

    if centres.len() < 2 {
        return centres;
    }

    let mut gaps: Vec<i32> = centres.windows(2).map(|pair| pair[1] - pair[0]).collect();
    gaps.sort_unstable();
    let spacing = gaps[gaps.len() / 2];
    if spacing <= 0 {
        return centres;
    }

    let first = centres[0];
    let last = *centres.last().unwrap();
    let mut lattice = Vec::new();
    let mut position = first;
    while position <= last {
        lattice.push(position);
        position += spacing;
    }
    lattice
}

/// The colour that covers the most of the bitmap: the flyout's own background.
fn dominant_colour(bitmap: &ScreenBitmap) -> [u8; 3] {
    let mut counts: HashMap<u32, u32> = HashMap::new();
    for pixel in bitmap.bgra.chunks_exact(4) {
        let key = u32::from_le_bytes([pixel[0], pixel[1], pixel[2], 0]);
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, seen)| *seen)
        .map(|(key, _)| {
            let bytes = key.to_le_bytes();
            [bytes[0], bytes[1], bytes[2]]
        })
        .unwrap_or([0, 0, 0])
}

/// Total per-channel difference at which a pixel counts as part of an icon rather
/// than as background. Low-contrast pale icons still clear it comfortably.
const INK_THRESHOLD: i32 = 60;

/// Difference within which a pixel counts as the flyout's own surface rather than
/// as the shadow around it, used to find where that surface begins and ends.
const SURFACE_TOLERANCE: i32 = 24;

/// Wraps PNG bytes that came from elsewhere — the shell's own icon snapshot — as
/// a data URL the webview can use as-is. No decoding, no recolouring: the icon
/// already carries its own alpha.
pub fn png_bytes_data_url(png: &[u8]) -> String {
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    )
}

/// Cuts a square icon out of a grabbed region.
///
/// Returns the RGBA pixels plus the colour that was keyed out. The caller passes
/// that colour back to the UI, which paints it behind the icon: that way the
/// anti-aliased edge blends into the panel instead of showing a halo, and icons
/// that are nearly the same colour as the flyout stay visible.
pub fn icon_rgba(source: &ScreenBitmap, left: i32, top: i32, size: i32) -> (Vec<u8>, [u8; 3]) {
    let size = size.max(1);
    let mut out = vec![0u8; (size as usize) * (size as usize) * 4];

    // Sample the four corners and keep the one that is least like the others:
    // an icon that reaches into a corner would otherwise win the vote and eat
    // the icon itself.
    let corners = [
        source.pixel(left, top),
        source.pixel(left + size - 1, top),
        source.pixel(left, top + size - 1),
        source.pixel(left + size - 1, top + size - 1),
    ];
    let background = corners
        .iter()
        .map(|candidate| {
            let spread: u32 = corners
                .iter()
                .map(|other| channel_distance(candidate, other))
                .sum();
            (spread, *candidate)
        })
        .min_by_key(|(spread, _)| *spread)
        .map(|(_, colour)| colour)
        .unwrap_or([0, 0, 0]);

    for row in 0..size {
        for column in 0..size {
            let target = ((row * size + column) * 4) as usize;
            let pixel = source.pixel(left + column, top + row);
            let distance = channel_distance(&pixel, &background);

            out[target] = pixel[2];
            out[target + 1] = pixel[1];
            out[target + 2] = pixel[0];
            // Icons are anti-aliased onto the flyout, so a hard cut-off would
            // leave a halo. Fade instead, then hard-zero anything close enough
            // that it is certainly background.
            out[target + 3] = if distance <= BACKGROUND_HARD {
                0
            } else if distance >= BACKGROUND_SOFT {
                255
            } else {
                (((distance - BACKGROUND_HARD) as f32 / (BACKGROUND_SOFT - BACKGROUND_HARD) as f32)
                    * 255.0) as u8
            };
        }
    }

    (out, background)
}

/// Total per-channel difference below which a pixel is certainly background, and
/// above which it is certainly part of the icon.
///
/// Deliberately tiny. The panel paints the keyed colour behind the icon anyway,
/// so a pixel that is merely *close* to the background is invisible whether it
/// survives or not — while a partially transparent pale glyph would be blended
/// into the very colour it was drawn on and disappear.
const BACKGROUND_HARD: u32 = 4;
const BACKGROUND_SOFT: u32 = 12;

fn channel_distance(a: &[u8; 3], b: &[u8; 3]) -> u32 {
    (a[0].abs_diff(b[0]) as u32) + (a[1].abs_diff(b[1]) as u32) + (a[2].abs_diff(b[2]) as u32)
}

/// Fraction of pixels that are not near black. Used to tell a real render apart
/// from the empty bitmap a compositor hands out before it has drawn anything.
pub fn ink_ratio(bitmap: &ScreenBitmap) -> f32 {
    if bitmap.bgra.is_empty() {
        return 0.0;
    }
    let mut inked = 0usize;
    for pixel in bitmap.bgra.chunks_exact(4) {
        if pixel[0] > 12 || pixel[1] > 12 || pixel[2] > 12 {
            inked += 1;
        }
    }
    inked as f32 / (bitmap.bgra.len() / 4) as f32
}

/// Encodes RGBA pixels as a PNG data URL the webview can put straight into an
/// `<img src>`.
pub fn png_data_url(width: u32, height: u32, rgba: &[u8]) -> anyhow::Result<String> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().context("PNG header failed")?;
        writer
            .write_image_data(rgba)
            .context("PNG payload failed")?;
    }
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    ))
}