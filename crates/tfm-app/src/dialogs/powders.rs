//! "Tipos de Pó": tabela nome / fator (g/mm²) / custo (R$/kg) com Adicionar,
//! Editar, Remover, Ativar e "Padrões PTA" (os 9 pós do config do processo).
//! A lista é editada numa cópia e só vai para o config (e para o disco) em
//! "Salvar".

use egui::{Color32, RichText};
use egui_extras::{Column, TableBuilder};
use tfm_core::config::{AppConfig, Powder};

use crate::app::{NotifyKind, TfmApp};
use crate::dialogs::{self, fmt_num, DialogId};
use crate::widgets::{num_field, parse_num, text_field};

/// Os 9 pós de "Padrões PTA": `(nome, fator g/mm², custo R$/kg)`.
pub const PTA_DEFAULTS: [(&str, f64, f64); 9] = [
    ("Inconel 625", 0.16, 350.0),
    ("Stellite 6", 0.16, 450.0),
    ("Stellite 12", 0.16, 480.0),
    ("Stellite 21", 0.16, 460.0),
    ("WC-Ni (Carbeto)", 0.25, 600.0),
    ("Aço Inox 316L", 0.15, 80.0),
    ("Aço Inox 309L", 0.15, 85.0),
    ("Bronze Alumínio", 0.14, 120.0),
    ("Hastelloy C-276", 0.17, 550.0),
];

/// Formulário inline de "Novo Pó"/"Editar Pó".
#[derive(Clone, Debug, Default)]
pub struct PowderEditor {
    /// `Some(i)` ao editar o pó `i`; `None` ao criar.
    pub index: Option<usize>,
    pub name: String,
    pub factor: String,
    pub cost: String,
}

impl PowderEditor {
    pub fn new() -> Self {
        PowderEditor {
            index: None,
            name: String::new(),
            factor: "0.16".to_string(),
            cost: "0.0".to_string(),
        }
    }

    pub fn edit(index: usize, p: &Powder) -> Self {
        PowderEditor {
            index: Some(index),
            name: p.name.clone(),
            factor: fmt_num(p.density_factor_g_mm2),
            cost: fmt_num(p.cost_brl_kg),
        }
    }

    /// Nome obrigatório, fator > 0 e custo >= 0.
    pub fn parse(&self) -> Result<Powder, String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("Nome obrigatório".to_string());
        }
        let factor = parse_num(&self.factor)
            .filter(|v| *v > 0.0)
            .ok_or("Fator (g/mm²) inválido: use um número maior que zero.")?;
        let cost = parse_num(&self.cost)
            .filter(|v| *v >= 0.0)
            .ok_or("Custo (R$/kg) inválido: use um número maior ou igual a zero.")?;
        Ok(Powder {
            name: name.to_string(),
            density_factor_g_mm2: factor,
            cost_brl_kg: cost,
        })
    }
}

/// Acrescenta os padrões PTA que ainda não existem (pelo nome); devolve
/// quantos entraram.
pub fn add_pta_defaults(powders: &mut Vec<Powder>) -> usize {
    let mut added = 0;
    for (name, factor, cost) in PTA_DEFAULTS {
        if !powders.iter().any(|p| p.name == name) {
            powders.push(Powder {
                name: name.to_string(),
                density_factor_g_mm2: factor,
                cost_brl_kg: cost,
            });
            added += 1;
        }
    }
    added
}

#[derive(Clone, Debug, Default)]
pub struct PowdersState {
    pub loaded: bool,
    /// Cópia de trabalho de `cfg.powders`.
    pub powders: Vec<Powder>,
    /// Cópia de trabalho de `cfg.active_powder`.
    pub active: String,
    pub selected: Option<usize>,
    pub editor: Option<PowderEditor>,
    pub confirm_remove: Option<usize>,
}

impl PowdersState {
    pub fn load(&mut self, cfg: &AppConfig) {
        self.powders = cfg.powders.clone();
        self.active = cfg.active_powder.clone();
        self.loaded = true;
    }

    /// Confirma o editor: insere ou substitui; nome repetido em outro
    /// índice é erro. Devolve o nome gravado.
    pub fn commit_editor(&mut self) -> Result<String, String> {
        let Some(editor) = self.editor.as_ref() else {
            return Err("Nenhum pó em edição.".to_string());
        };
        let powder = editor.parse()?;
        let duplicate = self
            .powders
            .iter()
            .enumerate()
            .any(|(i, p)| Some(i) != editor.index && p.name == powder.name);
        if duplicate {
            return Err(format!("Já existe um pó chamado \"{}\".", powder.name));
        }
        let name = powder.name.clone();
        match editor.index {
            Some(i) if i < self.powders.len() => {
                if self.powders[i].name == self.active {
                    self.active = name.clone();
                }
                self.powders[i] = powder;
                self.selected = Some(i);
            }
            _ => {
                self.powders.push(powder);
                self.selected = Some(self.powders.len() - 1);
            }
        }
        self.editor = None;
        Ok(name)
    }

    /// Remove o pó `index`; se era o ativo, o primeiro da lista assume.
    pub fn remove(&mut self, index: usize) -> Option<Powder> {
        if index >= self.powders.len() {
            return None;
        }
        let removed = self.powders.remove(index);
        if self.active == removed.name {
            self.active = self
                .powders
                .first()
                .map(|p| p.name.clone())
                .unwrap_or_default();
        }
        self.selected = None;
        Some(removed)
    }

    pub fn activate(&mut self, index: usize) -> bool {
        match self.powders.get(index) {
            Some(p) => {
                self.active = p.name.clone();
                true
            }
            None => false,
        }
    }

    /// Aplica a cópia no config.
    pub fn apply(&self, cfg: &mut AppConfig) {
        cfg.powders = self.powders.clone();
        cfg.active_powder = self.active.clone();
    }
}

fn table(ui: &mut egui::Ui, state: &mut PowdersState, accent: Color32) {
    let active = state.active.clone();
    let mut clicked = None;
    TableBuilder::new(ui)
        .id_salt("pos")
        .striped(true)
        .sense(egui::Sense::click())
        .column(Column::remainder().at_least(150.0))
        .column(Column::initial(130.0).at_least(90.0))
        .column(Column::initial(110.0).at_least(90.0))
        .header(20.0, |mut header| {
            header.col(|ui| {
                ui.strong("Nome");
            });
            header.col(|ui| {
                ui.strong("Peso Esp. (g/mm²)");
            });
            header.col(|ui| {
                ui.strong("Custo (R$/kg)");
            });
        })
        .body(|mut body| {
            for (i, p) in state.powders.iter().enumerate() {
                body.row(20.0, |mut row| {
                    row.set_selected(state.selected == Some(i));
                    row.col(|ui| {
                        let is_active = p.name == active;
                        let mut text = if is_active {
                            RichText::new(format!("● {}", p.name)).color(accent)
                        } else {
                            RichText::new(p.name.clone())
                        };
                        if is_active {
                            text = text.strong();
                        }
                        ui.label(text).on_hover_text(if is_active {
                            "Pó ativo (usado nos cálculos)"
                        } else {
                            "Clique para selecionar"
                        });
                    });
                    row.col(|ui| {
                        ui.monospace(fmt_num(p.density_factor_g_mm2));
                    });
                    row.col(|ui| {
                        ui.monospace(fmt_num(p.cost_brl_kg));
                    });
                    if row.response().clicked() {
                        clicked = Some(i);
                    }
                });
            }
        });
    if let Some(i) = clicked {
        state.selected = Some(i);
    }
}

pub fn show(app: &mut TfmApp, state: &mut PowdersState, ctx: &egui::Context, open: &mut bool) {
    if !state.loaded {
        state.load(&app.cfg);
    }
    let mut close = false;
    let mut save = false;
    let (accent, danger) = (app.palette.accent, app.palette.danger);
    let mut notices: Vec<(NotifyKind, String)> = Vec::new();
    dialogs::window(DialogId::Powders, open)
        .default_size([580.0, 440.0])
        .show(ctx, |ui| {
            let active = if state.active.is_empty() {
                "—".to_string()
            } else {
                state.active.clone()
            };
            ui.label(format!("Pó ativo: {active}"));
            let table_h = (ui.available_height() - 170.0).max(120.0);
            egui::ScrollArea::vertical()
                .id_salt("pos_tabela")
                .auto_shrink([false, false])
                .max_height(table_h)
                .show(ui, |ui| table(ui, state, accent));
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button("Padrões PTA")
                    .on_hover_text("Adiciona os 9 pós padrão do processo PTA que ainda não existem")
                    .clicked()
                {
                    let n = add_pta_defaults(&mut state.powders);
                    notices.push(if n > 0 {
                        (
                            NotifyKind::Sucesso,
                            format!("{n} pós padrão adicionados. Clique em Salvar para gravar."),
                        )
                    } else {
                        (NotifyKind::Info, "Todos os padrões já existem.".to_string())
                    });
                }
                if ui.button("Adicionar").clicked() {
                    state.editor = Some(PowderEditor::new());
                    state.confirm_remove = None;
                }
                let has_sel = state.selected.is_some_and(|i| i < state.powders.len());
                if ui.add_enabled(has_sel, egui::Button::new("Editar")).clicked() {
                    if let Some(p) = state.selected.and_then(|i| state.powders.get(i)) {
                        state.editor = Some(PowderEditor::edit(
                            state.selected.unwrap_or_default(),
                            p,
                        ));
                        state.confirm_remove = None;
                    }
                }
                if ui.add_enabled(has_sel, egui::Button::new("Remover")).clicked() {
                    state.confirm_remove = state.selected;
                    state.editor = None;
                }
                if ui
                    .add_enabled(has_sel, egui::Button::new("Ativar"))
                    .on_hover_text("Torna o pó selecionado o ativo (fator e custo usados nos cálculos)")
                    .clicked()
                {
                    if let Some(i) = state.selected {
                        state.activate(i);
                    }
                }
            });
            if let Some(i) = state.confirm_remove {
                let name = state
                    .powders
                    .get(i)
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                ui.group(|ui| {
                    ui.label(RichText::new(format!("Remover {name}?")).color(danger));
                    ui.horizontal(|ui| {
                        if ui.button("Sim, remover").clicked() {
                            state.remove(i);
                            state.confirm_remove = None;
                        }
                        if ui.button("Não").clicked() {
                            state.confirm_remove = None;
                        }
                    });
                });
            }
            let mut confirm = false;
            let mut cancel = false;
            if let Some(editor) = state.editor.as_mut() {
                let title = if editor.index.is_some() {
                    "Editar Pó"
                } else {
                    "Novo Pó"
                };
                ui.group(|ui| {
                    ui.strong(title);
                    text_field(ui, "Nome:", &mut editor.name, "Nome do pó (único na lista).");
                    let bad = parse_num(&editor.factor).is_none();
                    num_field(
                        ui,
                        "Fator (g/mm²):",
                        &mut editor.factor,
                        "Peso específico por área: gramas de pó por mm² revestido, por mm de espessura.",
                        bad,
                    );
                    let bad = parse_num(&editor.cost).is_none();
                    num_field(
                        ui,
                        "Custo (R$/kg):",
                        &mut editor.cost,
                        "Custo do pó por quilograma.",
                        bad,
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Confirmar").clicked() {
                            confirm = true;
                        }
                        if ui.button("Cancelar").clicked() {
                            cancel = true;
                        }
                    });
                });
            }
            if cancel {
                state.editor = None;
            } else if confirm {
                match state.commit_editor() {
                    Ok(name) => notices.push((
                        NotifyKind::Info,
                        format!("Pó \"{name}\" pronto. Clique em Salvar para gravar."),
                    )),
                    Err(err) => notices.push((NotifyKind::Aviso, format!("Valor inválido: {err}"))),
                }
            }
            ui.weak("As alterações só são gravadas ao clicar em Salvar.");
            dialogs::footer(ui, |ui| {
                if ui.button("Salvar").clicked() {
                    save = true;
                }
                if ui.button("Cancelar").clicked() {
                    close = true;
                }
            });
        });
    for (kind, text) in notices {
        app.notify(kind, text);
    }
    if save {
        state.apply(&mut app.cfg);
        if dialogs::save_config(app) {
            app.audit.log(
                "Cost/Config Edit",
                &format!(
                    "Pós atualizados ({} cadastrados, ativo: {}).",
                    state.powders.len(),
                    state.active
                ),
                "Engenharia",
            );
            close = true;
        }
    }
    if close {
        *open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padroes_pta_sem_repetir() {
        let mut powders = vec![Powder {
            name: "Stellite 6".to_string(),
            density_factor_g_mm2: 0.2,
            cost_brl_kg: 1.0,
        }];
        assert_eq!(add_pta_defaults(&mut powders), 8);
        assert_eq!(powders.len(), 9);
        assert_eq!(powders[0].cost_brl_kg, 1.0, "o existente não é sobrescrito");
        assert_eq!(add_pta_defaults(&mut powders), 0);
        let names: Vec<&str> = PTA_DEFAULTS.iter().map(|(n, _, _)| *n).collect();
        assert!(names.contains(&"Aço Inox 316L") && names.contains(&"Hastelloy C-276"));
    }

    #[test]
    fn editor_valida_campos() {
        let mut ed = PowderEditor::new();
        assert_eq!(ed.parse(), Err("Nome obrigatório".to_string()));
        ed.name = "Novo".into();
        ed.factor = "0".into();
        assert!(ed.parse().is_err());
        ed.factor = "0,2".into();
        ed.cost = "-5".into();
        assert!(ed.parse().is_err());
        ed.cost = "300".into();
        let p = ed.parse().expect("válido");
        assert_eq!((p.density_factor_g_mm2, p.cost_brl_kg), (0.2, 300.0));
    }

    #[test]
    fn commit_remove_e_ativo() {
        let mut st = PowdersState::default();
        st.load(&AppConfig::default());
        assert_eq!(st.active, "Pó padrão");
        st.editor = Some(PowderEditor {
            index: None,
            name: "Pó padrão".into(),
            factor: "0.1".into(),
            cost: "1".into(),
        });
        assert!(st.commit_editor().is_err(), "nome duplicado");
        st.editor = Some(PowderEditor {
            index: None,
            name: "X".into(),
            factor: "0.1".into(),
            cost: "1".into(),
        });
        assert_eq!(st.commit_editor(), Ok("X".to_string()));
        assert_eq!(st.selected, Some(1));
        assert!(st.activate(1));
        assert_eq!(st.active, "X");
        // Renomear o ativo acompanha.
        st.editor = Some(PowderEditor::edit(1, &st.powders[1]));
        if let Some(e) = st.editor.as_mut() {
            e.name = "Y".into();
        }
        assert_eq!(st.commit_editor(), Ok("Y".to_string()));
        assert_eq!(st.active, "Y");
        st.remove(1);
        assert_eq!(st.active, "Pó padrão");
        assert!(st.remove(7).is_none());
        let mut cfg = AppConfig::default();
        st.apply(&mut cfg);
        assert_eq!(cfg.powders.len(), 1);
    }
}
