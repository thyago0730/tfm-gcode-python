//! Painel direito: G-code com busca, filtros por eixo e realce. Esqueleto.

use crate::app::TfmApp;

#[derive(Clone, Debug, Default)]
pub struct GcodeView;

pub fn show_right(_app: &mut TfmApp, ui: &mut egui::Ui) {
    ui.heading("G-code");
    ui.weak("Nenhum G-code gerado ainda (em construção)");
}
