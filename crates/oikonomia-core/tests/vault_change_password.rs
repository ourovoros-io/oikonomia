//! Master password change: `SQLCipher` rekey + header salt rotation, and what
//! a backup or restore does while a change is unfinished.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use std::path::PathBuf;

use oikonomia_core::error::Error;
use oikonomia_core::error::ValidationError;
use oikonomia_core::vault::{
    Vault, VaultStatus, backup_to_path, restore_from_path, vault_db_path, vault_header_path,
};
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

/// A vault in the state a password change leaves when it stops between the
/// rekey and the publishing of the new header: the database is under the
/// key of [`NEW`], the published header is still the old one, and the new
/// header is only staged.
///
/// Built by completing a change and putting the old header back, since no
/// public call stops halfway.
fn vault_with_unfinished_password_change() -> TempDir {
    let (dir, mut vault) = init_vault();
    let header_path = vault_header_path(dir.path());
    let old_header = std::fs::read(&header_path).expect("read old header");

    vault.change_password(OLD, NEW).expect("change password");
    drop(vault);

    let new_header = std::fs::read(&header_path).expect("read new header");
    std::fs::write(staged_header_path(&dir), new_header).expect("stage new header");
    std::fs::write(&header_path, old_header).expect("restore old header");
    dir
}

/// The staged header a password change writes, `vault.header.json.tmp`.
fn staged_header_path(dir: &TempDir) -> PathBuf {
    dir.path().join("vault.header.json.tmp")
}

/// The bytes of the published header, the staged header and the database.
fn vault_files(dir: &TempDir) -> [Vec<u8>; 3] {
    [
        vault_header_path(dir.path()),
        staged_header_path(dir),
        vault_db_path(dir.path()),
    ]
    .map(|path| std::fs::read(path).expect("read vault file"))
}

/// A backup of a separate vault under `password`, for a restore to bring in.
fn archive_of_another_vault(password: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    vault.init(password).expect("init");
    let archive = dir.path().join("other.oikonomia-backup");
    vault.backup_to(&archive).expect("backup");
    (dir, archive)
}

#[test]
fn interrupted_change_recovers_via_staged_header() {
    let dir = vault_with_unfinished_password_change();
    let staged_path = staged_header_path(&dir);

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
fn a_locked_backup_is_refused_while_a_password_change_is_unfinished() {
    let dir = vault_with_unfinished_password_change();
    let vault = Vault::open_path(dir.path()).expect("reopen");
    let dest = TempDir::new().expect("archive dir");
    let archive = dest.path().join("books.oikonomia-backup");

    assert_eq!(
        vault.backup_to(&archive),
        Err(Error::PasswordChangeUnfinished)
    );
    assert_eq!(
        backup_to_path(dir.path(), &archive),
        Err(Error::PasswordChangeUnfinished)
    );
    assert_eq!(
        std::fs::read_dir(dest.path()).expect("archive dir").count(),
        0,
        "no archive and no partial archive"
    );
}

#[test]
fn a_replacing_restore_is_refused_while_a_password_change_is_unfinished() {
    let dir = vault_with_unfinished_password_change();
    let files_before = vault_files(&dir);
    let (_other, archive) = archive_of_another_vault("another password 12345");

    let mut vault = Vault::open_path(dir.path()).expect("reopen");
    assert_eq!(
        vault.restore_from(&archive, true),
        Err(Error::PasswordChangeUnfinished)
    );
    assert_eq!(
        restore_from_path(&archive, dir.path(), true),
        Err(Error::PasswordChangeUnfinished)
    );

    assert_eq!(
        vault_files(&dir),
        files_before,
        "header, staged header and database are untouched"
    );
    let mut names: Vec<_> = std::fs::read_dir(dir.path())
        .expect("data dir")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["vault.db", "vault.header.json", "vault.header.json.tmp"],
        "nothing was unpacked or set aside"
    );
    vault
        .unlock(NEW)
        .expect("the new password still opens the vault");
}

#[test]
fn after_one_unlock_a_backup_of_an_unfinished_change_opens_with_the_new_password() {
    let dir = vault_with_unfinished_password_change();
    let mut vault = Vault::open_path(dir.path()).expect("reopen");
    vault.unlock(NEW).expect("unlock settles the staged header");
    vault.lock();
    let dest = TempDir::new().expect("archive dir");
    let archive = dest.path().join("books.oikonomia-backup");

    vault.backup_to(&archive).expect("locked backup");

    let restore_dir = TempDir::new().expect("restore dir");
    restore_from_path(&archive, restore_dir.path(), false).expect("restore");
    let mut restored = Vault::open_path(restore_dir.path()).expect("open restored");
    assert_eq!(restored.unlock(OLD), Err(Error::InvalidPassword));
    restored
        .unlock(NEW)
        .expect("the restored vault opens with the new password");
}

#[test]
fn after_one_unlock_a_replacing_restore_goes_ahead() {
    let dir = vault_with_unfinished_password_change();
    let (_other, archive) = archive_of_another_vault("another password 12345");
    let mut vault = Vault::open_path(dir.path()).expect("reopen");
    vault.unlock(NEW).expect("unlock settles the staged header");

    vault.restore_from(&archive, true).expect("replace restore");

    assert!(!staged_header_path(&dir).exists());
    vault
        .unlock("another password 12345")
        .expect("the restored vault opens with its own password");
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
