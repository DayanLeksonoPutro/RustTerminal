//! Script engine for custom indicators, expert advisors and scripts written in the
//! MQL5 language (source .mq5 files).

pub mod ast;
pub mod builtins;
pub mod check;
pub mod interp;
pub mod lexer;
pub mod parser;

pub use ast::{Program, ProgramKind};
pub use interp::Instance;
