//! Recurring entry templates: a saved simple entry with a schedule.
//!
//! A template holds what a [`PostSimpleEntry`] holds except the date, plus a
//! schedule and the date of its next occurrence. All of that is one type,
//! [`RecurringTemplateFields`], which the inputs, the stored row and the view
//! share; its accounts are a [`SimpleEntryAccounts`], so a template cannot
//! hold a kind without the accounts that kind posts to. Nothing is posted in the
//! background. [`post_recurring_template`] is the only way a template becomes
//! a journal entry, and it runs when the user asks; until then a template
//! whose date has come is only reported as due.
//!
//! # Posting
//!
//! A post writes the entry and moves the template's `next_date` in one
//! transaction, so an occurrence is never posted without being counted. The
//! date moves one step of the cadence from the stored `next_date`, not from
//! the date the entry was posted with. Posting late or with another date
//! therefore does not shift the schedule, and an overdue template is caught
//! up one occurrence per post. [`RecurringSchedule`] defines the steps.
//!
//! # Validation
//!
//! Creating or updating a template checks its accounts exactly as posting
//! the entry would. A template that saves can be posted, unless one of its
//! accounts is archived afterwards; the post then reports that account.
//!
//! # Today
//!
//! Whether a template is due is decided against the current date in UTC. The
//! `_as_of` functions take that date from the caller so that tests can fix
//! it.

use crate::db::{collect_rows, corrupt_column, read_column, stored_date, stored_id};
use crate::domain::{AccountId, EntityId, RecurringTemplateId};
use crate::error::{DatabaseContext, Error, NameField, Resource, Result, ValidationError};
use crate::ledger::journals::{
    PostSimpleEntry, PostedEntryView, ensure_simple_entry_accounts, post_simple_entry_unchecked,
};
use crate::ledger::simple_entry::{
    MissingPart, SimpleBillStatus, SimpleEntryAccounts, SimpleEntryKind, SimpleEntryRoleAccounts,
};
use crate::util::{format_date, now_utc_string, utc_today};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use time::{Date, Duration, Month};

/// How often a template recurs, as the UI and the vault name it.
///
/// This is the flat half of a schedule: on the wire and in the
/// `recurring_templates` table a schedule is this name beside an optional
/// day of the month. Inside the crate the pair is one [`RecurringSchedule`],
/// which says what each cadence does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecurringCadence {
    /// Every calendar month, on a day of the month.
    Monthly,
    /// Every 7 days from `next_date`.
    Weekly,
    /// Every calendar year from `next_date`.
    Yearly,
}

impl RecurringCadence {
    /// Returns the cadence as the UI and the vault write it: `monthly`,
    /// `weekly` or `yearly`.
    ///
    /// This is the text serde writes and the text stored in
    /// `recurring_templates.cadence`, so it is part of the vault format.
    #[must_use]
    pub const fn identifier(self) -> &'static str {
        match self {
            Self::Monthly => "monthly",
            Self::Weekly => "weekly",
            Self::Yearly => "yearly",
        }
    }
}

/// A day of the month a monthly template recurs on: 1 to 31.
///
/// A month that is shorter than the day does not clamp it; see
/// [`RecurringSchedule::Monthly`] for where the occurrence lands then.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DayOfMonth(u8);

impl DayOfMonth {
    /// Returns `day` as a day of the month.
    ///
    /// # Errors
    ///
    /// [`ValidationError::DayOfMonthInvalid`] when `day` is outside 1 to 31.
    pub fn new(day: u8) -> Result<Self> {
        if (1..=31).contains(&day) {
            Ok(Self(day))
        } else {
            Err(ValidationError::DayOfMonthInvalid.into())
        }
    }

    /// Returns the day as a number, 1 to 31.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// When a template produces its next occurrence.
///
/// A monthly schedule always has its day and no other schedule has one, so
/// the pair the wire and the vault hold ([`RecurringCadence`] and an optional
/// day) cannot be read into a schedule that means nothing.
/// [`RecurringSchedule::from_cadence`] is the conversion out of that pair,
/// and [`RecurringSchedule::cadence`] and [`RecurringSchedule::day_of_month`]
/// the one back.
///
/// After a successful post, [`advance_next_date`] moves `next_date` by one
/// step of the schedule, as each variant says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecurringSchedule {
    /// Every 7 days. The weekday is implied by `next_date`; there is no
    /// separate weekday.
    Weekly,
    /// Every calendar month: the first occurrence of `day_of_month` strictly
    /// after `next_date`, so the date never moves backwards whatever day
    /// `next_date` is on.
    ///
    /// When the day does not exist in a month (31 in February), the
    /// occurrence overflows to the 1st of that month plus (`day_of_month`
    /// − 1) days: January 31 → March 3 in a non-leap year (February 1 + 30
    /// days). The following post then lands on the next real 31st (March 3 →
    /// March 31).
    Monthly {
        /// The day of the month the template recurs on.
        day_of_month: DayOfMonth,
    },
    /// Every calendar year. February 29 on a non-leap year overflows as a
    /// monthly day does, to March 1.
    Yearly,
}

impl RecurringSchedule {
    /// Builds a schedule from the flat pair the wire and the vault hold.
    ///
    /// # Errors
    ///
    /// [`ValidationError::DayOfMonthInvalid`] for a monthly cadence with no
    /// day or one outside 1 to 31, and for a weekly or yearly cadence with a
    /// day.
    pub fn from_cadence(cadence: RecurringCadence, day_of_month: Option<u8>) -> Result<Self> {
        match (cadence, day_of_month) {
            (RecurringCadence::Monthly, Some(day)) => Ok(Self::Monthly {
                day_of_month: DayOfMonth::new(day)?,
            }),
            (RecurringCadence::Weekly, None) => Ok(Self::Weekly),
            (RecurringCadence::Yearly, None) => Ok(Self::Yearly),
            (RecurringCadence::Monthly, None)
            | (RecurringCadence::Weekly | RecurringCadence::Yearly, Some(_)) => {
                Err(ValidationError::DayOfMonthInvalid.into())
            }
        }
    }

    /// Returns how often the schedule recurs, without its day.
    #[must_use]
    pub const fn cadence(self) -> RecurringCadence {
        match self {
            Self::Weekly => RecurringCadence::Weekly,
            Self::Monthly { .. } => RecurringCadence::Monthly,
            Self::Yearly => RecurringCadence::Yearly,
        }
    }

    /// Returns the day of a monthly schedule, and `None` for any other.
    #[must_use]
    pub const fn day_of_month(self) -> Option<u8> {
        match self {
            Self::Monthly { day_of_month } => Some(day_of_month.get()),
            Self::Weekly | Self::Yearly => None,
        }
    }
}

/// What a recurring template holds: the entry it posts and its schedule.
///
/// The inputs, the stored template and the view all carry this one type.
/// [`create_recurring_template`] and [`update_recurring_template`] check it
/// and store it with the name and the memo trimmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecurringTemplateFields {
    /// Name of the template, which is also the description of each entry it
    /// posts. It must not be blank.
    pub name: String,
    /// Positive amount of each entry, in minor units.
    pub amount_minor: i64,
    /// How often the template recurs, with the day of a monthly one.
    pub schedule: RecurringSchedule,
    /// The kind of entry the template posts and the accounts it debits and
    /// credits.
    pub accounts: SimpleEntryAccounts,
    /// Optional note stored on the template; a blank one is stored as none.
    /// Posting does not read it: a posted entry's description is `name`.
    pub memo: Option<String>,
    /// Date of the next occurrence. Only a successful post advances it.
    pub next_date: Date,
}

/// Input for [`create_recurring_template`].
///
/// The UI sends it as a
/// [`CreateRecurringTemplateRequest`](crate::ledger::CreateRecurringTemplateRequest),
/// which converts into this.
#[derive(Debug, Clone)]
pub struct CreateRecurringTemplate {
    /// Entity the template, and every entry it posts, belongs to.
    pub entity_id: EntityId,
    /// What the new template holds.
    pub fields: RecurringTemplateFields,
}

/// Input for [`update_recurring_template`]. Every field of the template is
/// replaced; its entity cannot be changed.
///
/// The UI sends it as an
/// [`UpdateRecurringTemplateRequest`](crate::ledger::UpdateRecurringTemplateRequest),
/// which converts into this.
#[derive(Debug, Clone)]
pub struct UpdateRecurringTemplate {
    /// Template to change.
    pub id: RecurringTemplateId,
    /// What the template holds from now on.
    pub fields: RecurringTemplateFields,
}

/// A stored template with its due flag, as the Recurring screen shows it.
///
/// It serializes in the flat shape the UI reads: the kind, the bill status
/// and the five role accounts spelled out beside the other fields, and the
/// date as `YYYY-MM-DD`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    into = "crate::ledger::wire::RecurringTemplateViewWire",
    try_from = "crate::ledger::wire::RecurringTemplateViewWire"
)]
pub struct RecurringTemplateView {
    /// Id of the template.
    pub id: RecurringTemplateId,
    /// Entity the template belongs to.
    pub entity_id: EntityId,
    /// What the template holds, as stored.
    pub fields: RecurringTemplateFields,
    /// Whether the next occurrence is on or before the day the view was
    /// built for: today in UTC, unless the caller passed its own date.
    pub due: bool,
}

/// Result of [`post_recurring_template`]: the new journal row plus the
/// template after `next_date` advanced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecurringPostResult {
    /// The journal entry the post created.
    pub entry: PostedEntryView,
    /// Template with the advanced `next_date` and refreshed `due`.
    pub template: RecurringTemplateView,
}

/// Lists templates for an entity, due first (`next_date` ascending), then by
/// name without regard to case.
///
/// # Errors
///
/// [`Error::NotFound`] for an unknown entity; [`Error::VaultCorrupt`] for a
/// stored template that does not parse; database errors as [`Error::Database`].
pub fn list_recurring_templates(
    conn: &Connection,
    entity_id: EntityId,
) -> Result<Vec<RecurringTemplateView>> {
    list_recurring_templates_as_of(conn, entity_id, utc_today())
}

/// Lists templates as [`list_recurring_templates`] does, deciding the due
/// flag against `today` instead of the current date.
///
/// # Errors
///
/// Those of [`list_recurring_templates`].
pub fn list_recurring_templates_as_of(
    conn: &Connection,
    entity_id: EntityId,
    today: Date,
) -> Result<Vec<RecurringTemplateView>> {
    ensure_entity_exists(conn, entity_id)?;

    let mut stmt = conn
        .prepare(
            "
            SELECT id, entity_id, name, kind, amount_minor, cadence, day_of_month,
                   category_account_id, wallet_account_id, payable_account_id,
                   from_account_id, to_account_id, memo, next_date, bill_status
            FROM recurring_templates
            WHERE entity_id = ?1
            ORDER BY next_date ASC, fold(name), name
            ",
        )
        .database("list recurring templates")?;

    let rows = stmt
        .query_map([entity_id.to_string()], |row| Ok(map_template_row(row)))
        .database("list recurring templates")?;

    Ok(collect_rows("list recurring templates", rows)?
        .into_iter()
        .map(|stored| stored.into_view(today))
        .collect())
}

/// Returns one template, with its due flag decided against the current date
/// in UTC.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown template.
/// - [`Error::VaultCorrupt`] for a stored template that does not parse.
/// - [`Error::Database`] on database errors.
pub fn get_recurring_template(
    conn: &Connection,
    id: RecurringTemplateId,
) -> Result<RecurringTemplateView> {
    get_recurring_template_as_of(conn, id, utc_today())
}

/// Returns one template as [`get_recurring_template`] does, deciding the due
/// flag against `today`.
///
/// # Errors
///
/// Those of [`get_recurring_template`].
pub(super) fn get_recurring_template_as_of(
    conn: &Connection,
    id: RecurringTemplateId,
    today: Date,
) -> Result<RecurringTemplateView> {
    Ok(load_template(conn, id)?.into_view(today))
}

/// Creates a template after validating its name, amount and role accounts.
///
/// The schedule needs no check here: a [`RecurringSchedule`] cannot hold a
/// monthly cadence without its day. A request with such a pair is refused
/// when it is converted, with
/// [`ValidationError::DayOfMonthInvalid`].
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown entity or an unknown account.
/// - [`ValidationError::NameRequired`] for an empty name.
/// - [`ValidationError::AmountNotPositive`] for an amount of zero or less.
/// - The account errors of
///   [`post_simple_entry`](crate::ledger::post_simple_entry): an account
///   that does not exist or has the wrong type, entity or state.
/// - [`Error::VaultCorrupt`] for a stored row that does not parse.
/// - [`Error::Database`] on database errors.
pub fn create_recurring_template(
    conn: &Connection,
    input: &CreateRecurringTemplate,
) -> Result<RecurringTemplateView> {
    let fields = validated_fields(conn, input.entity_id, &input.fields)?;
    let roles = fields.accounts.roles();
    let id = RecurringTemplateId::generate();

    conn.execute(
        "
        INSERT INTO recurring_templates (
            id, entity_id, name, kind, amount_minor, cadence, day_of_month,
            category_account_id, wallet_account_id, payable_account_id,
            from_account_id, to_account_id, memo, next_date, bill_status, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
        ",
        rusqlite::params![
            id.to_string(),
            input.entity_id.to_string(),
            fields.name,
            fields.accounts.kind().identifier(),
            fields.amount_minor,
            fields.schedule.cadence().identifier(),
            fields.schedule.day_of_month().map(i64::from),
            account_id_text(roles.category),
            account_id_text(roles.wallet),
            account_id_text(roles.payable),
            account_id_text(roles.from),
            account_id_text(roles.to),
            fields.memo,
            format_date(fields.next_date),
            fields.accounts.bill_status().map(bill_status_str),
            now_utc_string(),
        ],
    )
    .database("insert recurring template")?;

    get_recurring_template(conn, id)
}

/// Replaces every field of a template except its id and its entity.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown template.
/// - The errors of [`create_recurring_template`] for the new values.
pub fn update_recurring_template(
    conn: &Connection,
    input: &UpdateRecurringTemplate,
) -> Result<RecurringTemplateView> {
    let existing = load_template(conn, input.id)?;
    let fields = validated_fields(conn, existing.entity_id, &input.fields)?;
    let roles = fields.accounts.roles();

    let updated = conn
        .execute(
            "
            UPDATE recurring_templates SET
                name = ?1,
                kind = ?2,
                amount_minor = ?3,
                cadence = ?4,
                day_of_month = ?5,
                category_account_id = ?6,
                wallet_account_id = ?7,
                payable_account_id = ?8,
                from_account_id = ?9,
                to_account_id = ?10,
                memo = ?11,
                next_date = ?12,
                bill_status = ?13
            WHERE id = ?14
            ",
            rusqlite::params![
                fields.name,
                fields.accounts.kind().identifier(),
                fields.amount_minor,
                fields.schedule.cadence().identifier(),
                fields.schedule.day_of_month().map(i64::from),
                account_id_text(roles.category),
                account_id_text(roles.wallet),
                account_id_text(roles.payable),
                account_id_text(roles.from),
                account_id_text(roles.to),
                fields.memo,
                format_date(fields.next_date),
                fields.accounts.bill_status().map(bill_status_str),
                input.id.to_string(),
            ],
        )
        .database("update recurring template")?;

    if updated == 0 {
        return Err(Error::NotFound(Resource::RecurringTemplate));
    }

    get_recurring_template(conn, input.id)
}

/// Deletes a template. The entries it has posted are left as they are.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown template.
/// - [`Error::Database`] on database errors.
pub fn delete_recurring_template(conn: &Connection, id: RecurringTemplateId) -> Result<()> {
    let deleted = conn
        .execute(
            "DELETE FROM recurring_templates WHERE id = ?1",
            [id.to_string()],
        )
        .database("delete recurring template")?;
    if deleted == 0 {
        return Err(Error::NotFound(Resource::RecurringTemplate));
    }
    Ok(())
}

/// Posts one journal entry from a template, then advances `next_date`.
///
/// `entry_date` and `amount_minor` default to the template's `next_date` and
/// `amount_minor`. Overrides are for the confirmation sheet of the UI, where
/// the user can adjust either before saving; they do not rewrite the stored
/// template amount or change which
/// occurrence is advanced. Cadence always steps from the stored `next_date`.
///
/// No background auto-post: this is the only way a template creates an entry.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown template.
/// - [`ValidationError::AmountNotPositive`] for an override amount of zero or
///   less.
/// - The errors of [`post_simple_entry`](crate::ledger::post_simple_entry),
///   such as [`ValidationError::AccountInactive`] for an account archived
///   since the template was saved.
/// - The errors of [`advance_next_date`].
/// - [`Error::VaultCorrupt`] for a stored row that does not parse.
/// - [`Error::Database`] on database errors.
///
/// On an error before the commit neither the entry nor the new date is
/// stored. The template is read back after the commit to build the result;
/// if that read fails, the error is returned although both are stored.
pub fn post_recurring_template(
    conn: &Connection,
    id: RecurringTemplateId,
    entry_date: Option<Date>,
    amount_minor: Option<i64>,
) -> Result<RecurringPostResult> {
    let tx = conn
        .unchecked_transaction()
        .database("begin recurring template post")?;

    let stored = load_template(&tx, id)?;
    let template = &stored.fields;
    let post_amount = match amount_minor {
        Some(minor) if minor <= 0 => {
            return Err(ValidationError::AmountNotPositive.into());
        }
        Some(minor) => minor,
        None => template.amount_minor,
    };

    let input = PostSimpleEntry {
        entity_id: stored.entity_id,
        accounts: template.accounts,
        entry_date: entry_date.unwrap_or(template.next_date),
        description: template.name.clone(),
        reference: None,
        amount_minor: post_amount,
    };

    let entry = post_simple_entry_unchecked(&tx, &input)?;
    let advanced = advance_next_date(template.next_date, template.schedule)?;

    tx.execute(
        "UPDATE recurring_templates SET next_date = ?1 WHERE id = ?2",
        rusqlite::params![format_date(advanced), id.to_string()],
    )
    .database("advance recurring template")?;

    tx.commit().database("commit recurring template post")?;

    let template = get_recurring_template(conn, id)?;
    Ok(RecurringPostResult { entry, template })
}

/// Returns the `next_date` that follows `from` under `schedule`. See
/// [`RecurringSchedule`] for each step.
///
/// # Errors
///
/// [`ValidationError::DateOutOfRange`] when the next date is past the last
/// date the calendar holds.
pub fn advance_next_date(from: Date, schedule: RecurringSchedule) -> Result<Date> {
    match schedule {
        RecurringSchedule::Weekly => from
            .checked_add(Duration::days(7))
            .ok_or(ValidationError::DateOutOfRange.into()),
        RecurringSchedule::Yearly => add_calendar_years(from, 1),
        RecurringSchedule::Monthly { day_of_month } => next_monthly(from, day_of_month),
    }
}

/// Returns whether `next_date` is on or before `today`, both calendar dates
/// in UTC.
#[must_use]
pub fn template_is_due(next_date: Date, today: Date) -> bool {
    next_date <= today
}

/// Returns the first occurrence of `day_of_month` strictly after `from`.
///
/// The occurrence in `from`'s own month wins when it is still ahead: that is
/// how an overflow date returns to the real day (March 3 for day 31 steps to
/// March 31). Otherwise the next month's occurrence is taken, which is always
/// after `from` because it falls on or after that month's first day.
///
/// # Errors
///
/// [`ValidationError::DateOutOfRange`] when the occurrence is past the last
/// date the calendar holds.
fn next_monthly(from: Date, day_of_month: DayOfMonth) -> Result<Date> {
    let day_of_month = day_of_month.get();
    let this_month = place_day_or_next(from.year(), from.month(), day_of_month)?;
    if this_month > from {
        return Ok(this_month);
    }

    let (year, month) = add_months(from.year(), from.month(), 1)?;
    place_day_or_next(year, month, day_of_month)
}

/// Returns the year and month that lie `delta` calendar months after
/// `month` of `year`; a negative `delta` goes back.
///
/// # Errors
///
/// [`ValidationError::DateOutOfRange`] when the year does not fit in `i32`.
fn add_months(year: i32, month: Month, delta: i32) -> Result<(i32, Month)> {
    let out_of_range = || Error::from(ValidationError::DateOutOfRange);

    // Months counted from January of year zero, so that adding `delta` and
    // splitting again carries into the year in both directions.
    let month_from_zero = i64::from(u8::from(month)) - 1;
    let months = i64::from(year)
        .checked_mul(12)
        .and_then(|months| months.checked_add(month_from_zero))
        .and_then(|months| months.checked_add(i64::from(delta)))
        .ok_or_else(out_of_range)?;

    let year = i32::try_from(months.div_euclid(12)).map_err(|_| out_of_range())?;
    let month_number = u8::try_from(months.rem_euclid(12) + 1).map_err(|_| out_of_range())?;
    let month = Month::try_from(month_number).map_err(|_| out_of_range())?;

    Ok((year, month))
}

/// Returns the same month and day `years` calendar years after `from`.
///
/// When the day does not exist in that year (February 29), it overflows as
/// [`place_day_or_next`] does.
///
/// # Errors
///
/// [`ValidationError::DateOutOfRange`] when the year is outside the calendar.
fn add_calendar_years(from: Date, years: i32) -> Result<Date> {
    let year = from
        .year()
        .checked_add(years)
        .ok_or(ValidationError::DateOutOfRange)?;
    place_day_or_next(year, from.month(), from.day())
}

/// Returns `day` of `month` in `year`, or, when that month is shorter, its
/// first day plus (`day` − 1) days, which lands in the next month.
///
/// # Errors
///
/// [`ValidationError::DateOutOfRange`] when the date is outside the calendar.
fn place_day_or_next(year: i32, month: Month, day: u8) -> Result<Date> {
    if let Ok(date) = Date::from_calendar_date(year, month, day) {
        return Ok(date);
    }
    let first =
        Date::from_calendar_date(year, month, 1).map_err(|_| ValidationError::DateOutOfRange)?;
    first
        .checked_add(Duration::days(i64::from(day) - 1))
        .ok_or(ValidationError::DateOutOfRange.into())
}

/// A template as stored: every column of `recurring_templates` but
/// `created_at`, parsed.
///
/// It differs from [`RecurringTemplateView`] only in lacking the due flag,
/// which depends on the day it is asked for.
struct StoredTemplate {
    /// Id of the template.
    id: RecurringTemplateId,
    /// Entity the template belongs to.
    entity_id: EntityId,
    /// What the template holds.
    fields: RecurringTemplateFields,
}

impl StoredTemplate {
    /// Adds the due flag, decided against `today`.
    fn into_view(self, today: Date) -> RecurringTemplateView {
        RecurringTemplateView {
            id: self.id,
            entity_id: self.entity_id,
            due: template_is_due(self.fields.next_date, today),
            fields: self.fields,
        }
    }
}

/// Checks the parts of a template that need nothing but the values and that
/// its types do not rule out: its name and its amount.
///
/// The conversion of a request runs this before it reads the schedule and
/// the accounts, and [`validated_fields`] runs it on every template it is
/// given, so the two refuse the same things in the same order.
///
/// # Errors
///
/// - [`ValidationError::NameRequired`] for a name that is blank.
/// - [`ValidationError::AmountNotPositive`] for an amount of zero or less.
pub(crate) fn check_template_values(name: &str, amount_minor: i64) -> Result<()> {
    if name.trim().is_empty() {
        return Err(ValidationError::NameRequired {
            field: NameField::TemplateName,
        }
        .into());
    }
    if amount_minor <= 0 {
        return Err(ValidationError::AmountNotPositive.into());
    }
    Ok(())
}

/// Validates the fields of a template for `entity_id` and returns them as
/// they are stored: the name and the memo trimmed, a blank memo as none.
///
/// The accounts are checked as posting would check them.
///
/// # Errors
///
/// Those of [`create_recurring_template`].
fn validated_fields(
    conn: &Connection,
    entity_id: EntityId,
    fields: &RecurringTemplateFields,
) -> Result<RecurringTemplateFields> {
    ensure_entity_exists(conn, entity_id)?;
    check_template_values(&fields.name, fields.amount_minor)?;
    ensure_simple_entry_accounts(conn, entity_id, fields.accounts)?;

    let memo = fields
        .memo
        .as_deref()
        .map(str::trim)
        .filter(|memo| !memo.is_empty())
        .map(ToOwned::to_owned);

    Ok(RecurringTemplateFields {
        name: fields.name.trim().to_owned(),
        memo,
        ..fields.clone()
    })
}

/// Checks that the entity has a row, archived or not.
///
/// An archived entity passes: its templates can still be listed, created and
/// changed, where [`create_account`](crate::ledger::accounts::create_account)
/// refuses one. The `entities` module doc lists which operation does which.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown entity.
/// - [`Error::Database`] on database errors.
fn ensure_entity_exists(conn: &Connection, entity_id: EntityId) -> Result<()> {
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM entities WHERE id = ?1",
            [entity_id.to_string()],
            |row| row.get(0),
        )
        .database("check entity exists")?;
    if exists == 0 {
        return Err(Error::NotFound(Resource::Entity));
    }
    Ok(())
}

/// Loads one template as stored.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown template.
/// - [`Error::VaultCorrupt`] for a stored template that does not parse.
/// - [`Error::Database`] on database errors.
fn load_template(conn: &Connection, id: RecurringTemplateId) -> Result<StoredTemplate> {
    conn.query_row(
        "
        SELECT id, entity_id, name, kind, amount_minor, cadence, day_of_month,
               category_account_id, wallet_account_id, payable_account_id,
               from_account_id, to_account_id, memo, next_date, bill_status
        FROM recurring_templates
        WHERE id = ?1
        ",
        [id.to_string()],
        |row| Ok(map_template_row(row)),
    )
    .map_err(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => Error::NotFound(Resource::RecurringTemplate),
        other => Error::database("read recurring template", other),
    })?
}

/// Maps a row selected as `id, entity_id, name, kind, amount_minor, cadence,
/// day_of_month`, the five role account ids, then `memo, next_date, bill_status`.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming the column when an id, the kind, the
/// cadence, the day of the month, the date or the bill status does not
/// parse, when the day of a monthly template or an account or the bill
/// status the kind needs is missing, or when a column has the wrong storage
/// class.
fn map_template_row(row: &rusqlite::Row<'_>) -> Result<StoredTemplate> {
    let id = stored_id("recurring_templates.id", &read_column::<String>(row, 0)?)?;
    let entity_id = stored_id(
        "recurring_templates.entity_id",
        &read_column::<String>(row, 1)?,
    )?;
    let kind = parse_kind(&read_column::<String>(row, 3)?)?;
    let cadence = parse_cadence(&read_column::<String>(row, 5)?)?;
    let schedule = stored_schedule(cadence, read_column(row, 6)?)?;
    let next_date = stored_date(
        "recurring_templates.next_date",
        &read_column::<String>(row, 13)?,
    )?;
    let bill_status = read_column::<Option<String>>(row, 14)?
        .map(|text| parse_bill_status(&text))
        .transpose()?;

    let roles = SimpleEntryRoleAccounts {
        category: stored_account(row, 7, "recurring_templates.category_account_id")?,
        wallet: stored_account(row, 8, "recurring_templates.wallet_account_id")?,
        payable: stored_account(row, 9, "recurring_templates.payable_account_id")?,
        from: stored_account(row, 10, "recurring_templates.from_account_id")?,
        to: stored_account(row, 11, "recurring_templates.to_account_id")?,
    };
    // A template is saved only with the accounts its kind posts to, so a row
    // without one of them was not written by the application.
    let accounts = SimpleEntryAccounts::from_roles_or_missing_part(kind, bill_status, roles)
        .map_err(|missing| missing_part_corruption(kind, missing))?;

    Ok(StoredTemplate {
        id,
        entity_id,
        fields: RecurringTemplateFields {
            name: read_column(row, 2)?,
            amount_minor: read_column(row, 4)?,
            schedule,
            accounts,
            memo: read_column(row, 12)?,
            next_date,
        },
    })
}

/// Returns the error for a stored template of `kind` that lacks what the
/// kind needs, naming the column that is empty.
fn missing_part_corruption(kind: SimpleEntryKind, missing: MissingPart) -> Error {
    let kind = kind.identifier();
    let (column, what) = match missing {
        MissingPart::BillStatus => ("bill_status", "bill status"),
        MissingPart::Category(_) => ("category_account_id", "category account"),
        MissingPart::Wallet(_) => ("wallet_account_id", "wallet account"),
        MissingPart::Payable(_) => ("payable_account_id", "payable account"),
        MissingPart::TransferSource(_) => ("from_account_id", "source account"),
        MissingPart::TransferDestination(_) => ("to_account_id", "destination account"),
    };

    corrupt_column(
        &format!("recurring_templates.{column}"),
        format_args!("no {what} for a template of kind {kind}"),
    )
}

/// Builds the schedule of a stored template from its cadence and its
/// `day_of_month` column.
///
/// The column is read for a monthly template only. A day stored beside
/// another cadence is ignored, as an account stored for a part the kind
/// lacks is: nothing is computed from it.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming `recurring_templates.day_of_month` when a
/// monthly template has no day, which the nullable column allows and the
/// application never writes, or a day outside 1..=31, which the schema's
/// `CHECK` refuses.
fn stored_schedule(
    cadence: RecurringCadence,
    stored_day: Option<i64>,
) -> Result<RecurringSchedule> {
    let corrupt = |detail: std::fmt::Arguments<'_>| {
        corrupt_column("recurring_templates.day_of_month", detail)
    };

    match cadence {
        RecurringCadence::Weekly => Ok(RecurringSchedule::Weekly),
        RecurringCadence::Yearly => Ok(RecurringSchedule::Yearly),
        RecurringCadence::Monthly => {
            let stored = stored_day.ok_or_else(|| {
                corrupt(format_args!("no day of the month for a monthly template"))
            })?;
            let day_of_month = u8::try_from(stored)
                .ok()
                .and_then(|day| DayOfMonth::new(day).ok())
                .ok_or_else(|| corrupt(format_args!("not a day of the month: {stored}")))?;

            Ok(RecurringSchedule::Monthly { day_of_month })
        }
    }
}

/// Reads the role account id in column `index` of `row`; `None` when the
/// role is empty. `column` names it in an error.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming `column` when the stored text is not an id
/// or the column has the wrong storage class.
fn stored_account(
    row: &rusqlite::Row<'_>,
    index: usize,
    column: &str,
) -> Result<Option<AccountId>> {
    read_column::<Option<String>>(row, index)?
        .map(|text| stored_id(column, &text))
        .transpose()
}

/// Returns a role account id as the text it is stored as; `None` is stored
/// as `NULL`.
fn account_id_text(id: Option<AccountId>) -> Option<String> {
    id.map(|account| account.to_string())
}

/// Parses the text [`SimpleEntryKind::identifier`] writes.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming `recurring_templates.kind` when `stored` is
/// none of those strings.
fn parse_kind(stored: &str) -> Result<SimpleEntryKind> {
    match stored {
        "expense" => Ok(SimpleEntryKind::Expense),
        "income" => Ok(SimpleEntryKind::Income),
        "bill" => Ok(SimpleEntryKind::Bill),
        "transfer" => Ok(SimpleEntryKind::Transfer),
        other => Err(corrupt_column(
            "recurring_templates.kind",
            format_args!("unknown recurring kind: {other}"),
        )),
    }
}

/// Parses the text [`RecurringCadence::identifier`] writes.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming `recurring_templates.cadence` when `stored`
/// is none of those strings.
fn parse_cadence(stored: &str) -> Result<RecurringCadence> {
    match stored {
        "monthly" => Ok(RecurringCadence::Monthly),
        "weekly" => Ok(RecurringCadence::Weekly),
        "yearly" => Ok(RecurringCadence::Yearly),
        other => Err(corrupt_column(
            "recurring_templates.cadence",
            format_args!("unknown recurring cadence: {other}"),
        )),
    }
}

/// Returns the text `status` is stored as in
/// `recurring_templates.bill_status`.
///
/// The strings are part of the vault format; [`parse_bill_status`] reads
/// them back.
fn bill_status_str(status: SimpleBillStatus) -> &'static str {
    match status {
        SimpleBillStatus::Paid => "paid",
        SimpleBillStatus::Unpaid => "unpaid",
        SimpleBillStatus::PayExisting => "pay_existing",
    }
}

/// Parses the text [`bill_status_str`] writes.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming `recurring_templates.bill_status` when
/// `stored` is none of those strings.
fn parse_bill_status(stored: &str) -> Result<SimpleBillStatus> {
    match stored {
        "paid" => Ok(SimpleBillStatus::Paid),
        "unpaid" => Ok(SimpleBillStatus::Unpaid),
        "pay_existing" => Ok(SimpleBillStatus::PayExisting),
        other => Err(corrupt_column(
            "recurring_templates.bill_status",
            format_args!("unknown recurring bill_status: {other}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::parse_date;

    /// Parses a `YYYY-MM-DD` literal of a test.
    fn date(iso: &str) -> Date {
        parse_date(iso).expect("date")
    }

    /// The schedule of a template that recurs on `day` of every month.
    fn monthly(day: u8) -> RecurringSchedule {
        RecurringSchedule::Monthly {
            day_of_month: DayOfMonth::new(day).expect("a test names a day from 1 to 31"),
        }
    }

    #[test]
    fn weekly_adds_seven_days() {
        let next =
            advance_next_date(date("2026-03-10"), RecurringSchedule::Weekly).expect("weekly");
        assert_eq!(next, date("2026-03-17"));
    }

    #[test]
    fn yearly_adds_one_year() {
        let next =
            advance_next_date(date("2026-03-15"), RecurringSchedule::Yearly).expect("yearly");
        assert_eq!(next, date("2027-03-15"));
    }

    #[test]
    fn yearly_feb_29_advances_to_next_valid_day() {
        let next = advance_next_date(date("2024-02-29"), RecurringSchedule::Yearly).expect("leap");
        assert_eq!(next, date("2025-03-01"));
    }

    #[test]
    fn monthly_day_31_january_overflows_february() {
        let next = advance_next_date(date("2026-01-31"), monthly(31)).expect("jan");
        assert_eq!(
            next,
            date("2026-03-03"),
            "Feb 1 + 30 days in a non-leap year"
        );
    }

    #[test]
    fn monthly_overflow_date_returns_to_day_of_month() {
        let next = advance_next_date(date("2026-03-03"), monthly(31))
            .expect("overflow belongs to February");
        assert_eq!(next, date("2026-03-31"));
    }

    #[test]
    fn monthly_day_31_from_march_overflows_april() {
        let next = advance_next_date(date("2026-03-31"), monthly(31)).expect("mar");
        assert_eq!(next, date("2026-05-01"), "Apr 1 + 30 days");
    }

    #[test]
    fn monthly_day_15_is_next_month() {
        let next = advance_next_date(date("2026-01-15"), monthly(15)).expect("mid");
        assert_eq!(next, date("2026-02-15"));
    }

    #[test]
    fn monthly_leap_year_jan_31_overflows_to_march_2() {
        let next = advance_next_date(date("2024-01-31"), monthly(31)).expect("leap jan");
        assert_eq!(next, date("2024-03-02"), "Feb 1 + 30 days in a leap year");
    }

    #[test]
    fn monthly_from_a_later_day_than_day_of_month_moves_forward() {
        let next = advance_next_date(date("2026-01-20"), monthly(15)).expect("mismatch");
        assert_eq!(next, date("2026-02-15"));
    }

    #[test]
    fn monthly_from_an_earlier_day_than_day_of_month_lands_in_the_same_month() {
        let next = advance_next_date(date("2026-01-10"), monthly(15)).expect("mismatch");
        assert_eq!(next, date("2026-01-15"));
    }

    #[test]
    fn monthly_december_advances_into_january() {
        let advance =
            |from: &str, day: u8| advance_next_date(date(from), monthly(day)).expect("december");
        assert_eq!(advance("2026-12-15", 15), date("2027-01-15"));
        assert_eq!(advance("2026-12-20", 15), date("2027-01-15"));
        assert_eq!(advance("2026-12-31", 31), date("2027-01-31"));
    }

    #[test]
    fn monthly_always_moves_strictly_forward() {
        let mut from = date("2023-12-01");
        let end = date("2025-03-01");
        while from < end {
            for day in 1..=31 {
                let next = advance_next_date(from, monthly(day)).expect("in range");
                assert!(next > from, "from {from} day {day} gave {next}");
            }
            from = from.next_day().expect("in range");
        }
    }

    #[test]
    fn due_is_next_date_on_or_before_today() {
        assert!(template_is_due(date("2026-03-10"), date("2026-03-10")));
        assert!(template_is_due(date("2026-03-09"), date("2026-03-10")));
        assert!(!template_is_due(date("2026-03-11"), date("2026-03-10")));
    }

    #[test]
    fn a_schedule_is_monthly_exactly_when_it_has_a_day() {
        use RecurringCadence::{Monthly, Weekly, Yearly};
        let invalid = Err(Error::Validation(ValidationError::DayOfMonthInvalid));

        assert_eq!(
            RecurringSchedule::from_cadence(Monthly, Some(31)),
            Ok(monthly(31))
        );
        assert_eq!(RecurringSchedule::from_cadence(Monthly, None), invalid);
        assert_eq!(RecurringSchedule::from_cadence(Monthly, Some(0)), invalid);
        assert_eq!(RecurringSchedule::from_cadence(Monthly, Some(32)), invalid);

        for (cadence, schedule) in [
            (Weekly, RecurringSchedule::Weekly),
            (Yearly, RecurringSchedule::Yearly),
        ] {
            assert_eq!(RecurringSchedule::from_cadence(cadence, None), Ok(schedule));
            assert_eq!(RecurringSchedule::from_cadence(cadence, Some(10)), invalid);
        }
    }

    #[test]
    fn a_schedule_gives_back_the_pair_it_was_built_from() {
        for cadence in [
            RecurringCadence::Weekly,
            RecurringCadence::Monthly,
            RecurringCadence::Yearly,
        ] {
            for day in (1..=31).map(Some).chain([None]) {
                let Ok(schedule) = RecurringSchedule::from_cadence(cadence, day) else {
                    continue;
                };
                assert_eq!(
                    (schedule.cadence(), schedule.day_of_month()),
                    (cadence, day)
                );
            }
        }
    }

    #[test]
    fn a_cadence_is_stored_as_the_text_serde_writes() {
        for cadence in [
            RecurringCadence::Weekly,
            RecurringCadence::Monthly,
            RecurringCadence::Yearly,
        ] {
            assert_eq!(
                serde_json::to_value(cadence).unwrap(),
                serde_json::Value::from(cadence.identifier())
            );
            assert_eq!(parse_cadence(cadence.identifier()), Ok(cadence));
        }
    }
}
