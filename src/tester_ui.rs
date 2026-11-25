//! Strategy Tester panel: settings, inputs, progress, balance/equity graph, report and journal.

use crate::app::TerminalApp;
use crate::dialogs::{input_rows, inputs_ui, InputRow};
use crate::market::Timeframe;
use crate::mql::{Program, ProgramKind};
use crate::storage;
use crate::tester::{TestRun, TickMode};
use egui::{pos2, Color32, RichText, Stroke};
use std::path::PathBuf;
use std::rc::Rc;

pub struct TesterUi {
    pub expert: Option<PathBuf>,
    pub symbol: String,
    pub tf: Timeframe,
    pub mode: TickMode,
    pub deposit: f64,
    pub leverage: u32,
    pub rows: Vec<InputRow>,
    pub prog: Option<Rc<Program>>,
    pub run: Option<TestRun>,
    pub tab: usize,
    pub speed: usize,
    pub error: Option<String>,
}

impl Default for TesterUi {
    fn default() -> Self {
        TesterUi {
            expert: None,
            symbol: "EURUSD".into(),
            tf: Timeframe::H1,
            mode: TickMode::OhlcPoints,
            deposit: 10_000.0,
            leverage: 100,
            rows: vec![],
            prog: None,
            run: None,
            tab: 0,
            speed: 40,
            error: None,
        }
    }
}

impl TerminalApp {
    fn tester_select(&mut self, path: PathBuf) {
        self.tester.expert = Some(path.clone());
        self.tester.error = None;
        match crate::host::load_program(&mut self.cache, &path) {
            Ok(p) if p.kind == ProgramKind::Expert => {
                self.tester.rows = input_rows(&p, &Default::default());
                self.tester.prog = Some(p);
            }
            Ok(_) => {
                self.tester.prog = None;
                self.tester.error = Some("not an expert advisor (needs OnTick)".into());
            }
            Err(e) => {
                self.tester.prog = None;
                self.tester.error = Some(e);
            }
        }
    }

    fn tester_start(&mut self) {
        let Some(prog) = self.tester.prog.clone() else { return };
        let Some(si) = self.market.symbol_index(&self.tester.symbol) else { return };
        let spec = self.market.symbols[si].spec.clone();
        // real one-minute history drives the test when it exists
        let minutes = crate::history::load(spec.name, crate::market::Timeframe::M1).filter(|m| m.len() > 200);
        let inputs = self.tester.rows.iter().map(|r| (r.name.clone(), r.value.clone())).collect();
        let loader_cache = std::cell::RefCell::new(crate::host::ProgramCache::new());
        let loader = Box::new(move |name: &str| crate::host::load_program(&mut loader_cache.borrow_mut(), &crate::host::indicator_path(name)));
        let run = match minutes {
            Some(m) => TestRun::with_minutes(prog, spec, self.tester.tf, m, self.tester.mode, self.tester.deposit, self.tester.leverage, &inputs, loader),
            None => {
                let bars = self.market.bars(si, self.tester.tf).to_vec();
                TestRun::new(prog, spec, self.tester.tf, bars, self.tester.mode, self.tester.deposit, self.tester.leverage, &inputs, loader)
            }
        };
        self.tester.run = Some(run);
        self.tester.tab = 2;
    }

    pub fn tester_panel(&mut self, ui: &mut egui::Ui) {
        // advance a running test
        if let Some(run) = self.tester.run.as_mut() {
            if !run.done {
                run.step(self.tester.speed);
                ui.ctx().request_repaint();
            }
        }
        let experts = self.nav_list("Experts");
        ui.horizontal(|ui| {
            for (i, t) in ["Settings", "Inputs", "Graph", "Backtest", "Journal"].iter().enumerate() {
                ui.selectable_value(&mut self.tester.tab, i, *t);
            }
            ui.separator();
            let running = self.tester.run.as_ref().is_some_and(|r| !r.done);
            if running {
                if ui.button("⏹ Stop").clicked() {
                    if let Some(r) = self.tester.run.as_mut() {
                        r.done = true;
                    }
                }
            } else if ui.add_enabled(self.tester.prog.is_some(), egui::Button::new("▶ Start")).clicked() {
                self.tester_start();
            }
            if let Some(r) = &self.tester.run {
                ui.add(egui::ProgressBar::new(r.progress()).desired_width(220.0).show_percentage());
                ui.label(format!("{} bars, {} ticks", r.bar - r.start_bar, r.ticks));
            }
        });
        ui.separator();
        match self.tester.tab {
            0 => {
                egui::Grid::new("tester_settings").num_columns(2).min_col_width(110.0).show(ui, |ui| {
                    ui.label("Expert:");
                    let cur = self.tester.expert.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "(choose)".into());
                    let mut pick = None;
                    egui::ComboBox::from_id_salt("t_expert").selected_text(cur).width(260.0).show_ui(ui, |ui| {
                        for f in experts.iter().filter(|f| !f.ex5) {
                            if ui.selectable_label(false, &f.rel).clicked() {
                                pick = Some(f.path.clone());
                            }
                        }
                    });
                    if let Some(p) = pick {
                        self.tester_select(p);
                    }
                    ui.end_row();
                    ui.label("Symbol:");
                    egui::ComboBox::from_id_salt("t_sym").selected_text(self.tester.symbol.clone()).show_ui(ui, |ui| {
                        for s in &self.market.symbols {
                            ui.selectable_value(&mut self.tester.symbol, s.spec.name.to_string(), s.spec.name);
                        }
                    });
                    ui.end_row();
                    ui.label("Timeframe:");
                    egui::ComboBox::from_id_salt("t_tf").selected_text(self.tester.tf.label()).show_ui(ui, |ui| {
                        for tf in Timeframe::ALL {
                            ui.selectable_value(&mut self.tester.tf, tf, tf.label());
                        }
                    });
                    ui.end_row();
                    ui.label("Modelling:");
                    egui::ComboBox::from_id_salt("t_mode").selected_text(self.tester.mode.label()).show_ui(ui, |ui| {
                        for m in [TickMode::EveryTick, TickMode::OhlcPoints, TickMode::OpenPrices] {
                            ui.selectable_value(&mut self.tester.mode, m, m.label());
                        }
                    });
                    ui.end_row();
                    ui.label("Deposit:");
                    ui.add(egui::DragValue::new(&mut self.tester.deposit).speed(100.0).range(100.0..=10_000_000.0).suffix(" USD"));
                    ui.end_row();
                    ui.label("Leverage:");
                    egui::ComboBox::from_id_salt("t_lev").selected_text(format!("1:{}", self.tester.leverage)).show_ui(ui, |ui| {
                        for v in [10, 30, 50, 100, 200, 500] {
                            ui.selectable_value(&mut self.tester.leverage, v, format!("1:{}", v));
                        }
                    });
                    ui.end_row();
                    ui.label("Speed:");
                    ui.add(egui::Slider::new(&mut self.tester.speed, 1..=500).text("bars per frame"));
                    ui.end_row();
                });
                if let Some(e) = &self.tester.error {
                    ui.label(RichText::new(e).color(crate::style::DOWN));
                }
                ui.label(RichText::new(format!("History: the last {} bars of the chosen symbol and timeframe.", crate::market::HISTORY_BARS)).small().weak());
                if experts.is_empty() {
                    ui.label(format!("Put expert advisors (.mq5) in {}", storage::mql_dir().join("Experts").display()));
                }
            }
            1 => {
                egui::ScrollArea::vertical().show(ui, |ui| inputs_ui(ui, &mut self.tester.rows));
            }
            2 => self.tester_graph(ui),
            3 => self.tester_report(ui),
            _ => {
                egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                    if let Some(r) = &self.tester.run {
                        for l in r.host.log.iter().rev().take(400).collect::<Vec<_>>().into_iter().rev() {
                            ui.label(RichText::new(l).monospace().size(11.0));
                        }
                    }
                });
            }
        }
    }

    fn tester_graph(&mut self, ui: &mut egui::Ui) {
        let Some(run) = &self.tester.run else {
            ui.label("Run a test to see the balance and equity graph.");
            return;
        };
        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, egui::Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, Color32::WHITE);
        let curve = &run.curve;
        if curve.len() < 2 {
            return;
        }
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for (b, e) in curve {
            lo = lo.min(*b).min(*e);
            hi = hi.max(*b).max(*e);
        }
        let pad = (hi - lo).max(1.0) * 0.1;
        let (lo, hi) = (lo - pad, hi + pad);
        let left = rect.left() + 70.0;
        let w = rect.right() - left - 10.0;
        let total = (run.host.all.len() - run.start_bar).max(2);
        let x = |i: usize| left + w * i as f32 / (total - 1) as f32;
        let y = |v: f64| rect.bottom() - 16.0 - ((v - lo) / (hi - lo)) as f32 * (rect.height() - 26.0);
        let step = crate::chart::nice_step((hi - lo) / 5.0);
        let mut g = (lo / step).ceil() * step;
        while g < hi {
            p.line_segment([pos2(left, y(g)), pos2(rect.right() - 10.0, y(g))], Stroke::new(1.0_f32, Color32::from_rgb(230, 230, 230)));
            p.text(pos2(left - 4.0, y(g)), egui::Align2::RIGHT_CENTER, format!("{:.0}", g), egui::FontId::monospace(10.5), Color32::DARK_GRAY);
            g += step;
        }
        let eq: Vec<_> = curve.iter().enumerate().map(|(i, (_, e))| pos2(x(i), y(*e))).collect();
        let bal: Vec<_> = curve.iter().enumerate().map(|(i, (b, _))| pos2(x(i), y(*b))).collect();
        p.add(egui::Shape::line(eq, Stroke::new(1.2_f32, Color32::from_rgb(40, 180, 60))));
        p.add(egui::Shape::line(bal, Stroke::new(1.6_f32, Color32::from_rgb(30, 90, 200))));
        p.text(rect.left_top() + egui::vec2(76.0, 4.0), egui::Align2::LEFT_TOP, "— Balance", egui::FontId::proportional(11.0), Color32::from_rgb(30, 90, 200));
        p.text(rect.left_top() + egui::vec2(150.0, 4.0), egui::Align2::LEFT_TOP, "— Equity", egui::FontId::proportional(11.0), Color32::from_rgb(40, 180, 60));
    }

    fn tester_report(&mut self, ui: &mut egui::Ui) {
        let Some(run) = &self.tester.run else {
            ui.label("Run a test to see the report.");
            return;
        };
        let r = run.report();
        if let Some(e) = &run.error {
            ui.label(RichText::new(e).color(crate::style::DOWN));
        }
        egui::Grid::new("report").striped(true).num_columns(4).min_col_width(130.0).show(ui, |ui| {
            let row = |ui: &mut egui::Ui, a: &str, b: String, c: &str, d: String| {
                ui.label(a);
                ui.label(RichText::new(b).strong());
                ui.label(c);
                ui.label(RichText::new(d).strong());
                ui.end_row();
            };
            row(ui, "Initial Deposit", format!("{:.2}", r.initial), "Bars / Ticks", format!("{} / {}", r.bars, r.ticks));
            row(ui, "Total Net Profit", format!("{:.2}", r.net), "Profit Factor", format!("{:.2}", r.profit_factor));
            row(ui, "Gross Profit", format!("{:.2}", r.gross_profit), "Gross Loss", format!("{:.2}", r.gross_loss));
            row(ui, "Expected Payoff", format!("{:.2}", r.expected_payoff), "Max Drawdown", format!("{:.2} ({:.2}%)", r.max_dd, r.max_dd_pct));
            row(
                ui,
                "Total Trades",
                r.trades.to_string(),
                "Profit Trades (% of total)",
                format!("{} ({:.1}%)", r.wins, if r.trades > 0 { r.wins as f64 * 100.0 / r.trades as f64 } else { 0.0 }),
            );
            row(ui, "Loss Trades", r.losses.to_string(), "Largest profit / loss", format!("{:.2} / {:.2}", r.largest_win, r.largest_loss));
        });
    }
}
