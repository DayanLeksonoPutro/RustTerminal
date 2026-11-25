# AGENTS.md

Instructions for AI coding agents (Codex, Claude Code, Cursor, Gemini CLI, Copilot, Aider, …)
working in this repository. Humans: see `MECHANICS.md` for the behaviour spec.

## Project

**Rust Terminal** (`rust-terminal`) — a cross-platform (Windows / macOS / Linux) trading terminal
written in Rust with egui/eframe. It has a Market Watch, Navigator, charts (all timeframes M1–MN1),
16 built-in indicators, drawing objects, manual trading on a **simulated** demo account, a code
editor, a strategy tester, and a **home-made MQL5 interpreter** that runs custom indicators, expert
advisors (EAs) and scripts from `.mq5` source.

Hard limits (by design, do not try to "fix"):
- `.ex5` files cannot run — the format is encrypted. They are listed with a lock and an explanation.
- No real broker login/protocol. Accounts and prices are simulated.

## Commands

Run everything from this directory (`app/`, where `Cargo.toml` lives).

```sh
cargo build                     # debug build (opt-level 1, deps at 2)
cargo run                       # start the GUI
cargo run -- --screenshot out.png   # open window, capture after ~120 frames to PNG, exit (needs a display)
cargo test                      # engine tests (must stay green)
cargo clippy --all-targets      # lint
cargo fmt                       # format (default rustfmt settings)
```

Toolchain: stable Rust, edition 2021. No extra system deps beyond what eframe (glow, x11, wayland)
needs on Linux.

Set `RUST_TERMINAL_DATA=/some/tmp/dir` when running the app in tests or experiments so you do not
touch the user's real data folder (see *Runtime data* below).

## Layout

```
src/
  main.rs        entry point, window setup, --screenshot flag
  app.rs         top-level app state and update loop
  ui.rs          menus, toolbars, panels (Market Watch, Navigator, Toolbox tabs)
  chart.rs       chart rendering, scaling, indicator plots, object drawing
  dialogs.rs     login / new account / new order / indicator-properties dialogs
  editor.rs      MetaEditor-style code editor + Compile
  indicators.rs  the 16 built-in indicators (MA, MACD, RSI, Bollinger, …)
  objects.rs     chart objects (lines, trend, Fibonacci, rectangle, text)
  market.rs      simulated symbols, tick generator, bar history, Timeframe
  account.rs     simulated account: positions, orders, deals, margin, SL/TP, stop-out
  host.rs        bridge between the MQL engine and the live market/account (LiveHost)
  tester.rs      strategy tester engine (TestRun, TickMode)
  tester_ui.rs   strategy tester panel and report
  storage.rs     data folder, MQL5 folder layout, accounts.json persistence
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
    interp/calls.rs  implementation of every builtin function (big `match name`)
MECHANICS.md     behaviour spec: defaults, formulas, runtime rules, account/tester rules
mechanics.json   machine-readable version of the same values
shots/           reference screenshots
```

## How things fit together

- The MQL engine never touches the UI or market directly. It talks through the `Host` trait
  (`mql/interp.rs`). `host.rs::LiveHost` implements it for the live chart; `tester.rs` implements it
  for back-testing. New capabilities that need market/account data go on `Host`, then into both
  implementations.
- Adding an MQL builtin function: (1) add its name to `BUILTIN_FUNCS` in `mql/builtins.rs` (otherwise
  `check.rs` rejects it at compile time), (2) implement it in `mql/interp/calls.rs`, (3) add constants /
  enums it needs to `builtins.rs`, (4) cover it in `tests.rs`.
- Adding a built-in indicator: `indicators.rs` + the Navigator list in `ui.rs`; record its defaults
  and source in `MECHANICS.md` and `mechanics.json`.
- Unsupported language features (currently `class`, templates) must fail with a clear
  `CompileError`, never a panic.

## Rules

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
`accounts.json`, `.samples_v1` marker.

## Outside this directory

The parent folder (`../`) holds files from the tool that generated this project
(`craft.json`, `feed.json`, `agent-session.jsonl`, `build-activity.log`, `checks/`, `inbox/`,
`investigation.cany/` — reverse-engineering notes about the reference `terminal64.exe`). Read them
for background if useful; do not edit or delete them, and do not commit them.

## Known gaps / ideas

- MQL5 `class` and template support (would let many internet EAs run unchanged).
- Live prices from a free public feed (e.g. crypto) instead of the simulator.
- Mechanics values are not yet measured against the original at runtime.
