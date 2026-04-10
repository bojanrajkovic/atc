//! Clock abstraction for time-dependent operations.
//!
//! Provides a [`Clock`] trait that abstracts time access, with
//! [`SystemClock`] for production and [`TestClock`] for deterministic
//! testing without sleeps.

use chrono::{DateTime, Utc};
#[cfg(any(test, feature = "test-support"))]
use chrono::TimeDelta;
#[cfg(any(test, feature = "test-support"))]
use std::sync::Mutex;

/// Trait for abstracting time access.
///
/// Implementations must be `Send + Sync` for use behind `Arc` in
/// async contexts.
pub trait Clock: Send + Sync {
    /// Returns the current time.
    fn now(&self) -> DateTime<Utc>;
}

/// Clock implementation using the system clock.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Clock with manually controlled time for deterministic testing.
///
/// Uses [`std::sync::Mutex`] (not tokio) because `now()` is
/// synchronous and never held across `.await` points.
///
/// Only available when the `test-support` feature is enabled or
/// when running tests within this crate.
#[cfg(any(test, feature = "test-support"))]
pub struct TestClock {
    now: Mutex<DateTime<Utc>>,
}

#[cfg(any(test, feature = "test-support"))]
impl TestClock {
    /// Creates a new test clock starting at the given time.
    #[must_use]
    pub fn new(start: DateTime<Utc>) -> Self {
        Self {
            now: Mutex::new(start),
        }
    }

    /// Advances the clock by the given duration.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn advance(&self, delta: TimeDelta) {
        let mut now = self.now.lock().expect("test clock mutex poisoned");
        *now = *now + delta;
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Clock for TestClock {
    fn now(&self) -> DateTime<Utc> {
        *self.now.lock().expect("test clock mutex poisoned")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_returns_current_time() {
        let clock = SystemClock;
        let before = Utc::now();
        let time = clock.now();
        let after = Utc::now();

        assert!(time >= before && time <= after);
    }

    #[test]
    fn system_clock_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SystemClock>();
    }

    #[test]
    fn test_clock_starts_at_given_time() {
        let start = Utc::now();
        let clock = TestClock::new(start);

        assert_eq!(clock.now(), start);
    }

    #[test]
    fn test_clock_advances_by_duration() {
        let start = Utc::now();
        let clock = TestClock::new(start);

        let delta = TimeDelta::hours(5);
        clock.advance(delta);

        let expected = start + delta;
        assert_eq!(clock.now(), expected);
    }

    #[test]
    fn test_clock_multiple_advances() {
        let start = Utc::now();
        let clock = TestClock::new(start);

        clock.advance(TimeDelta::hours(1));
        clock.advance(TimeDelta::minutes(30));

        let expected = start + TimeDelta::hours(1) + TimeDelta::minutes(30);
        assert_eq!(clock.now(), expected);
    }

    #[test]
    fn test_clock_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TestClock>();
    }

    #[test]
    fn test_clock_implements_clock_trait() {
        let start = Utc::now();
        let clock = TestClock::new(start);
        let _: &dyn Clock = &clock;
    }
}
