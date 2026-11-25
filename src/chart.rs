//! A price chart: bars/candles/line, grid, scales, built-in and custom indicators,
//! sub-windows, trade levels, chart objects, drawing tools and crosshair.

use crate::account::{PendingOrder, Position};
use crate::indicators::{self, bgr, DrawKind, IndKind, IndOutput, IndParams, LineStyle};
use crate::market::{fmt_price, fmt_time, Bar, SymbolSpec, Timeframe};
use crate::mql::interp::PlotRt;
use crate::objects::{ChartObj, ChartObjects, ObjKind, FIBO_LEVELS};
use egui::{pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui};

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum ChartMode {
    Bars,
    Candles,
    Line,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Cursor,
    Crosshair,
    VLine,
    HLine,
    Trend,
    Rectangle,
    Fibo,
    Text,
    ArrowUp,
    ArrowDown,
}

impl Tool {
    pub fn points(self) -> usize {
        match self {
            Tool::Trend | Tool::Rectangle | Tool::Fibo => 2,
            Tool::Cursor | Tool::Crosshair => 0,
            _ => 1,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Scheme {
    GreenOnBlack,
    BlackOnWhite,
    ColorOnWhite,
}

impl Scheme {
    pub fn label(self) -> &'static str {
        match self {
            Scheme::GreenOnBlack => "Green on Black",
            Scheme::BlackOnWhite => "Black on White",
            Scheme::ColorOnWhite => "Color on White",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub bg: Color32,
    pub fg: Color32,
    pub grid: Color32,
    pub bar_up: Color32,
    pub bar_down: Color32,
    pub bull: Color32,
    pub bear: Color32,
    pub line: Color32,
    pub volumes: Color32,
    pub bid: Color32,
    pub ask: Color32,
    pub stops: Color32,
}

impl Theme {
    pub fn of(s: Scheme) -> Theme {
        match s {
            Scheme::GreenOnBlack => Theme {
                bg: Color32::BLACK,
                fg: Color32::WHITE,
                grid: Color32::from_rgb(52, 60, 70),
                bar_up: Color32::from_rgb(0, 255, 0),
                bar_down: Color32::from_rgb(0, 255, 0),
                bull: Color32::BLACK,
                bear: Color32::WHITE,
                line: Color32::from_rgb(0, 255, 0),
                volumes: Color32::from_rgb(50, 205, 50),
                bid: Color32::from_rgb(119, 136, 153),
                ask: Color32::from_rgb(255, 0, 0),
                stops: Color32::from_rgb(255, 0, 0),
            },
            Scheme::BlackOnWhite => Theme {
                bg: Color32::WHITE,
                fg: Color32::BLACK,
                grid: Color32::from_rgb(220, 220, 220),
                bar_up: Color32::BLACK,
                bar_down: Color32::BLACK,
                bull: Color32::WHITE,
                bear: Color32::BLACK,
                line: Color32::BLACK,
                volumes: Color32::from_rgb(0, 128, 0),
                bid: Color32::from_rgb(160, 160, 160),
                ask: Color32::from_rgb(255, 0, 0),
                stops: Color32::from_rgb(255, 0, 0),
            },
            Scheme::ColorOnWhite => Theme {
                bg: Color32::WHITE,
                fg: Color32::from_rgb(40, 40, 40),
                grid: Color32::from_rgb(225, 228, 232),
                bar_up: Color32::from_rgb(38, 166, 91),
                bar_down: Color32::from_rgb(220, 53, 69),
                bull: Color32::from_rgb(38, 166, 91),
                bear: Color32::from_rgb(220, 53, 69),
                line: Color32::from_rgb(30, 110, 230),
                volumes: Color32::from_rgb(38, 166, 91),
                bid: Color32::from_rgb(150, 150, 150),
                ask: Color32::from_rgb(220, 53, 69),
                stops: Color32::from_rgb(220, 53, 69),
            },
        }
    }
}

pub struct BuiltinInd {
    pub kind: IndKind,
    pub params: IndParams,
    cache_key: (usize, i64, u64),
    cache: Option<IndOutput>,
}

impl BuiltinInd {
    pub fn new(kind: IndKind, params: IndParams) -> Self {
        BuiltinInd { kind, params, cache_key: (0, 0, 0), cache: None }
    }
    pub fn invalidate(&mut self) {
        self.cache = None;
    }
    pub fn label(&self) -> String {
        let p = &self.params;
        match self.kind {
            IndKind::MovingAverage => format!("MA({})", p.p1),
            IndKind::Bands => format!("Bands({},{})", p.p1, p.f1),
            IndKind::Envelopes => format!("Env({},{})", p.p1, p.f1),
            IndKind::Ichimoku => format!("Ichimoku({},{},{})", p.p1, p.p2, p.p3),
            IndKind::Sar => format!("SAR({},{})", p.f1, p.f2),
            IndKind::Alligator => format!("Alligator({},{},{})", p.p1, p.p2, p.p3),
            IndKind::Fractals => "Fractals".into(),
            IndKind::Macd => format!("MACD({},{},{})", p.p1, p.p2, p.p3),
            IndKind::Rsi => format!("RSI({})", p.p1),
            IndKind::Stochastic => format!("Stoch({},{},{})", p.p1, p.p2, p.p3),
            IndKind::Atr => format!("ATR({})", p.p1),
            IndKind::Cci => format!("CCI({})", p.p1),
            IndKind::Momentum => format!("Momentum({})", p.p1),
            IndKind::Adx => format!("ADX({})", p.p1),
            IndKind::Wpr => format!("WPR({})", p.p1),
            IndKind::Volumes => "Volumes".into(),
        }
    }
    fn output(&mut self, bars: &[Bar]) -> &IndOutput {
        let key = (bars.len(), bars.last().map(|b| b.time).unwrap_or(0), bars.last().map(|b| b.close.to_bits()).unwrap_or(0));
        if self.cache.is_none() || key != self.cache_key {
            self.cache = Some(indicators::compute(self.kind, &self.params, bars));
            self.cache_key = key;
        }
        self.cache.as_ref().unwrap()
    }
}

/// What the app hands the chart for each attached custom indicator.
pub struct CustomView {
    pub name: String,
    pub separate: bool,
    pub plots: Vec<(PlotRt, Vec<Vec<f64>>)>,
    pub levels: Vec<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub digits: Option<i64>,
    pub error: Option<String>,
}

pub struct ExpertBadge {
    pub name: String,
    pub running: bool,
}

pub struct ChartInput<'a> {
    pub bars: &'a [Bar],
    pub spec: &'a SymbolSpec,
    pub bid: f64,
    pub ask: f64,
    pub positions: Vec<&'a Position>,
    pub orders: Vec<&'a PendingOrder>,
    pub theme: Theme,
    pub customs: Vec<CustomView>,
    pub expert: Option<ExpertBadge>,
    pub algo: bool,
}

#[derive(Clone, Debug)]
pub enum ChartAction {
    Buy(f64),
    Sell(f64),
    NewOrder,
    RemoveBuiltin(usize),
    EditBuiltin(usize),
    RemoveCustom(usize),
    EditCustom(usize),
    RemoveExpert,
    EditExpert,
    SetTimeframe(Timeframe),
    Properties,
    ObjectsList,
    AddIndicator(IndKind),
}

#[derive(Default)]
pub struct ChartOutput {
    pub hover: Option<(Bar, Vec<(String, String)>)>,
    pub actions: Vec<ChartAction>,
    pub status: Option<String>,
}

pub struct Chart {
    pub id: usize,
    pub symbol: String,
    pub tf: Timeframe,
    pub mode: ChartMode,
    pub bar_w: f32,
    pub scroll: f32,
    pub autoscroll: bool,
    pub shift: bool,
    pub grid: bool,
    pub volumes: bool,
    pub show_ask: bool,
    pub period_sep: bool,
    pub one_click: bool,
    pub lots: f64,
    pub builtins: Vec<BuiltinInd>,
    pub objects: ChartObjects,
    pub comment: String,
    pub tool: Tool,
    pending: Vec<(i64, f64)>,
    drag_origin: Option<(f32, f32)>,
    pub scheme: Scheme,
}

const SCALE_W: f32 = 64.0;
const TIME_H: f32 = 18.0;

impl Chart {
    pub fn new(id: usize, symbol: &str, tf: Timeframe) -> Self {
        Chart {
            id,
            symbol: symbol.to_string(),
            tf,
            mode: ChartMode::Candles,
            bar_w: 8.0,
            scroll: 0.0,
            autoscroll: true,
            shift: true,
            grid: true,
            volumes: false,
            show_ask: true,
            period_sep: false,
            one_click: true,
            lots: 0.10,
            builtins: vec![],
            objects: ChartObjects::default(),
            comment: String::new(),
            tool: Tool::Cursor,
            pending: vec![],
            drag_origin: None,
            scheme: Scheme::GreenOnBlack,
        }
    }

    pub fn title(&self) -> String {
        format!("{},{}", self.symbol, self.tf.label())
    }

    pub fn zoom(&mut self, factor: f32) {
        self.bar_w = (self.bar_w * factor).clamp(1.5, 40.0);
    }

    pub fn show(&mut self, ui: &mut Ui, inp: ChartInput) -> ChartOutput {
        let mut out = ChartOutput::default();
        let avail = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(avail, Sense::click_and_drag());
        let painter = ui.painter_at(avail);
        let th = inp.theme;
        painter.rect_filled(avail, 0.0, th.bg);
        let bars = inp.bars;
        let n = bars.len();
        if n == 0 {
            painter.text(avail.center(), Align2::CENTER_CENTER, "Waiting for update", FontId::proportional(14.0), th.fg);
            return out;
        }
        let digits = inp.spec.digits;

        // ----- layout: main window + sub-windows -----
        let plot_right = avail.right() - SCALE_W;
        let plot_bottom = avail.bottom() - TIME_H;
        let sep_builtins: Vec<usize> = (0..self.builtins.len()).filter(|&i| self.builtins[i].kind.separate_window()).collect();
        let sep_customs: Vec<usize> = (0..inp.customs.len()).filter(|&i| inp.customs[i].separate).collect();
        let nsub = sep_builtins.len() + sep_customs.len() + usize::from(self.volumes);
        let total_h = plot_bottom - avail.top();
        let sub_h = if nsub == 0 { 0.0 } else { (total_h * 0.24).min(total_h * 0.6 / nsub as f32).max(50.0) };
        let main_rect = Rect::from_min_max(avail.min, pos2(plot_right, plot_bottom - sub_h * nsub as f32));
        let mut sub_rects = vec![];
        for k in 0..nsub {
            let top = main_rect.bottom() + sub_h * k as f32;
            sub_rects.push(Rect::from_min_max(pos2(avail.left(), top), pos2(plot_right, top + sub_h)));
        }

        // ----- one-click panel geometry (needed before zoom handling) -----
        let header_top = main_rect.top() + 22.0;
        let overlay_count = self.builtins.iter().filter(|b| !b.kind.separate_window()).count() + inp.customs.iter().filter(|c| !c.separate).count();
        let panel_top = header_top + if overlay_count > 0 { 16.0 } else { 0.0 } + 2.0;
        let sell_r = Rect::from_min_size(pos2(main_rect.left() + 6.0, panel_top), vec2(70.0, 40.0));
        let buy_r = Rect::from_min_size(pos2(main_rect.left() + 126.0, panel_top), vec2(70.0, 40.0));
        let lots_r = Rect::from_min_size(pos2(main_rect.left() + 78.0, panel_top + 8.0), vec2(46.0, 24.0));
        let hover_pos = ui.input(|i| i.pointer.hover_pos());
        let over_lots = self.one_click && hover_pos.is_some_and(|p| lots_r.contains(p));

        // ----- input: scroll / zoom / drag -----
        let margin = if self.shift { (plot_right - avail.left()) * 0.12 } else { 4.0 };
        if resp.hovered() {
            let scroll_delta = ui.input(|i| i.raw_scroll_delta);
            if over_lots {
                if scroll_delta.y != 0.0 {
                    let step = if scroll_delta.y > 0.0 { 0.01 } else { -0.01 };
                    self.lots = (((self.lots + step) * 100.0).round() / 100.0).max(0.01);
                }
            } else {
                if scroll_delta.y.abs() > 0.0 {
                    self.zoom(if scroll_delta.y > 0.0 { 1.12 } else { 1.0 / 1.12 });
                }
                if scroll_delta.x.abs() > 0.0 {
                    self.scroll = (self.scroll + scroll_delta.x / self.bar_w).max(-((plot_right - avail.left()) / self.bar_w) * 0.5);
                    self.autoscroll = false;
                }
            }
        }
        let drawing = self.tool.points() > 0;
        if resp.drag_started() && !drawing {
            self.drag_origin = Some((resp.interact_pointer_pos().map(|p| p.x).unwrap_or(0.0), self.scroll));
        }
        if resp.dragged() && !drawing {
            if let (Some((x0, s0)), Some(p)) = (self.drag_origin, resp.interact_pointer_pos()) {
                let max_scroll = n as f32 - 5.0;
                self.scroll = (s0 + (p.x - x0) / self.bar_w).clamp(-((plot_right - avail.left()) / self.bar_w) * 0.5, max_scroll);
                self.autoscroll = false;
            }
        }
        if resp.drag_stopped() {
            self.drag_origin = None;
        }
        if self.autoscroll {
            self.scroll = 0.0;
        }

        let right_idx = n as f32 - 1.0 - self.scroll;
        let bw = self.bar_w;
        let x_of = |i: f32| plot_right - margin - (right_idx - i) * bw - bw * 0.5;
        let idx_of = |x: f32| right_idx - (plot_right - margin - bw * 0.5 - x) / bw;
        let first_vis = (idx_of(avail.left()).floor() as i64).max(0) as usize;
        let last_vis = (idx_of(plot_right).ceil() as i64).clamp(0, n as i64 - 1) as usize;
        let vis = if first_vis <= last_vis { first_vis..=last_vis } else { last_vis..=last_vis };

        // time helpers
        let secs = self.tf.secs();
        let time_of_idx = |i: f32| -> i64 {
            let ii = i.round() as i64;
            if ii < 0 {
                bars[0].time + ii * secs
            } else if ii as usize >= n {
                bars[n - 1].time + (ii - n as i64 + 1) * secs
            } else {
                bars[ii as usize].time
            }
        };
        let idx_of_time = |t: i64| -> f32 {
            if t >= bars[n - 1].time {
                (n - 1) as f32 + ((t - bars[n - 1].time) as f32 / secs as f32)
            } else if t <= bars[0].time {
                (t - bars[0].time) as f32 / secs as f32
            } else {
                match bars.binary_search_by_key(&t, |b| b.time) {
                    Ok(i) => i as f32,
                    Err(i) => i as f32 - 1.0 + ((t - bars[i - 1].time) as f32 / secs as f32).min(1.0),
                }
            }
        };

        // ----- main window price range -----
        let mut lo = f64::MAX;
        let mut hi = f64::MIN;
        for i in vis.clone() {
            lo = lo.min(bars[i].low);
            hi = hi.max(bars[i].high);
        }
        let mut overlays: Vec<IndOutput> = vec![];
        for ind in self.builtins.iter_mut() {
            if !ind.kind.separate_window() {
                let o = ind.output(bars);
                overlays.push(IndOutput { buffers: o.buffers.clone(), lines: o.lines.clone(), levels: vec![], fixed_range: None });
            }
        }
        for o in &overlays {
            for (bi, b) in o.buffers.iter().enumerate() {
                let shift = o.lines.get(bi).map(|l| l.shift).unwrap_or(0);
                for i in vis.clone() {
                    let src = i as i64 - shift;
                    if src >= 0 && (src as usize) < b.len() {
                        let v = b[src as usize];
                        if v.is_finite() && (v - bars[i].close).abs() < bars[i].close * 0.5 {
                            lo = lo.min(v);
                            hi = hi.max(v);
                        }
                    }
                }
            }
        }
        for c in inp.customs.iter().filter(|c| !c.separate) {
            for (_, data) in &c.plots {
                for d in data {
                    for i in vis.clone() {
                        if let Some(v) = d.get(i) {
                            if v.is_finite() && (v - bars[i].close).abs() < bars[i].close * 0.5 {
                                lo = lo.min(*v);
                                hi = hi.max(*v);
                            }
                        }
                    }
                }
            }
        }
        if hi <= lo {
            hi = lo + inp.spec.point() * 10.0;
        }
        let pad = (hi - lo) * 0.08;
        let (lo, hi) = (lo - pad, hi + pad);
        let y_main = |p: f64| main_rect.bottom() - ((p - lo) / (hi - lo)) as f32 * main_rect.height();
        let price_of_y = |y: f32| lo + ((main_rect.bottom() - y) / main_rect.height()) as f64 * (hi - lo);

        // ----- grid & price scale -----
        // Watermark in background (authentic MT5)
        let wm_text = format!("{}, {}", self.symbol, self.tf.label());
        painter.text(main_rect.center(), Align2::CENTER_CENTER, wm_text, FontId::proportional(54.0), th.fg.gamma_multiply(0.06));

        if self.shift {
            let sx = plot_right - margin;
            painter.add(Shape::convex_polygon(vec![pos2(sx, main_rect.top() + 7.0), pos2(sx - 4.5, main_rect.top() + 1.0), pos2(sx + 4.5, main_rect.top() + 1.0)], th.fg.gamma_multiply(0.4), Stroke::NONE));
            painter.add(Shape::convex_polygon(vec![pos2(sx, plot_bottom - 7.0), pos2(sx - 4.5, plot_bottom - 1.0), pos2(sx + 4.5, plot_bottom - 1.0)], th.fg.gamma_multiply(0.4), Stroke::NONE));
        }

        let font = FontId::monospace(11.0);
        let step = nice_step((hi - lo) / (main_rect.height() as f64 / 40.0).max(2.0));
        let mut p = (lo / step).ceil() * step;
        while p < hi {
            let y = y_main(p);
            if self.grid {
                dotted_h(&painter, main_rect.left(), plot_right, y, th.grid);
            }
            painter.text(pos2(plot_right + 4.0, y), Align2::LEFT_CENTER, fmt_price(p, digits), font.clone(), th.fg);
            p += step;
        }
        let label_every = ((90.0 / bw).ceil() as usize).max(1);
        for i in vis.clone() {
            if i % label_every == 0 {
                let x = x_of(i as f32);
                if self.grid {
                    dotted_v(&painter, x, main_rect.top(), plot_bottom, th.grid);
                }
                let t = bars[i].time;
                let label = if secs >= 86400 { fmt_time(t, false)[..10].to_string() } else { fmt_time(t, false)[5..].to_string() };
                painter.text(pos2(x, plot_bottom + 2.0), Align2::CENTER_TOP, label, FontId::monospace(10.5), th.fg);
            }
        }
        if self.period_sep {
            let sep_secs = if secs < 3600 {
                86400
            } else if secs < 86400 {
                604800
            } else {
                2592000
            };
            for i in vis.clone() {
                if i > 0 && bars[i].time / sep_secs != bars[i - 1].time / sep_secs {
                    let x = x_of(i as f32) - bw * 0.5;
                    dashed_v(&painter, x, main_rect.top(), plot_bottom, th.fg.gamma_multiply(0.5));
                }
            }
        }
        painter.line_segment([pos2(plot_right, avail.top()), pos2(plot_right, plot_bottom)], Stroke::new(1.0_f32, th.fg.gamma_multiply(0.6)));
        painter.line_segment([pos2(avail.left(), plot_bottom), pos2(plot_right, plot_bottom)], Stroke::new(1.0_f32, th.fg.gamma_multiply(0.6)));
        for r in &sub_rects {
            painter.line_segment([pos2(r.left(), r.top()), pos2(avail.right(), r.top())], Stroke::new(1.0_f32, th.fg.gamma_multiply(0.6)));
        }

        let main_painter = painter.with_clip_rect(Rect::from_min_max(main_rect.min, pos2(plot_right, main_rect.max.y)));
        // fills behind price
        for c in inp.customs.iter().filter(|c| !c.separate) {
            draw_custom(&main_painter, c, &vis, &x_of, &y_main, bw, true);
        }

        // ----- price -----
        let body_w = (bw * 0.7).max(1.0);
        match self.mode {
            ChartMode::Candles => {
                for i in vis.clone() {
                    let b = &bars[i];
                    let x = x_of(i as f32);
                    let up = b.close >= b.open;
                    let outline = if up { th.bar_up } else { th.bar_down };
                    main_painter.line_segment([pos2(x, y_main(b.high)), pos2(x, y_main(b.low))], Stroke::new(1.0_f32, outline));
                    let (t, bt) = (y_main(b.open.max(b.close)), y_main(b.open.min(b.close)));
                    let r = Rect::from_min_max(pos2(x - body_w / 2.0, t), pos2(x + body_w / 2.0, bt.max(t + 1.0)));
                    main_painter.rect_filled(r, 0.0, if up { th.bull } else { th.bear });
                    if bw > 3.0 {
                        main_painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, outline), egui::StrokeKind::Inside);
                    }
                }
            }
            ChartMode::Bars => {
                for i in vis.clone() {
                    let b = &bars[i];
                    let x = x_of(i as f32);
                    let c = if b.close >= b.open { th.bar_up } else { th.bar_down };
                    let s = Stroke::new(1.0_f32, c);
                    main_painter.line_segment([pos2(x, y_main(b.high)), pos2(x, y_main(b.low))], s);
                    main_painter.line_segment([pos2(x - body_w / 2.0, y_main(b.open)), pos2(x, y_main(b.open))], s);
                    main_painter.line_segment([pos2(x, y_main(b.close)), pos2(x + body_w / 2.0, y_main(b.close))], s);
                }
            }
            ChartMode::Line => {
                let pts: Vec<Pos2> = vis.clone().map(|i| pos2(x_of(i as f32), y_main(bars[i].close))).collect();
                main_painter.add(Shape::line(pts, Stroke::new(1.0_f32, th.line)));
            }
        }

        for o in &overlays {
            draw_lines(&main_painter, o, &vis, &x_of, &y_main, bw, None);
        }
        for c in inp.customs.iter().filter(|c| !c.separate) {
            draw_custom(&main_painter, c, &vis, &x_of, &y_main, bw, false);
        }

        // ----- bid / ask lines -----
        let yb = y_main(inp.bid);
        if yb > main_rect.top() && yb < main_rect.bottom() {
            dotted_h(&painter, main_rect.left(), plot_right, yb, th.bid);
            price_tag(&painter, plot_right, yb, &fmt_price(inp.bid, digits), th.bid, Color32::WHITE);
        }
        if self.show_ask {
            let ya = y_main(inp.ask);
            if ya > main_rect.top() && ya < main_rect.bottom() {
                dotted_h(&painter, main_rect.left(), plot_right, ya, th.ask);
                price_tag(&painter, plot_right, ya, &fmt_price(inp.ask, digits), th.ask, Color32::WHITE);
            }
        }

        // ----- trade levels -----
        let small = FontId::proportional(11.0);
        for p in &inp.positions {
            let y = y_main(p.price_open);
            let col = if p.buy { Color32::from_rgb(30, 144, 255) } else { Color32::from_rgb(255, 80, 80) };
            if y > main_rect.top() && y < main_rect.bottom() {
                dashed_h(&painter, main_rect.left(), plot_right, y, col);
                painter.text(
                    pos2(plot_right - 6.0, y - 2.0),
                    Align2::RIGHT_BOTTOM,
                    format!("#{} {} {:.2}  {:+.2}", p.ticket, if p.buy { "buy" } else { "sell" }, p.volume, p.profit),
                    small.clone(),
                    col,
                );
            }
            for (lvl, name) in [(p.sl, "sl"), (p.tp, "tp")] {
                if lvl > 0.0 {
                    let y = y_main(lvl);
                    if y > main_rect.top() && y < main_rect.bottom() {
                        dashed_h(&painter, main_rect.left(), plot_right, y, th.stops);
                        painter.text(pos2(plot_right - 6.0, y - 2.0), Align2::RIGHT_BOTTOM, format!("#{} {}", p.ticket, name), small.clone(), th.stops);
                    }
                }
            }
        }
        for o in &inp.orders {
            let y = y_main(o.price);
            if y > main_rect.top() && y < main_rect.bottom() {
                dashed_h(&painter, main_rect.left(), plot_right, y, Color32::from_rgb(0, 200, 0));
                painter.text(pos2(plot_right - 6.0, y - 2.0), Align2::RIGHT_BOTTOM, format!("#{} {} {:.2}", o.ticket, o.typ.label(), o.volume), small.clone(), Color32::from_rgb(0, 200, 0));
            }
        }

        // ----- chart objects -----
        for o in &self.objects.list {
            draw_object(&painter, o, main_rect, plot_right, &x_of, &idx_of_time, &y_main, th, digits);
        }

        // ----- sub-windows -----
        let mut sub_k = 0;
        let mut sub_info: Vec<(Rect, f64, f64)> = vec![];
        if self.volumes {
            let r = sub_rects[sub_k];
            sub_k += 1;
            let vmax = vis.clone().map(|i| bars[i].tick_volume).max().unwrap_or(1).max(1) as f64;
            let sp = painter.with_clip_rect(r);
            for i in vis.clone() {
                let x = x_of(i as f32);
                let y = r.bottom() - (bars[i].tick_volume as f64 / vmax) as f32 * (r.height() - 14.0);
                let up = i == 0 || bars[i].tick_volume >= bars[i - 1].tick_volume;
                sp.line_segment([pos2(x, r.bottom()), pos2(x, y)], Stroke::new(body_w.min(3.0), if up { th.volumes } else { Color32::RED }));
            }
            painter.text(r.left_top() + vec2(4.0, 2.0), Align2::LEFT_TOP, format!("Volumes {}", bars[n - 1].tick_volume), small.clone(), th.fg);
            sub_info.push((r, 0.0, vmax));
        }
        for &bi in &sep_builtins {
            let r = sub_rects[sub_k];
            sub_k += 1;
            let ind = &mut self.builtins[bi];
            let label = ind.label();
            let kind = ind.kind;
            let o = ind.output(bars);
            let (mut slo, mut shi) = (f64::MAX, f64::MIN);
            if let Some((a, b)) = o.fixed_range {
                slo = a;
                shi = b;
            } else {
                for b in &o.buffers[..o.lines.len().min(o.buffers.len())] {
                    for i in vis.clone() {
                        if let Some(v) = b.get(i) {
                            if v.is_finite() {
                                slo = slo.min(*v);
                                shi = shi.max(*v);
                            }
                        }
                    }
                }
                for l in &o.levels {
                    slo = slo.min(*l);
                    shi = shi.max(*l);
                }
                if matches!(kind, IndKind::Volumes | IndKind::Macd) {
                    slo = slo.min(0.0);
                }
                if slo == f64::MAX {
                    slo = 0.0;
                    shi = 1.0;
                }
                let pd = (shi - slo).max(1e-9) * 0.08;
                slo -= pd;
                shi += pd;
            }
            if shi <= slo {
                shi = slo + 1.0;
            }
            let yr = |v: f64| r.bottom() - ((v - slo) / (shi - slo)) as f32 * r.height();
            for l in &o.levels {
                dashed_h(&painter, r.left(), r.right(), yr(*l), th.fg.gamma_multiply(0.45));
                painter.text(pos2(plot_right + 4.0, yr(*l)), Align2::LEFT_CENTER, format!("{}", l), font.clone(), th.fg.gamma_multiply(0.7));
            }
            let zero = if slo < 0.0 && shi > 0.0 { Some(yr(0.0)) } else { Some(r.bottom()) };
            let sp = painter.with_clip_rect(r);
            draw_lines(&sp, o, &vis, &x_of, &yr, bw, zero);
            let dec = if matches!(kind, IndKind::Macd | IndKind::Atr) { digits as usize + 1 } else { 2 };
            let vals: Vec<String> = o
                .buffers
                .iter()
                .take(o.lines.len())
                .filter_map(|b| b.get(n - 1).filter(|v| v.is_finite()).map(|v| format!("{:.*}", dec, v)))
                .collect();
            painter.text(r.left_top() + vec2(4.0, 2.0), Align2::LEFT_TOP, format!("{} {}", label, vals.join(" ")), small.clone(), th.fg);
            painter.text(pos2(plot_right + 4.0, r.top() + 6.0), Align2::LEFT_CENTER, format!("{:.*}", dec, shi), font.clone(), th.fg.gamma_multiply(0.7));
            sub_info.push((r, slo, shi));
        }
        for &ci in &sep_customs {
            let r = sub_rects[sub_k];
            sub_k += 1;
            let c = &inp.customs[ci];
            let (mut slo, mut shi) = (f64::MAX, f64::MIN);
            for (_, data) in &c.plots {
                for d in data {
                    for i in vis.clone() {
                        if let Some(v) = d.get(i) {
                            if v.is_finite() {
                                slo = slo.min(*v);
                                shi = shi.max(*v);
                            }
                        }
                    }
                }
            }
            for l in &c.levels {
                slo = slo.min(*l);
                shi = shi.max(*l);
            }
            if c.plots.iter().any(|(p, _)| matches!(p.draw_type, 3 | 12)) && slo != f64::MAX {
                slo = slo.min(0.0);
                shi = shi.max(0.0);
            }
            if slo == f64::MAX {
                slo = 0.0;
                shi = 1.0;
            }
            let pd = (shi - slo).max(1e-9) * 0.08;
            slo -= pd;
            shi += pd;
            if let Some(m) = c.min {
                slo = m;
            }
            if let Some(m) = c.max {
                shi = m;
            }
            if shi <= slo {
                shi = slo + 1.0;
            }
            let yr = |v: f64| r.bottom() - ((v - slo) / (shi - slo)) as f32 * r.height();
            for l in &c.levels {
                dashed_h(&painter, r.left(), r.right(), yr(*l), th.fg.gamma_multiply(0.45));
                painter.text(pos2(plot_right + 4.0, yr(*l)), Align2::LEFT_CENTER, format!("{}", l), font.clone(), th.fg.gamma_multiply(0.7));
            }
            let sp = painter.with_clip_rect(r);
            draw_custom(&sp, c, &vis, &x_of, &yr, bw, true);
            draw_custom(&sp, c, &vis, &x_of, &yr, bw, false);
            let dec = c.digits.unwrap_or(digits as i64).clamp(0, 8) as usize;
            let vals: Vec<String> = c
                .plots
                .iter()
                .filter_map(|(_, d)| d.first().and_then(|b| b.get(n - 1)).filter(|v| v.is_finite()).map(|v| format!("{:.*}", dec, v)))
                .collect();
            let mut title = format!("{} {}", c.name, vals.join(" "));
            if let Some(e) = &c.error {
                title.push_str(&format!("  [{}]", e));
            }
            painter.text(r.left_top() + vec2(4.0, 2.0), Align2::LEFT_TOP, title, small.clone(), th.fg);
            sub_info.push((r, slo, shi));
        }

        // ----- header: symbol, OHLC, one-click panel, comment, expert badge -----
        let active_bar = hover_pos
            .filter(|p| main_rect.contains(*p))
            .and_then(|hp| {
                let idx = idx_of(hp.x).round();
                if idx >= 0.0 && (idx as usize) < n {
                    Some(&bars[idx as usize])
                } else {
                    None
                }
            })
            .unwrap_or(&bars[n - 1]);

        let ohlc_info = format!(
            "{}, {}   O: {}   H: {}   L: {}   C: {}   V: {}",
            self.symbol,
            self.tf.label(),
            fmt_price(active_bar.open, digits),
            fmt_price(active_bar.high, digits),
            fmt_price(active_bar.low, digits),
            fmt_price(active_bar.close, digits),
            active_bar.tick_volume
        );
        painter.text(main_rect.left_top() + vec2(6.0, 4.0), Align2::LEFT_TOP, ohlc_info, FontId::monospace(11.0), th.fg);

        let mut header_y = header_top;
        let mut overlay_labels: Vec<String> = self.builtins.iter().filter(|b| !b.kind.separate_window()).map(|b| b.label()).collect();
        overlay_labels.extend(inp.customs.iter().filter(|c| !c.separate).map(|c| match &c.error {
            Some(e) => format!("{} [{}]", c.name, e),
            None => c.name.clone(),
        }));
        if !overlay_labels.is_empty() {
            painter.text(pos2(main_rect.left() + 6.0, header_y), Align2::LEFT_TOP, overlay_labels.join("   "), small.clone(), th.fg.gamma_multiply(0.75));
            header_y += 16.0;
        }
        if self.one_click {
            // Win32 MT5 styled One-Click Trading widget
            painter.rect_filled(sell_r, 1.0, Color32::from_rgb(215, 30, 30));
            painter.rect_stroke(sell_r, 1.0, Stroke::new(1.0_f32, Color32::from_rgb(160, 20, 20)), egui::StrokeKind::Inside);
            painter.rect_filled(buy_r, 1.0, Color32::from_rgb(20, 100, 220));
            painter.rect_stroke(buy_r, 1.0, Stroke::new(1.0_f32, Color32::from_rgb(10, 70, 160)), egui::StrokeKind::Inside);
            painter.rect_filled(lots_r, 1.0, Color32::from_rgb(245, 245, 248));
            painter.rect_stroke(lots_r, 1.0, Stroke::new(1.0_f32, Color32::from_rgb(180, 180, 185)), egui::StrokeKind::Inside);

            painter.text(sell_r.center_top() + vec2(0.0, 3.0), Align2::CENTER_TOP, "SELL", FontId::proportional(10.0), Color32::WHITE);
            painter.text(buy_r.center_top() + vec2(0.0, 3.0), Align2::CENTER_TOP, "BUY", FontId::proportional(10.0), Color32::WHITE);
            painter.text(sell_r.center_bottom() - vec2(0.0, 3.0), Align2::CENTER_BOTTOM, fmt_price(inp.bid, digits), FontId::monospace(13.0), Color32::WHITE);
            painter.text(buy_r.center_bottom() - vec2(0.0, 3.0), Align2::CENTER_BOTTOM, fmt_price(inp.ask, digits), FontId::monospace(13.0), Color32::WHITE);
            painter.text(lots_r.center(), Align2::CENTER_CENTER, format!("{:.2}", self.lots), FontId::monospace(12.0), Color32::BLACK);

            // lot stepper buttons
            let up_rect = Rect::from_min_size(lots_r.right_top() + vec2(1.0, 0.0), vec2(11.0, 11.0));
            let dn_rect = Rect::from_min_size(lots_r.right_top() + vec2(1.0, 12.0), vec2(11.0, 11.0));
            painter.rect_filled(up_rect, 1.0, Color32::from_rgb(232, 232, 236));
            painter.rect_stroke(up_rect, 1.0, Stroke::new(1.0_f32, Color32::from_rgb(170, 170, 175)), egui::StrokeKind::Inside);
            painter.text(up_rect.center(), Align2::CENTER_CENTER, "▲", FontId::proportional(7.0), Color32::BLACK);

            painter.rect_filled(dn_rect, 1.0, Color32::from_rgb(232, 232, 236));
            painter.rect_stroke(dn_rect, 1.0, Stroke::new(1.0_f32, Color32::from_rgb(170, 170, 175)), egui::StrokeKind::Inside);
            painter.text(dn_rect.center(), Align2::CENTER_CENTER, "▼", FontId::proportional(7.0), Color32::BLACK);

            if resp.clicked() {
                if let Some(pp) = resp.interact_pointer_pos() {
                    if sell_r.contains(pp) {
                        out.actions.push(ChartAction::Sell(self.lots));
                    } else if buy_r.contains(pp) {
                        out.actions.push(ChartAction::Buy(self.lots));
                    } else if up_rect.contains(pp) {
                        self.lots = (((self.lots + 0.01) * 100.0).round() / 100.0).max(0.01);
                    } else if dn_rect.contains(pp) {
                        self.lots = (((self.lots - 0.01) * 100.0).round() / 100.0).max(0.01);
                    }
                }
            }
            header_y += 48.0;
        }
        if !self.comment.is_empty() {
            for (k, line) in self.comment.lines().enumerate() {
                painter.text(pos2(main_rect.left() + 6.0, header_y + k as f32 * 14.0), Align2::LEFT_TOP, line, small.clone(), th.fg);
            }
        }
        if let Some(e) = &inp.expert {
            let col = if inp.algo && e.running { Color32::from_rgb(30, 144, 255) } else { Color32::GRAY };
            let c = pos2(plot_right - 10.0, main_rect.top() + 12.0);
            painter.circle_filled(c, 7.0, col);
            painter.circle_filled(c + vec2(-2.3, -1.8), 1.0, Color32::WHITE);
            painter.circle_filled(c + vec2(2.3, -1.8), 1.0, Color32::WHITE);
            let mouth = if inp.algo && e.running { 1.6 } else { -0.8 };
            painter.line_segment([c + vec2(-3.0, 2.2), c + vec2(0.0, 2.2 + mouth)], Stroke::new(1.0_f32, Color32::WHITE));
            painter.line_segment([c + vec2(0.0, 2.2 + mouth), c + vec2(3.0, 2.2)], Stroke::new(1.0_f32, Color32::WHITE));
            painter.text(c - vec2(11.0, 0.0), Align2::RIGHT_CENTER, &e.name, FontId::proportional(12.0), th.fg);
        }

        // ----- crosshair / hover -----
        let hover = hover_pos.filter(|p| avail.contains(*p) && p.x < plot_right);
        if let Some(hp) = hover {
            let idx = idx_of(hp.x).round();
            if idx >= 0.0 && (idx as usize) < n {
                let ii = idx as usize;
                let b = bars[ii];
                let mut vals = vec![];
                for o in &overlays {
                    for (bi, l) in o.lines.iter().enumerate() {
                        let src = ii as i64 - l.shift;
                        if src >= 0 {
                            if let Some(v) = o.buffers.get(bi).and_then(|bb| bb.get(src as usize)).filter(|v| v.is_finite()) {
                                vals.push((l.label.clone(), fmt_price(*v, digits)));
                            }
                        }
                    }
                }
                for &bi in &sep_builtins {
                    let o = self.builtins[bi].output(bars);
                    for (k, l) in o.lines.iter().enumerate() {
                        if let Some(v) = o.buffers.get(k).and_then(|bb| bb.get(ii)).filter(|v| v.is_finite()) {
                            vals.push((l.label.clone(), format!("{:.4}", v)));
                        }
                    }
                }
                for c in &inp.customs {
                    for (p, d) in &c.plots {
                        if let Some(v) = d.first().and_then(|bb| bb.get(ii)).filter(|v| v.is_finite()) {
                            vals.push((p.label.clone(), format!("{:.5}", v)));
                        }
                    }
                }
                out.hover = Some((b, vals));
            }
            if self.tool == Tool::Crosshair || drawing {
                let cs = Stroke::new(1.0_f32, th.fg.gamma_multiply(0.6));
                painter.line_segment([pos2(avail.left(), hp.y), pos2(plot_right, hp.y)], cs);
                painter.line_segment([pos2(hp.x, avail.top()), pos2(hp.x, plot_bottom)], cs);
                if main_rect.contains(hp) {
                    price_tag(&painter, plot_right, hp.y, &fmt_price(price_of_y(hp.y), digits), th.fg, th.bg);
                } else if let Some((r, slo, shi)) = sub_info.iter().find(|(r, _, _)| r.contains(hp)) {
                    let v = slo + ((r.bottom() - hp.y) / r.height()) as f64 * (shi - slo);
                    price_tag(&painter, plot_right, hp.y, &format!("{:.4}", v), th.fg, th.bg);
                }
                let t = time_of_idx(idx_of(hp.x));
                let galley = painter.layout_no_wrap(fmt_time(t, false), FontId::monospace(10.5), th.bg);
                let tr = Rect::from_center_size(pos2(hp.x, plot_bottom + 9.0), galley.size() + vec2(8.0, 3.0));
                painter.rect_filled(tr, 0.0, th.fg);
                painter.galley(tr.min + vec2(4.0, 1.5), galley, th.bg);
            }
        }

        // ----- drawing tools -----
        if drawing && resp.clicked() {
            if let Some(pp) = resp.interact_pointer_pos() {
                if main_rect.contains(pp) {
                    let t = time_of_idx(idx_of(pp.x));
                    let price = price_of_y(pp.y);
                    self.pending.push((t, price));
                    if self.pending.len() >= self.tool.points() {
                        let kind = match self.tool {
                            Tool::VLine => ObjKind::VLine,
                            Tool::HLine => ObjKind::HLine,
                            Tool::Trend => ObjKind::Trend,
                            Tool::Rectangle => ObjKind::Rectangle,
                            Tool::Fibo => ObjKind::Fibo,
                            Tool::Text => ObjKind::Text,
                            Tool::ArrowUp => ObjKind::ArrowUp,
                            _ => ObjKind::ArrowDown,
                        };
                        let name = self.objects.unique_name(kind.label());
                        let mut o = ChartObj::new(&name, kind);
                        o.color = match kind {
                            ObjKind::Trend | ObjKind::Rectangle => 0xFF901E,
                            ObjKind::HLine | ObjKind::VLine => 0x00D7FF,
                            ObjKind::Text => 0xFFFFFF,
                            _ => o.color,
                        };
                        if kind == ObjKind::Text {
                            o.text = "Text".into();
                        }
                        for (k, (t, pr)) in self.pending.iter().enumerate() {
                            o.times[k] = *t;
                            o.prices[k] = *pr;
                        }
                        self.objects.list.push(o);
                        self.pending.clear();
                        self.tool = Tool::Cursor;
                    }
                }
            }
        }
        if !self.pending.is_empty() {
            if let Some(hp) = hover {
                let (t0, p0) = self.pending[0];
                let a = pos2(x_of(idx_of_time(t0)), y_main(p0));
                painter.line_segment([a, hp], Stroke::new(1.0_f32, Color32::from_rgb(30, 144, 255)));
            }
        }
        if drawing {
            out.status = Some(format!("{}: click on the chart ({} point{})", tool_name(self.tool), self.tool.points(), if self.tool.points() > 1 { "s" } else { "" }));
        }

        // ----- context menu -----
        let builtin_labels: Vec<String> = self.builtins.iter().map(|b| b.label()).collect();
        let custom_labels: Vec<String> = inp.customs.iter().map(|c| c.name.clone()).collect();
        let has_expert = inp.expert.as_ref().map(|e| e.name.clone());
        resp.context_menu(|ui| {
            ui.set_min_width(200.0);
            ui.menu_button("Trading", |ui| {
                if ui.button(format!("Buy {:.2}", self.lots)).clicked() {
                    out.actions.push(ChartAction::Buy(self.lots));
                    ui.close_menu();
                }
                if ui.button(format!("Sell {:.2}", self.lots)).clicked() {
                    out.actions.push(ChartAction::Sell(self.lots));
                    ui.close_menu();
                }
                if ui.button("New Order...   F9").clicked() {
                    out.actions.push(ChartAction::NewOrder);
                    ui.close_menu();
                }
            });
            ui.checkbox(&mut self.one_click, "One Click Trading");
            ui.separator();
            ui.menu_button("Indicators List", |ui| {
                if builtin_labels.is_empty() && custom_labels.is_empty() {
                    ui.label("No indicators on this chart");
                }
                for (i, l) in builtin_labels.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(l);
                        if ui.small_button("Edit").clicked() {
                            out.actions.push(ChartAction::EditBuiltin(i));
                            ui.close_menu();
                        }
                        if ui.small_button("Delete").clicked() {
                            out.actions.push(ChartAction::RemoveBuiltin(i));
                            ui.close_menu();
                        }
                    });
                }
                for (i, l) in custom_labels.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(l);
                        if ui.small_button("Edit").clicked() {
                            out.actions.push(ChartAction::EditCustom(i));
                            ui.close_menu();
                        }
                        if ui.small_button("Delete").clicked() {
                            out.actions.push(ChartAction::RemoveCustom(i));
                            ui.close_menu();
                        }
                    });
                }
            });
            ui.menu_button("Add Indicator", |ui| {
                for k in IndKind::ALL {
                    if ui.button(k.name()).clicked() {
                        out.actions.push(ChartAction::AddIndicator(k));
                        ui.close_menu();
                    }
                }
            });
            if let Some(name) = &has_expert {
                ui.menu_button("Expert Advisor", |ui| {
                    ui.label(name);
                    if ui.button("Properties...").clicked() {
                        out.actions.push(ChartAction::EditExpert);
                        ui.close_menu();
                    }
                    if ui.button("Remove").clicked() {
                        out.actions.push(ChartAction::RemoveExpert);
                        ui.close_menu();
                    }
                });
            }
            if ui.button("Objects List...   Ctrl+B").clicked() {
                out.actions.push(ChartAction::ObjectsList);
                ui.close_menu();
            }
            ui.separator();
            ui.menu_button("Timeframes", |ui| {
                for tf in Timeframe::ALL {
                    if ui.selectable_label(self.tf == tf, tf.label()).clicked() {
                        out.actions.push(ChartAction::SetTimeframe(tf));
                        ui.close_menu();
                    }
                }
            });
            ui.radio_value(&mut self.mode, ChartMode::Bars, "Bar Chart");
            ui.radio_value(&mut self.mode, ChartMode::Candles, "Candlesticks");
            ui.radio_value(&mut self.mode, ChartMode::Line, "Line Chart");
            ui.separator();
            ui.checkbox(&mut self.grid, "Grid");
            ui.checkbox(&mut self.volumes, "Volumes");
            ui.checkbox(&mut self.show_ask, "Ask Line");
            ui.checkbox(&mut self.period_sep, "Period Separators");
            ui.checkbox(&mut self.autoscroll, "Auto Scroll");
            ui.checkbox(&mut self.shift, "Chart Shift");
            if ui.button("Zoom In   +").clicked() {
                self.zoom(1.25);
            }
            if ui.button("Zoom Out   -").clicked() {
                self.zoom(0.8);
            }
            ui.separator();
            if ui.button("Properties...   F8").clicked() {
                out.actions.push(ChartAction::Properties);
                ui.close_menu();
            }
        });
        out
    }
}

fn tool_name(t: Tool) -> &'static str {
    match t {
        Tool::VLine => "Vertical Line",
        Tool::HLine => "Horizontal Line",
        Tool::Trend => "Trend Line",
        Tool::Rectangle => "Rectangle",
        Tool::Fibo => "Fibonacci Retracement",
        Tool::Text => "Text",
        Tool::ArrowUp => "Arrow Up",
        Tool::ArrowDown => "Arrow Down",
        Tool::Crosshair => "Crosshair",
        Tool::Cursor => "Cursor",
    }
}

pub fn nice_step(raw: f64) -> f64 {
    if raw <= 0.0 || !raw.is_finite() {
        return 1.0;
    }
    let mag = 10f64.powf(raw.log10().floor());
    let r = raw / mag;
    let m = if r < 1.5 {
        1.0
    } else if r < 3.5 {
        2.0
    } else if r < 7.5 {
        5.0
    } else {
        10.0
    };
    m * mag
}

fn dotted_h(p: &egui::Painter, x0: f32, x1: f32, y: f32, c: Color32) {
    let mut x = x0;
    while x < x1 {
        p.line_segment([pos2(x, y), pos2((x + 1.0_f32).min(x1), y)], Stroke::new(1.0_f32, c));
        x += 4.0;
    }
}

fn dotted_v(p: &egui::Painter, x: f32, y0: f32, y1: f32, c: Color32) {
    let mut y = y0;
    while y < y1 {
        p.line_segment([pos2(x, y), pos2(x, (y + 1.0_f32).min(y1))], Stroke::new(1.0_f32, c));
        y += 4.0;
    }
}

fn dashed_h(p: &egui::Painter, x0: f32, x1: f32, y: f32, c: Color32) {
    let mut x = x0;
    while x < x1 {
        p.line_segment([pos2(x, y), pos2((x + 3.0).min(x1), y)], Stroke::new(1.0_f32, c));
        x += 6.0;
    }
}

fn dashed_v(p: &egui::Painter, x: f32, y0: f32, y1: f32, c: Color32) {
    let mut y = y0;
    while y < y1 {
        p.line_segment([pos2(x, y), pos2(x, (y + 3.0).min(y1))], Stroke::new(1.0_f32, c));
        y += 6.0;
    }
}

fn styled_line(p: &egui::Painter, a: Pos2, b: Pos2, stroke: Stroke, style: LineStyle) {
    match style {
        LineStyle::Solid => {
            p.line_segment([a, b], stroke);
        }
        _ => {
            let len = a.distance(b);
            if len < 0.5 {
                return;
            }
            let (on, off) = if style == LineStyle::Dash { (6.0, 4.0) } else { (2.0, 3.0) };
            let dir = (b - a) / len;
            let mut d = 0.0;
            while d < len {
                let e = (d + on).min(len);
                p.line_segment([a + dir * d, a + dir * e], stroke);
                d += on + off;
            }
        }
    }
}

fn price_tag(p: &egui::Painter, x: f32, y: f32, text: &str, bg: Color32, fg: Color32) {
    let galley = p.layout_no_wrap(text.to_string(), FontId::monospace(11.0), fg);
    let r = Rect::from_min_size(pos2(x + 1.0, y - galley.size().y / 2.0 - 1.0), galley.size() + vec2(6.0, 2.0));
    p.rect_filled(r, 0.0, bg);
    p.galley(r.min + vec2(3.0, 1.0), galley, fg);
}

fn draw_lines(
    p: &egui::Painter,
    o: &IndOutput,
    vis: &std::ops::RangeInclusive<usize>,
    x_of: &dyn Fn(f32) -> f32,
    y_of: &dyn Fn(f64) -> f32,
    bw: f32,
    zero: Option<f32>,
) {
    for (bi, l) in o.lines.iter().enumerate() {
        let Some(buf) = o.buffers.get(bi) else { continue };
        let stroke = Stroke::new(l.width, l.color);
        let get = |i: usize| -> Option<f64> {
            let src = i as i64 - l.shift;
            if src < 0 {
                return None;
            }
            buf.get(src as usize).copied().filter(|v| v.is_finite())
        };
        let lo = vis.start().saturating_sub(l.shift.unsigned_abs() as usize + 1);
        let hi = *vis.end() + l.shift.unsigned_abs() as usize + 1;
        match l.draw {
            DrawKind::Line => {
                let mut prev: Option<Pos2> = None;
                for i in lo..=hi {
                    match get(i) {
                        Some(v) => {
                            let pt = pos2(x_of(i as f32), y_of(v));
                            if let Some(pp) = prev {
                                styled_line(p, pp, pt, stroke, l.style);
                            }
                            prev = Some(pt);
                        }
                        None => prev = None,
                    }
                }
            }
            DrawKind::Histogram | DrawKind::ColorHistogram => {
                let base = zero.unwrap_or(0.0);
                for i in vis.clone() {
                    if let Some(v) = get(i) {
                        let x = x_of(i as f32);
                        let mut c = l.color;
                        if l.draw == DrawKind::ColorHistogram {
                            if let Some(ci) = o.buffers.get(bi + 1).and_then(|cb| cb.get(i)) {
                                if *ci >= 1.0 {
                                    c = l.color2;
                                }
                            }
                        }
                        p.line_segment([pos2(x, base), pos2(x, y_of(v))], Stroke::new((bw * 0.6).clamp(1.0, 6.0), c));
                    }
                }
            }
            DrawKind::Dots => {
                for i in vis.clone() {
                    if let Some(v) = get(i) {
                        p.circle_filled(pos2(x_of(i as f32), y_of(v)), (bw * 0.18).clamp(1.2, 3.0), l.color);
                    }
                }
            }
            DrawKind::ArrowUp | DrawKind::ArrowDown => {
                for i in vis.clone() {
                    if let Some(v) = get(i) {
                        let x = x_of(i as f32);
                        let y = y_of(v);
                        let s = 4.0;
                        let pts = if l.draw == DrawKind::ArrowUp {
                            vec![pos2(x, y - 10.0), pos2(x - s, y - 4.0), pos2(x + s, y - 4.0)]
                        } else {
                            vec![pos2(x, y + 10.0), pos2(x - s, y + 4.0), pos2(x + s, y + 4.0)]
                        };
                        p.add(Shape::convex_polygon(pts, l.color, Stroke::NONE));
                    }
                }
            }
        }
    }
}

fn plot_color(plot: &PlotRt, data: &[Vec<f64>], color_buf: Option<usize>, i: usize) -> Color32 {
    let idx = color_buf.and_then(|cb| data.get(cb)).and_then(|b| b.get(i)).filter(|v| v.is_finite()).map(|v| *v as usize).unwrap_or(0);
    let c = plot.colors.get(idx).or(plot.colors.first()).copied().unwrap_or(0xFF901E);
    bgr(c)
}

fn style_of(s: i64) -> LineStyle {
    match s {
        1 | 3 | 4 => LineStyle::Dash,
        2 => LineStyle::Dot,
        _ => LineStyle::Solid,
    }
}

/// Draw a custom indicator's plots. `back` draws fills only; otherwise everything else.
fn draw_custom(
    p: &egui::Painter,
    c: &CustomView,
    vis: &std::ops::RangeInclusive<usize>,
    x_of: &dyn Fn(f32) -> f32,
    y_of: &dyn Fn(f64) -> f32,
    bw: f32,
    back: bool,
) {
    for (plot, data) in &c.plots {
        let dt = plot.draw_type;
        let shift = plot.shift;
        let begin = plot.draw_begin.max(0) as usize;
        let v = |b: usize, i: usize| -> Option<f64> {
            if i < begin {
                return None;
            }
            data.get(b).and_then(|d| d.get(i)).copied().filter(|x| x.is_finite())
        };
        let xs = |i: usize| x_of(i as f32 + shift as f32);
        let width = plot.width.max(1) as f32;
        if back != (dt == 7) {
            continue;
        }
        let n = data.first().map(|d| d.len()).unwrap_or(0);
        let start = vis.start().saturating_sub(1);
        let end = (*vis.end()).min(n.saturating_sub(1));
        if n == 0 {
            continue;
        }
        match dt {
            1 | 10 => {
                let cb = if dt == 10 { Some(1) } else { None };
                let mut prev: Option<Pos2> = None;
                for i in start..=end {
                    match v(0, i) {
                        Some(val) => {
                            let pt = pos2(xs(i), y_of(val));
                            if let Some(pp) = prev {
                                styled_line(p, pp, pt, Stroke::new(width, plot_color(plot, data, cb, i)), style_of(plot.style));
                            }
                            prev = Some(pt);
                        }
                        None => prev = None,
                    }
                }
            }
            2 | 11 | 6 | 15 => {
                let cb = match dt {
                    11 => Some(1),
                    15 => Some(2),
                    _ => None,
                };
                // start from the last non-empty point before the visible range
                let mut first = start;
                while first > 0 {
                    let has = if dt == 6 || dt == 15 { v(0, first).or(v(1, first)) } else { v(0, first) };
                    if has.is_some() && first < start {
                        break;
                    }
                    first -= 1;
                }
                let mut prev: Option<Pos2> = None;
                for i in first..=(end + 1).min(n - 1) {
                    let val = if dt == 6 || dt == 15 { v(0, i).or(v(1, i)) } else { v(0, i) };
                    if let Some(val) = val {
                        let pt = pos2(xs(i), y_of(val));
                        if let Some(pp) = prev {
                            p.line_segment([pp, pt], Stroke::new(width, plot_color(plot, data, cb, i)));
                        }
                        prev = Some(pt);
                    }
                }
            }
            3 | 12 => {
                let cb = if dt == 12 { Some(1) } else { None };
                let base = y_of(0.0);
                for i in start..=end {
                    if let Some(val) = v(0, i) {
                        let x = xs(i);
                        p.line_segment([pos2(x, base), pos2(x, y_of(val))], Stroke::new(width.max((bw * 0.5).min(width * 2.0)), plot_color(plot, data, cb, i)));
                    }
                }
            }
            4 | 13 => {
                let cb = if dt == 13 { Some(2) } else { None };
                for i in start..=end {
                    if let (Some(a), Some(b)) = (v(0, i), v(1, i)) {
                        let x = xs(i);
                        p.line_segment([pos2(x, y_of(a)), pos2(x, y_of(b))], Stroke::new(width, plot_color(plot, data, cb, i)));
                    }
                }
            }
            5 | 14 => {
                let cb = if dt == 14 { Some(1) } else { None };
                for i in start..=end {
                    if let Some(val) = v(0, i) {
                        let c = plot_color(plot, data, cb, i);
                        let (x, y) = (xs(i), y_of(val));
                        match plot.arrow {
                            233 | 225 | 241 => {
                                p.add(Shape::convex_polygon(vec![pos2(x, y - 6.0), pos2(x - 4.0, y + 2.0), pos2(x + 4.0, y + 2.0)], c, Stroke::NONE));
                            }
                            234 | 226 | 242 => {
                                p.add(Shape::convex_polygon(vec![pos2(x, y + 6.0), pos2(x - 4.0, y - 2.0), pos2(x + 4.0, y - 2.0)], c, Stroke::NONE));
                            }
                            _ => {
                                p.circle_filled(pos2(x, y), 1.5 + width, c);
                            }
                        }
                    }
                }
            }
            7 => {
                let c0 = bgr(plot.colors.first().copied().unwrap_or(0x904030));
                let c1 = bgr(plot.colors.get(1).copied().unwrap_or(plot.colors.first().copied().unwrap_or(0x304090)));
                for i in start..end {
                    if let (Some(a0), Some(b0), Some(a1), Some(b1)) = (v(0, i), v(1, i), v(0, i + 1), v(1, i + 1)) {
                        let col = if a1 >= b1 { c0 } else { c1 };
                        let pts = vec![pos2(xs(i), y_of(a0)), pos2(xs(i + 1), y_of(a1)), pos2(xs(i + 1), y_of(b1)), pos2(xs(i), y_of(b0))];
                        p.add(Shape::convex_polygon(pts, col.gamma_multiply(0.85), Stroke::NONE));
                    }
                }
            }
            8 | 9 | 16 | 17 => {
                let cb = if dt == 16 || dt == 17 { Some(4) } else { None };
                let body = (bw * 0.7).max(1.0);
                for i in start..=end {
                    if let (Some(o), Some(h), Some(l), Some(cl)) = (v(0, i), v(1, i), v(2, i), v(3, i)) {
                        let x = xs(i);
                        let c = plot_color(plot, data, cb, i);
                        p.line_segment([pos2(x, y_of(h)), pos2(x, y_of(l))], Stroke::new(1.0_f32, c));
                        if dt == 9 || dt == 17 {
                            let r = Rect::from_min_max(pos2(x - body / 2.0, y_of(o.max(cl))), pos2(x + body / 2.0, y_of(o.min(cl)) + 1.0));
                            p.rect_filled(r, 0.0, c);
                        } else {
                            p.line_segment([pos2(x - body / 2.0, y_of(o)), pos2(x, y_of(o))], Stroke::new(1.0_f32, c));
                            p.line_segment([pos2(x, y_of(cl)), pos2(x + body / 2.0, y_of(cl))], Stroke::new(1.0_f32, c));
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_object(
    p: &egui::Painter,
    o: &ChartObj,
    main: Rect,
    plot_right: f32,
    x_of: &dyn Fn(f32) -> f32,
    idx_of_time: &dyn Fn(i64) -> f32,
    y_of: &dyn Fn(f64) -> f32,
    th: Theme,
    digits: u32,
) {
    if o.hidden {
        return;
    }
    let c = bgr(o.color);
    let stroke = Stroke::new(o.width.max(1) as f32, c);
    let style = style_of(o.style);
    let pt = |k: usize| pos2(x_of(idx_of_time(o.times[k])), y_of(o.prices[k]));
    let clip = p.with_clip_rect(Rect::from_min_max(main.min, pos2(plot_right, main.max.y)));
    let font = FontId::proportional(o.font_size.max(6) as f32 + 1.0);
    match o.kind {
        ObjKind::HLine => {
            let y = y_of(o.prices[0]);
            styled_line(&clip, pos2(main.left(), y), pos2(plot_right, y), stroke, style);
            if y > main.top() && y < main.bottom() {
                price_tag(p, plot_right, y, &fmt_price(o.prices[0], digits), c, Color32::BLACK);
                if !o.text.is_empty() {
                    clip.text(pos2(plot_right - 4.0, y - 1.0), Align2::RIGHT_BOTTOM, &o.text, FontId::proportional(10.5), c);
                }
            }
        }
        ObjKind::VLine => {
            let x = x_of(idx_of_time(o.times[0]));
            styled_line(&clip, pos2(x, main.top()), pos2(x, main.bottom()), stroke, style);
        }
        ObjKind::Trend => {
            let (a, mut b) = (pt(0), pt(1));
            if o.ray_right && (b.x - a.x).abs() > 0.1 {
                let k = (b.y - a.y) / (b.x - a.x);
                b = pos2(plot_right, a.y + k * (plot_right - a.x));
            }
            styled_line(&clip, a, b, stroke, style);
        }
        ObjKind::Rectangle => {
            let r = Rect::from_two_pos(pt(0), pt(1));
            clip.rect_filled(r, 0.0, c.gamma_multiply(0.18));
            clip.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
        }
        ObjKind::Fibo => {
            let (a, b) = (pt(0), pt(1));
            let (p0, p1) = (o.prices[0], o.prices[1]);
            for lvl in FIBO_LEVELS {
                let price = p1 + (p0 - p1) * lvl;
                let y = y_of(price);
                styled_line(&clip, pos2(a.x.min(b.x), y), pos2(plot_right, y), Stroke::new(1.0_f32, c), LineStyle::Dot);
                clip.text(pos2(a.x.min(b.x) + 2.0, y - 1.0), Align2::LEFT_BOTTOM, format!("{:.1}  {}", lvl * 100.0, fmt_price(price, digits)), FontId::proportional(10.5), c);
            }
            styled_line(&clip, a, b, Stroke::new(1.0_f32, c), LineStyle::Dash);
        }
        ObjKind::Text => {
            clip.text(pt(0), Align2::LEFT_BOTTOM, &o.text, font, c);
        }
        ObjKind::ArrowUp | ObjKind::ArrowBuy => {
            let q = pt(0);
            let col = if o.kind == ObjKind::ArrowBuy { Color32::from_rgb(30, 144, 255) } else { c };
            clip.add(Shape::convex_polygon(vec![q, q + vec2(-5.0, 9.0), q + vec2(5.0, 9.0)], col, Stroke::NONE));
        }
        ObjKind::ArrowDown | ObjKind::ArrowSell => {
            let q = pt(0);
            let col = if o.kind == ObjKind::ArrowSell { Color32::from_rgb(255, 60, 60) } else { c };
            clip.add(Shape::convex_polygon(vec![q, q + vec2(5.0, -9.0), q + vec2(-5.0, -9.0)], col, Stroke::NONE));
        }
        ObjKind::Label | ObjKind::Button | ObjKind::RectLabel => {
            let (x, y) = (o.xdist as f32, o.ydist as f32);
            let origin = match o.corner {
                1 => pos2(plot_right - x, main.top() + y),
                2 => pos2(main.left() + x, main.bottom() - y),
                3 => pos2(plot_right - x, main.bottom() - y),
                _ => pos2(main.left() + x, main.top() + y),
            };
            if o.kind == ObjKind::Label {
                // the script's default red label colour reads poorly; use the chart text colour then
                let col = if o.color == 0x0000FF { th.fg } else { c };
                p.text(origin, Align2::LEFT_TOP, &o.text, font, col);
            } else {
                let r = Rect::from_min_size(origin, vec2(o.xsize as f32, o.ysize as f32));
                p.rect_filled(r, 2.0, bgr(o.bgcolor));
                p.rect_stroke(r, 2.0, Stroke::new(1.0_f32, c), egui::StrokeKind::Inside);
                p.text(r.center(), Align2::CENTER_CENTER, &o.text, font, c);
            }
        }
    }
}
