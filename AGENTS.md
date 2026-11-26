# AGENTS.md

Instructions for AI coding agents (Codex, Claude Code, Cursor, Gemini CLI, Copilot, Aider, …)
working in this repository. Humans: see `MECHANICS.md` for the behaviour spec.

## Project

**Rust Terminal** (`rust-terminal`) — a cross-platform (macOS / Windows / Linux, Android APK)
trading terminal written in Rust with egui/eframe. It has a Market Watch, Navigator, charts (all
timeframes M1–MN1), built-in indicators, drawing objects, paper trading on a demo account, a
code editor, a strategy tester, and a **home-made MQL5 interpreter** that runs custom indicators,
expert advisors (EAs) and scripts from `.mq5` source, including the real MetaTrader 5 Standard
Library (classes, templates, preprocessor are supported).

Data: prices are real where history was imported (CSV or the MT5 export script) and for crypto
symbols with live public market data (`data-api.binance.vision`, no key); otherwise simulated.

Hard limits (by design, do not try to "fix"):
- `.ex5` files cannot run — the format is encrypted. They are listed with a lock and an explanation.
- No real broker login/protocol; order execution is simulated (the user chose paper trading for
  now; a bridge to MT5 / broker API is a later step — ask before building it).

## Commands

Run everything from this directory (`app/`, where `Cargo.toml` lives).

```sh
cargo build                     # debug build (opt-level 1, deps at 2)
cargo run                       # start the GUI (live crypto prices start; RUST_TERMINAL_OFFLINE=1 to skip)
cargo run -- mcp                # MCP server for coding agents on stdio (no window), see docs/MCP.md
cargo run -- --screenshot out.png   # open window, capture after ~120 frames to PNG, exit (needs a display)
cargo test                      # unit tests (must stay green, no network)
cargo clippy --all-targets      # lint
cargo fmt                       # format (default rustfmt settings)

# compatibility against a real MQL5 folder (e.g. the user's MetaTrader 5 data folder)
CORPUS=/path/to/MQL5 cargo test corpus -- --ignored --nocapture
CORPUS=/path/to/MQL5 cargo test reference_indicators -- --ignored --nocapture
cargo test live_feed -- --ignored --nocapture      # needs network

tools/macos/bundle.sh           # "Rust Terminal.app" + dmg (UNIVERSAL=1, SIGN_ID, NOTARY_PROFILE)
tools/android/build.sh [debug|install]   # APK via cargo-apk (Android SDK + NDK)
```

On this machine the user's MetaTrader 5 (Wine) MQL5 folder was copied into the data folder
(`~/Library/Application Support/RustTerminal/MQL5`): 255 real programs for the corpus test.
Never commit those files (MetaQuotes copyright).

Toolchain: stable Rust, edition 2021. No extra system deps beyond what eframe (glow, x11, wayland)
needs on Linux.

Set `RUST_TERMINAL_DATA=/some/tmp/dir` when running the app in tests or experiments so you do not
touch the user's real data folder (see *Runtime data* below).

## Layout

```
src/
  lib.rs         module list, run_desktop(), android_main() (NativeActivity)
  main.rs        desktop entry point (calls rust_terminal::run_desktop)
  app.rs         top-level app state and update loop
  ui.rs          menus, toolbars, panels (Market Watch, Navigator, Toolbox tabs)
  chart.rs       chart rendering, scaling, indicator plots, object drawing
  dialogs.rs     login / new account / new order / indicator-properties dialogs
  editor.rs      MetaEditor-style code editor + Compile
  indicators.rs  the 16 chart indicators (drawing; warm-up bars hidden)
  indicators_ext.rs  extra i-functions (iDEMA, iAO, iMFI, …) — kinds and fallbacks
  mt5ind.rs      indicator values for CopyBuffer, step-by-step like the reference .mq5
                 (warm-up 0.0 / first price / EMPTY_VALUE exactly as the reference)
  history.rs     CSV import/parse, bar_open_time (W1 Sunday, MN1 1st), aggregate,
                 storage <data>/history/<SYM>/<TF>.csv, MT5 export script install/import
  feed.rs        live public market data in a background thread (mpsc messages)
  data_ui.rs     File > History Data window
  objects.rs     chart objects (lines, trend, Fibonacci, rectangle, text)
  market.rs      symbols (Source: Simulated | History | Live), ticks, bar series, Timeframe;
                 Market::new() loads real data, Market::new_simulated() for tests
  account.rs     simulated account: positions, orders, deals, margin, SL/TP, stop-out
  host.rs        bridge between the MQL engine and the live market/account (LiveHost)
  tester.rs      strategy tester engine (TestRun, TickMode)
  tester_ui.rs   strategy tester panel and report
  storage.rs     data folder, MQL5 folder layout, accounts.json persistence
  mcp.rs         MCP server (`rust-terminal mcp`, JSON-RPC over stdio, headless): tools to
                 write/compile/run indicators & EAs, back-test, analyse charts (docs/MCP.md)
  mcp_chart.rs   headless PNG chart renderer used by the `chart_image` tool
  samples.rs     bundled example .mq5 programs + New-file templates
  style.rs       colours / theme
  tests.rs       unit tests (cfg(test))
  mql/           MQL5 language engine
    lexer.rs     tokens, CompileError
    parser.rs    parser + `compile(name, src, loader)` -> Program
    ast.rs       AST, Program, ProgramKind
    check.rs     semantic checks (unknown names, etc.)
    builtins.rs  constants, enums, predefined vars, builtin structs/classes, BUILTIN_FUNCS list
    interp.rs    interpreter core: Value, Instance, `Host` trait, events, OnCalculate
    interp/calls.rs  builtin functions (big `match name`), Copy* ranges (series_range)
    interp/extra.rs  more builtins: files, strings/arrays, out-parameters, OpenCL stubs…
MECHANICS.md     behaviour spec: defaults, formulas, runtime rules, account/tester rules
mechanics.json   machine-readable version of the same values
shots/           reference screenshots
```

## How things fit together

- The MQL engine never touches the UI or market directly. It talks through the `Host` trait
  (`mql/interp.rs`). `host.rs::LiveHost` implements it for the live chart; `tester.rs` implements it
  for back-testing. New capabilities that need market/account data go on `Host`, then into both
  implementations.
- Adding an MQL builtin function: (1) add its name to `BUILTIN_FUNCS` or `EXTRA_FUNCS` in
  `mql/builtins.rs` (otherwise `check.rs` rejects it), (2) implement it in `interp/calls.rs` or
  `interp/extra.rs` (reference/out parameters: `extra::out_params`), (3) add constants it needs,
  (4) cover it in `tests.rs`, (5) rerun the corpus test.
- Indicator values seen by programs must keep matching `reference_indicators` (36/37 exact; AMA
  differs because the reference leaves slow SC = 0). Change `mt5ind.rs`, not `indicators.rs`.
- Classes: `Program.classes` (methods, base, own fields), `StructDef.fields` are flattened
  (inherited first). Objects are `Value::Struct` (Rc); pointers `Ty::Ptr`; `this_stack` holds
  the current object; overloads are chosen by argument count, then types (`overload_score`).
- Adding a built-in indicator: `indicators.rs` + the Navigator list in `ui.rs`; record its defaults
  and source in `MECHANICS.md` and `mechanics.json`.
- Unsupported language features (currently `class`, templates) must fail with a clear
  `CompileError`, never a panic.

## Rules

- MCP server (`mcp.rs`): stdout carries only JSON-RPC lines — never `println!` on that path; tool
  failures are `isError` results, not protocol errors. Writes stay inside the MQL5 folder.
- Keep `cargo build` warning-light and `cargo test` passing before you finish.
- The interpreter must not panic on user code: runtime problems become MQL runtime errors
  ("zero divide", "array out of range", endless-loop guard at 30,000,000 statements) and stop the
  program, as described in `MECHANICS.md`.
- Default values in `MECHANICS.md` marked with a source address come from the original program —
  do not change them without a reason. Values marked "estimated" may be improved; update both
  `MECHANICS.md` and `mechanics.json` when you do.
- Keep code cross-platform: no OS-specific APIs without `cfg` guards; paths via `PathBuf`.
- Sample programs in `samples.rs` are original code. Do not paste third-party / MetaQuotes example
  sources into the repo.
- Changing a sample's content requires bumping the `.samples_vN` marker in `storage.rs`, or existing
  users won't receive the update.
- UI copy is English; the user communicates in Indonesian — answer them in Indonesian.

## Runtime data

`storage::data_dir()`:
- `$RUST_TERMINAL_DATA` if set, otherwise
- Windows `%APPDATA%\RustTerminal`, macOS `~/Library/Application Support/RustTerminal`,
  Linux `$XDG_DATA_HOME/RustTerminal` (or `~/.local/share/RustTerminal`).

Inside: `MQL5/{Indicators,Experts,Scripts,Include}` (user programs, samples copied once),
`accounts.json`, `.samples_v1` marker, `history/` (imported bars, `symbols.json` from the broker),
`Common/Files` (FILE_COMMON). Android: the app's internal data path.

## Outside this directory

The parent folder (`../`) holds files from the tool that generated this project
(`craft.json`, `feed.json`, `agent-session.jsonl`, `build-activity.log`, `checks/`, `inbox/`,
`investigation.cany/` — reverse-engineering notes about the reference `terminal64.exe`). Read them
for background if useful; do not edit or delete them, and do not commit them.

## Known gaps / ideas

- Real order execution (MT5 bridge EA via Common Files, or broker API) — ask the user first.
- `matrix`/`vector` types; GUI panels (Controls/Canvas) need chart object rendering.
- Android: builds and packages (tools/android/build.sh) but uses the desktop layout and was not
  run on a device yet; a touch layout is future work. EAs running 24/7 need a headless engine.
- CopyBuffer and indicator plot shifts (iMA ma_shift, Alligator) — unshifted for now.
- MT5 exports use broker server time; live/simulated ticks use UTC.
