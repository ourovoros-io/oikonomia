//! The four commands that take a path from the webview, invoked through the
//! mock IPC with paths granted for each purpose.
//!
//! Each command accepts a path granted for its own purpose and refuses one
//! granted for any other with `path_not_granted`. The tests are together
//! here, not with each command, because the property is about all four at
//! once: a file handed over for one of them reaches none of the others.
//!
//! A path that passes the check goes on to the command's own work. The files
//! here are chosen so that this work then fails with a code of its own,
//! which shows that the grant was accepted without restoring a vault or
//! running the analyzer.
//!
//! Not built on Windows, where the mock runtime keeps a test executable from
//! starting; `commands::support::ipc_test_support` says why.

use crate::commands::support::ipc_test_support::MockApp;
use crate::state::GrantPurpose;
use oikonomia_core::domain::ChartTemplate;
use oikonomia_core::ledger::{CreateEntity, create_entity, list_accounts};
use oikonomia_core::prefs::Locale;
use oikonomia_core::vault::Connection;

/// The ids of the seeded book and of two of its accounts, as the frontend
/// holds them.
struct BookIds {
    /// The book's id.
    entity: String,
    /// The id of the book's food account.
    food_account: String,
    /// The id of the book's checking account.
    checking_account: String,
}

/// One command that takes a path: its purpose, and how the frontend calls it.
struct PathCommand {
    /// The command's name.
    name: &'static str,
    /// The purpose the command asks for.
    purpose: GrantPurpose,
    /// The name of a file whose content the command refuses once the path
    /// has passed the grant check.
    file_name: &'static str,
    /// The code of that refusal.
    code_once_granted: &'static str,
    /// Builds the payload the frontend sends for a path.
    payload: fn(&BookIds, &str) -> serde_json::Value,
}

/// The four commands that take a path. A fifth one has to be added here to
/// be covered.
const PATH_COMMANDS: [PathCommand; 4] = [
    PathCommand {
        name: "vault_restore",
        purpose: GrantPurpose::Backup,
        file_name: "not-an-archive.oikonomia-backup",
        code_once_granted: "backup_invalid",
        payload: |_book, path| serde_json::json!({ "path": path, "replace": true }),
    },
    PathCommand {
        name: "csv_import_preview",
        purpose: GrantPurpose::Csv,
        file_name: "empty.csv",
        code_once_granted: "csv_empty",
        payload: |book, path| {
            serde_json::json!({
                "input": {
                    "entity_id": book.entity,
                    "path": path,
                    "wallet_account_id": null,
                    "expense_account_id": null,
                    "income_account_id": null,
                },
            })
        },
    },
    PathCommand {
        name: "document_analyze_path",
        purpose: GrantPurpose::Document,
        file_name: "empty.pdf",
        code_once_granted: "file_empty",
        payload: |book, path| serde_json::json!({ "entityId": book.entity, "path": path }),
    },
    PathCommand {
        name: "entry_post_simple_with_document_path",
        purpose: GrantPurpose::Document,
        file_name: "empty.pdf",
        code_once_granted: "file_empty",
        payload: |book, path| {
            serde_json::json!({
                "input": {
                    "entity_id": book.entity,
                    "kind": "expense",
                    "bill_status": null,
                    "entry_date": "2026-08-05",
                    "description": "Groceries",
                    "reference": null,
                    "amount_minor": 2_500,
                    "category_account_id": book.food_account,
                    "wallet_account_id": book.checking_account,
                    "payable_account_id": null,
                    "from_account_id": null,
                    "to_account_id": null,
                },
                "path": path,
                "analysisJson": null,
            })
        },
    },
];

/// Every purpose. The `match` stops compiling when a purpose is added, so
/// the new one cannot be left out of these tests.
fn every_purpose() -> [GrantPurpose; 3] {
    let all = [
        GrantPurpose::Backup,
        GrantPurpose::Csv,
        GrantPurpose::Document,
    ];
    for purpose in all {
        match purpose {
            GrantPurpose::Backup | GrantPurpose::Csv | GrantPurpose::Document => {}
        }
    }
    all
}

/// Starts the mock app with the path-taking commands registered, over a
/// vault that holds one book.
fn mock_book(label: &str) -> (MockApp, BookIds) {
    MockApp::start(
        label,
        // Named by path: a command is a function plus a hidden macro beside
        // it, and a `use` of the function alone does not bring the macro.
        tauri::generate_handler![
            crate::commands::vault::vault_restore,
            crate::commands::vault::vault_status,
            crate::commands::csv::csv_import_preview,
            crate::commands::documents::document_analyze_path,
            crate::commands::journal::entry_post_simple_with_document_path
        ],
        seed_book,
    )
}

/// Creates the book.
fn seed_book(conn: &Connection) -> BookIds {
    let book = CreateEntity {
        name: "Home".into(),
        base_currency: "EUR".into(),
        chart_template: ChartTemplate::Personal,
        fiscal_year_start_month: None,
    };
    let entity = create_entity(conn, &book, Locale::En).unwrap();
    let accounts = list_accounts(conn, entity.id).unwrap();
    let account = |code: &str| {
        accounts
            .iter()
            .find(|account| account.code == code)
            .map(|account| account.id.to_string())
            .unwrap()
    };

    BookIds {
        entity: entity.id.to_string(),
        food_account: account("5100"),
        checking_account: account("1010"),
    }
}

/// The code a command is refused with; `null` for a refusal without one,
/// which is how Tauri's own refusal of a payload would show.
fn refusal_code(
    app: &MockApp,
    command: &PathCommand,
    book: &BookIds,
    path: &str,
) -> serde_json::Value {
    let refused = app
        .invoke(command.name, (command.payload)(book, path))
        .unwrap_err();

    refused["code"].clone()
}

#[test]
fn a_path_that_was_never_granted_is_refused_by_every_command() {
    let (app, book) = mock_book("grant-none");

    for command in &PATH_COMMANDS {
        let path = app.write_file(command.file_name, b"");

        assert_eq!(
            refusal_code(&app, command, &book, &path),
            "path_not_granted",
            "{}",
            command.name
        );
    }
}

#[test]
fn a_path_granted_for_another_purpose_is_refused_by_every_command() {
    for command in &PATH_COMMANDS {
        let others = every_purpose()
            .into_iter()
            .filter(|purpose| *purpose != command.purpose);

        for other in others {
            let (app, book) = mock_book("grant-other");
            let path = app.write_file(command.file_name, b"");
            app.grant(other, &path);

            assert_eq!(
                refusal_code(&app, command, &book, &path),
                "path_not_granted",
                "{} took a path granted for {other:?}",
                command.name
            );
        }
    }
}

#[test]
fn a_path_granted_for_its_own_purpose_passes_the_check_of_every_command() {
    for command in &PATH_COMMANDS {
        let (app, book) = mock_book("grant-own");
        let path = app.write_file(command.file_name, b"");
        app.grant(command.purpose, &path);

        assert_eq!(
            refusal_code(&app, command, &book, &path),
            command.code_once_granted,
            "{}",
            command.name
        );
    }
}

/// The two grants the original report was about: a statement picked for a
/// CSV import and a file dropped on a window, each named to the restore.
#[test]
fn a_restore_refuses_a_csv_pick_and_a_dropped_file_and_leaves_the_session_open() {
    let (app, _book) = mock_book("grant-restore");
    let statement = app.write_file("statement.csv", b"date,amount\n2026-08-05,-25.00\n");
    let dropped = app.write_file("dropped.oikonomia-backup", b"OIKOBACK");
    app.grant(GrantPurpose::Csv, &statement);
    app.grant(GrantPurpose::Document, &dropped);

    for path in [&statement, &dropped] {
        let refused = app
            .invoke(
                "vault_restore",
                serde_json::json!({ "path": path, "replace": true }),
            )
            .unwrap_err();

        assert_eq!(refused["code"], "path_not_granted", "{path}");
    }

    // The refusal comes before the restore locks the session.
    let status = app.invoke("vault_status", serde_json::json!({})).unwrap();
    assert_eq!(status, "unlocked");
}
