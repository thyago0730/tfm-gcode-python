//! Painel inferior: Resultados Calculados | Estimativa de Custo | Avisos.
//!
//! Corpo do painel "Resultados e Custos" (o cabeçalho com ▲/▼ fica em
//! `app.rs`). Porta dos três quadros do rodapé do app Python: `result_frame`
//! (nove indicadores), `cost_frame` (cinco custos/consumos) e
//! `notification_frame` (histórico rolável + botão "Limpar"). Os rótulos
//! repetem os do Python caractere a caractere; sem resultado válido valem os
//! placeholders de `limpar_resultados` (`"0.00"` nos resultados, `"..."` nos
//! custos) e o motivo (`gen_error`) abre a coluna de avisos em `danger`.

use egui::{Align, Button, Color32, Label, Layout, RichText, ScrollArea, Ui};
use tfm_core::estimates::{CostSummary, Estimates, ResultText};
use tfm_core::overlap::Uniformity;

use crate::app::{Notification, TfmApp};
use crate::theme::Palette;

/// Valor dos resultados sem cálculo válido (`limpar_resultados`).
const RESULT_PLACEHOLDER: &str = "0.00";
/// Valor dos custos sem cálculo válido (`limpar_resultados`).
const COST_PLACEHOLDER: &str = "...";
/// Altura mínima de uma linha rótulo/valor (menor que os 18 px padrão do
/// egui, para as nove linhas caberem na altura inicial do painel).
const ROW_H: f32 = 14.0;
/// Espaço vertical entre linhas.
const ROW_GAP: f32 = 2.0;

/// Rótulos de "Resultados Calculados", na ordem do Python.
const RESULT_LABELS: [&str; 9] = [
    "Rotação (min/volta):",
    "Passo Axial (mm):",
    "Rotações/Passos:",
    "Ângulo Total/Passo (A):",
    "Tempo Total (hh:mm):",
    "Largura Efetiva (mm):",
    "Passo Real (mm):",
    "Cobertura Estimada:",
    "Uniformidade:",
];

/// Rótulos de "Estimativa de Custo", na ordem do Python.
const COST_LABELS: [&str; 5] = [
    "Custo Consumíveis:",
    "Custo Operacional:",
    "CUSTO TOTAL:",
    "Consumo Pó (kg):",
    "Consumo Gás (m³):",
];
/// Posição de "CUSTO TOTAL:" em [`COST_LABELS`] (negrito em `success`).
const TOTAL_IDX: usize = 2;

/// Uma linha rótulo/valor pronta para desenhar.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    label: &'static str,
    value: String,
    /// Cor do valor; `None` usa a cor padrão do texto.
    color: Option<Color32>,
    /// Negrito no rótulo e no valor (totais).
    bold: bool,
}

impl Row {
    fn new(label: &'static str, value: impl Into<String>) -> Row {
        Row {
            label,
            value: value.into(),
            color: None,
            bold: false,
        }
    }

    fn colored(mut self, color: Option<Color32>) -> Row {
        self.color = color;
        self
    }

    fn bold(mut self) -> Row {
        self.bold = true;
        self
    }
}

/// Corpo do painel inferior: três colunas de largura igual.
pub fn show_bottom(app: &mut TfmApp, ui: &mut Ui) {
    let palette = app.palette;
    // Empréstimos disjuntos: o resultado só é lido; os avisos são limpos
    // pelo botão "Limpar".
    let result = app.result.as_ref();
    let notifications = &mut app.notifications;

    // Placeholders enquanto não há resultado, os parâmetros não validam ou
    // a geração falhou (`ComputeResult::failed` traz estimativas em branco).
    let valid = result.is_some_and(|r| r.field_errors.is_valid() && r.gen_error.is_none());
    let estimates = if valid {
        result.map(|r| &r.estimates)
    } else {
        None
    };
    let gen_error = result.and_then(|r| r.gen_error.as_deref());
    let results = result_rows(estimates, &palette);
    let costs = cost_rows(estimates.map(|e| &e.costs), &palette);

    ui.columns(3, |cols| {
        kpi_column(
            &mut cols[0],
            "Resultados Calculados",
            "resultados",
            &results,
        );
        kpi_column(&mut cols[1], "Estimativa de Custo", "custos", &costs);
        warnings_column(&mut cols[2], notifications, gen_error, &palette);
    });
}

/// As nove linhas de "Resultados Calculados": placeholders sem estimativa
/// válida; rotação e passo em `danger` acima do limite da máquina;
/// uniformidade na cor do nível.
fn result_rows(estimates: Option<&Estimates>, palette: &Palette) -> Vec<Row> {
    let Some(est) = estimates else {
        return RESULT_LABELS
            .into_iter()
            .map(|label| Row::new(label, RESULT_PLACEHOLDER))
            .collect();
    };
    let limit = |r: &ResultText| r.over_limit.then_some(palette.danger);
    let [rot, pitch, rots, angle, time, width, step, cover, unif] = RESULT_LABELS;
    vec![
        Row::new(rot, &est.rotation.text).colored(limit(&est.rotation)),
        Row::new(pitch, &est.pitch.text).colored(limit(&est.pitch)),
        Row::new(rots, &est.rotations),
        Row::new(angle, &est.total_angle),
        Row::new(time, &est.time_hhmm),
        Row::new(width, &est.overlap.effective_bead_width),
        Row::new(step, &est.overlap.surface_step),
        Row::new(cover, &est.overlap.coverage),
        Row::new(unif, &est.overlap.uniformity).colored(Some(uniformity_color(
            est.overlap.uniformity_level,
            palette,
        ))),
    ]
}

/// As cinco linhas de "Estimativa de Custo": `"..."` sem estimativa válida;
/// moeda com duas casas, consumos com três; "CUSTO TOTAL:" sempre em
/// negrito e `success`, como o rótulo fixo do Python.
fn cost_rows(costs: Option<&CostSummary>, palette: &Palette) -> Vec<Row> {
    let values: [String; 5] = match costs {
        None => std::array::from_fn(|_| COST_PLACEHOLDER.to_string()),
        Some(c) => [
            fmt_money(&c.currency, c.consumiveis),
            fmt_money(&c.currency, c.operacional),
            fmt_money(&c.currency, c.total),
            fmt_kg(c.powder_kg),
            fmt_m3(c.gas_m3),
        ],
    };
    COST_LABELS
        .into_iter()
        .zip(values)
        .enumerate()
        .map(|(i, (label, value))| {
            let row = Row::new(label, value);
            if i == TOTAL_IDX {
                row.colored(Some(palette.success)).bold()
            } else {
                row
            }
        })
        .collect()
}

/// `f"{symbol} {valor:.2f}"` do Python.
fn fmt_money(symbol: &str, value: f64) -> String {
    format!("{symbol} {value:.2}")
}

/// `f"{kg:.3f} kg"` do Python.
fn fmt_kg(kg: f64) -> String {
    format!("{kg:.3} kg")
}

/// `f"{m3:.3f} m³"` do Python.
fn fmt_m3(m3: f64) -> String {
    format!("{m3:.3} m³")
}

/// Cor do nível de uniformidade: `Boa` = accent, `Atencao` = warning,
/// `Critica` = danger (azul, âmbar e vermelho no Python).
fn uniformity_color(level: Uniformity, palette: &Palette) -> Color32 {
    match level {
        Uniformity::Boa => palette.accent,
        Uniformity::Atencao => palette.warning,
        Uniformity::Critica => palette.danger,
    }
}

/// Linha de aviso como o Python a escrevia: `HH:MM:SS [TIPO] texto`
/// (o prefixo é [`NotifyKind::label`], em maiúsculas).
fn notification_line(n: &Notification) -> String {
    format!("{} [{}] {}", n.time, n.kind.label(), n.text)
}

/// Coluna de indicadores: título em negrito e as linhas rótulo/valor numa
/// área rolável (só rola quando o painel fica mais baixo que o conteúdo).
fn kpi_column(ui: &mut Ui, title: &str, id_salt: &str, rows: &[Row]) {
    ui.strong(title);
    ScrollArea::vertical()
        .id_salt(id_salt)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            compact_rows(ui);
            for row in rows {
                draw_row(ui, row);
            }
        });
}

/// Linhas mais baixas e juntas que o padrão do egui.
fn compact_rows(ui: &mut Ui) {
    let spacing = ui.spacing_mut();
    spacing.interact_size.y = ROW_H;
    spacing.item_spacing.y = ROW_GAP;
}

/// Rótulo à esquerda e valor monoespaçado à direita (como `widgets::kpi`,
/// com cor = negrito). Em coluna estreita o valor é truncado com "…" e o
/// texto completo aparece ao passar o mouse, em vez de invadir o rótulo.
fn draw_row(ui: &mut Ui, row: &Row) {
    let mut label = RichText::new(row.label);
    let mut value = RichText::new(&row.value).monospace();
    if row.bold {
        label = label.strong();
        value = value.strong();
    }
    if let Some(color) = row.color {
        value = value.color(color).strong();
    }
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(Label::new(value).truncate());
        });
    });
}

/// Coluna "Avisos": título com o botão "Limpar" à direita e o histórico
/// numa área rolável presa ao fim (o aviso mais novo fica visível), com o
/// motivo da falha de geração em `danger` no topo.
fn warnings_column(
    ui: &mut Ui,
    notifications: &mut Vec<Notification>,
    gen_error: Option<&str>,
    palette: &Palette,
) {
    ui.horizontal(|ui| {
        ui.strong("Avisos");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let clear = Button::new("Limpar").small();
            if ui
                .add_enabled(!notifications.is_empty(), clear)
                .on_hover_text("Limpar o histórico de avisos")
                .clicked()
            {
                notifications.clear();
            }
        });
    });
    ScrollArea::vertical()
        .id_salt("avisos")
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = ROW_GAP;
            // Monoespaçado em tamanho normal (`TextStyle::Monospace`): o
            // tamanho "Small" ficava ilegível nas capturas de tela.
            if let Some(err) = gen_error {
                let text = RichText::new(format!("Geração do G-code: {err}"))
                    .monospace()
                    .strong()
                    .color(palette.danger);
                ui.label(text);
            }
            if notifications.is_empty() && gen_error.is_none() {
                ui.weak("Nenhum aviso.");
            }
            for n in notifications.iter() {
                let text = RichText::new(notification_line(n))
                    .monospace()
                    .color(n.kind.color(palette));
                ui.label(text);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::NotifyKind;
    use crate::compute::empty_estimates;

    fn palette() -> Palette {
        Palette::light()
    }

    #[test]
    fn placeholders_match_limpar_resultados() {
        let p = palette();
        let results = result_rows(None, &p);
        assert_eq!(results.len(), 9);
        assert!(results
            .iter()
            .all(|r| r.value == "0.00" && r.color.is_none() && !r.bold));
        let costs = cost_rows(None, &p);
        assert_eq!(costs.len(), 5);
        assert!(costs.iter().all(|r| r.value == "..."));
        // "CUSTO TOTAL:" mantém o destaque fixo do Python mesmo em branco.
        assert!(costs[TOTAL_IDX].bold);
        assert_eq!(costs[TOTAL_IDX].color, Some(p.success));
        assert!(costs
            .iter()
            .enumerate()
            .all(|(i, r)| i == TOTAL_IDX || !r.bold));
    }

    #[test]
    fn result_rows_pair_python_labels_with_fields() {
        let p = palette();
        let mut est = empty_estimates();
        est.rotation = ResultText {
            text: "0.123".into(),
            over_limit: true,
        };
        est.pitch = ResultText {
            text: "2.500 (axial)".into(),
            over_limit: false,
        };
        est.rotations = "40".into();
        est.total_angle = "14400.00".into();
        est.time_hhmm = "01:05".into();
        est.overlap.effective_bead_width = "6.00".into();
        est.overlap.surface_step = "2.50".into();
        est.overlap.coverage = "100.0% | curva 0.10 mm".into();
        est.overlap.uniformity = "Atencao | var 12.0%".into();
        est.overlap.uniformity_level = Uniformity::Atencao;

        let rows = result_rows(Some(&est), &p);
        let pairs: Vec<(&str, &str)> = rows.iter().map(|r| (r.label, r.value.as_str())).collect();
        assert_eq!(
            pairs,
            [
                ("Rotação (min/volta):", "0.123"),
                ("Passo Axial (mm):", "2.500 (axial)"),
                ("Rotações/Passos:", "40"),
                ("Ângulo Total/Passo (A):", "14400.00"),
                ("Tempo Total (hh:mm):", "01:05"),
                ("Largura Efetiva (mm):", "6.00"),
                ("Passo Real (mm):", "2.50"),
                ("Cobertura Estimada:", "100.0% | curva 0.10 mm"),
                ("Uniformidade:", "Atencao | var 12.0%"),
            ]
        );
        assert_eq!(rows[0].color, Some(p.danger), "rotação acima do limite");
        assert_eq!(rows[1].color, None, "passo dentro do limite");
        assert_eq!(rows[8].color, Some(p.warning), "uniformidade Atencao");
        assert!(rows.iter().all(|r| !r.bold));
    }

    #[test]
    fn uniformity_colors_follow_level() {
        let p = palette();
        assert_eq!(uniformity_color(Uniformity::Boa, &p), p.accent);
        assert_eq!(uniformity_color(Uniformity::Atencao, &p), p.warning);
        assert_eq!(uniformity_color(Uniformity::Critica, &p), p.danger);
    }

    #[test]
    fn cost_formats_match_python() {
        assert_eq!(fmt_money("R$", 12.5), "R$ 12.50");
        assert_eq!(fmt_money("R$", 1234.567), "R$ 1234.57");
        assert_eq!(fmt_money("$", 0.0), "$ 0.00");
        assert_eq!(fmt_kg(0.5), "0.500 kg");
        assert_eq!(fmt_m3(1.25), "1.250 m³");

        let p = palette();
        let costs = CostSummary {
            consumiveis: 10.0,
            operacional: 20.5,
            total: 30.5,
            powder_kg: 0.5,
            gas_m3: 1.25,
            currency: "R$".into(),
            ..Default::default()
        };
        let rows = cost_rows(Some(&costs), &p);
        let pairs: Vec<(&str, &str)> = rows.iter().map(|r| (r.label, r.value.as_str())).collect();
        assert_eq!(
            pairs,
            [
                ("Custo Consumíveis:", "R$ 10.00"),
                ("Custo Operacional:", "R$ 20.50"),
                ("CUSTO TOTAL:", "R$ 30.50"),
                ("Consumo Pó (kg):", "0.500 kg"),
                ("Consumo Gás (m³):", "1.250 m³"),
            ]
        );
    }

    #[test]
    fn notification_line_uses_python_tags() {
        let n = Notification {
            time: "12:34:56".into(),
            kind: NotifyKind::Aviso,
            text: "RPM acima do limite".into(),
        };
        assert_eq!(
            notification_line(&n),
            "12:34:56 [AVISO] RPM acima do limite"
        );
        assert_eq!(NotifyKind::Info.label(), "INFO");
        assert_eq!(NotifyKind::Sucesso.label(), "SUCESSO");
        assert_eq!(NotifyKind::Aviso.label(), "AVISO");
        assert_eq!(NotifyKind::Erro.label(), "ERRO");
    }
}
