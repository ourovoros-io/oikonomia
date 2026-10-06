//! The kinds of record a lookup can fail to find.

use std::fmt;

/// The kind of record [`Error::NotFound`](crate::Error::NotFound) is about.
///
/// It is sent to the UI as the `resource` parameter, by
/// [`Resource::identifier`], so the copy can name what is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    /// An entity: one book.
    Entity,
    /// An account of a book.
    Account,
    /// A journal entry.
    JournalEntry,
    /// A stored document.
    Document,
    /// A recurring template.
    RecurringTemplate,
}

impl Resource {
    /// Every kind of record, in declaration order.
    pub const ALL: &'static [Self] = &[
        Self::Entity,
        Self::Account,
        Self::JournalEntry,
        Self::Document,
        Self::RecurringTemplate,
    ];

    /// Returns the stable `snake_case` identifier sent as the `resource`
    /// parameter.
    #[must_use]
    pub fn identifier(self) -> &'static str {
        match self {
            Self::Entity => "entity",
            Self::Account => "account",
            Self::JournalEntry => "journal_entry",
            Self::Document => "document",
            Self::RecurringTemplate => "recurring_template",
        }
    }
}

impl fmt::Display for Resource {
    /// Writes the identifier with spaces for underscores, which reads as
    /// English in logs.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.identifier().replace('_', " "))
    }
}

#[cfg(test)]
mod tests {
    use super::Resource;
    use oikonomia_test_support::listed_variants;
    use std::collections::BTreeSet;

    listed_variants! {
        units listed_resources for Resource {
            Resource::Entity,
            Resource::Account,
            Resource::JournalEntry,
            Resource::Document,
            Resource::RecurringTemplate,
        }
    }

    /// Fails unless `Resource::ALL` is exactly the listed variants, each once.
    /// The compiler checks the list above against the enum with an exhaustive
    /// `match`, so a variant left out of it does not compile.
    #[test]
    fn all_lists_every_resource_once() {
        assert_eq!(Resource::ALL.len(), listed_resources::COUNT);
        listed_resources::assert_every_position_once(
            Resource::ALL
                .iter()
                .map(listed_resources::position)
                .collect(),
        );
    }

    #[test]
    fn every_resource_has_a_distinct_snake_case_identifier() {
        let identifiers: BTreeSet<&str> = Resource::ALL
            .iter()
            .map(|resource| resource.identifier())
            .collect();

        assert_eq!(identifiers.len(), Resource::ALL.len());
        for identifier in identifiers {
            assert!(
                identifier
                    .chars()
                    .all(|letter| letter.is_ascii_lowercase() || letter == '_'),
                "{identifier}"
            );
        }
    }

    #[test]
    fn a_resource_displays_as_words() {
        assert_eq!(Resource::JournalEntry.to_string(), "journal entry");
    }
}
