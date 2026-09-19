#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod auth;
mod config;
mod db;
mod filter;
mod fonts;
mod github;
mod i18n;
mod md;
mod model;
mod sync;
mod theme;
mod timefmt;
mod token;
mod tree;

fn main() -> eframe::Result<()> {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png"))
        .expect("app icon png");
    let layout = crate::db::Cache::open(&app::IssueViewerApp::cache_path())
        .ok()
        .and_then(|c| c.ui_layout().ok())
        .unwrap_or_default();
    let native = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_icon(icon)
            .with_inner_size([layout.window_w, layout.window_h])
            .with_min_inner_size([400.0, 300.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Issue Viewer",
        native,
        Box::new(|cc| Ok(Box::new(app::IssueViewerApp::new(cc)))),
    )
}
