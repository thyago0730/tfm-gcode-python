//! "Gerenciar EPS (WPS)": lista de EPS com "Nova vazia", "Nova a partir dos
//! parâmetros atuais", editor de limites min/max por parâmetro e Remover.
//! Criar, Salvar EPS e Remover gravam `wps.json` na hora, como no Python.

use egui::{RichText, TextEdit};
use indexmap::IndexMap;
use serde_json::Value;
use tfm_core::params::Params;
use tfm_core::quality::{Limit, Wps};

use crate::app::{NotifyKind, TfmApp};
use crate::dialogs::{self, current_params, fmt_num, DialogId};
use crate::widgets::{parse_num, text_field};

/// Parâmetros críticos que toda EPS lista (os do Python que existem em
/// [`Params`]): recebem faixa de ±10 % em "Nova a partir dos parâmetros
/// atuais".
pub const CRITICAL_KEYS: [&str; 6] = [
    "diametro",
    "velocidade_de_deposicao",
    "corrente_arco",
    "vazao_gas",
    "alim_po",
    "espessura_camada",
];

/// Uma linha do editor de limites (textos, para aceitar vírgula).
#[derive(Clone, Debug, Default)]
pub struct LimitRow {
    pub key: String,
    pub min: String,
    pub max: String,
}

/// Editor da EPS selecionada.
#[derive(Clone, Debug, Default)]
pub struct WpsEditor {
    pub index: usize,
    pub name: String,
    pub rows: Vec<LimitRow>,
    /// Parâmetro escolhido no combo "Adicionar parâmetro".
    pub add_key: String,
    /// Parâmetros numéricos de [`Params`] (candidatos a limite).
    pub candidates: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct WpsState {
    pub selected: Option<usize>,
    pub new_name: String,
    pub editor: Option<WpsEditor>,
    pub confirm_remove: Option<usize>,
}

impl WpsState {
    pub const EMPTY: Self = WpsState {
        selected: None,
        new_name: String::new(),
        editor: None,
        confirm_remove: None,
    };
}

/// [`Params`] como objeto JSON (os `params` de referência da EPS).
pub fn params_object(p: &Params) -> IndexMap<String, Value> {
    match serde_json::to_value(p) {
        Ok(Value::Object(map)) => map.into_iter().collect(),
        _ => IndexMap::new(),
    }
}

/// Nomes dos campos numéricos de [`Params`].
pub fn numeric_param_keys(p: &Params) -> Vec<String> {
    params_object(p)
        .into_iter()
        .filter(|(_, v)| v.is_number())
        .map(|(k, _)| k)
        .collect()
}

/// Faixa de ±10 % para cada chave crítica numérica > 0 (o "Capturar Atual
/// como EPS" do Python).
pub fn limits_from_params(params: &IndexMap<String, Value>) -> IndexMap<String, Limit> {
    CRITICAL_KEYS
        .iter()
        .filter_map(|key| {
            let value = params.get(*key)?.as_f64()?;
            (value > 0.0).then(|| {
                (
                    (*key).to_string(),
                    Limit {
                        min: value * 0.9,
                        max: value * 1.1,
                    },
                )
            })
        })
        .collect()
}

impl WpsEditor {
    /// Linhas = limites existentes + chaves críticas, em ordem alfabética.
    pub fn from_wps(index: usize, wps: &Wps) -> Self {
        let mut keys: Vec<String> = wps.limits.keys().cloned().collect();
        for key in CRITICAL_KEYS {
            if !keys.iter().any(|k| k == key) {
                keys.push(key.to_string());
            }
        }
        keys.sort();
        let rows = keys
            .into_iter()
            .map(|key| {
                let (min, max) = wps
                    .limits
                    .get(&key)
                    .map_or((0.0, 0.0), |l| (l.min, l.max));
                LimitRow {
                    key,
                    min: fmt_num(min),
                    max: fmt_num(max),
                }
            })
            .collect();
        WpsEditor {
            index,
            name: wps.name.clone(),
            rows,
            add_key: String::new(),
            candidates: numeric_param_keys(&Params::default()),
        }
    }

    /// Nome e limites válidos. Linhas com `max <= 0` são "sem limite" e não
    /// entram; as demais precisam de números com `min <= max`.
    pub fn parse(&self) -> Result<(String, IndexMap<String, Limit>), String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("Informe o nome da EPS.".to_string());
        }
        let mut limits = IndexMap::new();
        for row in &self.rows {
            let min = parse_num(&row.min).ok_or_else(|| format!("Mínimo inválido em '{}'.", row.key))?;
            let max = parse_num(&row.max).ok_or_else(|| format!("Máximo inválido em '{}'.", row.key))?;
            if max <= 0.0 {
                continue;
            }
            if min > max {
                return Err(format!(
                    "Em '{}' o mínimo ({}) é maior que o máximo ({}).",
                    row.key,
                    fmt_num(min),
                    fmt_num(max)
                ));
            }
            limits.insert(row.key.clone(), Limit { min, max });
        }
        Ok((name.to_string(), limits))
    }

    /// Acrescenta uma linha para `key` (ignora vazio e repetido).
    pub fn add_row(&mut self, key: &str) -> bool {
        let key = key.trim();
        if key.is_empty() || self.rows.iter().any(|r| r.key == key) {
            return false;
        }
        self.rows.push(LimitRow {
            key: key.to_string(),
            min: "0.0".to_string(),
            max: "0.0".to_string(),
        });
        true
    }
}

enum Act {
    New { from_params: bool },
    Edit(usize),
    Remove(usize),
    SaveEditor,
}

pub fn show(app: &mut TfmApp, state: &mut WpsState, ctx: &egui::Context, open: &mut bool) {
    let mut close = false;
    let mut cancel_editor = false;
    let mut act: Option<Act> = None;
    let (danger, muted) = (app.palette.danger, app.palette.muted);
    dialogs::window(DialogId::Wps, open)
        .default_size([760.0, 500.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Nome da nova EPS:");
                ui.add(
                    TextEdit::singleline(&mut state.new_name)
                        .hint_text("ex: EPS-PTA-001")
                        .desired_width(170.0),
                );
                if ui
                    .button("Nova vazia")
                    .on_hover_text("Cadastra a EPS sem parâmetros nem limites")
                    .clicked()
                {
                    act = Some(Act::New { from_params: false });
                }
                if ui
                    .button("Nova a partir dos parâmetros atuais")
                    .on_hover_text(
                        "Guarda os parâmetros atuais como referência e cria limites de ±10 % para as chaves críticas",
                    )
                    .clicked()
                {
                    act = Some(Act::New { from_params: true });
                }
            });
            ui.separator();
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(230.0);
                    ui.strong(format!("EPS cadastradas ({})", app.quality.wps.len()));
                    let mut clicked = None;
                    egui::ScrollArea::vertical()
                        .id_salt("wps_lista")
                        .auto_shrink([false, false])
                        .max_height((ui.available_height() - 70.0).max(100.0))
                        .show(ui, |ui| {
                            if app.quality.wps.is_empty() {
                                ui.weak("Nenhuma EPS cadastrada.");
                            }
                            for (i, wps) in app.quality.wps.iter().enumerate() {
                                let response = ui
                                    .selectable_label(state.selected == Some(i), &wps.name)
                                    .on_hover_text(format!(
                                        "{} limite(s), {} parâmetro(s) de referência",
                                        wps.limits.len(),
                                        wps.params.len()
                                    ));
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
                    ui.horizontal(|ui| {
                        let has_sel = state.selected.is_some_and(|i| i < app.quality.wps.len());
                        if ui.add_enabled(has_sel, egui::Button::new("Editar")).clicked() {
                            if let Some(i) = state.selected {
                                act = Some(Act::Edit(i));
                            }
                        }
                        if ui.add_enabled(has_sel, egui::Button::new("Remover")).clicked() {
                            state.confirm_remove = state.selected;
                        }
                    });
                    if let Some(i) = state.confirm_remove {
                        let name = app
                            .quality
                            .wps
                            .get(i)
                            .map(|w| w.name.clone())
                            .unwrap_or_default();
                        ui.label(RichText::new(format!("Remover EPS '{name}'?")).color(danger));
                        ui.horizontal(|ui| {
                            if ui.button("Sim, remover").clicked() {
                                act = Some(Act::Remove(i));
                            }
                            if ui.button("Não").clicked() {
                                state.confirm_remove = None;
                            }
                        });
                    }
                });
                ui.separator();
                ui.vertical(|ui| match state.editor.as_mut() {
                    None => {
                        ui.weak(
                            "Selecione uma EPS e clique em Editar para ajustar os limites (Min - Max) por parâmetro.",
                        );
                    }
                    Some(editor) => {
                        ui.strong(format!("Editar EPS: {}", editor.name));
                        text_field(ui, "Nome da EPS:", &mut editor.name, "Nome único da EPS.");
                        ui.label(
                            RichText::new("Limites de Parâmetros (Min - Max) — máx = 0 desativa o limite")
                                .small()
                                .color(muted),
                        );
                        let mut remove_row = None;
                        egui::ScrollArea::vertical()
                            .id_salt("wps_limites")
                            .auto_shrink([false, false])
                            .max_height((ui.available_height() - 90.0).max(100.0))
                            .show(ui, |ui| {
                                egui::Grid::new("wps_limites_grid")
                                    .num_columns(4)
                                    .striped(true)
                                    .spacing([8.0, 4.0])
                                    .show(ui, |ui| {
                                        ui.strong("Parâmetro");
                                        ui.strong("Mín");
                                        ui.strong("Máx");
                                        ui.label("");
                                        ui.end_row();
                                        for (i, row) in editor.rows.iter_mut().enumerate() {
                                            ui.label(RichText::new(&row.key).monospace());
                                            let bad_min = parse_num(&row.min).is_none();
                                            let bad_max = parse_num(&row.max).is_none();
                                            let mut min = TextEdit::singleline(&mut row.min).desired_width(80.0);
                                            if bad_min {
                                                min = min.text_color(danger);
                                            }
                                            ui.add(min);
                                            let mut max = TextEdit::singleline(&mut row.max).desired_width(80.0);
                                            if bad_max {
                                                max = max.text_color(danger);
                                            }
                                            ui.add(max);
                                            if ui
                                                .small_button("✕")
                                                .on_hover_text("Tirar este parâmetro da lista")
                                                .clicked()
                                            {
                                                remove_row = Some(i);
                                            }
                                            ui.end_row();
                                        }
                                    });
                            });
                        if let Some(i) = remove_row {
                            if i < editor.rows.len() {
                                editor.rows.remove(i);
                            }
                        }
                        ui.horizontal(|ui| {
                            let current = if editor.add_key.is_empty() {
                                "parâmetro...".to_string()
                            } else {
                                editor.add_key.clone()
                            };
                            let available: Vec<String> = editor
                                .candidates
                                .iter()
                                .filter(|k| !editor.rows.iter().any(|r| &r.key == *k))
                                .cloned()
                                .collect();
                            egui::ComboBox::from_id_salt("wps_add_key")
                                .selected_text(current)
                                .width(210.0)
                                .show_ui(ui, |ui| {
                                    for key in &available {
                                        if ui.selectable_label(editor.add_key == *key, key).clicked() {
                                            editor.add_key = key.clone();
                                        }
                                    }
                                });
                            if ui.button("Adicionar parâmetro").clicked() {
                                let key = editor.add_key.clone();
                                if editor.add_row(&key) {
                                    editor.add_key.clear();
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Salvar EPS").clicked() {
                                act = Some(Act::SaveEditor);
                            }
                            if ui.button("Cancelar edição").clicked() {
                                cancel_editor = true;
                            }
                        });
                    }
                });
            });
            dialogs::footer(ui, |ui| {
                if ui.button("Fechar").clicked() {
                    close = true;
                }
            });
        });
    if cancel_editor {
        state.editor = None;
    }
    match act {
        Some(Act::New { from_params }) => create(app, state, from_params),
        Some(Act::Edit(i)) => {
            if let Some(wps) = app.quality.wps.get(i) {
                state.editor = Some(WpsEditor::from_wps(i, wps));
                state.selected = Some(i);
                state.confirm_remove = None;
            }
        }
        Some(Act::Remove(i)) => {
            state.confirm_remove = None;
            if let Some(wps) = app.quality.wps.get(i).cloned() {
                app.quality.remove_wps(&wps.name);
                state.selected = None;
                if state.editor.as_ref().is_some_and(|e| e.index == i) {
                    state.editor = None;
                }
                app.audit.log(
                    "Quality",
                    &format!("EPS '{}' removida.", wps.name),
                    "Engenharia",
                );
                app.notify(NotifyKind::Sucesso, format!("EPS '{}' removida.", wps.name));
            }
        }
        Some(Act::SaveEditor) => save_editor(app, state),
        None => {}
    }
    if close {
        *open = false;
    }
}

/// "Nova vazia" / "Nova a partir dos parâmetros atuais".
fn create(app: &mut TfmApp, state: &mut WpsState, from_params: bool) {
    let name = state.new_name.trim().to_string();
    if name.is_empty() {
        app.notify(
            NotifyKind::Aviso,
            "Informe o nome da EPS (ex: EPS-PTA-001).",
        );
        return;
    }
    if app.quality.wps(&name).is_some() {
        app.notify(NotifyKind::Aviso, format!("EPS '{name}' já existe."));
        return;
    }
    if from_params {
        let params = params_object(&current_params(app));
        let limits = limits_from_params(&params);
        app.quality.add_wps(&name, Some(params));
        if let Some(wps) = app.quality.wps.iter_mut().find(|w| w.name == name) {
            wps.limits = limits;
        }
        app.quality.save_wps();
    } else {
        app.quality.add_wps(&name, None);
    }
    state.new_name.clear();
    state.selected = app.quality.wps.iter().position(|w| w.name == name);
    app.audit.log(
        "Quality",
        &format!("EPS '{name}' cadastrada."),
        "Engenharia",
    );
    let path = app.quality.wps_path();
    app.notify(
        NotifyKind::Sucesso,
        format!("EPS '{name}' cadastrada em {}.", path.display()),
    );
}

/// "Salvar EPS": valida o editor e grava nome e limites.
fn save_editor(app: &mut TfmApp, state: &mut WpsState) {
    let Some(editor) = state.editor.clone() else {
        return;
    };
    let (name, limits) = match editor.parse() {
        Ok(parsed) => parsed,
        Err(err) => {
            app.notify(NotifyKind::Aviso, err);
            return;
        }
    };
    let duplicate = app
        .quality
        .wps
        .iter()
        .enumerate()
        .any(|(j, w)| j != editor.index && w.name == name);
    if duplicate {
        app.notify(NotifyKind::Aviso, format!("Já existe uma EPS chamada '{name}'."));
        return;
    }
    let Some(wps) = app.quality.wps.get_mut(editor.index) else {
        app.notify(NotifyKind::Aviso, "A EPS em edição não existe mais.");
        state.editor = None;
        return;
    };
    wps.name = name.clone();
    wps.limits = limits;
    let count = wps.limits.len();
    app.quality.save_wps();
    app.audit.log(
        "Quality",
        &format!("EPS '{name}' atualizada ({count} limite(s))."),
        "Engenharia",
    );
    app.notify(
        NotifyKind::Sucesso,
        format!("EPS '{name}' salva com {count} limite(s)."),
    );
    state.editor = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limites_de_dez_por_cento_so_para_criticos_positivos() {
        let p = Params {
            diametro: 100.0,
            corrente_arco: 0.0,
            ..Params::default()
        };
        let params = params_object(&p);
        assert!(params.contains_key("diametro"));
        let limits = limits_from_params(&params);
        let d = limits.get("diametro").expect("diametro");
        assert!((d.min - 90.0).abs() < 1e-9 && (d.max - 110.0).abs() < 1e-9);
        assert!(!limits.contains_key("corrente_arco"));
        assert!(limits.keys().all(|k| CRITICAL_KEYS.contains(&k.as_str())));
    }

    #[test]
    fn editor_une_limites_e_criticos_em_ordem() {
        let mut wps = Wps {
            name: "EPS-1".into(),
            ..Wps::default()
        };
        wps.limits.insert("zeta".into(), Limit { min: 1.0, max: 2.0 });
        let ed = WpsEditor::from_wps(0, &wps);
        let keys: Vec<&str> = ed.rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys.len(), CRITICAL_KEYS.len() + 1);
        assert_eq!(keys.last(), Some(&"zeta"));
        assert!(keys.windows(2).all(|w| w[0] < w[1]), "{keys:?}");
        assert!(ed.candidates.contains(&"diametro".to_string()));
        assert!(!ed.candidates.contains(&"nome_procedimento".to_string()));
    }

    #[test]
    fn parse_descarta_max_zero_e_valida_faixa() {
        let mut ed = WpsEditor {
            name: "EPS-2".into(),
            ..WpsEditor::default()
        };
        ed.rows.push(LimitRow {
            key: "a".into(),
            min: "0".into(),
            max: "0".into(),
        });
        ed.rows.push(LimitRow {
            key: "b".into(),
            min: "1,5".into(),
            max: "3".into(),
        });
        let (name, limits) = ed.parse().expect("ok");
        assert_eq!(name, "EPS-2");
        assert_eq!(limits.len(), 1);
        assert_eq!(limits["b"], Limit { min: 1.5, max: 3.0 });
        ed.rows[1].min = "5".into();
        assert!(ed.parse().is_err_and(|e| e.contains("maior que o máximo")));
        ed.rows[1].min = "x".into();
        assert!(ed.parse().is_err_and(|e| e.contains("Mínimo inválido")));
        ed.name = " ".into();
        assert!(ed.parse().is_err());
    }

    #[test]
    fn add_row_sem_repetir() {
        let mut ed = WpsEditor::default();
        assert!(ed.add_row("diametro"));
        assert!(!ed.add_row("diametro"));
        assert!(!ed.add_row("  "));
        assert_eq!(ed.rows.len(), 1);
    }
}
