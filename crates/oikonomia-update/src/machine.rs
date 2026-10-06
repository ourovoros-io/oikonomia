//! Session state machine for unlock-screen update check / install.
//!
//! [`install_offer`]: crate::install_offer

use crate::client::{
    CheckOutcome, ClientConfig, InstallOutcome, InstallRoute, VerifiedOffer, perform_check,
};
use crate::error::{Result, UpdateError};
use crate::status::UpdateStatus;

/// In-process update machine. The webview sees only [`UpdateStatus`].
#[derive(Debug, Default)]
pub struct UpdateMachine {
    status: UpdateStatus,
    offer: Option<VerifiedOffer>,
}

impl UpdateMachine {
    /// Idle, no offer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Current status (clone for IPC).
    #[must_use]
    pub fn status(&self) -> UpdateStatus {
        self.status.clone()
    }

    /// Marks the machine Checking and drops any previous offer, unless an
    /// install is in flight.
    ///
    /// A refused check leaves the machine [`UpdateStatus::Installing`]: the
    /// caller must not run the check, and has no [`Self::finish_check`] to call.
    pub fn begin_check(&mut self) -> CheckStart {
        if self.status == UpdateStatus::Installing {
            return CheckStart::InstallInProgress;
        }
        self.status = UpdateStatus::Checking;
        self.offer = None;
        CheckStart::Started
    }

    /// Applies a finished check. Call after [`perform_check`] so HTTP is not done
    /// while the UI still needs to observe [`UpdateStatus::Checking`].
    ///
    /// Does nothing unless the machine is Checking. Two checks can overlap;
    /// when the first one ends in an offer and its install begins, the outcome
    /// of the second must not replace [`UpdateStatus::Installing`].
    pub fn finish_check(&mut self, outcome: CheckOutcome) {
        if self.status != UpdateStatus::Checking {
            return;
        }
        match outcome {
            CheckOutcome::UpToDate => {
                self.status = UpdateStatus::UpToDate;
                self.offer = None;
            }
            CheckOutcome::Available(offer) => {
                self.status = offer.status();
                // Only an offer this copy may install is kept, so an install
                // cannot reach a package-managed copy even through a bug.
                self.offer = match offer.install_route() {
                    InstallRoute::InApp => Some(offer),
                    InstallRoute::PackageManager => None,
                };
            }
            CheckOutcome::Failed => {
                self.status = UpdateStatus::Failed;
                self.offer = None;
            }
        }
    }

    /// Runs begin + [`perform_check`] + finish under one call (tests / single-threaded).
    pub fn check(&mut self, config: &ClientConfig) -> UpdateStatus {
        if self.begin_check() == CheckStart::Started {
            let outcome = perform_check(config);
            self.finish_check(outcome);
        }
        self.status()
    }

    /// Moves from Available to Installing and hands out the offer to install.
    ///
    /// The caller passes the offer to [`install_offer`] without holding the
    /// machine, then reports the result with [`Self::finish_install`]. Until
    /// then the machine refuses a check and a second install.
    ///
    /// # Errors
    ///
    /// Returns [`UpdateError::InstallNotAvailable`] from Idle, Checking,
    /// `UpToDate`, `AvailableManually`, Installing, or Failed.
    pub fn begin_install(&mut self) -> Result<VerifiedOffer> {
        let UpdateStatus::Available { .. } = &self.status else {
            return Err(UpdateError::InstallNotAvailable);
        };
        let Some(offer) = self.offer.take() else {
            return Err(UpdateError::InstallNotAvailable);
        };
        self.status = UpdateStatus::Installing;
        Ok(offer)
    }

    /// Applies the outcome of the install begun with [`Self::begin_install`].
    ///
    /// A failed install moves the machine to Failed, from where a new check
    /// may start. After a successful one the process is about to restart or
    /// exit, so the machine stays Installing and goes on refusing a check or
    /// another install in the meantime. Does nothing unless the machine is
    /// Installing.
    pub fn finish_install(&mut self, outcome: InstallOutcome) {
        if self.status != UpdateStatus::Installing {
            return;
        }
        match outcome {
            InstallOutcome::Failed => self.status = UpdateStatus::Failed,
            InstallOutcome::Installed(_) => {}
        }
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
