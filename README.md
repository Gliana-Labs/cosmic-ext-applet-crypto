# cbar-crypto

Live cryptocurrency prices in the COSMIC™ desktop panel, as a [cbar](https://github.com/alexandreprates/cbar) plugin.

Optionally quotes stocks too.

```
BTC $77,008 ▲ 6.30%
─────────────────────────────
BTC   $77,008    ▲ 6.30%
ETH   $2,412     ▲ 3.95%
BNB   $674.62    ▲ 3.99%
XRP   $1.37      ▲ 11.01%
SOL   $91.05     ▲ 4.33%
Refresh
```

## Why a cbar plugin

COSMIC has no crypto applet — the community applet collection has none, and KDE Plasma
widgets don't load in `cosmic-panel`. Rather than ship another standalone Rust binary,
this rides on cbar, which already solves panel rendering, scheduling and popup actions.

## Requirements

- [cbar](https://github.com/alexandreprates/cbar) installed and added to your COSMIC panel
- `curl` and `jq`

## Install

```bash
git clone https://github.com/<you>/cbar-crypto.git
install -m 0755 cbar-crypto/plugins/crypto.60s.sh ~/.config/cbar/plugins/
```

The `60s` in the filename is cbar's refresh interval — rename to `crypto.5m.sh` for a
slower poll.

## Configuration

All settings are optional. Put them in `~/.config/cbar/env`:

```sh
# CoinGecko coin ids — the lowercase slug from the coingecko.com URL
CBAR_CRYPTO_IDS="bitcoin,ethereum,binancecoin,ripple,solana"

# Yahoo Finance tickers. Leave empty (the default) for crypto only.
CBAR_CRYPTO_STOCKS=""

# Fiat currencies. The first is what the panel label shows; any others
# appear as indented rows under each coin.
CBAR_CRYPTO_VS="usd"

# Which coin id or stock ticker the panel label tracks.
CBAR_CRYPTO_PANEL="bitcoin"
```

### Multiple currencies

`CBAR_CRYPTO_VS="usd,idr"` renders:

```
BTC   $77,008    ▲ 6.30%
      Rp1,360,446,498
```

### Stocks

`CBAR_CRYPTO_STOCKS="HOOD,AAPL"` appends a stock section, quoted from Yahoo Finance.
Percentages are computed against the previous close, so they sit flat while markets
are shut.

## Data sources

| Source | Used for | Key | Notes |
|---|---|---|---|
| [CoinGecko](https://www.coingecko.com/en/api) `simple/price` | crypto | none | Public tier is rate limited; the 60s default is well inside it |
| Yahoo Finance `v8/finance/chart` | stocks | none | Undocumented endpoint — it can change without notice |

## Behaviour

- **Offline** — the last good response is cached under `$XDG_CACHE_HOME/cbar/` and
  re-rendered with a `(stale)` marker, so the panel label never goes blank.
- **No cache and no network** — shows `markets n/a` with a Retry action.
- **Unknown symbol** — that one row reads `no data`; every other row still renders.
- **Decimals scale with price**, so sub-dollar coins stay readable:
  `$77,008` · `$91.05` · `$0.0841`.

## Adding a coin

Use the slug from the CoinGecko URL — `coingecko.com/en/coins/`**`cardano`** → `cardano`.
Tickers for common coins are mapped in `symbol_for()`; anything unmapped falls back to
the uppercased id.

## License

MIT
