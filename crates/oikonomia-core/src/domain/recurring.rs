//! The id of a recurring entry template.
//!
//! Only the id lives in the domain layer. The template itself (its cadence,
//! amount, accounts and next date) is defined beside the code that stores and
//! posts it, in [`crate::ledger`], because validating one needs the database:
//! its accounts must exist in the book and pass the checks posting applies.
//! The id is here so that it sits with the other id types.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identifies one recurring entry template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RecurringTemplateId(pub Uuid);

impl RecurringTemplateId {
    /// Returns a new random (version 4) id.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for RecurringTemplateId {
    /// Returns a new random id, the same as [`RecurringTemplateId::new`], not a
    /// fixed value.
    fn default() -> Self {
        Self::new()
    }
}
