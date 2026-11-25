//! Tokenizer for the MQL5 language subset.

use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),
    /// A whole preprocessor line, without the leading '#'
    Directive(String),
    Punct(&'static str),
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
    pub col: u32,
    /// source file index (0 = the program itself, others are include files)
    pub file: u16,
}

#[derive(Debug, Clone)]
pub struct CompileError {
    pub line: u32,
    pub col: u32,
    pub msg: String,
}

const PUNCTS: [&str; 48] = [
    "##", "<<=", ">>=", "::", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "==", "!=", "<=", ">=", "&&", "||", "<<",
    ">>", "->", "+", "-", "*", "/", "%", "=", "<", ">", "!", "~", "&", "|", "^", "?", ":", ";", ",", ".", "(", ")", "[", "]",
    "{", "}", "#",
];

pub struct Lexed {
    pub tokens: Vec<Token>,
    /// trailing `//` comment text per line (used as input labels)
    pub line_comments: HashMap<u32, String>,
}

pub fn lex(src: &str) -> Result<Lexed, CompileError> {
    lex_impl(src, true)
}

/// Lex a fragment (macro body): '#' is an operator here, not a directive.
pub fn lex_fragment(src: &str) -> Result<Lexed, CompileError> {
    lex_impl(src, false)
}

fn lex_impl(src: &str, directives: bool) -> Result<Lexed, CompileError> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0usize;
    let mut line = 1u32;
    let mut col = 1u32;
    let mut out = Vec::new();
    let mut comments = HashMap::new();
    let mut line_start = directives;

    macro_rules! adv {
        () => {{
            if chars[i] == '\n' {
                line += 1;
                col = 1;
                line_start = directives;
            } else {
                col += 1;
            }
            i += 1;
        }};
    }

    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            adv!();
            continue;
        }
        if c.is_whitespace() {
            adv!();
            continue;
        }
        // comments
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            let l = line;
            let start = i + 2;
            while i < chars.len() && chars[i] != '\n' {
                adv!();
            }
            let text: String = chars[start..i].iter().collect();
            comments.insert(l, text.trim().to_string());
            continue;
        }
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            adv!();
            adv!();
            while i < chars.len() && !(chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '/') {
                adv!();
            }
            if i < chars.len() {
                adv!();
                adv!();
            }
            continue;
        }
        let (tl, tc) = (line, col);
        if c == '#' && line_start {
            let start = i + 1;
            let mut text = String::new();
            // support line continuation with backslash
            loop {
                while i < chars.len() && chars[i] != '\n' {
                    adv!();
                }
                let full: String = chars[start..i].iter().collect();
                text = full;
                if text.ends_with('\\') && i < chars.len() {
                    adv!();
                    continue;
                }
                break;
            }
            // strip trailing comment
            let mut t = text.replace("\\\r\n", " ").replace("\\\n", " ");
            if let Some(p) = find_comment(&t) {
                t.truncate(p);
            }
            out.push(Token { tok: Tok::Directive(t.trim().to_string()), line: tl, col: tc, file: 0 });
            continue;
        }
        line_start = false;
        // colour literal C'r,g,b' and date literal D'...'
        if (c == 'C' || c == 'D') && i + 1 < chars.len() && chars[i + 1] == '\'' {
            adv!();
            adv!();
            let start = i;
            while i < chars.len() && chars[i] != '\'' && chars[i] != '\n' {
                adv!();
            }
            let body: String = chars[start..i].iter().collect();
            if i < chars.len() && chars[i] == '\'' {
                adv!();
            }
            let v = if c == 'C' { parse_color_literal(&body) } else { parse_date_literal(&body) };
            match v {
                Some(v) => out.push(Token { tok: Tok::Int(v), line: tl, col: tc, file: 0 }),
                None => return Err(CompileError { line: tl, col: tc, msg: format!("'{}' - invalid literal", body) }),
            }
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                adv!();
            }
            let s: String = chars[start..i].iter().collect();
            out.push(Token { tok: Tok::Ident(s), line: tl, col: tc, file: 0 });
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()) {
            let start = i;
            if c == '0' && i + 1 < chars.len() && (chars[i + 1] == 'x' || chars[i + 1] == 'X') {
                adv!();
                adv!();
                let hs = i;
                while i < chars.len() && chars[i].is_ascii_hexdigit() {
                    adv!();
                }
                let h: String = chars[hs..i].iter().collect();
                let v = i64::from_str_radix(&h, 16).unwrap_or(0);
                out.push(Token { tok: Tok::Int(v), line: tl, col: tc, file: 0 });
                continue;
            }
            let mut is_float = false;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                if chars[i] == '.' {
                    is_float = true;
                }
                adv!();
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let save = (i, line, col);
                adv!();
                if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
                    adv!();
                }
                if i < chars.len() && chars[i].is_ascii_digit() {
                    is_float = true;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        adv!();
                    }
                } else {
                    i = save.0;
                    line = save.1;
                    col = save.2;
                }
            }
            let s: String = chars[start..i].iter().collect();
            // suffixes like 1.0f, 10L, 5u
            while i < chars.len() && matches!(chars[i], 'f' | 'F' | 'l' | 'L' | 'u' | 'U') {
                adv!();
            }
            if is_float {
                out.push(Token { tok: Tok::Float(s.parse().unwrap_or(0.0)), line: tl, col: tc, file: 0 });
            } else {
                match s.parse::<i64>() {
                    Ok(v) => out.push(Token { tok: Tok::Int(v), line: tl, col: tc, file: 0 }),
                    Err(_) => out.push(Token { tok: Tok::Float(s.parse().unwrap_or(0.0)), line: tl, col: tc, file: 0 }),
                }
            }
            continue;
        }
        if c == '"' {
            adv!();
            let mut s = String::new();
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\n' {
                    return Err(CompileError { line: tl, col: tc, msg: "unterminated string".into() });
                }
                if chars[i] == '\\' && i + 1 < chars.len() {
                    adv!();
                    let e = chars[i];
                    s.push(match e {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '0' => '\0',
                        other => other,
                    });
                    adv!();
                    continue;
                }
                s.push(chars[i]);
                adv!();
            }
            if i < chars.len() {
                adv!();
            }
            out.push(Token { tok: Tok::Str(s), line: tl, col: tc, file: 0 });
            continue;
        }
        if c == '\'' {
            adv!();
            let mut v = 0i64;
            if i < chars.len() {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    adv!();
                    v = match chars[i] {
                        'n' => 10,
                        't' => 9,
                        'r' => 13,
                        '0' => 0,
                        o => o as i64,
                    };
                } else {
                    v = chars[i] as i64;
                }
                adv!();
            }
            if i < chars.len() && chars[i] == '\'' {
                adv!();
            }
            out.push(Token { tok: Tok::Int(v), line: tl, col: tc, file: 0 });
            continue;
        }
        let mut matched = None;
        for p in PUNCTS.iter() {
            let pc: Vec<char> = p.chars().collect();
            if i + pc.len() <= chars.len() && chars[i..i + pc.len()] == pc[..] {
                matched = Some(*p);
                break;
            }
        }
        match matched {
            Some(p) => {
                for _ in 0..p.chars().count() {
                    adv!();
                }
                out.push(Token { tok: Tok::Punct(p), line: tl, col: tc, file: 0 });
            }
            None => {
                return Err(CompileError { line: tl, col: tc, msg: format!("'{}' - unexpected character", c) });
            }
        }
    }
    // join adjacent string literals
    let mut joined: Vec<Token> = Vec::with_capacity(out.len());
    for t in out {
        if let (Tok::Str(s), Some(Token { tok: Tok::Str(prev), .. })) = (&t.tok, joined.last_mut()) {
            prev.push_str(s);
            continue;
        }
        joined.push(t);
    }
    joined.push(Token { tok: Tok::Eof, line, col, file: 0 });
    Ok(Lexed { tokens: joined, line_comments: comments })
}

fn find_comment(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut in_str = false;
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'"' {
            in_str = !in_str;
        }
        if !in_str && b[i] == b'/' && b[i + 1] == b'/' {
            return Some(i);
        }
        i += 1;
    }
    None
}

pub fn parse_color_literal(body: &str) -> Option<i64> {
    let parts: Vec<&str> = body.split(',').map(|s| s.trim()).collect();
    if parts.len() == 3 {
        let p = |s: &str| -> Option<i64> {
            if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                i64::from_str_radix(h, 16).ok()
            } else {
                s.parse().ok()
            }
        };
        let (r, g, b) = (p(parts[0])?, p(parts[1])?, p(parts[2])?);
        return Some((r & 0xff) | ((g & 0xff) << 8) | ((b & 0xff) << 16));
    }
    None
}

/// D'2024.01.15 10:30' -> unix seconds
pub fn parse_date_literal(body: &str) -> Option<i64> {
    parse_datetime(body.trim())
}

pub fn parse_datetime(s: &str) -> Option<i64> {
    let mut it = s.split_whitespace();
    let date = it.next()?;
    let time = it.next().unwrap_or("00:00");
    let d: Vec<i64> = date.split('.').filter_map(|x| x.parse().ok()).collect();
    if d.len() != 3 {
        return None;
    }
    let t: Vec<i64> = time.split(':').filter_map(|x| x.parse().ok()).collect();
    let (h, mi, sec) = (t.first().copied().unwrap_or(0), t.get(1).copied().unwrap_or(0), t.get(2).copied().unwrap_or(0));
    Some(days_from_civil(d[0], d[1], d[2]) * 86400 + h * 3600 + mi * 60 + sec)
}

pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}
