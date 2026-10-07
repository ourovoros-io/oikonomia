//! Account commands: the chart of accounts of one entity, account registers
//! and balances.
//!
//! Every command here requires the unlocked vault. The four commands whose
//! names end in `_cmd` across this layer carry the suffix because the core
//! function they call has the bare name; the suffix is part of the IPC name.

use crate::commands::support::{with_connection, with_localized_connection};
use crate::error::CommandResult;
use crate::state::AppState;
use oikonomia_core::default_accounts::{DefaultAccounts, default_accounts_for_entity};
use oikonomia_core::domain::{Account, AccountId, EntityId};
use oikonomia_core::ledger::{
    CreateAccount, PostedEntryView, RegisterLine, UpdateAccount, account_balance, account_register,
    archive_account, create_account, list_accounts, set_account_opening_balance, update_account,
};
use oikonomia_core::util::DateText;
use tauri::State;

/// Lists an entity's accounts, active and inactive.
///
/// Requires the unlocked vault. An unknown entity has no accounts, so it
/// yields an empty list, not an error.
///
/// # Errors
///
/// Returns the [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn account_list(
    state: State<'_, AppState>,
    entity_id: EntityId,
) -> CommandResult<Vec<Account>> {
    with_connection(&state, move |conn| list_accounts(conn, entity_id)).await
}

/// Returns the default account for each role the entry forms need.
///
/// Requires the unlocked vault. The accounts are chosen in core by the seeded
/// account's template code and type, never by name, so the choice is right
/// for a renamed or translated chart.
///
/// # Errors
///
/// Returns `not_found` when the entity does not exist, and the
/// [common vault errors](crate::commands#common-vault-errors).
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

/// Creates an account in an entity's chart.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `not_found` when the entity does not exist or is archived,
/// `name_required` for an empty code or name, `account_code_taken` when the
/// entity already has an account with the code, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn account_create(
    state: State<'_, AppState>,
    input: CreateAccount,
) -> CommandResult<Account> {
    with_connection(&state, move |conn| create_account(conn, &input)).await
}

/// Updates an account's code, name, active flag and sort order.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `not_found` when the account does not exist, `name_required` for
/// an empty code or name, `account_code_taken` when the entity already has
/// another account with the code, `system_account_protected` for an attempt
/// to deactivate an account the books depend on, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn account_update(
    state: State<'_, AppState>,
    input: UpdateAccount,
) -> CommandResult<Account> {
    with_connection(&state, move |conn| update_account(conn, &input)).await
}

/// Deactivates an account. Its posted lines stay in the books.
///
/// Requires the unlocked vault. Deactivating an account that is already
/// inactive succeeds.
///
/// # Errors
///
/// Returns `not_found` when the account does not exist,
/// `system_account_protected` for an account the books depend on, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn account_archive(
    state: State<'_, AppState>,
    id: AccountId,
) -> CommandResult<()> {
    with_connection(&state, move |conn| archive_account(conn, id)).await
}

/// Returns the register of one account: its posted lines with a running
/// balance, optionally limited to the dates `from` through `to`.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `not_found` when the account does not exist, `invalid_date` for a
/// bound that is not a date, `money_overflow` when a balance does not fit
/// the money type, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn account_register_cmd(
    state: State<'_, AppState>,
    account_id: AccountId,
    from: Option<DateText>,
    to: Option<DateText>,
) -> CommandResult<Vec<RegisterLine>> {
    with_connection(&state, move |conn| {
        let from = DateText::parse_optional(from.as_ref())?;
        let to = DateText::parse_optional(to.as_ref())?;
        account_register(conn, account_id, from, to)
    })
    .await
}

/// Returns the balance of one account as of a date, in minor units, signed so
/// that the account's normal side is positive.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `not_found` when the account does not exist, `invalid_date` when
/// `as_of` is not a date, `money_overflow` when the balance does not fit the
/// money type, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn account_balance_cmd(
    state: State<'_, AppState>,
    account_id: AccountId,
    as_of: DateText,
) -> CommandResult<i64> {
    with_connection(&state, move |conn| {
        account_balance(conn, account_id, as_of.parse()?)
    })
    .await
}

/// Sets an account's balance as of a date by posting the difference against
/// the book's opening-balances equity account, and returns the posted entry.
///
/// Requires the unlocked vault. The entry's description is written in the
/// app's stored language.
///
/// # Errors
///
/// Returns `not_found` when the account does not exist,
/// `opening_balance_account_type` for an account that is neither an asset
/// nor a liability, `account_inactive` for an inactive account,
/// `invalid_date` when `as_of` is not a date, `opening_balance_unchanged`
/// when the account already has that balance, `no_equity_account` when the
/// book has no equity account to post against, `money_overflow` when an
/// amount does not fit the money type, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn account_set_opening_balance(
    state: State<'_, AppState>,
    account_id: AccountId,
    target_minor: i64,
    as_of: DateText,
) -> CommandResult<PostedEntryView> {
    with_localized_connection(&state, move |conn, locale| {
        set_account_opening_balance(conn, account_id, target_minor, as_of.parse()?, locale)
    })
    .await
}
