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

pub struct SymbolState {
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
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(1_700_000_000)
}

impl Market {
    pub fn new() -> Self {
        let now = unix_now();
        let mut rng = Rng::new(now as u64);
        let symbols = default_symbols()
            .into_iter()
            .map(|spec| {
                let p = spec.start_price * (1.0 + rng.normal() * 0.002);
                let p = round_to(p, spec.digits);
                let spread = spec.spread_points as f64 * spec.point();
                SymbolState {
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
        let mut m = Market { symbols, series: HashMap::new(), rng, now };
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

    /// Bars of a symbol/timeframe, oldest first. Generated on first use.
    pub fn bars(&mut self, sym: usize, tf: Timeframe) -> &[Bar] {
        if !self.series.contains_key(&(sym, tf)) {
            let bars = self.generate_history(sym, tf);
            self.series.insert((sym, tf), bars);
        }
        &self.series[&(sym, tf)]
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
        let mut ticked = Vec::new();
        for i in 0..self.symbols.len() {
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
    let secs = tf.secs();
    let open_time = time - time.rem_euclid(secs);
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
