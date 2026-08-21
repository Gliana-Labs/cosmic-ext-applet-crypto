// SPDX-License-Identifier: MIT

//! Quote fetching and number formatting.
//!
//! Prices come from CoinGecko's public `simple/price` endpoint, which needs no API
//! key. One request covers every tracked coin.

use serde::Deserialize;
use std::collections::HashMap;

const API: &str = "https://api.coingecko.com/api/v3/simple/price";

/// CoinGecko's edge rejects requests that arrive without a User-Agent, so one has to
/// be set explicitly — reqwest sends none by default.
const USER_AGENT: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/Zetakai/cosmic-applet-crypto)"
);

/// One coin's price in the display currency, plus its 24h move.
#[derive(Debug, Clone, PartialEq)]
pub struct Quote {
    pub id: String,
    pub symbol: String,
    pub price: f64,
    pub change: Option<f64>,
}

/// CoinGecko replies with `{"bitcoin": {"usd": 77000.0, "usd_24h_change": 6.3}}`,
/// so the per-coin object is just a currency-keyed map.
#[derive(Deserialize)]
struct Raw(HashMap<String, HashMap<String, f64>>);

/// Fetches every coin in `ids` in a single request.
///
/// `ids` are CoinGecko slugs (`bitcoin`, not `BTC`) and `vs` is a fiat code such as
/// `usd`. Returns quotes in the order the ids were requested, skipping any the API
/// did not know about.
pub async fn fetch(ids: &[String], vs: &str) -> Result<Vec<Quote>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let url = format!(
        "{API}?ids={}&vs_currencies={}&include_24hr_change=true",
        ids.join(","),
        vs
    );

    let response = reqwest::Client::new()
        .get(&url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("CoinGecko returned {}", response.status()));
    }

    let Raw(map) = response
        .json::<Raw>()
        .await
        .map_err(|e| format!("could not parse response: {e}"))?;

    let change_key = format!("{vs}_24h_change");
    let quotes = ids
        .iter()
        .filter_map(|id| {
            let entry = map.get(id)?;
            Some(Quote {
                id: id.clone(),
                symbol: symbol_for(id),
                price: *entry.get(vs)?,
                change: entry.get(&change_key).copied(),
            })
        })
        .collect();

    Ok(quotes)
}

/// Maps common CoinGecko slugs to their ticker. Anything unmapped falls back to the
/// uppercased slug, which is wrong-looking but never blank.
pub fn symbol_for(id: &str) -> String {
    match id {
        "bitcoin" => "BTC",
        "ethereum" => "ETH",
        "binancecoin" => "BNB",
        "ripple" => "XRP",
        "solana" => "SOL",
        "cardano" => "ADA",
        "dogecoin" => "DOGE",
        "polkadot" => "DOT",
        "chainlink" => "LINK",
        "litecoin" => "LTC",
        "avalanche-2" => "AVAX",
        "tron" => "TRX",
        "cosmos" => "ATOM",
        "tether" => "USDT",
        "usd-coin" => "USDC",
        other => return other.to_uppercase().replace('-', "_"),
    }
    .to_owned()
}

/// Currency symbol for the display currency, falling back to the uppercased code.
pub fn currency_prefix(vs: &str) -> String {
    match vs {
        "usd" => "$",
        "eur" => "€",
        "gbp" => "£",
        "jpy" => "¥",
        "idr" => "Rp",
        other => return format!("{} ", other.to_uppercase()),
    }
    .to_owned()
}

/// Full precision with grouped thousands, for the popup: `$77,081` / `$0.0841`.
pub fn format_amount(value: f64) -> String {
    let magnitude = value.abs();
    let decimals = if magnitude >= 1000.0 {
        0
    } else if magnitude >= 1.0 {
        2
    } else if magnitude >= 0.01 {
        4
    } else {
        8
    };

    let rendered = format!("{:.*}", decimals, magnitude);
    let (integer, fraction) = match rendered.split_once('.') {
        Some((i, f)) => (i.to_owned(), format!(".{f}")),
        None => (rendered, String::new()),
    };

    let mut grouped = String::new();
    for (count, ch) in integer.chars().rev().enumerate() {
        if count > 0 && count % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let grouped: String = grouped.chars().rev().collect();

    format!(
        "{}{grouped}{fraction}",
        if value < 0.0 { "-" } else { "" }
    )
}

/// Abbreviated form for the panel, where width is scarce: `77.1k`, `1.2M`.
pub fn format_compact(value: f64) -> String {
    let magnitude = value.abs();
    if magnitude >= 1e9 {
        format!("{:.1}B", value / 1e9)
    } else if magnitude >= 1e6 {
        format!("{:.1}M", value / 1e6)
    } else if magnitude >= 1e3 {
        format!("{:.1}k", value / 1e3)
    } else if magnitude >= 1.0 {
        format!("{value:.2}")
    } else if magnitude >= 0.01 {
        format!("{value:.4}")
    } else {
        format!("{value:.6}")
    }
}

/// `▲ 6.28%` for the popup, `▲6.3%` for the panel.
pub fn format_change(change: f64, short: bool) -> String {
    let arrow = if change > 0.0 {
        '▲'
    } else if change < 0.0 {
        '▼'
    } else {
        '•'
    };
    if short {
        format!("{arrow}{change:.1}%")
    } else {
        format!("{arrow} {change:.2}%")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_thousands_and_scales_decimals() {
        assert_eq!(format_amount(77081.0), "77,081");
        assert_eq!(format_amount(2413.5), "2,414");
        assert_eq!(format_amount(675.91), "675.91");
        assert_eq!(format_amount(1.37), "1.37");
        assert_eq!(format_amount(0.0841), "0.0841");
    }

    #[test]
    fn compact_abbreviates_large_values() {
        assert_eq!(format_compact(77081.0), "77.1k");
        assert_eq!(format_compact(1_250_000.0), "1.2M");
        assert_eq!(format_compact(675.91), "675.91");
        assert_eq!(format_compact(0.0841), "0.0841");
    }

    #[test]
    fn unmapped_ids_fall_back_to_uppercase() {
        assert_eq!(symbol_for("bitcoin"), "BTC");
        assert_eq!(symbol_for("some-new-coin"), "SOME_NEW_COIN");
    }

    /// Hits the live CoinGecko API. Run with `cargo test -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live_fetch_returns_quotes() {
        let ids: Vec<String> = ["bitcoin", "ethereum", "solana"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let quotes = fetch(&ids, "usd").await.expect("live fetch failed");
        assert_eq!(quotes.len(), 3, "expected one quote per requested id");
        for q in &quotes {
            assert!(q.price > 0.0, "{} had a non-positive price", q.symbol);
            println!(
                "{:<5} {}{:<12} {}",
                q.symbol,
                currency_prefix("usd"),
                format_amount(q.price),
                q.change.map(|c| format_change(c, false)).unwrap_or_default()
            );
        }
    }

    #[tokio::test]
    #[ignore]
    async fn unknown_id_is_skipped_not_fatal() {
        let ids: Vec<String> = ["bitcoin", "definitely-not-a-coin"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let quotes = fetch(&ids, "usd").await.expect("fetch failed");
        assert_eq!(quotes.len(), 1, "unknown id should be dropped, not fatal");
        assert_eq!(quotes[0].symbol, "BTC");
    }

    #[test]
    fn change_arrow_tracks_direction() {
        assert_eq!(format_change(6.28, false), "▲ 6.28%");
        assert_eq!(format_change(-1.5, true), "▼-1.5%");
        assert_eq!(format_change(0.0, true), "•0.0%");
    }
}
