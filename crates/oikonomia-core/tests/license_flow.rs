//! License + trial against a real vault: backup, export, restore, and the write gate.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]

use ed25519_dalek::{Signer, SigningKey};
use oikonomia_core::csv::export_journal_csv;
use oikonomia_core::documents::{attach_document, delete_document, get_document, list_documents};
use oikonomia_core::domain::ChartTemplate;
use oikonomia_core::error::Error;
use oikonomia_core::ledger::{
    CreateEntity, EntryFilter, PostSimpleEntry, SimpleEntryKind, create_entity,
    create_entity_allowed, get_lock_timeout_secs, list_accounts, list_entities, list_entries,
    post_simple_entry, set_lock_timeout_secs,
};
use oikonomia_core::license::{
    LicenseState, LicenseVerifier, PRODUCT, install_license, license_status, record_trial_start,
    require_writes_allowed, signed_payload, when_writes_allowed, writes_allowed,
};
use oikonomia_core::prefs::load_ui_prefs;
use oikonomia_core::vault::Vault;
use rand::RngCore;
use serde_json::json;
use tempfile::TempDir;

const PASSWORD: &str = "correct horse battery staple";

struct Ephemeral {
    verifier: LicenseVerifier,
    signing: SigningKey,
}

fn ephemeral() -> Ephemeral {
    let mut seed = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut seed);
    let signing = SigningKey::from_bytes(&seed);
    let verifier = LicenseVerifier::from_public_key_bytes(&signing.verifying_key().to_bytes())
        .expect("ephemeral verifying key");
    Ephemeral { verifier, signing }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn sign_lic(signing: &SigningKey, expiry: &str) -> String {
    let email = "buyer@example.com";
    let issued_at = "2026-01-15T12:00:00Z";
    let payload = signed_payload(PRODUCT, expiry, email, issued_at);
    let sig = signing.sign(payload.as_bytes());
    json!({
        "v": 1,
        "product": PRODUCT,
        "expiry": expiry,
        "email": email,
        "issued_at": issued_at,
        "sig": encode_hex(&sig.to_bytes()),
    })
    .to_string()
}

fn setup() -> (TempDir, Vault, Ephemeral) {
    let dir = TempDir::new().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    vault.init(PASSWORD).expect("init");
    (dir, vault, ephemeral())
}

fn seed_books(vault: &Vault) -> oikonomia_core::domain::EntityId {
    let conn = vault.connection().expect("conn");
    let entity = create_entity(
        conn,
        &CreateEntity {
            name: "Personal".into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: Some(1),
        },
    )
    .expect("entity");
    let accounts = list_accounts(conn, entity.id).expect("accounts");
    let wallet = accounts.iter().find(|a| a.code == "1010").expect("wallet");
    let expense = accounts.iter().find(|a| a.code == "5100").expect("expense");
    post_simple_entry(
        conn,
        &PostSimpleEntry {
            entity_id: entity.id,
            kind: SimpleEntryKind::Expense,
            bill_status: None,
            entry_date: "2026-03-15".into(),
            description: "Groceries".into(),
            reference: None,
            amount_minor: 2_500,
            category_account_id: Some(expense.id),
            wallet_account_id: Some(wallet.id),
            payable_account_id: None,
            from_account_id: None,
            to_account_id: None,
        },
    )
    .expect("post");
    entity.id
}

#[test]
fn vault_init_starts_trial_clock_without_unlock() {
    let dir = TempDir::new().expect("tempdir");
    let keys = ephemeral();
    let mut vault = Vault::open_path(dir.path()).expect("open");
    vault.init(PASSWORD).expect("init");
    record_trial_start(dir.path()).expect("stamp after init");

    let stamp = load_ui_prefs(dir.path()).trial_started_at;
    assert!(stamp.is_some(), "init stamps trial_started_at");
    let status = license_status(dir.path(), &keys.verifier).expect("status");
    assert_eq!(status.state, LicenseState::Trial);
    assert!(writes_allowed(&status));

    vault.lock();
    vault.unlock(PASSWORD).expect("later unlock");
    record_trial_start(dir.path()).expect("unlock must not restamp");
    assert_eq!(
        load_ui_prefs(dir.path()).trial_started_at,
        stamp,
        "later unlock must not reset trial_started_at"
    );
}

#[test]
fn first_unlock_sets_trial_once_and_is_writable() {
    let dir = TempDir::new().expect("tempdir");
    let keys = ephemeral();
    let mut vault = Vault::open_path(dir.path()).expect("open");
    vault.init(PASSWORD).expect("init");
    vault.lock();

    assert_eq!(
        license_status(dir.path(), &keys.verifier)
            .expect("pre-unlock")
            .state,
        LicenseState::None
    );

    vault.unlock(PASSWORD).expect("unlock 1");
    record_trial_start(dir.path()).expect("record 1");
    let first = load_ui_prefs(dir.path()).trial_started_at;
    assert!(first.is_some(), "first unlock stamps trial_started_at");

    let status = license_status(dir.path(), &keys.verifier).expect("trial");
    assert_eq!(status.state, LicenseState::Trial);
    assert!(writes_allowed(&status));
    assert!(require_writes_allowed(dir.path(), &keys.verifier).is_ok());

    vault.lock();
    vault.unlock(PASSWORD).expect("unlock 2");
    record_trial_start(dir.path()).expect("record 2");
    assert_eq!(
        load_ui_prefs(dir.path()).trial_started_at,
        first,
        "second unlock must not reset trial_started_at"
    );
}

#[test]
fn expired_license_allows_backup_export_restore_and_blocks_writes() {
    let (dir, vault, keys) = setup();
    let entity_id = seed_books(&vault);

    let body = sign_lic(&keys.signing, "2020-01-01");
    let src = dir.path().join("incoming.lic");
    std::fs::write(&src, &body).expect("write lic");
    let status = install_license(dir.path(), &src, &keys.verifier).expect("install");
    assert_eq!(status.state, LicenseState::Expired);
    assert_eq!(
        require_writes_allowed(dir.path(), &keys.verifier),
        Err(Error::LicenseExpired)
    );

    let csv = {
        let conn = vault.connection().expect("conn");
        export_journal_csv(conn, entity_id).expect("export while expired")
    };
    assert!(csv.contains("Groceries"), "export stays allowed: {csv}");

    let archive_dir = TempDir::new().expect("archive dir");
    let archive = archive_dir.path().join("books.oikonomia-backup");
    vault.backup_to(&archive).expect("backup while expired");

    let restore_dir = TempDir::new().expect("restore");
    oikonomia_core::vault::restore_from_path(&archive, restore_dir.path(), false)
        .expect("restore while expired");
    let mut restored = Vault::open_path(restore_dir.path()).expect("open restored");
    restored.unlock(PASSWORD).expect("unlock restored");
    let conn = restored.connection().expect("restored conn");
    let listed = list_entries(conn, entity_id, &EntryFilter::default()).expect("list restored");
    assert!(
        listed.iter().any(|v| v.entry.description == "Groceries"),
        "restore kept the journal: {listed:?}"
    );

    assert_eq!(
        require_writes_allowed(dir.path(), &keys.verifier),
        Err(Error::LicenseExpired),
        "a write still returns license_expired"
    );
}

#[test]
fn expired_license_rejects_document_attach_and_delete_but_allows_export() {
    let (dir, vault, keys) = setup();
    let entity_id = seed_books(&vault);
    let conn = vault.connection().expect("conn");
    let entry_id = list_entries(conn, entity_id, &EntryFilter::default())
        .expect("list")
        .into_iter()
        .find(|v| v.entry.description == "Groceries")
        .expect("seed entry")
        .entry
        .id;

    let existing = attach_document(
        conn,
        entity_id,
        entry_id,
        "receipt.txt",
        "text/plain",
        b"keep-me",
    )
    .expect("attach while writable");

    let body = sign_lic(&keys.signing, "2020-01-01");
    let src = dir.path().join("incoming.lic");
    std::fs::write(&src, &body).expect("write lic");
    let status = install_license(dir.path(), &src, &keys.verifier).expect("install");
    assert_eq!(status.state, LicenseState::Expired);

    let attach_err = when_writes_allowed(dir.path(), &keys.verifier, || {
        attach_document(
            conn,
            entity_id,
            entry_id,
            "invoice.txt",
            "text/plain",
            b"blocked",
        )
    })
    .expect_err("expired attach");
    assert!(
        matches!(attach_err, Error::LicenseExpired),
        "expired attach must return license_expired: {attach_err:?}"
    );

    let delete_err = when_writes_allowed(dir.path(), &keys.verifier, || {
        delete_document(conn, existing.id)
    });
    assert_eq!(
        delete_err,
        Err(Error::LicenseExpired),
        "expired delete must return license_expired"
    );

    let docs = list_documents(conn, entity_id).expect("list docs");
    assert_eq!(
        docs.len(),
        1,
        "blocked attach/delete must not mutate: {docs:?}"
    );
    assert_eq!(docs[0].id, existing.id);

    let (meta, data) = get_document(conn, existing.id).expect("export read");
    let export_path = dir.path().join(meta.filename);
    std::fs::write(&export_path, &data).expect("document export write");
    assert_eq!(
        std::fs::read(&export_path).expect("exported bytes"),
        b"keep-me",
        "document export stays allowed while expired"
    );
}

#[test]
fn expired_license_does_not_mutate_lock_timeout() {
    let (dir, vault, keys) = setup();
    let conn = vault.connection().expect("conn");
    let before = get_lock_timeout_secs(conn).expect("default timeout");

    let body = sign_lic(&keys.signing, "2020-01-01");
    let src = dir.path().join("incoming.lic");
    std::fs::write(&src, &body).expect("write lic");
    assert_eq!(
        install_license(dir.path(), &src, &keys.verifier)
            .expect("install")
            .state,
        LicenseState::Expired
    );

    let err = when_writes_allowed(dir.path(), &keys.verifier, || {
        set_lock_timeout_secs(conn, 120)
    });
    assert_eq!(err, Err(Error::LicenseExpired));
    assert_eq!(
        get_lock_timeout_secs(conn).expect("unchanged"),
        before,
        "expired must not persist a new lock timeout"
    );
}

#[test]
fn document_export_still_works_when_expired() {
    let (dir, vault, keys) = setup();
    let entity_id = seed_books(&vault);
    let conn = vault.connection().expect("conn");
    let entry_id = list_entries(conn, entity_id, &EntryFilter::default())
        .expect("list")
        .into_iter()
        .find(|v| v.entry.description == "Groceries")
        .expect("seed entry")
        .entry
        .id;
    let meta = attach_document(
        conn,
        entity_id,
        entry_id,
        "receipt.txt",
        "text/plain",
        b"export-me",
    )
    .expect("attach while writable");

    let body = sign_lic(&keys.signing, "2020-01-01");
    let src = dir.path().join("incoming.lic");
    std::fs::write(&src, &body).expect("write lic");
    assert_eq!(
        install_license(dir.path(), &src, &keys.verifier)
            .expect("install")
            .state,
        LicenseState::Expired
    );

    // Same read+write path as the `document_export` command (dialog is UI-only).
    let (exported_meta, data) = get_document(conn, meta.id).expect("document_export read");
    let dest = dir.path().join("exported-receipt.txt");
    std::fs::write(&dest, &data).expect("document_export write");
    assert_eq!(exported_meta.filename, "receipt.txt");
    assert_eq!(std::fs::read(&dest).expect("bytes"), b"export-me");
    assert_eq!(
        require_writes_allowed(dir.path(), &keys.verifier),
        Err(Error::LicenseExpired),
        "export must not require a writable license"
    );
}

fn book(name: &str) -> CreateEntity {
    CreateEntity {
        name: name.into(),
        base_currency: "EUR".into(),
        chart_template: ChartTemplate::Personal,
        fiscal_year_start_month: Some(1),
    }
}

#[test]
fn vault_init_first_entity_works_unlicensed() {
    let (dir, vault, keys) = setup();
    let conn = vault.connection().expect("conn");
    assert_eq!(
        license_status(dir.path(), &keys.verifier)
            .expect("status")
            .state,
        LicenseState::None
    );
    create_entity_allowed(dir.path(), &keys.verifier, conn, &book("First"))
        .expect("first entity while unlicensed");
    assert_eq!(list_entities(conn).expect("list").len(), 1);
}

#[test]
fn trial_second_entity_is_license_entity_limit() {
    let (dir, vault, keys) = setup();
    record_trial_start(dir.path()).expect("trial");
    let conn = vault.connection().expect("conn");
    create_entity_allowed(dir.path(), &keys.verifier, conn, &book("One")).expect("first");
    let err =
        create_entity_allowed(dir.path(), &keys.verifier, conn, &book("Two")).expect_err("second");
    assert!(
        matches!(err, Error::LicenseEntityLimit),
        "trial + existing entity: {err:?}"
    );
    assert_eq!(list_entities(conn).expect("list").len(), 1);
}

#[test]
fn licensed_second_entity_is_ok() {
    let (dir, vault, keys) = setup();
    let conn = vault.connection().expect("conn");
    create_entity(conn, &book("One")).expect("seed");
    let body = sign_lic(&keys.signing, "2099-12-31");
    let src = dir.path().join("incoming.lic");
    std::fs::write(&src, &body).expect("write lic");
    assert_eq!(
        install_license(dir.path(), &src, &keys.verifier)
            .expect("install")
            .state,
        LicenseState::Licensed
    );
    create_entity_allowed(dir.path(), &keys.verifier, conn, &book("Two"))
        .expect("licensed may create more");
    assert_eq!(list_entities(conn).expect("list").len(), 2);
}

#[test]
fn expired_entity_create_is_license_expired() {
    let (dir, vault, keys) = setup();
    let conn = vault.connection().expect("conn");
    create_entity(conn, &book("One")).expect("seed");
    let body = sign_lic(&keys.signing, "2020-01-01");
    let src = dir.path().join("incoming.lic");
    std::fs::write(&src, &body).expect("write lic");
    assert_eq!(
        install_license(dir.path(), &src, &keys.verifier)
            .expect("install")
            .state,
        LicenseState::Expired
    );
    let err =
        create_entity_allowed(dir.path(), &keys.verifier, conn, &book("Two")).expect_err("expired");
    assert!(
        matches!(err, Error::LicenseExpired),
        "expired create must not be license_entity_limit: {err:?}"
    );
    assert_eq!(list_entities(conn).expect("list").len(), 1);
}
