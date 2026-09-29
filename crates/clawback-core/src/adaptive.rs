//! Conservative hill-climbing for directory-scan concurrency.
//! The controller sees aggregated counters once per second, never file events.
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StorageKind {
    #[default]
    Unknown,
    Rotational,
    SolidState,
}

impl StorageKind {
    pub(crate) fn initial_workers(self) -> usize {
        if self == Self::SolidState { 2 } else { 1 }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Sample {
    pub entries: u64,
    pub work_nanos: u64,
    pub elapsed: Duration,
    /// Only compare windows with enough queued work to use every enabled worker.
    pub saturated: bool,
}

#[derive(Clone, Copy, Default)]
struct Window {
    entries: u64,
    work_nanos: u64,
    seconds: f64,
    samples: usize,
}
impl Window {
    fn add(&mut self, sample: &Sample) {
        self.entries += sample.entries;
        self.work_nanos += sample.work_nanos;
        self.seconds += sample.elapsed.as_secs_f64();
        self.samples += 1;
    }
    fn rate(self) -> f64 {
        self.entries as f64 / self.seconds.max(0.001)
    }
    fn latency(self) -> f64 {
        self.work_nanos as f64 / self.entries.max(1) as f64
    }
}

#[derive(Clone, Copy)]
struct Trial {
    previous: usize,
    baseline: Window,
    settling: bool,
}

pub(crate) struct Controller {
    limit: usize,
    maximum: usize,
    window: Window,
    trial: Option<Trial>,
    shrink_next: bool,
    cooldown: usize,
    rejected: usize,
}

impl Controller {
    pub fn new(storage: StorageKind, maximum: usize) -> Self {
        Self {
            limit: storage.initial_workers().min(maximum.max(1)),
            maximum: maximum.max(1),
            window: Window::default(),
            trial: None,
            shrink_next: false,
            cooldown: 0,
            rejected: 0,
        }
    }
    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn restrict(&mut self, available: usize) {
        self.maximum = available.max(1);
        self.limit = self.limit.min(self.maximum);
        self.trial = None;
        self.window = Window::default();
    }

    pub fn observe(&mut self, sample: Sample) -> usize {
        self.cooldown = self.cooldown.saturating_sub(1);
        if !sample.saturated || sample.entries < 8 || sample.work_nanos == 0 {
            // An empty queue or a tiny sample cannot prove a concurrency gain.
            if let Some(trial) = self.trial.take() {
                self.limit = trial.previous;
                self.cooldown = 2;
            }
            self.window = Window::default();
            return self.limit;
        }
        if let Some(trial) = &mut self.trial
            && trial.settling
        {
            trial.settling = false;
            return self.limit;
        }
        self.window.add(&sample);
        if self.window.samples < 2 {
            return self.limit;
        }
        let measured = std::mem::take(&mut self.window);
        if let Some(trial) = self.trial.take() {
            let growth = self.limit > trial.previous;
            let rate = measured.rate() / trial.baseline.rate().max(1.0);
            let latency = measured.latency() / trial.baseline.latency().max(1.0);
            let worthwhile = if growth {
                rate >= 1.10 && latency <= 1.50
            } else {
                rate >= 0.99 || (rate >= 0.95 && latency <= 0.80)
            };
            if worthwhile {
                self.rejected = 0;
                self.cooldown = 2;
                self.shrink_next = !growth;
            } else {
                self.limit = trial.previous;
                self.rejected = (self.rejected + 1).min(5);
                self.cooldown = (self.rejected * 6).min(30);
                self.shrink_next = growth;
            }
        } else if self.cooldown == 0 && self.maximum > 1 {
            let previous = self.limit;
            if (self.shrink_next || self.limit == self.maximum) && self.limit > 1 {
                self.limit -= 1;
            } else if self.limit < self.maximum {
                // Small proportional trials can still clear the gain threshold
                // at high concurrency; a +1 trial at 16 workers cannot yield
                // 10% even with perfect linear scaling.
                self.limit = (self.limit + (self.limit / 4).max(1)).min(self.maximum);
            }
            if self.limit != previous {
                self.trial = Some(Trial { previous, baseline: measured, settling: true });
            }
        }
        self.limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(rate: u64, latency: u64) -> Sample {
        Sample { entries: rate, work_nanos: rate * latency, elapsed: Duration::from_secs(1), saturated: true }
    }
    #[test]
    fn disk_hint_only_sets_the_starting_point() {
        assert_eq!(Controller::new(StorageKind::Unknown, 16).limit(), 1);
        assert_eq!(Controller::new(StorageKind::Rotational, 16).limit(), 1);
        assert_eq!(Controller::new(StorageKind::SolidState, 16).limit(), 2);
        assert_eq!(Controller::new(StorageKind::SolidState, 1).limit(), 1);
    }
    #[test]
    fn harmful_growth_is_reverted_and_retries_are_spaced_out() {
        let mut controller = Controller::new(StorageKind::Unknown, 8);
        controller.observe(sample(1000, 1000));
        assert_eq!(controller.observe(sample(1000, 1000)), 2);
        controller.observe(sample(600, 3000)); // settling
        controller.observe(sample(600, 3000));
        assert_eq!(controller.observe(sample(600, 3000)), 1);
        for _ in 0..5 {
            assert_eq!(controller.observe(sample(1000, 1000)), 1);
        }
    }
    #[test]
    fn throughput_gain_with_excessive_latency_is_rejected() {
        let mut controller = Controller::new(StorageKind::Unknown, 8);
        for _ in 0..2 {
            controller.observe(sample(1000, 1000));
        }
        for _ in 0..3 {
            controller.observe(sample(1200, 2000));
        }
        assert_eq!(controller.limit(), 1);
    }
    #[test]
    fn finds_plateau_and_adapts_when_storage_gets_slower() {
        let mut controller = Controller::new(StorageKind::SolidState, 8);
        for _ in 0..120 {
            let workers = controller.limit() as u64;
            controller.observe(sample(1000 * workers.min(4), 1000 * workers.div_ceil(4)));
        }
        assert!((3..=5).contains(&controller.limit()), "settles near the four-worker plateau");
        for _ in 0..180 {
            let workers = controller.limit() as u64;
            controller.observe(sample(1000 / workers, 1000 * workers * workers));
        }
        assert!(controller.limit() <= 2, "backs off when additional workers now hurt");
    }
    #[test]
    fn idle_and_small_samples_do_not_raise_concurrency() {
        let mut controller = Controller::new(StorageKind::Unknown, 32);
        for _ in 0..100 {
            let mut empty_queue = sample(100_000, 1000);
            empty_queue.saturated = false;
            assert_eq!(controller.observe(empty_queue), 1);
            assert_eq!(controller.observe(sample(1, 1000)), 1);
        }
    }

    #[test]
    fn beneficial_growth_stays_bounded_and_empty_queue_aborts_trial() {
        let mut controller = Controller::new(StorageKind::Unknown, 2);
        for _ in 0..2 {
            controller.observe(sample(1000, 1000));
        }
        for _ in 0..3 {
            controller.observe(sample(1900, 1050));
        }
        assert_eq!(controller.limit(), 2);
        for _ in 0..100 {
            let rate = 1000 * controller.limit() as u64;
            assert!(controller.observe(sample(rate, 1000)) <= 2);
        }
        let mut controller = Controller::new(StorageKind::Unknown, 8);
        for _ in 0..2 {
            controller.observe(sample(1000, 1000));
        }
        assert_eq!(controller.limit(), 2);
        let mut tail = sample(1000, 1000);
        tail.saturated = false;
        assert_eq!(controller.observe(tail), 1);
    }

    #[test]
    fn linear_scaling_can_reach_the_capacity_limit() {
        let mut controller = Controller::new(StorageKind::SolidState, 16);
        for _ in 0..240 {
            let workers = controller.limit() as u64;
            controller.observe(sample(1000 * workers, 1000));
        }
        assert!((15..=16).contains(&controller.limit()), "the gain threshold must not impose an accidental low cap");
    }
}
