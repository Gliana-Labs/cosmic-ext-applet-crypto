// SPDX-License-Identifier: MIT

use cosmic::cosmic_config::{self, cosmic_config_derive::CosmicConfigEntry, CosmicConfigEntry};
use serde::{Deserialize, Serialize};

/// How much detail the panel label carries. Panel width is scarce, so this is tuned
/// separately from the popup, which always shows full precision.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum PanelStyle {
    /// Icon + compact price + short change: `▲ $77.1k ▲6.3%`
    #[default]
    Icon,
    /// Compact price + short change, no icon.
    Compact,
    /// Compact price only — the narrowest option.
    Minimal,
    /// Ticker + grouped price + two-decimal change.
    Full,
}

#[derive(Debug, Clone, CosmicConfigEntry, Eq, PartialEq)]
#[version = 1]
pub struct Config {
    /// CoinGecko coin slugs to track.
    pub coins: Vec<String>,
    /// Fiat currency code prices are quoted in.
    pub currency: String,
    /// Which coin the panel label shows. Falls back to the first coin if unset or
    /// no longer in `coins`.
    pub panel_coin: String,
    /// How much detail the panel label carries.
    pub panel_style: PanelStyle,
    /// Seconds between refreshes. Clamped on read so a bad value cannot hammer the API.
    pub refresh_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            coins: ["bitcoin", "ethereum", "binancecoin", "ripple", "solana"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            currency: "usd".to_owned(),
            panel_coin: "bitcoin".to_owned(),
            panel_style: PanelStyle::Icon,
            refresh_secs: 60,
        }
    }
}

impl Config {
    /// The coins to display, falling back to the defaults when the stored list is
    /// empty. Removing every coin used to persist `[]`, which left the applet blank
    /// with no way back — an empty list is treated as "unset" rather than a choice.
    pub fn effective_coins(&self) -> Vec<String> {
        if self.coins.is_empty() {
            Self::default().coins
        } else {
            self.coins.clone()
        }
    }

    /// CoinGecko's public tier is rate limited, so never poll faster than 30s.
    pub fn refresh_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.refresh_secs.clamp(30, 3600))
    }

    /// The coin the panel should display, tolerating a stale `panel_coin`.
    pub fn effective_panel_coin(&self) -> Option<String> {
        let coins = self.effective_coins();
        coins
            .iter()
            .find(|c| **c == self.panel_coin)
            .or_else(|| coins.first())
            .cloned()
    }
}
