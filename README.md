# Crypto Prices

Live cryptocurrency prices in the panel of the COSMIC™ desktop, with a seven-day
sparkline per coin.

```
BTC   ╱▔      $77,318   ▲ 5.1%
ETH   ╱▔       $2,444   ▲ 7.2%
SOL   ╱▔       $93.75   ▲ 11.1%
BNB   ╱▔      $694.09   ▲ 9.5%
⟳  just updated              All coins   +
```

## Install

### COSMIC Store

Search for **Crypto Prices**, or:

```bash
flatpak install cosmic io.github.Zetakai.cosmic-ext-applet-crypto
```

The `cosmic` remote ships configured on COSMIC systems. If it is missing:

```bash
flatpak remote-add --if-not-exists --user cosmic https://apt.pop-os.org/cosmic/cosmic.flatpakrepo
```

Then add it in **Settings → Desktop → Panel → Add applet → Crypto Prices**.

### From source

Requires a Rust toolchain.

```bash
git clone https://github.com/Gliana-Labs/cosmic-ext-applet-crypto.git
cd cosmic-ext-applet-crypto
just build-release
just install
```

### cbar plugin

If you already run [cbar](https://github.com/alexandreprates/cbar), `plugins/crypto.60s.sh`
does the same job as a shell script, needing only `curl` and `jq`. Run `./install.sh`
and configure it through `~/.config/cbar/env`.

## Using it

The panel shows one coin at a glance. The popup lists every coin you track.

**Adding coins** — press **+** and type whichever you know:

| You type | Tracks |
|---|---|
| `btc` | bitcoin |
| `ADA` | cardano |
| `hbar` | hedera-hashgraph |
| `cardano` | cardano |

The input is checked as a CoinGecko id first and only falls back to search if that
finds nothing. Search results prefer an exact ticker match ranked by market cap, so
`btc` resolves to Bitcoin rather than to a wrapped derivative that merely contains
the string. A typo is rejected before anything is saved.

The coin symbol links to its CoinGecko page; **All coins** opens the market list.

## Configuration

Settings live in `~/.config/cosmic/io.github.Zetakai.cosmic-ext-applet-crypto/v1/`.

| Key | Default | Meaning |
|---|---|---|
| `coins` | `["bitcoin","ethereum","binancecoin","ripple","solana"]` | CoinGecko slugs |
| `currency` | `"usd"` | Fiat code prices are quoted in |
| `panel_coin` | `"bitcoin"` | Which coin the panel label tracks |
| `panel_style` | `Icon` | `Icon`, `Compact`, `Minimal`, or `Full` |
| `refresh_secs` | `60` | Seconds between refreshes, clamped to 30–3600 |

### Panel width

The panel label is the only part that costs screen space, so it is tuned separately
from the popup, which always shows full precision.

| `panel_style` | Renders | Width |
|---|---|---|
| `Icon` (default) | icon + `$77.3k ▲5.1%` | 12 chars + icon |
| `Compact` | `$77.3k ▲5.1%` | 12 chars |
| `Minimal` | `$77.3k` | 6 chars |
| `Full` | `BTC $77,318 ▲ 5.10%` | 19 chars |

On a panel anchored to the left or right edge the applet's width *is* the panel's
thickness, so a text label would force the whole bar wider. The default `Icon` style
therefore renders the icon alone there, with prices in the popup. Choosing a text
style explicitly overrides that.

## Behaviour

- **Startup costs no request.** The last good prices are cached under
  `$XDG_CACHE_HOME/cosmic-ext-applet-crypto/` and shown immediately; the network is
  only touched if that cache is older than the refresh interval. Restarting the panel
  therefore does not spend an API call.
- **Failed refresh** — the last good prices stay on screen, marked `(stale)`, rather
  than the panel going blank.
- **Rate limited (HTTP 429)** — reported as such, with the cached prices left in place.
- **Unknown coin id** — dropped from the list; every other coin still renders.
- **Decimals scale with price**, so sub-dollar coins stay readable:
  `$77,318` · `$93.75` · `$0.0841`.

## Colours

The sparkline and the percentage take the desktop theme's success and destructive
colours, so they follow light, dark, and your accent.

They are measured over different windows on purpose: the percentage is the 24h move,
the line is the whole week. A coin can be green for the day and red for the week, and
showing that is more honest than picking one.

## Data source

[CoinGecko](https://www.coingecko.com/en/api) `coins/markets`. No API key. A single
request returns the price, the 24h change, the ticker, and a seven-day sparkline for
every tracked coin. The applet contacts `api.coingecko.com` and nothing else.

Sparklines arrive as 168 hourly samples and are thinned to 48 points — more detail
than a 56px graph can render. The last sample is always kept so the line ends at the
current price.

Note that CoinGecko's edge rejects requests without a `User-Agent`, so the applet
sets one explicitly.

## Development

```bash
cargo test                          # formatting, parsing, cache, search ranking
cargo test -- --ignored --nocapture # live API checks, hits the network
```

Packaging notes, including how to publish an update to the COSMIC Store, are in
[PUBLISHING.md](PUBLISHING.md).

## Naming

Named `cosmic-ext-applet-crypto` rather than `cosmic-applet-crypto` because
[COSMIC's trademark policy](https://github.com/pop-os/cosmic-epoch/blob/master/TRADEMARK.md)
reserves the `cosmic-` namespace for official COSMIC software and directs
third-party applets to `cosmic-ext-`.

COSMIC is a trademark of System76. This is a third-party applet and is not affiliated
with, endorsed by, or sponsored by System76.

## License

MIT © [Gliana Labs](https://glianalabs.com)
