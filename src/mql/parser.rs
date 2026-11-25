//! Parser for the MQL5 subset: preprocessor (#property, #define, #include), enums, structs,
//! inputs, globals and functions.

use super::ast::*;
use super::builtins;
use super::lexer::{lex, lex_fragment, CompileError, Tok, Token};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

pub type Loader<'a> = &'a dyn Fn(&str) -> Option<String>;

pub fn compile(file_name: &str, src: &str, loader: Loader) -> Result<Program, Vec<CompileError>> {
    let lexed = lex(src).map_err(|e| vec![e])?;
    let mut p = Parser::new(file_name, lexed.line_comments);
    let toks = p.preprocess(lexed.tokens, loader, 0).map_err(|e| vec![e])?;
    p.predeclare(&toks);
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
    macros: HashMap<String, Macro>,
    cond: Vec<Cond>,
    counter: i64,
    included: HashSet<String>,
    cur_dir: String,
    files: Vec<String>,
    site_file: u16,
    stmt_file: u16,
    type_names: HashSet<String>,
    enum_types: HashSet<String>,
    structs: HashMap<Sym, Rc<StructDef>>,
    classes: HashMap<Sym, ClassInfo>,
    generics: HashSet<String>,
    template_classes: HashSet<String>,
    aliases: HashMap<String, Ty>,
    last_enum: Option<String>,
    var_enums: HashMap<Sym, String>,
    cur_func: String,
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
            macros: predefined_macros(),
            cond: vec![],
            counter: 0,
            included: HashSet::new(),
            cur_dir: String::new(),
            files: vec![file_name.to_string()],
            site_file: 0,
            stmt_file: 0,
            type_names,
            enum_types: HashSet::new(),
            structs,
            classes: HashMap::new(),
            generics: HashSet::new(),
            template_classes: HashSet::new(),
            aliases: HashMap::new(),
            last_enum: None,
            var_enums: HashMap::new(),
            cur_func: String::new(),
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
            } else if has("OnInit") || has("OnTimer") || has("OnChartEvent") || has("OnTrade") || has("OnTradeTransaction") || has("OnBookEvent") {
                // an expert advisor does not need OnTick
                ProgramKind::Expert
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
            classes: self.classes,
            var_enums: self.var_enums,
            interner: self.interner,
            warnings: self.warnings,
            files: self.files,
        }
    }

    // ---------- preprocessor ----------

    fn pp_active(&self) -> bool {
        self.cond.last().map(|c| c.active).unwrap_or(true)
    }

    fn is_defined(&self, name: &str) -> bool {
        self.macros.contains_key(name) || matches!(name, "__LINE__" | "__FILE__" | "__DATE__" | "__DATETIME__" | "__MQLBUILD__" | "__MQL5BUILD__" | "__FUNCTION__" | "__FUNCSIG__" | "__PATH__" | "__COUNTER__")
    }

    fn preprocess(&mut self, toks: Vec<Token>, loader: Loader, depth: usize) -> Result<Vec<Token>, CompileError> {
        let mut out = Vec::with_capacity(toks.len());
        let cond_depth = self.cond.len();
        let mut i = 0;
        while i < toks.len() {
            let t = &toks[i];
            match &t.tok {
                Tok::Directive(d) => {
                    let (word, rest) = split_directive(d);
                    match word {
                        "ifdef" | "ifndef" => {
                            let parent = self.pp_active();
                            let name = rest.split_whitespace().next().unwrap_or("");
                            let def = self.is_defined(name);
                            let cond = parent && (if word == "ifdef" { def } else { !def });
                            self.cond.push(Cond { parent, active: cond, taken: cond });
                        }
                        "else" => match self.cond.last_mut() {
                            Some(c) => {
                                c.active = c.parent && !c.taken;
                                c.taken = true;
                            }
                            None => return Err(CompileError { line: t.line, col: t.col, msg: "'#else' without '#ifdef'".into() }),
                        },
                        "endif" => {
                            if self.cond.len() <= cond_depth {
                                return Err(CompileError { line: t.line, col: t.col, msg: "'#endif' without '#ifdef'".into() });
                            }
                            self.cond.pop();
                        }
                        _ => {
                            if self.pp_active() {
                                let d = d.clone();
                                let t = t.clone();
                                self.directive(&d, &t, &mut out, loader, depth)?;
                            }
                        }
                    }
                    i += 1;
                }
                _ if !self.pp_active() => i += 1,
                Tok::Eof => {
                    if depth == 0 {
                        if self.cond.len() > cond_depth {
                            return Err(CompileError { line: t.line, col: t.col, msg: "'#endif' expected".into() });
                        }
                        out.push(t.clone());
                    }
                    i += 1;
                }
                Tok::Ident(name) if self.macros.contains_key(name) || self.is_special_macro(name) => {
                    let fn_like = self.macros.get(name).is_some_and(|m| m.params.is_some());
                    let end = if fn_like && matches!(toks.get(i + 1).map(|t| &t.tok), Some(Tok::Punct("("))) {
                        collect_args(&toks, i + 1).map(|(_, e)| e).ok_or_else(|| CompileError { line: t.line, col: t.col, msg: format!("'{}' - unterminated macro call", name) })?
                    } else {
                        i + 1
                    };
                    let (line, col) = (t.line, t.col);
                    self.site_file = t.file;
                    let exp = self.expand_list(&toks[i..end], &HashSet::new(), line, col, 0)?;
                    out.extend(exp.into_iter().map(|mut x| {
                        x.file = t.file;
                        x
                    }));
                    i = end;
                }
                _ => {
                    out.push(t.clone());
                    i += 1;
                }
            }
        }
        if depth > 0 && self.cond.len() > cond_depth {
            // unbalanced #ifdef inside an include: close it at the end of the file
            self.cond.truncate(cond_depth);
        }
        Ok(out)
    }

    fn is_special_macro(&self, name: &str) -> bool {
        matches!(name, "__LINE__" | "__FILE__" | "__DATE__" | "__DATETIME__" | "__MQLBUILD__" | "__MQL5BUILD__" | "__PATH__" | "__COUNTER__")
    }

    fn special_macro(&mut self, name: &str, line: u32, col: u32) -> Option<Token> {
        let tok = match name {
            "__LINE__" => Tok::Int(line as i64),
            "__FILE__" => Tok::Str(self.file_name.clone()),
            "__PATH__" => Tok::Str(self.file_name.clone()),
            "__DATE__" | "__DATETIME__" => {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
                Tok::Int(if name == "__DATE__" { now - now % 86400 } else { now })
            }
            "__MQLBUILD__" | "__MQL5BUILD__" => Tok::Int(4620),
            "__COUNTER__" => {
                self.counter += 1;
                Tok::Int(self.counter - 1)
            }
            _ => return None,
        };
        Some(Token { tok, line, col, file: self.site_file })
    }

    /// Fully macro-expand a token list. `hide` holds macros being expanded (no recursion).
    fn expand_list(&mut self, toks: &[Token], hide: &HashSet<String>, line: u32, col: u32, level: usize) -> Result<Vec<Token>, CompileError> {
        if level > 64 {
            return Err(CompileError { line, col, msg: "macro expansion too deep".into() });
        }
        let mut out = vec![];
        let mut i = 0;
        while i < toks.len() {
            let name = match &toks[i].tok {
                Tok::Ident(n) if !hide.contains(n) => n.clone(),
                _ => {
                    let mut t = toks[i].clone();
                    t.line = line;
                    t.col = col;
                    t.file = self.site_file;
                    out.push(t);
                    i += 1;
                    continue;
                }
            };
            if let Some(t) = self.special_macro(&name, line, col) {
                out.push(t);
                i += 1;
                continue;
            }
            let m = match self.macros.get(&name) {
                Some(m) => m.clone(),
                None => {
                    let mut t = toks[i].clone();
                    t.line = line;
                    t.col = col;
                    t.file = self.site_file;
                    out.push(t);
                    i += 1;
                    continue;
                }
            };
            let mut h = hide.clone();
            h.insert(name.clone());
            match &m.params {
                None => {
                    out.extend(self.expand_list(&m.body, &h, line, col, level + 1)?);
                    i += 1;
                }
                Some(params) => {
                    if !matches!(toks.get(i + 1).map(|t| &t.tok), Some(Tok::Punct("("))) {
                        let mut t = toks[i].clone();
                        t.line = line;
                        t.col = col;
                        out.push(t);
                        i += 1;
                        continue;
                    }
                    let (args, end) = collect_args(toks, i + 1).ok_or_else(|| CompileError { line, col, msg: format!("'{}' - unterminated macro call", name) })?;
                    let args = if params.is_empty() && args.len() == 1 && args[0].is_empty() { vec![] } else { args };
                    if args.len() != params.len() {
                        return Err(CompileError { line, col, msg: format!("'{}' - wrong parameters count for macro", name) });
                    }
                    let mut expanded_args = vec![];
                    for a in &args {
                        expanded_args.push(self.expand_list(a, hide, line, col, level + 1)?);
                    }
                    let sub = substitute(params, &m.body, &args, &expanded_args, line, col)?;
                    out.extend(self.expand_list(&sub, &h, line, col, level + 1)?);
                    i = end;
                }
            }
        }
        Ok(out)
    }

    fn directive(&mut self, d: &str, t: &Token, out: &mut Vec<Token>, loader: Loader, depth: usize) -> Result<(), CompileError> {
        let err = |msg: String| CompileError { line: t.line, col: t.col, msg };
        let (word, rest) = split_directive(d);
        match word {
            "property" => self.property(rest),
            "define" => {
                let (name, body) = match rest.find(|c: char| c.is_whitespace() || c == '(') {
                    Some(p) => (&rest[..p], &rest[p..]),
                    None => (rest, ""),
                };
                let (params, body) = if let Some(after) = body.strip_prefix('(') {
                    let close = after.find(')').ok_or_else(|| err(format!("'{}' - ')' expected in macro parameters", name)))?;
                    let params: Vec<String> = after[..close].split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                    (Some(params), &after[close + 1..])
                } else {
                    (None, body)
                };
                let lexed = lex_fragment(body).map_err(|e| err(e.msg))?;
                let mut toks = lexed.tokens;
                toks.pop(); // Eof
                self.macros.insert(name.to_string(), Macro { params, body: toks });
            }
            "undef" => {
                self.macros.remove(rest);
            }
            "include" => {
                let quoted = rest.starts_with('"');
                let path = rest.trim_matches(|c| c == '<' || c == '>' || c == '"').replace('\\', "/");
                if depth > 16 {
                    return Err(err("include nesting too deep".into()));
                }
                // quoted includes are looked up next to the including file first
                let mut cands = vec![];
                if quoted && !self.cur_dir.is_empty() {
                    cands.push(normalize_path(&format!("{}/{}", self.cur_dir, path)));
                }
                cands.push(normalize_path(&path));
                let native = |p: &str| {
                    let l = p.to_ascii_lowercase();
                    builtins::NATIVE_HEADERS.iter().any(|h| l == *h || l.ends_with(&format!("/{}", h)))
                };
                // the real header is used when present; otherwise the engine's own classes
                let found = cands.iter().find_map(|c| loader(c).map(|src| (c.clone(), src)));
                if found.is_none() && cands.iter().any(|c| native(c)) {
                    return Ok(());
                }
                match found {
                    Some((resolved, src)) => {
                        let key = resolved.to_ascii_lowercase();
                        if !self.included.insert(key) {
                            return Ok(()); // every include file is read once
                        }
                        let mut lexed = lex(&src).map_err(|e| err(format!("{}: {}", resolved, e.msg)))?;
                        self.files.push(resolved.clone());
                        let fidx = (self.files.len() - 1).min(u16::MAX as usize - 1) as u16;
                        for tk in lexed.tokens.iter_mut() {
                            tk.file = fidx;
                        }
                        let saved = std::mem::replace(&mut self.file_name, resolved.clone());
                        let dir = resolved.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
                        let saved_dir = std::mem::replace(&mut self.cur_dir, dir);
                        let inner = self.preprocess(lexed.tokens, loader, depth + 1);
                        self.file_name = saved;
                        self.cur_dir = saved_dir;
                        out.extend(inner.map_err(|e| if e.msg.contains(".mqh") { e } else { err(format!("{} ({}): {}", resolved, e.line, e.msg)) })?);
                    }
                    None => return Err(err(format!("can't open \"{}\" include file", path))),
                }
            }
            "import" => return Err(err("DLL imports are not allowed in this terminal".into())),
            "resource" | "pragma" => {
                self.warnings.push(format!("line {}: #{} ignored", t.line, word));
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

    /// Class and struct names are known before parsing, so a class may refer to one
    /// defined later in the file.
    fn predeclare(&mut self, toks: &[Token]) {
        for w in toks.windows(2) {
            if let (Tok::Ident(k), Tok::Ident(n)) = (&w[0].tok, &w[1].tok) {
                if (k == "class" || k == "struct" || k == "interface" || k == "union") && !self.type_names.contains(n) {
                    let sym = self.interner.intern(n);
                    self.type_names.insert(n.clone());
                    self.structs.insert(sym, Rc::new(StructDef { name: sym, fields: vec![] }));
                    self.classes.insert(sym, ClassInfo { is_class: k != "struct", ..Default::default() });
                }
            }
        }
        // template classes: `template<...> class X`
        for i in 0..toks.len() {
            if toks[i].tok == Tok::Ident("template".into()) {
                let mut j = i + 1;
                let mut depth = 0;
                while j < toks.len() {
                    match &toks[j].tok {
                        Tok::Punct("<") => depth += 1,
                        Tok::Punct(">") => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                if let (Some(Tok::Ident(k)), Some(Tok::Ident(n))) = (toks.get(j + 1).map(|t| &t.tok), toks.get(j + 2).map(|t| &t.tok)) {
                    if k == "class" || k == "struct" || k == "interface" {
                        self.template_classes.insert(n.clone());
                    }
                }
            }
        }
    }

    /// `typedef RetType (*Name)(params);` (function pointer) or `typedef Type Name;`.
    fn parse_typedef(&mut self) -> Result<(), CompileError> {
        self.bump();
        let (ty, _) = self.parse_type()?;
        if self.eat_punct("(") {
            self.expect("*")?;
            let n = self.ident()?;
            self.expect(")")?;
            while !self.is_punct(";") && !matches!(self.peek(), Tok::Eof) {
                self.bump();
            }
            self.eat_punct(";");
            self.type_names.insert(n.clone());
            self.aliases.insert(n, Ty::Any);
            return Ok(());
        }
        let n = self.ident()?;
        self.eat_punct(";");
        self.type_names.insert(n.clone());
        self.aliases.insert(n, ty);
        Ok(())
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
        match self.files.get(t.file as usize) {
            Some(f) if t.file > 0 => CompileError { line: t.line, col: t.col, msg: format!("{} ({}): {}", f, t.line, msg) },
            _ => CompileError { line: t.line, col: t.col, msg },
        }
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
        let r = self.parse_type_inner();
        self.last_enum = match &r {
            Ok((_, e)) => e.clone(),
            Err(_) => None,
        };
        r
    }

    fn parse_type_inner(&mut self) -> Result<(Ty, Option<String>), CompileError> {
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
                if self.generics.contains(&name) {
                    Ty::Any
                } else if let Some(t) = self.aliases.get(&name) {
                    t.clone()
                } else if name.starts_with("ENUM_") || self.enum_types.contains(&name) {
                    // allow `const` after type etc.
                    return Ok((Ty::Int, Some(name)));
                } else {
                    let sym = self.interner.intern(&name);
                    if self.structs.contains_key(&sym) {
                        self.skip_template_args();
                        Ty::Struct(sym)
                    } else if builtins::BUILTIN_CLASSES.contains(&name.as_str()) {
                        Ty::Object(sym)
                    } else {
                        return Err(self.err_here(format!("'{}' - unknown type", name)));
                    }
                }
            }
        };
        // skip `int unsigned`-like noise and `const` after type
        while self.is_ident("const") {
            self.bump();
        }
        let mut ty = ty;
        if self.is_punct("*") && matches!(ty, Ty::Struct(_) | Ty::Object(_) | Ty::Any) {
            self.bump();
            if let Ty::Struct(s) = ty {
                ty = Ty::Ptr(s);
            }
            while self.is_ident("const") {
                self.bump();
            }
        }
        Ok((ty, None))
    }

    /// Skip `<...>` template arguments if present.
    fn skip_template_args(&mut self) {
        if !self.is_punct("<") {
            return;
        }
        let mut depth = 0i32;
        loop {
            match self.peek() {
                Tok::Punct("<") => depth += 1,
                Tok::Punct(">") => depth -= 1,
                Tok::Punct(">>") => depth -= 2,
                Tok::Eof => return,
                _ => {}
            }
            self.bump();
            if depth <= 0 {
                return;
            }
        }
    }

    /// Size in bytes of a type, as `sizeof` reports it.
    fn type_size(&self, t: &str) -> i64 {
        match t {
            "bool" | "char" | "uchar" => 1,
            "short" | "ushort" => 2,
            "int" | "uint" | "color" | "float" => 4,
            "long" | "ulong" | "datetime" | "double" => 8,
            "string" => 12,
            _ if t.starts_with("ENUM_") || self.enum_types.contains(t) => 4,
            _ => match self.interner.get(t).and_then(|s| self.structs.get(&s)) {
                Some(d) => d.fields.iter().map(|(_, ty, dims)| {
                    let one = match ty {
                        Ty::Bool => 1,
                        Ty::Int => 4,
                        Ty::Double => 8,
                        Ty::Str => 12,
                        Ty::Struct(s) => self.type_size(self.interner.name(*s)),
                        _ => 8,
                    };
                    let n: i64 = dims.iter().map(|d| match d {
                        Some(Expr::Int(k)) => *k,
                        _ => 1,
                    }).product();
                    one * n.max(1)
                }).sum(),
                None => 8,
            },
        }
    }

    fn var_size(&self, v: Sym) -> i64 {
        let ty = self.globals.iter().rev().find(|g| g.name == v).map(|g| g.ty.clone());
        match ty {
            Some(Ty::Struct(s)) => self.type_size(self.interner.name(s)),
            Some(Ty::Double) => 8,
            Some(Ty::Str) => 12,
            Some(Ty::Bool) => 1,
            Some(Ty::Int) => 4,
            _ => 8,
        }
    }

    fn is_class_name(&self, name: &str) -> bool {
        self.interner.get(name).is_some_and(|s| self.classes.contains_key(&s))
    }

    /// `template<typename T, ...>` followed by a class or a function.
    fn parse_template(&mut self) -> Result<(), CompileError> {
        self.bump();
        self.expect("<")?;
        let mut added = vec![];
        loop {
            if self.is_ident("typename") || self.is_ident("class") {
                self.bump();
            }
            let n = self.ident()?;
            if self.generics.insert(n.clone()) {
                self.type_names.insert(n.clone());
                added.push(n);
            }
            if !self.eat_punct(",") {
                break;
            }
        }
        self.expect(">")?;
        let at_ctor = match self.peek().clone() {
            Tok::Ident(n) => self.is_class_name(&n) && self.is_ctor_def(),
            _ => false,
        };
        let r = if self.is_ident("class") || self.is_ident("struct") || self.is_ident("interface") {
            self.parse_class(true)
        } else if at_ctor {
            self.parse_ctor_def()
        } else {
            self.parse_global_decl_or_func()
        };
        for n in added {
            self.generics.remove(&n);
            self.type_names.remove(&n);
        }
        r
    }

    fn parse_operator_name(&mut self) -> Result<String, CompileError> {
        let t = self.bump();
        Ok(match t.tok {
            Tok::Punct("[") => {
                self.expect("]")?;
                "[]".into()
            }
            Tok::Punct("(") => {
                self.expect(")")?;
                "()".into()
            }
            Tok::Punct(p) => p.to_string(),
            Tok::Ident(i) => i,
            _ => return Err(self.err_here("operator expected".into())),
        })
    }

    /// class / struct / interface definition (also forward declarations).
    fn parse_class(&mut self, template: bool) -> Result<(), CompileError> {
        let kw = self.ident()?;
        let is_class = kw != "struct" && kw != "union";
        let name = self.ident()?;
        let sym = self.interner.intern(&name);
        self.type_names.insert(name.clone());
        if template {
            self.template_classes.insert(name.clone());
        }
        self.skip_template_args();
        if !self.structs.contains_key(&sym) {
            self.structs.insert(sym, Rc::new(StructDef { name: sym, fields: vec![] }));
        }
        self.classes.entry(sym).or_insert_with(|| ClassInfo { is_class, ..Default::default() });
        if self.eat_punct(";") {
            return Ok(()); // forward declaration
        }
        let mut base = None;
        if self.eat_punct(":") {
            self.eat_ident_seq(&["public", "private", "protected", "virtual"]);
            let b = self.ident()?;
            self.skip_template_args();
            let bs = self.interner.intern(&b);
            if !self.structs.contains_key(&bs) {
                return Err(self.err_here(format!("'{}' - unknown base class", b)));
            }
            base = Some(bs);
        }
        self.expect("{")?;
        let mut fields: Vec<(Sym, Ty, Vec<Option<Expr>>)> = base.map(|b| self.structs[&b].fields.clone()).unwrap_or_default();
        let mut own = vec![];
        while !self.is_punct("}") {
            if matches!(self.peek(), Tok::Eof) {
                return Err(self.err_here("'}' - unexpected end of program in class definition".into()));
            }
            if self.is_ident("public") || self.is_ident("private") || self.is_ident("protected") {
                self.bump();
                self.expect(":")?;
                continue;
            }
            if self.eat_punct(";") {
                continue;
            }
            if self.is_ident("enum") {
                self.parse_enum()?;
                continue;
            }
            if (self.is_ident("struct") || self.is_ident("class")) && matches!(self.peek_at(2), Tok::Punct("{") | Tok::Punct(":") | Tok::Punct(";")) {
                self.parse_class(false)?;
                continue;
            }
            if self.is_ident("typedef") {
                self.parse_typedef()?;
                continue;
            }
            if self.is_ident("friend") {
                while !self.is_punct(";") && !matches!(self.peek(), Tok::Eof) {
                    self.bump();
                }
                continue;
            }
            if self.is_ident("template") {
                // member template: make its parameters known for the rest of the class
                self.bump();
                self.expect("<")?;
                loop {
                    if self.is_ident("typename") || self.is_ident("class") {
                        self.bump();
                    }
                    let n = self.ident()?;
                    self.generics.insert(n.clone());
                    self.type_names.insert(n);
                    if !self.eat_punct(",") {
                        break;
                    }
                }
                self.expect(">")?;
                continue;
            }
            let mut is_static = false;
            loop {
                if self.is_ident("virtual") || self.is_ident("inline") || self.is_ident("explicit") {
                    self.bump();
                } else if self.is_ident("static") {
                    is_static = true;
                    self.bump();
                } else {
                    break;
                }
            }
            let line = self.tok().line;
            if self.eat_punct("~") {
                let n = self.ident()?;
                self.expect("(")?;
                let params = self.parse_params()?;
                self.method_tail(sym, &format!("~{}", n), Ty::Void, params, line)?;
                continue;
            }
            if self.is_ident(&name) && matches!(self.peek_at(1), Tok::Punct("(")) {
                self.bump();
                self.bump();
                let params = self.parse_params()?;
                self.method_tail(sym, &name, Ty::Void, params, line)?;
                continue;
            }
            let (ty, _) = self.parse_type()?;
            if self.is_ident("operator") {
                self.bump();
                let op = self.parse_operator_name()?;
                self.expect("(")?;
                let params = self.parse_params()?;
                self.method_tail(sym, &format!("operator{}", op), ty, params, line)?;
                continue;
            }
            if matches!(self.peek(), Tok::Ident(_)) && matches!(self.peek_at(1), Tok::Punct("(")) {
                let n = self.ident()?;
                self.bump();
                let params = self.parse_params()?;
                self.method_tail(sym, &n, ty, params, line)?;
                continue;
            }
            let decls = self.parse_declarators(ty, is_static, line)?;
            self.expect(";")?;
            for d in decls {
                if is_static {
                    self.globals.push(d);
                } else {
                    own.push(d.name);
                    fields.push((d.name, d.ty, d.dims));
                }
            }
        }
        self.expect("}")?;
        self.eat_punct(";");
        self.structs.insert(sym, Rc::new(StructDef { name: sym, fields }));
        let info = self.classes.entry(sym).or_default();
        info.base = base;
        info.own_fields = own;
        info.is_class = is_class;
        Ok(())
    }

    /// The part of a method after its parameter list: qualifiers, initializer list and body.
    fn method_tail(&mut self, cls: Sym, name: &str, ret: Ty, params: Vec<Param>, line: u32) -> Result<(), CompileError> {
        self.eat_ident_seq(&["const", "override", "final"]);
        let mut init_list = vec![];
        if self.eat_punct(":") {
            loop {
                let n = self.ident()?;
                self.skip_template_args();
                self.expect("(")?;
                let args = self.parse_args()?;
                init_list.push((self.interner.intern(&n), args));
                if !self.eat_punct(",") {
                    break;
                }
            }
        }
        if self.eat_punct("=") {
            // pure virtual: `= 0`
            self.bump();
            return self.expect(";");
        }
        if self.eat_punct(";") {
            return Ok(()); // declaration only; defined later or in a derived class
        }
        self.expect("{")?;
        let cls_name = self.interner.name(cls).to_string();
        let saved = std::mem::replace(&mut self.cur_func, format!("{}::{}", cls_name, name));
        let body = self.parse_block_body();
        self.cur_func = saved;
        let body = body?;
        let sym = self.interner.intern(name);
        let f = Rc::new(Func { name: sym, ret, params, body, line, class: Some(cls), init_list });
        self.classes.entry(cls).or_default().methods.entry(sym).or_default().push(f);
        Ok(())
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
                Tok::Ident(s) if s == "struct" || s == "class" || s == "interface" || s == "union" => self.parse_class(false)?,
                Tok::Ident(s) if s == "template" => self.parse_template()?,
                Tok::Ident(s) if s == "typedef" => self.parse_typedef()?,
                Tok::Ident(s) if self.is_class_name(&s) && matches!(self.peek_at(1), Tok::Punct("::") | Tok::Punct("<")) && self.is_ctor_def() => {
                    self.parse_ctor_def()?;
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
            if let Some(en) = &enum_name {
                self.var_enums.insert(sym, en.clone());
            }
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

    /// Out-of-class constructor / destructor: `CFoo::CFoo(...)` / `CFoo::~CFoo()`.
    fn parse_ctor_def(&mut self) -> Result<(), CompileError> {
        let line = self.tok().line;
        let s = self.ident()?;
        self.skip_template_args();
        self.expect("::")?;
        let dtor = self.eat_punct("~");
        let n = self.ident()?;
        self.expect("(")?;
        let params = self.parse_params()?;
        let cls = self.interner.intern(&s);
        let fname = if dtor { format!("~{}", n) } else { n };
        self.method_tail(cls, &fname, Ty::Void, params, line)
    }

    /// At `CFoo::CFoo(` or `CFoo<T>::~CFoo(`?
    fn is_ctor_def(&self) -> bool {
        let mut i = 1;
        if matches!(self.peek_at(1), Tok::Punct("<")) {
            let mut depth = 0;
            loop {
                match self.peek_at(i) {
                    Tok::Punct("<") => depth += 1,
                    Tok::Punct(">") => depth -= 1,
                    Tok::Eof => return false,
                    _ => {}
                }
                i += 1;
                if depth == 0 {
                    break;
                }
            }
        }
        if !matches!(self.peek_at(i), Tok::Punct("::")) {
            return false;
        }
        let name = match self.peek() {
            Tok::Ident(n) => n.clone(),
            _ => return false,
        };
        match self.peek_at(i + 1) {
            Tok::Punct("~") => true,
            Tok::Ident(n) => *n == name && matches!(self.peek_at(i + 2), Tok::Punct("(")),
            _ => false,
        }
    }

    fn parse_global_decl_or_func(&mut self) -> Result<(), CompileError> {
        let line = self.tok().line;
        let is_static = self.is_ident("static");
        let (ty, _) = self.parse_type()?;
        let is_static = is_static || self.eat_word("static");
        // out-of-class method or static member: `int CFoo::Bar(...)`, `int CFoo::count = 0;`
        if let Tok::Ident(cname) = self.peek().clone() {
            if self.is_class_name(&cname) && matches!(self.peek_at(1), Tok::Punct("::") | Tok::Punct("<")) {
                let save = self.pos;
                self.bump();
                self.skip_template_args();
                if self.eat_punct("::") {
                    let cls = self.interner.intern(&cname);
                    if self.eat_punct("~") {
                        let n = self.ident()?;
                        self.expect("(")?;
                        let params = self.parse_params()?;
                        return self.method_tail(cls, &format!("~{}", n), Ty::Void, params, line);
                    }
                    if self.is_ident("operator") {
                        self.bump();
                        let op = self.parse_operator_name()?;
                        self.expect("(")?;
                        let params = self.parse_params()?;
                        return self.method_tail(cls, &format!("operator{}", op), ty, params, line);
                    }
                    if matches!(self.peek(), Tok::Ident(_)) && matches!(self.peek_at(1), Tok::Punct("(")) {
                        let n = self.ident()?;
                        self.bump();
                        let params = self.parse_params()?;
                        return self.method_tail(cls, &n, ty, params, line);
                    }
                    let decls = self.parse_declarators(ty, true, line)?;
                    self.expect(";")?;
                    for d in decls {
                        // replace the declaration made inside the class
                        self.globals.retain(|g| g.name != d.name);
                        self.globals.push(d);
                    }
                    return Ok(());
                }
                self.pos = save;
            }
        }
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
            let saved = std::mem::replace(&mut self.cur_func, name.clone());
            let body = self.parse_block_body();
            self.cur_func = saved;
            let body = body?;
            let f = Rc::new(Func { name: sym, ret: ty, params, body, line, class: None, init_list: vec![] });
            self.funcs.entry(sym).or_default().push(f);
            return Ok(());
        }
        let decls = self.parse_declarators(ty, is_static, line)?;
        self.expect(";")?;
        self.globals.extend(decls);
        Ok(())
    }

    fn eat_word(&mut self, w: &str) -> bool {
        if self.is_ident(w) {
            self.bump();
            true
        } else {
            false
        }
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
            let (ty, enum_name) = self.parse_type()?;
            let by_ref = self.eat_punct("&");
            let name = self.ident()?;
            if let Some(en) = enum_name {
                let sym = self.interner.intern(&name);
                self.var_enums.insert(sym, en);
            }
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
        let enum_name = self.last_enum.clone();
        let mut out = vec![];
        let mut first = true;
        loop {
            self.eat_punct("&");
            let mut dty = ty.clone();
            if self.eat_punct("*") {
                if let Ty::Struct(s) = ty {
                    dty = Ty::Ptr(s);
                }
            } else if !first {
                if let Ty::Ptr(s) = ty {
                    dty = Ty::Struct(s);
                }
            }
            first = false;
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
            } else if self.is_punct("(") && matches!(dty, Ty::Object(_)) {
                // constructor arguments are ignored
                while !self.eat_punct(")") {
                    self.bump();
                }
                None
            } else if self.is_punct("(") && matches!(dty, Ty::Struct(_)) {
                self.bump();
                let args = self.parse_args()?;
                match dty {
                    Ty::Struct(s) => Some(Expr::New(s, args)),
                    _ => None,
                }
            } else {
                None
            };
            let id = self.new_id();
            let sym = self.interner.intern(&name);
            if let Some(en) = &enum_name {
                self.var_enums.insert(sym, en.clone());
            }
            out.push(VarDecl { id, name: sym, ty: dty, dims, init, is_static, line });
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
        Stmt { kind, line, col, file: self.stmt_file }
    }

    fn looks_like_decl(&self) -> bool {
        if !self.is_type_start() {
            return false;
        }
        // `Type name`, `Type &name`, `const ...`, `static ...`
        match self.peek() {
            Tok::Ident(s) if s == "const" || s == "static" => true,
            Tok::Ident(n) => {
                let n = n.clone();
                matches!(self.peek_at(1), Tok::Ident(_))
                    || matches!(self.peek_at(1), Tok::Punct("&"))
                    || (matches!(self.peek_at(1), Tok::Punct("*")) && matches!(self.peek_at(2), Tok::Ident(_)) && (self.is_class_name(&n) || self.generics.contains(&n) || builtins::BUILTIN_CLASSES.contains(&n.as_str())))
                    || (matches!(self.peek_at(1), Tok::Punct("<")) && self.template_classes.contains(&n))
            }
            _ => false,
        }
    }

    fn parse_stmt(&mut self) -> Result<Stmt, CompileError> {
        let (line, col) = (self.tok().line, self.tok().col);
        self.stmt_file = self.tok().file;
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
                "delete" => {
                    self.bump();
                    let e = self.parse_unary()?;
                    self.expect(";")?;
                    return Ok(self.mk(StmtKind::Expr(Expr::Delete(Box::new(e))), line, col));
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
        if (self.is_ident("struct") || self.is_ident("class") || self.is_ident("union")) && matches!(self.peek_at(2), Tok::Punct("{") | Tok::Punct(":")) {
            // a type defined inside a function
            self.parse_class(false)?;
            return Ok(self.mk(StmtKind::Empty, line, col));
        }
        if self.looks_like_decl() {
            let is_static = self.is_ident("static");
            let (ty, _) = self.parse_type()?;
            let is_static = is_static || self.eat_word("static");
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
            Tok::Punct("&") | Tok::Punct("*") => {
                // address-of / dereference: objects are references already
                self.bump();
                self.parse_unary()
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
                    let ptr_cast = matches!(self.peek_at(2), Tok::Punct("*") | Tok::Punct("&")) && matches!(self.peek_at(3), Tok::Punct(")"));
                    if is_type && (matches!(self.peek_at(2), Tok::Punct(")")) || ptr_cast) {
                        self.bump();
                        let (ty, _) = self.parse_type()?;
                        if self.eat_punct("&") {
                            // reference cast: same value
                            self.expect(")")?;
                            return self.parse_unary();
                        }
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
            Tok::Punct("::") => match self.parse_primary()? {
                Expr::Call(f, args) => Ok(Expr::GlobalCall(f, args)),
                other => Ok(other),
            },
            Tok::Ident(name) => match name.as_str() {
                "true" => Ok(Expr::Bool(true)),
                "false" => Ok(Expr::Bool(false)),
                "NULL" => Ok(Expr::Null),
                "__FUNCTION__" | "__FUNCSIG__" => Ok(Expr::Str(Rc::from(self.cur_func.as_str()))),
                "sizeof" => {
                    self.expect("(")?;
                    let size = match self.peek().clone() {
                        Tok::Ident(t) if (self.type_names.contains(&t) || t.starts_with("ENUM_")) && matches!(self.peek_at(1), Tok::Punct(")")) => {
                            self.bump();
                            self.type_size(&t)
                        }
                        _ => {
                            let e = self.parse_expr()?;
                            match e {
                                Expr::Var(v) => self.var_size(v),
                                _ => 8,
                            }
                        }
                    };
                    self.expect(")")?;
                    Ok(Expr::Int(size))
                }
                "new" => {
                    let n = self.ident()?;
                    let sym = self.interner.intern(&n);
                    self.skip_template_args();
                    let args = if self.eat_punct("(") { self.parse_args()? } else { vec![] };
                    Ok(Expr::New(sym, args))
                }
                "dynamic_cast" => {
                    self.skip_template_args();
                    self.expect("(")?;
                    let e = self.parse_expr()?;
                    self.expect(")")?;
                    Ok(e)
                }
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
                    if self.template_classes.contains(&name) && self.is_punct("<") {
                        self.skip_template_args();
                    }
                    if self.eat_punct("::") {
                        // ENUM::VALUE, Class::member or Class::Method(...)
                        let m = if self.is_punct("~") {
                            self.bump();
                            format!("~{}", self.ident()?)
                        } else {
                            self.ident()?
                        };
                        let msym = self.interner.intern(&m);
                        if self.eat_punct("(") {
                            let args = self.parse_args()?;
                            if self.structs.contains_key(&sym) {
                                return Ok(Expr::ScopedCall(sym, msym, args));
                            }
                            return Ok(Expr::Call(msym, args));
                        }
                        return Ok(Expr::Var(msym));
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

#[derive(Clone)]
struct Macro {
    params: Option<Vec<String>>,
    body: Vec<Token>,
}

struct Cond {
    parent: bool,
    active: bool,
    taken: bool,
}

fn predefined_macros() -> HashMap<String, Macro> {
    let mut m = HashMap::new();
    for n in ["__MQL5__", "__MQL__", "_RELEASE"] {
        m.insert(n.to_string(), Macro { params: None, body: vec![Token { tok: Tok::Int(1), line: 0, col: 0, file: 0 }] });
    }
    m
}

/// Arguments of a macro call starting at the '(' at `open`. Returns the arguments and the
/// index just past the closing ')'.
fn collect_args(toks: &[Token], open: usize) -> Option<(Vec<Vec<Token>>, usize)> {
    let mut depth = 0;
    let mut args = vec![vec![]];
    let mut i = open;
    while i < toks.len() {
        match &toks[i].tok {
            Tok::Punct("(") => {
                depth += 1;
                if depth > 1 {
                    args.last_mut().unwrap().push(toks[i].clone());
                }
            }
            Tok::Punct(")") => {
                depth -= 1;
                if depth == 0 {
                    return Some((args, i + 1));
                }
                args.last_mut().unwrap().push(toks[i].clone());
            }
            Tok::Punct(",") if depth == 1 => args.push(vec![]),
            Tok::Eof => return None,
            _ => args.last_mut().unwrap().push(toks[i].clone()),
        }
        i += 1;
    }
    None
}

fn token_text(t: &Tok) -> String {
    match t {
        Tok::Ident(s) => s.clone(),
        Tok::Int(v) => v.to_string(),
        Tok::Float(v) => {
            let s = v.to_string();
            if s.contains('.') || s.contains('e') { s } else { format!("{}.0", s) }
        }
        Tok::Str(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        Tok::Punct(p) => p.to_string(),
        Tok::Directive(d) => format!("#{}", d),
        Tok::Eof => String::new(),
    }
}

/// Replace macro parameters in `body`, handling `#param` (stringify) and `a ## b` (paste).
fn substitute(params: &[String], body: &[Token], raw: &[Vec<Token>], expanded: &[Vec<Token>], line: u32, col: u32) -> Result<Vec<Token>, CompileError> {
    let pidx = |t: &Tok| -> Option<usize> {
        match t {
            Tok::Ident(n) => params.iter().position(|p| p == n),
            _ => None,
        }
    };
    let mut out: Vec<Token> = vec![];
    let mut i = 0;
    while i < body.len() {
        let t = &body[i];
        // stringify
        if t.tok == Tok::Punct("#") {
            if let Some(k) = body.get(i + 1).and_then(|n| pidx(&n.tok)) {
                let text: Vec<String> = raw[k].iter().map(|x| token_text(&x.tok)).collect();
                out.push(Token { tok: Tok::Str(text.join(" ")), line, col, file: 0 });
                i += 2;
                continue;
            }
        }
        // paste
        if t.tok == Tok::Punct("##") {
            let left = out.pop().map(|x| token_text(&x.tok)).unwrap_or_default();
            let (right_toks, step) = match body.get(i + 1) {
                Some(n) => match pidx(&n.tok) {
                    Some(k) => (raw[k].clone(), 2),
                    None => (vec![n.clone()], 2),
                },
                None => (vec![], 1),
            };
            let mut text = left;
            let mut rest = vec![];
            if let Some((first, others)) = right_toks.split_first() {
                text.push_str(&token_text(&first.tok));
                rest = others.to_vec();
            }
            let lexed = lex_fragment(&text).map_err(|e| CompileError { line, col, msg: e.msg })?;
            for mut x in lexed.tokens {
                if x.tok == Tok::Eof {
                    continue;
                }
                x.line = line;
                x.col = col;
                x.file = u16::MAX;
                out.push(x);
            }
            out.extend(rest);
            i += step;
            continue;
        }
        if let Some(k) = pidx(&t.tok) {
            let next_is_paste = body.get(i + 1).is_some_and(|n| n.tok == Tok::Punct("##"));
            if next_is_paste {
                out.extend(raw[k].iter().cloned());
            } else {
                out.extend(expanded[k].iter().cloned());
            }
            i += 1;
            continue;
        }
        let mut c = t.clone();
        c.line = line;
        c.col = col;
        c.file = u16::MAX;
        out.push(c);
        i += 1;
    }
    Ok(out)
}

/// Split a directive into its word and the rest: `include<x.mqh>` -> ("include", "<x.mqh>").
fn split_directive(d: &str) -> (&str, &str) {
    match d.find(|c: char| c.is_whitespace() || c == '<' || c == '"') {
        Some(p) => (&d[..p], d[p..].trim()),
        None => (d, ""),
    }
}

/// Resolve `.` and `..` segments of a relative path.
fn normalize_path(p: &str) -> String {
    let mut parts: Vec<&str> = vec![];
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}
