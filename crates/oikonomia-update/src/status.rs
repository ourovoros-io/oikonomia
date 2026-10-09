//! The update status the webview is shown.
//!
//! [`UpdateStatus`] is the only update type that crosses IPC. It serializes
//! with a `kind` tag in snake case, and `web/src/lib/updateCheck.ts` decodes
//! exactly that shape, so a variant or field renamed here has to be renamed
//! there. It is a projection of the machine's private state, which holds the
//! artifact URL, signature and digest; none of those is in here. Of a failure
//! it carries the code of the error and none of its text.

use serde::{Deserialize, Serialize};

/// Where the session's update check or install stands, as the webview
/// sees it.
///
/// `kind` is the discriminant the webview matches on. The variants that
/// report a newer version carry its version and escaped notes only, never a
/// URL, a signature or a key.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateStatus {
    /// No check has been requested this session.
    #[default]
    Idle,
    /// A check is in flight.
    Checking,
    /// Nothing newer is published: the verified manifest names this version
    /// or an older one, or the feed server answered 204.
    UpToDate,
    /// The verified manifest offers a newer version this copy may install.
    Available {
        /// The published version from the signed manifest, without a
        /// leading `v`.
        version: String,
        /// The release notes with every HTML markup character escaped.
        notes: String,
    },
    /// Manifest verified and a newer version exists, but the system package
    /// manager owns this install. The app reports it and never installs it.
    AvailableManually {
        /// The published version from the signed manifest, without a
        /// leading `v`.
        version: String,
        /// The release notes with every HTML markup character escaped.
        notes: String,
    },
    /// An install is downloading, verifying or handing over the artifact.
    /// No check and no second install starts until it ends.
    Installing,
    /// The install was cancelled before the artifact reached the installer.
    /// Nothing was replaced and the offer stands: the machine reads
    /// [`Self::Available`] again, so the install can be started anew.
    ///
    /// Only `update_install` returns this, as the answer to the install
    /// that was cancelled; the machine's own status never is.
    Cancelled,
    /// The last check or install failed. The rest of the application is
    /// unaffected, and a new check may be started.
    Failed {
        /// The stable code of the error it failed with
        /// ([`UpdateError::code`](crate::UpdateError::code)), which the
        /// webview words. Absent, and then left out of the serialized form,
        /// when the step ended without an error to name: its task died.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::UpdateStatus;

    #[test]
    fn a_failure_serializes_with_its_code_and_without_one_as_the_kind_alone() {
        let coded = UpdateStatus::Failed {
            code: Some("update_network".to_owned()),
        };
        let bare = UpdateStatus::Failed { code: None };

        assert_eq!(
            serde_json::to_string(&coded).expect("json"),
            r#"{"kind":"failed","code":"update_network"}"#
        );
        assert_eq!(
            serde_json::to_string(&bare).expect("json"),
            r#"{"kind":"failed"}"#
        );
    }

    #[test]
    fn a_cancelled_install_serializes_as_its_kind_alone() {
        assert_eq!(
            serde_json::to_string(&UpdateStatus::Cancelled).expect("json"),
            r#"{"kind":"cancelled"}"#
        );
    }

    #[test]
    fn a_failure_reads_back_with_and_without_its_code() {
        for status in [
            UpdateStatus::Failed {
                code: Some("update_manifest_signature".to_owned()),
            },
            UpdateStatus::Failed { code: None },
        ] {
            let json = serde_json::to_string(&status).expect("json");

            let read: UpdateStatus = serde_json::from_str(&json).expect("status");

            assert_eq!(read, status);
        }
    }
}
