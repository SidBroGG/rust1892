pub const WINDOW_FRAMES: usize = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Input,
    Tick,
    RenderEncode,
    SubmitPresent,
    PaceWait,
}

pub const STAGE_COUNT: usize = 5;

impl Stage {
    pub const ALL: [Stage; STAGE_COUNT] = [
        Stage::Input,
        Stage::Tick,
        Stage::RenderEncode,
        Stage::SubmitPresent,
        Stage::PaceWait,
    ];

    pub fn index(self) -> usize {
        match self {
            Stage::Input => 0,
            Stage::Tick => 1,
            Stage::RenderEncode => 2,
            Stage::SubmitPresent => 3,
            Stage::PaceWait => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Stage::Input => "input",
            Stage::Tick => "tick",
            Stage::RenderEncode => "render_encode",
            Stage::SubmitPresent => "submit_present",
            Stage::PaceWait => "pace_wait",
        }
    }
}

pub struct RingBuffer {
    values: Vec<f32>,
    len: usize,
    next: usize,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            values: vec![0.0; capacity],
            len: 0,
            next: 0,
        }
    }

    pub fn push(&mut self, value: f32) {
        let capacity = self.values.len();
        if capacity == 0 {
            return;
        }
        self.values[self.next] = value;
        self.next = (self.next + 1) % capacity;
        if self.len < capacity {
            self.len += 1;
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn iter(&self) -> impl Iterator<Item = f32> + '_ {
        let capacity = self.values.len();
        let start = if self.len == capacity { self.next } else { 0 };
        (0..self.len).map(move |offset| self.values[(start + offset) % capacity])
    }

    pub fn average(&self) -> f32 {
        if self.len == 0 {
            0.0
        } else {
            self.iter().sum::<f32>() / self.len as f32
        }
    }

    pub fn sorted_into(&self, out: &mut Vec<f32>) {
        out.clear();
        out.extend(self.iter());
        out.sort_unstable_by(|a, b| a.total_cmp(b));
    }
}

pub fn percentile_sorted(sorted: &[f32], percentile: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((percentile / 100.0) * sorted.len() as f32).ceil() as usize;
    let index = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[index]
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MetricsSnapshot {
    pub fps: f32,
    pub frametime_avg: f32,
    pub frametime_p95: f32,
    pub frametime_p99: f32,
    pub stage_avg: [f32; STAGE_COUNT],
}

pub struct Metrics {
    frames: RingBuffer,
    stages: [RingBuffer; STAGE_COUNT],
    scratch: Vec<f32>,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            frames: RingBuffer::new(WINDOW_FRAMES),
            stages: std::array::from_fn(|_| RingBuffer::new(WINDOW_FRAMES)),
            scratch: Vec::with_capacity(WINDOW_FRAMES),
        }
    }

    pub fn record_frame(&mut self, frametime_ms: f32, stage_ms: [f32; STAGE_COUNT]) {
        self.frames.push(frametime_ms);
        for (buffer, value) in self.stages.iter_mut().zip(stage_ms) {
            buffer.push(value);
        }
    }

    pub fn snapshot(&mut self) -> MetricsSnapshot {
        let frametime_avg = self.frames.average();
        self.frames.sorted_into(&mut self.scratch);
        let frametime_p95 = percentile_sorted(&self.scratch, 95.0);
        let frametime_p99 = percentile_sorted(&self.scratch, 99.0);
        let fps = if frametime_avg > 0.0 {
            1000.0 / frametime_avg
        } else {
            0.0
        };
        let stage_avg = std::array::from_fn(|index| self.stages[index].average());
        MetricsSnapshot {
            fps,
            frametime_avg,
            frametime_p95,
            frametime_p99,
            stage_avg,
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_match_reference() {
        let mut metrics = Metrics::new();
        for value in 1..=100 {
            metrics.record_frame(value as f32, [0.0; STAGE_COUNT]);
        }
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.frametime_p95, 95.0);
        assert_eq!(snapshot.frametime_p99, 99.0);
        assert!((snapshot.frametime_avg - 50.5).abs() < 0.001);
        assert!((snapshot.fps - 1000.0 / 50.5).abs() < 0.001);
    }

    #[test]
    fn window_overwrites_oldest_values() {
        let mut buffer = RingBuffer::new(3);
        for value in 1..=4 {
            buffer.push(value as f32);
        }
        assert_eq!(buffer.len(), 3);
        let values: Vec<f32> = buffer.iter().collect();
        assert_eq!(values, vec![2.0, 3.0, 4.0]);
    }

    #[test]
    fn stage_averages_are_tracked() {
        let mut metrics = Metrics::new();
        metrics.record_frame(10.0, [1.0, 2.0, 3.0, 4.0, 5.0]);
        metrics.record_frame(20.0, [3.0, 4.0, 5.0, 6.0, 7.0]);
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.stage_avg, [2.0, 3.0, 4.0, 5.0, 6.0]);
        assert!((snapshot.frametime_avg - 15.0).abs() < 0.001);
    }

    #[test]
    fn empty_metrics_report_zero() {
        let mut metrics = Metrics::new();
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.fps, 0.0);
        assert_eq!(snapshot.frametime_p95, 0.0);
    }

    #[test]
    fn stable_sort_handles_nan() {
        let mut buffer = RingBuffer::new(4);
        buffer.push(3.0);
        buffer.push(f32::NAN);
        buffer.push(1.0);
        buffer.push(2.0);
        let mut sorted = Vec::new();
        buffer.sorted_into(&mut sorted);
        assert_eq!(sorted.len(), 4);
        assert!((sorted[0] - 1.0).abs() < f32::EPSILON);
        assert!((sorted[2] - 3.0).abs() < f32::EPSILON);
        assert!(sorted[3].is_nan());
    }
}
