//! Headless chart rendering to PNG for the MCP server: candles, indicator lines on the
//! price chart, one pane per separate-window indicator, price/time axis labels.

use crate::market::{civil, Bar, SymbolSpec, Timeframe};
use crate::mcp::IndOut;
use image::{Rgba, RgbaImage};

const BG: Rgba<u8> = Rgba([255, 255, 255, 255]);
const GRID: Rgba<u8> = Rgba([230, 232, 236, 255]);
const AXIS: Rgba<u8> = Rgba([150, 150, 155, 255]);
const TEXT: Rgba<u8> = Rgba([60, 60, 66, 255]);
const UP: Rgba<u8> = Rgba([0, 140, 90, 255]);
const DOWN: Rgba<u8> = Rgba([215, 40, 40, 255]);
const LAST: Rgba<u8> = Rgba([30, 90, 200, 255]);

const AXIS_W: u32 = 84;
const BOTTOM_H: u32 = 22;
const PANE_GAP: u32 = 6;

struct Pane {
    top: f64,
    h: f64,
    lo: f64,
    hi: f64,
}

impl Pane {
    fn y(&self, v: f64) -> f64 {
        self.top + (self.hi - v) / (self.hi - self.lo).max(1e-12) * self.h
    }
}

fn rgba(rgb: u32) -> Rgba<u8> {
    Rgba([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255])
}

fn ok(v: f64) -> bool {
    v.is_finite() && v.abs() < 1e300
}

pub fn render(
    spec: &SymbolSpec,
    tf: Timeframe,
    bars: &[Bar],
    inds: &[IndOut],
    w: u32,
    h: u32,
) -> Result<Vec<u8>, String> {
    let n = bars.len();
    if n == 0 {
        return Err("no bars to draw".into());
    }
    let mut img = RgbaImage::from_pixel(w, h, BG);
    let plot_w = (w - AXIS_W) as f64;
    let slot = plot_w / n as f64;
    let x = |i: usize| (i as f64 + 0.5) * slot;
    let off = |data: &Vec<f64>| data.len().saturating_sub(n);

    let subs: Vec<&IndOut> = inds.iter().filter(|i| i.separate).take(3).collect();
    let sub_h = if subs.is_empty() {
        0.0
    } else {
        ((h as f64 - BOTTOM_H as f64) * 0.22).max(70.0)
    };
    let main_h = h as f64 - BOTTOM_H as f64 - subs.len() as f64 * (sub_h + PANE_GAP as f64) - 8.0;

    // price pane range: bars plus overlay indicators
    let mut lo = bars.iter().map(|b| b.low).fold(f64::MAX, f64::min);
    let mut hi = bars.iter().map(|b| b.high).fold(f64::MIN, f64::max);
    for ind in inds.iter().filter(|i| !i.separate) {
        for p in &ind.plots {
            if let Some(d) = p.data.first() {
                for v in &d[off(d)..] {
                    if ok(*v) {
                        lo = lo.min(*v);
                        hi = hi.max(*v);
                    }
                }
            }
        }
    }
    let pad = (hi - lo).max(spec.point() * 10.0) * 0.05;
    let main = Pane {
        top: 8.0,
        h: main_h,
        lo: lo - pad,
        hi: hi + pad,
    };
    grid(&mut img, &main, plot_w, spec.digits as usize);

    // candles
    let body_w = (slot * 0.7).max(1.0);
    for (i, b) in bars.iter().enumerate() {
        let c = if b.close >= b.open { UP } else { DOWN };
        let cx = x(i);
        vline(&mut img, cx, main.y(b.high), main.y(b.low), c);
        let (y1, y2) = (main.y(b.open.max(b.close)), main.y(b.open.min(b.close)));
        fill_rect(
            &mut img,
            cx - body_w / 2.0,
            y1,
            body_w,
            (y2 - y1).max(1.0),
            c,
        );
    }
    for ind in inds.iter().filter(|i| !i.separate) {
        draw_plots(&mut img, ind, &main, n, slot);
    }
    // last price
    let last = bars[n - 1].close;
    let ly = main.y(last);
    dashed_hline(&mut img, 0.0, plot_w, ly, LAST);
    fill_rect(
        &mut img,
        plot_w + 1.0,
        ly - 8.0,
        AXIS_W as f64 - 2.0,
        16.0,
        LAST,
    );
    text(
        &mut img,
        plot_w + 6.0,
        ly - 5.0,
        &format!("{:.*}", spec.digits as usize, last),
        Rgba([255, 255, 255, 255]),
    );

    // separate panes
    let mut top = main.top + main.h + PANE_GAP as f64;
    for ind in subs {
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for p in &ind.plots {
            for d in p.data.iter().take(if matches!(p.draw_type, 10..=17) {
                p.data.len() - 1
            } else {
                p.data.len()
            }) {
                for v in &d[off(d)..] {
                    if ok(*v) {
                        lo = lo.min(*v);
                        hi = hi.max(*v);
                    }
                }
            }
        }
        for l in &ind.levels {
            lo = lo.min(*l);
            hi = hi.max(*l);
        }
        if ind.plots.iter().any(|p| matches!(p.draw_type, 3 | 12)) {
            lo = lo.min(0.0);
            hi = hi.max(0.0);
        }
        if lo > hi {
            lo = 0.0;
            hi = 1.0;
        }
        let pad = (hi - lo).max(1e-9) * 0.08;
        let pane = Pane {
            top,
            h: sub_h,
            lo: lo - pad,
            hi: hi + pad,
        };
        hline(&mut img, 0.0, w as f64, top - PANE_GAP as f64 / 2.0, AXIS);
        let dg = if (hi - lo) > 100.0 {
            0
        } else if (hi - lo) > 1.0 {
            2
        } else {
            spec.digits as usize + 1
        };
        grid(&mut img, &pane, plot_w, dg);
        for l in &ind.levels {
            dashed_hline(&mut img, 0.0, plot_w, pane.y(*l), AXIS);
        }
        draw_plots(&mut img, ind, &pane, n, slot);
        top += sub_h + PANE_GAP as f64;
    }

    // axes and time labels
    vline(&mut img, plot_w, 0.0, h as f64 - BOTTOM_H as f64, AXIS);
    let by = h as f64 - BOTTOM_H as f64;
    hline(&mut img, 0.0, w as f64, by, AXIS);
    let labels = ((plot_w / 150.0) as usize).max(2);
    for k in 0..labels {
        let i = (k * (n - 1)) / (labels - 1).max(1);
        let (_, mo, d, hh, mi, _) = civil(bars[i].time);
        let s = if tf.secs() >= 86400 {
            format!("{:02}.{:02}", mo, d)
        } else {
            format!("{:02}.{:02} {:02}:{:02}", mo, d, hh, mi)
        };
        let tx = (x(i) - text_w(&s) / 2.0).clamp(2.0, plot_w - text_w(&s) - 2.0);
        vline(&mut img, x(i), by, by + 4.0, AXIS);
        text(&mut img, tx, by + 7.0, &s, TEXT);
    }

    let mut out = std::io::Cursor::new(vec![]);
    img.write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

fn grid(img: &mut RgbaImage, p: &Pane, plot_w: f64, digits: usize) {
    for k in 0..=4 {
        let v = p.lo + (p.hi - p.lo) * (k as f64 + 0.5) / 5.0;
        let y = p.y(v);
        hline(img, 0.0, plot_w, y, GRID);
        text(
            img,
            plot_w + 6.0,
            y - 5.0,
            &format!("{:.*}", digits, v),
            TEXT,
        );
    }
}

fn draw_plots(img: &mut RgbaImage, ind: &IndOut, p: &Pane, n: usize, slot: f64) {
    let x = |i: usize| (i as f64 + 0.5) * slot;
    for plot in &ind.plots {
        let c = rgba(plot.rgb);
        let Some(d) = plot.data.first() else { continue };
        let off = d.len().saturating_sub(n);
        let d = &d[off..];
        match plot.draw_type {
            // histograms (from zero, or the pane bottom when 0 is out of range)
            3 | 12 => {
                let base = p.y(0.0_f64.clamp(p.lo, p.hi));
                let wd = (slot * 0.6).max(1.0);
                for (i, v) in d.iter().enumerate() {
                    if ok(*v) {
                        let y = p.y(*v);
                        fill_rect(
                            img,
                            x(i) - wd / 2.0,
                            y.min(base),
                            wd,
                            (y - base).abs().max(1.0),
                            c,
                        );
                    }
                }
            }
            // arrows / dots
            5 | 14 => {
                for (i, v) in d.iter().enumerate() {
                    if ok(*v) {
                        fill_rect(img, x(i) - 2.0, p.y(*v) - 2.0, 4.0, 4.0, c);
                    }
                }
            }
            // bars/candles drawn by an indicator: use the close buffer as a line
            8 | 9 | 16 | 17 => {
                if let Some(cl) = plot.data.get(3) {
                    polyline(img, &cl[cl.len().saturating_sub(n)..], p, slot, c);
                }
            }
            // two-line types
            4 | 6 | 7 | 13 | 15 => {
                polyline(img, d, p, slot, c);
                if let Some(d2) = plot.data.get(1) {
                    polyline(img, &d2[d2.len().saturating_sub(n)..], p, slot, c);
                }
            }
            _ => polyline(img, d, p, slot, c),
        }
    }
}

fn polyline(img: &mut RgbaImage, d: &[f64], p: &Pane, slot: f64, c: Rgba<u8>) {
    let mut prev: Option<(f64, f64)> = None;
    for (i, v) in d.iter().enumerate() {
        if !ok(*v) {
            prev = None;
            continue;
        }
        let pt = ((i as f64 + 0.5) * slot, p.y(*v));
        if let Some(a) = prev {
            line(img, a, pt, c);
            line(img, (a.0, a.1 + 0.7), (pt.0, pt.1 + 0.7), c);
        }
        prev = Some(pt);
    }
}

fn put(img: &mut RgbaImage, x: i64, y: i64, c: Rgba<u8>) {
    if x >= 0 && y >= 0 && (x as u32) < img.width() && (y as u32) < img.height() {
        img.put_pixel(x as u32, y as u32, c);
    }
}

fn line(img: &mut RgbaImage, a: (f64, f64), b: (f64, f64), c: Rgba<u8>) {
    let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs()).ceil().max(1.0) as i64;
    for s in 0..=steps {
        let t = s as f64 / steps as f64;
        put(
            img,
            (a.0 + (b.0 - a.0) * t).round() as i64,
            (a.1 + (b.1 - a.1) * t).round() as i64,
            c,
        );
    }
}

fn vline(img: &mut RgbaImage, x: f64, y1: f64, y2: f64, c: Rgba<u8>) {
    let (a, b) = (y1.min(y2).round() as i64, y1.max(y2).round() as i64);
    for y in a..=b {
        put(img, x.round() as i64, y, c);
    }
}

fn hline(img: &mut RgbaImage, x1: f64, x2: f64, y: f64, c: Rgba<u8>) {
    for x in x1.round() as i64..=x2.round() as i64 {
        put(img, x, y.round() as i64, c);
    }
}

fn dashed_hline(img: &mut RgbaImage, x1: f64, x2: f64, y: f64, c: Rgba<u8>) {
    for x in x1.round() as i64..=x2.round() as i64 {
        if (x / 4) % 2 == 0 {
            put(img, x, y.round() as i64, c);
        }
    }
}

fn fill_rect(img: &mut RgbaImage, x: f64, y: f64, w: f64, h: f64, c: Rgba<u8>) {
    for yy in y.round() as i64..(y + h).round().max(y.round() + 1.0) as i64 {
        for xx in x.round() as i64..(x + w).round().max(x.round() + 1.0) as i64 {
            put(img, xx, yy, c);
        }
    }
}

// a tiny 3x5 bitmap font for numbers, scaled 2x
fn glyph(ch: char) -> Option<[u8; 5]> {
    Some(match ch {
        '0' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [7, 1, 7, 4, 7],
        '3' => [7, 1, 7, 1, 7],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 7, 1, 7],
        '6' => [7, 4, 7, 5, 7],
        '7' => [7, 1, 1, 1, 1],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 7],
        '.' => [0, 0, 0, 0, 2],
        '-' => [0, 0, 7, 0, 0],
        ':' => [0, 2, 0, 2, 0],
        ' ' => [0, 0, 0, 0, 0],
        _ => return None,
    })
}

fn text_w(s: &str) -> f64 {
    s.chars().count() as f64 * 8.0
}

fn text(img: &mut RgbaImage, x: f64, y: f64, s: &str, c: Rgba<u8>) {
    let mut cx = x.round() as i64;
    let cy = y.round() as i64;
    for ch in s.chars() {
        if let Some(g) = glyph(ch) {
            for (row, bits) in g.iter().enumerate() {
                for col in 0..3 {
                    if bits & (4 >> col) != 0 {
                        for dy in 0..2 {
                            for dx in 0..2 {
                                put(img, cx + col * 2 + dx, cy + row as i64 * 2 + dy, c);
                            }
                        }
                    }
                }
            }
        }
        cx += 8;
    }
}
