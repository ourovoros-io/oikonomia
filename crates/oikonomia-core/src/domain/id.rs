//! The one definition every record id type is stamped from.
//!
//! Each kind of record has its own id type, so an account id cannot be passed
//! where an entry id is expected. The types differ in name only, and
//! [`define_id`] writes all of them, so they cannot drift apart.
//!
//! # What an id type is
//!
//! - A wrapper around a random (version 4) UUID whose field is private. The
//!   ways in are `generate` for a new record, [`FromStr`](std::str::FromStr)
//!   for text from outside, and `From<Uuid>` for a UUID already in hand; the
//!   way out is `as_uuid` or [`Display`](std::fmt::Display).
//! - Written as the hyphenated lowercase UUID by `Display`, which is the text
//!   the database stores. Serde writes the same text, because the type is
//!   `#[serde(transparent)]` over the UUID.
//! - Without a `Default`. A default id would have to be a fresh random one,
//!   and a struct that derived `Default` around it would mint an id nobody
//!   asked for. A new id is always the result of a visible `generate` call.
//!
//! # Text from outside and text from the vault
//!
//! `FromStr` reports text that is not a UUID as
//! [`ValidationError::Internal`](crate::error::ValidationError::Internal):
//! ids are produced by the application and never typed by the user, so a bad
//! one from a caller is a caller bug. An id read back out of a stored row
//! does not go through `FromStr`. It goes through `crate::db::stored_id`,
//! which names the column and reports
//! [`Error::VaultCorrupt`](crate::error::Error::VaultCorrupt), because there
//! the bad text was written by the application itself.
//!
//! # Examples
//!
//! ```
//! use oikonomia_core::domain::AccountId;
//!
//! let id: AccountId = "22222222-2222-4222-8222-222222222222".parse()?;
//! assert_eq!(id.to_string(), "22222222-2222-4222-8222-222222222222");
//! assert_eq!(serde_json::to_string(&id)?, r#""22222222-2222-4222-8222-222222222222""#);
//!
//! assert_ne!(AccountId::generate(), AccountId::generate());
//! let refused = "not an id".parse::<AccountId>().map_err(|error| error.code());
//! assert_eq!(refused, Err("validation_internal"));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

/// Defines a record id type: a private-field wrapper around a UUID.
///
/// The attributes given before the name, normally the type's documentation,
/// are placed on the type. See the [module documentation](self) for what
/// every id type has and why it has no `Default`.
macro_rules! define_id {
    ($(#[$attribute:meta])* $name:ident) => {
        $(#[$attribute])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(::uuid::Uuid);

        impl $name {
            /// Returns a new random (version 4) id.
            #[must_use]
            pub fn generate() -> Self {
                Self(::uuid::Uuid::new_v4())
            }

            /// Returns the UUID this id wraps.
            #[must_use]
            pub const fn as_uuid(&self) -> &::uuid::Uuid {
                &self.0
            }
        }

        impl ::std::fmt::Display for $name {
            /// Writes the id as a hyphenated lowercase UUID, the form the
            /// database stores.
            fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                ::std::fmt::Display::fmt(self.0.as_hyphenated(), formatter)
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $crate::error::Error;

            /// Parses an id from any text form `Uuid::parse_str` reads.
            ///
            /// # Errors
            ///
            /// Returns [`Error::Validation`]($crate::error::Error::Validation)
            /// with
            /// [`ValidationError::Internal`]($crate::error::ValidationError::Internal)
            /// when `text` is not a UUID.
            fn from_str(text: &str) -> $crate::error::Result<Self> {
                $crate::util::parse_uuid(text).map(Self)
            }
        }

        impl ::std::convert::From<::uuid::Uuid> for $name {
            /// Wraps a UUID that is already known to identify such a record.
            fn from(uuid: ::uuid::Uuid) -> Self {
                Self(uuid)
            }
        }
    };
}

pub(crate) use define_id;

#[cfg(test)]
mod tests {
    use crate::domain::{AccountId, EntityId, JournalEntryId, JournalLineId, RecurringTemplateId};
    use crate::error::{Error, ValidationError};

    /// A UUID in the hyphenated lowercase form the application writes.
    const STORED: &str = "22222222-2222-4222-8222-222222222222";

    #[test]
    fn an_id_serializes_as_the_bare_uuid_string_and_reads_back() {
        let id: AccountId = STORED.parse().unwrap();
        let json = serde_json::to_string(&id).unwrap();

        assert_eq!(json, r#""22222222-2222-4222-8222-222222222222""#);
        assert_eq!(serde_json::from_str::<AccountId>(&json).unwrap(), id);
    }

    #[test]
    fn every_id_type_displays_the_text_it_parses() {
        assert_eq!(STORED.parse::<AccountId>().unwrap().to_string(), STORED);
        assert_eq!(STORED.parse::<EntityId>().unwrap().to_string(), STORED);
        assert_eq!(
            STORED.parse::<JournalEntryId>().unwrap().to_string(),
            STORED
        );
        assert_eq!(STORED.parse::<JournalLineId>().unwrap().to_string(), STORED);
        assert_eq!(
            STORED.parse::<RecurringTemplateId>().unwrap().to_string(),
            STORED
        );
    }

    #[test]
    fn display_is_lowercase_whatever_case_was_parsed() {
        let id: EntityId = "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA".parse().unwrap();
        assert_eq!(id.to_string(), "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    }

    #[test]
    fn text_that_is_not_a_uuid_is_the_internal_validation_error() {
        assert_eq!(
            "nope".parse::<AccountId>(),
            Err(Error::Validation(ValidationError::Internal {
                detail: "invalid id: nope".to_owned(),
            }))
        );
    }

    #[test]
    fn generated_ids_are_version_four_and_distinct() {
        let first = JournalEntryId::generate();
        let second = JournalEntryId::generate();

        assert_ne!(first, second);
        assert_eq!(first.as_uuid().get_version_num(), 4);
    }
}

#[cfg(test)]
mod properties {
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;

    use crate::domain::AccountId;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        #[test]
        fn an_id_survives_display_then_parse(bytes in any::<[u8; 16]>()) {
            let id = AccountId::from(uuid::Uuid::from_bytes(bytes));
            prop_assert_eq!(id.to_string().parse::<AccountId>(), Ok(id));
        }

        #[test]
        fn parsing_any_text_returns_instead_of_panicking(text in any::<String>()) {
            let _ = text.parse::<AccountId>();
        }
    }
}
