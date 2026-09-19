mod app;
mod auth;
mod config;
mod db;
mod fonts;
mod github;
mod i18n;
mod model;
mod sync;
mod token;
mod tree;

fn main() -> eframe::Result<()> {
    let native = eframe::NativeOptions::default();
    eframe::run_native(
        "Issue Viewer",
        native,
        Box::new(|cc| Ok(Box::new(app::IssueViewerApp::new(cc)))),
    )
}
