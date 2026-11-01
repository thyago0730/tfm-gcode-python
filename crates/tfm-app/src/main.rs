//! Ponto de entrada do `TFM_GCODE.exe`.

// Em release o app é só janela (sem console); em debug o console mostra o log.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use tfm_app::app::TfmApp;
use tfm_app::{window_icon, APP_TITLE};

fn main() -> eframe::Result {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,tfm_app=info,tfm_core=info"),
    )
    .init();
    log::info!("{APP_TITLE}");

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(APP_TITLE)
        .with_app_id("TFM_GCODE")
        .with_maximized(true)
        .with_min_inner_size([1024.0, 680.0]);
    match window_icon() {
        Some(icon) => viewport = viewport.with_icon(Arc::new(icon)),
        None => log::warn!("Iniciando sem ícone de janela"),
    }

    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        APP_TITLE,
        options,
        Box::new(|cc| Ok(Box::new(TfmApp::new(cc)))),
    )
}
