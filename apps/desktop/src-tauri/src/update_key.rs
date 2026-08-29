//! Baked updater minisign public key (Tauri / `tauri signer generate` format).
//!
//! This is the Ops-sent production public half. Do not invent a replacement
//! and do not treat it as a throwaway fixture. The private key never enters
//! this repository, CI logs, the vault, or `.lic` files. This is not
//! [`oikonomia_core::license::PRODUCTION_PUBLIC_KEY_HEX`].
//!
//! An empty value fails compile so a future blank-out is a CI fail.

/// Tauri-format minisign public key (base64 of the minisign public-key file).
///
/// Must stay identical to `tauri.conf.json` `plugins.updater.pubkey`.
/// Exact string from Ops; do not generate another key.
pub const UPDATER_PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDQ0NkIxNkZDMkE1MDBCOApSV1M0QUtYQ2I3RkdCR0hIQWVaTlFadDZHbGppOWkrejQ1M0c3WWFKMmx1ek50R09pZ1lQSnVJeAo=";

const _: () = assert!(
    !UPDATER_PUBLIC_KEY.is_empty(),
    "updater public key is empty; Ops must bake UPDATER_PUBLIC_KEY and plugins.updater.pubkey"
);
