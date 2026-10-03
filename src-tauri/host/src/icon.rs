//! A tray icon's pixels, copied while the sender is still inside `Shell_NotifyIcon`.
//!
//! The `HICON` in the payload is the caller's handle. Shared icons (`LoadIcon`)
//! are valid in every process; a private icon usually is not. When it is not,
//! this returns nothing and the panel falls back to the shell's registry snapshot.

use base64::Engine as _;
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS,
};
use windows::Win32::UI::WindowsAndMessaging::{GetIconInfo, HICON, ICONINFO};

/// PNG data URL, or nothing when the handle cannot be read from this process.
pub fn png_data_url(icon: isize) -> Option<String> {
    if icon == 0 {
        return None;
    }
    let handle = HICON(icon as *mut _);
    let mut info = ICONINFO::default();
    if unsafe { GetIconInfo(handle, &mut info) }.is_err() {
        return None;
    }
    let color = info.hbmColor;
    let mask = info.hbmMask;
    let png = color_bitmap_png(color);
    unsafe {
        if !color.is_invalid() {
            let _ = DeleteObject(color.into());
        }
        if !mask.is_invalid() {
            let _ = DeleteObject(mask.into());
        }
    }
    png.map(|bytes| {
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        format!("data:image/png;base64,{encoded}")
    })
}

fn color_bitmap_png(bitmap: windows::Win32::Graphics::Gdi::HBITMAP) -> Option<Vec<u8>> {
    if bitmap.is_invalid() {
        return None;
    }
    let mut desc = BITMAP::default();
    let wrote = unsafe {
        GetObjectW(
            bitmap.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut desc as *mut _ as *mut _),
        )
    };
    if wrote == 0 || desc.bmWidth <= 0 || desc.bmHeight <= 0 {
        return None;
    }
    // Tray icons are small. A runaway size is not an icon we should decode on
    // the shell's UI thread.
    if desc.bmWidth > 256 || desc.bmHeight > 256 {
        return None;
    }
    let width = desc.bmWidth;
    let height = desc.bmHeight;
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
    unsafe { ReleaseDC(None, screen) };
    if rows == 0 {
        return None;
    }
    // GDI hands back BGRA. The PNG encoder wants RGBA.
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&pixels).ok()?;
    }
    Some(encoded)
}
