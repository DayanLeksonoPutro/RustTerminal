//! Live prices and history from online market-data services.
//!
//! Every symbol gets a list of sources in priority order ("routes"); the first one that
//! answers is used and the next one takes over when it keeps failing. Free sources need no
//! account. Premium ones (Alpaca, Polygon, OANDA) are used only when the user saved a key
//! in `<data dir>/feeds.json`:
//!
//! ```json
//! {
//!   "alpaca": { "keyId": "PK...", "secretKey": "...", "feed": "iex" },
//!   "polygon": { "apiKey": "..." },
//!   "oanda": { "token": "...", "account": "practice" }
//! }
//! ```
//!
//! Symbols: crypto such as `BTCUSD`/`ETHUSDT`, perpetual futures `BTCUSDT.P` (Binance USD-M,
//! then Bybit) and `BLOFIN:BTCUSDT.P`, US stocks such as `AAPL`, forex and metals such as
//! `EURUSD`/`XAUUSD`, CME futures `NQ`, `MNQ`, `ES`, `MES`. A `SOURCE:` prefix
//! (`KRAKEN:BTCUSD`, `YAHOO:AAPL`, `OANDA:EURUSD`...) pins one source.
//!
//! All times leave this module in UTC; the market converts them to the terminal zone.

use crate::market::{Bar, Timeframe};
use serde_json::Value as J;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------- sources ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Provider {
    Binance,
    BinanceUs,
    Coinbase,
    Kraken,
    Bybit,
    Blofin,
    BinanceFutures,
    Yahoo,
    Nasdaq,
    Alpaca,
    Polygon,
    Oanda,
}

impl Provider {
    pub const ALL: [Provider; 12] = [
        Provider::Binance,
        Provider::BinanceUs,
        Provider::Coinbase,
        Provider::Kraken,
        Provider::Bybit,
        Provider::Blofin,
        Provider::BinanceFutures,
        Provider::Yahoo,
        Provider::Nasdaq,
        Provider::Alpaca,
        Provider::Polygon,
        Provider::Oanda,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Provider::Binance => "Binance",
            Provider::BinanceUs => "Binance.US",
            Provider::Coinbase => "Coinbase",
            Provider::Kraken => "Kraken",
            Provider::Bybit => "Bybit",
            Provider::Blofin => "BloFin",
            Provider::BinanceFutures => "Binance USD-M futures",
            Provider::Yahoo => "Yahoo",
            Provider::Nasdaq => "Nasdaq",
            Provider::Alpaca => "Alpaca",
            Provider::Polygon => "Polygon",
            Provider::Oanda => "OANDA",
        }
    }
    /// The `PREFIX:` that pins this source in a symbol name.
    pub fn prefix(self) -> &'static str {
        match self {
            Provider::Binance => "BINANCE",
            Provider::BinanceUs => "BINANCEUS",
            Provider::Coinbase => "COINBASE",
            Provider::Kraken => "KRAKEN",
            Provider::Bybit => "BYBIT",
            Provider::Blofin => "BLOFIN",
            Provider::BinanceFutures => "BINANCEF",
            Provider::Yahoo => "YAHOO",
            Provider::Nasdaq => "NASDAQ",
            Provider::Alpaca => "ALPACA",
            Provider::Polygon => "POLYGON",
            Provider::Oanda => "OANDA",
        }
    }
    pub fn needs_key(self) -> bool {
        matches!(self, Provider::Alpaca | Provider::Polygon | Provider::Oanda)
    }
    /// How often quotes are asked for.
    fn poll(self) -> Duration {
        Duration::from_millis(match self {
            Provider::Yahoo => 4000,
            Provider::Nasdaq => 5000,
            Provider::Polygon => 3000,
            _ => 1000,
        })
    }
}

/// One source for one symbol: the provider and its own name for the instrument.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub provider: Provider,
    pub ticker: String,
}

// ---------- keys ----------

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct AlpacaKey {
    #[serde(rename = "keyId", default)]
    pub key_id: String,
    #[serde(rename = "secretKey", default)]
    pub secret_key: String,
    /// "iex" (free) or "sip" (paid)
    #[serde(default = "iex")]
    pub feed: String,
}

fn iex() -> String {
    "iex".into()
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct PolygonKey {
    #[serde(rename = "apiKey", default)]
    pub api_key: String,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct OandaKey {
    #[serde(default)]
    pub token: String,
    /// "practice" (demo) or "live"
    #[serde(default = "practice")]
    pub account: String,
}

fn practice() -> String {
    "practice".into()
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Keys {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpaca: Option<AlpacaKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polygon: Option<PolygonKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oanda: Option<OandaKey>,
}

impl Keys {
    pub fn path() -> std::path::PathBuf {
        crate::storage::data_dir().join("feeds.json")
    }
    pub fn load() -> Keys {
        std::fs::read_to_string(Keys::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(crate::storage::data_dir())?;
        let text = serde_json::to_string_pretty(self).unwrap_or_default();
        let p = Keys::path();
        std::fs::write(&p, text)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
    pub fn has(&self, p: Provider) -> bool {
        match p {
            Provider::Alpaca => self
                .alpaca
                .as_ref()
                .is_some_and(|k| !k.key_id.is_empty() && !k.secret_key.is_empty()),
            Provider::Polygon => self.polygon.as_ref().is_some_and(|k| !k.api_key.is_empty()),
            Provider::Oanda => self.oanda.as_ref().is_some_and(|k| !k.token.is_empty()),
            _ => true,
        }
    }
}

// ---------- which symbol goes where ----------

const COINS: [&str; 40] = [
    "BTC", "ETH", "BNB", "SOL", "XRP", "ADA", "DOGE", "LTC", "DOT", "AVAX", "LINK", "TRX", "MATIC",
    "POL", "BCH", "XLM", "ATOM", "UNI", "ETC", "FIL", "NEAR", "APT", "ARB", "OP", "SUI", "TON",
    "SHIB", "PEPE", "INJ", "AAVE", "MKR", "HBAR", "ICP", "RNDR", "TIA", "SEI", "WIF", "XMR",
    "ALGO", "PAXG",
];
const FX: [&str; 18] = [
    "USD", "EUR", "GBP", "JPY", "CHF", "AUD", "CAD", "NZD", "SEK", "NOK", "DKK", "SGD", "HKD",
    "MXN", "ZAR", "TRY", "PLN", "CNH",
];
const METALS: [&str; 4] = ["XAU", "XAG", "XPT", "XPD"];
/// CME futures: root, Yahoo name, contract months
const FUTURES: [(&str, &str, &str); 11] = [
    ("NQ", "NQ=F", "HMUZ"),
    ("MNQ", "MNQ=F", "HMUZ"),
    ("ES", "ES=F", "HMUZ"),
    ("MES", "MES=F", "HMUZ"),
    ("YM", "YM=F", "HMUZ"),
    ("MYM", "MYM=F", "HMUZ"),
    ("RTY", "RTY=F", "HMUZ"),
    ("M2K", "M2K=F", "HMUZ"),
    ("CL", "CL=F", "FGHJKMNQUVXZ"),
    ("GC", "GC=F", "GJMQVZ"),
    ("SI", "SI=F", "HKNUZ"),
];

/// What kind of instrument a symbol is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Crypto,
    Perpetual,
    Forex,
    Metal,
    Future,
    Stock,
}

fn split_crypto(s: &str) -> Option<(&str, &str)> {
    for q in ["USDT", "USDC", "USD"] {
        if let Some(b) = s.strip_suffix(q) {
            if COINS.contains(&b) {
                return Some((b, q));
            }
        }
    }
    None
}

fn future_root(s: &str) -> Option<&'static (&'static str, &'static str, &'static str)> {
    let s = s.trim_end_matches("1!").trim_end_matches("=F");
    FUTURES.iter().find(|f| f.0 == s)
}

fn is_stock(s: &str) -> bool {
    let (main, class) = match s.split_once(['.', '-']) {
        Some((m, c)) => (m, c),
        None => (s, "A"),
    };
    !main.is_empty()
        && main.len() <= 5
        && main.chars().all(|c| c.is_ascii_uppercase())
        && class.len() == 1
        && class.chars().all(|c| c.is_ascii_uppercase())
}

/// The kind of instrument, from the symbol name.
pub fn classify(symbol: &str) -> Option<Class> {
    let s = symbol.to_ascii_uppercase();
    let s = s.split_once(':').map(|x| x.1.to_string()).unwrap_or(s);
    if let Some(base) = s.strip_suffix(".P") {
        return split_crypto(base).map(|_| Class::Perpetual);
    }
    if split_crypto(&s).is_some() {
        return Some(Class::Crypto);
    }
    if s.len() == 6 {
        let (a, b) = (&s[..3], &s[3..]);
        if METALS.contains(&a) && FX.contains(&b) {
            return Some(Class::Metal);
        }
        if FX.contains(&a) && FX.contains(&b) && a != b {
            return Some(Class::Forex);
        }
    }
    if future_root(&s).is_some() {
        return Some(Class::Future);
    }
    if is_stock(&s) {
        return Some(Class::Stock);
    }
    None
}

/// The front quarterly (or listed) contract of a future, e.g. NQZ6, for sources that
/// need a contract code. Rolls a week before the third Friday of the month.
pub fn front_contract(root: &str, months: &str, now: i64) -> String {
    const CODES: &str = "FGHJKMNQUVXZ";
    let (y, m, d, _, _, _) = crate::market::civil(now);
    let mut year = y;
    let mut month = m as usize;
    let mut first = true;
    loop {
        let code = CODES.as_bytes()[month - 1] as char;
        if months.contains(code) {
            // third Friday minus a week: roll then
            let first_day = crate::mql::lexer::days_from_civil(year, month as i64, 1);
            let dow = (first_day + 4).rem_euclid(7); // 0 = Sunday
            let first_friday = 1 + (5 - dow).rem_euclid(7);
            let roll_day = first_friday + 14 - 7;
            if !(first && d as i64 >= roll_day) {
                return format!("{}{}{}", root, code, year % 10);
            }
        }
        first = false;
        month += 1;
        if month > 12 {
            month = 1;
            year += 1;
        }
    }
}

/// Sources for a symbol, best first, skipping premium ones without a key.
pub fn routes(symbol: &str, keys: &Keys) -> Vec<Route> {
    let up = symbol.trim().to_ascii_uppercase();
    let (pin, s) = match up.split_once(':') {
        Some((p, rest)) => (
            Provider::ALL.iter().copied().find(|x| x.prefix() == p),
            rest.to_string(),
        ),
        None => (None, up.clone()),
    };
    let r = |p: Provider, t: String| Route {
        provider: p,
        ticker: t,
    };
    let mut out: Vec<Route> = vec![];
    match classify(&s) {
        Some(Class::Perpetual) => {
            let base = s.trim_end_matches(".P");
            let (coin, _) = split_crypto(base).unwrap_or((base, "USDT"));
            let pair = format!("{}USDT", coin);
            if pin == Some(Provider::Blofin) {
                out.push(r(Provider::Blofin, format!("{}-USDT", coin)));
            } else {
                out.push(r(Provider::BinanceFutures, pair.clone()));
                out.push(r(Provider::Bybit, pair));
                out.push(r(Provider::Blofin, format!("{}-USDT", coin)));
            }
        }
        Some(Class::Crypto) => {
            let (coin, quote) = split_crypto(&s).unwrap_or(("BTC", "USD"));
            let usdt = format!("{}USDT", coin);
            out.push(r(
                Provider::Binance,
                if quote == "USDC" {
                    s.clone()
                } else {
                    usdt.clone()
                },
            ));
            out.push(r(Provider::BinanceUs, usdt));
            out.push(r(Provider::Coinbase, format!("{}-USD", coin)));
            let kcoin = if coin == "BTC" { "XBT" } else { coin };
            out.push(r(Provider::Kraken, format!("{}USD", kcoin)));
            out.push(r(Provider::Bybit, format!("{}USDT", coin)));
        }
        Some(Class::Forex) => {
            out.push(r(Provider::Oanda, format!("{}_{}", &s[..3], &s[3..])));
            out.push(r(
                Provider::Yahoo,
                if &s[..3] == "USD" {
                    format!("{}=X", &s[3..])
                } else {
                    format!("{}=X", s)
                },
            ));
            out.push(r(Provider::Kraken, s.clone()));
        }
        Some(Class::Metal) => {
            out.push(r(Provider::Oanda, format!("{}_{}", &s[..3], &s[3..])));
            let y = match &s[..3] {
                "XAU" => "GC=F",
                "XAG" => "SI=F",
                "XPT" => "PL=F",
                _ => "PA=F",
            };
            out.push(r(Provider::Yahoo, y.to_string()));
        }
        Some(Class::Future) => {
            if let Some(f) = future_root(&s) {
                out.push(r(
                    Provider::Polygon,
                    format!(
                        "fut:{}",
                        front_contract(f.0, f.2, crate::market::unix_now())
                    ),
                ));
                out.push(r(Provider::Yahoo, f.1.to_string()));
            }
        }
        Some(Class::Stock) => {
            let t = s.replace('-', ".");
            out.push(r(Provider::Alpaca, t.clone()));
            out.push(r(Provider::Polygon, t.clone()));
            out.push(r(Provider::Yahoo, t.replace('.', "-")));
            out.push(r(Provider::Nasdaq, t));
        }
        None => {
            if pin.is_some() {
                // unknown kind but a pinned source: pass the name through
                out.push(r(pin.unwrap_or(Provider::Yahoo), s.clone()));
            }
        }
    }
    if let Some(p) = pin {
        out.retain(|x| x.provider == p);
        if out.is_empty() {
            out.push(r(p, s));
        }
    }
    out.retain(|x| keys.has(x.provider));
    out
}

/// Whether a symbol has at least one online source.
pub fn has_source(symbol: &str) -> bool {
    !routes(symbol, &Keys::load()).is_empty()
}

/// Properties for a symbol added by name: (digits, contract size, description).
pub fn guess_spec(symbol: &str, price: f64) -> (u32, f64, String) {
    let up = symbol.to_ascii_uppercase();
    let s = up.split_once(':').map(|x| x.1).unwrap_or(&up);
    let digits_for = |p: f64| -> u32 {
        if p <= 0.0 {
            2
        } else if p < 0.001 {
            8
        } else if p < 1.0 {
            5
        } else if p < 10.0 {
            4
        } else if p < 1000.0 {
            2
        } else {
            2
        }
    };
    match classify(s) {
        Some(Class::Forex) => (
            if s.contains("JPY") { 3 } else { 5 },
            100000.0,
            format!("{} vs {}", &s[..3], &s[3..]),
        ),
        Some(Class::Metal) => (2, 100.0, format!("{} vs {}", &s[..3], &s[3..])),
        Some(Class::Future) => {
            let root = future_root(s).map(|f| f.0).unwrap_or("");
            // point value in USD per contract
            let value = match root {
                "NQ" => 20.0,
                "MNQ" => 2.0,
                "ES" => 50.0,
                "MES" => 5.0,
                "YM" => 5.0,
                "MYM" => 0.5,
                "RTY" => 50.0,
                "M2K" => 5.0,
                "CL" => 1000.0,
                "GC" => 100.0,
                "SI" => 5000.0,
                _ => 1.0,
            };
            (2, value, format!("{} futures (CME)", root))
        }
        Some(Class::Perpetual) => (digits_for(price), 1.0, format!("{} perpetual", s)),
        Some(Class::Crypto) => (digits_for(price), 1.0, format!("{} spot", s)),
        _ => (2, 1.0, s.to_string()),
    }
}

// ---------- HTTP ----------

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(12))
        .user_agent("Mozilla/5.0 (rust-terminal)")
        .build()
}

fn get(agent: &ureq::Agent, url: &str, headers: &[(&str, &str)]) -> Result<J, String> {
    let mut req = agent.get(url);
    for (k, v) in headers {
        req = req.set(k, v);
    }
    match req.call() {
        Ok(resp) => {
            let text = resp.into_string().map_err(|e| e.to_string())?;
            serde_json::from_str(&text).map_err(|e| format!("bad answer: {}", e))
        }
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            let body: String = body.chars().take(160).collect();
            Err(format!("HTTP {} {}", code, body.trim()))
        }
        Err(e) => Err(explain(&redact(&e.to_string()))),
    }
}

/// Hide API keys that travel in URLs.
fn redact(s: &str) -> String {
    let mut out = s.to_string();
    if let Some(i) = out.find("apiKey=") {
        let end = out[i..]
            .find(|c: char| c == '&' || c.is_whitespace() || c == ':')
            .map(|e| i + e)
            .unwrap_or(out.len());
        out.replace_range(i + 7..end, "***");
    }
    out
}

/// Add a hint to connection errors that mean the network blocked the site.
fn explain(e: &str) -> String {
    // drop the URL ureq puts in front
    let msg = match e.find(": ") {
        Some(i) if e.starts_with("http") => &e[i + 2..],
        _ => e,
    };
    if msg.contains("certificate") || msg.contains("HandshakeFailure") {
        format!(
            "{} (the connection was intercepted: the site is blocked on this network; a VPN or another DNS helps)",
            msg
        )
    } else {
        msg.to_string()
    }
}

fn num(v: &J) -> f64 {
    match v {
        J::String(s) => s
            .trim()
            .trim_start_matches('$')
            .replace(',', "")
            .parse()
            .unwrap_or(f64::NAN),
        J::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

/// "2024-01-02T15:04:05.123456789Z" or with "+hh:mm" → unix seconds.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    if s.len() < 19 {
        return None;
    }
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let mo: i64 = s.get(5..7)?.parse().ok()?;
    let d: i64 = s.get(8..10)?.parse().ok()?;
    let h: i64 = s.get(11..13)?.parse().ok()?;
    let mi: i64 = s.get(14..16)?.parse().ok()?;
    let se: i64 = s.get(17..19)?.parse().ok()?;
    let mut t = crate::mql::lexer::days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + se;
    let rest = &s[19..];
    let rest = rest.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    if let Some(off) = rest.strip_prefix('+').or_else(|| rest.strip_prefix('-')) {
        let sign = if rest.starts_with('-') { -1 } else { 1 };
        let oh: i64 = off.get(0..2)?.parse().ok()?;
        let om: i64 = off.get(3..5).and_then(|x| x.parse().ok()).unwrap_or(0);
        t -= sign * (oh * 3600 + om * 60);
    }
    Some(t)
}

fn rfc3339(t: i64) -> String {
    let (y, mo, d, h, mi, s) = crate::market::civil(t);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, mi, s)
}

fn ymd(t: i64) -> String {
    let (y, mo, d, _, _, _) = crate::market::civil(t);
    format!("{:04}-{:02}-{:02}", y, mo, d)
}

fn bar(time: i64, o: f64, h: f64, l: f64, c: f64, v: f64) -> Option<Bar> {
    if !(o.is_finite() && h.is_finite() && l.is_finite() && c.is_finite()) || c <= 0.0 {
        return None;
    }
    Some(Bar {
        time,
        open: o,
        high: h.max(o).max(c),
        low: l.min(o).min(c),
        close: c,
        tick_volume: if v.is_finite() {
            v.round().max(0.0) as i64
        } else {
            0
        },
        spread: 0,
    })
}

// ---------- history ----------

/// The provider's own name for a timeframe, when it has one.
fn native(p: Provider, tf: Timeframe) -> Option<&'static str> {
    use Timeframe::*;
    Some(match p {
        Provider::Binance | Provider::BinanceUs | Provider::BinanceFutures => match tf {
            M1 => "1m",
            M5 => "5m",
            M15 => "15m",
            M30 => "30m",
            H1 => "1h",
            H4 => "4h",
            D1 => "1d",
            W1 => "1w",
            MN1 => "1M",
        },
        Provider::Coinbase => match tf {
            M1 => "60",
            M5 => "300",
            M15 => "900",
            H1 => "3600",
            D1 => "86400",
            _ => return None,
        },
        Provider::Kraken => match tf {
            M1 => "1",
            M5 => "5",
            M15 => "15",
            M30 => "30",
            H1 => "60",
            H4 => "240",
            D1 => "1440",
            W1 => "10080",
            MN1 => return None,
        },
        Provider::Bybit => match tf {
            M1 => "1",
            M5 => "5",
            M15 => "15",
            M30 => "30",
            H1 => "60",
            H4 => "240",
            D1 => "D",
            W1 => "W",
            MN1 => "M",
        },
        Provider::Blofin => match tf {
            M1 => "1m",
            M5 => "5m",
            M15 => "15m",
            M30 => "30m",
            H1 => "1H",
            H4 => "4H",
            D1 => "1D",
            W1 => "1W",
            MN1 => "1M",
        },
        Provider::Yahoo => match tf {
            M1 => "1m",
            M5 => "5m",
            M15 => "15m",
            M30 => "30m",
            H1 => "60m",
            D1 => "1d",
            W1 => "1wk",
            MN1 => "1mo",
            H4 => return None,
        },
        Provider::Nasdaq => match tf {
            M1 => "1m",
            D1 => "1d",
            _ => return None,
        },
        Provider::Alpaca => match tf {
            M1 => "1Min",
            M5 => "5Min",
            M15 => "15Min",
            M30 => "30Min",
            H1 => "1Hour",
            H4 => "4Hour",
            D1 => "1Day",
            W1 => "1Week",
            MN1 => "1Month",
        },
        Provider::Polygon => match tf {
            M1 => "1/minute",
            M5 => "5/minute",
            M15 => "15/minute",
            M30 => "30/minute",
            H1 => "1/hour",
            H4 => "4/hour",
            D1 => "1/day",
            W1 => "1/week",
            MN1 => "1/month",
        },
        Provider::Oanda => match tf {
            M1 => "M1",
            M5 => "M5",
            M15 => "M15",
            M30 => "M30",
            H1 => "H1",
            H4 => "H4",
            D1 => "D",
            W1 => "W",
            MN1 => "M",
        },
    })
}

/// The timeframe to build `tf` from when the provider has no such bars.
fn base_for(p: Provider, tf: Timeframe) -> Option<Timeframe> {
    use Timeframe::*;
    let cands: &[Timeframe] = match tf {
        M5 => &[M1],
        M15 => &[M5, M1],
        M30 => &[M15, M5],
        H1 => &[M30, M15, M5, M1],
        H4 => &[H1, M30, M15, M1],
        D1 => &[H4, H1],
        W1 => &[D1],
        MN1 => &[D1, W1],
        M1 => &[],
    };
    cands.iter().copied().find(|b| native(p, *b).is_some())
}

/// Bars (UTC, oldest first) of one timeframe from one source; built from a smaller
/// timeframe when the source has no bars of that size.
pub fn fetch_history(
    agent: &ureq::Agent,
    keys: &Keys,
    route: &Route,
    tf: Timeframe,
) -> Result<Vec<Bar>, String> {
    if native(route.provider, tf).is_none() {
        let base = base_for(route.provider, tf)
            .ok_or_else(|| format!("{} has no {} bars", route.provider.name(), tf.label()))?;
        let b = fetch_native(agent, keys, route, base)?;
        return Ok(crate::history::aggregate(&b, tf));
    }
    let mut bars = fetch_native(agent, keys, route, tf)?;
    bars.sort_by_key(|b| b.time);
    bars.dedup_by_key(|b| b.time);
    if tf == Timeframe::W1 {
        // sources start weeks on Monday; the platform on Sunday
        for b in bars.iter_mut() {
            b.time = crate::history::bar_open_time(Timeframe::W1, b.time);
        }
        bars.dedup_by_key(|b| b.time);
    }
    Ok(bars)
}

fn fetch_native(
    agent: &ureq::Agent,
    keys: &Keys,
    route: &Route,
    tf: Timeframe,
) -> Result<Vec<Bar>, String> {
    let iv = native(route.provider, tf).unwrap_or("");
    let t = route.ticker.as_str();
    let now = crate::market::unix_now();
    match route.provider {
        Provider::Binance | Provider::BinanceUs | Provider::BinanceFutures => {
            let base = match route.provider {
                Provider::Binance => "https://data-api.binance.vision/api/v3",
                Provider::BinanceUs => "https://api.binance.us/api/v3",
                _ => "https://fapi.binance.com/fapi/v1",
            };
            let v = get(
                agent,
                &format!("{}/klines?symbol={}&interval={}&limit=1000", base, t, iv),
                &[],
            )?;
            let arr = v.as_array().ok_or("unexpected answer")?;
            Ok(arr
                .iter()
                .filter_map(|k| {
                    let k = k.as_array()?;
                    bar(
                        k.first()?.as_i64()? / 1000,
                        num(k.get(1)?),
                        num(k.get(2)?),
                        num(k.get(3)?),
                        num(k.get(4)?),
                        num(k.get(5)?),
                    )
                })
                .collect())
        }
        Provider::Coinbase => {
            // 300 candles per request: [time, low, high, open, close, volume], newest first
            let gran: i64 = iv.parse().unwrap_or(60);
            let mut out = vec![];
            let mut end = now;
            for _ in 0..3 {
                let start = end - gran * 300;
                let v = get(
                    agent,
                    &format!(
                        "https://api.exchange.coinbase.com/products/{}/candles?granularity={}&start={}&end={}",
                        t,
                        gran,
                        rfc3339(start),
                        rfc3339(end)
                    ),
                    &[],
                )?;
                let arr = v.as_array().ok_or("unexpected answer")?;
                if arr.is_empty() {
                    break;
                }
                out.extend(arr.iter().filter_map(|k| {
                    let k = k.as_array()?;
                    bar(
                        k.first()?.as_i64()?,
                        num(k.get(3)?),
                        num(k.get(2)?),
                        num(k.get(1)?),
                        num(k.get(4)?),
                        num(k.get(5)?),
                    )
                }));
                end = start;
            }
            Ok(out)
        }
        Provider::Kraken => {
            let v = get(
                agent,
                &format!(
                    "https://api.kraken.com/0/public/OHLC?pair={}&interval={}",
                    t, iv
                ),
                &[],
            )?;
            kraken_err(&v)?;
            let res = v["result"].as_object().ok_or("unexpected answer")?;
            let arr = res
                .iter()
                .find(|(k, _)| k.as_str() != "last")
                .and_then(|(_, v)| v.as_array())
                .ok_or("unexpected answer")?;
            Ok(arr
                .iter()
                .filter_map(|k| {
                    let k = k.as_array()?;
                    bar(
                        k.first()?.as_i64()?,
                        num(k.get(1)?),
                        num(k.get(2)?),
                        num(k.get(3)?),
                        num(k.get(4)?),
                        num(k.get(6)?),
                    )
                })
                .collect())
        }
        Provider::Bybit => {
            let v = get(
                agent,
                &format!(
                    "https://api.bybit.com/v5/market/kline?category=linear&symbol={}&interval={}&limit=1000",
                    t, iv
                ),
                &[],
            )?;
            if v["retCode"].as_i64().unwrap_or(0) != 0 {
                return Err(v["retMsg"].as_str().unwrap_or("error").to_string());
            }
            let arr = v["result"]["list"].as_array().ok_or("unexpected answer")?;
            Ok(arr
                .iter()
                .filter_map(|k| {
                    let k = k.as_array()?;
                    bar(
                        num(k.first()?) as i64 / 1000,
                        num(k.get(1)?),
                        num(k.get(2)?),
                        num(k.get(3)?),
                        num(k.get(4)?),
                        num(k.get(5)?),
                    )
                })
                .collect())
        }
        Provider::Blofin => {
            let v = get(
                agent,
                &format!(
                    "https://openapi.blofin.com/api/v1/market/candles?instId={}&bar={}&limit=1440",
                    t, iv
                ),
                &[],
            )?;
            if v["code"].as_str().unwrap_or("0") != "0" {
                return Err(v["msg"].as_str().unwrap_or("error").to_string());
            }
            let arr = v["data"].as_array().ok_or("unexpected answer")?;
            Ok(arr
                .iter()
                .filter_map(|k| {
                    let k = k.as_array()?;
                    bar(
                        num(k.first()?) as i64 / 1000,
                        num(k.get(1)?),
                        num(k.get(2)?),
                        num(k.get(3)?),
                        num(k.get(4)?),
                        num(k.get(5)?),
                    )
                })
                .collect())
        }
        Provider::Yahoo => {
            let range = match tf {
                Timeframe::M1 => "7d",
                Timeframe::M5 | Timeframe::M15 | Timeframe::M30 => "60d",
                Timeframe::H1 => "730d",
                Timeframe::D1 => "10y",
                _ => "max",
            };
            let v = get(
                agent,
                &format!(
                    "https://query1.finance.yahoo.com/v8/finance/chart/{}?interval={}&range={}&includePrePost=false",
                    t, iv, range
                ),
                &[],
            )?;
            let r = &v["chart"]["result"][0];
            if r.is_null() {
                return Err(v["chart"]["error"]["description"]
                    .as_str()
                    .unwrap_or("no data")
                    .to_string());
            }
            let ts = r["timestamp"].as_array().cloned().unwrap_or_default();
            let q = &r["indicators"]["quote"][0];
            let col = |k: &str| q[k].as_array().cloned().unwrap_or_default();
            let (o, h, l, c, vol) = (
                col("open"),
                col("high"),
                col("low"),
                col("close"),
                col("volume"),
            );
            let at = |a: &Vec<J>, i: usize| a.get(i).map(num).unwrap_or(f64::NAN);
            let mut out: Vec<Bar> = ts
                .iter()
                .enumerate()
                .filter_map(|(i, t)| {
                    bar(
                        t.as_i64()?,
                        at(&o, i),
                        at(&h, i),
                        at(&l, i),
                        at(&c, i),
                        at(&vol, i),
                    )
                })
                .collect();
            if matches!(tf, Timeframe::D1 | Timeframe::W1 | Timeframe::MN1) {
                // daily stamps are the exchange's session start; label by date
                for b in out.iter_mut() {
                    b.time = crate::history::bar_open_time(tf, b.time + 6 * 3600);
                }
            }
            Ok(out)
        }
        Provider::Nasdaq => nasdaq_history(agent, t, tf),
        Provider::Alpaca => {
            let k = keys.alpaca.clone().unwrap_or_default();
            // look far enough back for 1000 bars of trading hours
            let back = tf.secs() * 1000 * if tf.secs() < 86400 { 5 } else { 2 };
            let v = get(
                agent,
                &format!(
                    "https://data.alpaca.markets/v2/stocks/{}/bars?timeframe={}&limit=1000&sort=desc&adjustment=split&feed={}&start={}",
                    t,
                    iv,
                    k.feed,
                    rfc3339(now - back)
                ),
                &[
                    ("APCA-API-KEY-ID", k.key_id.as_str()),
                    ("APCA-API-SECRET-KEY", k.secret_key.as_str()),
                ],
            )?;
            let arr = v["bars"].as_array().cloned().unwrap_or_default();
            Ok(arr
                .iter()
                .filter_map(|b| {
                    bar(
                        parse_rfc3339(b["t"].as_str()?)?,
                        num(&b["o"]),
                        num(&b["h"]),
                        num(&b["l"]),
                        num(&b["c"]),
                        num(&b["v"]),
                    )
                })
                .collect())
        }
        Provider::Polygon => {
            let key = keys.polygon.clone().unwrap_or_default().api_key;
            if let Some(contract) = t.strip_prefix("fut:") {
                return polygon_futures(agent, &key, contract, tf);
            }
            let back = tf.secs() * 1000 * if tf.secs() < 86400 { 5 } else { 2 };
            let v = get(
                agent,
                &format!(
                    "https://api.polygon.io/v2/aggs/ticker/{}/range/{}/{}/{}?adjusted=true&sort=desc&limit=5000&apiKey={}",
                    t,
                    iv,
                    ymd(now - back),
                    ymd(now + 86400),
                    key
                ),
                &[],
            )?;
            polygon_err(&v)?;
            let arr = v["results"].as_array().cloned().unwrap_or_default();
            Ok(arr
                .iter()
                .filter_map(|b| {
                    bar(
                        b["t"].as_i64()? / 1000,
                        num(&b["o"]),
                        num(&b["h"]),
                        num(&b["l"]),
                        num(&b["c"]),
                        num(&b["v"]),
                    )
                })
                .collect())
        }
        Provider::Oanda => {
            let k = keys.oanda.clone().unwrap_or_default();
            let v = get(
                agent,
                &format!(
                    "{}/v3/instruments/{}/candles?granularity={}&count=5000&price=M&dailyAlignment=0&alignmentTimezone=UTC&weeklyAlignment=Sunday",
                    oanda_base(&k),
                    t,
                    iv
                ),
                &[("Authorization", &format!("Bearer {}", k.token))],
            )?;
            if let Some(m) = v["errorMessage"].as_str() {
                return Err(m.to_string());
            }
            let arr = v["candles"].as_array().cloned().unwrap_or_default();
            Ok(arr
                .iter()
                .filter_map(|c| {
                    let m = &c["mid"];
                    bar(
                        parse_rfc3339(c["time"].as_str()?)?,
                        num(&m["o"]),
                        num(&m["h"]),
                        num(&m["l"]),
                        num(&m["c"]),
                        num(&c["volume"]),
                    )
                })
                .collect())
        }
    }
}

fn kraken_err(v: &J) -> Result<(), String> {
    match v["error"].as_array() {
        Some(e) if !e.is_empty() => Err(e
            .iter()
            .filter_map(|x| x.as_str())
            .collect::<Vec<_>>()
            .join(", ")),
        _ => Ok(()),
    }
}

fn polygon_err(v: &J) -> Result<(), String> {
    match v["status"].as_str() {
        Some("ERROR") | Some("NOT_AUTHORIZED") => Err(v["error"]
            .as_str()
            .or(v["message"].as_str())
            .unwrap_or("not authorized")
            .to_string()),
        _ => Ok(()),
    }
}

fn oanda_base(k: &OandaKey) -> &'static str {
    if k.account.eq_ignore_ascii_case("live") {
        "https://api-fxtrade.oanda.com"
    } else {
        "https://api-fxpractice.oanda.com"
    }
}

/// Polygon futures aggregates (needs a plan with futures data).
fn polygon_futures(
    agent: &ureq::Agent,
    key: &str,
    contract: &str,
    tf: Timeframe,
) -> Result<Vec<Bar>, String> {
    let res = match tf {
        Timeframe::M1 => "1min",
        Timeframe::M5 => "5min",
        Timeframe::M15 => "15min",
        Timeframe::M30 => "30min",
        Timeframe::H1 => "1hour",
        Timeframe::H4 => "4hour",
        _ => "1session",
    };
    let v = get(
        agent,
        &format!(
            "https://api.polygon.io/futures/vX/aggs/{}?resolution={}&sort=window_start.desc&limit=5000&apiKey={}",
            contract, res, key
        ),
        &[],
    )?;
    polygon_err(&v)?;
    let arr = v["results"].as_array().cloned().unwrap_or_default();
    let bars: Vec<Bar> = arr
        .iter()
        .filter_map(|b| {
            let ws = b["window_start"].as_i64()?;
            // nanoseconds
            let t = if ws > 10_000_000_000_000 {
                ws / 1_000_000_000
            } else {
                ws / 1000
            };
            bar(
                t,
                num(&b["open"]),
                num(&b["high"]),
                num(&b["low"]),
                num(&b["close"]),
                num(&b["volume"]),
            )
        })
        .collect();
    Ok(match tf {
        Timeframe::W1 | Timeframe::MN1 => crate::history::aggregate(
            &{
                let mut b = bars;
                b.sort_by_key(|x| x.time);
                b
            },
            tf,
        ),
        _ => bars,
    })
}

/// Nasdaq: daily history (about ten years) and today's minute prices.
fn nasdaq_history(agent: &ureq::Agent, t: &str, tf: Timeframe) -> Result<Vec<Bar>, String> {
    let hdr = [("Accept", "application/json")];
    let mut last_err = String::from("no data");
    for class in ["stocks", "etf"] {
        if tf == Timeframe::D1 {
            let from = ymd(crate::market::unix_now() - 10 * 365 * 86400);
            let v = match get(
                agent,
                &format!(
                    "https://api.nasdaq.com/api/quote/{}/historical?assetclass={}&fromdate={}&limit=9999",
                    t, class, from
                ),
                &hdr,
            ) {
                Ok(v) => v,
                Err(e) => {
                    last_err = e;
                    continue;
                }
            };
            let rows = v["data"]["tradesTable"]["rows"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if rows.is_empty() {
                continue;
            }
            return Ok(rows
                .iter()
                .filter_map(|r| {
                    let d = r["date"].as_str()?;
                    let mut p = d.split('/');
                    let (m, dd, y): (i64, i64, i64) = (
                        p.next()?.parse().ok()?,
                        p.next()?.parse().ok()?,
                        p.next()?.parse().ok()?,
                    );
                    bar(
                        crate::mql::lexer::days_from_civil(y, m, dd) * 86400,
                        num(&r["open"]),
                        num(&r["high"]),
                        num(&r["low"]),
                        num(&r["close"]),
                        num(&r["volume"]),
                    )
                })
                .collect());
        } else {
            let v = match get(
                agent,
                &format!(
                    "https://api.nasdaq.com/api/quote/{}/chart?assetclass={}",
                    t, class
                ),
                &hdr,
            ) {
                Ok(v) => v,
                Err(e) => {
                    last_err = e;
                    continue;
                }
            };
            let pts = v["data"]["chart"].as_array().cloned().unwrap_or_default();
            if pts.is_empty() {
                continue;
            }
            // price points (x = ms, Eastern time shown as UTC by the service): one bar each
            let mut out: Vec<Bar> = vec![];
            for p in pts {
                let (Some(x), y) = (p["x"].as_i64(), num(&p["y"])) else {
                    continue;
                };
                let t = crate::history::bar_open_time(Timeframe::M1, x / 1000);
                // the service's x is New York wall time written as UTC
                let t = t + if crate::tz::us_dst(t) {
                    4 * 3600
                } else {
                    5 * 3600
                };
                match out.last_mut() {
                    Some(l) if l.time == t => {
                        l.high = l.high.max(y);
                        l.low = l.low.min(y);
                        l.close = y;
                    }
                    _ => {
                        if let Some(b) = bar(t, y, y, y, y, 0.0) {
                            out.push(b);
                        }
                    }
                }
            }
            return Ok(out);
        }
    }
    Err(last_err)
}

// ---------- quotes ----------

/// A price: ask is NaN when the source gives only the last trade.
pub struct Q {
    pub ticker: String,
    pub bid: f64,
    pub ask: f64,
}

/// Current prices of several instruments from one source.
pub fn fetch_quotes(
    agent: &ureq::Agent,
    keys: &Keys,
    p: Provider,
    tickers: &[String],
) -> Result<Vec<Q>, String> {
    let q = |t: &str, b: f64, a: f64| Q {
        ticker: t.to_string(),
        bid: b,
        ask: a,
    };
    let mut out = vec![];
    match p {
        Provider::Binance | Provider::BinanceUs => {
            let base = if p == Provider::Binance {
                "https://data-api.binance.vision/api/v3"
            } else {
                "https://api.binance.us/api/v3"
            };
            let list = tickers
                .iter()
                .map(|t| format!("\"{}\"", t))
                .collect::<Vec<_>>()
                .join(",");
            let v = get(
                agent,
                &format!("{}/ticker/bookTicker?symbols=[{}]", base, list),
                &[],
            )?;
            for x in v.as_array().cloned().unwrap_or_default() {
                if let Some(s) = x["symbol"].as_str() {
                    out.push(q(s, num(&x["bidPrice"]), num(&x["askPrice"])));
                }
            }
        }
        Provider::BinanceFutures => {
            for t in tickers {
                let x = get(
                    agent,
                    &format!(
                        "https://fapi.binance.com/fapi/v1/ticker/bookTicker?symbol={}",
                        t
                    ),
                    &[],
                )?;
                out.push(q(t, num(&x["bidPrice"]), num(&x["askPrice"])));
            }
        }
        Provider::Coinbase => {
            for t in tickers {
                let x = get(
                    agent,
                    &format!("https://api.exchange.coinbase.com/products/{}/ticker", t),
                    &[],
                )?;
                out.push(q(t, num(&x["bid"]), num(&x["ask"])));
            }
        }
        Provider::Kraken => {
            for t in tickers {
                let v = get(
                    agent,
                    &format!("https://api.kraken.com/0/public/Ticker?pair={}", t),
                    &[],
                )?;
                kraken_err(&v)?;
                if let Some((_, x)) = v["result"].as_object().and_then(|o| o.iter().next()) {
                    out.push(q(t, num(&x["b"][0]), num(&x["a"][0])));
                }
            }
        }
        Provider::Bybit => {
            for t in tickers {
                let v = get(
                    agent,
                    &format!(
                        "https://api.bybit.com/v5/market/tickers?category=linear&symbol={}",
                        t
                    ),
                    &[],
                )?;
                let x = &v["result"]["list"][0];
                out.push(q(t, num(&x["bid1Price"]), num(&x["ask1Price"])));
            }
        }
        Provider::Blofin => {
            for t in tickers {
                let v = get(
                    agent,
                    &format!(
                        "https://openapi.blofin.com/api/v1/market/tickers?instId={}",
                        t
                    ),
                    &[],
                )?;
                let x = &v["data"][0];
                out.push(q(t, num(&x["bidPrice"]), num(&x["askPrice"])));
            }
        }
        Provider::Yahoo => {
            let v = get(
                agent,
                &format!(
                    "https://query1.finance.yahoo.com/v7/finance/spark?symbols={}&range=1d&interval=5m",
                    tickers.join(",")
                ),
                &[],
            )?;
            for r in v["spark"]["result"].as_array().cloned().unwrap_or_default() {
                let s = r["symbol"].as_str().unwrap_or("");
                let price = num(&r["response"][0]["meta"]["regularMarketPrice"]);
                out.push(q(s, price, f64::NAN));
            }
        }
        Provider::Nasdaq => {
            for t in tickers {
                let mut got = false;
                for class in ["stocks", "etf"] {
                    let Ok(v) = get(
                        agent,
                        &format!(
                            "https://api.nasdaq.com/api/quote/{}/info?assetclass={}",
                            t, class
                        ),
                        &[("Accept", "application/json")],
                    ) else {
                        continue;
                    };
                    let d = &v["data"]["primaryData"];
                    if d.is_null() {
                        continue;
                    }
                    let last = num(&d["lastSalePrice"]);
                    let (b, a) = (num(&d["bidPrice"]), num(&d["askPrice"]));
                    if b.is_finite() && a.is_finite() && b > 0.0 && a >= b {
                        out.push(q(t, b, a));
                    } else {
                        out.push(q(t, last, f64::NAN));
                    }
                    got = true;
                    break;
                }
                if !got {
                    return Err(format!("Nasdaq has no quote for {}", t));
                }
            }
        }
        Provider::Alpaca => {
            let k = keys.alpaca.clone().unwrap_or_default();
            let v = get(
                agent,
                &format!(
                    "https://data.alpaca.markets/v2/stocks/quotes/latest?symbols={}&feed={}",
                    tickers.join(","),
                    k.feed
                ),
                &[
                    ("APCA-API-KEY-ID", k.key_id.as_str()),
                    ("APCA-API-SECRET-KEY", k.secret_key.as_str()),
                ],
            )?;
            if let Some(m) = v["quotes"].as_object() {
                for (s, x) in m {
                    out.push(q(s, num(&x["bp"]), num(&x["ap"])));
                }
            }
        }
        Provider::Polygon => {
            let key = keys.polygon.clone().unwrap_or_default().api_key;
            for t in tickers {
                if let Some(contract) = t.strip_prefix("fut:") {
                    let b = polygon_futures(agent, &key, contract, Timeframe::M1)?;
                    if let Some(l) = b.iter().max_by_key(|b| b.time) {
                        out.push(q(t, l.close, f64::NAN));
                    }
                    continue;
                }
                // live bid/ask on paid plans; the free plan has the previous close only
                match get(
                    agent,
                    &format!("https://api.polygon.io/v2/last/nbbo/{}?apiKey={}", t, key),
                    &[],
                ) {
                    Ok(v) if v["results"].is_object() => {
                        out.push(q(t, num(&v["results"]["p"]), num(&v["results"]["P"])));
                    }
                    _ => {
                        let v = get(
                            agent,
                            &format!(
                                "https://api.polygon.io/v2/aggs/ticker/{}/prev?adjusted=true&apiKey={}",
                                t, key
                            ),
                            &[],
                        )?;
                        polygon_err(&v)?;
                        out.push(q(t, num(&v["results"][0]["c"]), f64::NAN));
                    }
                }
            }
        }
        Provider::Oanda => {
            let k = keys.oanda.clone().unwrap_or_default();
            let auth = format!("Bearer {}", k.token);
            let id = oanda_account(agent, &k)?;
            let v = get(
                agent,
                &format!(
                    "{}/v3/accounts/{}/pricing?instruments={}",
                    oanda_base(&k),
                    id,
                    tickers.join(",")
                ),
                &[("Authorization", &auth)],
            )?;
            if let Some(m) = v["errorMessage"].as_str() {
                return Err(m.to_string());
            }
            for x in v["prices"].as_array().cloned().unwrap_or_default() {
                if let Some(s) = x["instrument"].as_str() {
                    out.push(q(
                        s,
                        num(&x["bids"][0]["price"]),
                        num(&x["asks"][0]["price"]),
                    ));
                }
            }
        }
    }
    Ok(out)
}

/// The first account of an OANDA token (cached).
fn oanda_account(agent: &ureq::Agent, k: &OandaKey) -> Result<String, String> {
    static CACHE: Mutex<Option<(String, String)>> = Mutex::new(None);
    if let Ok(c) = CACHE.lock() {
        if let Some((tok, id)) = c.as_ref() {
            if *tok == k.token {
                return Ok(id.clone());
            }
        }
    }
    let v = get(
        agent,
        &format!("{}/v3/accounts", oanda_base(k)),
        &[("Authorization", &format!("Bearer {}", k.token))],
    )?;
    let id = v["accounts"][0]["id"]
        .as_str()
        .ok_or_else(|| {
            v["errorMessage"]
                .as_str()
                .unwrap_or("no account for this token")
                .to_string()
        })?
        .to_string();
    if let Ok(mut c) = CACHE.lock() {
        *c = Some((k.token.clone(), id.clone()));
    }
    Ok(id)
}

// ---------- checks ----------

/// Try each premium source that has a key (and, with `all`, every free one) and report.
pub fn check_sources(all: bool) -> Vec<(String, Result<String, String>)> {
    let keys = Keys::load();
    let a = agent();
    let mut out = vec![];
    let probe = |p: Provider, t: &str| -> Result<String, String> {
        let r = Route {
            provider: p,
            ticker: t.to_string(),
        };
        let bars = fetch_history(&a, &keys, &r, Timeframe::H1)?;
        let q = fetch_quotes(&a, &keys, p, &[t.to_string()])?;
        let last = q
            .first()
            .map(|x| x.bid)
            .filter(|x| x.is_finite())
            .or(bars.last().map(|b| b.close))
            .unwrap_or(f64::NAN);
        Ok(format!("{} H1 bars, {} last {}", bars.len(), t, last))
    };
    let sample = |p: Provider| -> &'static str {
        match p {
            Provider::Binance
            | Provider::BinanceUs
            | Provider::Bybit
            | Provider::BinanceFutures => "BTCUSDT",
            Provider::Coinbase => "BTC-USD",
            Provider::Kraken => "XBTUSD",
            Provider::Blofin => "BTC-USDT",
            Provider::Yahoo | Provider::Nasdaq | Provider::Alpaca | Provider::Polygon => "AAPL",
            Provider::Oanda => "EUR_USD",
        }
    };
    for p in Provider::ALL {
        if p.needs_key() {
            if !keys.has(p) {
                out.push((p.name().to_string(), Err("no key in feeds.json".into())));
                continue;
            }
        } else if !all {
            continue;
        }
        out.push((p.name().to_string(), probe(p, sample(p))));
    }
    out
}

/// `rust-terminal check-feeds`: print the result of every source check.
pub fn print_check() {
    println!("keys file: {}", Keys::path().display());
    for (name, r) in check_sources(true) {
        match r {
            Ok(s) => println!("  OK    {:<22} {}", name, s),
            Err(e) => println!("  FAIL  {:<22} {}", name, e),
        }
    }
}

// ---------- the background feed ----------

pub enum FeedMsg {
    Quote {
        symbol: String,
        bid: f64,
        /// NaN when the source gives only one price
        ask: f64,
        time: i64,
    },
    History {
        symbol: String,
        tf: Timeframe,
        bars: Vec<Bar>,
    },
    /// the source now serving a symbol
    Source {
        symbol: String,
        provider: Provider,
    },
    Status(String),
    Error(String),
}

/// Bars of one timeframe for a symbol from its first working source (MCP tools).
pub fn fetch_any(symbol: &str, tf: Timeframe) -> Result<(Provider, Vec<Bar>), String> {
    let keys = Keys::load();
    let a = agent();
    let mut errs = vec![];
    for r in routes(symbol, &keys) {
        match fetch_history(&a, &keys, &r, tf) {
            Ok(b) if !b.is_empty() => return Ok((r.provider, b)),
            Ok(_) => errs.push(format!("{}: no bars", r.provider.name())),
            Err(e) => errs.push(format!("{}: {}", r.provider.name(), e)),
        }
    }
    if errs.is_empty() {
        Err(format!("{} has no online source", symbol))
    } else {
        Err(errs.join("; "))
    }
}

pub struct LiveFeed {
    rx: Receiver<FeedMsg>,
    stop: Arc<Mutex<bool>>,
    pub symbols: Vec<String>,
}

impl LiveFeed {
    /// Start streaming the given symbols.
    pub fn start(symbols: Vec<String>) -> LiveFeed {
        let (tx, rx) = channel();
        let stop = Arc::new(Mutex::new(false));
        let stop2 = stop.clone();
        let names = symbols.clone();
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

struct Live {
    name: String,
    routes: Vec<Route>,
    cur: usize,
    fails: u32,
    ok: bool,
}

impl Live {
    fn route(&self) -> &Route {
        &self.routes[self.cur]
    }
}

/// Load every timeframe of a symbol from its current route, moving down the list until a
/// source answers. Returns false when no source works.
fn load_symbol(
    agent: &ureq::Agent,
    keys: &Keys,
    s: &mut Live,
    tx: &Sender<FeedMsg>,
    stopped: &dyn Fn() -> bool,
) -> bool {
    let mut errs = vec![];
    while s.cur < s.routes.len() {
        let r = s.route().clone();
        // probe with hourly bars, then the rest
        match fetch_history(agent, keys, &r, Timeframe::H1) {
            Ok(b) if !b.is_empty() => {
                let _ = tx.send(FeedMsg::Source {
                    symbol: s.name.clone(),
                    provider: r.provider,
                });
                let _ = tx.send(FeedMsg::History {
                    symbol: s.name.clone(),
                    tf: Timeframe::H1,
                    bars: b,
                });
                for tf in Timeframe::ALL {
                    if tf == Timeframe::H1 {
                        continue;
                    }
                    if stopped() {
                        return true;
                    }
                    match fetch_history(agent, keys, &r, tf) {
                        Ok(b) if !b.is_empty() => {
                            let _ = tx.send(FeedMsg::History {
                                symbol: s.name.clone(),
                                tf,
                                bars: b,
                            });
                        }
                        Ok(_) => {}
                        Err(e) => {
                            let _ = tx.send(FeedMsg::Error(format!(
                                "{} {} ({}): {}",
                                s.name,
                                tf.label(),
                                r.provider.name(),
                                e
                            )));
                        }
                    }
                }
                s.ok = true;
                s.fails = 0;
                return true;
            }
            Ok(_) => errs.push(format!("{}: no bars", r.provider.name())),
            Err(e) => errs.push(format!("{}: {}", r.provider.name(), e)),
        }
        s.cur += 1;
    }
    s.cur = 0;
    s.ok = false;
    let _ = tx.send(FeedMsg::Error(format!(
        "{}: no source answered ({})",
        s.name,
        errs.join("; ")
    )));
    false
}

fn run(symbols: Vec<String>, tx: Sender<FeedMsg>, stop: Arc<Mutex<bool>>) {
    let agent = agent();
    let keys = Keys::load();
    let stopped = || stop.lock().map(|s| *s).unwrap_or(true);
    let mut live: Vec<Live> = symbols
        .iter()
        .map(|n| Live {
            name: n.clone(),
            routes: routes(n, &keys),
            cur: 0,
            fails: 0,
            ok: false,
        })
        .filter(|l| !l.routes.is_empty())
        .collect();
    if live.is_empty() {
        let _ = tx.send(FeedMsg::Error(
            "no symbol has an online price source".into(),
        ));
        return;
    }
    for s in live.iter_mut() {
        if stopped() {
            return;
        }
        load_symbol(&agent, &keys, s, &tx, &stopped);
    }
    let summary = |live: &[Live]| {
        let ok: Vec<String> = live
            .iter()
            .filter(|s| s.ok)
            .map(|s| format!("{} ({})", s.name, s.route().provider.name()))
            .collect();
        format!(
            "live prices: {}",
            if ok.is_empty() {
                "none".into()
            } else {
                ok.join(", ")
            }
        )
    };
    let _ = tx.send(FeedMsg::Status(summary(&live)));
    let mut next: std::collections::HashMap<Provider, Instant> = Default::default();
    let mut retry_failed = Instant::now() + Duration::from_secs(60);
    while !stopped() {
        let now = Instant::now();
        let providers: Vec<Provider> = {
            let mut v: Vec<Provider> = live
                .iter()
                .filter(|s| s.ok)
                .map(|s| s.route().provider)
                .collect();
            v.sort_by_key(|p| p.name());
            v.dedup();
            v
        };
        for p in providers {
            if next.get(&p).is_some_and(|t| *t > now) {
                continue;
            }
            next.insert(p, now + p.poll());
            let idx: Vec<usize> = (0..live.len())
                .filter(|i| live[*i].ok && live[*i].route().provider == p)
                .collect();
            let tickers: Vec<String> = idx
                .iter()
                .map(|i| live[*i].route().ticker.clone())
                .collect();
            match fetch_quotes(&agent, &keys, p, &tickers) {
                Ok(qs) => {
                    let t = crate::market::unix_now();
                    for i in &idx {
                        let tk = live[*i].route().ticker.clone();
                        let found = qs.iter().find(|q| {
                            q.ticker.eq_ignore_ascii_case(&tk) && q.bid.is_finite() && q.bid > 0.0
                        });
                        match found {
                            Some(q) => {
                                live[*i].fails = 0;
                                let _ = tx.send(FeedMsg::Quote {
                                    symbol: live[*i].name.clone(),
                                    bid: q.bid,
                                    ask: q.ask,
                                    time: t,
                                });
                            }
                            None => live[*i].fails += 1,
                        }
                    }
                }
                Err(e) => {
                    for i in &idx {
                        live[*i].fails += 1;
                        if live[*i].fails == 1 || live[*i].fails % 30 == 0 {
                            let _ = tx.send(FeedMsg::Error(format!(
                                "{} quotes ({}): {}",
                                live[*i].name,
                                p.name(),
                                e
                            )));
                        }
                    }
                }
            }
            // a source that keeps failing: the next one takes over
            let mut changed = false;
            for i in idx {
                if live[i].fails >= 5 && live[i].cur + 1 < live[i].routes.len() {
                    live[i].cur += 1;
                    live[i].fails = 0;
                    let s = &mut live[i];
                    load_symbol(&agent, &keys, s, &tx, &stopped);
                    changed = true;
                }
            }
            if changed {
                let _ = tx.send(FeedMsg::Status(summary(&live)));
            }
        }
        // symbols with no working source: try again now and then
        if Instant::now() >= retry_failed {
            retry_failed = Instant::now() + Duration::from_secs(120);
            let mut changed = false;
            for s in live.iter_mut().filter(|s| !s.ok) {
                if stopped() {
                    return;
                }
                changed |= load_symbol(&agent, &keys, s, &tx, &stopped);
            }
            if changed {
                let _ = tx.send(FeedMsg::Status(summary(&live)));
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
