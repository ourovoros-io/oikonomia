# Security policy

## Reporting a vulnerability

Email **info@ourovoros.io**. Please do not open a public issue for a
vulnerability. Never attach a vault or backup file to a report.

Include the app version (shown under Settings → Support), your operating
system, and steps to reproduce. You will get an acknowledgment, and a fix or
a decision will be announced with the next release.

## Supported versions

Only the latest release receives security fixes.

## What Oikonomia protects

- The vault is a SQLCipher database keyed from your master password with
  Argon2id. A stolen disk, or a stolen backup file, does not reveal your books,
  provided the master password is strong.
- The app makes no background network connection. The only network actions are
  the update check you click on the unlock screen and, if you accept an update,
  its download. Both verify a minisign signature before anything is installed.

## What is not encrypted

- `vault.header.json` holds the salt and key-derivation parameters. It is not
  secret by design.
- `ui-prefs.json` holds interface preferences in plaintext: your language and
  the last entity and account choices made in the quick-add window.
- The window-state file written by the window-state plugin (window size and
  position).
- Anything you export: journal CSV, expense PDF, and documents saved through
  the export action are written as ordinary files. (A vault backup is the
  exception: it stays encrypted.)

## What it does not protect

- Malware running as you while the vault is unlocked, keyloggers, and memory
  forensics on a running app.
- A lost master password. There is no recovery key.
