//! "Manutenção Preventiva": cartões por componente (uso/limite, barra de
//! desgaste, Registrar Troca/Reset, Editar Limite), "Adicionar Novo
//! Componente Monitorado" e a aba Histórico de Trocas. Toda ação grava
//! `maintenance_log.json` na hora, como o Python.

use egui::{Align, Color32, Layout, RichText};
use tfm_core::quality::Component;

use crate::app::{NotifyKind, TfmApp};
use crate::dialogs::{self, fmt_num, DialogId};
use crate::widgets::{num_field, parse_num, text_field};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaintTab {
    #[default]
    Status,
    Historico,
}

#[derive(Clone, Debug, Default)]
pub struct MaintenanceState {
    pub tab: MaintTab,
    /// Componente aguardando confirmação de reset.
    pub confirm_reset: Option<String>,
    /// `(componente, novo limite em texto)` em edição.
    pub edit_limit: Option<(String, String)>,
    pub adding: bool,
    pub new_name: String,
    pub new_limit: String,
}

/// Chave do componente a partir do nome digitado (`name.lower().replace(" ", "_")`).
pub fn component_key(name: &str) -> String {
    name.trim().to_lowercase().replace(' ', "_")
}

/// `bico_constritor` -> `Bico Constritor` (o `.title()` do Python).
pub fn title_case(key: &str) -> String {
    key.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Percentual de uso, 0 a 100 (limite <= 0 => 0).
pub fn usage_pct(current: f64, limit: f64) -> f64 {
    if limit > 0.0 {
        (current / limit * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    }
}

/// Limite de horas digitado: número >= 1 (o `minvalue=1.0` do Python).
pub fn parse_limit(s: &str) -> Result<f64, String> {
    parse_num(s).filter(|v| *v >= 1.0).ok_or_else(|| {
        "Limite inválido: informe um número de horas maior ou igual a 1.".to_string()
    })
}

/// Horas de limite como o Python mostrava: inteiro sem casas, senão uma casa.
pub fn fmt_hours(hours: f64) -> String {
    if hours == hours.trunc() && hours.abs() < 1e15 {
        format!("{hours:.0}")
    } else {
        format!("{hours:.1}")
    }
}

enum Act {
    Reset(String),
    SetLimit(String, f64),
    Add(String, f64),
}

struct Colors {
    success: Color32,
    warning: Color32,
    danger: Color32,
    muted: Color32,
}

pub fn show(app: &mut TfmApp, state: &mut MaintenanceState, ctx: &egui::Context, open: &mut bool) {
    let mut close = false;
    let mut act: Option<Act> = None;
    let mut notices: Vec<(NotifyKind, String)> = Vec::new();
    let colors = Colors {
        success: app.palette.success,
        warning: app.palette.warning,
        danger: app.palette.danger,
        muted: app.palette.muted,
    };
    let accent = app.palette.accent;
    dialogs::window(DialogId::Maintenance, open)
        .default_size([640.0, 540.0])
        .show(ctx, |ui| {
            let total_h = app.quality.maintenance.total_arc_time_minutes / 60.0;
            ui.label(
                RichText::new("Monitoramento de Desgaste (Tempo de Arco)")
                    .strong()
                    .size(15.0),
            );
            ui.label(
                RichText::new(format!("Tempo Total Acumulado: {total_h:.1} horas")).color(accent),
            );
            for alert in app.quality.maintenance_alerts() {
                ui.label(RichText::new(alert).color(colors.danger));
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(state.tab == MaintTab::Status, "Status dos Componentes")
                    .clicked()
                {
                    state.tab = MaintTab::Status;
                }
                if ui
                    .selectable_label(state.tab == MaintTab::Historico, "Histórico de Trocas")
                    .clicked()
                {
                    state.tab = MaintTab::Historico;
                }
            });
            ui.separator();
            match state.tab {
                MaintTab::Status => status_tab(ui, app, state, &colors, &mut act, &mut notices),
                MaintTab::Historico => history_tab(ui, app),
            }
            dialogs::footer(ui, |ui| {
                if ui.button("Fechar").clicked() {
                    close = true;
                }
            });
        });
    for (kind, text) in notices {
        app.notify(kind, text);
    }
    match act {
        Some(Act::Reset(name)) => {
            app.quality.reset_component(&name);
            app.audit.log(
                "Maintenance",
                &format!("Reset {name} (troca/manutenção registrada)."),
                "Engenharia",
            );
            app.notify(
                NotifyKind::Sucesso,
                format!(
                    "Contador de '{}' zerado e registrado no histórico.",
                    title_case(&name)
                ),
            );
        }
        Some(Act::SetLimit(name, limit)) => {
            if let Some(component) = app.quality.maintenance.components.get_mut(&name) {
                component.limit_hours = limit;
                app.quality.save_maintenance();
                app.notify(
                    NotifyKind::Sucesso,
                    format!(
                        "Limite de '{}' definido em {} h.",
                        title_case(&name),
                        fmt_hours(limit)
                    ),
                );
            }
        }
        Some(Act::Add(name, limit)) => {
            let key = component_key(&name);
            if key.is_empty() {
                app.notify(NotifyKind::Aviso, "Informe o nome do componente.");
            } else if app.quality.maintenance.components.contains_key(&key) {
                app.notify(
                    NotifyKind::Aviso,
                    format!("O componente '{key}' já existe."),
                );
            } else {
                app.quality
                    .maintenance
                    .components
                    .insert(key.clone(), Component::with_limit(limit));
                app.quality.save_maintenance();
                state.adding = false;
                state.new_name.clear();
                state.new_limit.clear();
                app.notify(
                    NotifyKind::Sucesso,
                    format!(
                        "Componente '{key}' monitorado com limite de {} h.",
                        fmt_hours(limit)
                    ),
                );
            }
        }
        None => {}
    }
    if close {
        *open = false;
    }
}

fn status_tab(
    ui: &mut egui::Ui,
    app: &TfmApp,
    state: &mut MaintenanceState,
    colors: &Colors,
    act: &mut Option<Act>,
    notices: &mut Vec<(NotifyKind, String)>,
) {
    let mut clear_edit = false;
    egui::ScrollArea::vertical()
        .id_salt("manutencao_status")
        .auto_shrink([false, false])
        .max_height((ui.available_height() - 40.0).max(120.0))
        .show(ui, |ui| {
            for (name, component) in &app.quality.maintenance.components {
                let pct = usage_pct(component.current_hours, component.limit_hours);
                let color = if pct > 90.0 {
                    colors.danger
                } else if pct > 75.0 {
                    colors.warning
                } else {
                    colors.success
                };
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.strong(title_case(name));
                    ui.horizontal(|ui| {
                        ui.label(format!("Uso Atual: {:.1} h", component.current_hours));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(format!("Limite: {} h", fmt_hours(component.limit_hours)));
                        });
                    });
                    ui.add(
                        egui::ProgressBar::new((pct / 100.0) as f32)
                            .fill(color)
                            .text(format!("{pct:.0} %")),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Editar Limite").clicked() {
                            state.edit_limit = Some((name.clone(), fmt_num(component.limit_hours)));
                            state.confirm_reset = None;
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .button("Registrar Troca/Reset")
                                .on_hover_text("Zera o contador deste componente e registra no histórico")
                                .clicked()
                            {
                                state.confirm_reset = Some(name.clone());
                                state.edit_limit = None;
                            }
                        });
                    });
                    if state.confirm_reset.as_deref() == Some(name.as_str()) {
                        ui.label(
                            RichText::new(format!(
                                "Você realizou a troca ou manutenção de '{name}'? Isso zerará o contador deste componente."
                            ))
                            .color(colors.danger),
                        );
                        ui.horizontal(|ui| {
                            if ui.button("Sim, zerar").clicked() {
                                *act = Some(Act::Reset(name.clone()));
                                state.confirm_reset = None;
                            }
                            if ui.button("Não").clicked() {
                                state.confirm_reset = None;
                            }
                        });
                    }
                    if let Some((edit_name, value)) = state.edit_limit.as_mut() {
                        if edit_name == name {
                            let bad = parse_limit(value).is_err();
                            num_field(
                                ui,
                                "Novo limite (h):",
                                value,
                                "Horas de arco até a próxima troca (mínimo 1).",
                                bad,
                            );
                            ui.horizontal(|ui| {
                                if ui.button("Confirmar").clicked() {
                                    match parse_limit(value) {
                                        Ok(limit) => {
                                            *act = Some(Act::SetLimit(name.clone(), limit));
                                            clear_edit = true;
                                        }
                                        Err(err) => notices.push((NotifyKind::Aviso, err)),
                                    }
                                }
                                if ui.button("Cancelar").clicked() {
                                    clear_edit = true;
                                }
                            });
                        }
                    }
                });
            }
            ui.add_space(6.0);
            if state.adding {
                ui.group(|ui| {
                    ui.strong("Novo Componente");
                    text_field(
                        ui,
                        "Nome do componente:",
                        &mut state.new_name,
                        "Ex.: Bocal de gás (vira a chave bocal_de_gás).",
                    );
                    let bad = parse_limit(&state.new_limit).is_err();
                    num_field(
                        ui,
                        "Limite de horas:",
                        &mut state.new_limit,
                        "Horas de arco até a troca (mínimo 1).",
                        bad,
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Confirmar").clicked() {
                            match parse_limit(&state.new_limit) {
                                Ok(limit) => *act = Some(Act::Add(state.new_name.clone(), limit)),
                                Err(err) => notices.push((NotifyKind::Aviso, err)),
                            }
                        }
                        if ui.button("Cancelar").clicked() {
                            state.adding = false;
                        }
                    });
                });
            } else if ui
                .button("+ Adicionar Novo Componente Monitorado")
                .clicked()
            {
                state.adding = true;
                state.new_limit = "100".to_string();
            }
            ui.label(
                RichText::new(format!(
                    "Arquivo: {}",
                    app.quality.maintenance_path().display()
                ))
                .small()
                .color(colors.muted),
            );
        });
    if clear_edit {
        state.edit_limit = None;
    }
}

fn history_tab(ui: &mut egui::Ui, app: &TfmApp) {
    let history = &app.quality.maintenance.history;
    egui::ScrollArea::vertical()
        .id_salt("manutencao_historico")
        .auto_shrink([false, false])
        .max_height((ui.available_height() - 40.0).max(120.0))
        .show(ui, |ui| {
            if history.is_empty() {
                ui.weak("Nenhuma troca registrada.");
                return;
            }
            egui::Grid::new("manutencao_historico_grid")
                .num_columns(3)
                .striped(true)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    ui.strong("Data/Hora");
                    ui.strong("Evento");
                    ui.strong("Horímetro Total");
                    ui.end_row();
                    for entry in history.iter().rev() {
                        ui.monospace(&entry.date);
                        ui.label(&entry.action);
                        ui.monospace(format!("{:.1} h", entry.total_time / 60.0));
                        ui.end_row();
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chave_e_titulo() {
        assert_eq!(component_key("  Bocal de Gás "), "bocal_de_gás");
        assert_eq!(title_case("bico_constritor"), "Bico Constritor");
        assert_eq!(title_case("eletrodo"), "Eletrodo");
        assert_eq!(title_case("__x"), "X");
    }

    #[test]
    fn percentual_e_limite() {
        assert_eq!(usage_pct(5.0, 20.0), 25.0);
        assert_eq!(usage_pct(30.0, 20.0), 100.0);
        assert_eq!(usage_pct(1.0, 0.0), 0.0);
        assert_eq!(parse_limit("40"), Ok(40.0));
        assert!(parse_limit("0,5").is_err());
        assert!(parse_limit("abc").is_err());
        assert_eq!(fmt_hours(20.0), "20");
        assert_eq!(fmt_hours(12.5), "12.5");
    }
}
