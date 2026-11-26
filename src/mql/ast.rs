//! Syntax tree for the MQL5 subset.

use std::collections::HashMap;
use std::rc::Rc;

pub type Sym = u32;

#[derive(Default, Debug, Clone)]
pub struct Interner {
    pub names: Vec<String>,
    pub map: HashMap<String, Sym>,
}

impl Interner {
    pub fn intern(&mut self, s: &str) -> Sym {
        if let Some(&v) = self.map.get(s) {
            return v;
        }
        let id = self.names.len() as Sym;
        self.names.push(s.to_string());
        self.map.insert(s.to_string(), id);
        id
    }
    pub fn name(&self, s: Sym) -> &str {
        &self.names[s as usize]
    }
    pub fn get(&self, s: &str) -> Option<Sym> {
        self.map.get(s).copied()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    Void,
    Bool,
    /// all integer-like types (int, long, uint, char, datetime, color, enums...)
    Int,
    Double,
    Str,
    Struct(Sym),
    /// built-in class (CTrade, CPositionInfo...)
    Object(Sym),
    /// pointer to a user class or struct (`CFoo *p`)
    Ptr(Sym),
    /// template parameter: any value, never converted
    Any,
}

impl Ty {
    pub fn is_numeric(&self) -> bool {
        matches!(self, Ty::Int | Ty::Double | Ty::Bool)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
    BitNot,
    Plus,
}

#[derive(Clone, Debug)]
pub enum Expr {
    Int(i64),
    Double(f64),
    Str(Rc<str>),
    Bool(bool),
    Null,
    Var(Sym),
    Index(Box<Expr>, Box<Expr>),
    Member(Box<Expr>, Sym),
    Call(Sym, Vec<Expr>),
    MethodCall(Box<Expr>, Sym, Vec<Expr>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Assign(Option<BinOp>, Box<Expr>, Box<Expr>),
    IncDec {
        pre: bool,
        inc: bool,
        target: Box<Expr>,
    },
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    Cast(Ty, Box<Expr>),
    /// `{a, b, c}` initializer list
    List(Vec<Expr>),
    /// `new CFoo(args)`, also used for `CFoo obj(args);`
    New(Sym, Vec<Expr>),
    /// `delete p`
    Delete(Box<Expr>),
    /// `CBase::Method(args)`: a call that skips virtual dispatch
    ScopedCall(Sym, Sym, Vec<Expr>),
    /// `::Func(args)`: a global function even inside a class with a method of that name
    GlobalCall(Sym, Vec<Expr>),
}

#[derive(Clone, Debug)]
pub struct VarDecl {
    pub id: u32,
    pub name: Sym,
    pub ty: Ty,
    /// array dimensions; `None` means dynamic
    pub dims: Vec<Option<Expr>>,
    pub init: Option<Expr>,
    pub is_static: bool,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub line: u32,
    pub col: u32,
    pub file: u16,
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    Expr(Expr),
    Decl(Vec<VarDecl>),
    Block(Vec<Stmt>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    For(Option<Box<Stmt>>, Option<Expr>, Option<Expr>, Box<Stmt>),
    Return(Option<Expr>),
    Break,
    Continue,
    Switch(Expr, Vec<SwitchArm>),
    Empty,
}

#[derive(Clone, Debug)]
pub struct SwitchArm {
    /// None = default
    pub label: Option<Expr>,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: Sym,
    pub ty: Ty,
    pub by_ref: bool,
    pub is_array: bool,
    pub default: Option<Expr>,
}

#[derive(Clone, Debug)]
pub struct Func {
    pub name: Sym,
    pub ret: Ty,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    pub line: u32,
    /// owning class for methods
    pub class: Option<Sym>,
    /// constructor initializer list: `: CBase(a), m_x(b)`
    pub init_list: Vec<(Sym, Vec<Expr>)>,
}

#[derive(Clone, Debug)]
pub struct InputDecl {
    pub name: Sym,
    pub ty: Ty,
    /// enum type name when the input is an enum (for drop-downs)
    pub enum_name: Option<String>,
    pub label: String,
    pub decl_index: usize,
}

#[derive(Clone, Debug, Default)]
pub struct PlotProp {
    pub draw_type: Option<i64>,
    pub colors: Vec<u32>,
    pub style: Option<i64>,
    pub width: Option<i64>,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Properties {
    pub chart_window: bool,
    pub separate_window: bool,
    pub buffers: usize,
    pub plots: usize,
    pub plot: HashMap<usize, PlotProp>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub levels: Vec<f64>,
    pub level_color: Option<u32>,
    pub copyright: String,
    pub version: String,
    pub description: String,
    pub link: String,
    pub show_inputs: bool,
    pub other: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramKind {
    Indicator,
    Expert,
    Script,
    Unknown,
}

impl ProgramKind {
    pub fn label(self) -> &'static str {
        match self {
            ProgramKind::Indicator => "Indicator",
            ProgramKind::Expert => "Expert Advisor",
            ProgramKind::Script => "Script",
            ProgramKind::Unknown => "Program",
        }
    }
}

#[derive(Clone, Debug)]
pub struct StructDef {
    pub name: Sym,
    /// all fields, inherited ones first
    pub fields: Vec<(Sym, Ty, Vec<Option<Expr>>)>,
}

/// Methods and base class of a user class or struct.
#[derive(Clone, Debug, Default)]
pub struct ClassInfo {
    pub base: Option<Sym>,
    pub methods: HashMap<Sym, Vec<Rc<Func>>>,
    pub is_class: bool,
    /// fields declared in this class (not inherited), in order
    pub own_fields: Vec<Sym>,
}

#[derive(Clone, Debug)]
pub struct Program {
    pub file_name: String,
    pub kind: ProgramKind,
    pub props: Properties,
    pub globals: Vec<VarDecl>,
    pub inputs: Vec<InputDecl>,
    pub funcs: HashMap<Sym, Vec<Rc<Func>>>,
    pub enum_consts: HashMap<Sym, i64>,
    /// enum name -> (member name, value)
    pub enums: HashMap<String, Vec<(String, i64)>>,
    pub structs: HashMap<Sym, Rc<StructDef>>,
    pub classes: HashMap<Sym, ClassInfo>,
    /// enum type of variables, parameters and fields declared with one (for EnumToString)
    pub var_enums: HashMap<Sym, String>,
    pub interner: Interner,
    pub warnings: Vec<String>,
    /// source files: [0] is the program, the rest are include files
    pub files: Vec<String>,
}

impl Program {
    /// Find a method by walking from `class` up its base classes.
    pub fn find_method(&self, class: Sym, name: Sym) -> Option<&Vec<Rc<Func>>> {
        let mut c = Some(class);
        let mut guard = 0;
        while let Some(cls) = c {
            let info = self.classes.get(&cls)?;
            if let Some(m) = info.methods.get(&name) {
                return Some(m);
            }
            c = info.base;
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        None
    }

    pub fn is_user_class(&self, s: Sym) -> bool {
        self.classes.contains_key(&s)
    }
}
