//! Diálogos modais (custos, pós, fórmulas, perfis, qualidade, sobre...).
//! Esqueleto: as tasks de diálogos preenchem este módulo.

use crate::app::TfmApp;

#[derive(Clone, Debug, Default)]
pub struct Dialogs;

pub fn show_dialogs(_app: &mut TfmApp, _ctx: &egui::Context) {}
