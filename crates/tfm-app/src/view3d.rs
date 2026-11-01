//! Visualização 3D interativa do percurso da tocha.
//! Esqueleto: a task do 3D substitui este placeholder.

use crate::app::TfmApp;

#[derive(Clone, Debug, Default)]
pub struct View3d;

pub fn show_view3d(_app: &mut TfmApp, ui: &mut egui::Ui) {
    ui.centered_and_justified(|ui| ui.weak("Visualização 3D interativa (em construção)"));
}
