//! Live prices from a public market-data service (no account or key needed).
//! Crypto symbols such as BTCUSD map to the exchange's USDT pairs. A background
//! thread loads real bar history once, then polls best bid/ask every second.

use crate::market::{Bar, Timeframe};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const BASE: &str = "https://data-api.binance.vision/api/v3";

pub enum FeedMsg {
    Quote {
        symbol: String,
        bid: f64,
        ask: f64,
        time: i64,
    },
    History {
        symbol: String,
        tf: Timeframe,
        bars: Vec<Bar>,
    },
    Status(String),
    Error(String),
}

/// Exchange pair for one of our symbols, when it is a crypto coin quoted in USD.
pub fn pair_for(symbol: &str) -> Option<String> {
    const COINS: [&str; 20] = [
        "BTC", "ETH", "BNB", "SOL", "XRP", "ADA", "DOGE", "LTC", "DOT", "AVAX", "LINK", "TRX",
        "MATIC", "BCH", "XLM", "ATOM", "UNI", "ETC", "FIL", "NEAR",
    ];
    let s = symbol.to_ascii_uppercase();
    let base = s.strip_suffix("USDT").or_else(|| s.strip_suffix("USD"))?;
    if COINS.contains(&base) {
        Some(format!("{}USDT", base))
    } else {
        None
    }
}

fn interval(tf: Timeframe) -> &'static str {
    match tf {
        Timeframe::M1 => "1m",
        Timeframe::M5 => "5m",
        Timeframe::M15 => "15m",
        Timeframe::M30 => "30m",
        Timeframe::H1 => "1h",
        Timeframe::H4 => "4h",
        Timeframe::D1 => "1d",
        Timeframe::W1 => "1w",
        Timeframe::MN1 => "1M",
    }
}

fn get_json(agent: &ureq::Agent, url: &str) -> Result<serde_json::Value, String> {
    let resp = agent.get(url).call().map_err(|e| e.to_string())?;
    let text = resp.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

fn num(v: &serde_json::Value) -> f64 {
    match v {
        serde_json::Value::String(s) => s.parse().unwrap_or(0.0),
        serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// Bars from the exchange's kline endpoint (oldest first).
pub fn fetch_klines(
    agent: &ureq::Agent,
    pair: &str,
    tf: Timeframe,
    limit: usize,
    digits: u32,
) -> Result<Vec<Bar>, String> {
    let url = format!(
        "{}/klines?symbol={}&interval={}&limit={}",
        BASE,
        pair,
        interval(tf),
        limit.min(1000)
    );
    let v = get_json(agent, &url)?;
    let arr = v.as_array().ok_or("unexpected answer")?;
    let r = |x: f64| crate::market::round_to(x, digits);
    let mut bars = Vec::with_capacity(arr.len());
    for k in arr {
        let k = match k.as_array() {
            Some(k) if k.len() >= 9 => k,
            _ => continue,
        };
        let open_ms = k[0].as_i64().unwrap_or(0);
        let mut time = open_ms / 1000;
        if tf == Timeframe::W1 {
            // the exchange starts weeks on Monday; the platform on Sunday
            time = crate::history::bar_open_time(Timeframe::W1, time);
        }
        bars.push(Bar {
            time,
            open: r(num(&k[1])),
            high: r(num(&k[2])),
            low: r(num(&k[3])),
            close: r(num(&k[4])),
            tick_volume: k[8].as_i64().unwrap_or(0),
            spread: 0,
        });
    }
    Ok(bars)
}

pub struct LiveFeed {
    rx: Receiver<FeedMsg>,
    stop: Arc<Mutex<bool>>,
    pub symbols: Vec<String>,
}

impl LiveFeed {
    /// Start streaming the given symbols (ours, e.g. "BTCUSD") with their price digits.
    pub fn start(symbols: Vec<(String, u32)>) -> LiveFeed {
        let (tx, rx) = channel();
        let stop = Arc::new(Mutex::new(false));
        let stop2 = stop.clone();
        let names = symbols.iter().map(|s| s.0.clone()).collect();
        std::thread::spawn(move || run(symbols, tx, stop2));
        LiveFeed {
            rx,
            stop,
            symbols: names,
        }
    }

    /// Messages received since the last call.
    pub fn poll(&self) -> Vec<FeedMsg> {
        self.rx.try_iter().collect()
    }

    pub fn stop(&self) {
        if let Ok(mut s) = self.stop.lock() {
            *s = true;
        }
    }
}

impl Drop for LiveFeed {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(symbols: Vec<(String, u32)>, tx: Sender<FeedMsg>, stop: Arc<Mutex<bool>>) {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .user_agent("rust-terminal")
        .build();
    let pairs: Vec<(String, String, u32)> = symbols
        .iter()
        .filter_map(|(s, d)| pair_for(s).map(|p| (s.clone(), p, *d)))
        .collect();
    if pairs.is_empty() {
        let _ = tx.send(FeedMsg::Error("no symbol has a live price source".into()));
        return;
    }
    let stopped = || stop.lock().map(|s| *s).unwrap_or(true);
    // history first, so charts open on real data
    for (sym, pair, digits) in &pairs {
        for tf in Timeframe::ALL {
            if stopped() {
                return;
            }
            match fetch_klines(&agent, pair, tf, 1000, *digits) {
                Ok(bars) if !bars.is_empty() => {
                    let _ = tx.send(FeedMsg::History {
                        symbol: sym.clone(),
                        tf,
                        bars,
                    });
                }
                Ok(_) => {}
                Err(e) => {
                    let _ = tx.send(FeedMsg::Error(format!("{} {}: {}", sym, tf.label(), e)));
                }
            }
        }
    }
    let _ = tx.send(FeedMsg::Status(format!(
        "live prices: {}",
        pairs
            .iter()
            .map(|p| p.0.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    )));
    let list = pairs
        .iter()
        .map(|p| format!("\"{}\"", p.1))
        .collect::<Vec<_>>()
        .join(",");
    let url = format!("{}/ticker/bookTicker?symbols=[{}]", BASE, list);
    let mut failures = 0;
    while !stopped() {
        match get_json(&agent, &url) {
            Ok(v) => {
                failures = 0;
                let now = crate::market::unix_now();
                for q in v.as_array().cloned().unwrap_or_default() {
                    let pair = q.get("symbol").and_then(|s| s.as_str()).unwrap_or("");
                    if let Some((sym, _, digits)) = pairs.iter().find(|p| p.1 == pair) {
                        let r = |x: f64| crate::market::round_to(x, *digits);
                        let (bid, ask) = (r(num(&q["bidPrice"])), r(num(&q["askPrice"])));
                        if bid > 0.0 && ask > 0.0 {
                            let _ = tx.send(FeedMsg::Quote {
                                symbol: sym.clone(),
                                bid,
                                ask,
                                time: now,
                            });
                        }
                    }
                }
            }
            Err(e) => {
                failures += 1;
                if failures == 1 || failures % 30 == 0 {
                    let _ = tx.send(FeedMsg::Error(format!("live prices unavailable: {}", e)));
                }
            }
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
}
