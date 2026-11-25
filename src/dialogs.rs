//! Dialogs: new order, modify position, login, open demo account, program inputs,
//! indicator parameters, chart properties, objects list, messages.

use crate::account::{Account, OrderType};
use crate::app::TerminalApp;
use crate::chart::{BuiltinInd, Scheme};
use crate::indicators::{IndKind, IndParams};
use crate::market::{fmt_price, round_to};
use crate::mql::ast::Expr;
use crate::mql::interp::TradeOp;
use crate::mql::{builtins, Program, ProgramKind};
use egui::{Color32, RichText};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

pub struct OrderDlg {
    pub symbol: String,
    pub pending: bool,
    pub ptype: OrderType,
    pub volume: f64,
    pub sl: f64,
    pub tp: f64,
    pub price: f64,
    pub comment: String,
}

pub struct ModifyDlg {
    pub ticket: u64,
    pub order: bool,
    pub sl: f64,
    pub tp: f64,
    pub price: f64,
}

pub struct LoginDlg {
    pub login: String,
    pub password: String,
    pub save: bool,
    pub error: Option<String>,
}

pub struct DemoDlg {
    pub name: String,
    pub email: String,
    pub deposit: f64,
    pub leverage: u32,
    pub created: Option<(u64, String)>,
}

pub struct InputRow {
    pub name: String,
    pub label: String,
    pub kind: InputKind,
    pub value: String,
}

pub enum InputKind {
    Bool,
    Number,
    Text,
    Choice(Vec<(String, i64)>),
}

pub enum AttachTarget {
    New,
    Custom(usize),
    Expert,
}

pub struct AttachDlg {
    pub chart: usize,
    pub path: PathBuf,
    pub prog: Rc<Program>,
    pub rows: Vec<InputRow>,
    pub allow_algo: bool,
    pub tab: usize,
    pub target: AttachTarget,
}

pub struct IndDlg {
    pub chart: usize,
    pub kind: IndKind,
    pub params: IndParams,
    pub edit: Option<usize>,
}

#[derive(Default)]
pub struct Dialogs {
    pub order: Option<OrderDlg>,
    pub modify: Option<ModifyDlg>,
    pub login: Option<LoginDlg>,
    pub demo: Option<DemoDlg>,
    pub attach: Option<AttachDlg>,
    pub ind: Option<IndDlg>,
    pub props: bool,
    pub objects: bool,
    pub about: bool,
    pub message: Option<(String, String)>,
    pub data: Option<crate::data_ui::DataDlg>,
}

/// The default value written in the program for an input, as text.
fn default_text(prog: &Program, e: Option<&Expr>) -> String {
    match e {
        Some(Expr::Int(v)) => v.to_string(),
        Some(Expr::Double(v)) => crate::mql::interp::fmt_double(*v),
        Some(Expr::Str(s)) => s.to_string(),
        Some(Expr::Bool(b)) => b.to_string(),
        Some(Expr::Unary(crate::mql::ast::UnOp::Neg, x)) => format!("-{}", default_text(prog, Some(x))),
        Some(Expr::Var(s)) => {
            let n = prog.interner.name(*s);
            if let Some(v) = prog.enum_consts.get(s) {
                v.to_string()
            } else if let Some(c) = builtins::constant(n) {
                match c {
                    builtins::Const::I(i) => i.to_string(),
                    builtins::Const::D(d) => crate::mql::interp::fmt_double(d),
                }
            } else {
                n.to_string()
            }
        }
        Some(Expr::Cast(_, x)) => default_text(prog, Some(x)),
        _ => String::new(),
    }
}

pub fn input_rows(prog: &Program, current: &HashMap<String, String>) -> Vec<InputRow> {
    prog.inputs
        .iter()
        .map(|inp| {
            let name = prog.interner.name(inp.name).to_string();
            let decl = &prog.globals[inp.decl_index];
            let def = default_text(prog, decl.init.as_ref());
            let kind = if let Some(en) = &inp.enum_name {
                let opts: Vec<(String, i64)> = builtins::builtin_enum(en)
                    .map(|v| v.into_iter().map(|(a, b)| (a.to_string(), b)).collect())
                    .or_else(|| prog.enums.get(en).cloned())
                    .unwrap_or_default();
                if opts.is_empty() {
                    InputKind::Number
                } else {
                    InputKind::Choice(opts)
                }
            } else {
                match inp.ty {
                    crate::mql::ast::Ty::Bool => InputKind::Bool,
                    crate::mql::ast::Ty::Str => InputKind::Text,
                    _ => InputKind::Number,
                }
            };
            let value = current.get(&name).cloned().filter(|v| !v.is_empty()).unwrap_or(def);
            InputRow { name, label: inp.label.clone(), kind, value }
        })
        .collect()
}

pub fn inputs_ui(ui: &mut egui::Ui, rows: &mut [InputRow]) {
    if rows.is_empty() {
        ui.label(RichText::new("This program has no inputs.").weak());
        return;
    }
    egui::Grid::new("inputs_grid").striped(true).num_columns(2).min_col_width(140.0).show(ui, |ui| {
        ui.label(RichText::new("Variable").strong());
        ui.label(RichText::new("Value").strong());
        ui.end_row();
        for r in rows.iter_mut() {
            ui.label(&r.label).on_hover_text(&r.name);
            match &r.kind {
                InputKind::Bool => {
                    let mut b = r.value == "true" || r.value == "1";
                    egui::ComboBox::from_id_salt(&r.name).selected_text(if b { "true" } else { "false" }).show_ui(ui, |ui| {
                        ui.selectable_value(&mut b, true, "true");
                        ui.selectable_value(&mut b, false, "false");
                    });
                    r.value = b.to_string();
                }
                InputKind::Choice(opts) => {
                    let cur = r.value.parse::<i64>().ok().or_else(|| opts.iter().find(|(n, _)| *n == r.value).map(|x| x.1));
                    let label = opts.iter().find(|(_, v)| Some(*v) == cur).map(|x| x.0.clone()).unwrap_or(r.value.clone());
                    egui::ComboBox::from_id_salt(&r.name).selected_text(label).show_ui(ui, |ui| {
                        for (n, v) in opts {
                            if ui.selectable_label(cur == Some(*v), n).clicked() {
                                r.value = v.to_string();
                            }
                        }
                    });
                }
                _ => {
                    ui.add(egui::TextEdit::singleline(&mut r.value).desired_width(160.0));
                }
            }
            ui.end_row();
        }
    });
}

fn num(ui: &mut egui::Ui, label: &str, v: &mut i64, min: i64) {
    ui.label(label);
    ui.add(egui::DragValue::new(v).range(min..=5000).speed(0.2));
    ui.end_row();
}
fn fnum(ui: &mut egui::Ui, label: &str, v: &mut f64, step: f64) {
    ui.label(label);
    ui.add(egui::DragValue::new(v).speed(step).max_decimals(3).range(0.0..=1000.0));
    ui.end_row();
}

fn method_combo(ui: &mut egui::Ui, v: &mut i64) {
    ui.label("MA method");
    let names = ["Simple", "Exponential", "Smoothed", "Linear Weighted"];
    egui::ComboBox::from_id_salt("ma_method").selected_text(names[(*v).clamp(0, 3) as usize]).show_ui(ui, |ui| {
        for (i, n) in names.iter().enumerate() {
            ui.selectable_value(v, i as i64, *n);
        }
    });
    ui.end_row();
}
fn price_combo(ui: &mut egui::Ui, v: &mut i64) {
    ui.label("Apply to");
    let names = ["Close", "Open", "High", "Low", "Median Price (HL/2)", "Typical Price (HLC/3)", "Weighted Close (HLCC/4)"];
    egui::ComboBox::from_id_salt("apply_to").selected_text(names[((*v).clamp(1, 7) - 1) as usize]).show_ui(ui, |ui| {
        for (i, n) in names.iter().enumerate() {
            ui.selectable_value(v, i as i64 + 1, *n);
        }
    });
    ui.end_row();
}

fn params_ui(ui: &mut egui::Ui, kind: IndKind, p: &mut IndParams) {
    egui::Grid::new("ind_params").num_columns(2).min_col_width(120.0).show(ui, |ui| match kind {
        IndKind::MovingAverage => {
            num(ui, "Period", &mut p.p1, 1);
            num(ui, "Shift", &mut p.shift, -500);
            method_combo(ui, &mut p.method);
            price_combo(ui, &mut p.price);
        }
        IndKind::Bands => {
            num(ui, "Period", &mut p.p1, 2);
            num(ui, "Shift", &mut p.shift, -500);
            fnum(ui, "Deviations", &mut p.f1, 0.05);
            price_combo(ui, &mut p.price);
        }
        IndKind::Envelopes => {
            num(ui, "Period", &mut p.p1, 1);
            num(ui, "Shift", &mut p.shift, -500);
            method_combo(ui, &mut p.method);
            price_combo(ui, &mut p.price);
            fnum(ui, "Deviation (%)", &mut p.f1, 0.01);
        }
        IndKind::Ichimoku => {
            num(ui, "Tenkan-sen", &mut p.p1, 1);
            num(ui, "Kijun-sen", &mut p.p2, 1);
            num(ui, "Senkou Span B", &mut p.p3, 1);
        }
        IndKind::Sar => {
            fnum(ui, "Step", &mut p.f1, 0.001);
            fnum(ui, "Maximum", &mut p.f2, 0.01);
        }
        IndKind::Alligator => {
            num(ui, "Jaws period", &mut p.p1, 1);
            num(ui, "Teeth period", &mut p.p2, 1);
            num(ui, "Lips period", &mut p.p3, 1);
            method_combo(ui, &mut p.method);
            price_combo(ui, &mut p.price);
        }
        IndKind::Macd => {
            num(ui, "Fast EMA", &mut p.p1, 1);
            num(ui, "Slow EMA", &mut p.p2, 1);
            num(ui, "MACD SMA", &mut p.p3, 1);
            price_combo(ui, &mut p.price);
        }
        IndKind::Rsi | IndKind::Momentum | IndKind::Cci => {
            num(ui, "Period", &mut p.p1, 1);
            price_combo(ui, &mut p.price);
        }
        IndKind::Stochastic => {
            num(ui, "%K period", &mut p.p1, 1);
            num(ui, "%D period", &mut p.p2, 1);
            num(ui, "Slowing", &mut p.p3, 1);
        }
        IndKind::Atr | IndKind::Adx | IndKind::Wpr => {
            num(ui, "Period", &mut p.p1, 1);
        }
        IndKind::Fractals | IndKind::Volumes => {
            ui.label("No parameters");
            ui.end_row();
        }
    });
}

impl TerminalApp {
    pub fn open_attach_dialog(&mut self, ci: usize, path: PathBuf, target: AttachTarget) {
        if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("ex5")) {
            self.dialogs.message = Some((
                "Compiled program".into(),
                "This is a compiled .ex5 file. Its format is encrypted and closed, so this terminal can't run it.\n\nPut the program's .mq5 source code in the same folder (or open it in MetaEditor) and attach that instead.".into(),
            ));
            return;
        }
        let prog = match crate::host::load_program(&mut self.cache, &path) {
            Ok(p) => p,
            Err(e) => {
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                self.log_error("Experts", format!("cannot load '{}' [{}]", name, e));
                self.dialogs.message = Some(("Compilation errors".into(), format!("{}\n\n{}\n\nOpen it in MetaEditor (F4) to fix the errors.", name, e)));
                return;
            }
        };
        let current = match &target {
            AttachTarget::Custom(k) => self.charts[ci].customs.get(*k).map(|c| c.inputs.clone()).unwrap_or_default(),
            AttachTarget::Expert => self.charts[ci].expert.as_ref().map(|e| e.inputs.clone()).unwrap_or_default(),
            AttachTarget::New => HashMap::new(),
        };
        let rows = input_rows(&prog, &current);
        let tab = if prog.inputs.is_empty() { 0 } else { 1 };
        if prog.kind == ProgramKind::Script && !prog.props.show_inputs {
            self.attach_compiled(ci, &path, prog, HashMap::new());
            return;
        }
        self.dialogs.attach = Some(AttachDlg { chart: ci, path, prog, rows, allow_algo: self.algo, tab, target });
    }

    pub fn new_order_dialog(&mut self, symbol: &str) {
        let (bid, digits) = self.market.symbol_index(symbol).map(|i| (self.market.symbols[i].bid, self.market.symbols[i].spec.digits)).unwrap_or((0.0, 5));
        self.dialogs.order = Some(OrderDlg {
            symbol: symbol.to_string(),
            pending: false,
            ptype: OrderType::BuyLimit,
            volume: 0.10,
            sl: 0.0,
            tp: 0.0,
            price: round_to(bid, digits),
            comment: String::new(),
        });
    }

    pub fn show_dialogs(&mut self, ctx: &egui::Context) {
        self.order_window(ctx);
        self.modify_window(ctx);
        self.login_window(ctx);
        self.demo_window(ctx);
        self.attach_window(ctx);
        self.ind_window(ctx);
        self.props_window(ctx);
        self.objects_window(ctx);
        self.data_window(ctx);
        if self.dialogs.about {
            let mut open = true;
            egui::Window::new("About").collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
                ui.heading("Rust Terminal");
                ui.label("A cross-platform trading terminal written in Rust.");
                ui.label("Charts, built-in indicators, custom indicators, expert advisors and scripts from .mq5 source, a code editor and a strategy tester.");
                ui.label(RichText::new("Accounts and order execution are simulated. Prices are real where history was imported or live prices are on (File > History Data).").weak());
            });
            self.dialogs.about = open;
        }
        if let Some((title, text)) = self.dialogs.message.clone() {
            let mut open = true;
            let mut ok = false;
            egui::Window::new(title).collapsible(false).resizable(false).open(&mut open).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.label(text);
                ui.add_space(6.0);
                if ui.button("   OK   ").clicked() {
                    ok = true;
                }
            });
            if !open || ok {
                self.dialogs.message = None;
            }
        }
        // alerts window
        if self.alerts.len() > self.alerts_seen {
            let mut open = true;
            egui::Window::new("Alerts").collapsible(false).default_width(460.0).open(&mut open).show(ctx, |ui| {
                egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                    for a in self.alerts.iter().rev().take(50) {
                        ui.label(RichText::new(format!("{}  {}  {}", crate::host::fmt_log_time(a.time), a.source, a.text)).monospace());
                    }
                });
                if ui.button("Close").clicked() {
                    self.alerts_seen = self.alerts.len();
                }
            });
            if !open {
                self.alerts_seen = self.alerts.len();
            }
        }
    }

    fn order_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.dialogs.order.take() else { return };
        let mut open = true;
        let mut action: Option<TradeOp> = None;
        let symbols: Vec<String> = self.market.symbols.iter().map(|s| s.spec.name.to_string()).collect();
        let si = self.market.symbol_index(&d.symbol).unwrap_or(0);
        let (bid, ask, digits, desc) = {
            let s = &self.market.symbols[si];
            (s.bid, s.ask, s.spec.digits, s.spec.description)
        };
        egui::Window::new("Order").collapsible(false).resizable(false).open(&mut open).default_width(560.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Left pane: Authentic MT5 Live Tick Chart
                ui.allocate_ui(egui::vec2(190.0, 280.0), |ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Tick Chart").strong().size(11.0));
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(185.0, 250.0), egui::Sense::hover());
                        let p = ui.painter();
                        p.rect_filled(rect, 0.0, Color32::from_rgb(15, 18, 24));
                        p.rect_stroke(rect, 0.0, egui::Stroke::new(1.0_f32, Color32::from_rgb(60, 65, 75)), egui::StrokeKind::Inside);

                        // Draw subtle dotted grid
                        let mut gx = rect.left() + 20.0;
                        while gx < rect.right() {
                            let mut gy = rect.top();
                            while gy < rect.bottom() {
                                p.line_segment([egui::pos2(gx, gy), egui::pos2(gx, (gy + 1.0_f32).min(rect.bottom()))], egui::Stroke::new(1.0_f32, Color32::from_rgb(35, 40, 50)));
                                gy += 4.0;
                            }
                            gx += 30.0;
                        }

                        // Bid line (blue) and Ask line (red)
                        let mid_y = rect.center().y;
                        let bid_y = mid_y + 12.0;
                        let ask_y = mid_y - 12.0;

                        // Mock tick line step graph
                        let pts_bid = vec![
                            egui::pos2(rect.left() + 10.0, mid_y + 16.0),
                            egui::pos2(rect.left() + 50.0, mid_y + 16.0),
                            egui::pos2(rect.left() + 50.0, mid_y + 10.0),
                            egui::pos2(rect.left() + 100.0, mid_y + 10.0),
                            egui::pos2(rect.left() + 100.0, bid_y),
                            egui::pos2(rect.right() - 25.0, bid_y),
                        ];
                        p.add(egui::Shape::line(pts_bid, egui::Stroke::new(1.4_f32, Color32::from_rgb(30, 110, 220))));
                        p.line_segment([egui::pos2(rect.left(), bid_y), egui::pos2(rect.right(), bid_y)], egui::Stroke::new(1.0_f32, Color32::from_rgb(30, 110, 220)));

                        let pts_ask = vec![
                            egui::pos2(rect.left() + 10.0, mid_y - 8.0),
                            egui::pos2(rect.left() + 50.0, mid_y - 8.0),
                            egui::pos2(rect.left() + 50.0, mid_y - 14.0),
                            egui::pos2(rect.left() + 100.0, mid_y - 14.0),
                            egui::pos2(rect.left() + 100.0, ask_y),
                            egui::pos2(rect.right() - 25.0, ask_y),
                        ];
                        p.add(egui::Shape::line(pts_ask, egui::Stroke::new(1.4_f32, Color32::from_rgb(220, 30, 30))));
                        p.line_segment([egui::pos2(rect.left(), ask_y), egui::pos2(rect.right(), ask_y)], egui::Stroke::new(1.0_f32, Color32::from_rgb(220, 30, 30)));

                        // Price labels on the right
                        p.text(egui::pos2(rect.right() - 2.0, bid_y), egui::Align2::RIGHT_BOTTOM, fmt_price(bid, digits), egui::FontId::monospace(9.5), Color32::from_rgb(50, 140, 255));
                        p.text(egui::pos2(rect.right() - 2.0, ask_y), egui::Align2::RIGHT_BOTTOM, fmt_price(ask, digits), egui::FontId::monospace(9.5), Color32::from_rgb(255, 60, 60));
                    });
                });

                ui.separator();

                // Right pane: Order Inputs & Execution Buttons
                ui.vertical(|ui| {
                    egui::Grid::new("order_grid").num_columns(2).min_col_width(95.0).spacing([8.0, 3.0]).show(ui, |ui| {
                        ui.label("Symbol:");
                        egui::ComboBox::from_id_salt("ord_sym").selected_text(format!("{}, {}", d.symbol, desc)).show_ui(ui, |ui| {
                            for s in &symbols {
                                ui.selectable_value(&mut d.symbol, s.clone(), s);
                            }
                        });
                        ui.end_row();
                        ui.label("Type:");
                        egui::ComboBox::from_id_salt("ord_type").selected_text(if d.pending { "Pending Order" } else { "Market Execution" }).show_ui(ui, |ui| {
                            ui.selectable_value(&mut d.pending, false, "Market Execution");
                            ui.selectable_value(&mut d.pending, true, "Pending Order");
                        });
                        ui.end_row();
                        ui.label("Volume:");
                        ui.add(egui::DragValue::new(&mut d.volume).speed(0.01).range(0.01..=100.0).max_decimals(2));
                        ui.end_row();
                        let step = 10f64.powi(-(digits as i32));
                        ui.label("Stop Loss:");
                        ui.add(egui::DragValue::new(&mut d.sl).speed(step * 10.0).max_decimals(digits as usize).range(0.0..=1e7));
                        ui.end_row();
                        ui.label("Take Profit:");
                        ui.add(egui::DragValue::new(&mut d.tp).speed(step * 10.0).max_decimals(digits as usize).range(0.0..=1e7));
                        ui.end_row();
                        if d.pending {
                            ui.label("Order type:");
                            egui::ComboBox::from_id_salt("ord_ptype").selected_text(d.ptype.label()).show_ui(ui, |ui| {
                                for t in [OrderType::BuyLimit, OrderType::SellLimit, OrderType::BuyStop, OrderType::SellStop] {
                                    ui.selectable_value(&mut d.ptype, t, t.label());
                                }
                            });
                            ui.end_row();
                            ui.label("At price:");
                            ui.add(egui::DragValue::new(&mut d.price).speed(step * 10.0).max_decimals(digits as usize).range(0.0..=1e7));
                            ui.end_row();
                        }
                        ui.label("Comment:");
                        ui.text_edit_singleline(&mut d.comment);
                        ui.end_row();
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(fmt_price(bid, digits)).size(22.0).color(crate::style::DOWN).strong());
                        ui.label(RichText::new(" / ").size(22.0).weak());
                        ui.label(RichText::new(fmt_price(ask, digits)).size(22.0).color(crate::style::UP).strong());
                    });
                    ui.add_space(8.0);
                    if d.pending {
                        if ui.add_sized([320.0, 32.0], egui::Button::new(RichText::new("Place").strong())).clicked() {
                            action = Some(TradeOp::Pending {
                                symbol: d.symbol.clone(),
                                typ: d.ptype as i64,
                                volume: d.volume,
                                price: d.price,
                                sl: d.sl,
                                tp: d.tp,
                                magic: 0,
                                comment: d.comment.clone(),
                            });
                        }
                    } else {
                        ui.horizontal(|ui| {
                            let sell = egui::Button::new(RichText::new("Sell by Market").color(Color32::WHITE).strong()).fill(Color32::from_rgb(215, 30, 30));
                            if ui.add_sized([156.0, 34.0], sell).clicked() {
                                action = Some(TradeOp::Market { symbol: d.symbol.clone(), buy: false, volume: d.volume, sl: d.sl, tp: d.tp, magic: 0, comment: d.comment.clone() });
                            }
                            let buy = egui::Button::new(RichText::new("Buy by Market").color(Color32::WHITE).strong()).fill(Color32::from_rgb(20, 100, 220));
                            if ui.add_sized([156.0, 34.0], buy).clicked() {
                                action = Some(TradeOp::Market { symbol: d.symbol.clone(), buy: true, volume: d.volume, sl: d.sl, tp: d.tp, magic: 0, comment: d.comment.clone() });
                            }
                        });
                    }
                });
            });
        });
        if let Some(op) = action {
            self.trade_ui(op);
            open = false;
        }
        if open {
            self.dialogs.order = Some(d);
        }
    }

    fn modify_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.dialogs.modify.take() else { return };
        let mut open = true;
        let mut action = None;
        let info = if d.order {
            self.account().orders.iter().find(|o| o.ticket == d.ticket).map(|o| (o.symbol.clone(), o.digits, format!("#{} {} {:.2} {}", o.ticket, o.typ.label(), o.volume, o.symbol)))
        } else {
            self.account().positions.iter().find(|p| p.ticket == d.ticket).map(|p| (p.symbol.clone(), p.digits, format!("#{} {} {:.2} {} at {}", p.ticket, if p.buy { "buy" } else { "sell" }, p.volume, p.symbol, fmt_price(p.price_open, p.digits))))
        };
        let Some((_sym, digits, title)) = info else { return };
        let step = 10f64.powi(-(digits as i32)) * 10.0;
        egui::Window::new(if d.order { "Modify order" } else { "Modify position" }).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
            ui.label(RichText::new(title).strong());
            egui::Grid::new("mod_grid").num_columns(2).show(ui, |ui| {
                if d.order {
                    ui.label("Price:");
                    ui.add(egui::DragValue::new(&mut d.price).speed(step).max_decimals(digits as usize));
                    ui.end_row();
                }
                ui.label("Stop Loss:");
                ui.add(egui::DragValue::new(&mut d.sl).speed(step).max_decimals(digits as usize).range(0.0..=1e7));
                ui.end_row();
                ui.label("Take Profit:");
                ui.add(egui::DragValue::new(&mut d.tp).speed(step).max_decimals(digits as usize).range(0.0..=1e7));
                ui.end_row();
            });
            ui.horizontal(|ui| {
                if ui.button("Modify").clicked() {
                    action = Some(if d.order {
                        TradeOp::ModifyOrder { ticket: d.ticket, price: d.price, sl: d.sl, tp: d.tp }
                    } else {
                        TradeOp::Modify { ticket: d.ticket, sl: d.sl, tp: d.tp }
                    });
                }
                if d.order {
                    if ui.button("Delete order").clicked() {
                        action = Some(TradeOp::Delete { ticket: d.ticket });
                    }
                } else if ui.button("Close position").clicked() {
                    action = Some(TradeOp::Close { ticket: d.ticket, volume: None });
                }
            });
        });
        if let Some(op) = action {
            self.trade_ui(op);
            open = false;
        }
        if open {
            self.dialogs.modify = Some(d);
        }
    }

    fn login_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.dialogs.login.take() else { return };
        let mut open = true;
        let mut try_login = false;
        let mut new_demo = false;
        egui::Window::new("Login to Trade Account").collapsible(false).resizable(false).open(&mut open).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            egui::Grid::new("login_grid").num_columns(2).min_col_width(90.0).show(ui, |ui| {
                ui.label("Login:");
                ui.text_edit_singleline(&mut d.login);
                ui.end_row();
                ui.label("Password:");
                ui.add(egui::TextEdit::singleline(&mut d.password).password(true));
                ui.end_row();
                ui.label("Server:");
                ui.label("Simulated-Demo");
                ui.end_row();
            });
            ui.checkbox(&mut d.save, "Save password");
            if let Some(e) = &d.error {
                ui.label(RichText::new(e).color(crate::style::DOWN));
            }
            ui.label(RichText::new("Accounts on this computer:").small().weak());
            for a in &self.accounts {
                if ui.small_button(format!("{} — {} ({})", a.login, a.name, a.server)).clicked() {
                    d.login = a.login.to_string();
                }
            }
            ui.horizontal(|ui| {
                if ui.button("   OK   ").clicked() {
                    try_login = true;
                }
                if ui.button("Open a demo account...").clicked() {
                    new_demo = true;
                }
            });
            ui.label(
                RichText::new("Real broker servers use a closed, encrypted protocol, so this terminal works with simulated demo accounts.")
                    .small()
                    .weak(),
            );
        });
        if try_login {
            let login = d.login.trim().parse::<u64>().unwrap_or(0);
            match self.login(login, &d.password) {
                Ok(()) => {
                    if !d.save {
                        // nothing stored besides the account itself
                    }
                    open = false;
                }
                Err(e) => d.error = Some(e),
            }
        }
        if new_demo {
            self.dialogs.demo = Some(DemoDlg { name: "Demo Trader".into(), email: String::new(), deposit: 10_000.0, leverage: 100, created: None });
            open = false;
        }
        if open {
            self.dialogs.login = Some(d);
        }
    }

    fn demo_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.dialogs.demo.take() else { return };
        let mut open = true;
        let mut create = false;
        let mut finish = false;
        egui::Window::new("Open an Account").collapsible(false).resizable(false).open(&mut open).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            if let Some((login, pass)) = &d.created {
                ui.label(RichText::new("Your demo account is ready").strong());
                egui::Grid::new("demo_done").num_columns(2).show(ui, |ui| {
                    ui.label("Login:");
                    ui.label(RichText::new(login.to_string()).monospace());
                    ui.end_row();
                    ui.label("Password:");
                    ui.label(RichText::new(pass).monospace());
                    ui.end_row();
                    ui.label("Server:");
                    ui.label("Simulated-Demo");
                    ui.end_row();
                });
                if ui.button("Finish").clicked() {
                    finish = true;
                }
                return;
            }
            egui::Grid::new("demo_grid").num_columns(2).min_col_width(100.0).show(ui, |ui| {
                ui.label("Name:");
                ui.text_edit_singleline(&mut d.name);
                ui.end_row();
                ui.label("Email (optional):");
                ui.text_edit_singleline(&mut d.email);
                ui.end_row();
                ui.label("Deposit:");
                egui::ComboBox::from_id_salt("dep").selected_text(format!("{:.0} USD", d.deposit)).show_ui(ui, |ui| {
                    for v in [1000.0, 3000.0, 5000.0, 10000.0, 50000.0, 100000.0] {
                        ui.selectable_value(&mut d.deposit, v, format!("{:.0} USD", v));
                    }
                });
                ui.end_row();
                ui.label("Leverage:");
                egui::ComboBox::from_id_salt("lev").selected_text(format!("1:{}", d.leverage)).show_ui(ui, |ui| {
                    for v in [10, 30, 50, 100, 200, 500] {
                        ui.selectable_value(&mut d.leverage, v, format!("1:{}", v));
                    }
                });
                ui.end_row();
            });
            if ui.add_enabled(!d.name.trim().is_empty(), egui::Button::new("Open account")).clicked() {
                create = true;
            }
        });
        if create {
            let mut rng = crate::market::Rng::new(crate::market::unix_now() as u64 ^ 0xA5A5);
            let login = 50_000_000 + rng.next_u64() % 9_000_000;
            let pass: String = (0..8).map(|_| (b'a' + (rng.next_u64() % 26) as u8) as char).collect();
            let mut a = Account::new_demo(login, d.name.trim(), d.deposit, d.leverage);
            a.password = pass.clone();
            self.accounts.push(a);
            self.log_journal("Network", format!("demo account {} opened", login));
            let _ = self.login(login, &pass);
            d.created = Some((login, pass));
        }
        if finish {
            open = false;
        }
        if open {
            self.dialogs.demo = Some(d);
        }
    }

    fn attach_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.dialogs.attach.take() else { return };
        let mut open = true;
        let mut ok = false;
        let title = format!("{} - {}", d.prog.kind.label(), d.prog.file_name.trim_end_matches(".mq5"));
        egui::Window::new(title).collapsible(false).resizable(true).default_width(460.0).open(&mut open).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut d.tab, 0, "Common");
                ui.selectable_value(&mut d.tab, 1, "Inputs");
            });
            ui.separator();
            if d.tab == 0 {
                let p = &d.prog.props;
                if !p.description.is_empty() {
                    ui.label(&p.description);
                }
                if !p.copyright.is_empty() {
                    ui.label(RichText::new(&p.copyright).weak());
                }
                ui.label(RichText::new(d.path.display().to_string()).small().weak());
                if d.prog.kind == ProgramKind::Expert || d.prog.kind == ProgramKind::Script {
                    ui.checkbox(&mut d.allow_algo, "Allow Algo Trading");
                }
            } else {
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    inputs_ui(ui, &mut d.rows);
                });
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("   OK   ").clicked() {
                    ok = true;
                }
                if ui.button("Reset").clicked() {
                    d.rows = input_rows(&d.prog, &HashMap::new());
                }
            });
        });
        if ok {
            let inputs: HashMap<String, String> = d.rows.iter().map(|r| (r.name.clone(), r.value.clone())).collect();
            if matches!(d.prog.kind, ProgramKind::Expert | ProgramKind::Script) && d.allow_algo && !self.algo {
                self.algo = true;
                self.log_journal("Experts", "automated trading is enabled".into());
            }
            match d.target {
                AttachTarget::Custom(k) => {
                    self.remove_custom(d.chart, k, 5);
                    self.attach_compiled(d.chart, &d.path, d.prog.clone(), inputs);
                    // keep the order of indicators
                    let n = self.charts[d.chart].customs.len();
                    if n > 0 && k < n - 1 {
                        let c = self.charts[d.chart].customs.pop().unwrap();
                        self.charts[d.chart].customs.insert(k, c);
                    }
                }
                AttachTarget::Expert => {
                    self.remove_expert(d.chart, 5);
                    self.attach_compiled(d.chart, &d.path, d.prog.clone(), inputs);
                }
                AttachTarget::New => {
                    self.attach_compiled(d.chart, &d.path, d.prog.clone(), inputs);
                }
            }
            open = false;
        }
        if open {
            self.dialogs.attach = Some(d);
        }
    }

    fn ind_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.dialogs.ind.take() else { return };
        let mut open = true;
        let mut ok = false;
        egui::Window::new(d.kind.name()).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
            ui.label(RichText::new("Parameters").strong());
            params_ui(ui, d.kind, &mut d.params);
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("   OK   ").clicked() {
                    ok = true;
                }
                if ui.button("Reset").clicked() {
                    d.params = IndParams::defaults(d.kind);
                }
            });
        });
        if ok {
            if d.chart < self.charts.len() {
                let list = &mut self.charts[d.chart].chart.builtins;
                match d.edit {
                    Some(i) if i < list.len() => list[i] = BuiltinInd::new(d.kind, d.params),
                    _ => list.push(BuiltinInd::new(d.kind, d.params)),
                }
            }
            open = false;
        }
        if open {
            self.dialogs.ind = Some(d);
        }
    }

    fn props_window(&mut self, ctx: &egui::Context) {
        if !self.dialogs.props || self.charts.is_empty() {
            return;
        }
        let mut open = true;
        let ai = self.active.min(self.charts.len() - 1);
        let mut scheme = self.scheme;
        egui::Window::new("Properties").collapsible(false).resizable(false).default_width(440.0).open(&mut open).show(ctx, |ui| {
            let c = &mut self.charts[ai].chart;
            ui.label(RichText::new(c.title()).strong());
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Color scheme:").strong());
                    for s in [Scheme::GreenOnBlack, Scheme::BlackOnWhite, Scheme::ColorOnWhite] {
                        ui.radio_value(&mut scheme, s, s.label());
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new("Chart mode:").strong());
                    ui.radio_value(&mut c.mode, crate::chart::ChartMode::Bars, "Bar Chart");
                    ui.radio_value(&mut c.mode, crate::chart::ChartMode::Candles, "Candlesticks");
                    ui.radio_value(&mut c.mode, crate::chart::ChartMode::Line, "Line Chart");
                });

                ui.separator();

                // Live Color Scheme Preview Thumbnail (authentic MT5 F8 feature)
                ui.vertical(|ui| {
                    ui.label(RichText::new("Preview:").strong());
                    let (pre_rect, _) = ui.allocate_exact_size(egui::vec2(160.0, 120.0), egui::Sense::hover());
                    let p = ui.painter();
                    let th = crate::chart::Theme::of(scheme);
                    p.rect_filled(pre_rect, 0.0, th.bg);
                    p.rect_stroke(pre_rect, 0.0, egui::Stroke::new(1.0_f32, Color32::from_rgb(120, 120, 125)), egui::StrokeKind::Inside);

                    // Grid dots
                    let mut gx = pre_rect.left() + 20.0;
                    while gx < pre_rect.right() {
                        let mut gy = pre_rect.top() + 10.0;
                        while gy < pre_rect.bottom() {
                            p.line_segment([egui::pos2(gx, gy), egui::pos2(gx, gy + 1.0_f32)], egui::Stroke::new(1.0_f32, th.grid));
                            gy += 6.0;
                        }
                        gx += 25.0;
                    }

                    // Sample candlesticks
                    let xs = [pre_rect.left() + 35.0, pre_rect.left() + 65.0, pre_rect.left() + 95.0, pre_rect.left() + 125.0];
                    let samples = [(30.0_f32, 50.0_f32, true), (25.0_f32, 60.0_f32, false), (40.0_f32, 20.0_f32, true), (35.0_f32, 55.0_f32, false)];
                    for (x, (t, h, up)) in xs.iter().zip(samples.iter()) {
                        let top_y = pre_rect.top() + *t;
                        let bot_y = top_y + *h;
                        let out_col = if *up { th.bar_up } else { th.bar_down };
                        let fill_col = if *up { th.bull } else { th.bear };
                        p.line_segment([egui::pos2(*x, top_y - 8.0), egui::pos2(*x, bot_y + 8.0)], egui::Stroke::new(1.0_f32, out_col));
                        let cr = egui::Rect::from_min_max(egui::pos2(*x - 4.0, top_y), egui::pos2(*x + 4.0, bot_y));
                        p.rect_filled(cr, 0.0, fill_col);
                        p.rect_stroke(cr, 0.0, egui::Stroke::new(1.0_f32, out_col), egui::StrokeKind::Inside);
                    }
                });
            });

            ui.separator();
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.checkbox(&mut c.grid, "Show grid");
                    ui.checkbox(&mut c.volumes, "Show volumes");
                    ui.checkbox(&mut c.show_ask, "Show Ask line");
                    ui.checkbox(&mut c.period_sep, "Show period separators");
                });
                ui.vertical(|ui| {
                    ui.checkbox(&mut c.one_click, "Show One Click Trading panel");
                    ui.checkbox(&mut c.autoscroll, "Auto scroll");
                    ui.checkbox(&mut c.shift, "Chart shift");
                });
            });
        });
        if scheme != self.scheme {
            self.scheme = scheme;
            for s in self.charts.iter_mut() {
                s.chart.scheme = scheme;
            }
        }
        self.dialogs.props = open;
    }

    fn objects_window(&mut self, ctx: &egui::Context) {
        if !self.dialogs.objects || self.charts.is_empty() {
            return;
        }
        let mut open = true;
        let ai = self.active.min(self.charts.len() - 1);
        egui::Window::new("Objects List").collapsible(false).default_width(420.0).open(&mut open).show(ctx, |ui| {
            let c = &mut self.charts[ai].chart;
            ui.label(RichText::new(c.title()).strong());
            let mut delete = None;
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                egui::Grid::new("objs").striped(true).num_columns(4).show(ui, |ui| {
                    ui.label(RichText::new("Name").strong());
                    ui.label(RichText::new("Type").strong());
                    ui.label(RichText::new("Text").strong());
                    ui.label("");
                    ui.end_row();
                    for (i, o) in c.objects.list.iter_mut().enumerate() {
                        ui.label(&o.name);
                        ui.label(o.kind.label());
                        ui.add(egui::TextEdit::singleline(&mut o.text).desired_width(120.0));
                        if ui.small_button("Delete").clicked() {
                            delete = Some(i);
                        }
                        ui.end_row();
                    }
                });
            });
            if c.objects.list.is_empty() {
                ui.label(RichText::new("No objects. Use the drawing tools on the toolbar.").weak());
            }
            if let Some(i) = delete {
                c.objects.list.remove(i);
            }
            if ui.button("Delete all").clicked() {
                c.objects.list.clear();
            }
        });
        self.dialogs.objects = open;
    }
}
