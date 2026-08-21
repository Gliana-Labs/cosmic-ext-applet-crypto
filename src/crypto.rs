// SPDX-License-Identifier: MIT

//! Quote fetching and number formatting.
//!
//! Prices come from CoinGecko's public `simple/price` endpoint, which needs no API
//! key. One request covers every tracked coin.

use serde::Deserialize;

/// `coins/markets` returns price, 24h change, the ticker, and a 7-day sparkline in
/// a single request. Using it for everything keeps the displayed numbers
/// self-consistent — note its `price_change_percentage_24h` uses a different 24h
/// reference than `simple/price`, so the two endpoints disagree slightly.
const API: &str = "https://api.coingecko.com/api/v3/coins/markets";

/// CoinGecko's edge rejects requests that arrive without a User-Agent, so one has to
/// be set explicitly — reqwest sends none by default.
const USER_AGENT: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/Zetakai/cosmic-applet-crypto)"
);

/// Sparkline points kept after downsampling. 168 hourly samples is far more detail
/// than a 64px-wide graph can show.
const SPARK_POINTS: usize = 48;

/// One coin's price in the display currency, its 24h move, and a 7-day price series.
#[derive(Debug, Clone, PartialEq)]
pub struct Quote {
    pub id: String,
    pub symbol: String,
    pub price: f64,
    pub change: Option<f64>,
    /// Downsampled 7-day price series, oldest first. Empty if the API omitted it.
    pub sparkline: Vec<f64>,
}

#[derive(Deserialize)]
struct Market {
    id: String,
    symbol: String,
    current_price: Option<f64>,
    price_change_percentage_24h: Option<f64>,
    #[serde(default)]
    sparkline_in_7d: Sparkline,
}

#[derive(Deserialize, Default)]
struct Sparkline {
    #[serde(default)]
    price: Vec<f64>,
}

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
        "{API}?vs_currency={vs}&ids={}&sparkline=true&price_change_percentage=24h",
        ids.join(",")
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

    let markets = response
        .json::<Vec<Market>>()
        .await
        .map_err(|e| format!("could not parse response: {e}"))?;

    // Preserve the caller's ordering rather than the API's.
    let quotes = ids
        .iter()
        .filter_map(|id| {
            let market = markets.iter().find(|m| &m.id == id)?;
            Some(Quote {
                id: id.clone(),
                symbol: market.symbol.to_uppercase(),
                price: market.current_price?,
                change: market.price_change_percentage_24h,
                sparkline: downsample(&market.sparkline_in_7d.price, SPARK_POINTS),
            })
        })
        .collect();

    Ok(quotes)
}

/// Evenly thins `values` to at most `target` points, always keeping the last one so
/// the graph ends at the current price.
fn downsample(values: &[f64], target: usize) -> Vec<f64> {
    if values.len() <= target || target == 0 {
        return values.to_vec();
    }
    let step = values.len() as f64 / target as f64;
    let mut out: Vec<f64> = (0..target)
        .map(|i| values[((i as f64 * step) as usize).min(values.len() - 1)])
        .collect();
    if let (Some(last), Some(actual)) = (out.last_mut(), values.last()) {
        *last = *actual;
    }
    out
}

/// Renders a price series as a standalone SVG polyline, sized `width` x `height`.
///
/// Returned as bytes for `icon::from_svg_bytes`, which avoids pulling in the canvas
/// feature just to draw a line. Colour tracks direction over the window, so it can
/// differ from the 24h arrow — a coin can be up on the day and down on the week.
pub fn sparkline_svg(prices: &[f64], width: u32, height: u32) -> Option<Vec<u8>> {
    if prices.len() < 2 {
        return None;
    }

    let min = prices.iter().copied().fold(f64::INFINITY, f64::min);
    let max = prices.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !min.is_finite() || !max.is_finite() {
        return None;
    }

    // Inset by the stroke width so the line is never clipped at the edges.
    let stroke = 1.5_f64;
    let span = (max - min).max(f64::EPSILON);
    let usable_h = f64::from(height) - stroke * 2.0;
    let usable_w = f64::from(width) - stroke * 2.0;

    let points: Vec<String> = prices
        .iter()
        .enumerate()
        .map(|(i, price)| {
            let x = stroke + usable_w * (i as f64) / ((prices.len() - 1) as f64);
            // SVG y grows downward, so the highest price maps to the smallest y.
            let y = stroke + usable_h * (1.0 - (price - min) / span);
            format!("{x:.2},{y:.2}")
        })
        .collect();

    let rising = prices.last() >= prices.first();
    let colour = if rising { "#4ade80" } else { "#f87171" };

    Some(
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}"><polyline points="{}" fill="none" stroke="{colour}" stroke-width="{stroke}" stroke-linecap="round" stroke-linejoin="round"/></svg>"#,
            points.join(" ")
        )
        .into_bytes(),
    )
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
    fn downsample_thins_but_keeps_the_latest_value() {
        let series: Vec<f64> = (0..168).map(f64::from).collect();
        let thinned = downsample(&series, 48);
        assert_eq!(thinned.len(), 48);
        assert_eq!(thinned[0], 0.0, "should start at the oldest sample");
        assert_eq!(*thinned.last().unwrap(), 167.0, "must end at the current price");
    }

    #[test]
    fn downsample_leaves_short_series_alone() {
        let series = vec![1.0, 2.0, 3.0];
        assert_eq!(downsample(&series, 48), series);
    }

    #[test]
    fn sparkline_needs_at_least_two_points() {
        assert!(sparkline_svg(&[], 56, 18).is_none());
        assert!(sparkline_svg(&[1.0], 56, 18).is_none());
        assert!(sparkline_svg(&[1.0, 2.0], 56, 18).is_some());
    }

    #[test]
    fn sparkline_stays_inside_its_viewbox() {
        let series = vec![10.0, 50.0, 30.0, 70.0, 20.0];
        let svg = String::from_utf8(sparkline_svg(&series, 56, 18).unwrap()).unwrap();
        let points = svg
            .split("points=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .expect("polyline points");

        for pair in points.split_whitespace() {
            let (x, y) = pair.split_once(',').expect("x,y pair");
            let (x, y): (f64, f64) = (x.parse().unwrap(), y.parse().unwrap());
            assert!((0.0..=56.0).contains(&x), "x {x} outside viewBox");
            assert!((0.0..=18.0).contains(&y), "y {y} outside viewBox");
        }
    }

    #[test]
    fn sparkline_colour_tracks_direction() {
        let up = String::from_utf8(sparkline_svg(&[1.0, 5.0], 56, 18).unwrap()).unwrap();
        let down = String::from_utf8(sparkline_svg(&[5.0, 1.0], 56, 18).unwrap()).unwrap();
        assert!(up.contains("#4ade80"), "rising series should be green");
        assert!(down.contains("#f87171"), "falling series should be red");
    }

    /// A flat series has zero range; the divide must not produce NaN coordinates.
    #[test]
    fn sparkline_survives_a_flat_series() {
        let svg = String::from_utf8(sparkline_svg(&[42.0; 10], 56, 18).unwrap()).unwrap();
        assert!(!svg.contains("NaN"), "flat series produced NaN: {svg}");
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
            assert!(
                !q.sparkline.is_empty(),
                "{} came back with no sparkline series",
                q.symbol
            );
            assert!(
                q.sparkline.len() <= SPARK_POINTS,
                "{} sparkline was not downsampled: {} points",
                q.symbol,
                q.sparkline.len()
            );
            println!(
                "{:<5} {}{:<12} {:<10} {} spark points",
                q.symbol,
                currency_prefix("usd"),
                format_amount(q.price),
                q.change.map(|c| format_change(c, false)).unwrap_or_default(),
                q.sparkline.len()
            );

            // Write one out large so the rendered shape can be eyeballed.
            if q.id == "bitcoin" {
                let svg = sparkline_svg(&q.sparkline, 240, 72).expect("svg");
                std::fs::write("/tmp/btc-sparkline.svg", &svg).ok();
                let small = sparkline_svg(&q.sparkline, 56, 18).expect("svg");
                std::fs::write("/tmp/btc-sparkline-56.svg", &small).ok();
            }
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
