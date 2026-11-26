# Rust Terminal

A cross-platform (macOS / Windows / Linux, Android build available) trading terminal written
in Rust with [egui](https://github.com/emilk/egui). It has charts, built-in indicators, a
demo account, a strategy tester, and a built-in **MQL5 interpreter** that runs custom
indicators, expert advisors (EAs) and scripts straight from `.mq5` source code, including the
MetaTrader 5 Standard Library (`Trade.mqh`, `Indicators/*`, `Arrays/*`, ...).

![Rust Terminal screenshot](docs/screenshot.png)

> **Note:** Accounts and order execution are **simulated** (paper trading). Prices are real
> where you import history (CSV or straight from your MetaTrader 5) and for crypto symbols
> with live public market data; everything else is simulated.

## Features

- **Market Watch**: EURUSD, GBPUSD, USDJPY, USDCHF, AUDUSD, USDCAD, NZDUSD, XAUUSD, BTCUSD,
  plus every symbol exported from your MetaTrader 5 (with the broker's digits, contract size
  and tick value). BTCUSD and other crypto symbols get **live prices** from public market data.
- **Real history**: File > History Data imports CSV (MetaTrader 5/4 exports, Dukascopy,
  unix-time CSV) or everything the bundled `RustTerminal_Export` script exports from your
  MetaTrader 5. Higher timeframes are built from lower ones.
- **Charts**: candles, bars or line, all timeframes from M1 to MN1, several chart tabs, zoom,
  chart shift, crosshair and one-click trading (BUY/SELL panel).
- **38 built-in indicators** for programs (iMA, iRSI, iMACD, iBands, iStochastic, iATR, iCCI,
  iADX, iADXWilder, iSAR, iIchimoku, iAlligator, iGator, iFractals, iAO, iAC, iDEMA, iTEMA,
  iTriX, iAMA, iFrAMA, iVIDyA, iOsMA, iMFI, iOBV, iAD, iChaikin, iForce, iRVI, iStdDev,
  iDeMarker, iBearsPower, iBullsPower, iBWMFI, iEnvelopes, iMomentum, iWPR, iVolumes); their
  values match the platform's reference implementations bar for bar (see MECHANICS.md).
  16 of them can also be added to charts from the Navigator.
- **Drawing objects**: horizontal/vertical lines, trend lines, Fibonacci retracement,
  rectangles, text and arrows.
- **Simulated demo account**: market and pending orders, stop loss / take profit, margin,
  margin call and stop out, hedging mode, trade history and journal.
- **MetaEditor-style code editor**: write `.mq5` files and compile them, with errors reported
  by line.
- **MQL5 engine**: runs custom indicators (`OnCalculate`), expert advisors (`OnInit`,
  `OnTick`, `OnTimer`, `OnTrade`, …) and scripts (`OnStart`) on the live chart.
- **Strategy Tester**: back-test EAs with "every tick", "1 minute OHLC" or "open prices only"
  modelling, and get a report with net profit, profit factor, expected payoff, drawdown and
  trade counts. With imported M1 history the test runs on real minutes and EAs can read any
  timeframe.

- **MCP server for AI coding agents**: `rust-terminal mcp` lets Claude Code, opencode, Codex,
  Gemini CLI, Cursor, … write, compile and run custom indicators and EAs, back-test them and
  analyse charts (with PNG chart images). Setup: [docs/MCP.md](docs/MCP.md).

## Getting started

### Requirements

- [Rust](https://rustup.rs/) (stable, edition 2021)
- On Linux, the usual libraries for an OpenGL window (X11 or Wayland). For example on
  Debian/Ubuntu:

  ```sh
  sudo apt install libx11-dev libxkbcommon-dev libgl1-mesa-dev libwayland-dev
  ```

### Build and run

```sh
git clone https://github.com/DayanLeksonoPutro/RustTerminal.git
cd RustTerminal
cargo run --release
```

Other commands:

```sh
cargo build                          # debug build
cargo test                           # engine tests
cargo run -- --screenshot out.png    # open the window, save a screenshot, then exit
tools/macos/bundle.sh                # macOS: "Rust Terminal.app" + .dmg (UNIVERSAL=1 for Intel too)
tools/android/build.sh               # Android: signed release APK (needs SDK, NDK, cargo-apk)
```

Compatibility checks against a real MQL5 folder (for example your MetaTrader 5 data folder):

```sh
CORPUS="/path/to/MQL5" cargo test corpus -- --ignored --nocapture                # compile + run every program
CORPUS="/path/to/MQL5" cargo test reference_indicators -- --ignored --nocapture  # indicator values vs. reference
```

### Real data from MetaTrader 5

1. In Rust Terminal: File > History Data > **Install Export Script** (copies
   `RustTerminal_Export.mq5` into every MetaTrader 5 found, including Wine on macOS).
2. In MetaTrader 5: Navigator > Scripts > RustTerminal_Export, choose symbols/timeframe/days.
3. In Rust Terminal: File > **Import from MetaTrader 5**.

## Writing your own programs

On first start the terminal creates an `MQL5` folder and copies some example programs into it:

| Platform | Data folder |
|---|---|
| Windows | `%APPDATA%\RustTerminal` |
| macOS | `~/Library/Application Support/RustTerminal` |
| Linux | `$XDG_DATA_HOME/RustTerminal` (or `~/.local/share/RustTerminal`) |

Set the `RUST_TERMINAL_DATA` environment variable to use another folder.

```
MQL5/
  Indicators/   custom indicators
  Experts/      expert advisors
  Scripts/      scripts
  Include/      files for #include
```

Put your `.mq5` files in these folders (or create a new one from a template in
**MetaEditor**), then double-click a program in the **Navigator** to attach it to the active
chart. Trading from an EA needs the
**Algo Trading** button to be on.

Included examples:

- Indicators: *Smoothed Price Line*, *RSI Color Histogram*, *Price Channel*,
  *Heikin Ashi Candles*
- Expert advisors: *MA Crossover EA*, *RSI Reversal EA*
- Scripts: *Close All Positions*, *Draw Day Levels*

A minimal indicator:

```mql5
#property indicator_chart_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrGold

input int InpPeriod = 14;
double Buffer[];

int OnInit()
{
   SetIndexBuffer(0, Buffer, INDICATOR_DATA);
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total, const int prev_calculated, const int begin,
                const double &price[])
{
   for (int i = MathMax(prev_calculated - 1, InpPeriod); i < rates_total; i++)
   {
      double sum = 0;
      for (int k = 0; k < InpPeriod; k++) sum += price[i - k];
      Buffer[i] = sum / InpPeriod;
   }
   return rates_total;
}
```

### What the MQL5 engine supports

- Classes and structs with methods, constructors/initializer lists, destructors, inheritance,
  virtual methods, operators, pointers (`new`/`delete`), static members, interfaces, unions,
  function and class templates, typedefs and function pointers
- Preprocessor: `#define` with parameters, `#` and `##`, `#ifdef`/`#ifndef`/`#else`,
  `#include` (relative and library paths), `__FUNCTION__`, `__LINE__`, ...
- The real MetaTrader 5 Standard Library when its `Include` folder is present (copy your
  terminal's `MQL5/Include` into the data folder); built-in CTrade/CPositionInfo/... otherwise
- Indicator buffers and plots, `CopyBuffer` / `CopyRates` / ... by position or by time,
  `iCustom`, `IndicatorCreate`, files (`FileOpen`, CSV/TXT/BIN), global variables, ~600 constants
- Trading functions, positions, orders and history from EAs and scripts
- Runtime errors such as "zero divide" and "array out of range" stop the program instead of
  crashing the terminal, and an endless-loop guard stops runaway handlers

On the 255 programs of a MetaTrader 5 installation (MetaQuotes examples, Standard Library
tests and user EAs) 231 compile and 225 run. Current limits:

- `matrix`/`vector` types, OpenCL and DirectX are not supported.
- Compiled `.ex5` files cannot run: the format is encrypted. Use the `.mq5` source.

## Project layout

```
src/
  lib.rs          modules, desktop start, Android entry (android_main)
  main.rs         desktop entry point
  app.rs          app state and update loop
  ui.rs           menus, toolbars, Market Watch, Navigator, Toolbox
  chart.rs        chart drawing, indicator plots, objects
  indicators.rs   the 16 chart indicators
  indicators_ext.rs  more i-functions (iDEMA, iAO, ...)
  mt5ind.rs       indicator values for programs, following the reference algorithms
  market.rs       symbols, ticks and bar history (simulated, real history or live)
  history.rs      CSV import, timeframes, MetaTrader 5 export/import
  feed.rs         live public market data (background thread)
  data_ui.rs      History Data window
  account.rs      simulated account: orders, positions, margin, SL/TP
  editor.rs       code editor and compiler
  tester.rs       strategy tester engine
  tester_ui.rs    strategy tester panel and report
  host.rs         link between the MQL engine and the live market/account
  storage.rs      data folder and saved accounts
  samples.rs      example programs and templates
  mql/            MQL5 lexer, parser, checker and interpreter
```

[`MECHANICS.md`](MECHANICS.md) describes the indicator defaults, formulas, account rules and
tester rules in detail. [`AGENTS.md`](AGENTS.md) has notes for contributors and AI coding
agents.

## Roadmap

- Real order execution (a bridge to MetaTrader 5 or a broker API)
- A touch layout for phones (the Android build uses the desktop layout)
- `matrix`/`vector` types

## Contributing

Issues and pull requests are welcome. Before opening a pull request, please run:

```sh
cargo fmt
cargo clippy --all-targets
cargo test
```

## Disclaimer

Rust Terminal is an independent project and is not affiliated with or endorsed by MetaQuotes.
"MQL5" is used only to describe the language of the programs it runs. This software is for
learning and testing strategies; it is not financial advice.
