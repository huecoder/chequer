use crate::raster::Rgba;

const HUE_SECTORS: f32 = 6.0;
const FULL_TURN: f32 = 1.0;
const COLOR_MAX: f32 = 255.0;

pub fn shifted(color: Rgba, hue_turns: f32, saturation: f32, value: f32) -> Rgba {
    let [r, g, b] = [color[0], color[1], color[2]].map(|channel| channel as f32 / COLOR_MAX);
    let max = r.max(g).max(b);
    let delta = max - r.min(g).min(b);
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(HUE_SECTORS)
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    } / HUE_SECTORS;
    let saturation_now = if max == 0.0 { 0.0 } else { delta / max };
    let hue = (hue + hue_turns).rem_euclid(FULL_TURN) * HUE_SECTORS;
    let saturation = (saturation_now + saturation).clamp(0.0, 1.0);
    let value = (max + value).clamp(0.0, 1.0);
    let chroma = value * saturation;
    let secondary = chroma * (1.0 - (hue % 2.0 - 1.0).abs());
    let (r, g, b) = match hue as u32 {
        0 => (chroma, secondary, 0.0),
        1 => (secondary, chroma, 0.0),
        2 => (0.0, chroma, secondary),
        3 => (0.0, secondary, chroma),
        4 => (secondary, 0.0, chroma),
        _ => (chroma, 0.0, secondary),
    };
    let floor = value - chroma;
    let channel = |part: f32| ((part + floor) * COLOR_MAX).round() as u8;
    [channel(r), channel(g), channel(b), color[3]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_shift_keeps_color() {
        assert_eq!(shifted([200, 80, 40, 255], 0.0, 0.0, 0.0), [200, 80, 40, 255]);
    }

    #[test]
    fn half_turn_moves_red_to_cyan() {
        assert_eq!(shifted([255, 0, 0, 255], 0.5, 0.0, 0.0), [0, 255, 255, 255]);
    }
}
