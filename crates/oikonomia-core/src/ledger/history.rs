//! The trail a correction leaves behind, read back for one entry.
//!
//! Correcting an entry ([`replace_simple_entry`](crate::ledger::replace_simple_entry))
//! voids the original, which posts a reversal, and posts a replacement. The
//! register hides all but the replacement, so without this module the copy
//! core wrote for the audit trail (the original and the `VOID:` reversal)
//! is in the vault but nowhere to be seen.
//!
//! # Links
//!
//! A void links the original and its reversal in both directions through
//! `voided_by_entry_id`. The replacement is tied to the entry it took the
//! place of by `replaces_entry_id`, which only a correction writes. Walking
//! `replaces_entry_id` back from an entry therefore finds every earlier
//! version, and each version's `voided_by_entry_id` finds its reversal. The
//! reversal is read from the version that was replaced, never guessed from
//! the pair, so the two sides of a void are not confused.
//!
//! An entry corrected before the column existed has no `replaces_entry_id`,
//! and its history is empty.

use crate::db::{corrupt_column, read_column, stored_date, stored_id};
use crate::domain::JournalEntryId;
use crate::error::{Error, Resource, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use time::Date;

/// The operation named in errors raised while reading a history.
const OPERATION: &str = "read entry history";

/// How an entry in a history relates to the correction that replaced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryChange {
    /// The first version of the entry, as it was first posted.
    Original,
    /// The entry core posted to cancel a version that was replaced.
    Reversal,
    /// A later version that was itself replaced afterwards.
    Replacement,
}

/// One entry of a history: what core stored for an earlier version of an
/// entry, or for the reversal that cancelled one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryHistoryItem {
    /// The stored entry.
    pub entry_id: JournalEntryId,
    /// Its accounting date, serialized as `YYYY-MM-DD`.
    #[serde(with = "crate::util::serde_date")]
    pub entry_date: Date,
    /// Its stored description. For a reversal this is the text core wrote, in
    /// the language the app was set to when the correction was made.
    pub description: String,
    /// How it relates to the correction.
    pub change: EntryChange,
}

/// What the walk needs to know about one stored entry.
struct Node {
    id: JournalEntryId,
    entry_date: Date,
    description: String,
    replaces: Option<JournalEntryId>,
    voided_by: Option<JournalEntryId>,
}

impl Node {
    fn into_item(self, change: EntryChange) -> EntryHistoryItem {
        EntryHistoryItem {
            entry_id: self.id,
            entry_date: self.entry_date,
            description: self.description,
            change,
        }
    }
}

/// Lists the earlier versions of an entry and the reversals that cancelled
/// them, oldest first.
///
/// For an entry that was corrected once the list is the original and its
/// reversal. For one corrected again it continues with the replacement and
/// its reversal. An entry that was never a replacement has an empty history.
/// The entry itself is not listed.
///
/// # Errors
///
/// - [`Error::NotFound`] for an unknown entry.
/// - [`Error::VaultCorrupt`] for a stored id or date that does not parse, or
///   for links that form a loop.
/// - [`Error::Database`] on database errors.
pub fn entry_history(conn: &Connection, id: JournalEntryId) -> Result<Vec<EntryHistoryItem>> {
    let mut versions = earlier_versions(conn, id)?;
    versions.reverse();

    let mut history = Vec::new();
    for (index, version) in versions.into_iter().enumerate() {
        let reversal = version.voided_by;
        let change = if index == 0 {
            EntryChange::Original
        } else {
            EntryChange::Replacement
        };
        history.push(version.into_item(change));

        if let Some(reversal_id) = reversal {
            history.push(load_node(conn, reversal_id)?.into_item(EntryChange::Reversal));
        }
    }

    Ok(history)
}

/// Follows `replaces_entry_id` back from `id`, newest version first.
fn earlier_versions(conn: &Connection, id: JournalEntryId) -> Result<Vec<Node>> {
    let mut versions = Vec::new();
    let mut seen = HashSet::from([id]);
    let mut next = load_node(conn, id)?.replaces;

    while let Some(previous_id) = next {
        if !seen.insert(previous_id) {
            return Err(corrupt_column(
                "journal_entries.replaces_entry_id",
                format_args!("replacement links loop at {previous_id}"),
            ));
        }
        let previous = load_node(conn, previous_id)?;
        next = previous.replaces;
        versions.push(previous);
    }

    Ok(versions)
}

/// Reads the columns of one entry that a history needs.
fn load_node(conn: &Connection, id: JournalEntryId) -> Result<Node> {
    conn.query_row(
        "
        SELECT entry_date, description, replaces_entry_id, voided_by_entry_id
        FROM journal_entries WHERE id = ?1
        ",
        [id.to_string()],
        |row| Ok(map_node(id, row)),
    )
    .map_err(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => Error::NotFound(Resource::JournalEntry),
        other => Error::database(OPERATION, other),
    })?
}

/// Maps a row selected as `entry_date, description, replaces_entry_id,
/// voided_by_entry_id`.
///
/// # Errors
///
/// [`Error::VaultCorrupt`] naming the column when a date or an id does not
/// parse or a column has the wrong storage class.
fn map_node(id: JournalEntryId, row: &rusqlite::Row<'_>) -> Result<Node> {
    let optional_id = |index, column| -> Result<Option<JournalEntryId>> {
        read_column::<Option<String>>(OPERATION, row, index)?
            .map(|text| stored_id(column, &text))
            .transpose()
    };

    Ok(Node {
        id,
        entry_date: stored_date(
            "journal_entries.entry_date",
            &read_column::<String>(OPERATION, row, 0)?,
        )?,
        description: read_column(OPERATION, row, 1)?,
        replaces: optional_id(2, "journal_entries.replaces_entry_id")?,
        voided_by: optional_id(3, "journal_entries.voided_by_entry_id")?,
    })
}
