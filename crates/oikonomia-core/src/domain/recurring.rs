//! Recurring entry template identity.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable identifier for a local recurring entry template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RecurringTemplateId(pub Uuid);

impl RecurringTemplateId {
    /// Generate a new random template id.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for RecurringTemplateId {
    fn default() -> Self {
        Self::new()
    }
}
