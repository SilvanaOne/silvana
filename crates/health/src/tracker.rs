//! Per-component error-duration tracker for raising delayed alerts.
//!
//! Wraps a reconnect / retry loop. The caller calls `record_failure` on every
//! Err and `record_success` on every Ok. The tracker returns an `AlertKind`
//! when the caller should fire an alert: `First` after `alert_after` of
//! continuous failure, `Reminder` every `reminder_interval` while still
//! failing, and `Recovered` once on the next success (only if a prior alert
//! had been fired).
//!
//! All state is in-process; no I/O. Delivery is the caller's responsibility
//! — pair this with `health::alert(...)` for Telegram routing.

use std::fmt;
use std::time::{Duration, Instant};

use tracing::debug;

/// What kind of alert the caller should send right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind {
    /// First crossing of `alert_after` in this failure window.
    First,
    /// Re-alert because the failure is still ongoing one
    /// `reminder_interval` after the previous alert.
    Reminder,
    /// First success after one or more First/Reminder alerts fired —
    /// pair this with a "recovered" Telegram message.
    Recovered,
}

/// Per-(component, instance) tracker.
///
/// Cheap to construct. Hold one per loop iteration target — e.g. one per
/// party in a fan-out reconnect loop.
pub struct ErrorDurationTracker {
    component: String,
    instance: String,
    alert_after: Duration,
    reminder_interval: Duration,
    failing_since: Option<Instant>,
    last_alert_at: Option<Instant>,
    /// True once a First or Reminder has been emitted in the current failure
    /// window. Gates Recovered so flaps shorter than `alert_after` stay silent.
    alerted: bool,
}

impl ErrorDurationTracker {
    /// Construct with defaults from env vars:
    /// - `ALERT_WORKER_FAILURE_AFTER_SECS` (default 600 = 10 min)
    /// - `ALERT_REMINDER_INTERVAL_HOURS`   (default 24)
    pub fn new(component: &str, instance: &str) -> Self {
        let alert_after_secs = std::env::var("ALERT_WORKER_FAILURE_AFTER_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(600);
        let reminder_hours = std::env::var("ALERT_REMINDER_INTERVAL_HOURS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(24);
        Self::with_intervals(
            component,
            instance,
            Duration::from_secs(alert_after_secs),
            Duration::from_secs(reminder_hours.saturating_mul(3600)),
        )
    }

    /// Explicit constructor — preferred in tests.
    pub fn with_intervals(
        component: &str,
        instance: &str,
        alert_after: Duration,
        reminder_interval: Duration,
    ) -> Self {
        Self {
            component: component.to_string(),
            instance: instance.to_string(),
            alert_after,
            reminder_interval,
            failing_since: None,
            last_alert_at: None,
            alerted: false,
        }
    }

    pub fn component(&self) -> &str {
        &self.component
    }

    pub fn instance(&self) -> &str {
        &self.instance
    }

    /// How long the current failure window has been open. None if not currently failing.
    pub fn failing_for(&self) -> Option<Duration> {
        self.failing_since.map(|t| t.elapsed())
    }

    /// Call on Err in the reconnect / retry loop.
    pub fn record_failure(&mut self, err: &dyn fmt::Display) -> Option<AlertKind> {
        self.record_failure_at(err, Instant::now())
    }

    /// Call on Ok / successful processing.
    pub fn record_success(&mut self) -> Option<AlertKind> {
        self.record_success_at(Instant::now())
    }

    // Internal: parameterized on `now` for unit testing.
    fn record_failure_at(&mut self, err: &dyn fmt::Display, now: Instant) -> Option<AlertKind> {
        let started = *self.failing_since.get_or_insert(now);
        let elapsed = now.saturating_duration_since(started);

        if elapsed < self.alert_after {
            debug!(
                component = %self.component,
                instance = %self.instance,
                elapsed_secs = elapsed.as_secs(),
                "tracker: failure below threshold ({})",
                err
            );
            return None;
        }

        // Threshold crossed. Either first crossing or a reminder.
        match self.last_alert_at {
            None => {
                self.last_alert_at = Some(now);
                self.alerted = true;
                Some(AlertKind::First)
            }
            Some(prev) if now.saturating_duration_since(prev) >= self.reminder_interval => {
                self.last_alert_at = Some(now);
                self.alerted = true;
                Some(AlertKind::Reminder)
            }
            _ => None,
        }
    }

    fn record_success_at(&mut self, _now: Instant) -> Option<AlertKind> {
        let was_alerted = self.alerted;
        self.failing_since = None;
        self.last_alert_at = None;
        self.alerted = false;
        if was_alerted { Some(AlertKind::Recovered) } else { None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dummy(&'static str);
    impl fmt::Display for Dummy {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0)
        }
    }

    #[test]
    fn no_alert_below_threshold() {
        let mut t = ErrorDurationTracker::with_intervals(
            "w",
            "i",
            Duration::from_secs(10),
            Duration::from_secs(60),
        );
        let t0 = Instant::now();
        assert_eq!(t.record_failure_at(&Dummy("e"), t0), None);
        assert_eq!(
            t.record_failure_at(&Dummy("e"), t0 + Duration::from_secs(9)),
            None
        );
    }

    #[test]
    fn first_alert_at_threshold() {
        let mut t = ErrorDurationTracker::with_intervals(
            "w",
            "i",
            Duration::from_secs(10),
            Duration::from_secs(60),
        );
        let t0 = Instant::now();
        t.record_failure_at(&Dummy("e"), t0);
        assert_eq!(
            t.record_failure_at(&Dummy("e"), t0 + Duration::from_secs(10)),
            Some(AlertKind::First)
        );
        // Subsequent calls within the reminder window stay silent.
        assert_eq!(
            t.record_failure_at(&Dummy("e"), t0 + Duration::from_secs(30)),
            None
        );
    }

    #[test]
    fn reminder_after_interval() {
        let mut t = ErrorDurationTracker::with_intervals(
            "w",
            "i",
            Duration::from_secs(10),
            Duration::from_secs(60),
        );
        let t0 = Instant::now();
        t.record_failure_at(&Dummy("e"), t0);
        let _first = t.record_failure_at(&Dummy("e"), t0 + Duration::from_secs(10));
        assert_eq!(
            t.record_failure_at(&Dummy("e"), t0 + Duration::from_secs(69)),
            None
        );
        assert_eq!(
            t.record_failure_at(&Dummy("e"), t0 + Duration::from_secs(70)),
            Some(AlertKind::Reminder)
        );
    }

    #[test]
    fn recovered_only_after_alerted() {
        let mut t = ErrorDurationTracker::with_intervals(
            "w",
            "i",
            Duration::from_secs(10),
            Duration::from_secs(60),
        );
        let t0 = Instant::now();
        // Short flap that never crosses threshold → no recovered.
        t.record_failure_at(&Dummy("e"), t0);
        t.record_failure_at(&Dummy("e"), t0 + Duration::from_secs(5));
        assert_eq!(t.record_success_at(t0 + Duration::from_secs(6)), None);

        // Longer outage crosses threshold → recovered on next success.
        t.record_failure_at(&Dummy("e"), t0 + Duration::from_secs(100));
        let _first = t.record_failure_at(&Dummy("e"), t0 + Duration::from_secs(120));
        assert_eq!(
            t.record_success_at(t0 + Duration::from_secs(125)),
            Some(AlertKind::Recovered)
        );
        // Calling success again is a no-op.
        assert_eq!(t.record_success_at(t0 + Duration::from_secs(126)), None);
    }

    #[test]
    fn recovery_resets_failing_window() {
        let mut t = ErrorDurationTracker::with_intervals(
            "w",
            "i",
            Duration::from_secs(10),
            Duration::from_secs(60),
        );
        let t0 = Instant::now();
        t.record_failure_at(&Dummy("e"), t0);
        let _ = t.record_failure_at(&Dummy("e"), t0 + Duration::from_secs(15));
        let _ = t.record_success_at(t0 + Duration::from_secs(16));
        // New failure window starts fresh — 1s in, no alert yet.
        assert_eq!(
            t.record_failure_at(&Dummy("e"), t0 + Duration::from_secs(20)),
            None
        );
    }
}
