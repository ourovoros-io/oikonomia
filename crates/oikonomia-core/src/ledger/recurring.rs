//! Local-only recurring entry templates (encrypted vault, no auto-post).

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use time::{Date, Duration, Month};

use super::journals::{
    PostSimpleEntry, PostedEntryView, SimpleBillStatus, SimpleEntryKind, ensure_simple_entry_roles,
    post_simple_entry_unchecked,
};
use crate::domain::{AccountId, EntityId, RecurringTemplateId};
use crate::error::{Error, Result, ValidationError};
use crate::util::{format_date, now_utc_string, parse_date, parse_uuid, utc_today};

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
    /// Every calendar month on [`CreateRecurringTemplate::day_of_month`].
    Monthly,
    /// Every 7 days from `next_date`.
    Weekly,
    /// Every calendar year from `next_date`.
    Yearly,
}

/// Input for [`create_recurring_template`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRecurringTemplate {
    /// Owning entity.
    pub entity_id: EntityId,
    /// Display name (required).
    pub name: String,
    /// Simple-entry kind (same mapping as [`PostSimpleEntry`]).
    pub kind: SimpleEntryKind,
    /// Required when `kind` is [`SimpleEntryKind::Bill`].
    pub bill_status: Option<SimpleBillStatus>,
    /// Positive amount in minor units.
    pub amount_minor: i64,
    /// Repeat rule.
    pub cadence: RecurringCadence,
    /// Day of month 1–31. Required for [`RecurringCadence::Monthly`]; forbidden
    /// for weekly and yearly.
    pub day_of_month: Option<u8>,
    /// Expense or income category account.
    pub category_account_id: Option<AccountId>,
    /// Bank / cash / card account.
    pub wallet_account_id: Option<AccountId>,
    /// Bills payable / AP liability account.
    pub payable_account_id: Option<AccountId>,
    /// Transfer source.
    pub from_account_id: Option<AccountId>,
    /// Transfer destination.
    pub to_account_id: Option<AccountId>,
    /// Optional memo stored on the template (used as the posted description).
    pub memo: Option<String>,
    /// Next occurrence `YYYY-MM-DD`. Advanced only after a successful post.
    pub next_date: String,
}

/// Input for [`update_recurring_template`]. `entity_id` is immutable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRecurringTemplate {
    /// Template to replace.
    pub id: RecurringTemplateId,
    /// Display name (required).
    pub name: String,
    /// Simple-entry kind.
    pub kind: SimpleEntryKind,
    /// Required when `kind` is [`SimpleEntryKind::Bill`].
    pub bill_status: Option<SimpleBillStatus>,
    /// Positive amount in minor units.
    pub amount_minor: i64,
    /// Repeat rule.
    pub cadence: RecurringCadence,
    /// Day of month 1–31. Required for monthly; forbidden otherwise.
    pub day_of_month: Option<u8>,
    /// Expense or income category account.
    pub category_account_id: Option<AccountId>,
    /// Bank / cash / card account.
    pub wallet_account_id: Option<AccountId>,
    /// Bills payable / AP liability account.
    pub payable_account_id: Option<AccountId>,
    /// Transfer source.
    pub from_account_id: Option<AccountId>,
    /// Transfer destination.
    pub to_account_id: Option<AccountId>,
    /// Optional memo.
    pub memo: Option<String>,
    /// Next occurrence `YYYY-MM-DD`.
    pub next_date: String,
}

/// List/get payload for the Recurring screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecurringTemplateView {
    /// Primary key.
    pub id: RecurringTemplateId,
    /// Owning entity.
    pub entity_id: EntityId,
    /// Display name.
    pub name: String,
    /// Simple-entry kind.
    pub kind: SimpleEntryKind,
    /// Bill payment state when `kind` is bill.
    pub bill_status: Option<SimpleBillStatus>,
    /// Template amount in minor units.
    pub amount_minor: i64,
    /// Repeat rule.
    pub cadence: RecurringCadence,
    /// Day of month when monthly; `null` otherwise.
    pub day_of_month: Option<u8>,
    /// Expense or income category account.
    pub category_account_id: Option<AccountId>,
    /// Bank / cash / card account.
    pub wallet_account_id: Option<AccountId>,
    /// Bills payable / AP liability account.
    pub payable_account_id: Option<AccountId>,
    /// Transfer source.
    pub from_account_id: Option<AccountId>,
    /// Transfer destination.
    pub to_account_id: Option<AccountId>,
    /// Optional memo.
    pub memo: Option<String>,
    /// Next scheduled occurrence.
    #[serde(with = "crate::util::serde_date")]
    pub next_date: Date,
    /// `true` when `next_date` is today or earlier (UTC).
    pub due: bool,
}

/// Result of [`post_recurring_template`]: the new journal row plus the
/// template after `next_date` advanced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecurringPostResult {
    /// Posted journal entry (same shape as `entry_post_simple`).
    pub entry: PostedEntryView,
    /// Template with the advanced `next_date` and refreshed `due`.
    pub template: RecurringTemplateView,
}

/// List templates for an entity, due first (`next_date` ascending).
///
/// # Errors
///
/// Unknown entity or DB errors.
pub fn list_recurring_templates(
    conn: &Connection,
    entity_id: EntityId,
) -> Result<Vec<RecurringTemplateView>> {
    list_recurring_templates_as_of(conn, entity_id, utc_today())
}

/// [`list_recurring_templates`] with an explicit `today` (tests pin due).
///
/// # Errors
///
/// Unknown entity or DB errors.
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
            ORDER BY next_date ASC, name COLLATE NOCASE ASC
            ",
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    let rows = stmt
        .query_map([entity_id.0.to_string()], map_template_row)
        .map_err(|err| Error::Io(err.to_string()))?;

    let mut out = Vec::new();
    for row in rows {
        let stored = row.map_err(|err| Error::Io(err.to_string()))?;
        out.push(stored.into_view(today));
    }
    Ok(out)
}

/// Fetch one template.
///
/// # Errors
///
/// Not found or DB error.
pub fn get_recurring_template(
    conn: &Connection,
    id: RecurringTemplateId,
) -> Result<RecurringTemplateView> {
    get_recurring_template_as_of(conn, id, utc_today())
}

/// [`get_recurring_template`] with an explicit `today`.
///
/// # Errors
///
/// Not found or DB error.
pub fn get_recurring_template_as_of(
    conn: &Connection,
    id: RecurringTemplateId,
    today: Date,
) -> Result<RecurringTemplateView> {
    Ok(load_template(conn, id)?.into_view(today))
}

/// Create a template after validating name, amount, cadence, and role accounts.
///
/// # Errors
///
/// Validation, unknown entity, or DB errors.
pub fn create_recurring_template(
    conn: &Connection,
    input: &CreateRecurringTemplate,
) -> Result<RecurringTemplateView> {
    let fields = validated_fields(conn, input.entity_id, &ValidatedInput::from_create(input))?;
    let id = RecurringTemplateId::new();
    let created = now_utc_string();

    conn.execute(
        "
        INSERT INTO recurring_templates (
            id, entity_id, name, kind, amount_minor, cadence, day_of_month,
            category_account_id, wallet_account_id, payable_account_id,
            from_account_id, to_account_id, memo, next_date, bill_status, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
        ",
        rusqlite::params![
            id.0.to_string(),
            fields.entity_id.0.to_string(),
            fields.name,
            kind_str(fields.kind),
            fields.amount_minor,
            cadence_str(fields.cadence),
            fields.day_of_month.map(i64::from),
            opt_account(fields.category_account_id),
            opt_account(fields.wallet_account_id),
            opt_account(fields.payable_account_id),
            opt_account(fields.from_account_id),
            opt_account(fields.to_account_id),
            fields.memo,
            format_date(fields.next_date),
            fields.bill_status.map(bill_status_str),
            created,
        ],
    )
    .map_err(|err| Error::Io(err.to_string()))?;

    get_recurring_template(conn, id)
}

/// Replace mutable fields on an existing template.
///
/// # Errors
///
/// Not found, validation, or DB errors.
pub fn update_recurring_template(
    conn: &Connection,
    input: &UpdateRecurringTemplate,
) -> Result<RecurringTemplateView> {
    let existing = load_template(conn, input.id)?;
    let fields = validated_fields(
        conn,
        existing.entity_id,
        &ValidatedInput::from_update(input),
    )?;

    let n = conn
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
                kind_str(fields.kind),
                fields.amount_minor,
                cadence_str(fields.cadence),
                fields.day_of_month.map(i64::from),
                opt_account(fields.category_account_id),
                opt_account(fields.wallet_account_id),
                opt_account(fields.payable_account_id),
                opt_account(fields.from_account_id),
                opt_account(fields.to_account_id),
                fields.memo,
                format_date(fields.next_date),
                fields.bill_status.map(bill_status_str),
                input.id.0.to_string(),
            ],
        )
        .map_err(|err| Error::Io(err.to_string()))?;

    if n == 0 {
        return Err(Error::NotFound("recurring template".into()));
    }

    get_recurring_template(conn, input.id)
}

/// Delete a template. Does not void or delete posted journal entries.
///
/// # Errors
///
/// Not found or DB error.
pub fn delete_recurring_template(conn: &Connection, id: RecurringTemplateId) -> Result<()> {
    let n = conn
        .execute(
            "DELETE FROM recurring_templates WHERE id = ?1",
            [id.0.to_string()],
        )
        .map_err(|err| Error::Io(err.to_string()))?;
    if n == 0 {
        return Err(Error::NotFound("recurring template".into()));
    }
    Ok(())
}

/// Post one journal entry from a template, then advance `next_date`.
///
/// `entry_date` and `amount_minor` default to the template's `next_date` and
/// `amount_minor`. Overrides are for the FE confirm sheet (adjust before
/// save); they do not rewrite the stored template amount or change which
/// occurrence is advanced. Cadence always steps from the stored `next_date`.
///
/// No background auto-post: this is the only way a template creates an entry.
///
/// # Errors
///
/// Not found, validation (including a non-positive override amount), posting
/// errors, or DB errors.
pub fn post_recurring_template(
    conn: &Connection,
    id: RecurringTemplateId,
    entry_date: Option<&str>,
    amount_minor: Option<i64>,
) -> Result<RecurringPostResult> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| Error::Io(err.to_string()))?;

    let stored = load_template(&tx, id)?;
    let post_amount = match amount_minor {
        Some(minor) if minor <= 0 => {
            return Err(Error::Validation(ValidationError::AmountNotPositive));
        }
        Some(minor) => minor,
        None => stored.amount_minor,
    };
    let post_date = match entry_date {
        Some(raw) => parse_date(raw)?,
        None => stored.next_date,
    };

    let input = PostSimpleEntry {
        entity_id: stored.entity_id,
        kind: stored.kind,
        bill_status: stored.bill_status,
        entry_date: format_date(post_date),
        description: stored.name.clone(),
        reference: None,
        amount_minor: post_amount,
        category_account_id: stored.category_account_id,
        wallet_account_id: stored.wallet_account_id,
        payable_account_id: stored.payable_account_id,
        from_account_id: stored.from_account_id,
        to_account_id: stored.to_account_id,
    };

    let entry = post_simple_entry_unchecked(&tx, &input)?;
    let advanced = advance_next_date(stored.next_date, stored.cadence, stored.day_of_month)?;

    tx.execute(
        "UPDATE recurring_templates SET next_date = ?1 WHERE id = ?2",
        rusqlite::params![format_date(advanced), id.0.to_string()],
    )
    .map_err(|err| Error::Io(err.to_string()))?;

    tx.commit().map_err(|err| Error::Io(err.to_string()))?;

    let template = get_recurring_template(conn, id)?;
    Ok(RecurringPostResult { entry, template })
}

/// Next `next_date` after a successful post. See [`RecurringCadence`].
///
/// # Errors
///
/// Monthly without `day_of_month`, `day_of_month` outside 1–31, or date overflow.
pub fn advance_next_date(
    from: Date,
    cadence: RecurringCadence,
    day_of_month: Option<u8>,
) -> Result<Date> {
    match cadence {
        RecurringCadence::Weekly => from
            .checked_add(Duration::days(7))
            .ok_or(Error::Validation(ValidationError::DateOutOfRange)),
        RecurringCadence::Yearly => add_calendar_years(from, 1),
        RecurringCadence::Monthly => {
            let day = require_day_of_month(day_of_month)?;
            next_monthly(from, day)
        }
    }
}

/// `true` when `next_date` is on or before `today` (UTC calendar dates).
#[must_use]
pub fn template_is_due(next_date: Date, today: Date) -> bool {
    next_date <= today
}

/// The first occurrence of `day_of_month` strictly after `from`.
///
/// The occurrence in `from`'s own month wins when it is still ahead: that is
/// how an overflow date returns to the real day (March 3 for day 31 steps to
/// March 31). Otherwise the next month's occurrence is taken, which is always
/// after `from` because it falls on or after that month's first day.
fn next_monthly(from: Date, day_of_month: u8) -> Result<Date> {
    let this_month = place_day_or_next(from.year(), from.month(), day_of_month)?;
    if this_month > from {
        return Ok(this_month);
    }

    let (year, month) = add_months(from.year(), from.month(), 1)?;
    place_day_or_next(year, month, day_of_month)
}

fn add_months(year: i32, month: Month, delta: i32) -> Result<(i32, Month)> {
    let year_i = i64::from(year);
    let month0 = i64::from(u8::from(month)) - 1;
    let total = year_i
        .checked_mul(12)
        .and_then(|months| months.checked_add(month0))
        .and_then(|months| months.checked_add(i64::from(delta)))
        .ok_or(Error::Validation(ValidationError::DateOutOfRange))?;
    let new_year = i32::try_from(total.div_euclid(12))
        .map_err(|_| Error::Validation(ValidationError::DateOutOfRange))?;
    let month_num = u8::try_from(total.rem_euclid(12) + 1)
        .map_err(|_| Error::Validation(ValidationError::DateOutOfRange))?;
    let new_month = Month::try_from(month_num)
        .map_err(|_| Error::Validation(ValidationError::DateOutOfRange))?;
    Ok((new_year, new_month))
}

fn add_calendar_years(from: Date, years: i32) -> Result<Date> {
    let year = from
        .year()
        .checked_add(years)
        .ok_or(Error::Validation(ValidationError::DateOutOfRange))?;
    place_day_or_next(year, from.month(), from.day())
}

/// Place `day` in `year`/`month`, or overflow: 1st + (`day` − 1) days.
fn place_day_or_next(year: i32, month: Month, day: u8) -> Result<Date> {
    if let Ok(date) = Date::from_calendar_date(year, month, day) {
        return Ok(date);
    }
    let first = Date::from_calendar_date(year, month, 1)
        .map_err(|_| Error::Validation(ValidationError::DateOutOfRange))?;
    first
        .checked_add(Duration::days(i64::from(day) - 1))
        .ok_or(Error::Validation(ValidationError::DateOutOfRange))
}

struct StoredTemplate {
    id: RecurringTemplateId,
    entity_id: EntityId,
    name: String,
    kind: SimpleEntryKind,
    bill_status: Option<SimpleBillStatus>,
    amount_minor: i64,
    cadence: RecurringCadence,
    day_of_month: Option<u8>,
    category_account_id: Option<AccountId>,
    wallet_account_id: Option<AccountId>,
    payable_account_id: Option<AccountId>,
    from_account_id: Option<AccountId>,
    to_account_id: Option<AccountId>,
    memo: Option<String>,
    next_date: Date,
}

impl StoredTemplate {
    fn into_view(self, today: Date) -> RecurringTemplateView {
        let due = template_is_due(self.next_date, today);
        RecurringTemplateView {
            id: self.id,
            entity_id: self.entity_id,
            name: self.name,
            kind: self.kind,
            bill_status: self.bill_status,
            amount_minor: self.amount_minor,
            cadence: self.cadence,
            day_of_month: self.day_of_month,
            category_account_id: self.category_account_id,
            wallet_account_id: self.wallet_account_id,
            payable_account_id: self.payable_account_id,
            from_account_id: self.from_account_id,
            to_account_id: self.to_account_id,
            memo: self.memo,
            next_date: self.next_date,
            due,
        }
    }
}

struct ValidatedInput<'a> {
    name: &'a str,
    kind: SimpleEntryKind,
    bill_status: Option<SimpleBillStatus>,
    amount_minor: i64,
    cadence: RecurringCadence,
    day_of_month: Option<u8>,
    category_account_id: Option<AccountId>,
    wallet_account_id: Option<AccountId>,
    payable_account_id: Option<AccountId>,
    from_account_id: Option<AccountId>,
    to_account_id: Option<AccountId>,
    memo: Option<&'a str>,
    next_date: &'a str,
}

impl<'a> ValidatedInput<'a> {
    fn from_create(input: &'a CreateRecurringTemplate) -> Self {
        Self {
            name: &input.name,
            kind: input.kind,
            bill_status: input.bill_status,
            amount_minor: input.amount_minor,
            cadence: input.cadence,
            day_of_month: input.day_of_month,
            category_account_id: input.category_account_id,
            wallet_account_id: input.wallet_account_id,
            payable_account_id: input.payable_account_id,
            from_account_id: input.from_account_id,
            to_account_id: input.to_account_id,
            memo: input.memo.as_deref(),
            next_date: &input.next_date,
        }
    }

    fn from_update(input: &'a UpdateRecurringTemplate) -> Self {
        Self {
            name: &input.name,
            kind: input.kind,
            bill_status: input.bill_status,
            amount_minor: input.amount_minor,
            cadence: input.cadence,
            day_of_month: input.day_of_month,
            category_account_id: input.category_account_id,
            wallet_account_id: input.wallet_account_id,
            payable_account_id: input.payable_account_id,
            from_account_id: input.from_account_id,
            to_account_id: input.to_account_id,
            memo: input.memo.as_deref(),
            next_date: &input.next_date,
        }
    }
}

struct ValidatedFields {
    entity_id: EntityId,
    name: String,
    kind: SimpleEntryKind,
    bill_status: Option<SimpleBillStatus>,
    amount_minor: i64,
    cadence: RecurringCadence,
    day_of_month: Option<u8>,
    category_account_id: Option<AccountId>,
    wallet_account_id: Option<AccountId>,
    payable_account_id: Option<AccountId>,
    from_account_id: Option<AccountId>,
    to_account_id: Option<AccountId>,
    memo: Option<String>,
    next_date: Date,
}

fn validated_fields(
    conn: &Connection,
    entity_id: EntityId,
    input: &ValidatedInput<'_>,
) -> Result<ValidatedFields> {
    ensure_entity_exists(conn, entity_id)?;

    let name = input.name.trim();
    if name.is_empty() {
        return Err(Error::Validation(ValidationError::NameRequired {
            field: "template name",
        }));
    }
    if input.amount_minor <= 0 {
        return Err(Error::Validation(ValidationError::AmountNotPositive));
    }

    let day_of_month = match input.cadence {
        RecurringCadence::Monthly => Some(require_day_of_month(input.day_of_month)?),
        RecurringCadence::Weekly | RecurringCadence::Yearly => {
            if input.day_of_month.is_some() {
                return Err(Error::Validation(ValidationError::DayOfMonthInvalid));
            }
            None
        }
    };

    let next_date = parse_date(input.next_date)?;
    let memo = input
        .memo
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);

    let fields = ValidatedFields {
        entity_id,
        name: name.to_owned(),
        kind: input.kind,
        bill_status: input.bill_status,
        amount_minor: input.amount_minor,
        cadence: input.cadence,
        day_of_month,
        category_account_id: input.category_account_id,
        wallet_account_id: input.wallet_account_id,
        payable_account_id: input.payable_account_id,
        from_account_id: input.from_account_id,
        to_account_id: input.to_account_id,
        memo,
        next_date,
    };

    let probe = PostSimpleEntry {
        entity_id,
        kind: fields.kind,
        bill_status: fields.bill_status,
        entry_date: format_date(fields.next_date),
        description: fields.name.clone(),
        reference: None,
        amount_minor: fields.amount_minor,
        category_account_id: fields.category_account_id,
        wallet_account_id: fields.wallet_account_id,
        payable_account_id: fields.payable_account_id,
        from_account_id: fields.from_account_id,
        to_account_id: fields.to_account_id,
    };
    ensure_simple_entry_roles(conn, &probe)?;

    Ok(fields)
}

fn require_day_of_month(day: Option<u8>) -> Result<u8> {
    let day = day.ok_or(Error::Validation(ValidationError::DayOfMonthInvalid))?;
    if !(1..=31).contains(&day) {
        return Err(Error::Validation(ValidationError::DayOfMonthInvalid));
    }
    Ok(day)
}

fn ensure_entity_exists(conn: &Connection, entity_id: EntityId) -> Result<()> {
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM entities WHERE id = ?1",
            [entity_id.0.to_string()],
            |row| row.get(0),
        )
        .map_err(|err| Error::Io(err.to_string()))?;
    if exists == 0 {
        return Err(Error::NotFound("entity".into()));
    }
    Ok(())
}

fn load_template(conn: &Connection, id: RecurringTemplateId) -> Result<StoredTemplate> {
    conn.query_row(
        "
        SELECT id, entity_id, name, kind, amount_minor, cadence, day_of_month,
               category_account_id, wallet_account_id, payable_account_id,
               from_account_id, to_account_id, memo, next_date, bill_status
        FROM recurring_templates
        WHERE id = ?1
        ",
        [id.0.to_string()],
        map_template_row,
    )
    .map_err(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => Error::NotFound("recurring template".into()),
        other => Error::Io(other.to_string()),
    })
}

fn map_template_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredTemplate> {
    let id = RecurringTemplateId(
        parse_uuid(&row.get::<_, String>(0)?).map_err(|e| sql_conversion_error(0, &e))?,
    );
    let entity_id =
        EntityId(parse_uuid(&row.get::<_, String>(1)?).map_err(|e| sql_conversion_error(1, &e))?);
    let kind = parse_kind(&row.get::<_, String>(3)?).map_err(|e| sql_conversion_error(3, &e))?;
    let cadence =
        parse_cadence(&row.get::<_, String>(5)?).map_err(|e| sql_conversion_error(5, &e))?;
    let day_raw: Option<i64> = row.get(6)?;
    let day_of_month = day_raw
        .map(|n| u8::try_from(n).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(6, n)))
        .transpose()?;
    let next_date =
        parse_date(&row.get::<_, String>(13)?).map_err(|e| sql_conversion_error(13, &e))?;
    let bill_raw: Option<String> = row.get(14)?;
    let bill_status = bill_raw
        .map(|s| parse_bill_status(&s).map_err(|e| sql_conversion_error(14, &e)))
        .transpose()?;

    Ok(StoredTemplate {
        id,
        entity_id,
        name: row.get(2)?,
        kind,
        bill_status,
        amount_minor: row.get(4)?,
        cadence,
        day_of_month,
        category_account_id: opt_account_from_row(row, 7)?,
        wallet_account_id: opt_account_from_row(row, 8)?,
        payable_account_id: opt_account_from_row(row, 9)?,
        from_account_id: opt_account_from_row(row, 10)?,
        to_account_id: opt_account_from_row(row, 11)?,
        memo: row.get(12)?,
        next_date,
    })
}

fn opt_account_from_row(
    row: &rusqlite::Row<'_>,
    idx: usize,
) -> rusqlite::Result<Option<AccountId>> {
    let raw: Option<String> = row.get(idx)?;
    match raw {
        None => Ok(None),
        Some(s) => parse_uuid(&s)
            .map(|id| Some(AccountId(id)))
            .map_err(|e| sql_conversion_error(idx, &e)),
    }
}

fn sql_conversion_error(col: usize, err: &Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        col,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            err.to_string(),
        )),
    )
}

fn opt_account(id: Option<AccountId>) -> Option<String> {
    id.map(|account| account.0.to_string())
}

fn kind_str(kind: SimpleEntryKind) -> &'static str {
    match kind {
        SimpleEntryKind::Expense => "expense",
        SimpleEntryKind::Income => "income",
        SimpleEntryKind::Bill => "bill",
        SimpleEntryKind::Transfer => "transfer",
    }
}

fn parse_kind(s: &str) -> Result<SimpleEntryKind> {
    match s {
        "expense" => Ok(SimpleEntryKind::Expense),
        "income" => Ok(SimpleEntryKind::Income),
        "bill" => Ok(SimpleEntryKind::Bill),
        "transfer" => Ok(SimpleEntryKind::Transfer),
        other => Err(Error::VaultCorrupt(format!(
            "unknown recurring kind: {other}"
        ))),
    }
}

fn cadence_str(cadence: RecurringCadence) -> &'static str {
    match cadence {
        RecurringCadence::Monthly => "monthly",
        RecurringCadence::Weekly => "weekly",
        RecurringCadence::Yearly => "yearly",
    }
}

fn parse_cadence(s: &str) -> Result<RecurringCadence> {
    match s {
        "monthly" => Ok(RecurringCadence::Monthly),
        "weekly" => Ok(RecurringCadence::Weekly),
        "yearly" => Ok(RecurringCadence::Yearly),
        other => Err(Error::VaultCorrupt(format!(
            "unknown recurring cadence: {other}"
        ))),
    }
}

fn bill_status_str(status: SimpleBillStatus) -> &'static str {
    match status {
        SimpleBillStatus::Paid => "paid",
        SimpleBillStatus::Unpaid => "unpaid",
        SimpleBillStatus::PayExisting => "pay_existing",
    }
}

fn parse_bill_status(s: &str) -> Result<SimpleBillStatus> {
    match s {
        "paid" => Ok(SimpleBillStatus::Paid),
        "unpaid" => Ok(SimpleBillStatus::Unpaid),
        "pay_existing" => Ok(SimpleBillStatus::PayExisting),
        other => Err(Error::VaultCorrupt(format!(
            "unknown recurring bill_status: {other}"
        ))),
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use super::*;

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
