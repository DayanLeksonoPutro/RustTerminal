//! Simulated market: symbols, price history per timeframe and a live tick generator.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Timeframe {
    M1,
    M5,
    M15,
    M30,
    H1,
    H4,
    D1,
    W1,
    MN1,
}

impl Timeframe {
    pub const ALL: [Timeframe; 9] = [
        Timeframe::M1,
        Timeframe::M5,
        Timeframe::M15,
        Timeframe::M30,
        Timeframe::H1,
        Timeframe::H4,
        Timeframe::D1,
        Timeframe::W1,
        Timeframe::MN1,
    ];
    pub fn secs(self) -> i64 {
        match self {
            Timeframe::M1 => 60,
            Timeframe::M5 => 300,
            Timeframe::M15 => 900,
            Timeframe::M30 => 1800,
            Timeframe::H1 => 3600,
            Timeframe::H4 => 14400,
            Timeframe::D1 => 86400,
            Timeframe::W1 => 604800,
            Timeframe::MN1 => 2592000,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Timeframe::M1 => "M1",
            Timeframe::M5 => "M5",
            Timeframe::M15 => "M15",
            Timeframe::M30 => "M30",
            Timeframe::H1 => "H1",
            Timeframe::H4 => "H4",
            Timeframe::D1 => "D1",
            Timeframe::W1 => "W1",
            Timeframe::MN1 => "MN1",
        }
    }
    /// The numeric value scripts see (ENUM_TIMEFRAMES).
    pub fn mql_value(self) -> i64 {
        match self {
            Timeframe::M1 => 1,
            Timeframe::M5 => 5,
            Timeframe::M15 => 15,
            Timeframe::M30 => 30,
            Timeframe::H1 => 16385,
            Timeframe::H4 => 16388,
            Timeframe::D1 => 16408,
            Timeframe::W1 => 32769,
            Timeframe::MN1 => 49153,
        }
    }
    pub fn from_mql(v: i64) -> Option<Timeframe> {
        Timeframe::ALL.iter().copied().find(|t| t.mql_value() == v)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Bar {
    pub time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub tick_volume: i64,
    pub spread: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProfitCalc {
    /// Quote currency is USD (EURUSD, XAUUSD, BTCUSD...)
    QuoteUsd,
    /// Base currency is USD (USDJPY, USDCHF...)
    BaseUsd,
    /// Other symbols: profit per tick in USD as reported by the broker's terminal
    TickValue { tick_value: f64, tick_size: f64 },
}

#[derive(Clone, Debug)]
pub struct SymbolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub digits: u32,
    pub contract_size: f64,
    pub start_price: f64,
    /// relative volatility per minute (fraction of price)
    pub vol_per_min: f64,
    pub spread_points: i32,
    pub calc: ProfitCalc,
    pub volume_min: f64,
    pub volume_step: f64,
    pub volume_max: f64,
}

impl SymbolSpec {
    pub fn point(&self) -> f64 {
        10f64.powi(-(self.digits as i32))
    }
}

pub fn default_symbols() -> Vec<SymbolSpec> {
    let fx = |name, description, digits, price, vol, spread, calc| SymbolSpec {
        name,
        description,
        digits,
        contract_size: 100000.0,
        start_price: price,
        vol_per_min: vol,
        spread_points: spread,
        calc,
        volume_min: 0.01,
        volume_step: 0.01,
        volume_max: 100.0,
    };
    vec![
        fx("EURUSD", "Euro vs US Dollar", 5, 1.08520, 0.00012, 12, ProfitCalc::QuoteUsd),
        fx("GBPUSD", "Great Britain Pound vs US Dollar", 5, 1.27140, 0.00014, 15, ProfitCalc::QuoteUsd),
        fx("USDJPY", "US Dollar vs Japanese Yen", 3, 151.420, 0.00013, 14, ProfitCalc::BaseUsd),
        fx("USDCHF", "US Dollar vs Swiss Franc", 5, 0.88210, 0.00012, 16, ProfitCalc::BaseUsd),
        fx("AUDUSD", "Australian Dollar vs US Dollar", 5, 0.65830, 0.00015, 14, ProfitCalc::QuoteUsd),
        fx("USDCAD", "US Dollar vs Canadian Dollar", 5, 1.36250, 0.00011, 18, ProfitCalc::BaseUsd),
        fx("NZDUSD", "New Zealand Dollar vs US Dollar", 5, 0.60120, 0.00016, 18, ProfitCalc::QuoteUsd),
        SymbolSpec {
            name: "XAUUSD",
            description: "Gold vs US Dollar",
            digits: 2,
            contract_size: 100.0,
            start_price: 2335.40,
            vol_per_min: 0.00018,
            spread_points: 25,
            calc: ProfitCalc::QuoteUsd,
            volume_min: 0.01,
            volume_step: 0.01,
            volume_max: 50.0,
        },
        SymbolSpec {
            name: "BTCUSD",
            description: "Bitcoin vs US Dollar",
            digits: 2,
            contract_size: 1.0,
            start_price: 67250.00,
            vol_per_min: 0.0009,
            spread_points: 1800,
            calc: ProfitCalc::QuoteUsd,
            volume_min: 0.01,
            volume_step: 0.01,
            volume_max: 20.0,
        },
    ]
}

/// Small deterministic random generator (xorshift64*).
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn normal(&mut self) -> f64 {
        let u1 = self.uniform().max(1e-12);
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

/// Where a symbol's prices come from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    /// generated prices
    Simulated,
    /// real history (imported); new prices are simulated from the last real close
    History,
    /// real history and real live quotes
    Live,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Simulated => "simulated",
            Source::History => "real history",
            Source::Live => "live",
        }
    }
}

pub struct SymbolState {
    pub source: Source,
    pub spec: SymbolSpec,
    pub bid: f64,
    pub ask: f64,
    pub prev_bid: f64,
    pub day_open: f64,
    pub day_high: f64,
    pub day_low: f64,
    pub last_tick_time: i64,
    pub ticks: u64,
    /// slow drift so trends appear
    pub drift: f64,
}

impl SymbolState {
    pub fn spread_points(&self) -> i64 {
        ((self.ask - self.bid) / self.spec.point()).round() as i64
    }
}

pub const HISTORY_BARS: usize = 1500;

pub struct Market {
    pub symbols: Vec<SymbolState>,
    series: HashMap<(usize, Timeframe), Vec<Bar>>,
    rng: Rng,
    pub now: i64,
    feed: Option<crate::feed::LiveFeed>,
    /// last message from the live price service
    pub feed_status: String,
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(1_700_000_000)
}

impl Market {
    /// The market with real data where it exists (imported history, exported symbol
    /// properties); everything else simulated.
    pub fn new() -> Self {
        let mut m = Market::new_simulated();
        m.load_real_data();
        m
    }

    /// A fully simulated market (tests).
    pub fn new_simulated() -> Self {
        let now = unix_now();
        let mut rng = Rng::new(now as u64);
        let symbols = default_symbols()
            .into_iter()
            .map(|spec| {
                let p = spec.start_price * (1.0 + rng.normal() * 0.002);
                let p = round_to(p, spec.digits);
                let spread = spec.spread_points as f64 * spec.point();
                SymbolState {
                    source: Source::Simulated,
                    bid: p,
                    ask: round_to(p + spread, spec.digits),
                    prev_bid: p,
                    day_open: p,
                    day_high: p,
                    day_low: p,
                    last_tick_time: now,
                    ticks: 0,
                    drift: 0.0,
                    spec,
                }
            })
            .collect();
        let mut m = Market { symbols, series: HashMap::new(), rng, now, feed: None, feed_status: String::new() };
        for i in 0..m.symbols.len() {
            let d1 = m.bars(i, Timeframe::D1).to_vec();
            if let Some(last) = d1.last() {
                let s = &mut m.symbols[i];
                s.day_open = last.open;
                s.day_high = last.high.max(s.bid);
                s.day_low = last.low.min(s.bid);
            }
        }
        m
    }

    pub fn symbol_index(&self, name: &str) -> Option<usize> {
        self.symbols.iter().position(|s| s.spec.name.eq_ignore_ascii_case(name))
    }

    /// Bars of a symbol/timeframe, oldest first: real history when the symbol has it,
    /// otherwise generated on first use.
    pub fn bars(&mut self, sym: usize, tf: Timeframe) -> &[Bar] {
        if !self.series.contains_key(&(sym, tf)) {
            let real = match self.symbols[sym].source {
                Source::Simulated => None,
                _ => crate::history::load(self.symbols[sym].spec.name, tf).filter(|b| !b.is_empty()),
            };
            let bars = real.unwrap_or_else(|| self.generate_history(sym, tf));
            self.series.insert((sym, tf), bars);
        }
        &self.series[&(sym, tf)]
    }

    // ---------- real data ----------

    /// Use exported symbol properties and imported history.
    pub fn load_real_data(&mut self) {
        for info in crate::history::load_symbols() {
            self.add_or_update_symbol(&info);
        }
        for (name, tfs) in crate::history::stored() {
            if self.symbol_index(&name).is_none() {
                // a symbol known only from its history: guess its properties from prices
                if let Some(bars) = tfs.first().and_then(|tf| crate::history::load(&name, *tf)) {
                    let info = guess_info(&name, &bars);
                    self.add_or_update_symbol(&info);
                }
            }
            if let Some(i) = self.symbol_index(&name) {
                self.use_history(i);
            }
        }
    }

    /// Add a symbol or replace its properties with the broker's.
    pub fn add_or_update_symbol(&mut self, info: &crate::history::SymbolInfo) -> usize {
        let spec = spec_from_info(info);
        match self.symbol_index(&info.name) {
            Some(i) => {
                let s = &mut self.symbols[i];
                s.spec = spec;
                i
            }
            None => {
                let p = if info.bid > 0.0 { info.bid } else { 1.0 };
                let spread = spec.spread_points as f64 * spec.point();
                self.symbols.push(SymbolState {
                    source: Source::Simulated,
                    bid: p,
                    ask: round_to(p + spread, spec.digits),
                    prev_bid: p,
                    day_open: p,
                    day_high: p,
                    day_low: p,
                    last_tick_time: self.now,
                    ticks: 0,
                    drift: 0.0,
                    spec,
                });
                self.symbols.len() - 1
            }
        }
    }

    /// Switch a symbol to its stored history (after an import, too).
    pub fn use_history(&mut self, i: usize) {
        let name = self.symbols[i].spec.name;
        if !crate::history::has_history(name) {
            return;
        }
        self.series.retain(|(s, _), _| *s != i);
        if self.symbols[i].source == Source::Simulated {
            self.symbols[i].source = Source::History;
        }
        // current price and day range from the newest real bars
        let finest = Timeframe::ALL.iter().find_map(|tf| crate::history::load(name, *tf).filter(|b| !b.is_empty()).map(|b| b.last().copied().unwrap()));
        if let Some(last) = finest {
            let s = &mut self.symbols[i];
            let spread = (if last.spread > 0 { last.spread } else { s.spec.spread_points }) as f64 * s.spec.point();
            s.bid = last.close;
            s.prev_bid = last.close;
            s.ask = round_to(last.close + spread, s.spec.digits);
            s.last_tick_time = last.time;
        }
        if let Some(d) = crate::history::load(name, Timeframe::D1).and_then(|b| b.last().copied()) {
            let s = &mut self.symbols[i];
            s.day_open = d.open;
            s.day_high = d.high.max(s.bid);
            s.day_low = d.low.min(s.bid);
        }
    }

    /// Start live quotes for every symbol that has a public price source.
    pub fn start_live(&mut self) -> usize {
        let list: Vec<(String, u32)> = self.symbols.iter().filter(|s| crate::feed::pair_for(s.spec.name).is_some()).map(|s| (s.spec.name.to_string(), s.spec.digits)).collect();
        let n = list.len();
        if n > 0 {
            self.feed_status = "connecting to live prices...".into();
            self.feed = Some(crate::feed::LiveFeed::start(list));
        }
        n
    }

    pub fn stop_live(&mut self) {
        self.feed = None;
        for s in self.symbols.iter_mut() {
            if s.source == Source::Live {
                s.source = if crate::history::has_history(s.spec.name) { Source::History } else { Source::Simulated };
            }
        }
        self.feed_status = "live prices off".into();
    }

    pub fn live_running(&self) -> bool {
        self.feed.is_some()
    }

    /// Apply what the live service sent; returns the symbols that got a real tick.
    fn poll_feed(&mut self) -> Vec<usize> {
        let msgs = match &self.feed {
            Some(f) => f.poll(),
            None => return vec![],
        };
        let mut ticked = vec![];
        for m in msgs {
            match m {
                crate::feed::FeedMsg::History { symbol, tf, bars } => {
                    if let Some(i) = self.symbol_index(&symbol) {
                        self.symbols[i].source = Source::Live;
                        if let Some(last) = bars.last() {
                            let s = &mut self.symbols[i];
                            if tf == Timeframe::D1 {
                                s.day_open = last.open;
                                s.day_high = last.high;
                                s.day_low = last.low;
                            }
                            if tf == Timeframe::M1 {
                                s.bid = last.close;
                                s.prev_bid = last.close;
                                s.ask = round_to(last.close + s.spec.spread_points as f64 * s.spec.point(), s.spec.digits);
                            }
                        }
                        self.series.insert((i, tf), bars);
                    }
                }
                crate::feed::FeedMsg::Quote { symbol, bid, ask, time } => {
                    if let Some(i) = self.symbol_index(&symbol) {
                        self.symbols[i].source = Source::Live;
                        if self.apply_quote(i, bid, ask, time) {
                            ticked.push(i);
                        }
                    }
                }
                crate::feed::FeedMsg::Status(s) | crate::feed::FeedMsg::Error(s) => self.feed_status = s,
            }
        }
        ticked
    }

    /// A real quote: set the prices and update every loaded series.
    pub fn apply_quote(&mut self, i: usize, bid: f64, ask: f64, time: i64) -> bool {
        let s = &mut self.symbols[i];
        if s.bid == bid && s.ask == ask {
            return false;
        }
        s.prev_bid = s.bid;
        s.bid = bid;
        s.ask = ask;
        s.day_high = s.day_high.max(bid);
        s.day_low = s.day_low.min(bid);
        s.last_tick_time = time;
        s.ticks += 1;
        let spread = ((ask - bid) / s.spec.point()).round() as i32;
        for tf in Timeframe::ALL {
            if let Some(bars) = self.series.get_mut(&(i, tf)) {
                apply_tick(bars, tf, time, bid, spread);
            }
        }
        true
    }

    pub fn bars_if_loaded(&self, sym: usize, tf: Timeframe) -> Option<&[Bar]> {
        self.series.get(&(sym, tf)).map(|v| v.as_slice())
    }

    fn generate_history(&self, sym: usize, tf: Timeframe) -> Vec<Bar> {
        let s = &self.symbols[sym];
        let spec = &s.spec;
        let secs = tf.secs();
        let last_open = self.now - self.now.rem_euclid(secs);
        let mut rng = Rng::new((sym as u64 + 1) * 7919 + secs as u64 * 31 + (self.now / 86400) as u64);
        let minutes = secs as f64 / 60.0;
        let sigma = spec.vol_per_min * minutes.sqrt();
        let n = HISTORY_BARS;
        let mut bars = vec![Bar::default(); n];
        let mut close = s.bid;
        let mut trend = 0.0;
        for k in (0..n).rev() {
            trend = trend * 0.97 + rng.normal() * sigma * 0.08;
            let ret = rng.normal() * sigma + trend;
            let open = close / (1.0 + ret);
            let span = (open - close).abs();
            let wick = sigma * open * 0.6;
            let high = open.max(close) + rng.uniform() * wick + span * 0.1;
            let low = open.min(close) - rng.uniform() * wick - span * 0.1;
            let vol = (minutes * (20.0 + rng.uniform() * 40.0)).round() as i64 + 1;
            bars[k] = Bar {
                time: last_open - (n - 1 - k) as i64 * secs,
                open: round_to(open, spec.digits),
                high: round_to(high, spec.digits),
                low: round_to(low, spec.digits),
                close: round_to(close, spec.digits),
                tick_volume: vol,
                spread: spec.spread_points,
            };
            close = open;
        }
        // make sure the last bar holds the current price
        if let Some(b) = bars.last_mut() {
            b.close = s.bid;
            b.high = b.high.max(s.bid);
            b.low = b.low.min(s.bid);
        }
        bars
    }

    /// Advance the market to the current time with a batch of ticks. Returns which symbols ticked.
    pub fn update(&mut self) -> Vec<usize> {
        self.now = unix_now();
        let mut ticked = self.poll_feed();
        for i in 0..self.symbols.len() {
            if self.symbols[i].source == Source::Live || ticked.contains(&i) {
                continue; // real quotes only
            }
            // each symbol ticks with some probability per frame
            if self.rng.uniform() > 0.35 {
                continue;
            }
            self.tick_symbol(i);
            ticked.push(i);
        }
        ticked
    }

    pub fn tick_symbol(&mut self, i: usize) {
        let now = self.now;
        let (bid, ask, spread_pts) = {
            let r1 = self.rng.normal();
            let r2 = self.rng.normal();
            let u = self.rng.uniform();
            let s = &mut self.symbols[i];
            let sigma = s.spec.vol_per_min * 0.18;
            s.drift = s.drift * 0.995 + r2 * sigma * 0.02;
            let new_bid = s.bid * (1.0 + r1 * sigma + s.drift);
            let new_bid = round_to(new_bid, s.spec.digits);
            let spread_pts = (s.spec.spread_points as f64 * (0.8 + u * 0.5)).round() as i32;
            s.prev_bid = s.bid;
            s.bid = new_bid;
            s.ask = round_to(new_bid + spread_pts as f64 * s.spec.point(), s.spec.digits);
            s.day_high = s.day_high.max(new_bid);
            s.day_low = s.day_low.min(new_bid);
            s.last_tick_time = now;
            s.ticks += 1;
            (s.bid, s.ask, spread_pts)
        };
        let _ = ask;
        for tf in Timeframe::ALL {
            if let Some(bars) = self.series.get_mut(&(i, tf)) {
                apply_tick(bars, tf, now, bid, spread_pts);
            }
        }
    }
}

/// Apply a tick at `time` with price `bid` to a bar series, opening new bars as time passes.
pub fn apply_tick(bars: &mut Vec<Bar>, tf: Timeframe, time: i64, bid: f64, spread: i32) {
    let open_time = crate::history::bar_open_time(tf, time);
    match bars.last_mut() {
        Some(last) if last.time >= open_time => {
            last.high = last.high.max(bid);
            last.low = last.low.min(bid);
            last.close = bid;
            last.tick_volume += 1;
            last.spread = spread;
        }
        _ => {
            let prev_close = bars.last().map(|b| b.close).unwrap_or(bid);
            bars.push(Bar {
                time: open_time,
                open: prev_close,
                high: prev_close.max(bid),
                low: prev_close.min(bid),
                close: bid,
                tick_volume: 1,
                spread,
            });
            if bars.len() > HISTORY_BARS * 3 {
                bars.drain(0..HISTORY_BARS);
            }
        }
    }
}

pub fn round_to(v: f64, digits: u32) -> f64 {
    let m = 10f64.powi(digits as i32);
    (v * m).round() / m
}

pub fn fmt_price(v: f64, digits: u32) -> String {
    format!("{:.*}", digits as usize, v)
}

/// Format a unix time as "YYYY.MM.DD HH:MM".
pub fn fmt_time(t: i64, with_secs: bool) -> String {
    let (y, mo, d, h, mi, s) = civil(t);
    if with_secs {
        format!("{:04}.{:02}.{:02} {:02}:{:02}:{:02}", y, mo, d, h, mi, s)
    } else {
        format!("{:04}.{:02}.{:02} {:02}:{:02}", y, mo, d, h, mi)
    }
}

pub fn civil(t: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = t.div_euclid(86400);
    let rem = t.rem_euclid(86400);
    let (h, mi, s) = ((rem / 3600) as u32, ((rem % 3600) / 60) as u32, (rem % 60) as u32);
    // civil from days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d, h, mi, s)
}

/// Leak a string for the 'static names symbol specs use (a handful per run).
fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// Symbol properties from the broker's terminal.
pub fn spec_from_info(info: &crate::history::SymbolInfo) -> SymbolSpec {
    let calc = if info.currency_profit.eq_ignore_ascii_case("USD") {
        ProfitCalc::QuoteUsd
    } else if info.currency_base.eq_ignore_ascii_case("USD") {
        ProfitCalc::BaseUsd
    } else if info.tick_value > 0.0 && info.tick_size > 0.0 {
        ProfitCalc::TickValue { tick_value: info.tick_value, tick_size: info.tick_size }
    } else {
        ProfitCalc::QuoteUsd
    };
    let known = default_symbols().into_iter().find(|s| s.name.eq_ignore_ascii_case(&info.name));
    let vol = known.as_ref().map(|k| k.vol_per_min).unwrap_or(0.00015);
    SymbolSpec {
        name: known.as_ref().map(|k| k.name).unwrap_or_else(|| leak(&info.name)),
        description: if info.description.is_empty() { known.as_ref().map(|k| k.description).unwrap_or("") } else { leak(&info.description) },
        digits: info.digits,
        contract_size: if info.contract_size > 0.0 { info.contract_size } else { 100000.0 },
        start_price: info.bid,
        vol_per_min: vol,
        spread_points: if info.spread > 0 { info.spread } else { 10 },
        calc,
        volume_min: if info.volume_min > 0.0 { info.volume_min } else { 0.01 },
        volume_step: if info.volume_step > 0.0 { info.volume_step } else { 0.01 },
        volume_max: if info.volume_max > 0.0 { info.volume_max } else { 100.0 },
    }
}

/// Properties guessed from price history alone.
fn guess_info(name: &str, bars: &[Bar]) -> crate::history::SymbolInfo {
    let last = bars.last().map(|b| b.close).unwrap_or(1.0);
    let digits = bars
        .iter()
        .rev()
        .take(200)
        .map(|b| {
            let s = format!("{}", b.close);
            s.split('.').nth(1).map(|f| f.len() as u32).unwrap_or(0)
        })
        .max()
        .unwrap_or(5)
        .min(8);
    let upper = name.to_ascii_uppercase();
    let (base, profit) = if upper.len() >= 6 { (upper[0..3].to_string(), upper[3..6].to_string()) } else { (String::new(), "USD".to_string()) };
    let spread = bars.iter().rev().take(50).map(|b| b.spread).max().unwrap_or(0);
    crate::history::SymbolInfo {
        name: upper.clone(),
        description: String::new(),
        digits,
        contract_size: if upper.starts_with("XAU") { 100.0 } else if upper.len() == 6 { 100000.0 } else { 1.0 },
        volume_min: 0.01,
        volume_max: 100.0,
        volume_step: 0.01,
        spread,
        currency_base: base,
        currency_profit: profit,
        currency_margin: String::new(),
        tick_value: 0.0,
        tick_size: 0.0,
        bid: last,
    }
}
