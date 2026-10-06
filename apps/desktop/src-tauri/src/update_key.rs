//! Baked updater minisign public key (Tauri / `tauri signer generate` format).
//!
//! This is the Ops-sent production public half. Do not invent a replacement
//! and do not treat it as a throwaway fixture. The private key never enters
//! this repository, CI logs, or the vault.
//!
//! An empty value fails compile so a future blank-out is a CI fail.

/// Tauri-format minisign public key (base64 of the minisign public-key file).
///
/// Must stay identical to `tauri.conf.json` `plugins.updater.pubkey`.
/// Exact string from Ops; do not generate another key.
pub(crate) const UPDATER_PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDM2RkYxMTYzMThFRDNDNkIKUldSclBPMFlZeEgvTnVjaENqMnQxU1VRY0VqYXRveEVJczE5bXF5dDFaWmtzMFF1d0Q5RGlIRlUK";

const _: () = assert!(
    !UPDATER_PUBLIC_KEY.is_empty(),
    "updater public key is empty; Ops must bake UPDATER_PUBLIC_KEY and plugins.updater.pubkey"
);
