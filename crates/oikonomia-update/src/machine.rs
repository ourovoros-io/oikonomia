//! Session state machine for unlock-screen update check / install.

use crate::client::{
    ArtifactInstaller, CheckOutcome, ClientConfig, InstallHandoff, InstallOutcome, InstallRoute,
    VerifiedOffer, delete_artifact, download_and_verify, perform_check,
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

    /// Marks the machine Checking and drops any previous offer.
    pub fn begin_check(&mut self) {
        self.status = UpdateStatus::Checking;
        self.offer = None;
    }

    /// Applies a finished check. Call after [`perform_check`] so HTTP is not done
    /// while the UI still needs to observe [`UpdateStatus::Checking`].
    pub fn finish_check(&mut self, outcome: CheckOutcome) {
        match outcome {
            CheckOutcome::UpToDate => {
                self.status = UpdateStatus::UpToDate;
                self.offer = None;
            }
            CheckOutcome::Available(offer) => {
                self.status = offer.status();
                // Only an offer this copy may install is kept, so `install`
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
        self.begin_check();
        let outcome = perform_check(config);
        self.finish_check(outcome);
        self.status()
    }

    /// Hard error unless the machine is Available with a stored offer.
    ///
    /// # Errors
    ///
    /// Returns [`UpdateError::InstallNotAvailable`] from Idle, Checking, `UpToDate`,
    /// `AvailableManually`, or Failed.
    pub fn require_available(&self) -> Result<&VerifiedOffer> {
        let UpdateStatus::Available { .. } = &self.status else {
            return Err(UpdateError::InstallNotAvailable);
        };
        let Some(offer) = self.offer.as_ref() else {
            return Err(UpdateError::InstallNotAvailable);
        };
        Ok(offer)
    }

    /// Moves to Failed and forgets the offer.
    pub fn fail(&mut self) {
        self.status = UpdateStatus::Failed;
        self.offer = None;
    }

    /// Downloads and verifies the artifact, then execs via `installer`.
    ///
    /// Illegal state is a hard error. Hash/sig/network failure becomes Failed
    /// (and the partial file is deleted). `installer` is not called on verify failure.
    /// The artifact is deleted afterwards unless an installer process is still
    /// running from it.
    ///
    /// # Errors
    ///
    /// Returns [`UpdateError::InstallNotAvailable`] when status is not Available.
    pub fn install(
        &mut self,
        config: &ClientConfig,
        installer: &impl ArtifactInstaller,
    ) -> Result<InstallOutcome> {
        let offer = self.require_available()?.clone();
        let path = match download_and_verify(config, &offer) {
            Ok(path) => path,
            Err(err) => {
                log::warn!("update install verify failed: {err}");
                self.fail();
                return Ok(InstallOutcome::Failed);
            }
        };
        match installer.install(&path) {
            Ok(InstallHandoff::Replaced) => {
                delete_artifact(&path);
                Ok(InstallOutcome::Installed(InstallHandoff::Replaced))
            }
            Ok(InstallHandoff::InstallerStarted) => {
                Ok(InstallOutcome::Installed(InstallHandoff::InstallerStarted))
            }
            Err(err) => {
                log::warn!("update install exec failed: {err}");
                delete_artifact(&path);
                self.fail();
                Ok(InstallOutcome::Failed)
            }
        }
    }
}
