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
    CreateEntity, archive_entity, create_entity, delete_entity, list_entities, update_entity,
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

/// Archives an entity: it leaves the lists but keeps its books.
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
