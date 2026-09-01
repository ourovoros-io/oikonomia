//! Release-side manifest assembly.
//!
//! The promote workflow calls the `assemble_feed` bin, which calls
//! [`assemble_manifest`]; the client tests parse the same output through
//! `perform_check`, so the promote lane and the client cannot drift.

use crate::error::{Result, UpdateError};
use crate::verify::parse_sha256_hex;
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

/// Build the exact `latest.json` body the update client parses.
///
/// # Errors
///
/// [`UpdateError::ManifestParse`] when there are no artifacts, or an entry has
/// an empty platform/file/signature. [`UpdateError::ArtifactIntegrity`] when
/// `sha256_hex` is malformed (not 64 hex characters).
pub fn assemble_manifest(
    version: &str,
    notes: &str,
    base_url: &str,
    artifacts: &[FeedArtifact],
) -> Result<String> {
    if artifacts.is_empty() || version.trim().is_empty() {
        return Err(UpdateError::ManifestParse);
    }

    let base = base_url.trim_end_matches('/');
    let mut platforms = BTreeMap::new();
    for artifact in artifacts {
        if artifact.platform.trim().is_empty()
            || artifact.file_name.trim().is_empty()
            || artifact.signature.trim().is_empty()
        {
            return Err(UpdateError::ManifestParse);
        }
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
        version: version.trim().trim_start_matches('v').to_owned(),
        notes: notes.to_owned(),
        platforms,
    };
    serde_json::to_string_pretty(&manifest).map_err(|_| UpdateError::ManifestParse)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
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
            "https://github.com/ourovoros-io/oikonomia-releases/releases/download/v1.0.0",
            &artifacts,
        )
        .expect("assemble");
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(value["version"], "1.0.0");
        assert_eq!(value["notes"], "First release.");
        let platform = &value["platforms"]["darwin-aarch64"];
        assert_eq!(
            platform["url"],
            "https://github.com/ourovoros-io/oikonomia-releases/releases/download/v1.0.0/Oikonomia_aarch64.app.tar.gz"
        );
        assert_eq!(platform["signature"], "TESTSIG");
        assert_eq!(platform["sha256"], "ab".repeat(32));
    }

    #[test]
    fn assemble_rejects_empty_inputs() {
        assert!(assemble_manifest("1.0.0", "", "https://example.com", &[]).is_err());
        let bad_sha = vec![FeedArtifact {
            platform: "darwin-aarch64".to_owned(),
            file_name: "a.tar.gz".to_owned(),
            signature: "SIG".to_owned(),
            sha256_hex: "zz".to_owned(),
        }];
        assert!(assemble_manifest("1.0.0", "", "https://example.com", &bad_sha).is_err());
    }
}
