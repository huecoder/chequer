use chequer_core::raster::Rgba;

const HEX_RGB_LEN: usize = 6;
const HEX_RGBA_LEN: usize = 8;
const HUE_SECTORS: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsv {
    pub h: f32,
    pub s: f32,
    pub v: f32,
}

pub fn rgb_to_hsv(rgb: [u8; 3]) -> Hsv {
    let [r, g, b] = rgb.map(|c| c as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let h = if delta == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / delta).rem_euclid(HUE_SECTORS))
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    Hsv { h, s: if max == 0.0 { 0.0 } else { delta / max }, v: max }
}

pub fn hsv_to_rgb(hsv: Hsv) -> [u8; 3] {
    let h = hsv.h.rem_euclid(360.0) / 60.0;
    let c = hsv.v * hsv.s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = hsv.v - c;
    [r + m, g + m, b + m].map(|channel| (channel * 255.0).round().clamp(0.0, 255.0) as u8)
}

pub fn to_hex(color: Rgba) -> String {
    format!("{:02X}{:02X}{:02X}", color[0], color[1], color[2])
}

pub fn parse_hex(text: &str, current_alpha: u8) -> Option<Rgba> {
    let digits = text.trim().trim_start_matches('#');
    if digits.len() != HEX_RGB_LEN && digits.len() != HEX_RGBA_LEN {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(digits.get(i..i + 2)?, 16).ok();
    let alpha = if digits.len() == HEX_RGBA_LEN { byte(6)? } else { current_alpha };
    Some([byte(0)?, byte(2)?, byte(4)?, alpha])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_round_trips_primary_colors() {
        for rgb in [[255, 0, 0], [0, 255, 0], [0, 0, 255], [10, 200, 90], [128, 128, 128]] {
            assert_eq!(hsv_to_rgb(rgb_to_hsv(rgb)), rgb);
        }
    }

    #[test]
    fn hex_parses_with_and_without_hash() {
        assert_eq!(parse_hex("#ff8000", 255), Some([255, 128, 0, 255]));
        assert_eq!(parse_hex("00ff0080", 255), Some([0, 255, 0, 128]));
    }

    #[test]
    fn bad_hex_is_rejected() {
        assert_eq!(parse_hex("xyz", 255), None);
        assert_eq!(parse_hex("12345", 255), None);
    }
}
