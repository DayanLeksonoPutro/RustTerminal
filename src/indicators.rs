//! Built-in technical indicators. Default parameters come from the original's indicator
//! setup functions (see MECHANICS.md).

use crate::market::Bar;
use egui::Color32;

pub const EMPTY: f64 = f64::NAN;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum IndKind {
    MovingAverage,
    Bands,
    Envelopes,
    Ichimoku,
    Sar,
    Alligator,
    Fractals,
    Macd,
    Rsi,
    Stochastic,
    Atr,
    Cci,
    Momentum,
    Adx,
    Wpr,
    Volumes,
}

impl IndKind {
    pub fn name(self) -> &'static str {
        match self {
            IndKind::MovingAverage => "Moving Average",
            IndKind::Bands => "Bollinger Bands",
            IndKind::Envelopes => "Envelopes",
            IndKind::Ichimoku => "Ichimoku Kinko Hyo",
            IndKind::Sar => "Parabolic SAR",
            IndKind::Alligator => "Alligator",
            IndKind::Fractals => "Fractals",
            IndKind::Macd => "MACD",
            IndKind::Rsi => "Relative Strength Index",
            IndKind::Stochastic => "Stochastic Oscillator",
            IndKind::Atr => "Average True Range",
            IndKind::Cci => "Commodity Channel Index",
            IndKind::Momentum => "Momentum",
            IndKind::Adx => "Average Directional Movement Index",
            IndKind::Wpr => "Williams' Percent Range",
            IndKind::Volumes => "Volumes",
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            IndKind::MovingAverage => "MA",
            IndKind::Bands => "Bands",
            IndKind::Envelopes => "Env",
            IndKind::Ichimoku => "Ichimoku",
            IndKind::Sar => "SAR",
            IndKind::Alligator => "Alligator",
            IndKind::Fractals => "Fractals",
            IndKind::Macd => "MACD",
            IndKind::Rsi => "RSI",
            IndKind::Stochastic => "Stoch",
            IndKind::Atr => "ATR",
            IndKind::Cci => "CCI",
            IndKind::Momentum => "Momentum",
            IndKind::Adx => "ADX",
            IndKind::Wpr => "WPR",
            IndKind::Volumes => "Volumes",
        }
    }
    pub fn separate_window(self) -> bool {
        !matches!(
            self,
            IndKind::MovingAverage
                | IndKind::Bands
                | IndKind::Envelopes
                | IndKind::Ichimoku
                | IndKind::Sar
                | IndKind::Alligator
                | IndKind::Fractals
        )
    }
    pub fn group(self) -> &'static str {
        match self {
            IndKind::MovingAverage | IndKind::Bands | IndKind::Envelopes | IndKind::Ichimoku | IndKind::Sar | IndKind::Adx => "Trend",
            IndKind::Alligator | IndKind::Fractals => "Bill Williams",
            IndKind::Volumes => "Volumes",
            _ => "Oscillators",
        }
    }
    pub const ALL: [IndKind; 16] = [
        IndKind::MovingAverage,
        IndKind::Bands,
        IndKind::Envelopes,
        IndKind::Ichimoku,
        IndKind::Sar,
        IndKind::Adx,
        IndKind::Alligator,
        IndKind::Fractals,
        IndKind::Macd,
        IndKind::Rsi,
        IndKind::Stochastic,
        IndKind::Atr,
        IndKind::Cci,
        IndKind::Momentum,
        IndKind::Wpr,
        IndKind::Volumes,
    ];
}

/// Parameters shared by all built-ins; each indicator reads the fields it needs.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IndParams {
    pub p1: i64,
    pub p2: i64,
    pub p3: i64,
    pub shift: i64,
    /// 0 simple, 1 exponential, 2 smoothed, 3 linear weighted
    pub method: i64,
    /// 1 close, 2 open, 3 high, 4 low, 5 median, 6 typical, 7 weighted
    pub price: i64,
    pub f1: f64,
    pub f2: f64,
}

impl IndParams {
    pub fn defaults(kind: IndKind) -> IndParams {
        let base = IndParams { p1: 14, p2: 0, p3: 0, shift: 0, method: 0, price: 1, f1: 0.0, f2: 0.0 };
        match kind {
            // MA period 10: not found in the code, estimated
            IndKind::MovingAverage => IndParams { p1: 10, ..base },
            IndKind::Bands => IndParams { p1: 20, f1: 2.0, ..base },
            IndKind::Envelopes => IndParams { p1: 14, f1: 0.1, ..base },
            IndKind::Ichimoku => IndParams { p1: 9, p2: 26, p3: 52, ..base },
            IndKind::Sar => IndParams { f1: 0.02, f2: 0.2, ..base },
            IndKind::Alligator => IndParams { p1: 13, p2: 8, p3: 5, method: 2, price: 5, ..base },
            IndKind::Fractals => base,
            IndKind::Macd => IndParams { p1: 12, p2: 26, p3: 9, ..base },
            IndKind::Stochastic => IndParams { p1: 5, p2: 3, p3: 3, ..base },
            IndKind::Volumes => base,
            _ => base,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineStyle {
    Solid,
    Dash,
    Dot,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DrawKind {
    Line,
    Histogram,
    Dots,
    ArrowUp,
    ArrowDown,
    /// histogram whose colour comes from a second buffer (0 = colour 0, 1 = colour 1)
    ColorHistogram,
}

#[derive(Clone, Debug)]
pub struct LineSpec {
    pub label: String,
    pub color: Color32,
    pub color2: Color32,
    pub style: LineStyle,
    pub width: f32,
    pub draw: DrawKind,
    pub shift: i64,
}

impl LineSpec {
    fn new(label: &str, color: Color32) -> Self {
        LineSpec {
            label: label.to_string(),
            color,
            color2: color,
            style: LineStyle::Solid,
            width: 1.0,
            draw: DrawKind::Line,
            shift: 0,
        }
    }
    fn draw(mut self, d: DrawKind) -> Self {
        self.draw = d;
        self
    }
    fn style(mut self, s: LineStyle) -> Self {
        self.style = s;
        self
    }
    fn shift(mut self, s: i64) -> Self {
        self.shift = s;
        self
    }
}

pub fn bgr(c: u32) -> Color32 {
    Color32::from_rgb((c & 0xff) as u8, ((c >> 8) & 0xff) as u8, ((c >> 16) & 0xff) as u8)
}

pub const RED: Color32 = Color32::from_rgb(255, 0, 0);
pub const BLUE: Color32 = Color32::from_rgb(0, 0, 255);
pub const LIME: Color32 = Color32::from_rgb(0, 255, 0);
pub const DODGER: Color32 = Color32::from_rgb(30, 144, 255);
pub const SEAGREEN: Color32 = Color32::from_rgb(32, 178, 170);
pub const SILVER: Color32 = Color32::from_rgb(192, 192, 192);

pub struct IndOutput {
    pub buffers: Vec<Vec<f64>>,
    pub lines: Vec<LineSpec>,
    pub levels: Vec<f64>,
    pub fixed_range: Option<(f64, f64)>,
}

pub fn applied_price(b: &Bar, price: i64) -> f64 {
    match price {
        2 => b.open,
        3 => b.high,
        4 => b.low,
        5 => (b.high + b.low) / 2.0,
        6 => (b.high + b.low + b.close) / 3.0,
        7 => (b.high + b.low + 2.0 * b.close) / 4.0,
        _ => b.close,
    }
}

pub fn ma(src: &[f64], period: usize, method: i64) -> Vec<f64> {
    let n = src.len();
    let mut out = vec![EMPTY; n];
    let p = period.max(1);
    // find first valid
    let start = match src.iter().position(|v| !v.is_nan()) {
        Some(s) => s,
        None => return out,
    };
    if n < start + p {
        return out;
    }
    match method {
        1 => {
            let a = 2.0 / (p as f64 + 1.0);
            let mut prev = src[start];
            out[start] = prev;
            for i in start + 1..n {
                let v = if src[i].is_nan() { prev } else { src[i] };
                prev = v * a + prev * (1.0 - a);
                out[i] = prev;
            }
            for v in out.iter_mut().take(start + p - 1) {
                *v = EMPTY;
            }
        }
        2 => {
            let mut sum: f64 = src[start..start + p].iter().sum();
            let mut prev = sum / p as f64;
            out[start + p - 1] = prev;
            for i in start + p..n {
                prev = (prev * (p as f64 - 1.0) + src[i]) / p as f64;
                out[i] = prev;
            }
            sum = 0.0;
            let _ = sum;
        }
        3 => {
            let wsum = (p * (p + 1) / 2) as f64;
            for i in start + p - 1..n {
                let mut s = 0.0;
                for k in 0..p {
                    s += src[i - k] * (p - k) as f64;
                }
                out[i] = s / wsum;
            }
        }
        _ => {
            let mut sum: f64 = src[start..start + p].iter().sum();
            out[start + p - 1] = sum / p as f64;
            for i in start + p..n {
                sum += src[i] - src[i - p];
                out[i] = sum / p as f64;
            }
        }
    }
    out
}

fn highest(v: &[f64], i: usize, p: usize) -> f64 {
    v[i + 1 - p..=i].iter().cloned().fold(f64::MIN, f64::max)
}
fn lowest(v: &[f64], i: usize, p: usize) -> f64 {
    v[i + 1 - p..=i].iter().cloned().fold(f64::MAX, f64::min)
}

pub fn compute(kind: IndKind, pr: &IndParams, bars: &[Bar]) -> IndOutput {
    let n = bars.len();
    let close: Vec<f64> = bars.iter().map(|b| b.close).collect();
    let high: Vec<f64> = bars.iter().map(|b| b.high).collect();
    let low: Vec<f64> = bars.iter().map(|b| b.low).collect();
    let price: Vec<f64> = bars.iter().map(|b| applied_price(b, pr.price)).collect();
    let p1 = pr.p1.max(1) as usize;
    let p2 = pr.p2.max(1) as usize;
    let p3 = pr.p3.max(1) as usize;
    let mut levels = vec![];
    let mut fixed_range = None;
    let (buffers, lines) = match kind {
        IndKind::MovingAverage => {
            let m = ma(&price, p1, pr.method);
            let label = format!("MA({})", p1);
            (vec![m], vec![LineSpec::new(&label, RED).shift(pr.shift)])
        }
        IndKind::Bands => {
            let mid = ma(&price, p1, 0);
            let mut up = vec![EMPTY; n];
            let mut dn = vec![EMPTY; n];
            for i in 0..n {
                if i + 1 >= p1 && !mid[i].is_nan() {
                    let mean = mid[i];
                    let var: f64 = price[i + 1 - p1..=i].iter().map(|x| (x - mean).powi(2)).sum::<f64>() / p1 as f64;
                    let sd = var.sqrt();
                    up[i] = mean + pr.f1 * sd;
                    dn[i] = mean - pr.f1 * sd;
                }
            }
            // colour from the original's setup function: 0x71b33c (BGR)
            let c = bgr(0x71b33c);
            (
                vec![mid, up, dn],
                vec![
                    LineSpec::new("Bands", c).shift(pr.shift),
                    LineSpec::new("Upper", c).shift(pr.shift),
                    LineSpec::new("Lower", c).shift(pr.shift),
                ],
            )
        }
        IndKind::Envelopes => {
            let m = ma(&price, p1, pr.method);
            let up: Vec<f64> = m.iter().map(|v| v * (1.0 + pr.f1 / 100.0)).collect();
            let dn: Vec<f64> = m.iter().map(|v| v * (1.0 - pr.f1 / 100.0)).collect();
            (vec![up, dn], vec![LineSpec::new("Upper", BLUE).shift(pr.shift), LineSpec::new("Lower", RED).shift(pr.shift)])
        }
        IndKind::Ichimoku => {
            let mut tenkan = vec![EMPTY; n];
            let mut kijun = vec![EMPTY; n];
            let mut sa = vec![EMPTY; n];
            let mut sb = vec![EMPTY; n];
            let mut chikou = vec![EMPTY; n];
            for i in 0..n {
                if i + 1 >= p1 {
                    tenkan[i] = (highest(&high, i, p1) + lowest(&low, i, p1)) / 2.0;
                }
                if i + 1 >= p2 {
                    kijun[i] = (highest(&high, i, p2) + lowest(&low, i, p2)) / 2.0;
                }
                if !tenkan[i].is_nan() && !kijun[i].is_nan() {
                    sa[i] = (tenkan[i] + kijun[i]) / 2.0;
                }
                if i + 1 >= p3 {
                    sb[i] = (highest(&high, i, p3) + lowest(&low, i, p3)) / 2.0;
                }
                chikou[i] = close[i];
            }
            let k = p2 as i64;
            (
                vec![tenkan, kijun, sa, sb, chikou],
                vec![
                    LineSpec::new("Tenkan-sen", RED),
                    LineSpec::new("Kijun-sen", BLUE),
                    LineSpec::new("Senkou Span A", Color32::from_rgb(244, 164, 96)).style(LineStyle::Dot).shift(k),
                    LineSpec::new("Senkou Span B", Color32::from_rgb(216, 191, 216)).style(LineStyle::Dot).shift(k),
                    LineSpec::new("Chikou Span", LIME).shift(-k),
                ],
            )
        }
        IndKind::Sar => {
            let mut sar = vec![EMPTY; n];
            if n > 2 {
                let mut long = close[1] >= close[0];
                let mut af = pr.f1;
                let mut ep = if long { high[1] } else { low[1] };
                let mut s = if long { low[0] } else { high[0] };
                for i in 1..n {
                    s += af * (ep - s);
                    if long {
                        s = s.min(low[i - 1]).min(if i >= 2 { low[i - 2] } else { low[i - 1] });
                        if low[i] < s {
                            long = false;
                            s = ep;
                            ep = low[i];
                            af = pr.f1;
                        } else if high[i] > ep {
                            ep = high[i];
                            af = (af + pr.f1).min(pr.f2);
                        }
                    } else {
                        s = s.max(high[i - 1]).max(if i >= 2 { high[i - 2] } else { high[i - 1] });
                        if high[i] > s {
                            long = true;
                            s = ep;
                            ep = high[i];
                            af = pr.f1;
                        } else if low[i] < ep {
                            ep = low[i];
                            af = (af + pr.f1).min(pr.f2);
                        }
                    }
                    sar[i] = s;
                }
            }
            (vec![sar], vec![LineSpec::new("SAR", LIME).draw(DrawKind::Dots)])
        }
        IndKind::Alligator => {
            let med: Vec<f64> = bars.iter().map(|b| applied_price(b, pr.price)).collect();
            (
                vec![ma(&med, p1, pr.method), ma(&med, p2, pr.method), ma(&med, p3, pr.method)],
                vec![
                    LineSpec::new("Jaws", BLUE).shift(8),
                    LineSpec::new("Teeth", RED).shift(5),
                    LineSpec::new("Lips", LIME).shift(3),
                ],
            )
        }
        IndKind::Fractals => {
            let mut up = vec![EMPTY; n];
            let mut dn = vec![EMPTY; n];
            for i in 2..n.saturating_sub(2) {
                if high[i] > high[i - 1] && high[i] > high[i - 2] && high[i] >= high[i + 1] && high[i] >= high[i + 2] {
                    up[i] = high[i];
                }
                if low[i] < low[i - 1] && low[i] < low[i - 2] && low[i] <= low[i + 1] && low[i] <= low[i + 2] {
                    dn[i] = low[i];
                }
            }
            (
                vec![up, dn],
                vec![
                    LineSpec::new("Fractal Up", SILVER).draw(DrawKind::ArrowUp),
                    LineSpec::new("Fractal Down", SILVER).draw(DrawKind::ArrowDown),
                ],
            )
        }
        IndKind::Macd => {
            let fast = ma(&price, p1, 1);
            let slow = ma(&price, p2, 1);
            let macd: Vec<f64> = (0..n)
                .map(|i| if i + 1 >= p2 { fast[i] - slow[i] } else { EMPTY })
                .collect();
            let signal = ma(&macd, p3, 0);
            levels.push(0.0);
            (
                vec![macd, signal],
                vec![
                    // histogram silver (0xc0c0c0) and red signal line, from the original's setup function
                    LineSpec::new("MACD", bgr(0xc0c0c0)).draw(DrawKind::Histogram),
                    LineSpec::new("Signal", bgr(0x0000ff)).style(LineStyle::Dot),
                ],
            )
        }
        IndKind::Rsi => {
            let mut out = vec![EMPTY; n];
            if n > p1 {
                let mut gain = 0.0;
                let mut loss = 0.0;
                for i in 1..=p1 {
                    let d = price[i] - price[i - 1];
                    if d > 0.0 { gain += d } else { loss -= d }
                }
                gain /= p1 as f64;
                loss /= p1 as f64;
                out[p1] = if loss == 0.0 { 100.0 } else { 100.0 - 100.0 / (1.0 + gain / loss) };
                for i in p1 + 1..n {
                    let d = price[i] - price[i - 1];
                    gain = (gain * (p1 as f64 - 1.0) + d.max(0.0)) / p1 as f64;
                    loss = (loss * (p1 as f64 - 1.0) + (-d).max(0.0)) / p1 as f64;
                    out[i] = if loss == 0.0 { 100.0 } else { 100.0 - 100.0 / (1.0 + gain / loss) };
                }
            }
            levels = vec![30.0, 70.0];
            fixed_range = Some((0.0, 100.0));
            (vec![out], vec![LineSpec::new(&format!("RSI({})", p1), DODGER)])
        }
        IndKind::Stochastic => {
            let mut k = vec![EMPTY; n];
            for i in 0..n {
                if i + 1 >= p1 + p3 - 1 {
                    let mut num = 0.0;
                    let mut den = 0.0;
                    for j in 0..p3 {
                        let idx = i - j;
                        let ll = lowest(&low, idx, p1);
                        let hh = highest(&high, idx, p1);
                        num += close[idx] - ll;
                        den += hh - ll;
                    }
                    k[i] = if den == 0.0 { 100.0 } else { num / den * 100.0 };
                }
            }
            let d = ma(&k, p2, 0);
            levels = vec![20.0, 80.0];
            fixed_range = Some((0.0, 100.0));
            (
                vec![k, d],
                vec![LineSpec::new("Main", SEAGREEN), LineSpec::new("Signal", RED).style(LineStyle::Dash)],
            )
        }
        IndKind::Atr => {
            let tr: Vec<f64> = (0..n)
                .map(|i| {
                    if i == 0 {
                        high[0] - low[0]
                    } else {
                        (high[i] - low[i]).max((high[i] - close[i - 1]).abs()).max((low[i] - close[i - 1]).abs())
                    }
                })
                .collect();
            (vec![ma(&tr, p1, 0)], vec![LineSpec::new(&format!("ATR({})", p1), DODGER)])
        }
        IndKind::Cci => {
            let tp: Vec<f64> = bars.iter().map(|b| applied_price(b, if pr.price == 1 { 6 } else { pr.price })).collect();
            let sma = ma(&tp, p1, 0);
            let mut out = vec![EMPTY; n];
            for i in 0..n {
                if i + 1 >= p1 {
                    let md: f64 = tp[i + 1 - p1..=i].iter().map(|x| (x - sma[i]).abs()).sum::<f64>() / p1 as f64;
                    out[i] = if md == 0.0 { 0.0 } else { (tp[i] - sma[i]) / (0.015 * md) };
                }
            }
            levels = vec![-100.0, 100.0];
            (vec![out], vec![LineSpec::new(&format!("CCI({})", p1), SEAGREEN)])
        }
        IndKind::Momentum => {
            let out: Vec<f64> = (0..n)
                .map(|i| if i >= p1 && price[i - p1] != 0.0 { price[i] / price[i - p1] * 100.0 } else { EMPTY })
                .collect();
            levels = vec![100.0];
            (vec![out], vec![LineSpec::new(&format!("Momentum({})", p1), DODGER)])
        }
        IndKind::Adx => {
            let mut pdm = vec![0.0; n];
            let mut mdm = vec![0.0; n];
            let mut tr = vec![0.0; n];
            for i in 1..n {
                let up = high[i] - high[i - 1];
                let dn = low[i - 1] - low[i];
                pdm[i] = if up > dn && up > 0.0 { up } else { 0.0 };
                mdm[i] = if dn > up && dn > 0.0 { dn } else { 0.0 };
                tr[i] = (high[i] - low[i]).max((high[i] - close[i - 1]).abs()).max((low[i] - close[i - 1]).abs());
            }
            let pdi_raw: Vec<f64> = (0..n).map(|i| if tr[i] > 0.0 { 100.0 * pdm[i] / tr[i] } else { 0.0 }).collect();
            let mdi_raw: Vec<f64> = (0..n).map(|i| if tr[i] > 0.0 { 100.0 * mdm[i] / tr[i] } else { 0.0 }).collect();
            let pdi = ma(&pdi_raw, p1, 1);
            let mdi = ma(&mdi_raw, p1, 1);
            let dx: Vec<f64> = (0..n)
                .map(|i| {
                    let s = pdi[i] + mdi[i];
                    if s.is_nan() { EMPTY } else if s == 0.0 { 0.0 } else { 100.0 * (pdi[i] - mdi[i]).abs() / s }
                })
                .collect();
            let adx = ma(&dx, p1, 1);
            (
                vec![adx, pdi, mdi],
                vec![
                    LineSpec::new(&format!("ADX({})", p1), SEAGREEN),
                    LineSpec::new("+DI", Color32::from_rgb(154, 205, 50)).style(LineStyle::Dot),
                    LineSpec::new("-DI", Color32::from_rgb(245, 222, 179)).style(LineStyle::Dot),
                ],
            )
        }
        IndKind::Wpr => {
            let out: Vec<f64> = (0..n)
                .map(|i| {
                    if i + 1 >= p1 {
                        let hh = highest(&high, i, p1);
                        let ll = lowest(&low, i, p1);
                        if hh == ll { 0.0 } else { -100.0 * (hh - close[i]) / (hh - ll) }
                    } else {
                        EMPTY
                    }
                })
                .collect();
            levels = vec![-20.0, -80.0];
            fixed_range = Some((-100.0, 0.0));
            (vec![out], vec![LineSpec::new(&format!("%R({})", p1), DODGER)])
        }
        IndKind::Volumes => {
            let v: Vec<f64> = bars.iter().map(|b| b.tick_volume as f64).collect();
            let c: Vec<f64> = (0..n).map(|i| if i > 0 && v[i] < v[i - 1] { 1.0 } else { 0.0 }).collect();
            let mut spec = LineSpec::new("Volumes", Color32::from_rgb(50, 205, 50)).draw(DrawKind::ColorHistogram);
            spec.color2 = RED;
            spec.width = 2.0;
            fixed_range = None;
            (vec![v, c], vec![spec])
        }
    };
    IndOutput { buffers, lines, levels, fixed_range }
}

/// The buffers a script sees through CopyBuffer for a built-in indicator handle.
pub fn script_buffers(kind: IndKind, pr: &IndParams, bars: &[Bar]) -> Vec<Vec<f64>> {
    let out = compute(kind, pr, bars);
    let mut bufs = out.buffers;
    // Scripts expect EMPTY_VALUE instead of NaN.
    for b in bufs.iter_mut() {
        for v in b.iter_mut() {
            if v.is_nan() {
                *v = f64::MAX;
            }
        }
    }
    bufs
}
