use std::time::Duration;
use std::time::Instant;

/// Reports a value only after it has stayed unchanged for `window`.
///
/// The debouncer never reads the clock or sleeps. The caller passes `now` in,
/// asks `deadline()` when to wake up, and calls `on_timeout` at that time.
pub struct Debouncer<T> {
    reported: Option<T>,
    candidate: Option<T>,
    window: Duration,
    deadline: Option<Instant>,
}

impl<T: PartialEq + Clone> Debouncer<T> {
    /// Creates a debouncer with nothing reported yet.
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            reported: None,
            candidate: None,
            deadline: None,
        }
    }

    /// Records a reading. Restarts the window only if it differs from both
    /// the pending candidate and the last reported value.
    pub fn on_reading(&mut self, value: T, now: Instant) {
        if self.candidate.as_ref() == Some(&value) {
            return;
        }
        if self.reported.as_ref() == Some(&value) {
            self.candidate = None;
            self.deadline = None;
            return;
        }

        self.candidate = Some(value);
        self.deadline = Some(now + self.window);
    }

    /// When the caller should call `on_timeout`, or `None` if nothing is pending.
    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Returns the candidate once its window has elapsed, and marks it reported.
    pub fn on_timeout(&mut self, now: Instant) -> Option<T> {
        match self.deadline {
            None => None,
            Some(d) if now < d => None,
            Some(_) => {
                self.reported = self.candidate.take();
                self.deadline = None;
                self.reported.clone()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Duration = Duration::from_secs(3);

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn reports_after_window() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(WINDOW);
        d.on_reading("a", t0);
        assert_eq!(d.on_timeout(t0 + secs(2)), None);
        assert_eq!(d.on_timeout(t0 + secs(3)), Some("a"));
        assert_eq!(d.on_timeout(t0 + secs(4)), None);
    }

    #[test]
    fn flap_back_to_reported_value_reports_nothing() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(WINDOW);
        d.on_reading("full", t0);
        assert_eq!(d.on_timeout(t0 + secs(3)), Some("full"));

        d.on_reading("offline", t0 + secs(10));
        d.on_reading("full", t0 + secs(11));
        assert_eq!(d.deadline(), None);
        assert_eq!(d.on_timeout(t0 + secs(20)), None);
    }

    #[test]
    fn repeated_reading_does_not_restart_the_window() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(WINDOW);
        d.on_reading("a", t0);
        d.on_reading("a", t0 + secs(2));
        assert_eq!(d.deadline(), Some(t0 + WINDOW));
        assert_eq!(d.on_timeout(t0 + secs(3)), Some("a"));
    }

    #[test]
    fn new_value_restarts_the_window() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(WINDOW);
        d.on_reading("a", t0);
        d.on_reading("b", t0 + secs(2));
        assert_eq!(d.on_timeout(t0 + secs(3)), None);
        assert_eq!(d.on_timeout(t0 + secs(5)), Some("b"));
    }

    #[test]
    fn same_value_is_never_reported_twice() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(WINDOW);
        d.on_reading("a", t0);
        assert_eq!(d.on_timeout(t0 + secs(3)), Some("a"));
        d.on_reading("a", t0 + secs(5));
        assert_eq!(d.deadline(), None);
    }
}
