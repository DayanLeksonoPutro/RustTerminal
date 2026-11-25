//! Built-in functions and object methods available to scripts.

use super::*;
use crate::account::{Account, Deal, PendingOrder, Position, RET_DONE};
use crate::indicators::{script_buffers, IndKind, IndParams};
use crate::market::{civil, Bar, Timeframe};
use crate::objects::{ChartObj, ObjKind};

const RET_AT_DISABLED: i64 = 10027;

fn arg_f(a: &[Value], i: usize, d: f64) -> f64 {
    a.get(i).map(|v| v.as_f64()).unwrap_or(d)
}
fn arg_i(a: &[Value], i: usize, d: i64) -> i64 {
    a.get(i).map(|v| v.as_i64()).unwrap_or(d)
}
fn arg_s(a: &[Value], i: usize) -> String {
    a.get(i).map(|v| v.to_text()).unwrap_or_default()
}
fn vd(v: f64) -> Value {
    Value::Double(v)
}
fn vi(v: i64) -> Value {
    Value::Int(v)
}
fn vb(v: bool) -> Value {
    Value::Bool(v)
}

impl Instance {
    pub(super) fn eval_args(&mut self, args: &[Expr], host: &mut dyn Host) -> R<Vec<Value>> {
        args.iter().map(|a| self.eval(a, host)).collect()
    }

    pub(super) fn arr_of(&self, v: &Value) -> R<ArrRef> {
        match v {
            Value::Arr(a) => Ok(a.clone()),
            _ => Err(self.err("array expected")),
        }
    }

    pub(super) fn sym_arg(&self, v: Option<&Value>) -> String {
        match v {
            Some(Value::Str(s)) if !s.is_empty() => s.to_string(),
            _ => self.symbol.clone(),
        }
    }

    pub(super) fn tf_arg(&self, v: Option<&Value>) -> Timeframe {
        match v.map(|v| v.as_i64()) {
            Some(0) | None => self.tf,
            Some(x) => Timeframe::from_mql(x).unwrap_or(self.tf),
        }
    }

    pub(super) fn struct_get(&self, s: &Value, field: &str) -> Value {
        if let (Value::Struct(s), Some(sym)) = (s, self.prog.interner.get(field)) {
            if let Some((_, _, v)) = s.borrow().fields.iter().find(|(n, _, _)| *n == sym) {
                return v.clone();
            }
        }
        Value::Void
    }

    pub(super) fn struct_set(&self, s: &Value, field: &str, v: Value) {
        if let (Value::Struct(s), Some(sym)) = (s, self.prog.interner.get(field)) {
            let mut sb = s.borrow_mut();
            if let Some(f) = sb.fields.iter_mut().find(|(n, _, _)| *n == sym) {
                let t = f.1.clone();
                f.2 = match t {
                    Ty::Int => vi(v.as_i64()),
                    Ty::Double => vd(v.as_f64()),
                    Ty::Str => Value::str(&v.to_text()),
                    Ty::Bool => vb(v.truthy()),
                    _ => v,
                };
            }
        }
    }

    /// Store copied series data (oldest first). An indicator buffer keeps its size and
    /// receives the values at its newest end; a static array is filled from the start.
    pub(super) fn set_array_chrono(&self, arr: &ArrRef, data: Vec<Value>) {
        let is_buffer = self.buffers.iter().flatten().any(|(b, _)| Rc::ptr_eq(b, arr));
        let mut a = arr.borrow_mut();
        if is_buffer {
            let n = a.data.len();
            let k = data.len().min(n);
            let skip = data.len() - k;
            for (i, v) in data.into_iter().skip(skip).enumerate() {
                a.data[n - k + i] = v;
            }
        } else if !a.dynamic && a.data.len() >= data.len() {
            for (i, v) in data.into_iter().enumerate() {
                a.data[i] = v;
            }
        } else {
            a.data = data;
        }
    }

    fn do_trade(&mut self, host: &mut dyn Host, op: TradeOp) -> TradeResult {
        if !host.algo_allowed() {
            host.print("automated trading is disabled");
            self.last_error = 4109;
            return TradeResult { retcode: RET_AT_DISABLED, order: 0, deal: 0, price: 0.0, volume: 0.0, comment: "AutoTrading disabled by client".into() };
        }
        if self.prog.kind == ProgramKind::Indicator {
            self.last_error = 4109;
            return TradeResult { retcode: 10017, order: 0, deal: 0, price: 0.0, volume: 0.0, comment: "Trade is not allowed for indicators".into() };
        }
        let r = host.trade(op);
        if r.retcode != RET_DONE {
            self.last_error = 4756;
        }
        r
    }

    pub(super) fn position(&self, host: &dyn Host, ticket: Option<u64>) -> Option<Position> {
        let t = ticket?;
        host.account().positions.iter().find(|p| p.ticket == t).cloned()
    }
    pub(super) fn order(&self, host: &dyn Host, ticket: Option<u64>) -> Option<PendingOrder> {
        let t = ticket?;
        host.account().orders.iter().find(|o| o.ticket == t).cloned()
    }

    pub(super) fn with_bars<T>(&self, host: &mut dyn Host, sym: &str, tf: Timeframe, f: impl FnOnce(&[Bar]) -> T) -> Option<T> {
        host.bars(sym, tf).map(f)
    }

    /// Copy `count` elements starting `start` bars back from the newest, oldest first.
    pub(super) fn copy_series(&mut self, host: &mut dyn Host, a: &[Value], get: fn(&Bar) -> Value) -> R<Value> {
        let sym = self.sym_arg(a.first());
        let tf = self.tf_arg(a.get(1));
        let start = arg_i(a, 2, 0);
        let count = arg_i(a, 3, 0);
        let arr = self.arr_of(a.get(4).unwrap_or(&Value::Void))?;
        let data = self.with_bars(host, &sym, tf, |bars| {
            let n = bars.len() as i64;
            let (newest, cnt) = if start > 100_000_000 {
                let idx = bars.iter().rposition(|b| b.time <= start).map(|i| i as i64).unwrap_or(-1);
                (idx + count - 1, count)
            } else {
                (n - 1 - start, count)
            };
            let newest = newest.min(n - 1);
            let oldest = (newest - cnt + 1).max(0);
            if newest < 0 || oldest > newest {
                return vec![];
            }
            bars[oldest as usize..=newest as usize].iter().map(get).collect::<Vec<Value>>()
        });
        match data {
            Some(d) if !d.is_empty() => {
                let k = d.len() as i64;
                self.set_array_chrono(&arr, d);
                Ok(vi(k))
            }
            _ => {
                self.last_error = 4401;
                Ok(vi(-1))
            }
        }
    }

    pub(super) fn bar_at(&self, host: &mut dyn Host, a: &[Value]) -> Option<Bar> {
        let sym = self.sym_arg(a.first());
        let tf = self.tf_arg(a.get(1));
        let shift = arg_i(a, 2, 0);
        self.with_bars(host, &sym, tf, |b| {
            let n = b.len() as i64;
            let i = n - 1 - shift;
            if i >= 0 && i < n {
                Some(b[i as usize])
            } else {
                None
            }
        })
        .flatten()
    }

    pub(super) fn add_handle(&mut self, h: Handle) -> Value {
        self.handles.push(Some(h));
        vi(self.handles.len() as i64 - 1)
    }

    fn builtin_handle(&mut self, a: &[Value], kind: IndKind, params: IndParams) -> Value {
        let symbol = self.sym_arg(a.first());
        let tf = self.tf_arg(a.get(1));
        self.add_handle(Handle::Builtin { kind, params, symbol, tf })
    }

    fn handle_buffer(&mut self, host: &mut dyn Host, h: i64, buf: usize) -> R<Option<Vec<f64>>> {
        let idx = h as usize;
        if h < 0 || idx >= self.handles.len() {
            return Ok(None);
        }
        let mut handle = self.handles[idx].take();
        let result = match &mut handle {
            Some(Handle::Builtin { kind, params, symbol, tf }) => {
                let (k, p, s, t) = (*kind, *params, symbol.clone(), *tf);
                self.with_bars(host, &s, t, |bars| script_buffers(k, &p, bars))
                    .and_then(|mut v| if buf < v.len() { Some(v.swap_remove(buf)) } else { None })
            }
            Some(Handle::Ext { kind, params, symbol, tf }) => {
                let (k, p, s, t) = (*kind, params.clone(), symbol.clone(), *tf);
                self.with_bars(host, &s, t, |bars| crate::indicators_ext::compute(k, &p, bars))
                    .and_then(|mut v| if buf < v.len() { Some(v.swap_remove(buf)) } else { None })
            }
            Some(Handle::Custom { inst: Some(child), symbol, tf }) => {
                let bars: Option<Vec<Bar>> = host.bars(symbol, *tf).map(|b| b.to_vec());
                match bars {
                    Some(bars) => match child.calculate_on(&bars, host) {
                        Ok(()) => child
                            .buffers
                            .get(buf)
                            .and_then(|b| b.as_ref())
                            .map(|(arr, _)| arr.borrow().data.iter().map(|v| v.as_f64()).collect()),
                        Err(e) => {
                            let msg = child.fmt_err(&e);
                            host.print(&format!("{}: {}", child.name, msg));
                            None
                        }
                    },
                    None => None,
                }
            }
            _ => None,
        };
        self.handles[idx] = handle;
        Ok(result)
    }

    pub(super) fn call_builtin(&mut self, name: &str, args: &[Expr], host: &mut dyn Host) -> R<Value> {
        // functions that write back into a variable argument
        match name {
            "StringReplace" | "StringToUpper" | "StringToLower" | "StringTrimLeft" | "StringTrimRight" | "StringAdd" | "StringConcatenate" => {
                let target = args.first().ok_or_else(|| self.err("wrong parameters count"))?.clone();
                let a = self.eval_args(args, host)?;
                let s = arg_s(&a, 0);
                let (new, ret) = match name {
                    "StringReplace" => {
                        let (f, r) = (arg_s(&a, 1), arg_s(&a, 2));
                        if f.is_empty() {
                            (s.clone(), vi(0))
                        } else {
                            let c = s.matches(&f).count() as i64;
                            (s.replace(&f, &r), vi(c))
                        }
                    }
                    "StringToUpper" => (s.to_uppercase(), vb(true)),
                    "StringToLower" => (s.to_lowercase(), vb(true)),
                    "StringTrimLeft" => {
                        let t = s.trim_start().to_string();
                        let n = (s.len() - t.len()) as i64;
                        (t, vi(n))
                    }
                    "StringTrimRight" => {
                        let t = s.trim_end().to_string();
                        let n = (s.len() - t.len()) as i64;
                        (t, vi(n))
                    }
                    "StringAdd" => (s + &arg_s(&a, 1), vb(true)),
                    _ => {
                        let joined: String = a[1..].iter().map(|v| v.to_text()).collect();
                        let n = joined.chars().count() as i64;
                        (joined, vi(n))
                    }
                };
                self.assign(&target, Value::str(&new), host)?;
                return Ok(ret);
            }
            "StringSetCharacter" | "StringInit" | "StringFill" => {
                let target = args.first().ok_or_else(|| self.err("wrong parameters count"))?.clone();
                let a = self.eval_args(args, host)?;
                let s = arg_s(&a, 0);
                let (new, ret) = match name {
                    "StringSetCharacter" => {
                        let pos = arg_i(&a, 1, 0).max(0) as usize;
                        let ch = char::from_u32(arg_i(&a, 2, 0) as u32).unwrap_or('\0');
                        let mut cs: Vec<char> = s.chars().collect();
                        if pos < cs.len() {
                            cs[pos] = ch;
                        } else if pos == cs.len() {
                            cs.push(ch);
                        } else {
                            return Ok(vb(false));
                        }
                        (cs.into_iter().filter(|c| *c != '\0').collect::<String>(), vb(true))
                    }
                    "StringInit" => {
                        let n = arg_i(&a, 1, 0).max(0) as usize;
                        let ch = char::from_u32(arg_i(&a, 2, 0) as u32).unwrap_or('\0');
                        (if ch == '\0' { String::new() } else { std::iter::repeat(ch).take(n).collect() }, vb(true))
                    }
                    _ => {
                        let ch = char::from_u32(arg_i(&a, 1, 0) as u32).unwrap_or(' ');
                        (std::iter::repeat(ch).take(s.chars().count()).collect(), vb(true))
                    }
                };
                self.assign(&target, Value::str(&new), host)?;
                return Ok(ret);
            }
            _ if super::extra::out_params(name, args.len()).is_some() => {
                let outs = super::extra::out_params(name, args.len()).unwrap();
                let a = self.eval_args(args, host)?;
                let (ret, vals) = self.call_out(name, &a, host)?;
                for (k, v) in vals {
                    if let Some(i) = outs.get(k) {
                        if let Some(e) = args.get(*i) {
                            self.assign(e, v, host)?;
                        }
                    }
                }
                return Ok(ret);
            }
            "SymbolInfoDouble" | "SymbolInfoInteger" if args.len() == 3 => {
                let a = self.eval_args(&args[..2], host)?;
                let v = self.call_builtin_values(name, &a, host)?;
                let ok = !matches!(v, Value::Void);
                self.assign(&args[2], v, host)?;
                return Ok(vb(ok));
            }
            _ => {}
        }
        let a = self.eval_args(args, host)?;
        self.call_builtin_values(name, &a, host)
    }

    pub(super) fn call_builtin_values(&mut self, name: &str, a: &[Value], host: &mut dyn Host) -> R<Value> {
        Ok(match name {
            "__comma" => a.last().cloned().unwrap_or(Value::Void),
            // ---------- output ----------
            "Print" => {
                let s: String = a.iter().map(|v| v.to_text()).collect();
                host.print(&s);
                Value::Void
            }
            "PrintFormat" => {
                let s = printf(&arg_s(a, 0), &a[1.min(a.len())..]);
                host.print(&s);
                Value::Void
            }
            "StringFormat" => Value::str(&printf(&arg_s(a, 0), &a[1.min(a.len())..])),
            "Comment" => {
                let s: String = a.iter().map(|v| v.to_text()).collect();
                host.set_comment(&s);
                Value::Void
            }
            "Alert" => {
                let s: String = a.iter().map(|v| v.to_text()).collect();
                host.alert(&s);
                Value::Void
            }
            "PlaySound" => vb(true),
            "SendNotification" | "SendMail" => vb(false),
            // ---------- conversion ----------
            "DoubleToString" => {
                let d = arg_i(a, 1, 8).clamp(0, 16) as usize;
                Value::str(&format!("{:.*}", d, arg_f(a, 0, 0.0)))
            }
            "IntegerToString" => {
                let v = arg_i(a, 0, 0).to_string();
                let len = arg_i(a, 1, 0).max(0) as usize;
                let fill = a.get(2).map(|f| char::from_u32(f.as_i64() as u32).unwrap_or(' ')).unwrap_or(' ');
                let mut s = String::new();
                while s.chars().count() + v.len() < len {
                    s.push(fill);
                }
                Value::str(&(s + &v))
            }
            "StringToDouble" => vd(arg_s(a, 0).trim().parse().unwrap_or(0.0)),
            "StringToInteger" => vi(arg_s(a, 0).trim().parse::<i64>().unwrap_or(0)),
            "TimeToString" => {
                let t = arg_i(a, 0, 0);
                let flags = arg_i(a, 1, 3);
                let (y, mo, d, h, mi, s) = civil(t);
                let mut out = String::new();
                if flags & 1 != 0 {
                    out.push_str(&format!("{:04}.{:02}.{:02}", y, mo, d));
                }
                if flags & 6 != 0 {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(&format!("{:02}:{:02}", h, mi));
                    if flags & 4 != 0 {
                        out.push_str(&format!(":{:02}", s));
                    }
                }
                Value::str(&out)
            }
            "StringToTime" => vi(crate::mql::lexer::parse_datetime(&arg_s(a, 0)).unwrap_or(0)),
            "CharToString" | "ShortToString" => Value::str(&char::from_u32(arg_i(a, 0, 0) as u32).map(|c| c.to_string()).unwrap_or_default()),
            "ColorToString" => {
                let c = arg_i(a, 0, 0);
                Value::str(&format!("{},{},{}", c & 0xff, (c >> 8) & 0xff, (c >> 16) & 0xff))
            }
            "NormalizeDouble" => {
                let m = 10f64.powi(arg_i(a, 1, 0).clamp(0, 16) as i32);
                vd((arg_f(a, 0, 0.0) * m).round() / m)
            }
            "EnumToString" => Value::str(&arg_i(a, 0, 0).to_string()),
            // ---------- math ----------
            "MathAbs" | "fabs" => match a.first() {
                Some(Value::Int(i)) => vi(i.abs()),
                _ => vd(arg_f(a, 0, 0.0).abs()),
            },
            "MathMax" | "fmax" => match (a.first(), a.get(1)) {
                (Some(Value::Int(x)), Some(Value::Int(y))) => vi(*x.max(y)),
                _ => vd(arg_f(a, 0, 0.0).max(arg_f(a, 1, 0.0))),
            },
            "MathMin" | "fmin" => match (a.first(), a.get(1)) {
                (Some(Value::Int(x)), Some(Value::Int(y))) => vi(*x.min(y)),
                _ => vd(arg_f(a, 0, 0.0).min(arg_f(a, 1, 0.0))),
            },
            "MathPow" | "pow" => vd(arg_f(a, 0, 0.0).powf(arg_f(a, 1, 0.0))),
            "MathSqrt" | "sqrt" => vd(arg_f(a, 0, 0.0).sqrt()),
            "MathFloor" | "floor" => vd(arg_f(a, 0, 0.0).floor()),
            "MathCeil" | "ceil" => vd(arg_f(a, 0, 0.0).ceil()),
            "MathRound" | "round" => vd(arg_f(a, 0, 0.0).round()),
            "MathLog" | "log" => vd(arg_f(a, 0, 0.0).ln()),
            "MathLog10" | "log10" => vd(arg_f(a, 0, 0.0).log10()),
            "MathExp" | "exp" => vd(arg_f(a, 0, 0.0).exp()),
            "MathSin" | "sin" => vd(arg_f(a, 0, 0.0).sin()),
            "MathCos" | "cos" => vd(arg_f(a, 0, 0.0).cos()),
            "MathTan" | "tan" => vd(arg_f(a, 0, 0.0).tan()),
            "MathArctan" | "atan" => vd(arg_f(a, 0, 0.0).atan()),
            "MathArcsin" | "asin" => vd(arg_f(a, 0, 0.0).asin()),
            "MathArccos" | "acos" => vd(arg_f(a, 0, 0.0).acos()),
            "MathMod" | "fmod" => vd(arg_f(a, 0, 0.0) % arg_f(a, 1, 1.0)),
            "MathSign" => vd(arg_f(a, 0, 0.0).signum()),
            "MathRand" | "rand" => vi(self.next_rand()),
            "MathSrand" | "srand" => {
                self.seed_rand(arg_i(a, 0, 1) as u64);
                Value::Void
            }
            "MathIsValidNumber" => vb(arg_f(a, 0, 0.0).is_finite()),
            // ---------- strings ----------
            "StringLen" => vi(arg_s(a, 0).chars().count() as i64),
            "StringSubstr" => {
                let s: Vec<char> = arg_s(a, 0).chars().collect();
                let st = arg_i(a, 1, 0).max(0) as usize;
                let len = arg_i(a, 2, -1);
                let end = if len < 0 { s.len() } else { (st + len as usize).min(s.len()) };
                Value::str(&if st < s.len() { s[st..end].iter().collect::<String>() } else { String::new() })
            }
            "StringFind" => {
                let s = arg_s(a, 0);
                let f = arg_s(a, 1);
                let st = arg_i(a, 2, 0).max(0) as usize;
                let tail: String = s.chars().skip(st).collect();
                vi(tail.find(&f).map(|p| (tail[..p].chars().count() + st) as i64).unwrap_or(-1))
            }
            "StringSplit" => {
                let s = arg_s(a, 0);
                let sep = char::from_u32(arg_i(a, 1, 44) as u32).unwrap_or(',');
                let arr = self.arr_of(a.get(2).unwrap_or(&Value::Void))?;
                let parts: Vec<Value> = s.split(sep).map(Value::str).collect();
                let n = parts.len() as i64;
                self.set_array_chrono(&arr, parts);
                vi(n)
            }
            "StringCompare" => {
                let (x, y) = if a.get(2).map(|v| v.truthy()).unwrap_or(true) {
                    (arg_s(a, 0), arg_s(a, 1))
                } else {
                    (arg_s(a, 0).to_lowercase(), arg_s(a, 1).to_lowercase())
                };
                vi(match x.cmp(&y) {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                })
            }
            "StringGetCharacter" => vi(arg_s(a, 0).chars().nth(arg_i(a, 1, 0).max(0) as usize).map(|c| c as i64).unwrap_or(0)),
            // ---------- arrays ----------
            "ArraySize" => match a.first() {
                Some(Value::Arr(arr)) => {
                    let ab = arr.borrow();
                    let inner: usize = ab.inner.iter().product::<usize>().max(1);
                    vi((ab.data.len() * inner) as i64)
                }
                _ => vi(0),
            },
            "ArrayRange" => {
                let arr = self.arr_of(a.first().unwrap_or(&Value::Void))?;
                let d = arg_i(a, 1, 0) as usize;
                let ab = arr.borrow();
                vi(if d == 0 { ab.data.len() as i64 } else { ab.inner.get(d - 1).copied().unwrap_or(0) as i64 })
            }
            "ArrayResize" => {
                let arr = self.arr_of(a.first().unwrap_or(&Value::Void))?;
                let n = arg_i(a, 1, 0).max(0) as usize;
                let (elem, inner) = {
                    let ab = arr.borrow();
                    (ab.elem.clone(), ab.inner.clone())
                };
                let d = self.array_elem_default(&elem, &inner);
                arr.borrow_mut().data.resize(n, d);
                vi(n as i64)
            }
            "ArrayFree" => {
                if let Some(Value::Arr(arr)) = a.first() {
                    arr.borrow_mut().data.clear();
                }
                Value::Void
            }
            "ArrayInitialize" => {
                let arr = self.arr_of(a.first().unwrap_or(&Value::Void))?;
                let elem = arr.borrow().elem.clone();
                let v = self.coerce(a.get(1).cloned().unwrap_or(vi(0)), &elem);
                let mut ab = arr.borrow_mut();
                let n = ab.data.len();
                for x in ab.data.iter_mut() {
                    *x = v.clone();
                }
                vi(n as i64)
            }
            "ArrayFill" => {
                let arr = self.arr_of(a.first().unwrap_or(&Value::Void))?;
                let elem = arr.borrow().elem.clone();
                let st = arg_i(a, 1, 0).max(0) as usize;
                let cnt = arg_i(a, 2, 0).max(0) as usize;
                let v = self.coerce(a.get(3).cloned().unwrap_or(vi(0)), &elem);
                let mut ab = arr.borrow_mut();
                let len = ab.data.len();
                let series = ab.series;
                for k in st..(st + cnt).min(len) {
                    let p = if series { len - 1 - k } else { k };
                    ab.data[p] = v.clone();
                }
                Value::Void
            }
            "ArraySetAsSeries" => {
                if let Some(Value::Arr(arr)) = a.first() {
                    arr.borrow_mut().series = a.get(1).map(|v| v.truthy()).unwrap_or(false);
                    vb(true)
                } else {
                    vb(false)
                }
            }
            "ArrayGetAsSeries" | "ArrayIsSeries" => vb(matches!(a.first(), Some(Value::Arr(arr)) if arr.borrow().series)),
            "ArrayIsDynamic" => vb(matches!(a.first(), Some(Value::Arr(arr)) if arr.borrow().dynamic)),
            "ArrayCopy" => {
                let dst = self.arr_of(a.first().unwrap_or(&Value::Void))?;
                let src = self.arr_of(a.get(1).unwrap_or(&Value::Void))?;
                let ds = arg_i(a, 2, 0).max(0) as usize;
                let ss = arg_i(a, 3, 0).max(0) as usize;
                let cnt = arg_i(a, 4, -1);
                let (sdata, sseries) = {
                    let s = src.borrow();
                    (s.data.clone(), s.series)
                };
                let logical: Vec<Value> = if sseries { sdata.into_iter().rev().collect() } else { sdata };
                let avail = logical.len().saturating_sub(ss);
                let take = if cnt < 0 { avail } else { (cnt as usize).min(avail) };
                let elem = dst.borrow().elem.clone();
                let def = self.default_value(&elem);
                let mut d = dst.borrow_mut();
                let dseries = d.series;
                let need = ds + take;
                if d.data.len() < need {
                    d.data.resize(need, def);
                }
                let len = d.data.len();
                for k in 0..take {
                    let v = logical[ss + k].clone();
                    let p = if dseries { len - 1 - (ds + k) } else { ds + k };
                    d.data[p] = v;
                }
                vi(take as i64)
            }
            "ArrayMaximum" | "ArrayMinimum" => {
                let arr = self.arr_of(a.first().unwrap_or(&Value::Void))?;
                let ab = arr.borrow();
                let len = ab.data.len();
                let st = arg_i(a, 1, 0).max(0) as usize;
                let cnt = arg_i(a, 2, -1);
                let end = if cnt < 0 { len } else { (st + cnt as usize).min(len) };
                let mut best: Option<(usize, f64)> = None;
                for k in st..end {
                    let p = if ab.series { len - 1 - k } else { k };
                    let v = ab.data[p].as_f64();
                    let better = match best {
                        None => true,
                        Some((_, b)) => {
                            if name == "ArrayMaximum" {
                                v > b
                            } else {
                                v < b
                            }
                        }
                    };
                    if better {
                        best = Some((k, v));
                    }
                }
                vi(best.map(|b| b.0 as i64).unwrap_or(-1))
            }
            "ArraySort" => {
                if let Some(Value::Arr(arr)) = a.first() {
                    let mut ab = arr.borrow_mut();
                    if matches!(ab.elem, Ty::Str) {
                        ab.data.sort_by_key(|x| x.to_text());
                    } else {
                        ab.data.sort_by(|x, y| x.as_f64().partial_cmp(&y.as_f64()).unwrap_or(std::cmp::Ordering::Equal));
                    }
                }
                vb(true)
            }
            "ArrayReverse" => {
                if let Some(Value::Arr(arr)) = a.first() {
                    arr.borrow_mut().data.reverse();
                }
                vb(true)
            }
            "ZeroMemory" => {
                match a.first() {
                    Some(Value::Arr(arr)) => {
                        let elem = arr.borrow().elem.clone();
                        let d = self.default_value(&elem);
                        for x in arr.borrow_mut().data.iter_mut() {
                            *x = d.clone();
                        }
                    }
                    Some(Value::Struct(s)) => {
                        let ty = s.borrow().ty;
                        if let Value::Struct(fresh) = self.new_struct(ty) {
                            let f = fresh.borrow().fields.clone();
                            s.borrow_mut().fields = f;
                        }
                    }
                    _ => {}
                }
                Value::Void
            }
            "CheckPointer" => match a.first() {
                Some(Value::Struct(_)) | Some(Value::Obj(_)) => vi(1),
                _ => vi(0),
            },
            "GetPointer" => a.first().cloned().unwrap_or(vi(0)),
            // ---------- time ----------
            "TimeCurrent" | "TimeTradeServer" => vi(host.now()),
            "TimeLocal" | "TimeGMT" => vi(if host.is_tester() { host.now() } else { crate::market::unix_now() }),
            "TimeToStruct" => {
                let t = arg_i(a, 0, 0);
                if let Some(s) = a.get(1) {
                    let (y, mo, d, h, mi, sec) = civil(t);
                    let days = t.div_euclid(86400);
                    let jan1 = crate::mql::lexer::days_from_civil(y, 1, 1);
                    self.struct_set(s, "year", vi(y));
                    self.struct_set(s, "mon", vi(mo as i64));
                    self.struct_set(s, "day", vi(d as i64));
                    self.struct_set(s, "hour", vi(h as i64));
                    self.struct_set(s, "min", vi(mi as i64));
                    self.struct_set(s, "sec", vi(sec as i64));
                    self.struct_set(s, "day_of_week", vi((days + 4).rem_euclid(7)));
                    self.struct_set(s, "day_of_year", vi(days - jan1));
                }
                vb(true)
            }
            "StructToTime" => {
                let s = a.first().cloned().unwrap_or(Value::Void);
                let g = |f: &str| self.struct_get(&s, f).as_i64();
                vi(crate::mql::lexer::days_from_civil(g("year"), g("mon"), g("day")) * 86400 + g("hour") * 3600 + g("min") * 60 + g("sec"))
            }
            "GetTickCount" => vi((std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0) & 0xffff_ffff) as i64),
            "GetMicrosecondCount" => vi(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros() as i64).unwrap_or(0)),
            // ---------- series ----------
            "Bars" | "iBars" => {
                let sym = self.sym_arg(a.first());
                let tf = self.tf_arg(a.get(1));
                vi(self.with_bars(host, &sym, tf, |b| b.len() as i64).unwrap_or(0))
            }
            "iClose" => vd(self.bar_at(host, a).map(|b| b.close).unwrap_or(0.0)),
            "iOpen" => vd(self.bar_at(host, a).map(|b| b.open).unwrap_or(0.0)),
            "iHigh" => vd(self.bar_at(host, a).map(|b| b.high).unwrap_or(0.0)),
            "iLow" => vd(self.bar_at(host, a).map(|b| b.low).unwrap_or(0.0)),
            "iTime" => vi(self.bar_at(host, a).map(|b| b.time).unwrap_or(0)),
            "iVolume" | "iTickVolume" => vi(self.bar_at(host, a).map(|b| b.tick_volume).unwrap_or(0)),
            "iHighest" | "iLowest" => {
                let sym = self.sym_arg(a.first());
                let tf = self.tf_arg(a.get(1));
                let mode = arg_i(a, 2, 2);
                let count = arg_i(a, 3, -1);
                let start = arg_i(a, 4, 0).max(0);
                let highest = name == "iHighest";
                vi(self
                    .with_bars(host, &sym, tf, |b| {
                        let n = b.len() as i64;
                        let cnt = if count < 0 { n - start } else { count };
                        let mut best: Option<(i64, f64)> = None;
                        for sh in start..(start + cnt).min(n) {
                            let bar = &b[(n - 1 - sh) as usize];
                            let v = match mode {
                                0 => bar.open,
                                1 => bar.low,
                                3 => bar.close,
                                4 => bar.tick_volume as f64,
                                7 => bar.time as f64,
                                _ => bar.high,
                            };
                            let better = match best {
                                None => true,
                                Some((_, bv)) => {
                                    if highest {
                                        v > bv
                                    } else {
                                        v < bv
                                    }
                                }
                            };
                            if better {
                                best = Some((sh, v));
                            }
                        }
                        best.map(|x| x.0).unwrap_or(-1)
                    })
                    .unwrap_or(-1))
            }
            "iBarShift" => {
                let sym = self.sym_arg(a.first());
                let tf = self.tf_arg(a.get(1));
                let t = arg_i(a, 2, 0);
                let exact = a.get(3).map(|v| v.truthy()).unwrap_or(false);
                vi(self
                    .with_bars(host, &sym, tf, |b| {
                        let n = b.len() as i64;
                        match b.iter().rposition(|x| x.time <= t) {
                            Some(i) => {
                                if exact && b[i].time != t {
                                    -1
                                } else {
                                    n - 1 - i as i64
                                }
                            }
                            None => -1,
                        }
                    })
                    .unwrap_or(-1))
            }
            "CopyClose" => self.copy_series(host, a, |b| Value::Double(b.close))?,
            "CopyOpen" => self.copy_series(host, a, |b| Value::Double(b.open))?,
            "CopyHigh" => self.copy_series(host, a, |b| Value::Double(b.high))?,
            "CopyLow" => self.copy_series(host, a, |b| Value::Double(b.low))?,
            "CopyTime" => self.copy_series(host, a, |b| Value::Int(b.time))?,
            "CopyTickVolume" => self.copy_series(host, a, |b| Value::Int(b.tick_volume))?,
            "CopyRates" => {
                let sym = self.sym_arg(a.first());
                let tf = self.tf_arg(a.get(1));
                let start = arg_i(a, 2, 0);
                let count = arg_i(a, 3, 0);
                let arr = self.arr_of(a.get(4).unwrap_or(&Value::Void))?;
                let bars: Vec<Bar> = self
                    .with_bars(host, &sym, tf, |b| {
                        let n = b.len() as i64;
                        let newest = n - 1 - start;
                        let oldest = (newest - count + 1).max(0);
                        if newest < 0 || oldest > newest {
                            vec![]
                        } else {
                            b[oldest as usize..=newest as usize].to_vec()
                        }
                    })
                    .unwrap_or_default();
                let sym_rates = self.prog.interner.get("MqlRates");
                let mut vals = vec![];
                for b in &bars {
                    let s = match sym_rates {
                        Some(s) => self.new_struct(s),
                        None => Value::Void,
                    };
                    self.struct_set(&s, "time", vi(b.time));
                    self.struct_set(&s, "open", vd(b.open));
                    self.struct_set(&s, "high", vd(b.high));
                    self.struct_set(&s, "low", vd(b.low));
                    self.struct_set(&s, "close", vd(b.close));
                    self.struct_set(&s, "tick_volume", vi(b.tick_volume));
                    self.struct_set(&s, "spread", vi(b.spread as i64));
                    vals.push(s);
                }
                let n = vals.len() as i64;
                self.set_array_chrono(&arr, vals);
                vi(if n == 0 { -1 } else { n })
            }
            // ---------- symbols ----------
            "Symbol" | "ChartSymbol" => Value::str(&self.symbol),
            "Period" | "ChartPeriod" => vi(self.tf.mql_value()),
            "Point" => vd(host.quote(&self.symbol).map(|q| q.spec.point()).unwrap_or(0.00001)),
            "Digits" => vi(host.quote(&self.symbol).map(|q| q.spec.digits as i64).unwrap_or(5)),
            "SymbolInfoDouble" => {
                let sym = self.sym_arg(a.first());
                match host.quote(&sym) {
                    Some(q) => {
                        let tick_value = match q.spec.calc {
                            crate::market::ProfitCalc::QuoteUsd => q.spec.contract_size * q.spec.point(),
                            crate::market::ProfitCalc::BaseUsd => q.spec.contract_size * q.spec.point() / q.bid.max(1e-9),
                        };
                        vd(match arg_i(a, 1, 0) {
                            1 | 13 | 22 | 23 | 30 => q.bid,
                            2 => q.ask,
                            3 | 11 => q.spec.point(),
                            6 => q.spec.volume_min,
                            7 => q.spec.volume_max,
                            8 => q.spec.volume_step,
                            9 => q.spec.contract_size,
                            10 | 28 | 29 => tick_value,
                            _ => 0.0,
                        })
                    }
                    None => {
                        self.last_error = 4301;
                        Value::Void
                    }
                }
            }
            "SymbolInfoInteger" => {
                let sym = self.sym_arg(a.first());
                match host.quote(&sym) {
                    Some(q) => vi(match arg_i(a, 1, 0) {
                        4 => q.spec.digits as i64,
                        5 => ((q.ask - q.bid) / q.spec.point()).round() as i64,
                        14 => host.now(),
                        15 | 24 | 25 => 1,
                        26 => 4,
                        27 => 3,
                        _ => 0,
                    }),
                    None => {
                        self.last_error = 4301;
                        Value::Void
                    }
                }
            }
            "SymbolInfoString" => {
                let sym = self.sym_arg(a.first());
                match host.quote(&sym) {
                    Some(q) => {
                        let n = q.spec.name;
                        Value::str(match arg_i(a, 1, 0) {
                            17 => q.spec.description,
                            18 => &n[..3.min(n.len())],
                            19 | 20 => &n[n.len().saturating_sub(3)..],
                            21 => "Simulated",
                            _ => "",
                        })
                    }
                    None => Value::str(""),
                }
            }
            "SymbolInfoTick" => {
                let sym = self.sym_arg(a.first());
                match (host.quote(&sym), a.get(1)) {
                    (Some(q), Some(t)) => {
                        let now = host.now();
                        self.struct_set(t, "time", vi(now));
                        self.struct_set(t, "bid", vd(q.bid));
                        self.struct_set(t, "ask", vd(q.ask));
                        self.struct_set(t, "last", vd(q.bid));
                        self.struct_set(t, "time_msc", vi(now * 1000));
                        vb(true)
                    }
                    _ => vb(false),
                }
            }
            "SymbolsTotal" => vi(host.symbol_names().len() as i64),
            "SymbolName" => Value::str(host.symbol_names().get(arg_i(a, 0, 0).max(0) as usize).map(|s| s.as_str()).unwrap_or("")),
            "SymbolSelect" | "RefreshRates" => vb(true),
            // ---------- account ----------
            "AccountInfoDouble" => {
                let acc = host.account();
                let eq = acc.equity();
                let bal = acc.balance;
                let fl = acc.floating();
                let m = host.margin();
                vd(match arg_i(a, 0, 0) {
                    1 => bal,
                    2 => eq,
                    3 => fl,
                    4 => m,
                    5 => eq - m,
                    6 => {
                        if m > 0.0 {
                            eq / m * 100.0
                        } else {
                            0.0
                        }
                    }
                    8 => crate::account::MARGIN_CALL_LEVEL,
                    9 => crate::account::STOP_OUT_LEVEL,
                    _ => 0.0,
                })
            }
            "AccountInfoInteger" => {
                let acc = host.account();
                vi(match arg_i(a, 0, 0) {
                    10 => acc.login as i64,
                    11 => acc.leverage as i64,
                    12 | 13 => 1,
                    14 => 200,
                    16 => 2,
                    _ => 0,
                })
            }
            "AccountInfoString" => {
                let acc = host.account();
                Value::str(&match arg_i(a, 0, 0) {
                    17 => acc.name.clone(),
                    18 => acc.server.clone(),
                    19 => acc.currency.clone(),
                    20 => "Simulated Broker".to_string(),
                    _ => String::new(),
                })
            }
            // ---------- trading ----------
            "OrderSend" | "OrderCheck" => {
                let req = a.first().cloned().unwrap_or(Value::Void);
                let res = a.get(1).cloned().unwrap_or(Value::Void);
                let action = self.struct_get(&req, "action").as_i64();
                let mut symbol = self.struct_get(&req, "symbol").to_text();
                if symbol.is_empty() {
                    symbol = self.symbol.clone();
                }
                let volume = self.struct_get(&req, "volume").as_f64();
                let typ = self.struct_get(&req, "type").as_i64();
                let price = self.struct_get(&req, "price").as_f64();
                let sl = self.struct_get(&req, "sl").as_f64();
                let tp = self.struct_get(&req, "tp").as_f64();
                let magic = self.struct_get(&req, "magic").as_i64();
                let comment = self.struct_get(&req, "comment").to_text();
                let position = self.struct_get(&req, "position").as_i64() as u64;
                let order = self.struct_get(&req, "order").as_i64() as u64;
                if name == "OrderCheck" {
                    let ok = volume > 0.0 || action != 1;
                    self.struct_set(&res, "retcode", vi(if ok { 0 } else { 10014 }));
                    let (bal, eq) = (host.account().balance, host.account().equity());
                    self.struct_set(&res, "balance", vd(bal));
                    self.struct_set(&res, "equity", vd(eq));
                    return Ok(vb(ok));
                }
                let op = match action {
                    1 if position != 0 => TradeOp::Close { ticket: position, volume: Some(volume) },
                    1 => TradeOp::Market { symbol, buy: typ == 0, volume, sl, tp, magic, comment },
                    5 => TradeOp::Pending { symbol, typ, volume, price, sl, tp, magic, comment },
                    6 => {
                        let t = if position != 0 {
                            position
                        } else {
                            host.account().positions.iter().find(|p| p.symbol == symbol).map(|p| p.ticket).unwrap_or(0)
                        };
                        TradeOp::Modify { ticket: t, sl, tp }
                    }
                    7 => TradeOp::ModifyOrder { ticket: order, price, sl, tp },
                    8 => TradeOp::Delete { ticket: order },
                    _ => {
                        self.struct_set(&res, "retcode", vi(10013));
                        return Ok(vb(false));
                    }
                };
                let r = self.do_trade(host, op);
                let qq = host.quote(&self.symbol);
                self.struct_set(&res, "retcode", vi(r.retcode));
                self.struct_set(&res, "deal", vi(r.deal as i64));
                self.struct_set(&res, "order", vi(r.order as i64));
                self.struct_set(&res, "volume", vd(r.volume));
                self.struct_set(&res, "price", vd(r.price));
                self.struct_set(&res, "bid", vd(qq.as_ref().map(|q| q.bid).unwrap_or(0.0)));
                self.struct_set(&res, "ask", vd(qq.as_ref().map(|q| q.ask).unwrap_or(0.0)));
                self.struct_set(&res, "comment", Value::str(&r.comment));
                vb(r.retcode == RET_DONE)
            }
            "PositionsTotal" => vi(host.account().positions.len() as i64),
            "PositionGetTicket" => {
                let i = arg_i(a, 0, 0);
                let t = host.account().positions.get(i.max(0) as usize).map(|p| p.ticket);
                self.select_position(t);
                vi(t.unwrap_or(0) as i64)
            }
            "PositionGetSymbol" => {
                let i = arg_i(a, 0, 0);
                let p = host.account().positions.get(i.max(0) as usize).cloned();
                self.select_position(p.as_ref().map(|p| p.ticket));
                Value::str(&p.map(|p| p.symbol).unwrap_or_default())
            }
            "PositionSelect" => {
                let sym = arg_s(a, 0);
                let t = host.account().positions.iter().find(|p| p.symbol == sym).map(|p| p.ticket);
                self.select_position(t);
                vb(t.is_some())
            }
            "PositionSelectByTicket" => {
                let t = arg_i(a, 0, 0) as u64;
                let ok = host.account().positions.iter().any(|p| p.ticket == t);
                self.select_position(if ok { Some(t) } else { None });
                vb(ok)
            }
            "PositionGetDouble" | "PositionGetInteger" | "PositionGetString" => {
                let p = self.position(host, self.selected_position());
                match p {
                    Some(p) => pos_prop(&p, arg_i(a, 0, 0), name),
                    None => {
                        self.last_error = 4753;
                        match name {
                            "PositionGetString" => Value::str(""),
                            "PositionGetDouble" => vd(0.0),
                            _ => vi(0),
                        }
                    }
                }
            }
            "OrdersTotal" => vi(host.account().orders.len() as i64),
            "OrderGetTicket" => {
                let t = host.account().orders.get(arg_i(a, 0, 0).max(0) as usize).map(|o| o.ticket);
                self.select_order(t);
                vi(t.unwrap_or(0) as i64)
            }
            "OrderSelect" => {
                let t = arg_i(a, 0, 0) as u64;
                let ok = host.account().orders.iter().any(|o| o.ticket == t);
                self.select_order(if ok { Some(t) } else { None });
                vb(ok)
            }
            "OrderGetDouble" | "OrderGetInteger" | "OrderGetString" => {
                let o = self.order(host, self.selected_order());
                let bid = o.as_ref().and_then(|o| host.quote(&o.symbol)).map(|q| q.bid).unwrap_or(0.0);
                match o {
                    Some(o) => order_prop(&o, arg_i(a, 0, 0), name, bid),
                    None => {
                        self.last_error = 4754;
                        match name {
                            "OrderGetString" => Value::str(""),
                            "OrderGetDouble" => vd(0.0),
                            _ => vi(0),
                        }
                    }
                }
            }
            "HistorySelect" => vb(true),
            "HistoryDealsTotal" => vi(host.account().deals.len() as i64),
            "HistoryOrdersTotal" => vi(host.account().deals.iter().filter(|d| d.order != 0).count() as i64),
            "HistoryDealGetTicket" => vi(host.account().deals.get(arg_i(a, 0, 0).max(0) as usize).map(|d| d.ticket as i64).unwrap_or(0)),
            "HistoryDealGetDouble" | "HistoryDealGetInteger" | "HistoryDealGetString" => {
                let t = arg_i(a, 0, 0) as u64;
                let d = host.account().deals.iter().find(|d| d.ticket == t).cloned();
                match d {
                    Some(d) => deal_prop(&d, arg_i(a, 1, 0), name),
                    None => match name {
                        "HistoryDealGetString" => Value::str(""),
                        "HistoryDealGetDouble" => vd(0.0),
                        _ => vi(0),
                    },
                }
            }
            // ---------- indicators ----------
            "iMA" => {
                let p = IndParams { p1: arg_i(a, 2, 14), shift: arg_i(a, 3, 0), method: arg_i(a, 4, 0), price: arg_i(a, 5, 1), ..IndParams::defaults(IndKind::MovingAverage) };
                self.builtin_handle(a, IndKind::MovingAverage, p)
            }
            "iRSI" => {
                let p = IndParams { p1: arg_i(a, 2, 14), price: arg_i(a, 3, 1), ..IndParams::defaults(IndKind::Rsi) };
                self.builtin_handle(a, IndKind::Rsi, p)
            }
            "iMACD" => {
                let p = IndParams { p1: arg_i(a, 2, 12), p2: arg_i(a, 3, 26), p3: arg_i(a, 4, 9), price: arg_i(a, 5, 1), ..IndParams::defaults(IndKind::Macd) };
                self.builtin_handle(a, IndKind::Macd, p)
            }
            "iBands" => {
                let p = IndParams { p1: arg_i(a, 2, 20), shift: arg_i(a, 3, 0), f1: arg_f(a, 4, 2.0), price: arg_i(a, 5, 1), ..IndParams::defaults(IndKind::Bands) };
                self.builtin_handle(a, IndKind::Bands, p)
            }
            "iStochastic" => {
                let p = IndParams { p1: arg_i(a, 2, 5), p2: arg_i(a, 3, 3), p3: arg_i(a, 4, 3), method: arg_i(a, 5, 0), ..IndParams::defaults(IndKind::Stochastic) };
                self.builtin_handle(a, IndKind::Stochastic, p)
            }
            "iATR" => {
                let p = IndParams { p1: arg_i(a, 2, 14), ..IndParams::defaults(IndKind::Atr) };
                self.builtin_handle(a, IndKind::Atr, p)
            }
            "iCCI" => {
                let p = IndParams { p1: arg_i(a, 2, 14), price: arg_i(a, 3, 6), ..IndParams::defaults(IndKind::Cci) };
                self.builtin_handle(a, IndKind::Cci, p)
            }
            "iMomentum" => {
                let p = IndParams { p1: arg_i(a, 2, 14), price: arg_i(a, 3, 1), ..IndParams::defaults(IndKind::Momentum) };
                self.builtin_handle(a, IndKind::Momentum, p)
            }
            "iADX" => {
                let p = IndParams { p1: arg_i(a, 2, 14), ..IndParams::defaults(IndKind::Adx) };
                self.builtin_handle(a, IndKind::Adx, p)
            }
            "iEnvelopes" => {
                let p = IndParams { p1: arg_i(a, 2, 14), shift: arg_i(a, 3, 0), method: arg_i(a, 4, 0), price: arg_i(a, 5, 1), f1: arg_f(a, 6, 0.1), ..IndParams::defaults(IndKind::Envelopes) };
                self.builtin_handle(a, IndKind::Envelopes, p)
            }
            "iSAR" => {
                let p = IndParams { f1: arg_f(a, 2, 0.02), f2: arg_f(a, 3, 0.2), ..IndParams::defaults(IndKind::Sar) };
                self.builtin_handle(a, IndKind::Sar, p)
            }
            "iWPR" => {
                let p = IndParams { p1: arg_i(a, 2, 14), ..IndParams::defaults(IndKind::Wpr) };
                self.builtin_handle(a, IndKind::Wpr, p)
            }
            "iIchimoku" => {
                let p = IndParams { p1: arg_i(a, 2, 9), p2: arg_i(a, 3, 26), p3: arg_i(a, 4, 52), ..IndParams::defaults(IndKind::Ichimoku) };
                self.builtin_handle(a, IndKind::Ichimoku, p)
            }
            "iAlligator" => {
                let p = IndParams { p1: arg_i(a, 2, 13), p2: arg_i(a, 4, 8), p3: arg_i(a, 6, 5), method: arg_i(a, 8, 2), price: arg_i(a, 9, 5), ..IndParams::defaults(IndKind::Alligator) };
                self.builtin_handle(a, IndKind::Alligator, p)
            }
            "iFractals" => self.builtin_handle(a, IndKind::Fractals, IndParams::defaults(IndKind::Fractals)),
            "iVolumes" => self.builtin_handle(a, IndKind::Volumes, IndParams::defaults(IndKind::Volumes)),
            "iCustom" => {
                let symbol = self.sym_arg(a.first());
                let tf = self.tf_arg(a.get(1));
                let pname = arg_s(a, 2);
                match host.load_program(&pname) {
                    Ok(prog) => {
                        if prog.kind != ProgramKind::Indicator {
                            host.print(&format!("iCustom: '{}' is not an indicator", pname));
                            return Ok(vi(-1));
                        }
                        let mut overrides = HashMap::new();
                        for (k, inp) in prog.inputs.iter().enumerate() {
                            if let Some(v) = a.get(3 + k) {
                                overrides.insert(prog.interner.name(inp.name).to_string(), v.to_text());
                            }
                        }
                        let mut child = Instance::new(prog, &symbol, tf, &overrides);
                        match child.init(host) {
                            Ok(_) => self.add_handle(Handle::Custom { inst: Some(Box::new(child)), symbol, tf }),
                            Err(e) => {
                                let m = child.fmt_err(&e);
                                host.print(&format!("iCustom '{}': {}", pname, m));
                                vi(-1)
                            }
                        }
                    }
                    Err(e) => {
                        host.print(&format!("cannot load custom indicator '{}' [{}]", pname, e));
                        self.last_error = 4802;
                        vi(-1)
                    }
                }
            }
            "IndicatorRelease" => {
                let h = arg_i(a, 0, -1);
                if h >= 0 && (h as usize) < self.handles.len() {
                    self.handles[h as usize] = None;
                    vb(true)
                } else {
                    vb(false)
                }
            }
            "BarsCalculated" => {
                let h = arg_i(a, 0, -1);
                let st = match self.handles.get(h.max(0) as usize).and_then(|x| x.as_ref()) {
                    Some(Handle::Builtin { symbol, tf, .. }) | Some(Handle::Custom { symbol, tf, .. }) | Some(Handle::Ext { symbol, tf, .. }) => Some((symbol.clone(), *tf)),
                    None => None,
                };
                match st {
                    Some((s, t)) => vi(self.with_bars(host, &s, t, |b| b.len() as i64).unwrap_or(-1)),
                    None => vi(-1),
                }
            }
            "CopyBuffer" => {
                let h = arg_i(a, 0, -1);
                let buf = arg_i(a, 1, 0).max(0) as usize;
                let start = arg_i(a, 2, 0);
                let count = arg_i(a, 3, 0);
                let arr = self.arr_of(a.get(4).unwrap_or(&Value::Void))?;
                match self.handle_buffer(host, h, buf)? {
                    Some(data) => {
                        let n = data.len() as i64;
                        let newest = n - 1 - start;
                        let oldest = (newest - count + 1).max(0);
                        if newest < 0 || oldest > newest {
                            self.last_error = 4806;
                            vi(-1)
                        } else {
                            let vals: Vec<Value> = data[oldest as usize..=newest as usize].iter().map(|v| vd(*v)).collect();
                            let k = vals.len() as i64;
                            self.set_array_chrono(&arr, vals);
                            vi(k)
                        }
                    }
                    None => {
                        self.last_error = 4806;
                        vi(-1)
                    }
                }
            }
            "SetIndexBuffer" => {
                let idx = arg_i(a, 0, 0).max(0) as usize;
                let arr = self.arr_of(a.get(1).unwrap_or(&Value::Void))?;
                let typ = arg_i(a, 2, 0);
                if self.buffers.len() <= idx {
                    self.buffers.resize(idx + 1, None);
                }
                self.buffers[idx] = Some((arr, typ));
                vb(true)
            }
            "PlotIndexSetInteger" => {
                let plot = arg_i(a, 0, 0).max(0) as usize;
                let prop = arg_i(a, 1, 0);
                let (modifier, val) = if a.len() >= 4 { (arg_i(a, 2, 0), arg_i(a, 3, 0)) } else { (0, arg_i(a, 2, 0)) };
                self.ensure_plot(plot);
                let p = &mut self.plots[plot];
                match prop {
                    0 => p.arrow = val,
                    2 => p.draw_begin = val,
                    3 => p.draw_type = val,
                    5 => p.shift = val,
                    6 => p.style = val,
                    7 => p.width = val,
                    8 => {
                        let first = p.colors.first().copied().unwrap_or(0xFF901E);
                        p.colors.resize(val.max(1) as usize, first);
                    }
                    9 => {
                        let m = modifier.max(0) as usize;
                        if p.colors.len() <= m {
                            p.colors.resize(m + 1, val as u32);
                        }
                        p.colors[m] = val as u32;
                    }
                    _ => {}
                }
                vb(true)
            }
            "PlotIndexSetDouble" => {
                let plot = arg_i(a, 0, 0).max(0) as usize;
                self.ensure_plot(plot);
                if arg_i(a, 1, 0) == 10 {
                    self.plots[plot].empty_value = arg_f(a, 2, f64::MAX);
                }
                vb(true)
            }
            "PlotIndexSetString" => {
                let plot = arg_i(a, 0, 0).max(0) as usize;
                self.ensure_plot(plot);
                if arg_i(a, 1, 0) == 11 {
                    self.plots[plot].label = arg_s(a, 2);
                }
                vb(true)
            }
            "PlotIndexGetInteger" => {
                let plot = arg_i(a, 0, 0).max(0) as usize;
                self.ensure_plot(plot);
                let p = &self.plots[plot];
                vi(match arg_i(a, 1, 0) {
                    0 => p.arrow,
                    2 => p.draw_begin,
                    3 => p.draw_type,
                    5 => p.shift,
                    6 => p.style,
                    7 => p.width,
                    9 => p.colors.get(arg_i(a, 2, 0).max(0) as usize).copied().unwrap_or(0) as i64,
                    _ => 0,
                })
            }
            "IndicatorSetString" => {
                if arg_i(a, 0, -1) == 0 {
                    self.short_name = arg_s(a, 1);
                }
                vb(true)
            }
            "IndicatorSetInteger" => {
                match arg_i(a, 0, -1) {
                    1 => self.digits = Some(arg_i(a, 1, 5)),
                    3 => {
                        let n = arg_i(a, 1, 0).max(0) as usize;
                        self.levels.resize(n, 0.0);
                    }
                    _ => {}
                }
                vb(true)
            }
            "IndicatorSetDouble" => {
                match arg_i(a, 0, -1) {
                    7 => self.minimum = Some(arg_f(a, 1, 0.0)),
                    8 => self.maximum = Some(arg_f(a, 1, 0.0)),
                    9 => {
                        let m = arg_i(a, 1, 0).max(0) as usize;
                        if self.levels.len() <= m {
                            self.levels.resize(m + 1, 0.0);
                        }
                        self.levels[m] = arg_f(a, 2, 0.0);
                    }
                    _ => {}
                }
                vb(true)
            }
            // ---------- chart objects ----------
            "ObjectCreate" => {
                let oname = arg_s(a, 1);
                let kind = match ObjKind::from_mql(arg_i(a, 2, 0)) {
                    Some(k) => k,
                    None => {
                        self.last_error = 4202;
                        return Ok(vb(false));
                    }
                };
                let objs = host.objects();
                if objs.find(&oname).is_some() {
                    self.last_error = 4200;
                    return Ok(vb(false));
                }
                let mut o = ChartObj::new(&oname, kind);
                o.from_program = true;
                for k in 0..3 {
                    o.times[k] = arg_i(a, 4 + k * 2, 0);
                    o.prices[k] = arg_f(a, 5 + k * 2, 0.0);
                }
                objs.list.push(o);
                vb(true)
            }
            "ObjectDelete" => vb(host.objects().delete(&arg_s(a, 1))),
            "ObjectsDeleteAll" => {
                let prefix = match a.get(1) {
                    Some(Value::Str(s)) => s.to_string(),
                    _ => String::new(),
                };
                let objs = host.objects();
                let n = objs.list.len();
                objs.list.retain(|o| !o.name.starts_with(&prefix));
                vi((n - objs.list.len()) as i64)
            }
            "ObjectFind" => vi(if host.objects().find(&arg_s(a, 1)).is_some() { 0 } else { -1 }),
            "ObjectsTotal" => vi(host.objects().list.len() as i64),
            "ObjectName" => Value::str(&host.objects().list.get(arg_i(a, 1, 0).max(0) as usize).map(|o| o.name.clone()).unwrap_or_default()),
            "ObjectMove" => {
                let oname = arg_s(a, 1);
                let pt = arg_i(a, 2, 0).clamp(0, 2) as usize;
                let (t, p) = (arg_i(a, 3, 0), arg_f(a, 4, 0.0));
                match host.objects().find_mut(&oname) {
                    Some(o) => {
                        o.times[pt] = t;
                        o.prices[pt] = p;
                        vb(true)
                    }
                    None => vb(false),
                }
            }
            "ObjectSetInteger" | "ObjectSetDouble" | "ObjectSetString" => {
                let oname = arg_s(a, 1);
                let prop = arg_i(a, 2, 0);
                let (modifier, val) = if a.len() >= 5 { (arg_i(a, 3, 0), a[4].clone()) } else { (0, a.get(3).cloned().unwrap_or(Value::Void)) };
                match host.objects().find_mut(&oname) {
                    Some(o) => {
                        set_obj_prop(o, prop, modifier, &val);
                        vb(true)
                    }
                    None => {
                        self.last_error = 4202;
                        vb(false)
                    }
                }
            }
            "ObjectGetInteger" | "ObjectGetDouble" | "ObjectGetString" => {
                let oname = arg_s(a, 1);
                let prop = arg_i(a, 2, 0);
                let modifier = arg_i(a, 3, 0);
                match host.objects().find(&oname) {
                    Some(o) => get_obj_prop(o, prop, modifier),
                    None => vi(0),
                }
            }
            "ChartRedraw" | "ChartSetInteger" => vb(true),
            "ChartID" => vi(0),
            "ChartGetInteger" => vi(match arg_i(a, 1, 0) {
                1 => 1000,
                2 => 600,
                3 | 4 => 120,
                5..=7 => 1,
                _ => 0,
            }),
            "ChartGetDouble" => {
                let (s, t) = (self.symbol.clone(), self.tf);
                let prop = arg_i(a, 1, 0);
                vd(self
                    .with_bars(host, &s, t, |b| {
                        let tail = &b[b.len().saturating_sub(100)..];
                        if prop == 9 {
                            tail.iter().map(|x| x.high).fold(f64::MIN, f64::max)
                        } else {
                            tail.iter().map(|x| x.low).fold(f64::MAX, f64::min)
                        }
                    })
                    .unwrap_or(0.0))
            }
            // ---------- events / misc ----------
            "EventSetTimer" => {
                self.timer_ms = (arg_i(a, 0, 1).max(1) * 1000) as u64;
                vb(true)
            }
            "EventSetMillisecondTimer" => {
                self.timer_ms = arg_i(a, 0, 100).max(10) as u64;
                vb(true)
            }
            "EventKillTimer" => {
                self.timer_ms = 0;
                Value::Void
            }
            "GetLastError" => vi(self.last_error),
            "ResetLastError" => {
                self.last_error = 0;
                Value::Void
            }
            "IsStopped" => vb(false),
            "ExpertRemove" => {
                self.removed = true;
                Value::Void
            }
            "Sleep" | "TerminalClose" => Value::Void,
            "IsTradeAllowed" => vb(host.algo_allowed()),
            "TerminalInfoInteger" => vi(match arg_i(a, 0, 0) {
                1 => host.algo_allowed() as i64,
                2 | 3 | 11 => 1,
                5 => 100000,
                _ => 0,
            }),
            "TerminalInfoString" => Value::str(match arg_i(a, 0, 0) {
                8 => "English",
                9 => "Rust Terminal",
                10 => "Simulated Broker",
                _ => "",
            }),
            "MQLInfoInteger" => vi(match arg_i(a, 0, 0) {
                1 => host.algo_allowed() as i64,
                2 | 4 => host.is_tester() as i64,
                7 => match self.prog.kind {
                    ProgramKind::Script => 1,
                    ProgramKind::Expert => 2,
                    _ => 3,
                },
                _ => 0,
            }),
            "MQLInfoString" => Value::str(&match arg_i(a, 0, 0) {
                6 => self.name.clone(),
                9 => self.prog.file_name.clone(),
                _ => String::new(),
            }),
            "GlobalVariableSet" => {
                host.global_vars().insert(arg_s(a, 0), arg_f(a, 1, 0.0));
                vi(host.now())
            }
            "GlobalVariableGet" => vd(host.global_vars().get(&arg_s(a, 0)).copied().unwrap_or(0.0)),
            "GlobalVariableCheck" => vb(host.global_vars().contains_key(&arg_s(a, 0))),
            "GlobalVariableDel" => vb(host.global_vars().remove(&arg_s(a, 0)).is_some()),
            _ => match self.call_extra(name, a, host)? {
                Some(v) => v,
                None => return Err(self.err(format!("'{}' - function is not available in this terminal", name))),
            },
        })
    }

    fn ensure_plot(&mut self, i: usize) {
        while self.plots.len() <= i {
            let k = self.plots.len();
            self.plots.push(PlotRt {
                draw_type: 1,
                colors: vec![default_plot_color(k)],
                width: 1,
                style: 0,
                label: format!("Plot {}", k + 1),
                empty_value: f64::MAX,
                draw_begin: 0,
                shift: 0,
                arrow: 159,
            });
        }
    }

    // ---------- object methods ----------

    pub(super) fn call_method(&mut self, obj: &ObjRef, m: &str, args: &[Expr], host: &mut dyn Host) -> R<Value> {
        let a = self.eval_args(args, host)?;
        let kind = obj.borrow().clone();
        match kind {
            ObjVal::Trade { magic, last } => self.trade_method(obj, magic, &last, m, &a, host),
            ObjVal::PosInfo { ticket } => {
                let set = |t: Option<u64>| {
                    if let ObjVal::PosInfo { ticket } = &mut *obj.borrow_mut() {
                        *ticket = t.unwrap_or(0);
                    }
                };
                match m {
                    "SelectByIndex" => {
                        let t = host.account().positions.get(arg_i(&a, 0, 0).max(0) as usize).map(|p| p.ticket);
                        set(t);
                        return Ok(vb(t.is_some()));
                    }
                    "SelectByTicket" => {
                        let t = arg_i(&a, 0, 0) as u64;
                        let ok = host.account().positions.iter().any(|p| p.ticket == t);
                        set(if ok { Some(t) } else { None });
                        return Ok(vb(ok));
                    }
                    "Select" => {
                        let s = arg_s(&a, 0);
                        let t = host.account().positions.iter().find(|p| p.symbol == s).map(|p| p.ticket);
                        set(t);
                        return Ok(vb(t.is_some()));
                    }
                    "SelectByMagic" => {
                        let (s, mg) = (arg_s(&a, 0), arg_i(&a, 1, 0));
                        let t = host.account().positions.iter().find(|p| p.symbol == s && p.magic == mg).map(|p| p.ticket);
                        set(t);
                        return Ok(vb(t.is_some()));
                    }
                    _ => {}
                }
                let p = match self.position(host, Some(ticket)) {
                    Some(p) => p,
                    None => return Ok(vi(0)),
                };
                Ok(match m {
                    "Ticket" | "Identifier" => vi(p.ticket as i64),
                    "Symbol" => Value::str(&p.symbol),
                    "Magic" => vi(p.magic),
                    "PositionType" | "Type" => vi(if p.buy { 0 } else { 1 }),
                    "TypeDescription" => Value::str(if p.buy { "buy" } else { "sell" }),
                    "Volume" => vd(p.volume),
                    "PriceOpen" => vd(p.price_open),
                    "PriceCurrent" => vd(p.price_current),
                    "StopLoss" => vd(p.sl),
                    "TakeProfit" => vd(p.tp),
                    "Profit" => vd(p.profit),
                    "Swap" | "Commission" => vd(0.0),
                    "Time" => vi(p.time),
                    "Comment" => Value::str(&p.comment),
                    _ => return Err(self.err(format!("'{}' - unknown method of CPositionInfo", m))),
                })
            }
            ObjVal::OrdInfo { ticket } => {
                let set = |t: Option<u64>| {
                    if let ObjVal::OrdInfo { ticket } = &mut *obj.borrow_mut() {
                        *ticket = t.unwrap_or(0);
                    }
                };
                match m {
                    "SelectByIndex" => {
                        let t = host.account().orders.get(arg_i(&a, 0, 0).max(0) as usize).map(|o| o.ticket);
                        set(t);
                        return Ok(vb(t.is_some()));
                    }
                    "Select" => {
                        let t = arg_i(&a, 0, 0) as u64;
                        let ok = host.account().orders.iter().any(|o| o.ticket == t);
                        set(if ok { Some(t) } else { None });
                        return Ok(vb(ok));
                    }
                    _ => {}
                }
                let o = match self.order(host, Some(ticket)) {
                    Some(o) => o,
                    None => return Ok(vi(0)),
                };
                Ok(match m {
                    "Ticket" => vi(o.ticket as i64),
                    "OrderType" | "Type" => vi(o.typ as i64),
                    "Symbol" => Value::str(&o.symbol),
                    "Magic" => vi(o.magic),
                    "VolumeInitial" | "VolumeCurrent" => vd(o.volume),
                    "PriceOpen" => vd(o.price),
                    "StopLoss" => vd(o.sl),
                    "TakeProfit" => vd(o.tp),
                    "TimeSetup" => vi(o.time),
                    "Comment" => Value::str(&o.comment),
                    _ => return Err(self.err(format!("'{}' - unknown method of COrderInfo", m))),
                })
            }
            ObjVal::SymInfo { name } => {
                if m == "Name" && !a.is_empty() {
                    let n = arg_s(&a, 0);
                    let ok = host.quote(&n).is_some();
                    *obj.borrow_mut() = ObjVal::SymInfo { name: n };
                    return Ok(vb(ok));
                }
                let q = match host.quote(&name) {
                    Some(q) => q,
                    None => return Ok(vi(0)),
                };
                Ok(match m {
                    "Name" => Value::str(&name),
                    "Refresh" | "RefreshRates" | "Select" => vb(true),
                    "Bid" => vd(q.bid),
                    "Ask" => vd(q.ask),
                    "Point" | "TickSize" => vd(q.spec.point()),
                    "Digits" => vi(q.spec.digits as i64),
                    "Spread" => vi(((q.ask - q.bid) / q.spec.point()).round() as i64),
                    "TickValue" => vd(q.spec.contract_size * q.spec.point()),
                    "LotsMin" => vd(q.spec.volume_min),
                    "LotsMax" => vd(q.spec.volume_max),
                    "LotsStep" => vd(q.spec.volume_step),
                    "ContractSize" => vd(q.spec.contract_size),
                    "StopsLevel" | "FreezeLevel" => vi(0),
                    "Description" => Value::str(q.spec.description),
                    "NormalizePrice" => {
                        let mm = 10f64.powi(q.spec.digits as i32);
                        vd((arg_f(&a, 0, 0.0) * mm).round() / mm)
                    }
                    _ => return Err(self.err(format!("'{}' - unknown method of CSymbolInfo", m))),
                })
            }
            ObjVal::AccInfo => {
                let acc: &Account = host.account();
                let (bal, eq, fl, login, lev, nm, srv, cur) =
                    (acc.balance, acc.equity(), acc.floating(), acc.login, acc.leverage, acc.name.clone(), acc.server.clone(), acc.currency.clone());
                let mg = host.margin();
                Ok(match m {
                    "Balance" => vd(bal),
                    "Equity" => vd(eq),
                    "Profit" => vd(fl),
                    "Margin" => vd(mg),
                    "FreeMargin" => vd(eq - mg),
                    "MarginLevel" => vd(if mg > 0.0 { eq / mg * 100.0 } else { 0.0 }),
                    "Credit" => vd(0.0),
                    "Login" => vi(login as i64),
                    "Leverage" => vi(lev as i64),
                    "Name" => Value::str(&nm),
                    "Server" => Value::str(&srv),
                    "Currency" => Value::str(&cur),
                    "Company" => Value::str("Simulated Broker"),
                    "TradeAllowed" | "TradeExpert" => vb(true),
                    _ => return Err(self.err(format!("'{}' - unknown method of CAccountInfo", m))),
                })
            }
        }
    }

    fn trade_method(&mut self, obj: &ObjRef, magic: i64, last: &TradeInfo, m: &str, a: &[Value], host: &mut dyn Host) -> R<Value> {
        let sym_or = |v: Option<&Value>, me: &Instance| -> String {
            match v {
                Some(Value::Str(s)) if !s.is_empty() => s.to_string(),
                _ => me.symbol.clone(),
            }
        };
        let find_ticket = |v: Option<&Value>, host: &dyn Host| -> u64 {
            match v {
                Some(Value::Str(s)) => {
                    let s = s.to_string();
                    host.account().positions.iter().find(|p| p.symbol == s).map(|p| p.ticket).unwrap_or(0)
                }
                Some(v) => v.as_i64() as u64,
                None => 0,
            }
        };
        let op = match m {
            "SetExpertMagicNumber" => {
                if let ObjVal::Trade { magic, .. } = &mut *obj.borrow_mut() {
                    *magic = arg_i(a, 0, 0);
                }
                return Ok(Value::Void);
            }
            "SetDeviationInPoints" | "SetTypeFilling" | "SetTypeFillingBySymbol" | "SetAsyncMode" | "SetMarginMode" | "LogLevel" => return Ok(Value::Void),
            "ResultRetcode" => return Ok(vi(last.retcode)),
            "ResultRetcodeDescription" | "ResultComment" => return Ok(Value::str(&last.comment)),
            "ResultOrder" => return Ok(vi(last.order as i64)),
            "ResultDeal" => return Ok(vi(last.deal as i64)),
            "ResultPrice" => return Ok(vd(last.price)),
            "ResultVolume" => return Ok(vd(last.volume)),
            "RequestMagic" => return Ok(vi(magic)),
            "Buy" | "Sell" => TradeOp::Market {
                symbol: sym_or(a.get(1), self),
                buy: m == "Buy",
                volume: arg_f(a, 0, 0.0),
                sl: arg_f(a, 3, 0.0),
                tp: arg_f(a, 4, 0.0),
                magic,
                comment: arg_s(a, 5),
            },
            "PositionOpen" => TradeOp::Market {
                symbol: sym_or(a.first(), self),
                buy: arg_i(a, 1, 0) == 0,
                volume: arg_f(a, 2, 0.0),
                sl: arg_f(a, 4, 0.0),
                tp: arg_f(a, 5, 0.0),
                magic,
                comment: arg_s(a, 6),
            },
            "PositionClose" => TradeOp::Close { ticket: find_ticket(a.first(), host), volume: None },
            "PositionClosePartial" => TradeOp::Close { ticket: find_ticket(a.first(), host), volume: Some(arg_f(a, 1, 0.0)) },
            "PositionModify" => TradeOp::Modify { ticket: find_ticket(a.first(), host), sl: arg_f(a, 1, 0.0), tp: arg_f(a, 2, 0.0) },
            "BuyLimit" | "SellLimit" | "BuyStop" | "SellStop" => TradeOp::Pending {
                symbol: sym_or(a.get(2), self),
                typ: match m {
                    "BuyLimit" => 2,
                    "SellLimit" => 3,
                    "BuyStop" => 4,
                    _ => 5,
                },
                volume: arg_f(a, 0, 0.0),
                price: arg_f(a, 1, 0.0),
                sl: arg_f(a, 3, 0.0),
                tp: arg_f(a, 4, 0.0),
                magic,
                comment: arg_s(a, 7),
            },
            "OrderOpen" => TradeOp::Pending {
                symbol: sym_or(a.first(), self),
                typ: arg_i(a, 1, 2),
                volume: arg_f(a, 2, 0.0),
                price: arg_f(a, 4, 0.0),
                sl: arg_f(a, 5, 0.0),
                tp: arg_f(a, 6, 0.0),
                magic,
                comment: arg_s(a, 9),
            },
            "OrderModify" => TradeOp::ModifyOrder { ticket: arg_i(a, 0, 0) as u64, price: arg_f(a, 1, 0.0), sl: arg_f(a, 2, 0.0), tp: arg_f(a, 3, 0.0) },
            "OrderDelete" => TradeOp::Delete { ticket: arg_i(a, 0, 0) as u64 },
            _ => return Err(self.err(format!("'{}' - unknown method of CTrade", m))),
        };
        let r = self.do_trade(host, op);
        if let ObjVal::Trade { last, .. } = &mut *obj.borrow_mut() {
            *last = TradeInfo { retcode: r.retcode, order: r.order, deal: r.deal, price: r.price, volume: r.volume, comment: r.comment.clone() };
        }
        Ok(vb(r.retcode == RET_DONE))
    }
}

fn pos_prop(p: &Position, prop: i64, f: &str) -> Value {
    match f {
        "PositionGetString" => Value::str(match prop {
            13 => &p.symbol,
            14 => &p.comment,
            _ => "",
        }),
        "PositionGetDouble" => vd(match prop {
            6 => p.volume,
            7 => p.price_open,
            8 => p.sl,
            9 => p.tp,
            10 => p.price_current,
            11 => p.profit,
            _ => 0.0,
        }),
        _ => vi(match prop {
            1 | 5 => p.ticket as i64,
            2 => p.time,
            3 => {
                if p.buy {
                    0
                } else {
                    1
                }
            }
            4 => p.magic,
            15 => p.time * 1000,
            _ => 0,
        }),
    }
}

fn order_prop(o: &PendingOrder, prop: i64, f: &str, cur: f64) -> Value {
    match f {
        "OrderGetString" => Value::str(match prop {
            10 => &o.symbol,
            11 => &o.comment,
            _ => "",
        }),
        "OrderGetDouble" => vd(match prop {
            5 | 6 => o.volume,
            7 => o.price,
            8 => o.sl,
            9 => o.tp,
            12 => cur,
            _ => 0.0,
        }),
        _ => vi(match prop {
            1 => o.ticket as i64,
            2 => o.typ as i64,
            3 => o.magic,
            4 => o.time,
            13 => 1,
            _ => 0,
        }),
    }
}

fn deal_prop(d: &Deal, prop: i64, f: &str) -> Value {
    match f {
        "HistoryDealGetString" => Value::str(match prop {
            11 => &d.symbol,
            12 => &d.comment,
            _ => "",
        }),
        "HistoryDealGetDouble" => vd(match prop {
            8 => d.volume,
            9 => d.price,
            10 => d.profit,
            _ => 0.0,
        }),
        _ => vi(match prop {
            1 => d.ticket as i64,
            2 => d.order as i64,
            3 => d.time,
            4 => match d.kind.as_str() {
                "buy" => 0,
                "sell" => 1,
                _ => 2,
            },
            5 => (d.entry == "out") as i64,
            6 => d.magic,
            7 => d.position as i64,
            _ => 0,
        }),
    }
}

fn set_obj_prop(o: &mut ChartObj, prop: i64, modifier: i64, v: &Value) {
    let m = modifier.clamp(0, 2) as usize;
    match prop {
        0 => o.color = v.as_i64() as u32,
        1 => o.style = v.as_i64(),
        2 => o.width = v.as_i64(),
        3 => o.back = v.truthy(),
        4 => o.selectable = v.truthy(),
        6 => o.hidden = v.truthy(),
        7 => o.times[m] = v.as_i64(),
        8 => o.prices[m] = v.as_f64(),
        9 => o.text = v.to_text(),
        11 => o.font_size = v.as_i64(),
        12 => o.xdist = v.as_i64(),
        13 => o.ydist = v.as_i64(),
        14 => o.corner = v.as_i64(),
        15 => o.anchor = v.as_i64(),
        16 => o.arrow_code = v.as_i64(),
        17 => o.ray_right = v.truthy(),
        19 => o.xsize = v.as_i64(),
        20 => o.ysize = v.as_i64(),
        21 => o.bgcolor = v.as_i64() as u32,
        24 => o.state = v.truthy(),
        25 => o.tooltip = v.to_text(),
        _ => {}
    }
}

fn get_obj_prop(o: &ChartObj, prop: i64, modifier: i64) -> Value {
    let m = modifier.clamp(0, 2) as usize;
    match prop {
        0 => vi(o.color as i64),
        1 => vi(o.style),
        2 => vi(o.width),
        3 => vb(o.back),
        7 => vi(o.times[m]),
        8 => vd(o.prices[m]),
        9 => Value::str(&o.text),
        11 => vi(o.font_size),
        12 => vi(o.xdist),
        13 => vi(o.ydist),
        14 => vi(o.corner),
        16 => vi(o.arrow_code),
        19 => vi(o.xsize),
        20 => vi(o.ysize),
        21 => vi(o.bgcolor as i64),
        24 => vb(o.state),
        26 => vi(o.kind.mql()),
        27 => Value::str(&o.name),
        _ => vi(0),
    }
}

/// printf-style formatting used by PrintFormat and StringFormat.
pub fn printf(fmt: &str, args: &[Value]) -> String {
    let mut out = String::new();
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    let mut ai = 0;
    while i < chars.len() {
        let c = chars[i];
        if c != '%' {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        if i < chars.len() && chars[i] == '%' {
            out.push('%');
            i += 1;
            continue;
        }
        let mut flags = String::new();
        while i < chars.len() && "-+ 0#".contains(chars[i]) {
            flags.push(chars[i]);
            i += 1;
        }
        let mut width = String::new();
        while i < chars.len() && chars[i].is_ascii_digit() {
            width.push(chars[i]);
            i += 1;
        }
        let mut prec: Option<usize> = None;
        if i < chars.len() && chars[i] == '.' {
            i += 1;
            let mut p = String::new();
            while i < chars.len() && chars[i].is_ascii_digit() {
                p.push(chars[i]);
                i += 1;
            }
            prec = Some(p.parse().unwrap_or(0));
        }
        while i < chars.len() && "hlLqIw".contains(chars[i]) {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let conv = chars[i];
        i += 1;
        let arg = args.get(ai).cloned().unwrap_or(Value::Int(0));
        ai += 1;
        let mut s = match conv {
            'd' | 'i' => {
                let v = arg.as_i64();
                if flags.contains('+') && v >= 0 {
                    format!("+{}", v)
                } else {
                    v.to_string()
                }
            }
            'u' => (arg.as_i64() as u64).to_string(),
            'x' => format!("{:x}", arg.as_i64()),
            'X' => format!("{:X}", arg.as_i64()),
            'o' => format!("{:o}", arg.as_i64()),
            'c' => char::from_u32(arg.as_i64() as u32).map(|c| c.to_string()).unwrap_or_default(),
            'f' | 'F' => {
                let v = arg.as_f64();
                let p = prec.unwrap_or(6);
                if flags.contains('+') && v >= 0.0 {
                    format!("+{:.*}", p, v)
                } else {
                    format!("{:.*}", p, v)
                }
            }
            'e' | 'E' => {
                let s = format!("{:.*e}", prec.unwrap_or(6), arg.as_f64());
                if conv == 'E' {
                    s.to_uppercase()
                } else {
                    s
                }
            }
            'g' | 'G' => fmt_double(arg.as_f64()),
            's' => {
                let t = arg.to_text();
                match prec {
                    Some(p) => t.chars().take(p).collect(),
                    None => t,
                }
            }
            other => format!("%{}", other),
        };
        if let Ok(w) = width.parse::<usize>() {
            let len = s.chars().count();
            if len < w {
                let pad = w - len;
                if flags.contains('-') {
                    s.push_str(&" ".repeat(pad));
                } else if flags.contains('0') && !matches!(conv, 's' | 'c') {
                    let (sign, rest) = if s.starts_with('-') || s.starts_with('+') { s.split_at(1) } else { ("", s.as_str()) };
                    s = format!("{}{}{}", sign, "0".repeat(pad), rest);
                } else {
                    s = format!("{}{}", " ".repeat(pad), s);
                }
            }
        }
        out.push_str(&s);
    }
    out
}
