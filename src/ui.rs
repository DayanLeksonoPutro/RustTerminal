//! Main window layout: menu, toolbar, Market Watch, Navigator / Data Window, chart tabs,
//! Toolbox (Trade, History, Journal, Experts), Strategy Tester and status bar.

use crate::app::{LeftTab, TerminalApp, ToolboxTab};
use crate::chart::{ChartAction, ChartInput, ChartMode, ExpertBadge, Theme, Tool};
use crate::dialogs::{AttachTarget, IndDlg, LoginDlg, ModifyDlg};
use crate::indicators::{IndKind, IndParams};
use crate::market::{fmt_price, fmt_time, Timeframe};
use crate::mql::interp::TradeOp;
use crate::storage;
use crate::style::{DOWN, HEADER, UP};
use egui::{Color32, RichText};

pub fn draw(app: &mut TerminalApp, ctx: &egui::Context) {
    shortcuts(app, ctx);
    menu_bar(app, ctx);
    toolbar(app, ctx);
    status_bar(app, ctx);
    if app.show_tester {
        egui::TopBottomPanel::bottom("tester").resizable(true).default_height(250.0).min_height(120.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Strategy Tester").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✖").clicked() {
                        app.show_tester = false;
                    }
                });
            });
            app.tester_panel(ui);
        });
    }
    if app.show_toolbox {
        egui::TopBottomPanel::bottom("toolbox").resizable(true).default_height(170.0).min_height(80.0).show(ctx, |ui| toolbox(app, ui));
    }
    if app.show_market_watch || app.show_navigator {
        egui::SidePanel::left("left").resizable(true).default_width(290.0).min_width(200.0).show(ctx, |ui| left_panel(app, ui));
    }
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(Color32::from_rgb(90, 90, 96))).show(ctx, |ui| central(app, ui));
    if let Some(ev) = app.editor.show(ctx) {
        match ev {
            crate::editor::EditorEvent::Attach(p) => {
                app.nav_dirty = true;
                if !app.charts.is_empty() {
                    let ai = app.active.min(app.charts.len() - 1);
                    app.cache.remove(&p);
                    app.open_attach_dialog(ai, p, AttachTarget::New);
                }
            }
            crate::editor::EditorEvent::Compiled(p) => {
                app.nav_dirty = true;
                app.cache.remove(&p);
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                app.log_journal("Compiler", format!("'{}' compiled successfully", name));
            }
        }
    }
    app.show_dialogs(ctx);
}

fn shortcuts(app: &mut TerminalApp, ctx: &egui::Context) {
    if ctx.wants_keyboard_input() {
        return;
    }
    let ai = app.active;
    ctx.input_mut(|i| {
        use egui::{Key, Modifiers};
        if i.consume_key(Modifiers::NONE, Key::F9) {
            if let Some(s) = app.charts.get(ai).map(|c| c.chart.symbol.clone()) {
                app.new_order_dialog(&s);
            }
        }
        if i.consume_key(Modifiers::NONE, Key::F4) {
            app.editor.open = true;
        }
        if i.consume_key(Modifiers::NONE, Key::F8) {
            app.dialogs.props = true;
        }
        if i.consume_key(Modifiers::COMMAND, Key::M) {
            app.show_market_watch = !app.show_market_watch;
        }
        if i.consume_key(Modifiers::COMMAND, Key::N) {
            app.show_navigator = !app.show_navigator;
        }
        if i.consume_key(Modifiers::COMMAND, Key::T) {
            app.show_toolbox = !app.show_toolbox;
        }
        if i.consume_key(Modifiers::COMMAND, Key::R) {
            app.show_tester = !app.show_tester;
        }
        if i.consume_key(Modifiers::COMMAND, Key::D) {
            app.left_tab = LeftTab::DataWindow;
            app.show_navigator = true;
        }
        if i.consume_key(Modifiers::COMMAND, Key::B) {
            app.dialogs.objects = true;
        }
        if i.consume_key(Modifiers::COMMAND, Key::E) {
            app.algo = !app.algo;
        }
        if let Some(c) = app.charts.get_mut(ai) {
            if i.consume_key(Modifiers::NONE, Key::Plus) || i.consume_key(Modifiers::NONE, Key::Equals) {
                c.chart.zoom(1.25);
            }
            if i.consume_key(Modifiers::NONE, Key::Minus) {
                c.chart.zoom(0.8);
            }
            if i.consume_key(Modifiers::COMMAND, Key::G) {
                c.chart.grid = !c.chart.grid;
            }
            if i.consume_key(Modifiers::COMMAND, Key::L) {
                c.chart.volumes = !c.chart.volumes;
            }
            if i.consume_key(Modifiers::COMMAND, Key::Y) {
                c.chart.period_sep = !c.chart.period_sep;
            }
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                c.chart.tool = Tool::Cursor;
            }
            if i.consume_key(Modifiers::NONE, Key::End) {
                c.chart.autoscroll = true;
            }
        }
    });
}

fn menu_bar(app: &mut TerminalApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("menu").show(ctx, |ui| {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                ui.menu_button("New Chart", |ui| {
                    let names: Vec<String> = app.market.symbols.iter().map(|s| s.spec.name.to_string()).collect();
                    for n in names {
                        if ui.button(&n).clicked() {
                            app.open_chart(&n, Timeframe::H1);
                            ui.close_menu();
                        }
                    }
                });
                if ui.button("Open Data Folder").clicked() {
                    storage::open_in_file_manager(&storage::data_dir());
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Login to Trade Account...").clicked() {
                    app.dialogs.login = Some(LoginDlg { login: app.account().login.to_string(), password: String::new(), save: true, error: None });
                    ui.close_menu();
                }
                if ui.button("Open an Account...").clicked() {
                    app.dialogs.demo = Some(crate::dialogs::DemoDlg { name: "Demo Trader".into(), email: String::new(), deposit: 10000.0, leverage: 100, created: None });
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Save Workspace").clicked() {
                    app.save_all();
                    ui.close_menu();
                }
                if ui.button("Exit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("View", |ui| {
                ui.checkbox(&mut app.show_market_watch, "Market Watch   Ctrl+M");
                ui.checkbox(&mut app.show_navigator, "Navigator   Ctrl+N");
                if ui.button("Data Window   Ctrl+D").clicked() {
                    app.left_tab = LeftTab::DataWindow;
                    app.show_navigator = true;
                    ui.close_menu();
                }
                ui.checkbox(&mut app.show_toolbox, "Toolbox   Ctrl+T");
                ui.checkbox(&mut app.show_tester, "Strategy Tester   Ctrl+R");
                if ui.button("MetaEditor   F4").clicked() {
                    app.editor.open = true;
                    ui.close_menu();
                }
            });
            ui.menu_button("Insert", |ui| {
                ui.menu_button("Indicators", |ui| {
                    for group in ["Trend", "Oscillators", "Volumes", "Bill Williams"] {
                        ui.menu_button(group, |ui| {
                            for k in IndKind::ALL.iter().filter(|k| k.group() == group) {
                                if ui.button(k.name()).clicked() {
                                    open_ind(app, *k, None);
                                    ui.close_menu();
                                }
                            }
                        });
                    }
                    ui.menu_button("Custom", |ui| {
                        for f in app.nav_list("Indicators") {
                            if ui.button(&f.rel).clicked() {
                                attach_file(app, f.path.clone());
                                ui.close_menu();
                            }
                        }
                    });
                });
                ui.menu_button("Objects", |ui| tool_buttons(app, ui, true));
                ui.menu_button("Expert Advisors", |ui| {
                    for f in app.nav_list("Experts") {
                        if ui.button(&f.rel).clicked() {
                            attach_file(app, f.path.clone());
                            ui.close_menu();
                        }
                    }
                });
                ui.menu_button("Scripts", |ui| {
                    for f in app.nav_list("Scripts") {
                        if ui.button(&f.rel).clicked() {
                            attach_file(app, f.path.clone());
                            ui.close_menu();
                        }
                    }
                });
            });
            ui.menu_button("Charts", |ui| {
                let ai = app.active;
                if let Some(slot) = app.charts.get_mut(ai) {
                    let c = &mut slot.chart;
                    ui.radio_value(&mut c.mode, ChartMode::Bars, "Bar Chart");
                    ui.radio_value(&mut c.mode, ChartMode::Candles, "Candlesticks");
                    ui.radio_value(&mut c.mode, ChartMode::Line, "Line Chart");
                    ui.separator();
                    ui.checkbox(&mut c.grid, "Grid   Ctrl+G");
                    ui.checkbox(&mut c.volumes, "Volumes   Ctrl+L");
                    ui.checkbox(&mut c.period_sep, "Period Separators   Ctrl+Y");
                    ui.checkbox(&mut c.autoscroll, "Auto Scroll");
                    ui.checkbox(&mut c.shift, "Chart Shift");
                    ui.checkbox(&mut c.one_click, "One Click Trading");
                    if ui.button("Zoom In   +").clicked() {
                        c.zoom(1.25);
                    }
                    if ui.button("Zoom Out   -").clicked() {
                        c.zoom(0.8);
                    }
                }
                ui.menu_button("Timeframes", |ui| {
                    for tf in Timeframe::ALL {
                        if ui.button(tf.label()).clicked() {
                            if ai < app.charts.len() {
                                app.set_timeframe(ai, tf);
                            }
                            ui.close_menu();
                        }
                    }
                });
                ui.separator();
                if ui.button("Objects List   Ctrl+B").clicked() {
                    app.dialogs.objects = true;
                    ui.close_menu();
                }
                if ui.button("Properties   F8").clicked() {
                    app.dialogs.props = true;
                    ui.close_menu();
                }
            });
            ui.menu_button("Tools", |ui| {
                if ui.button("New Order   F9").clicked() {
                    if let Some(s) = app.charts.get(app.active).map(|c| c.chart.symbol.clone()) {
                        app.new_order_dialog(&s);
                    }
                    ui.close_menu();
                }
                if ui.button("MetaEditor   F4").clicked() {
                    app.editor.open = true;
                    ui.close_menu();
                }
                if ui.button("Strategy Tester   Ctrl+R").clicked() {
                    app.show_tester = true;
                    ui.close_menu();
                }
                ui.checkbox(&mut app.algo, "Algo Trading   Ctrl+E");
            });
            ui.menu_button("Window", |ui| {
                let titles: Vec<String> = app.charts.iter().map(|c| c.chart.title()).collect();
                for (i, t) in titles.iter().enumerate() {
                    if ui.selectable_label(i == app.active, format!("{} {}", i + 1, t)).clicked() {
                        app.active = i;
                        ui.close_menu();
                    }
                }
                ui.separator();
                if ui.button("Close Chart").clicked() {
                    let a = app.active;
                    app.close_chart(a);
                    ui.close_menu();
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("About").clicked() {
                    app.dialogs.about = true;
                    ui.close_menu();
                }
            });
        });
    });
}

fn open_ind(app: &mut TerminalApp, kind: IndKind, edit: Option<usize>) {
    if app.charts.is_empty() {
        return;
    }
    let chart = app.active.min(app.charts.len() - 1);
    let params = match edit {
        Some(i) => app.charts[chart].chart.builtins.get(i).map(|b| b.params).unwrap_or(IndParams::defaults(kind)),
        None => IndParams::defaults(kind),
    };
    app.dialogs.ind = Some(IndDlg { chart, kind, params, edit });
}

fn attach_file(app: &mut TerminalApp, path: std::path::PathBuf) {
    if app.charts.is_empty() {
        let i = app.open_chart("EURUSD", Timeframe::H1);
        app.active = i;
    }
    let ai = app.active.min(app.charts.len() - 1);
    app.open_attach_dialog(ai, path, AttachTarget::New);
}

#[derive(Clone, Copy)]
enum Icon {
    Cursor,
    Cross,
    VLine,
    HLine,
    Trend,
    Rect,
    Fibo,
    Text,
    Up,
    Down,
    Bars,
    Candles,
    Line,
    AutoScroll,
    Shift,
}

/// A small toolbar button with a drawn icon (independent of the fonts' glyphs).
fn icon_button(ui: &mut egui::Ui, icon: Icon, selected: bool, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 20.0), egui::Sense::click());
    let resp = resp.on_hover_text(tip);
    let p = ui.painter();
    if selected || resp.hovered() {
        let fill = if selected { Color32::from_rgb(200, 220, 245) } else { Color32::from_rgb(225, 230, 238) };
        p.rect_filled(rect, 2.0, fill);
        p.rect_stroke(rect, 2.0, egui::Stroke::new(1.0_f32, Color32::from_rgb(150, 170, 200)), egui::StrokeKind::Inside);
    }
    let c = rect.center();
    let s = egui::Stroke::new(1.4_f32, Color32::from_rgb(40, 50, 70));
    let blue = egui::Stroke::new(1.4_f32, Color32::from_rgb(30, 110, 220));
    use egui::pos2;
    let v = |dx: f32, dy: f32| pos2(c.x + dx, c.y + dy);
    match icon {
        Icon::Cursor => {
            p.add(egui::Shape::convex_polygon(vec![v(-4.0, -7.0), v(-4.0, 5.0), v(-1.0, 2.0), v(4.0, 2.0)], Color32::WHITE, s));
        }
        Icon::Cross => {
            p.line_segment([v(-7.0, 0.0), v(7.0, 0.0)], s);
            p.line_segment([v(0.0, -7.0), v(0.0, 7.0)], s);
        }
        Icon::VLine => {
            p.line_segment([v(0.0, -7.0), v(0.0, 7.0)], blue);
        }
        Icon::HLine => {
            p.line_segment([v(-8.0, 0.0), v(8.0, 0.0)], blue);
        }
        Icon::Trend => {
            p.line_segment([v(-7.0, 6.0), v(7.0, -6.0)], blue);
            p.circle_filled(v(-7.0, 6.0), 1.8, Color32::from_rgb(30, 110, 220));
            p.circle_filled(v(7.0, -6.0), 1.8, Color32::from_rgb(30, 110, 220));
        }
        Icon::Rect => {
            p.rect_stroke(egui::Rect::from_center_size(c, egui::vec2(14.0, 10.0)), 0.0, blue, egui::StrokeKind::Inside);
        }
        Icon::Fibo => {
            for (k, y) in [-6.0, -2.0, 1.0, 6.0].iter().enumerate() {
                p.line_segment([v(-8.0 + k as f32, *y), v(8.0, *y)], egui::Stroke::new(1.0_f32, Color32::from_rgb(200, 140, 0)));
            }
        }
        Icon::Text => {
            p.text(c, egui::Align2::CENTER_CENTER, "T", egui::FontId::proportional(14.0), Color32::from_rgb(40, 50, 70));
        }
        Icon::Up => {
            p.add(egui::Shape::convex_polygon(vec![v(0.0, -6.0), v(-6.0, 5.0), v(6.0, 5.0)], Color32::from_rgb(30, 110, 220), egui::Stroke::NONE));
        }
        Icon::Down => {
            p.add(egui::Shape::convex_polygon(vec![v(0.0, 6.0), v(6.0, -5.0), v(-6.0, -5.0)], Color32::from_rgb(210, 40, 40), egui::Stroke::NONE));
        }
        Icon::Bars => {
            for (dx, h) in [(-5.0, 6.0), (0.0, 4.0), (5.0, 7.0)] {
                p.line_segment([v(dx, -h), v(dx, h)], s);
                p.line_segment([v(dx - 2.0, -h + 2.0), v(dx, -h + 2.0)], s);
                p.line_segment([v(dx, h - 2.0), v(dx + 2.0, h - 2.0)], s);
            }
        }
        Icon::Candles => {
            p.line_segment([v(-4.0, -8.0), v(-4.0, 8.0)], s);
            p.rect_filled(egui::Rect::from_center_size(v(-4.0, 0.0), egui::vec2(5.0, 10.0)), 0.0, Color32::from_rgb(40, 160, 70));
            p.line_segment([v(4.0, -7.0), v(4.0, 7.0)], s);
            p.rect_filled(egui::Rect::from_center_size(v(4.0, 1.0), egui::vec2(5.0, 8.0)), 0.0, Color32::from_rgb(210, 50, 50));
        }
        Icon::Line => {
            p.add(egui::Shape::line(vec![v(-8.0, 5.0), v(-4.0, -1.0), v(0.0, 2.0), v(4.0, -5.0), v(8.0, -2.0)], blue));
        }
        Icon::AutoScroll => {
            p.add(egui::Shape::convex_polygon(vec![v(-7.0, -5.0), v(-1.0, 0.0), v(-7.0, 5.0)], Color32::from_rgb(40, 50, 70), egui::Stroke::NONE));
            p.add(egui::Shape::convex_polygon(vec![v(0.0, -5.0), v(6.0, 0.0), v(0.0, 5.0)], Color32::from_rgb(40, 50, 70), egui::Stroke::NONE));
        }
        Icon::Shift => {
            p.line_segment([v(-7.0, 0.0), v(4.0, 0.0)], s);
            p.add(egui::Shape::convex_polygon(vec![v(1.0, -4.0), v(5.0, 0.0), v(1.0, 4.0)], Color32::from_rgb(40, 50, 70), egui::Stroke::NONE));
            p.line_segment([v(7.0, -6.0), v(7.0, 6.0)], s);
        }
    }
    resp
}

fn tool_buttons(app: &mut TerminalApp, ui: &mut egui::Ui, menu: bool) {
    let ai = app.active;
    let Some(slot) = app.charts.get_mut(ai) else { return };
    let tools = [
        (Tool::Cursor, Icon::Cursor, "Cursor"),
        (Tool::Crosshair, Icon::Cross, "Crosshair"),
        (Tool::VLine, Icon::VLine, "Vertical Line"),
        (Tool::HLine, Icon::HLine, "Horizontal Line"),
        (Tool::Trend, Icon::Trend, "Trend Line"),
        (Tool::Rectangle, Icon::Rect, "Rectangle"),
        (Tool::Fibo, Icon::Fibo, "Fibonacci Retracement"),
        (Tool::Text, Icon::Text, "Text"),
        (Tool::ArrowUp, Icon::Up, "Arrow Up"),
        (Tool::ArrowDown, Icon::Down, "Arrow Down"),
    ];
    for (t, icon, name) in tools {
        let r = if menu {
            ui.horizontal(|ui| {
                let a = icon_button(ui, icon, slot.chart.tool == t, name);
                let b = ui.selectable_label(slot.chart.tool == t, name);
                a.union(b)
            })
            .inner
        } else {
            icon_button(ui, icon, slot.chart.tool == t, name)
        };
        if r.clicked() {
            slot.chart.tool = t;
            if menu {
                ui.close_menu();
            }
        }
    }
}

fn toolbar(app: &mut TerminalApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let ai = app.active;
            if ui.button("📈 New Chart").clicked() {
                let sym = app.charts.get(ai).map(|c| c.chart.symbol.clone()).unwrap_or_else(|| "EURUSD".into());
                app.open_chart(&sym, Timeframe::H1);
            }
            if ui.button("📝 MetaEditor").on_hover_text("F4").clicked() {
                app.editor.open = !app.editor.open;
            }
            if ui.button("➕ New Order").on_hover_text("F9").clicked() {
                if let Some(s) = app.charts.get(ai).map(|c| c.chart.symbol.clone()) {
                    app.new_order_dialog(&s);
                }
            }
            let (txt, col) = if app.algo { ("▶ Algo Trading", Color32::from_rgb(30, 150, 50)) } else { ("Algo Trading", Color32::from_rgb(200, 40, 40)) };
            if ui.add(egui::Button::new(RichText::new(txt).color(Color32::WHITE)).fill(col)).on_hover_text("Allow expert advisors to trade (Ctrl+E)").clicked() {
                app.algo = !app.algo;
                let msg = if app.algo { "automated trading is enabled" } else { "automated trading is disabled" };
                app.log_journal("Experts", msg.into());
            }
            ui.separator();
            if let Some(slot) = app.charts.get_mut(ai) {
                let c = &mut slot.chart;
                for (m, icon, tip) in [(ChartMode::Bars, Icon::Bars, "Bar Chart"), (ChartMode::Candles, Icon::Candles, "Candlesticks"), (ChartMode::Line, Icon::Line, "Line Chart")] {
                    if icon_button(ui, icon, c.mode == m, tip).clicked() {
                        c.mode = m;
                    }
                }
                if ui.button("🔍+").on_hover_text("Zoom In").clicked() {
                    c.zoom(1.25);
                }
                if ui.button("🔍-").on_hover_text("Zoom Out").clicked() {
                    c.zoom(0.8);
                }
                if icon_button(ui, Icon::AutoScroll, c.autoscroll, "Auto Scroll").clicked() {
                    c.autoscroll = !c.autoscroll;
                }
                if icon_button(ui, Icon::Shift, c.shift, "Chart Shift").clicked() {
                    c.shift = !c.shift;
                }
            }
            ui.menu_button("ƒ Indicators", |ui| {
                for k in IndKind::ALL {
                    if ui.button(k.name()).clicked() {
                        open_ind(app, k, None);
                        ui.close_menu();
                    }
                }
            });
            ui.separator();
            tool_buttons(app, ui, false);
            ui.separator();
            if let Some(cur_tf) = app.charts.get(ai).map(|c| c.chart.tf) {
                for tf in Timeframe::ALL {
                    if ui.selectable_label(cur_tf == tf, tf.label()).clicked() {
                        app.set_timeframe(ai, tf);
                    }
                }
            }
            ui.separator();
            if ui.selectable_label(app.show_tester, "Strategy Tester").clicked() {
                app.show_tester = !app.show_tester;
            }
        });
    });
}

fn status_bar(app: &mut TerminalApp, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("status").exact_height(20.0).show(ctx, |ui| {
        ui.horizontal_centered(|ui| {
            ui.label("For Help, press F1");
            ui.separator();
            ui.label("Default");
            ui.separator();
            if !app.status.is_empty() {
                ui.label(RichText::new(&app.status).color(UP));
                ui.separator();
            }
            if let Some((b, _)) = &app.hover {
                let digits = app.charts.get(app.active).and_then(|c| app.market.symbol_index(&c.chart.symbol)).map(|i| app.market.symbols[i].spec.digits).unwrap_or(5);
                ui.label(RichText::new(format!(
                    "{}   O: {}   H: {}   L: {}   C: {}   V: {}",
                    fmt_time(b.time, false),
                    fmt_price(b.open, digits),
                    fmt_price(b.high, digits),
                    fmt_price(b.low, digits),
                    fmt_price(b.close, digits),
                    b.tick_volume
                ))
                .monospace());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if app.connected {
                    ui.label(RichText::new(format!("{} / {:.1} ms", app.account().server, app.ping_ms as f32)).color(Color32::from_rgb(20, 130, 40)));
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 14.0), egui::Sense::hover());
                    for k in 0..4 {
                        let h = 4.0 + k as f32 * 3.0;
                        let x = rect.left() + k as f32 * 4.5;
                        ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(x, rect.bottom() - h), egui::pos2(x + 3.0, rect.bottom())), 0.0, Color32::from_rgb(20, 150, 40));
                    }
                } else {
                    ui.label(RichText::new("No connection").color(DOWN));
                }
                ui.separator();
                ui.label(fmt_time(app.market.now, true));
            });
        });
    });
}

fn left_panel(app: &mut TerminalApp, ui: &mut egui::Ui) {
    if app.show_market_watch {
        let h = if app.show_navigator { ui.available_height() * 0.48 } else { ui.available_height() };
        ui.allocate_ui(egui::vec2(ui.available_width(), h), |ui| {
            ui.vertical(|ui| {
                header(ui, &format!("Market Watch: {}", fmt_time(app.market.now, true)[11..].to_string()));
                market_watch(app, ui);
            });
        });
    }
    if app.show_navigator {
        ui.separator();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut app.left_tab, LeftTab::Navigator, "Navigator");
            ui.selectable_value(&mut app.left_tab, LeftTab::DataWindow, "Data Window");
        });
        match app.left_tab {
            LeftTab::Navigator => navigator(app, ui),
            LeftTab::DataWindow => data_window(app, ui),
        }
    }
}

fn header(ui: &mut egui::Ui, text: &str) {
    egui::Frame::NONE.fill(HEADER).inner_margin(egui::Margin::symmetric(4, 2)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(text).strong());
    });
}

fn market_watch(app: &mut TerminalApp, ui: &mut egui::Ui) {
    let mut open: Option<String> = None;
    let mut order: Option<String> = None;
    egui::ScrollArea::vertical().id_salt("mw").auto_shrink([false, true]).show(ui, |ui| {
        egui::Grid::new("mw_grid").striped(true).num_columns(5).min_col_width(40.0).spacing([8.0, 2.0]).show(ui, |ui| {
            for h in ["Symbol", "Bid", "Ask", "Spread", "Daily Change"] {
                ui.label(RichText::new(h).strong().size(11.5));
            }
            ui.end_row();
            for s in &app.market.symbols {
                let up = s.bid >= s.prev_bid;
                let col = if up { UP } else { DOWN };
                let r = ui
                    .horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, 12.0), egui::Sense::hover());
                        let c = rect.center();
                        let pts = if up {
                            vec![egui::pos2(c.x, c.y - 4.0), egui::pos2(c.x - 4.0, c.y + 3.0), egui::pos2(c.x + 4.0, c.y + 3.0)]
                        } else {
                            vec![egui::pos2(c.x, c.y + 4.0), egui::pos2(c.x + 4.0, c.y - 3.0), egui::pos2(c.x - 4.0, c.y - 3.0)]
                        };
                        ui.painter().add(egui::Shape::convex_polygon(pts, col, egui::Stroke::NONE));
                        ui.add(egui::Label::new(RichText::new(s.spec.name).color(col)).sense(egui::Sense::click()))
                    })
                    .inner;
                if r.double_clicked() {
                    open = Some(s.spec.name.to_string());
                }
                let name = s.spec.name.to_string();
                r.on_hover_text(format!("{}\nDigits: {}  Contract: {}", s.spec.description, s.spec.digits, s.spec.contract_size)).context_menu(|ui| {
                    if ui.button("New Order   F9").clicked() {
                        order = Some(name.clone());
                        ui.close_menu();
                    }
                    if ui.button("Chart Window").clicked() {
                        open = Some(name.clone());
                        ui.close_menu();
                    }
                });
                ui.label(RichText::new(fmt_price(s.bid, s.spec.digits)).monospace().color(col));
                ui.label(RichText::new(fmt_price(s.ask, s.spec.digits)).monospace().color(col));
                ui.label(RichText::new(s.spread_points().to_string()).monospace());
                let ch = (s.bid - s.day_open) / s.day_open * 100.0;
                ui.label(RichText::new(format!("{:+.2}%", ch)).monospace().color(if ch >= 0.0 { UP } else { DOWN }));
                ui.end_row();
            }
        });
    });
    if let Some(s) = open {
        app.open_chart(&s, Timeframe::H1);
    }
    if let Some(s) = order {
        app.new_order_dialog(&s);
    }
}

fn navigator(app: &mut TerminalApp, ui: &mut egui::Ui) {
    let indicators = app.nav_list("Indicators");
    let experts = app.nav_list("Experts");
    let scripts = app.nav_list("Scripts");
    let mut attach: Option<std::path::PathBuf> = None;
    let mut edit: Option<std::path::PathBuf> = None;
    let mut add_ind: Option<IndKind> = None;
    let mut login: Option<u64> = None;
    egui::ScrollArea::vertical().id_salt("nav").auto_shrink([false, false]).show(ui, |ui| {
        ui.label(RichText::new("📂 Rust Terminal").strong());
        egui::CollapsingHeader::new("👤 Accounts").default_open(true).show(ui, |ui| {
            for (i, a) in app.accounts.iter().enumerate() {
                let txt = format!("{}: {} ({})", a.login, a.name, a.server);
                let r = ui.selectable_label(i == app.cur, txt).on_hover_text("Double-click to log in");
                if r.double_clicked() {
                    login = Some(a.login);
                }
            }
        });
        egui::CollapsingHeader::new("📊 Indicators").default_open(true).show(ui, |ui| {
            for group in ["Trend", "Oscillators", "Volumes", "Bill Williams"] {
                egui::CollapsingHeader::new(group).default_open(false).show(ui, |ui| {
                    for k in IndKind::ALL.iter().filter(|k| k.group() == group) {
                        if ui.selectable_label(false, format!("ƒ {}", k.name())).on_hover_text("Double-click to add to the chart").double_clicked() {
                            add_ind = Some(*k);
                        }
                    }
                });
            }
            egui::CollapsingHeader::new("Custom").default_open(true).show(ui, |ui| {
                program_list(ui, &indicators, "ƒ", &mut attach, &mut edit);
            });
        });
        egui::CollapsingHeader::new("☺ Expert Advisors").default_open(true).show(ui, |ui| {
            program_list(ui, &experts, "☺", &mut attach, &mut edit);
        });
        egui::CollapsingHeader::new("📜 Scripts").default_open(true).show(ui, |ui| {
            program_list(ui, &scripts, "▶", &mut attach, &mut edit);
        });
        ui.add_space(4.0);
        if ui.small_button("⟳ Refresh").clicked() {
            app.nav_dirty = true;
            app.editor.refresh_tree();
        }
    });
    if let Some(k) = add_ind {
        open_ind(app, k, None);
    }
    if let Some(p) = attach {
        attach_file(app, p);
    }
    if let Some(p) = edit {
        app.editor.open_file(p);
    }
    if let Some(l) = login {
        app.dialogs.login = Some(LoginDlg { login: l.to_string(), password: String::new(), save: true, error: None });
    }
}

fn program_list(ui: &mut egui::Ui, files: &[storage::ProgramFile], icon: &str, attach: &mut Option<std::path::PathBuf>, edit: &mut Option<std::path::PathBuf>) {
    if files.is_empty() {
        ui.label(RichText::new("(none)").weak());
    }
    for f in files {
        let label = if f.ex5 { RichText::new(format!("🔒 {}", f.rel)).weak() } else { RichText::new(format!("{} {}", icon, f.rel.trim_end_matches(".mq5"))) };
        let hover = if f.ex5 { "Compiled .ex5 (encrypted) — needs the .mq5 source to run here" } else { "Double-click to attach, right-click for more" };
        let r = ui.selectable_label(false, label).on_hover_text(hover);
        if r.double_clicked() {
            *attach = Some(f.path.clone());
        }
        let p = f.path.clone();
        r.context_menu(|ui| {
            if ui.button("Attach to Chart").clicked() {
                *attach = Some(p.clone());
                ui.close_menu();
            }
            if ui.button("Modify (MetaEditor)").clicked() {
                *edit = Some(p.clone());
                ui.close_menu();
            }
        });
    }
}

fn data_window(app: &mut TerminalApp, ui: &mut egui::Ui) {
    let Some((b, vals)) = &app.hover else {
        ui.label(RichText::new("Move the mouse over a chart").weak());
        return;
    };
    let digits = app.charts.get(app.active).and_then(|c| app.market.symbol_index(&c.chart.symbol)).map(|i| app.market.symbols[i].spec.digits).unwrap_or(5);
    let title = app.charts.get(app.active).map(|c| c.chart.title()).unwrap_or_default();
    ui.label(RichText::new(title).strong());
    egui::Grid::new("dw").striped(true).num_columns(2).show(ui, |ui| {
        let t = fmt_time(b.time, false);
        let rows = vec![
            ("Date".to_string(), t[..10].to_string()),
            ("Time".to_string(), t[11..].to_string()),
            ("Open".to_string(), fmt_price(b.open, digits)),
            ("High".to_string(), fmt_price(b.high, digits)),
            ("Low".to_string(), fmt_price(b.low, digits)),
            ("Close".to_string(), fmt_price(b.close, digits)),
            ("Tick Volume".to_string(), b.tick_volume.to_string()),
            ("Spread".to_string(), b.spread.to_string()),
        ];
        for (k, v) in rows.iter().chain(vals.iter()) {
            ui.label(k);
            ui.label(RichText::new(v).monospace());
            ui.end_row();
        }
    });
}

fn toolbox(app: &mut TerminalApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let errs = app.journal.iter().rev().take(50).any(|l| l.error);
        for (t, name) in [(ToolboxTab::Trade, "Trade"), (ToolboxTab::History, "History"), (ToolboxTab::Journal, "Journal"), (ToolboxTab::Experts, "Experts")] {
            let label = if t == ToolboxTab::Journal && errs { format!("{} ●", name) } else { name.to_string() };
            ui.selectable_value(&mut app.toolbox_tab, t, label);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("✖").on_hover_text("Hide Toolbox (Ctrl+T)").clicked() {
                app.show_toolbox = false;
            }
        });
    });
    ui.separator();
    match app.toolbox_tab {
        ToolboxTab::Trade => trade_tab(app, ui),
        ToolboxTab::History => history_tab(app, ui),
        ToolboxTab::Journal => {
            let lines = app.journal.clone();
            log_list(ui, &lines, "journal");
        }
        ToolboxTab::Experts => {
            let lines = app.experts_log.clone();
            log_list(ui, &lines, "experts");
        }
    }
}

fn trade_tab(app: &mut TerminalApp, ui: &mut egui::Ui) {
    let acc = app.account().clone();
    let margin = acc.margin(&app.market);
    let mut close: Option<u64> = None;
    let mut delete: Option<u64> = None;
    let mut modify: Option<ModifyDlg> = None;
    egui::ScrollArea::both().id_salt("trade").auto_shrink([false, false]).show(ui, |ui| {
        egui::Grid::new("trade_grid").striped(true).num_columns(12).spacing([12.0, 2.0]).show(ui, |ui| {
            for h in ["Symbol", "Ticket", "Time", "Type", "Volume", "Price", "S / L", "T / P", "Price", "Profit", "Comment", ""] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for p in &acc.positions {
                let r = ui.add(egui::Label::new(RichText::new(&p.symbol).strong()).sense(egui::Sense::click()));
                if r.double_clicked() {
                    modify = Some(ModifyDlg { ticket: p.ticket, order: false, sl: p.sl, tp: p.tp, price: p.price_open });
                }
                ui.label(p.ticket.to_string());
                ui.label(fmt_time(p.time, true));
                ui.label(RichText::new(if p.buy { "buy" } else { "sell" }).color(if p.buy { UP } else { DOWN }));
                ui.label(format!("{:.2}", p.volume));
                ui.label(fmt_price(p.price_open, p.digits));
                ui.label(fmt_price(p.sl, p.digits));
                ui.label(fmt_price(p.tp, p.digits));
                ui.label(fmt_price(p.price_current, p.digits));
                ui.label(RichText::new(format!("{:.2}", p.profit)).strong().color(if p.profit >= 0.0 { UP } else { DOWN }));
                ui.label(&p.comment);
                if ui.small_button("✖").on_hover_text("Close position").clicked() {
                    close = Some(p.ticket);
                }
                ui.end_row();
            }
            for o in &acc.orders {
                let r = ui.add(egui::Label::new(RichText::new(&o.symbol).strong()).sense(egui::Sense::click()));
                if r.double_clicked() {
                    modify = Some(ModifyDlg { ticket: o.ticket, order: true, sl: o.sl, tp: o.tp, price: o.price });
                }
                ui.label(o.ticket.to_string());
                ui.label(fmt_time(o.time, true));
                ui.label(o.typ.label());
                ui.label(format!("{:.2}", o.volume));
                ui.label(fmt_price(o.price, o.digits));
                ui.label(fmt_price(o.sl, o.digits));
                ui.label(fmt_price(o.tp, o.digits));
                let cur = app.market.symbol_index(&o.symbol).map(|i| app.market.symbols[i].bid).unwrap_or(0.0);
                ui.label(fmt_price(cur, o.digits));
                ui.label("placed");
                ui.label(&o.comment);
                if ui.small_button("✖").on_hover_text("Delete order").clicked() {
                    delete = Some(o.ticket);
                }
                ui.end_row();
            }
        });
        // account line
        let m = |v: f64| format!("{:.2}", if v.abs() < 0.005 { 0.0 } else { v });
        let eq = acc.equity();
        let level = if margin > 0.0 { format!("   Margin Level: {:.2} %", eq / margin * 100.0) } else { String::new() };
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "Balance: {} {}   Equity: {}   Margin: {}   Free Margin: {}{}",
                    m(acc.balance),
                    acc.currency,
                    m(eq),
                    m(margin),
                    m(eq - margin),
                    level
                ))
                .strong(),
            );
            ui.add_space(30.0);
            let fl = acc.floating();
            ui.label(RichText::new(format!("Floating P/L: {}", m(fl))).strong().color(if fl >= 0.0 { UP } else { DOWN }));
        });
        if acc.positions.is_empty() && acc.orders.is_empty() {
            ui.label(RichText::new("No open positions. Use New Order (F9), the BUY/SELL panel on the chart, or let an expert advisor trade.").weak());
        }
    });
    if let Some(t) = close {
        app.trade_ui(TradeOp::Close { ticket: t, volume: None });
    }
    if let Some(t) = delete {
        app.trade_ui(TradeOp::Delete { ticket: t });
    }
    if modify.is_some() {
        app.dialogs.modify = modify;
    }
}

fn history_tab(app: &mut TerminalApp, ui: &mut egui::Ui) {
    let acc = app.account();
    let deposit: f64 = acc.deals.iter().filter(|d| d.kind == "balance").map(|d| d.profit).sum();
    let profit: f64 = acc.deals.iter().filter(|d| d.kind != "balance").map(|d| d.profit).sum();
    egui::ScrollArea::both().id_salt("hist").auto_shrink([false, false]).show(ui, |ui| {
        egui::Grid::new("hist_grid").striped(true).num_columns(9).spacing([12.0, 2.0]).show(ui, |ui| {
            for h in ["Time", "Deal", "Symbol", "Type", "Direction", "Volume", "Price", "Profit", "Comment"] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for d in acc.deals.iter().rev().take(500) {
                ui.label(fmt_time(d.time, true));
                ui.label(d.ticket.to_string());
                ui.label(&d.symbol);
                ui.label(&d.kind);
                ui.label(&d.entry);
                ui.label(if d.kind == "balance" { String::new() } else { format!("{:.2}", d.volume) });
                ui.label(if d.kind == "balance" { String::new() } else { fmt_price(d.price, d.digits) });
                ui.label(RichText::new(format!("{:.2}", d.profit)).color(if d.profit >= 0.0 { UP } else { DOWN }));
                ui.label(&d.comment);
                ui.end_row();
            }
        });
        ui.label(RichText::new(format!("Profit: {:.2}   Deposit: {:.2}   Balance: {:.2}", profit, deposit, acc.balance)).strong());
    });
}

fn log_list(ui: &mut egui::Ui, lines: &[crate::host::LogLine], id: &str) {
    egui::ScrollArea::vertical().id_salt(id).stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
        egui::Grid::new(format!("{}_grid", id)).striped(true).num_columns(3).spacing([12.0, 1.0]).show(ui, |ui| {
            ui.label(RichText::new("Time").strong());
            ui.label(RichText::new("Source").strong());
            ui.label(RichText::new("Message").strong());
            ui.end_row();
            let start = lines.len().saturating_sub(500);
            for l in &lines[start..] {
                let col = if l.error { DOWN } else { Color32::from_rgb(30, 30, 30) };
                ui.label(RichText::new(fmt_time(l.time, true)).monospace().size(11.0));
                ui.label(RichText::new(&l.source).size(11.5));
                ui.label(RichText::new(&l.text).color(col).size(11.5));
                ui.end_row();
            }
        });
    });
}

fn central(app: &mut TerminalApp, ui: &mut egui::Ui) {
    if app.charts.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(RichText::new("No charts open — File > New Chart, or double-click a symbol in Market Watch").color(Color32::WHITE));
        });
        return;
    }
    app.active = app.active.min(app.charts.len() - 1);
    // chart tabs at the bottom like the original
    let mut close = None;
    egui::TopBottomPanel::bottom("chart_tabs").exact_height(22.0).frame(egui::Frame::NONE.fill(crate::style::PANEL)).show_inside(ui, |ui| {
        ui.horizontal_centered(|ui| {
            for i in 0..app.charts.len() {
                let t = app.charts[i].chart.title();
                let mark = if app.charts[i].expert.is_some() { " ☺" } else { "" };
                if ui.selectable_label(i == app.active, format!("{}{}", t, mark)).clicked() {
                    app.active = i;
                }
                if i == app.active && ui.small_button("✖").on_hover_text("Close chart").clicked() {
                    close = Some(i);
                }
            }
        });
    });
    if let Some(i) = close {
        app.close_chart(i);
        return;
    }
    let ai = app.active;
    let customs = app.custom_views(ai);
    let expert = app.charts[ai].expert.as_ref().map(|e| ExpertBadge { name: e.inst.name.clone(), running: true });
    let algo = app.algo;
    let out = {
        let TerminalApp { market, charts, accounts, cur, .. } = app;
        let slot = &mut charts[ai];
        let sym = slot.chart.symbol.clone();
        let tf = slot.chart.tf;
        let si = market.symbol_index(&sym).unwrap_or(0);
        market.bars(si, tf);
        let bars = market.bars_if_loaded(si, tf).unwrap_or(&[]);
        let s = &market.symbols[si];
        let acc = &accounts[*cur];
        let input = ChartInput {
            bars,
            spec: &s.spec,
            bid: s.bid,
            ask: s.ask,
            positions: acc.positions.iter().filter(|p| p.symbol == sym).collect(),
            orders: acc.orders.iter().filter(|o| o.symbol == sym).collect(),
            theme: Theme::of(slot.chart.scheme),
            customs,
            expert,
            algo,
        };
        egui::Frame::NONE.show(ui, |ui| slot.chart.show(ui, input)).inner
    };
    app.hover = out.hover;
    app.status = out.status.unwrap_or_default();
    let sym = app.charts[ai].chart.symbol.clone();
    for a in out.actions {
        match a {
            ChartAction::Buy(l) => app.trade_ui(TradeOp::Market { symbol: sym.clone(), buy: true, volume: l, sl: 0.0, tp: 0.0, magic: 0, comment: String::new() }),
            ChartAction::Sell(l) => app.trade_ui(TradeOp::Market { symbol: sym.clone(), buy: false, volume: l, sl: 0.0, tp: 0.0, magic: 0, comment: String::new() }),
            ChartAction::NewOrder => app.new_order_dialog(&sym),
            ChartAction::RemoveBuiltin(i) => {
                if i < app.charts[ai].chart.builtins.len() {
                    app.charts[ai].chart.builtins.remove(i);
                }
            }
            ChartAction::EditBuiltin(i) => {
                if let Some(k) = app.charts[ai].chart.builtins.get(i).map(|b| b.kind) {
                    open_ind(app, k, Some(i));
                }
            }
            ChartAction::RemoveCustom(i) => app.remove_custom(ai, i, 1),
            ChartAction::EditCustom(i) => {
                if let Some(p) = app.charts[ai].customs.get(i).map(|c| c.path.clone()) {
                    app.open_attach_dialog(ai, p, AttachTarget::Custom(i));
                }
            }
            ChartAction::RemoveExpert => app.remove_expert(ai, 1),
            ChartAction::EditExpert => {
                if let Some(p) = app.charts[ai].expert.as_ref().map(|e| e.path.clone()) {
                    app.open_attach_dialog(ai, p, AttachTarget::Expert);
                }
            }
            ChartAction::SetTimeframe(tf) => app.set_timeframe(ai, tf),
            ChartAction::Properties => app.dialogs.props = true,
            ChartAction::ObjectsList => app.dialogs.objects = true,
            ChartAction::AddIndicator(k) => open_ind(app, k, None),
        }
    }
}
