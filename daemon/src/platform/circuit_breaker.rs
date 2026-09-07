//! Circuit breaker and exponential backoff implementation for resilient background tasks.

use std::time::{Duration, Instant};

/// Exponential backoff manager with failure tracking and automatic reset after sustained success.
#[derive(Debug, Clone)]
pub struct ExponentialBackoff {
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub factor: f64,
    pub current_delay: Duration,
    pub last_attempt: Option<Instant>,
    pub reset_after: Duration,
    pub failure_count: u32,
}

impl ExponentialBackoff {
    /// Creates a new `ExponentialBackoff` with the specified delay bounds, multiplier, and reset timeout.
    pub fn new(initial: Duration, max: Duration, factor: f64, reset_after: Duration) -> Self {
        let initial_delay = initial.min(max);
        Self {
            initial_delay,
            max_delay: max,
            factor: if factor > 1.0 { factor } else { 1.5 },
            current_delay: initial_delay,
            last_attempt: None,
            reset_after,
            failure_count: 0,
        }
    }

    /// Records a failure, advances the backoff delay, and returns the duration to wait.
    pub fn record_failure(&mut self) -> Duration {
        if let Some(last) = self.last_attempt
            && last.elapsed() >= self.reset_after
        {
            self.failure_count = 0;
            self.current_delay = self.initial_delay;
        }

        self.last_attempt = Some(Instant::now());

        if self.failure_count == 0 {
            self.failure_count = 1;
            self.current_delay = self.initial_delay;
        } else {
            self.failure_count = self.failure_count.saturating_add(1);
            let next_secs = self.current_delay.as_secs_f64() * self.factor;
            let next_delay = Duration::from_secs_f64(next_secs);
            self.current_delay = next_delay.min(self.max_delay);
        }

        self.current_delay
    }

    /// Resets the failure count and backoff delay to their initial healthy state.
    pub fn record_success(&mut self) {
        self.failure_count = 0;
        self.current_delay = self.initial_delay;
        self.last_attempt = Some(Instant::now());
    }

    /// Records a failure and sleeps the current thread for the computed backoff duration.
    pub fn wait(&mut self) {
        let delay = self.record_failure();
        std::thread::sleep(delay);
    }
}

impl Default for ExponentialBackoff {
    fn default() -> Self {
        Self::new(
            Duration::from_millis(50),
            Duration::from_secs(10),
            2.0,
            Duration::from_secs(30),
        )
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_backoff_defaults() {
        let backoff = ExponentialBackoff::default();
        assert_eq!(backoff.initial_delay, Duration::from_millis(50));
        assert_eq!(backoff.max_delay, Duration::from_secs(10));
        assert_eq!(backoff.factor, 2.0);
        assert_eq!(backoff.reset_after, Duration::from_secs(30));
        assert_eq!(backoff.failure_count, 0);
        assert_eq!(backoff.current_delay, Duration::from_millis(50));
        assert!(backoff.last_attempt.is_none());
    }

    #[test]
    fn test_backoff_progression() {
        let mut backoff = ExponentialBackoff::new(
            Duration::from_millis(50),
            Duration::from_secs(10),
            2.0,
            Duration::from_secs(30),
        );

        let d1 = backoff.record_failure();
        assert_eq!(d1, Duration::from_millis(50));
        assert_eq!(backoff.failure_count, 1);

        let d2 = backoff.record_failure();
        assert_eq!(d2, Duration::from_millis(100));
        assert_eq!(backoff.failure_count, 2);

        let d3 = backoff.record_failure();
        assert_eq!(d3, Duration::from_millis(200));
        assert_eq!(backoff.failure_count, 3);

        let d4 = backoff.record_failure();
        assert_eq!(d4, Duration::from_millis(400));
        assert_eq!(backoff.failure_count, 4);
    }

    #[test]
    fn test_backoff_capping() {
        let mut backoff = ExponentialBackoff::new(
            Duration::from_millis(100),
            Duration::from_millis(350),
            2.0,
            Duration::from_secs(30),
        );

        let d1 = backoff.record_failure();
        assert_eq!(d1, Duration::from_millis(100));

        let d2 = backoff.record_failure();
        assert_eq!(d2, Duration::from_millis(200));

        let d3 = backoff.record_failure();
        assert_eq!(d3, Duration::from_millis(350));

        let d4 = backoff.record_failure();
        assert_eq!(d4, Duration::from_millis(350));
        assert_eq!(backoff.failure_count, 4);
    }

    #[test]
    fn test_backoff_record_success() {
        let mut backoff = ExponentialBackoff::default();
        backoff.record_failure();
        backoff.record_failure();
        assert_eq!(backoff.failure_count, 2);

        backoff.record_success();
        assert_eq!(backoff.failure_count, 0);
        assert_eq!(backoff.current_delay, Duration::from_millis(50));

        let next = backoff.record_failure();
        assert_eq!(next, Duration::from_millis(50));
        assert_eq!(backoff.failure_count, 1);
    }

    #[test]
    fn test_backoff_reset_after_duration() {
        let mut backoff = ExponentialBackoff::new(
            Duration::from_millis(10),
            Duration::from_millis(200),
            2.0,
            Duration::from_millis(40),
        );

        let d1 = backoff.record_failure();
        assert_eq!(d1, Duration::from_millis(10));

        let d2 = backoff.record_failure();
        assert_eq!(d2, Duration::from_millis(20));

        std::thread::sleep(Duration::from_millis(50));

        let d3 = backoff.record_failure();
        assert_eq!(d3, Duration::from_millis(10));
        assert_eq!(backoff.failure_count, 1);
    }

    #[test]
    fn test_backoff_wait_sleeps() {
        let mut backoff = ExponentialBackoff::new(
            Duration::from_millis(15),
            Duration::from_millis(100),
            2.0,
            Duration::from_secs(10),
        );

        let start = Instant::now();
        backoff.wait();
        let elapsed = start.elapsed();

        assert!(elapsed >= Duration::from_millis(14));
        assert_eq!(backoff.failure_count, 1);
    }
}
