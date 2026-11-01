//! Centro: abas Análise Temporal / Oscilação / Estatísticas / Processo.
//!
//! A aba "3D Interativo" nunca chega aqui (o `app.rs` a despacha para
//! [`crate::view3d`]). As quatro abas de gráficos recebem os dados do último
//! [`crate::compute::ComputeResult`]; sem resultado, ou com a geração em erro,
//! mostram "Sem dados: ajuste os parâmetros".

use egui::RichText;

use crate::app::{CenterTab, TfmApp};
use crate::charts;

/// Mensagem das abas de gráfico quando não há G-code gerado.
pub const EMPTY_MESSAGE: &str = "Sem dados: ajuste os parâmetros";

pub fn show_center(app: &mut TfmApp, ui: &mut egui::Ui) {
    let palette = &app.palette;
    let Some(result) = app.result.as_ref() else {
        empty_state(ui, None);
        return;
    };
    if let Some(reason) = result.gen_error.as_deref() {
        empty_state(ui, Some(reason));
        return;
    }
    match app.center_tab {
        CenterTab::AnaliseTemporal => charts::temporal(ui, &result.series, palette),
        CenterTab::Oscilacao => charts::oscillation(ui, &result.osc, palette),
        CenterTab::Estatisticas => charts::statistics(ui, &result.dists, palette),
        CenterTab::Processo => {
            charts::process_dashboard(ui, &result.estimates, &result.params, palette)
        }
        // Despachada pelo app.rs antes de chegar aqui; por segurança, só o rótulo.
        CenterTab::View3d => {
            ui.centered_and_justified(|ui| ui.weak(CenterTab::View3d.label()));
        }
    }
}

/// "Sem dados: ajuste os parâmetros" centrado, com o motivo da falha de
/// geração (quando houver) em letra menor logo abaixo.
fn empty_state(ui: &mut egui::Ui, reason: Option<&str>) {
    let weak = ui.visuals().weak_text_color();
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.4).max(0.0));
        ui.label(RichText::new(EMPTY_MESSAGE).size(15.0).color(weak));
        if let Some(reason) = reason {
            ui.add_space(6.0);
            ui.label(RichText::new(reason).size(12.0).color(weak));
        }
    });
}
