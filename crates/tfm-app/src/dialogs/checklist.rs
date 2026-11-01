//! Checklist do "Baixar G-code": o resultado de `gcode_checklist` sobre o
//! programa gerado, item a item (OK ou as mensagens de falha). "Salvar .tap"
//! só fica ativo com tudo OK e chama [`download::choose_and_save`]; "Voltar"
//! (ou Esc) fecha sem gravar nada.

use chrono::NaiveDateTime;
use egui::{Button, Key, RichText};
use tfm_core::params::Params;
use tfm_core::validator::{checklist_passed, ChecklistItem};

use crate::app::TfmApp;
use crate::dialogs::{self, DialogId};
use crate::download::{self, suggested_tap_name};

/// Programa, parâmetros e checklist do download em curso.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChecklistState {
    pub items: Vec<ChecklistItem>,
    /// Programa completo (linhas unidas por quebra de linha simples).
    pub text: String,
    pub params: Params,
}

impl ChecklistState {
    /// Todos os itens em OK: salvar liberado.
    pub fn can_save(&self) -> bool {
        checklist_passed(&self.items)
    }

    /// Nome sugerido para o `.tap` ([`suggested_tap_name`] da ordem de
    /// serviço).
    pub fn suggested_name(&self, now: &NaiveDateTime) -> String {
        suggested_tap_name(&self.params.ordem_servico, now)
    }
}

pub fn show(app: &mut TfmApp, state: &mut ChecklistState, ctx: &egui::Context, open: &mut bool) {
    let mut close = false;
    let mut save = false;
    let (success, danger, muted) = (app.palette.success, app.palette.danger, app.palette.muted);
    let can_save = state.can_save();
    let suggested = state.suggested_name(&chrono::Local::now().naive_local());
    dialogs::window(DialogId::DownloadChecklist, open)
        .default_width(520.0)
        .show(ctx, |ui| {
            ui.label("Conferência do programa antes de salvar:");
            ui.add_space(4.0);
            for (index, item) in state.items.iter().enumerate() {
                let (mark, color) = if item.is_ok() {
                    ("OK", success)
                } else {
                    ("FALHA", danger)
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(mark).strong().color(color));
                    ui.label(item.name);
                });
                if !item.is_ok() {
                    ui.indent(("checklist_falhas", index), |ui| {
                        for failure in &item.failures {
                            ui.label(RichText::new(failure).color(danger));
                        }
                    });
                }
            }
            ui.add_space(6.0);
            if can_save {
                ui.label(
                    RichText::new(format!("Nome sugerido: {suggested}"))
                        .small()
                        .color(muted),
                );
            } else {
                ui.label(
                    RichText::new("Corrija os parâmetros e gere o G-code de novo antes de baixar.")
                        .color(danger),
                );
            }
            dialogs::footer(ui, |ui| {
                if ui
                    .add_enabled(can_save, Button::new("Salvar .tap"))
                    .on_hover_text(
                        "Escolher onde salvar o .tap; o registro de auditoria vai ao lado",
                    )
                    .on_disabled_hover_text("Há itens com falha na checklist")
                    .clicked()
                {
                    save = true;
                }
                if ui
                    .button("Voltar")
                    .on_hover_text("Fechar sem salvar (Esc)")
                    .clicked()
                {
                    close = true;
                }
            });
        });
    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        close = true;
    }
    if save {
        download::choose_and_save(app, state);
        close = true;
    }
    if close {
        *open = false;
    }
}
