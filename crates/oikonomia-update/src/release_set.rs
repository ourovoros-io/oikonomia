//! Which files of a draft release are published, and which feed entry each
//! one fills.
//!
//! The promote workflow asks this module instead of matching file names in
//! shell, so the rules are tested and the feed and the allow-list cannot
//! disagree.

use thiserror::Error;

/// Whether the Windows installer in a draft is published.
///
/// The release workflow always builds it. Promotion publishes it only when
/// the repository's `WINDOWS_SIGNING` variable records a deliberate choice
/// (today only `none`: shipped without Authenticode, so a first install
/// shows the "Windows protected your PC" warning). The in-app update of
/// either kind is verified with the updater minisign key, not Authenticode.
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

/// Version-free copy of the macOS disk image. getoikonomia.app links to
/// `releases/latest/download/<name>`, so these names are a contract with the
/// site and must not change without it.
pub const FIXED_MACOS_DMG: &str = "Oikonomia-macos-arm64.dmg";
/// Version-free copy of the Windows installer, published only with Windows.
pub const FIXED_WINDOWS_SETUP: &str = "Oikonomia-windows-x64-setup.exe";
/// Version-free copy of the Linux `AppImage`.
pub const FIXED_LINUX_APPIMAGE: &str = "Oikonomia-linux-x86_64.AppImage";
/// Version-free copy of the Debian package.
pub const FIXED_LINUX_DEB: &str = "Oikonomia-linux-amd64.deb";

/// One version-free copy: the fixed name, which platform it is for, and the
/// suffix of the versioned file it copies.
struct FixedCopy {
    name: &'static str,
    platform: &'static str,
    suffix: &'static str,
}

const FIXED_COPIES: [FixedCopy; 4] = [
    FixedCopy {
        name: FIXED_MACOS_DMG,
        platform: "macos",
        suffix: ".dmg",
    },
    FixedCopy {
        name: FIXED_LINUX_APPIMAGE,
        platform: "linux-appimage",
        suffix: ".AppImage",
    },
    FixedCopy {
        name: FIXED_LINUX_DEB,
        platform: "linux-deb",
        suffix: ".deb",
    },
    FixedCopy {
        name: FIXED_WINDOWS_SETUP,
        platform: "windows",
        suffix: "-setup.exe",
    },
];

/// True for one of the version-free copy names, whatever the Windows choice.
#[must_use]
pub fn is_fixed_name(file_name: &str) -> bool {
    FIXED_COPIES.iter().any(|copy| copy.name == file_name)
}

/// The version-free names a release publishes, in a stable order.
#[must_use]
pub fn fixed_names(windows: WindowsBuild) -> Vec<&'static str> {
    FIXED_COPIES
        .iter()
        .filter(|copy| copy.platform != "windows" || windows == WindowsBuild::Published)
        .map(|copy| copy.name)
        .collect()
}

/// The `(versioned source, fixed name)` pairs to copy before publishing.
///
/// Each fixed name needs exactly one versioned file among `file_names`.
/// Fixed names already present (from an earlier, interrupted promotion) are
/// never treated as a source.
///
/// # Errors
///
/// [`ReleaseSetError::Missing`] or [`ReleaseSetError::Ambiguous`] when a
/// fixed name has no source or more than one.
pub fn fixed_name_copies<'a>(
    file_names: &[&'a str],
    windows: WindowsBuild,
) -> Result<Vec<(&'a str, &'static str)>, ReleaseSetError> {
    let mut copies = Vec::new();

    for copy in &FIXED_COPIES {
        if copy.platform == "windows" && windows == WindowsBuild::Withheld {
            continue;
        }

        let mut candidates = Vec::new();
        for name in file_names {
            if !is_fixed_name(name) && name.ends_with(copy.suffix) {
                candidates.push(*name);
            }
        }

        match candidates.as_slice() {
            [] => {
                return Err(ReleaseSetError::Missing {
                    platform: copy.platform,
                    suffix: copy.suffix,
                });
            }
            [only] => copies.push((*only, copy.name)),
            _ => {
                return Err(ReleaseSetError::Ambiguous {
                    platform: copy.platform,
                    suffix: copy.suffix,
                    found: candidates.join(", "),
                });
            }
        }
    }

    Ok(copies)
}

fn platforms(windows: WindowsBuild) -> Vec<FeedPlatform> {
    match windows {
        WindowsBuild::Withheld => vec![MACOS, LINUX],
        WindowsBuild::Published => vec![MACOS, LINUX, WINDOWS],
    }
}

/// The platform keys `latest.json` holds for this release, no more and no
/// fewer, in a stable order.
#[must_use]
pub fn feed_platform_keys(windows: WindowsBuild) -> Vec<&'static str> {
    platforms(windows)
        .iter()
        .map(|platform| platform.key)
        .collect()
}

/// One installer the update client can be asked to download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdaterArtifactKind {
    /// Feed key, for example `windows-x86_64`.
    pub platform: &'static str,
    /// File-name suffix of the artifact that key installs.
    pub suffix: &'static str,
}

/// Every artifact a client downloads, Windows included.
///
/// Promotion can withhold Windows, but the Windows release job still builds
/// the installer. The size gate has to recognize it there, before a feed
/// exists.
#[must_use]
pub const fn updater_artifact_kinds() -> &'static [UpdaterArtifactKind] {
    const KINDS: [UpdaterArtifactKind; 3] = [
        UpdaterArtifactKind {
            platform: MACOS.key,
            suffix: MACOS.suffix,
        },
        UpdaterArtifactKind {
            platform: LINUX.key,
            suffix: LINUX.suffix,
        },
        UpdaterArtifactKind {
            platform: WINDOWS.key,
            suffix: WINDOWS.suffix,
        },
    ];
    &KINDS
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
            if !is_fixed_name(name) && name.ends_with(platform.suffix) {
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
/// a manual download, the signature of either, the feed itself, the
/// checksum file, or a version-free copy. Everything else
/// in the draft is deleted before publishing.
#[must_use]
pub fn is_published_asset(file_name: &str, windows: WindowsBuild) -> bool {
    if FEED_FILES.contains(&file_name) || file_name == CHECKSUMS_FILE {
        return true;
    }

    // Decided by name, not suffix: the Windows copy ends in `-setup.exe` too,
    // and a fixed name never has a detached signature.
    if is_fixed_name(file_name) {
        return fixed_names(windows).contains(&file_name);
    }
    if file_name.strip_suffix(".sig").is_some_and(is_fixed_name) {
        return false;
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
mod tests {
    use super::{
        CHECKSUMS_FILE, FIXED_LINUX_APPIMAGE, FIXED_LINUX_DEB, FIXED_MACOS_DMG,
        FIXED_WINDOWS_SETUP, ReleaseSetError, UpdaterArtifactKind, WindowsBuild, checksum_line,
        checksummed_assets, feed_entries, feed_platform_keys, fixed_name_copies, fixed_names,
        is_published_asset, updater_artifact_kinds,
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
    fn updater_artifact_kinds_are_the_feed_suffixes_including_windows() {
        assert_eq!(
            updater_artifact_kinds(),
            &[
                UpdaterArtifactKind {
                    platform: "darwin-aarch64",
                    suffix: ".app.tar.gz",
                },
                UpdaterArtifactKind {
                    platform: "linux-x86_64",
                    suffix: ".AppImage",
                },
                UpdaterArtifactKind {
                    platform: "windows-x86_64",
                    suffix: "-setup.exe",
                },
            ]
        );

        let entries = feed_entries(&DRAFT, WindowsBuild::Published).expect("entries");
        for (key, name) in entries {
            assert!(
                updater_artifact_kinds()
                    .iter()
                    .any(|kind| { kind.platform == key && name.ends_with(kind.suffix) }),
                "{key} {name}"
            );
        }
    }

    #[test]
    fn feed_platform_keys_follow_the_windows_choice() {
        assert_eq!(
            feed_platform_keys(WindowsBuild::Withheld),
            ["darwin-aarch64", "linux-x86_64"]
        );
        assert_eq!(
            feed_platform_keys(WindowsBuild::Published),
            ["darwin-aarch64", "linux-x86_64", "windows-x86_64"]
        );

        // The keys a verifier expects are exactly the ones assembly fills.
        let entries = feed_entries(&DRAFT, WindowsBuild::Published).expect("entries");
        let keys: Vec<&str> = entries.iter().map(|(key, _)| *key).collect();
        assert_eq!(keys, feed_platform_keys(WindowsBuild::Published));
    }

    #[test]
    fn fixed_names_with_windows_are_the_four_site_downloads() {
        assert_eq!(
            fixed_names(WindowsBuild::Published),
            [
                FIXED_MACOS_DMG,
                FIXED_LINUX_APPIMAGE,
                FIXED_LINUX_DEB,
                FIXED_WINDOWS_SETUP
            ]
        );
    }

    #[test]
    fn checksum_file_covers_all_four_downloads_when_windows_is_published() {
        let mut release = DRAFT.to_vec();
        release.extend(fixed_names(WindowsBuild::Published));
        let listed = checksummed_assets(&release, WindowsBuild::Published);

        for name in fixed_names(WindowsBuild::Published) {
            assert!(listed.contains(&name), "{name} missing from SHA256SUMS");
        }
        assert!(listed.contains(&"Oikonomia_0.2.0_x64-setup.exe.sig"));
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

    #[test]
    fn fixed_names_match_the_site_download_links() {
        // getoikonomia.app (worker/index.ts) redirects to
        // releases/latest/download/<name> with exactly these names.
        assert_eq!(FIXED_MACOS_DMG, "Oikonomia-macos-arm64.dmg");
        assert_eq!(FIXED_WINDOWS_SETUP, "Oikonomia-windows-x64-setup.exe");
        assert_eq!(FIXED_LINUX_APPIMAGE, "Oikonomia-linux-x86_64.AppImage");
        assert_eq!(FIXED_LINUX_DEB, "Oikonomia-linux-amd64.deb");
        assert_eq!(CHECKSUMS_FILE, "SHA256SUMS");
    }

    #[test]
    fn fixed_copies_come_from_the_versioned_files() {
        let copies = fixed_name_copies(&DRAFT, WindowsBuild::Withheld).expect("copies");

        assert_eq!(
            copies,
            [
                ("Oikonomia_0.2.0_aarch64.dmg", FIXED_MACOS_DMG),
                ("Oikonomia_0.2.0_amd64.AppImage", FIXED_LINUX_APPIMAGE),
                ("Oikonomia_0.2.0_amd64.deb", FIXED_LINUX_DEB),
            ]
        );

        let with_windows = fixed_name_copies(&DRAFT, WindowsBuild::Published).expect("copies");
        assert_eq!(
            with_windows.last(),
            Some(&("Oikonomia_0.2.0_x64-setup.exe", FIXED_WINDOWS_SETUP))
        );
        assert_eq!(with_windows.len(), 4);
    }

    #[test]
    fn a_missing_source_for_a_fixed_copy_stops_the_promotion() {
        let without_deb: Vec<&str> = DRAFT
            .iter()
            .copied()
            .filter(|name| !name.contains(".deb"))
            .collect();

        assert_eq!(
            fixed_name_copies(&without_deb, WindowsBuild::Withheld),
            Err(ReleaseSetError::Missing {
                platform: "linux-deb",
                suffix: ".deb",
            })
        );
    }

    #[test]
    fn copies_left_by_an_interrupted_promotion_are_not_sources_or_feed_artifacts() {
        let mut rerun = DRAFT.to_vec();
        rerun.extend([
            FIXED_MACOS_DMG,
            FIXED_LINUX_APPIMAGE,
            FIXED_LINUX_DEB,
            FIXED_WINDOWS_SETUP,
        ]);

        assert_eq!(
            fixed_name_copies(&rerun, WindowsBuild::Published).expect("copies"),
            fixed_name_copies(&DRAFT, WindowsBuild::Published).expect("copies")
        );
        assert_eq!(
            feed_entries(&rerun, WindowsBuild::Published).expect("entries"),
            feed_entries(&DRAFT, WindowsBuild::Published).expect("entries")
        );
    }

    #[test]
    fn fixed_windows_copy_is_published_only_with_windows() {
        assert_eq!(
            fixed_names(WindowsBuild::Withheld),
            [FIXED_MACOS_DMG, FIXED_LINUX_APPIMAGE, FIXED_LINUX_DEB]
        );
        assert!(!is_published_asset(
            FIXED_WINDOWS_SETUP,
            WindowsBuild::Withheld
        ));
        assert!(is_published_asset(
            FIXED_WINDOWS_SETUP,
            WindowsBuild::Published
        ));
        for name in fixed_names(WindowsBuild::Withheld) {
            assert!(is_published_asset(name, WindowsBuild::Withheld), "{name}");
        }
    }

    #[test]
    fn fixed_copies_carry_no_signature_and_are_checksummed() {
        let sig = format!("{FIXED_LINUX_APPIMAGE}.sig");
        assert!(!is_published_asset(&sig, WindowsBuild::Published));

        let mut release = DRAFT.to_vec();
        release.extend(fixed_names(WindowsBuild::Withheld));
        release.push(FIXED_WINDOWS_SETUP);
        let listed = checksummed_assets(&release, WindowsBuild::Withheld);

        for name in fixed_names(WindowsBuild::Withheld) {
            assert!(listed.contains(&name), "{name} missing from SHA256SUMS");
        }
        assert!(!listed.contains(&FIXED_WINDOWS_SETUP));
    }
}
