use chequer_core::brush::MaskTip;

pub const STAMP_SIDE: u32 = 256;
const EDGE_TEXELS: f32 = 1.0;

pub fn pixel_mask(width: u32, height: u32, coverage: impl Fn(f32, f32) -> f32) -> MaskTip {
    let pixels = (0..width * height)
        .map(|index| {
            let (x, y) = ((index % width) as f32 + 0.5, (index / width) as f32 + 0.5);
            (coverage(x, y).clamp(0.0, 1.0) * u8::MAX as f32).round() as u8
        })
        .collect();
    MaskTip::new(width, height, pixels).expect("stamp size is within tip limits")
}

pub fn unit_mask(coverage: impl Fn(f32, f32) -> f32) -> MaskTip {
    let half = STAMP_SIDE as f32 / 2.0;
    pixel_mask(STAMP_SIDE, STAMP_SIDE, |x, y| coverage(x / half - 1.0, y / half - 1.0))
}

pub fn unit_edge(inside: f32) -> f32 {
    (inside * STAMP_SIDE as f32 / 2.0 / EDGE_TEXELS + 0.5).clamp(0.0, 1.0)
}

pub fn texel_edge(inside_texels: f32) -> f32 {
    (inside_texels / EDGE_TEXELS + 0.5).clamp(0.0, 1.0)
}

pub fn rotated(x: f32, y: f32, angle: f32) -> (f32, f32) {
    let (sin, cos) = angle.sin_cos();
    (x * cos + y * sin, y * cos - x * sin)
}

pub fn convex_inside(x: f32, y: f32, vertices: &[(f32, f32)]) -> f32 {
    let count = vertices.len();
    (0..count)
        .map(|index| {
            let ((from_x, from_y), (to_x, to_y)) = (vertices[index], vertices[(index + 1) % count]);
            let (edge_x, edge_y) = (to_x - from_x, to_y - from_y);
            (edge_x * (y - from_y) - edge_y * (x - from_x)) / edge_x.hypot(edge_y)
        })
        .fold(f32::MAX, f32::min)
}
