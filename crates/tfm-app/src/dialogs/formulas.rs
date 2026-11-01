//! "Fórmulas": editor JSON de `formulas.expressions` (indent 4, monoespaçado)
//! com lista de variáveis (clique insere no cursor), Validar, Formatar,
//! Avaliar (tabela chave/valor com os parâmetros atuais), Restaurar padrão,
//! Importar e Exportar. Só grava o config ao clicar em "Salvar".

use egui::text::{CCursor, CCursorRange};
use egui::text_edit::TextEditState;
use egui::{RichText, TextEdit};
use indexmap::IndexMap;
use serde_json::Value;
use tfm_core::config::{json_pretty4, AppConfig, Formulas};
use tfm_core::formula::{evaluate_block, formula_locals};

use crate::app::{NotifyKind, TfmApp};
use crate::dialogs::{self, DialogId};

/// Id fixo do editor, para ler/gravar o cursor entre quadros.
const EDITOR_ID: &str = "tfm_formulas_editor";

/// Mensagem de estado do editor (verde quando `ok`, `danger` quando não).
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub ok: bool,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct FormulasState {
    pub loaded: bool,
    /// Texto do editor (JSON de `expressions`).
    pub text: String,
    /// `formulas.notes`, editável abaixo do editor.
    pub notes: String,
    pub status: Option<Status>,
    /// Resultado do último "Avaliar": `(chave, valor ou erro)`.
    pub results: Vec<(String, Option<f64>)>,
    /// Nomes disponíveis nas fórmulas (chaves de `formula_locals`).
    pub variables: Vec<String>,
    /// Nome clicado na lista, inserido no editor no próximo desenho.
    pub pending_insert: Option<String>,
}

/// Interpreta o texto do editor. Aceita `{"expressions": {...}, "notes": ..}`
/// ou o mapa de expressões direto; toda expressão deve ser string. Devolve o
/// mapa (na ordem do texto) e as notas, se vieram no JSON.
pub fn parse_expressions(text: &str) -> Result<(IndexMap<String, String>, Option<String>), String> {
    let raw = text.trim();
    if raw.is_empty() {
        return Ok((IndexMap::new(), None));
    }
    let value: Value = serde_json::from_str(raw).map_err(|e| {
        format!(
            "Erro de JSON: linha {}, coluna {}: {}",
            e.line(),
            e.column(),
            e
        )
    })?;
    let Value::Object(root) = value else {
        return Err("Estrutura inválida: esperado objeto JSON na raiz.".to_string());
    };
    let (map, notes) = if root.contains_key("expressions") {
        let notes = root
            .get("notes")
            .and_then(Value::as_str)
            .map(str::to_string);
        match root.get("expressions") {
            Some(Value::Object(exprs)) => (exprs.clone(), notes),
            _ => {
                return Err("A chave 'expressions' deve conter um objeto de expressões.".to_string())
            }
        }
    } else {
        (root, None)
    };
    let mut out = IndexMap::with_capacity(map.len());
    let mut bad = Vec::new();
    for (key, value) in map {
        if key.trim().is_empty() {
            return Err("Chaves inválidas (devem ser strings não vazias).".to_string());
        }
        match value {
            Value::String(expr) => {
                out.insert(key, expr);
            }
            _ => bad.push(key),
        }
    }
    if !bad.is_empty() {
        return Err(format!(
            "Valores inválidos em chaves: {}. Use string de expressão.",
            bad.join(", ")
        ));
    }
    Ok((out, notes))
}

/// JSON das expressões com indent 4 (como o `config.json`).
pub fn format_expressions(exprs: &IndexMap<String, String>) -> String {
    json_pretty4(exprs).unwrap_or_else(|_| "{}".to_string())
}

/// Insere `snippet` na posição de caractere `at` (ou no fim, se `None` ou
/// além do texto); devolve a posição do cursor após a inserção.
pub fn insert_at_char(text: &mut String, at: Option<usize>, snippet: &str) -> usize {
    let n_chars = text.chars().count();
    let at = at.map_or(n_chars, |i| i.min(n_chars));
    let byte = text.char_indices().nth(at).map_or(text.len(), |(b, _)| b);
    text.insert_str(byte, snippet);
    at + snippet.chars().count()
}

/// Valor da tabela de resultados: seis casas, ou `erro` quando a chave falhou.
pub fn format_value(value: Option<f64>) -> String {
    match value {
        Some(x) if x.is_finite() => format!("{x:.6}"),
        Some(x) => format!("{x}"),
        None => "erro".to_string(),
    }
}

impl FormulasState {
    pub fn load(&mut self, app: &TfmApp) {
        self.text = format_expressions(&app.cfg.formulas.expressions);
        self.notes = app.cfg.formulas.notes.clone();
        self.refresh_variables(app);
        self.loaded = true;
    }

    pub fn refresh_variables(&mut self, app: &TfmApp) {
        let params = app.current_params();
        self.variables = formula_locals(&params, &app.cfg.costs)
            .keys()
            .cloned()
            .collect();
    }

    pub fn set_status(&mut self, ok: bool, text: impl Into<String>) {
        self.status = Some(Status {
            ok,
            text: text.into(),
        });
    }

    /// "Validar": interpreta o texto e mostra o resultado.
    pub fn validate(&mut self) -> Option<IndexMap<String, String>> {
        match parse_expressions(&self.text) {
            Ok((exprs, _)) => {
                self.set_status(
                    true,
                    format!("JSON válido: {} expressão(ões).", exprs.len()),
                );
                Some(exprs)
            }
            Err(err) => {
                self.set_status(false, err);
                None
            }
        }
    }

    /// "Formatar": reescreve o texto com indent 4.
    pub fn format(&mut self) {
        if let Some(exprs) = self.validate() {
            self.text = format_expressions(&exprs);
            self.set_status(true, "JSON formatado.");
        }
    }

    /// "Restaurar padrão": as 20 expressões e a nota originais (não salvas).
    pub fn restore_default(&mut self) {
        let defaults = AppConfig::default_formulas();
        self.text = format_expressions(&defaults.expressions);
        self.notes = defaults.notes;
        self.results.clear();
        self.set_status(
            true,
            "Fórmulas padrão restauradas (clique em Salvar para gravar).",
        );
    }

    /// "Avaliar": calcula todas as chaves com os parâmetros atuais.
    pub fn evaluate(&mut self, app: &TfmApp) {
        let Some(exprs) = self.validate() else {
            self.results.clear();
            return;
        };
        let params = app.current_params();
        let locals = formula_locals(&params, &app.cfg.costs);
        self.variables = locals.keys().cloned().collect();
        let results = evaluate_block(&exprs, &locals);
        let total = results.len();
        let errors = results.values().filter(|v| v.is_none()).count();
        self.results = results.into_iter().collect();
        self.set_status(
            errors == 0,
            format!("{total} chave(s) avaliada(s), {errors} com erro."),
        );
    }
}

fn import(state: &mut FormulasState) {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Importar fórmulas (JSON)")
        .add_filter("JSON", &["json"])
        .pick_file()
    else {
        return;
    };
    let read = std::fs::read_to_string(&path)
        .map_err(|e| format!("{} ({e})", path.display()))
        .and_then(|text| parse_expressions(&text));
    match read {
        Ok((exprs, notes)) => {
            state.text = format_expressions(&exprs);
            if let Some(notes) = notes {
                state.notes = notes;
            }
            state.results.clear();
            state.set_status(
                true,
                format!(
                    "Fórmulas importadas de {} (clique em Salvar para gravar).",
                    path.display()
                ),
            );
        }
        Err(err) => state.set_status(false, format!("Falha ao importar: {err}")),
    }
}

fn export(state: &mut FormulasState) -> Option<(NotifyKind, String)> {
    let exprs = state.validate()?;
    let path = rfd::FileDialog::new()
        .set_title("Exportar fórmulas (JSON)")
        .add_filter("JSON", &["json"])
        .set_file_name("formulas.json")
        .save_file()?;
    let formulas = Formulas {
        expressions: exprs,
        notes: state.notes.clone(),
    };
    let written = json_pretty4(&formulas)
        .map_err(|e| e.to_string())
        .and_then(|text| std::fs::write(&path, text).map_err(|e| e.to_string()));
    Some(match written {
        Ok(()) => (
            NotifyKind::Sucesso,
            format!("Fórmulas exportadas para {}", path.display()),
        ),
        Err(err) => (NotifyKind::Erro, format!("Falha ao exportar: {err}")),
    })
}

/// Insere o nome pendente no cursor do editor (ou no fim) e move o cursor
/// para depois dele.
fn apply_pending_insert(ctx: &egui::Context, state: &mut FormulasState) {
    let Some(snippet) = state.pending_insert.take() else {
        return;
    };
    let id = egui::Id::unique(EDITOR_ID);
    let mut edit_state = TextEditState::load(ctx, id).unwrap_or_default();
    let at = edit_state
        .cursor
        .char_range()
        .map(|range| range.primary.index.0);
    let cursor = insert_at_char(&mut state.text, at, &snippet);
    edit_state
        .cursor
        .set_char_range(Some(CCursorRange::one(CCursor::new(cursor))));
    edit_state.store(ctx, id);
    ctx.memory_mut(|mem| mem.request_focus(id));
}

pub fn show(app: &mut TfmApp, state: &mut FormulasState, ctx: &egui::Context, open: &mut bool) {
    if !state.loaded {
        state.load(app);
    }
    let mut close = false;
    let mut save = false;
    let mut notices: Vec<(NotifyKind, String)> = Vec::new();
    let (danger, success) = (app.palette.danger, app.palette.success);
    dialogs::window(DialogId::Formulas, open)
        .default_size([940.0, 640.0])
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(230.0);
                    ui.strong("Variáveis de Cálculo");
                    ui.weak("Clique para inserir no editor.");
                    let mut insert = None;
                    egui::ScrollArea::vertical()
                        .id_salt("formulas_vars")
                        .auto_shrink([false, false])
                        .max_height((ui.available_height() - 60.0).max(120.0))
                        .show(ui, |ui| {
                            for name in &state.variables {
                                if ui
                                    .selectable_label(false, RichText::new(name).monospace())
                                    .on_hover_text("Inserir no editor, na posição do cursor")
                                    .clicked()
                                {
                                    insert = Some(name.clone());
                                }
                            }
                        });
                    if insert.is_some() {
                        state.pending_insert = insert;
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.strong("Editor de Fórmulas (JSON)");
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Validar").clicked() {
                            state.validate();
                        }
                        if ui.button("Formatar").clicked() {
                            state.format();
                        }
                        if ui
                            .button("Avaliar")
                            .on_hover_text("Calcula todas as chaves com os parâmetros atuais")
                            .clicked()
                        {
                            state.evaluate(app);
                        }
                        ui.separator();
                        if ui.button("Restaurar padrão").clicked() {
                            state.restore_default();
                        }
                        if ui.button("Importar").clicked() {
                            import(state);
                        }
                        if ui.button("Exportar").clicked() {
                            if let Some(notice) = export(state) {
                                notices.push(notice);
                            }
                        }
                    });
                    match &state.status {
                        Some(status) => {
                            let color = if status.ok { success } else { danger };
                            ui.label(RichText::new(status.text.clone()).color(color));
                        }
                        None => {
                            ui.weak("Edite o JSON e use Validar/Avaliar antes de salvar.");
                        }
                    }
                    apply_pending_insert(ctx, state);
                    let editor_h = (ui.available_height() * 0.45).max(160.0);
                    egui::ScrollArea::vertical()
                        .id_salt("formulas_editor_scroll")
                        .auto_shrink([false, false])
                        .max_height(editor_h)
                        .show(ui, |ui| {
                            ui.add(
                                TextEdit::multiline(&mut state.text)
                                    .id(egui::Id::unique(EDITOR_ID))
                                    .code_editor()
                                    .desired_rows(14)
                                    .desired_width(f32::INFINITY)
                                    .lock_focus(true),
                            );
                        });
                    ui.add_space(4.0);
                    ui.strong("Resultados (avaliados com parâmetros atuais)");
                    let results_h = (ui.available_height() - 110.0).max(80.0);
                    egui::ScrollArea::vertical()
                        .id_salt("formulas_results")
                        .auto_shrink([false, false])
                        .max_height(results_h)
                        .show(ui, |ui| {
                            if state.results.is_empty() {
                                ui.weak("Clique em Avaliar para calcular as chaves.");
                                return;
                            }
                            egui::Grid::new("formulas_result_grid")
                                .num_columns(2)
                                .striped(true)
                                .spacing([24.0, 4.0])
                                .show(ui, |ui| {
                                    ui.strong("Chave");
                                    ui.strong("Valor");
                                    ui.end_row();
                                    for (key, value) in &state.results {
                                        ui.label(RichText::new(key).monospace());
                                        let mut text =
                                            RichText::new(format_value(*value)).monospace();
                                        if value.is_none() {
                                            text = text.color(danger).strong();
                                        }
                                        ui.label(text);
                                        ui.end_row();
                                    }
                                });
                        });
                    ui.add_space(4.0);
                    ui.strong("Notas");
                    ui.add(
                        TextEdit::multiline(&mut state.notes)
                            .desired_rows(3)
                            .desired_width(f32::INFINITY),
                    );
                });
            });
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
        match parse_expressions(&state.text) {
            Ok((exprs, _)) => {
                app.cfg.formulas.expressions = exprs;
                app.cfg.formulas.notes = state.notes.clone();
                if dialogs::save_config(app) {
                    app.audit.log(
                        "Formula Edit",
                        "Fórmulas atualizadas via editor de configurações.",
                        "Engenharia",
                    );
                    close = true;
                }
            }
            Err(err) => {
                state.set_status(false, err.clone());
                app.notify(NotifyKind::Erro, format!("Erro ao salvar fórmulas: {err}"));
            }
        }
    }
    if close {
        *open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tfm_core::params::Params;

    #[test]
    fn aceita_mapa_direto_e_envelope() {
        let (flat, notes) = parse_expressions(r#"{"a": "1 + 1", "b": "a * 2"}"#).expect("ok");
        assert_eq!(flat.len(), 2);
        assert_eq!(flat.get_index(0).map(|(k, _)| k.as_str()), Some("a"));
        assert!(notes.is_none());
        let (wrapped, notes) =
            parse_expressions(r#"{"expressions": {"x": "2"}, "notes": "nota"}"#).expect("ok");
        assert_eq!(wrapped["x"], "2");
        assert_eq!(notes.as_deref(), Some("nota"));
        assert!(parse_expressions("   ").expect("vazio").0.is_empty());
    }

    #[test]
    fn rejeita_estruturas_invalidas() {
        assert!(parse_expressions("[1, 2]").is_err_and(|e| e.contains("objeto JSON na raiz")));
        assert!(parse_expressions(r#"{"a": 1}"#).is_err_and(|e| e.contains("Valores inválidos")));
        assert!(
            parse_expressions(r#"{"expressions": 3}"#).is_err_and(|e| e.contains("expressions"))
        );
        assert!(parse_expressions(r#"{"": "1"}"#).is_err_and(|e| e.contains("Chaves inválidas")));
        let err = parse_expressions("{\n  \"a\": \"1\",\n}").expect_err("vírgula sobrando");
        assert!(err.starts_with("Erro de JSON: linha "), "{err}");
    }

    #[test]
    fn formatar_preserva_ordem_com_indent_4() {
        let defaults = AppConfig::default_formulas();
        let text = format_expressions(&defaults.expressions);
        assert!(text.starts_with("{\n    \"circumference_mm\""), "{text}");
        let (parsed, _) = parse_expressions(&text).expect("ok");
        assert_eq!(parsed, defaults.expressions);
        assert_eq!(parsed.len(), 20);
    }

    #[test]
    fn avaliar_padroes_resolve_as_20_chaves() {
        let defaults = AppConfig::default_formulas();
        let locals = formula_locals(&Params::default(), &AppConfig::default().costs);
        let results = evaluate_block(&defaults.expressions, &locals);
        assert_eq!(results.len(), 20);
        assert!(results.values().all(Option::is_some), "{results:?}");
        assert!(locals.contains_key("diametro") && locals.contains_key("pi"));
    }

    #[test]
    fn inserir_no_cursor_ou_no_fim() {
        let mut s = String::from("açaí");
        assert_eq!(insert_at_char(&mut s, Some(2), "XY"), 4);
        assert_eq!(s, "açXYaí");
        assert_eq!(insert_at_char(&mut s, None, "!"), 7);
        assert_eq!(s, "açXYaí!");
        assert_eq!(insert_at_char(&mut s, Some(99), "?"), 8);
        assert_eq!(s, "açXYaí!?");
        assert_eq!(insert_at_char(&mut s, Some(0), "<"), 1);
        assert!(s.starts_with('<'));
    }

    #[test]
    fn valor_formatado() {
        assert_eq!(format_value(Some(2.5)), "2.500000");
        assert_eq!(format_value(Some(1.0 / 3.0)), "0.333333");
        assert_eq!(format_value(None), "erro");
        assert_eq!(format_value(Some(f64::INFINITY)), "inf");
    }
}
