//! Indicator values as programs read them through CopyBuffer. These follow the
//! platform's reference algorithms step by step, including what the first bars hold
//! (0.0 while an average warms up, the first price for an EMA, EMPTY_VALUE where nothing
//! is written), so expert advisors that read early bars see the same numbers.
//! The chart draws from indicators.rs, which hides warm-up bars instead.

use crate::indicators::{applied_price, IndKind, IndParams};
use crate::indicators_ext::{ExtKind, ExtParams};
use crate::market::Bar;

pub const EMPTY: f64 = f64::MAX;

// ---------- moving averages (the platform's MovingAverages library) ----------

/// SimpleMAOnBuffer on a first calculation.
pub fn sma_on(price: &[f64], period: usize, begin: usize, buf: &mut [f64]) {
    let n = price.len();
    if period <= 1 || period > n.saturating_sub(begin) {
        return;
    }
    let start = period + begin;
    for b in buf.iter_mut().take(start - 1) {
        *b = 0.0;
    }
    let first: f64 = price[begin..start].iter().sum();
    buf[start - 1] = first / period as f64;
    for i in start..n {
        buf[i] = buf[i - 1] + (price[i] - price[i - period]) / period as f64;
    }
}

/// ExponentialMAOnBuffer on a first calculation.
pub fn ema_on(price: &[f64], period: usize, begin: usize, buf: &mut [f64]) {
    let n = price.len();
    if period <= 1 || period > n.saturating_sub(begin) {
        return;
    }
    let k = 2.0 / (1.0 + period as f64);
    for b in buf.iter_mut().take(begin) {
        *b = 0.0;
    }
    buf[begin] = price[begin];
    for i in begin + 1..n {
        buf[i] = price[i] * k + buf[i - 1] * (1.0 - k);
    }
}

/// SmoothedMAOnBuffer on a first calculation.
pub fn smma_on(price: &[f64], period: usize, begin: usize, buf: &mut [f64]) {
    let n = price.len();
    if period <= 1 || period > n.saturating_sub(begin) {
        return;
    }
    let start = period + begin;
    for b in buf.iter_mut().take(start - 1) {
        *b = 0.0;
    }
    let first: f64 = price[begin..start].iter().sum();
    buf[start - 1] = first / period as f64;
    for i in start..n {
        buf[i] = (buf[i - 1] * (period as f64 - 1.0) + price[i]) / period as f64;
    }
}

/// LinearWeightedMAOnBuffer on a first calculation.
pub fn lwma_on(price: &[f64], period: usize, begin: usize, buf: &mut [f64]) {
    let n = price.len();
    if period <= 1 || period > n.saturating_sub(begin) {
        return;
    }
    let start = period + begin;
    for b in buf.iter_mut().take(start) {
        *b = 0.0;
    }
    let (mut sum, mut lsum, mut weight) = (0.0, 0.0, 0.0);
    for (l, i) in (start - period..start).enumerate() {
        let w = (l + 1) as f64;
        sum += price[i] * w;
        lsum += price[i];
        weight += w;
    }
    buf[start - 1] = sum / weight;
    for i in start..n {
        sum = sum - lsum + price[i] * period as f64;
        lsum = lsum - price[i - period] + price[i];
        buf[i] = sum / weight;
    }
}

/// Average by method (0 SMA, 1 EMA, 2 SMMA, 3 LWMA) the way the library functions do.
pub fn average_on(method: i64, price: &[f64], period: usize, begin: usize, buf: &mut [f64]) {
    match method {
        1 => ema_on(price, period, begin, buf),
        2 => smma_on(price, period, begin, buf),
        3 => lwma_on(price, period, begin, buf),
        _ => sma_on(price, period, begin, buf),
    }
}

/// SimpleMA(position, period, price): 0 until enough bars.
fn simple_ma(pos: usize, period: usize, price: &[f64]) -> f64 {
    if period > 0 && period <= pos + 1 {
        price[pos + 1 - period..=pos].iter().sum::<f64>() / period as f64
    } else {
        0.0
    }
}

fn exponential_ma(pos: usize, period: usize, prev: f64, price: &[f64]) -> f64 {
    if period > 0 {
        let pr = 2.0 / (period as f64 + 1.0);
        price[pos] * pr + prev * (1.0 - pr)
    } else {
        0.0
    }
}

fn smoothed_ma(pos: usize, period: usize, prev: f64, price: &[f64]) -> f64 {
    if period > 0 && period <= pos + 1 {
        if pos == period - 1 {
            return price[pos + 1 - period..=pos].iter().sum::<f64>() / period as f64;
        }
        (prev * (period as f64 - 1.0) + price[pos]) / period as f64
    } else {
        0.0
    }
}

fn lwma_at(pos: usize, period: usize, price: &[f64]) -> f64 {
    if period > 0 && period <= pos + 1 {
        let (mut sum, mut wsum) = (0.0, 0.0);
        for k in 0..period {
            let w = (period - k) as f64;
            sum += price[pos - k] * w;
            wsum += w;
        }
        sum / wsum
    } else {
        0.0
    }
}

/// The built-in moving average (iMA): Custom Moving Average with begin 0.
pub fn ima(price: &[f64], period: usize, method: i64) -> Vec<f64> {
    let n = price.len();
    let mut buf = vec![EMPTY; n];
    let p = period.max(1);
    if n == 0 || n + 1 < p {
        return buf;
    }
    if p == 1 {
        return price.to_vec();
    }
    if n < p {
        return buf;
    }
    match method {
        1 => {
            let k = 2.0 / (1.0 + p as f64);
            buf[0] = price[0];
            for i in 1..n {
                buf[i] = price[i] * k + buf[i - 1] * (1.0 - k);
            }
        }
        2 => smma_on(price, p, 0, &mut buf),
        3 => lwma_on(price, p, 0, &mut buf),
        _ => sma_on(price, p, 0, &mut buf),
    }
    buf
}

fn highest_from(a: &[f64], period: usize, cur: usize) -> f64 {
    let lo = (cur + 1).saturating_sub(period);
    a[lo..=cur].iter().cloned().fold(f64::MIN, f64::max)
}
fn lowest_from(a: &[f64], period: usize, cur: usize) -> f64 {
    let lo = (cur + 1).saturating_sub(period);
    a[lo..=cur].iter().cloned().fold(f64::MAX, f64::min)
}

/// Values of a built-in indicator, buffer by buffer in CopyBuffer order.
pub fn builtin(kind: IndKind, p: &IndParams, bars: &[Bar]) -> Vec<Vec<f64>> {
    let n = bars.len();
    let high: Vec<f64> = bars.iter().map(|b| b.high).collect();
    let low: Vec<f64> = bars.iter().map(|b| b.low).collect();
    let close: Vec<f64> = bars.iter().map(|b| b.close).collect();
    let price: Vec<f64> = bars.iter().map(|b| applied_price(b, p.price)).collect();
    let p1 = p.p1.max(1) as usize;
    let p2 = p.p2.max(1) as usize;
    let p3 = p.p3.max(1) as usize;
    let mut out: Vec<Vec<f64>> = vec![];
    match kind {
        IndKind::MovingAverage => out.push(ima(&price, p1, p.method)),
        IndKind::Rsi => {
            let mut rsi = vec![EMPTY; n];
            if n > p1 {
                let (mut pos_b, mut neg_b) = (vec![0.0; n], vec![0.0; n]);
                rsi[0] = 0.0;
                let (mut sp, mut sn) = (0.0, 0.0);
                for i in 1..=p1 {
                    rsi[i] = 0.0;
                    let d = price[i] - price[i - 1];
                    sp += if d > 0.0 { d } else { 0.0 };
                    sn += if d < 0.0 { -d } else { 0.0 };
                }
                pos_b[p1] = sp / p1 as f64;
                neg_b[p1] = sn / p1 as f64;
                rsi[p1] = rsi_value(pos_b[p1], neg_b[p1]);
                for i in p1 + 1..n {
                    let d = price[i] - price[i - 1];
                    pos_b[i] = (pos_b[i - 1] * (p1 as f64 - 1.0) + if d > 0.0 { d } else { 0.0 })
                        / p1 as f64;
                    neg_b[i] = (neg_b[i - 1] * (p1 as f64 - 1.0) + if d < 0.0 { -d } else { 0.0 })
                        / p1 as f64;
                    rsi[i] = rsi_value(pos_b[i], neg_b[i]);
                }
            }
            out.push(rsi);
        }
        IndKind::Macd => {
            let f = ima(&price, p1, 1);
            let s = ima(&price, p2, 1);
            let macd: Vec<f64> = (0..n).map(|i| f[i] - s[i]).collect();
            let mut sig = vec![EMPTY; n];
            sma_on(&macd, p3, 0, &mut sig);
            out.push(macd);
            out.push(sig);
        }
        IndKind::Bands => {
            let mut ml = vec![EMPTY; n];
            let mut tl = vec![EMPTY; n];
            let mut bl = vec![EMPTY; n];
            if n >= p1 {
                for i in 0..n {
                    ml[i] = simple_ma(i, p1, &price);
                    let sd = if i >= p1 {
                        let s: f64 = (0..p1).map(|k| (price[i - k] - ml[i]).powi(2)).sum();
                        (s / p1 as f64).sqrt()
                    } else {
                        0.0
                    };
                    tl[i] = ml[i] + p.f1 * sd;
                    bl[i] = ml[i] - p.f1 * sd;
                }
            }
            out.extend([ml, tl, bl]);
        }
        IndKind::Stochastic => {
            let (k, d, sl) = (p1, p2, p3);
            let (hi_src, lo_src) = (&high, &low);
            let mut main = vec![EMPTY; n];
            let mut sig = vec![EMPTY; n];
            if n > k + d + sl {
                let mut lows = vec![0.0; n];
                let mut highs = vec![0.0; n];
                for i in k - 1..n {
                    lows[i] = lowest_from(lo_src, k, i);
                    highs[i] = highest_from(hi_src, k, i);
                }
                let start = k - 1 + sl - 1;
                for m in main.iter_mut().take(start) {
                    *m = 0.0;
                }
                for i in start..n {
                    let (mut sum_low, mut sum_high) = (0.0, 0.0);
                    for j in i + 1 - sl..=i {
                        sum_low += close[j] - lows[j];
                        sum_high += highs[j] - lows[j];
                    }
                    main[i] = if sum_high == 0.0 {
                        100.0
                    } else {
                        sum_low / sum_high * 100.0
                    };
                }
                if p.method == 0 {
                    for s in sig.iter_mut().take(d - 1) {
                        *s = 0.0;
                    }
                    for i in d - 1..n {
                        sig[i] = (0..d).map(|j| main[i - j]).sum::<f64>() / d as f64;
                    }
                } else {
                    average_on(p.method, &main, d, start, &mut sig);
                }
            }
            out.extend([main, sig]);
        }
        IndKind::Atr => {
            let mut atr = vec![EMPTY; n];
            if n > p1 {
                let mut tr = vec![0.0; n];
                for i in 1..n {
                    tr[i] = high[i].max(close[i - 1]) - low[i].min(close[i - 1]);
                }
                atr[0] = 0.0;
                let mut first = 0.0;
                for i in 1..=p1 {
                    atr[i] = 0.0;
                    first += tr[i];
                }
                atr[p1] = first / p1 as f64;
                for i in p1 + 1..n {
                    atr[i] = atr[i - 1] + (tr[i] - tr[i - p1]) / p1 as f64;
                }
            }
            out.push(atr);
        }
        IndKind::Cci => {
            let mut cci = vec![EMPTY; n];
            let start = p1 - 1;
            if n >= start {
                let mult = 0.015 / p1 as f64;
                for i in start..n {
                    let sp = simple_ma(i, p1, &price);
                    let dev: f64 = (0..p1).map(|j| (price[i - j] - sp).abs()).sum::<f64>() * mult;
                    cci[i] = if dev != 0.0 {
                        (price[i] - sp) / dev
                    } else {
                        0.0
                    };
                }
            }
            out.push(cci);
        }
        IndKind::Momentum => {
            let mut m = vec![EMPTY; n];
            if n + 1 >= p1 {
                for i in p1..n {
                    m[i] = price[i] * 100.0 / price[i - p1];
                }
            }
            out.push(m);
        }
        IndKind::Adx => {
            let mut adx = vec![EMPTY; n];
            let mut pdi = vec![EMPTY; n];
            let mut ndi = vec![EMPTY; n];
            if n >= p1 && n > 1 {
                let mut pd = vec![0.0; n];
                let mut nd = vec![0.0; n];
                let mut tmp = vec![0.0; n];
                pdi[0] = 0.0;
                ndi[0] = 0.0;
                adx[0] = 0.0;
                for i in 1..n {
                    let mut up = high[i] - high[i - 1];
                    let mut dn = low[i - 1] - low[i];
                    if up < 0.0 {
                        up = 0.0;
                    }
                    if dn < 0.0 {
                        dn = 0.0;
                    }
                    if up > dn {
                        dn = 0.0;
                    } else if up < dn {
                        up = 0.0;
                    } else {
                        up = 0.0;
                        dn = 0.0;
                    }
                    let tr = (high[i] - low[i])
                        .abs()
                        .max((high[i] - close[i - 1]).abs())
                        .max((low[i] - close[i - 1]).abs());
                    if tr != 0.0 {
                        pd[i] = 100.0 * up / tr;
                        nd[i] = 100.0 * dn / tr;
                    }
                    pdi[i] = exponential_ma(i, p1, pdi[i - 1], &pd);
                    ndi[i] = exponential_ma(i, p1, ndi[i - 1], &nd);
                    let s = pdi[i] + ndi[i];
                    tmp[i] = if s != 0.0 {
                        100.0 * ((pdi[i] - ndi[i]) / s).abs()
                    } else {
                        0.0
                    };
                    adx[i] = exponential_ma(i, p1, adx[i - 1], &tmp);
                }
            }
            out.extend([adx, pdi, ndi]);
        }
        IndKind::Wpr => {
            let mut w = vec![EMPTY; n];
            if n >= p1 {
                for x in w.iter_mut().take(p1 - 1) {
                    *x = 0.0;
                }
                for i in p1 - 1..n {
                    let hh = highest_from(&high, p1, i);
                    let ll = lowest_from(&low, p1, i);
                    w[i] = if hh != ll {
                        -(hh - close[i]) * 100.0 / (hh - ll)
                    } else if i > 0 {
                        w[i - 1]
                    } else {
                        0.0
                    };
                }
            }
            out.push(w);
        }
        IndKind::Envelopes => {
            let m = ima(&price, p1, p.method);
            let mut up = vec![EMPTY; n];
            let mut dn = vec![EMPTY; n];
            if n >= p1 {
                for i in p1..n {
                    up[i] = (1.0 + p.f1 / 100.0) * m[i];
                    dn[i] = (1.0 - p.f1 / 100.0) * m[i];
                }
            }
            out.extend([up, dn]);
        }
        IndKind::Ichimoku => {
            let mut tenkan = vec![EMPTY; n];
            let mut kijun = vec![EMPTY; n];
            let mut span_a = vec![EMPTY; n];
            let mut span_b = vec![EMPTY; n];
            let chikou = close.clone();
            for i in 0..n {
                tenkan[i] = (highest_from(&high, p1, i) + lowest_from(&low, p1, i)) / 2.0;
                kijun[i] = (highest_from(&high, p2, i) + lowest_from(&low, p2, i)) / 2.0;
                span_a[i] = (tenkan[i] + kijun[i]) / 2.0;
                span_b[i] = (highest_from(&high, p3, i) + lowest_from(&low, p3, i)) / 2.0;
            }
            out.extend([tenkan, kijun, span_a, span_b, chikou]);
        }
        IndKind::Sar => out.push(sar(&high, &low, p.f1, p.f2)),
        IndKind::Alligator => {
            out.push(ima(&price, p1, p.method));
            out.push(ima(&price, p2, p.method));
            out.push(ima(&price, p3, p.method));
        }
        IndKind::Fractals => {
            let mut up = vec![EMPTY; n];
            let mut dn = vec![EMPTY; n];
            if n >= 5 {
                for i in 2..n - 3 {
                    if high[i] > high[i + 1]
                        && high[i] > high[i + 2]
                        && high[i] >= high[i - 1]
                        && high[i] >= high[i - 2]
                    {
                        up[i] = high[i];
                    }
                    if low[i] < low[i + 1]
                        && low[i] < low[i + 2]
                        && low[i] <= low[i - 1]
                        && low[i] <= low[i - 2]
                    {
                        dn[i] = low[i];
                    }
                }
            }
            out.extend([up, dn]);
        }
        IndKind::Volumes => {
            let v: Vec<f64> = bars.iter().map(|b| b.tick_volume as f64).collect();
            let mut c = vec![EMPTY; n];
            if n > 0 {
                c[0] = 0.0;
                for i in 1..n {
                    c[i] = if v[i] > v[i - 1] { 0.0 } else { 1.0 };
                }
            }
            out.extend([v, c]);
        }
    }
    out
}

fn rsi_value(pos: f64, neg: f64) -> f64 {
    if neg != 0.0 {
        100.0 - 100.0 / (1.0 + pos / neg)
    } else if pos != 0.0 {
        100.0
    } else {
        50.0
    }
}

fn sar(high: &[f64], low: &[f64], step: f64, maximum: f64) -> Vec<f64> {
    let n = high.len();
    let mut sar = vec![EMPTY; n];
    if n < 3 {
        return sar;
    }
    let mut ep = vec![0.0; n];
    let mut af = vec![0.0; n];
    let get_high =
        |cur: usize, start: usize| high[start..=cur].iter().cloned().fold(f64::MIN, f64::max);
    let get_low =
        |cur: usize, start: usize| low[start..=cur].iter().cloned().fold(f64::MAX, f64::min);
    af[0] = step;
    af[1] = step;
    sar[0] = high[0];
    let mut last_rev = 0usize;
    let mut long = false;
    sar[1] = get_high(1, last_rev);
    ep[0] = low[1];
    ep[1] = low[1];
    for i in 1..n - 1 {
        if long {
            if sar[i] > low[i] {
                long = false;
                sar[i] = get_high(i, last_rev);
                ep[i] = low[i];
                last_rev = i;
                af[i] = step;
            }
        } else if sar[i] < high[i] {
            long = true;
            sar[i] = get_low(i, last_rev);
            ep[i] = high[i];
            last_rev = i;
            af[i] = step;
        }
        if long {
            if high[i] > ep[i - 1] && i != last_rev {
                ep[i] = high[i];
                af[i] = (af[i - 1] + step).min(maximum);
            } else if i != last_rev {
                af[i] = af[i - 1];
                ep[i] = ep[i - 1];
            }
            sar[i + 1] = sar[i] + af[i] * (ep[i] - sar[i]);
            if sar[i + 1] > low[i] || sar[i + 1] > low[i - 1] {
                sar[i + 1] = low[i].min(low[i - 1]);
            }
        } else {
            if low[i] < ep[i - 1] && i != last_rev {
                ep[i] = low[i];
                af[i] = (af[i - 1] + step).min(maximum);
            } else if i != last_rev {
                af[i] = af[i - 1];
                ep[i] = ep[i - 1];
            }
            sar[i + 1] = sar[i] + af[i] * (ep[i] - sar[i]);
            if sar[i + 1] < high[i] || sar[i + 1] < high[i - 1] {
                sar[i + 1] = high[i].max(high[i - 1]);
            }
        }
    }
    sar
}

/// Values of the extra built-in indicators (iDEMA, iAO, ...), in CopyBuffer order.
/// Returns None for kinds computed elsewhere.
pub fn ext(kind: ExtKind, p: &ExtParams, bars: &[Bar], point: f64) -> Option<Vec<Vec<f64>>> {
    let n = bars.len();
    let pi = |k: usize, d: i64| p.i.get(k).copied().unwrap_or(d);
    let price_of = |pr: i64| -> Vec<f64> { bars.iter().map(|b| applied_price(b, pr)).collect() };
    let high: Vec<f64> = bars.iter().map(|b| b.high).collect();
    let low: Vec<f64> = bars.iter().map(|b| b.low).collect();
    let close: Vec<f64> = bars.iter().map(|b| b.close).collect();
    let open: Vec<f64> = bars.iter().map(|b| b.open).collect();
    let vol: Vec<f64> = bars.iter().map(|b| b.tick_volume as f64).collect();
    let median: Vec<f64> = bars.iter().map(|b| (b.high + b.low) / 2.0).collect();
    let e = || vec![EMPTY; n];
    Some(match kind {
        ExtKind::Dema => {
            let pp = pi(0, 14).max(1) as usize;
            let pr = price_of(pi(2, 1));
            let (mut e1, mut e2, mut out) = (e(), e(), e());
            if n + 2 >= 2 * pp {
                ema_on(&pr, pp, 0, &mut e1);
                ema_on(&e1, pp, pp - 1, &mut e2);
                for i in 0..n {
                    out[i] = 2.0 * e1[i] - e2[i];
                }
            }
            vec![out]
        }
        ExtKind::Tema => {
            let pp = pi(0, 14).max(1) as usize;
            let pr = price_of(pi(2, 1));
            let (mut e1, mut e2, mut e3, mut out) = (e(), e(), e(), e());
            if n + 3 >= 3 * pp {
                ema_on(&pr, pp, 0, &mut e1);
                ema_on(&e1, pp, pp - 1, &mut e2);
                ema_on(&e2, pp, 2 * pp - 2, &mut e3);
                for i in 0..n {
                    out[i] = 3.0 * e1[i] - 3.0 * e2[i] + e3[i];
                }
            }
            vec![out]
        }
        ExtKind::Trix => {
            let pp = pi(0, 14).max(1) as usize;
            let pr = price_of(pi(1, 1));
            let (mut e1, mut e2, mut e3, mut out) = (e(), e(), e(), e());
            if n + 3 >= 3 * pp {
                ema_on(&pr, pp, 0, &mut e1);
                ema_on(&e1, pp, pp - 1, &mut e2);
                ema_on(&e2, pp, 2 * pp - 2, &mut e3);
                for i in 3 * (pp - 1)..n {
                    if i == 0 {
                        continue;
                    }
                    out[i] = if e3[i - 1] != 0.0 {
                        (e3[i] - e3[i - 1]) / e3[i - 1]
                    } else {
                        0.0
                    };
                }
            }
            vec![out]
        }
        ExtKind::Osma => {
            let pr = price_of(pi(3, 1));
            let f = ima(&pr, pi(0, 12).max(1) as usize, 1);
            let s = ima(&pr, pi(1, 26).max(1) as usize, 1);
            let macd: Vec<f64> = (0..n).map(|i| f[i] - s[i]).collect();
            let mut sig = e();
            sma_on(&macd, pi(2, 9).max(1) as usize, 0, &mut sig);
            vec![(0..n).map(|i| macd[i] - sig[i]).collect()]
        }
        ExtKind::Ao => {
            let f = ima(&median, 5, 0);
            let s = ima(&median, 34, 0);
            let (mut ao, mut c) = (e(), e());
            if n > 33 {
                for x in ao.iter_mut().take(33) {
                    *x = 0.0;
                }
                for i in 33..n {
                    ao[i] = f[i] - s[i];
                    c[i] = if ao[i] > ao[i - 1] { 0.0 } else { 1.0 };
                }
            }
            vec![ao, c]
        }
        ExtKind::Ac => {
            let f = ima(&median, 5, 0);
            let s = ima(&median, 34, 0);
            let (mut ac, mut c, mut ao) = (e(), e(), e());
            if n >= 37 {
                for i in 0..33 {
                    ac[i] = 0.0;
                    ao[i] = 0.0;
                }
                for i in 33..37 {
                    ac[i] = 0.0;
                    ao[i] = f[i] - s[i];
                }
                for i in 37..n {
                    ao[i] = f[i] - s[i];
                    let sm: f64 = (0..5).map(|j| ao[i - j]).sum::<f64>() / 5.0;
                    ac[i] = ao[i] - sm;
                    c[i] = if ac[i] >= ac[i - 1] { 0.0 } else { 1.0 };
                }
            }
            vec![ac, c]
        }
        ExtKind::Bears | ExtKind::Bulls => {
            let pp = pi(0, 13).max(1) as usize;
            let m = ima(&close, pp, 1);
            let mut v = e();
            if n >= pp {
                for i in pp..n {
                    v[i] = if kind == ExtKind::Bears {
                        low[i] - m[i]
                    } else {
                        high[i] - m[i]
                    };
                }
            }
            vec![v]
        }
        ExtKind::Force => {
            let pp = pi(0, 13).max(1) as usize;
            let m = ima(&close, pp, pi(1, 0));
            let mut v = e();
            for i in pp..n {
                v[i] = vol[i] * (m[i] - m[i - 1]);
            }
            vec![v]
        }
        ExtKind::Rvi => {
            let pp = pi(0, 10).max(1) as usize;
            let (mut main, mut sig) = (e(), e());
            if n > pp + 8 {
                for x in main.iter_mut().take(pp + 3) {
                    *x = 0.0;
                }
                for x in sig.iter_mut().take(pp + 6) {
                    *x = 0.0;
                }
                let up = |j: usize| {
                    close[j] - open[j]
                        + 2.0 * (close[j - 1] - open[j - 1])
                        + 2.0 * (close[j - 2] - open[j - 2])
                        + close[j - 3]
                        - open[j - 3]
                };
                let dn = |j: usize| {
                    high[j] - low[j]
                        + 2.0 * (high[j - 1] - low[j - 1])
                        + 2.0 * (high[j - 2] - low[j - 2])
                        + high[j - 3]
                        - low[j - 3]
                };
                for i in pp + 2..n {
                    let (mut su, mut sd) = (0.0, 0.0);
                    for j in (i + 1 - pp..=i).rev() {
                        su += up(j);
                        sd += dn(j);
                    }
                    main[i] = if sd != 0.0 { su / sd } else { su };
                }
                for i in pp + 5..n {
                    sig[i] = (main[i] + 2.0 * main[i - 1] + 2.0 * main[i - 2] + main[i - 3]) / 6.0;
                }
            }
            vec![main, sig]
        }
        ExtKind::StdDev => {
            let pp = pi(0, 20).max(1) as usize;
            let method = pi(2, 0);
            let pr = price_of(pi(3, 1));
            let mut sd = vec![0.0; n];
            let mut m = vec![0.0; n];
            if n >= pp {
                for i in pp - 1..n {
                    m[i] = match method {
                        1 if i != pp - 1 => exponential_ma(i, pp, m[i - 1], &pr),
                        2 if i != pp - 1 => smoothed_ma(i, pp, m[i - 1], &pr),
                        3 => lwma_at(i, pp, &pr),
                        _ => simple_ma(i, pp, &pr),
                    };
                    let s: f64 = (0..pp).map(|k| (pr[i - k] - m[i]).powi(2)).sum();
                    sd[i] = (s / pp as f64).sqrt();
                }
            } else {
                sd = e();
            }
            vec![sd]
        }
        ExtKind::DeMarker => {
            let pp = pi(0, 14).max(1) as usize;
            let mut dem = e();
            if n >= pp {
                let mut mx = vec![0.0; n];
                let mut mn = vec![0.0; n];
                for i in 1..n {
                    mx[i] = if high[i] > high[i - 1] {
                        high[i] - high[i - 1]
                    } else {
                        0.0
                    };
                    mn[i] = if low[i - 1] > low[i] {
                        low[i - 1] - low[i]
                    } else {
                        0.0
                    };
                }
                for x in dem.iter_mut().take(pp) {
                    *x = 0.0;
                }
                for i in pp.max(2) - 1..n {
                    if i == 0 {
                        continue;
                    }
                    let a = simple_ma(i, pp, &mx);
                    let b = simple_ma(i, pp, &mn);
                    dem[i] = if a + b != 0.0 { a / (a + b) } else { 0.0 };
                }
            }
            vec![dem]
        }
        ExtKind::Chaikin => {
            let (fp, sp) = (pi(0, 3).max(1) as usize, pi(1, 10).max(1) as usize);
            let method = pi(2, 1);
            let mut ad = vec![0.0; n];
            for i in 0..n {
                ad[i] = ad_step(high[i], low[i], close[i], vol[i]);
                if i > 0 {
                    ad[i] += ad[i - 1];
                }
            }
            let (mut f, mut s) = (e(), e());
            let mut out = e();
            if n >= sp {
                average_on(method, &ad, fp, 0, &mut f);
                average_on(method, &ad, sp, 0, &mut s);
                for i in 0..n {
                    out[i] = f[i] - s[i];
                }
            }
            vec![out]
        }
        ExtKind::Bwmfi => {
            let pt = if point > 0.0 { point } else { 1.0 };
            let mut v = e();
            let mut c = e();
            let (mut mfi_up, mut vol_up) = (true, true);
            for i in 0..n {
                v[i] = if vol[i] == 0.0 {
                    if i > 0 {
                        v[i - 1]
                    } else {
                        0.0
                    }
                } else {
                    (high[i] - low[i]) / pt / vol[i]
                };
                if i > 0 {
                    if v[i] > v[i - 1] {
                        mfi_up = true;
                    }
                    if v[i] < v[i - 1] {
                        mfi_up = false;
                    }
                    if vol[i] > vol[i - 1] {
                        vol_up = true;
                    }
                    if vol[i] < vol[i - 1] {
                        vol_up = false;
                    }
                }
                c[i] = match (mfi_up, vol_up) {
                    (true, true) => 0.0,
                    (false, false) => 1.0,
                    (true, false) => 2.0,
                    (false, true) => 3.0,
                };
            }
            vec![v, c]
        }
        ExtKind::Gator => {
            let (jp, js, tp, ts, lp, ls) = (
                pi(0, 13).max(1) as usize,
                pi(1, 8),
                pi(2, 8).max(1) as usize,
                pi(3, 5),
                pi(4, 5).max(1) as usize,
                pi(5, 3),
            );
            let method = pi(6, 2);
            let pr = price_of(pi(7, 5));
            let jaw = ima(&pr, jp, method);
            let teeth = ima(&pr, tp, method);
            let lips = ima(&pr, lp, method);
            let us = (js - ts).max(0) as usize;
            let lsft = (ts - ls).max(0) as usize;
            let shift = us.max(lsft);
            let (mut up, mut uc, mut lo, mut lc) = (e(), e(), e(), e());
            if shift <= n {
                for i in 0..shift.min(n) {
                    up[i] = 0.0;
                    uc[i] = 0.0;
                    lo[i] = 0.0;
                    lc[i] = 0.0;
                }
                let lower_limit = lsft + ls.max(0) as usize + lp;
                let upper_limit = us + ts.max(0) as usize + tp;
                for i in shift.max(1)..n {
                    if i >= lower_limit {
                        let cur = -(teeth[i - lsft] - lips[i]).abs();
                        let prev = lo[i - 1];
                        lo[i] = cur;
                        lc[i] = if prev == cur {
                            lc[i - 1]
                        } else if prev < cur {
                            1.0
                        } else {
                            0.0
                        };
                    } else {
                        lo[i] = 0.0;
                        lc[i] = 0.0;
                    }
                    if i >= upper_limit {
                        let cur = (jaw[i - us] - teeth[i]).abs();
                        up[i] = cur;
                        let prev = up[i - 1];
                        uc[i] = if prev == cur {
                            uc[i - 1]
                        } else if prev < cur {
                            0.0
                        } else {
                            1.0
                        };
                    } else {
                        up[i] = 0.0;
                        uc[i] = 0.0;
                    }
                }
            }
            vec![up, uc, lo, lc]
        }
        ExtKind::Ama => {
            let pp = pi(0, 10).max(1) as usize;
            let fast = 2.0 / (pi(1, 2) as f64 + 1.0);
            let slow = 2.0 / (pi(2, 30) as f64 + 1.0);
            let pr = price_of(pi(4, 1));
            let mut v = e();
            if n >= pp && pp >= 1 {
                for x in v.iter_mut().take(pp - 1) {
                    *x = 0.0;
                }
                v[pp - 1] = pr[pp - 1];
                for i in pp..n {
                    let signal = (pr[i] - pr[i - pp]).abs();
                    let mut noise = 0.0;
                    for dlt in 0..pp {
                        if i >= dlt + 1 {
                            noise += (pr[i - dlt] - pr[i - dlt - 1]).abs();
                        }
                    }
                    let er = if noise != 0.0 { signal / noise } else { 0.0 };
                    let ssc = er * (fast - slow) + slow;
                    v[i] = ssc.powi(2) * (pr[i] - v[i - 1]) + v[i - 1];
                }
            }
            vec![v]
        }
        ExtKind::Frama => {
            let pp = pi(0, 14).max(1) as usize;
            let pr = price_of(pi(2, 1));
            let mut v = e();
            if n >= 2 * pp {
                let start = 2 * pp - 1;
                for i in 0..=start {
                    v[i] = pr[i];
                }
                let hl = |from: usize, to: usize| -> (f64, f64) {
                    (
                        high[from..=to].iter().cloned().fold(f64::MIN, f64::max),
                        low[from..=to].iter().cloned().fold(f64::MAX, f64::min),
                    )
                };
                let ln2 = 2f64.ln();
                for i in start..n {
                    let (h1, l1) = hl(i + 1 - pp, i);
                    let (h2, l2) = hl(i + 1 - 2 * pp, i - pp);
                    let (h3, l3) = hl(i + 1 - 2 * pp, i);
                    let n1 = (h1 - l1) / pp as f64;
                    let n2 = (h2 - l2) / pp as f64;
                    let n3 = (h3 - l3) / (2 * pp) as f64;
                    let d = ((n1 + n2).ln() - n3.ln()) / ln2;
                    let alfa = (-4.6 * (d - 1.0)).exp();
                    v[i] = alfa * pr[i] + (1.0 - alfa) * v[i - 1];
                }
            }
            vec![v]
        }
        ExtKind::Vidya => {
            let cp = pi(0, 9).max(1) as usize;
            let ep = pi(1, 12).max(1) as usize;
            let pr = price_of(pi(3, 1));
            let f = 2.0 / (ep as f64 + 1.0);
            let mut v = e();
            let start = ep + cp - 1;
            if n >= start {
                for i in 0..start.min(n) {
                    v[i] = pr[i];
                }
                for i in start.max(1)..n {
                    let cmo = if i >= cp {
                        let (mut up, mut dn) = (0.0, 0.0);
                        for k in 0..cp {
                            let d = pr[i - k] - pr[i - k - 1];
                            if d > 0.0 {
                                up += d;
                            } else {
                                dn -= d;
                            }
                        }
                        if up + dn != 0.0 {
                            (up - dn) / (up + dn)
                        } else {
                            0.0
                        }
                    } else {
                        0.0
                    };
                    let m = cmo.abs();
                    v[i] = pr[i] * f * m + v[i - 1] * (1.0 - f * m);
                }
            }
            vec![v]
        }
        ExtKind::AdxWilder => {
            let pp = pi(0, 14).max(1) as usize;
            let (mut adx, mut pdi, mut ndi) = (e(), e(), e());
            if n >= pp && n > 1 {
                let z = vec![0.0; n];
                let (mut pd, mut nd, mut tr, mut atr, mut pds, mut nds, mut dx) = (
                    z.clone(),
                    z.clone(),
                    z.clone(),
                    z.clone(),
                    z.clone(),
                    z.clone(),
                    z,
                );
                for i in 0..pp.min(n) {
                    adx[i] = 0.0;
                    pdi[i] = 0.0;
                    ndi[i] = 0.0;
                }
                for i in 1..n {
                    let mut up = (high[i] - high[i - 1]).max(0.0);
                    let mut dn = (low[i - 1] - low[i]).max(0.0);
                    if dn == up {
                        up = 0.0;
                        dn = 0.0;
                    } else if up < dn {
                        up = 0.0;
                    } else {
                        dn = 0.0;
                    }
                    pd[i] = up;
                    nd[i] = dn;
                    tr[i] = (high[i] - low[i])
                        .abs()
                        .max((high[i] - close[i - 1]).abs())
                        .max((low[i] - close[i - 1]).abs());
                    if i < pp {
                        atr[i] = 0.0;
                        pdi[i] = 0.0;
                        ndi[i] = 0.0;
                    } else {
                        atr[i] = smoothed_ma(i, pp, atr[i - 1], &tr);
                        pds[i] = smoothed_ma(i, pp, pds[i - 1], &pd);
                        nds[i] = smoothed_ma(i, pp, nds[i - 1], &nd);
                    }
                    if atr[i] != 0.0 {
                        pdi[i] = 100.0 * pds[i] / atr[i];
                        ndi[i] = 100.0 * nds[i] / atr[i];
                    } else {
                        pdi[i] = 0.0;
                        ndi[i] = 0.0;
                    }
                    let s = pdi[i] + ndi[i];
                    dx[i] = if s != 0.0 {
                        100.0 * ((pdi[i] - ndi[i]) / s).abs()
                    } else {
                        0.0
                    };
                    adx[i] = smoothed_ma(i, pp, adx[i - 1], &dx);
                }
            }
            vec![adx, pdi, ndi]
        }
        // MFI, OBV and A/D already follow the reference exactly
        ExtKind::Mfi | ExtKind::Obv | ExtKind::Ad => return None,
    })
}

fn ad_step(high: f64, low: f64, close: f64, volume: f64) -> f64 {
    let sum = (close - low) - (high - close);
    if sum != 0.0 && high != low {
        sum / (high - low) * volume
    } else {
        0.0
    }
}
