//! Known biller brands and utility-service classification.
//!
//! Greek bills rarely spell out the issuer in extractable text — branding
//! lives in logos and images — so recognition leans on portal names, product
//! lines, and brand tokens that do survive extraction. Matches are
//! token-bounded to avoid substrings inside unrelated words.

/// What kind of service a recognized bill covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Service {
    Electricity,
    Gas,
    Telecom,
    Water,
}

impl Service {
    /// English display label used in suggested descriptions.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Electricity => "Electricity",
            Self::Gas => "Gas",
            Self::Telecom => "Telecom",
            Self::Water => "Water",
        }
    }
}

/// Recognize a known biller in lowercased document text.
///
/// Returns the display name and, when the brand implies it, the service.
/// Order matters: more specific tokens come before their substrings
/// (ΔΕΔΔΗΕ before ΔΕΗ).
pub(crate) fn known_brand(lower: &str) -> Option<(&'static str, Option<Service>)> {
    // Suppliers come first: ΔΕΔΔΗΕ (the grid operator) is printed on every
    // electricity bill regardless of who actually issues it, so it may only
    // win when no supplier brand is present.
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

    for (token, display, service) in BRANDS {
        if contains_token(lower, token) {
            return Some((display, *service));
        }
    }

    None
}

/// Classify the service of a utility-style bill by weighted keyword score.
///
/// Needed because bills mention other services in passing — electricity
/// bills carry a national energy-mix table that includes natural gas, so a
/// single keyword hit must never decide.
pub(crate) fn classify_service(lower: &str) -> Option<Service> {
    let weigh = |pairs: &[(&str, u32)]| -> u32 {
        pairs
            .iter()
            .map(|(kw, w)| if lower.contains(kw) { *w } else { 0 })
            .sum()
    };

    let electricity = weigh(&[
        ("προμήθεια ρεύματος", 4),
        ("προμηθεια ρευματος", 4),
        ("kwh", 2),
        ("δεδδηε", 2),
        ("ρεύμα", 1),
        ("ρευμα", 1),
    ]);

    let gas = weigh(&[
        ("προμήθεια φυσικού αερίου", 4),
        ("προμηθεια φυσικου αεριου", 4),
        ("ηκασπ", 3),
        ("χρέωση προμήθειας φ.α", 3),
        ("φυσικού αερίου", 1),
        ("φυσικου αεριου", 1),
        ("φυσικό αέριο", 1),
        ("φυσικο αεριο", 1),
    ]);

    let telecom = weigh(&[
        ("κινητό", 2),
        ("κινητο", 2),
        ("σταθερό", 1),
        ("σταθερο", 1),
        ("τηλεφων", 2),
        ("internet", 1),
    ]);

    let water = weigh(&[("ύδρευσ", 3), ("υδρευσ", 3), ("κατανάλωση νερού", 3)]);

    let best = [
        (electricity, Service::Electricity),
        (gas, Service::Gas),
        (telecom, Service::Telecom),
        (water, Service::Water),
    ]
    .into_iter()
    .max_by_key(|(score, _)| *score)?;

    (best.0 > 0).then_some(best.1)
}

/// Substring match with token boundaries: the neighbors of the match must
/// not be alphanumeric, so "nova" never fires inside "innovation".
fn contains_token(lower: &str, token: &str) -> bool {
    for (at, _) in lower.match_indices(token) {
        let before_ok = lower[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());

        let after_ok = lower[at + token.len()..]
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

    #[test]
    fn brand_tokens_respect_word_boundaries() {
        assert!(known_brand("πληρωμή μέσω nova app").is_some());
        assert!(known_brand("driving innovation forward").is_none());
        assert!(known_brand("synergy of the group").is_none());
    }

    #[test]
    fn energy_mix_table_does_not_flip_electricity_to_gas() {
        let lower = "προμήθεια ρεύματος zeniθ 25,29 kwh 119 \
                     2. φυσικού αερίου 1410.00 30% 3. υδροηλεκτρικη";

        assert_eq!(classify_service(lower), Some(Service::Electricity));
    }

    #[test]
    fn gas_bill_classifies_as_gas() {
        let lower = "προμηθεια φυσικου αεριου κατανάλωση ηκασπ 20190002009749";

        assert_eq!(classify_service(lower), Some(Service::Gas));
    }

    #[test]
    fn portal_name_identifies_volton() {
        assert_eq!(
            known_brand("συνδέσου στο myon και διαχειρίσου τον λογαριασμό σου"),
            Some(("Volton", None))
        );
    }
}
