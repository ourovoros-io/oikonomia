//! Allow-list for every URL this crate fetches.
//!
//! The allow-list is not what makes an update trustworthy; the signature
//! over the feed and over the artifact is. It limits where the app will
//! connect at all. Without it a redirect, or a manifest signed by mistake
//! with a wrong URL, could send the request, which names the running version
//! and platform, to a host that has nothing to do with the release. With it
//! the only hosts ever contacted are the ones GitHub serves release assets
//! from, and only over https.
//!
//! The client asks [`HostPolicy::is_allowed_fetch_url`] before every request,
//! including each hop of a redirect it follows.

use url::Url;

/// Hosts GitHub uses for release assets. GitHub is the CDN, not the trust root.
const PRODUCTION_HOSTS: &[&str] = &[
    "github.com",
    "objects.githubusercontent.com",
    "github-releases.githubusercontent.com",
    // GitHub's current release-asset CDN; trust is the minisign key, not the CDN.
    "release-assets.githubusercontent.com",
];

/// The schemes and hosts a check or an install may contact.
#[derive(Debug, Clone)]
pub(crate) struct HostPolicy {
    /// Whether plain `http` is accepted beside `https`. Only the test policy
    /// sets it, for servers on the loopback interface.
    allow_http: bool,
    /// The host names a URL may have, compared exactly: a subdomain of a
    /// listed host is not allowed unless it is listed itself.
    hosts: Vec<String>,
}

impl HostPolicy {
    /// Returns the policy the application ships with: https only, and only
    /// the hosts GitHub serves release assets from.
    #[must_use]
    pub(crate) fn production() -> Self {
        Self {
            allow_http: false,
            hosts: PRODUCTION_HOSTS
                .iter()
                .map(|host| (*host).to_owned())
                .collect(),
        }
    }

    /// Returns whether `url` may be requested: the feed, its detached
    /// signature, an artifact, or a redirect from any of them.
    #[must_use]
    pub(crate) fn is_allowed_fetch_url(&self, url: &Url) -> bool {
        let scheme_allowed = url.scheme() == "https" || (self.allow_http && url.scheme() == "http");

        scheme_allowed
            && url
                .host_str()
                .is_some_and(|host| self.hosts.iter().any(|allowed| allowed == host))
    }

    /// Returns whether `url` may be offered as the artifact to install: it
    /// must pass [`Self::is_allowed_fetch_url`] and must not be a `.deb`.
    ///
    /// A Debian package is installed by the system package manager, never by
    /// the app, so a manifest that offers one for in-app install is refused.
    /// On Linux the in-app artifact is the `AppImage`.
    #[must_use]
    pub(crate) fn is_allowed_artifact_url(&self, url: &Url) -> bool {
        self.is_allowed_fetch_url(url) && !ends_with_ignore_ascii_case(url.path(), ".deb")
    }
}

#[cfg(test)]
impl HostPolicy {
    /// Returns a policy for local test servers: `http` is accepted, and only
    /// `hosts` may be contacted.
    ///
    /// Compiled for tests only, so a release build has no way to accept
    /// plain `http` or a host outside [`PRODUCTION_HOSTS`].
    #[must_use]
    pub(crate) fn test_http_hosts(hosts: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            allow_http: true,
            hosts: hosts.into_iter().map(Into::into).collect(),
        }
    }
}

/// Returns whether `text` ends with `suffix`, comparing ASCII letters without
/// regard to case.
///
/// Compares bytes, so no string is sliced and a multi-byte character near
/// the end of `text` cannot cause a panic.
fn ends_with_ignore_ascii_case(text: &str, suffix: &str) -> bool {
    let text = text.as_bytes();
    let suffix = suffix.as_bytes();

    text.len()
        .checked_sub(suffix.len())
        .and_then(|start| text.get(start..))
        .is_some_and(|tail| tail.eq_ignore_ascii_case(suffix))
}

#[cfg(test)]
mod tests {
    use super::{HostPolicy, ends_with_ignore_ascii_case};
    use url::Url;

    #[test]
    fn production_rejects_file_and_http_and_random_host() {
        let policy = HostPolicy::production();
        let file = Url::parse("file:///tmp/evil").expect("url");
        let http = Url::parse("http://github.com/x").expect("url");
        let other = Url::parse("https://evil.example/payload").expect("url");
        assert!(!policy.is_allowed_fetch_url(&file));
        assert!(!policy.is_allowed_artifact_url(&file));
        assert!(!policy.is_allowed_fetch_url(&http));
        assert!(!policy.is_allowed_artifact_url(&other));
    }

    #[test]
    fn production_rejects_a_subdomain_and_a_lookalike_of_an_allowed_host() {
        let policy = HostPolicy::production();

        for url in [
            "https://evil.github.com/payload",
            "https://github.com.evil.example/payload",
            "https://notgithub.com/payload",
        ] {
            let url = Url::parse(url).expect("url");
            assert!(!policy.is_allowed_fetch_url(&url), "{url}");
        }
    }

    #[test]
    fn production_allows_github_https_appimage() {
        let policy = HostPolicy::production();
        let url = Url::parse(
            "https://github.com/ourovoros-io/oikonomia/releases/download/v0.2.0/Oikonomia.AppImage",
        )
        .expect("url");
        assert!(policy.is_allowed_artifact_url(&url));
    }

    #[test]
    fn production_rejects_deb_even_on_github() {
        let policy = HostPolicy::production();
        let url = Url::parse(
            "https://github.com/ourovoros-io/oikonomia/releases/download/v0.2.0/oikonomia.deb",
        )
        .expect("url");
        assert!(policy.is_allowed_fetch_url(&url));
        assert!(!policy.is_allowed_artifact_url(&url));

        let shouting =
            Url::parse("https://github.com/o/r/releases/download/v1/OIKONOMIA.DEB").expect("url");
        assert!(!policy.is_allowed_artifact_url(&shouting));
    }

    #[test]
    fn production_allows_release_assets_redirect_host() {
        // GitHub answers a release-asset download with a 302 to
        // release-assets.githubusercontent.com. `fetch_once` in `client.rs`
        // follows it and checks `is_allowed_fetch_url` on every hop, so the
        // manifest, its signature and the artifact all need this host allowed.
        let policy = HostPolicy::production();
        let url = Url::parse(
            "https://release-assets.githubusercontent.com/github-production-release-asset/000000000/abc123def",
        )
        .expect("url");
        assert!(policy.is_allowed_fetch_url(&url));
        assert!(policy.is_allowed_artifact_url(&url));

        let deb_url = Url::parse(
            "https://release-assets.githubusercontent.com/github-production-release-asset/000000000/oikonomia.deb",
        )
        .expect("url");
        assert!(policy.is_allowed_fetch_url(&deb_url));
        assert!(!policy.is_allowed_artifact_url(&deb_url));

        let other = Url::parse("https://evil.example/payload").expect("url");
        assert!(!policy.is_allowed_fetch_url(&other));
        assert!(!policy.is_allowed_artifact_url(&other));
    }

    #[test]
    fn suffix_comparison_handles_short_and_multi_byte_text() {
        assert!(ends_with_ignore_ascii_case("a.DeB", ".deb"));
        assert!(!ends_with_ignore_ascii_case("deb", ".deb"));
        assert!(!ends_with_ignore_ascii_case("", ".deb"));
        // The last four bytes start inside the two-byte `é`.
        assert!(!ends_with_ignore_ascii_case("\u{e9}deb", ".deb"));
        assert!(ends_with_ignore_ascii_case("\u{e9}.deb", ".deb"));
    }
}
