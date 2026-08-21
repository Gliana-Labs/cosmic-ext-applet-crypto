# cosmic-applet-crypto

Live cryptocurrency prices in the COSMIC™ desktop panel.

```
▲ $76.9k ▲6.4%
──────────────────────────────────────
BTC   ╱▔   $76,909    ▲ 6.40%
ETH   ╱▔   $2,411     ▲ 4.20%
BNB   ╱▔   $675.91    ▲ 4.20%
XRP   ╱▔   $1.37      ▲ 11.71%
SOL   ╱▔   $90.69     ▲ 4.40%
Refresh
```

Every row is clickable and opens that coin's CoinGecko page; **All coins** at the
bottom opens the full market list.

Each row carries a 7-day sparkline, drawn green when the week closed up and red when
it closed down — so a coin can show a green arrow for the day beside a red line for
the week.

COSMIC ships no crypto applet, and KDE Plasma widgets do not load in `cosmic-panel`.
This is a native applet built on libcosmic.

## Install

### Native applet (recommended)

Requires a Rust toolchain.

```bash
git clone https://github.com/Zetakai/cosmic-applet-crypto.git
cd cosmic-applet-crypto
just build-release
just install
```

Then add it in **Settings → Desktop → Panel → Add applet → Crypto**. Log out and back
in if it does not appear immediately.

### cbar plugin (no compiler needed)

If you already run [cbar](https://github.com/alexandreprates/cbar), `plugins/crypto.60s.sh`
does the same job as a shell script. It needs `curl` and `jq`.

```bash
./install.sh
```

Configure it through `~/.config/cbar/env` — see the comments at the top of the script.

## Configuration

The applet stores settings via `cosmic-config` at
`~/.config/cosmic/io.github.zetakai.CosmicAppletCrypto/v1/`.

| Key | Default | Meaning |
|---|---|---|
| `coins` | `["bitcoin","ethereum","binancecoin","ripple","solana"]` | CoinGecko slugs — the lowercase name from the coingecko.com URL |
| `currency` | `"usd"` | Fiat code prices are quoted in |
| `panel_coin` | `"bitcoin"` | Which coin the panel label tracks |
| `panel_style` | `Icon` | `Icon`, `Compact`, `Minimal`, or `Full` |
| `refresh_secs` | `60` | Seconds between refreshes, clamped to 30–3600 |

### Panel width

The panel label is the only part that costs screen space, so it is tuned separately
from the popup, which always shows full precision.

| `panel_style` | Renders | Width |
|---|---|---|
| `Icon` (default) | icon + `$77.3k ▲6.5%` | 12 chars + icon |
| `Compact` | `$77.3k ▲6.5%` | 12 chars |
| `Minimal` | `$77.3k` | 6 chars |
| `Full` | `BTC $77,283 ▲ 6.45%` | 19 chars |

### Vertical panels

On a panel anchored to the left or right edge, the applet's width is the panel's
thickness — a text label there would force the whole bar wider. The default `Icon`
style therefore renders the icon alone on vertical panels, with prices in the popup.

Choosing `Compact`, `Minimal`, or `Full` explicitly overrides this, since picking a
text style is accepting the width that comes with it.

## Behaviour

- **Failed refresh** — the last good prices stay on screen, marked `(stale)`, rather
  than the panel going blank.
- **Unknown coin id** — dropped from the list; every other coin still renders.
- **Decimals scale with price**, so sub-dollar coins stay readable:
  `$77,283` · `$91.30` · `$0.0841`.
- **Rate limiting** — `refresh_secs` is clamped to a 30s floor so a bad config cannot
  hammer the public API.

## Data source

[CoinGecko](https://www.coingecko.com/en/api) `coins/markets`. No API key. A single
request returns the price, the 24h change, the ticker, and a 7-day sparkline for
every tracked coin.

Sparklines arrive as 168 hourly samples and are thinned to 48 points — more detail
than a 56px-wide graph can render. The last sample is always kept so the line ends at
the current price.

Note that `coins/markets` computes `price_change_percentage_24h` against a different
24h reference than `simple/price`, so the two endpoints report slightly different
percentages for the same instant. Everything here comes from `coins/markets` so the
figures stay self-consistent.

Note that CoinGecko's edge rejects requests without a `User-Agent`, so the applet
sets one explicitly.

## Development

```bash
cargo test                          # formatting and parsing
cargo test -- --ignored --nocapture # live API check, hits the network
```

## License

MIT
