use std::{time::{Duration, Instant}};





#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub struct ExponentialBackoff {
    reset_val: Duration,
    backoff_factor: u32,
    timeout: Duration,
    started: Option<Instant>,
}

impl ExponentialBackoff {
    pub fn new(backoff_factor: u32, next_timeout: Duration) -> Self {
        ExponentialBackoff {
            backoff_factor,
            timeout: next_timeout * backoff_factor,
            reset_val: next_timeout,
            started: None,
        }
    }

    // reset backoff
    pub fn reset(&mut self) {
        self.timeout = self.reset_val;
    }

    pub fn start_next(&mut self, result: bool) {
        // success
        if result {
            self.timeout = self.reset_val;
            self.started = Some(Instant::now());
        }
        // failure
        else {
            self.timeout *= self.backoff_factor;
            self.started = Some(Instant::now());
        }
    }

    pub fn is_expired(&self) -> bool {
        if let Some(then) = self.started {
            Instant::now() - then > self.timeout
        } else {
            true
        }
    }
}

impl Default for ExponentialBackoff {
    fn default() -> Self {
        Self {
            reset_val: Duration::from_millis(200),
            backoff_factor: 2,
            timeout: Duration::from_millis(200) * 2,
            started: None
        }
    }
}
