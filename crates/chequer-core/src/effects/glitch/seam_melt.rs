use crate::effects::params::{choice, int};
use crate::effects::util::Cancel;
use crate::effects::{Category, EffectDef, Job, ids};
use crate::raster::{BYTES_PER_PIXEL, Raster, Rgba};

const DIRECTIONS: &[&str] = &["seams_vertical", "seams_horizontal", "seams_both"];
const MODES: &[&str] = &["carve", "inflate"];
const ENERGIES: &[&str] = &["energy_edges", "energy_dark", "energy_light", "energy_flat"];
const RESAMPLES: &[&str] = &["nearest", "bilinear"];
const HORIZONTAL: usize = 1;
const INFLATE: usize = 1;
const BILINEAR: usize = 1;
const WORK_SIDE: usize = 640;
const MIN_FIELD_WIDTH: usize = 3;
const MAX_CHANNEL: f32 = 255.0;
const PERCENT: f32 = 100.0;
const MAX_GRADIENT: f32 = 2.0;

pub const DEF: EffectDef = EffectDef {
    id: ids::SEAM_MELT,
    category: Category::Glitch,
    params: &[
        choice("direction", DIRECTIONS, 0),
        choice("mode", MODES, 0),
        int("seams", 0, 90, 30),
        choice("energy", ENERGIES, 0),
        int("max_jump", 1, 8, 1),
        choice("resample", RESAMPLES, 1),
    ],
    render,
};

#[derive(Clone, Copy)]
enum Energy {
    Edges,
    Dark,
    Light,
    Flat,
}

const ENERGY_KINDS: [Energy; 4] = [Energy::Edges, Energy::Dark, Energy::Light, Energy::Flat];

struct Settings {
    inflate: bool,
    share: f32,
    energy: Energy,
    jump: usize,
    bilinear: bool,
}

struct Frame {
    width: usize,
    height: usize,
    pixels: Vec<Rgba>,
}

impl Frame {
    fn transposed(&self) -> Frame {
        let mut pixels = vec![Rgba::default(); self.pixels.len()];
        for y in 0..self.height {
            for x in 0..self.width {
                pixels[x * self.height + y] = self.pixels[y * self.width + x];
            }
        }
        Frame { width: self.height, height: self.width, pixels }
    }

    fn resized_width(&self, target: usize, bilinear: bool) -> Frame {
        let ratio = self.width as f32 / target as f32;
        let mut pixels = Vec::with_capacity(target * self.height);
        for row in self.pixels.chunks_exact(self.width) {
            for x in 0..target {
                let position = ((x as f32 + 0.5) * ratio - 0.5).clamp(0.0, (self.width - 1) as f32);
                pixels.push(if bilinear { blend(row[position.floor() as usize], row[(position.floor() as usize + 1).min(self.width - 1)], position.fract()) } else { row[position.round() as usize] });
            }
        }
        Frame { width: target, height: self.height, pixels }
    }
}

fn blend(from: Rgba, to: Rgba, amount: f32) -> Rgba {
    std::array::from_fn(|channel| (f32::from(from[channel]) + (f32::from(to[channel]) - f32::from(from[channel])) * amount).round() as u8)
}

fn luma(color: Rgba) -> f32 {
    (0.299 * f32::from(color[0]) + 0.587 * f32::from(color[1]) + 0.114 * f32::from(color[2])) / MAX_CHANNEL
}

struct SeamField {
    stride: usize,
    width: usize,
    height: usize,
    luma: Vec<f32>,
    energy: Vec<f32>,
    origin: Vec<u32>,
}

impl SeamField {
    fn reduced(frame: &Frame, factor: usize, kind: Energy) -> SeamField {
        let (stride, height) = ((frame.width / factor).max(1), (frame.height / factor).max(1));
        let mut luma_values = Vec::with_capacity(stride * height);
        for row in 0..height {
            let rows = row * factor..if row == height - 1 { frame.height } else { (row + 1) * factor };
            for column in 0..stride {
                let columns = column * factor..if column == stride - 1 { frame.width } else { (column + 1) * factor };
                let total: f32 = rows.clone().flat_map(|y| columns.clone().map(move |x| (y, x))).map(|(y, x)| luma(frame.pixels[y * frame.width + x])).sum();
                luma_values.push(total / (rows.len() * columns.len()) as f32);
            }
        }
        let mut field = SeamField { stride, width: stride, height, luma: luma_values, energy: vec![0.0; stride * height], origin: (0..stride * height).map(|index| (index % stride) as u32).collect() };
        for y in 0..height {
            for x in 0..stride {
                field.energy[y * stride + x] = field.energy_at(kind, y, x);
            }
        }
        field
    }

    fn energy_at(&self, kind: Energy, y: usize, x: usize) -> f32 {
        let at = |dy: isize, dx: isize| {
            let row = (y as isize + dy).clamp(0, self.height as isize - 1) as usize;
            let column = (x as isize + dx).clamp(0, self.width as isize - 1) as usize;
            self.luma[row * self.stride + column]
        };
        let gradient = (at(0, 1) - at(0, -1)).abs() + (at(1, 0) - at(-1, 0)).abs();
        match kind {
            Energy::Edges => gradient,
            Energy::Dark => at(0, 0),
            Energy::Light => 1.0 - at(0, 0),
            Energy::Flat => MAX_GRADIENT - gradient,
        }
    }

    fn seam(&self, jump: usize, cost: &mut [f32], back: &mut [i8]) -> Vec<usize> {
        let (stride, width) = (self.stride, self.width);
        cost[..width].copy_from_slice(&self.energy[..width]);
        for y in 1..self.height {
            for x in 0..width {
                let (low, high) = (x.saturating_sub(jump), (x + jump).min(width - 1));
                let (mut best, mut best_at) = (f32::MAX, low);
                for candidate in low..=high {
                    let value = cost[(y - 1) * stride + candidate];
                    if value < best {
                        (best, best_at) = (value, candidate);
                    }
                }
                cost[y * stride + x] = self.energy[y * stride + x] + best;
                back[y * stride + x] = (best_at as isize - x as isize) as i8;
            }
        }
        let last = (self.height - 1) * stride;
        let mut x = (0..width).min_by(|&a, &b| cost[last + a].total_cmp(&cost[last + b])).unwrap_or(0);
        let mut seam = vec![0; self.height];
        for y in (0..self.height).rev() {
            seam[y] = x;
            x = (x as isize + isize::from(back[y * stride + x])) as usize;
        }
        seam
    }

    fn remove(&mut self, seam: &[usize], jump: usize, kind: Energy) {
        for (y, &x) in seam.iter().enumerate() {
            let (start, end) = (y * self.stride + x, y * self.stride + self.width);
            self.luma.copy_within(start + 1..end, start);
            self.energy.copy_within(start + 1..end, start);
            self.origin.copy_within(start + 1..end, start);
        }
        self.width -= 1;
        for (y, &x) in seam.iter().enumerate() {
            for column in x.saturating_sub(jump + 1)..=(x + jump + 1).min(self.width - 1) {
                self.energy[y * self.stride + column] = self.energy_at(kind, y, column);
            }
        }
    }

    fn removed_flags(mut self, count: usize, settings: &Settings, cancel: &Cancel) -> Option<Vec<Vec<bool>>> {
        let mut flags = vec![vec![false; self.stride]; self.height];
        let mut cost = vec![0.0; self.stride * self.height];
        let mut back = vec![0i8; self.stride * self.height];
        for _ in 0..count {
            if cancel.is_set() {
                return None;
            }
            let seam = self.seam(settings.jump, &mut cost, &mut back);
            for (y, &x) in seam.iter().enumerate() {
                flags[y][self.origin[y * self.stride + x] as usize] = true;
            }
            self.remove(&seam, settings.jump, settings.energy);
        }
        Some(flags)
    }
}

fn rebuild(frame: &Frame, factor: usize, flags: &[Vec<bool>], inflate: bool) -> Frame {
    let (bands, rows) = (flags[0].len(), flags.len());
    let mut pixels = Vec::new();
    let mut width = frame.width;
    for (y, row) in frame.pixels.chunks_exact(frame.width).enumerate() {
        let row_flags = &flags[(y / factor).min(rows - 1)];
        let before = pixels.len();
        for (x, &pixel) in row.iter().enumerate() {
            let band = (x / factor).min(bands - 1);
            let marked = row_flags[band] && x - band * factor < factor;
            if !marked || inflate {
                pixels.push(pixel);
            }
            if marked && inflate {
                pixels.push(blend(pixel, row[(x + 1).min(frame.width - 1)], 0.5));
            }
        }
        width = pixels.len() - before;
    }
    Frame { width, height: frame.height, pixels }
}

fn melt(frame: Frame, settings: &Settings, cancel: &Cancel) -> Option<Frame> {
    let factor = frame.width.max(frame.height).div_ceil(WORK_SIDE);
    let field = SeamField::reduced(&frame, factor, settings.energy);
    if field.width < MIN_FIELD_WIDTH {
        return Some(frame);
    }
    let count = ((settings.share * field.width as f32).round() as usize).min(field.width - MIN_FIELD_WIDTH + 1);
    if count == 0 {
        return Some(frame);
    }
    let flags = field.removed_flags(count, settings, cancel)?;
    Some(rebuild(&frame, factor, &flags, settings.inflate).resized_width(frame.width, settings.bilinear))
}

fn render(job: &Job) -> Option<Raster> {
    let region = job.source.crop(job.area);
    let params = job.params;
    let settings = Settings {
        inflate: params.choice("mode") == INFLATE,
        share: params.int("seams") as f32 / PERCENT,
        energy: ENERGY_KINDS[params.choice("energy")],
        jump: params.int("max_jump") as usize,
        bilinear: params.choice("resample") == BILINEAR,
    };
    let (width, height) = (region.width() as usize, region.height() as usize);
    let pixels = region.as_bytes().chunks_exact(BYTES_PER_PIXEL).map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]]).collect();
    let mut frame = Frame { width, height, pixels };
    let direction = params.choice("direction");
    if direction != HORIZONTAL {
        frame = melt(frame, &settings, job.cancel)?;
    }
    if direction >= HORIZONTAL {
        frame = melt(frame.transposed(), &settings, job.cancel)?.transposed();
    }
    Raster::from_rgba(region.width(), region.height(), frame.pixels.into_iter().flatten().collect()).ok()
}
