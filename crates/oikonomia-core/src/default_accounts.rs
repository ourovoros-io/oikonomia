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
#[must_use]
pub fn default_account_for_role(
    template: ChartTemplate,
    accounts: &[Account],
    role: AccountRole,
) -> Option<AccountId> {
    let account_type = role_account_type(role);

    account_by_codes(accounts, account_type, default_role_codes(template, role))
        .or_else(|| first_of_type(accounts, account_type))
        .map(|account| account.id)
}

/// Default account for every role.
#[must_use]
pub fn default_accounts(template: ChartTemplate, accounts: &[Account]) -> DefaultAccounts {
    let pick = |role| default_account_for_role(template, accounts, role);

    DefaultAccounts {
        category: pick(AccountRole::Category),
        payment: pick(AccountRole::Payment),
        deposit: pick(AccountRole::Deposit),
        income: pick(AccountRole::Income),
        bill_category: pick(AccountRole::BillCategory),
        bills_payable: pick(AccountRole::BillsPayable),
        transfer_source: pick(AccountRole::TransferSource),
        transfer_destination: pick(AccountRole::TransferDestination),
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
    accounts
        .iter()
        .filter(|account| account.is_active && account.account_type == account_type)
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
