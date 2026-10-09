//! A feed override for testing the whole update flow against a local feed.
//!
//! Compiled only with the `debug-feed` feature, and refused by the compiler
//! in an optimised build, so a shipped binary has no code that reads the
//! variable and does not contain its name. The release workflow builds
//! without the feature and checks each bundled binary for the name.
//!
//! With the variable set to a feed URL, a check fetches that feed, its
//! `.sig` beside it, and the artifact the feed names, from that URL's host
//! only, over `http` or `https`. Nothing else changes: the feed and the
//! artifact must still be signed with the key compiled into the app, so a
//! local feed is signed with that key or the app is built with a test key.

#[cfg(not(debug_assertions))]
compile_error!("the `debug-feed` feature is for debug builds only; release builds never enable it");

use crate::error::{Result, UpdateError};
use crate::hosts::HostPolicy;
use url::Url;

/// The environment variable that names the feed to use instead of the
/// production one.
pub(crate) const FEED_ENV: &str = "OIKONOMIA_UPDATE_FEED";

/// Returns the feed URL and the host policy the variable asks for, or
/// `None` when it is unset or empty.
///
/// # Errors
///
/// As [`from_value`].
pub(crate) fn from_env() -> Result<Option<(Url, HostPolicy)>> {
    let overridden = from_value(std::env::var_os(FEED_ENV).as_deref())?;
    if let Some((feed_url, _policy)) = &overridden {
        log::warn!("update feed overridden by {FEED_ENV}: {feed_url}");
    }
    Ok(overridden)
}

/// Returns the feed URL and the host policy for the variable's `value`, or
/// `None` when it is unset or empty.
///
/// # Errors
///
/// Returns [`UpdateError::InvalidFeedUrl`] when `value` is a URL without a
/// host or does not parse.
fn from_value(value: Option<&std::ffi::OsStr>) -> Result<Option<(Url, HostPolicy)>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.to_string_lossy();
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }

    let feed_url = Url::parse(value).map_err(|_| UpdateError::InvalidFeedUrl)?;
    let Some(host) = feed_url.host_str() else {
        return Err(UpdateError::InvalidFeedUrl);
    };
    let policy = HostPolicy::with_http_hosts([host.to_owned()]);

    Ok(Some((feed_url, policy)))
}

#[cfg(test)]
mod tests {
    use super::from_value;
    use std::ffi::OsStr;
    use url::Url;

    #[test]
    fn an_unset_or_empty_variable_keeps_the_production_feed() {
        assert!(from_value(None).expect("unset").is_none());
        assert!(from_value(Some(OsStr::new("  "))).expect("empty").is_none());
    }

    #[test]
    fn a_local_feed_allows_its_own_host_only() {
        let (feed, policy) = from_value(Some(OsStr::new("http://127.0.0.1:8000/latest.json")))
            .expect("parses")
            .expect("set");

        assert_eq!(feed.as_str(), "http://127.0.0.1:8000/latest.json");
        let same_host = Url::parse("http://127.0.0.1:8000/Oikonomia.AppImage").expect("url");
        let other_host = Url::parse("https://github.com/x").expect("url");
        assert!(policy.is_allowed_artifact_url(&same_host));
        assert!(!policy.is_allowed_fetch_url(&other_host));
    }

    #[test]
    fn a_variable_that_is_not_a_url_fails_instead_of_falling_back() {
        let err = from_value(Some(OsStr::new("not a url"))).expect_err("not a url");

        assert_eq!(err.code(), "update_invalid_feed_url");
    }
}
