//! Allow-list for every URL this crate fetches.

use url::Url;

/// Hosts GitHub uses for release assets. GitHub is the CDN, not the trust root.
const PRODUCTION_HOSTS: &[&str] = &[
    "github.com",
    "objects.githubusercontent.com",
    "github-releases.githubusercontent.com",
];

/// Which hosts (and schemes) a check/install may contact.
#[derive(Debug, Clone)]
pub struct HostPolicy {
    allow_http: bool,
    hosts: Vec<String>,
}

impl HostPolicy {
    /// Production: https only, GitHub release hosts only.
    #[must_use]
    pub fn production() -> Self {
        let mut hosts = Vec::new();
        for host in PRODUCTION_HOSTS {
            hosts.push((*host).to_owned());
        }
        Self {
            allow_http: false,
            hosts,
        }
    }

    /// True when `url` may be fetched (manifest, detached sig, or artifact).
    #[must_use]
    pub fn is_allowed_fetch_url(&self, url: &Url) -> bool {
        let scheme_ok = url.scheme() == "https" || (self.allow_http && url.scheme() == "http");
        if !scheme_ok {
            return false;
        }
        let Some(host) = url.host_str() else {
            return false;
        };
        for allowed in &self.hosts {
            if allowed == host {
                return true;
            }
        }
        false
    }

    /// Artifact URLs must pass [`Self::is_allowed_fetch_url`] and must not be `.deb`.
    ///
    /// Linux in-app updates are `AppImage` only; `.deb` stays a manual download.
    #[must_use]
    pub fn is_allowed_artifact_url(&self, url: &Url) -> bool {
        if !self.is_allowed_fetch_url(url) {
            return false;
        }
        let path = url.path();
        if path_ends_with_ignore_ascii_case(path, ".deb") {
            return false;
        }
        true
    }
}

fn path_ends_with_ignore_ascii_case(path: &str, suffix: &str) -> bool {
    let path = path.as_bytes();
    let suffix = suffix.as_bytes();
    if path.len() < suffix.len() {
        return false;
    }
    let start = path.len() - suffix.len();
    path[start..].eq_ignore_ascii_case(suffix)
}

#[cfg(test)]
impl HostPolicy {
    /// Local httptest servers. Never used by the desktop production constructor.
    #[must_use]
    pub fn test_http_hosts(hosts: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let mut allowed = Vec::new();
        for host in hosts {
            allowed.push(host.into());
        }
        Self {
            allow_http: true,
            hosts: allowed,
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use super::HostPolicy;
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
    fn production_allows_github_https_appimage() {
        let policy = HostPolicy::production();
        let url = Url::parse(
            "https://github.com/ourovoros-io/oikonomia-releases/releases/download/v0.2.0/Oikonomia.AppImage",
        )
        .expect("url");
        assert!(policy.is_allowed_artifact_url(&url));
    }

    #[test]
    fn production_rejects_deb_even_on_github() {
        let policy = HostPolicy::production();
        let url = Url::parse(
            "https://github.com/ourovoros-io/oikonomia-releases/releases/download/v0.2.0/oikonomia.deb",
        )
        .expect("url");
        assert!(policy.is_allowed_fetch_url(&url));
        assert!(!policy.is_allowed_artifact_url(&url));
    }
}
