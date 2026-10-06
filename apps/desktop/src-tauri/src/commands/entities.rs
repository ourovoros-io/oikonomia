//! Entity (book) commands.

use crate::commands::support::{with_connection, with_localized_connection};
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
    with_connection(&state, list_entities).await
}

/// Create entity with chart template.
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

/// Rename entity.
#[tauri::command]
pub(crate) async fn entity_update(
    state: State<'_, AppState>,
    id: EntityId,
    name: String,
) -> CommandResult<Entity> {
    with_connection(&state, move |conn| update_entity(conn, id, &name)).await
}

/// Archive entity (soft-hide).
#[tauri::command]
pub(crate) async fn entity_archive(state: State<'_, AppState>, id: EntityId) -> CommandResult<()> {
    with_connection(&state, move |conn| archive_entity(conn, id)).await
}

/// Permanently delete an entity and all of its books data.
#[tauri::command]
pub(crate) async fn entity_delete(state: State<'_, AppState>, id: EntityId) -> CommandResult<()> {
    with_connection(&state, move |conn| delete_entity(conn, id)).await
}
