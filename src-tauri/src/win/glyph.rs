//! Shell glyphs such as "Safely Remove Hardware" are one flat colour.
//!
//! The registered bitmap is the form the taskbar uses, white with an alpha
//! shape. The overflow flyout recolours that to the shell text colour, so on a
//! light flyout the same glyph is black. A colourful icon is not a glyph and
//! is returned unchanged.

use base64::Engine as _;

const LIGHT_INK: [u8; 3] = [0x16, 0x19, 0x1f];
const DARK_INK: [u8; 3] = [0xe8, 0xec, 0xf2];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Light,
    Dark,
    Other,
}

/// Recolours a flat glyph so it matches the flyout. `dark` is the shell scheme.
pub fn for_shell(data_url: &str, dark: bool) -> String {
    let Some(encoded) = data_url.strip_prefix("data:image/png;base64,") else {
        return data_url.to_string();
    };
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
        return data_url.to_string();
    };
    let Some((width, height, mut pixels)) = decode(&bytes) else {
        return data_url.to_string();
    };
    let target = match (classify(&pixels), dark) {
        (Kind::Light, false) => LIGHT_INK,
        (Kind::Dark, true) => DARK_INK,
        _ => return data_url.to_string(),
    };
    for pixel in pixels.chunks_exact_mut(4) {
        if pixel[3] < 24 {
            continue;
        }
        pixel[0] = target[0];
        pixel[1] = target[1];
        pixel[2] = target[2];
    }
    encode(width, height, &pixels)
        .map(|png| {
            let encoded = base64::engine::general_purpose::STANDARD.encode(png);
            format!("data:image/png;base64,{encoded}")
        })
        .unwrap_or_else(|| data_url.to_string())
}

fn classify(pixels: &[u8]) -> Kind {
    let mut seen = 0u32;
    let mut light = 0u32;
    let mut dark = 0u32;
    for pixel in pixels.chunks_exact(4) {
        let alpha = pixel[3];
        if alpha < 24 {
            continue;
        }
        let (red, green, blue) = (pixel[0], pixel[1], pixel[2]);
        let max = red.max(green).max(blue);
        let min = red.min(green).min(blue);
        if max - min > 18 {
            return Kind::Other;
        }
        seen += 1;
        // Straight white keeps the colour and varies the alpha. A premultiplied
        // white glyph stores the coverage in the colour channels instead.
        if red >= 200 || red.abs_diff(alpha) <= 24 {
            light += 1;
        }
        if red <= 48 && alpha.abs_diff(red) > 40 {
            dark += 1;
        }
    }
    if seen < 8 {
        return Kind::Other;
    }
    if light * 10 >= seen * 9 && dark == 0 {
        Kind::Light
    } else if dark * 10 >= seen * 9 && light == 0 {
        Kind::Dark
    } else {
        Kind::Other
    }
}

fn decode(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    buffer.truncate(info.buffer_size());
    Some((info.width, info.height, buffer))
}

fn encode(width: u32, height: u32, pixels: &[u8]) -> Option<Vec<u8>> {
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(pixels).ok()?;
    }
    Some(encoded)
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;

    use super::for_shell;

    fn png(pixels: &[u8]) -> String {
        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, 4, 4);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(pixels).unwrap();
        }
        let text = base64::engine::general_purpose::STANDARD.encode(encoded);
        format!("data:image/png;base64,{text}")
    }

    fn first_pixel(data_url: &str) -> [u8; 4] {
        let encoded = data_url.strip_prefix("data:image/png;base64,").unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).unwrap();
        let (_, _, pixels) = super::decode(&bytes).unwrap();
        pixels[..4].try_into().unwrap()
    }

    #[test]
    fn a_white_glyph_is_black_on_a_light_shell() {
        let mut pixels = vec![0u8; 4 * 16];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[255, 255, 255, 255]);
        }
        let tinted = for_shell(&png(&pixels), false);
        assert_eq!(first_pixel(&tinted), [0x16, 0x19, 0x1f, 255]);
    }

    #[test]
    fn a_white_glyph_stays_on_a_dark_shell() {
        let mut pixels = vec![0u8; 4 * 16];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[255, 255, 255, 180]);
        }
        let source = png(&pixels);
        assert_eq!(for_shell(&source, true), source);
    }

    #[test]
    fn a_coloured_icon_is_not_recoloured() {
        let mut pixels = vec![0u8; 4 * 16];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[20, 80, 220, 255]);
        }
        let source = png(&pixels);
        assert_eq!(for_shell(&source, false), source);
    }
}
