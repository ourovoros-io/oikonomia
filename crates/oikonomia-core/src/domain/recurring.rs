//! The id of a recurring entry template.
//!
//! Only the id lives in the domain layer. The template itself (its cadence,
//! amount, accounts and next date) is defined beside the code that stores and
//! posts it, in [`crate::ledger`], because validating one needs the database:
//! its accounts must exist in the book and pass the checks posting applies.
//! The id is here so that it sits with the other id types.

use crate::domain::define_id;

define_id! {
    /// Identifies one recurring entry template.
    RecurringTemplateId
}
