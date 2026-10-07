//! Master password change: `SQLCipher` rekey + header salt rotation.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use oikonomia_core::error::{Error, ValidationError, VaultCorruption};
use oikonomia_core::vault::{
    Vault, VaultStatus, backup_to_path, restore_from_path, vault_db_path, vault_header_path,
};
use tempfile::TempDir;

const OLD: &str = "old password 12345";
const NEW: &str = "new password 12345";

/// On-disk name of the header a password change stages, spelled out so that
/// renaming it in the crate is a visible format change here.
const STAGED_HEADER: &str = "vault.header.json.tmp";

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
    assert_eq!(
        vault.status(),
        VaultStatus::Unlocked,
        "a vault that was unlocked stays unlocked"
    );

    vault.lock();
    assert_eq!(
        vault.unlock(OLD),
        Err(Error::InvalidPassword),
        "old password must no longer open the vault"
    );
    vault.unlock(NEW).expect("new password unlocks");

    // A fresh Vault instance (fresh header read) must also accept only the new password.
    let mut reopened = Vault::open_path(dir.path()).expect("reopen");
    assert_eq!(reopened.unlock(OLD), Err(Error::InvalidPassword));
    reopened.unlock(NEW).expect("new password after reopen");
}

#[test]
fn change_password_on_a_locked_vault_leaves_it_locked() {
    let (dir, mut vault) = init_vault();
    vault.lock();

    vault.change_password(OLD, NEW).expect("change password");

    assert_eq!(
        vault.status(),
        VaultStatus::Locked,
        "changing the password must not open a session nobody asked for"
    );
    assert_eq!(vault.unlock(OLD), Err(Error::InvalidPassword));
    vault.unlock(NEW).expect("new password unlocks");

    let mut reopened = Vault::open_path(dir.path()).expect("reopen");
    reopened.unlock(NEW).expect("new password after reopen");
}

#[test]
fn change_password_on_an_unlocked_vault_keeps_the_session_usable() {
    let (_dir, mut vault) = init_vault();

    vault.change_password(OLD, NEW).expect("change password");

    assert_eq!(vault.status(), VaultStatus::Unlocked);
    let schema_version: i64 = vault
        .connection()
        .expect("still unlocked")
        .query_row("SELECT schema_version FROM vault_meta", [], |row| {
            row.get(0)
        })
        .expect("the reopened connection reads under the new key");
    assert!(schema_version >= 1);
}

#[test]
fn a_locked_vault_stays_locked_when_the_old_password_is_wrong() {
    let (_dir, mut vault) = init_vault();
    vault.lock();

    assert_eq!(
        vault.change_password("not the password", NEW),
        Err(Error::InvalidPassword)
    );
    assert_eq!(vault.status(), VaultStatus::Locked);
    vault.unlock(OLD).expect("old password still valid");
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

/// Leaves a data directory the way a password change from [`OLD`] to [`NEW`]
/// does when the process dies between the rekey and the header rename: the
/// database is under the new key, the published header is still the old one,
/// and the new header exists only as the staged file.
///
/// The change is completed and the header rename is then taken back.
fn crashed_between_rekey_and_publish() -> TempDir {
    let (dir, mut vault) = init_vault();
    let header_path = vault_header_path(dir.path());
    let old_header = std::fs::read_to_string(&header_path).expect("read old header");

    vault.change_password(OLD, NEW).expect("change password");
    drop(vault);

    let new_header = std::fs::read_to_string(&header_path).expect("read new header");
    std::fs::write(dir.path().join(STAGED_HEADER), new_header).expect("stage new header");
    std::fs::write(&header_path, old_header).expect("put old header back");
    dir
}

/// A destination for an archive, outside any data directory.
fn archive_destination() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().expect("archive dir");
    let archive = dir.path().join("books.oikonomia-backup");
    (dir, archive)
}

#[test]
fn a_locked_backup_is_refused_while_a_password_change_is_unfinished() {
    let dir = crashed_between_rekey_and_publish();
    let (_archive_dir, archive) = archive_destination();
    let unfinished = Error::VaultCorrupt(VaultCorruption::UnfinishedPasswordChange);

    // The published header does not fit the database, so an archive of the
    // two files as they are would open with neither password.
    let refused = backup_to_path(dir.path(), &archive).expect_err("the backup is refused");
    assert_eq!(refused, unfinished.clone());
    assert_eq!(
        refused.code(),
        "vault_unlock_before_backup",
        "the user is told to unlock once, not that the vault is corrupt"
    );
    assert!(!archive.exists(), "no archive is written");

    let vault = Vault::open_path(dir.path()).expect("reopen");
    assert_eq!(
        vault.backup_to(&archive),
        Err(unfinished),
        "a locked handle takes the same path"
    );
}

#[test]
fn unlocking_once_after_an_unfinished_password_change_makes_the_vault_backable() {
    let dir = crashed_between_rekey_and_publish();
    let (_archive_dir, archive) = archive_destination();

    let mut vault = Vault::open_path(dir.path()).expect("reopen");
    vault
        .unlock(NEW)
        .expect("unlock publishes the staged header");
    vault.lock();
    vault.backup_to(&archive).expect("locked backup");

    let restored_dir = TempDir::new().expect("restore dir");
    restore_from_path(&archive, restored_dir.path(), false).expect("restore");
    let mut restored = Vault::open_path(restored_dir.path()).expect("open restored");
    restored
        .unlock(NEW)
        .expect("the archive opens with the new password");
}

#[test]
fn a_staged_header_left_before_the_rekey_is_cleared_by_one_unlock() {
    // A change that died before the rekey leaves a staged header that does
    // not fit. Nothing tells it from the one that does without the password,
    // so the locked backup is refused here too, until an unlock removes it.
    let (dir, mut vault) = init_vault();
    vault.lock();
    std::fs::write(dir.path().join(STAGED_HEADER), b"{ truncated").expect("stage");
    let (_archive_dir, archive) = archive_destination();

    assert_eq!(
        vault.backup_to(&archive),
        Err(Error::VaultCorrupt(
            VaultCorruption::UnfinishedPasswordChange
        ))
    );

    vault.unlock(OLD).expect("the published header still fits");
    vault.lock();
    vault.backup_to(&archive).expect("locked backup");
}

#[test]
fn interrupted_change_recovers_via_staged_header() {
    let dir = crashed_between_rekey_and_publish();
    let staged_path = dir.path().join(STAGED_HEADER);

    let mut recovered = Vault::open_path(dir.path()).expect("reopen");
    assert_eq!(
        recovered.unlock(OLD),
        Err(Error::InvalidPassword),
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
