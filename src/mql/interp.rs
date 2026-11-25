//! Tree-walking interpreter: one `Instance` per running indicator, expert or script.

use super::ast::*;
use super::builtins;
use crate::account::{Account, Quote, TradeResult};
use crate::indicators::{IndKind, IndParams};
use crate::market::{Bar, Timeframe};
use crate::objects::ChartObjects;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub type ArrRef = Rc<RefCell<Array>>;
pub type StructRef = Rc<RefCell<StructVal>>;
pub type ObjRef = Rc<RefCell<ObjVal>>;
pub type VarRef = Rc<RefCell<Var>>;

#[derive(Clone, Debug)]
pub enum Value {
    Void,
    Int(i64),
    Double(f64),
    Bool(bool),
    Str(Rc<str>),
    Arr(ArrRef),
    Struct(StructRef),
    Obj(ObjRef),
}

#[derive(Debug)]
pub struct Array {
    pub data: Vec<Value>,
    pub elem: Ty,
    pub series: bool,
    /// inner dimensions for multi-dimensional arrays
    pub inner: Vec<usize>,
    pub dynamic: bool,
}

#[derive(Debug, Clone)]
pub struct StructVal {
    pub ty: Sym,
    pub fields: Vec<(Sym, Ty, Value)>,
}

#[derive(Debug, Clone, Default)]
pub struct TradeInfo {
    pub retcode: i64,
    pub order: u64,
    pub deal: u64,
    pub price: f64,
    pub volume: f64,
    pub comment: String,
}

#[derive(Debug, Clone)]
pub enum ObjVal {
    Trade { magic: i64, last: TradeInfo },
    PosInfo { ticket: u64 },
    SymInfo { name: String },
    AccInfo,
    OrdInfo { ticket: u64 },
}

#[derive(Debug)]
pub struct Var {
    pub ty: Ty,
    pub v: Value,
}

impl Value {
    pub fn as_f64(&self) -> f64 {
        match self {
            Value::Struct(s) => Rc::as_ptr(s) as usize as f64,
            Value::Int(i) => *i as f64,
            Value::Double(d) => *d,
            Value::Bool(b) => *b as i64 as f64,
            Value::Str(s) => s.trim().parse().unwrap_or(0.0),
            _ => 0.0,
        }
    }
    pub fn as_i64(&self) -> i64 {
        match self {
            Value::Int(i) => *i,
            Value::Double(d) => {
                if d.is_finite() {
                    *d as i64
                } else {
                    0
                }
            }
            Value::Bool(b) => *b as i64,
            Value::Struct(s) => Rc::as_ptr(s) as usize as i64,
            Value::Obj(o) => Rc::as_ptr(o) as usize as i64,
            Value::Str(s) => s.trim().parse::<i64>().or_else(|_| s.trim().parse::<f64>().map(|f| f as i64)).unwrap_or(0),
            _ => 0,
        }
    }
    pub fn truthy(&self) -> bool {
        match self {
            Value::Int(i) => *i != 0,
            Value::Double(d) => *d != 0.0,
            Value::Bool(b) => *b,
            Value::Str(s) => !s.is_empty(),
            Value::Void => false,
            _ => true,
        }
    }
    pub fn to_text(&self) -> String {
        match self {
            Value::Void => String::new(),
            Value::Int(i) => i.to_string(),
            Value::Double(d) => fmt_double(*d),
            Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            Value::Str(s) => s.to_string(),
            Value::Arr(a) => format!("array[{}]", a.borrow().data.len()),
            Value::Struct(_) => "struct".to_string(),
            Value::Obj(_) => "object".to_string(),
        }
    }
    pub fn str(s: &str) -> Value {
        Value::Str(Rc::from(s))
    }
}

pub fn fmt_double(d: f64) -> String {
    if d == f64::MAX {
        return "1.797693134862316e+308".into();
    }
    if d.is_finite() && d == d.trunc() && d.abs() < 1e15 {
        return format!("{:.1}", d).trim_end_matches(".0").to_string();
    }
    let s = format!("{:.8}", d);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" {
        "0".into()
    } else {
        s
    }
}

#[derive(Debug)]
pub struct RtError {
    pub msg: String,
    pub line: u32,
    pub col: u32,
    /// ExpertRemove() was called: not an error
    pub removed: bool,
}

pub enum Flow {
    Normal,
    Break,
    Continue,
    Return(Value),
}

type R<T> = Result<T, RtError>;

/// A trading request a program sends through its host.
pub enum TradeOp {
    Market { symbol: String, buy: bool, volume: f64, sl: f64, tp: f64, magic: i64, comment: String },
    Close { ticket: u64, volume: Option<f64> },
    Modify { ticket: u64, sl: f64, tp: f64 },
    Pending { symbol: String, typ: i64, volume: f64, price: f64, sl: f64, tp: f64, magic: i64, comment: String },
    Delete { ticket: u64 },
    ModifyOrder { ticket: u64, price: f64, sl: f64, tp: f64 },
}

/// What the terminal (or the strategy tester) provides to a running program.
pub trait Host {
    fn quote(&self, symbol: &str) -> Option<Quote>;
    fn bars(&mut self, symbol: &str, tf: Timeframe) -> Option<&[Bar]>;
    fn now(&self) -> i64;
    fn account(&self) -> &Account;
    fn margin(&self) -> f64;
    fn trade(&mut self, op: TradeOp) -> TradeResult;
    fn print(&mut self, msg: &str);
    fn alert(&mut self, msg: &str);
    fn set_comment(&mut self, text: &str);
    fn objects(&mut self) -> &mut ChartObjects;
    fn algo_allowed(&self) -> bool;
    fn load_program(&mut self, name: &str) -> Result<Rc<Program>, String>;
    fn is_tester(&self) -> bool;
    fn symbol_names(&self) -> Vec<String>;
    fn global_vars(&mut self) -> &mut HashMap<String, f64>;
    /// Sandbox folder for FileOpen.
    fn files_dir(&self) -> std::path::PathBuf {
        crate::storage::mql_dir().join("Files")
    }
    /// Folder for FILE_COMMON.
    fn common_files_dir(&self) -> std::path::PathBuf {
        crate::storage::data_dir().join("Common").join("Files")
    }
}

#[derive(Clone, Debug)]
pub struct PlotRt {
    pub draw_type: i64,
    pub colors: Vec<u32>,
    pub width: i64,
    pub style: i64,
    pub label: String,
    pub empty_value: f64,
    pub draw_begin: i64,
    pub shift: i64,
    pub arrow: i64,
}

pub enum Handle {
    Builtin { kind: IndKind, params: IndParams, symbol: String, tf: Timeframe },
    Ext { kind: crate::indicators_ext::ExtKind, params: crate::indicators_ext::ExtParams, symbol: String, tf: Timeframe },
    Custom { inst: Option<Box<Instance>>, symbol: String, tf: Timeframe },
}

pub struct PriceArrays {
    pub time: ArrRef,
    pub open: ArrRef,
    pub high: ArrRef,
    pub low: ArrRef,
    pub close: ArrRef,
    pub tick_volume: ArrRef,
    pub volume: ArrRef,
    pub spread: ArrRef,
    pub price: ArrRef,
}

pub fn new_array(elem: Ty, data: Vec<Value>) -> ArrRef {
    Rc::new(RefCell::new(Array { data, elem, series: false, inner: vec![], dynamic: true }))
}

pub struct Instance {
    pub prog: Rc<Program>,
    pub name: String,
    pub symbol: String,
    pub tf: Timeframe,
    globals: HashMap<Sym, VarRef>,
    statics: HashMap<u32, VarRef>,
    frames: Vec<Vec<Vec<(Sym, VarRef)>>>,
    /// object of the running method (None inside plain functions)
    this_stack: Vec<Option<StructRef>>,
    pub buffers: Vec<Option<(ArrRef, i64)>>,
    pub plots: Vec<PlotRt>,
    pub short_name: String,
    pub digits: Option<i64>,
    pub levels: Vec<f64>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub handles: Vec<Option<Handle>>,
    prices: Option<PriceArrays>,
    pub prev_calculated: i64,
    last_rates_total: usize,
    last_first_time: i64,
    pub timer_ms: u64,
    pub removed: bool,
    pub last_error: i64,
    steps: u64,
    pub step_limit: u64,
    depth: usize,
    cur_line: u32,
    cur_col: u32,
    pub cur_func: String,
    selected_position: Option<u64>,
    selected_order: Option<u64>,
    rand_state: u64,
    pub inputs_text: Vec<(String, String)>,
    pub initialized: bool,
    files: HashMap<i64, extra::FileH>,
    next_file: i64,
}

impl Instance {
    pub fn new(prog: Rc<Program>, symbol: &str, tf: Timeframe, input_overrides: &HashMap<String, String>) -> Self {
        let name = prog.file_name.trim_end_matches(".mq5").to_string();
        let mut plots = vec![];
        let nplots = prog.props.plots.max(if prog.kind == ProgramKind::Indicator { prog.props.buffers.min(8) } else { 0 });
        for i in 0..nplots {
            let pp = prog.props.plot.get(&(i + 1)).cloned().unwrap_or_default();
            plots.push(PlotRt {
                draw_type: pp.draw_type.unwrap_or(if i < prog.props.plots { 1 } else { 0 }),
                colors: if pp.colors.is_empty() { vec![default_plot_color(i)] } else { pp.colors.clone() },
                width: pp.width.unwrap_or(1),
                style: pp.style.unwrap_or(0),
                label: pp.label.clone().unwrap_or_else(|| format!("Plot {}", i + 1)),
                empty_value: f64::MAX,
                draw_begin: 0,
                shift: 0,
                arrow: 159,
            });
        }
        let mut inst = Instance {
            name: name.clone(),
            symbol: symbol.to_string(),
            tf,
            globals: HashMap::new(),
            statics: HashMap::new(),
            frames: vec![],
            this_stack: vec![],
            buffers: vec![None; prog.props.buffers.max(0)],
            plots,
            short_name: name,
            digits: None,
            levels: prog.props.levels.clone(),
            minimum: prog.props.minimum,
            maximum: prog.props.maximum,
            handles: vec![],
            prices: None,
            prev_calculated: 0,
            last_rates_total: 0,
            last_first_time: 0,
            timer_ms: 0,
            removed: false,
            last_error: 0,
            steps: 0,
            step_limit: 30_000_000,
            depth: 0,
            cur_line: 0,
            cur_col: 0,
            cur_func: String::new(),
            selected_position: None,
            selected_order: None,
            rand_state: 1,
            inputs_text: vec![],
            initialized: false,
            files: HashMap::new(),
            next_file: 1,
            prog,
        };
        inst.inputs_text = inst
            .prog
            .inputs
            .iter()
            .map(|i| {
                let n = inst.prog.interner.name(i.name).to_string();
                let v = input_overrides.get(&n).cloned().unwrap_or_default();
                (n, v)
            })
            .collect();
        inst
    }

    pub fn sym(&self, s: Sym) -> &str {
        self.prog.interner.name(s)
    }

    fn err(&self, msg: impl Into<String>) -> RtError {
        RtError { msg: msg.into(), line: self.cur_line, col: self.cur_col, removed: false }
    }

    pub fn fmt_err(&self, e: &RtError) -> String {
        if e.line > 0 {
            format!("{} in '{}' ({},{})", e.msg, self.prog.file_name, e.line, e.col)
        } else {
            e.msg.clone()
        }
    }

    // ---------- values ----------

    pub fn default_value(&self, ty: &Ty) -> Value {
        match ty {
            Ty::Void => Value::Void,
            Ty::Bool => Value::Bool(false),
            Ty::Int => Value::Int(0),
            Ty::Double => Value::Double(0.0),
            Ty::Str => Value::str(""),
            Ty::Struct(s) => self.new_struct(*s),
            Ty::Object(s) => {
                let o = match self.sym(*s) {
                    "CTrade" => ObjVal::Trade { magic: 0, last: TradeInfo::default() },
                    "CPositionInfo" => ObjVal::PosInfo { ticket: 0 },
                    "CSymbolInfo" => ObjVal::SymInfo { name: self.symbol.clone() },
                    "COrderInfo" => ObjVal::OrdInfo { ticket: 0 },
                    _ => ObjVal::AccInfo,
                };
                Value::Obj(Rc::new(RefCell::new(o)))
            }
            Ty::Ptr(_) => Value::Int(0),
            Ty::Any => Value::Void,
        }
    }

    pub fn new_struct(&self, s: Sym) -> Value {
        let def = match self.prog.structs.get(&s) {
            Some(d) => d.clone(),
            None => return Value::Void,
        };
        let fields = def
            .fields
            .iter()
            .map(|(n, t, dims)| {
                let v = if dims.is_empty() {
                    self.default_value(t)
                } else {
                    let n0 = dims.first().and_then(|d| d.as_ref()).map(|e| const_dim(e)).unwrap_or(0);
                    let data = (0..n0).map(|_| self.default_value(t)).collect();
                    Value::Arr(Rc::new(RefCell::new(Array { data, elem: t.clone(), series: false, inner: vec![], dynamic: dims[0].is_none() })))
                };
                (*n, t.clone(), v)
            })
            .collect();
        Value::Struct(Rc::new(RefCell::new(StructVal { ty: s, fields })))
    }

    pub fn coerce(&self, v: Value, ty: &Ty) -> Value {
        match (ty, v) {
            (_, Value::Arr(a)) => Value::Arr(a),
            (Ty::Int, v) => Value::Int(v.as_i64()),
            (Ty::Double, v) => Value::Double(v.as_f64()),
            (Ty::Bool, v) => Value::Bool(v.truthy()),
            (Ty::Str, Value::Str(s)) => Value::Str(s),
            (Ty::Str, v) => Value::str(&v.to_text()),
            (Ty::Struct(_), Value::Struct(s)) => Value::Struct(Rc::new(RefCell::new(deep_copy_struct(&s.borrow())))),
            (Ty::Struct(s), _) => self.new_struct(*s),
            (Ty::Object(_), v @ Value::Obj(_)) => v,
            (Ty::Ptr(_), v @ (Value::Struct(_) | Value::Void)) => v,
            (Ty::Ptr(_), v) => Value::Int(v.as_i64()),
            (Ty::Any, v) => v,
            (Ty::Void, _) => Value::Void,
            (_, v) => v,
        }
    }

    // ---------- variables ----------

    fn lookup(&self, s: Sym) -> Option<VarRef> {
        if let Some(frame) = self.frames.last() {
            for scope in frame.iter().rev() {
                for (n, v) in scope.iter().rev() {
                    if *n == s {
                        return Some(v.clone());
                    }
                }
            }
        }
        if self.this_has_field(s) {
            return None; // a member of the current object hides globals
        }
        self.globals.get(&s).cloned()
    }

    fn cur_this(&self) -> Option<StructRef> {
        self.this_stack.last().cloned().flatten()
    }

    fn this_has_field(&self, s: Sym) -> bool {
        match self.this_stack.last() {
            Some(Some(t)) => t.borrow().fields.iter().any(|f| f.0 == s),
            _ => false,
        }
    }

    fn this_field(&self, s: Sym) -> Option<Value> {
        let t = self.cur_this()?;
        let tb = t.borrow();
        tb.fields.iter().find(|f| f.0 == s).map(|f| f.2.clone())
    }

    fn set_this_field(&self, s: Sym, v: Value) -> bool {
        let t = match self.cur_this() {
            Some(t) => t,
            None => return false,
        };
        let ty = t.borrow().fields.iter().find(|f| f.0 == s).map(|f| f.1.clone());
        match ty {
            Some(ty) => {
                let nv = if matches!(v, Value::Arr(_)) { v } else { self.coerce(v, &ty) };
                if let Some(f) = t.borrow_mut().fields.iter_mut().find(|f| f.0 == s) {
                    f.2 = nv;
                }
                true
            }
            None => false,
        }
    }

    // ---------- objects ----------

    /// Create an object of a user class (or built-in class) and run its constructors.
    pub fn construct(&mut self, cls: Sym, args: &[Expr], host: &mut dyn Host) -> R<Value> {
        if !self.prog.structs.contains_key(&cls) {
            if builtins::BUILTIN_CLASSES.contains(&self.sym(cls)) {
                return Ok(self.default_value(&Ty::Object(cls)));
            }
            return Err(self.err(format!("'{}' - unknown class", self.sym(cls))));
        }
        let argv = args.iter().map(|a| self.eval(a, host)).collect::<R<Vec<_>>>()?;
        let obj = match self.new_struct(cls) {
            Value::Struct(s) => s,
            _ => return Err(self.err("cannot create object")),
        };
        self.run_ctor(cls, &obj, argv, host)?;
        Ok(Value::Struct(obj))
    }

    fn run_ctor(&mut self, cls: Sym, obj: &StructRef, args: Vec<Value>, host: &mut dyn Host) -> R<()> {
        let prog = self.prog.clone();
        let info = match prog.classes.get(&cls) {
            Some(i) => i,
            None => return Ok(()),
        };
        let ctor = info.methods.get(&cls).and_then(|fs| {
            fs.iter()
                .find(|f| {
                    let req = f.params.iter().filter(|p| p.default.is_none()).count();
                    args.len() >= req && args.len() <= f.params.len()
                })
                .cloned()
        });
        if self.depth > 200 {
            return Err(self.err("stack overflow"));
        }
        let mut scope = vec![];
        if let Some(f) = &ctor {
            for (i, p) in f.params.iter().enumerate() {
                let v = match args.get(i) {
                    Some(v) => v.clone(),
                    None => match &p.default {
                        Some(e) => self.eval(e, host)?,
                        None => self.default_value(&p.ty),
                    },
                };
                let v = if p.is_array || p.by_ref { v } else { self.coerce(v, &p.ty) };
                scope.push((p.name, Rc::new(RefCell::new(Var { ty: p.ty.clone(), v }))));
            }
        }
        self.depth += 1;
        self.frames.push(vec![scope]);
        self.this_stack.push(Some(obj.clone()));
        let r = self.ctor_inner(info, ctor.as_ref(), obj, host);
        self.this_stack.pop();
        self.frames.pop();
        self.depth -= 1;
        r
    }

    fn ctor_inner(&mut self, info: &ClassInfo, ctor: Option<&Rc<Func>>, obj: &StructRef, host: &mut dyn Host) -> R<()> {
        let empty = vec![];
        let init_list = ctor.map(|f| &f.init_list).unwrap_or(&empty);
        if let Some(b) = info.base {
            let bargs = match init_list.iter().find(|(n, _)| *n == b) {
                Some((_, a)) => a.iter().map(|e| self.eval(e, host)).collect::<R<Vec<_>>>()?,
                None => vec![],
            };
            self.run_ctor(b, obj, bargs, host)?;
        }
        for fsym in &info.own_fields {
            let (fty, fval) = match obj.borrow().fields.iter().find(|f| f.0 == *fsym) {
                Some(f) => (f.1.clone(), f.2.clone()),
                None => continue,
            };
            let init = init_list.iter().find(|(n, _)| n == fsym);
            match (init, &fty, &fval) {
                (Some((_, a)), Ty::Struct(c), Value::Struct(sub)) if self.prog.classes.contains_key(c) => {
                    let vals = a.iter().map(|e| self.eval(e, host)).collect::<R<Vec<_>>>()?;
                    if vals.len() == 1 && matches!(vals[0], Value::Struct(_)) && self.prog.classes.get(c).is_some_and(|ci| !ci.methods.contains_key(c)) {
                        self.set_this_field(*fsym, vals[0].clone());
                    } else {
                        let sub = sub.clone();
                        self.run_ctor(*c, &sub, vals, host)?;
                    }
                }
                (Some((_, a)), _, _) => {
                    let v = match a.first() {
                        Some(e) => self.eval(e, host)?,
                        None => self.default_value(&fty),
                    };
                    self.set_this_field(*fsym, v);
                }
                (None, Ty::Struct(c), Value::Struct(sub)) if self.prog.classes.contains_key(c) => {
                    let sub = sub.clone();
                    self.run_ctor(*c, &sub, vec![], host)?;
                }
                _ => {}
            }
        }
        if let Some(f) = ctor {
            for s in f.body.iter() {
                if let Flow::Return(_) = self.exec(s, host)? {
                    break;
                }
            }
        }
        Ok(())
    }

    /// `delete p`: run destructors from the most derived class up.
    fn destroy(&mut self, obj: &StructRef, host: &mut dyn Host) -> R<()> {
        let prog = self.prog.clone();
        let mut c = Some(obj.borrow().ty);
        let mut guard = 0;
        while let Some(cls) = c {
            let info = match prog.classes.get(&cls) {
                Some(i) => i,
                None => break,
            };
            let dname = format!("~{}", prog.interner.name(cls));
            if let Some(f) = prog.interner.get(&dname).and_then(|d| info.methods.get(&d)).and_then(|v| v.first()) {
                self.call_values_this(f, vec![], Some(obj.clone()), host)?;
            }
            c = info.base;
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        Ok(())
    }

    fn declare(&mut self, s: Sym, var: VarRef) {
        if let Some(frame) = self.frames.last_mut() {
            if let Some(scope) = frame.last_mut() {
                scope.push((s, var));
                return;
            }
        }
        self.globals.insert(s, var);
    }

    fn make_var(&mut self, d: &VarDecl, host: &mut dyn Host) -> R<VarRef> {
        let v = if !d.dims.is_empty() {
            let mut sizes = vec![];
            for dim in &d.dims {
                sizes.push(match dim {
                    Some(e) => Some(self.eval(e, host)?.as_i64().max(0) as usize),
                    None => None,
                });
            }
            let arr = self.build_array(&d.ty, &sizes);
            if let Some(Expr::List(items)) = &d.init {
                let vals: Vec<Value> = items.iter().map(|e| self.eval(e, host)).collect::<R<Vec<_>>>()?;
                fill_array(self, &arr, vals, &d.ty);
            }
            Value::Arr(arr)
        } else {
            match &d.init {
                Some(Expr::List(items)) => {
                    // struct initializer
                    let v = self.default_value(&d.ty);
                    if let Value::Struct(s) = &v {
                        let vals: Vec<Value> = items.iter().map(|e| self.eval(e, host)).collect::<R<Vec<_>>>()?;
                        let mut sb = s.borrow_mut();
                        for (i, val) in vals.into_iter().enumerate() {
                            if i < sb.fields.len() {
                                let t = sb.fields[i].1.clone();
                                sb.fields[i].2 = self.coerce(val, &t);
                            }
                        }
                    }
                    v
                }
                Some(e @ Expr::New(..)) if matches!(d.ty, Ty::Struct(_)) => self.eval(e, host)?,
                Some(e) => {
                    let v = self.eval(e, host)?;
                    self.coerce(v, &d.ty)
                }
                None => match &d.ty {
                    Ty::Struct(c) if self.prog.classes.contains_key(c) => self.construct(*c, &[], host)?,
                    _ => self.default_value(&d.ty),
                },
            }
        };
        Ok(Rc::new(RefCell::new(Var { ty: d.ty.clone(), v })))
    }

    fn build_array(&self, elem: &Ty, sizes: &[Option<usize>]) -> ArrRef {
        let n0 = sizes.first().copied().flatten().unwrap_or(0);
        let inner: Vec<usize> = sizes.iter().skip(1).map(|s| s.unwrap_or(0)).collect();
        let data = (0..n0).map(|_| self.array_elem_default(elem, &inner)).collect();
        Rc::new(RefCell::new(Array { data, elem: elem.clone(), series: false, inner, dynamic: sizes.first().is_some_and(|s| s.is_none()) }))
    }

    pub fn array_elem_default(&self, elem: &Ty, inner: &[usize]) -> Value {
        if inner.is_empty() {
            self.default_value(elem)
        } else {
            let sizes: Vec<Option<usize>> = inner.iter().map(|x| Some(*x)).collect();
            Value::Arr(self.build_array(elem, &sizes))
        }
    }

    // ---------- program entry points ----------

    /// Initialise globals and call OnInit. Returns OnInit's result (0 = success).
    pub fn init(&mut self, host: &mut dyn Host) -> R<i64> {
        self.steps = 0;
        let prog = self.prog.clone();
        let overrides: HashMap<String, String> = self.inputs_text.iter().cloned().collect();
        for (gi, g) in prog.globals.iter().enumerate() {
            self.cur_line = g.line;
            let var = self.make_var(g, host)?;
            if let Some(inp) = prog.inputs.iter().find(|i| i.decl_index == gi) {
                let n = prog.interner.name(inp.name);
                if let Some(text) = overrides.get(n) {
                    if !text.is_empty() {
                        let v = parse_input_value(text, &inp.ty, inp.enum_name.as_deref(), &prog);
                        var.borrow_mut().v = self.coerce(v, &inp.ty);
                    }
                }
            }
            self.globals.insert(g.name, var);
        }
        // record final input values
        self.inputs_text = prog
            .inputs
            .iter()
            .map(|i| {
                let n = prog.interner.name(i.name).to_string();
                let v = self.globals.get(&i.name).map(|v| v.borrow().v.to_text()).unwrap_or_default();
                (n, v)
            })
            .collect();
        self.initialized = true;
        match self.call_event("OnInit", vec![], host)? {
            Some(v) => Ok(v.as_i64()),
            None => Ok(0),
        }
    }

    pub fn has_func(&self, name: &str) -> bool {
        self.prog.interner.get(name).is_some_and(|s| self.prog.funcs.contains_key(&s))
    }

    /// Call an event handler if the program defines it.
    pub fn call_event(&mut self, name: &str, args: Vec<Value>, host: &mut dyn Host) -> R<Option<Value>> {
        let sym = match self.prog.interner.get(name) {
            Some(s) => s,
            None => return Ok(None),
        };
        let f = match self.prog.funcs.get(&sym).and_then(|fs| fs.iter().find(|f| f.params.len() == args.len()).or(fs.first())) {
            Some(f) => f.clone(),
            None => return Ok(None),
        };
        self.steps = 0;
        self.cur_func = name.to_string();
        let r = self.call_values(&f, args, host)?;
        Ok(Some(r))
    }

    pub fn deinit(&mut self, reason: i64, host: &mut dyn Host) -> R<()> {
        self.call_event("OnDeinit", vec![Value::Int(reason)], host)?;
        Ok(())
    }

    /// Run an indicator's OnCalculate over the current bars of its symbol and timeframe.
    pub fn calculate(&mut self, host: &mut dyn Host) -> R<()> {
        let bars: Vec<Bar> = match host.bars(&self.symbol.clone(), self.tf) {
            Some(b) => b.to_vec(),
            None => return Ok(()),
        };
        self.calculate_on(&bars, host)
    }

    pub fn calculate_on(&mut self, bars: &[Bar], host: &mut dyn Host) -> R<()> {
        let n = bars.len();
        if n == 0 {
            return Ok(());
        }
        let first = bars[0].time;
        let mut reset = first != self.last_first_time || n < self.last_rates_total;
        if self.last_rates_total == 0 {
            reset = true;
        }
        if reset {
            self.prev_calculated = 0;
        }
        // shift buffers when history was trimmed at the front
        for (arr, _) in self.buffers.iter().flatten() {
            let mut a = arr.borrow_mut();
            if reset {
                a.data.clear();
            }
            let empty = Value::Double(f64::MAX);
            if a.data.len() < n {
                a.data.resize(n, empty);
            } else if a.data.len() > n {
                a.data.truncate(n);
            }
        }
        self.fill_prices(bars);
        self.last_first_time = first;
        self.last_rates_total = n;
        let sym = match self.prog.interner.get("OnCalculate") {
            Some(s) => s,
            None => return Ok(()),
        };
        let f = match self.prog.funcs.get(&sym).and_then(|v| v.first()) {
            Some(f) => f.clone(),
            None => return Ok(()),
        };
        let p = self.prices.as_ref().unwrap();
        let args = if f.params.len() >= 10 {
            vec![
                Value::Int(n as i64),
                Value::Int(self.prev_calculated),
                Value::Arr(p.time.clone()),
                Value::Arr(p.open.clone()),
                Value::Arr(p.high.clone()),
                Value::Arr(p.low.clone()),
                Value::Arr(p.close.clone()),
                Value::Arr(p.tick_volume.clone()),
                Value::Arr(p.volume.clone()),
                Value::Arr(p.spread.clone()),
            ]
        } else {
            vec![Value::Int(n as i64), Value::Int(self.prev_calculated), Value::Int(0), Value::Arr(p.price.clone())]
        };
        self.steps = 0;
        self.cur_func = "OnCalculate".into();
        let r = self.call_values(&f, args, host)?;
        self.prev_calculated = r.as_i64();
        Ok(())
    }

    fn fill_prices(&mut self, bars: &[Bar]) {
        if self.prices.is_none() {
            let mk = |t: Ty| new_array(t, vec![]);
            self.prices = Some(PriceArrays {
                time: mk(Ty::Int),
                open: mk(Ty::Double),
                high: mk(Ty::Double),
                low: mk(Ty::Double),
                close: mk(Ty::Double),
                tick_volume: mk(Ty::Int),
                volume: mk(Ty::Int),
                spread: mk(Ty::Int),
                price: mk(Ty::Double),
            });
        }
        let p = self.prices.as_ref().unwrap();
        let set = |a: &ArrRef, f: &dyn Fn(&Bar) -> Value| {
            let mut a = a.borrow_mut();
            a.data.clear();
            a.data.extend(bars.iter().map(f));
        };
        set(&p.time, &|b| Value::Int(b.time));
        set(&p.open, &|b| Value::Double(b.open));
        set(&p.high, &|b| Value::Double(b.high));
        set(&p.low, &|b| Value::Double(b.low));
        set(&p.close, &|b| Value::Double(b.close));
        set(&p.tick_volume, &|b| Value::Int(b.tick_volume));
        set(&p.volume, &|_| Value::Int(0));
        set(&p.spread, &|b| Value::Int(b.spread as i64));
        set(&p.price, &|b| Value::Double(b.close));
    }

    /// The plots of an indicator with their data, for drawing.
    pub fn plot_data(&self) -> Vec<(PlotRt, Vec<Vec<f64>>)> {
        let mut out = vec![];
        let bufs: Vec<&(ArrRef, i64)> = self.buffers.iter().flatten().filter(|(_, t)| *t != 2).collect();
        let mut bi = 0;
        for plot in &self.plots {
            let need = match plot.draw_type {
                4 | 7 | 6 => 2,
                8 | 9 => 4,
                10 | 11 | 12 | 14 => 2,
                13 | 15 => 3,
                16 | 17 => 5,
                _ => 1,
            };
            let mut data = vec![];
            for k in 0..need {
                if let Some((arr, _)) = bufs.get(bi + k) {
                    let a = arr.borrow();
                    let empty = plot.empty_value;
                    data.push(
                        a.data
                            .iter()
                            .map(|v| {
                                let x = v.as_f64();
                                if x == empty || x == f64::MAX || !x.is_finite() {
                                    f64::NAN
                                } else {
                                    x
                                }
                            })
                            .collect(),
                    );
                }
            }
            bi += need;
            out.push((plot.clone(), data));
        }
        out
    }

    // ---------- calls ----------

    pub fn call_values(&mut self, f: &Rc<Func>, args: Vec<Value>, host: &mut dyn Host) -> R<Value> {
        self.call_values_this(f, args, None, host)
    }

    pub fn call_values_this(&mut self, f: &Rc<Func>, args: Vec<Value>, this: Option<StructRef>, host: &mut dyn Host) -> R<Value> {
        let mut scope = vec![];
        for (i, p) in f.params.iter().enumerate() {
            let v = match args.get(i) {
                Some(v) => v.clone(),
                None => match &p.default {
                    Some(e) => self.eval(e, host)?,
                    None => self.default_value(&p.ty),
                },
            };
            let v = if p.is_array { v } else { self.coerce(v, &p.ty) };
            scope.push((p.name, Rc::new(RefCell::new(Var { ty: p.ty.clone(), v }))));
        }
        self.this_stack.push(this);
        let r = self.run_body(f, scope, host);
        self.this_stack.pop();
        r
    }

    fn run_body(&mut self, f: &Rc<Func>, scope: Vec<(Sym, VarRef)>, host: &mut dyn Host) -> R<Value> {
        if self.depth > 200 {
            return Err(self.err("stack overflow"));
        }
        self.depth += 1;
        self.frames.push(vec![scope]);
        let mut result = Value::Void;
        let mut outcome = Ok(());
        for s in f.body.iter() {
            match self.exec(s, host) {
                Ok(Flow::Return(v)) => {
                    result = v;
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    outcome = Err(e);
                    break;
                }
            }
        }
        self.frames.pop();
        self.depth -= 1;
        outcome?;
        Ok(if f.ret == Ty::Void { Value::Void } else { self.coerce(result, &f.ret) })
    }

    fn call_user(&mut self, fs: &[Rc<Func>], args: &[Expr], this: Option<StructRef>, host: &mut dyn Host) -> R<Value> {
        let f = fs
            .iter()
            .find(|f| {
                let req = f.params.iter().filter(|p| p.default.is_none()).count();
                args.len() >= req && args.len() <= f.params.len()
            })
            .unwrap_or(&fs[0])
            .clone();
        let mut scope = vec![];
        let mut writebacks: Vec<(Expr, VarRef)> = vec![];
        for (i, p) in f.params.iter().enumerate() {
            let var = match args.get(i) {
                Some(a) => {
                    if p.by_ref || p.is_array {
                        match a {
                            Expr::Var(s) if self.lookup(*s).is_some() => self.lookup(*s).unwrap(),
                            other => {
                                let v = self.eval(other, host)?;
                                let var = Rc::new(RefCell::new(Var { ty: p.ty.clone(), v }));
                                if p.by_ref && matches!(other, Expr::Index(..) | Expr::Member(..) | Expr::Var(_)) {
                                    writebacks.push((other.clone(), var.clone()));
                                }
                                var
                            }
                        }
                    } else {
                        let v = self.eval(a, host)?;
                        Rc::new(RefCell::new(Var { ty: p.ty.clone(), v: self.coerce(v, &p.ty) }))
                    }
                }
                None => {
                    let v = match &p.default {
                        Some(e) => self.eval(e, host)?,
                        None => self.default_value(&p.ty),
                    };
                    Rc::new(RefCell::new(Var { ty: p.ty.clone(), v: self.coerce(v, &p.ty) }))
                }
            };
            scope.push((p.name, var));
        }
        let saved = (self.cur_line, self.cur_col);
        self.this_stack.push(this);
        let r = self.run_body(&f, scope, host);
        self.this_stack.pop();
        let r = r?;
        self.cur_line = saved.0;
        self.cur_col = saved.1;
        for (e, var) in writebacks {
            let v = var.borrow().v.clone();
            self.assign(&e, v, host)?;
        }
        Ok(r)
    }

    // ---------- statements ----------

    fn exec(&mut self, s: &Stmt, host: &mut dyn Host) -> R<Flow> {
        self.steps += 1;
        if self.steps > self.step_limit {
            return Err(self.err(format!("endless loop detected in {} function", self.cur_func)));
        }
        self.cur_line = s.line;
        self.cur_col = s.col;
        match &s.kind {
            StmtKind::Expr(e) => {
                self.eval(e, host)?;
                Ok(Flow::Normal)
            }
            StmtKind::Decl(ds) => {
                for d in ds {
                    if d.is_static {
                        if let Some(v) = self.statics.get(&d.id).cloned() {
                            self.declare(d.name, v);
                            continue;
                        }
                        let v = self.make_var(d, host)?;
                        self.statics.insert(d.id, v.clone());
                        self.declare(d.name, v);
                    } else {
                        let v = self.make_var(d, host)?;
                        self.declare(d.name, v);
                    }
                }
                Ok(Flow::Normal)
            }
            StmtKind::Block(b) => self.block(b, host),
            StmtKind::If(c, t, e) => {
                if self.eval(c, host)?.truthy() {
                    self.scoped(t, host)
                } else if let Some(e) = e {
                    self.scoped(e, host)
                } else {
                    Ok(Flow::Normal)
                }
            }
            StmtKind::While(c, b) => {
                while self.eval(c, host)?.truthy() {
                    match self.scoped(b, host)? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        _ => {}
                    }
                    self.tick_steps()?;
                }
                Ok(Flow::Normal)
            }
            StmtKind::DoWhile(b, c) => {
                loop {
                    match self.scoped(b, host)? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        _ => {}
                    }
                    if !self.eval(c, host)?.truthy() {
                        break;
                    }
                    self.tick_steps()?;
                }
                Ok(Flow::Normal)
            }
            StmtKind::For(init, cond, step, body) => {
                self.push_scope();
                let r = (|| -> R<Flow> {
                    if let Some(i) = init {
                        self.exec(i, host)?;
                    }
                    loop {
                        if let Some(c) = cond {
                            if !self.eval(c, host)?.truthy() {
                                break;
                            }
                        }
                        match self.scoped(body, host)? {
                            Flow::Break => break,
                            Flow::Return(v) => return Ok(Flow::Return(v)),
                            _ => {}
                        }
                        if let Some(st) = step {
                            self.eval(st, host)?;
                        }
                        self.tick_steps()?;
                    }
                    Ok(Flow::Normal)
                })();
                self.pop_scope();
                r
            }
            StmtKind::Return(e) => {
                let v = match e {
                    Some(e) => self.eval(e, host)?,
                    None => Value::Void,
                };
                Ok(Flow::Return(v))
            }
            StmtKind::Break => Ok(Flow::Break),
            StmtKind::Continue => Ok(Flow::Continue),
            StmtKind::Switch(v, arms) => {
                let val = self.eval(v, host)?;
                let mut start = None;
                for (i, a) in arms.iter().enumerate() {
                    if let Some(l) = &a.label {
                        let lv = self.eval(l, host)?;
                        let eq = match (&val, &lv) {
                            (Value::Str(a), Value::Str(b)) => a == b,
                            _ => val.as_i64() == lv.as_i64(),
                        };
                        if eq {
                            start = Some(i);
                            break;
                        }
                    }
                }
                if start.is_none() {
                    start = arms.iter().position(|a| a.label.is_none());
                }
                if let Some(st) = start {
                    self.push_scope();
                    for a in &arms[st..] {
                        for s in &a.body {
                            match self.exec(s, host) {
                                Ok(Flow::Break) => {
                                    self.pop_scope();
                                    return Ok(Flow::Normal);
                                }
                                Ok(Flow::Return(v)) => {
                                    self.pop_scope();
                                    return Ok(Flow::Return(v));
                                }
                                Ok(Flow::Continue) => {
                                    self.pop_scope();
                                    return Ok(Flow::Continue);
                                }
                                Ok(Flow::Normal) => {}
                                Err(e) => {
                                    self.pop_scope();
                                    return Err(e);
                                }
                            }
                        }
                    }
                    self.pop_scope();
                }
                Ok(Flow::Normal)
            }
            StmtKind::Empty => Ok(Flow::Normal),
        }
    }

    fn tick_steps(&mut self) -> R<()> {
        self.steps += 1;
        if self.steps > self.step_limit {
            return Err(self.err(format!("endless loop detected in {} function", self.cur_func)));
        }
        Ok(())
    }

    fn push_scope(&mut self) {
        if let Some(f) = self.frames.last_mut() {
            f.push(vec![]);
        }
    }
    fn pop_scope(&mut self) {
        if let Some(f) = self.frames.last_mut() {
            f.pop();
        }
    }

    fn block(&mut self, b: &[Stmt], host: &mut dyn Host) -> R<Flow> {
        self.push_scope();
        for s in b {
            match self.exec(s, host) {
                Ok(Flow::Normal) => {}
                Ok(other) => {
                    self.pop_scope();
                    return Ok(other);
                }
                Err(e) => {
                    self.pop_scope();
                    return Err(e);
                }
            }
        }
        self.pop_scope();
        Ok(Flow::Normal)
    }

    fn scoped(&mut self, s: &Stmt, host: &mut dyn Host) -> R<Flow> {
        match &s.kind {
            StmtKind::Block(b) => self.block(b, host),
            StmtKind::Decl(_) => {
                self.push_scope();
                let r = self.exec(s, host);
                self.pop_scope();
                r
            }
            _ => self.exec(s, host),
        }
    }

    // ---------- expressions ----------

    pub fn eval(&mut self, e: &Expr, host: &mut dyn Host) -> R<Value> {
        match e {
            Expr::Int(v) => Ok(Value::Int(*v)),
            Expr::Double(v) => Ok(Value::Double(*v)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Null => Ok(Value::Int(0)),
            Expr::Var(s) => self.read_var(*s, host),
            Expr::Index(a, i) => {
                let av = self.eval(a, host)?;
                let idx = self.eval(i, host)?.as_i64();
                match av {
                    Value::Arr(arr) => {
                        let a = arr.borrow();
                        let len = a.data.len() as i64;
                        if idx < 0 || idx >= len {
                            return Err(self.err("array out of range"));
                        }
                        let p = if a.series { len - 1 - idx } else { idx };
                        Ok(a.data[p as usize].clone())
                    }
                    Value::Str(s) => Ok(Value::Int(s.chars().nth(idx.max(0) as usize).map(|c| c as i64).unwrap_or(0))),
                    Value::Struct(st) => match self.operator_method(&st, "operator[]") {
                        Some(f) => self.call_values_this(&f, vec![Value::Int(idx)], Some(st), host),
                        None => Err(self.err("'[' - array required")),
                    },
                    _ => Err(self.err("'[' - array required")),
                }
            }
            Expr::Member(o, f) => {
                let ov = self.eval(o, host)?;
                match ov {
                    Value::Int(0) | Value::Void => Err(self.err("invalid pointer access")),
                    Value::Struct(s) => {
                        let sb = s.borrow();
                        match sb.fields.iter().find(|(n, _, _)| n == f) {
                            Some((_, _, v)) => Ok(v.clone()),
                            None => Err(self.err(format!("'{}' - undeclared member", self.sym(*f)))),
                        }
                    }
                    _ => Err(self.err(format!("'{}' - struct member access on a non-struct value", self.sym(*f)))),
                }
            }
            Expr::Call(s, args) => {
                if let Some(t) = self.cur_this() {
                    let cls = t.borrow().ty;
                    if let Some(fs) = self.prog.find_method(cls, *s).cloned() {
                        return self.call_user(&fs, args, Some(t), host);
                    }
                }
                if let Some(fs) = self.prog.funcs.get(s).cloned() {
                    return self.call_user(&fs, args, None, host);
                }
                let name = self.sym(*s).to_string();
                self.call_builtin(&name, args, host)
            }
            Expr::MethodCall(o, m, args) => {
                let ov = self.eval(o, host)?;
                let name = self.sym(*m).to_string();
                match ov {
                    Value::Obj(obj) => self.call_method(&obj, &name, args, host),
                    Value::Struct(st) => {
                        let cls = st.borrow().ty;
                        match self.prog.find_method(cls, *m).cloned() {
                            Some(fs) => self.call_user(&fs, args, Some(st), host),
                            None => Err(self.err(format!("'{}' - undeclared method of '{}'", name, self.sym(cls)))),
                        }
                    }
                    Value::Int(0) | Value::Void => Err(self.err("invalid pointer access")),
                    _ => Err(self.err(format!("'{}' - method call on a value that is not an object", name))),
                }
            }
            Expr::ScopedCall(cls, m, args) => match self.prog.find_method(*cls, *m).cloned() {
                Some(fs) => {
                    let this = self.cur_this();
                    self.call_user(&fs, args, this, host)
                }
                None => {
                    if let Some(fs) = self.prog.funcs.get(m).cloned() {
                        return self.call_user(&fs, args, None, host);
                    }
                    let name = self.sym(*m).to_string();
                    self.call_builtin(&name, args, host)
                }
            },
            Expr::New(cls, args) => self.construct(*cls, args, host),
            Expr::Delete(e) => {
                if let Value::Struct(st) = self.eval(e, host)? {
                    self.destroy(&st, host)?;
                }
                Ok(Value::Void)
            }
            Expr::Unary(op, a) => {
                let v = self.eval(a, host)?;
                Ok(match op {
                    UnOp::Neg => match v {
                        Value::Int(i) => Value::Int(-i),
                        other => Value::Double(-other.as_f64()),
                    },
                    UnOp::Plus => v,
                    UnOp::Not => Value::Bool(!v.truthy()),
                    UnOp::BitNot => Value::Int(!v.as_i64()),
                })
            }
            Expr::Binary(op, a, b) => {
                match op {
                    BinOp::And => {
                        let l = self.eval(a, host)?;
                        if !l.truthy() {
                            return Ok(Value::Bool(false));
                        }
                        return Ok(Value::Bool(self.eval(b, host)?.truthy()));
                    }
                    BinOp::Or => {
                        let l = self.eval(a, host)?;
                        if l.truthy() {
                            return Ok(Value::Bool(true));
                        }
                        return Ok(Value::Bool(self.eval(b, host)?.truthy()));
                    }
                    _ => {}
                }
                let l = self.eval(a, host)?;
                let r = self.eval(b, host)?;
                if let Value::Struct(st) = &l {
                    if let Some(f) = self.operator_method(st, &format!("operator{}", binop_text(*op))) {
                        return self.call_values_this(&f, vec![r], Some(st.clone()), host);
                    }
                }
                if matches!(op, BinOp::Eq | BinOp::Ne) && (matches!(l, Value::Struct(_)) || matches!(r, Value::Struct(_))) {
                    let same = match (&l, &r) {
                        (Value::Struct(x), Value::Struct(y)) => Rc::ptr_eq(x, y),
                        _ => false,
                    };
                    return Ok(Value::Bool(if *op == BinOp::Eq { same } else { !same }));
                }
                self.binop(*op, l, r)
            }
            Expr::Assign(op, lhs, rhs) => {
                let rv = match rhs.as_ref() {
                    Expr::List(items) => {
                        // assigning an initializer list to an array or struct
                        let cur = self.eval(lhs, host)?;
                        let vals: Vec<Value> = items.iter().map(|e| self.eval(e, host)).collect::<R<Vec<_>>>()?;
                        match &cur {
                            Value::Arr(a) => {
                                let elem = a.borrow().elem.clone();
                                fill_array(self, a, vals, &elem);
                            }
                            Value::Struct(s) => {
                                let mut sb = s.borrow_mut();
                                for (i, v) in vals.into_iter().enumerate() {
                                    if i < sb.fields.len() {
                                        let t = sb.fields[i].1.clone();
                                        sb.fields[i].2 = self.coerce(v, &t);
                                    }
                                }
                            }
                            _ => {}
                        }
                        return Ok(cur);
                    }
                    other => self.eval(other, host)?,
                };
                let v = match op {
                    None => rv,
                    Some(o) => {
                        let cur = self.eval(lhs, host)?;
                        self.binop(*o, cur, rv)?
                    }
                };
                self.assign(lhs, v.clone(), host)?;
                Ok(v)
            }
            Expr::IncDec { pre, inc, target } => {
                let cur = self.eval(target, host)?;
                let new = match &cur {
                    Value::Double(d) => Value::Double(if *inc { d + 1.0 } else { d - 1.0 }),
                    other => Value::Int(if *inc { other.as_i64() + 1 } else { other.as_i64() - 1 }),
                };
                self.assign(target, new.clone(), host)?;
                Ok(if *pre { new } else { cur })
            }
            Expr::Ternary(c, a, b) => {
                if self.eval(c, host)?.truthy() {
                    self.eval(a, host)
                } else {
                    self.eval(b, host)
                }
            }
            Expr::Cast(ty, a) => {
                let v = self.eval(a, host)?;
                Ok(self.coerce(v, ty))
            }
            Expr::List(items) => {
                let vals: Vec<Value> = items.iter().map(|e| self.eval(e, host)).collect::<R<Vec<_>>>()?;
                Ok(Value::Arr(new_array(Ty::Double, vals)))
            }
        }
    }

    fn read_var(&mut self, s: Sym, host: &mut dyn Host) -> R<Value> {
        if let Some(v) = self.lookup(s) {
            return Ok(v.borrow().v.clone());
        }
        if let Some(v) = self.this_field(s) {
            return Ok(v);
        }
        if self.sym(s) == "this" {
            if let Some(t) = self.cur_this() {
                return Ok(Value::Struct(t));
            }
        }
        if let Some(c) = self.prog.enum_consts.get(&s) {
            return Ok(Value::Int(*c));
        }
        let name = self.sym(s).to_string();
        if let Some(c) = builtins::constant(&name) {
            return Ok(match c {
                builtins::Const::I(i) => Value::Int(i),
                builtins::Const::D(d) => Value::Double(d),
            });
        }
        let q = host.quote(&self.symbol);
        Ok(match name.as_str() {
            "_Symbol" => Value::str(&self.symbol),
            "_Period" => Value::Int(self.tf.mql_value()),
            "_Point" => Value::Double(q.map(|q| q.spec.point()).unwrap_or(0.00001)),
            "_Digits" => Value::Int(q.map(|q| q.spec.digits as i64).unwrap_or(5)),
            "Bid" => Value::Double(q.map(|q| q.bid).unwrap_or(0.0)),
            "Ask" => Value::Double(q.map(|q| q.ask).unwrap_or(0.0)),
            "_LastError" => Value::Int(self.last_error),
            "_StopFlag" => Value::Bool(false),
            "_UninitReason" => Value::Int(0),
            "_RandomSeed" => Value::Int(self.rand_state as i64),
            "_IsX64" => Value::Bool(true),
            _ => return Err(self.err(format!("'{}' - undeclared identifier", name))),
        })
    }

    pub fn assign(&mut self, lhs: &Expr, v: Value, host: &mut dyn Host) -> R<()> {
        match lhs {
            Expr::Var(s) => match self.lookup(*s) {
                Some(var) => {
                    let ty = var.borrow().ty.clone();
                    let is_arr = matches!(var.borrow().v, Value::Arr(_));
                    let nv = if is_arr && !matches!(v, Value::Arr(_)) {
                        return Err(self.err("invalid array access"));
                    } else {
                        self.coerce(v, &ty)
                    };
                    var.borrow_mut().v = nv;
                    Ok(())
                }
                None => {
                    if self.set_this_field(*s, v) {
                        return Ok(());
                    }
                    Err(self.err(format!("'{}' - l-value required", self.sym(*s))))
                }
            },
            Expr::Index(a, i) => {
                let av = self.eval(a, host)?;
                let idx = self.eval(i, host)?.as_i64();
                match av {
                    Value::Arr(arr) => {
                        let elem = arr.borrow().elem.clone();
                        let nv = if matches!(v, Value::Arr(_)) { v } else { self.coerce(v, &elem) };
                        let mut a = arr.borrow_mut();
                        let len = a.data.len() as i64;
                        if idx < 0 || idx >= len {
                            drop(a);
                            return Err(self.err("array out of range"));
                        }
                        let p = if a.series { len - 1 - idx } else { idx };
                        a.data[p as usize] = nv;
                        Ok(())
                    }
                    _ => Err(self.err("'[' - array required")),
                }
            }
            Expr::Member(o, f) => {
                let ov = self.eval(o, host)?;
                match ov {
                    Value::Struct(s) => {
                        let ty = s.borrow().fields.iter().find(|(n, _, _)| n == f).map(|x| x.1.clone());
                        match ty {
                            Some(t) => {
                                let nv = self.coerce(v, &t);
                                let mut sb = s.borrow_mut();
                                if let Some(fl) = sb.fields.iter_mut().find(|(n, _, _)| n == f) {
                                    fl.2 = nv;
                                }
                                Ok(())
                            }
                            None => Err(self.err(format!("'{}' - undeclared member", self.sym(*f)))),
                        }
                    }
                    _ => Err(self.err("struct member access on a non-struct value")),
                }
            }
            _ => Err(self.err("l-value required")),
        }
    }

    fn operator_method(&self, st: &StructRef, name: &str) -> Option<Rc<Func>> {
        let sym = self.prog.interner.get(name)?;
        let cls = st.borrow().ty;
        self.prog.find_method(cls, sym).and_then(|v| v.first().cloned())
    }

    fn binop(&self, op: BinOp, l: Value, r: Value) -> R<Value> {
        use BinOp::*;
        if let (Value::Str(_), _) | (_, Value::Str(_)) = (&l, &r) {
            if matches!(l, Value::Str(_)) && matches!(r, Value::Str(_)) || op == Add {
                let (a, b) = (l.to_text(), r.to_text());
                return Ok(match op {
                    Add => Value::str(&(a + &b)),
                    Eq => Value::Bool(a == b),
                    Ne => Value::Bool(a != b),
                    Lt => Value::Bool(a < b),
                    Le => Value::Bool(a <= b),
                    Gt => Value::Bool(a > b),
                    Ge => Value::Bool(a >= b),
                    _ => return Err(self.err("illegal operation use with strings")),
                });
            }
        }
        let both_int = matches!(l, Value::Int(_) | Value::Bool(_)) && matches!(r, Value::Int(_) | Value::Bool(_));
        Ok(match op {
            Add | Sub | Mul | Div | Mod => {
                if both_int {
                    let (a, b) = (l.as_i64(), r.as_i64());
                    Value::Int(match op {
                        Add => a.wrapping_add(b),
                        Sub => a.wrapping_sub(b),
                        Mul => a.wrapping_mul(b),
                        Div => {
                            if b == 0 {
                                return Err(self.err("zero divide"));
                            }
                            a.wrapping_div(b)
                        }
                        _ => {
                            if b == 0 {
                                return Err(self.err("zero divide"));
                            }
                            a.wrapping_rem(b)
                        }
                    })
                } else {
                    let (a, b) = (l.as_f64(), r.as_f64());
                    Value::Double(match op {
                        Add => a + b,
                        Sub => a - b,
                        Mul => a * b,
                        Div => {
                            if b == 0.0 {
                                return Err(self.err("zero divide"));
                            }
                            a / b
                        }
                        _ => a % b,
                    })
                }
            }
            Lt | Le | Gt | Ge | Eq | Ne => {
                let c = if both_int {
                    l.as_i64().partial_cmp(&r.as_i64())
                } else {
                    l.as_f64().partial_cmp(&r.as_f64())
                };
                let c = c.unwrap_or(std::cmp::Ordering::Equal);
                use std::cmp::Ordering::*;
                Value::Bool(match op {
                    Lt => c == Less,
                    Le => c != Greater,
                    Gt => c == Greater,
                    Ge => c != Less,
                    Eq => c == Equal,
                    _ => c != Equal,
                })
            }
            BitAnd => Value::Int(l.as_i64() & r.as_i64()),
            BitOr => Value::Int(l.as_i64() | r.as_i64()),
            BitXor => Value::Int(l.as_i64() ^ r.as_i64()),
            Shl => Value::Int(l.as_i64().wrapping_shl(r.as_i64() as u32)),
            Shr => Value::Int(l.as_i64().wrapping_shr(r.as_i64() as u32)),
            And => Value::Bool(l.truthy() && r.truthy()),
            Or => Value::Bool(l.truthy() || r.truthy()),
        })
    }

    pub fn next_rand(&mut self) -> i64 {
        // classic 15-bit generator like MathRand (0..32767)
        self.rand_state = self.rand_state.wrapping_mul(214013).wrapping_add(2531011);
        ((self.rand_state >> 16) & 0x7fff) as i64
    }
    pub fn seed_rand(&mut self, s: u64) {
        self.rand_state = s;
    }
    pub fn selected_position(&self) -> Option<u64> {
        self.selected_position
    }
    pub fn select_position(&mut self, t: Option<u64>) {
        self.selected_position = t;
    }
    pub fn selected_order(&self) -> Option<u64> {
        self.selected_order
    }
    pub fn select_order(&mut self, t: Option<u64>) {
        self.selected_order = t;
    }
    pub fn rt_err(&self, msg: impl Into<String>) -> RtError {
        self.err(msg)
    }
    pub fn prices_close(&self) -> Option<ArrRef> {
        self.prices.as_ref().map(|p| p.close.clone())
    }
}

fn binop_text(op: BinOp) -> &'static str {
    use BinOp::*;
    match op {
        Add => "+",
        Sub => "-",
        Mul => "*",
        Div => "/",
        Mod => "%",
        Lt => "<",
        Le => "<=",
        Gt => ">",
        Ge => ">=",
        Eq => "==",
        Ne => "!=",
        And => "&&",
        Or => "||",
        BitAnd => "&",
        BitOr => "|",
        BitXor => "^",
        Shl => "<<",
        Shr => ">>",
    }
}

fn const_dim(e: &Expr) -> usize {
    match e {
        Expr::Int(v) => (*v).max(0) as usize,
        _ => 0,
    }
}

pub fn deep_copy_struct(s: &StructVal) -> StructVal {
    StructVal {
        ty: s.ty,
        fields: s
            .fields
            .iter()
            .map(|(n, t, v)| {
                let v = match v {
                    Value::Struct(x) if !matches!(t, Ty::Ptr(_) | Ty::Any) => Value::Struct(Rc::new(RefCell::new(deep_copy_struct(&x.borrow())))),
                    Value::Arr(a) => {
                        let ab = a.borrow();
                        Value::Arr(Rc::new(RefCell::new(Array {
                            data: ab.data.clone(),
                            elem: ab.elem.clone(),
                            series: ab.series,
                            inner: ab.inner.clone(),
                            dynamic: ab.dynamic,
                        })))
                    }
                    other => other.clone(),
                };
                (*n, t.clone(), v)
            })
            .collect(),
    }
}

fn fill_array(inst: &Instance, arr: &ArrRef, vals: Vec<Value>, elem: &Ty) {
    let mut a = arr.borrow_mut();
    if a.data.len() < vals.len() {
        let inner = a.inner.clone();
        let d = inst.array_elem_default(elem, &inner);
        a.data.resize(vals.len(), d);
    }
    for (i, v) in vals.into_iter().enumerate() {
        a.data[i] = match (&a.data[i], v) {
            (Value::Arr(inner), Value::Arr(src)) => {
                let src = src.borrow().data.clone();
                let mut ib = inner.borrow_mut();
                for (k, x) in src.into_iter().enumerate() {
                    if k < ib.data.len() {
                        ib.data[k] = inst.coerce(x, elem);
                    }
                }
                Value::Arr(inner.clone())
            }
            (_, v) => inst.coerce(v, elem),
        };
    }
}

pub fn default_plot_color(i: usize) -> u32 {
    [0xFF901E, 0x0000FF, 0x00FF00, 0x00D7FF, 0xFF00FF, 0xFFFF00, 0xC0C0C0, 0x00A5FF][i % 8]
}

pub fn parse_input_value(text: &str, ty: &Ty, enum_name: Option<&str>, prog: &Program) -> Value {
    let t = text.trim();
    match ty {
        Ty::Bool => Value::Bool(matches!(t.to_ascii_lowercase().as_str(), "true" | "1" | "yes")),
        Ty::Double => Value::Double(t.parse().unwrap_or(0.0)),
        Ty::Str => Value::str(t),
        Ty::Int => {
            if let Ok(v) = t.parse::<i64>() {
                return Value::Int(v);
            }
            if let Some(en) = enum_name {
                if let Some(members) = prog.enums.get(en) {
                    if let Some((_, v)) = members.iter().find(|(n, _)| n == t) {
                        return Value::Int(*v);
                    }
                }
            }
            if let Some(c) = builtins::constant(t).and_then(|c| c.as_i64()) {
                return Value::Int(c);
            }
            if let Some(v) = crate::mql::lexer::parse_datetime(t) {
                return Value::Int(v);
            }
            Value::Int(t.parse::<f64>().map(|f| f as i64).unwrap_or(0))
        }
        _ => Value::str(t),
    }
}

mod calls;
mod extra;
