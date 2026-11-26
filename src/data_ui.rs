//! History Data window: online data sources and API keys, time zones, import price
//! history (CSV or from MetaTrader 5), see where each symbol's prices come from, and switch
//! live prices on or off.

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
    /// API keys being edited (loaded when the window opens)
    pub keys: Option<crate::feed::Keys>,
    pub show_secrets: bool,
    /// a running source check
    pub check: Option<std::sync::mpsc::Receiver<Vec<(String, Result<String, String>)>>>,
    pub check_result: Vec<(String, Result<String, String>)>,
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
        egui::Window::new("History Data").collapsible(false).resizable(true).vscroll(true).default_width(620.0).default_height(640.0).open(&mut open).show(ctx, |ui| {
            // ---- sources ----
            ui.label(RichText::new("Symbols").strong());
            let keys_now = crate::feed::Keys::load();
            egui::Grid::new("sources").striped(true).num_columns(4).show(ui, |ui| {
                ui.label(RichText::new("Symbol").weak());
                ui.label(RichText::new("Prices").weak());
                ui.label(RichText::new("Online sources (first that works)").weak());
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
                    let routes = crate::feed::routes(s.spec.name, &keys_now);
                    let list = routes.iter().map(|r| {
                        if Some(r.provider.name()) == s.provider {
                            format!("[{}]", r.provider.name())
                        } else {
                            r.provider.name().to_string()
                        }
                    }).collect::<Vec<_>>().join(" > ");
                    ui.label(if list.is_empty() { "-".to_string() } else { list });
                    let tfs = stored.iter().find(|(n, _)| n.eq_ignore_ascii_case(s.spec.name)).map(|(_, t)| t.iter().map(|x| x.label()).collect::<Vec<_>>().join(" ")).unwrap_or_default();
                    ui.label(tfs);
                    ui.end_row();
                }
            });
            ui.add_space(8.0);
            ui.separator();

            // ---- live prices ----
            let mut live = self.market.live_running();
            if ui.checkbox(&mut live, "Live prices from online sources").changed() {
                if live {
                    let n = self.market.start_live();
                    if n == 0 {
                        dlg.result = vec!["no symbol has an online price source".into()];
                    }
                } else {
                    self.market.stop_live();
                }
            }
            if !self.market.feed_status.is_empty() {
                ui.label(RichText::new(&self.market.feed_status).weak());
            }
            if !self.market.feed_log.is_empty() {
                egui::CollapsingHeader::new("Source messages").id_salt("feed_log").show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                        for l in self.market.feed_log.iter().rev() {
                            ui.label(RichText::new(l).small());
                        }
                    });
                });
            }
            ui.horizontal(|ui| {
                ui.label("Add symbol");
                let id = egui::Id::new("data_add_symbol");
                let mut text = ui.data_mut(|d| d.get_temp::<String>(id).unwrap_or_default());
                ui.add(egui::TextEdit::singleline(&mut text).desired_width(160.0).hint_text("AAPL, NQ, BTCUSDT.P"));
                if ui.button("Add").clicked() {
                    dlg.result = match self.market.add_symbol(&text) {
                        Ok(i) => {
                            text.clear();
                            vec![format!("{} added", self.market.symbols[i].spec.name)]
                        }
                        Err(e) => vec![e],
                    };
                }
                ui.data_mut(|d| d.insert_temp(id, text));
            });
            ui.label(RichText::new("Crypto BTCUSD/ETHUSDT, perpetuals BTCUSDT.P or BLOFIN:BTCUSDT.P, US stocks AAPL, forex/metals EURUSD/XAUUSD, CME futures NQ MNQ ES MES. A prefix pins one source: KRAKEN:, COINBASE:, YAHOO:, NASDAQ:, ALPACA:, POLYGON:, OANDA:.").weak().small());
            ui.add_space(6.0);
            ui.separator();

            // ---- API keys ----
            ui.label(RichText::new("API keys (optional, premium sources)").strong());
            let keys = dlg.keys.get_or_insert_with(crate::feed::Keys::load);
            let pw = !dlg.show_secrets;
            egui::Grid::new("keys").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                let a = keys.alpaca.get_or_insert_with(Default::default);
                ui.label("Alpaca key ID");
                ui.add(egui::TextEdit::singleline(&mut a.key_id).desired_width(300.0).hint_text("PK... (free at alpaca.markets)"));
                ui.end_row();
                ui.label("Alpaca secret");
                ui.add(egui::TextEdit::singleline(&mut a.secret_key).password(pw).desired_width(300.0));
                ui.end_row();
                ui.label("Alpaca feed");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut a.feed, "iex".to_string(), "iex (free)");
                    ui.radio_value(&mut a.feed, "sip".to_string(), "sip (paid)");
                });
                ui.end_row();
                let p = keys.polygon.get_or_insert_with(Default::default);
                ui.label("Polygon API key");
                ui.add(egui::TextEdit::singleline(&mut p.api_key).password(pw).desired_width(300.0).hint_text("free = end of day; paid = live, futures"));
                ui.end_row();
                let o = keys.oanda.get_or_insert_with(Default::default);
                ui.label("OANDA token");
                ui.add(egui::TextEdit::singleline(&mut o.token).password(pw).desired_width(300.0).hint_text("free demo account at oanda.com"));
                ui.end_row();
                ui.label("OANDA account");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut o.account, "practice".to_string(), "practice (demo)");
                    ui.radio_value(&mut o.account, "live".to_string(), "live");
                });
                ui.end_row();
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut dlg.show_secrets, "Show secrets");
                if ui.button("Save Keys").clicked() {
                    let mut k = dlg.keys.clone().unwrap_or_default();
                    // empty entries are left out of the file
                    if k.alpaca.as_ref().is_some_and(|a| a.key_id.trim().is_empty() && a.secret_key.trim().is_empty()) {
                        k.alpaca = None;
                    }
                    if k.polygon.as_ref().is_some_and(|p| p.api_key.trim().is_empty()) {
                        k.polygon = None;
                    }
                    if k.oanda.as_ref().is_some_and(|o| o.token.trim().is_empty()) {
                        k.oanda = None;
                    }
                    dlg.result = match k.save() {
                        Ok(()) => {
                            if self.market.live_running() {
                                self.market.start_live();
                            }
                            vec![format!("keys saved to {}", crate::feed::Keys::path().display())]
                        }
                        Err(e) => vec![e.to_string()],
                    };
                }
                let busy = dlg.check.is_some();
                if ui.add_enabled(!busy, egui::Button::new(if busy { "Checking..." } else { "Check Sources" })).on_hover_text("Saved keys and every free source: fetch hourly bars and a price").clicked() {
                    let (tx, rx) = std::sync::mpsc::channel();
                    std::thread::spawn(move || {
                        let _ = tx.send(crate::feed::check_sources(true));
                    });
                    dlg.check = Some(rx);
                    dlg.check_result.clear();
                }
            });
            if let Some(rx) = &dlg.check {
                if let Ok(r) = rx.try_recv() {
                    dlg.check_result = r;
                    dlg.check = None;
                } else {
                    ctx.request_repaint_after(std::time::Duration::from_millis(300));
                }
            }
            if !dlg.check_result.is_empty() {
                egui::Grid::new("check").striped(true).num_columns(3).show(ui, |ui| {
                    for (name, r) in &dlg.check_result {
                        ui.label(name);
                        match r {
                            Ok(s) => {
                                ui.label(RichText::new("OK").color(egui::Color32::from_rgb(20, 140, 40)));
                                ui.label(RichText::new(s).small());
                            }
                            Err(e) => {
                                ui.label(RichText::new("FAIL").color(egui::Color32::from_rgb(190, 40, 40)));
                                ui.label(RichText::new(e).small());
                            }
                        }
                        ui.end_row();
                    }
                });
            }
            ui.label(RichText::new(format!("Keys are stored only on this computer: {}", crate::feed::Keys::path().display())).weak().small());
            ui.add_space(6.0);
            ui.separator();

            // ---- time zones ----
            ui.label(RichText::new("Time zones").strong());
            ui.horizontal(|ui| {
                ui.label("Terminal (server) time");
                let cur = crate::tz::server();
                let mut sel = cur;
                egui::ComboBox::from_id_salt("server_zone").selected_text(cur.label()).width(260.0).show_ui(ui, |ui| {
                    for z in crate::tz::Zone::choices() {
                        ui.selectable_value(&mut sel, z, z.label());
                    }
                });
                if sel != cur {
                    crate::tz::save_server(sel);
                    self.market.reload_time_zone();
                    self.log_journal("History", format!("terminal time zone: {}", sel.label()));
                }
            });
            ui.horizontal(|ui| {
                ui.label("CSV files are written in");
                let cur = crate::tz::import_zone();
                let mut sel = cur;
                egui::ComboBox::from_id_salt("import_zone").selected_text(cur.label()).width(260.0).show_ui(ui, |ui| {
                    for z in crate::tz::Zone::choices() {
                        ui.selectable_value(&mut sel, z, z.label());
                    }
                });
                if sel != cur {
                    crate::tz::save_import_zone(sel);
                }
            });
            ui.label(RichText::new("Online prices arrive in UTC and MetaTrader exports record their broker's offset; both are converted to the terminal time, which charts, bar times and TimeCurrent() use. TimeGMT() stays UTC.").weak().small());
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
                    dlg.result = match crate::history::import_file(&path, if sym.is_empty() { None } else { Some(&sym) }, dlg.tf, crate::tz::import_zone()) {
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
            ui.label(RichText::new("Accounts and order execution stay simulated.").weak().small());
        });
        if open {
            self.dialogs.data = Some(dlg);
        }
    }
}
