//! More built-in functions: files, string/array helpers, trading calculations, global
//! variables, tester statistics, series info and the extra indicator handles.

use super::*;
use crate::indicators_ext::{self, ExtParams};
use std::path::PathBuf;

const FILE_READ: i64 = 1;
const FILE_WRITE: i64 = 2;
const FILE_BIN: i64 = 4;
const FILE_CSV: i64 = 8;
const FILE_TXT: i64 = 16;
const FILE_ANSI: i64 = 32;
const FILE_COMMON: i64 = 4096;

fn ai(a: &[Value], i: usize, d: i64) -> i64 {
    a.get(i).map(|v| v.as_i64()).unwrap_or(d)
}
fn af(a: &[Value], i: usize, d: f64) -> f64 {
    a.get(i).map(|v| v.as_f64()).unwrap_or(d)
}
fn as_(a: &[Value], i: usize) -> String {
    a.get(i).map(|v| v.to_text()).unwrap_or_default()
}

/// An open file. Text and CSV files are kept decoded in memory; binary files as bytes.
pub struct FileH {
    path: PathBuf,
    flags: i64,
    delim: char,
    text: Vec<char>,
    bin: Vec<u8>,
    pos: usize,
    dirty: bool,
    line_end: bool,
}

impl FileH {
    fn is_bin(&self) -> bool {
        self.flags & FILE_BIN != 0
    }
    fn is_csv(&self) -> bool {
        self.flags & FILE_CSV != 0
    }
    fn len(&self) -> usize {
        if self.is_bin() {
            self.bin.len()
        } else {
            self.text.len()
        }
    }
    fn write_text(&mut self, s: &str) -> usize {
        let mut n = 0;
        for ch in s.chars() {
            if self.pos < self.text.len() {
                self.text[self.pos] = ch;
            } else {
                self.text.push(ch);
            }
            self.pos += 1;
            n += 1;
        }
        self.dirty = true;
        n
    }
    fn write_bytes(&mut self, b: &[u8]) -> usize {
        for &x in b {
            if self.pos < self.bin.len() {
                self.bin[self.pos] = x;
            } else {
                self.bin.push(x);
            }
            self.pos += 1;
        }
        self.dirty = true;
        b.len()
    }
    fn read_bytes(&mut self, n: usize) -> Vec<u8> {
        let end = (self.pos + n).min(self.bin.len());
        let v = self.bin[self.pos..end].to_vec();
        self.pos = end;
        v
    }
    /// Read one CSV field or one text line.
    fn read_field(&mut self) -> String {
        let csv = self.is_csv();
        let mut out = String::new();
        self.line_end = false;
        if csv && self.text.get(self.pos) == Some(&'"') {
            self.pos += 1;
            while self.pos < self.text.len() {
                let c = self.text[self.pos];
                self.pos += 1;
                if c == '"' {
                    if self.text.get(self.pos) == Some(&'"') {
                        out.push('"');
                        self.pos += 1;
                        continue;
                    }
                    break;
                }
                out.push(c);
            }
        }
        while self.pos < self.text.len() {
            let c = self.text[self.pos];
            if c == '\r' || c == '\n' {
                if c == '\r' && self.text.get(self.pos + 1) == Some(&'\n') {
                    self.pos += 1;
                }
                self.pos += 1;
                self.line_end = true;
                break;
            }
            if csv && c == self.delim {
                self.pos += 1;
                break;
            }
            out.push(c);
            self.pos += 1;
        }
        if self.pos >= self.text.len() {
            self.line_end = true;
        }
        out
    }
    fn flush(&mut self) -> std::io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(d) = self.path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let bytes: Vec<u8> = if self.is_bin() {
            self.bin.clone()
        } else if self.flags & FILE_ANSI != 0 {
            self.text.iter().map(|&c| if (c as u32) < 256 { c as u8 } else { b'?' }).collect()
        } else {
            let mut b = vec![0xFF, 0xFE];
            let s: String = self.text.iter().collect();
            for u in s.encode_utf16() {
                b.extend_from_slice(&u.to_le_bytes());
            }
            b
        };
        std::fs::write(&self.path, bytes)?;
        self.dirty = false;
        Ok(())
    }
}

impl Drop for FileH {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

/// Parameters a function returns through reference arguments (by argument count).
pub(super) fn out_params(name: &str, argc: usize) -> Option<&'static [usize]> {
    Some(match (name, argc) {
        ("OrderCalcProfit", 6) => &[5],
        ("OrderCalcMargin", 5) => &[4],
        ("SymbolInfoString", 3) => &[2],
        ("SymbolExist", 2) => &[1],
        ("SymbolInfoSessionTrade" | "SymbolInfoSessionQuote", 5) => &[3, 4],
        ("GlobalVariableGet", 2) => &[1],
        ("PositionGetDouble" | "PositionGetInteger" | "PositionGetString", 2) => &[1],
        ("OrderGetDouble" | "OrderGetInteger" | "OrderGetString", 2) => &[1],
        ("HistoryDealGetDouble" | "HistoryDealGetInteger" | "HistoryDealGetString", 3) => &[2],
        ("HistoryOrderGetDouble" | "HistoryOrderGetInteger" | "HistoryOrderGetString", 3) => &[2],
        ("ChartGetInteger" | "ChartGetDouble", 4) => &[3],
        ("ChartGetString", 3) => &[2],
        ("ObjectGetInteger" | "ObjectGetDouble" | "ObjectGetString", 5) => &[4],
        ("ChartTimePriceToXY", 6) => &[4, 5],
        ("ChartXYToTimePrice", 6) => &[3, 4, 5],
        ("TimeCurrent" | "TimeLocal" | "TimeGMT" | "TimeTradeServer", 1) => &[0],
        ("IndicatorParameters", 3) => &[1],
        _ => return None,
    })
}

impl Instance {
    /// Functions with reference results: returns the function result and the values for
    /// the reference arguments (in the order `out_params` lists them).
    pub(super) fn call_out(&mut self, name: &str, a: &[Value], host: &mut dyn Host) -> R<(Value, Vec<(usize, Value)>)> {
        Ok(match name {
            "OrderCalcProfit" => {
                let typ = ai(a, 0, 0);
                let sym = as_(a, 1);
                match host.quote(&sym) {
                    Some(q) => {
                        let p = crate::account::profit_usd(&q.spec, typ % 2 == 0, af(a, 2, 0.0), af(a, 3, 0.0), af(a, 4, 0.0), q.bid);
                        (Value::Bool(true), vec![(0, Value::Double(p))])
                    }
                    None => (Value::Bool(false), vec![(0, Value::Double(0.0))]),
                }
            }
            "OrderCalcMargin" => {
                let sym = as_(a, 1);
                match host.quote(&sym) {
                    Some(q) => {
                        let m = crate::account::margin_usd(&q.spec, af(a, 2, 0.0), af(a, 3, 0.0), host.account().leverage);
                        (Value::Bool(true), vec![(0, Value::Double(m))])
                    }
                    None => (Value::Bool(false), vec![(0, Value::Double(0.0))]),
                }
            }
            "SymbolExist" => {
                let ok = host.symbol_names().iter().any(|s| *s == as_(a, 0));
                (Value::Bool(ok), vec![(0, Value::Bool(false))])
            }
            "SymbolInfoSessionTrade" | "SymbolInfoSessionQuote" => {
                if ai(a, 2, 0) == 0 {
                    (Value::Bool(true), vec![(0, Value::Int(0)), (1, Value::Int(86400))])
                } else {
                    (Value::Bool(false), vec![])
                }
            }
            "ChartTimePriceToXY" | "ChartXYToTimePrice" => (Value::Bool(false), vec![]),
            "TimeCurrent" | "TimeLocal" | "TimeGMT" | "TimeTradeServer" => {
                let t = self.call_builtin_values(name, &[], host)?;
                let st = self.new_struct(self.prog.interner.get("MqlDateTime").unwrap_or(0));
                self.call_builtin_values("TimeToStruct", &[t.clone(), st.clone()], host)?;
                (t, vec![(0, st)])
            }
            "IndicatorParameters" => (Value::Int(-1), vec![]),
            // getter overloads `F(..., &value)`: call the plain form and return its value
            _ => {
                let v = self.call_builtin_values(name, &a[..a.len() - 1], host)?;
                let ok = match name {
                    "GlobalVariableGet" => host.global_vars().contains_key(&as_(a, 0)),
                    _ => true,
                };
                (Value::Bool(ok), vec![(0, v)])
            }
        })
    }

    fn file_path(&self, host: &dyn Host, name: &str, flags: i64) -> Option<PathBuf> {
        let n = name.replace('\\', "/");
        if n.is_empty() || n.starts_with('/') || n.contains("..") || n.contains(':') {
            return None;
        }
        let root = if flags & FILE_COMMON != 0 { host.common_files_dir() } else { host.files_dir() };
        Some(root.join(n))
    }

    fn file(&mut self, h: i64) -> R<&mut FileH> {
        if self.files.contains_key(&h) {
            Ok(self.files.get_mut(&h).unwrap())
        } else {
            Err(self.err("invalid file handle"))
        }
    }

    /// Extra built-ins; `None` when the name is not one of them.
    pub(super) fn call_extra(&mut self, name: &str, a: &[Value], host: &mut dyn Host) -> R<Option<Value>> {
        let v = match name {
            "printf" => self.call_builtin_values("PrintFormat", a, host)?,
            "GetTickCount64" => self.call_builtin_values("GetTickCount", a, host)?,
            "OrderSendAsync" => self.call_builtin_values("OrderSend", a, host)?,
            "PeriodSeconds" => {
                let tf = match ai(a, 0, 0) {
                    0 => self.tf.mql_value(),
                    x => x,
                };
                Value::Int(period_seconds(tf))
            }
            // ---------- math ----------
            "MathArctan2" => Value::Double(af(a, 0, 0.0).atan2(af(a, 1, 0.0))),
            "MathSinh" => Value::Double(af(a, 0, 0.0).sinh()),
            "MathCosh" => Value::Double(af(a, 0, 0.0).cosh()),
            "MathTanh" => Value::Double(af(a, 0, 0.0).tanh()),
            "MathArcsinh" => Value::Double(af(a, 0, 0.0).asinh()),
            "MathArccosh" => Value::Double(af(a, 0, 0.0).acosh()),
            "MathArctanh" => Value::Double(af(a, 0, 0.0).atanh()),
            "MathLog1p" => Value::Double(af(a, 0, 0.0).ln_1p()),
            "MathExpm1" => Value::Double(af(a, 0, 0.0).exp_m1()),
            // ---------- strings ----------
            "StringBufferLen" => Value::Int(as_(a, 0).chars().count() as i64),
            "StringReserve" => Value::Bool(true),
            "StringToCharArray" | "StringToShortArray" => {
                let s = as_(a, 0);
                let arr = self.arr_of(a.get(1).unwrap_or(&Value::Void))?;
                let start = ai(a, 2, 0).max(0) as usize;
                let count = ai(a, 3, -1);
                let utf8 = name == "StringToCharArray" && ai(a, 4, 0) == 65001;
                let mut codes: Vec<i64> = if name == "StringToShortArray" {
                    s.encode_utf16().map(|u| u as i64).collect()
                } else if utf8 {
                    s.bytes().map(|b| b as i64).collect()
                } else {
                    s.chars().map(|c| if (c as u32) < 256 { c as i64 } else { '?' as i64 }).collect()
                };
                codes.push(0);
                let n = if count < 0 { codes.len() } else { (count as usize).min(codes.len()) };
                let mut ab = arr.borrow_mut();
                if ab.data.len() < start + n {
                    ab.data.resize(start + n, Value::Int(0));
                }
                for (i, c) in codes.into_iter().take(n).enumerate() {
                    ab.data[start + i] = Value::Int(c);
                }
                Value::Int(n as i64)
            }
            "CharArrayToString" | "ShortArrayToString" => {
                let arr = self.arr_of(a.first().unwrap_or(&Value::Void))?;
                let start = ai(a, 1, 0).max(0) as usize;
                let count = ai(a, 2, -1);
                let utf8 = name == "CharArrayToString" && ai(a, 3, 0) == 65001;
                let ab = arr.borrow();
                let end = if count < 0 { ab.data.len() } else { (start + count as usize).min(ab.data.len()) };
                let codes: Vec<i64> = ab.data.get(start..end).unwrap_or(&[]).iter().map(|v| v.as_i64()).take_while(|c| *c != 0).collect();
                let s = if name == "ShortArrayToString" {
                    String::from_utf16_lossy(&codes.iter().map(|c| *c as u16).collect::<Vec<_>>())
                } else if utf8 {
                    String::from_utf8_lossy(&codes.iter().map(|c| *c as u8).collect::<Vec<_>>()).to_string()
                } else {
                    codes.iter().map(|c| (*c as u8) as char).collect()
                };
                Value::str(&s)
            }
            "ColorToARGB" => {
                let c = ai(a, 0, 0) as u32;
                let alpha = ai(a, 1, 255) as u32 & 0xff;
                let (r, g, b) = (c & 0xff, (c >> 8) & 0xff, (c >> 16) & 0xff);
                Value::Int(((alpha << 24) | (r << 16) | (g << 8) | b) as i64)
            }
            "StringToColor" => {
                let s = as_(a, 0);
                let t = s.trim();
                let parts: Vec<&str> = t.split(',').collect();
                if parts.len() == 3 {
                    let n: Vec<i64> = parts.iter().map(|p| p.trim().parse::<i64>().unwrap_or(0).clamp(0, 255)).collect();
                    Value::Int(n[0] | (n[1] << 8) | (n[2] << 16))
                } else {
                    let key = if t.starts_with("clr") { t.to_string() } else { format!("clr{}", t) };
                    Value::Int(builtins::constant(&key).and_then(|c| c.as_i64()).unwrap_or(0))
                }
            }
            // ---------- arrays ----------
            "ArrayRemove" => {
                let arr = self.arr_of(a.first().unwrap_or(&Value::Void))?;
                let start = ai(a, 1, 0).max(0) as usize;
                let count = ai(a, 2, -1);
                let mut ab = arr.borrow_mut();
                let len = ab.data.len();
                if start >= len {
                    Value::Bool(false)
                } else {
                    let end = if count < 0 { len } else { (start + count as usize).min(len) };
                    if ab.series {
                        let (s2, e2) = (len - end, len - start);
                        ab.data.drain(s2..e2);
                    } else {
                        ab.data.drain(start..end);
                    }
                    Value::Bool(true)
                }
            }
            "ArrayInsert" => {
                let dst = self.arr_of(a.first().unwrap_or(&Value::Void))?;
                let src = self.arr_of(a.get(1).unwrap_or(&Value::Void))?;
                let dst_start = ai(a, 2, 0).max(0) as usize;
                let src_start = ai(a, 3, 0).max(0) as usize;
                let count = ai(a, 4, -1);
                let items: Vec<Value> = {
                    let sb = src.borrow();
                    let end = if count < 0 { sb.data.len() } else { (src_start + count as usize).min(sb.data.len()) };
                    sb.data.get(src_start..end).unwrap_or(&[]).to_vec()
                };
                let mut db = dst.borrow_mut();
                let at = dst_start.min(db.data.len());
                for (k, v) in items.into_iter().enumerate() {
                    db.data.insert(at + k, v);
                }
                Value::Bool(true)
            }
            "ArraySwap" => {
                let x = self.arr_of(a.first().unwrap_or(&Value::Void))?;
                let y = self.arr_of(a.get(1).unwrap_or(&Value::Void))?;
                if !Rc::ptr_eq(&x, &y) {
                    std::mem::swap(&mut x.borrow_mut().data, &mut y.borrow_mut().data);
                }
                Value::Bool(true)
            }
            "ArrayCompare" => {
                let x = self.arr_of(a.first().unwrap_or(&Value::Void))?;
                let y = self.arr_of(a.get(1).unwrap_or(&Value::Void))?;
                let (xb, yb) = (x.borrow(), y.borrow());
                let mut r = 0;
                for (p, q) in xb.data.iter().zip(yb.data.iter()) {
                    let c = match (p, q) {
                        (Value::Str(s1), Value::Str(s2)) => s1.cmp(s2),
                        _ => p.as_f64().partial_cmp(&q.as_f64()).unwrap_or(std::cmp::Ordering::Equal),
                    };
                    if c != std::cmp::Ordering::Equal {
                        r = if c == std::cmp::Ordering::Less { -1 } else { 1 };
                        break;
                    }
                }
                if r == 0 && xb.data.len() != yb.data.len() {
                    r = if xb.data.len() < yb.data.len() { -1 } else { 1 };
                }
                Value::Int(r)
            }
            "ArrayBsearch" => {
                let arr = self.arr_of(a.first().unwrap_or(&Value::Void))?;
                let x = af(a, 1, 0.0);
                let ab = arr.borrow();
                let n = ab.data.len();
                if n == 0 {
                    Value::Int(-1)
                } else {
                    let idx = ab.data.partition_point(|v| v.as_f64() < x);
                    let best = if idx >= n {
                        n - 1
                    } else if idx > 0 && (ab.data[idx].as_f64() - x).abs() > (x - ab.data[idx - 1].as_f64()).abs() {
                        idx - 1
                    } else {
                        idx
                    };
                    Value::Int(if ab.series { (n - 1 - best) as i64 } else { best as i64 })
                }
            }
            "ArrayPrint" => {
                let arr = self.arr_of(a.first().unwrap_or(&Value::Void))?;
                let digits = ai(a, 1, -1);
                let text: Vec<String> = {
                    let ab = arr.borrow();
                    let mut items: Vec<Value> = ab.data.clone();
                    if ab.series {
                        items.reverse();
                    }
                    items
                        .iter()
                        .map(|v| match v {
                            Value::Double(d) if digits >= 0 => format!("{:.*}", digits as usize, d),
                            other => other.to_text(),
                        })
                        .collect()
                };
                host.print(&text.join(" "));
                Value::Void
            }
            // ---------- time ----------
            "TimeDaylightSavings" | "TimeGMTOffset" => Value::Int(0),
            // ---------- series ----------
            "SeriesInfoInteger" => {
                let sym = self.sym_arg(a.first());
                let tf = self.tf_arg(a.get(1));
                let prop = ai(a, 2, 0);
                let info = self.with_bars(host, &sym, tf, |b| (b.len() as i64, b.first().map(|x| x.time).unwrap_or(0), b.last().map(|x| x.time).unwrap_or(0)));
                match info {
                    Some((n, first, last)) => Value::Int(match prop {
                        0 => n,
                        1..=3 => first,
                        4 => 1,
                        5 => last,
                        _ => 0,
                    }),
                    None => Value::Int(0),
                }
            }
            "CopySpread" => self.copy_series(host, a, |b| Value::Int(b.spread as i64))?,
            "CopyRealVolume" => self.copy_series(host, a, |_| Value::Int(0))?,
            "iSpread" => Value::Int(self.bar_at(host, a).map(|b| b.spread as i64).unwrap_or(0)),
            "iRealVolume" => Value::Int(0),
            "iTickVolume" => Value::Int(self.bar_at(host, a).map(|b| b.tick_volume).unwrap_or(0)),
            "CopyTicks" | "CopyTicksRange" => {
                // ticks are reconstructed from bar closes: one tick per bar
                let sym = self.sym_arg(a.first());
                let arr = self.arr_of(a.get(1).unwrap_or(&Value::Void))?;
                let count = if name == "CopyTicks" { ai(a, 4, 2000).max(1) as usize } else { 100_000 };
                let spec_point = host.quote(&sym).map(|q| q.spec.point()).unwrap_or(0.00001);
                let tick_sym = self.prog.interner.get("MqlTick");
                let bars: Vec<crate::market::Bar> = host.bars(&sym, Timeframe::M1).map(|b| b.to_vec()).unwrap_or_default();
                let take = bars.len().saturating_sub(count);
                let mut out = vec![];
                for b in &bars[take..] {
                    if let Some(ts) = tick_sym {
                        let t = self.new_struct(ts);
                        self.struct_set(&t, "time", Value::Int(b.time));
                        self.struct_set(&t, "time_msc", Value::Int(b.time * 1000));
                        self.struct_set(&t, "bid", Value::Double(b.close));
                        self.struct_set(&t, "ask", Value::Double(b.close + b.spread as f64 * spec_point));
                        self.struct_set(&t, "last", Value::Double(b.close));
                        self.struct_set(&t, "volume", Value::Int(b.tick_volume));
                        out.push(t);
                    }
                }
                let n = out.len() as i64;
                arr.borrow_mut().data = out;
                Value::Int(n)
            }
            "SymbolIsSynchronized" => Value::Bool(true),
            // ---------- terminal / chart ----------
            "MessageBox" => {
                host.alert(&format!("{} {}", as_(a, 1), as_(a, 0)).trim().to_string());
                Value::Int(1)
            }
            "DebugBreak" | "ChartRedraw_" => Value::Void,
            "SetUserError" => {
                self.last_error = 65536 + ai(a, 0, 0);
                Value::Void
            }
            "TerminalInfoDouble" => Value::Double(0.0),
            "ChartSetString" | "ChartSetDouble" | "ChartNavigate" | "ChartIndicatorAdd" | "ChartIndicatorDelete" | "ChartApplyTemplate"
            | "ChartSaveTemplate" | "ChartScreenShot" | "ChartSetSymbolPeriod" | "EventChartCustom" | "ChartClose" => Value::Bool(true),
            "ChartGetString" => Value::str(""),
            "ChartWindowFind" | "ChartIndicatorsTotal" | "WindowsTotal" => Value::Int(0),
            "ChartIndicatorName" => Value::str(""),
            "ChartIndicatorGet" => Value::Int(-1),
            "ChartOpen" | "ChartFirst" => Value::Int(0),
            "ChartNext" => Value::Int(-1),
            "ResourceCreate" | "ResourceSave" | "ResourceReadImage" => Value::Bool(false),
            "ResourceFree" => Value::Bool(true),
            "WebRequest" => {
                host.print("WebRequest is not allowed in this terminal");
                self.last_error = 4014;
                Value::Int(-1)
            }
            // ---------- history ----------
            "HistorySelectByPosition" | "HistoryOrderSelect" => Value::Bool(true),
            "HistoryDealSelect" => Value::Bool(host.account().deals.iter().any(|d| d.ticket as i64 == ai(a, 0, 0))),
            "HistoryOrderGetTicket" => {
                let orders: Vec<i64> = host.account().deals.iter().filter(|d| d.order != 0).map(|d| d.order as i64).collect();
                Value::Int(orders.get(ai(a, 0, 0).max(0) as usize).copied().unwrap_or(0))
            }
            "HistoryOrderGetDouble" | "HistoryOrderGetInteger" | "HistoryOrderGetString" => {
                let ticket = ai(a, 0, 0) as u64;
                let deal = host.account().deals.iter().find(|d| d.order == ticket).map(|d| d.ticket as i64);
                let dname = name.replace("HistoryOrder", "HistoryDeal");
                // order properties share numbers with the matching deal properties where it matters
                match deal {
                    Some(t) => self.call_builtin_values(&dname, &[Value::Int(t), a.get(1).cloned().unwrap_or(Value::Int(0))], host)?,
                    None => match name {
                        "HistoryOrderGetString" => Value::str(""),
                        "HistoryOrderGetDouble" => Value::Double(0.0),
                        _ => Value::Int(0),
                    },
                }
            }
            // ---------- global variables ----------
            "GlobalVariableTime" => Value::Int(if host.global_vars().contains_key(&as_(a, 0)) { host.now() } else { 0 }),
            "GlobalVariablesTotal" => Value::Int(host.global_vars().len() as i64),
            "GlobalVariableName" => {
                let mut keys: Vec<String> = host.global_vars().keys().cloned().collect();
                keys.sort();
                Value::str(keys.get(ai(a, 0, 0).max(0) as usize).map(|s| s.as_str()).unwrap_or(""))
            }
            "GlobalVariablesDeleteAll" => {
                let prefix = as_(a, 0);
                let gv = host.global_vars();
                let before = gv.len();
                gv.retain(|k, _| !k.starts_with(&prefix));
                Value::Int((before - gv.len()) as i64)
            }
            "GlobalVariableTemp" => {
                host.global_vars().entry(as_(a, 0)).or_insert(0.0);
                Value::Bool(true)
            }
            "GlobalVariableSetOnCondition" => {
                let (n, v, check) = (as_(a, 0), af(a, 1, 0.0), af(a, 2, 0.0));
                let gv = host.global_vars();
                match gv.get(&n) {
                    Some(cur) if *cur == check => {
                        gv.insert(n, v);
                        Value::Bool(true)
                    }
                    _ => Value::Bool(false),
                }
            }
            "GlobalVariablesFlush" => Value::Void,
            // ---------- tester ----------
            "TesterStatistics" => Value::Double(tester_stat(host.account(), ai(a, 0, 0))),
            "TesterStop" | "TesterHideIndicators" => Value::Void,
            "TesterWithdrawal" | "TesterDeposit" => Value::Bool(true),
            // ---------- indicators ----------
            "IndicatorCreate" => self.indicator_create(a, host)?,
            n if indicators_ext::by_function(n).is_some() => {
                let kind = indicators_ext::by_function(n).unwrap();
                let symbol = self.sym_arg(a.first());
                let tf = self.tf_arg(a.get(1));
                let params = ExtParams { i: a.iter().skip(2).map(|v| v.as_i64()).collect(), f: a.iter().skip(2).map(|v| v.as_f64()).collect() };
                self.add_handle(Handle::Ext { kind, params, symbol, tf })
            }
            // ---------- files ----------
            "FileOpen" => {
                let fname = as_(a, 0);
                let mut flags = ai(a, 1, FILE_READ | FILE_CSV);
                if flags & (FILE_BIN | FILE_CSV | FILE_TXT) == 0 {
                    flags |= FILE_CSV;
                }
                let delim = a.get(2).map(|v| match v {
                    Value::Str(s) => s.chars().next().unwrap_or('\t'),
                    other => char::from_u32(other.as_i64() as u32).unwrap_or('\t'),
                });
                let delim = delim.unwrap_or('\t');
                let path = match self.file_path(host, &fname, flags) {
                    Some(p) => p,
                    None => {
                        self.last_error = 5002;
                        return Ok(Some(Value::Int(-1)));
                    }
                };
                if flags & (FILE_READ | FILE_WRITE) == 0 {
                    self.last_error = 5004;
                    return Ok(Some(Value::Int(-1)));
                }
                let exists = path.exists();
                if flags & FILE_READ != 0 && flags & FILE_WRITE == 0 && !exists {
                    self.last_error = 5004;
                    return Ok(Some(Value::Int(-1)));
                }
                let raw = if flags & FILE_READ != 0 && exists { std::fs::read(&path).unwrap_or_default() } else { vec![] };
                let mut fh = FileH { path, flags, delim, text: vec![], bin: vec![], pos: 0, dirty: flags & FILE_READ == 0 || !exists, line_end: false };
                if flags & FILE_BIN != 0 {
                    fh.bin = raw;
                } else {
                    fh.text = crate::storage::decode_source(&raw).chars().collect();
                }
                let h = self.next_file;
                self.next_file += 1;
                self.files.insert(h, fh);
                Value::Int(h)
            }
            "FileClose" => {
                if let Some(mut f) = self.files.remove(&ai(a, 0, -1)) {
                    let _ = f.flush();
                }
                Value::Void
            }
            "FileFlush" => {
                let f = self.file(ai(a, 0, -1))?;
                let _ = f.flush();
                Value::Void
            }
            "FileWrite" => {
                let f = self.file(ai(a, 0, -1))?;
                let parts: Vec<String> = a.iter().skip(1).map(|v| v.to_text()).collect();
                let sep = if f.is_csv() { f.delim.to_string() } else { String::new() };
                let line = parts.join(&sep) + "\r\n";
                let n = if f.is_bin() { f.write_bytes(line.as_bytes()) } else { f.write_text(&line) };
                Value::Int(n as i64)
            }
            "FileWriteString" => {
                let f = self.file(ai(a, 0, -1))?;
                let mut s = as_(a, 1);
                if let Some(n) = a.get(2) {
                    s = s.chars().take(n.as_i64().max(0) as usize).collect();
                }
                let n = if f.is_bin() {
                    let mut b = vec![];
                    if f.flags & FILE_ANSI != 0 {
                        b.extend(s.chars().map(|c| c as u32 as u8));
                    } else {
                        for u in s.encode_utf16() {
                            b.extend_from_slice(&u.to_le_bytes());
                        }
                    }
                    f.write_bytes(&b);
                    s.chars().count()
                } else {
                    f.write_text(&s)
                };
                Value::Int(n as i64)
            }
            "FileReadString" => {
                let f = self.file(ai(a, 0, -1))?;
                if f.is_bin() {
                    let n = ai(a, 1, 0).max(0) as usize;
                    if f.flags & FILE_ANSI != 0 {
                        let b = f.read_bytes(n);
                        Value::str(&b.iter().map(|&c| c as char).collect::<String>())
                    } else {
                        let b = f.read_bytes(n * 2);
                        let u: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                        Value::str(&String::from_utf16_lossy(&u))
                    }
                } else {
                    Value::str(&f.read_field())
                }
            }
            "FileReadNumber" | "FileReadBool" | "FileReadDatetime" => {
                let f = self.file(ai(a, 0, -1))?;
                let s = f.read_field();
                let t = s.trim();
                match name {
                    "FileReadNumber" => Value::Double(t.parse().unwrap_or(0.0)),
                    "FileReadBool" => Value::Bool(t.eq_ignore_ascii_case("true") || t.parse::<f64>().map(|x| x != 0.0).unwrap_or(false)),
                    _ => Value::Int(crate::mql::lexer::parse_datetime(t).unwrap_or(0)),
                }
            }
            "FileWriteDouble" | "FileWriteFloat" | "FileWriteInteger" | "FileWriteLong" => {
                let f = self.file(ai(a, 0, -1))?;
                let bytes: Vec<u8> = match name {
                    "FileWriteDouble" => af(a, 1, 0.0).to_le_bytes().to_vec(),
                    "FileWriteFloat" => (af(a, 1, 0.0) as f32).to_le_bytes().to_vec(),
                    "FileWriteLong" => ai(a, 1, 0).to_le_bytes().to_vec(),
                    _ => {
                        let size = ai(a, 2, 4).clamp(1, 8) as usize;
                        ai(a, 1, 0).to_le_bytes()[..size].to_vec()
                    }
                };
                Value::Int(f.write_bytes(&bytes) as i64)
            }
            "FileReadDouble" | "FileReadFloat" | "FileReadInteger" | "FileReadLong" => {
                let f = self.file(ai(a, 0, -1))?;
                match name {
                    "FileReadDouble" => {
                        let b = f.read_bytes(8);
                        Value::Double(if b.len() == 8 { f64::from_le_bytes(b.try_into().unwrap()) } else { 0.0 })
                    }
                    "FileReadFloat" => {
                        let b = f.read_bytes(4);
                        Value::Double(if b.len() == 4 { f32::from_le_bytes(b.try_into().unwrap()) as f64 } else { 0.0 })
                    }
                    "FileReadLong" => {
                        let b = f.read_bytes(8);
                        Value::Int(if b.len() == 8 { i64::from_le_bytes(b.try_into().unwrap()) } else { 0 })
                    }
                    _ => {
                        let size = ai(a, 1, 4).clamp(1, 8) as usize;
                        let b = f.read_bytes(size);
                        let mut full = [0u8; 8];
                        full[..b.len()].copy_from_slice(&b);
                        let mut v = i64::from_le_bytes(full);
                        // sign-extend like the platform's signed CHAR/SHORT/INT reads
                        if b.len() < 8 && b.len() == size {
                            let shift = 64 - 8 * size as u32;
                            v = (v << shift) >> shift;
                        }
                        Value::Int(v)
                    }
                }
            }
            "FileWriteArray" | "FileReadArray" => {
                let h = ai(a, 0, -1);
                let arr = self.arr_of(a.get(1).unwrap_or(&Value::Void))?;
                let start = ai(a, 2, 0).max(0) as usize;
                let count = ai(a, 3, -1);
                let elem = arr.borrow().elem.clone();
                let size = if elem == Ty::Double { 8 } else { 4 };
                let f = self.file(h)?;
                if name == "FileWriteArray" {
                    let ab = arr.borrow();
                    let end = if count < 0 { ab.data.len() } else { (start + count as usize).min(ab.data.len()) };
                    let mut n = 0;
                    for v in ab.data.get(start..end).unwrap_or(&[]) {
                        if size == 8 {
                            f.write_bytes(&v.as_f64().to_le_bytes());
                        } else {
                            f.write_bytes(&(v.as_i64() as i32).to_le_bytes());
                        }
                        n += 1;
                    }
                    Value::Int(n)
                } else {
                    let avail = (f.bin.len() - f.pos.min(f.bin.len())) / size;
                    let n = if count < 0 { avail } else { (count as usize).min(avail) };
                    let mut vals = vec![];
                    for _ in 0..n {
                        let b = f.read_bytes(size);
                        vals.push(if size == 8 {
                            Value::Double(f64::from_le_bytes(b.try_into().unwrap_or([0; 8])))
                        } else {
                            Value::Int(i32::from_le_bytes(b.try_into().unwrap_or([0; 4])) as i64)
                        });
                    }
                    let mut ab = arr.borrow_mut();
                    if ab.data.len() < start + n {
                        let d = if size == 8 { Value::Double(0.0) } else { Value::Int(0) };
                        ab.data.resize(start + n, d);
                    }
                    for (i, v) in vals.into_iter().enumerate() {
                        ab.data[start + i] = v;
                    }
                    Value::Int(n as i64)
                }
            }
            "FileIsEnding" => {
                let f = self.file(ai(a, 0, -1))?;
                Value::Bool(f.pos >= f.len())
            }
            "FileIsLineEnding" => {
                let f = self.file(ai(a, 0, -1))?;
                Value::Bool(f.line_end || f.pos >= f.len())
            }
            "FileSize" => {
                let f = self.file(ai(a, 0, -1))?;
                Value::Int(f.len() as i64)
            }
            "FileTell" => {
                let f = self.file(ai(a, 0, -1))?;
                Value::Int(f.pos as i64)
            }
            "FileSeek" => {
                let f = self.file(ai(a, 0, -1))?;
                let off = ai(a, 1, 0);
                let base = match ai(a, 2, 0) {
                    1 => f.pos as i64,
                    2 => f.len() as i64,
                    _ => 0,
                };
                f.pos = (base + off).clamp(0, f.len() as i64) as usize;
                Value::Bool(true)
            }
            "FileGetInteger" => {
                let prop = ai(a, 1, 0);
                match a.first() {
                    Some(Value::Str(n)) => {
                        let p = self.file_path(host, n, ai(a, 2, 0));
                        let meta = p.as_ref().and_then(|p| std::fs::metadata(p).ok());
                        Value::Int(match prop {
                            1 => meta.is_some() as i64,
                            5 => meta.map(|m| m.len() as i64).unwrap_or(0),
                            _ => 0,
                        })
                    }
                    other => {
                        let h = other.map(|v| v.as_i64()).unwrap_or(-1);
                        let f = self.file(h)?;
                        Value::Int(match prop {
                            1 => 1,
                            5 => f.len() as i64,
                            6 => f.pos as i64,
                            7 => (f.pos >= f.len()) as i64,
                            8 => f.line_end as i64,
                            9 => f.is_bin() as i64,
                            10 => f.is_csv() as i64,
                            11 => (f.flags & FILE_ANSI != 0) as i64,
                            12 => (f.flags & FILE_TXT != 0) as i64,
                            13 => (f.flags & FILE_READ != 0) as i64,
                            14 => (f.flags & FILE_WRITE != 0) as i64,
                            _ => 0,
                        })
                    }
                }
            }
            "FileIsExist" => Value::Bool(self.file_path(host, &as_(a, 0), ai(a, 1, 0)).is_some_and(|p| p.is_file())),
            "FileDelete" => Value::Bool(self.file_path(host, &as_(a, 0), ai(a, 1, 0)).is_some_and(|p| std::fs::remove_file(p).is_ok())),
            "FileMove" | "FileCopy" => {
                let src = self.file_path(host, &as_(a, 0), ai(a, 1, 0));
                let dst = self.file_path(host, &as_(a, 2), ai(a, 3, 0));
                match (src, dst) {
                    (Some(s), Some(d)) => {
                        if let Some(dir) = d.parent() {
                            let _ = std::fs::create_dir_all(dir);
                        }
                        let ok = if name == "FileMove" { std::fs::rename(&s, &d).is_ok() } else { std::fs::copy(&s, &d).is_ok() };
                        Value::Bool(ok)
                    }
                    _ => Value::Bool(false),
                }
            }
            "FolderCreate" => Value::Bool(self.file_path(host, &as_(a, 0), ai(a, 1, 0)).is_some_and(|p| std::fs::create_dir_all(p).is_ok())),
            "FolderDelete" => Value::Bool(self.file_path(host, &as_(a, 0), ai(a, 1, 0)).is_some_and(|p| std::fs::remove_dir(p).is_ok())),
            "FolderClean" => {
                let ok = self.file_path(host, &as_(a, 0), ai(a, 1, 0)).is_some_and(|p| {
                    std::fs::read_dir(&p)
                        .map(|rd| {
                            for e in rd.flatten() {
                                let q = e.path();
                                let _ = if q.is_dir() { std::fs::remove_dir_all(q) } else { std::fs::remove_file(q) };
                            }
                        })
                        .is_ok()
                });
                Value::Bool(ok)
            }
            _ => return Ok(None),
        };
        Ok(Some(v))
    }

    /// IndicatorCreate(symbol, period, type, count, params[]) for the built-in types.
    fn indicator_create(&mut self, a: &[Value], host: &mut dyn Host) -> R<Value> {
        let typ = ai(a, 2, -1);
        let params: Vec<Value> = match a.get(4) {
            Some(Value::Arr(arr)) => arr
                .borrow()
                .data
                .iter()
                .map(|p| {
                    let t = self.struct_get(p, "type").as_i64();
                    match t {
                        12 | 13 => self.struct_get(p, "double_value"),
                        14 => self.struct_get(p, "string_value"),
                        _ => self.struct_get(p, "integer_value"),
                    }
                })
                .collect(),
            _ => vec![],
        };
        let fname = match typ {
            0 => "iAC",
            1 => "iAD",
            2 => "iADX",
            4 => "iAlligator",
            5 => "iAMA",
            6 => "iAO",
            7 => "iATR",
            8 => "iBands",
            9 => "iBearsPower",
            10 => "iBullsPower",
            11 => "iBWMFI",
            12 => "iCCI",
            13 => "iChaikin",
            14 => "iCustom",
            15 => "iDEMA",
            16 => "iDeMarker",
            17 => "iEnvelopes",
            18 => "iForce",
            19 => "iFractals",
            20 => "iFrAMA",
            21 => "iGator",
            22 => "iIchimoku",
            23 => "iMA",
            24 => "iMACD",
            25 => "iMFI",
            26 => "iMomentum",
            27 => "iOBV",
            28 => "iOsMA",
            29 => "iRSI",
            30 => "iRVI",
            31 => "iSAR",
            32 => "iStdDev",
            33 => "iStochastic",
            34 => "iTEMA",
            35 => "iTriX",
            36 => "iVIDyA",
            37 => "iVolumes",
            38 => "iWPR",
            _ => return Ok(Value::Int(-1)),
        };
        let mut args = vec![a.first().cloned().unwrap_or(Value::str("")), a.get(1).cloned().unwrap_or(Value::Int(0))];
        args.extend(params);
        self.call_builtin_values(fname, &args, host)
    }
}

pub fn period_seconds(tf: i64) -> i64 {
    match tf {
        1..=30 => tf * 60,
        16385..=16396 => (tf - 16384) * 3600,
        16408 => 86400,
        32769 => 604800,
        49153 => 2592000,
        _ => 0,
    }
}

fn tester_stat(acc: &crate::account::Account, stat: i64) -> f64 {
    let outs: Vec<f64> = acc.deals.iter().filter(|d| d.entry == "out").map(|d| d.profit).collect();
    let deposits: f64 = acc.deals.iter().filter(|d| d.kind == "balance").map(|d| d.profit).sum();
    let profit: f64 = outs.iter().sum();
    let gp: f64 = outs.iter().filter(|p| **p > 0.0).sum();
    let gl: f64 = outs.iter().filter(|p| **p < 0.0).sum();
    let wins = outs.iter().filter(|p| **p > 0.0).count() as f64;
    let losses = outs.iter().filter(|p| **p < 0.0).count() as f64;
    match stat {
        0 => deposits,
        2 => profit,
        3 => gp,
        4 => gl,
        5 => outs.iter().cloned().fold(0.0, f64::max),
        6 => outs.iter().cloned().fold(0.0, f64::min),
        25 => {
            if outs.is_empty() {
                0.0
            } else {
                profit / outs.len() as f64
            }
        }
        26 => {
            if gl != 0.0 {
                gp / -gl
            } else {
                0.0
            }
        }
        31 => acc.deals.iter().filter(|d| d.kind != "balance").count() as f64,
        32 => outs.len() as f64,
        33 => wins,
        34 => losses,
        _ => 0.0,
    }
}
