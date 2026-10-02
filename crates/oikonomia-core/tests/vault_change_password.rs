//! Master password change: `SQLCipher` rekey + header salt rotation.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::error::Error;
use oikonomia_core::error::ValidationError;
use oikonomia_core::vault::{Vault, VaultStatus, vault_db_path};
use tempfile::TempDir;

const OLD: &str = "old password 12345";
const NEW: &str = "new password 12345";

#[test]
fn unlock_missing_db_is_corrupt_not_wrong_password() {
    let dir = TempDir::new().expect("dir");
    let mut vault = Vault::open_path(dir.path()).expect("open");
    vault.init(OLD).expect("init");
    vault.lock();
    std::fs::remove_file(vault_db_path(dir.path())).expect("rm db");

    let mut reopened = Vault::open_path(dir.path()).expect("reopen");
    let err = reopened.unlock(OLD).expect_err("must not create empty db");
    assert!(
        matches!(err, Error::VaultCorrupt(_)),
        "missing db must be corrupt, got {err:?}"
    );
    assert!(
        !vault_db_path(dir.path()).exists(),
        "must not plant a new ciphertext"
    );
}

fn init_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    vault.init(OLD).expect("init");
    (dir, vault)
}

#[test]
fn change_password_rekeys_vault() {
    let (dir, mut vault) = init_vault();

    vault.change_password(OLD, NEW).expect("change password");
    assert_eq!(vault.status(), VaultStatus::Unlocked);

    vault.lock();
    assert!(
        vault.unlock(OLD).is_err(),
        "old password must no longer open the vault"
    );
    vault.unlock(NEW).expect("new password unlocks");

    // A fresh Vault instance (fresh header read) must also accept only the new password.
    let mut reopened = Vault::open_path(dir.path()).expect("reopen");
    assert!(reopened.unlock(OLD).is_err());
    reopened.unlock(NEW).expect("new password after reopen");
}

#[test]
fn change_password_rejects_wrong_old_and_weak_new() {
    let (_dir, mut vault) = init_vault();

    assert_eq!(
        vault.change_password("not the password", NEW),
        Err(Error::InvalidPassword),
        "wrong old password must be rejected"
    );
    assert_eq!(
        vault.status(),
        VaultStatus::Unlocked,
        "a failed verification must not lock the vault"
    );

    let weak = vault.change_password(OLD, "short");
    assert!(
        matches!(
            weak,
            Err(Error::Validation(ValidationError::PasswordTooShort {
                min: 12
            }))
        ),
        "weak new password must be rejected, got {weak:?}"
    );
    assert_eq!(vault.status(), VaultStatus::Unlocked);

    // Vault still opens with the old password after failed attempts.
    vault.lock();
    vault.unlock(OLD).expect("old password still valid");
}

#[test]
fn interrupted_change_recovers_via_staged_header() {
    let (dir, mut vault) = init_vault();

    // Complete a change, then reconstruct the crash state: database already
    // rekeyed, real header still the old one, new header only staged.
    let header_path = dir.path().join("vault.header.json");
    let staged_path = dir.path().join("vault.header.json.tmp");
    let old_header = std::fs::read_to_string(&header_path).expect("read old header");

    vault.change_password(OLD, NEW).expect("change password");
    drop(vault);

    let new_header = std::fs::read_to_string(&header_path).expect("read new header");
    std::fs::write(&staged_path, new_header).expect("stage new header");
    std::fs::write(&header_path, old_header).expect("restore old header");

    let mut recovered = Vault::open_path(dir.path()).expect("reopen");
    assert!(
        recovered.unlock(OLD).is_err(),
        "old password no longer matches the rekeyed database"
    );
    recovered
        .unlock(NEW)
        .expect("staged-header recovery unlocks");
    assert!(
        !staged_path.exists(),
        "staged header must be promoted to the real header"
    );

    recovered.lock();
    recovered
        .unlock(NEW)
        .expect("promoted header works on its own");
}

#[test]
fn init_rejects_password_shorter_than_min() {
    let dir = TempDir::new().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    let err = vault.init("short");
    assert!(
        matches!(
            err,
            Err(Error::Validation(ValidationError::PasswordTooShort {
                min: 12
            }))
        ),
        "weak init password must be rejected, got {err:?}"
    );
    assert_eq!(vault.status(), VaultStatus::Uninitialized);
    assert!(
        !vault_db_path(dir.path()).exists(),
        "rejected init must not leave a database"
    );
}

#[test]
fn change_password_requires_initialized_vault() {
    let dir = TempDir::new().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    assert_eq!(
        vault.change_password(OLD, NEW),
        Err(Error::VaultUninitialized)
    );
}
