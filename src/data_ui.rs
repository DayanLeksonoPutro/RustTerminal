//! History Data window: import price history (CSV or from MetaTrader 5), see where each
//! symbol's prices come from, and switch live prices on or off.

use crate::app::TerminalApp;
use crate::market::{fmt_time, Source, Timeframe};
use egui::RichText;

#[derive(Default)]
pub struct DataDlg {
    pub path: String,
    pub symbol: String,
    /// None = detect from the file
    pub tf: Option<Timeframe>,
    pub result: Vec<String>,
}

impl TerminalApp {
    /// Import everything the MetaTrader 5 export script wrote and refresh the symbols.
    pub fn import_from_terminal(&mut self) -> Vec<String> {
        let (done, syms, errors) = crate::history::import_from_terminal();
        let mut out = vec![];
        for s in &syms {
            self.market.add_or_update_symbol(s);
        }
        if !syms.is_empty() {
            out.push(format!(
                "{} symbol(s) with the broker's properties",
                syms.len()
            ));
        }
        for i in &done {
            if self.market.symbol_index(&i.symbol).is_none() {
                self.market.load_real_data();
            }
            if let Some(si) = self.market.symbol_index(&i.symbol) {
                self.market.use_history(si);
            }
            out.push(format!(
                "{} {}: {} bars, {} - {}",
                i.symbol,
                i.tf.label(),
                i.bars,
                fmt_time(i.first, false),
                fmt_time(i.last, false)
            ));
        }
        out.extend(errors);
        if done.is_empty() && syms.is_empty() && out.is_empty() {
            out.push("nothing to import".into());
        }
        for l in &out {
            self.log_journal("History", l.clone());
        }
        out
    }

    pub fn data_window(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.dialogs.data.take() else {
            return;
        };
        let mut open = true;
        egui::Window::new("History Data").collapsible(false).resizable(true).default_width(560.0).open(&mut open).show(ctx, |ui| {
            // ---- sources ----
            ui.label(RichText::new("Symbols").strong());
            egui::Grid::new("sources").striped(true).num_columns(3).show(ui, |ui| {
                ui.label(RichText::new("Symbol").weak());
                ui.label(RichText::new("Prices").weak());
                ui.label(RichText::new("Stored history").weak());
                ui.end_row();
                let stored = crate::history::stored();
                for s in &self.market.symbols {
                    ui.label(s.spec.name);
                    let (txt, col) = match s.source {
                        Source::Live => ("live", egui::Color32::from_rgb(20, 140, 40)),
                        Source::History => ("real history", egui::Color32::from_rgb(20, 90, 170)),
                        Source::Simulated => ("simulated", egui::Color32::GRAY),
                    };
                    ui.label(RichText::new(txt).color(col));
                    let tfs = stored.iter().find(|(n, _)| n.eq_ignore_ascii_case(s.spec.name)).map(|(_, t)| t.iter().map(|x| x.label()).collect::<Vec<_>>().join(" ")).unwrap_or_default();
                    ui.label(tfs);
                    ui.end_row();
                }
            });
            ui.add_space(8.0);
            ui.separator();

            // ---- live prices ----
            let mut live = self.market.live_running();
            if ui.checkbox(&mut live, "Live prices for crypto symbols (public market data, no account needed)").changed() {
                if live {
                    let n = self.market.start_live();
                    if n == 0 {
                        dlg.result = vec!["no symbol has a live price source (crypto symbols such as BTCUSD)".into()];
                    }
                } else {
                    self.market.stop_live();
                }
            }
            if !self.market.feed_status.is_empty() {
                ui.label(RichText::new(&self.market.feed_status).weak());
            }
            ui.add_space(6.0);
            ui.separator();

            // ---- MetaTrader 5 ----
            ui.label(RichText::new("From MetaTrader 5 (your broker's real data)").strong());
            ui.label("1. Install the export script, 2. run RustTerminal_Export from Navigator > Scripts in MetaTrader 5, 3. import.");
            ui.horizontal(|ui| {
                if ui.button("Install Export Script").clicked() {
                    dlg.result = match crate::history::install_export_script() {
                        Ok(paths) => paths.iter().map(|p| format!("installed: {}", p.display())).collect(),
                        Err(e) => vec![e],
                    };
                }
                if ui.button("Import from MetaTrader 5").clicked() {
                    dlg.result = self.import_from_terminal();
                }
            });
            ui.add_space(6.0);
            ui.separator();

            // ---- CSV ----
            ui.label(RichText::new("Import a CSV file").strong());
            ui.label(RichText::new("MetaTrader exports (Symbols > Bars > Export), MT4/HST-style CSV, Dukascopy, or date,open,high,low,close,volume.").weak());
            ui.horizontal(|ui| {
                ui.label("File");
                ui.add(egui::TextEdit::singleline(&mut dlg.path).desired_width(380.0).hint_text("/path/to/EURUSD_M1.csv"));
            });
            ui.horizontal(|ui| {
                ui.label("Symbol");
                ui.add(egui::TextEdit::singleline(&mut dlg.symbol).desired_width(90.0).hint_text("from name"));
                ui.label("Timeframe");
                egui::ComboBox::from_id_salt("imp_tf").selected_text(dlg.tf.map(|t| t.label()).unwrap_or("detect")).show_ui(ui, |ui| {
                    ui.selectable_value(&mut dlg.tf, None, "detect");
                    for t in Timeframe::ALL {
                        ui.selectable_value(&mut dlg.tf, Some(t), t.label());
                    }
                });
                if ui.button("Import").clicked() {
                    let path = std::path::PathBuf::from(dlg.path.trim());
                    let sym = dlg.symbol.trim().to_string();
                    dlg.result = match crate::history::import_file(&path, if sym.is_empty() { None } else { Some(&sym) }, dlg.tf) {
                        Ok(i) => {
                            if self.market.symbol_index(&i.symbol).is_none() {
                                self.market.load_real_data();
                            }
                            if let Some(si) = self.market.symbol_index(&i.symbol) {
                                self.market.use_history(si);
                            }
                            let l = format!("{} {}: {} bars, {} - {}", i.symbol, i.tf.label(), i.bars, fmt_time(i.first, false), fmt_time(i.last, false));
                            self.log_journal("History", l.clone());
                            vec![l]
                        }
                        Err(e) => vec![e],
                    };
                }
            });
            if ui.button("Open History Folder").clicked() {
                let d = crate::history::history_dir();
                let _ = std::fs::create_dir_all(&d);
                crate::storage::open_in_file_manager(&d);
            }
            if !dlg.result.is_empty() {
                ui.add_space(6.0);
                ui.separator();
                for l in &dlg.result {
                    ui.label(l);
                }
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Accounts and order execution stay simulated. Times are as the source wrote them (MetaTrader exports use the broker's server time).").weak().small());
        });
        if open {
            self.dialogs.data = Some(dlg);
        }
    }
}
