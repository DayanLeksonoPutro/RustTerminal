# Data sources

Rust Terminal gets live prices and bar history from online market-data services. Every symbol
has a list of sources in priority order. The first source that answers is used. If it fails
five quote requests in a row, the next source takes over. A symbol with no working source is
tried again every two minutes. File > History Data shows each symbol's list, with the active
source in `[brackets]`.

Free sources need no account. The premium sources (Alpaca, Polygon, OANDA) are used only after
you save a key. Some of them have free plans; see the tables below.

## Crypto (free, live)

| Source | Notes |
|---|---|
| Binance | Main source. Live, all timeframes M1 to MN1, 1000 bars per timeframe (`data-api.binance.vision`) |
| Binance.US | Backup mirror of Binance |
| Coinbase | Backup when Binance cannot be reached; 900 bars (M30, H4, W1, MN1 are built from smaller bars) |
| Kraken | Backup; 720 bars per timeframe (MN1 is built from D1) |
| Bybit | Perpetual futures (`BTCUSDT.P`) and a spot backup (linear contracts) |
| BloFin | Perpetual futures (`BLOFIN:BTCUSDT.P`), up to 1440 bars |
| Binance USD-M futures | Main source for perpetual futures (`BTCUSDT.P`) |

Order for spot coins (`BTCUSD`, `ETHUSDT`, ...): Binance → Binance.US → Coinbase → Kraken → Bybit.
Order for perpetuals (`BTCUSDT.P`): Binance USD-M futures → Bybit → BloFin.

## US stocks (free)

| Source | Notes |
|---|---|
| Yahoo | Global stocks. About 10 years of daily history; M1 for the last 7 days, M5–M30 for 60 days, H1 for 2 years. Quotes are the last trade price, delayed |
| Nasdaq | Nasdaq's public API. About 10 years of daily history, today's prices minute by minute, bid/ask while the market is open |

## US stocks (premium, optional)

| Provider | Cost | Notes |
|---|---|---|
| Alpaca | Free (sign up at alpaca.markets) | Live prices through the IEX feed (`"feed": "iex"`), or the full SIP feed with a paid plan (`"sip"`) |
| Polygon | Free = end-of-day only; paid = live | Full market feed. The free plan returns the previous close as the price |

Order for stocks (`AAPL`, `BRK.B`, ...): Alpaca → Polygon → Yahoo → Nasdaq. Alpaca and Polygon
are skipped when they have no key.

## Forex & metals (free)

| Source | Notes |
|---|---|
| Yahoo | All forex pairs (`EURUSD=X`). Gold, silver, platinum and palladium come from COMEX/NYMEX futures (`GC=F`, `SI=F`, `PL=F`, `PA=F`), which trade a little above spot |
| Kraken | Live forex with real bid/ask (EURUSD, GBPUSD, USDJPY, USDCAD, USDCHF, AUDUSD, ...), 720 bars per timeframe |

## Forex & metals (premium, optional)

| Provider | Cost | Notes |
|---|---|---|
| OANDA | Free (demo account at oanda.com) | Live OANDA prices with bid/ask, including XAUUSD spot gold. 5000 bars per timeframe |

Order for forex: OANDA → Yahoo → Kraken. Order for metals: OANDA → Yahoo.

## CME futures: NQ, MNQ, ES, MES

| Source | Notes |
|---|---|
| Yahoo | Delayed about 10 minutes, free (`NQ=F`, continuous front month) |
| Polygon futures | Live with the Advanced plan. The terminal requests the front quarterly contract (for example `NQZ6`), rolling a week before the third Friday |

YM, MYM, RTY, M2K, CL, GC and SI work the same way. Contract values: NQ 20 USD per point,
MNQ 2, ES 50, MES 5.

## Symbol names

Type a name in Market Watch (the "+ add symbol" field) or in File > History Data > Add symbol:

| You type | Kind | Example sources |
|---|---|---|
| `BTCUSD`, `ETHUSDT`, `SOLUSDC` | crypto spot | Binance, Coinbase, ... |
| `BTCUSDT.P` | perpetual future | Binance USD-M, Bybit, BloFin |
| `BLOFIN:BTCUSDT.P` | perpetual on BloFin only | BloFin |
| `AAPL`, `MSFT`, `BRK.B` | US stock | Alpaca, Polygon, Yahoo, Nasdaq |
| `EURUSD`, `USDJPY` | forex | OANDA, Yahoo, Kraken |
| `XAUUSD`, `XAGUSD` | metal | OANDA, Yahoo |
| `NQ`, `MNQ`, `ES`, `MES` (also `NQ1!`) | CME future | Polygon, Yahoo |

A `SOURCE:` prefix pins one source: `BINANCE:`, `BINANCEUS:`, `COINBASE:`, `KRAKEN:`, `BYBIT:`,
`BLOFIN:`, `BINANCEF:`, `YAHOO:`, `NASDAQ:`, `ALPACA:`, `POLYGON:`, `OANDA:`. For example
`KRAKEN:BTCUSD` or `YAHOO:^GSPC`. Added symbols are remembered in `watchlist.json`.

## Adding keys (optional)

In the terminal, open File > History Data > API keys, fill in the fields, then click **Save
Keys**. **Check Sources** tests every source and every saved key: it fetches hourly bars and
a price from each one.

The keys are saved in `feeds.json` in the data folder. On macOS and Linux only your user
account can read the file. You can also write it yourself:

| System | File |
|---|---|
| macOS | `~/Library/Application Support/RustTerminal/feeds.json` |
| Windows | `%APPDATA%\RustTerminal\feeds.json` |
| Linux | `~/.local/share/RustTerminal/feeds.json` |

```json
{
  "alpaca": { "keyId": "PK...", "secretKey": "...", "feed": "iex" },
  "polygon": { "apiKey": "..." },
  "oanda": { "token": "...", "account": "practice" }
}
```

For a live OANDA account, replace `"practice"` with `"live"`.

After saving, run this command to check that the keys work. It does the same as the Check
Sources button:

```
rust-terminal check-feeds
```

## Times

Sources send UTC. The terminal converts every time to its own *server time*. By default that
is the broker zone (GMT+2, or GMT+3 while the US is on daylight saving time), so each week has
five daily bars, as at most MetaTrader brokers. You can change it under File > History Data >
Time zones. TimeGMT() always returns UTC.

## When a source does not answer

Some networks block exchange sites. In Indonesia, for example, the ISP filter answers instead
of Kraken, Bybit, BloFin or Binance.US. The error then says *"the connection was intercepted:
the site is blocked on this network"*. The terminal moves on to the next source. A VPN or
another DNS server unblocks the site. Binance USD-M futures returns HTTP 403 in some regions.

Not yet supported: 1-second bars (Binance has them, but the terminal's smallest timeframe is
M1) and streaming (websocket) prices. Quotes are fetched every 1 s (exchanges, Alpaca, OANDA),
3 s (Polygon), 4 s (Yahoo) or 5 s (Nasdaq).
