//! Painel esquerdo: seletor de operação e abas Parâmetros / Processo /
//! Rastreabilidade. Esqueleto.

use crate::app::TfmApp;

pub fn show_left(app: &mut TfmApp, ui: &mut egui::Ui) {
    ui.heading(app.ui.operation.label());
    ui.weak("Parâmetros da operação (em construção)");
}
