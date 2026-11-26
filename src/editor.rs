//! Code editor for .mq5 programs: file tree, syntax colouring, line numbers, compile with
//! error list, new-program templates and "attach to chart".

use crate::mql::builtins;
use crate::mql::parser::compile;
use crate::storage;
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, RichText};
use std::path::PathBuf;

pub struct EditorFile {
    pub path: PathBuf,
    pub name: String,
    pub text: String,
    pub dirty: bool,
}

pub enum EditorEvent {
    Attach(PathBuf),
    Compiled(PathBuf),
}

pub struct Editor {
    pub open: bool,
    pub files: Vec<EditorFile>,
    pub current: usize,
    pub messages: Vec<(bool, String)>,
    new_dialog: Option<(usize, String)>,
    tree: Vec<(String, Vec<storage::ProgramFile>)>,
    tree_dirty: bool,
}

const KEYWORDS: &[&str] = &[
    "if",
    "else",
    "for",
    "while",
    "do",
    "return",
    "break",
    "continue",
    "switch",
    "case",
    "default",
    "input",
    "sinput",
    "const",
    "static",
    "struct",
    "enum",
    "class",
    "true",
    "false",
    "NULL",
    "extern",
    "public",
    "private",
    "protected",
    "void",
    "int",
    "uint",
    "long",
    "ulong",
    "short",
    "ushort",
    "char",
    "uchar",
    "double",
    "float",
    "bool",
    "string",
    "datetime",
    "color",
    "group",
];

impl Default for Editor {
    fn default() -> Self {
        Editor {
            open: false,
            files: vec![],
            current: 0,
            messages: vec![],
            new_dialog: None,
            tree: vec![],
            tree_dirty: true,
        }
    }
}

impl Editor {
    pub fn open_file(&mut self, path: PathBuf) {
        if let Some(i) = self.files.iter().position(|f| f.path == path) {
            self.current = i;
            self.open = true;
            return;
        }
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("ex5"))
        {
            self.messages = vec![(
                false,
                ".ex5 files are compiled and encrypted; open the .mq5 source instead".into(),
            )];
            self.open = true;
            return;
        }
        let text = crate::storage::read_source(&path).unwrap_or_default();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("untitled.mq5")
            .to_string();
        self.files.push(EditorFile {
            path,
            name,
            text,
            dirty: false,
        });
        self.current = self.files.len() - 1;
        self.open = true;
    }

    pub fn refresh_tree(&mut self) {
        self.tree_dirty = true;
    }

    fn save_current(&mut self) {
        if let Some(f) = self.files.get_mut(self.current) {
            if let Some(dir) = f.path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            match std::fs::write(&f.path, &f.text) {
                Ok(()) => f.dirty = false,
                Err(e) => self.messages = vec![(false, format!("cannot save {}: {}", f.name, e))],
            }
        }
    }

    fn compile_current(&mut self) -> Option<PathBuf> {
        self.save_current();
        let f = self.files.get(self.current)?;
        let loader = storage::include_loader(f.path.parent().map(|p| p.to_path_buf()));
        let start = std::time::Instant::now();
        self.messages.clear();
        self.messages.push((true, format!("'{}'", f.name)));
        match compile(&f.name, &f.text, &loader) {
            Ok(p) => {
                for w in &p.warnings {
                    self.messages.push((true, format!("warning: {}", w)));
                }
                self.messages.push((
                    true,
                    format!(
                        "{} ready: {} input(s), {} function(s)",
                        p.kind.label(),
                        p.inputs.len(),
                        p.funcs.values().map(|v| v.len()).sum::<usize>()
                    ),
                ));
                self.messages.push((
                    true,
                    format!(
                        "0 errors, {} warnings, {} ms elapsed",
                        p.warnings.len(),
                        start.elapsed().as_millis()
                    ),
                ));
                Some(f.path.clone())
            }
            Err(errs) => {
                let n = errs.len();
                for e in errs {
                    self.messages.push((
                        false,
                        format!("{} ({},{}): {}", f.name, e.line, e.col, e.msg),
                    ));
                }
                self.messages
                    .push((false, format!("{} errors, 0 warnings", n)));
                None
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<EditorEvent> {
        if !self.open {
            return None;
        }
        if self.tree_dirty {
            self.tree = ["Experts", "Indicators", "Scripts", "Include"]
                .iter()
                .map(|f| (f.to_string(), list_with_include(f)))
                .collect();
            self.tree_dirty = false;
        }
        let mut event = None;
        let mut open = self.open;
        let (f7, ctrl_s) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::F7),
                i.modifiers.command && i.key_pressed(egui::Key::S),
            )
        });
        egui::Window::new("MetaEditor — code editor")
            .open(&mut open)
            .default_size([980.0, 640.0])
            .min_width(600.0)
            .min_height(360.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.menu_button("📄 New", |ui| {
                        for (k, label) in ["Expert Advisor", "Custom Indicator", "Script"].iter().enumerate() {
                            if ui.button(*label).clicked() {
                                self.new_dialog = Some((k, String::new()));
                                ui.close_menu();
                            }
                        }
                    });
                    if ui.button("💾 Save").on_hover_text(crate::style::sc("Ctrl+S")).clicked() {
                        self.save_current();
                        self.tree_dirty = true;
                    }
                    if ui.button("⚙ Compile").on_hover_text("F7").clicked() {
                        if let Some(p) = self.compile_current() {
                            event = Some(EditorEvent::Compiled(p));
                        }
                    }
                    let can_attach = self.files.get(self.current).is_some_and(|f| !f.path.starts_with(storage::mql_dir().join("Include")));
                    if ui.add_enabled(can_attach, egui::Button::new("▶ Attach to chart")).clicked() {
                        if let Some(p) = self.compile_current() {
                            event = Some(EditorEvent::Attach(p));
                        }
                    }
                    ui.separator();
                    ui.label(RichText::new(storage::mql_dir().display().to_string()).small().weak());
                });
                ui.separator();
                egui::SidePanel::left("ed_tree").resizable(true).default_width(200.0).show_inside(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for (folder, files) in self.tree.clone() {
                            egui::CollapsingHeader::new(format!("📁 {}", folder)).default_open(folder != "Include").show(ui, |ui| {
                                if files.is_empty() {
                                    ui.label(RichText::new("(empty)").weak());
                                }
                                for f in files {
                                    let label = if f.ex5 { format!("🔒 {}", f.rel) } else { format!("📝 {}", f.rel) };
                                    let r = ui.selectable_label(self.files.get(self.current).is_some_and(|c| c.path == f.path), label);
                                    if r.clicked() {
                                        self.open_file(f.path.clone());
                                    }
                                }
                            });
                        }
                    });
                });
                egui::TopBottomPanel::bottom("ed_errors").resizable(true).default_height(110.0).show_inside(ui, |ui| {
                    ui.label(RichText::new("Errors").strong());
                    egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                        if self.messages.is_empty() {
                            ui.label(RichText::new("Press Compile (F7) to check the program").weak());
                        }
                        for (ok, m) in &self.messages {
                            let c = if *ok { Color32::from_rgb(20, 110, 20) } else { Color32::from_rgb(190, 20, 20) };
                            ui.label(RichText::new(m).monospace().color(c));
                        }
                    });
                });
                // tabs
                ui.horizontal_wrapped(|ui| {
                    let mut close = None;
                    for (i, f) in self.files.iter().enumerate() {
                        let title = if f.dirty { format!("{} *", f.name) } else { f.name.clone() };
                        if ui.selectable_label(i == self.current, title).clicked() {
                            self.current = i;
                        }
                        if ui.small_button("✖").clicked() {
                            close = Some(i);
                        }
                    }
                    if let Some(i) = close {
                        self.files.remove(i);
                        if self.current >= self.files.len() {
                            self.current = self.files.len().saturating_sub(1);
                        }
                    }
                });
                if let Some(f) = self.files.get_mut(self.current) {
                    let lines = f.text.lines().count().max(1) + usize::from(f.text.ends_with('\n'));
                    let numbers: String = (1..=lines).map(|n| format!("{:>4}\n", n)).collect();
                    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                        ui.horizontal_top(|ui| {
                            ui.add(
                                egui::Label::new(RichText::new(numbers).monospace().size(13.0).color(Color32::from_rgb(120, 120, 140)))
                                    .selectable(false),
                            );
                            let mut layouter = |ui: &egui::Ui, text: &str, wrap: f32| {
                                let mut job = highlight(text);
                                job.wrap.max_width = wrap;
                                ui.fonts(|fo| fo.layout_job(job))
                            };
                            let r = ui.add(
                                egui::TextEdit::multiline(&mut f.text)
                                    .code_editor()
                                    .font(FontId::monospace(13.0))
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(30)
                                    .lock_focus(true)
                                    .layouter(&mut layouter),
                            );
                            if r.changed() {
                                f.dirty = true;
                            }
                        });
                    });
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label("Open a program from the tree on the left, or create a new one with New.");
                    });
                }
            });
        self.open = open;
        if ctrl_s && self.open {
            self.save_current();
            self.tree_dirty = true;
        }
        if f7 && self.open {
            if let Some(p) = self.compile_current() {
                event = Some(EditorEvent::Compiled(p));
            }
        }
        // new program dialog
        let mut create = None;
        if let Some((kind, name)) = &mut self.new_dialog {
            let mut keep = true;
            let label = ["Expert Advisor", "Custom Indicator", "Script"][*kind];
            egui::Window::new(format!("New {}", label))
                .collapsible(false)
                .resizable(false)
                .open(&mut keep)
                .show(ctx, |ui| {
                    ui.label("Name:");
                    ui.text_edit_singleline(name);
                    if ui
                        .add_enabled(!name.trim().is_empty(), egui::Button::new("Create"))
                        .clicked()
                    {
                        create = Some((*kind, name.trim().to_string()));
                    }
                });
            if !keep {
                self.new_dialog = None;
            }
        }
        if let Some((kind, name)) = create {
            let (folder, tpl) = match kind {
                0 => ("Experts", crate::samples::TEMPLATE_EXPERT),
                1 => ("Indicators", crate::samples::TEMPLATE_INDICATOR),
                _ => ("Scripts", crate::samples::TEMPLATE_SCRIPT),
            };
            let file = if name.to_lowercase().ends_with(".mq5") {
                name.clone()
            } else {
                format!("{}.mq5", name)
            };
            let path = storage::mql_dir().join(folder).join(file);
            if !path.exists() {
                let _ = std::fs::write(&path, tpl);
            }
            self.new_dialog = None;
            self.tree_dirty = true;
            self.open_file(path);
        }
        event
    }
}

fn list_with_include(folder: &str) -> Vec<storage::ProgramFile> {
    if folder != "Include" {
        return storage::list_programs(folder);
    }
    let root = storage::mql_dir().join("Include");
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| x.eq_ignore_ascii_case("mqh"))
            {
                let rel = p
                    .file_name()
                    .and_then(|x| x.to_str())
                    .unwrap_or("")
                    .to_string();
                out.push(storage::ProgramFile {
                    name: rel.clone(),
                    rel,
                    path: p.clone(),
                    ex5: false,
                });
            }
        }
    }
    out
}

/// Syntax colouring for the editor.
pub fn highlight(text: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    let font = FontId::monospace(13.0);
    let fmt = |c: Color32| TextFormat {
        font_id: font.clone(),
        color: c,
        ..Default::default()
    };
    let normal = Color32::from_rgb(20, 20, 20);
    let kw = Color32::from_rgb(0, 0, 230);
    let func = Color32::from_rgb(128, 0, 128);
    let cnst = Color32::from_rgb(140, 70, 0);
    let string = Color32::from_rgb(150, 0, 150);
    let number = Color32::from_rgb(190, 30, 30);
    let comment = Color32::from_rgb(110, 130, 110);
    let pre = Color32::from_rgb(150, 100, 30);
    let b = text.as_bytes();
    let mut i = 0;
    let mut line_start = true;
    while i < b.len() {
        let c = b[i];
        let start = i;
        if c == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            job.append(&text[start..i], 0.0, fmt(comment));
            continue;
        }
        if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(b.len());
            job.append(&text[start..i], 0.0, fmt(comment));
            continue;
        }
        if c == b'#' && line_start {
            while i < b.len()
                && b[i] != b'\n'
                && !(b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'/')
            {
                i += 1;
            }
            job.append(&text[start..i], 0.0, fmt(pre));
            continue;
        }
        if c == b'"' {
            i += 1;
            while i < b.len() && b[i] != b'"' && b[i] != b'\n' {
                if b[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(b.len());
            job.append(&text[start..i], 0.0, fmt(string));
            line_start = false;
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let w = &text[start..i];
            let col = if KEYWORDS.contains(&w) {
                kw
            } else if builtins::is_builtin_func(w) {
                func
            } else if builtins::constant(w).is_some()
                || w.starts_with("ENUM_")
                || w.starts_with("Mql")
                || w.starts_with('C') && builtins::BUILTIN_CLASSES.contains(&w)
            {
                cnst
            } else {
                normal
            };
            job.append(w, 0.0, fmt(col));
            line_start = false;
            continue;
        }
        if c.is_ascii_digit() {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.') {
                i += 1;
            }
            job.append(&text[start..i], 0.0, fmt(number));
            line_start = false;
            continue;
        }
        // other characters (keep UTF-8 boundaries)
        let ch = text[i..].chars().next().unwrap();
        i += ch.len_utf8();
        if ch == '\n' {
            line_start = true;
        } else if !ch.is_whitespace() {
            line_start = false;
        }
        job.append(&text[start..i], 0.0, fmt(normal));
    }
    job
}
