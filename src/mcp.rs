//! MCP server: lets coding agents (Claude Code, opencode, Codex, Gemini CLI, Cursor, …)
//! write, compile and run custom indicators and expert advisors, back-test them and
//! analyse charts with this terminal's engine — headless, no window.
//!
//! Protocol: Model Context Protocol, JSON-RPC 2.0, one message per line on stdin/stdout.
//! Start with `rust-terminal mcp`. Nothing but protocol messages may go to stdout;
//! diagnostics go to stderr.

use crate::history;
use crate::market::{fmt_time, round_to, Bar, Market, Source, SymbolSpec, Timeframe};
use crate::mql::parser::compile;
use crate::mql::{Instance, Program, ProgramKind};
use crate::storage;
use crate::tester::{TestRun, TesterHost, TickMode};
use serde_json::{json, Value as J};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "Rust Terminal: a trading terminal with a built-in MQL5 engine. Use these tools to create and \
manage custom indicators and expert advisors (EAs) as .mq5 source, check them for compile errors, run them on price \
history, back-test EAs in the strategy tester and analyse charts.\n\
Typical loop: new_program or write_program (returns compile errors with line numbers) -> fix -> run_indicator / \
run_backtest -> chart_image to look at the result. Paths are relative to the MQL5 folder, e.g. \
\"Indicators/My Trend.mq5\" or \"Experts/My EA.mq5\"; files written here show up in the terminal's Navigator.\n\
Engine: MQL5 with classes, templates, the preprocessor and the Standard Library (Trade/Trade.mqh, Indicators/*, \
Arrays/*). Not available: .ex5 files (encrypted), DLL imports, matrix/vector types. Trading is simulated (paper).\n\
Every data result has a data_source field: 'history' (imported real data), 'live' (public exchange data for crypto) \
or 'simulated' (random walk, NOT real prices — say so when you analyse it).";

pub fn run() {
    storage::ensure_layout();
    let mut server = Server::new();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<J>(line) {
            Ok(J::Array(batch)) => {
                let r: Vec<J> = batch.iter().filter_map(|m| server.handle(m)).collect();
                if r.is_empty() {
                    None
                } else {
                    Some(J::Array(r))
                }
            }
            Ok(msg) => server.handle(&msg),
            Err(e) => Some(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":format!("parse error: {}", e)}}),
            ),
        };
        if let Some(r) = reply {
            let _ = writeln!(out, "{}", r);
            let _ = out.flush();
        }
    }
}

struct Server {
    market: Market,
    live: HashMap<(String, Timeframe), (Instant, Vec<Bar>)>,
    agent: Option<ureq::Agent>,
}

struct Loaded {
    spec: SymbolSpec,
    tf: Timeframe,
    bars: Vec<Bar>,
    source: &'static str,
    note: Option<String>,
}

impl Server {
    fn new() -> Server {
        Server {
            market: Market::new(),
            live: HashMap::new(),
            agent: None,
        }
    }

    fn handle(&mut self, msg: &J) -> Option<J> {
        let method = msg.get("method").and_then(|m| m.as_str())?;
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        // notifications get no answer
        let id = id?;
        let result: Result<J, (i64, String)> = match method {
            "initialize" => {
                let asked = params
                    .get("protocolVersion")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let version = if PROTOCOLS.contains(&asked) {
                    asked
                } else {
                    PROTOCOLS[0]
                };
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "rust-terminal", "title": "Rust Terminal", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": INSTRUCTIONS,
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tool_list()})),
            "tools/call" => {
                let name = params
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    self.call(&name, &args)
                }));
                let r = r.unwrap_or_else(|_| Err(format!("internal error while running {}", name)));
                Ok(match r {
                    Ok(content) => json!({"content": content, "isError": false}),
                    Err(e) => json!({"content": [{"type":"text","text": e}], "isError": true}),
                })
            }
            "resources/list" => Ok(json!({"resources": []})),
            "prompts/list" => Ok(json!({"prompts": []})),
            _ => Err((-32601, format!("method not found: {}", method))),
        };
        Some(match result {
            Ok(r) => json!({"jsonrpc":"2.0","id":id,"result":r}),
            Err((code, m)) => json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":m}}),
        })
    }

    fn call(&mut self, name: &str, a: &J) -> Result<Vec<J>, String> {
        match name {
            "terminal_info" => self.terminal_info().map(text_json),
            "list_programs" => list_programs(a).map(text_json),
            "read_program" => read_program(a).map(text_json),
            "write_program" => write_program(a).map(text_json),
            "new_program" => new_program(a).map(text_json),
            "compile_program" => compile_program(a).map(text_json),
            "list_symbols" => self.list_symbols().map(text_json),
            "get_rates" => self.get_rates(a).map(text_json),
            "run_indicator" => self.run_indicator(a).map(text_json),
            "run_backtest" => self.run_backtest(a).map(text_json),
            "analyze_chart" => self.analyze_chart(a).map(text_json),
            "chart_image" => self.chart_image(a),
            _ => Err(format!("unknown tool: {}", name)),
        }
    }

    // ---------- data ----------

    fn load(&mut self, a: &J, default_count: usize) -> Result<Loaded, String> {
        let symbol = a
            .get("symbol")
            .and_then(|s| s.as_str())
            .ok_or("missing 'symbol'")?;
        let tf = parse_tf(a.get("timeframe").and_then(|s| s.as_str()).unwrap_or("H1"))?;
        let count = arg_usize(a, "bars", default_count).clamp(10, 100_000);
        let Some(i) = self.market.symbol_index(symbol) else {
            let names: Vec<&str> = self.market.symbols.iter().map(|s| s.spec.name).collect();
            return Err(format!(
                "unknown symbol '{}'. Available: {}",
                symbol,
                names.join(", ")
            ));
        };
        let spec = self.market.symbols[i].spec.clone();
        let mut note = None;
        let want_live = a.get("live").and_then(|v| v.as_bool()).unwrap_or(true)
            && std::env::var("RUST_TERMINAL_OFFLINE").is_err();
        if self.market.symbols[i].source == Source::Simulated && want_live {
            if let Some(pair) = crate::feed::pair_for(spec.name) {
                match self.fetch_live(spec.name, &pair, tf, spec.digits) {
                    Ok(bars) if !bars.is_empty() => {
                        let n = bars.len();
                        let bars = bars[n.saturating_sub(count)..].to_vec();
                        if count > n {
                            note = Some(format!("live source returns at most {} bars", n));
                        }
                        return Ok(Loaded {
                            spec,
                            tf,
                            bars,
                            source: "live",
                            note,
                        });
                    }
                    Ok(_) => {
                        note = Some("live source returned no bars; using simulated prices".into())
                    }
                    Err(e) => {
                        note = Some(format!(
                            "live prices unavailable ({}); using simulated prices",
                            e
                        ))
                    }
                }
            }
        }
        let source = match self.market.symbols[i].source {
            Source::Simulated => "simulated",
            Source::Live => "live",
            _ => "history",
        };
        let all = self.market.bars(i, tf);
        let bars = all[all.len().saturating_sub(count)..].to_vec();
        if bars.is_empty() {
            return Err(format!("no bars for {} {}", spec.name, tf.label()));
        }
        Ok(Loaded {
            spec,
            tf,
            bars,
            source,
            note,
        })
    }

    fn fetch_live(
        &mut self,
        sym: &str,
        pair: &str,
        tf: Timeframe,
        digits: u32,
    ) -> Result<Vec<Bar>, String> {
        let key = (sym.to_string(), tf);
        if let Some((t, b)) = self.live.get(&key) {
            if t.elapsed() < Duration::from_secs(20) {
                return Ok(b.clone());
            }
        }
        let agent = self.agent.get_or_insert_with(|| {
            ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(10))
                .user_agent("rust-terminal")
                .build()
        });
        let bars = crate::feed::fetch_klines(agent, pair, tf, 1000, digits)?;
        self.live.insert(key, (Instant::now(), bars.clone()));
        Ok(bars)
    }

    // ---------- tools ----------

    fn terminal_info(&mut self) -> Result<J, String> {
        let count = |f: &str| storage::list_programs(f).len();
        Ok(json!({
            "data_dir": storage::data_dir(),
            "mql5_dir": storage::mql_dir(),
            "programs": {"Indicators": count("Indicators"), "Experts": count("Experts"), "Scripts": count("Scripts")},
            "symbols": self.market.symbols.len(),
            "timeframes": Timeframe::ALL.iter().map(|t| t.label()).collect::<Vec<_>>(),
            "builtin_indicators": BUILTINS.iter().map(|b| b.0).collect::<Vec<_>>(),
            "trading": "simulated (paper trading)",
        }))
    }

    fn list_symbols(&mut self) -> Result<J, String> {
        let stored: HashMap<String, Vec<Timeframe>> = history::stored().into_iter().collect();
        let list: Vec<J> = self
            .market
            .symbols
            .iter()
            .map(|s| {
                let src = match s.source {
                    Source::Simulated if crate::feed::pair_for(s.spec.name).is_some() => "live",
                    Source::Simulated => "simulated",
                    Source::Live => "live",
                    _ => "history",
                };
                json!({
                    "symbol": s.spec.name,
                    "description": s.spec.description,
                    "digits": s.spec.digits,
                    "data_source": src,
                    "stored_timeframes": stored.get(s.spec.name).map(|v| v.iter().map(|t| t.label()).collect::<Vec<_>>()).unwrap_or_default(),
                })
            })
            .collect();
        Ok(json!({"symbols": list}))
    }

    fn get_rates(&mut self, a: &J) -> Result<J, String> {
        let d = self.load(a, 100)?;
        let dg = d.spec.digits;
        let mut csv = String::from("time,open,high,low,close,tick_volume\n");
        for b in &d.bars {
            csv.push_str(&format!(
                "{},{:.*},{:.*},{:.*},{:.*},{}\n",
                fmt_time(b.time, false),
                dg as usize,
                b.open,
                dg as usize,
                b.high,
                dg as usize,
                b.low,
                dg as usize,
                b.close,
                b.tick_volume
            ));
        }
        Ok(
            json!({"symbol": d.spec.name, "timeframe": d.tf.label(), "data_source": d.source, "note": d.note, "bars": d.bars.len(), "csv": csv}),
        )
    }

    fn run_indicator(&mut self, a: &J) -> Result<J, String> {
        let d = self.load(a, 500)?;
        let last = arg_usize(a, "last", 20).clamp(1, d.bars.len());
        let ind = compute_indicator(a, &d)?;
        let n = d.bars.len();
        let times: Vec<String> = d.bars[n - last..]
            .iter()
            .map(|b| fmt_time(b.time, false))
            .collect();
        let plots: Vec<J> = ind
            .plots
            .iter()
            .enumerate()
            .map(|(k, p)| {
                let mut series = serde_json::Map::new();
                for (name, data) in p.names.iter().zip(&p.data) {
                    series.insert(
                        name.to_string(),
                        J::Array(
                            data[data.len().saturating_sub(last)..]
                                .iter()
                                .map(|v| num(*v))
                                .collect(),
                        ),
                    );
                }
                json!({"index": k, "label": p.label, "draw_type": p.draw_type, "series": series})
            })
            .collect();
        Ok(json!({
            "indicator": ind.name,
            "symbol": d.spec.name,
            "timeframe": d.tf.label(),
            "data_source": d.source,
            "note": d.note,
            "bars_calculated": n,
            "inputs": ind.inputs,
            "window": if ind.separate { "separate" } else { "chart" },
            "levels": ind.levels,
            "times": times,
            "plots": plots,
            "log": tail(&ind.log, 30),
        }))
    }

    fn run_backtest(&mut self, a: &J) -> Result<J, String> {
        let path = resolve(
            a.get("path")
                .and_then(|s| s.as_str())
                .ok_or("missing 'path'")?,
            "Experts",
        )?;
        let prog = compile_path(&path)?;
        if prog.kind != ProgramKind::Expert {
            return Err(format!(
                "{} is a {}, not an expert advisor (needs OnTick)",
                rel(&path),
                prog.kind.label()
            ));
        }
        let symbol = a
            .get("symbol")
            .and_then(|s| s.as_str())
            .ok_or("missing 'symbol'")?;
        let tf = parse_tf(a.get("timeframe").and_then(|s| s.as_str()).unwrap_or("H1"))?;
        let mode = match a.get("mode").and_then(|s| s.as_str()).unwrap_or("ohlc") {
            "every_tick" => TickMode::EveryTick,
            "open_prices" => TickMode::OpenPrices,
            _ => TickMode::OhlcPoints,
        };
        let deposit = a.get("deposit").and_then(|v| v.as_f64()).unwrap_or(10000.0);
        let leverage = a.get("leverage").and_then(|v| v.as_u64()).unwrap_or(100) as u32;
        let inputs = arg_inputs(a);
        let from = a
            .get("from")
            .and_then(|s| s.as_str())
            .map(parse_date)
            .transpose()?;
        let to = a
            .get("to")
            .and_then(|s| s.as_str())
            .map(parse_date)
            .transpose()?
            .map(|t| t + 86400);
        let in_range = |b: &Bar| from.is_none_or(|f| b.time >= f) && to.is_none_or(|t| b.time < t);
        let timeout = Duration::from_secs(arg_usize(a, "timeout_secs", 120).clamp(5, 600) as u64);

        let si = self
            .market
            .symbol_index(symbol)
            .ok_or_else(|| format!("unknown symbol '{}'", symbol))?;
        let spec = self.market.symbols[si].spec.clone();
        let minutes = if self.market.symbols[si].source == Source::Simulated {
            None
        } else {
            history::load(spec.name, Timeframe::M1).filter(|m| m.len() > 200)
        };
        let loader = program_loader();
        let (mut run, source, note) = match minutes {
            Some(m) => {
                let mut m: Vec<Bar> = m.into_iter().filter(|b| in_range(b)).collect();
                if let Some(n) = a.get("bars").and_then(|v| v.as_u64()) {
                    let agg = history::aggregate(&m, tf);
                    if let Some(start) = agg
                        .len()
                        .checked_sub(n as usize)
                        .and_then(|k| agg.get(k))
                        .map(|b| b.time)
                    {
                        m.retain(|b| b.time >= start);
                    }
                }
                if m.len() < 200 {
                    return Err("not enough one-minute history in the chosen range".into());
                }
                (
                    TestRun::with_minutes(
                        prog,
                        spec.clone(),
                        tf,
                        m,
                        mode,
                        deposit,
                        leverage,
                        &inputs,
                        loader,
                    ),
                    "history",
                    Some("driven by real one-minute history".to_string()),
                )
            }
            None => {
                let mut args = a.clone();
                args["bars"] = json!(a.get("bars").and_then(|v| v.as_u64()).unwrap_or(5000));
                let d = self.load(&args, 5000)?;
                let bars: Vec<Bar> = d.bars.into_iter().filter(|b| in_range(b)).collect();
                if bars.len() < 50 {
                    return Err("not enough bars in the chosen range".into());
                }
                (
                    TestRun::new(
                        prog,
                        spec.clone(),
                        tf,
                        bars,
                        mode,
                        deposit,
                        leverage,
                        &inputs,
                        loader,
                    ),
                    d.source,
                    d.note,
                )
            }
        };
        let started = Instant::now();
        let mut timed_out = false;
        while !run.step(500) {
            if started.elapsed() > timeout {
                timed_out = true;
                break;
            }
        }
        let r = run.report();
        let deals: Vec<J> = run
            .host
            .account
            .deals
            .iter()
            .filter(|d| d.kind != "balance")
            .map(|d| json!({"time": fmt_time(d.time, false), "type": d.kind, "entry": d.entry, "volume": d.volume, "price": d.price, "profit": round_to(d.profit, 2), "comment": d.comment}))
            .collect();
        let step = (run.curve.len() / 40).max(1);
        let curve: Vec<J> = run
            .curve
            .iter()
            .step_by(step)
            .map(|(b, e)| json!([round_to(*b, 2), round_to(*e, 2)]))
            .collect();
        let first = run
            .host
            .all
            .get(run.start_bar)
            .map(|b| fmt_time(b.time, false));
        let last = run.host.all.last().map(|b| fmt_time(b.time, false));
        Ok(json!({
            "expert": rel(&path),
            "symbol": spec.name,
            "timeframe": tf.label(),
            "mode": mode.label(),
            "data_source": source,
            "note": note,
            "period": {"from": first, "to": last},
            "finished": run.done && !timed_out,
            "timed_out": timed_out,
            "error": run.error,
            "inputs": run.inst.inputs_text,
            "report": {
                "initial_deposit": r.initial,
                "net_profit": round_to(r.net, 2),
                "gross_profit": round_to(r.gross_profit, 2),
                "gross_loss": round_to(r.gross_loss, 2),
                "profit_factor": round_to(r.profit_factor, 2),
                "expected_payoff": round_to(r.expected_payoff, 2),
                "max_drawdown": round_to(r.max_dd, 2),
                "max_drawdown_pct": round_to(r.max_dd_pct, 2),
                "trades": r.trades,
                "wins": r.wins,
                "losses": r.losses,
                "win_rate_pct": if r.trades > 0 { round_to(r.wins as f64 * 100.0 / r.trades as f64, 1) } else { 0.0 },
                "largest_win": round_to(r.largest_win, 2),
                "largest_loss": round_to(r.largest_loss, 2),
                "bars": r.bars,
                "ticks": r.ticks,
            },
            "equity_curve": {"columns": ["balance", "equity"], "points": curve},
            "deals_total": deals.len(),
            "last_deals": tail_j(&deals, 30),
            "log": tail(&run.host.log, 40),
        }))
    }

    fn analyze_chart(&mut self, a: &J) -> Result<J, String> {
        let d = self.load(a, 300)?;
        let bars = &d.bars;
        let n = bars.len();
        if n < 30 {
            return Err("need at least 30 bars to analyse".into());
        }
        let dg = d.spec.digits;
        let close: Vec<f64> = bars.iter().map(|b| b.close).collect();
        let last = *bars.last().unwrap();
        let price = last.close;
        let lastv = |v: &[f64]| {
            v.last()
                .copied()
                .filter(|x| x.is_finite() && x.abs() < 1e300)
        };
        let sma = |p: usize| {
            if n >= p {
                lastv(&crate::mt5ind::ima(&close, p, 0))
            } else {
                None
            }
        };
        let (s20, s50, s200) = (sma(20), sma(50), sma(200));
        let b = |name: &str, params: J| {
            builtin_series(name, &params, bars)
                .map(|(_, v)| v)
                .unwrap_or_default()
        };
        let rsi = b("RSI", json!({"period": 14}))
            .first()
            .and_then(|v| lastv(v));
        let atr = b("ATR", json!({"period": 14}))
            .first()
            .and_then(|v| lastv(v));
        let macd = b("MACD", json!({}));
        let (m_main, m_sig) = (
            macd.first().and_then(|v| lastv(v)),
            macd.get(1).and_then(|v| lastv(v)),
        );
        let bb = b("Bands", json!({}));
        let (bb_mid, bb_up, bb_lo) = (
            bb.first().and_then(|v| lastv(v)),
            bb.get(1).and_then(|v| lastv(v)),
            bb.get(2).and_then(|v| lastv(v)),
        );
        let adx = b("ADX", json!({"period": 14}));
        let (adx_v, pdi, mdi) = (
            adx.first().and_then(|v| lastv(v)),
            adx.get(1).and_then(|v| lastv(v)),
            adx.get(2).and_then(|v| lastv(v)),
        );
        let st = b("Stochastic", json!({}));
        let (st_k, st_d) = (
            st.first().and_then(|v| lastv(v)),
            st.get(1).and_then(|v| lastv(v)),
        );

        let hi = bars.iter().map(|b| b.high).fold(f64::MIN, f64::max);
        let lo = bars.iter().map(|b| b.low).fold(f64::MAX, f64::min);
        let chg = |k: usize| {
            if n > k {
                Some((price / close[n - 1 - k] - 1.0) * 100.0)
            } else {
                None
            }
        };

        // swing points -> support / resistance levels
        let w = 3;
        let mut swings_hi = vec![];
        let mut swings_lo = vec![];
        for i in w..n.saturating_sub(w) {
            let win = &bars[i - w..=i + w];
            if win.iter().all(|x| x.high <= bars[i].high) {
                swings_hi.push(bars[i].high);
            }
            if win.iter().all(|x| x.low >= bars[i].low) {
                swings_lo.push(bars[i].low);
            }
        }
        let tol = atr.unwrap_or((hi - lo) / 50.0) * 0.5;
        let levels = cluster(
            swings_hi.iter().chain(swings_lo.iter()).copied().collect(),
            tol,
        );
        let mut res: Vec<&(f64, usize)> = levels.iter().filter(|l| l.0 > price).collect();
        res.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut sup: Vec<&(f64, usize)> = levels.iter().filter(|l| l.0 <= price).collect();
        sup.sort_by(|a, b| b.0.total_cmp(&a.0));
        let lvl = |v: Vec<&(f64, usize)>| {
            v.into_iter()
                .take(3)
                .map(|(p, t)| json!({"price": round_to(*p, dg), "touches": t}))
                .collect::<Vec<_>>()
        };

        // descriptive observations (not trade advice)
        let mut obs = vec![];
        let mut score = 0i32;
        let mut cmp = |name: &str, v: Option<f64>| {
            if let Some(v) = v {
                if price > v {
                    score += 1;
                    obs.push(format!("price above {}", name));
                } else {
                    score -= 1;
                    obs.push(format!("price below {}", name));
                }
            }
        };
        cmp("SMA20", s20);
        cmp("SMA50", s50);
        cmp("SMA200", s200);
        if let (Some(a50), Some(a200)) = (s50, s200) {
            if a50 > a200 {
                score += 1;
                obs.push("SMA50 above SMA200 (bullish structure)".into());
            } else {
                score -= 1;
                obs.push("SMA50 below SMA200 (bearish structure)".into());
            }
        }
        if let (Some(m), Some(s)) = (m_main, m_sig) {
            if m > s {
                score += 1;
                obs.push("MACD above its signal line".into());
            } else {
                score -= 1;
                obs.push("MACD below its signal line".into());
            }
        }
        if let Some(r) = rsi {
            obs.push(match r {
                r if r >= 70.0 => format!("RSI {:.1}: overbought zone (>=70)", r),
                r if r <= 30.0 => format!("RSI {:.1}: oversold zone (<=30)", r),
                r => format!("RSI {:.1}: neutral", r),
            });
        }
        if let Some(x) = adx_v {
            obs.push(if x >= 25.0 {
                format!("ADX {:.1}: trending market", x)
            } else {
                format!("ADX {:.1}: weak trend / ranging", x)
            });
        }
        if let (Some(u), Some(l)) = (bb_up, bb_lo) {
            if price >= u {
                obs.push("price at/above upper Bollinger Band".into());
            } else if price <= l {
                obs.push("price at/below lower Bollinger Band".into());
            }
        }
        let bias = match score {
            s if s >= 3 => "bullish",
            s if s <= -3 => "bearish",
            _ => "mixed / sideways",
        };
        let r = |v: Option<f64>, k: u32| v.map(|x| round_to(x, k));
        Ok(json!({
            "symbol": d.spec.name,
            "timeframe": d.tf.label(),
            "data_source": d.source,
            "note": d.note,
            "bars_analysed": n,
            "last_bar": fmt_time(last.time, false),
            "price": round_to(price, dg),
            "change_pct": {"1_bar": r(chg(1), 3), "10_bars": r(chg(10), 3), "50_bars": r(chg(50), 3)},
            "range": {"high": round_to(hi, dg), "low": round_to(lo, dg), "position_pct": round_to((price - lo) / (hi - lo).max(1e-12) * 100.0, 1)},
            "moving_averages": {"sma20": r(s20, dg), "sma50": r(s50, dg), "sma200": r(s200, dg)},
            "oscillators": {"rsi14": r(rsi, 2), "stoch_k": r(st_k, 2), "stoch_d": r(st_d, 2), "macd": r(m_main, dg + 1), "macd_signal": r(m_sig, dg + 1)},
            "trend_strength": {"adx14": r(adx_v, 2), "plus_di": r(pdi, 2), "minus_di": r(mdi, 2)},
            "volatility": {"atr14": r(atr, dg), "atr_pct": atr.map(|x| round_to(x / price * 100.0, 3)), "bollinger": {"upper": r(bb_up, dg), "middle": r(bb_mid, dg), "lower": r(bb_lo, dg)}},
            "resistance": lvl(res),
            "support": lvl(sup),
            "trend_score": score,
            "bias": bias,
            "observations": obs,
            "disclaimer": "Technical description of the data, not financial advice.",
        }))
    }

    fn chart_image(&mut self, a: &J) -> Result<Vec<J>, String> {
        // extra history in front so indicators are past their warm-up where the chart starts
        let show = arg_usize(a, "bars", 150).clamp(10, 5000);
        let mut args = a.clone();
        args["bars"] = json!(show + 300);
        let d = self.load(&args, show + 300)?;
        let show = show.min(d.bars.len());
        let width = arg_usize(a, "width", 1200).clamp(400, 3000) as u32;
        let height = arg_usize(a, "height", 700).clamp(300, 2000) as u32;
        let mut inds = vec![];
        for spec in a
            .get("indicators")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
        {
            inds.push(compute_indicator(&spec, &d)?);
        }
        // distinct colours for built-ins across the whole chart
        let mut ci = 0;
        for i in inds.iter_mut().filter(|i| i.builtin) {
            for p in i.plots.iter_mut() {
                p.rgb = PALETTE[ci % PALETTE.len()];
                ci += 1;
            }
        }
        let shown = &d.bars[d.bars.len() - show..];
        let png = crate::mcp_chart::render(&d.spec, d.tf, shown, &inds, width, height)?;
        let path = match a.get("save_to").and_then(|s| s.as_str()) {
            Some(p) => PathBuf::from(p),
            None => {
                let dir = storage::data_dir().join("mcp").join("charts");
                let _ = std::fs::create_dir_all(&dir);
                dir.join(format!(
                    "{}_{}_{}.png",
                    d.spec.name,
                    d.tf.label(),
                    crate::market::unix_now()
                ))
            }
        };
        std::fs::write(&path, &png)
            .map_err(|e| format!("cannot save {}: {}", path.display(), e))?;
        let legend: Vec<J> = inds
            .iter()
            .map(|i| json!({"indicator": i.name, "window": if i.separate { "separate pane" } else { "on price chart" }, "lines": i.plots.iter().map(|p| json!({"label": p.label, "color": format!("#{:06x}", p.rgb)})).collect::<Vec<_>>()}))
            .collect();
        let first = shown.first().map(|b| fmt_time(b.time, false));
        let last = shown.last().map(|b| fmt_time(b.time, false));
        let info = json!({
            "saved_to": path,
            "symbol": d.spec.name,
            "timeframe": d.tf.label(),
            "data_source": d.source,
            "note": d.note,
            "bars": show,
            "from": first,
            "to": last,
            "last_close": d.bars.last().map(|b| round_to(b.close, d.spec.digits)),
            "legend": legend,
        });
        Ok(vec![
            json!({"type": "image", "data": base64(&png), "mimeType": "image/png"}),
            json!({"type": "text", "text": serde_json::to_string_pretty(&info).unwrap_or_default()}),
        ])
    }
}

// ---------- indicators ----------

pub struct PlotOut {
    pub label: String,
    pub draw_type: i64,
    pub rgb: u32,
    pub names: Vec<&'static str>,
    pub data: Vec<Vec<f64>>,
}

pub struct IndOut {
    pub name: String,
    pub builtin: bool,
    pub separate: bool,
    pub levels: Vec<f64>,
    pub inputs: J,
    pub plots: Vec<PlotOut>,
    pub log: Vec<String>,
}

/// (name, kind, buffer labels, separate window)
const BUILTINS: [(&str, crate::indicators::IndKind, &[&str]); 16] = {
    use crate::indicators::IndKind::*;
    [
        ("MA", MovingAverage, &["MA"]),
        ("Bands", Bands, &["Middle", "Upper", "Lower"]),
        ("Envelopes", Envelopes, &["Upper", "Lower"]),
        (
            "Ichimoku",
            Ichimoku,
            &["Tenkan", "Kijun", "SpanA", "SpanB", "Chikou"],
        ),
        ("SAR", Sar, &["SAR"]),
        ("ADX", Adx, &["ADX", "+DI", "-DI"]),
        ("Alligator", Alligator, &["Jaw", "Teeth", "Lips"]),
        ("Fractals", Fractals, &["Up", "Down"]),
        ("MACD", Macd, &["MACD", "Signal"]),
        ("RSI", Rsi, &["RSI"]),
        ("Stochastic", Stochastic, &["Main", "Signal"]),
        ("ATR", Atr, &["ATR"]),
        ("CCI", Cci, &["CCI"]),
        ("Momentum", Momentum, &["Momentum"]),
        ("WPR", Wpr, &["WPR"]),
        ("Volumes", Volumes, &["Volume", "Color"]),
    ]
};

const PALETTE: [u32; 6] = [0x1e90ff, 0xff8c00, 0xdc143c, 0x2e8b57, 0x9932cc, 0x8b4513];

fn find_builtin(name: &str) -> Option<usize> {
    let n = name.to_ascii_lowercase().replace([' ', '_', '-'], "");
    let alias = match n.as_str() {
        "sma" | "ema" | "smma" | "lwma" | "movingaverage" | "ima" => "ma",
        "bb" | "bollinger" | "bollingerbands" | "ibands" => "bands",
        "stoch" | "istochastic" => "stochastic",
        "parabolicsar" | "psar" | "isar" => "sar",
        "volume" | "ivolumes" => "volumes",
        "williams" | "williamsr" | "iwpr" => "wpr",
        other => other
            .strip_prefix('i')
            .filter(|s| BUILTINS.iter().any(|b| b.0.eq_ignore_ascii_case(s)))
            .unwrap_or(other),
    }
    .to_string();
    BUILTINS
        .iter()
        .position(|b| b.0.eq_ignore_ascii_case(&alias))
}

fn builtin_series(name: &str, params: &J, bars: &[Bar]) -> Result<(usize, Vec<Vec<f64>>), String> {
    let k = find_builtin(name).ok_or_else(|| {
        format!(
            "unknown built-in indicator '{}'. Known: {}",
            name,
            BUILTINS.iter().map(|b| b.0).collect::<Vec<_>>().join(", ")
        )
    })?;
    let kind = BUILTINS[k].1;
    let mut p = crate::indicators::IndParams::defaults(kind);
    let lname = name.to_ascii_lowercase();
    p.method = match lname.as_str() {
        "ema" => 1,
        "smma" => 2,
        "lwma" => 3,
        _ => p.method,
    };
    let gi = |keys: &[&str]| {
        keys.iter().find_map(|k| {
            params.get(*k).and_then(|v| {
                v.as_f64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
        })
    };
    if let Some(v) = gi(&["period", "p1", "fast", "k_period", "tenkan", "jaw"]) {
        p.p1 = v as i64;
    }
    if let Some(v) = gi(&["p2", "slow", "d_period", "kijun", "teeth"]) {
        p.p2 = v as i64;
    }
    if let Some(v) = gi(&["p3", "signal", "slowing", "senkou", "lips"]) {
        p.p3 = v as i64;
    }
    if let Some(v) = gi(&["method", "ma_method"]) {
        p.method = v as i64;
    }
    if let Some(v) = gi(&["price", "applied_price"]) {
        p.price = v as i64;
    }
    if let Some(v) = gi(&["shift"]) {
        p.shift = v as i64;
    }
    if let Some(v) = gi(&["deviation", "f1", "step"]) {
        p.f1 = v;
    }
    if let Some(v) = gi(&["f2", "maximum"]) {
        p.f2 = v;
    }
    Ok((k, crate::mt5ind::builtin(kind, &p, bars)))
}

fn compute_indicator(a: &J, d: &Loaded) -> Result<IndOut, String> {
    if let Some(name) = a.get("builtin").and_then(|s| s.as_str()) {
        let params = a.get("params").cloned().unwrap_or(json!({}));
        let (k, data) = builtin_series(name, &params, &d.bars)?;
        let (bname, kind, labels) = BUILTINS[k];
        let plots = data
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !(kind == crate::indicators::IndKind::Volumes && *i == 1))
            .map(|(i, v)| PlotOut {
                label: labels.get(i).unwrap_or(&"value").to_string(),
                draw_type: match kind {
                    crate::indicators::IndKind::Volumes => 3,
                    crate::indicators::IndKind::Fractals | crate::indicators::IndKind::Sar => 5,
                    _ => 1,
                },
                rgb: PALETTE[i % PALETTE.len()],
                names: vec!["value"],
                data: vec![v],
            })
            .collect();
        let levels = match kind {
            crate::indicators::IndKind::Rsi => vec![30.0, 70.0],
            crate::indicators::IndKind::Stochastic => vec![20.0, 80.0],
            crate::indicators::IndKind::Cci => vec![-100.0, 100.0],
            crate::indicators::IndKind::Wpr => vec![-80.0, -20.0],
            _ => vec![],
        };
        let shown = if bname == "MA" && name.len() <= 4 {
            name.to_ascii_uppercase()
        } else {
            bname.to_string()
        };
        let args: Vec<String> = params
            .as_object()
            .map(|o| o.iter().map(|(k, v)| format!("{}={}", k, v)).collect())
            .unwrap_or_default();
        let display = if args.is_empty() {
            shown
        } else {
            format!("{}({})", shown, args.join(", "))
        };
        return Ok(IndOut {
            name: display,
            builtin: true,
            separate: kind.separate_window(),
            levels,
            inputs: params,
            plots,
            log: vec![],
        });
    }
    let path = resolve(
        a.get("path")
            .and_then(|s| s.as_str())
            .ok_or("give either 'builtin' (e.g. \"RSI\") or 'path' (e.g. \"Indicators/My.mq5\")")?,
        "Indicators",
    )?;
    let prog = compile_path(&path)?;
    if prog.kind != ProgramKind::Indicator {
        return Err(format!(
            "{} is a {}, not an indicator (needs OnCalculate)",
            rel(&path),
            prog.kind.label()
        ));
    }
    let inputs = arg_inputs(a);
    let mut host = TesterHost::over_bars(d.spec.clone(), d.tf, d.bars.clone(), program_loader());
    let mut inst = Instance::new(prog.clone(), d.spec.name, d.tf, &inputs);
    match inst.init(&mut host) {
        Ok(0) => {}
        Ok(c) => {
            return Err(format!(
                "OnInit returned {} (initialisation failed). Log: {:?}",
                c,
                tail(&host.log, 10)
            ))
        }
        Err(e) => {
            return Err(format!(
                "runtime error in OnInit: {}. Log: {:?}",
                inst.fmt_err(&e),
                tail(&host.log, 10)
            ))
        }
    }
    if let Err(e) = inst.calculate(&mut host) {
        return Err(format!(
            "runtime error in OnCalculate: {}. Log: {:?}",
            inst.fmt_err(&e),
            tail(&host.log, 10)
        ));
    }
    let plots = inst
        .plot_data()
        .into_iter()
        .enumerate()
        .map(|(i, (pr, data))| {
            let c = pr
                .colors
                .first()
                .copied()
                .unwrap_or(PALETTE[i % PALETTE.len()]);
            let names: Vec<&'static str> = match pr.draw_type {
                4 | 6 | 7 => vec!["value1", "value2"],
                8 | 9 => vec!["open", "high", "low", "close"],
                10 | 11 | 12 | 14 => vec!["value", "color_index"],
                13 | 15 => vec!["value1", "value2", "color_index"],
                16 | 17 => vec!["open", "high", "low", "close", "color_index"],
                _ => vec!["value"],
            };
            // MQL colours are 0x00BBGGRR
            let rgb = ((c & 0xff) << 16) | (c & 0xff00) | ((c >> 16) & 0xff);
            PlotOut {
                label: if pr.label.is_empty() {
                    format!("Plot {}", i)
                } else {
                    pr.label.clone()
                },
                draw_type: pr.draw_type,
                rgb,
                names,
                data,
            }
        })
        .filter(|p| p.draw_type != 0)
        .collect();
    let mut log = host.log.clone();
    if !host.comment.is_empty() {
        log.push(format!("Comment: {}", host.comment));
    }
    let inputs_j: serde_json::Map<String, J> = inst
        .inputs_text
        .iter()
        .map(|(k, v)| (k.clone(), J::String(v.clone())))
        .collect();
    Ok(IndOut {
        name: if inst.short_name.is_empty() {
            rel(&path)
        } else {
            inst.short_name.clone()
        },
        builtin: false,
        separate: prog.props.separate_window,
        levels: inst.levels.clone(),
        inputs: J::Object(inputs_j),
        plots,
        log,
    })
}

fn program_loader() -> Box<dyn Fn(&str) -> Result<Rc<Program>, String>> {
    let cache = std::cell::RefCell::new(crate::host::ProgramCache::new());
    Box::new(move |name: &str| {
        crate::host::load_program(&mut cache.borrow_mut(), &crate::host::indicator_path(name))
    })
}

// ---------- program files ----------

/// A path inside the MQL5 folder. `default_folder` is used when only a name is given.
fn resolve(path: &str, default_folder: &str) -> Result<PathBuf, String> {
    let mut p = path.trim().replace('\\', "/");
    if let Some(s) = p.strip_prefix("MQL5/") {
        p = s.to_string();
    }
    let root = storage::mql_dir();
    // an absolute path is fine when it points into the MQL5 folder
    if Path::new(&p).is_absolute() {
        let abs = PathBuf::from(&p);
        let r = abs
            .strip_prefix(&root)
            .map_err(|_| format!("path must be inside the MQL5 folder {}", root.display()))?;
        p = r.to_string_lossy().replace('\\', "/");
    }
    if p.split('/').any(|c| c == ".." || c.is_empty()) {
        return Err(format!("invalid path '{}'", path));
    }
    let first = p.split('/').next().unwrap_or("");
    let folder = storage::FOLDERS
        .iter()
        .find(|f| f.eq_ignore_ascii_case(first));
    let p = match folder {
        Some(f) => format!("{}{}", f, &p[first.len()..]),
        None if !p.contains('/') => format!("{}/{}", default_folder, p),
        None => {
            return Err(format!(
                "path must start with one of {:?}",
                storage::FOLDERS
            ))
        }
    };
    let lower = p.to_ascii_lowercase();
    let p = if lower.ends_with(".mq5") || lower.ends_with(".mqh") {
        p
    } else {
        format!("{}.mq5", p)
    };
    Ok(root.join(p))
}

fn rel(p: &Path) -> String {
    p.strip_prefix(storage::mql_dir())
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

fn compile_source(name: &str, src: &str, dir: Option<PathBuf>) -> Result<Program, J> {
    let loader = storage::include_loader(dir);
    compile(name, src, &loader).map_err(|errs| {
        let lines: Vec<&str> = src.lines().collect();
        J::Array(
            errs.iter()
                .take(20)
                .map(|e| json!({"line": e.line, "col": e.col, "message": e.msg, "code": lines.get((e.line as usize).wrapping_sub(1)).map(|l| l.trim())}))
                .collect(),
        )
    })
}

fn compile_path(path: &Path) -> Result<Rc<Program>, String> {
    if !path.exists() {
        return Err(format!("{} does not exist", rel(path)));
    }
    let src =
        storage::read_source(path).map_err(|e| format!("cannot read {}: {}", rel(path), e))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("program.mq5");
    compile_source(name, &src, path.parent().map(|p| p.to_path_buf()))
        .map(Rc::new)
        .map_err(|e| format!("{} has compile errors: {}", rel(path), e))
}

fn summary(p: &Program) -> J {
    let inputs: Vec<J> = p.inputs.iter().map(|i| json!({"name": p.interner.name(i.name), "type": format!("{:?}", i.ty), "label": i.label})).collect();
    let mut j = json!({"kind": p.kind.label(), "inputs": inputs, "warnings": p.warnings, "included_files": p.files});
    if p.kind == ProgramKind::Indicator {
        j["window"] = json!(if p.props.separate_window {
            "separate"
        } else {
            "chart"
        });
        j["buffers"] = json!(p.props.buffers);
        j["plots"] = json!(p.props.plots);
    }
    j
}

fn compile_report(path: Option<&Path>, name: &str, src: &str) -> J {
    match compile_source(
        name,
        src,
        path.and_then(|p| p.parent()).map(|p| p.to_path_buf()),
    ) {
        Ok(p) => json!({"ok": true, "program": summary(&p)}),
        Err(e) => json!({"ok": false, "errors": e}),
    }
}

fn list_programs(a: &J) -> Result<J, String> {
    let folders: Vec<&str> = match a.get("folder").and_then(|s| s.as_str()) {
        Some(f) => vec![storage::FOLDERS
            .iter()
            .find(|x| x.eq_ignore_ascii_case(f))
            .copied()
            .ok_or_else(|| format!("folder must be one of {:?}", storage::FOLDERS))?],
        None => vec!["Indicators", "Experts", "Scripts"],
    };
    let mut out = serde_json::Map::new();
    for f in folders {
        let list = if f == "Include" {
            let mut v = vec![];
            walk_mqh(
                &storage::mql_dir().join("Include"),
                &storage::mql_dir().join("Include"),
                0,
                &mut v,
            );
            v
        } else {
            storage::list_programs(f)
                .into_iter()
                .map(|p| {
                    format!(
                        "{}/{}{}",
                        f,
                        p.rel,
                        if p.ex5 {
                            "  (ex5: compiled, cannot run)"
                        } else {
                            ""
                        }
                    )
                })
                .collect()
        };
        out.insert(f.to_string(), json!(list));
    }
    Ok(json!({"mql5_dir": storage::mql_dir(), "programs": out}))
}

fn walk_mqh(root: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() && depth < 3 {
            walk_mqh(root, &p, depth + 1, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("mqh")) {
            out.push(format!(
                "Include/{}",
                p.strip_prefix(root)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .replace('\\', "/")
            ));
        }
    }
    out.sort();
}

fn read_program(a: &J) -> Result<J, String> {
    let path = resolve(
        a.get("path")
            .and_then(|s| s.as_str())
            .ok_or("missing 'path'")?,
        "Indicators",
    )?;
    let src =
        storage::read_source(&path).map_err(|e| format!("cannot read {}: {}", rel(&path), e))?;
    Ok(json!({"path": rel(&path), "source": src}))
}

fn write_program(a: &J) -> Result<J, String> {
    let path = resolve(
        a.get("path")
            .and_then(|s| s.as_str())
            .ok_or("missing 'path'")?,
        "Indicators",
    )?;
    let src = a
        .get("source")
        .and_then(|s| s.as_str())
        .ok_or("missing 'source'")?;
    let overwrite = a.get("overwrite").and_then(|v| v.as_bool()).unwrap_or(true);
    let existed = path.exists();
    if existed && !overwrite {
        return Err(format!(
            "{} already exists (set overwrite: true to replace it)",
            rel(&path)
        ));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // keep one backup of the previous version
    if existed {
        let _ = std::fs::copy(
            &path,
            path.with_extension(format!(
                "{}.bak",
                path.extension().and_then(|e| e.to_str()).unwrap_or("mq5")
            )),
        );
    }
    std::fs::write(&path, src).map_err(|e| format!("cannot write {}: {}", rel(&path), e))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("program.mq5")
        .to_string();
    let mut j = if name.to_ascii_lowercase().ends_with(".mqh") {
        json!({"ok": true, "note": "include file saved (compile the programs that include it)"})
    } else {
        compile_report(Some(&path), &name, src)
    };
    j["path"] = json!(rel(&path));
    j["replaced_existing"] = json!(existed);
    Ok(j)
}

fn new_program(a: &J) -> Result<J, String> {
    let kind = a
        .get("kind")
        .and_then(|s| s.as_str())
        .unwrap_or("indicator")
        .to_ascii_lowercase();
    let name = a
        .get("name")
        .and_then(|s| s.as_str())
        .ok_or("missing 'name'")?;
    let (folder, tpl) = match kind.as_str() {
        "indicator" => ("Indicators", crate::samples::TEMPLATE_INDICATOR),
        "expert" | "ea" | "expert_advisor" => ("Experts", crate::samples::TEMPLATE_EXPERT),
        "script" => ("Scripts", crate::samples::TEMPLATE_SCRIPT),
        _ => return Err("kind must be indicator, expert or script".into()),
    };
    let base = name.replace('\\', "/");
    let base = base.rsplit('/').next().unwrap_or(name);
    let path = resolve(&format!("{}/{}", folder, base), folder)?;
    if path.exists() {
        return Err(format!(
            "{} already exists; use read_program / write_program",
            rel(&path)
        ));
    }
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(&storage::mql_dir()));
    std::fs::write(&path, tpl).map_err(|e| e.to_string())?;
    Ok(
        json!({"path": rel(&path), "source": tpl, "next": "edit the source and save it with write_program"}),
    )
}

fn compile_program(a: &J) -> Result<J, String> {
    if let Some(src) = a.get("source").and_then(|s| s.as_str()) {
        let path = a
            .get("path")
            .and_then(|s| s.as_str())
            .map(|p| resolve(p, "Indicators"))
            .transpose()?;
        let name = path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("program.mq5")
            .to_string();
        return Ok(compile_report(path.as_deref(), &name, src));
    }
    let path = resolve(
        a.get("path")
            .and_then(|s| s.as_str())
            .ok_or("give 'path' or 'source'")?,
        "Indicators",
    )?;
    let src =
        storage::read_source(&path).map_err(|e| format!("cannot read {}: {}", rel(&path), e))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("program.mq5")
        .to_string();
    let mut j = compile_report(Some(&path), &name, &src);
    j["path"] = json!(rel(&path));
    Ok(j)
}

// ---------- helpers ----------

fn text_json(v: J) -> Vec<J> {
    vec![json!({"type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default()})]
}

fn num(x: f64) -> J {
    if !x.is_finite() || x.abs() >= 1e300 {
        J::Null
    } else {
        json!((x * 1e8).round() / 1e8)
    }
}

fn tail(v: &[String], n: usize) -> Vec<String> {
    v[v.len().saturating_sub(n)..].to_vec()
}

fn tail_j(v: &[J], n: usize) -> Vec<J> {
    v[v.len().saturating_sub(n)..].to_vec()
}

fn arg_usize(a: &J, key: &str, default: usize) -> usize {
    a.get(key)
        .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)))
        .map(|v| v as usize)
        .unwrap_or(default)
}

fn arg_inputs(a: &J) -> HashMap<String, String> {
    a.get("inputs")
        .and_then(|v| v.as_object())
        .map(|o| {
            o.iter()
                .map(|(k, v)| {
                    let s = match v {
                        J::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    (k.clone(), s)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_tf(s: &str) -> Result<Timeframe, String> {
    let u = s.trim().to_ascii_uppercase();
    let u = u.strip_prefix("PERIOD_").unwrap_or(&u);
    let u = match u {
        "MN" | "MN1" | "1MO" | "MONTH" => "MN1",
        "1M" => "M1",
        "5M" => "M5",
        "15M" => "M15",
        "30M" => "M30",
        "1H" => "H1",
        "4H" => "H4",
        "1D" | "D" => "D1",
        "1W" | "W" => "W1",
        x => x,
    };
    Timeframe::ALL
        .iter()
        .copied()
        .find(|t| t.label() == u)
        .ok_or_else(|| {
            format!(
                "unknown timeframe '{}'. Use M1, M5, M15, M30, H1, H4, D1, W1 or MN1",
                s
            )
        })
}

/// "YYYY-MM-DD" or "YYYY.MM.DD" -> unix time at 00:00 UTC.
fn parse_date(s: &str) -> Result<i64, String> {
    let parts: Vec<i64> = s
        .trim()
        .split(['-', '.', '/'])
        .map(|x| x.parse::<i64>())
        .collect::<Result<_, _>>()
        .map_err(|_| format!("bad date '{}', use YYYY-MM-DD", s))?;
    let [y, m, d] = parts[..] else {
        return Err(format!("bad date '{}', use YYYY-MM-DD", s));
    };
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Ok((era * 146097 + doe - 719468) * 86400)
}

/// Group nearby prices; returns (average price, count) per group.
fn cluster(mut v: Vec<f64>, tol: f64) -> Vec<(f64, usize)> {
    v.sort_by(f64::total_cmp);
    let mut out: Vec<(f64, usize, f64)> = vec![];
    for x in v {
        match out.last_mut() {
            Some((avg, n, sum)) if (x - *avg).abs() <= tol => {
                *n += 1;
                *sum += x;
                *avg = *sum / *n as f64;
            }
            _ => out.push((x, 1, x)),
        }
    }
    out.into_iter().map(|(a, n, _)| (a, n)).collect()
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        s.push(if c.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    s
}

// ---------- tool schemas ----------

fn tool_list() -> J {
    let sym = json!({"type": "string", "description": "Symbol, e.g. EURUSD, XAUUSD, BTCUSD (see list_symbols)"});
    let tf = json!({"type": "string", "description": "M1, M5, M15, M30, H1, H4, D1, W1 or MN1", "default": "H1"});
    let inputs = json!({"type": "object", "description": "Input parameter overrides by name, e.g. {\"InpPeriod\": 21}", "additionalProperties": true});
    let path_ind = json!({"type": "string", "description": "Program path inside the MQL5 folder, e.g. \"Indicators/My Trend.mq5\" (a bare name means Indicators/)"});
    let ind_spec = json!({
        "type": "object",
        "description": "One indicator: either {builtin, params} or {path, inputs}",
        "properties": {
            "builtin": {"type": "string", "description": "Built-in: MA/SMA/EMA, Bands, Envelopes, Ichimoku, SAR, ADX, Alligator, Fractals, MACD, RSI, Stochastic, ATR, CCI, Momentum, WPR, Volumes"},
            "params": {"type": "object", "description": "Built-in parameters: period (p1), p2, p3 (e.g. MACD fast/slow/signal = period/p2/p3), method (0 SMA,1 EMA,2 SMMA,3 LWMA), price (1 close..7 weighted), deviation (Bands)"},
            "path": path_ind,
            "inputs": inputs,
        }
    });
    let ro = json!({"readOnlyHint": true, "openWorldHint": false});
    json!([
        {
            "name": "terminal_info",
            "title": "Terminal info",
            "description": "Data/MQL5 folder locations, program counts, built-in indicators and timeframes.",
            "inputSchema": {"type": "object", "properties": {}},
            "annotations": ro,
        },
        {
            "name": "list_programs",
            "title": "List MQL5 programs",
            "description": "List custom indicators, expert advisors, scripts (or Include files) in the MQL5 folder.",
            "inputSchema": {"type": "object", "properties": {"folder": {"type": "string", "enum": ["Indicators", "Experts", "Scripts", "Include"]}}},
            "annotations": ro,
        },
        {
            "name": "read_program",
            "title": "Read program source",
            "description": "Read the .mq5/.mqh source of a program.",
            "inputSchema": {"type": "object", "properties": {"path": path_ind}, "required": ["path"]},
            "annotations": ro,
        },
        {
            "name": "write_program",
            "title": "Write program",
            "description": "Create or replace an .mq5 indicator/EA/script (or .mqh include) and compile it. Returns compile errors with line, column and the offending code, or the program's kind, inputs and plots. The previous version is kept as .bak.",
            "inputSchema": {"type": "object", "properties": {
                "path": {"type": "string", "description": "e.g. \"Indicators/Trend Strength.mq5\", \"Experts/Breakout EA.mq5\", \"Include/MyLib.mqh\""},
                "source": {"type": "string", "description": "Full MQL5 source code"},
                "overwrite": {"type": "boolean", "default": true}
            }, "required": ["path", "source"]},
            "annotations": {"readOnlyHint": false, "destructiveHint": true, "idempotentHint": true, "openWorldHint": false},
        },
        {
            "name": "new_program",
            "title": "New program from template",
            "description": "Create a new indicator, expert advisor or script from the terminal's template and return its source.",
            "inputSchema": {"type": "object", "properties": {
                "kind": {"type": "string", "enum": ["indicator", "expert", "script"]},
                "name": {"type": "string", "description": "File name without folder, e.g. \"My Indicator\""}
            }, "required": ["kind", "name"]},
            "annotations": {"readOnlyHint": false, "destructiveHint": false, "openWorldHint": false},
        },
        {
            "name": "compile_program",
            "title": "Compile",
            "description": "Check a saved program (path) or unsaved source for compile errors without running it.",
            "inputSchema": {"type": "object", "properties": {"path": path_ind, "source": {"type": "string", "description": "Source to check instead of the saved file (path is then only used to resolve #include)"}}},
            "annotations": ro,
        },
        {
            "name": "list_symbols",
            "title": "List symbols",
            "description": "Symbols in Market Watch with digits and data source (history = imported real data, live = public crypto prices, simulated = generated).",
            "inputSchema": {"type": "object", "properties": {}},
            "annotations": ro,
        },
        {
            "name": "get_rates",
            "title": "Get price bars",
            "description": "OHLC bars (oldest first) as CSV.",
            "inputSchema": {"type": "object", "properties": {"symbol": sym, "timeframe": tf, "bars": {"type": "integer", "default": 100}, "live": {"type": "boolean", "default": true, "description": "Use public live data for crypto symbols"}}, "required": ["symbol"]},
            "annotations": ro,
        },
        {
            "name": "run_indicator",
            "title": "Run indicator",
            "description": "Calculate a custom indicator (path) or a built-in one over price history and return the last values of every plot, with times, levels and the program's Print log.",
            "inputSchema": {"type": "object", "properties": {
                "symbol": sym, "timeframe": tf,
                "path": path_ind, "inputs": inputs,
                "builtin": ind_spec["properties"]["builtin"], "params": ind_spec["properties"]["params"],
                "bars": {"type": "integer", "default": 500, "description": "History length to calculate on"},
                "last": {"type": "integer", "default": 20, "description": "How many of the latest values to return"},
                "live": {"type": "boolean", "default": true}
            }, "required": ["symbol"]},
            "annotations": ro,
        },
        {
            "name": "run_backtest",
            "title": "Back-test expert advisor",
            "description": "Run an expert advisor in the strategy tester on a simulated account and return the report (net profit, profit factor, drawdown, win rate), equity curve, last deals and log. Uses real one-minute history when imported.",
            "inputSchema": {"type": "object", "properties": {
                "path": {"type": "string", "description": "e.g. \"Experts/MA Crossover EA.mq5\" (a bare name means Experts/)"},
                "symbol": sym, "timeframe": tf, "inputs": inputs,
                "mode": {"type": "string", "enum": ["every_tick", "ohlc", "open_prices"], "default": "ohlc"},
                "deposit": {"type": "number", "default": 10000},
                "leverage": {"type": "integer", "default": 100},
                "bars": {"type": "integer", "description": "Test only the latest N bars of the timeframe"},
                "from": {"type": "string", "description": "YYYY-MM-DD"},
                "to": {"type": "string", "description": "YYYY-MM-DD (inclusive)"},
                "timeout_secs": {"type": "integer", "default": 120},
                "live": {"type": "boolean", "default": true}
            }, "required": ["path", "symbol"]},
            "annotations": ro,
        },
        {
            "name": "analyze_chart",
            "title": "Analyse chart",
            "description": "Technical snapshot of a symbol/timeframe: trend vs SMA20/50/200, RSI, Stochastic, MACD, ADX, ATR, Bollinger Bands, recent range, support/resistance from swing points, and plain observations.",
            "inputSchema": {"type": "object", "properties": {"symbol": sym, "timeframe": tf, "bars": {"type": "integer", "default": 300}, "live": {"type": "boolean", "default": true}}, "required": ["symbol"]},
            "annotations": ro,
        },
        {
            "name": "chart_image",
            "title": "Chart image",
            "description": "Render a candlestick chart PNG with optional indicators (built-in or custom; oscillators get their own pane). Returns the image and saves it to a file (path in the result).",
            "inputSchema": {"type": "object", "properties": {
                "symbol": sym, "timeframe": tf,
                "bars": {"type": "integer", "default": 150},
                "indicators": {"type": "array", "items": ind_spec},
                "width": {"type": "integer", "default": 1200},
                "height": {"type": "integer", "default": 700},
                "save_to": {"type": "string", "description": "Optional PNG file path"},
                "live": {"type": "boolean", "default": true}
            }, "required": ["symbol"]},
            "annotations": {"readOnlyHint": true, "openWorldHint": false},
        }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_timeframes() {
        assert_eq!(parse_date("1970-01-02").unwrap(), 86400);
        assert_eq!(parse_date("2024.03.01").unwrap(), 1709251200);
        assert_eq!(parse_tf("h4").unwrap(), Timeframe::H4);
        assert_eq!(parse_tf("PERIOD_D1").unwrap(), Timeframe::D1);
        assert!(parse_tf("H2").is_err());
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(find_builtin("ema"), Some(0));
        assert_eq!(find_builtin("iRSI"), find_builtin("RSI"));
        assert_eq!(find_builtin("bb"), Some(1));
    }
}
