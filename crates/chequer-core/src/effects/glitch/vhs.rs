use std::f32::consts::{FRAC_PI_2, PI};

use super::signed_unit;
use crate::effects::params::{choice, float, int, seed};
use crate::effects::util::{Rng, clamp_channel, gaussian_noise, hash, map_area, par_rows, value_noise};
use crate::effects::{Category, EffectDef, Job, ids};
use crate::geom::IRect;
use crate::raster::{BYTES_PER_PIXEL, Raster, Rgba, TRANSPARENT};

const STANDARDS: &[&str] = &["ntsc", "pal"];
const TAPE_SPEEDS: &[&str] = &["sp", "lp", "ep"];
const PERCENT: f32 = 100.0;
const MAX_CHANNEL: f32 = 255.0;
const CONTRAST_HALF_RANGE: f32 = 200.0;
const MIN_FILTER_WIDTH: f32 = 0.05;
const POLE_SCALE: f32 = 0.7;
const LUMA_POLES: usize = 2;
const CHROMA_POLES: usize = 2;
const SHARPEN_WIDEN: f32 = 2.5;
const SHARPEN_GAIN: f32 = 1.5;
const WAVE_PERIOD: f32 = 40.0;
const WAVE_SHIFT: f32 = 10.0;
const JITTER_SHIFT: f32 = 2.5;
const HEAD_SHIFT: f32 = 28.0;
const HEAD_JITTER: f32 = 6.0;
const HEAD_PHASE_ERROR: f32 = 0.8;
const HEAD_NOISE_GAIN: f32 = 8.0;
const HEAD_CHROMA_LOSS: f32 = 0.6;
const TRACKING_SHIFT: f32 = 18.0;
const TRACKING_SKEW: f32 = 0.5;
const TRACKING_THRESHOLD: f32 = 0.95;
const TRACKING_COVERAGE: f32 = 0.5;
const TRACKING_SOFTNESS: f32 = 0.1;
const TRACKING_NOISE_GAIN: f32 = 6.0;
const TRACKING_CHROMA_LOSS: f32 = 0.7;
const TRACKING_DIM: f32 = 0.2;
const PHASE_PERIOD: f32 = 18.0;
const PHASE_JITTER: f32 = 0.3;
const CRAWL_GAIN: f32 = 0.35;
const LUMA_NOISE_SCALE: f32 = 0.12;
const CHROMA_NOISE_SCALE: f32 = 0.1;
const LUMA_NOISE_STREAK: f32 = 0.8;
const CHROMA_NOISE_STREAK_RATIO: f32 = 0.4;
const NOISE_CHANNEL_STRIDE: i32 = 4;
const DROPOUT_LINE_RATE: f32 = 0.06;
const DROPOUT_MIN_LENGTH: f32 = 4.0;
const DROPOUT_MAX_LENGTH: f32 = 48.0;
const DROPOUT_LEVEL: f32 = 0.95;
const DROPOUT_FLICKER: f32 = 0.08;
const DROPOUT_FADE_UNITS: f32 = 2.0;
const DROPOUT_CHROMA_LEFT: f32 = 0.25;
const MIN_UNIT: f32 = 1.0;
const WAVE_SALT: u32 = 0x1f83_d9ab;
const JITTER_SALT: u32 = 0x5be0_cd19;
const HEAD_SALT: u32 = 0x428a_2f98;
const TRACKING_SALT: u32 = 0x7137_4491;
const BAND_SALT: u32 = 0xb5c0_fbcf;
const PHASE_SALT: u32 = 0xe9b5_dba5;
const DROPOUT_SALT: u32 = 0x3956_c25b;
const LUMA_NOISE_SALT: u32 = 0x59f1_11f1;
const CHROMA_NOISE_SALT: u32 = 0x923f_82a4;

pub const DEF: EffectDef = EffectDef {
    id: ids::VHS,
    category: Category::Glitch,
    params: &[
        choice("standard", STANDARDS, 0),
        choice("tape_speed", TAPE_SPEEDS, 0),
        float("scale", 0.25, 4.0, 1.0, 0.05),
        float("luma_blur", 0.0, 12.0, 1.5, 0.1),
        float("chroma_blur", 0.0, 40.0, 8.0, 0.5),
        float("chroma_shift", -20.0, 20.0, 1.5, 0.5),
        int("chroma_vertical", 0, 100, 55),
        int("sharpen", 0, 100, 30),
        int("saturation", 0, 200, 85),
        int("brightness", -100, 100, 0),
        int("contrast", -100, 100, 8),
        int("noise", 0, 100, 20),
        int("chroma_noise", 0, 100, 25),
        float("phase_error", 0.0, 45.0, 3.0, 0.5),
        int("dot_crawl", 0, 100, 20),
        int("wave", 0, 100, 15),
        int("line_jitter", 0, 100, 12),
        int("head_switching", 0, 100, 45),
        int("head_lines", 0, 40, 8),
        int("tracking", 0, 100, 20),
        int("tracking_height", 2, 200, 24),
        int("dropouts", 0, 100, 15),
        seed("seed"),
    ],
    render,
};

struct Standard {
    lines: f32,
    phase_error: f32,
    crawl_line_phase: f32,
}

const STANDARD_TABLE: [Standard; 2] = [
    Standard { lines: 480.0, phase_error: 1.0, crawl_line_phase: PI },
    Standard { lines: 576.0, phase_error: 0.15, crawl_line_phase: FRAC_PI_2 },
];

struct Tape {
    bandwidth_loss: f32,
    noise: f32,
}

const TAPE_TABLE: [Tape; 3] = [Tape { bandwidth_loss: 1.0, noise: 1.0 }, Tape { bandwidth_loss: 1.35, noise: 1.5 }, Tape { bandwidth_loss: 1.8, noise: 2.2 }];

struct Signal {
    luma: Vec<f32>,
    in_phase: Vec<f32>,
    quadrature: Vec<f32>,
    alpha: Vec<f32>,
}

struct Vhs {
    seed: u32,
    unit: f32,
    lines: usize,
    width: usize,
    height: usize,
    luma_blur: f32,
    chroma_blur: f32,
    chroma_shift: f32,
    chroma_vertical: f32,
    sharpen: f32,
    saturation: f32,
    brightness: f32,
    contrast: f32,
    luma_noise: f32,
    chroma_noise: f32,
    phase_error: f32,
    dot_crawl: f32,
    crawl_line_phase: f32,
    wave: f32,
    line_jitter: f32,
    head_switching: f32,
    head_lines: usize,
    tracking: f32,
    tracking_height: f32,
    dropouts: f32,
}

fn lerp(from: f32, to: f32, amount: f32) -> f32 {
    from + (to - from) * amount
}

fn to_yiq([red, green, blue]: [f32; 3]) -> [f32; 3] {
    [
        0.299 * red + 0.587 * green + 0.114 * blue,
        0.596 * red - 0.274 * green - 0.322 * blue,
        0.211 * red - 0.523 * green + 0.312 * blue,
    ]
}

fn to_rgba(luma: f32, in_phase: f32, quadrature: f32, alpha: f32) -> Rgba {
    let channels = [
        luma + 0.956 * in_phase + 0.621 * quadrature,
        luma - 0.272 * in_phase - 0.647 * quadrature,
        luma - 1.106 * in_phase + 1.703 * quadrature,
    ];
    let [red, green, blue] = channels.map(|channel| clamp_channel(channel * MAX_CHANNEL));
    [red, green, blue, clamp_channel(alpha * MAX_CHANNEL)]
}

fn lowpass(signal: &mut [f32], width: f32, poles: usize) {
    if width < MIN_FILTER_WIDTH || signal.is_empty() {
        return;
    }
    let gain = 1.0 - (-1.0 / (width * POLE_SCALE)).exp();
    for _ in 0..poles {
        let mut state = signal[0];
        for sample in signal.iter_mut() {
            state += (*sample - state) * gain;
            *sample = state;
        }
    }
}

fn shifted(signal: &[f32], offset: f32) -> Vec<f32> {
    if offset.abs() < f32::EPSILON || signal.is_empty() {
        return signal.to_vec();
    }
    let last = signal.len() - 1;
    (0..signal.len())
        .map(|index| {
            let position = (index as f32 - offset).clamp(0.0, last as f32);
            let base = position.floor() as usize;
            lerp(signal[base], signal[(base + 1).min(last)], position - base as f32)
        })
        .collect()
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn noise_line(seed: u32, line: usize, channel: i32, width: usize, streak: f32) -> Vec<f32> {
    let row = line as i32 * NOISE_CHANNEL_STRIDE + channel;
    let mut noise: Vec<f32> = (0..width).map(|x| gaussian_noise(seed, x as i32, row)).collect();
    lowpass(&mut noise, streak, 1);
    let compensation = (1.0 + streak).sqrt();
    noise.iter_mut().for_each(|sample| *sample *= compensation);
    noise
}

impl Signal {
    fn shift(&mut self, offset: f32) {
        for channel in [&mut self.luma, &mut self.in_phase, &mut self.quadrature, &mut self.alpha] {
            *channel = shifted(channel, offset);
        }
    }
}

impl Vhs {
    fn new(job: &Job) -> Vhs {
        let params = job.params;
        let standard = &STANDARD_TABLE[params.choice("standard")];
        let tape = &TAPE_TABLE[params.choice("tape_speed")];
        let (width, height) = (job.area.width() as usize, job.area.height() as usize);
        let unit = (height as f32 / standard.lines * params.float("scale")).max(MIN_UNIT);
        let percent = |key: &str| params.int(key) as f32 / PERCENT;
        Vhs {
            seed: params.seed("seed"),
            unit,
            lines: (height as f32 / unit).ceil() as usize,
            width,
            height,
            luma_blur: params.float("luma_blur") * unit * tape.bandwidth_loss,
            chroma_blur: params.float("chroma_blur") * unit * tape.bandwidth_loss,
            chroma_shift: params.float("chroma_shift") * unit,
            chroma_vertical: percent("chroma_vertical"),
            sharpen: percent("sharpen"),
            saturation: percent("saturation"),
            brightness: params.int("brightness") as f32 / CONTRAST_HALF_RANGE,
            contrast: 1.0 + percent("contrast"),
            luma_noise: percent("noise") * tape.noise,
            chroma_noise: percent("chroma_noise") * tape.noise,
            phase_error: params.float("phase_error").to_radians() * standard.phase_error,
            dot_crawl: percent("dot_crawl"),
            crawl_line_phase: standard.crawl_line_phase,
            wave: percent("wave"),
            line_jitter: percent("line_jitter"),
            head_switching: percent("head_switching"),
            head_lines: params.int("head_lines") as usize,
            tracking: percent("tracking"),
            tracking_height: params.int("tracking_height") as f32,
            dropouts: percent("dropouts"),
        }
    }

    fn line_of(&self, row: i32) -> usize {
        ((row as f32 / self.unit) as usize).min(self.lines - 1)
    }

    fn head_level(&self, line: usize) -> f32 {
        let first = self.lines.saturating_sub(self.head_lines);
        if self.head_lines == 0 || line < first {
            return 0.0;
        }
        (line - first + 1) as f32 / self.head_lines as f32
    }

    fn tracking_level(&self, line: usize) -> f32 {
        if self.tracking <= 0.0 {
            return 0.0;
        }
        let noise = value_noise(self.seed ^ BAND_SALT, line as f32 / self.tracking_height, 0.0);
        let threshold = TRACKING_THRESHOLD - TRACKING_COVERAGE * self.tracking;
        smoothstep(((noise - threshold) / TRACKING_SOFTNESS).clamp(0.0, 1.0))
    }

    fn line_shift(&self, line: usize, head: f32, tracking: f32) -> f32 {
        let row = line as i32;
        let wave = (value_noise(self.seed ^ WAVE_SALT, line as f32 / WAVE_PERIOD, 0.0) - 0.5) * 2.0 * self.wave * WAVE_SHIFT;
        let jitter = signed_unit(self.seed ^ JITTER_SALT, row, 0) * self.line_jitter * JITTER_SHIFT;
        let head_pull = -head * head * self.head_switching * HEAD_SHIFT;
        let head_noise = signed_unit(self.seed ^ HEAD_SALT, row, 0) * head * self.head_switching * HEAD_JITTER;
        let tracking_shift = (signed_unit(self.seed ^ TRACKING_SALT, row, 0) + TRACKING_SKEW) * tracking * self.tracking * TRACKING_SHIFT;
        (wave + jitter + head_pull + head_noise + tracking_shift) * self.unit
    }

    fn read_line(&self, source: &Raster, area: IRect, line: usize) -> Signal {
        let first = (line as f32 * self.unit) as i32;
        let last = (((line + 1) as f32 * self.unit) as i32).max(first + 1).min(self.height as i32);
        let mut sums = vec![[0.0f32; BYTES_PER_PIXEL]; self.width];
        for row in first..last {
            let bytes = source.row_bytes((area.y0 + row) as u32, area.x0 as u32, area.x1 as u32);
            for (sum, pixel) in sums.iter_mut().zip(bytes.chunks_exact(BYTES_PER_PIXEL)) {
                sum.iter_mut().zip(pixel).for_each(|(total, &channel)| *total += f32::from(channel));
            }
        }
        let scale = 1.0 / ((last - first) as f32 * MAX_CHANNEL);
        let mut signal = Signal { luma: Vec::new(), in_phase: Vec::new(), quadrature: Vec::new(), alpha: Vec::new() };
        for sum in sums {
            let [luma, in_phase, quadrature] = to_yiq([sum[0] * scale, sum[1] * scale, sum[2] * scale]);
            signal.luma.push(luma);
            signal.in_phase.push(in_phase);
            signal.quadrature.push(quadrature);
            signal.alpha.push(sum[3] * scale);
        }
        signal
    }

    fn crawl(&self, signal: &Signal, line: usize) -> Vec<f32> {
        let line_phase = line as f32 * self.crawl_line_phase;
        (0..self.width)
            .map(|x| {
                let phase = x as f32 / self.unit * FRAC_PI_2 + line_phase;
                (signal.in_phase[x] * phase.cos() + signal.quadrature[x] * phase.sin()) * self.dot_crawl * CRAWL_GAIN
            })
            .collect()
    }

    fn rotate_phase(&self, signal: &mut Signal, line: usize, head: f32) {
        let drift = (value_noise(self.seed ^ PHASE_SALT, line as f32 / PHASE_PERIOD, 0.0) - 0.5) * 2.0;
        let wobble = signed_unit(self.seed ^ PHASE_SALT, line as i32, 1) * PHASE_JITTER;
        let (sin, cos) = (self.phase_error * (drift + wobble) + head * self.head_switching * HEAD_PHASE_ERROR).sin_cos();
        for (in_phase, quadrature) in signal.in_phase.iter_mut().zip(signal.quadrature.iter_mut()) {
            (*in_phase, *quadrature) = (*in_phase * cos - *quadrature * sin, *in_phase * sin + *quadrature * cos);
        }
    }

    fn drop_out(&self, signal: &mut Signal, line: usize) {
        let mut rng = Rng::new(hash(self.seed ^ DROPOUT_SALT, line as i32, 0));
        if rng.next_unit() >= self.dropouts * DROPOUT_LINE_RATE {
            return;
        }
        let length = ((DROPOUT_MIN_LENGTH + rng.next_unit() * (DROPOUT_MAX_LENGTH - DROPOUT_MIN_LENGTH)) * self.unit) as usize;
        let start = (rng.next_unit() * self.width as f32) as usize;
        let end = (start + length).min(self.width);
        for x in start..end {
            let from_edge = (x - start).min(end - 1 - x) + 1;
            let fade = (from_edge as f32 / (self.unit * DROPOUT_FADE_UNITS)).min(1.0);
            let level = DROPOUT_LEVEL + gaussian_noise(self.seed ^ DROPOUT_SALT, x as i32, line as i32) * DROPOUT_FLICKER;
            signal.luma[x] = lerp(signal.luma[x], level, fade);
            signal.in_phase[x] *= lerp(1.0, DROPOUT_CHROMA_LEFT, fade);
            signal.quadrature[x] *= lerp(1.0, DROPOUT_CHROMA_LEFT, fade);
        }
    }

    fn encode_line(&self, source: &Raster, area: IRect, line: usize, row: &mut [[f32; 4]]) {
        let (head, tracking) = (self.head_level(line), self.tracking_level(line));
        let mut signal = self.read_line(source, area, line);
        signal.shift(self.line_shift(line, head, tracking));
        let crawl = self.crawl(&signal, line);
        self.rotate_phase(&mut signal, line, head);
        let chroma_left = (1.0 - HEAD_CHROMA_LOSS * head) * (1.0 - TRACKING_CHROMA_LOSS * tracking);
        let dim = 1.0 - TRACKING_DIM * tracking;
        lowpass(&mut signal.luma, self.luma_blur, LUMA_POLES);
        if self.sharpen > 0.0 {
            let mut soft = signal.luma.clone();
            lowpass(&mut soft, self.luma_blur * SHARPEN_WIDEN + self.unit, LUMA_POLES);
            signal.luma.iter_mut().zip(&soft).for_each(|(sharp, soft)| *sharp += (*sharp - soft) * self.sharpen * SHARPEN_GAIN);
        }
        signal.in_phase = shifted(&signal.in_phase, self.chroma_shift);
        signal.quadrature = shifted(&signal.quadrature, self.chroma_shift);
        lowpass(&mut signal.in_phase, self.chroma_blur, CHROMA_POLES);
        lowpass(&mut signal.quadrature, self.chroma_blur, CHROMA_POLES);
        let luma_gain = self.luma_noise * LUMA_NOISE_SCALE * (1.0 + TRACKING_NOISE_GAIN * tracking + HEAD_NOISE_GAIN * head);
        let chroma_gain = self.chroma_noise * CHROMA_NOISE_SCALE * (1.0 + TRACKING_NOISE_GAIN * tracking + HEAD_NOISE_GAIN * head);
        let chroma_streak = self.chroma_blur * CHROMA_NOISE_STREAK_RATIO;
        let luma_noise = noise_line(self.seed ^ LUMA_NOISE_SALT, line, 0, self.width, self.unit * LUMA_NOISE_STREAK);
        let in_phase_noise = noise_line(self.seed ^ CHROMA_NOISE_SALT, line, 1, self.width, chroma_streak);
        let quadrature_noise = noise_line(self.seed ^ CHROMA_NOISE_SALT, line, 2, self.width, chroma_streak);
        for x in 0..self.width {
            signal.luma[x] = (signal.luma[x] + crawl[x]) * dim + luma_noise[x] * luma_gain;
            signal.in_phase[x] = signal.in_phase[x] * chroma_left + in_phase_noise[x] * chroma_gain;
            signal.quadrature[x] = signal.quadrature[x] * chroma_left + quadrature_noise[x] * chroma_gain;
        }
        self.drop_out(&mut signal, line);
        for (x, out) in row.iter_mut().enumerate() {
            *out = [signal.luma[x], signal.in_phase[x], signal.quadrature[x], signal.alpha[x]];
        }
    }

    fn decode_line(&self, planes: &[[f32; 4]], line: usize, row: &mut [Rgba]) {
        let at = |index: usize| &planes[index * self.width..(index + 1) * self.width];
        let (previous, current, next) = (at(line.saturating_sub(1)), at(line), at((line + 1).min(self.lines - 1)));
        for (x, out) in row.iter_mut().enumerate() {
            let [luma, _, _, alpha] = current[x];
            let smeared = |channel: usize| {
                let own = current[x][channel];
                lerp(own, (previous[x][channel] + own + next[x][channel]) / 3.0, self.chroma_vertical) * self.saturation
            };
            let graded = (luma - 0.5) * self.contrast + 0.5 + self.brightness;
            *out = to_rgba(graded, smeared(1), smeared(2), alpha);
        }
    }
}

fn render(job: &Job) -> Option<Raster> {
    let vhs = Vhs::new(job);
    let mut planes = vec![[0.0f32; 4]; vhs.lines * vhs.width];
    if !par_rows(&mut planes, vhs.width, job.cancel, |line, row| vhs.encode_line(job.source, job.area, line, row)) {
        return None;
    }
    let mut graded = vec![TRANSPARENT; vhs.lines * vhs.width];
    if !par_rows(&mut graded, vhs.width, job.cancel, |line, row| vhs.decode_line(&planes, line, row)) {
        return None;
    }
    map_area(job.area, job.cancel, |x, y| graded[vhs.line_of(y - job.area.y0) * vhs.width + (x - job.area.x0) as usize])
}
