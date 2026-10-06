//! Donation addresses. Oikonomia is free; these are the only way to pay for it.
//!
//! The table is the single source of truth: the Settings page renders what
//! [`donation_addresses`] returns, and a test keeps `README.md` in step.
//! The tests check each address's format only; they cannot confirm an
//! address is the owner's or that its checksum is valid. Verify checksums
//! offline whenever an address is added or changed.
//! Nothing here touches the network.

use serde::Serialize;

/// A coin with its own receiving address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub(crate) enum Coin {
    /// Bitcoin.
    Btc,
    /// Ether, on Ethereum mainnet.
    Eth,
    /// Monero.
    Xmr,
    /// Dash.
    Dash,
    /// Litecoin.
    Ltc,
    /// SOL, on Solana.
    Sol,
    /// Zcash, transparent address.
    Zec,
}

/// One receiving address, as shown in Settings and the README.
#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct DonationAddress {
    /// The coin this address belongs to.
    pub(crate) coin: Coin,
    /// Network name, shown so a donor does not send on the wrong chain.
    pub(crate) network: &'static str,
    /// Tokens the same address also receives on this network.
    pub(crate) also_accepts: &'static [&'static str],
    /// The address itself.
    pub(crate) address: &'static str,
}

/// Every address donations are accepted on.
///
/// Each was supplied by the project owner; never edit one without the owner
/// confirming it against their wallet. The tests only check format, so
/// verify the checksum offline whenever an address is added or changed.
pub(crate) const DONATION_ADDRESSES: &[DonationAddress] = &[
    DonationAddress {
        coin: Coin::Btc,
        network: "Bitcoin",
        also_accepts: &[],
        address: "bc1q9gey0j6vvd2nh7eh76tp932r2ttmdj75u55een",
    },
    DonationAddress {
        coin: Coin::Eth,
        network: "Ethereum",
        also_accepts: &["USDC", "USDT"],
        address: "0x544506F873EF9157E3639B9D0Af13562245baf07",
    },
    DonationAddress {
        coin: Coin::Xmr,
        network: "Monero",
        also_accepts: &[],
        address: "8ABaPsJS6754dY7YsZLKuHRrYFMtE5BBmi8SwZ7n79ukMAHkN987PZFHPMwaD4QhLegX6MPAjwEup69RbMAEnRcENDdfavg",
    },
    DonationAddress {
        coin: Coin::Dash,
        network: "Dash",
        also_accepts: &[],
        address: "XkfYevHXMAFwMcY6nj95oedwxaicTpqbSs",
    },
    DonationAddress {
        coin: Coin::Ltc,
        network: "Litecoin",
        also_accepts: &[],
        address: "ltc1q2lhxq7crwaam9upc8ks0gmyvtxn9htatfgns33",
    },
    DonationAddress {
        coin: Coin::Sol,
        network: "Solana",
        also_accepts: &["USDC", "USDT"],
        address: "EZVrqTLW3sTHdFZydoR31Nv3QLnWShfV4riah1otTC26",
    },
    DonationAddress {
        coin: Coin::Zec,
        network: "Zcash (transparent)",
        also_accepts: &[],
        address: "t1MyGx1wXSQRyQZeXJjyKjHKTZEWSyEiBkj",
    },
];

/// Donation addresses for the Settings page. Readable while locked.
#[tauri::command]
pub fn donation_addresses() -> Vec<DonationAddress> {
    DONATION_ADDRESSES.to_vec()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{Coin, DONATION_ADDRESSES, DonationAddress};

    const BASE58: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    const BECH32: &str = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";

    fn all_in(text: &str, alphabet: &str) -> bool {
        !text.is_empty() && text.chars().all(|character| alphabet.contains(character))
    }

    fn is_base58_with(
        address: &str,
        prefixes: &[char],
        lengths: std::ops::RangeInclusive<usize>,
    ) -> bool {
        all_in(address, BASE58) && lengths.contains(&address.len()) && address.starts_with(prefixes)
    }

    fn is_bech32_with(
        address: &str,
        prefix: &str,
        lengths: std::ops::RangeInclusive<usize>,
    ) -> bool {
        address
            .strip_prefix(prefix)
            .is_some_and(|data| all_in(data, BECH32))
            && lengths.contains(&address.len())
    }

    /// Whether `address` has the right shape for `coin`. A format check, not
    /// a checksum: it catches a changed prefix, a changed length, or a
    /// character outside the alphabet. A same-length substitution of another
    /// in-alphabet character passes, as does a case change in an Ethereum
    /// address. Verify checksums separately whenever an address is added or
    /// changed. It cannot tell whose address it is.
    fn has_valid_shape(coin: Coin, address: &str) -> bool {
        match coin {
            Coin::Btc => {
                is_bech32_with(address, "bc1", 42..=62)
                    || is_base58_with(address, &['1', '3'], 26..=35)
            }
            Coin::Ltc => {
                is_bech32_with(address, "ltc1", 43..=63)
                    || is_base58_with(address, &['L', 'M', '3'], 26..=35)
            }
            Coin::Dash => is_base58_with(address, &['X', '7'], 34..=34),
            Coin::Xmr => {
                is_base58_with(address, &['4', '8'], 95..=95)
                    || is_base58_with(address, &['4'], 106..=106)
            }
            Coin::Sol => all_in(address, BASE58) && (32..=44).contains(&address.len()),
            Coin::Zec => {
                all_in(address, BASE58)
                    && address.len() == 35
                    && (address.starts_with("t1") || address.starts_with("t3"))
            }
            Coin::Eth => address.strip_prefix("0x").is_some_and(|hex| {
                hex.len() == 40 && hex.chars().all(|character| character.is_ascii_hexdigit())
            }),
        }
    }

    #[test]
    fn shape_check_accepts_well_formed_addresses() {
        assert!(has_valid_shape(
            Coin::Btc,
            "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa"
        ));
        assert!(has_valid_shape(
            Coin::Btc,
            "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq"
        ));
        assert!(has_valid_shape(
            Coin::Eth,
            "0xde0B295669a9FD93d5F28D9Ec85E40f4cb697BAe"
        ));
        assert!(has_valid_shape(
            Coin::Sol,
            "11111111111111111111111111111111"
        ));
        assert!(has_valid_shape(Coin::Dash, &format!("X{}", "a".repeat(33))));
        assert!(has_valid_shape(Coin::Ltc, &format!("L{}", "a".repeat(33))));
        assert!(has_valid_shape(Coin::Xmr, &format!("4{}", "A".repeat(94))));
        assert!(has_valid_shape(Coin::Xmr, &format!("8{}", "A".repeat(94))));
        assert!(has_valid_shape(Coin::Zec, &format!("t1{}", "a".repeat(33))));
    }

    #[test]
    fn shape_check_rejects_damaged_or_misplaced_addresses() {
        let bitcoin = "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq";
        let ether = "0xde0B295669a9FD93d5F28D9Ec85E40f4cb697BAe";

        // Whitespace from a careless paste.
        assert!(!has_valid_shape(Coin::Btc, &format!("{bitcoin}\n")));
        assert!(!has_valid_shape(Coin::Eth, &format!(" {ether}")));

        // Truncated, empty, or a character outside the alphabet.
        assert!(!has_valid_shape(Coin::Eth, &ether[..41]));
        assert!(!has_valid_shape(Coin::Btc, ""));
        assert!(!has_valid_shape(
            Coin::Sol,
            "0OIl0OIl0OIl0OIl0OIl0OIl0OIl0OIl"
        ));
        assert!(!has_valid_shape(Coin::Xmr, &format!("4{}", "A".repeat(93))));
        assert!(!has_valid_shape(
            Coin::Zec,
            &format!("t1{}", "a".repeat(32))
        ));

        // The right address in the wrong row.
        assert!(!has_valid_shape(Coin::Ltc, bitcoin));
        assert!(!has_valid_shape(Coin::Btc, ether));
        assert!(!has_valid_shape(
            Coin::Dash,
            "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa"
        ));
        assert!(!has_valid_shape(
            Coin::Zec,
            "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa"
        ));
        assert!(!has_valid_shape(
            Coin::Dash,
            &format!("t1{}", "a".repeat(33))
        ));
    }

    #[test]
    fn every_listed_address_is_well_formed_and_unique() {
        for entry in DONATION_ADDRESSES {
            assert!(
                has_valid_shape(entry.coin, entry.address),
                "{:?} address is malformed: {:?}",
                entry.coin,
                entry.address
            );
            assert_ne!(entry.network, "", "{:?} has no network name", entry.coin);
        }

        let unique: HashSet<&str> = DONATION_ADDRESSES
            .iter()
            .map(|entry| entry.address)
            .collect();
        assert_eq!(unique.len(), DONATION_ADDRESSES.len(), "duplicate address");

        let coins: HashSet<Coin> = DONATION_ADDRESSES.iter().map(|entry| entry.coin).collect();
        assert_eq!(
            coins.len(),
            DONATION_ADDRESSES.len(),
            "a coin is listed twice"
        );
    }

    #[test]
    fn table_lists_the_supplied_coins_in_display_order() {
        let coins: Vec<Coin> = DONATION_ADDRESSES.iter().map(|entry| entry.coin).collect();

        assert_eq!(
            coins,
            [
                Coin::Btc,
                Coin::Eth,
                Coin::Xmr,
                Coin::Dash,
                Coin::Ltc,
                Coin::Sol,
                Coin::Zec
            ]
        );
    }

    #[test]
    fn payload_carries_ticker_network_and_stablecoin_note() {
        let json = serde_json::to_value(super::donation_addresses()).expect("serialize");
        let entries = json.as_array().expect("array");
        let find = |ticker: &str| {
            entries
                .iter()
                .find(|entry| entry["coin"] == ticker)
                .expect("every listed coin is in the payload")
        };

        let bitcoin = find("BTC");
        assert_eq!(bitcoin["network"], "Bitcoin");
        assert_eq!(bitcoin["also_accepts"].as_array().expect("array").len(), 0);

        let ether = find("ETH");
        assert_eq!(ether["also_accepts"], serde_json::json!(["USDC", "USDT"]));

        let monero = find("XMR");
        assert_eq!(monero["network"], "Monero");

        let zcash = find("ZEC");
        assert_eq!(zcash["network"], "Zcash (transparent)");
    }

    #[test]
    fn stablecoins_ride_on_the_ether_and_solana_addresses_only() {
        for entry in DONATION_ADDRESSES {
            let expected: &[&str] = match entry.coin {
                Coin::Eth | Coin::Sol => &["USDC", "USDT"],
                _ => &[],
            };
            assert_eq!(entry.also_accepts, expected, "{:?}", entry.coin);
        }
    }

    fn ticker(coin: Coin) -> String {
        serde_json::to_value(coin)
            .expect("serialize")
            .as_str()
            .expect("string")
            .to_owned()
    }

    /// The exact README table line an entry must have: the ticker and any
    /// tokens it also accepts, the network, then the address in backticks.
    fn expected_readme_row(entry: &DonationAddress) -> String {
        let mut labels = vec![ticker(entry.coin)];
        labels.extend(entry.also_accepts.iter().map(|token| (*token).to_owned()));

        format!(
            "| {} | {} | `{}` |",
            labels.join(", "),
            entry.network,
            entry.address
        )
    }

    #[test]
    fn readme_donate_table_matches_the_address_table() {
        let readme = include_str!("../../../../README.md");

        let after_heading = readme
            .split_once("## Donate")
            .expect("README.md has a Donate section")
            .1;
        let section = after_heading
            .split_once("\n## ")
            .map_or(after_heading, |(section, _)| section);

        let rows: Vec<&str> = section
            .lines()
            .filter(|line| line.starts_with("| ") && line.contains('`'))
            .collect();

        let expected: Vec<String> = DONATION_ADDRESSES.iter().map(expected_readme_row).collect();

        // Whole-line comparison catches a wrong coin or network label, a
        // stale or dropped row, and rows listed in a different order.
        assert_eq!(rows, expected);
    }

    #[test]
    fn coins_serialize_as_ticker_symbols() {
        let coins = [
            Coin::Btc,
            Coin::Eth,
            Coin::Xmr,
            Coin::Dash,
            Coin::Ltc,
            Coin::Sol,
            Coin::Zec,
        ];

        let tickers: Vec<String> = coins
            .iter()
            .map(|coin| {
                serde_json::to_value(coin)
                    .expect("serialize")
                    .as_str()
                    .expect("string")
                    .to_owned()
            })
            .collect();

        assert_eq!(tickers, ["BTC", "ETH", "XMR", "DASH", "LTC", "SOL", "ZEC"]);
    }
}
