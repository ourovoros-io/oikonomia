//! Which account plays which role by default, chosen by identity, never by name.
//!
//! A seeded account is recognised by its template code and its type. Names are
//! free text: the user can rename them and a translated chart gives them in
//! another language, so no choice here may read one. The mapping from role to
//! code lives next to the chart templates in [`crate::coa`].

use rusqlite::Connection;
use serde::Serialize;

use crate::coa::{default_role_codes, role_account_type};
use crate::domain::{Account, AccountId, AccountType, ChartTemplate, EntityId};
use crate::error::{AccountRole, Result};
use crate::ledger::{get_entity, list_accounts};

/// The default account for each role the entry flows need.
///
/// A role is `None` only when the book has no active account of the type the
/// role needs. The field names are the role identifiers the UI already uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DefaultAccounts {
    /// Expense an expense is for.
    pub category: Option<AccountId>,
    /// Asset an expense or bill is paid from.
    pub payment: Option<AccountId>,
    /// Asset an income is received into.
    pub deposit: Option<AccountId>,
    /// Income account an income is booked to.
    pub income: Option<AccountId>,
    /// Expense a bill is for.
    pub bill_category: Option<AccountId>,
    /// Liability that holds an unpaid bill.
    pub bills_payable: Option<AccountId>,
    /// Asset a transfer takes money from.
    pub transfer_source: Option<AccountId>,
    /// Asset a transfer puts money into.
    pub transfer_destination: Option<AccountId>,
}

/// Default account for one role, or `None` if the book has no usable account.
///
/// The seeded accounts the template names for the role are tried in order,
/// each only if it is active and of the role's type. Otherwise the first
/// active account of the role's type, by sort order, is used. That covers a
/// blank book, a seeded account the user deactivated, and one whose code the
/// user changed.
///
/// # Known limitation
///
/// The schema has no account subtype, so the by-type fallback cannot tell a
/// cash account from any other asset. For a wallet role (payment, deposit,
/// transfer) in a blank or heavily edited book it may select a non-cash asset
/// such as Investments. The accounts remembered from the last entry correct
/// that after the first one.
#[must_use]
pub fn default_account_for_role(
    template: ChartTemplate,
    accounts: &[Account],
    role: AccountRole,
) -> Option<AccountId> {
    seeded_account_for_role(template, accounts, role)
        .or_else(|| first_of_type(accounts, role_account_type(role)).map(|account| account.id))
}

/// The seeded account for a role, by identity only.
///
/// This is the first account, in the template's order, that carries one of the
/// role's template codes, is active and has the role's type. Unlike
/// [`default_account_for_role`] there is no by-type fallback, so `None` means
/// the account the template intends for the role is missing, deactivated or
/// re-coded. A blank template seeds nothing, so it always gives `None`.
#[must_use]
pub fn seeded_account_for_role(
    template: ChartTemplate,
    accounts: &[Account],
    role: AccountRole,
) -> Option<AccountId> {
    let account_type = role_account_type(role);

    account_by_codes(accounts, account_type, default_role_codes(template, role))
        .map(|account| account.id)
}

/// Default account for every role.
///
/// A transfer never defaults to the same account on both sides: when the
/// destination would equal the source, it is the first other active asset by
/// sort order, or `None` if there is none.
#[must_use]
pub fn default_accounts(template: ChartTemplate, accounts: &[Account]) -> DefaultAccounts {
    let pick = |role| default_account_for_role(template, accounts, role);
    let transfer_source = pick(AccountRole::TransferSource);
    let mut transfer_destination = pick(AccountRole::TransferDestination);

    if transfer_destination.is_some() && transfer_destination == transfer_source {
        transfer_destination = first_of_type_except(accounts, AccountType::Asset, transfer_source)
            .map(|account| account.id);
    }

    DefaultAccounts {
        category: pick(AccountRole::Category),
        payment: pick(AccountRole::Payment),
        deposit: pick(AccountRole::Deposit),
        income: pick(AccountRole::Income),
        bill_category: pick(AccountRole::BillCategory),
        bills_payable: pick(AccountRole::BillsPayable),
        transfer_source,
        transfer_destination,
    }
}

/// Default account for every role in an entity's book.
///
/// # Errors
///
/// The entity is missing or the database cannot be read.
pub fn default_accounts_for_entity(
    conn: &Connection,
    entity_id: EntityId,
) -> Result<DefaultAccounts> {
    let entity = get_entity(conn, entity_id)?;
    let accounts = list_accounts(conn, entity_id)?;

    Ok(default_accounts(entity.chart_template, &accounts))
}

/// First active account of `account_type` whose code is in `codes`, in the
/// order of `codes`.
pub(crate) fn account_by_codes<'a>(
    accounts: &'a [Account],
    account_type: AccountType,
    codes: &[&str],
) -> Option<&'a Account> {
    codes.iter().find_map(|code| {
        accounts.iter().find(|account| {
            account.is_active && account.account_type == account_type && account.code == *code
        })
    })
}

/// First active account of `account_type` by sort order, then code.
pub(crate) fn first_of_type(accounts: &[Account], account_type: AccountType) -> Option<&Account> {
    first_of_type_except(accounts, account_type, None)
}

/// First active account of `account_type` by sort order, then code, leaving
/// out the account with the `excluded` id.
fn first_of_type_except(
    accounts: &[Account],
    account_type: AccountType,
    excluded: Option<AccountId>,
) -> Option<&Account> {
    accounts
        .iter()
        .filter(|account| {
            account.is_active
                && account.account_type == account_type
                && Some(account.id) != excluded
        })
        .min_by(|left, right| {
            left.sort_order
                .cmp(&right.sort_order)
                .then_with(|| left.code.cmp(&right.code))
        })
}

/// A seeded chart as in-memory `Account` rows.
///
/// With `rename`, every name is replaced by a Greek placeholder that shares no
/// word with the English one, so a selection that still reads names fails.
#[cfg(test)]
pub(crate) fn seeded_chart_for_tests(template: ChartTemplate, rename: bool) -> Vec<Account> {
    let entity_id = EntityId::new();

    crate::coa::template_accounts(template)
        .into_iter()
        .map(|seed| Account {
            id: AccountId::new(),
            entity_id,
            code: seed.code.to_owned(),
            name: if rename {
                format!("Λογαριασμός {}", seed.code)
            } else {
                seed.name.to_owned()
            },
            account_type: seed.account_type,
            parent_id: None,
            is_active: true,
            is_system: seed.is_system,
            sort_order: seed.sort_order,
        })
        .collect()
}

/// The code of the account with `id`, if both are present.
#[cfg(test)]
pub(crate) fn code_of_for_tests(accounts: &[Account], id: Option<AccountId>) -> Option<String> {
    let id = id?;

    accounts
        .iter()
        .find(|account| account.id == id)
        .map(|account| account.code.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Codes of every role's default, in `AccountRole::ALL` order.
    fn role_codes(template: ChartTemplate, accounts: &[Account]) -> Vec<Option<String>> {
        AccountRole::ALL
            .iter()
            .map(|role| {
                let id = default_account_for_role(template, accounts, *role);
                code_of_for_tests(accounts, id)
            })
            .collect()
    }

    fn codes(expected: [&str; 8]) -> Vec<Option<String>> {
        expected
            .iter()
            .map(|code| Some((*code).to_owned()))
            .collect()
    }

    fn deactivate(accounts: &mut [Account], code: &str) {
        for account in accounts {
            if account.code == code {
                account.is_active = false;
            }
        }
    }

    #[test]
    fn english_personal_chart_gets_todays_defaults() {
        let accounts = seeded_chart_for_tests(ChartTemplate::Personal, false);

        // category, payment, deposit, income, bill category, payable, from, to
        assert_eq!(
            role_codes(ChartTemplate::Personal, &accounts),
            codes([
                "5100", "1010", "1010", "4000", "5300", "2050", "1010", "1020"
            ]),
        );
    }

    #[test]
    fn english_company_chart_gets_todays_defaults() {
        let accounts = seeded_chart_for_tests(ChartTemplate::Company, false);

        assert_eq!(
            role_codes(ChartTemplate::Company, &accounts),
            codes([
                "5900", "1010", "1010", "4000", "5200", "2000", "1010", "1000"
            ]),
        );
    }

    #[test]
    fn renaming_every_account_changes_no_default() {
        for template in [ChartTemplate::Personal, ChartTemplate::Company] {
            let english = seeded_chart_for_tests(template, false);
            let renamed = seeded_chart_for_tests(template, true);

            assert_eq!(
                role_codes(template, &renamed),
                role_codes(template, &english),
                "{template:?}",
            );
        }
    }

    #[test]
    fn a_deactivated_seeded_account_yields_to_the_next_code_then_to_the_type() {
        let mut accounts = seeded_chart_for_tests(ChartTemplate::Personal, true);

        deactivate(&mut accounts, "1010");
        assert_eq!(
            code_of_for_tests(
                &accounts,
                default_account_for_role(ChartTemplate::Personal, &accounts, AccountRole::Payment),
            )
            .as_deref(),
            Some("1000"),
            "Cash is the template's second choice for a wallet",
        );

        // With Checking and Cash both gone the first remaining asset by sort
        // order is Savings.
        deactivate(&mut accounts, "1000");
        assert_eq!(
            code_of_for_tests(
                &accounts,
                default_account_for_role(ChartTemplate::Personal, &accounts, AccountRole::Payment),
            )
            .as_deref(),
            Some("1020"),
        );

        // The payable has no second code, so the first liability by sort order
        // (Credit Card) stands in.
        deactivate(&mut accounts, "2050");
        assert_eq!(
            code_of_for_tests(
                &accounts,
                default_account_for_role(
                    ChartTemplate::Personal,
                    &accounts,
                    AccountRole::BillsPayable
                ),
            )
            .as_deref(),
            Some("2000"),
        );
    }

    #[test]
    fn a_blank_book_takes_the_first_account_of_each_type_by_sort_order() {
        let mut accounts = seeded_chart_for_tests(ChartTemplate::Personal, true);
        // None of these carries a seeded code: they are the user's own.
        for account in &mut accounts {
            account.code = format!("U-{}", account.code);
        }
        // Make the sort order differ from the code order for one pair.
        for account in &mut accounts {
            if account.code == "U-1020" {
                account.sort_order = 1;
            }
        }

        assert_eq!(
            role_codes(ChartTemplate::Blank, &accounts),
            codes([
                "U-5000", "U-1020", "U-1020", "U-4000", "U-5000", "U-2000", "U-1020", "U-1020"
            ]),
        );
    }

    #[test]
    fn a_book_with_no_account_of_a_type_returns_none_for_those_roles() {
        let assets_only: Vec<Account> = seeded_chart_for_tests(ChartTemplate::Personal, false)
            .into_iter()
            .filter(|account| account.account_type == AccountType::Asset)
            .collect();
        let defaults = default_accounts(ChartTemplate::Personal, &assets_only);

        assert_eq!(defaults.category, None);
        assert_eq!(defaults.income, None);
        assert_eq!(defaults.bill_category, None);
        assert_eq!(defaults.bills_payable, None);
        assert!(defaults.payment.is_some());
        assert!(defaults.transfer_destination.is_some());

        let none = default_accounts(ChartTemplate::Blank, &[]);
        assert_eq!(none.payment, None);
        assert_eq!(none.transfer_source, None);
    }

    #[test]
    fn a_seeded_code_on_an_account_of_the_wrong_type_is_never_chosen() {
        let mut accounts = seeded_chart_for_tests(ChartTemplate::Personal, false);
        accounts.retain(|account| account.account_type != AccountType::Liability);
        // The user recoded an income account to the Bills Payable code.
        for account in &mut accounts {
            if account.code == "4000" {
                account.code = "2050".to_owned();
            }
        }

        assert_eq!(
            default_account_for_role(
                ChartTemplate::Personal,
                &accounts,
                AccountRole::BillsPayable
            ),
            None,
        );
    }

    /// Rewrites every account's code to a user's own, so the book is blank.
    fn make_blank(accounts: &mut [Account]) {
        for account in accounts {
            account.code = format!("U-{}", account.code);
        }
    }

    fn transfer_codes(
        template: ChartTemplate,
        accounts: &[Account],
    ) -> (Option<String>, Option<String>) {
        let defaults = default_accounts(template, accounts);

        (
            code_of_for_tests(accounts, defaults.transfer_source),
            code_of_for_tests(accounts, defaults.transfer_destination),
        )
    }

    #[test]
    fn a_transfer_never_defaults_to_the_same_account_on_both_sides() {
        let mut accounts = seeded_chart_for_tests(ChartTemplate::Company, true);
        deactivate(&mut accounts, "1010");

        // Cash is now both the source and the template's destination, so the
        // destination is the first other asset by sort order.
        assert_eq!(
            transfer_codes(ChartTemplate::Company, &accounts),
            (Some("1000".to_owned()), Some("1100".to_owned())),
        );
    }

    #[test]
    fn a_blank_book_with_two_assets_transfers_between_them() {
        let mut accounts = seeded_chart_for_tests(ChartTemplate::Personal, true);
        accounts.retain(|account| matches!(account.code.as_str(), "1000" | "1010"));
        make_blank(&mut accounts);

        assert_eq!(
            transfer_codes(ChartTemplate::Blank, &accounts),
            (Some("U-1000".to_owned()), Some("U-1010".to_owned())),
        );
    }

    #[test]
    fn a_blank_book_with_one_asset_has_no_transfer_destination() {
        let mut accounts = seeded_chart_for_tests(ChartTemplate::Personal, true);
        accounts.retain(|account| account.code == "1000");
        make_blank(&mut accounts);

        assert_eq!(
            transfer_codes(ChartTemplate::Blank, &accounts),
            (Some("U-1000".to_owned()), None),
        );
    }

    #[test]
    fn the_seeded_account_for_a_role_has_no_by_type_fallback() {
        let mut accounts = seeded_chart_for_tests(ChartTemplate::Personal, true);

        assert_eq!(
            code_of_for_tests(
                &accounts,
                seeded_account_for_role(
                    ChartTemplate::Personal,
                    &accounts,
                    AccountRole::BillsPayable
                ),
            )
            .as_deref(),
            Some("2050"),
        );

        deactivate(&mut accounts, "2050");
        assert_eq!(
            seeded_account_for_role(
                ChartTemplate::Personal,
                &accounts,
                AccountRole::BillsPayable
            ),
            None,
            "a liability still exists, but it is not the seeded one",
        );
        assert_eq!(
            seeded_account_for_role(ChartTemplate::Blank, &accounts, AccountRole::BillsPayable),
            None,
        );
    }

    #[test]
    fn the_response_uses_the_role_identifiers_and_null_for_a_missing_role() {
        let defaults = default_accounts(ChartTemplate::Blank, &[]);
        let json = serde_json::to_value(defaults).unwrap_or_default();

        for role in AccountRole::ALL {
            assert_eq!(
                json.get(role.identifier()),
                Some(&serde_json::Value::Null),
                "{role}",
            );
        }
    }
}
