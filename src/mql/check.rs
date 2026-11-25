//! Semantic check after parsing: undeclared identifiers, unknown functions and wrong
//! argument counts, reported the way a compiler would.

use super::ast::*;
use super::builtins;
use super::lexer::CompileError;
use std::collections::HashSet;

struct Checker<'a> {
    p: &'a Program,
    scopes: Vec<HashSet<Sym>>,
    globals: HashSet<Sym>,
    errors: Vec<CompileError>,
    line: u32,
    col: u32,
}

pub fn check(p: &Program) -> Vec<CompileError> {
    let mut c = Checker {
        p,
        scopes: vec![],
        globals: p.globals.iter().map(|g| g.name).collect(),
        errors: vec![],
        line: 0,
        col: 0,
    };
    for g in &p.globals {
        c.line = g.line;
        if let Some(e) = &g.init {
            c.expr(e);
        }
    }
    let mut names: Vec<_> = p.funcs.keys().copied().collect();
    names.sort();
    for n in names {
        for f in &p.funcs[&n] {
            c.scopes.push(f.params.iter().map(|x| x.name).collect());
            c.line = f.line;
            for s in &f.body {
                c.stmt(s);
            }
            c.scopes.pop();
        }
    }
    if p.kind == ProgramKind::Unknown {
        c.errors.push(CompileError {
            line: 1,
            col: 1,
            msg: "event handling function not found (add OnCalculate for an indicator, OnTick for an expert, or OnStart for a script)".into(),
        });
    }
    c.errors.truncate(50);
    c.errors
}

impl<'a> Checker<'a> {
    fn err(&mut self, msg: String) {
        if self.errors.iter().any(|e| e.line == self.line && e.msg == msg) {
            return;
        }
        self.errors.push(CompileError { line: self.line, col: self.col, msg });
    }

    fn declared(&self, s: Sym) -> bool {
        if self.scopes.iter().rev().any(|sc| sc.contains(&s)) || self.globals.contains(&s) || self.p.enum_consts.contains_key(&s) {
            return true;
        }
        let name = self.p.interner.name(s);
        builtins::constant(name).is_some() || builtins::PREDEFINED_VARS.contains(&name)
    }

    fn stmt(&mut self, s: &Stmt) {
        self.line = s.line;
        self.col = s.col;
        match &s.kind {
            StmtKind::Expr(e) => self.expr(e),
            StmtKind::Decl(ds) => {
                for d in ds {
                    for e in d.dims.iter().flatten() {
                        self.expr(e);
                    }
                    if let Some(e) = &d.init {
                        self.expr(e);
                    }
                    if let Some(sc) = self.scopes.last_mut() {
                        sc.insert(d.name);
                    }
                }
            }
            StmtKind::Block(b) => {
                self.scopes.push(HashSet::new());
                for s in b {
                    self.stmt(s);
                }
                self.scopes.pop();
            }
            StmtKind::If(c, t, e) => {
                self.expr(c);
                self.scoped(t);
                if let Some(e) = e {
                    self.scoped(e);
                }
            }
            StmtKind::While(c, b) => {
                self.expr(c);
                self.scoped(b);
            }
            StmtKind::DoWhile(b, c) => {
                self.scoped(b);
                self.expr(c);
            }
            StmtKind::For(i, c, st, b) => {
                self.scopes.push(HashSet::new());
                if let Some(i) = i {
                    self.stmt(i);
                }
                if let Some(c) = c {
                    self.expr(c);
                }
                if let Some(st) = st {
                    self.expr(st);
                }
                self.scoped(b);
                self.scopes.pop();
            }
            StmtKind::Return(Some(e)) => self.expr(e),
            StmtKind::Switch(v, arms) => {
                self.expr(v);
                self.scopes.push(HashSet::new());
                for a in arms {
                    if let Some(l) = &a.label {
                        self.expr(l);
                    }
                    for s in &a.body {
                        self.stmt(s);
                    }
                }
                self.scopes.pop();
            }
            _ => {}
        }
    }

    fn scoped(&mut self, s: &Stmt) {
        self.scopes.push(HashSet::new());
        self.stmt(s);
        self.scopes.pop();
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Var(s) => {
                if !self.declared(*s) {
                    let n = self.p.interner.name(*s).to_string();
                    if self.p.funcs.contains_key(s) || builtins::is_builtin_func(&n) {
                        return; // function name used as a value
                    }
                    self.err(format!("'{}' - undeclared identifier", n));
                }
            }
            Expr::Index(a, b) => {
                self.expr(a);
                self.expr(b);
            }
            Expr::Member(a, _) => self.expr(a),
            Expr::Call(s, args) => {
                let name = self.p.interner.name(*s).to_string();
                if let Some(fs) = self.p.funcs.get(s) {
                    let ok = fs.iter().any(|f| {
                        let req = f.params.iter().filter(|p| p.default.is_none()).count();
                        args.len() >= req && args.len() <= f.params.len()
                    });
                    if !ok {
                        self.err(format!("'{}' - wrong parameters count", name));
                    }
                } else if !builtins::is_builtin_func(&name) {
                    self.err(format!("'{}' - undeclared identifier", name));
                }
                for a in args {
                    self.expr(a);
                }
            }
            Expr::MethodCall(o, _, args) => {
                self.expr(o);
                for a in args {
                    self.expr(a);
                }
            }
            Expr::Unary(_, a) | Expr::Cast(_, a) => self.expr(a),
            Expr::Binary(_, a, b) | Expr::Assign(_, a, b) => {
                self.expr(a);
                self.expr(b);
            }
            Expr::IncDec { target, .. } => self.expr(target),
            Expr::Ternary(a, b, c) => {
                self.expr(a);
                self.expr(b);
                self.expr(c);
            }
            Expr::List(items) => {
                for i in items {
                    self.expr(i);
                }
            }
            _ => {}
        }
    }
}
