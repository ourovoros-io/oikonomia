//! What an install reports while it runs, and how it is stopped.
//!
//! [`InstallProgress`] is what the desktop forwards to the webview over a
//! Tauri channel while [`install_offer_reporting`] runs: download progress,
//! throttled, then one `installing` report. [`InstallControl`] is how the
//! webview's cancel reaches the download, and the one place that decides
//! whether a cancel still can: up to the moment the verified artifact is
//! handed to the installer, and never after.
//!
//! [`install_offer_reporting`]: crate::install_offer_reporting

use serde::Serialize;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// The shortest time between two download reports: at most ten a second.
///
/// The webview repaints a progress bar from each report, and a fast
/// connection delivers a chunk far more often than a person can see.
pub(crate) const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// One report of an install in flight, as the webview receives it.
///
/// Serialized with a `kind` tag in snake case, like
/// [`UpdateStatus`](crate::UpdateStatus). The reports of one install come in
/// this order: one or more `downloading`, the last of which has `received`
/// equal to the bytes that were downloaded, then at most one `installing`.
/// An install that fails or is cancelled stops sending; the command's return
/// value says how it ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InstallProgress {
    /// The artifact is downloading.
    Downloading {
        /// Bytes of the artifact received so far.
        received: u64,
        /// The size the server announced in `Content-Length`, or `None` when
        /// it announced none. Only a display hint: the download is capped
        /// and verified by what arrives, never by this number.
        total: Option<u64>,
    },
    /// The artifact verified and is being handed to the installer. A cancel
    /// is refused from here on.
    Installing,
}

/// Decides which download reports are sent: the first, then at most one per
/// [`PROGRESS_INTERVAL`], then the last.
///
/// Times are passed in, not read, so a test can state them.
#[derive(Debug)]
pub(crate) struct ProgressThrottle {
    /// The shortest time between two reports that are not the last.
    interval: Duration,
    /// When the last report was let through.
    last_sent: Option<Instant>,
    /// The byte count of the last report let through.
    last_received: Option<u64>,
}

impl ProgressThrottle {
    /// Returns a throttle that lets through at most one report per
    /// `interval`, besides the last.
    pub(crate) fn new(interval: Duration) -> Self {
        Self {
            interval,
            last_sent: None,
            last_received: None,
        }
    }

    /// Returns whether a report of `received` bytes at `now` is sent, and
    /// records it when it is.
    pub(crate) fn admit(&mut self, now: Instant, received: u64) -> bool {
        if let Some(last) = self.last_sent
            && now.saturating_duration_since(last) < self.interval
        {
            return false;
        }

        self.last_sent = Some(now);
        self.last_received = Some(received);
        true
    }

    /// Returns whether the final report, of `received` bytes, is sent: it
    /// is, whatever the time, unless the last report sent said the same.
    pub(crate) fn admit_final(&mut self, now: Instant, received: u64) -> bool {
        if self.last_received == Some(received) {
            return false;
        }

        self.last_sent = Some(now);
        self.last_received = Some(received);
        true
    }
}

/// Where an install stands, as far as a cancel is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Downloading or verifying. A cancel is accepted.
    Downloading,
    /// A cancel was accepted. The install stops at its next check and
    /// leaves no artifact.
    Cancelled,
    /// The verified artifact is with the installer. A cancel is refused.
    Installing,
}

/// The cancel switch of one install, shared between the install and
/// whoever may cancel it.
///
/// Clones share one switch. It starts in the downloading phase; a cancel
/// moves it to cancelled, and the install moves it to installing just
/// before the installer runs. Each move happens only from the downloading
/// phase, under one lock, so a cancel and the start of installing cannot
/// both succeed.
#[derive(Debug, Clone)]
pub struct InstallControl {
    /// The phase, shared by every clone.
    phase: Arc<Mutex<Phase>>,
}

impl Default for InstallControl {
    fn default() -> Self {
        Self::new()
    }
}

impl InstallControl {
    /// Returns the switch of a new install, in the downloading phase.
    #[must_use]
    pub fn new() -> Self {
        Self {
            phase: Arc::new(Mutex::new(Phase::Downloading)),
        }
    }

    /// Cancels the install if it has not reached the installer, and returns
    /// whether it did.
    ///
    /// False once installing has begun, and false for a second cancel.
    #[must_use = "false means the install was not stopped"]
    pub fn cancel(&self) -> bool {
        self.move_from_downloading(Phase::Cancelled)
    }

    /// Returns whether the install was cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        *self.lock() == Phase::Cancelled
    }

    /// Moves the install to the installing phase, after which a cancel is
    /// refused. Returns false when it was cancelled first.
    pub(crate) fn begin_installing(&self) -> bool {
        self.move_from_downloading(Phase::Installing)
    }

    /// Moves to `next` if the phase is downloading; returns whether it did.
    fn move_from_downloading(&self, next: Phase) -> bool {
        let mut phase = self.lock();
        match *phase {
            Phase::Downloading => {
                *phase = next;
                true
            }
            Phase::Cancelled | Phase::Installing => false,
        }
    }

    /// Locks the phase. A panic while it was held cannot leave it half
    /// written, since it is one enum value, so a poisoned lock is used as
    /// it is.
    fn lock(&self) -> std::sync::MutexGuard<'_, Phase> {
        self.phase.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::{InstallControl, InstallProgress, PROGRESS_INTERVAL, ProgressThrottle};
    use std::time::{Duration, Instant};

    #[test]
    fn progress_serializes_with_a_snake_case_kind_and_a_null_unknown_total() {
        let known = InstallProgress::Downloading {
            received: 5,
            total: Some(10),
        };
        let unknown = InstallProgress::Downloading {
            received: 5,
            total: None,
        };

        assert_eq!(
            serde_json::to_value(known).expect("json"),
            serde_json::json!({ "kind": "downloading", "received": 5, "total": 10 })
        );
        assert_eq!(
            serde_json::to_value(unknown).expect("json"),
            serde_json::json!({ "kind": "downloading", "received": 5, "total": null })
        );
        assert_eq!(
            serde_json::to_value(InstallProgress::Installing).expect("json"),
            serde_json::json!({ "kind": "installing" })
        );
    }

    #[test]
    fn the_throttle_lets_through_at_most_ten_reports_a_second_plus_the_last() {
        let start = Instant::now();
        let mut throttle = ProgressThrottle::new(PROGRESS_INTERVAL);
        let mut sent = 0_u32;

        // A chunk every millisecond for one second.
        for millis in 0..1000_u64 {
            if throttle.admit(start + Duration::from_millis(millis), millis) {
                sent += 1;
            }
        }
        let last = throttle.admit_final(start + Duration::from_millis(1000), 1000);

        assert_eq!(sent, 10);
        assert!(last, "the last report is sent whatever the time");
    }

    #[test]
    fn the_last_report_is_not_repeated_when_it_was_just_sent() {
        let start = Instant::now();
        let mut throttle = ProgressThrottle::new(PROGRESS_INTERVAL);

        assert!(throttle.admit(start, 42));
        assert!(!throttle.admit_final(start, 42));
        assert!(throttle.admit_final(start, 43));
    }

    #[test]
    fn a_cancel_wins_only_before_installing_and_only_once() {
        let cancelled = InstallControl::new();
        assert!(cancelled.cancel());
        assert!(cancelled.is_cancelled());
        assert!(!cancelled.cancel(), "a second cancel");
        assert!(!cancelled.begin_installing(), "installing after a cancel");

        let installing = InstallControl::new();
        assert!(installing.begin_installing());
        assert!(
            !installing.clone().cancel(),
            "a cancel after installing began"
        );
        assert!(!installing.is_cancelled());
    }
}
