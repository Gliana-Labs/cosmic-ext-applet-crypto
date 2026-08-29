// SPDX-License-Identifier: MIT

//! Quote fetching and number formatting.
//!
//! Prices come from CoinGecko's public `simple/price` endpoint, which needs no API
//! key. One request covers every tracked coin.

use serde::{Deserialize, Serialize};

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
    " (+https://github.com/Gliana-Labs/cosmic-ext-applet-crypto)"
);

/// Sparkline points kept after downsampling. 168 hourly samples is far more detail
/// than a 64px-wide graph can show.
const SPARK_POINTS: usize = 48;

/// One coin's price in the display currency, its 24h move, and a 7-day price series.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err("rate limited by CoinGecko".to_owned());
    }
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

/// Where the last good response is kept between runs.
///
/// Without this every launch is a cold start that must reach the network before it
/// can show anything, so a single rate-limited request leaves the popup empty.
fn cache_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache")))?;
    Some(base.join("cosmic-ext-applet-crypto").join("quotes.json"))
}

/// Quotes plus when they were taken, so staleness can be judged on load.
#[derive(Serialize, Deserialize)]
pub struct CachedQuotes {
    pub quotes: Vec<Quote>,
    /// Unix seconds at which these were fetched.
    pub fetched_at: u64,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Reads the cached quotes and their age in seconds. Any failure — missing file,
/// unreadable, written by an older version — is simply treated as no cache.
pub fn load_cache() -> Option<(Vec<Quote>, u64)> {
    load_cache_from(&cache_path()?)
}

/// Best-effort write of the latest quotes. A failure here must never affect what is
/// on screen, so the result is discarded.
pub fn save_cache(quotes: &[Quote]) {
    let Some(path) = cache_path() else { return };
    save_cache_to(&path, quotes);
}

// The path is taken explicitly by these two so tests can exercise them without
// mutating XDG_CACHE_HOME, which would race across parallel test threads.

fn load_cache_from(path: &std::path::Path) -> Option<(Vec<Quote>, u64)> {
    let raw = std::fs::read(path).ok()?;
    let cached: CachedQuotes = serde_json::from_slice(&raw).ok()?;
    let age = now_secs().saturating_sub(cached.fetched_at);
    Some((cached.quotes, age))
}

fn save_cache_to(path: &std::path::Path, quotes: &[Quote]) {
    if let Some(dir) = path.parent() {
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    let payload = CachedQuotes { quotes: quotes.to_vec(), fetched_at: now_secs() };
    if let Ok(bytes) = serde_json::to_vec(&payload) {
        let _ = std::fs::write(path, bytes);
    }
}

/// Resolves whatever the user typed into a CoinGecko slug.
///
/// Tries the input as a slug first, since that is exact and costs one request. Only
/// if that finds nothing does it fall back to search, which lets people type the
/// ticker they actually know (`btc`, `ada`) or a display name rather than having to
/// look up CoinGecko's internal id.
pub async fn resolve(query: &str, vs: &str) -> Result<String, String> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Err("nothing to add".to_owned());
    }

    let as_slug = query.replace(' ', "-");
    if let Ok(quotes) = fetch(std::slice::from_ref(&as_slug), vs).await {
        if !quotes.is_empty() {
            return Ok(as_slug);
        }
    }

    let url = format!("https://api.coingecko.com/api/v3/search?query={query}");
    let response = reqwest::Client::new()
        .get(&url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("search failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("CoinGecko returned {}", response.status()));
    }

    let results = response
        .json::<SearchResults>()
        .await
        .map_err(|e| format!("could not parse search: {e}"))?;

    pick_best_match(&results.coins, &query).ok_or_else(|| "no such coin".to_owned())
}

#[derive(Deserialize, Default)]
struct SearchResults {
    #[serde(default)]
    coins: Vec<SearchCoin>,
}

#[derive(Deserialize, Clone)]
struct SearchCoin {
    id: String,
    symbol: String,
    market_cap_rank: Option<u32>,
}

/// Prefers an exact ticker match, then an exact id match, then whatever ranks highest
/// by market cap. Searching `btc` should land on Bitcoin, not on a wrapped derivative
/// that happens to contain the string.
fn pick_best_match(coins: &[SearchCoin], query: &str) -> Option<String> {
    // Unranked coins sort last rather than first.
    let rank = |c: &SearchCoin| c.market_cap_rank.unwrap_or(u32::MAX);

    let exact_symbol = coins
        .iter()
        .filter(|c| c.symbol.to_lowercase() == query)
        .min_by_key(|c| rank(c));
    if let Some(c) = exact_symbol {
        return Some(c.id.clone());
    }

    if let Some(c) = coins.iter().find(|c| c.id.to_lowercase() == query) {
        return Some(c.id.clone());
    }

    coins.iter().min_by_key(|c| rank(c)).map(|c| c.id.clone())
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

/// Whether a price series ended the window higher than it started.
pub fn is_rising(prices: &[f64]) -> bool {
    prices.last() >= prices.first()
}

/// Renders a price series as a standalone SVG polyline, sized `width` x `height`.
///
/// Returned as bytes for `icon::from_svg_bytes`, which avoids pulling in the canvas
/// feature just to draw a line. The colour is passed in rather than fixed here so it
/// can come from the desktop theme's own success and destructive colours, matching
/// the percentage beside it.
///
/// Note the line's direction is over the whole window, so it can disagree with the
/// 24h arrow: a coin can be up on the day and down on the week.
pub fn sparkline_svg(
    prices: &[f64],
    width: u32,
    height: u32,
    colour: &str,
) -> Option<Vec<u8>> {
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
        // Sub-cent coins are padded out to eight decimals, most of which are
        // usually zeros; trimming them keeps the column narrow without losing any
        // significant digits. The two-decimal range keeps its zeros so prices in a
        // normal range stay aligned with each other.
        Some((i, f)) if decimals >= 4 => {
            let trimmed = f.trim_end_matches('0');
            (
                i.to_owned(),
                if trimmed.is_empty() {
                    String::new()
                } else {
                    format!(".{trimmed}")
                },
            )
        }
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

/// Direction glyph on its own, so it can be laid out in a fixed column where the
/// arrows line up instead of drifting with the width of the number beside them.
pub fn change_arrow(change: f64) -> &'static str {
    if change > 0.0 {
        "▲"
    } else if change < 0.0 {
        "▼"
    } else {
        "•"
    }
}

/// The magnitude alone: `6.28%`.
///
/// Unsigned, because the arrow already carries the direction and rendering both
/// gives `▼-1.50%`, which says the same thing twice.
pub fn format_change_value(change: f64, short: bool) -> String {
    let magnitude = change.abs();
    if short {
        format!("{magnitude:.1}%")
    } else {
        format!("{magnitude:.2}%")
    }
}

/// Arrow and magnitude together, for the single-line panel label.
pub fn format_change(change: f64, short: bool) -> String {
    format!(
        "{} {}",
        change_arrow(change),
        format_change_value(change, short)
    )
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
    fn sub_cent_prices_drop_their_padding_zeros() {
        // Real values: SHIB and a hypothetical one ending in zeros.
        assert_eq!(format_amount(0.00000541), "0.00000541");
        assert_eq!(format_amount(0.000005), "0.000005");
        assert_eq!(format_amount(0.05), "0.05");
        // The two-decimal range keeps its zeros so ordinary prices stay aligned.
        assert_eq!(format_amount(675.90), "675.90");
        assert_eq!(format_amount(675.00), "675.00");
    }

    #[test]
    fn high_denomination_currencies_still_group() {
        // IDR prices are the widest realistic case.
        assert_eq!(format_amount(1_360_446_498.0), "1,360,446,498");
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
        assert!(sparkline_svg(&[], 56, 18, "#4ade80").is_none());
        assert!(sparkline_svg(&[1.0], 56, 18, "#4ade80").is_none());
        assert!(sparkline_svg(&[1.0, 2.0], 56, 18, "#4ade80").is_some());
    }

    #[test]
    fn sparkline_stays_inside_its_viewbox() {
        let series = vec![10.0, 50.0, 30.0, 70.0, 20.0];
        let svg = String::from_utf8(sparkline_svg(&series, 56, 18, "#4ade80").unwrap()).unwrap();
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
    fn direction_is_measured_over_the_whole_window() {
        assert!(is_rising(&[1.0, 5.0]));
        assert!(!is_rising(&[5.0, 1.0]));
        // A dip in the middle does not change where the window ended.
        assert!(is_rising(&[1.0, 0.1, 5.0]));
    }

    #[test]
    fn sparkline_uses_the_colour_it_is_given() {
        let svg = String::from_utf8(sparkline_svg(&[1.0, 5.0], 56, 18, "#123456").unwrap()).unwrap();
        assert!(svg.contains("#123456"), "colour should come from the caller");
    }

    /// A flat series has zero range; the divide must not produce NaN coordinates.
    #[test]
    fn sparkline_survives_a_flat_series() {
        let svg = String::from_utf8(sparkline_svg(&[42.0; 10], 56, 18, "#4ade80").unwrap()).unwrap();
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
                let svg = sparkline_svg(&q.sparkline, 240, 72, "#4ade80").expect("svg");
                std::fs::write("/tmp/btc-sparkline.svg", &svg).ok();
                let small = sparkline_svg(&q.sparkline, 56, 18, "#4ade80").expect("svg");
                std::fs::write("/tmp/btc-sparkline-56.svg", &small).ok();
            }
        }
    }

    /// Exercises the real resolver against the live API.
    #[tokio::test]
    #[ignore]
    async fn resolve_accepts_slugs_tickers_and_names() {
        for (input, expected) in [
            ("bitcoin", "bitcoin"),
            ("btc", "bitcoin"),
            ("BTC", "bitcoin"),
            ("ada", "cardano"),
            ("hbar", "hedera-hashgraph"),
        ] {
            let got = resolve(input, "usd").await;
            assert_eq!(
                got.as_deref(),
                Ok(expected),
                "resolving {input:?} gave {got:?}"
            );
            println!("{input:>8} -> {expected}");
        }
    }

    #[tokio::test]
    #[ignore]
    async fn resolve_rejects_nonsense() {
        let got = resolve("zzzznotacoin", "usd").await;
        assert!(got.is_err(), "expected an error, got {got:?}");
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

    fn coin(id: &str, symbol: &str, rank: Option<u32>) -> SearchCoin {
        SearchCoin { id: id.to_owned(), symbol: symbol.to_owned(), market_cap_rank: rank }
    }

    #[test]
    fn exact_ticker_beats_a_higher_ranked_partial_match() {
        // Searching "btc" must not land on a wrapped derivative.
        let coins = vec![
            coin("bitget-wrapped-btc", "BGBTC", Some(316)),
            coin("bitcoin", "BTC", Some(1)),
        ];
        assert_eq!(pick_best_match(&coins, "btc").as_deref(), Some("bitcoin"));
    }

    #[test]
    fn exact_ticker_picks_the_largest_by_market_cap() {
        let coins = vec![
            coin("fake-ada", "ADA", Some(9000)),
            coin("cardano", "ADA", Some(18)),
        ];
        assert_eq!(pick_best_match(&coins, "ada").as_deref(), Some("cardano"));
    }

    #[test]
    fn unranked_coins_do_not_win_by_default() {
        let coins = vec![
            coin("obscure", "OBS", None),
            coin("solana", "SOL", Some(7)),
        ];
        assert_eq!(pick_best_match(&coins, "sol").as_deref(), Some("solana"));
    }

    #[test]
    fn empty_search_results_yield_nothing() {
        assert!(pick_best_match(&[], "btc").is_none());
    }

    fn scratch_cache(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!("cosmic-ext-applet-crypto-test-{}-{tag}", std::process::id()))
            .join("quotes.json")
    }

    #[test]
    fn cache_round_trips_and_reports_age() {
        let path = scratch_cache("roundtrip");
        let quotes = vec![Quote {
            id: "bitcoin".into(),
            symbol: "BTC".into(),
            price: 77_000.0,
            change: Some(6.4),
            sparkline: vec![1.0, 2.0, 3.0],
        }];

        save_cache_to(&path, &quotes);
        let (loaded, age) = load_cache_from(&path).expect("cache should read back");
        assert_eq!(loaded, quotes);
        assert!(age < 5, "freshly written cache reported age {age}s");

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn missing_cache_is_not_an_error() {
        assert!(load_cache_from(&scratch_cache("absent")).is_none());
    }

    #[test]
    fn corrupt_cache_is_not_an_error() {
        let path = scratch_cache("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not json at all").unwrap();
        assert!(
            load_cache_from(&path).is_none(),
            "a corrupt cache must read as absent, not panic"
        );
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn change_arrow_tracks_direction() {
        assert_eq!(change_arrow(6.28), "▲");
        assert_eq!(change_arrow(-1.5), "▼");
        assert_eq!(change_arrow(0.0), "•");
    }

    #[test]
    fn change_value_is_unsigned() {
        // The arrow carries the direction; a minus would repeat it.
        assert_eq!(format_change_value(-1.5, true), "1.5%");
        assert_eq!(format_change_value(-12.34, false), "12.34%");
        assert_eq!(format_change_value(6.28, false), "6.28%");
    }

    #[test]
    fn combined_form_separates_arrow_from_value() {
        assert_eq!(format_change(6.28, false), "▲ 6.28%");
        assert_eq!(format_change(-1.5, true), "▼ 1.5%");
        assert_eq!(format_change(0.0, true), "• 0.0%");
    }
}
