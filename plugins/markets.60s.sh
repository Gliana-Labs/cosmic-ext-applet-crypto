#!/usr/bin/env bash
# cbar plugin: crypto prices (CoinGecko) + stock quotes (Yahoo Finance). No API keys required.
#
# Configure in ~/.config/cbar/env:
#   CBAR_MKT_IDS="bitcoin,ethereum,binancecoin,ripple,solana"  # CoinGecko coin ids
#   CBAR_MKT_STOCKS="HOOD,AAPL"                                 # Yahoo tickers; empty = crypto only
#   CBAR_MKT_VS="usd"                                          # fiat currencies; first shows in the panel
#   CBAR_MKT_PANEL="bitcoin"                                   # coin id or stock ticker for the panel label

set -uo pipefail

IDS="${CBAR_MKT_IDS:-bitcoin,ethereum,binancecoin,ripple,solana}"
STOCKS="${CBAR_MKT_STOCKS-}"
VS="${CBAR_MKT_VS:-usd}"
PANEL_KEY="${CBAR_MKT_PANEL:-${IDS%%,*}}"

CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/cbar"
COIN_CACHE="$CACHE_DIR/markets-coins.json"
STOCK_CACHE="$CACHE_DIR/markets-stocks.json"
mkdir -p "$CACHE_DIR"

stale=""

# ---------- fetch: crypto ----------
coins="$(curl -fsS --max-time 8 -G "https://api.coingecko.com/api/v3/simple/price" \
  --data-urlencode "ids=$IDS" \
  --data-urlencode "vs_currencies=$VS" \
  --data-urlencode "include_24hr_change=true" 2>/dev/null)"

if [[ -n "$coins" ]] && printf '%s' "$coins" | jq -e 'type == "object" and length > 0' >/dev/null 2>&1; then
  printf '%s' "$coins" > "$COIN_CACHE"
elif [[ -r "$COIN_CACHE" ]]; then
  coins="$(cat "$COIN_CACHE")"; stale=" (stale)"
else
  coins="{}"
fi

# ---------- fetch: stocks ----------
# Yahoo's chart endpoint is one request per symbol; results are normalised into the
# same {SYM:{price,change}} shape the crypto path uses.
stocks="{}"
if [[ -n "$STOCKS" ]]; then
  acc="{}"; got_any=0
  IFS=',' read -r -a sym_list <<< "$STOCKS"
  for sym in "${sym_list[@]}"; do
    sym="${sym// /}"; [[ -z "$sym" ]] && continue
    raw="$(curl -fsS --max-time 8 -H 'User-Agent: Mozilla/5.0' \
      "https://query1.finance.yahoo.com/v8/finance/chart/${sym}?interval=1d&range=1d" 2>/dev/null)"
    [[ -z "$raw" ]] && continue
    entry="$(printf '%s' "$raw" | jq -c --arg s "$sym" '
      (.chart.result[0].meta // {}) as $m
      | ($m.regularMarketPrice // empty) as $p
      | ($m.previousClose // $m.chartPreviousClose // empty) as $pc
      | if $p == null then empty
        else {($s): {price: $p, name: ($m.shortName // $s), cur: ($m.currency // "USD"),
                     change: (if $pc and $pc != 0 then (($p - $pc) / $pc * 100) else null end)}}
        end' 2>/dev/null)"
    [[ -z "$entry" ]] && continue
    acc="$(jq -c -s '.[0] * .[1]' <(printf '%s' "$acc") <(printf '%s' "$entry") 2>/dev/null || printf '%s' "$acc")"
    got_any=1
  done
  if [[ "$got_any" -eq 1 ]]; then
    stocks="$acc"; printf '%s' "$stocks" > "$STOCK_CACHE"
  elif [[ -r "$STOCK_CACHE" ]]; then
    stocks="$(cat "$STOCK_CACHE")"; stale=" (stale)"
  fi
fi

if [[ "$coins" == "{}" && "$stocks" == "{}" ]]; then
  echo "markets n/a"
  echo "---"
  echo "No price sources reachable and no cache | disabled=true"
  echo "Retry | refresh=true"
  exit 0
fi

# ---------- helpers ----------
symbol_for() {
  case "$1" in
    bitcoin) echo BTC ;;      ethereum) echo ETH ;;
    binancecoin) echo BNB ;;  ripple) echo XRP ;;
    solana) echo SOL ;;       cardano) echo ADA ;;
    dogecoin) echo DOGE ;;    polkadot) echo DOT ;;
    chainlink) echo LINK ;;   litecoin) echo LTC ;;
    avalanche-2) echo AVAX ;; tron) echo TRX ;;
    cosmos) echo ATOM ;;      tether) echo USDT ;;
    usd-coin) echo USDC ;;
    *) printf '%s' "$1" | tr '[:lower:]-' '[:upper:]_' ;;
  esac
}

currency_prefix() {
  case "${1,,}" in
    usd) echo '$' ;; eur) echo '€' ;; gbp) echo '£' ;;
    jpy) echo '¥' ;; idr) echo 'Rp' ;; *) printf '%s ' "${1^^}" ;;
  esac
}

# Decimals scale with magnitude so sub-dollar coins stay readable, then group thousands.
format_amount() {
  awk -v v="$1" 'BEGIN {
    a = (v < 0 ? -v : v)
    d = (a >= 1000 ? 0 : a >= 1 ? 2 : a >= 0.01 ? 4 : 8)
    s = sprintf("%.*f", d, v)
    neg = (substr(s, 1, 1) == "-"); if (neg) s = substr(s, 2)
    dot = index(s, ".")
    ip = (dot ? substr(s, 1, dot - 1) : s); fp = (dot ? substr(s, dot) : "")
    out = ""
    while (length(ip) > 3) { out = "," substr(ip, length(ip) - 2) out; ip = substr(ip, 1, length(ip) - 3) }
    printf "%s%s%s%s", (neg ? "-" : ""), ip, out, fp
  }'
}

change_arrow() { awk -v c="$1" 'BEGIN { printf "%s %.2f%%", (c > 0 ? "▲" : c < 0 ? "▼" : "•"), c }'; }

coin_q()  { printf '%s' "$coins"  | jq -r --arg id "$1" --arg k "$2" '.[$id][$k] // empty'; }
stock_q() { printf '%s' "$stocks" | jq -r --arg s "$1"  --arg k "$2" '.[$s][$k] // empty'; }

IFS=',' read -r -a id_list <<< "$IDS"
IFS=',' read -r -a vs_list <<< "$VS"
primary_vs="${vs_list[0]}"

# ---------- panel label (first line) ----------
label=""
p_price="$(coin_q "$PANEL_KEY" "$primary_vs")"
if [[ -n "$p_price" ]]; then
  p_change="$(coin_q "$PANEL_KEY" "${primary_vs}_24h_change")"
  label="$(symbol_for "$PANEL_KEY") $(currency_prefix "$primary_vs")$(format_amount "$p_price")"
  [[ -n "$p_change" ]] && label+=" $(change_arrow "$p_change")"
else
  s_price="$(stock_q "$PANEL_KEY" price)"
  if [[ -n "$s_price" ]]; then
    s_change="$(stock_q "$PANEL_KEY" change)"
    label="$PANEL_KEY $(currency_prefix "$(stock_q "$PANEL_KEY" cur)")$(format_amount "$s_price")"
    [[ -n "$s_change" ]] && label+=" $(change_arrow "$s_change")"
  fi
fi
echo "${label:-markets n/a}${stale}"

# ---------- popup ----------
echo "---"
for id in "${id_list[@]}"; do
  id="${id// /}"; [[ -z "$id" ]] && continue
  sym="$(symbol_for "$id")"
  price="$(coin_q "$id" "$primary_vs")"
  if [[ -z "$price" ]]; then
    echo "$sym  no data | disabled=true"
    continue
  fi
  change="$(coin_q "$id" "${primary_vs}_24h_change")"
  line="$sym  $(currency_prefix "$primary_vs")$(format_amount "$price")"
  [[ -n "$change" ]] && line+="  $(change_arrow "$change")"
  echo "$line | href=https://www.coingecko.com/en/coins/$id"

  for vs in "${vs_list[@]:1}"; do
    vs="${vs// /}"; [[ -z "$vs" ]] && continue
    alt="$(coin_q "$id" "$vs")"
    [[ -n "$alt" ]] && echo "    $(currency_prefix "$vs")$(format_amount "$alt") | disabled=true"
  done
done

if [[ -n "$STOCKS" ]]; then
  IFS=',' read -r -a sym_list <<< "$STOCKS"
  for sym in "${sym_list[@]}"; do
    sym="${sym// /}"; [[ -z "$sym" ]] && continue
    price="$(stock_q "$sym" price)"
    if [[ -z "$price" ]]; then
      echo "$sym  no data | disabled=true"
      continue
    fi
    change="$(stock_q "$sym" change)"
    line="$sym  $(currency_prefix "$(stock_q "$sym" cur)")$(format_amount "$price")"
    [[ -n "$change" ]] && line+="  $(change_arrow "$change")"
    echo "$line | href=https://finance.yahoo.com/quote/$sym"
  done
fi

echo "Refresh | refresh=true"
