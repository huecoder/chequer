use std::f32::consts::{FRAC_PI_2, TAU};
use std::thread;

use crate::effects::params::{choice, float, int};
use crate::effects::util::{Cancel, clamp_channel};
use crate::effects::{Category, EffectDef, Job, ids};
use crate::raster::{BYTES_PER_PIXEL, Raster};

const SCANS: &[&str] = &["stream_rows", "stream_columns"];
const FILTERS: &[&str] = &["filter_off", "lowpass", "highpass", "bandpass"];
const CLIPS: &[&str] = &["clip_soft", "clip_hard", "clip_fold"];
const COLUMNS: usize = 1;
const COLOR_CHANNELS: usize = 3;
const MAX_CHANNEL: f32 = 255.0;
const PERCENT: f32 = 100.0;
const MAX_ECHO_DELAY: i32 = 8192;
const MAX_FLANGER_DEPTH: i32 = 512;
const MAX_FLANGER_PERIOD: i32 = 8192;
const CUTOFF_STEPS: f32 = 1000.0;
const CUTOFF_FLOOR: f32 = 0.001;
const CUTOFF_RANGE: f32 = 0.489;
const DRIVE_PER_PERCENT: f32 = 0.2;
const COMB_DELAYS: [usize; 4] = [1116, 1188, 1277, 1356];
const ALLPASS_DELAYS: [usize; 2] = [556, 441];
const COMB_FEEDBACK: f32 = 0.78;
const ALLPASS_GAIN: f32 = 0.5;
const FULL_BITS: i32 = 8;
const OFF: i32 = 0;

pub const DEF: EffectDef = EffectDef {
    id: ids::SOUND_BEND,
    category: Category::Glitch,
    params: &[
        choice("scan", SCANS, 0),
        int("stereo_spread", 0, 100, 15),
        choice("filter", FILTERS, 0),
        int("cutoff", 1, 1000, 500),
        float("resonance", 0.3, 12.0, 0.7, 0.1),
        int("bits", 1, 8, 8),
        int("downsample", 1, 64, 1),
        int("distortion", 0, 100, 0),
        choice("clip", CLIPS, 0),
        int("flanger_depth", 0, MAX_FLANGER_DEPTH, 0),
        int("flanger_period", 2, MAX_FLANGER_PERIOD, 256),
        int("flanger_mix", 0, 100, 50),
        int("echo_delay", 0, MAX_ECHO_DELAY, 48),
        int("echo_feedback", 0, 95, 55),
        int("echo_mix", 0, 100, 60),
        int("reverb_mix", 0, 100, 0),
        float("reverb_size", 0.1, 8.0, 1.0, 0.1),
    ],
    render,
};

struct Chain {
    filter: usize,
    cutoff: f32,
    resonance: f32,
    bits: i32,
    downsample: usize,
    distortion: f32,
    clip: usize,
    flanger_depth: f32,
    flanger_period: f32,
    flanger_mix: f32,
    echo_delay: f32,
    echo_feedback: f32,
    echo_mix: f32,
    reverb_mix: f32,
    reverb_size: f32,
}

fn lerp(from: f32, to: f32, amount: f32) -> f32 {
    from + (to - from) * amount
}

fn biquad(signal: &mut [f32], filter: usize, cutoff: f32, resonance: f32) {
    let normalized = cutoff / CUTOFF_STEPS;
    let frequency = CUTOFF_FLOOR + CUTOFF_RANGE * normalized * normalized;
    let (sin, cos) = (TAU * frequency).sin_cos();
    let alpha = sin / (2.0 * resonance);
    let (b0, b1, b2) = match filter {
        1 => ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0),
        2 => ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0),
        _ => (alpha, 0.0, -alpha),
    };
    let (a0, a1, a2) = (1.0 + alpha, -2.0 * cos, 1.0 - alpha);
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    for sample in signal.iter_mut() {
        let input = *sample;
        let output = (b0 * input + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2) / a0;
        (x2, x1, y2, y1) = (x1, input, y1, output);
        *sample = output;
    }
}

fn bitcrush(signal: &mut [f32], bits: i32, downsample: usize) {
    let levels = ((1u32 << bits) - 1) as f32;
    let mut held = 0.0;
    for (index, sample) in signal.iter_mut().enumerate() {
        if index % downsample == 0 {
            held = ((*sample * 0.5 + 0.5) * levels).round() / levels * 2.0 - 1.0;
        }
        *sample = held;
    }
}

fn distort(signal: &mut [f32], amount: f32, clip: usize) {
    let drive = 1.0 + amount * DRIVE_PER_PERCENT;
    for sample in signal.iter_mut() {
        *sample = match clip {
            0 => (*sample * drive).tanh() / drive.tanh(),
            1 => (*sample * drive).clamp(-1.0, 1.0),
            _ => (*sample * drive * FRAC_PI_2).sin(),
        };
    }
}

fn flange(signal: &mut [f32], depth: f32, period: f32, mix: f32) {
    let dry = signal.to_vec();
    for (index, sample) in signal.iter_mut().enumerate() {
        let delay = depth * (1.0 + (TAU * index as f32 / period).sin()) * 0.5;
        let position = (index as f32 - delay).max(0.0);
        let base = position.floor() as usize;
        let delayed = lerp(dry[base], dry[(base + 1).min(dry.len() - 1)], position - base as f32);
        *sample = (dry[index] + mix * delayed) / (1.0 + mix);
    }
}

fn echo(signal: &mut [f32], delay: usize, feedback: f32, mix: f32) {
    let dry = signal.to_vec();
    for index in delay..signal.len() {
        signal[index] = dry[index] + feedback * signal[index - delay];
    }
    for (sample, &original) in signal.iter_mut().zip(&dry) {
        *sample = lerp(original, *sample * (1.0 - feedback), mix);
    }
}

fn scaled_delay(base: usize, size: f32) -> usize {
    ((base as f32 * size).round() as usize).max(1)
}

fn reverb(signal: &mut [f32], size: f32, mix: f32) {
    let dry = signal.to_vec();
    let mut wet = vec![0.0; dry.len()];
    for base in COMB_DELAYS {
        let delay = scaled_delay(base, size);
        let mut comb = dry.clone();
        for index in delay..comb.len() {
            comb[index] = dry[index] + COMB_FEEDBACK * comb[index - delay];
        }
        wet.iter_mut().zip(&comb).for_each(|(sum, value)| *sum += value * (1.0 - COMB_FEEDBACK) / COMB_DELAYS.len() as f32);
    }
    for base in ALLPASS_DELAYS {
        let delay = scaled_delay(base, size);
        let input = wet.clone();
        for index in 0..wet.len() {
            let (earlier_in, earlier_out) = if index >= delay { (input[index - delay], wet[index - delay]) } else { (0.0, 0.0) };
            wet[index] = -ALLPASS_GAIN * input[index] + earlier_in + ALLPASS_GAIN * earlier_out;
        }
    }
    for ((sample, &original), &tail) in signal.iter_mut().zip(&dry).zip(&wet) {
        *sample = lerp(original, tail, mix);
    }
}

impl Chain {
    fn from_job(job: &Job) -> Chain {
        let params = job.params;
        let percent = |key: &str| params.int(key) as f32 / PERCENT;
        Chain {
            filter: params.choice("filter"),
            cutoff: params.int("cutoff") as f32,
            resonance: params.float("resonance"),
            bits: params.int("bits"),
            downsample: params.int("downsample") as usize,
            distortion: params.int("distortion") as f32,
            clip: params.choice("clip"),
            flanger_depth: params.int("flanger_depth") as f32,
            flanger_period: params.int("flanger_period") as f32,
            flanger_mix: percent("flanger_mix"),
            echo_delay: params.int("echo_delay") as f32,
            echo_feedback: percent("echo_feedback"),
            echo_mix: percent("echo_mix"),
            reverb_mix: percent("reverb_mix"),
            reverb_size: params.float("reverb_size"),
        }
    }

    fn apply(&self, signal: &mut [f32], channel_factor: f32, cancel: &Cancel) {
        if self.filter != OFF as usize {
            biquad(signal, self.filter, self.cutoff, self.resonance);
        }
        if self.bits < FULL_BITS || self.downsample > 1 {
            bitcrush(signal, self.bits, self.downsample);
        }
        if self.distortion > 0.0 {
            distort(signal, self.distortion, self.clip);
        }
        if cancel.is_set() {
            return;
        }
        if self.flanger_depth > 0.0 && self.flanger_mix > 0.0 {
            flange(signal, self.flanger_depth, self.flanger_period * channel_factor, self.flanger_mix);
        }
        let delay = (self.echo_delay * channel_factor).round() as usize;
        if delay > 0 && self.echo_mix > 0.0 {
            echo(signal, delay, self.echo_feedback, self.echo_mix);
        }
        if cancel.is_set() {
            return;
        }
        if self.reverb_mix > 0.0 {
            reverb(signal, self.reverb_size * channel_factor, self.reverb_mix);
        }
    }
}

fn render(job: &Job) -> Option<Raster> {
    let region = job.source.crop(job.area);
    let (width, height) = (region.width() as usize, region.height() as usize);
    let length = width * height;
    let by_columns = job.params.choice("scan") == COLUMNS;
    let pixel_at = |sample: usize| if by_columns { (sample % height) * width + sample / height } else { sample };
    let source = region.as_bytes();
    let mut streams = vec![0.0f32; length * COLOR_CHANNELS];
    for (channel, stream) in streams.chunks_mut(length).enumerate() {
        for (sample, value) in stream.iter_mut().enumerate() {
            *value = f32::from(source[pixel_at(sample) * BYTES_PER_PIXEL + channel]) / MAX_CHANNEL * 2.0 - 1.0;
        }
    }
    let chain = Chain::from_job(job);
    let spread = job.params.int("stereo_spread") as f32 / PERCENT;
    thread::scope(|scope| {
        for (channel, stream) in streams.chunks_mut(length).enumerate() {
            let (chain, cancel) = (&chain, job.cancel);
            scope.spawn(move || chain.apply(stream, 1.0 + (channel as f32 - 1.0) * spread, cancel));
        }
    });
    if job.cancel.is_set() {
        return None;
    }
    let mut data = source.to_vec();
    for (channel, stream) in streams.chunks(length).enumerate() {
        for (sample, &value) in stream.iter().enumerate() {
            data[pixel_at(sample) * BYTES_PER_PIXEL + channel] = clamp_channel((value * 0.5 + 0.5) * MAX_CHANNEL);
        }
    }
    Raster::from_rgba(region.width(), region.height(), data).ok()
}

