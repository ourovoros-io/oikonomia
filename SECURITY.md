# Security policy

## Reporting a vulnerability

Email **info@ourovoros.io**. Please do not open a public issue for a
vulnerability. Never attach a vault or backup file to a report.

Include the app version (Settings, or the unlock screen), your operating
system, and steps to reproduce. You will get an acknowledgement, and a fix or
a decision is published in the release notes.

## Supported versions

Only the latest release receives security fixes.

## What Oikonomia protects

- The vault is a SQLCipher database keyed from your master password with
  Argon2id. A stolen disk, or a stolen backup file, does not reveal your books.
- The app makes no background network connection. The only network action is
  the update check you click on the unlock screen, which verifies a minisign
  signature before anything is installed.

## What it does not protect

- Malware running as you while the vault is unlocked, keyloggers, and memory
  forensics on a running app.
- A lost master password. There is no recovery key.
