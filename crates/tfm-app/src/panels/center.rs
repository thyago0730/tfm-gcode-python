//! Centro: abas Análise Temporal / Oscilação / Estatísticas / Processo. Esqueleto.

use crate::app::TfmApp;

pub fn show_center(app: &mut TfmApp, ui: &mut egui::Ui) {
    let label = format!("{} (em construção)", app.center_tab.label());
    ui.centered_and_justified(|ui| ui.weak(label));
}
