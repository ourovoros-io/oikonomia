//! Entity commands. An entity is one set of books: a person, a household or a
//! company.
//!
//! Every command here requires the unlocked vault and is one call into
//! `oikonomia_core::ledger`.

use crate::commands::support::{with_connection, with_localized_connection};
use crate::error::CommandResult;
use crate::state::AppState;
use oikonomia_core::domain::{Entity, EntityId};
use oikonomia_core::ledger::{
    CreateEntity, archive_entity, create_entity, delete_entity, list_archived_entities,
    list_entities, unarchive_entity, update_entity,
};
use tauri::State;

/// Lists the entities that are not archived.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns the [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entity_list(state: State<'_, AppState>) -> CommandResult<Vec<Entity>> {
    with_connection(&state, list_entities).await
}

/// Lists the archived entities, which [`entity_list`] leaves out.
///
/// Requires the unlocked vault. The two lists have no entity in common.
///
/// # Errors
///
/// Returns the [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entity_list_archived(state: State<'_, AppState>) -> CommandResult<Vec<Entity>> {
    with_connection(&state, list_archived_entities).await
}

/// Creates an entity and seeds its chart of accounts from the chosen
/// template.
///
/// Requires the unlocked vault. The seeded account names are written in the
/// language the app is set to now, which comes from the stored preference,
/// never from the webview.
///
/// # Errors
///
/// Returns `name_required` for an empty name, `name_taken` when another
/// entity that is not archived has the name, `currency_invalid` for a base
/// currency that is not a currency code, `validation_internal` for a fiscal
/// year start outside months 1 to 12, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entity_create(
    state: State<'_, AppState>,
    input: CreateEntity,
) -> CommandResult<Entity> {
    with_localized_connection(&state, move |conn, locale| {
        create_entity(conn, &input, locale)
    })
    .await
}

/// Renames an entity.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `not_found` when the entity does not exist or is archived,
/// `name_required` for an empty name, `name_taken` when another entity that
/// is not archived has the name, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entity_update(
    state: State<'_, AppState>,
    id: EntityId,
    name: String,
) -> CommandResult<Entity> {
    with_connection(&state, move |conn| update_entity(conn, id, &name)).await
}

/// Archives an entity: it leaves [`entity_list`] for
/// [`entity_list_archived`], keeps its books, and can no longer be written
/// to. [`entity_unarchive`] undoes it.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `not_found` when the entity does not exist or is already
/// archived, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entity_archive(state: State<'_, AppState>, id: EntityId) -> CommandResult<()> {
    with_connection(&state, move |conn| archive_entity(conn, id)).await
}

/// Makes an archived entity active again: it returns to [`entity_list`] and
/// can be written to.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `not_found` when the entity does not exist or is not archived,
/// `name_taken` (with the name as `name`) when an entity that is not archived
/// has taken its name in the meantime, in which case it stays archived, and
/// the [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entity_unarchive(
    state: State<'_, AppState>,
    id: EntityId,
) -> CommandResult<()> {
    with_connection(&state, move |conn| unarchive_entity(conn, id)).await
}

/// Deletes an entity and all of its books, permanently.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `not_found` when the entity does not exist, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entity_delete(state: State<'_, AppState>, id: EntityId) -> CommandResult<()> {
    with_connection(&state, move |conn| delete_entity(conn, id)).await
}

/// The archive commands invoked through the mock IPC, the way the Settings
/// screen invokes them.
///
/// Not built on Windows, where the mock runtime keeps a test executable from
/// starting; `commands::support::ipc_test_support` says why.
#[cfg(test)]
#[cfg(not(windows))]
mod ipc_tests {
    use crate::commands::entities::{
        entity_archive, entity_create, entity_list, entity_list_archived, entity_unarchive,
    };
    use crate::commands::support::ipc_test_support::MockApp;
    use oikonomia_core::domain::ChartTemplate;
    use oikonomia_core::ledger::{CreateEntity, create_entity};
    use oikonomia_core::prefs::Locale;
    use oikonomia_core::vault::Connection;

    /// Starts the mock app with the entity commands registered, over a vault
    /// that holds the books "Home" and "Shop". Returns their ids, in that
    /// order, as the frontend holds them.
    fn mock_books(label: &str) -> (MockApp, [String; 2]) {
        MockApp::start(
            label,
            tauri::generate_handler![
                entity_list,
                entity_list_archived,
                entity_create,
                entity_archive,
                entity_unarchive,
                crate::commands::accounts::account_create
            ],
            |conn| ["Home", "Shop"].map(|name| seed_book(conn, name)),
        )
    }

    /// Creates a book named `name` with no accounts and returns its id.
    fn seed_book(conn: &Connection, name: &str) -> String {
        let book = CreateEntity {
            name: name.into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Blank,
            fiscal_year_start_month: None,
        };

        create_entity(conn, &book, Locale::En)
            .unwrap()
            .id
            .to_string()
    }

    /// Returns the names `command` lists, in the order it lists them.
    fn listed(app: &MockApp, command: &str) -> Vec<String> {
        let books = app.invoke(command, serde_json::json!({})).unwrap();

        books
            .as_array()
            .unwrap()
            .iter()
            .map(|book| book["name"].as_str().unwrap().to_owned())
            .collect()
    }

    /// Invokes `command` with the id payload the frontend sends.
    fn invoke_with_id(
        app: &MockApp,
        command: &str,
        id: &str,
    ) -> Result<serde_json::Value, serde_json::Value> {
        app.invoke(command, serde_json::json!({ "id": id }))
    }

    /// Asks for a new account in the book `entity`, a write that takes the
    /// entity.
    fn create_account(app: &MockApp, entity: &str) -> Result<serde_json::Value, serde_json::Value> {
        app.invoke(
            "account_create",
            serde_json::json!({
                "input": {
                    "entity_id": entity,
                    "code": "1000",
                    "name": "Cash",
                    "account_type": "asset",
                },
            }),
        )
    }

    #[test]
    fn archiving_moves_a_book_to_the_archived_list_and_makes_it_read_only() {
        let (app, [home, _shop]) = mock_books("entity-archive");
        assert_eq!(listed(&app, "entity_list"), ["Home", "Shop"]);
        assert_eq!(listed(&app, "entity_list_archived"), Vec::<String>::new());

        let answer = invoke_with_id(&app, "entity_archive", &home).unwrap();

        assert_eq!(answer, serde_json::Value::Null);
        assert_eq!(listed(&app, "entity_list"), ["Shop"]);
        assert_eq!(listed(&app, "entity_list_archived"), ["Home"]);
        let refused = create_account(&app, &home).unwrap_err();
        assert_eq!(refused["code"], "not_found", "{refused}");
        assert_eq!(refused["params"]["resource"], "entity");
    }

    #[test]
    fn the_archived_list_sends_a_book_in_the_shape_of_the_active_list() {
        let (app, [home, _shop]) = mock_books("entity-archived-shape");
        let active = app.invoke("entity_list", serde_json::json!({})).unwrap();
        invoke_with_id(&app, "entity_archive", &home).unwrap();

        let archived = app
            .invoke("entity_list_archived", serde_json::json!({}))
            .unwrap();

        assert_eq!(archived, serde_json::json!([active[0]]));
        assert_eq!(archived[0]["id"], home);
    }

    #[test]
    fn unarchiving_returns_the_book_to_the_list_and_to_being_writable() {
        let (app, [home, _shop]) = mock_books("entity-unarchive");
        invoke_with_id(&app, "entity_archive", &home).unwrap();

        let answer = invoke_with_id(&app, "entity_unarchive", &home).unwrap();

        assert_eq!(answer, serde_json::Value::Null);
        assert_eq!(listed(&app, "entity_list"), ["Home", "Shop"]);
        assert_eq!(listed(&app, "entity_list_archived"), Vec::<String>::new());
        let account = create_account(&app, &home).unwrap();
        assert_eq!(account["entity_id"], home);
    }

    #[test]
    fn unarchiving_a_book_whose_name_was_taken_answers_name_taken_and_changes_nothing() {
        let (app, [home, _shop]) = mock_books("entity-unarchive-clash");
        invoke_with_id(&app, "entity_archive", &home).unwrap();
        app.invoke(
            "entity_create",
            serde_json::json!({
                "input": {
                    "name": "home",
                    "base_currency": "EUR",
                    "chart_template": "blank",
                    "fiscal_year_start_month": null,
                },
            }),
        )
        .unwrap();

        let refused = invoke_with_id(&app, "entity_unarchive", &home).unwrap_err();

        assert_eq!(refused["code"], "name_taken", "{refused}");
        assert_eq!(refused["params"], serde_json::json!({ "name": "Home" }));
        assert_eq!(listed(&app, "entity_list"), ["home", "Shop"]);
        assert_eq!(listed(&app, "entity_list_archived"), ["Home"]);
    }

    #[test]
    fn unarchiving_an_active_or_an_unknown_book_answers_not_found() {
        let (app, [home, _shop]) = mock_books("entity-unarchive-missing");

        for id in [home.as_str(), "99999999-9999-4999-8999-999999999999"] {
            let refused = invoke_with_id(&app, "entity_unarchive", id).unwrap_err();

            assert_eq!(refused["code"], "not_found", "{refused}");
            assert_eq!(refused["params"]["resource"], "entity");
        }
        assert_eq!(listed(&app, "entity_list"), ["Home", "Shop"]);
    }
}
