//! Recurring template commands.
//!
//! A template is a saved simple entry with a cadence. Nothing posts on its
//! own: the UI shows which templates are due and the user posts each one
//! ([`recurring_post`]). Every command here requires the unlocked vault.

use crate::commands::support::with_connection;
use crate::error::CommandResult;
use crate::state::AppState;
use oikonomia_core::domain::{EntityId, RecurringTemplateId};
use oikonomia_core::ledger::{
    CreateRecurringTemplate, RecurringPostResult, RecurringTemplateView, UpdateRecurringTemplate,
    create_recurring_template, delete_recurring_template, get_recurring_template,
    list_recurring_templates, post_recurring_template, update_recurring_template,
};
use tauri::State;

/// Lists an entity's recurring templates, each marked due when its next date
/// is today or earlier by the UTC calendar.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `not_found` when the entity does not exist, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn recurring_list(
    state: State<'_, AppState>,
    entity_id: EntityId,
) -> CommandResult<Vec<RecurringTemplateView>> {
    with_connection(&state, move |conn| {
        list_recurring_templates(conn, entity_id)
    })
    .await
}

/// Returns one recurring template.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `not_found` when the template does not exist, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn recurring_get(
    state: State<'_, AppState>,
    id: RecurringTemplateId,
) -> CommandResult<RecurringTemplateView> {
    with_connection(&state, move |conn| get_recurring_template(conn, id)).await
}

/// Creates a recurring template.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `not_found` when the entity or an account does not exist,
/// `name_required` for an empty name, `amount_not_positive` for an amount of
/// zero or less, `day_of_month_invalid` for a monthly template without a day
/// from 1 to 31 or another cadence with one, `invalid_date` when the next
/// date is not a date, the account errors of a simple entry
/// (`account_required`, `account_wrong_type`, `bill_status_required`,
/// `same_account`, `account_wrong_entity`, `account_inactive`), and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn recurring_create(
    state: State<'_, AppState>,
    input: CreateRecurringTemplate,
) -> CommandResult<RecurringTemplateView> {
    with_connection(&state, move |conn| create_recurring_template(conn, &input)).await
}

/// Replaces the editable fields of a recurring template. Its entity cannot
/// change.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `not_found` when the template or an account does not exist, the
/// validation errors of [`recurring_create`], and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn recurring_update(
    state: State<'_, AppState>,
    input: UpdateRecurringTemplate,
) -> CommandResult<RecurringTemplateView> {
    with_connection(&state, move |conn| update_recurring_template(conn, &input)).await
}

/// Deletes a recurring template. Entries already posted from it stay.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `not_found` when the template does not exist, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn recurring_delete(
    state: State<'_, AppState>,
    id: RecurringTemplateId,
) -> CommandResult<()> {
    with_connection(&state, move |conn| delete_recurring_template(conn, id)).await
}

/// Posts one entry from a template and advances the template's next date.
///
/// Requires the unlocked vault. `entry_date` defaults to the template's next
/// date and `amount_minor` to its amount. An override applies to this post
/// only, and the cadence still steps from the stored next date, not from the
/// date posted.
///
/// # Errors
///
/// Returns `not_found` when the template does not exist, `date_out_of_range`
/// when the next date cannot be advanced, `day_of_month_invalid` for a
/// monthly template stored without a day, the
/// [simple-entry errors](crate::commands::journal#simple-entry-errors), and
/// the [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn recurring_post(
    state: State<'_, AppState>,
    id: RecurringTemplateId,
    entry_date: Option<String>,
    amount_minor: Option<i64>,
) -> CommandResult<RecurringPostResult> {
    with_connection(&state, move |conn| {
        post_recurring_template(conn, id, entry_date.as_deref(), amount_minor)
    })
    .await
}
