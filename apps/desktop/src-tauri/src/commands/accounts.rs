//! Account commands.

use crate::commands::support::{stored_text_locale, with_connection};
use crate::error::CommandResult;
use crate::state::AppState;
use oikonomia_core::default_accounts::{DefaultAccounts, default_accounts_for_entity};
use oikonomia_core::domain::{Account, AccountId, EntityId};
use oikonomia_core::ledger::{
    CreateAccount, PostedEntryView, RegisterLine, UpdateAccount, account_balance, account_register,
    archive_account, create_account, list_accounts, set_account_opening_balance, update_account,
};
use tauri::State;

/// List accounts for an entity.
#[tauri::command]
pub(crate) async fn account_list(
    state: State<'_, AppState>,
    entity_id: EntityId,
) -> CommandResult<Vec<Account>> {
    with_connection(&state, move |conn| list_accounts(conn, entity_id)).await
}

/// The default account for each role the entry forms need.
///
/// Chosen in Rust by the seeded account's template code and type, never by
/// name, so it is right for a renamed or translated chart.
#[tauri::command]
pub(crate) async fn account_defaults(
    state: State<'_, AppState>,
    entity_id: EntityId,
) -> CommandResult<DefaultAccounts> {
    with_connection(&state, move |conn| {
        default_accounts_for_entity(conn, entity_id)
    })
    .await
}

/// Create account.
#[tauri::command]
pub(crate) async fn account_create(
    state: State<'_, AppState>,
    input: CreateAccount,
) -> CommandResult<Account> {
    with_connection(&state, move |conn| create_account(conn, &input)).await
}

/// Update account.
#[tauri::command]
pub(crate) async fn account_update(
    state: State<'_, AppState>,
    input: UpdateAccount,
) -> CommandResult<Account> {
    with_connection(&state, move |conn| update_account(conn, &input)).await
}

/// Archive (deactivate) account.
#[tauri::command]
pub(crate) async fn account_archive(
    state: State<'_, AppState>,
    id: AccountId,
) -> CommandResult<()> {
    with_connection(&state, move |conn| archive_account(conn, id)).await
}

/// Account register.
#[tauri::command]
pub(crate) async fn account_register_cmd(
    state: State<'_, AppState>,
    account_id: AccountId,
    from: Option<String>,
    to: Option<String>,
) -> CommandResult<Vec<RegisterLine>> {
    with_connection(&state, move |conn| {
        account_register(conn, account_id, from.as_deref(), to.as_deref())
    })
    .await
}

/// Signed normal balance of one account as of a date.
#[tauri::command]
pub(crate) async fn account_balance_cmd(
    state: State<'_, AppState>,
    account_id: AccountId,
    as_of: String,
) -> CommandResult<i64> {
    with_connection(&state, move |conn| {
        account_balance(conn, account_id, &as_of)
    })
    .await
}

/// Set an account's balance as of a date by posting the difference against
/// the book's Opening Balances equity account.
#[tauri::command]
pub(crate) async fn account_set_opening_balance(
    state: State<'_, AppState>,
    account_id: AccountId,
    target_minor: i64,
    as_of: String,
) -> CommandResult<PostedEntryView> {
    let locale = stored_text_locale(&state);

    with_connection(&state, move |conn| {
        set_account_opening_balance(conn, account_id, target_minor, &as_of, locale)
    })
    .await
}
