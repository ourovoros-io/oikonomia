//! Document commands: analyze a bill or receipt into a draft entry, and
//! store, list, show, export and delete the documents attached to entries.
//!
//! Analysis is offline: the text is read by the OCR models shipped with the
//! app and nothing is sent anywhere. Analysis also stores nothing. A document
//! reaches the vault only together with the entry it belongs to
//! ([`entry_post_simple_with_document`](crate::commands::entry_post_simple_with_document)),
//! so a document without an entry cannot exist.
//!
//! A document arrives in one of two ways. Picked in the webview, it comes as
//! base64 with a name and a type. Dropped on a window, it comes as a path,
//! which must be a granted one and is read here.

use crate::commands::support::{
    Arguments, SaveTarget, decode_document_base64, dropped_file_name, require_granted_path,
    run_blocking, save_with_dialog, stored_text_locale, with_connection,
};
use crate::error::{CommandError, CommandResult, DesktopError};
use crate::state::{AppState, GatedVault, GrantPurpose};
use base64::Engine;
use oikonomia_core::documents::{
    AnalyzeContext, AnalyzerStatus, DocumentId, DocumentMeta, DocumentSuggestion, NewDocument,
    ReadDocument, analyze_document_bytes, analyzer_status, attach_document, delete_document,
    get_document, list_documents, read_validated_file, suggest_accounts_for_entity,
};
use oikonomia_core::domain::{EntityId, JournalEntryId};
use oikonomia_core::error::Error as CoreError;
use oikonomia_core::ledger::get_entity;
use oikonomia_core::prefs::Locale;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::State;

/// A stored document's metadata and bytes, for the in-app viewer.
#[derive(Debug, Serialize)]
pub(crate) struct DocumentContent {
    /// The document's metadata.
    pub meta: DocumentMeta,
    /// The document's bytes as base64, the form IPC carries. Bounded by core's
    /// size cap on a stored document.
    pub data_base64: String,
}

/// A document the webview picked, as it crosses IPC: `filename`, `mimeType`
/// and `dataBase64`.
///
/// The three keys sit beside a command's other arguments in the payload, so
/// an arguments struct holds this one flattened.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PickedDocument {
    /// The file's name.
    filename: String,
    /// The MIME type the webview reports for the file; often empty.
    mime_type: String,
    /// The file's bytes as base64.
    data_base64: String,
}

impl std::fmt::Debug for PickedDocument {
    /// Shows the name and type; the payload can be megabytes of base64.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PickedDocument")
            .field("filename", &self.filename)
            .field("mime_type", &self.mime_type)
            .finish_non_exhaustive()
    }
}

impl PickedDocument {
    /// Decodes the bytes, up to the size core stores.
    ///
    /// # Errors
    ///
    /// Returns `file_too_large` (with the cap as `max_mb`) for a payload over
    /// the cap and `file_data_invalid` for one that is not base64.
    pub(super) fn decode(self) -> CommandResult<DecodedDocument> {
        let data = decode_document_base64(&self.data_base64)?;

        Ok(DecodedDocument {
            filename: self.filename,
            mime_type: self.mime_type,
            data,
        })
    }
}

/// A [`PickedDocument`] with its bytes decoded.
pub(super) struct DecodedDocument {
    /// The file's name.
    filename: String,
    /// The MIME type the webview reported.
    mime_type: String,
    /// The file's bytes.
    data: Vec<u8>,
}

impl std::fmt::Debug for DecodedDocument {
    /// Shows the name, the type and the size, not the bytes.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DecodedDocument")
            .field("filename", &self.filename)
            .field("mime_type", &self.mime_type)
            .field("len", &self.data.len())
            .finish_non_exhaustive()
    }
}

impl DecodedDocument {
    /// The document in the form core stores and analyzes.
    pub(super) fn as_new(&self) -> NewDocument<'_> {
        NewDocument {
            filename: &self.filename,
            mime_type: &self.mime_type,
            data: &self.data,
        }
    }
}

/// Returns whether the OCR models shipped with the app were found, or the
/// engine is already loaded.
///
/// Needs no vault. The answer looks at the model files, which is file I/O,
/// so it is taken on the blocking pool and not on the main thread. It does
/// not wait for an analysis in progress: core answers without the OCR
/// engine's lock.
///
/// # Errors
///
/// Returns `task_failed` when the blocking task panics.
#[tauri::command]
pub(crate) async fn document_analyzer_status(
    state: State<'_, AppState>,
) -> CommandResult<AnalyzerStatus> {
    let model_dir = state.ocr_model_dir().clone();

    run_blocking(move || Ok(analyzer_status(Some(model_dir.as_path())))).await
}

/// Analyzes a document the webview picked and returns a draft entry
/// suggestion.
///
/// Requires the unlocked vault, for the entity's accounts and currency.
/// Nothing is stored. Core reads the document's text with the app's stored
/// language.
///
/// # Errors
///
/// Returns `file_data_invalid` when `data_base64` is not base64;
/// `file_too_large` (with the cap as `max_mb`), `file_empty`,
/// `file_type_unsupported` and `name_required` when the document is refused;
/// `not_found` when the entity does not exist; and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn document_analyze(
    state: State<'_, AppState>,
    arguments: Arguments<AnalyzeArguments>,
) -> CommandResult<DocumentSuggestion> {
    let Arguments(AnalyzeArguments {
        entity_id,
        document,
    }) = arguments;
    let document = document.decode()?;

    let vault = state.vault();
    let model_dir = state.ocr_model_dir().clone();
    let data_dir = state.data_dir().to_path_buf();
    state.touch();

    run_blocking(move || {
        let locale = stored_text_locale(&data_dir);

        analyze_readonly(&vault, &model_dir, entity_id, &document.as_new(), locale)
    })
    .await
}

/// The arguments of [`document_analyze`], as the webview names them:
/// `entityId`, `filename`, `mimeType` and `dataBase64`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnalyzeArguments {
    /// Entity whose accounts and currency the suggestion is made for.
    entity_id: EntityId,
    /// The document to analyze.
    #[serde(flatten)]
    document: PickedDocument,
}

/// Analyzes the document at `path` and returns a draft entry suggestion.
///
/// Requires the unlocked vault and a granted path: the file the user dropped
/// on a window. The size and type are checked from the file's metadata before
/// it is read, so an oversized drop is never loaded into memory. Nothing is
/// stored.
///
/// # Errors
///
/// Returns `path_not_granted` for a path the user did not drop on a window;
/// `file_unreadable` when the file cannot be read; `file_too_large` (with the
/// cap as `max_mb`), `file_empty`, `file_type_unsupported` and
/// `name_required` when the document is refused; `not_found` when the entity
/// does not exist; and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn document_analyze_path(
    state: State<'_, AppState>,
    entity_id: EntityId,
    path: String,
) -> CommandResult<DocumentSuggestion> {
    let filename = dropped_file_name(&path);
    let grants = state.path_grants();
    let path =
        run_blocking(move || require_granted_path(&grants, GrantPurpose::Document, &path)).await?;

    let vault = state.vault();
    let model_dir = state.ocr_model_dir().clone();
    let data_dir = state.data_dir().to_path_buf();
    state.touch();

    run_blocking(move || {
        let locale = stored_text_locale(&data_dir);

        let document = read_dropped_document(&path, &filename)?;

        analyze_readonly(&vault, &model_dir, entity_id, &document.as_new(), locale)
    })
    .await
}

/// Reads a dropped file as a document named `filename`.
///
/// Core checks the size and the type from the file's metadata and name
/// before it reads the file, so an oversized drop is never loaded into
/// memory.
///
/// # Errors
///
/// Returns `file_unreadable` when the file's metadata or bytes cannot be
/// read, and `file_too_large` (with the cap as `max_mb`), `file_empty`,
/// `file_type_unsupported` and `name_required` when the document is refused.
pub(super) fn read_dropped_document(path: &Path, filename: &str) -> CommandResult<ReadDocument> {
    read_validated_file(path, filename).map_err(|err| match err {
        CoreError::Io { .. } => CommandError::desktop(
            DesktopError::FileUnreadable,
            format!("could not read the dropped file: {err}"),
        ),
        refused => CommandError::from(refused),
    })
}

/// Lists the documents stored for an entity, metadata only.
///
/// Requires the unlocked vault. An unknown entity has no documents, so it
/// yields an empty list, not an error.
///
/// # Errors
///
/// Returns `validation_internal` when a stored identifier cannot be parsed,
/// and the [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn document_list(
    state: State<'_, AppState>,
    entity_id: EntityId,
) -> CommandResult<Vec<DocumentMeta>> {
    with_connection(&state, move |conn| list_documents(conn, entity_id)).await
}

/// Returns one stored document, decrypted, for the in-app viewer.
///
/// Requires the unlocked vault. The bytes cross IPC only; nothing is written
/// to disk.
///
/// # Errors
///
/// Returns `not_found` when the document does not exist,
/// `validation_internal` when a stored identifier cannot be parsed, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn document_get(
    state: State<'_, AppState>,
    document_id: DocumentId,
) -> CommandResult<DocumentContent> {
    with_connection(&state, move |conn| {
        let (meta, data) = get_document(conn, document_id)?;
        Ok(DocumentContent {
            meta,
            data_base64: base64::engine::general_purpose::STANDARD.encode(data),
        })
    })
    .await
}

/// Deletes a stored document, permanently. The entry it was attached to
/// stays.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `not_found` when the document does not exist, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn document_delete(
    state: State<'_, AppState>,
    document_id: DocumentId,
) -> CommandResult<()> {
    with_connection(&state, move |conn| delete_document(conn, document_id)).await
}

/// Stores a document the webview picked and attaches it to a posted entry.
///
/// Requires the unlocked vault. The document is not analyzed: analysis
/// belongs to the flow that drafts a new entry, so the stored analysis stays
/// empty here.
///
/// # Errors
///
/// Returns `file_data_invalid` when `data_base64` is not base64; `not_found`
/// when the entry does not exist; `wrong_book` when it belongs to another
/// entity; `file_too_large` (with the cap as `max_mb`), `file_empty`,
/// `file_type_unsupported` and `name_required` when the document is refused;
/// `name_taken` when the entity already stores a document under the file
/// name; and the [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn document_attach(
    state: State<'_, AppState>,
    arguments: Arguments<AttachArguments>,
) -> CommandResult<DocumentMeta> {
    let Arguments(AttachArguments {
        entity_id,
        entry_id,
        document,
    }) = arguments;
    let document = document.decode()?;

    with_connection(&state, move |conn| {
        attach_document(conn, entity_id, entry_id, &document.as_new())
    })
    .await
}

/// The arguments of [`document_attach`], as the webview names them:
/// `entityId`, `entryId`, `filename`, `mimeType` and `dataBase64`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttachArguments {
    /// Entity the entry belongs to.
    entity_id: EntityId,
    /// Entry the document is attached to.
    entry_id: JournalEntryId,
    /// The document to store.
    #[serde(flatten)]
    document: PickedDocument,
}

/// Exports a stored document, decrypted, to a path chosen in a native save
/// dialog. Returns the path written, or `None` if the user cancelled.
///
/// Requires the unlocked vault. Like the journal CSV export
/// ([`csv_export_journal`](crate::commands::csv_export_journal)) and the
/// report PDF ([`report_export_pdf`](crate::commands::report_export_pdf)),
/// this writes plaintext to disk, and like them only to a path the user
/// picked in the dialog ([`save_with_dialog`]).
///
/// # Errors
///
/// Returns `not_found` when the document does not exist,
/// `validation_internal` when a stored identifier cannot be parsed,
/// `save_location_invalid` when the dialog's answer is not a path,
/// `save_failed` when the file cannot be written, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn document_export(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    document_id: DocumentId,
) -> CommandResult<Option<String>> {
    let (document, data) =
        with_connection(&state, move |conn| get_document(conn, document_id)).await?;

    let target = SaveTarget {
        filter: None,
        file_name: document.filename,
        complete_path: std::convert::identity,
    };
    save_with_dialog(&app, target, data).await
}

/// Analyzes a document held in memory and suggests a draft entry.
///
/// The vault is held only to read the entity and its accounts, and released
/// before the OCR pass, which can take seconds. Stores nothing.
///
/// # Errors
///
/// Returns the validation errors of a refused document, `not_found` when the
/// entity does not exist, and `vault_locked`, `database` or `vault_corrupt`
/// from the vault.
fn analyze_readonly(
    vault: &GatedVault,
    model_dir: &Path,
    entity_id: EntityId,
    document: &NewDocument<'_>,
    locale: Locale,
) -> CommandResult<DocumentSuggestion> {
    document.validate()?;

    let (accounts, entity) = {
        let guard = vault.acquire();
        let conn = guard.connection()?;
        (
            suggest_accounts_for_entity(conn, entity_id)?,
            get_entity(conn, entity_id)?,
        )
    };

    let suggestion = analyze_document_bytes(
        document,
        &AnalyzeContext {
            template: entity.chart_template,
            accounts: &accounts,
            default_currency: entity.base_currency,
            locale,
        },
        Some(model_dir),
    );

    Ok(suggestion)
}

/// The document commands that read their arguments as one struct, invoked
/// through the mock IPC the way the webview invokes them.
///
/// Not built on Windows, where the mock runtime keeps a test executable from
/// starting; `commands::support::ipc_test_support` says why.
#[cfg(test)]
#[cfg(not(windows))]
mod ipc_tests {
    use crate::commands::documents::{
        document_analyze, document_analyzer_status, document_attach, document_list,
    };
    use crate::commands::support::ipc_test_support::MockApp;
    use base64::Engine;
    use oikonomia_core::domain::ChartTemplate;
    use oikonomia_core::ledger::{
        CreateEntity, PostSimpleEntry, PostSimpleEntryRequest, SimpleEntryKind, create_entity,
        list_accounts, post_simple_entry,
    };
    use oikonomia_core::prefs::Locale;
    use oikonomia_core::vault::Connection;

    /// The ids the tests send, as the frontend holds them.
    struct Ids {
        /// The book the entry is in.
        entity: String,
        /// An expense entry of that book.
        entry: String,
        /// A second book, which the entry is not in.
        other_entity: String,
    }

    /// Starts the mock app with the document commands registered, over a
    /// vault that holds two books and one entry in the first.
    fn mock_books(label: &str) -> (MockApp, Ids) {
        MockApp::start(
            label,
            tauri::generate_handler![
                document_analyze,
                document_analyzer_status,
                document_attach,
                document_list
            ],
            seed_books,
        )
    }

    /// Creates the two books and the entry.
    fn seed_books(conn: &Connection) -> Ids {
        let book = |name: &str| CreateEntity {
            name: name.into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: None,
        };
        let entity = create_entity(conn, &book("Home"), Locale::En).unwrap();
        let other = create_entity(conn, &book("Shop"), Locale::En).unwrap();

        let accounts = list_accounts(conn, entity.id).unwrap();
        let account = |code: &str| {
            accounts
                .iter()
                .find(|account| account.code == code)
                .map(|account| account.id)
        };
        let request = PostSimpleEntryRequest {
            entity_id: entity.id,
            kind: SimpleEntryKind::Expense,
            bill_status: None,
            entry_date: "2026-08-05".into(),
            description: "Groceries".into(),
            reference: None,
            amount_minor: 2_500,
            category_account_id: account("5100"),
            wallet_account_id: account("1010"),
            payable_account_id: None,
            from_account_id: None,
            to_account_id: None,
        };
        let posted = post_simple_entry(conn, &PostSimpleEntry::try_from(request).unwrap()).unwrap();

        Ids {
            entity: entity.id.to_string(),
            entry: posted.entry.id.to_string(),
            other_entity: other.id.to_string(),
        }
    }

    /// `bytes` as the base64 the webview sends.
    fn base64_of(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    // The payloads below are the objects `documentAttach` and
    // `documentAnalyze` in `web/src/lib/api.ts` pass to `invoke`: camelCase
    // keys, the document's three beside the ids.

    #[test]
    fn the_ipc_call_the_frontend_makes_attaches_the_document_to_the_entry() {
        let (app, ids) = mock_books("attach");

        let meta = app
            .invoke(
                "document_attach",
                serde_json::json!({
                    "entityId": ids.entity,
                    "entryId": ids.entry,
                    "filename": "receipt.txt",
                    "mimeType": "text/plain",
                    "dataBase64": base64_of(b"TOTAL 25,00"),
                }),
            )
            .unwrap();

        assert_eq!(meta["filename"], "receipt.txt");
        assert_eq!(meta["mime_type"], "text/plain");
        assert_eq!(meta["entry_id"], ids.entry);
        assert_eq!(meta["entry_description"], "Groceries");

        let listed = app
            .invoke(
                "document_list",
                serde_json::json!({ "entityId": ids.entity }),
            )
            .unwrap();
        assert_eq!(listed[0]["id"], meta["id"]);
    }

    #[test]
    fn an_attachment_core_refuses_comes_back_with_its_code() {
        let (app, ids) = mock_books("attach-refused");
        let attach = |entity: &str, filename: &str, data_base64: &str| {
            app.invoke(
                "document_attach",
                serde_json::json!({
                    "entityId": entity,
                    "entryId": ids.entry,
                    "filename": filename,
                    "mimeType": "",
                    "dataBase64": data_base64,
                }),
            )
            .unwrap_err()
        };
        let text = base64_of(b"TOTAL 25,00");

        assert_eq!(
            attach(&ids.other_entity, "a.txt", &text)["code"],
            "wrong_book"
        );
        assert_eq!(
            attach(&ids.entity, "a.exe", &text)["code"],
            "file_type_unsupported"
        );
        assert_eq!(
            attach(&ids.entity, "a.txt", "not base64!")["code"],
            "file_data_invalid"
        );

        let listed = app
            .invoke(
                "document_list",
                serde_json::json!({ "entityId": ids.entity }),
            )
            .unwrap();
        assert_eq!(listed.as_array().map(Vec::len), Some(0));
    }

    #[test]
    fn an_attachment_without_the_entry_is_refused_by_the_argument_layer() {
        let (app, ids) = mock_books("attach-no-entry");

        let refused = app
            .invoke(
                "document_attach",
                serde_json::json!({
                    "entityId": ids.entity,
                    "filename": "a.txt",
                    "mimeType": "text/plain",
                    "dataBase64": base64_of(b"a"),
                }),
            )
            .unwrap_err();

        // Tauri's own refusal is text, not a coded error.
        assert!(refused.is_string(), "{refused}");
    }

    #[test]
    fn the_ipc_call_the_frontend_makes_analyzes_a_document_into_a_suggestion() {
        let (app, ids) = mock_books("analyze");

        let suggestion = app
            .invoke(
                "document_analyze",
                serde_json::json!({
                    "entityId": ids.entity,
                    "filename": "bill.txt",
                    "mimeType": "text/plain",
                    "dataBase64": base64_of(b"Invoice\nDate 15/03/2026\nTOTAL 45,90 EUR"),
                }),
            )
            .unwrap();

        assert_eq!(suggestion["source"], "heuristic");
        assert_eq!(suggestion["amount_minor"], 4590);
        assert_eq!(suggestion["entry_date"], "2026-03-15");
        assert!(suggestion["wallet_account_id"].is_string(), "{suggestion}");

        // Analysis stores nothing.
        let listed = app
            .invoke(
                "document_list",
                serde_json::json!({ "entityId": ids.entity }),
            )
            .unwrap();
        assert_eq!(listed.as_array().map(Vec::len), Some(0));
    }

    #[test]
    fn a_document_that_may_not_be_stored_is_not_analyzed() {
        let (app, ids) = mock_books("analyze-refused");

        let refused = app
            .invoke(
                "document_analyze",
                serde_json::json!({
                    "entityId": ids.entity,
                    "filename": "tool.exe",
                    "mimeType": "application/x-msdownload",
                    "dataBase64": base64_of(b"MZ"),
                }),
            )
            .unwrap_err();

        assert_eq!(refused["code"], "file_type_unsupported");
    }

    #[test]
    fn the_analyzer_status_comes_back_with_its_three_fields() {
        let (app, _ids) = mock_books("analyzer-status");

        // `documentAnalyzerStatus` in `web/src/lib/api.ts` sends no arguments.
        let status = app
            .invoke("document_analyzer_status", serde_json::json!({}))
            .unwrap();

        // The mock app's model directory is its empty data directory, and no
        // test of this crate loads an OCR engine.
        assert_eq!(
            status,
            serde_json::json!({
                "ocr_available": false,
                "offline": true,
                "hint": "models_missing",
            })
        );
    }
}
