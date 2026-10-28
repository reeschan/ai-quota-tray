use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct LoopState {
    pub failures: u32,
    pub next_due: Instant,
    pub last_fetch: Option<Instant>,
}

impl LoopState {
    pub fn new(now: Instant) -> Self {
        Self {
            failures: 0,
            next_due: now,
            last_fetch: None,
        }
    }

    pub fn due(&self, now: Instant) -> bool {
        now >= self.next_due
    }

    pub fn succeed(&mut self, now: Instant, interval: Duration) {
        self.failures = 0;
        self.last_fetch = Some(now);
        self.next_due = now + interval;
    }

    pub fn fail(&mut self, now: Instant, base: Duration, max: Duration) {
        self.failures = self.failures.saturating_add(1);
        self.last_fetch = Some(now);
        self.next_due = now + backoff_delay(self.failures, base, max);
    }

    pub fn kick(&mut self, now: Instant) {
        let cooled_down = self
            .last_fetch
            .map(|then| now.saturating_duration_since(then) >= Duration::from_secs(2))
            .unwrap_or(true);
        self.next_due = if cooled_down {
            now
        } else {
            now + Duration::from_secs(2)
        };
    }
}

pub fn backoff_delay(failures: u32, base: Duration, max: Duration) -> Duration {
    if failures == 0 {
        return base.min(max);
    }
    let shift = failures.saturating_sub(1).min(6);
    let factor = 1u32 << shift;
    base.saturating_mul(factor)
        .min(max)
        .max(Duration::from_secs(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps() {
        let base = Duration::from_secs(30);
        let max = Duration::from_secs(100);
        assert_eq!(backoff_delay(1, base, max), Duration::from_secs(30));
        assert_eq!(backoff_delay(2, base, max), Duration::from_secs(60));
        assert_eq!(backoff_delay(3, base, max), Duration::from_secs(100));
        assert_eq!(backoff_delay(8, base, max), Duration::from_secs(100));
    }

    #[test]
    fn success_resets_failures_and_kick_debounces() {
        let start = Instant::now();
        let mut state = LoopState::new(start);
        state.fail(start, Duration::from_secs(30), Duration::from_secs(600));
        assert_eq!(state.failures, 1);
        state.succeed(start, Duration::from_secs(600));
        assert_eq!(state.failures, 0);
        assert!(!state.due(start));
        state.kick(start);
        assert!(!state.due(start));
        assert!(state.due(start + Duration::from_secs(2)));
    }
}
