//! Master password change: SQLCipher rekey + header salt rotation.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::error::Error;
use oikonomia_core::vault::{Vault, VaultStatus};
use tempfile::TempDir;

const OLD: &str = "old password 12345";
const NEW: &str = "new password 12345";

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

    let weak = vault.change_password(OLD, "short");
    assert!(
        matches!(weak, Err(Error::Validation(_))),
        "weak new password must be rejected, got {weak:?}"
    );

    // Vault still opens with the old password after failed attempts.
    vault.lock();
    vault.unlock(OLD).expect("old password still valid");
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
