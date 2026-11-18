const EDGE_SOFTNESS: f32 = 0.25;

pub fn dry_gate(load: f32, dryness: f32) -> f32 {
    let low = dryness * (1.0 - EDGE_SOFTNESS);
    let high = dryness * (1.0 + EDGE_SOFTNESS);
    if load >= high {
        return 1.0;
    }
    if load <= low {
        return 0.0;
    }
    let t = (load - low) / (high - low);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_brush_drops_light_bristles_and_keeps_loaded_ones() {
        assert_eq!(dry_gate(0.3, 0.0), 1.0);
        assert_eq!(dry_gate(0.2, 0.5), 0.0);
        assert_eq!(dry_gate(0.9, 0.5), 1.0);
        assert!(dry_gate(0.5, 0.5) > 0.0 && dry_gate(0.5, 0.5) < 1.0);
    }
}
