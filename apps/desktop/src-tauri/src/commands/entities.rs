//! Entity (book) commands.

use crate::commands::support::{stored_text_locale, with_vault_blocking};
use crate::error::CommandResult;
use crate::state::AppState;
use oikonomia_core::domain::{Entity, EntityId};
use oikonomia_core::ledger::{
    CreateEntity, archive_entity, create_entity, delete_entity, list_entities, update_entity,
};
use tauri::State;

/// List non-archived entities.
#[tauri::command]
pub(crate) async fn entity_list(state: State<'_, AppState>) -> CommandResult<Vec<Entity>> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        list_entities(conn)
    })
    .await
}

/// Create entity with chart template.
#[tauri::command]
pub(crate) async fn entity_create(
    state: State<'_, AppState>,
    input: CreateEntity,
) -> CommandResult<Entity> {
    // The seeded account names are written in the language the app is set to
    // now. It comes from the stored preference, never from the webview.
    let locale = stored_text_locale(&state);

    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        create_entity(conn, &input, locale)
    })
    .await
}

/// Rename entity.
#[tauri::command]
pub(crate) async fn entity_update(
    state: State<'_, AppState>,
    id: EntityId,
    name: String,
) -> CommandResult<Entity> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        update_entity(conn, id, &name)
    })
    .await
}

/// Archive entity (soft-hide).
#[tauri::command]
pub(crate) async fn entity_archive(state: State<'_, AppState>, id: EntityId) -> CommandResult<()> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        archive_entity(conn, id)
    })
    .await
}

/// Permanently delete an entity and all of its books data.
#[tauri::command]
pub(crate) async fn entity_delete(state: State<'_, AppState>, id: EntityId) -> CommandResult<()> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        delete_entity(conn, id)
    })
    .await
}
