//! "Gerenciar Operadores": lista de operadores qualificados e formulário
//! Nome / ID Qualificação / Validade (AAAA-MM-DD). Adicionar, Salvar
//! Alterações e Remover gravam `operators.json` na hora, como no Python.

use chrono::{Local, NaiveDate};
use egui::RichText;
use tfm_core::quality::{Operator, DATE_FMT};

use crate::app::{NotifyKind, TfmApp};
use crate::dialogs::{self, DialogId};
use crate::widgets::text_field;

#[derive(Clone, Debug, Default)]
pub struct OperatorsState {
    pub selected: Option<usize>,
    /// `Some(i)` enquanto o formulário edita o operador `i`.
    pub editing: Option<usize>,
    pub name: String,
    pub qualification_id: String,
    pub validity: String,
}

/// Validade vazia é aceita (sem validade); senão precisa estar em
/// `AAAA-MM-DD` e ser uma data real.
pub fn validate_date(s: &str) -> Result<Option<NaiveDate>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    NaiveDate::parse_from_str(s, DATE_FMT)
        .map(Some)
        .map_err(|_| "Validade inválida: use AAAA-MM-DD (ex.: 2026-12-31).".to_string())
}

/// Qualificação vencida: validade legível e anterior a `today`.
pub fn is_expired(validity: &str, today: NaiveDate) -> bool {
    matches!(validate_date(validity), Ok(Some(d)) if d < today)
}

impl OperatorsState {
    pub const EMPTY: Self = OperatorsState {
        selected: None,
        editing: None,
        name: String::new(),
        qualification_id: String::new(),
        validity: String::new(),
    };

    pub fn clear_form(&mut self) {
        self.editing = None;
        self.name.clear();
        self.qualification_id.clear();
        self.validity.clear();
    }

    pub fn load_form(&mut self, index: usize, op: &Operator) {
        self.editing = Some(index);
        self.name = op.name.clone();
        self.qualification_id = op.qualification_id.clone();
        self.validity = op.qualification_validity.clone();
    }
}

enum Act {
    Save,
    Edit(usize),
    Remove(usize),
    Clear,
}

pub fn show(app: &mut TfmApp, state: &mut OperatorsState, ctx: &egui::Context, open: &mut bool) {
    let mut close = false;
    let mut act: Option<Act> = None;
    let today = Local::now().date_naive();
    let (danger, muted) = (app.palette.danger, app.palette.muted);
    dialogs::window(DialogId::Operators, open)
        .default_size([600.0, 380.0])
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(260.0);
                    ui.strong(format!("Operadores ({})", app.quality.operators.len()));
                    let mut clicked = None;
                    egui::ScrollArea::vertical()
                        .id_salt("operadores_lista")
                        .auto_shrink([false, false])
                        .max_height((ui.available_height() - 30.0).max(100.0))
                        .show(ui, |ui| {
                            if app.quality.operators.is_empty() {
                                ui.weak("Nenhum operador cadastrado.");
                            }
                            for (i, op) in app.quality.operators.iter().enumerate() {
                                let qual = if op.qualification_id.is_empty() {
                                    "-"
                                } else {
                                    op.qualification_id.as_str()
                                };
                                let expired = is_expired(&op.qualification_validity, today);
                                let mut text = RichText::new(format!("{} (Qualif: {qual})", op.name));
                                if expired {
                                    text = text.color(danger);
                                }
                                let response = ui.selectable_label(state.selected == Some(i), text);
                                let response = if expired {
                                    response.on_hover_text(format!(
                                        "Qualificação vencida em {}",
                                        op.qualification_validity
                                    ))
                                } else if op.qualification_validity.is_empty() {
                                    response.on_hover_text("Sem validade cadastrada")
                                } else {
                                    response.on_hover_text(format!(
                                        "Válida até {}",
                                        op.qualification_validity
                                    ))
                                };
                                if response.double_clicked() {
                                    act = Some(Act::Edit(i));
                                } else if response.clicked() {
                                    clicked = Some(i);
                                }
                            }
                        });
                    if let Some(i) = clicked {
                        state.selected = Some(i);
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    let title = if state.editing.is_some() {
                        "Editar Operador"
                    } else {
                        "Novo Operador"
                    };
                    ui.strong(title);
                    text_field(
                        ui,
                        "Nome:",
                        &mut state.name,
                        "Nome do operador, como aparece na rastreabilidade.",
                    );
                    text_field(
                        ui,
                        "ID Qualificação:",
                        &mut state.qualification_id,
                        "Identificador do certificado/qualificação do operador.",
                    );
                    text_field(
                        ui,
                        "Validade (AAAA-MM-DD):",
                        &mut state.validity,
                        "Validade da qualificação; vencida, bloqueia a geração de G-code (ISO 3834). Vazio = sem validade.",
                    );
                    if let Err(err) = validate_date(&state.validity) {
                        ui.label(RichText::new(err).color(danger).small());
                    }
                    ui.horizontal_wrapped(|ui| {
                        let save_label = if state.editing.is_some() {
                            "Salvar Alterações"
                        } else {
                            "Adicionar"
                        };
                        if ui.button(save_label).clicked() {
                            act = Some(Act::Save);
                        }
                        let has_sel = state
                            .selected
                            .is_some_and(|i| i < app.quality.operators.len());
                        if ui.add_enabled(has_sel, egui::Button::new("Editar")).clicked() {
                            if let Some(i) = state.selected {
                                act = Some(Act::Edit(i));
                            }
                        }
                        if ui.add_enabled(has_sel, egui::Button::new("Remover")).clicked() {
                            if let Some(i) = state.selected {
                                act = Some(Act::Remove(i));
                            }
                        }
                        if ui.button("Limpar").clicked() {
                            act = Some(Act::Clear);
                        }
                    });
                    ui.label(
                        RichText::new(format!(
                            "Arquivo: {}",
                            app.quality.operators_path().display()
                        ))
                        .small()
                        .color(muted),
                    );
                });
            });
            dialogs::footer(ui, |ui| {
                if ui.button("Fechar").clicked() {
                    close = true;
                }
            });
        });
    match act {
        Some(Act::Save) => save(app, state),
        Some(Act::Edit(i)) => {
            if let Some(op) = app.quality.operators.get(i).cloned() {
                state.load_form(i, &op);
                state.selected = Some(i);
            }
        }
        Some(Act::Remove(i)) => {
            if let Some(op) = app.quality.operators.get(i).cloned() {
                app.quality.remove_operator(&op.name);
                state.selected = None;
                if state.editing == Some(i) {
                    state.clear_form();
                }
                app.audit.log(
                    "Quality",
                    &format!("Operador '{}' removido.", op.name),
                    "Engenharia",
                );
                app.notify(
                    NotifyKind::Sucesso,
                    format!("Operador '{}' removido.", op.name),
                );
            }
        }
        Some(Act::Clear) => state.clear_form(),
        None => {}
    }
    if close {
        *open = false;
    }
}

/// "Adicionar"/"Salvar Alterações": valida o formulário e grava.
fn save(app: &mut TfmApp, state: &mut OperatorsState) {
    let name = state.name.trim().to_string();
    if name.is_empty() {
        app.notify(NotifyKind::Aviso, "Informe o nome do operador.");
        return;
    }
    let validity = match validate_date(&state.validity) {
        Ok(Some(date)) => date.format(DATE_FMT).to_string(),
        Ok(None) => String::new(),
        Err(err) => {
            app.notify(NotifyKind::Aviso, err);
            return;
        }
    };
    let qualification = state.qualification_id.trim().to_string();
    match state.editing {
        Some(i) if i < app.quality.operators.len() => {
            let duplicate = app
                .quality
                .operators
                .iter()
                .enumerate()
                .any(|(j, op)| j != i && op.name == name);
            if duplicate {
                app.notify(
                    NotifyKind::Aviso,
                    format!("Já existe um operador chamado '{name}'."),
                );
                return;
            }
            let op = &mut app.quality.operators[i];
            op.name = name.clone();
            op.qualification_id = qualification;
            op.qualification_validity = validity;
            app.quality.save_operators();
            app.notify(
                NotifyKind::Sucesso,
                format!("Operador '{name}' atualizado."),
            );
        }
        _ => {
            app.quality
                .upsert_operator(&name, &qualification, &validity);
            app.notify(NotifyKind::Sucesso, format!("Operador '{name}' salvo."));
        }
    }
    app.audit.log(
        "Quality",
        &format!("Operador '{name}' cadastrado/atualizado."),
        "Engenharia",
    );
    state.clear_form();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valida_data() {
        assert_eq!(validate_date(""), Ok(None));
        assert_eq!(validate_date("  "), Ok(None));
        assert_eq!(
            validate_date("2026-12-31"),
            Ok(NaiveDate::from_ymd_opt(2026, 12, 31))
        );
        assert!(validate_date("31/12/2026").is_err());
        assert!(validate_date("2026-13-01").is_err());
    }

    #[test]
    fn vencimento() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 1).expect("data");
        assert!(is_expired("2026-10-31", today));
        assert!(!is_expired("2026-11-01", today));
        assert!(!is_expired("", today));
        assert!(!is_expired("lixo", today));
    }

    #[test]
    fn formulario() {
        let mut st = OperatorsState::default();
        let op = Operator {
            name: "Ana".into(),
            code: "A1".into(),
            qualification_id: "Q-1".into(),
            qualification_validity: "2027-01-01".into(),
        };
        st.load_form(2, &op);
        assert_eq!(st.editing, Some(2));
        assert_eq!(st.validity, "2027-01-01");
        st.clear_form();
        assert!(st.editing.is_none() && st.name.is_empty());
    }
}
