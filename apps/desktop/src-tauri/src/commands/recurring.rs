//! Recurring template commands.

use crate::commands::support::with_vault_blocking;
use crate::error::CommandResult;
use crate::state::AppState;
use oikonomia_core::domain::{EntityId, RecurringTemplateId};
use oikonomia_core::ledger::{
    CreateRecurringTemplate, RecurringPostResult, RecurringTemplateView, UpdateRecurringTemplate,
    create_recurring_template, delete_recurring_template, get_recurring_template,
    list_recurring_templates, post_recurring_template, update_recurring_template,
};
use tauri::State;

/// List recurring templates for an entity (`due` when `next_date` ≤ UTC today).
#[tauri::command]
pub(crate) async fn recurring_list(
    state: State<'_, AppState>,
    entity_id: EntityId,
) -> CommandResult<Vec<RecurringTemplateView>> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        list_recurring_templates(conn, entity_id)
    })
    .await
}

/// Fetch one template.
#[tauri::command]
pub(crate) async fn recurring_get(
    state: State<'_, AppState>,
    id: RecurringTemplateId,
) -> CommandResult<RecurringTemplateView> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        get_recurring_template(conn, id)
    })
    .await
}

/// Create a local recurring template.
#[tauri::command]
pub(crate) async fn recurring_create(
    state: State<'_, AppState>,
    input: CreateRecurringTemplate,
) -> CommandResult<RecurringTemplateView> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        create_recurring_template(conn, &input)
    })
    .await
}

/// Replace mutable fields on a template.
#[tauri::command]
pub(crate) async fn recurring_update(
    state: State<'_, AppState>,
    input: UpdateRecurringTemplate,
) -> CommandResult<RecurringTemplateView> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        update_recurring_template(conn, &input)
    })
    .await
}

/// Delete a template. Posted journal entries are left intact.
#[tauri::command]
pub(crate) async fn recurring_delete(
    state: State<'_, AppState>,
    id: RecurringTemplateId,
) -> CommandResult<()> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        delete_recurring_template(conn, id)
    })
    .await
}

/// Post one journal entry from a template, then advance `next_date`.
///
/// Defaults: `entry_date` = template `next_date`, `amount_minor` = template
/// amount. Overrides apply only to this post (confirm-sheet adjust-before-save);
/// cadence still steps from the stored `next_date`.
#[tauri::command]
pub(crate) async fn recurring_post(
    state: State<'_, AppState>,
    id: RecurringTemplateId,
    entry_date: Option<String>,
    amount_minor: Option<i64>,
) -> CommandResult<RecurringPostResult> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        post_recurring_template(conn, id, entry_date.as_deref(), amount_minor)
    })
    .await
}
