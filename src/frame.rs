use std::hint::spin_loop;
use std::thread;
use std::time::{Duration, Instant};

pub const TICK_STEP: Duration = Duration::from_millis(50);
pub const MAX_TICKS_PER_FRAME: u32 = 5;
const SPIN_WINDOW: Duration = Duration::from_millis(1);

#[derive(Clone, Copy, Debug)]
pub struct StepResult {
    pub steps: u32,
    pub alpha: f32,
}

pub struct FixedStep {
    step: Duration,
    max_steps: u32,
    accumulator: Duration,
}

impl FixedStep {
    pub fn new(step: Duration, max_steps: u32) -> Self {
        Self {
            step,
            max_steps,
            accumulator: Duration::ZERO,
        }
    }

    pub fn advance(&mut self, elapsed: Duration) -> StepResult {
        self.accumulator += elapsed;
        let step_nanos = self.step.as_nanos().max(1);
        let mut steps = (self.accumulator.as_nanos() / step_nanos) as u32;
        if steps > self.max_steps {
            steps = self.max_steps;
            self.accumulator = self.step * self.max_steps;
        }
        self.accumulator = self.accumulator.saturating_sub(self.step * steps);
        let alpha = self.accumulator.as_nanos() as f32 / step_nanos as f32;
        StepResult { steps, alpha }
    }

    #[cfg(test)]
    pub fn accumulator(&self) -> Duration {
        self.accumulator
    }
}

impl Default for FixedStep {
    fn default() -> Self {
        Self::new(TICK_STEP, MAX_TICKS_PER_FRAME)
    }
}

pub struct FramePacer {
    interval: Option<Duration>,
    last_frame: Option<Instant>,
}

impl FramePacer {
    pub fn new(limit_fps: u32) -> Self {
        Self {
            interval: interval_for(limit_fps),
            last_frame: None,
        }
    }

    pub fn set_limit(&mut self, limit_fps: u32) {
        self.interval = interval_for(limit_fps);
        self.last_frame = None;
    }

    pub fn begin_frame(&mut self, now: Instant) {
        self.last_frame = Some(now);
    }

    pub fn wait_time(&self, now: Instant) -> Duration {
        match (self.interval, self.last_frame) {
            (Some(interval), Some(last)) => (last + interval).saturating_duration_since(now),
            _ => Duration::ZERO,
        }
    }
}

fn interval_for(limit_fps: u32) -> Option<Duration> {
    if limit_fps == 0 {
        None
    } else {
        Some(Duration::from_secs_f64(1.0 / f64::from(limit_fps)))
    }
}

pub fn wait_precise(wait: Duration) {
    if wait.is_zero() {
        return;
    }
    let deadline = Instant::now() + wait;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining > SPIN_WINDOW {
            thread::sleep(remaining - SPIN_WINDOW);
        } else if remaining.is_zero() {
            break;
        } else {
            spin_loop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn two_ticks_and_remainder() {
        let mut fixed = FixedStep::default();
        let result = fixed.advance(ms(120));
        assert_eq!(result.steps, 2);
        assert_eq!(fixed.accumulator(), ms(20));
        assert!((result.alpha - 0.4).abs() < f32::EPSILON);
    }

    #[test]
    fn caps_ticks_and_drops_debt() {
        let mut fixed = FixedStep::default();
        let result = fixed.advance(Duration::from_secs(10));
        assert_eq!(result.steps, MAX_TICKS_PER_FRAME);
        assert!(fixed.accumulator() < TICK_STEP);
        assert!((0.0..1.0).contains(&result.alpha));
    }

    #[test]
    fn alpha_stays_in_range() {
        let mut fixed = FixedStep::default();
        for step in 0..200 {
            let result = fixed.advance(ms(step % 47));
            assert!((0.0..1.0).contains(&result.alpha));
        }
    }

    #[test]
    fn pacer_unlimited_has_no_wait() {
        let pacer = FramePacer::new(0);
        let now = Instant::now();
        assert_eq!(pacer.wait_time(now), Duration::ZERO);
    }

    #[test]
    fn pacer_60_targets_one_sixtieth() {
        let mut pacer = FramePacer::new(60);
        let now = Instant::now();
        pacer.begin_frame(now);
        let wait = pacer.wait_time(now);
        let expected = Duration::from_secs_f64(1.0 / 60.0);
        assert_eq!(wait, expected);
        assert!((wait.as_secs_f64() - 0.016_666).abs() < 0.001);
    }

    #[test]
    fn pacer_late_frame_has_no_wait_and_no_debt() {
        let mut pacer = FramePacer::new(60);
        let start = Instant::now();
        pacer.begin_frame(start);
        assert_eq!(pacer.wait_time(start + ms(50)), Duration::ZERO);
        pacer.begin_frame(start + ms(50));
        assert_eq!(
            pacer.wait_time(start + ms(50)),
            Duration::from_secs_f64(1.0 / 60.0)
        );
    }
}
