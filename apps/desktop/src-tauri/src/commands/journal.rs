//! Journal entry commands: list, post, correct, hide and void.
//!
//! Every command here requires the unlocked vault. The entry forms post
//! through the simple-entry commands, which send a kind and the accounts for
//! its roles; core turns that into balanced lines. [`entry_post`] takes the
//! lines themselves.
//!
//! # Simple-entry errors
//!
//! Every command that posts a simple entry can return these, from core's
//! validation of the entry:
//!
//! - `amount_not_positive` for an amount of zero or less;
//! - `account_required` (with the role as `role`) when a role the kind needs
//!   has no account;
//! - `account_wrong_type` (with `role` and the account's `code`) when a role's account has
//!   a type the role does not accept;
//! - `bill_status_required` when a bill does not say whether it is paid;
//! - `same_account` when both sides name one account;
//! - `account_wrong_entity` when an account belongs to another entity;
//! - `account_inactive` when an account is inactive;
//! - `invalid_date` when the entry date is not a date;
//! - `not_found` when an account does not exist.

use crate::commands::documents::{PickedDocument, read_dropped_document};
use crate::commands::support::{
    Arguments, dropped_file_name, require_granted_path, run_blocking, with_connection,
    with_localized_connection,
};
use crate::error::CommandResult;
use crate::state::{AppState, GrantPurpose};
use oikonomia_core::documents::post_simple_entry_with_document;
use oikonomia_core::domain::{AccountId, EntityId, JournalEntryId};
use oikonomia_core::ledger::{
    EntryFilter, PostJournal, PostJournalRequest, PostSimpleEntry, PostSimpleEntryRequest,
    PostedEntryView, VoidResult, get_entry, list_entries, post_entry, post_simple_entry,
    replace_simple_entry, set_entry_hidden, void_entry,
};
use oikonomia_core::util::DateText;
use serde::Deserialize;
use tauri::State;

/// Lists an entity's posted entries, voided ones included, optionally
/// narrowed by text, by a date range and by account.
///
/// Requires the unlocked vault. An unknown entity has no entries, so it
/// yields an empty list, not an error.
///
/// # Errors
///
/// Returns `invalid_date` when `from` or `to` is not a date, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entry_list(
    state: State<'_, AppState>,
    arguments: Arguments<EntryListArguments>,
) -> CommandResult<Vec<PostedEntryView>> {
    let Arguments(arguments) = arguments;

    with_connection(&state, move |conn| {
        let filter = EntryFilter {
            text: arguments.search,
            date_from: DateText::parse_optional(arguments.from.as_ref())?,
            date_to: DateText::parse_optional(arguments.to.as_ref())?,
            account_id: arguments.account_id,
        };
        list_entries(conn, arguments.entity_id, &filter)
    })
    .await
}

/// The arguments of [`entry_list`], as the webview names them: `entityId`,
/// `from`, `to`, `search` and `accountId`.
///
/// Every argument but the entity may be left out or sent as `null`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EntryListArguments {
    /// Entity whose entries are listed.
    entity_id: EntityId,
    /// Inclusive lower bound on the entry date.
    from: Option<DateText>,
    /// Inclusive upper bound on the entry date.
    to: Option<DateText>,
    /// Text to look for in descriptions, references and memos.
    search: Option<String>,
    /// Only entries with a line on this account.
    account_id: Option<AccountId>,
}

/// Returns one posted entry with its lines.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `not_found` when the entry does not exist, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entry_get(
    state: State<'_, AppState>,
    id: JournalEntryId,
) -> CommandResult<PostedEntryView> {
    with_connection(&state, move |conn| get_entry(conn, id)).await
}

/// Posts a journal entry given as explicit debit and credit lines.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `too_few_lines` for fewer than two lines, `invalid_line_amounts`
/// for a line that is not a debit or a credit but both or neither,
/// `negative_money` for a negative amount, `unbalanced_entry` when debits
/// and credits differ, `money_overflow` when a total does not fit the money
/// type, `invalid_date` when the entry date is not a date, `not_found` when
/// a line's account does not exist, `account_wrong_entity` when it belongs to
/// another entity, `account_inactive` when it is inactive, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entry_post(
    state: State<'_, AppState>,
    input: PostJournalRequest,
) -> CommandResult<PostedEntryView> {
    with_connection(&state, move |conn| {
        let input = PostJournal::try_from(input)?;
        post_entry(conn, &input)
    })
    .await
}

/// Posts an entry from the simple form: a kind, an amount and the accounts
/// for the kind's roles. Core builds the lines.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns the [simple-entry errors](self#simple-entry-errors) and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entry_post_simple(
    state: State<'_, AppState>,
    input: PostSimpleEntryRequest,
) -> CommandResult<PostedEntryView> {
    with_connection(&state, move |conn| {
        let input = PostSimpleEntry::try_from(input)?;
        post_simple_entry(conn, &input)
    })
    .await
}

/// Posts a simple entry and stores the document it was drafted from, in one
/// transaction.
///
/// Requires the unlocked vault. The document is the one the webview picked;
/// if either the entry or the document is refused, neither is written.
///
/// # Errors
///
/// Returns `file_data_invalid` when `dataBase64` is not base64;
/// `file_too_large` (with the cap as `max_mb`), `file_empty`,
/// `file_type_unsupported` and `name_required` when the document is refused;
/// `name_taken` when the entity already stores a document under the file
/// name; the [simple-entry errors](self#simple-entry-errors); and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entry_post_simple_with_document(
    state: State<'_, AppState>,
    arguments: Arguments<PostWithDocumentArguments>,
) -> CommandResult<PostedEntryView> {
    let Arguments(PostWithDocumentArguments {
        input,
        document,
        analysis_json,
    }) = arguments;
    let document = document.decode()?;

    with_connection(&state, move |conn| {
        let input = PostSimpleEntry::try_from(input)?;
        let (view, _document) = post_simple_entry_with_document(
            conn,
            &input,
            &document.as_new(),
            analysis_json.as_deref(),
        )?;
        Ok(view)
    })
    .await
}

/// The arguments of [`entry_post_simple_with_document`], as the webview
/// names them: `input`, `filename`, `mimeType`, `dataBase64` and
/// `analysisJson`.
///
/// `analysisJson` may be left out or sent as `null`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostWithDocumentArguments {
    /// The entry to post.
    input: PostSimpleEntryRequest,
    /// The document the entry was drafted from.
    #[serde(flatten)]
    document: PickedDocument,
    /// The analysis the draft came from, stored with the document as given.
    analysis_json: Option<String>,
}

/// Posts a simple entry and stores the document at `path`, in one
/// transaction.
///
/// Requires the unlocked vault and a granted path: the file the user dropped
/// on a window. The file is read and validated again at post time, so one
/// that moved since the drop yields an error and nothing is written. The
/// document is stored under the name it was dropped as.
///
/// # Errors
///
/// Returns `path_not_granted` for a path the user did not drop on a window;
/// `file_unreadable` when the file cannot be read; `file_too_large` (with the
/// cap as `max_mb`), `file_empty`, `file_type_unsupported` and
/// `name_required` when the document is refused; `name_taken` when the entity
/// already stores a document under the file name; the
/// [simple-entry errors](self#simple-entry-errors); and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entry_post_simple_with_document_path(
    state: State<'_, AppState>,
    input: PostSimpleEntryRequest,
    path: String,
    analysis_json: Option<String>,
) -> CommandResult<PostedEntryView> {
    let filename = dropped_file_name(&path);
    let grants = state.path_grants();
    let path =
        run_blocking(move || require_granted_path(&grants, GrantPurpose::Document, &path)).await?;

    let vault = state.vault();
    state.touch();

    run_blocking(move || {
        let document = read_dropped_document(&path, &filename)?;

        let guard = vault.acquire();
        let conn = guard.connection()?;
        let input = PostSimpleEntry::try_from(input)?;
        let (view, _document) = post_simple_entry_with_document(
            conn,
            &input,
            &document.as_new(),
            analysis_json.as_deref(),
        )?;
        Ok(view)
    })
    .await
}

/// Corrects a posted entry: voids the original and posts the replacement in
/// one transaction.
///
/// Requires the unlocked vault. The replacement keeps the original's hidden
/// flag and takes over its documents. The reversal's description is written
/// in the app's stored language.
///
/// # Errors
///
/// Returns `not_found` when the original does not exist, `wrong_book` when
/// the replacement names another entity, `entry_already_voided` when the
/// original has been voided or is itself a reversal, the
/// [simple-entry errors](self#simple-entry-errors), and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entry_replace_simple(
    state: State<'_, AppState>,
    original_id: JournalEntryId,
    input: PostSimpleEntryRequest,
) -> CommandResult<PostedEntryView> {
    with_localized_connection(&state, move |conn, locale| {
        let input = PostSimpleEntry::try_from(input)?;
        replace_simple_entry(conn, original_id, &input, locale)
    })
    .await
}

/// Sets or clears the hidden flag on a posted entry and returns the entry.
///
/// Requires the unlocked vault. A hidden entry stays in the books and is left
/// out of the exports made for other people.
///
/// # Errors
///
/// Returns `not_found` when the entry does not exist, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entry_set_hidden(
    state: State<'_, AppState>,
    id: JournalEntryId,
    hidden: bool,
) -> CommandResult<PostedEntryView> {
    with_connection(&state, move |conn| set_entry_hidden(conn, id, hidden)).await
}

/// Voids a posted entry by posting its reversal on the same date.
///
/// Requires the unlocked vault. The reversal's description is written in the
/// app's stored language.
///
/// # Errors
///
/// Returns `not_found` when the entry does not exist, `entry_already_voided`
/// when it has been voided or is itself a reversal, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entry_void(
    state: State<'_, AppState>,
    id: JournalEntryId,
) -> CommandResult<VoidResult> {
    with_localized_connection(&state, move |conn, locale| void_entry(conn, id, locale)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An entity id as the webview sends it.
    const ENTITY: &str = "11111111-1111-4111-8111-111111111111";
    /// An account id as the webview sends it.
    const ACCOUNT: &str = "22222222-2222-4222-8222-222222222222";

    #[test]
    fn the_entry_list_arguments_are_read_from_the_camel_case_payload() {
        let payload = serde_json::json!({
            "entityId": ENTITY,
            "from": "2026-08-01",
            "to": "2026-08-31",
            "search": "rent",
            "accountId": ACCOUNT,
        });
        let arguments: EntryListArguments = serde_json::from_value(payload).unwrap();

        assert_eq!(arguments.entity_id.to_string(), ENTITY);
        assert_eq!(
            arguments.account_id.map(|id| id.to_string()).as_deref(),
            Some(ACCOUNT)
        );
        assert_eq!(arguments.search.as_deref(), Some("rent"));
        assert!(arguments.from.is_some_and(|from| from.parse().is_ok()));
        assert!(arguments.to.is_some_and(|to| to.parse().is_ok()));
    }

    #[test]
    fn an_optional_entry_list_argument_may_be_left_out_or_null() {
        let left_out = serde_json::json!({ "entityId": ENTITY });
        let null = serde_json::json!({
            "entityId": ENTITY,
            "from": null,
            "to": null,
            "search": null,
            "accountId": null,
        });

        for payload in [left_out, null] {
            let arguments: EntryListArguments = serde_json::from_value(payload).unwrap();

            assert!(arguments.from.is_none() && arguments.to.is_none());
            assert!(arguments.search.is_none() && arguments.account_id.is_none());
        }
    }

    #[test]
    fn a_malformed_date_is_read_and_left_for_core_to_refuse_with_its_code() {
        let payload = serde_json::json!({ "entityId": ENTITY, "from": "1/8/2026" });
        let arguments: EntryListArguments = serde_json::from_value(payload).unwrap();

        assert_eq!(
            arguments
                .from
                .map(|from| from.parse().map_err(|error| error.code())),
            Some(Err("invalid_date"))
        );
    }

    #[test]
    fn the_entry_list_arguments_need_an_entity() {
        let payload = serde_json::json!({ "search": "rent" });

        assert!(serde_json::from_value::<EntryListArguments>(payload).is_err());
    }
}

/// `entry_list` and `entry_post_simple_with_document` invoked through the
/// mock IPC, the way the webview invokes them.
///
/// Not built on Windows, where the mock runtime keeps a test executable from
/// starting; `commands::support::ipc_test_support` says why.
#[cfg(test)]
#[cfg(not(windows))]
mod ipc_tests {
    use crate::commands::journal::{entry_list, entry_post_simple_with_document};
    use crate::commands::support::ipc_test_support::MockApp;
    use base64::Engine;
    use oikonomia_core::domain::ChartTemplate;
    use oikonomia_core::ledger::{
        CreateEntity, PostSimpleEntry, PostSimpleEntryRequest, SimpleEntryKind, create_entity,
        list_accounts, post_simple_entry,
    };
    use oikonomia_core::prefs::Locale;
    use oikonomia_core::vault::Connection;

    /// The ids of the seeded book and of two of its accounts, as the
    /// frontend holds them.
    struct BookIds {
        /// The book's id.
        entity: String,
        /// The id of the book's food account.
        food_account: String,
        /// The id of the book's checking account.
        checking_account: String,
    }

    /// Starts the mock app with the commands under test registered, over a
    /// vault that holds groceries on 5 August, rent on 20 August and a
    /// salary on 1 September 2026.
    fn mock_book(label: &str) -> (MockApp, BookIds) {
        MockApp::start(
            label,
            tauri::generate_handler![
                entry_list,
                entry_post_simple_with_document,
                crate::commands::documents::document_list
            ],
            seed_book,
        )
    }

    /// Creates the book and its three entries.
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
                .map(|account| account.id)
                .unwrap()
        };
        let (food, checking, salary) = (account("5100"), account("1010"), account("4000"));

        for (kind, category, entry_date, description) in [
            (SimpleEntryKind::Expense, food, "2026-08-05", "Groceries"),
            (SimpleEntryKind::Expense, food, "2026-08-20", "Rent"),
            (SimpleEntryKind::Income, salary, "2026-09-01", "Salary"),
        ] {
            let request = PostSimpleEntryRequest {
                entity_id: entity.id,
                kind,
                bill_status: None,
                entry_date: entry_date.into(),
                description: description.into(),
                reference: None,
                amount_minor: 2_500,
                category_account_id: Some(category),
                wallet_account_id: Some(checking),
                payable_account_id: None,
                from_account_id: None,
                to_account_id: None,
            };
            post_simple_entry(conn, &PostSimpleEntry::try_from(request).unwrap()).unwrap();
        }

        BookIds {
            entity: entity.id.to_string(),
            food_account: food.to_string(),
            checking_account: checking.to_string(),
        }
    }

    /// The descriptions of the entries in a response, in its order.
    fn descriptions(entries: &serde_json::Value) -> Vec<&str> {
        entries
            .as_array()
            .unwrap()
            .iter()
            .map(|view| view["entry"]["description"].as_str().unwrap())
            .collect()
    }

    // The payloads below are the object `entryList` in `web/src/lib/api.ts`
    // passes to `invoke`: camelCase keys, and `null` for a filter not set.

    #[test]
    fn the_ipc_call_the_frontend_makes_without_filters_lists_every_entry() {
        let (app, book) = mock_book("no-filters");

        let listed = app
            .invoke(
                "entry_list",
                serde_json::json!({
                    "entityId": book.entity,
                    "from": null,
                    "to": null,
                    "search": null,
                    "accountId": null,
                }),
            )
            .unwrap();

        assert_eq!(descriptions(&listed), ["Salary", "Rent", "Groceries"]);
    }

    #[test]
    fn the_ipc_call_the_frontend_makes_with_every_filter_applies_each_of_them() {
        let (app, book) = mock_book("all-filters");
        let august = |search: &str, account_id: &str| {
            serde_json::json!({
                "entityId": book.entity,
                "from": "2026-08-01",
                "to": "2026-08-31",
                "search": search,
                "accountId": account_id,
            })
        };

        let rent = app
            .invoke("entry_list", august("rent", &book.food_account))
            .unwrap();
        assert_eq!(descriptions(&rent), ["Rent"]);

        // The date range alone keeps both August entries, so the single
        // result above is the search at work.
        let both = app
            .invoke("entry_list", august("", &book.food_account))
            .unwrap();
        assert_eq!(descriptions(&both), ["Rent", "Groceries"]);

        // The entity id stands in for an account no entry has a line on.
        let none = app
            .invoke("entry_list", august("rent", &book.entity))
            .unwrap();
        assert_eq!(descriptions(&none), [] as [&str; 0]);
    }

    #[test]
    fn a_malformed_date_sent_over_ipc_comes_back_with_the_invalid_date_code() {
        let (app, book) = mock_book("bad-date");

        let refused = app
            .invoke(
                "entry_list",
                serde_json::json!({ "entityId": book.entity, "from": "1/8/2026" }),
            )
            .unwrap_err();

        assert_eq!(refused["code"], "invalid_date");
    }

    #[test]
    fn an_ipc_call_without_the_entity_is_refused_by_the_argument_layer() {
        let (app, _book) = mock_book("no-entity");

        let refused = app
            .invoke("entry_list", serde_json::json!({ "search": "rent" }))
            .unwrap_err();

        // Tauri's own refusal is text, not a coded error.
        assert!(refused.is_string(), "{refused}");
    }

    /// `bytes` as the base64 the webview sends.
    fn base64_of(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    /// The `input` object the simple entry form sends for an expense of
    /// 12,50 on 25 August 2026: every key of `SimpleEntryInput` in
    /// `web/src/lib/api.ts`, in snake case, with `null` for a role not used.
    fn expense_input(book: &BookIds, description: &str) -> serde_json::Value {
        serde_json::json!({
            "entity_id": book.entity,
            "kind": "expense",
            "bill_status": null,
            "entry_date": "2026-08-25",
            "description": description,
            "reference": null,
            "amount_minor": 1250,
            "category_account_id": book.food_account,
            "wallet_account_id": book.checking_account,
            "payable_account_id": null,
            "from_account_id": null,
            "to_account_id": null,
        })
    }

    /// The documents of the book, as `document_list` returns them.
    fn stored_documents(app: &MockApp, book: &BookIds) -> Vec<serde_json::Value> {
        app.invoke(
            "document_list",
            serde_json::json!({ "entityId": book.entity }),
        )
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
    }

    /// The descriptions of every entry of the book, newest first.
    fn all_descriptions(app: &MockApp, book: &BookIds) -> Vec<String> {
        let listed = app
            .invoke("entry_list", serde_json::json!({ "entityId": book.entity }))
            .unwrap();

        descriptions(&listed)
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    // The payloads below are the object `entryPostSimpleWithDocument` in
    // `web/src/lib/api.ts` passes to `invoke`: `input` beside the camelCase
    // keys of the document, and `analysisJson` as a string or `null`.

    #[test]
    fn the_ipc_call_the_frontend_makes_posts_the_entry_and_stores_its_document() {
        let (app, book) = mock_book("post-with-document");

        let posted = app
            .invoke(
                "entry_post_simple_with_document",
                serde_json::json!({
                    "input": expense_input(&book, "Paper"),
                    "filename": "receipt.txt",
                    "mimeType": "text/plain",
                    "dataBase64": base64_of(b"TOTAL 12,50"),
                    "analysisJson": "{\"notes\":[]}",
                }),
            )
            .unwrap();

        assert_eq!(posted["entry"]["description"], "Paper");

        let documents = stored_documents(&app, &book);
        assert_eq!(documents.len(), 1, "{documents:?}");
        assert_eq!(documents[0]["filename"], "receipt.txt");
        assert_eq!(documents[0]["mime_type"], "text/plain");
        assert_eq!(documents[0]["entry_id"], posted["entry"]["id"]);
        assert_eq!(documents[0]["size_bytes"], 11);
    }

    #[test]
    fn the_analysis_may_be_null_or_left_out_and_an_empty_type_is_resolved_by_the_name() {
        let (app, book) = mock_book("post-without-analysis");

        let with_null = serde_json::json!({
            "input": expense_input(&book, "Null analysis"),
            "filename": "a.txt",
            "mimeType": "",
            "dataBase64": base64_of(b"a"),
            "analysisJson": null,
        });
        let left_out = serde_json::json!({
            "input": expense_input(&book, "No analysis"),
            "filename": "b.txt",
            "mimeType": "",
            "dataBase64": base64_of(b"b"),
        });

        for payload in [with_null, left_out] {
            app.invoke("entry_post_simple_with_document", payload)
                .unwrap();
        }

        let documents = stored_documents(&app, &book);
        assert_eq!(documents.len(), 2, "{documents:?}");
        for document in &documents {
            assert_eq!(document["mime_type"], "text/plain");
        }
    }

    #[test]
    fn a_refused_document_comes_back_with_its_code_and_no_entry_is_posted() {
        let (app, book) = mock_book("post-refused-document");
        let post = |filename: &str, data_base64: &str| {
            app.invoke(
                "entry_post_simple_with_document",
                serde_json::json!({
                    "input": expense_input(&book, "Never posted"),
                    "filename": filename,
                    "mimeType": "",
                    "dataBase64": data_base64,
                    "analysisJson": null,
                }),
            )
            .unwrap_err()
        };

        assert_eq!(
            post("receipt.txt", "not base64!")["code"],
            "file_data_invalid"
        );
        assert_eq!(
            post("tool.exe", &base64_of(b"MZ"))["code"],
            "file_type_unsupported"
        );
        assert_eq!(post("receipt.txt", "")["code"], "file_empty");

        assert_eq!(
            all_descriptions(&app, &book),
            ["Salary", "Rent", "Groceries"],
            "the entry must roll back with its document"
        );
        assert_eq!(stored_documents(&app, &book).len(), 0);
    }

    #[test]
    fn a_post_without_the_document_keys_is_refused_by_the_argument_layer() {
        let (app, book) = mock_book("post-no-document");

        let refused = app
            .invoke(
                "entry_post_simple_with_document",
                serde_json::json!({ "input": expense_input(&book, "No document") }),
            )
            .unwrap_err();

        // Tauri's own refusal is text, not a coded error.
        assert!(refused.is_string(), "{refused}");
        assert_eq!(all_descriptions(&app, &book).len(), 3);
    }
}
