
use super::params::{angle, color, flag, int, ColorSource};
use super::util::{Plane, gaussian_blur, map_area, read_region};
use super::{Category, EffectDef, Job, ids};
use crate::raster::{Raster, TRANSPARENT, over, scale_alpha};

const RADIUS_PER_SIGMA: f32 = 2.0;
const OPAQUE_BLACK: [u8; 4] = [0, 0, 0, 255];
const PERCENT: u32 = 100;
const MAX_ALPHA: u32 = 255;

pub static DEFS: [EffectDef; 1] = [EffectDef {
    id: ids::DROP_SHADOW,
    category: Category::Object,
    params: &[
        int("radius", 0, 100, 6),
        int("distance", 0, 200, 8),
        angle("angle", -180.0, 180.0, -45.0),
        int("opacity", 0, 100, 50),
        color("color", OPAQUE_BLACK, ColorSource::Fixed),
        flag("shadow_only", false),
    ],
    render: drop_shadow,
}];

fn drop_shadow(job: &Job) -> Option<Raster> {
    let sigma = job.params.int("radius") as f32 / RADIUS_PER_SIGMA;
    let distance = job.params.int("distance") as f32;
    let direction = job.params.float("angle").to_radians();
    let (shift_x, shift_y) = ((distance * direction.cos()).round() as i32, (-distance * direction.sin()).round() as i32);
    let opacity = job.params.int("opacity") as u32;
    let tint = job.params.color("color");
    let reach = (sigma * 3.0).ceil() as i32 + shift_x.abs().max(shift_y.abs());
    let (region, inner) = read_region(job.source, job.area, reach);
    let silhouette = map_area(region, job.cancel, |x, y| {
        let alpha = job.source.get_at(crate::geom::IPoint::new(x - shift_x, y - shift_y)).map_or(0, |pixel| pixel[3]);
        if alpha == 0 {
            return TRANSPARENT;
        }
        let covered = (u32::from(alpha) * opacity * u32::from(tint[3]) / (PERCENT * MAX_ALPHA)) as u8;
        scale_alpha([tint[0], tint[1], tint[2], u8::MAX], covered)
    })?;
    let shadow = gaussian_blur(Plane::from_raster(&silhouette, silhouette.bounds(), 1.0), sigma, job.cancel)?.to_raster(inner, 1.0);
    let shadow_only = job.params.bool("shadow_only");
    map_area(job.area, job.cancel, |x, y| {
        let cast = shadow.get((x - job.area.x0) as u32, (y - job.area.y0) as u32);
        if shadow_only { cast } else { over(cast, job.source.get(x as u32, y as u32)) }
    })
}
