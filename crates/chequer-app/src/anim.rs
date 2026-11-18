use std::time::{Duration, Instant};

use eframe::egui::Context;

use crate::skin;

const FRAME_SECONDS: f32 = 0.03;
const FRAME_COUNT: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slide {
    In,
    Out,
}

#[derive(Clone, Copy, Debug)]
pub struct Slot {
    pub slide: Slide,
    pub index: usize,
    started: Instant,
}

impl Slot {
    pub fn start(slide: Slide, index: usize) -> Self {
        Self { slide, index, started: Instant::now() }
    }

    pub fn extent(&self, ctx: &Context, full: f32) -> Option<f32> {
        let step = step_of(self.started)?;
        ctx.request_repaint_after(step.until_next);
        let share = if self.slide == Slide::In { step.eased } else { 1.0 - step.eased };
        Some(snap_to_pixels(full * share, ctx.pixels_per_point()))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Glide {
    from: f32,
    to: f32,
    started: Option<Instant>,
}

impl Glide {
    pub const REST: Self = Self { from: 0.0, to: 0.0, started: None };

    pub fn from_offset(offset: f32) -> Self {
        Self { from: offset, to: 0.0, started: Some(Instant::now()) }
    }

    pub fn current(&self) -> f32 {
        let Some(step) = self.started.and_then(step_of) else {
            return self.to;
        };
        self.from + (self.to - self.from) * step.eased
    }

    pub fn value(&self, ctx: &Context) -> f32 {
        if let Some(step) = self.started.and_then(step_of) {
            ctx.request_repaint_after(step.until_next);
        }
        snap_to_pixels(self.current(), ctx.pixels_per_point())
    }

    pub fn retarget(&mut self, target: f32) {
        if self.to != target {
            *self = Self { from: self.current(), to: target, started: Some(Instant::now()) };
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LayerMove {
    pub from: usize,
    pub to: usize,
    pub held_offset: Option<f32>,
}

pub fn apply_layer_move(glides: &mut Vec<Glide>, layer_move: LayerMove, row_height: f32) {
    let LayerMove { from, to, held_offset } = layer_move;
    if from == to || from >= glides.len() || to >= glides.len() {
        return;
    }
    let step = if to > from { row_height } else { -row_height };
    for index in from.min(to)..=from.max(to) {
        glides[index] = Glide::from_offset(glides[index].current() - step);
    }
    let held = Glide::from_offset(held_offset.unwrap_or((to as f32 - from as f32) * row_height));
    glides.remove(from);
    glides.insert(to, held);
}

struct Step {
    eased: f32,
    until_next: Duration,
}

fn step_of(started: Instant) -> Option<Step> {
    let elapsed = started.elapsed().as_secs_f32();
    let frame = (elapsed / FRAME_SECONDS) as u32;
    (frame < FRAME_COUNT).then(|| Step {
        eased: ease_out(frame as f32 / FRAME_COUNT as f32),
        until_next: Duration::from_secs_f32((frame + 1) as f32 * FRAME_SECONDS - elapsed),
    })
}

fn ease_out(progress: f32) -> f32 {
    1.0 - (1.0 - progress).powi(3)
}

pub fn snap_to_pixels(length: f32, ppp: f32) -> f32 {
    let grid = skin::grid_ppp(ppp);
    (length * grid).round() / grid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_out_starts_at_zero_and_front_loads_the_motion() {
        assert_eq!(ease_out(0.0), 0.0);
        assert!(ease_out(0.2) > 0.4);
        assert_eq!(ease_out(1.0), 1.0);
    }

    #[test]
    fn moving_a_layer_up_sends_the_passed_rows_down_and_the_layer_up_from_its_old_place() {
        let mut glides = vec![Glide::REST; 4];
        apply_layer_move(&mut glides, LayerMove { from: 0, to: 2, held_offset: None }, 10.0);
        let offsets: Vec<f32> = glides.iter().map(Glide::current).collect();
        assert_eq!(offsets, vec![-10.0, -10.0, 20.0, 0.0]);
    }

    #[test]
    fn moving_a_layer_down_mirrors_the_offsets() {
        let mut glides = vec![Glide::REST; 3];
        apply_layer_move(&mut glides, LayerMove { from: 2, to: 0, held_offset: Some(7.0) }, 10.0);
        let offsets: Vec<f32> = glides.iter().map(Glide::current).collect();
        assert_eq!(offsets, vec![7.0, 10.0, 10.0]);
    }

    #[test]
    fn lengths_land_on_whole_ui_pixels() {
        assert_eq!(snap_to_pixels(10.4, 1.0), 10.0);
        assert_eq!(snap_to_pixels(10.6, 2.0), 10.5);
    }
}
