//! Known biller brands and utility-service classification.
//!
//! Greek bills rarely spell out the issuer in extractable text — branding
//! lives in logos and images — so recognition leans on portal names, product
//! lines, and brand tokens that do survive extraction. Matches are
//! token-bounded to avoid substrings inside unrelated words.

use crate::text::BillKind;

/// What kind of service a recognized bill covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Service {
    Electricity,
    Gas,
    Telecom,
    Water,
}

impl Service {
    /// The bill kind whose suggested title covers this service.
    pub(crate) const fn bill_kind(self) -> BillKind {
        match self {
            Self::Electricity => BillKind::Electricity,
            Self::Gas => BillKind::Gas,
            Self::Telecom => BillKind::Telecom,
            Self::Water => BillKind::Water,
        }
    }
}

/// Brand tokens in folded form, with the display name and, when the brand
/// implies it, the service. The first entry that matches wins.
///
/// Suppliers come before ΔΕΔΔΗΕ: the grid operator is printed on every
/// electricity bill regardless of who issues it, so it may only win when no
/// supplier brand is present.
const BRANDS: &[(&str, &str, Option<Service>)] = &[
    ("zenith", "ZeniΘ", None),
    ("zeniθ", "ZeniΘ", None),
    // "MyON" is the customer portal printed on Volton gas bills; the
    // company name itself never survives text extraction.
    ("volton", "Volton", None),
    ("myon", "Volton", None),
    ("nova", "Nova", Some(Service::Telecom)),
    ("cosmote", "Cosmote", Some(Service::Telecom)),
    ("vodafone", "Vodafone", Some(Service::Telecom)),
    ("elpedison", "Elpedison", None),
    ("protergia", "Protergia", None),
    ("ηρων", "ΗΡΩΝ", None),
    ("heron", "ΗΡΩΝ", None),
    ("nrg", "nrg", None),
    ("δεη", "ΔΕΗ", Some(Service::Electricity)),
    ("ευδαπ", "ΕΥΔΑΠ", Some(Service::Water)),
    ("ευαθ", "ΕΥΑΘ", Some(Service::Water)),
    ("δεδδηε", "ΔΕΔΔΗΕ", Some(Service::Electricity)),
];

/// How strongly a keyword points at a service.
mod weight {
    /// A word that bills of other services also print.
    pub(super) const MENTION: u32 = 1;
    /// A unit or term typical of the service.
    pub(super) const TERM: u32 = 2;
    /// A term that only a bill of this service prints.
    pub(super) const OWN_TERM: u32 = 3;
    /// The line that names the supply being billed.
    pub(super) const SUPPLY_LINE: u32 = 4;
}

/// Keywords of each service in folded form, with their weights, in the order
/// [`classify_service`] compares them.
const SERVICE_KEYWORDS: &[(Service, &[(&str, u32)])] = &[
    (
        Service::Electricity,
        &[
            ("προμηθεια ρευματος", weight::SUPPLY_LINE),
            ("kwh", weight::TERM),
            ("δεδδηε", weight::TERM),
            ("ρευμα", weight::MENTION),
        ],
    ),
    (
        Service::Gas,
        &[
            ("προμηθεια φυσικου αεριου", weight::SUPPLY_LINE),
            ("ηκασπ", weight::OWN_TERM),
            ("χρεωση προμηθειας φ.α", weight::OWN_TERM),
            ("φυσικου αεριου", weight::MENTION),
            ("φυσικο αεριο", weight::MENTION),
        ],
    ),
    (
        Service::Telecom,
        &[
            ("κινητο", weight::TERM),
            ("σταθερο", weight::MENTION),
            ("τηλεφων", weight::TERM),
            ("internet", weight::MENTION),
        ],
    ),
    (
        Service::Water,
        &[
            ("υδρευσ", weight::OWN_TERM),
            ("καταναλωση νερου", weight::OWN_TERM),
        ],
    ),
];

/// Recognizes a known biller in folded document text
/// ([`folded`](crate::documents::invoice::folded)).
///
/// Returns the display name and, when the brand implies it, the service.
/// The first entry of [`BRANDS`] whose token appears wins.
pub(crate) fn known_brand(folded_text: &str) -> Option<(&'static str, Option<Service>)> {
    BRANDS
        .iter()
        .find(|(token, _, _)| contains_token(folded_text, token))
        .map(|(_, display, service)| (*display, *service))
}

/// Classifies the service of a utility-style bill in folded text by weighted
/// keyword score.
///
/// Every keyword of [`SERVICE_KEYWORDS`] found in the text adds its weight
/// to its service, and the highest total wins. Bills mention other services
/// in passing: an electricity bill carries a national energy-mix table that
/// names natural gas. Such a mention scores 1, so it loses to the keywords
/// of the bill's own service. When one keyword is all the text has, that
/// keyword decides.
///
/// On a tie the service listed later in [`SERVICE_KEYWORDS`] wins. Returns
/// `None` when no keyword matches.
pub(crate) fn classify_service(folded_text: &str) -> Option<Service> {
    let score = |keywords: &[(&str, u32)]| -> u32 {
        keywords
            .iter()
            .filter(|(keyword, _)| folded_text.contains(keyword))
            .map(|(_, weight)| *weight)
            .sum()
    };

    // `max_by_key` keeps the last of several equal maxima.
    let (best_score, service) = SERVICE_KEYWORDS
        .iter()
        .map(|(service, keywords)| (score(keywords), *service))
        .max_by_key(|(total, _)| *total)?;

    (best_score > 0).then_some(service)
}

/// Substring match with token boundaries: the neighbors of the match must
/// not be alphanumeric, so "nova" never fires inside "innovation".
#[expect(
    clippy::string_slice,
    reason = "`match_indices` yields the offset of a match of `token`, \
              so both ends of the match are character boundaries"
)]
fn contains_token(folded_text: &str, token: &str) -> bool {
    for (at, _) in folded_text.match_indices(token) {
        let before_ok = folded_text[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());

        let after_ok = folded_text[at + token.len()..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric());

        if before_ok && after_ok {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::invoice::folded;

    #[test]
    fn brand_tokens_respect_word_boundaries() {
        assert!(known_brand(&folded("Πληρωμή μέσω Nova app")).is_some());
        assert!(known_brand("driving innovation forward").is_none());
        assert!(known_brand("synergy of the group").is_none());
    }

    #[test]
    fn a_supplier_wins_over_the_grid_operator_wherever_it_is_printed() {
        assert_eq!(
            known_brand(&folded("Δίκτυο ΔΕΔΔΗΕ\nΠρομήθεια Ρεύματος ΔΕΗ")),
            Some(("ΔΕΗ", Some(Service::Electricity)))
        );
        assert_eq!(
            known_brand(&folded("Δίκτυο ΔΕΔΔΗΕ")),
            Some(("ΔΕΔΔΗΕ", Some(Service::Electricity)))
        );
    }

    #[test]
    fn energy_mix_table_does_not_flip_electricity_to_gas() {
        let text = folded(
            "Προμήθεια Ρεύματος ZeniΘ 25,29 kWh 119 \
             2. ΦΥΣΙΚΟΥ ΑΕΡΙΟΥ 1410.00 30% 3. ΥΔΡΟΗΛΕΚΤΡΙΚΗ",
        );

        assert_eq!(classify_service(&text), Some(Service::Electricity));
    }

    #[test]
    fn gas_bill_classifies_as_gas() {
        let text = folded("ΠΡΟΜΗΘΕΙΑ ΦΥΣΙΚΟΥ ΑΕΡΙΟΥ Κατανάλωση ΗΚΑΣΠ SYN000000001");

        assert_eq!(classify_service(&text), Some(Service::Gas));
    }

    #[test]
    fn accented_and_all_caps_keywords_classify_alike() {
        for text in ["Ύδρευση", "ΥΔΡΕΥΣΗ", "υδρευση"] {
            assert_eq!(
                classify_service(&folded(text)),
                Some(Service::Water),
                "{text}"
            );
        }
        for text in ["Κινητό τηλέφωνο", "ΚΙΝΗΤΟ ΤΗΛΕΦΩΝΟ"] {
            assert_eq!(
                classify_service(&folded(text)),
                Some(Service::Telecom),
                "{text}"
            );
        }
    }

    #[test]
    fn one_keyword_decides_when_it_is_the_only_one() {
        assert_eq!(classify_service("internet"), Some(Service::Telecom));
        assert_eq!(classify_service("nothing relevant"), None);
    }

    #[test]
    fn a_tie_goes_to_the_service_listed_later() {
        // "kwh" scores 2 for electricity, "τηλεφων" scores 2 for telecom.
        assert_eq!(classify_service("kwh τηλεφων"), Some(Service::Telecom));
    }

    #[test]
    fn portal_name_identifies_volton() {
        assert_eq!(
            known_brand(&folded(
                "Συνδέσου στο MyON και διαχειρίσου τον λογαριασμό σου"
            )),
            Some(("Volton", None))
        );
    }

    #[test]
    fn every_brand_token_and_service_keyword_is_in_folded_form() {
        for (token, _, _) in BRANDS {
            assert_eq!(folded(token), *token, "brand token {token:?}");
        }
        for (service, keywords) in SERVICE_KEYWORDS {
            for (keyword, _) in *keywords {
                assert_eq!(folded(keyword), *keyword, "{service:?} keyword {keyword:?}");
            }
        }
    }
}
