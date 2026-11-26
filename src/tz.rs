//! Time zones. Everything that comes from outside is turned into one clock, the terminal's
//! "server time", as in MetaTrader 5: bar times, TimeCurrent() and the chart axis use it.
//!
//! - Online sources and the system clock give UTC.
//! - Stored history is kept in UTC (a `.utc` marker in the symbol folder; older folders
//!   without it hold server time already and are used as they are).
//! - CSV files and MetaTrader exports are in the zone they were written in; the importer
//!   converts them to UTC.
//!
//! The default server zone is the one most brokers use: GMT+2, GMT+3 while the USA has
//! daylight saving time, so a trading day ends at 17:00 New York time and a week has five
//! daily bars.

use crate::market::{civil, Bar, Timeframe};
use std::sync::atomic::{AtomicI64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Utc,
    /// GMT+2 / GMT+3 with US daylight saving time (most MetaTrader brokers)
    Broker,
    /// the computer's own zone
    Local,
    /// a fixed offset in minutes east of UTC
    Fixed(i32),
}

impl Zone {
    pub fn key(self) -> String {
        match self {
            Zone::Utc => "utc".into(),
            Zone::Broker => "broker".into(),
            Zone::Local => "local".into(),
            Zone::Fixed(m) => format!(
                "{}{:02}:{:02}",
                if m < 0 { '-' } else { '+' },
                m.abs() / 60,
                m.abs() % 60
            ),
        }
    }
    pub fn parse(s: &str) -> Option<Zone> {
        let s = s.trim().to_ascii_lowercase();
        let s = s
            .strip_prefix("gmt")
            .or_else(|| s.strip_prefix("utc"))
            .map(|x| x.to_string())
            .unwrap_or(s);
        match s.as_str() {
            "" | "utc" | "gmt" => return Some(Zone::Utc),
            "broker" | "server" => return Some(Zone::Broker),
            "local" => return Some(Zone::Local),
            _ => {}
        }
        let (sign, rest) = match s.chars().next()? {
            '+' => (1, &s[1..]),
            '-' => (-1, &s[1..]),
            _ => (1, &s[..]),
        };
        let (h, m) = match rest.split_once(':') {
            Some((h, m)) => (h.parse::<i32>().ok()?, m.parse::<i32>().ok()?),
            None => (rest.parse::<i32>().ok()?, 0),
        };
        if h > 14 || m >= 60 {
            return None;
        }
        Some(Zone::Fixed(sign * (h * 60 + m)))
    }
    pub fn label(self) -> String {
        match self {
            Zone::Utc => "UTC".into(),
            Zone::Broker => "Broker (GMT+2, GMT+3 in US summer)".into(),
            Zone::Local => format!(
                "This computer ({})",
                Zone::Fixed((local_offset() / 60) as i32).key()
            ),
            Zone::Fixed(_) => format!("GMT{}", self.key()),
        }
    }
    /// The choices offered in the settings.
    pub fn choices() -> Vec<Zone> {
        let mut v = vec![Zone::Broker, Zone::Utc, Zone::Local];
        for h in -12..=14 {
            if h != 0 {
                v.push(Zone::Fixed(h * 60));
            }
        }
        v.push(Zone::Fixed(330)); // India
        v.push(Zone::Fixed(570)); // central Australia
        v
    }
    /// Seconds to add to a UTC time to get this zone's time.
    pub fn offset_at(self, utc: i64) -> i64 {
        match self {
            Zone::Utc => 0,
            Zone::Fixed(m) => m as i64 * 60,
            Zone::Local => local_offset(),
            Zone::Broker => {
                if us_dst(utc) {
                    3 * 3600
                } else {
                    2 * 3600
                }
            }
        }
    }
    pub fn from_utc(self, utc: i64) -> i64 {
        utc + self.offset_at(utc)
    }
    pub fn to_utc(self, t: i64) -> i64 {
        // the offset at the UTC instant itself; exact except inside the switch hour
        let guess = t - self.offset_at(t);
        t - self.offset_at(guess)
    }
}

/// Whether US daylight saving time is in effect: from the second Sunday of March 02:00
/// to the first Sunday of November 02:00 (New York local time).
pub fn us_dst(utc: i64) -> bool {
    let (y, _, _, _, _, _) = civil(utc);
    let nth_sunday = |month: i64, n: i64| -> i64 {
        let first = crate::mql::lexer::days_from_civil(y, month, 1);
        let dow = (first + 4).rem_euclid(7); // 0 = Sunday
        first + (7 - dow) % 7 + (n - 1) * 7
    };
    let start = nth_sunday(3, 2) * 86400 + 7 * 3600; // 02:00 EST = 07:00 UTC
    let end = nth_sunday(11, 1) * 86400 + 6 * 3600; // 02:00 EDT = 06:00 UTC
    utc >= start && utc < end
}

/// The computer's UTC offset in seconds (read once).
pub fn local_offset() -> i64 {
    static OFF: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| {
        if let Ok(v) = std::env::var("RUST_TERMINAL_LOCAL_OFFSET") {
            if let Some(Zone::Fixed(m)) = Zone::parse(&v) {
                return m as i64 * 60;
            }
        }
        #[cfg(unix)]
        {
            if let Ok(o) = std::process::Command::new("date").arg("+%z").output() {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if s.len() == 5 {
                    let sign = if s.starts_with('-') { -1 } else { 1 };
                    let h: i64 = s[1..3].parse().unwrap_or(0);
                    let m: i64 = s[3..5].parse().unwrap_or(0);
                    return sign * (h * 3600 + m * 60);
                }
            }
        }
        #[cfg(windows)]
        {
            if let Ok(o) = std::process::Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    "[int][TimeZoneInfo]::Local.GetUtcOffset([DateTime]::UtcNow).TotalMinutes",
                ])
                .output()
            {
                if let Ok(m) = String::from_utf8_lossy(&o.stdout).trim().parse::<i64>() {
                    return m * 60;
                }
            }
        }
        0
    })
}

// ---------- the terminal's zone (a setting) ----------

const BROKER: i64 = i64::MIN;
const LOCAL: i64 = i64::MIN + 1;
static SERVER: AtomicI64 = AtomicI64::new(BROKER);

fn encode(z: Zone) -> i64 {
    match z {
        Zone::Broker => BROKER,
        Zone::Local => LOCAL,
        Zone::Utc => 0,
        Zone::Fixed(m) => m as i64,
    }
}

/// The terminal's (server) zone.
pub fn server() -> Zone {
    match SERVER.load(Ordering::Relaxed) {
        BROKER => Zone::Broker,
        LOCAL => Zone::Local,
        0 => Zone::Utc,
        m => Zone::Fixed(m as i32),
    }
}

pub fn set_server(z: Zone) {
    SERVER.store(encode(z), Ordering::Relaxed);
}

/// Current server time.
pub fn server_now() -> i64 {
    server().from_utc(crate::market::unix_now())
}

/// A server time back to UTC (TimeGMT in the tester).
pub fn server_to_utc(t: i64) -> i64 {
    server().to_utc(t)
}

/// Bars in UTC to the server zone. Intraday bars move by the offset; H4 and longer bars are
/// labelled with the server period that contains their (shifted) opening time, so a source's
/// daily bar keeps its own day boundaries but gets the right date.
pub fn bars_to_server(bars: &mut Vec<Bar>, tf: Timeframe) {
    convert_bars(bars, tf, |t| server().from_utc(t));
}

/// Bars written in `zone` to UTC.
pub fn bars_to_utc(bars: &mut Vec<Bar>, zone: Zone) {
    if zone == Zone::Utc {
        return;
    }
    for b in bars.iter_mut() {
        b.time = zone.to_utc(b.time);
    }
}

fn convert_bars(bars: &mut Vec<Bar>, tf: Timeframe, f: impl Fn(i64) -> i64) {
    if server() == Zone::Utc {
        return;
    }
    for b in bars.iter_mut() {
        let t = f(b.time);
        b.time = if tf.secs() > 3600 {
            crate::history::bar_open_time(tf, t)
        } else {
            t
        };
    }
    // two bars can land in one period at a switch: keep the later values merged
    let mut out: Vec<Bar> = Vec::with_capacity(bars.len());
    for b in bars.drain(..) {
        match out.last_mut() {
            Some(l) if l.time >= b.time => {
                l.high = l.high.max(b.high);
                l.low = l.low.min(b.low);
                l.close = b.close;
                l.tick_volume += b.tick_volume;
            }
            _ => out.push(b),
        }
    }
    *bars = out;
}

// ---------- settings file ----------

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Settings {
    #[serde(default)]
    server_zone: Option<String>,
    #[serde(default)]
    import_zone: Option<String>,
}

fn settings_path() -> std::path::PathBuf {
    crate::storage::data_dir().join("settings.json")
}

fn read_settings() -> Settings {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_settings(s: &Settings) {
    let _ = std::fs::create_dir_all(crate::storage::data_dir());
    if let Ok(t) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(settings_path(), t);
    }
}

/// Load the saved server zone (call once at start).
pub fn load_settings() {
    if let Some(z) = read_settings().server_zone.as_deref().and_then(Zone::parse) {
        set_server(z);
    }
}

pub fn save_server(z: Zone) {
    set_server(z);
    let mut s = read_settings();
    s.server_zone = Some(z.key());
    write_settings(&s);
}

/// Zone that CSV files are assumed to be written in (default: the broker zone).
pub fn import_zone() -> Zone {
    read_settings()
        .import_zone
        .as_deref()
        .and_then(Zone::parse)
        .unwrap_or(Zone::Broker)
}

pub fn save_import_zone(z: Zone) {
    let mut s = read_settings();
    s.import_zone = Some(z.key());
    write_settings(&s);
}

/// The zone of a MetaTrader export, from the server offset (seconds) the script recorded.
pub fn zone_from_offset(secs: i64) -> Zone {
    if secs == 7200 || secs == 10800 {
        Zone::Broker
    } else {
        Zone::Fixed((secs / 60) as i32)
    }
}
