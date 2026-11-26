//! Real price history: CSV import (MetaTrader exports and common formats), storage per
//! symbol and timeframe, building higher timeframes from lower ones, and symbol
//! properties exported from a real terminal.
//!
//! Stored files: `<data dir>/history/<SYMBOL>/<TF>.csv` with the header
//! `time,open,high,low,close,tick_volume,spread` (time in unix seconds).

use crate::market::{civil, Bar, Timeframe};
use std::path::{Path, PathBuf};

pub fn history_dir() -> PathBuf {
    crate::storage::data_dir().join("history")
}

/// Opening time of the bar that contains `t`: minutes and hours align to their size,
/// weeks start on Sunday 00:00 and months on the 1st, as in the platform.
pub fn bar_open_time(tf: Timeframe, t: i64) -> i64 {
    match tf {
        Timeframe::W1 => {
            let day = t.div_euclid(86400);
            let since_sunday = (day + 4).rem_euclid(7);
            (day - since_sunday) * 86400
        }
        Timeframe::MN1 => {
            let (y, m, _, _, _, _) = civil(t);
            crate::mql::lexer::days_from_civil(y, m as i64, 1) * 86400
        }
        _ => t - t.rem_euclid(tf.secs()),
    }
}

/// Build bars of `tf` from bars of a smaller timeframe (both oldest first).
pub fn aggregate(src: &[Bar], tf: Timeframe) -> Vec<Bar> {
    let mut out: Vec<Bar> = Vec::with_capacity(src.len() / 4 + 1);
    for b in src {
        let t = bar_open_time(tf, b.time);
        match out.last_mut() {
            Some(l) if l.time == t => {
                l.high = l.high.max(b.high);
                l.low = l.low.min(b.low);
                l.close = b.close;
                l.tick_volume += b.tick_volume;
                l.spread = b.spread;
            }
            _ => out.push(Bar { time: t, ..*b }),
        }
    }
    out
}

/// Timeframe whose length is closest to the typical spacing of the bars.
pub fn detect_timeframe(bars: &[Bar]) -> Option<Timeframe> {
    if bars.len() < 3 {
        return None;
    }
    let mut d: Vec<i64> = bars
        .windows(2)
        .map(|w| w[1].time - w[0].time)
        .filter(|x| *x > 0)
        .collect();
    if d.is_empty() {
        return None;
    }
    d.sort_unstable();
    // the smallest common gap: weekends and holidays make larger gaps
    let typical = d[d.len() / 10];
    Timeframe::ALL.iter().copied().min_by_key(|tf| {
        let s = tf.secs();
        let r = if typical > s {
            typical as f64 / s as f64
        } else {
            s as f64 / typical as f64
        };
        (r * 1000.0) as i64
    })
}

fn parse_date(s: &str) -> Option<(i64, u32, u32)> {
    let parts: Vec<&str> = s.split(|c| c == '.' || c == '-' || c == '/').collect();
    if parts.len() != 3 {
        if s.len() == 8 && s.chars().all(|c| c.is_ascii_digit()) {
            // YYYYMMDD
            return Some((
                s[0..4].parse().ok()?,
                s[4..6].parse().ok()?,
                s[6..8].parse().ok()?,
            ));
        }
        return None;
    }
    let a: i64 = parts[0].trim().parse().ok()?;
    let b: u32 = parts[1].trim().parse().ok()?;
    let c: i64 = parts[2].trim().parse().ok()?;
    if parts[0].trim().len() == 4 {
        Some((a, b, c as u32))
    } else {
        // DD.MM.YYYY
        Some((c, b, a as u32))
    }
}

fn parse_time(s: &str) -> Option<i64> {
    let s = s.trim();
    let s = s.split('.').next().unwrap_or(s); // drop milliseconds
    let parts: Vec<&str> = s.split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        if s.len() == 6 && s.chars().all(|c| c.is_ascii_digit()) {
            let h: i64 = s[0..2].parse().ok()?;
            let m: i64 = s[2..4].parse().ok()?;
            let sec: i64 = s[4..6].parse().ok()?;
            return Some(h * 3600 + m * 60 + sec);
        }
        return None;
    }
    let h: i64 = parts[0].parse().ok()?;
    let m: i64 = parts.get(1).map(|x| x.parse().unwrap_or(0)).unwrap_or(0);
    let sec: i64 = parts.get(2).map(|x| x.parse().unwrap_or(0)).unwrap_or(0);
    Some(h * 3600 + m * 60 + sec)
}

fn to_unix(y: i64, m: u32, d: u32, secs: i64) -> i64 {
    crate::mql::lexer::days_from_civil(y, m as i64, d as i64) * 86400 + secs
}

/// Parse a date-time written in one field: "2024.01.02 10:00", "2024-01-02T10:00:00Z",
/// a unix time in seconds or milliseconds.
fn parse_datetime_field(s: &str) -> Option<i64> {
    let s = s.trim().trim_matches('"');
    if let Ok(n) = s.parse::<i64>() {
        if n > 100_000_000_000 {
            return Some(n / 1000);
        }
        if n > 100_000_000 {
            return Some(n);
        }
    }
    let s2 = s.trim_end_matches('Z').replace('T', " ");
    let mut it = s2.split_whitespace();
    let date = parse_date(it.next()?)?;
    let secs = it.next().map(parse_time).unwrap_or(Some(0))?;
    Some(to_unix(date.0, date.1, date.2, secs))
}

/// Parse CSV price history. Recognised layouts:
/// - MetaTrader 5 export: `<DATE> <TIME> <OPEN> <HIGH> <LOW> <CLOSE> <TICKVOL> <VOL> <SPREAD>`
/// - MetaTrader 4 / generic: `2024.01.02,10:00,open,high,low,close,volume`
/// - one date-time column (text or unix seconds/milliseconds) followed by OHLC(V)
/// Separator can be tab, comma or semicolon; a header line is optional.
pub fn parse_csv(text: &str) -> Result<Vec<Bar>, String> {
    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    let first = lines.first().ok_or("the file is empty")?;
    let sep = [
        ('\t', first.matches('\t').count()),
        (';', first.matches(';').count()),
        (',', first.matches(',').count()),
    ]
    .into_iter()
    .max_by_key(|(_, n)| *n)
    .map(|(c, _)| c)
    .unwrap_or(',');
    let split = |l: &str| -> Vec<String> {
        l.split(sep)
            .map(|f| f.trim().trim_matches('"').to_string())
            .collect()
    };
    // header?
    let head = split(first);
    let has_header = head.first().is_some_and(|f| {
        f.chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '<')
    });
    let lower: Vec<String> = head
        .iter()
        .map(|h| {
            h.to_ascii_lowercase()
                .trim_matches(|c| c == '<' || c == '>')
                .to_string()
        })
        .collect();
    let col = |names: &[&str]| -> Option<usize> {
        lower.iter().position(|h| names.iter().any(|n| h == n))
    };
    let (mut i_date, mut i_time, mut i_o, mut i_h, mut i_l, mut i_c, mut i_v, mut i_sp) =
        (None, None, None, None, None, None, None, None);
    if has_header {
        i_date = col(&[
            "date",
            "datetime",
            "timestamp",
            "time (utc)",
            "gmt time",
            "local time",
            "open time",
            "open_time",
        ]);
        i_time = col(&["time"]);
        if i_date.is_none() {
            i_date = i_time;
            i_time = None;
        }
        i_o = col(&["open", "o"]);
        i_h = col(&["high", "h"]);
        i_l = col(&["low", "l"]);
        i_c = col(&["close", "c"]);
        i_v = col(&["tickvol", "tick_volume", "tickvolume", "volume", "vol", "v"]);
        i_sp = col(&["spread"]);
    }
    let body = if has_header { &lines[1..] } else { &lines[..] };
    let sample = body.first().map(|l| split(l)).ok_or("no price rows")?;
    if i_o.is_none() {
        // no header: decide from the first row
        let two_cols = sample.len() >= 6
            && parse_date(&sample[0]).is_some()
            && parse_time(&sample[1]).is_some()
            && !sample[0].contains(' ');
        if two_cols {
            i_date = Some(0);
            i_time = Some(1);
            i_o = Some(2);
        } else {
            i_date = Some(0);
            i_o = Some(1);
        }
        let o = i_o.unwrap();
        i_h = Some(o + 1);
        i_l = Some(o + 2);
        i_c = Some(o + 3);
        i_v = if sample.len() > o + 4 {
            Some(o + 4)
        } else {
            None
        };
        // MetaTrader 5 layout without header: tickvol, vol, spread
        if sample.len() > o + 6 {
            i_sp = Some(o + 6);
        }
    }
    let (i_date, i_o, i_h, i_l, i_c) = (
        i_date.ok_or("no date column")?,
        i_o.ok_or("no open column")?,
        i_h.ok_or("no high column")?,
        i_l.ok_or("no low column")?,
        i_c.ok_or("no close column")?,
    );
    let mut bars = Vec::with_capacity(body.len());
    let mut bad = 0;
    for l in body {
        let f = split(l);
        let get = |i: usize| f.get(i).map(|s| s.as_str()).unwrap_or("");
        let t = match i_time {
            Some(it) => parse_date(get(i_date))
                .and_then(|d| parse_time(get(it)).map(|s| to_unix(d.0, d.1, d.2, s))),
            None => parse_datetime_field(get(i_date)),
        };
        let num = |i: usize| get(i).replace(',', ".").parse::<f64>().ok();
        match (t, num(i_o), num(i_h), num(i_l), num(i_c)) {
            (Some(time), Some(open), Some(high), Some(low), Some(close)) => {
                let tick_volume = i_v.and_then(num).unwrap_or(0.0) as i64;
                let spread = i_sp.and_then(num).unwrap_or(0.0) as i32;
                bars.push(Bar {
                    time,
                    open,
                    high: high.max(open).max(close),
                    low: low.min(open).min(close),
                    close,
                    tick_volume,
                    spread,
                });
            }
            _ => {
                bad += 1;
                if bad > 20 && bars.is_empty() {
                    return Err(format!("unrecognised row: {}", l));
                }
            }
        }
    }
    if bars.is_empty() {
        return Err("no price rows could be read".into());
    }
    bars.sort_by_key(|b| b.time);
    bars.dedup_by_key(|b| b.time);
    Ok(bars)
}

/// Symbol and timeframe from a file name such as `EURUSD_M1.csv`,
/// `EURUSD_M1_202401020000_202411221759.csv` or `XAUUSD,H1.csv`.
pub fn name_hints(path: &Path) -> (Option<String>, Option<Timeframe>) {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let parts: Vec<&str> = stem
        .split(|c: char| c == '_' || c == ',' || c == ' ' || c == '-')
        .filter(|p| !p.is_empty())
        .collect();
    let tf = parts.iter().find_map(|p| {
        Timeframe::ALL
            .iter()
            .copied()
            .find(|t| t.label().eq_ignore_ascii_case(p))
    });
    let sym = parts
        .first()
        .filter(|p| {
            p.chars().any(|c| c.is_alphabetic())
                && Timeframe::ALL
                    .iter()
                    .all(|t| !t.label().eq_ignore_ascii_case(p))
        })
        .map(|s| s.to_ascii_uppercase());
    (sym, tf)
}

pub fn file_for(symbol: &str, tf: Timeframe) -> PathBuf {
    history_dir()
        .join(symbol.to_ascii_uppercase())
        .join(format!("{}.csv", tf.label()))
}

pub fn save(symbol: &str, tf: Timeframe, bars: &[Bar]) -> std::io::Result<()> {
    let p = file_for(symbol, tf);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut s = String::with_capacity(bars.len() * 48 + 64);
    s.push_str("time,open,high,low,close,tick_volume,spread\n");
    for b in bars {
        s.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            b.time, b.open, b.high, b.low, b.close, b.tick_volume, b.spread
        ));
    }
    let tmp = p.with_extension("csv.tmp");
    std::fs::write(&tmp, s)?;
    std::fs::rename(tmp, p)
}

fn read_stored(symbol: &str, tf: Timeframe) -> Option<Vec<Bar>> {
    let text = std::fs::read_to_string(file_for(symbol, tf)).ok()?;
    parse_csv(&text).ok()
}

/// Real bars for a symbol and timeframe: the stored file, or built from the nearest
/// smaller stored timeframe.
pub fn load(symbol: &str, tf: Timeframe) -> Option<Vec<Bar>> {
    if let Some(b) = read_stored(symbol, tf) {
        return Some(b);
    }
    for smaller in Timeframe::ALL.iter().rev().filter(|t| t.secs() < tf.secs()) {
        if let Some(b) = read_stored(symbol, *smaller) {
            return Some(aggregate(&b, tf));
        }
    }
    None
}

/// Symbols with stored history and their stored timeframes.
pub fn stored() -> Vec<(String, Vec<Timeframe>)> {
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir(history_dir()) {
        for e in rd.flatten() {
            if !e.path().is_dir() {
                continue;
            }
            let sym = e.file_name().to_string_lossy().to_string();
            let tfs: Vec<Timeframe> = Timeframe::ALL
                .iter()
                .copied()
                .filter(|t| file_for(&sym, *t).exists())
                .collect();
            if !tfs.is_empty() {
                out.push((sym, tfs));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

pub fn has_history(symbol: &str) -> bool {
    Timeframe::ALL.iter().any(|t| file_for(symbol, *t).exists())
}

/// Result of importing one file.
#[derive(Debug, Clone)]
pub struct Imported {
    pub symbol: String,
    pub tf: Timeframe,
    pub bars: usize,
    pub first: i64,
    pub last: i64,
}

/// Import a CSV file. `symbol`/`tf` override what the file name and the data suggest.
/// New bars are merged with already stored ones.
pub fn import_file(
    path: &Path,
    symbol: Option<&str>,
    tf: Option<Timeframe>,
) -> Result<Imported, String> {
    let raw = std::fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let text = crate::storage::decode_source(&raw);
    let bars = parse_csv(&text).map_err(|e| format!("{}: {}", path.display(), e))?;
    let (hint_sym, hint_tf) = name_hints(path);
    let symbol = symbol
        .map(|s| s.to_ascii_uppercase())
        .or(hint_sym)
        .ok_or("cannot tell the symbol from the file name; choose it")?;
    let tf = tf
        .or(hint_tf)
        .or_else(|| detect_timeframe(&bars))
        .ok_or("cannot tell the timeframe")?;
    let merged = match read_stored(&symbol, tf) {
        Some(mut old) => {
            old.extend(bars);
            old.sort_by_key(|b| b.time);
            // newer rows win on equal times
            let mut out: Vec<Bar> = Vec::with_capacity(old.len());
            for b in old {
                match out.last_mut() {
                    Some(l) if l.time == b.time => *l = b,
                    _ => out.push(b),
                }
            }
            out
        }
        None => bars,
    };
    save(&symbol, tf, &merged).map_err(|e| e.to_string())?;
    Ok(Imported {
        symbol,
        tf,
        bars: merged.len(),
        first: merged.first().map(|b| b.time).unwrap_or(0),
        last: merged.last().map(|b| b.time).unwrap_or(0),
    })
}

// ---------- symbol properties from a real terminal ----------

/// Properties written by the RustTerminal_Export script (symbols.csv).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SymbolInfo {
    pub name: String,
    pub description: String,
    pub digits: u32,
    pub contract_size: f64,
    pub volume_min: f64,
    pub volume_max: f64,
    pub volume_step: f64,
    pub spread: i32,
    pub currency_base: String,
    pub currency_profit: String,
    pub currency_margin: String,
    pub tick_value: f64,
    pub tick_size: f64,
    pub bid: f64,
}

pub fn symbols_file() -> PathBuf {
    history_dir().join("symbols.json")
}

pub fn load_symbols() -> Vec<SymbolInfo> {
    std::fs::read_to_string(symbols_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_symbols(list: &[SymbolInfo]) -> std::io::Result<()> {
    std::fs::create_dir_all(history_dir())?;
    std::fs::write(
        symbols_file(),
        serde_json::to_string_pretty(list).unwrap_or_default(),
    )
}

/// Parse symbols.csv: name;description;digits;contract;vmin;vmax;vstep;spread;base;profit;margin;tick_value;tick_size;bid
pub fn parse_symbols_csv(text: &str) -> Vec<SymbolInfo> {
    let mut out = vec![];
    for l in text.lines().skip(1) {
        let f: Vec<&str> = l.split(';').map(|x| x.trim()).collect();
        if f.len() < 14 {
            continue;
        }
        let n = |i: usize| f[i].replace(',', ".").parse::<f64>().unwrap_or(0.0);
        out.push(SymbolInfo {
            name: f[0].to_string(),
            description: f[1].to_string(),
            digits: n(2) as u32,
            contract_size: n(3),
            volume_min: n(4),
            volume_max: n(5),
            volume_step: n(6),
            spread: n(7) as i32,
            currency_base: f[8].to_string(),
            currency_profit: f[9].to_string(),
            currency_margin: f[10].to_string(),
            tick_value: n(11),
            tick_size: n(12),
            bid: n(13),
        });
    }
    out
}

// ---------- the real terminal's export folder ----------

/// `Common/Files/RustTerminal` folders of MetaTrader installations on this computer
/// (Windows, or Wine on macOS/Linux), where the export script writes.
pub fn terminal_export_dirs() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = vec![];
    if let Ok(appdata) = std::env::var("APPDATA") {
        roots.push(PathBuf::from(appdata).join("MetaQuotes/Terminal/Common/Files"));
    }
    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        let prefixes = [
            home.join("Library/Application Support/net.metaquotes.wine.metatrader5/drive_c/users"),
            home.join(".wine/drive_c/users"),
            home.join(".mt5/drive_c/users"),
        ];
        for p in prefixes {
            if let Ok(rd) = std::fs::read_dir(&p) {
                for u in rd.flatten() {
                    roots.push(
                        u.path()
                            .join("AppData/Roaming/MetaQuotes/Terminal/Common/Files"),
                    );
                }
            }
        }
    }
    roots
        .into_iter()
        .filter(|r| r.is_dir())
        .map(|r| r.join("RustTerminal"))
        .collect()
}

/// Import everything the export script wrote: `<SYMBOL>_<TF>.csv` bar files and
/// `symbols.csv`. Returns what was imported and any problems.
pub fn import_from_terminal() -> (Vec<Imported>, Vec<SymbolInfo>, Vec<String>) {
    let (mut done, mut syms, mut errors) = (vec![], vec![], vec![]);
    let dirs = terminal_export_dirs();
    if dirs.iter().all(|d| !d.is_dir()) {
        errors.push(
            "no export found: run the RustTerminal_Export script in MetaTrader 5 first".into(),
        );
        return (done, syms, errors);
    }
    for d in dirs.iter().filter(|d| d.is_dir()) {
        if let Ok(rd) = std::fs::read_dir(d) {
            let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
            files.sort();
            for p in files {
                let name = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                if name.eq_ignore_ascii_case("symbols.csv") {
                    if let Ok(b) = std::fs::read(&p) {
                        syms.extend(parse_symbols_csv(&crate::storage::decode_source(&b)));
                    }
                } else if name.to_ascii_lowercase().ends_with(".csv") {
                    match import_file(&p, None, None) {
                        Ok(i) => done.push(i),
                        Err(e) => errors.push(e),
                    }
                }
            }
        }
    }
    if !syms.is_empty() {
        let mut all = load_symbols();
        for s in &syms {
            all.retain(|x| x.name != s.name);
            all.push(s.clone());
        }
        all.sort_by(|a, b| a.name.cmp(&b.name));
        if let Err(e) = save_symbols(&all) {
            errors.push(e.to_string());
        }
    }
    (done, syms, errors)
}

/// The export script, ready to copy into MetaTrader 5.
pub const EXPORT_SCRIPT: &str = include_str!("../tools/mt5/RustTerminal_Export.mq5");

/// MQL5 data folders of MetaTrader 5 installations on this computer.
pub fn terminal_mql_dirs() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = vec![];
    let mut drive_roots: Vec<PathBuf> = vec![];
    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        drive_roots
            .push(home.join("Library/Application Support/net.metaquotes.wine.metatrader5/drive_c"));
        drive_roots.push(home.join(".wine/drive_c"));
        drive_roots.push(home.join(".mt5/drive_c"));
    }
    if cfg!(target_os = "windows") {
        drive_roots.push(PathBuf::from("C:\\"));
    }
    for d in drive_roots {
        out.push(d.join("Program Files/MetaTrader 5/MQL5"));
        if let Ok(rd) = std::fs::read_dir(d.join("users")) {
            for u in rd.flatten() {
                if let Ok(t) =
                    std::fs::read_dir(u.path().join("AppData/Roaming/MetaQuotes/Terminal"))
                {
                    for term in t.flatten() {
                        out.push(term.path().join("MQL5"));
                    }
                }
            }
        }
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        if let Ok(t) = std::fs::read_dir(PathBuf::from(appdata).join("MetaQuotes/Terminal")) {
            for term in t.flatten() {
                out.push(term.path().join("MQL5"));
            }
        }
    }
    out.into_iter()
        .filter(|p| p.join("Scripts").is_dir())
        .collect()
}

/// Copy the export script into every MetaTrader 5 found. Returns where it was written.
pub fn install_export_script() -> Result<Vec<PathBuf>, String> {
    let dirs = terminal_mql_dirs();
    if dirs.is_empty() {
        return Err("MetaTrader 5 was not found on this computer".into());
    }
    let mut done = vec![];
    for d in dirs {
        let p = d.join("Scripts").join("RustTerminal_Export.mq5");
        std::fs::write(&p, EXPORT_SCRIPT).map_err(|e| format!("{}: {}", p.display(), e))?;
        done.push(p);
    }
    Ok(done)
}
