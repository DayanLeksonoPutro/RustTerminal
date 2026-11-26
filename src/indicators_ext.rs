//! Built-in indicators that programs reach through iDEMA, iAO, iMFI... handles.
//! Formulas follow the platform's standard definitions; buffer order matches the
//! buffer numbers CopyBuffer expects. Values not yet calculated are EMPTY_VALUE.

use crate::indicators::{applied_price, ma};
use crate::market::Bar;

pub const EMPTY: f64 = f64::MAX;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExtKind {
    Dema,
    Tema,
    Trix,
    Osma,
    Ao,
    Ac,
    Bears,
    Bulls,
    Force,
    Mfi,
    Obv,
    Rvi,
    StdDev,
    DeMarker,
    Chaikin,
    Ad,
    Bwmfi,
    Gator,
    Ama,
    Frama,
    Vidya,
    AdxWilder,
}

/// Parameters in the order the i-function takes them (after symbol and timeframe).
#[derive(Clone, Debug, Default)]
pub struct ExtParams {
    pub i: Vec<i64>,
    pub f: Vec<f64>,
}

impl ExtParams {
    fn i(&self, k: usize, d: i64) -> i64 {
        self.i.get(k).copied().unwrap_or(d)
    }
}

fn nan_to_empty(v: Vec<f64>) -> Vec<f64> {
    v.into_iter()
        .map(|x| {
            if x.is_nan() || !x.is_finite() {
                EMPTY
            } else {
                x
            }
        })
        .collect()
}

fn ema_from(src: &[f64], p: usize) -> Vec<f64> {
    ma(src, p, 1)
}

fn vol(b: &Bar) -> f64 {
    b.tick_volume as f64
}

/// Colour index buffer: 0 when the value rose (or stayed), 1 when it fell.
fn rise_fall(v: &[f64]) -> Vec<f64> {
    let mut c = vec![EMPTY; v.len()];
    for i in 1..v.len() {
        if v[i].is_finite() && v[i - 1].is_finite() {
            c[i] = if v[i] >= v[i - 1] { 0.0 } else { 1.0 };
        } else if v[i].is_finite() {
            c[i] = 0.0;
        }
    }
    c
}

pub fn compute(kind: ExtKind, p: &ExtParams, bars: &[Bar]) -> Vec<Vec<f64>> {
    let n = bars.len();
    let nan = f64::NAN;
    let price_of = |pr: i64| -> Vec<f64> { bars.iter().map(|b| applied_price(b, pr)).collect() };
    let close: Vec<f64> = bars.iter().map(|b| b.close).collect();
    let median: Vec<f64> = bars.iter().map(|b| (b.high + b.low) / 2.0).collect();
    let out: Vec<Vec<f64>> = match kind {
        // iDEMA(period, shift, price)
        ExtKind::Dema => {
            let pp = p.i(0, 14).max(1) as usize;
            let e1 = ema_from(&price_of(p.i(2, 1)), pp);
            let e2 = ema_from(&e1, pp);
            vec![(0..n).map(|i| 2.0 * e1[i] - e2[i]).collect()]
        }
        // iTEMA(period, shift, price)
        ExtKind::Tema => {
            let pp = p.i(0, 14).max(1) as usize;
            let e1 = ema_from(&price_of(p.i(2, 1)), pp);
            let e2 = ema_from(&e1, pp);
            let e3 = ema_from(&e2, pp);
            vec![(0..n).map(|i| 3.0 * e1[i] - 3.0 * e2[i] + e3[i]).collect()]
        }
        // iTriX(period, price)
        ExtKind::Trix => {
            let pp = p.i(0, 14).max(1) as usize;
            let e1 = ema_from(&price_of(p.i(1, 1)), pp);
            let e2 = ema_from(&e1, pp);
            let e3 = ema_from(&e2, pp);
            let mut t = vec![nan; n];
            for i in 1..n {
                if e3[i].is_finite() && e3[i - 1].is_finite() && e3[i - 1] != 0.0 {
                    t[i] = (e3[i] - e3[i - 1]) / e3[i - 1];
                }
            }
            vec![t]
        }
        // iOsMA(fast, slow, signal, price): MACD minus its simple-average signal
        ExtKind::Osma => {
            let pr = price_of(p.i(3, 1));
            let f = ema_from(&pr, p.i(0, 12).max(1) as usize);
            let s = ema_from(&pr, p.i(1, 26).max(1) as usize);
            let macd: Vec<f64> = (0..n).map(|i| f[i] - s[i]).collect();
            let sig = ma(&macd, p.i(2, 9).max(1) as usize, 0);
            vec![(0..n).map(|i| macd[i] - sig[i]).collect()]
        }
        // iAO(): SMA(median,5) - SMA(median,34); buffers value, colour
        ExtKind::Ao => {
            let a = ma(&median, 5, 0);
            let b = ma(&median, 34, 0);
            let v: Vec<f64> = (0..n).map(|i| a[i] - b[i]).collect();
            let c = rise_fall(&v);
            vec![v, c]
        }
        // iAC(): AO - SMA(AO,5); buffers value, colour
        ExtKind::Ac => {
            let a = ma(&median, 5, 0);
            let b = ma(&median, 34, 0);
            let ao: Vec<f64> = (0..n).map(|i| a[i] - b[i]).collect();
            let sm = ma(&ao, 5, 0);
            let v: Vec<f64> = (0..n).map(|i| ao[i] - sm[i]).collect();
            let c = rise_fall(&v);
            vec![v, c]
        }
        // iBearsPower(period): low - EMA(close)
        ExtKind::Bears => {
            let e = ema_from(&close, p.i(0, 13).max(1) as usize);
            vec![(0..n).map(|i| bars[i].low - e[i]).collect()]
        }
        // iBullsPower(period): high - EMA(close)
        ExtKind::Bulls => {
            let e = ema_from(&close, p.i(0, 13).max(1) as usize);
            vec![(0..n).map(|i| bars[i].high - e[i]).collect()]
        }
        // iForce(period, method, volume): volume * (MA[i] - MA[i-1])
        ExtKind::Force => {
            let m = ma(&close, p.i(0, 13).max(1) as usize, p.i(1, 0));
            let mut v = vec![nan; n];
            for i in 1..n {
                v[i] = vol(&bars[i]) * (m[i] - m[i - 1]);
            }
            vec![v]
        }
        // iMFI(period, volume)
        ExtKind::Mfi => {
            let pp = p.i(0, 14).max(1) as usize;
            let tp: Vec<f64> = bars
                .iter()
                .map(|b| (b.high + b.low + b.close) / 3.0)
                .collect();
            let mut v = vec![nan; n];
            for i in pp..n {
                let (mut pos, mut neg) = (0.0, 0.0);
                for j in (i + 1 - pp)..=i {
                    let flow = tp[j] * vol(&bars[j]);
                    if tp[j] > tp[j - 1] {
                        pos += flow;
                    } else if tp[j] < tp[j - 1] {
                        neg += flow;
                    }
                }
                v[i] = if neg != 0.0 {
                    100.0 - 100.0 / (1.0 + pos / neg)
                } else {
                    100.0
                };
            }
            vec![v]
        }
        // iOBV(volume)
        ExtKind::Obv => {
            let mut v = vec![nan; n];
            if n > 0 {
                v[0] = vol(&bars[0]);
                for i in 1..n {
                    v[i] = if close[i] > close[i - 1] {
                        v[i - 1] + vol(&bars[i])
                    } else if close[i] < close[i - 1] {
                        v[i - 1] - vol(&bars[i])
                    } else {
                        v[i - 1]
                    };
                }
            }
            vec![v]
        }
        // iRVI(period): buffers main, signal
        ExtKind::Rvi => {
            let pp = p.i(0, 10).max(1) as usize;
            let mut main = vec![nan; n];
            let mut sig = vec![nan; n];
            let up = |j: usize| -> f64 {
                let b = |k: usize| bars[k].close - bars[k].open;
                b(j) + 2.0 * b(j - 1) + 2.0 * b(j - 2) + b(j - 3)
            };
            let dn = |j: usize| -> f64 {
                let b = |k: usize| bars[k].high - bars[k].low;
                b(j) + 2.0 * b(j - 1) + 2.0 * b(j - 2) + b(j - 3)
            };
            for i in (pp + 2)..n {
                let (mut num, mut den) = (0.0, 0.0);
                for j in (i + 1 - pp)..=i {
                    num += up(j);
                    den += dn(j);
                }
                main[i] = if den != 0.0 { num / den } else { num };
            }
            for i in (pp + 5)..n {
                sig[i] = (main[i] + 2.0 * main[i - 1] + 2.0 * main[i - 2] + main[i - 3]) / 6.0;
            }
            vec![main, sig]
        }
        // iStdDev(period, shift, method, price)
        ExtKind::StdDev => {
            let pp = p.i(0, 20).max(1) as usize;
            let pr = price_of(p.i(3, 1));
            let m = ma(&pr, pp, p.i(2, 0));
            let mut v = vec![nan; n];
            for i in (pp - 1)..n {
                if m[i].is_finite() {
                    let s: f64 = pr[i + 1 - pp..=i].iter().map(|x| (x - m[i]).powi(2)).sum();
                    v[i] = (s / pp as f64).sqrt();
                }
            }
            vec![v]
        }
        // iDeMarker(period)
        ExtKind::DeMarker => {
            let pp = p.i(0, 14).max(1) as usize;
            let mut mx = vec![0.0; n];
            let mut mn = vec![0.0; n];
            for i in 1..n {
                mx[i] = if bars[i].high > bars[i - 1].high {
                    bars[i].high - bars[i - 1].high
                } else {
                    0.0
                };
                mn[i] = if bars[i].low < bars[i - 1].low {
                    bars[i - 1].low - bars[i].low
                } else {
                    0.0
                };
            }
            let smx = ma(&mx[1..].to_vec(), pp, 0);
            let smn = ma(&mn[1..].to_vec(), pp, 0);
            let mut v = vec![nan; n];
            for i in 1..n {
                let (a, b) = (smx[i - 1], smn[i - 1]);
                if a.is_finite() && b.is_finite() {
                    v[i] = if a + b != 0.0 { a / (a + b) } else { 0.0 };
                }
            }
            vec![v]
        }
        // iAD(volume)
        ExtKind::Ad => vec![ad_line(bars)],
        // iChaikin(fast, slow, method, volume)
        ExtKind::Chaikin => {
            let ad = ad_line(bars);
            let f = ma(&ad, p.i(0, 3).max(1) as usize, p.i(2, 1));
            let s = ma(&ad, p.i(1, 10).max(1) as usize, p.i(2, 1));
            vec![(0..n).map(|i| f[i] - s[i]).collect()]
        }
        // iBWMFI(volume): (high-low)/volume; colour 0..3 by MFI and volume direction
        ExtKind::Bwmfi => {
            let mut v = vec![nan; n];
            let mut c = vec![EMPTY; n];
            for i in 0..n {
                let vo = vol(&bars[i]);
                v[i] = if vo > 0.0 {
                    (bars[i].high - bars[i].low) / vo
                } else if i > 0 {
                    v[i - 1]
                } else {
                    0.0
                };
                if i > 0 {
                    let mfi_up = v[i] > v[i - 1];
                    let vol_up = vo > vol(&bars[i - 1]);
                    c[i] = match (mfi_up, vol_up) {
                        (true, true) => 0.0,
                        (false, false) => 1.0,
                        (true, false) => 2.0,
                        (false, true) => 3.0,
                    };
                }
            }
            vec![v, c]
        }
        // iGator(jaw, jaw_shift, teeth, teeth_shift, lips, lips_shift, method, price)
        ExtKind::Gator => {
            let pr = price_of(p.i(7, 5));
            let method = p.i(6, 2);
            let jaw = ma(&pr, p.i(0, 13).max(1) as usize, method);
            let teeth = ma(&pr, p.i(2, 8).max(1) as usize, method);
            let lips = ma(&pr, p.i(4, 5).max(1) as usize, method);
            let (js, ts, ls) = (p.i(1, 8), p.i(3, 5), p.i(5, 3));
            let at = |v: &Vec<f64>, i: i64| -> f64 {
                if i >= 0 && (i as usize) < v.len() {
                    v[i as usize]
                } else {
                    nan
                }
            };
            let mut upper = vec![nan; n];
            let mut lower = vec![nan; n];
            for i in 0..n as i64 {
                upper[i as usize] = (at(&jaw, i - js + ts) - at(&teeth, i)).abs();
                lower[i as usize] = -(at(&teeth, i - ts + ls) - at(&lips, i)).abs();
            }
            let uc = rise_fall(&upper);
            let lc: Vec<f64> = rise_fall(&lower.iter().map(|x| -x).collect::<Vec<_>>());
            vec![upper, uc, lower, lc]
        }
        // iAMA(period, fast, slow, shift, price)
        ExtKind::Ama => {
            let pp = p.i(0, 10).max(1) as usize;
            let fast = 2.0 / (p.i(1, 2).max(1) as f64 + 1.0);
            let slow = 2.0 / (p.i(2, 30).max(1) as f64 + 1.0);
            let pr = price_of(p.i(4, 1));
            let mut v = vec![nan; n];
            if n > pp {
                v[pp - 1] = pr[pp - 1];
                for i in pp..n {
                    let signal = (pr[i] - pr[i - pp]).abs();
                    let noise: f64 = (i + 1 - pp..=i).map(|k| (pr[k] - pr[k - 1]).abs()).sum();
                    let er = if noise != 0.0 { signal / noise } else { 0.0 };
                    let sc = (er * (fast - slow) + slow).powi(2);
                    v[i] = v[i - 1] + sc * (pr[i] - v[i - 1]);
                }
            }
            vec![v]
        }
        // iFrAMA(period, shift, price)
        ExtKind::Frama => {
            let pp = p.i(0, 14).max(1) as usize;
            let pr = price_of(p.i(2, 1));
            let mut v = vec![nan; n];
            let hl = |from: usize, to: usize| -> (f64, f64) {
                let mut h = f64::MIN;
                let mut l = f64::MAX;
                for b in &bars[from..=to] {
                    h = h.max(b.high);
                    l = l.min(b.low);
                }
                (h, l)
            };
            if n >= 2 * pp {
                v[2 * pp - 2] = pr[2 * pp - 2];
                for i in (2 * pp - 1)..n {
                    let (h1, l1) = hl(i + 1 - pp, i);
                    let (h2, l2) = hl(i + 1 - 2 * pp, i - pp);
                    let (h3, l3) = hl(i + 1 - 2 * pp, i);
                    let n1 = (h1 - l1) / pp as f64;
                    let n2 = (h2 - l2) / pp as f64;
                    let n3 = (h3 - l3) / (2 * pp) as f64;
                    let d = if n1 + n2 > 0.0 && n3 > 0.0 {
                        ((n1 + n2).ln() - n3.ln()) / 2f64.ln()
                    } else {
                        1.0
                    };
                    let alpha = (-4.6 * (d - 1.0)).exp().clamp(0.01, 1.0);
                    let prev = if v[i - 1].is_finite() {
                        v[i - 1]
                    } else {
                        pr[i - 1]
                    };
                    v[i] = alpha * pr[i] + (1.0 - alpha) * prev;
                }
            }
            vec![v]
        }
        // iADXWilder(period): buffers ADX, +DI, -DI with Wilder smoothing throughout
        ExtKind::AdxWilder => {
            let pp = p.i(0, 14).max(1) as usize;
            let mut adx = vec![nan; n];
            let mut pdi = vec![nan; n];
            let mut mdi = vec![nan; n];
            if n > pp + 1 {
                let (mut tr_s, mut pdm_s, mut mdm_s) = (0.0, 0.0, 0.0);
                let mut dx_hist: Vec<f64> = vec![];
                let mut prev_adx = f64::NAN;
                for i in 1..n {
                    let (h, l, pc) = (bars[i].high, bars[i].low, bars[i - 1].close);
                    let tr = (h - l).max((h - pc).abs()).max((l - pc).abs());
                    let up = h - bars[i - 1].high;
                    let dn = bars[i - 1].low - l;
                    let pdm = if up > dn && up > 0.0 { up } else { 0.0 };
                    let mdm = if dn > up && dn > 0.0 { dn } else { 0.0 };
                    if i <= pp {
                        tr_s += tr;
                        pdm_s += pdm;
                        mdm_s += mdm;
                        if i < pp {
                            continue;
                        }
                    } else {
                        tr_s = tr_s - tr_s / pp as f64 + tr;
                        pdm_s = pdm_s - pdm_s / pp as f64 + pdm;
                        mdm_s = mdm_s - mdm_s / pp as f64 + mdm;
                    }
                    let (pd, md) = if tr_s != 0.0 {
                        (100.0 * pdm_s / tr_s, 100.0 * mdm_s / tr_s)
                    } else {
                        (0.0, 0.0)
                    };
                    pdi[i] = pd;
                    mdi[i] = md;
                    let dx = if pd + md != 0.0 {
                        100.0 * (pd - md).abs() / (pd + md)
                    } else {
                        0.0
                    };
                    if prev_adx.is_nan() {
                        dx_hist.push(dx);
                        if dx_hist.len() == pp {
                            prev_adx = dx_hist.iter().sum::<f64>() / pp as f64;
                            adx[i] = prev_adx;
                        }
                    } else {
                        prev_adx = (prev_adx * (pp as f64 - 1.0) + dx) / pp as f64;
                        adx[i] = prev_adx;
                    }
                }
            }
            vec![adx, pdi, mdi]
        }
        // iVIDyA(cmo_period, ema_period, shift, price)
        ExtKind::Vidya => {
            let cp = p.i(0, 9).max(1) as usize;
            let ep = p.i(1, 12).max(1) as usize;
            let pr = price_of(p.i(3, 1));
            let alpha = 2.0 / (ep as f64 + 1.0);
            let mut v = vec![nan; n];
            if n > cp {
                v[cp - 1] = pr[cp - 1];
                for i in cp..n {
                    let (mut up, mut dn) = (0.0, 0.0);
                    for k in (i + 1 - cp)..=i {
                        let d = pr[k] - pr[k - 1];
                        if d > 0.0 {
                            up += d;
                        } else {
                            dn -= d;
                        }
                    }
                    let cmo = if up + dn != 0.0 {
                        ((up - dn) / (up + dn)).abs()
                    } else {
                        0.0
                    };
                    v[i] = pr[i] * alpha * cmo + v[i - 1] * (1.0 - alpha * cmo);
                }
            }
            vec![v]
        }
    };
    out.into_iter().map(nan_to_empty).collect()
}

fn ad_line(bars: &[Bar]) -> Vec<f64> {
    let mut v = vec![0.0; bars.len()];
    let mut acc = 0.0;
    for (i, b) in bars.iter().enumerate() {
        let range = b.high - b.low;
        if range != 0.0 {
            acc += ((b.close - b.low) - (b.high - b.close)) / range * vol(b);
        }
        v[i] = acc;
    }
    v
}

/// Map an i-function name to its kind.
pub fn by_function(name: &str) -> Option<ExtKind> {
    Some(match name {
        "iDEMA" => ExtKind::Dema,
        "iTEMA" => ExtKind::Tema,
        "iTriX" => ExtKind::Trix,
        "iOsMA" => ExtKind::Osma,
        "iAO" => ExtKind::Ao,
        "iAC" => ExtKind::Ac,
        "iBearsPower" => ExtKind::Bears,
        "iBullsPower" => ExtKind::Bulls,
        "iForce" => ExtKind::Force,
        "iMFI" => ExtKind::Mfi,
        "iOBV" => ExtKind::Obv,
        "iRVI" => ExtKind::Rvi,
        "iStdDev" => ExtKind::StdDev,
        "iDeMarker" => ExtKind::DeMarker,
        "iChaikin" => ExtKind::Chaikin,
        "iAD" => ExtKind::Ad,
        "iBWMFI" => ExtKind::Bwmfi,
        "iGator" => ExtKind::Gator,
        "iAMA" => ExtKind::Ama,
        "iFrAMA" => ExtKind::Frama,
        "iVIDyA" => ExtKind::Vidya,
        "iADXWilder" => ExtKind::AdxWilder,
        _ => return None,
    })
}

/// Which parameters of the i-function are doubles (by position after symbol, timeframe).
pub fn double_params(kind: ExtKind) -> &'static [usize] {
    let _ = kind;
    &[]
}

pub const FUNCTIONS: [&str; 22] = [
    "iADXWilder",
    "iDEMA",
    "iTEMA",
    "iTriX",
    "iOsMA",
    "iAO",
    "iAC",
    "iBearsPower",
    "iBullsPower",
    "iForce",
    "iMFI",
    "iOBV",
    "iRVI",
    "iStdDev",
    "iDeMarker",
    "iChaikin",
    "iAD",
    "iBWMFI",
    "iGator",
    "iAMA",
    "iFrAMA",
    "iVIDyA",
];
