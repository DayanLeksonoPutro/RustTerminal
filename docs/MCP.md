# MCP server for coding agents

`rust-terminal mcp` starts a [Model Context Protocol](https://modelcontextprotocol.io) server on
stdin/stdout without opening a window. Coding agents (Claude Code, opencode, Codex CLI, Gemini CLI,
Cursor, …) can then write, compile, run and back-test custom indicators and expert advisors, and
analyse charts, with the same MQL5 engine the terminal uses.

Programs are saved in the terminal's normal `MQL5` folder, so everything an agent creates shows up
in the Navigator of the running app within a couple of seconds.

## Tools

| Tool | What it does |
|---|---|
| `terminal_info` | data / MQL5 folders, program counts, built-in indicators, timeframes |
| `list_programs` | indicators, experts, scripts or Include files |
| `read_program` | source of a `.mq5` / `.mqh` |
| `write_program` | create or replace a program and compile it — errors come back with line, column and the offending code (previous version kept as `.bak`) |
| `new_program` | new indicator / expert / script from the template |
| `compile_program` | check a saved file or unsaved source |
| `list_symbols` | symbols, digits and data source |
| `get_rates` | OHLC bars as CSV |
| `run_indicator` | calculate a custom (`path`) or built-in (`builtin`) indicator, latest values of every plot + Print log |
| `run_backtest` | strategy tester: net profit, profit factor, drawdown, win rate, equity curve, last deals, log |
| `analyze_chart` | SMA20/50/200, RSI, Stochastic, MACD, ADX, ATR, Bollinger, range, support/resistance, observations |
| `chart_image` | PNG candlestick chart with indicators (oscillators in their own pane); returned as an image and saved to a file |

Paths are relative to the MQL5 folder: `Indicators/My Trend.mq5`, `Experts/Breakout EA.mq5`,
`Include/MyLib.mqh`. Writes outside the MQL5 folder are refused.

Every data result has `data_source`: `history` (imported real data), `live` (public exchange data,
crypto only) or `simulated` (generated — not real prices). Import history (File > History Data) to
analyse and back-test forex/gold on real prices.

## Setup

Build once: `cargo build --release` (binary: `target/release/rust-terminal`; in the macOS app
bundle: `Rust Terminal.app/Contents/MacOS/rust-terminal`). Use the absolute path below.

**Claude Code**

```sh
claude mcp add rust-terminal -- /path/to/rust-terminal mcp
# available in every project:
claude mcp add --scope user rust-terminal -- /path/to/rust-terminal mcp
```

**opencode** (`opencode.json` in the project, or `~/.config/opencode/opencode.json`)

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "rust-terminal": { "type": "local", "command": ["/path/to/rust-terminal", "mcp"], "enabled": true }
  }
}
```

**Codex CLI** (`~/.codex/config.toml`)

```toml
[mcp_servers.rust-terminal]
command = "/path/to/rust-terminal"
args = ["mcp"]
```

**Gemini CLI** (`~/.gemini/settings.json`), **Cursor** (`.cursor/mcp.json`), **Claude Desktop**
and most other clients use the same shape:

```json
{ "mcpServers": { "rust-terminal": { "command": "/path/to/rust-terminal", "args": ["mcp"] } } }
```

Environment variables: `RUST_TERMINAL_DATA` (use another data folder), `RUST_TERMINAL_OFFLINE=1`
(no network: crypto symbols use simulated prices).

## Example prompts

- "Buat indikator custom Supertrend di Rust Terminal, compile, lalu jalankan di BTCUSD H1."
- "Back-test `Experts/MA Crossover EA.mq5` di EURUSD H1, lalu coba optimalkan periode MA-nya."
- "Analisis chart XAUUSD H4 dan tunjukkan gambarnya dengan EMA 50, Bollinger dan RSI."

## Testing by hand

```sh
printf '%s\n' \
 '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}' \
 '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"analyze_chart","arguments":{"symbol":"BTCUSD","timeframe":"H4"}}}' \
 | RUST_TERMINAL_DATA=/tmp/rt target/release/rust-terminal mcp
```

Or with the official inspector: `npx @modelcontextprotocol/inspector target/release/rust-terminal mcp`.
