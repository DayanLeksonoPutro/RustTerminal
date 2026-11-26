#![allow(dead_code)]
//! Rust Terminal: a cross-platform trading terminal where custom indicators, expert
//! advisors and scripts written as .mq5 source run in a built-in engine.
//! The desktop binary (src/main.rs) and the Android library entry share this crate.

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
mod market;
mod mcp;
mod mcp_chart;
mod mql;
mod mt5ind;
mod objects;
mod samples;
mod storage;
mod style;
mod tester;
mod tester_ui;
#[cfg(test)]
mod tests;
pub mod tz;
mod ui;

use std::path::PathBuf;

/// Run the MCP server on stdin/stdout (no window): `rust-terminal mcp`.
pub fn run_mcp() {
    mcp::run()
}

/// Start the desktop application.
pub fn run_desktop() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let screenshot = args
        .iter()
        .position(|a| a == "--screenshot")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Rust Terminal — Simulated-Demo")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([900.0, 560.0]);
    if let Ok(img) = image::load_from_memory(include_bytes!("../assets/icon-256.png")) {
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        viewport = viewport.with_icon(egui::IconData {
            rgba: rgba.into_raw(),
            width: w,
            height: h,
        });
    }
    if screenshot.is_some() {
        viewport = viewport.with_active(false);
        #[cfg(target_os = "windows")]
        {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
    let mut options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    #[cfg(target_os = "macos")]
    if screenshot.is_some() {
        options.event_loop_builder = Some(Box::new(|builder| {
            use winit::platform::macos::EventLoopBuilderExtMacOS;
            builder.with_activate_ignoring_other_apps(false);
        }));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = &mut options;
    eframe::run_native(
        "Rust Terminal",
        options,
        Box::new(move |cc| Ok(Box::new(app::TerminalApp::new(cc, screenshot)))),
    )
}

/// Android entry point (NativeActivity). Data lives in the app's internal storage.
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("RustTerminal"),
    );
    if let Some(p) = app.internal_data_path() {
        std::env::set_var("RUST_TERMINAL_DATA", p);
    }
    let options = eframe::NativeOptions {
        android_app: Some(app),
        ..Default::default()
    };
    if let Err(e) = eframe::run_native(
        "Rust Terminal",
        options,
        Box::new(|cc| Ok(Box::new(app::TerminalApp::new(cc, None)))),
    ) {
        log::error!("{}", e);
    }
}
