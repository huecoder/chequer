use std::sync::OnceLock;
use std::time::Instant;

const MILLIS_PER_SECOND: f64 = 1000.0;

static START: OnceLock<Instant> = OnceLock::new();

pub fn now_ms() -> f64 {
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * MILLIS_PER_SECOND
}
