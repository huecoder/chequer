use super::{BrushTip, MaskTip};
use crate::geom::FPoint;
use crate::raster::Rgba;

const SPLITMIX_INCREMENT: u64 = 0x9E37_79B9_7F4A_7C15;
const SPLITMIX_FIRST_MULTIPLIER: u64 = 0xBF58_476D_1CE4_E5B9;
const SPLITMIX_SECOND_MULTIPLIER: u64 = 0x94D0_49BB_1331_11EB;
const UNIT_BITS: u32 = 24;
const MASK_EDGE_THRESHOLD: f32 = 0.5;

#[derive(Clone, Copy, Debug)]
pub struct Dab {
    pub center: FPoint,
    pub size: f32,
    pub angle: f32,
    pub roundness: f32,
    pub strength: f32,
    pub hardness: f32,
    pub color: Rgba,
    pub variant: usize,
    pub dryness: f32,
}

pub struct Jitter {
    state: u64,
}

impl Jitter {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn unit(&mut self) -> f32 {
        self.state = self.state.wrapping_add(SPLITMIX_INCREMENT);
        let mut mixed = self.state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(SPLITMIX_FIRST_MULTIPLIER);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(SPLITMIX_SECOND_MULTIPLIER);
        mixed ^= mixed >> 31;
        (mixed >> (u64::BITS - UNIT_BITS)) as f32 / (1u64 << UNIT_BITS) as f32
    }

    pub fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }
}

pub struct DabSampler<'a> {
    mask: Option<&'a MaskTip>,
    dab: Dab,
    cos: f32,
    sin: f32,
    hardness: f32,
    antialias: bool,
    texel_px: f32,
}

impl<'a> DabSampler<'a> {
    pub fn new(tip: &'a BrushTip, dab: Dab, hardness: f32, antialias: bool) -> Self {
        let mask = tip.mask_for(dab.variant);
        let texel_px = mask.map_or(1.0, |mask| dab.size / mask.width().max(mask.height()) as f32);
        Self { mask, dab, cos: dab.angle.cos(), sin: dab.angle.sin(), hardness, antialias, texel_px }
    }

    pub fn reach(&self) -> f32 {
        match self.mask {
            None => self.dab.size / 2.0,
            Some(mask) => (mask.width() as f32).hypot(mask.height() as f32) * self.texel_px / 2.0,
        }
    }

    pub fn coverage_at(&self, x: f32, y: f32) -> f32 {
        let (dx, dy) = (x - self.dab.center.x, y - self.dab.center.y);
        let along = dx * self.cos + dy * self.sin;
        let across = (dy * self.cos - dx * self.sin) / self.dab.roundness;
        match self.mask {
            None => brush_coverage(along.hypot(across), self.dab.size / 2.0, self.hardness, self.antialias),
            Some(mask) => self.mask_coverage(mask, along, across),
        }
    }

    fn mask_coverage(&self, mask: &MaskTip, along: f32, across: f32) -> f32 {
        let tip_x = along / self.texel_px + mask.width() as f32 / 2.0;
        let tip_y = across / self.texel_px + mask.height() as f32 / 2.0;
        if self.antialias {
            return mask.sample(tip_x, tip_y, self.texel_px);
        }
        if mask.sample_nearest(tip_x, tip_y) >= MASK_EDGE_THRESHOLD { 1.0 } else { 0.0 }
    }
}

fn brush_coverage(distance: f32, radius: f32, hardness: f32, antialias: bool) -> f32 {
    let edge = if antialias {
        (radius - distance + 0.5).clamp(0.0, 1.0)
    } else if distance <= radius {
        1.0
    } else {
        0.0
    };
    let inner = radius * hardness.clamp(0.0, 1.0);
    if edge == 0.0 || distance <= inner || radius <= inner {
        return edge;
    }
    let t = (distance - inner) / (radius - inner);
    edge * (1.0 - t * t * (3.0 - 2.0 * t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_brush_center_is_stronger_than_rim() {
        let center = brush_coverage(0.0, 10.0, 0.0, true);
        let rim = brush_coverage(8.0, 10.0, 0.0, true);
        assert!(center > rim);
    }

    #[test]
    fn non_antialiased_brush_is_binary() {
        assert_eq!(brush_coverage(2.0, 3.0, 1.0, false), 1.0);
        assert_eq!(brush_coverage(3.4, 3.0, 1.0, false), 0.0);
    }

    #[test]
    fn jitter_is_reproducible_and_stays_in_the_unit_interval() {
        let (mut first, mut second) = (Jitter::new(7), Jitter::new(7));
        for _ in 0..100 {
            let value = first.unit();
            assert_eq!(value, second.unit());
            assert!((0.0..1.0).contains(&value));
        }
    }

    #[test]
    fn rotated_flat_mask_covers_its_rotated_corner() {
        let tip = BrushTip::Mask(MaskTip::new(8, 2, vec![255; 16]).unwrap());
        let upright = Dab { center: FPoint::new(0.0, 0.0), size: 8.0, angle: 0.0, roundness: 1.0, strength: 1.0, hardness: 1.0, color: [0, 0, 0, 255], variant: 0, dryness: 0.0 };
        let turned = Dab { angle: std::f32::consts::FRAC_PI_2, ..upright };
        assert!(DabSampler::new(&tip, upright, 1.0, true).coverage_at(3.0, 0.0) > 0.9);
        assert!(DabSampler::new(&tip, upright, 1.0, true).coverage_at(0.0, 3.0) < 0.1);
        assert!(DabSampler::new(&tip, turned, 1.0, true).coverage_at(0.0, 3.0) > 0.9);
    }
}
