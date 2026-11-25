//! Where the terminal keeps its files: the MQL5 folder with programs, and saved accounts.

use crate::account::Account;
use std::path::{Path, PathBuf};

pub fn data_dir() -> PathBuf {
    if let Ok(p) = std::env::var("RUST_TERMINAL_DATA") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_else(|_| ".".into());
    let base = if cfg!(target_os = "windows") {
        std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(&home))
    } else if cfg!(target_os = "macos") {
        PathBuf::from(&home).join("Library").join("Application Support")
    } else {
        std::env::var("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(&home).join(".local").join("share"))
    };
    base.join("RustTerminal")
}

pub fn mql_dir() -> PathBuf {
    data_dir().join("MQL5")
}

pub const FOLDERS: [&str; 4] = ["Indicators", "Experts", "Scripts", "Include"];

/// Create the folders and copy the example programs once.
pub fn ensure_layout() {
    let root = mql_dir();
    for f in FOLDERS {
        let _ = std::fs::create_dir_all(root.join(f));
    }
    let marker = data_dir().join(".samples_v1");
    if !marker.exists() {
        for (rel, src) in crate::samples::SAMPLES {
            let p = root.join(rel);
            if !p.exists() {
                let _ = std::fs::write(&p, src);
            }
        }
        let _ = std::fs::write(marker, "1");
    }
}

#[derive(Clone, Debug)]
pub struct ProgramFile {
    pub name: String,
    pub rel: String,
    pub path: PathBuf,
    pub ex5: bool,
}

/// List .mq5 / .ex5 files under a folder (one level of sub-folders).
pub fn list_programs(folder: &str) -> Vec<ProgramFile> {
    let root = mql_dir().join(folder);
    let mut out = vec![];
    walk(&root, &root, 0, &mut out);
    out.sort_by(|a, b| a.rel.to_lowercase().cmp(&b.rel.to_lowercase()));
    out
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<ProgramFile>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() && depth < 2 {
            walk(root, &p, depth + 1, out);
            continue;
        }
        let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext == "mq5" || ext == "ex5" {
            let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            let name = p.file_stem().and_then(|x| x.to_str()).unwrap_or("").to_string();
            out.push(ProgramFile { name, rel, path: p.clone(), ex5: ext == "ex5" });
        }
    }
}

/// Resolve `#include` paths against the Include folder (and the program's own folder).
/// Decode a source file the way MetaEditor saves them: UTF-16 LE/BE with BOM, UTF-8
/// (with or without BOM), otherwise Windows-1252.
pub fn decode_source(b: &[u8]) -> String {
    if b.len() >= 2 && b[0] == 0xFF && b[1] == 0xFE {
        let u: Vec<u16> = b[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&u);
    }
    if b.len() >= 2 && b[0] == 0xFE && b[1] == 0xFF {
        let u: Vec<u16> = b[2..].chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&u);
    }
    let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b);
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => b.iter().map(|&c| c as char).collect(),
    }
}

pub fn read_source(p: &Path) -> std::io::Result<String> {
    std::fs::read(p).map(|b| decode_source(&b))
}

pub fn include_loader(base: Option<PathBuf>) -> impl Fn(&str) -> Option<String> {
    move |path: &str| {
        let mut cands = vec![mql_dir().join("Include").join(path)];
        if let Some(b) = &base {
            cands.insert(0, b.join(path));
        }
        cands.into_iter().find_map(|p| read_source(&p).ok())
    }
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct Saved {
    pub accounts: Vec<Account>,
    pub current: Option<u64>,
}

pub fn load_accounts() -> Saved {
    std::fs::read_to_string(data_dir().join("accounts.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_accounts(s: &Saved) {
    let _ = std::fs::create_dir_all(data_dir());
    if let Ok(j) = serde_json::to_string_pretty(s) {
        let tmp = data_dir().join("accounts.json.tmp");
        if std::fs::write(&tmp, j).is_ok() {
            let _ = std::fs::rename(tmp, data_dir().join("accounts.json"));
        }
    }
}

pub fn open_in_file_manager(p: &Path) {
    let cmd = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(cmd).arg(p).spawn();
}
