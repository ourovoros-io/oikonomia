//! Release-side manifest assembly.
//!
//! The promote workflow calls the `assemble_feed` bin, which calls
//! [`assemble_manifest`]; the client tests parse the same output through
//! `perform_check`, so the promote lane and the client cannot drift.

use crate::error::{Result, UpdateError};
use crate::verify::parse_sha256_hex;
use crate::version::parse_version;
use serde::Serialize;
use std::collections::BTreeMap;

/// One platform artifact entry destined for `latest.json`.
#[derive(Debug, Clone)]
pub struct FeedArtifact {
    /// Client platform key, e.g. `darwin-aarch64`.
    pub platform: String,
    /// File name of the artifact as uploaded to the release.
    pub file_name: String,
    /// Minisign signature over the artifact (contents of the `.sig` file).
    pub signature: String,
    /// Lowercase hex SHA-256 of the artifact bytes.
    pub sha256_hex: String,
}

impl FeedArtifact {
    /// Checks that no text field is empty or only whitespace.
    ///
    /// # Errors
    ///
    /// Returns [`UpdateError::InvalidFeedInput`] naming the first empty field.
    fn require_fields(&self) -> Result<()> {
        let fields = [
            ("platform", &self.platform),
            ("file name", &self.file_name),
            ("signature", &self.signature),
        ];

        match fields.iter().find(|(_, value)| value.trim().is_empty()) {
            Some((field, _)) => Err(UpdateError::InvalidFeedInput { field }),
            None => Ok(()),
        }
    }
}

#[derive(Serialize)]
struct ManifestPlatform {
    url: String,
    signature: String,
    sha256: String,
}

#[derive(Serialize)]
struct Manifest {
    version: String,
    notes: String,
    platforms: BTreeMap<String, ManifestPlatform>,
}

/// Builds the exact `latest.json` body the update client parses.
///
/// `version` may carry the leading `v` of a release tag; the manifest holds
/// the version without it. `base_url` is the release download URL the file
/// names are appended to, with or without a trailing slash.
///
/// # Errors
///
/// Returns [`UpdateError::InvalidVersion`] when `version` is not `SemVer`,
/// which the client would refuse; [`UpdateError::InvalidFeedInput`] when
/// there are no artifacts or an artifact has an empty platform, file name or
/// signature; and [`UpdateError::ArtifactIntegrity`] when a `sha256_hex` is
/// not 64 hex characters. [`UpdateError::ManifestParse`] stands for a failure
/// to serialize the manifest, which its fields (strings, and a map keyed by
/// strings) give `serde_json` no reason for.
pub fn assemble_manifest(
    version: &str,
    notes: &str,
    base_url: &str,
    artifacts: &[FeedArtifact],
) -> Result<String> {
    let version = parse_version(version)?;
    if artifacts.is_empty() {
        return Err(UpdateError::InvalidFeedInput { field: "artifacts" });
    }

    let base = base_url.trim_end_matches('/');
    let mut platforms = BTreeMap::new();
    for artifact in artifacts {
        artifact.require_fields()?;
        parse_sha256_hex(&artifact.sha256_hex)?;
        platforms.insert(
            artifact.platform.clone(),
            ManifestPlatform {
                url: format!("{base}/{}", artifact.file_name),
                signature: artifact.signature.clone(),
                sha256: artifact.sha256_hex.to_lowercase(),
            },
        );
    }

    let manifest = Manifest {
        version: version.to_string(),
        notes: notes.to_owned(),
        platforms,
    };
    serde_json::to_string_pretty(&manifest).map_err(|_| UpdateError::ManifestParse)
}

#[cfg(test)]
mod tests {
    use super::{FeedArtifact, assemble_manifest};

    #[test]
    fn assembled_manifest_has_exactly_the_fields_the_client_parses() {
        let artifacts = vec![FeedArtifact {
            platform: "darwin-aarch64".to_owned(),
            file_name: "Oikonomia_aarch64.app.tar.gz".to_owned(),
            signature: "TESTSIG".to_owned(),
            sha256_hex: "ab".repeat(32),
        }];
        let json = assemble_manifest(
            "1.0.0",
            "First release.",
            "https://github.com/ourovoros-io/oikonomia/releases/download/v1.0.0",
            &artifacts,
        )
        .expect("assemble");
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(value["version"], "1.0.0");
        assert_eq!(value["notes"], "First release.");
        let platform = &value["platforms"]["darwin-aarch64"];
        assert_eq!(
            platform["url"],
            "https://github.com/ourovoros-io/oikonomia/releases/download/v1.0.0/Oikonomia_aarch64.app.tar.gz"
        );
        assert_eq!(platform["signature"], "TESTSIG");
        assert_eq!(platform["sha256"], "ab".repeat(32));
    }

    fn artifact() -> FeedArtifact {
        FeedArtifact {
            platform: "darwin-aarch64".to_owned(),
            file_name: "a.tar.gz".to_owned(),
            signature: "SIG".to_owned(),
            sha256_hex: "ab".repeat(32),
        }
    }

    fn error_of(version: &str, artifacts: &[FeedArtifact]) -> String {
        assemble_manifest(version, "", "https://example.com", artifacts)
            .expect_err("refused")
            .to_string()
    }

    #[test]
    fn assemble_names_the_input_that_is_empty() {
        assert_eq!(error_of("1.0.0", &[]), "feed input is empty: artifacts");

        let no_platform = FeedArtifact {
            platform: " ".to_owned(),
            ..artifact()
        };
        assert_eq!(
            error_of("1.0.0", &[no_platform]),
            "feed input is empty: platform"
        );

        let no_file_name = FeedArtifact {
            file_name: String::new(),
            ..artifact()
        };
        assert_eq!(
            error_of("1.0.0", &[no_file_name]),
            "feed input is empty: file name"
        );

        let no_signature = FeedArtifact {
            signature: "\n".to_owned(),
            ..artifact()
        };
        assert_eq!(
            error_of("1.0.0", &[no_signature]),
            "feed input is empty: signature"
        );
    }

    #[test]
    fn assemble_refuses_a_version_the_client_could_not_read() {
        assert_eq!(error_of("", &[artifact()]), "version \"\" is not semver");
        assert_eq!(
            error_of("v1.0", &[artifact()]),
            "version \"1.0\" is not semver"
        );
    }

    #[test]
    fn assemble_refuses_a_digest_that_is_not_sha256_hex() {
        let bad_sha = FeedArtifact {
            sha256_hex: "zz".to_owned(),
            ..artifact()
        };

        let err = assemble_manifest("1.0.0", "", "https://example.com", &[bad_sha])
            .expect_err("bad digest");

        assert_eq!(err.code(), "update_artifact_integrity");
    }
}
