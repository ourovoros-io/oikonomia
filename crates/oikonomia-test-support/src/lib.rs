//! Test support shared by the workspace's crates.
//!
//! A dev-dependency only: nothing here is compiled into the application.
//! It holds [`listed_variants!`], which the tests of the hand-written `ALL`
//! lists are built on, and [`PROPERTY_CASES`], the size of a property test.

/// The number of cases each property test runs.
///
/// Miri interprets every case, which is far slower than running it, so only
/// a few cases run there.
pub const PROPERTY_CASES: u32 = if cfg!(miri) { 8 } else { 256 };

/// Lists the variants of an enumeration once and derives from that single list
/// everything a code-list guard needs.
///
/// A hand-written `ALL` list is only trustworthy if something other than the
/// list itself knows every variant. An exhaustive `match` is that something,
/// but a guard that pairs it with a hand-kept count still passes when a new
/// variant gets a match arm and is left out of both the list and the count.
/// This macro removes the count: the variants are written once, and both the
/// exhaustive `match` and the number of variants are generated from them.
///
/// The invocation generates a module named `$listing` with:
///
/// - `COUNT`, the number of listed variants;
/// - `position`, a function whose `match` has one arm per listed variant and
///   no wildcard arm, so an enumeration variant missing from the list does not
///   compile, and which returns the variant's place in the list;
/// - `assert_every_position_once`, which fails unless the given positions are
///   exactly `0..COUNT` once each, so a listed variant that the checked list
///   leaves out is reported;
/// - for the `units` form only, `variants`, the array of the listed variants
///   themselves, for comparing against an `ALL` list as a set.
///
/// The `patterns` form takes match patterns, so variants that carry data are
/// written as `Enum::Variant { .. }`. The `units` form takes plain paths,
/// which are valid both as patterns and as values.
///
/// # Examples
///
/// ```
/// use oikonomia_test_support::listed_variants;
///
/// #[derive(Debug, PartialEq)]
/// enum Role {
///     Category,
///     Payment,
/// }
///
/// listed_variants! {
///     units listed_roles for Role {
///         Role::Category,
///         Role::Payment,
///     }
/// }
///
/// fn main() {
///     assert_eq!(listed_roles::COUNT, 2);
///     assert_eq!(listed_roles::position(&Role::Payment), 1);
///     assert_eq!(listed_roles::variants(), [Role::Category, Role::Payment]);
///     listed_roles::assert_every_position_once(vec![1, 0]);
/// }
/// ```
#[macro_export]
macro_rules! listed_variants {
    (@one $variant:pat) => {
        ()
    };

    (@arms $value:ident, [$($arms:tt)*], ($index:expr),) => {
        match $value {
            $($arms)*
        }
    };

    (@arms
        $value:ident,
        [$($arms:tt)*],
        ($index:expr),
        $head:pat,
        $($tail:pat,)*
    ) => {
        $crate::listed_variants!(
            @arms
            $value,
            [$($arms)* $head => $index,],
            ($index + 1),
            $($tail,)*
        )
    };

    (@module $listing:ident $enum:ident [$($extra:item)*] $($variant:pat),+) => {
        mod $listing {
            use super::$enum;

            pub(super) const COUNT: usize = [
                $($crate::listed_variants!(@one $variant)),+
            ]
            .len();

            pub(super) fn position(value: &$enum) -> usize {
                $crate::listed_variants!(@arms value, [], (0), $($variant,)+)
            }

            pub(super) fn assert_every_position_once(mut positions: Vec<usize>) {
                positions.sort_unstable();

                assert_eq!(
                    positions,
                    (0..COUNT).collect::<Vec<_>>(),
                    "every listed variant must appear exactly once"
                );
            }

            $($extra)*
        }
    };

    (units $listing:ident for $enum:ident { $($variant:path),+ $(,)? }) => {
        $crate::listed_variants!(
            @module
            $listing
            $enum
            [
                pub(super) fn variants() -> [$enum; COUNT] {
                    [$($variant),+]
                }
            ]
            $($variant),+
        );
    };

    (patterns $listing:ident for $enum:ident { $($variant:pat),+ $(,)? }) => {
        $crate::listed_variants!(@module $listing $enum [] $($variant),+);
    };
}
