//! Parser for the MQL5 subset: preprocessor (#property, #define, #include), enums, structs,
//! inputs, globals and functions.

use super::ast::*;
use super::builtins;
use super::lexer::{lex, CompileError, Tok, Token};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

pub type Loader<'a> = &'a dyn Fn(&str) -> Option<String>;

pub fn compile(file_name: &str, src: &str, loader: Loader) -> Result<Program, Vec<CompileError>> {
    let lexed = lex(src).map_err(|e| vec![e])?;
    let mut p = Parser::new(file_name, lexed.line_comments);
    let toks = p.preprocess(lexed.tokens, loader, 0).map_err(|e| vec![e])?;
    p.toks = toks;
    p.pos = 0;
    if let Err(e) = p.parse_top() {
        return Err(vec![e]);
    }
    let prog = p.finish();
    let errs = super::check::check(&prog);
    if !errs.is_empty() {
        return Err(errs);
    }
    Ok(prog)
}

struct Parser {
    file_name: String,
    toks: Vec<Token>,
    pos: usize,
    interner: Interner,
    macros: HashMap<String, Vec<Token>>,
    type_names: HashSet<String>,
    enum_types: HashSet<String>,
    structs: HashMap<Sym, Rc<StructDef>>,
    next_id: u32,
    props: Properties,
    comments: HashMap<u32, String>,
    globals: Vec<VarDecl>,
    inputs: Vec<InputDecl>,
    funcs: HashMap<Sym, Vec<Rc<Func>>>,
    enum_consts: HashMap<Sym, i64>,
    enums: HashMap<String, Vec<(String, i64)>>,
    warnings: Vec<String>,
}

const PRIM_TYPES: [&str; 17] = [
    "void", "bool", "char", "uchar", "short", "ushort", "int", "uint", "long", "ulong", "float", "double", "string", "datetime",
    "color", "unsigned", "signed",
];

impl Parser {
    fn new(file_name: &str, comments: HashMap<u32, String>) -> Self {
        let mut type_names: HashSet<String> = PRIM_TYPES.iter().map(|s| s.to_string()).collect();
        let mut interner = Interner::default();
        let mut structs = HashMap::new();
        for (name, fields) in builtins::builtin_structs() {
            type_names.insert(name.to_string());
            let sym = interner.intern(name);
            let fields = fields.iter().map(|(f, t)| (interner.intern(f), t.clone(), vec![])).collect();
            structs.insert(sym, Rc::new(StructDef { name: sym, fields }));
        }
        for c in builtins::BUILTIN_CLASSES {
            type_names.insert(c.to_string());
        }
        Parser {
            file_name: file_name.to_string(),
            toks: vec![],
            pos: 0,
            interner,
            macros: HashMap::new(),
            type_names,
            enum_types: HashSet::new(),
            structs,
            next_id: 1,
            props: Properties::default(),
            comments,
            globals: vec![],
            inputs: vec![],
            funcs: HashMap::new(),
            enum_consts: HashMap::new(),
            enums: HashMap::new(),
            warnings: vec![],
        }
    }

    fn finish(self) -> Program {
        let kind = {
            let has = |n: &str| self.interner.get(n).is_some_and(|s| self.funcs.contains_key(&s));
            if has("OnCalculate") {
                ProgramKind::Indicator
            } else if has("OnTick") {
                ProgramKind::Expert
            } else if has("OnStart") {
                ProgramKind::Script
            } else {
                ProgramKind::Unknown
            }
        };
        Program {
            file_name: self.file_name,
            kind,
            props: self.props,
            globals: self.globals,
            inputs: self.inputs,
            funcs: self.funcs,
            enum_consts: self.enum_consts,
            enums: self.enums,
            structs: self.structs,
            interner: self.interner,
            warnings: self.warnings,
        }
    }

    // ---------- preprocessor ----------

    fn preprocess(&mut self, toks: Vec<Token>, loader: Loader, depth: usize) -> Result<Vec<Token>, CompileError> {
        let mut out = Vec::with_capacity(toks.len());
        for t in toks {
            match &t.tok {
                Tok::Directive(d) => self.directive(d, &t, &mut out, loader, depth)?,
                Tok::Ident(name) if self.macros.contains_key(name) => {
                    let mut expanded = self.macros[name].clone();
                    // one level of nested macros
                    let mut again = Vec::new();
                    for mut e in expanded.drain(..) {
                        e.line = t.line;
                        e.col = t.col;
                        if let Tok::Ident(n2) = &e.tok {
                            if let Some(m2) = self.macros.get(n2) {
                                for mut x in m2.clone() {
                                    x.line = t.line;
                                    x.col = t.col;
                                    again.push(x);
                                }
                                continue;
                            }
                        }
                        again.push(e);
                    }
                    out.extend(again);
                }
                Tok::Eof => {
                    if depth == 0 {
                        out.push(t);
                    }
                }
                _ => out.push(t),
            }
        }
        Ok(out)
    }

    fn directive(&mut self, d: &str, t: &Token, out: &mut Vec<Token>, loader: Loader, depth: usize) -> Result<(), CompileError> {
        let err = |msg: String| CompileError { line: t.line, col: t.col, msg };
        let (word, rest) = match d.find(char::is_whitespace) {
            Some(p) => (&d[..p], d[p..].trim()),
            None => (d, ""),
        };
        match word {
            "property" => self.property(rest),
            "define" => {
                let (name, body) = match rest.find(|c: char| c.is_whitespace() || c == '(') {
                    Some(p) => (&rest[..p], &rest[p..]),
                    None => (rest, ""),
                };
                if body.starts_with('(') {
                    return Err(err(format!("'{}' - macros with parameters are not supported yet", name)));
                }
                let lexed = lex(body).map_err(|e| err(e.msg))?;
                let mut toks = lexed.tokens;
                toks.pop(); // Eof
                self.macros.insert(name.to_string(), toks);
            }
            "undef" => {
                self.macros.remove(rest);
            }
            "include" => {
                let path = rest.trim_matches(|c| c == '<' || c == '>' || c == '"').replace('\\', "/");
                if depth > 8 {
                    return Err(err("include nesting too deep".into()));
                }
                match loader(&path) {
                    Some(src) => {
                        let lexed = lex(&src).map_err(|e| err(format!("{}: {}", path, e.msg)))?;
                        let inner = self.preprocess(lexed.tokens, loader, depth + 1)?;
                        out.extend(inner);
                    }
                    None => {
                        let lower = path.to_ascii_lowercase();
                        let builtin = ["trade/", "indicators/", "arrays/", "movingaverages", "object.mqh", "math/"]
                            .iter()
                            .any(|p| lower.starts_with(p) || lower.contains(p));
                        if !builtin {
                            return Err(err(format!("can't open \"{}\" include file", path)));
                        }
                    }
                }
            }
            "import" => return Err(err("DLL imports are not allowed in this terminal".into())),
            "ifdef" | "ifndef" | "else" | "endif" => {
                self.warnings.push(format!("line {}: #{} ignored", t.line, word));
            }
            "resource" => {
                self.warnings.push(format!("line {}: #resource ignored", t.line));
            }
            _ => return Err(err(format!("'#{}' - unknown directive", word))),
        }
        Ok(())
    }

    fn property(&mut self, rest: &str) {
        let (key, val) = match rest.find(char::is_whitespace) {
            Some(p) => (&rest[..p], rest[p..].trim()),
            None => (rest, ""),
        };
        let unq = val.trim_matches('"').to_string();
        let num = |v: &str| -> Option<f64> {
            let v = v.trim();
            v.parse::<f64>().ok().or_else(|| builtins::constant(v).and_then(|c| c.as_f64()))
        };
        let plot_idx = |prefix: &str| -> Option<usize> { key.strip_prefix(prefix).and_then(|n| n.parse::<usize>().ok()) };
        match key {
            "indicator_chart_window" => self.props.chart_window = true,
            "indicator_separate_window" => self.props.separate_window = true,
            "indicator_buffers" => self.props.buffers = num(val).unwrap_or(0.0) as usize,
            "indicator_plots" => self.props.plots = num(val).unwrap_or(0.0) as usize,
            "indicator_minimum" => self.props.minimum = num(val),
            "indicator_maximum" => self.props.maximum = num(val),
            "indicator_levelcolor" => self.props.level_color = parse_color_value(val),
            "copyright" => self.props.copyright = unq,
            "version" => self.props.version = unq,
            "description" => self.props.description = unq,
            "link" => self.props.link = unq,
            "script_show_inputs" => self.props.show_inputs = true,
            _ => {
                if let Some(i) = plot_idx("indicator_type") {
                    self.props.plot.entry(i).or_default().draw_type = num(val).map(|v| v as i64);
                } else if let Some(i) = plot_idx("indicator_color") {
                    self.props.plot.entry(i).or_default().colors = val.split(',').filter_map(|c| parse_color_value(c.trim())).collect();
                } else if let Some(i) = plot_idx("indicator_style") {
                    self.props.plot.entry(i).or_default().style = num(val).map(|v| v as i64);
                } else if let Some(i) = plot_idx("indicator_width") {
                    self.props.plot.entry(i).or_default().width = num(val).map(|v| v as i64);
                } else if let Some(i) = plot_idx("indicator_label") {
                    self.props.plot.entry(i).or_default().label = Some(unq);
                } else if key.starts_with("indicator_level") && !key.starts_with("indicator_levelstyle") && !key.starts_with("indicator_levelwidth") {
                    if let Some(v) = num(val) {
                        self.props.levels.push(v);
                    }
                } else {
                    self.props.other.push((key.to_string(), unq));
                }
            }
        }
    }

    // ---------- token helpers ----------

    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }
    fn peek_at(&self, n: usize) -> &Tok {
        let i = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[i].tok
    }
    fn tok(&self) -> &Token {
        &self.toks[self.pos]
    }
    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }
    fn is_punct(&self, p: &str) -> bool {
        matches!(self.peek(), Tok::Punct(x) if *x == p)
    }
    fn is_ident(&self, s: &str) -> bool {
        matches!(self.peek(), Tok::Ident(x) if x == s)
    }
    fn eat_punct(&mut self, p: &str) -> bool {
        if self.is_punct(p) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn err_here(&self, msg: String) -> CompileError {
        let t = self.tok();
        CompileError { line: t.line, col: t.col, msg }
    }
    fn tok_text(&self) -> String {
        match self.peek() {
            Tok::Ident(s) => s.clone(),
            Tok::Int(v) => v.to_string(),
            Tok::Float(v) => v.to_string(),
            Tok::Str(s) => format!("\"{}\"", s),
            Tok::Punct(p) => p.to_string(),
            Tok::Directive(d) => format!("#{}", d),
            Tok::Eof => "end of file".into(),
        }
    }
    fn expect(&mut self, p: &str) -> Result<(), CompileError> {
        if self.eat_punct(p) {
            Ok(())
        } else {
            let what = match p {
                ";" => "semicolon expected".to_string(),
                ")" => "')' - parenthesis expected".to_string(),
                _ => format!("'{}' expected", p),
            };
            Err(self.err_here(format!("'{}' - {}", self.tok_text(), what)))
        }
    }
    fn ident(&mut self) -> Result<String, CompileError> {
        match self.peek().clone() {
            Tok::Ident(s) => {
                self.bump();
                Ok(s)
            }
            _ => Err(self.err_here(format!("'{}' - identifier expected", self.tok_text()))),
        }
    }
    fn new_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    fn is_type_start(&self) -> bool {
        match self.peek() {
            Tok::Ident(s) => {
                self.type_names.contains(s) || s.starts_with("ENUM_") || matches!(s.as_str(), "const" | "static")
            }
            _ => false,
        }
    }

    /// Parse a type, returning (type, enum name if any).
    fn parse_type(&mut self) -> Result<(Ty, Option<String>), CompileError> {
        loop {
            if self.is_ident("const") || self.is_ident("static") || self.is_ident("extern") || self.is_ident("unsigned") || self.is_ident("signed") {
                self.bump();
            } else {
                break;
            }
        }
        let name = self.ident()?;
        let ty = match name.as_str() {
            "void" => Ty::Void,
            "bool" => Ty::Bool,
            "char" | "uchar" | "short" | "ushort" | "int" | "uint" | "long" | "ulong" | "datetime" | "color" => Ty::Int,
            "float" | "double" => Ty::Double,
            "string" => Ty::Str,
            _ => {
                if name.starts_with("ENUM_") || self.enum_types.contains(&name) {
                    // allow `const` after type etc.
                    return Ok((Ty::Int, Some(name)));
                }
                let sym = self.interner.intern(&name);
                if self.structs.contains_key(&sym) {
                    Ty::Struct(sym)
                } else if builtins::BUILTIN_CLASSES.contains(&name.as_str()) {
                    Ty::Object(sym)
                } else {
                    return Err(self.err_here(format!("'{}' - unknown type", name)));
                }
            }
        };
        // skip `int unsigned`-like noise and `const` after type
        while self.is_ident("const") {
            self.bump();
        }
        Ok((ty, None))
    }

    // ---------- top level ----------

    fn parse_top(&mut self) -> Result<(), CompileError> {
        loop {
            match self.peek().clone() {
                Tok::Eof => break,
                Tok::Punct(";") => {
                    self.bump();
                }
                Tok::Ident(s) if s == "input" || s == "sinput" => self.parse_input()?,
                Tok::Ident(s) if s == "enum" => self.parse_enum()?,
                Tok::Ident(s) if s == "struct" => self.parse_struct()?,
                Tok::Ident(s) if s == "class" || s == "interface" || s == "template" => {
                    return Err(self.err_here(format!("'{}' - classes and templates are not supported in this version; use functions and structs", s)));
                }
                Tok::Ident(s) if s == "extern" => {
                    self.bump();
                }
                _ if self.is_type_start() => self.parse_global_decl_or_func()?,
                _ => return Err(self.err_here(format!("'{}' - unexpected token", self.tok_text()))),
            }
        }
        Ok(())
    }

    fn parse_input(&mut self) -> Result<(), CompileError> {
        let line = self.tok().line;
        self.bump();
        if self.is_ident("group") {
            self.bump();
            if let Tok::Str(_) = self.peek() {
                self.bump();
            }
            self.eat_punct(";");
            return Ok(());
        }
        let (ty, enum_name) = self.parse_type()?;
        loop {
            let name = self.ident()?;
            let sym = self.interner.intern(&name);
            let init = if self.eat_punct("=") { Some(self.parse_assign()?) } else { None };
            let id = self.new_id();
            let label = self.comments.get(&line).cloned().filter(|c| !c.is_empty()).unwrap_or_else(|| name.clone());
            self.inputs.push(InputDecl { name: sym, ty: ty.clone(), enum_name: enum_name.clone(), label, decl_index: self.globals.len() });
            self.globals.push(VarDecl { id, name: sym, ty: ty.clone(), dims: vec![], init, is_static: false, line });
            if !self.eat_punct(",") {
                break;
            }
        }
        self.expect(";")
    }

    fn parse_enum(&mut self) -> Result<(), CompileError> {
        self.bump();
        let name = self.ident()?;
        self.enum_types.insert(name.clone());
        self.type_names.insert(name.clone());
        self.expect("{")?;
        let mut v = 0i64;
        let mut members = vec![];
        while !self.is_punct("}") {
            let m = self.ident()?;
            if self.eat_punct("=") {
                let e = self.parse_ternary()?;
                v = self.const_eval(&e).ok_or_else(|| self.err_here("constant expression expected".into()))?;
            }
            let sym = self.interner.intern(&m);
            self.enum_consts.insert(sym, v);
            members.push((m, v));
            v += 1;
            if !self.eat_punct(",") {
                break;
            }
        }
        self.expect("}")?;
        self.eat_punct(";");
        self.enums.insert(name, members);
        Ok(())
    }

    fn const_eval(&self, e: &Expr) -> Option<i64> {
        match e {
            Expr::Int(v) => Some(*v),
            Expr::Unary(UnOp::Neg, x) => self.const_eval(x).map(|v| -v),
            Expr::Var(s) => self
                .enum_consts
                .get(s)
                .copied()
                .or_else(|| builtins::constant(self.interner.name(*s)).and_then(|c| c.as_i64())),
            Expr::Binary(op, a, b) => {
                let (a, b) = (self.const_eval(a)?, self.const_eval(b)?);
                Some(match op {
                    BinOp::Add => a + b,
                    BinOp::Sub => a - b,
                    BinOp::Mul => a * b,
                    BinOp::Div if b != 0 => a / b,
                    BinOp::Shl => a << b,
                    BinOp::BitOr => a | b,
                    _ => return None,
                })
            }
            _ => None,
        }
    }

    fn parse_struct(&mut self) -> Result<(), CompileError> {
        self.bump();
        let name = self.ident()?;
        let sym = self.interner.intern(&name);
        self.type_names.insert(name.clone());
        // register early to allow self-reference errors to be clear
        self.structs.insert(sym, Rc::new(StructDef { name: sym, fields: vec![] }));
        self.expect("{")?;
        let mut fields = vec![];
        while !self.is_punct("}") {
            if self.is_ident("public") || self.is_ident("private") || self.is_ident("protected") {
                self.bump();
                self.expect(":")?;
                continue;
            }
            let (ty, _) = self.parse_type()?;
            loop {
                let f = self.ident()?;
                if self.is_punct("(") {
                    return Err(self.err_here("methods inside structs are not supported in this version".into()));
                }
                let mut dims = vec![];
                while self.eat_punct("[") {
                    if self.eat_punct("]") {
                        dims.push(None);
                    } else {
                        dims.push(Some(self.parse_expr()?));
                        self.expect("]")?;
                    }
                }
                fields.push((self.interner.intern(&f), ty.clone(), dims));
                if !self.eat_punct(",") {
                    break;
                }
            }
            self.expect(";")?;
        }
        self.expect("}")?;
        self.eat_punct(";");
        self.structs.insert(sym, Rc::new(StructDef { name: sym, fields }));
        Ok(())
    }

    fn parse_global_decl_or_func(&mut self) -> Result<(), CompileError> {
        let line = self.tok().line;
        let is_static = self.is_ident("static");
        let (ty, _) = self.parse_type()?;
        // function?
        if matches!(self.peek(), Tok::Ident(_)) && matches!(self.peek_at(1), Tok::Punct("(")) {
            let name = self.ident()?;
            let sym = self.interner.intern(&name);
            self.bump(); // (
            let params = self.parse_params()?;
            self.eat_ident_seq(&["const"]);
            if self.eat_punct(";") {
                return Ok(()); // prototype
            }
            self.expect("{")?;
            let body = self.parse_block_body()?;
            let f = Rc::new(Func { name: sym, ret: ty, params, body, line });
            self.funcs.entry(sym).or_default().push(f);
            return Ok(());
        }
        let decls = self.parse_declarators(ty, is_static, line)?;
        self.expect(";")?;
        self.globals.extend(decls);
        Ok(())
    }

    fn eat_ident_seq(&mut self, words: &[&str]) {
        while words.iter().any(|w| self.is_ident(w)) {
            self.bump();
        }
    }

    fn parse_params(&mut self) -> Result<Vec<Param>, CompileError> {
        let mut params = vec![];
        if self.eat_punct(")") {
            return Ok(params);
        }
        if self.is_ident("void") && matches!(self.peek_at(1), Tok::Punct(")")) {
            self.bump();
            self.bump();
            return Ok(params);
        }
        loop {
            let (ty, _) = self.parse_type()?;
            let by_ref = self.eat_punct("&");
            let name = self.ident()?;
            let mut is_array = false;
            while self.eat_punct("[") {
                is_array = true;
                while !self.is_punct("]") {
                    self.bump();
                }
                self.expect("]")?;
            }
            let default = if self.eat_punct("=") { Some(self.parse_assign()?) } else { None };
            params.push(Param { name: self.interner.intern(&name), ty, by_ref, is_array, default });
            if self.eat_punct(")") {
                break;
            }
            self.expect(",")?;
        }
        Ok(params)
    }

    fn parse_declarators(&mut self, ty: Ty, is_static: bool, line: u32) -> Result<Vec<VarDecl>, CompileError> {
        let mut out = vec![];
        loop {
            self.eat_punct("&");
            let name = self.ident()?;
            let mut dims = vec![];
            while self.eat_punct("[") {
                if self.eat_punct("]") {
                    dims.push(None);
                } else {
                    dims.push(Some(self.parse_expr()?));
                    self.expect("]")?;
                }
            }
            let init = if self.eat_punct("=") {
                if self.is_punct("{") {
                    Some(self.parse_init_list()?)
                } else {
                    Some(self.parse_assign()?)
                }
            } else if self.is_punct("(") && matches!(ty, Ty::Object(_)) {
                // constructor arguments are ignored
                while !self.eat_punct(")") {
                    self.bump();
                }
                None
            } else {
                None
            };
            let id = self.new_id();
            out.push(VarDecl { id, name: self.interner.intern(&name), ty: ty.clone(), dims, init, is_static, line });
            if !self.eat_punct(",") {
                break;
            }
        }
        Ok(out)
    }

    fn parse_init_list(&mut self) -> Result<Expr, CompileError> {
        self.expect("{")?;
        let mut items = vec![];
        while !self.is_punct("}") {
            if self.is_punct("{") {
                items.push(self.parse_init_list()?);
            } else {
                items.push(self.parse_assign()?);
            }
            if !self.eat_punct(",") {
                break;
            }
        }
        self.expect("}")?;
        Ok(Expr::List(items))
    }

    // ---------- statements ----------

    fn parse_block_body(&mut self) -> Result<Vec<Stmt>, CompileError> {
        let mut out = vec![];
        while !self.is_punct("}") {
            if matches!(self.peek(), Tok::Eof) {
                return Err(self.err_here("'}' - unbalanced parentheses / unexpected end of program".into()));
            }
            out.push(self.parse_stmt()?);
        }
        self.bump();
        Ok(out)
    }

    fn mk(&self, kind: StmtKind, line: u32, col: u32) -> Stmt {
        Stmt { kind, line, col }
    }

    fn looks_like_decl(&self) -> bool {
        if !self.is_type_start() {
            return false;
        }
        // `Type name`, `Type &name`, `const ...`, `static ...`
        match self.peek() {
            Tok::Ident(s) if s == "const" || s == "static" => true,
            _ => matches!(self.peek_at(1), Tok::Ident(_)) || matches!(self.peek_at(1), Tok::Punct("&")),
        }
    }

    fn parse_stmt(&mut self) -> Result<Stmt, CompileError> {
        let (line, col) = (self.tok().line, self.tok().col);
        if self.eat_punct("{") {
            let b = self.parse_block_body()?;
            return Ok(self.mk(StmtKind::Block(b), line, col));
        }
        if self.eat_punct(";") {
            return Ok(self.mk(StmtKind::Empty, line, col));
        }
        if let Tok::Ident(w) = self.peek().clone() {
            match w.as_str() {
                "if" => {
                    self.bump();
                    self.expect("(")?;
                    let c = self.parse_expr()?;
                    self.expect(")")?;
                    let t = self.parse_stmt()?;
                    let e = if self.is_ident("else") {
                        self.bump();
                        Some(Box::new(self.parse_stmt()?))
                    } else {
                        None
                    };
                    return Ok(self.mk(StmtKind::If(c, Box::new(t), e), line, col));
                }
                "while" => {
                    self.bump();
                    self.expect("(")?;
                    let c = self.parse_expr()?;
                    self.expect(")")?;
                    let b = self.parse_stmt()?;
                    return Ok(self.mk(StmtKind::While(c, Box::new(b)), line, col));
                }
                "do" => {
                    self.bump();
                    let b = self.parse_stmt()?;
                    if !self.is_ident("while") {
                        return Err(self.err_here("'while' expected".into()));
                    }
                    self.bump();
                    self.expect("(")?;
                    let c = self.parse_expr()?;
                    self.expect(")")?;
                    self.expect(";")?;
                    return Ok(self.mk(StmtKind::DoWhile(Box::new(b), c), line, col));
                }
                "for" => {
                    self.bump();
                    self.expect("(")?;
                    let init = if self.eat_punct(";") {
                        None
                    } else if self.looks_like_decl() {
                        let is_static = self.is_ident("static");
                        let (ty, _) = self.parse_type()?;
                        let d = self.parse_declarators(ty, is_static, line)?;
                        self.expect(";")?;
                        Some(Box::new(self.mk(StmtKind::Decl(d), line, col)))
                    } else {
                        let e = self.parse_expr()?;
                        self.expect(";")?;
                        Some(Box::new(self.mk(StmtKind::Expr(e), line, col)))
                    };
                    let cond = if self.is_punct(";") { None } else { Some(self.parse_expr()?) };
                    self.expect(";")?;
                    let step = if self.is_punct(")") { None } else { Some(self.parse_expr()?) };
                    self.expect(")")?;
                    let body = self.parse_stmt()?;
                    return Ok(self.mk(StmtKind::For(init, cond, step, Box::new(body)), line, col));
                }
                "return" => {
                    self.bump();
                    let e = if self.is_punct(";") { None } else { Some(self.parse_expr()?) };
                    self.expect(";")?;
                    return Ok(self.mk(StmtKind::Return(e), line, col));
                }
                "break" => {
                    self.bump();
                    self.expect(";")?;
                    return Ok(self.mk(StmtKind::Break, line, col));
                }
                "continue" => {
                    self.bump();
                    self.expect(";")?;
                    return Ok(self.mk(StmtKind::Continue, line, col));
                }
                "switch" => {
                    self.bump();
                    self.expect("(")?;
                    let v = self.parse_expr()?;
                    self.expect(")")?;
                    self.expect("{")?;
                    let mut arms: Vec<SwitchArm> = vec![];
                    while !self.is_punct("}") {
                        if self.is_ident("case") {
                            self.bump();
                            let e = self.parse_ternary()?;
                            self.expect(":")?;
                            arms.push(SwitchArm { label: Some(e), body: vec![] });
                        } else if self.is_ident("default") {
                            self.bump();
                            self.expect(":")?;
                            arms.push(SwitchArm { label: None, body: vec![] });
                        } else {
                            let s = self.parse_stmt()?;
                            match arms.last_mut() {
                                Some(a) => a.body.push(s),
                                None => return Err(self.err_here("'case' expected".into())),
                            }
                        }
                    }
                    self.bump();
                    return Ok(self.mk(StmtKind::Switch(v, arms), line, col));
                }
                _ => {}
            }
        }
        if self.looks_like_decl() {
            let is_static = self.is_ident("static");
            let (ty, _) = self.parse_type()?;
            let d = self.parse_declarators(ty, is_static, line)?;
            self.expect(";")?;
            return Ok(self.mk(StmtKind::Decl(d), line, col));
        }
        let e = self.parse_expr()?;
        self.expect(";")?;
        Ok(self.mk(StmtKind::Expr(e), line, col))
    }

    // ---------- expressions ----------

    fn parse_expr(&mut self) -> Result<Expr, CompileError> {
        let mut e = self.parse_assign()?;
        // comma operator (used in for-steps): evaluate all, keep last
        while self.is_punct(",") && self.in_comma_context() {
            self.bump();
            let r = self.parse_assign()?;
            e = Expr::Call(self.interner.intern("__comma"), vec![e, r]);
        }
        Ok(e)
    }

    fn in_comma_context(&self) -> bool {
        true
    }

    fn parse_assign(&mut self) -> Result<Expr, CompileError> {
        let lhs = self.parse_ternary()?;
        let op = match self.peek() {
            Tok::Punct("=") => Some(None),
            Tok::Punct("+=") => Some(Some(BinOp::Add)),
            Tok::Punct("-=") => Some(Some(BinOp::Sub)),
            Tok::Punct("*=") => Some(Some(BinOp::Mul)),
            Tok::Punct("/=") => Some(Some(BinOp::Div)),
            Tok::Punct("%=") => Some(Some(BinOp::Mod)),
            Tok::Punct("&=") => Some(Some(BinOp::BitAnd)),
            Tok::Punct("|=") => Some(Some(BinOp::BitOr)),
            Tok::Punct("^=") => Some(Some(BinOp::BitXor)),
            Tok::Punct("<<=") => Some(Some(BinOp::Shl)),
            Tok::Punct(">>=") => Some(Some(BinOp::Shr)),
            _ => None,
        };
        if let Some(op) = op {
            if !matches!(lhs, Expr::Var(_) | Expr::Index(..) | Expr::Member(..)) {
                return Err(self.err_here("'=' - l-value required".into()));
            }
            self.bump();
            let rhs = if self.is_punct("{") { self.parse_init_list()? } else { self.parse_assign()? };
            return Ok(Expr::Assign(op, Box::new(lhs), Box::new(rhs)));
        }
        Ok(lhs)
    }

    fn parse_ternary(&mut self) -> Result<Expr, CompileError> {
        let c = self.parse_bin(1)?;
        if self.eat_punct("?") {
            let a = self.parse_assign()?;
            self.expect(":")?;
            let b = self.parse_assign()?;
            return Ok(Expr::Ternary(Box::new(c), Box::new(a), Box::new(b)));
        }
        Ok(c)
    }

    fn bin_op(&self) -> Option<(BinOp, u8)> {
        let p = match self.peek() {
            Tok::Punct(p) => *p,
            _ => return None,
        };
        Some(match p {
            "||" => (BinOp::Or, 1),
            "&&" => (BinOp::And, 2),
            "|" => (BinOp::BitOr, 3),
            "^" => (BinOp::BitXor, 4),
            "&" => (BinOp::BitAnd, 5),
            "==" => (BinOp::Eq, 6),
            "!=" => (BinOp::Ne, 6),
            "<" => (BinOp::Lt, 7),
            "<=" => (BinOp::Le, 7),
            ">" => (BinOp::Gt, 7),
            ">=" => (BinOp::Ge, 7),
            "<<" => (BinOp::Shl, 8),
            ">>" => (BinOp::Shr, 8),
            "+" => (BinOp::Add, 9),
            "-" => (BinOp::Sub, 9),
            "*" => (BinOp::Mul, 10),
            "/" => (BinOp::Div, 10),
            "%" => (BinOp::Mod, 10),
            _ => return None,
        })
    }

    fn parse_bin(&mut self, min: u8) -> Result<Expr, CompileError> {
        let mut lhs = self.parse_unary()?;
        while let Some((op, prec)) = self.bin_op() {
            if prec < min {
                break;
            }
            self.bump();
            let rhs = self.parse_bin(prec + 1)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, CompileError> {
        match self.peek().clone() {
            Tok::Punct("-") => {
                self.bump();
                let e = self.parse_unary()?;
                Ok(match e {
                    Expr::Int(v) => Expr::Int(-v),
                    Expr::Double(v) => Expr::Double(-v),
                    e => Expr::Unary(UnOp::Neg, Box::new(e)),
                })
            }
            Tok::Punct("+") => {
                self.bump();
                let e = self.parse_unary()?;
                Ok(Expr::Unary(UnOp::Plus, Box::new(e)))
            }
            Tok::Punct("!") => {
                self.bump();
                Ok(Expr::Unary(UnOp::Not, Box::new(self.parse_unary()?)))
            }
            Tok::Punct("~") => {
                self.bump();
                Ok(Expr::Unary(UnOp::BitNot, Box::new(self.parse_unary()?)))
            }
            Tok::Punct("++") | Tok::Punct("--") => {
                let inc = self.is_punct("++");
                self.bump();
                let t = self.parse_unary()?;
                Ok(Expr::IncDec { pre: true, inc, target: Box::new(t) })
            }
            Tok::Punct("(") => {
                // cast?
                if let Tok::Ident(n) = self.peek_at(1).clone() {
                    let is_type = self.type_names.contains(&n) || n.starts_with("ENUM_");
                    if is_type && matches!(self.peek_at(2), Tok::Punct(")")) {
                        self.bump();
                        let (ty, _) = self.parse_type()?;
                        self.expect(")")?;
                        let e = self.parse_unary()?;
                        return Ok(Expr::Cast(ty, Box::new(e)));
                    }
                }
                self.parse_postfix()
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_args(&mut self) -> Result<Vec<Expr>, CompileError> {
        let mut args = vec![];
        if self.eat_punct(")") {
            return Ok(args);
        }
        loop {
            args.push(self.parse_assign()?);
            if self.eat_punct(")") {
                break;
            }
            self.expect(",")?;
        }
        Ok(args)
    }

    fn parse_postfix(&mut self) -> Result<Expr, CompileError> {
        let mut e = self.parse_primary()?;
        loop {
            if self.eat_punct("[") {
                let i = self.parse_expr()?;
                self.expect("]")?;
                e = Expr::Index(Box::new(e), Box::new(i));
            } else if self.is_punct(".") || self.is_punct("->") {
                self.bump();
                let name = self.ident()?;
                let sym = self.interner.intern(&name);
                if self.eat_punct("(") {
                    let args = self.parse_args()?;
                    e = Expr::MethodCall(Box::new(e), sym, args);
                } else {
                    e = Expr::Member(Box::new(e), sym);
                }
            } else if self.is_punct("++") || self.is_punct("--") {
                let inc = self.is_punct("++");
                self.bump();
                e = Expr::IncDec { pre: false, inc, target: Box::new(e) };
            } else {
                break;
            }
        }
        Ok(e)
    }

    fn parse_primary(&mut self) -> Result<Expr, CompileError> {
        let t = self.bump();
        match t.tok {
            Tok::Int(v) => Ok(Expr::Int(v)),
            Tok::Float(v) => Ok(Expr::Double(v)),
            Tok::Str(s) => Ok(Expr::Str(Rc::from(s.as_str()))),
            Tok::Punct("(") => {
                let e = self.parse_expr()?;
                self.expect(")")?;
                Ok(e)
            }
            Tok::Punct("::") => self.parse_primary(),
            Tok::Ident(name) => match name.as_str() {
                "true" => Ok(Expr::Bool(true)),
                "false" => Ok(Expr::Bool(false)),
                "NULL" => Ok(Expr::Null),
                _ => {
                    // function-style cast: double(x)
                    if self.is_punct("(") && PRIM_TYPES.contains(&name.as_str()) {
                        self.bump();
                        let ty = match name.as_str() {
                            "bool" => Ty::Bool,
                            "float" | "double" => Ty::Double,
                            "string" => Ty::Str,
                            _ => Ty::Int,
                        };
                        let e = self.parse_expr()?;
                        self.expect(")")?;
                        return Ok(Expr::Cast(ty, Box::new(e)));
                    }
                    let sym = self.interner.intern(&name);
                    if self.eat_punct("(") {
                        let args = self.parse_args()?;
                        return Ok(Expr::Call(sym, args));
                    }
                    if self.eat_punct("::") {
                        // ENUM::VALUE or Class::member
                        let m = self.ident()?;
                        return Ok(Expr::Var(self.interner.intern(&m)));
                    }
                    Ok(Expr::Var(sym))
                }
            },
            _ => {
                self.pos = self.pos.saturating_sub(1);
                Err(self.err_here(format!("'{}' - unexpected token", self.tok_text())))
            }
        }
    }
}

fn parse_color_value(v: &str) -> Option<u32> {
    let v = v.trim();
    if v.starts_with("C'") {
        return super::lexer::parse_color_literal(v.trim_start_matches("C'").trim_end_matches('\'')).map(|c| c as u32);
    }
    if let Some(h) = v.strip_prefix("0x") {
        return u32::from_str_radix(h, 16).ok();
    }
    if let Ok(n) = v.parse::<i64>() {
        return Some(n as u32);
    }
    builtins::constant(v).and_then(|c| c.as_i64()).map(|c| c as u32)
}
