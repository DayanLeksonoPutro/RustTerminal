#![allow(dead_code)]
//! Rust Terminal: a cross-platform trading terminal where custom indicators, expert
//! advisors and scripts written as .mq5 source run in a built-in engine.

mod account;
mod app;
mod chart;
mod data_ui;
mod dialogs;
mod editor;
mod feed;
mod history;
mod host;
mod indicators;
mod indicators_ext;
mod mt5ind;
mod market;
mod mql;
mod objects;
mod samples;
mod storage;
mod style;
mod tester;
mod tester_ui;
#[cfg(test)]
mod tests;
mod ui;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let screenshot = args.iter().position(|a| a == "--screenshot").and_then(|i| args.get(i + 1)).map(PathBuf::from);

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Rust Terminal — Simulated-Demo")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([900.0, 560.0]);
    if screenshot.is_some() {
        viewport = viewport.with_active(false);
        #[cfg(target_os = "windows")]
        {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
    let mut options = eframe::NativeOptions { viewport, ..Default::default() };
    #[cfg(target_os = "macos")]
    if screenshot.is_some() {
        options.event_loop_builder = Some(Box::new(|builder| {
            use winit::platform::macos::EventLoopBuilderExtMacOS;
            builder.with_activate_ignoring_other_apps(false);
        }));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = &mut options;
    eframe::run_native("Rust Terminal", options, Box::new(move |cc| Ok(Box::new(app::TerminalApp::new(cc, screenshot)))))
}
