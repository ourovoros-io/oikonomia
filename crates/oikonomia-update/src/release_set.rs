//! Which files of a draft release are published, and which feed entry each
//! one fills.
//!
//! The promote workflow asks this module instead of matching file names in
//! shell, so the rules are tested and the feed and the allow-list cannot
//! disagree.

use thiserror::Error;

/// Whether the Windows installer in a draft is published.
///
/// The release workflow always builds it, to prove the build. It is withheld
/// until it is code-signed: an unsigned installer shows the "Windows
/// protected your PC" warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsBuild {
    /// Leave the installer out of the feed and delete it from the release.
    Withheld,
    /// Publish the installer and offer it to installed copies.
    Published,
}

/// A draft release does not hold the artifacts a feed needs.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReleaseSetError {
    /// No file for a platform that must be in the feed.
    #[error("no {suffix} file for {platform}")]
    Missing {
        /// Feed key of the platform.
        platform: &'static str,
        /// File-name suffix that was looked for.
        suffix: &'static str,
    },

    /// More than one candidate; promoting would have to guess.
    #[error("expected one {suffix} file for {platform}, found: {found}")]
    Ambiguous {
        /// Feed key of the platform.
        platform: &'static str,
        /// File-name suffix that was looked for.
        suffix: &'static str,
        /// The candidates, comma-separated.
        found: String,
    },
}

struct FeedPlatform {
    /// Key the update client derives from its own OS and architecture.
    key: &'static str,
    /// Suffix of the one artifact that platform installs.
    suffix: &'static str,
}

const MACOS: FeedPlatform = FeedPlatform {
    key: "darwin-aarch64",
    suffix: ".app.tar.gz",
};
const LINUX: FeedPlatform = FeedPlatform {
    key: "linux-x86_64",
    suffix: ".AppImage",
};
const WINDOWS: FeedPlatform = FeedPlatform {
    key: "windows-x86_64",
    suffix: "-setup.exe",
};

/// Suffixes published next to the feed artifacts: the disk image and the
/// package people download by hand.
const MANUAL_DOWNLOAD_SUFFIXES: [&str; 2] = [".dmg", ".deb"];

const FEED_FILES: [&str; 2] = ["latest.json", "latest.json.sig"];

/// Name of the checksum file published with every release: one
/// `<sha256>  <file name>` line per published file, the format
/// `sha256sum --check` reads.
pub const CHECKSUMS_FILE: &str = "SHA256SUMS";

fn platforms(windows: WindowsBuild) -> Vec<FeedPlatform> {
    match windows {
        WindowsBuild::Withheld => vec![MACOS, LINUX],
        WindowsBuild::Published => vec![MACOS, LINUX, WINDOWS],
    }
}

/// The `(platform key, file name)` pairs for `latest.json`.
///
/// Every platform in the set needs exactly one artifact among `file_names`.
///
/// # Errors
///
/// [`ReleaseSetError::Missing`] or [`ReleaseSetError::Ambiguous`] when a
/// platform has no artifact or more than one.
pub fn feed_entries<'a>(
    file_names: &[&'a str],
    windows: WindowsBuild,
) -> Result<Vec<(&'static str, &'a str)>, ReleaseSetError> {
    let mut entries = Vec::new();

    for platform in platforms(windows) {
        let mut candidates = Vec::new();
        for name in file_names {
            if name.ends_with(platform.suffix) {
                candidates.push(*name);
            }
        }

        match candidates.as_slice() {
            [] => {
                return Err(ReleaseSetError::Missing {
                    platform: platform.key,
                    suffix: platform.suffix,
                });
            }
            [only] => entries.push((platform.key, *only)),
            _ => {
                return Err(ReleaseSetError::Ambiguous {
                    platform: platform.key,
                    suffix: platform.suffix,
                    found: candidates.join(", "),
                });
            }
        }
    }

    Ok(entries)
}

/// The files [`CHECKSUMS_FILE`] lists: every published file except the
/// checksum file itself, sorted by name so the file is reproducible.
#[must_use]
pub fn checksummed_assets<'a>(file_names: &[&'a str], windows: WindowsBuild) -> Vec<&'a str> {
    let mut listed = Vec::new();
    for name in file_names {
        if *name != CHECKSUMS_FILE && is_published_asset(name, windows) {
            listed.push(*name);
        }
    }
    listed.sort_unstable();
    listed
}

/// One line of [`CHECKSUMS_FILE`], newline included.
#[must_use]
pub fn checksum_line(sha256_hex: &str, file_name: &str) -> String {
    format!("{sha256_hex}  {file_name}\n")
}

/// True when `file_name` belongs in the published release: a feed artifact,
/// a manual download, the signature of either, the feed itself, or the
/// checksum file. Everything else
/// in the draft is deleted before publishing.
#[must_use]
pub fn is_published_asset(file_name: &str, windows: WindowsBuild) -> bool {
    if FEED_FILES.contains(&file_name) || file_name == CHECKSUMS_FILE {
        return true;
    }

    // A detached signature is published exactly when its file is.
    let artifact = file_name.strip_suffix(".sig").unwrap_or(file_name);

    for suffix in MANUAL_DOWNLOAD_SUFFIXES {
        if artifact.ends_with(suffix) {
            return true;
        }
    }

    for platform in platforms(windows) {
        if artifact.ends_with(platform.suffix) {
            return true;
        }
    }

    false
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use super::{
        CHECKSUMS_FILE, ReleaseSetError, WindowsBuild, checksum_line, checksummed_assets,
        feed_entries, is_published_asset,
    };
    use crate::client::current_updater_platform;

    /// What tauri-action leaves in a draft for one tag.
    const DRAFT: [&str; 12] = [
        "Oikonomia.app.tar.gz",
        "Oikonomia.app.tar.gz.sig",
        "Oikonomia_0.2.0_aarch64.dmg",
        "Oikonomia_0.2.0_amd64.AppImage",
        "Oikonomia_0.2.0_amd64.AppImage.sig",
        "Oikonomia_0.2.0_amd64.deb",
        "Oikonomia_0.2.0_amd64.deb.sig",
        "Oikonomia_0.2.0_x64-setup.exe",
        "Oikonomia_0.2.0_x64-setup.exe.sig",
        "latest.json",
        "latest.json.sig",
        "SHA256SUMS.txt",
    ];

    #[test]
    fn feed_names_macos_and_linux_and_withholds_windows_by_default() {
        let entries = feed_entries(&DRAFT, WindowsBuild::Withheld).expect("entries");

        assert_eq!(
            entries,
            [
                ("darwin-aarch64", "Oikonomia.app.tar.gz"),
                ("linux-x86_64", "Oikonomia_0.2.0_amd64.AppImage"),
            ]
        );
    }

    #[test]
    fn feed_adds_windows_only_when_asked() {
        let entries = feed_entries(&DRAFT, WindowsBuild::Published).expect("entries");

        assert_eq!(
            entries.last(),
            Some(&("windows-x86_64", "Oikonomia_0.2.0_x64-setup.exe"))
        );
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn feed_keys_are_the_ones_the_client_asks_for() {
        // The client looks itself up by `{os}-{arch}`; a key it never
        // derives would leave that platform without updates, silently.
        let entries = feed_entries(&DRAFT, WindowsBuild::Published).expect("entries");
        let keys: Vec<&str> = entries.iter().map(|(key, _)| *key).collect();
        let own = current_updater_platform();

        if cfg!(any(
            all(target_os = "macos", target_arch = "aarch64"),
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "windows", target_arch = "x86_64"),
        )) {
            assert!(keys.contains(&own.as_str()), "{own} missing from {keys:?}");
        }
    }

    #[test]
    fn a_missing_platform_artifact_stops_the_promotion() {
        let without_appimage: Vec<&str> = DRAFT
            .iter()
            .copied()
            .filter(|name| !name.contains("AppImage"))
            .collect();

        assert_eq!(
            feed_entries(&without_appimage, WindowsBuild::Withheld),
            Err(ReleaseSetError::Missing {
                platform: "linux-x86_64",
                suffix: ".AppImage",
            })
        );
    }

    #[test]
    fn a_missing_windows_installer_matters_only_when_windows_is_published() {
        let without_windows: Vec<&str> = DRAFT
            .iter()
            .copied()
            .filter(|name| !name.contains("setup.exe"))
            .collect();

        assert!(feed_entries(&without_windows, WindowsBuild::Withheld).is_ok());
        assert_eq!(
            feed_entries(&without_windows, WindowsBuild::Published),
            Err(ReleaseSetError::Missing {
                platform: "windows-x86_64",
                suffix: "-setup.exe",
            })
        );
    }

    #[test]
    fn two_candidates_for_one_platform_are_refused() {
        let mut twice = DRAFT.to_vec();
        twice.push("Oikonomia_0.2.0_aarch64.AppImage");

        let err = feed_entries(&twice, WindowsBuild::Withheld).expect_err("ambiguous");

        assert_eq!(
            err.to_string(),
            "expected one .AppImage file for linux-x86_64, found: \
             Oikonomia_0.2.0_amd64.AppImage, Oikonomia_0.2.0_aarch64.AppImage"
        );
    }

    #[test]
    fn published_release_keeps_installers_signatures_and_the_feed() {
        let kept: Vec<&str> = DRAFT
            .iter()
            .copied()
            .filter(|name| is_published_asset(name, WindowsBuild::Published))
            .collect();

        assert_eq!(kept, DRAFT[..11]);
    }

    #[test]
    fn withheld_windows_installer_and_its_signature_are_removed() {
        let removed: Vec<&str> = DRAFT
            .iter()
            .copied()
            .filter(|name| !is_published_asset(name, WindowsBuild::Withheld))
            .collect();

        assert_eq!(
            removed,
            [
                "Oikonomia_0.2.0_x64-setup.exe",
                "Oikonomia_0.2.0_x64-setup.exe.sig",
                "SHA256SUMS.txt",
            ]
        );
    }

    #[test]
    fn checksum_file_is_published_and_lists_every_other_published_file() {
        let mut release = DRAFT.to_vec();
        release.push(CHECKSUMS_FILE);

        assert!(is_published_asset(CHECKSUMS_FILE, WindowsBuild::Withheld));

        let listed = checksummed_assets(&release, WindowsBuild::Withheld);
        let mut expected: Vec<&str> = release
            .iter()
            .copied()
            .filter(|name| is_published_asset(name, WindowsBuild::Withheld))
            .filter(|name| *name != CHECKSUMS_FILE)
            .collect();
        expected.sort_unstable();

        assert_eq!(listed, expected);
        assert!(listed.contains(&"Oikonomia_0.2.0_amd64.deb"));
        assert!(listed.contains(&"latest.json"));
        assert!(!listed.contains(&CHECKSUMS_FILE));
    }

    #[test]
    fn checksum_file_never_lists_a_withheld_or_stray_file() {
        let listed = checksummed_assets(&DRAFT, WindowsBuild::Withheld);

        assert!(!listed.iter().any(|name| name.contains("setup.exe")));
        assert!(!listed.contains(&"SHA256SUMS.txt"));

        let with_windows = checksummed_assets(&DRAFT, WindowsBuild::Published);
        assert!(with_windows.contains(&"Oikonomia_0.2.0_x64-setup.exe"));
    }

    #[test]
    fn checksum_lines_use_the_format_sha256sum_checks() {
        // Two spaces between hash and name: text mode in `sha256sum --check`.
        assert_eq!(
            checksum_line(&"ab".repeat(32), "Oikonomia_0.2.0_amd64.deb"),
            format!("{}  Oikonomia_0.2.0_amd64.deb\n", "ab".repeat(32))
        );
    }

    #[test]
    fn unsupported_package_formats_are_never_published() {
        for name in [
            "Oikonomia_0.2.0_x64_en-US.msi",
            "Oikonomia-0.2.0-1.x86_64.rpm",
            "Oikonomia_0.2.0_x64-setup.nsis.zip",
            "latest.json.bak",
        ] {
            assert!(
                !is_published_asset(name, WindowsBuild::Published),
                "{name} must not be published"
            );
        }
    }
}
