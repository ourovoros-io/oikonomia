//! Recurring entry templates: a saved simple entry with a schedule.
//!
//! A template holds what a [`PostSimpleEntry`] holds except the date, plus a
//! cadence and the date of its next occurrence. All of that is one type,
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
//! up one occurrence per post. [`RecurringCadence`] defines the steps.
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

/// How often a template produces the next occurrence.
///
/// After a successful post, [`advance_next_date`] moves `next_date`:
///
/// - **Weekly:** add 7 days. The weekday is implied by `next_date`; there is
///   no separate weekday column.
/// - **Monthly:** the first occurrence of `day_of_month` (1–31) strictly after
///   `next_date`, so the date never moves backwards whatever day `next_date`
///   is on. When the day does not exist in a month (31 in February), the
///   occurrence overflows to the 1st of that month plus (`day_of_month` − 1)
///   days: January 31 → March 3 in a non-leap year (February 1 + 30 days).
///   The following post then lands on the next real 31st (March 3 → March 31).
/// - **Yearly:** add one calendar year. February 29 on a non-leap year
///   overflows the same way (March 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecurringCadence {
    /// Every calendar month on [`RecurringTemplateFields::day_of_month`].
    Monthly,
    /// Every 7 days from `next_date`.
    Weekly,
    /// Every calendar year from `next_date`.
    Yearly,
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
    /// How often the template recurs.
    pub cadence: RecurringCadence,
    /// Day of the month, 1 to 31. Required for [`RecurringCadence::Monthly`]
    /// and forbidden for the other cadences.
    pub day_of_month: Option<u8>,
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

    Ok(collect_rows(rows)?
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

/// Creates a template after validating its name, amount, cadence and role
/// accounts.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown entity or an unknown account.
/// - [`ValidationError::NameRequired`] for an empty name.
/// - [`ValidationError::AmountNotPositive`] for an amount of zero or less.
/// - [`ValidationError::DayOfMonthInvalid`] for a monthly template with no
///   day or one outside 1–31, and for a weekly or yearly template with a day.
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
            cadence_str(fields.cadence),
            fields.day_of_month.map(i64::from),
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
                cadence_str(fields.cadence),
                fields.day_of_month.map(i64::from),
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
    let advanced = advance_next_date(template.next_date, template.cadence, template.day_of_month)?;

    tx.execute(
        "UPDATE recurring_templates SET next_date = ?1 WHERE id = ?2",
        rusqlite::params![format_date(advanced), id.to_string()],
    )
    .database("advance recurring template")?;

    tx.commit().database("commit recurring template post")?;

    let template = get_recurring_template(conn, id)?;
    Ok(RecurringPostResult { entry, template })
}

/// Returns the `next_date` that follows `from` under `cadence`. See
/// [`RecurringCadence`] for each step.
///
/// # Errors
///
/// - [`ValidationError::DayOfMonthInvalid`] for a monthly cadence with no
///   `day_of_month` or one outside 1–31.
/// - [`ValidationError::DateOutOfRange`] when the next date is past the last
///   date the calendar holds.
pub fn advance_next_date(
    from: Date,
    cadence: RecurringCadence,
    day_of_month: Option<u8>,
) -> Result<Date> {
    match cadence {
        RecurringCadence::Weekly => from
            .checked_add(Duration::days(7))
            .ok_or(ValidationError::DateOutOfRange.into()),
        RecurringCadence::Yearly => add_calendar_years(from, 1),
        RecurringCadence::Monthly => {
            let day = require_day_of_month(day_of_month)?;
            next_monthly(from, day)
        }
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
fn next_monthly(from: Date, day_of_month: u8) -> Result<Date> {
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

/// Checks the parts of a template that need nothing but the values: its
/// name, its amount, and its day of the month against its cadence.
///
/// The conversion of a request runs this before it reads the accounts, and
/// [`validated_fields`] runs it on every template it is given, so the two
/// refuse the same things in the same order.
///
/// # Errors
///
/// - [`ValidationError::NameRequired`] for a name that is blank.
/// - [`ValidationError::AmountNotPositive`] for an amount of zero or less.
/// - [`ValidationError::DayOfMonthInvalid`] for a monthly cadence with no
///   day or one outside 1–31, and for a weekly or yearly cadence with a day.
pub(crate) fn check_template_values(
    name: &str,
    amount_minor: i64,
    cadence: RecurringCadence,
    day_of_month: Option<u8>,
) -> Result<()> {
    if name.trim().is_empty() {
        return Err(ValidationError::NameRequired {
            field: NameField::TemplateName,
        }
        .into());
    }
    if amount_minor <= 0 {
        return Err(ValidationError::AmountNotPositive.into());
    }

    match cadence {
        RecurringCadence::Monthly => require_day_of_month(day_of_month).map(|_| ()),
        RecurringCadence::Weekly | RecurringCadence::Yearly => {
            if day_of_month.is_some() {
                return Err(ValidationError::DayOfMonthInvalid.into());
            }
            Ok(())
        }
    }
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
    check_template_values(
        &fields.name,
        fields.amount_minor,
        fields.cadence,
        fields.day_of_month,
    )?;
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

/// Returns the day of the month a monthly cadence needs.
///
/// # Errors
///
/// [`ValidationError::DayOfMonthInvalid`] when `day` is `None` or outside
/// 1–31.
fn require_day_of_month(day: Option<u8>) -> Result<u8> {
    let day = day.ok_or(ValidationError::DayOfMonthInvalid)?;
    if !(1..=31).contains(&day) {
        return Err(ValidationError::DayOfMonthInvalid.into());
    }
    Ok(day)
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
/// parse, when an account or the bill status the kind needs is missing, or
/// when a column has the wrong storage class.
fn map_template_row(row: &rusqlite::Row<'_>) -> Result<StoredTemplate> {
    let id = stored_id("recurring_templates.id", &read_column::<String>(row, 0)?)?;
    let entity_id = stored_id(
        "recurring_templates.entity_id",
        &read_column::<String>(row, 1)?,
    )?;
    let kind = parse_kind(&read_column::<String>(row, 3)?)?;
    let cadence = parse_cadence(&read_column::<String>(row, 5)?)?;
    let day_of_month = read_column::<Option<i64>>(row, 6)?
        .map(stored_day_of_month)
        .transpose()?;
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
            cadence,
            day_of_month,
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

/// Returns a template's day of the month as stored; the schema's `CHECK`
/// keeps it in 1..=31.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming `recurring_templates.day_of_month` when
/// `stored` is outside that range.
fn stored_day_of_month(stored: i64) -> Result<u8> {
    u8::try_from(stored)
        .ok()
        .filter(|day| (1..=31).contains(day))
        .ok_or_else(|| {
            corrupt_column(
                "recurring_templates.day_of_month",
                format_args!("not a day of the month: {stored}"),
            )
        })
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

/// Returns the text `cadence` is stored as in `recurring_templates.cadence`.
///
/// The strings are part of the vault format; [`parse_cadence`] reads them
/// back.
fn cadence_str(cadence: RecurringCadence) -> &'static str {
    match cadence {
        RecurringCadence::Monthly => "monthly",
        RecurringCadence::Weekly => "weekly",
        RecurringCadence::Yearly => "yearly",
    }
}

/// Parses the text [`cadence_str`] writes.
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

    #[test]
    fn weekly_adds_seven_days() {
        let next =
            advance_next_date(date("2026-03-10"), RecurringCadence::Weekly, None).expect("weekly");
        assert_eq!(next, date("2026-03-17"));
    }

    #[test]
    fn yearly_adds_one_year() {
        let next =
            advance_next_date(date("2026-03-15"), RecurringCadence::Yearly, None).expect("yearly");
        assert_eq!(next, date("2027-03-15"));
    }

    #[test]
    fn yearly_feb_29_advances_to_next_valid_day() {
        let next =
            advance_next_date(date("2024-02-29"), RecurringCadence::Yearly, None).expect("leap");
        assert_eq!(next, date("2025-03-01"));
    }

    #[test]
    fn monthly_day_31_january_overflows_february() {
        let next = advance_next_date(date("2026-01-31"), RecurringCadence::Monthly, Some(31))
            .expect("jan");
        assert_eq!(
            next,
            date("2026-03-03"),
            "Feb 1 + 30 days in a non-leap year"
        );
    }

    #[test]
    fn monthly_overflow_date_returns_to_day_of_month() {
        let next = advance_next_date(date("2026-03-03"), RecurringCadence::Monthly, Some(31))
            .expect("overflow belongs to February");
        assert_eq!(next, date("2026-03-31"));
    }

    #[test]
    fn monthly_day_31_from_march_overflows_april() {
        let next = advance_next_date(date("2026-03-31"), RecurringCadence::Monthly, Some(31))
            .expect("mar");
        assert_eq!(next, date("2026-05-01"), "Apr 1 + 30 days");
    }

    #[test]
    fn monthly_day_15_is_next_month() {
        let next = advance_next_date(date("2026-01-15"), RecurringCadence::Monthly, Some(15))
            .expect("mid");
        assert_eq!(next, date("2026-02-15"));
    }

    #[test]
    fn monthly_leap_year_jan_31_overflows_to_march_2() {
        let next = advance_next_date(date("2024-01-31"), RecurringCadence::Monthly, Some(31))
            .expect("leap jan");
        assert_eq!(next, date("2024-03-02"), "Feb 1 + 30 days in a leap year");
    }

    #[test]
    fn monthly_from_a_later_day_than_day_of_month_moves_forward() {
        let next = advance_next_date(date("2026-01-20"), RecurringCadence::Monthly, Some(15))
            .expect("mismatch");
        assert_eq!(next, date("2026-02-15"));
    }

    #[test]
    fn monthly_from_an_earlier_day_than_day_of_month_lands_in_the_same_month() {
        let next = advance_next_date(date("2026-01-10"), RecurringCadence::Monthly, Some(15))
            .expect("mismatch");
        assert_eq!(next, date("2026-01-15"));
    }

    #[test]
    fn monthly_december_advances_into_january() {
        let advance = |from: &str, day: u8| {
            advance_next_date(date(from), RecurringCadence::Monthly, Some(day)).expect("december")
        };
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
                let next = advance_next_date(from, RecurringCadence::Monthly, Some(day))
                    .expect("in range");
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
    fn monthly_requires_day_of_month() {
        let err = advance_next_date(date("2026-01-15"), RecurringCadence::Monthly, None)
            .expect_err("monthly needs day");
        assert_eq!(err, Error::Validation(ValidationError::DayOfMonthInvalid));
    }
}
