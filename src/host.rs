//! The live terminal side of a running program: prices from the market, trades on the
//! logged-in account, chart objects and logs.

use crate::account::{Account, OrderType, Quote, QuoteSource, TradeResult};
use crate::market::{fmt_time, unix_now, Bar, Market, Timeframe};
use crate::mql::interp::{Host, TradeOp};
use crate::mql::parser::compile;
use crate::mql::Program;
use crate::objects::ChartObjects;
use crate::storage;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

#[derive(Clone)]
pub struct LogLine {
    pub time: i64,
    pub source: String,
    pub text: String,
    pub error: bool,
}

impl QuoteSource for Market {
    fn quote(&self, symbol: &str) -> Option<Quote> {
        let i = self.symbol_index(symbol)?;
        let s = &self.symbols[i];
        Some(Quote { spec: s.spec.clone(), bid: s.bid, ask: s.ask })
    }
}

pub type ProgramCache = HashMap<PathBuf, (Option<SystemTime>, Rc<Program>)>;

/// Compile a program file, reusing the last result while the file is unchanged.
pub fn load_program(cache: &mut ProgramCache, path: &Path) -> Result<Rc<Program>, String> {
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    if let Some((t, p)) = cache.get(path) {
        if *t == mtime && mtime.is_some() {
            return Ok(p.clone());
        }
    }
    if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("ex5")) {
        return Err("compiled .ex5 files are encrypted and can't be run here; use the program's .mq5 source".into());
    }
    let src = storage::read_source(path).map_err(|e| format!("cannot open file: {}", e))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("program.mq5").to_string();
    let loader = storage::include_loader(path.parent().map(|p| p.to_path_buf()));
    match compile(&name, &src, &loader) {
        Ok(p) => {
            let p = Rc::new(p);
            cache.insert(path.to_path_buf(), (mtime, p.clone()));
            Ok(p)
        }
        Err(errs) => Err(errs.iter().take(5).map(|e| format!("({},{}) {}", e.line, e.col, e.msg)).collect::<Vec<_>>().join("; ")),
    }
}

pub fn indicator_path(name: &str) -> PathBuf {
    let clean = name.replace('\\', "/");
    let clean = clean.trim_end_matches(".ex5").trim_end_matches(".mq5");
    storage::mql_dir().join("Indicators").join(format!("{}.mq5", clean))
}

pub struct LiveHost<'a> {
    pub market: &'a mut Market,
    pub account: &'a mut Account,
    pub objects: &'a mut ChartObjects,
    pub comment: &'a mut String,
    pub journal: &'a mut Vec<LogLine>,
    pub experts: &'a mut Vec<LogLine>,
    pub alerts: &'a mut Vec<LogLine>,
    pub gv: &'a mut HashMap<String, f64>,
    pub cache: &'a mut ProgramCache,
    pub algo: bool,
    pub source: String,
}

impl<'a> LiveHost<'a> {
    pub fn journal_lines(&mut self, lines: Vec<String>) {
        let now = unix_now();
        for l in lines {
            self.journal.push(LogLine { time: now, source: "Trades".into(), text: l, error: false });
        }
    }
}

pub fn apply_trade(market: &Market, account: &mut Account, op: TradeOp, log: &mut Vec<String>) -> TradeResult {
    let t = market.now;
    match op {
        TradeOp::Market { symbol, buy, volume, sl, tp, magic, comment } => account.market_order(market, &symbol, buy, volume, sl, tp, magic, &comment, t, log),
        TradeOp::Close { ticket, volume } => account.close_position(market, ticket, volume, t, "", log),
        TradeOp::Modify { ticket, sl, tp } => account.modify_position(market, ticket, sl, tp, log),
        TradeOp::Pending { symbol, typ, volume, price, sl, tp, magic, comment } => match OrderType::from_i64(typ) {
            Some(ot) => account.pending_order(market, &symbol, ot, volume, price, sl, tp, magic, &comment, t, log),
            None => TradeResult { retcode: crate::account::RET_INVALID, order: 0, deal: 0, price: 0.0, volume: 0.0, comment: "invalid order type".into() },
        },
        TradeOp::Delete { ticket } => account.delete_order(ticket, log),
        TradeOp::ModifyOrder { ticket, price, sl, tp } => account.modify_order(ticket, price, sl, tp),
    }
}

impl<'a> Host for LiveHost<'a> {
    fn quote(&self, symbol: &str) -> Option<Quote> {
        QuoteSource::quote(&*self.market, symbol)
    }
    fn bars(&mut self, symbol: &str, tf: Timeframe) -> Option<&[Bar]> {
        let i = self.market.symbol_index(symbol)?;
        Some(self.market.bars(i, tf))
    }
    fn now(&self) -> i64 {
        self.market.now
    }
    fn account(&self) -> &Account {
        self.account
    }
    fn margin(&self) -> f64 {
        self.account.margin(&*self.market)
    }
    fn trade(&mut self, op: TradeOp) -> TradeResult {
        let mut log = vec![];
        let r = apply_trade(self.market, self.account, op, &mut log);
        self.journal_lines(log);
        r
    }
    fn print(&mut self, msg: &str) {
        self.experts.push(LogLine { time: unix_now(), source: self.source.clone(), text: msg.to_string(), error: false });
    }
    fn alert(&mut self, msg: &str) {
        self.alerts.push(LogLine { time: unix_now(), source: self.source.clone(), text: msg.to_string(), error: false });
        self.experts.push(LogLine { time: unix_now(), source: self.source.clone(), text: format!("Alert: {}", msg), error: false });
    }
    fn set_comment(&mut self, text: &str) {
        *self.comment = text.to_string();
    }
    fn objects(&mut self) -> &mut ChartObjects {
        self.objects
    }
    fn algo_allowed(&self) -> bool {
        self.algo
    }
    fn load_program(&mut self, name: &str) -> Result<Rc<Program>, String> {
        load_program(self.cache, &indicator_path(name))
    }
    fn is_tester(&self) -> bool {
        false
    }
    fn symbol_names(&self) -> Vec<String> {
        self.market.symbols.iter().map(|s| s.spec.name.to_string()).collect()
    }
    fn global_vars(&mut self) -> &mut HashMap<String, f64> {
        self.gv
    }
}

pub fn fmt_log_time(t: i64) -> String {
    fmt_time(t, true)
}
