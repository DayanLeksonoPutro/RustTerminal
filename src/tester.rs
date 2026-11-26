//! Strategy tester: runs an expert advisor over history with generated ticks on its own
//! virtual account, step by step so the window stays responsive.

use crate::account::{Account, Quote, QuoteSource, TradeResult};
use crate::market::{Bar, SymbolSpec, Timeframe};
use crate::mql::interp::{Host, Instance, TradeOp};
use crate::mql::Program;
use crate::objects::ChartObjects;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TickMode {
    EveryTick,
    OhlcPoints,
    OpenPrices,
}

impl TickMode {
    pub fn label(self) -> &'static str {
        match self {
            TickMode::EveryTick => "Every tick (generated)",
            TickMode::OhlcPoints => "1 minute OHLC",
            TickMode::OpenPrices => "Open prices only",
        }
    }
}

pub struct TesterHost {
    pub spec: SymbolSpec,
    pub tf: Timeframe,
    pub all: Vec<Bar>,
    /// bars visible to the program (closed bars + forming bar)
    pub view: Vec<Bar>,
    pub bid: f64,
    pub ask: f64,
    pub time: i64,
    pub account: Account,
    pub log: Vec<String>,
    pub objects: ChartObjects,
    pub gv: HashMap<String, f64>,
    pub comment: String,
    agg_cache: HashMap<Timeframe, (usize, Vec<Bar>)>,
    pub loader: Box<dyn Fn(&str) -> Result<Rc<Program>, String>>,
    /// real one-minute history behind the test (empty when there is none)
    pub base: Vec<Bar>,
    /// one-minute bars visible so far (closed + forming)
    pub base_view: Vec<Bar>,
    base_pos: usize,
}

impl QuoteSource for TesterHost {
    fn quote(&self, symbol: &str) -> Option<Quote> {
        if symbol.eq_ignore_ascii_case(self.spec.name) {
            Some(Quote {
                spec: self.spec.clone(),
                bid: self.bid,
                ask: self.ask,
            })
        } else {
            None
        }
    }
}

struct Q<'a>(&'a SymbolSpec, f64, f64);
impl QuoteSource for Q<'_> {
    fn quote(&self, symbol: &str) -> Option<Quote> {
        if symbol.eq_ignore_ascii_case(self.0.name) {
            Some(Quote {
                spec: self.0.clone(),
                bid: self.1,
                ask: self.2,
            })
        } else {
            None
        }
    }
}

impl TesterHost {
    /// A host that shows every bar at once (no stepping): for running an indicator over
    /// finished history outside the tester, e.g. from the MCP server.
    pub fn over_bars(
        spec: SymbolSpec,
        tf: Timeframe,
        bars: Vec<Bar>,
        loader: Box<dyn Fn(&str) -> Result<Rc<Program>, String>>,
    ) -> TesterHost {
        let last = bars.last().copied().unwrap_or_default();
        let spread = spec.spread_points as f64 * spec.point();
        let ask = crate::market::round_to(last.close + spread, spec.digits);
        let mut account = Account::new_demo(1, "MCP", 10000.0, 100);
        account.server = "MCP".into();
        TesterHost {
            spec,
            tf,
            view: bars.clone(),
            all: bars,
            bid: last.close,
            ask,
            time: last.time,
            account,
            log: vec![],
            objects: ChartObjects::default(),
            gv: HashMap::new(),
            comment: String::new(),
            agg_cache: HashMap::new(),
            loader,
            base: vec![],
            base_view: vec![],
            base_pos: 0,
        }
    }

    pub fn set_price(&mut self, time: i64, bid: f64) {
        let spread = self.spec.spread_points as f64 * self.spec.point();
        self.time = time;
        self.bid = bid;
        self.ask = crate::market::round_to(bid + spread, self.spec.digits);
        let q = Q(&self.spec, self.bid, self.ask);
        self.account.process(&q, time, &mut self.log);
    }
}

impl Host for TesterHost {
    fn quote(&self, symbol: &str) -> Option<Quote> {
        QuoteSource::quote(self, symbol)
    }
    fn bars(&mut self, symbol: &str, tf: Timeframe) -> Option<&[Bar]> {
        if !symbol.eq_ignore_ascii_case(self.spec.name) {
            return None;
        }
        if tf == self.tf {
            return Some(&self.view);
        }
        let with_base = !self.base.is_empty();
        if with_base && tf == Timeframe::M1 {
            return Some(&self.base_view);
        }
        if !with_base && tf.secs() < self.tf.secs() {
            return None;
        }
        // other timeframes are built from what is visible so far
        let src: &Vec<Bar> = if with_base {
            &self.base_view
        } else {
            &self.view
        };
        let key = src.len() * 1_000_000 + src.last().map(|b| b.tick_volume as usize).unwrap_or(0);
        let fresh = !matches!(self.agg_cache.get(&tf), Some((k, _)) if *k == key);
        if fresh {
            let out = crate::history::aggregate(src, tf);
            self.agg_cache.insert(tf, (key, out));
        }
        self.agg_cache.get(&tf).map(|(_, v)| v.as_slice())
    }
    fn now(&self) -> i64 {
        self.time
    }
    fn account(&self) -> &Account {
        &self.account
    }
    fn margin(&self) -> f64 {
        self.account.margin(self)
    }
    fn trade(&mut self, op: TradeOp) -> TradeResult {
        let q = Q(&self.spec, self.bid, self.ask);
        let t = self.time;
        let log = &mut self.log;
        match op {
            TradeOp::Market {
                symbol,
                buy,
                volume,
                sl,
                tp,
                magic,
                comment,
            } => self
                .account
                .market_order(&q, &symbol, buy, volume, sl, tp, magic, &comment, t, log),
            TradeOp::Close { ticket, volume } => {
                self.account.close_position(&q, ticket, volume, t, "", log)
            }
            TradeOp::Modify { ticket, sl, tp } => {
                self.account.modify_position(&q, ticket, sl, tp, log)
            }
            TradeOp::Pending {
                symbol,
                typ,
                volume,
                price,
                sl,
                tp,
                magic,
                comment,
            } => match crate::account::OrderType::from_i64(typ) {
                Some(ot) => self.account.pending_order(
                    &q, &symbol, ot, volume, price, sl, tp, magic, &comment, t, log,
                ),
                None => TradeResult {
                    retcode: crate::account::RET_INVALID,
                    order: 0,
                    deal: 0,
                    price: 0.0,
                    volume: 0.0,
                    comment: "invalid order type".into(),
                },
            },
            TradeOp::Delete { ticket } => self.account.delete_order(ticket, log),
            TradeOp::ModifyOrder {
                ticket,
                price,
                sl,
                tp,
            } => self.account.modify_order(ticket, price, sl, tp),
        }
    }
    fn print(&mut self, msg: &str) {
        self.log.push(format!(
            "{}   {}",
            crate::market::fmt_time(self.time, true),
            msg
        ));
    }
    fn alert(&mut self, msg: &str) {
        self.log.push(format!(
            "{}   Alert: {}",
            crate::market::fmt_time(self.time, true),
            msg
        ));
    }
    fn set_comment(&mut self, text: &str) {
        self.comment = text.to_string();
    }
    fn objects(&mut self) -> &mut ChartObjects {
        &mut self.objects
    }
    fn algo_allowed(&self) -> bool {
        true
    }
    fn load_program(&mut self, name: &str) -> Result<Rc<Program>, String> {
        (self.loader)(name)
    }
    fn is_tester(&self) -> bool {
        true
    }
    fn symbol_names(&self) -> Vec<String> {
        vec![self.spec.name.to_string()]
    }
    fn global_vars(&mut self) -> &mut HashMap<String, f64> {
        &mut self.gv
    }
}

#[derive(Clone, Debug, Default)]
pub struct TestReport {
    pub initial: f64,
    pub net: f64,
    pub gross_profit: f64,
    pub gross_loss: f64,
    pub profit_factor: f64,
    pub expected_payoff: f64,
    pub max_dd: f64,
    pub max_dd_pct: f64,
    pub trades: usize,
    pub wins: usize,
    pub losses: usize,
    pub largest_win: f64,
    pub largest_loss: f64,
    pub bars: usize,
    pub ticks: usize,
}

pub struct TestRun {
    pub host: TesterHost,
    pub inst: Instance,
    pub bar: usize,
    pub start_bar: usize,
    pub mode: TickMode,
    pub done: bool,
    pub error: Option<String>,
    pub curve: Vec<(f64, f64)>,
    pub ticks: usize,
    pub deposit: f64,
}

impl TestRun {
    pub fn new(
        prog: Rc<Program>,
        spec: SymbolSpec,
        tf: Timeframe,
        bars: Vec<Bar>,
        mode: TickMode,
        deposit: f64,
        leverage: u32,
        inputs: &HashMap<String, String>,
        loader: Box<dyn Fn(&str) -> Result<Rc<Program>, String>>,
    ) -> TestRun {
        TestRun::new_inner(
            prog,
            spec,
            tf,
            bars,
            mode,
            deposit,
            leverage,
            inputs,
            loader,
            vec![],
            0,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_inner(
        prog: Rc<Program>,
        spec: SymbolSpec,
        tf: Timeframe,
        bars: Vec<Bar>,
        mode: TickMode,
        deposit: f64,
        leverage: u32,
        inputs: &HashMap<String, String>,
        loader: Box<dyn Fn(&str) -> Result<Rc<Program>, String>>,
        base: Vec<Bar>,
        base_pos: usize,
    ) -> TestRun {
        let warmup = 100.min(bars.len() / 4);
        let view = bars[..warmup].to_vec();
        let first = bars.get(warmup).copied().unwrap_or_default();
        let mut account = Account::new_demo(1, "Tester", deposit, leverage);
        account.server = "Strategy Tester".into();
        let mut host = TesterHost {
            spec: spec.clone(),
            tf,
            all: bars,
            view,
            bid: first.open,
            ask: first.open + spec.spread_points as f64 * spec.point(),
            time: first.time,
            account,
            log: vec![],
            objects: ChartObjects::default(),
            gv: HashMap::new(),
            comment: String::new(),
            agg_cache: HashMap::new(),
            loader,
            base_view: base[..base_pos.min(base.len())].to_vec(),
            base,
            base_pos,
        };
        let mut inst = Instance::new(prog, spec.name, tf, inputs);
        let mut error = None;
        host.log.push(format!(
            "testing of {} on {} {} started ({})",
            inst.name,
            spec.name,
            tf.label(),
            mode.label()
        ));
        match inst.init(&mut host) {
            Ok(0) => {}
            Ok(code) => error = Some(format!("OnInit returned non-zero code {}", code)),
            Err(e) => {
                error = Some(format!(
                    "critical runtime error: {} in OnInit function",
                    inst.fmt_err(&e)
                ))
            }
        }
        if let Some(e) = &error {
            host.log.push(e.clone());
        }
        TestRun {
            host,
            inst,
            bar: warmup,
            start_bar: warmup,
            mode,
            done: error.is_some(),
            error,
            curve: vec![],
            ticks: 0,
            deposit,
        }
    }

    /// A test driven by real one-minute history: the tested timeframe is built from it,
    /// ticks follow every minute (open, low/high, high/low, close) like the platform's
    /// "1 minute OHLC" model, and programs can read any timeframe.
    #[allow(clippy::too_many_arguments)]
    pub fn with_minutes(
        prog: Rc<Program>,
        spec: SymbolSpec,
        tf: Timeframe,
        minutes: Vec<Bar>,
        mode: TickMode,
        deposit: f64,
        leverage: u32,
        inputs: &HashMap<String, String>,
        loader: Box<dyn Fn(&str) -> Result<Rc<Program>, String>>,
    ) -> TestRun {
        let bars = if tf == Timeframe::M1 {
            minutes.clone()
        } else {
            crate::history::aggregate(&minutes, tf)
        };
        let warmup = 100.min(bars.len() / 4);
        let first_time = bars.get(warmup).map(|b| b.time).unwrap_or(i64::MAX);
        let base_pos = minutes.partition_point(|b| b.time < first_time);
        // the host must see the warm-up minutes before OnInit runs
        let mut run = TestRun::new_inner(
            prog, spec, tf, bars, mode, deposit, leverage, inputs, loader, minutes, base_pos,
        );
        run.host.base_pos = base_pos;
        run
    }

    pub fn progress(&self) -> f32 {
        let n = self.host.all.len().max(1);
        ((self.bar - self.start_bar) as f32 / (n - self.start_bar).max(1) as f32).clamp(0.0, 1.0)
    }

    fn tick_path(&self, b: &Bar) -> Vec<f64> {
        match self.mode {
            TickMode::OpenPrices => vec![b.open],
            TickMode::OhlcPoints => {
                if b.close >= b.open {
                    vec![b.open, b.low, b.high, b.close]
                } else {
                    vec![b.open, b.high, b.low, b.close]
                }
            }
            TickMode::EveryTick => {
                let pts = if b.close >= b.open {
                    [b.open, b.low, b.high, b.close]
                } else {
                    [b.open, b.high, b.low, b.close]
                };
                let mut out = vec![];
                for w in pts.windows(2) {
                    for k in 0..4 {
                        let f = k as f64 / 4.0;
                        out.push(crate::market::round_to(
                            w[0] + (w[1] - w[0]) * f,
                            self.host.spec.digits,
                        ));
                    }
                }
                out.push(b.close);
                out
            }
        }
    }

    /// Process up to `max_bars` bars. Returns true when finished.
    pub fn step(&mut self, max_bars: usize) -> bool {
        if self.done {
            return true;
        }
        let secs = self.host.tf.secs();
        for _ in 0..max_bars {
            if self.bar >= self.host.all.len() {
                self.finish();
                return true;
            }
            let b = self.host.all[self.bar];
            // ticks of this bar: (time, price, minute bar index when driven by minutes)
            let ticks: Vec<(i64, f64, Option<usize>)> = if self.host.base.is_empty() {
                let path = self.tick_path(&b);
                let n = path.len();
                path.into_iter()
                    .enumerate()
                    .map(|(k, p)| (b.time + (secs * k as i64) / n.max(1) as i64, p, None))
                    .collect()
            } else {
                let end = self
                    .host
                    .all
                    .get(self.bar + 1)
                    .map(|x| x.time)
                    .unwrap_or(i64::MAX);
                let mut v = vec![];
                let mut j = self.host.base_pos;
                while j < self.host.base.len() && self.host.base[j].time < end {
                    let m = self.host.base[j];
                    let path = if self.mode == TickMode::OpenPrices {
                        vec![m.open]
                    } else {
                        self.tick_path(&m)
                    };
                    let n = path.len();
                    for (k, p) in path.into_iter().enumerate() {
                        v.push((m.time + (60 * k as i64) / n.max(1) as i64, p, Some(j)));
                    }
                    if self.mode == TickMode::OpenPrices {
                        // open prices only: one tick at the bar's open, minutes still advance
                        j += 1;
                        while j < self.host.base.len() && self.host.base[j].time < end {
                            j += 1;
                        }
                        break;
                    }
                    j += 1;
                }
                if v.is_empty() {
                    v.push((b.time, b.open, None));
                }
                v
            };
            for (k, (t, p, minute)) in ticks.into_iter().enumerate() {
                if k == 0 {
                    self.host.view.push(Bar {
                        time: b.time,
                        open: p,
                        high: p,
                        low: p,
                        close: p,
                        tick_volume: 1,
                        spread: b.spread,
                    });
                } else if let Some(l) = self.host.view.last_mut() {
                    l.high = l.high.max(p);
                    l.low = l.low.min(p);
                    l.close = p;
                    l.tick_volume += 1;
                }
                if let Some(j) = minute {
                    let m = self.host.base[j];
                    match self.host.base_view.last_mut() {
                        Some(l) if l.time == m.time => {
                            l.high = l.high.max(p);
                            l.low = l.low.min(p);
                            l.close = p;
                            l.tick_volume += 1;
                        }
                        _ => self.host.base_view.push(Bar {
                            time: m.time,
                            open: p,
                            high: p,
                            low: p,
                            close: p,
                            tick_volume: 1,
                            spread: m.spread,
                        }),
                    }
                }
                self.host.set_price(t, p);
                self.ticks += 1;
                let deals_before = self.host.account.deals.len();
                if let Err(e) = self.inst.call_event("OnTick", vec![], &mut self.host) {
                    let msg = format!(
                        "critical runtime error: {} in OnTick function",
                        self.inst.fmt_err(&e)
                    );
                    self.host.log.push(msg.clone());
                    self.error = Some(msg);
                    self.finish();
                    return true;
                }
                if self.host.account.deals.len() != deals_before {
                    let _ = self.inst.call_event("OnTrade", vec![], &mut self.host);
                }
                if self.inst.removed {
                    self.host.log.push("expert removed itself".into());
                    self.finish();
                    return true;
                }
            }
            if let Some(l) = self.host.view.last_mut() {
                l.tick_volume = b.tick_volume;
            }
            if !self.host.base.is_empty() {
                // the minutes of this bar are complete: make them exact
                let end = self
                    .host
                    .all
                    .get(self.bar + 1)
                    .map(|x| x.time)
                    .unwrap_or(i64::MAX);
                let from = self.host.base_pos;
                let mut to = from;
                while to < self.host.base.len() && self.host.base[to].time < end {
                    to += 1;
                }
                let keep = self.host.base_view.partition_point(|x| {
                    x.time < self.host.base.get(from).map(|m| m.time).unwrap_or(i64::MAX)
                });
                self.host.base_view.truncate(keep);
                self.host
                    .base_view
                    .extend_from_slice(&self.host.base[from..to]);
                self.host.base_pos = to;
            }
            self.curve
                .push((self.host.account.balance, self.host.account.equity()));
            self.bar += 1;
        }
        false
    }

    fn finish(&mut self) {
        if self.done {
            return;
        }
        self.done = true;
        let tickets: Vec<u64> = self
            .host
            .account
            .positions
            .iter()
            .map(|p| p.ticket)
            .collect();
        let q = Q(&self.host.spec, self.host.bid, self.host.ask);
        let t = self.host.time;
        for tk in tickets {
            self.host
                .account
                .close_position(&q, tk, None, t, "end of test ", &mut self.host.log);
        }
        let _ = self.inst.deinit(0, &mut self.host);
        self.curve
            .push((self.host.account.balance, self.host.account.equity()));
        self.host.log.push(format!(
            "final balance {:.2} USD",
            self.host.account.balance
        ));
    }

    pub fn report(&self) -> TestReport {
        let mut r = TestReport {
            initial: self.deposit,
            bars: self.bar - self.start_bar,
            ticks: self.ticks,
            ..Default::default()
        };
        for d in self.host.account.deals.iter().filter(|d| d.entry == "out") {
            r.trades += 1;
            if d.profit >= 0.0 {
                r.wins += 1;
                r.gross_profit += d.profit;
                r.largest_win = r.largest_win.max(d.profit);
            } else {
                r.losses += 1;
                r.gross_loss += d.profit;
                r.largest_loss = r.largest_loss.min(d.profit);
            }
        }
        r.net = r.gross_profit + r.gross_loss;
        r.profit_factor = if r.gross_loss < 0.0 {
            r.gross_profit / -r.gross_loss
        } else {
            0.0
        };
        r.expected_payoff = if r.trades > 0 {
            r.net / r.trades as f64
        } else {
            0.0
        };
        let mut peak = self.deposit;
        for (_, eq) in &self.curve {
            peak = peak.max(*eq);
            let dd = peak - eq;
            if dd > r.max_dd {
                r.max_dd = dd;
                r.max_dd_pct = if peak > 0.0 { dd / peak * 100.0 } else { 0.0 };
            }
        }
        r
    }
}
