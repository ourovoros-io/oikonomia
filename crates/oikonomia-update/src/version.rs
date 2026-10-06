//! Version text, read the same way wherever it enters the crate.
//!
//! Three places take a version as text: the running version handed to the
//! client, the version in a signed manifest, and the version the release lane
//! writes into a manifest. All three go through [`parse_version`], so a tag
//! such as `v0.2.0` means the same version on both sides of the feed.

use crate::error::{Result, UpdateError};
use semver::Version;

/// Parses a `SemVer` version, ignoring surrounding whitespace and one
/// leading `v` (release tags are written `v0.2.0`).
///
/// # Errors
///
/// Returns [`UpdateError::InvalidVersion`] when what remains is not `SemVer`.
/// An empty string is not.
pub(crate) fn parse_version(text: &str) -> Result<Version> {
    let text = text.trim();
    let text = text.strip_prefix('v').unwrap_or(text);

    Version::parse(text).map_err(|_| UpdateError::InvalidVersion {
        version: text.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::parse_version;
    use semver::Version;

    #[test]
    fn a_release_tag_parses_as_its_version() {
        let expected = Version::new(0, 2, 0);

        assert_eq!(parse_version("0.2.0").expect("plain"), expected);
        assert_eq!(parse_version(" v0.2.0\n").expect("tag"), expected);
    }

    #[test]
    fn text_that_is_not_semver_is_refused_with_what_was_read() {
        for text in [
            "",
            "  ",
            "v",
            "1.2",
            "latest",
            "0.2.0 beta",
            "vv1.0.0",
            "V1.0.0",
        ] {
            let err = parse_version(text).expect_err(text);

            assert_eq!(err.code(), "update_invalid_version", "{text:?}");
        }

        let err = parse_version(" v1.2 ").expect_err("two parts");
        assert_eq!(err.to_string(), "version \"1.2\" is not semver");
    }
}

#[cfg(test)]
mod properties {
    use super::parse_version;
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;
    use semver::Version;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        #[test]
        fn a_version_written_as_a_tag_parses_back(
            major in any::<u64>(),
            minor in any::<u64>(),
            patch in any::<u64>(),
            tagged in any::<bool>(),
        ) {
            let version = Version::new(major, minor, patch);
            let prefix = if tagged { "v" } else { "" };

            let parsed = parse_version(&format!(" {prefix}{version}\n"));

            prop_assert!(matches!(&parsed, Ok(read) if *read == version), "{:?}", parsed);
        }

        #[test]
        fn parsing_any_text_returns_instead_of_panicking(text in any::<String>()) {
            let _ = parse_version(&text);
        }
    }
}
