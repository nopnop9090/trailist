//! A tray icon's pixels, copied while the sender is still inside `Shell_NotifyIcon`.
//!
//! The `HICON` in the payload is the caller's handle. Reading it has to leave
//! that handle alone: `GetIconInfo` on an icon built with `CreateIconIndirect`
//! can return the caller's own bitmaps, and deleting those blanks the glyph
//! Explorer is about to draw. Process Lasso replaces its icon constantly, so
//! the blank sticks. The copy is thrown away after the pixels are read.

use base64::Engine as _;
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::UI::WindowsAndMessaging::{CopyIcon, DestroyIcon, GetIconInfo, HICON, ICONINFO};

/// PNG data URL, or nothing when the handle cannot be read from this process.
pub fn png_data_url(icon: isize) -> Option<String> {
    if icon == 0 {
        return None;
    }
    let original = HICON(icon as *mut _);
    let copy = unsafe { CopyIcon(original) }.ok()?;
    let png = snapshot(copy);
    unsafe {
        let _ = DestroyIcon(copy);
    }
    png.map(|bytes| {
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        format!("data:image/png;base64,{encoded}")
    })
}

fn snapshot(icon: HICON) -> Option<Vec<u8>> {
    let mut info = ICONINFO::default();
    if unsafe { GetIconInfo(icon, &mut info) }.is_err() {
        return None;
    }
    let color = info.hbmColor;
    let mask = info.hbmMask;
    let png = compose(color, mask);
    unsafe {
        if !color.is_invalid() {
            let _ = DeleteObject(color.into());
        }
        if !mask.is_invalid() {
            let _ = DeleteObject(mask.into());
        }
    }
    png
}

fn compose(color: HBITMAP, mask: HBITMAP) -> Option<Vec<u8>> {
    let (width, height, mut pixels) = color_pixels(color)?;
    let mask_opaque = mask_opaque(mask, width, height);
    paint_alpha(&mut pixels, mask_opaque.as_deref());
    encode_png(width, height, &pixels)
}

/// 32-bit icons often leave the alpha byte at zero and describe the shape in
/// the mask instead. A bitmap that already has alpha is left as it is.
fn paint_alpha(pixels: &mut [u8], mask_opaque: Option<&[bool]>) {
    let uses_alpha = pixels.chunks_exact(4).any(|pixel| pixel[3] > 16);
    if uses_alpha {
        return;
    }
    if let Some(mask) = mask_opaque {
        if mask.len() == pixels.len() / 4 {
            for (pixel, on) in pixels.chunks_exact_mut(4).zip(mask.iter()) {
                pixel[3] = if *on { 255 } else { 0 };
            }
            return;
        }
    }
    for pixel in pixels.chunks_exact_mut(4) {
        if pixel[0] | pixel[1] | pixel[2] != 0 {
            pixel[3] = 255;
        }
    }
}

fn color_pixels(bitmap: HBITMAP) -> Option<(i32, i32, Vec<u8>)> {
    if bitmap.is_invalid() {
        return None;
    }
    let (width, height) = bitmap_size(bitmap)?;
    if width > 256 || height > 256 {
        return None;
    }
    let pixels = dib_rgba(bitmap, width, height)?;
    Some((width, height, pixels))
}

/// Opaque where the AND mask is clear. A colour icon's mask is one bit deep
/// and the same size; a monochrome icon's mask is twice as tall and only the
/// top half is the AND mask.
fn mask_opaque(bitmap: HBITMAP, width: i32, height: i32) -> Option<Vec<bool>> {
    if bitmap.is_invalid() {
        return None;
    }
    let (mask_width, mask_height) = bitmap_size(bitmap)?;
    if mask_width != width || (mask_height != height && mask_height != height * 2) {
        return None;
    }
    let pixels = dib_rgba(bitmap, width, height)?;
    Some(pixels.chunks_exact(4).map(|pixel| pixel[0] < 128).collect())
}

fn bitmap_size(bitmap: HBITMAP) -> Option<(i32, i32)> {
    let mut desc = BITMAP::default();
    let wrote = unsafe {
        GetObjectW(
            bitmap.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut desc as *mut _ as *mut _),
        )
    };
    if wrote == 0 || desc.bmWidth <= 0 || desc.bmHeight <= 0 {
        None
    } else {
        Some((desc.bmWidth, desc.bmHeight))
    }
}

fn dib_rgba(bitmap: HBITMAP, width: i32, height: i32) -> Option<Vec<u8>> {
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let screen = unsafe { GetDC(None) };
    let rows = unsafe {
        GetDIBits(
            screen,
            bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    unsafe {
        ReleaseDC(None, screen);
    }
    if rows == 0 {
        return None;
    }
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Some(pixels)
}

fn encode_png(width: i32, height: i32, pixels: &[u8]) -> Option<Vec<u8>> {
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(pixels).ok()?;
    }
    Some(encoded)
}

#[cfg(test)]
mod tests {
    use super::paint_alpha;

    #[test]
    fn existing_alpha_is_kept() {
        let mut pixels = [255, 255, 255, 200, 0, 0, 0, 0];
        paint_alpha(&mut pixels, Some(&[true, true]));
        assert_eq!(pixels[3], 200);
        assert_eq!(pixels[7], 0);
    }

    #[test]
    fn zero_alpha_takes_the_mask() {
        let mut pixels = [10, 20, 30, 0, 10, 20, 30, 0];
        paint_alpha(&mut pixels, Some(&[true, false]));
        assert_eq!(pixels[3], 255);
        assert_eq!(pixels[7], 0);
    }

    #[test]
    fn zero_alpha_without_a_mask_uses_ink() {
        let mut pixels = [10, 0, 0, 0, 0, 0, 0, 0];
        paint_alpha(&mut pixels, None);
        assert_eq!(pixels[3], 255);
        assert_eq!(pixels[7], 0);
    }
}
