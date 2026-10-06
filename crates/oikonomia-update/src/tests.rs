//! Tests of the client against local servers. Nothing here contacts GitHub.
//!
//! These live in `src/` and not in `tests/` because they need what only a
//! unit test can reach: `ClientConfig::for_test` and
//! `HostPolicy::test_http_hosts`, which point the client at a loopback server
//! over plain `http`. Both are compiled under `#[cfg(test)]` only. An
//! integration test in `tests/` links the crate as a release build would
//! see it, so serving it would mean making that override public, and then a
//! shipped application could be pointed away from the signed feed.
//!
//! One module per topic; [`support`] holds what they share. Tests of a single
//! function sit next to it, in the module that defines it.

#![expect(clippy::panic, reason = "tests fail loudly by design")]

mod cache;
mod check;
mod install;
mod redirects;
mod support;
