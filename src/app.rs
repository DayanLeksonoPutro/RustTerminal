//! The terminal window: state, the tick loop that feeds running programs, and program
//! management (attach, remove, rebuild on timeframe change).

use crate::account::Account;
use crate::chart::{BuiltinInd, Chart, CustomView, Scheme};
use crate::editor::Editor;
use crate::host::{load_program, LiveHost, LogLine, ProgramCache};
use crate::indicators::{IndKind, IndParams};
use crate::market::{unix_now, Market, Timeframe};
use crate::mql::interp::TradeOp;
use crate::mql::{Instance, Program, ProgramKind};
use crate::storage;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

pub struct CustomInd {
    pub inst: Instance,
    pub path: PathBuf,
    pub inputs: HashMap<String, String>,
    pub error: Option<String>,
}

pub struct ExpertRun {
    pub inst: Instance,
    pub path: PathBuf,
    pub inputs: HashMap<String, String>,
    pub last_timer: Instant,
}

pub struct ChartSlot {
    pub chart: Chart,
    pub customs: Vec<CustomInd>,
    pub expert: Option<ExpertRun>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolboxTab {
    Trade,
    History,
    Journal,
    Experts,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LeftTab {
    Navigator,
    DataWindow,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProgEvent {
    Tick,
    Timer,
    Trade,
}

pub struct ScreenshotState {
    pub path: PathBuf,
    pub frames: u64,
    pub requested: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct Workspace {
    pub charts: Vec<SavedChart>,
    pub active: usize,
    pub algo: bool,
    pub scheme: Option<Scheme>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct SavedChart {
    pub symbol: String,
    pub tf: Timeframe,
    pub mode: crate::chart::ChartMode,
    pub builtins: Vec<(IndKind, IndParams)>,
    pub customs: Vec<(String, HashMap<String, String>)>,
    pub expert: Option<(String, HashMap<String, String>)>,
    pub objects: crate::objects::ChartObjects,
    pub volumes: bool,
    pub grid: bool,
}

pub struct TerminalApp {
    pub market: Market,
    pub accounts: Vec<Account>,
    pub cur: usize,
    pub connected: bool,
    pub charts: Vec<ChartSlot>,
    pub active: usize,
    pub next_chart_id: usize,
    pub algo: bool,
    pub scheme: Scheme,
    pub journal: Vec<LogLine>,
    pub experts_log: Vec<LogLine>,
    pub alerts: Vec<LogLine>,
    pub alerts_seen: usize,
    pub gv: HashMap<String, f64>,
    pub cache: ProgramCache,
    pub show_market_watch: bool,
    pub show_navigator: bool,
    pub show_toolbox: bool,
    pub show_tester: bool,
    pub toolbox_tab: ToolboxTab,
    pub left_tab: LeftTab,
    pub hover: Option<(crate::market::Bar, Vec<(String, String)>)>,
    pub status: String,
    pub editor: Editor,
    pub dialogs: crate::dialogs::Dialogs,
    pub tester: crate::tester_ui::TesterUi,
    pub screenshot: Option<ScreenshotState>,
    pub last_tick: Instant,
    pub last_save: Instant,
    pub nav: HashMap<&'static str, Vec<storage::ProgramFile>>,
    pub nav_dirty: bool,
    pub last_deals: usize,
    pub ping_ms: u32,
}

impl TerminalApp {
    pub fn new(cc: &eframe::CreationContext<'_>, screenshot: Option<PathBuf>) -> Self {
        crate::style::apply(&cc.egui_ctx);
        storage::ensure_layout();
        let mut saved = storage::load_accounts();
        if saved.accounts.is_empty() {
            let mut a = Account::new_demo(51_270_384, "Demo Trader", 10_000.0, 100);
            a.password = "demo1234".into();
            saved.accounts.push(a);
            saved.current = Some(51_270_384);
            storage::save_accounts(&saved);
        }
        let cur = saved.current.and_then(|l| saved.accounts.iter().position(|a| a.login == l)).unwrap_or(0);
        let mut app = TerminalApp {
            market: Market::new(),
            accounts: saved.accounts,
            cur,
            connected: true,
            charts: vec![],
            active: 0,
            next_chart_id: 1,
            algo: false,
            scheme: Scheme::GreenOnBlack,
            journal: vec![],
            experts_log: vec![],
            alerts: vec![],
            alerts_seen: 0,
            gv: HashMap::new(),
            cache: HashMap::new(),
            show_market_watch: true,
            show_navigator: true,
            show_toolbox: true,
            show_tester: false,
            toolbox_tab: ToolboxTab::Trade,
            left_tab: LeftTab::Navigator,
            hover: None,
            status: String::new(),
            editor: Editor::default(),
            dialogs: Default::default(),
            tester: Default::default(),
            screenshot: screenshot.map(|path| ScreenshotState { path, frames: 0, requested: false }),
            last_tick: Instant::now(),
            last_save: Instant::now(),
            nav: HashMap::new(),
            nav_dirty: true,
            last_deals: 0,
            ping_ms: 38,
        };
        app.last_deals = app.account().deals.len();
        app.log_journal("Terminal", "Rust Terminal started, data folder ".to_string() + &storage::data_dir().display().to_string());
        let login = app.account().login;
        let server = app.account().server.clone();
        app.log_journal("Network", format!("'{}': authorized on {}", login, server));
        app.log_journal("Network", format!("'{}': trading has been enabled - hedging mode", login));
        let ws = if app.screenshot.is_some() { None } else { load_workspace() };
        match ws {
            Some(ws) if !ws.charts.is_empty() => app.restore_workspace(ws),
            _ => app.default_workspace(),
        }
        app
    }

    pub fn account(&self) -> &Account {
        &self.accounts[self.cur]
    }

    pub fn log_journal(&mut self, source: &str, text: String) {
        self.journal.push(LogLine { time: unix_now(), source: source.to_string(), text, error: false });
    }
    pub fn log_error(&mut self, source: &str, text: String) {
        self.journal.push(LogLine { time: unix_now(), source: source.to_string(), text, error: true });
    }

    fn default_workspace(&mut self) {
        let ind = |f: &str| storage::mql_dir().join("Indicators").join(f);
        let i = self.open_chart("EURUSD", Timeframe::H1);
        self.charts[i].chart.builtins.push(BuiltinInd::new(IndKind::MovingAverage, IndParams::defaults(IndKind::MovingAverage)));
        self.attach_program(i, &ind("Smoothed Price Line.mq5"), HashMap::new(), true);
        self.attach_program(i, &ind("RSI Color Histogram.mq5"), HashMap::new(), true);
        self.attach_program(i, &storage::mql_dir().join("Experts").join("MA Crossover EA.mq5"), HashMap::new(), true);
        let j = self.open_chart("GBPUSD", Timeframe::H4);
        self.charts[j].chart.builtins.push(BuiltinInd::new(IndKind::Bands, IndParams::defaults(IndKind::Bands)));
        self.charts[j].chart.builtins.push(BuiltinInd::new(IndKind::Stochastic, IndParams::defaults(IndKind::Stochastic)));
        let k = self.open_chart("XAUUSD", Timeframe::M15);
        self.charts[k].chart.builtins.push(BuiltinInd::new(IndKind::Ichimoku, IndParams::defaults(IndKind::Ichimoku)));
        let l = self.open_chart("USDJPY", Timeframe::D1);
        self.attach_program(l, &ind("Price Channel.mq5"), HashMap::new(), true);
        self.charts[l].chart.volumes = true;
        self.active = 0;
    }

    pub fn workspace(&self) -> Workspace {
        let rel = |p: &Path| p.strip_prefix(storage::mql_dir()).unwrap_or(p).to_string_lossy().replace('\\', "/");
        Workspace {
            charts: self
                .charts
                .iter()
                .map(|s| SavedChart {
                    symbol: s.chart.symbol.clone(),
                    tf: s.chart.tf,
                    mode: s.chart.mode,
                    builtins: s.chart.builtins.iter().map(|b| (b.kind, b.params)).collect(),
                    customs: s.customs.iter().map(|c| (rel(&c.path), c.inputs.clone())).collect(),
                    expert: s.expert.as_ref().map(|e| (rel(&e.path), e.inputs.clone())),
                    objects: crate::objects::ChartObjects { list: s.chart.objects.list.iter().filter(|o| !o.from_program).cloned().collect() },
                    volumes: s.chart.volumes,
                    grid: s.chart.grid,
                })
                .collect(),
            active: self.active,
            algo: self.algo,
            scheme: Some(self.scheme),
        }
    }

    fn restore_workspace(&mut self, ws: Workspace) {
        self.scheme = ws.scheme.unwrap_or(Scheme::GreenOnBlack);
        for c in ws.charts {
            if self.market.symbol_index(&c.symbol).is_none() {
                continue;
            }
            let i = self.open_chart(&c.symbol, c.tf);
            {
                let ch = &mut self.charts[i].chart;
                ch.mode = c.mode;
                ch.volumes = c.volumes;
                ch.grid = c.grid;
                ch.objects = c.objects;
                for (k, p) in c.builtins {
                    ch.builtins.push(BuiltinInd::new(k, p));
                }
            }
            for (rel, inputs) in c.customs {
                self.attach_program(i, &storage::mql_dir().join(rel), inputs, true);
            }
            if let Some((rel, inputs)) = c.expert {
                self.attach_program(i, &storage::mql_dir().join(rel), inputs, true);
            }
        }
        self.algo = ws.algo;
        self.active = ws.active.min(self.charts.len().saturating_sub(1));
    }

    pub fn save_all(&mut self) {
        let saved = storage::Saved { accounts: self.accounts.clone(), current: Some(self.account().login) };
        storage::save_accounts(&saved);
        if self.screenshot.is_none() {
            if let Ok(j) = serde_json::to_string_pretty(&self.workspace()) {
                let _ = std::fs::write(storage::data_dir().join("workspace.json"), j);
            }
        }
    }

    pub fn open_chart(&mut self, symbol: &str, tf: Timeframe) -> usize {
        let mut c = Chart::new(self.next_chart_id, symbol, tf);
        c.scheme = self.scheme;
        self.next_chart_id += 1;
        if let Some(i) = self.market.symbol_index(symbol) {
            self.market.bars(i, tf);
        }
        self.charts.push(ChartSlot { chart: c, customs: vec![], expert: None });
        self.active = self.charts.len() - 1;
        self.active
    }

    pub fn close_chart(&mut self, i: usize) {
        if i >= self.charts.len() {
            return;
        }
        self.remove_expert(i, 4);
        while !self.charts[i].customs.is_empty() {
            self.remove_custom(i, 0, 4);
        }
        self.charts.remove(i);
        if self.active >= self.charts.len() {
            self.active = self.charts.len().saturating_sub(1);
        }
    }

    /// Compile and attach a program file to a chart: indicator, expert, or run a script.
    pub fn attach_program(&mut self, ci: usize, path: &Path, inputs: HashMap<String, String>, quiet: bool) -> bool {
        let prog = match load_program(&mut self.cache, path) {
            Ok(p) => p,
            Err(e) => {
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if !quiet || path.exists() {
                    self.log_error("Experts", format!("cannot load '{}' [{}]", name, e));
                    if !quiet {
                        self.dialogs.message = Some(("Cannot load program".into(), format!("{}\n\n{}", name, e)));
                    }
                }
                return false;
            }
        };
        self.attach_compiled(ci, path, prog, inputs)
    }

    pub fn attach_compiled(&mut self, ci: usize, path: &Path, prog: Rc<Program>, inputs: HashMap<String, String>) -> bool {
        if ci >= self.charts.len() {
            return false;
        }
        let (symbol, tf) = (self.charts[ci].chart.symbol.clone(), self.charts[ci].chart.tf);
        let mut inst = Instance::new(prog.clone(), &symbol, tf, &inputs);
        let source = format!("{} ({},{})", inst.name, symbol, tf.label());
        let kind = prog.kind;
        let init = self.with_host(ci, &source, |h| inst.init(h));
        match init {
            Ok(0) => {}
            Ok(code) => {
                self.log_error("Experts", format!("{}: initialization failed (OnInit returned non-zero code {})", source, code));
                return false;
            }
            Err(e) => {
                let msg = inst.fmt_err(&e);
                self.log_error("Experts", format!("{}: critical runtime error in OnInit function ({})", source, msg));
                return false;
            }
        }
        match kind {
            ProgramKind::Indicator => {
                let r = self.with_host(ci, &source, |h| inst.calculate(h));
                let error = r.err().map(|e| inst.fmt_err(&e));
                self.log_journal("Experts", format!("custom indicator {} loaded successfully", source));
                self.charts[ci].customs.push(CustomInd { inst, path: path.to_path_buf(), inputs, error });
            }
            ProgramKind::Expert => {
                if self.charts[ci].expert.is_some() {
                    self.remove_expert(ci, 1);
                }
                self.log_journal("Experts", format!("expert {} loaded successfully", source));
                self.charts[ci].expert = Some(ExpertRun { inst, path: path.to_path_buf(), inputs, last_timer: Instant::now() });
            }
            ProgramKind::Script => {
                self.log_journal("Experts", format!("script {} loaded successfully", source));
                let r = self.with_host(ci, &source, |h| {
                    let r = inst.call_event("OnStart", vec![], h);
                    let _ = inst.deinit(0, h);
                    r
                });
                if let Err(e) = r {
                    let msg = inst.fmt_err(&e);
                    self.log_error("Experts", format!("{}: critical runtime error in OnStart function ({})", source, msg));
                }
                self.log_journal("Experts", format!("script {} removed", source));
                self.after_trades();
            }
            ProgramKind::Unknown => return false,
        }
        true
    }

    /// Run `f` with a live host for chart `ci`.
    pub fn with_host<R>(&mut self, ci: usize, source: &str, f: impl FnOnce(&mut LiveHost) -> R) -> R {
        let TerminalApp { market, accounts, cur, charts, journal, experts_log, alerts, gv, cache, algo, .. } = self;
        let slot = &mut charts[ci];
        let mut host = LiveHost {
            market,
            account: &mut accounts[*cur],
            objects: &mut slot.chart.objects,
            comment: &mut slot.chart.comment,
            journal,
            experts: experts_log,
            alerts,
            gv,
            cache,
            algo: *algo,
            source: source.to_string(),
        };
        f(&mut host)
    }

    pub fn remove_expert(&mut self, ci: usize, reason: i64) {
        if let Some(mut e) = self.charts[ci].expert.take() {
            let source = format!("{} ({},{})", e.inst.name, e.inst.symbol, e.inst.tf.label());
            let _ = self.with_host(ci, &source, |h| e.inst.deinit(reason, h));
            self.charts[ci].chart.objects.list.retain(|o| !o.from_program);
            self.charts[ci].chart.comment.clear();
            self.log_journal("Experts", format!("expert {} removed", source));
        }
    }

    pub fn remove_custom(&mut self, ci: usize, k: usize, reason: i64) {
        if k >= self.charts[ci].customs.len() {
            return;
        }
        let mut c = self.charts[ci].customs.remove(k);
        let source = format!("{} ({},{})", c.inst.name, c.inst.symbol, c.inst.tf.label());
        let _ = self.with_host(ci, &source, |h| c.inst.deinit(reason, h));
        self.log_journal("Experts", format!("custom indicator {} removed", source));
    }

    /// Re-create the programs of a chart after its symbol or timeframe changed.
    pub fn rebuild_programs(&mut self, ci: usize) {
        let customs: Vec<(PathBuf, HashMap<String, String>)> = self.charts[ci].customs.iter().map(|c| (c.path.clone(), c.inputs.clone())).collect();
        let expert = self.charts[ci].expert.as_ref().map(|e| (e.path.clone(), e.inputs.clone()));
        self.remove_expert(ci, 3);
        while !self.charts[ci].customs.is_empty() {
            self.remove_custom(ci, 0, 3);
        }
        for (p, i) in customs {
            self.attach_program(ci, &p, i, true);
        }
        if let Some((p, i)) = expert {
            self.attach_program(ci, &p, i, true);
        }
    }

    pub fn set_timeframe(&mut self, ci: usize, tf: Timeframe) {
        if self.charts[ci].chart.tf == tf {
            return;
        }
        self.charts[ci].chart.tf = tf;
        for b in self.charts[ci].chart.builtins.iter_mut() {
            b.invalidate();
        }
        if let Some(i) = self.market.symbol_index(&self.charts[ci].chart.symbol.clone()) {
            self.market.bars(i, tf);
        }
        self.rebuild_programs(ci);
    }

    /// Run program events of one chart.
    pub fn run_chart(&mut self, ci: usize, ev: ProgEvent) {
        let mut customs = std::mem::take(&mut self.charts[ci].customs);
        let mut expert = self.charts[ci].expert.take();
        let mut logs: Vec<(bool, String)> = vec![];
        let mut remove_expert = false;
        if ev == ProgEvent::Tick {
            for c in customs.iter_mut().filter(|c| c.error.is_none()) {
                let source = format!("{} ({},{})", c.inst.name, c.inst.symbol, c.inst.tf.label());
                if let Err(e) = self.with_host(ci, &source, |h| c.inst.calculate(h)) {
                    let msg = c.inst.fmt_err(&e);
                    logs.push((true, format!("{}: critical runtime error in OnCalculate function ({})", source, msg)));
                    c.error = Some(msg);
                }
            }
        }
        if let Some(e) = expert.as_mut() {
            let source = format!("{} ({},{})", e.inst.name, e.inst.symbol, e.inst.tf.label());
            let name = match ev {
                ProgEvent::Tick => Some("OnTick"),
                ProgEvent::Trade => Some("OnTrade"),
                ProgEvent::Timer => {
                    if e.inst.timer_ms > 0 && e.last_timer.elapsed() >= Duration::from_millis(e.inst.timer_ms) {
                        e.last_timer = Instant::now();
                        Some("OnTimer")
                    } else {
                        None
                    }
                }
            };
            if let Some(name) = name {
                let r = self.with_host(ci, &source, |h| e.inst.call_event(name, vec![], h));
                if let Err(err) = r {
                    let msg = e.inst.fmt_err(&err);
                    logs.push((true, format!("{}: critical runtime error in {} function ({})", source, name, msg)));
                    remove_expert = true;
                } else if e.inst.removed {
                    logs.push((false, format!("{}: expert removed itself", source)));
                    remove_expert = true;
                }
            }
        }
        self.charts[ci].customs = customs;
        self.charts[ci].expert = expert;
        for (err, l) in logs {
            if err {
                self.log_error("Experts", l);
            } else {
                self.log_journal("Experts", l);
            }
        }
        if remove_expert {
            self.remove_expert(ci, 0);
        }
    }

    /// Advance the market, process stops and feed the programs.
    pub fn tick(&mut self) {
        let interval = if self.screenshot.is_some() { 40 } else { 250 };
        if self.last_tick.elapsed() < Duration::from_millis(interval) {
            return;
        }
        self.last_tick = Instant::now();
        let ticked = self.market.update();
        if self.connected {
            let mut log = vec![];
            let now = self.market.now;
            let TerminalApp { market, accounts, cur, .. } = self;
            accounts[*cur].process(&*market, now, &mut log);
            for l in log {
                self.log_journal("Trades", l);
            }
        }
        for ci in 0..self.charts.len() {
            let sym = self.charts[ci].chart.symbol.clone();
            let Some(si) = self.market.symbol_index(&sym) else { continue };
            if ticked.contains(&si) {
                self.run_chart(ci, ProgEvent::Tick);
            }
            self.run_chart(ci, ProgEvent::Timer);
        }
        self.after_trades();
        if self.last_save.elapsed() > Duration::from_secs(10) {
            self.save_all();
            self.last_save = Instant::now();
        }
        self.ping_ms = 30 + (self.market.now % 17) as u32;
    }

    /// Fire OnTrade when deals were made.
    pub fn after_trades(&mut self) {
        let n = self.account().deals.len();
        if n != self.last_deals {
            self.last_deals = n;
            for ci in 0..self.charts.len() {
                if self.charts[ci].expert.is_some() {
                    self.run_chart(ci, ProgEvent::Trade);
                }
            }
        }
    }

    /// A trade the person asked for from the window.
    pub fn trade_ui(&mut self, op: TradeOp) {
        if !self.connected {
            self.dialogs.message = Some(("No connection".into(), "Log in to an account first (File > Login to Trade Account).".into()));
            return;
        }
        let mut log = vec![];
        let r = {
            let TerminalApp { market, accounts, cur, .. } = self;
            crate::host::apply_trade(market, &mut accounts[*cur], op, &mut log)
        };
        for l in log {
            self.log_journal("Trades", l);
        }
        if r.retcode != crate::account::RET_DONE {
            self.dialogs.message = Some(("Order failed".into(), format!("{} (code {})", r.comment, r.retcode)));
        }
        self.after_trades();
    }

    pub fn custom_views(&self, ci: usize) -> Vec<CustomView> {
        self.charts[ci]
            .customs
            .iter()
            .map(|c| {
                let i = &c.inst;
                CustomView {
                    name: i.short_name.clone(),
                    separate: i.prog.props.separate_window && !i.prog.props.chart_window,
                    plots: i.plot_data(),
                    levels: i.levels.clone(),
                    min: i.minimum,
                    max: i.maximum,
                    digits: i.digits,
                    error: c.error.clone(),
                }
            })
            .collect()
    }

    pub fn login(&mut self, login: u64, password: &str) -> Result<(), String> {
        let idx = self.accounts.iter().position(|a| a.login == login).ok_or_else(|| "Invalid account".to_string())?;
        if !self.accounts[idx].password.is_empty() && self.accounts[idx].password != password {
            return Err("Invalid account".into());
        }
        self.cur = idx;
        self.connected = true;
        self.last_deals = self.account().deals.len();
        let server = self.account().server.clone();
        self.log_journal("Network", format!("'{}': authorized on {}", login, server));
        self.log_journal("Network", format!("'{}': trading has been enabled - hedging mode", login));
        // experts see the account change
        for ci in 0..self.charts.len() {
            if self.charts[ci].expert.is_some() {
                self.rebuild_programs(ci);
            }
        }
        self.save_all();
        Ok(())
    }

    pub fn nav_list(&mut self, folder: &'static str) -> Vec<storage::ProgramFile> {
        if self.nav_dirty {
            self.nav.clear();
            self.nav_dirty = false;
        }
        self.nav.entry(folder).or_insert_with(|| storage::list_programs(folder)).clone()
    }
}

fn load_workspace() -> Option<Workspace> {
    std::fs::read_to_string(storage::data_dir().join("workspace.json")).ok().and_then(|s| serde_json::from_str(&s).ok())
}

impl eframe::App for TerminalApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick();
        crate::ui::draw(self, ctx);
        // screenshot mode
        if let Some(s) = &mut self.screenshot {
            s.frames += 1;
            let mut shot = None;
            ctx.input(|i| {
                for e in &i.raw.events {
                    if let egui::Event::Screenshot { image, .. } = e {
                        shot = Some(image.clone());
                    }
                }
            });
            if let Some(img) = shot {
                let [w, h] = img.size;
                let mut bytes = Vec::with_capacity(w * h * 4);
                for p in &img.pixels {
                    bytes.extend_from_slice(&[p.r(), p.g(), p.b(), 255]);
                }
                if let Some(dir) = s.path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                match image::save_buffer(&s.path, &bytes, w as u32, h as u32, image::ColorType::Rgba8) {
                    Ok(()) => println!("screenshot saved to {}", s.path.display()),
                    Err(e) => eprintln!("failed to save screenshot: {}", e),
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if s.frames >= 120 && !s.requested {
                s.requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_all();
    }
}
