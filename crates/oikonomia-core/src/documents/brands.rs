//! Known biller brands and the service a utility bill is for.
//!
//! Greek bills rarely spell out the issuer in text that can be extracted: the
//! branding is in logos and images. Recognition therefore leans on what does
//! survive extraction, such as portal names, product lines and brand tokens.
//!
//! Two questions are answered here, both on folded text
//! ([`folded`](crate::documents::keyword::folded)):
//!
//! - [`known_brand`]: who issued this? The first entry of [`BRANDS`] whose
//!   token stands in the text as a whole word wins, so the order of the
//!   table is a priority order.
//! - [`classify_service`]: is this electricity, gas, telecom or water? Each
//!   keyword of [`SERVICE_KEYWORDS`] found in the text adds its weight to its
//!   service, and the highest total wins.
//!
//! The invoice reader asks the brand first and falls back to the keyword
//! score only when the brand does not imply a service.

use crate::documents::keyword::Keyword;
use crate::documents::keyword::Keyword::{Prefix, Unit, Word};
use crate::text::BillKind;

/// What kind of service a recognized bill covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Service {
    /// Electric power.
    Electricity,
    /// Natural gas.
    Gas,
    /// Fixed or mobile phone, and internet.
    Telecom,
    /// Water supply.
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

/// A biller the reader knows by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Brand {
    /// The name to show for the biller.
    pub name: &'static str,
    /// The service the brand sells, when it sells only one.
    pub service: Option<Service>,
}

impl Brand {
    /// A brand shown as `name` that implies `service`.
    const fn new(name: &'static str, service: Option<Service>) -> Self {
        Self { name, service }
    }
}

/// Brand tokens in folded form, each with the brand it stands for. The first
/// entry that matches wins.
///
/// Suppliers come before ΔΕΔΔΗΕ: the grid operator is printed on every
/// electricity bill regardless of who issues it, so it may only win when no
/// supplier brand is present.
const BRANDS: &[(Keyword, Brand)] = &[
    (Word("zenith"), Brand::new("ZeniΘ", None)),
    (Word("zeniθ"), Brand::new("ZeniΘ", None)),
    // "MyON" is the customer portal printed on Volton gas bills; the
    // company name itself never survives text extraction.
    (Word("volton"), Brand::new("Volton", None)),
    (Word("myon"), Brand::new("Volton", None)),
    (Word("nova"), Brand::new("Nova", Some(Service::Telecom))),
    (
        Word("cosmote"),
        Brand::new("Cosmote", Some(Service::Telecom)),
    ),
    (
        Word("vodafone"),
        Brand::new("Vodafone", Some(Service::Telecom)),
    ),
    (Word("elpedison"), Brand::new("Elpedison", None)),
    (Word("protergia"), Brand::new("Protergia", None)),
    (Word("ηρων"), Brand::new("ΗΡΩΝ", None)),
    (Word("heron"), Brand::new("ΗΡΩΝ", None)),
    (Word("nrg"), Brand::new("nrg", None)),
    (Word("δεη"), Brand::new("ΔΕΗ", Some(Service::Electricity))),
    (Word("ευδαπ"), Brand::new("ΕΥΔΑΠ", Some(Service::Water))),
    (Word("ευαθ"), Brand::new("ΕΥΑΘ", Some(Service::Water))),
    (
        Word("δεδδηε"),
        Brand::new("ΔΕΔΔΗΕ", Some(Service::Electricity)),
    ),
];

/// How strongly a keyword points at a service: the scale of
/// [`SERVICE_KEYWORDS`].
///
/// A service's score is the sum of the weights of its keywords that the text
/// holds, and the highest sum wins. A bill names other services in passing:
/// an electricity bill carries the national energy-mix table, which lists
/// natural gas. Such a word is a [`MENTION`](weight::MENTION), the lowest
/// level, so one mention loses to any one stronger keyword of the bill's own
/// service. Sums can still tie or overtake: two mentions equal one
/// [`TERM`](weight::TERM), and [`SERVICE_KEYWORDS`] says how a tie ends.
///
/// The four values are 1 to 4, the smallest whole numbers in that order.
/// Which keyword got which level is not recorded. One test needs `MENTION`
/// to be above zero (`one_keyword_decides_when_it_is_the_only_one`); no test
/// fails when any other level is lowered to 1.
mod weight {
    /// A word that bills of other services also print.
    pub(super) const MENTION: u32 = 1;
    /// A unit or term typical of the service.
    pub(super) const TERM: u32 = 2;
    /// A term that only a bill of this service prints.
    pub(super) const OWN_TERM: u32 = 3;
    /// The line that names the supply being billed ("supply of ...").
    pub(super) const SUPPLY_LINE: u32 = 4;
}

/// Keywords of each service in folded form, with their weights, in the order
/// [`classify_service`] compares them.
///
/// Each keyword is matched under its own rule, and every keyword that
/// matches counts. So a supply line also scores the shorter keyword of its
/// service: `προμηθεια ρευματος` holds the stem `ρευμα`, and scores a
/// `SUPPLY_LINE` and a `MENTION` together.
///
/// The order of the services breaks ties: the later one wins.
const SERVICE_KEYWORDS: &[(Service, &[(Keyword, u32)])] = &[
    (
        Service::Electricity,
        &[
            (Word("προμηθεια ρευματος"), weight::SUPPLY_LINE),
            (Unit("kwh"), weight::TERM),
            (Word("δεδδηε"), weight::TERM),
            (Prefix("ρευμα"), weight::MENTION),
        ],
    ),
    (
        Service::Gas,
        &[
            (Word("προμηθεια φυσικου αεριου"), weight::SUPPLY_LINE),
            (Unit("ηκασπ"), weight::OWN_TERM),
            (Word("χρεωση προμηθειας φ.α"), weight::OWN_TERM),
            (Word("φυσικου αεριου"), weight::MENTION),
            (Prefix("φυσικο αεριο"), weight::MENTION),
        ],
    ),
    (
        Service::Telecom,
        &[
            (Prefix("κινητο"), weight::TERM),
            (Prefix("σταθερο"), weight::MENTION),
            (Prefix("τηλεφων"), weight::TERM),
            (Word("internet"), weight::MENTION),
        ],
    ),
    (
        Service::Water,
        &[
            (Prefix("υδρευσ"), weight::OWN_TERM),
            (Word("καταναλωση νερου"), weight::OWN_TERM),
        ],
    ),
];

/// Recognizes a known biller in folded document text.
///
/// The first entry of [`BRANDS`] whose token appears wins, wherever in the
/// text each token is. Returns `None` when no token appears.
pub(crate) fn known_brand(folded_text: &str) -> Option<Brand> {
    BRANDS
        .iter()
        .find(|(token, _)| token.occurs_in(folded_text))
        .map(|(_, brand)| *brand)
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
    let score = |keywords: &[(Keyword, u32)]| -> u32 {
        keywords
            .iter()
            .filter(|(keyword, _)| keyword.occurs_in(folded_text))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::keyword::folded;

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
            Some(Brand::new("ΔΕΗ", Some(Service::Electricity)))
        );
        assert_eq!(
            known_brand(&folded("Δίκτυο ΔΕΔΔΗΕ")),
            Some(Brand::new("ΔΕΔΔΗΕ", Some(Service::Electricity)))
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
            Some(Brand::new("Volton", None))
        );
    }

    #[test]
    fn every_brand_token_and_service_keyword_is_in_folded_form() {
        for (token, _) in BRANDS {
            assert_eq!(folded(token.text()), token.text(), "brand token {token:?}");
        }
        for (service, keywords) in SERVICE_KEYWORDS {
            for (keyword, _) in *keywords {
                assert_eq!(
                    folded(keyword.text()),
                    keyword.text(),
                    "{service:?} keyword {keyword:?}"
                );
            }
        }
    }
}
