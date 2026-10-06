//! Session state machine for the update check and install.
//!
//! The desktop keeps one [`UpdateMachine`] behind a mutex for the life of the
//! process. Network work never runs inside it: a caller begins a step on the
//! machine, does the work with the machine unlocked, and finishes the step on
//! it. That is why every transition comes as a `begin_*` and `finish_*` pair.
//!
//! ```text
//! any state but Installing ── begin_check ──▶ Checking
//! Checking ── finish_check ──▶ UpToDate | Installable | Manual | Failed
//! Installable ── begin_install ──▶ Installing
//! Installing ── finish_install(Failed) ──▶ Failed
//! ```
//!
//! The docs here name states as the webview sees them, by their
//! [`UpdateStatus`] names: Available is [`State::Installable`] and
//! `AvailableManually` is [`State::Manual`].
//!
//! The state is one private enum, [`State`], and the offer lives inside the
//! only variant that may use it. "Available but without an offer" therefore
//! has no representation, and the [`UpdateStatus`] the webview sees is
//! derived from the state instead of being stored next to it.

use crate::client::{CheckOutcome, InstallOutcome, VerifiedOffer};
use crate::error::{Result, UpdateError};
use crate::status::UpdateStatus;
use semver::Version;

/// Tracks one session's update check and install.
///
/// The webview sees only the [`UpdateStatus`] that [`Self::status`] derives.
#[derive(Debug, Default)]
pub struct UpdateMachine {
    /// Where the session stands, together with the offer when there is one.
    state: State,
}

impl UpdateMachine {
    /// Creates a machine that is idle and holds no offer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the status the webview is shown for the current state.
    #[must_use]
    pub fn status(&self) -> UpdateStatus {
        match &self.state {
            State::Idle => UpdateStatus::Idle,
            State::Checking => UpdateStatus::Checking,
            State::UpToDate => UpdateStatus::UpToDate,
            State::Installable(offer) => UpdateStatus::Available {
                version: offer.version().to_string(),
                notes: offer.notes().to_owned(),
            },
            State::Manual { version, notes } => UpdateStatus::AvailableManually {
                version: version.to_string(),
                notes: notes.clone(),
            },
            State::Installing => UpdateStatus::Installing,
            State::Failed => UpdateStatus::Failed,
        }
    }

    /// Marks the machine Checking and drops any previous offer, unless an
    /// install is in flight.
    ///
    /// A refused check leaves the machine [`UpdateStatus::Installing`]: the
    /// caller must not run the check, and has no [`Self::finish_check`] to call.
    pub fn begin_check(&mut self) -> CheckStart {
        if matches!(self.state, State::Installing) {
            return CheckStart::InstallInProgress;
        }

        self.state = State::Checking;
        CheckStart::Started
    }

    /// Applies the outcome of the check begun with [`Self::begin_check`].
    ///
    /// The check itself, [`perform_check`](crate::perform_check), runs between
    /// the two calls with the machine unlocked, so the status reads
    /// [`UpdateStatus::Checking`] while the request is in flight.
    ///
    /// Does nothing unless the machine is Checking. Two checks can overlap;
    /// when the first one ends in an offer and its install begins, the outcome
    /// of the second must not replace [`UpdateStatus::Installing`].
    pub fn finish_check(&mut self, outcome: CheckOutcome) {
        if !matches!(self.state, State::Checking) {
            return;
        }

        self.state = match outcome {
            CheckOutcome::UpToDate => State::UpToDate,
            CheckOutcome::Available(offer) => State::Installable(offer),
            CheckOutcome::AvailableManually { version, notes } => State::Manual { version, notes },
            CheckOutcome::Failed => State::Failed,
        };
    }

    /// Moves from Available to Installing and hands out the offer to install.
    ///
    /// The caller passes the offer to [`install_offer`](crate::install_offer)
    /// with the machine unlocked, then reports the result with
    /// [`Self::finish_install`]. Until then the machine refuses a check and a
    /// second install.
    ///
    /// # Errors
    ///
    /// Returns [`UpdateError::InstallNotAvailable`] from Idle, Checking,
    /// `UpToDate`, `AvailableManually`, Installing, or Failed. The machine is
    /// left as it was.
    pub fn begin_install(&mut self) -> Result<VerifiedOffer> {
        match std::mem::replace(&mut self.state, State::Installing) {
            State::Installable(offer) => Ok(offer),
            other => {
                self.state = other;
                Err(UpdateError::InstallNotAvailable)
            }
        }
    }

    /// Applies the outcome of the install begun with [`Self::begin_install`].
    ///
    /// A failed install moves the machine to Failed, from where a new check
    /// may start. After a successful one the process is about to restart or
    /// exit, so the machine stays Installing and goes on refusing a check or
    /// another install in the meantime. Does nothing unless the machine is
    /// Installing.
    pub fn finish_install(&mut self, outcome: InstallOutcome) {
        if !matches!(self.state, State::Installing) {
            return;
        }

        match outcome {
            InstallOutcome::Failed => self.state = State::Failed,
            InstallOutcome::Installed(_) => {}
        }
    }
}

#[cfg(test)]
impl UpdateMachine {
    /// Runs a whole check in one call and returns the status it led to.
    ///
    /// Test-only: it performs the request between begin and finish, which a
    /// caller that shares the machine behind a lock must not do.
    pub(crate) fn check(&mut self, config: &crate::client::ClientConfig) -> UpdateStatus {
        if self.begin_check() == CheckStart::Started {
            let outcome = crate::client::perform_check(config);
            self.finish_check(outcome);
        }
        self.status()
    }
}

/// Whether [`UpdateMachine::begin_check`] started a check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "a refused check must not be run or finished"]
pub enum CheckStart {
    /// The machine is Checking; run the check and finish it.
    Started,
    /// An install is in flight; the machine is unchanged.
    InstallInProgress,
}

/// Where a session stands. Each variant holds exactly the data that state
/// may use, so no transition has to keep two fields in step.
#[derive(Debug, Default)]
enum State {
    /// No check has been requested this session.
    #[default]
    Idle,
    /// A check is in flight.
    Checking,
    /// The last check found nothing newer.
    UpToDate,
    /// The last check found a newer version this copy may install.
    Installable(VerifiedOffer),
    /// The last check found a newer version that the system package manager
    /// has to install. No artifact is kept, so nothing can install it.
    Manual {
        /// The newer version.
        version: Version,
        /// Its release notes, sanitized.
        notes: String,
    },
    /// The offer was handed out by [`UpdateMachine::begin_install`] and is
    /// being downloaded, verified or handed to the installer.
    Installing,
    /// The last check or install failed.
    Failed,
}
