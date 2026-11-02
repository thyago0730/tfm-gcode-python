//! Widgets reutilizáveis (campos com rótulo e tooltip, seções, KPIs).
//!
//! Todos os campos seguem a mesma linha: rótulo à esquerda (com tooltip) e o
//! controle alinhado à direita (com o mesmo tooltip). As funções de campo
//! devolvem `true` quando o valor mudou neste quadro.

use egui::{
    Align, CollapsingHeader, Color32, ComboBox, Layout, Response, RichText, Stroke, StrokeKind,
    TextEdit, Ui,
};

use crate::theme::Palette;

/// Largura (px) dos campos numéricos, igual em todos os painéis.
pub const NUM_FIELD_WIDTH: f32 = 90.0;
/// Espaço que a seta ocupa além da largura pedida a um combo.
const COMBO_ARROW_W: f32 = 16.0;

/// Converte o texto de um campo numérico em `f64`.
///
/// Aceita vírgula **ou** ponto como separador decimal (nunca ambos), espaços
/// nas pontas, sinal e notação científica (`1e-3`). Vazio, texto inválido e
/// resultados não finitos (`inf`, `nan`, `1e999`) devolvem `None`.
pub fn parse_num(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() || (s.contains(',') && s.contains('.')) {
        return None;
    }
    let value: f64 = s.replace(',', ".").parse().ok()?;
    value.is_finite().then_some(value)
}

/// Rótulo + [`TextEdit`] de uma linha com [`NUM_FIELD_WIDTH`] alinhado à
/// direita. Com `invalid`, o campo ganha borda e fundo na cor `danger` do tema.
/// Devolve `true` se o texto mudou.
pub fn num_field(ui: &mut Ui, label: &str, value: &mut String, tip: &str, invalid: bool) -> bool {
    field_row(ui, label, tip, |ui| {
        let palette = Palette::resolve(ui.ctx());
        let mut edit = TextEdit::singleline(value).desired_width(NUM_FIELD_WIDTH);
        if invalid {
            edit = edit.background_color(invalid_tint(palette.danger));
        }
        let response = edit.show(ui).response.response;
        if invalid {
            ui.painter().rect_stroke(
                response.rect,
                ui.visuals().widgets.inactive.corner_radius,
                Stroke::new(1.5, palette.danger),
                StrokeKind::Inside,
            );
        }
        let changed = response.changed();
        let field_tip = if invalid {
            format!("{tip}\nValor numérico inválido.")
        } else {
            tip.to_owned()
        };
        with_tip(response, field_tip.trim());
        changed
    })
}

/// Rótulo + campo de texto livre que ocupa o resto da linha.
/// Devolve `true` se o texto mudou.
pub fn text_field(ui: &mut Ui, label: &str, value: &mut String, tip: &str) -> bool {
    field_row(ui, label, tip, |ui| {
        let response = TextEdit::singleline(value)
            .desired_width(f32::INFINITY)
            .show(ui)
            .response
            .response;
        let changed = response.changed();
        with_tip(response, tip);
        changed
    })
}

/// Rótulo + caixa de seleção alinhada à direita. Devolve `true` se alternou.
pub fn check_field(ui: &mut Ui, label: &str, value: &mut bool, tip: &str) -> bool {
    field_row(ui, label, tip, |ui| {
        let response = ui.checkbox(value, "");
        let changed = response.changed();
        with_tip(response, tip);
        changed
    })
}

/// Rótulo + combo com as opções `(valor, rótulo em português)`.
/// Devolve `true` se outra opção foi escolhida.
pub fn combo_field<T: PartialEq + Copy>(
    ui: &mut Ui,
    label: &str,
    value: &mut T,
    options: &[(T, &str)],
    tip: &str,
) -> bool {
    field_row(ui, label, tip, |ui| {
        let current = options
            .iter()
            .find(|(option, _)| *option == *value)
            .map_or("—", |(_, name)| *name);
        let mut changed = false;
        let response = ComboBox::from_id_salt(label)
            // A seta do combo soma à largura pedida: sem a folga, ele invade o
            // rótulo.
            .width((ui.available_width() - COMBO_ARROW_W).max(NUM_FIELD_WIDTH))
            .selected_text(current)
            .show_ui(ui, |ui| {
                for (option, name) in options {
                    changed |= ui.selectable_value(value, *option, *name).changed();
                }
            })
            .response;
        with_tip(response, tip);
        changed
    })
}

/// [`CollapsingHeader`] com o título em negrito.
pub fn section(ui: &mut Ui, title: &str, default_open: bool, add: impl FnOnce(&mut Ui)) {
    CollapsingHeader::new(RichText::new(title).strong())
        .default_open(default_open)
        .show(ui, add);
}

/// Linha de indicador: rótulo em peso normal e valor em monoespaçado,
/// alinhado à direita; com `color` o valor sai colorido e em negrito.
pub fn kpi(ui: &mut Ui, label: &str, value: &str, color: Option<Color32>) {
    let mut text = RichText::new(value).monospace();
    if let Some(color) = color {
        text = text.color(color).strong();
    }
    kpi_line(ui, RichText::new(label), text);
}

/// Variante de [`kpi`] com negrito explícito no rótulo **e** no valor
/// (usada em totais, como "CUSTO TOTAL").
pub fn kpi_row(ui: &mut Ui, label: &str, value: &str, color: Option<Color32>, bold: bool) {
    let mut name = RichText::new(label);
    let mut text = RichText::new(value).monospace();
    if let Some(color) = color {
        text = text.color(color);
    }
    if bold {
        name = name.strong();
        text = text.strong();
    }
    kpi_line(ui, name, text);
}

fn kpi_line(ui: &mut Ui, label: RichText, value: RichText) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(value);
        });
    });
}

/// Linha padrão dos campos: rótulo (com tooltip) à esquerda e `add_field`
/// disposto da direita para a esquerda no espaço restante.
fn field_row<R>(ui: &mut Ui, label: &str, tip: &str, add_field: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        with_tip(ui.label(label), tip);
        ui.with_layout(Layout::right_to_left(Align::Center), add_field)
            .inner
    })
    .inner
}

/// Aplica o tooltip só quando há texto (evita balão vazio).
fn with_tip(response: Response, tip: &str) -> Response {
    if tip.is_empty() {
        response
    } else {
        response.on_hover_text(tip)
    }
}

/// Fundo translúcido na cor `danger`: visível sobre o fundo claro e o escuro.
fn invalid_tint(danger: Color32) -> Color32 {
    danger.gamma_multiply(0.14)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_num_accepts_comma() {
        assert_eq!(parse_num("12,5"), Some(12.5));
        assert_eq!(parse_num("12.5"), Some(12.5));
        assert_eq!(parse_num(" 7 "), Some(7.0));
        assert_eq!(parse_num(""), None);
        assert_eq!(parse_num("   "), None);
        assert_eq!(parse_num("abc"), None);
        assert_eq!(parse_num("1e-3"), Some(0.001));
        assert_eq!(parse_num("-2,5"), Some(-2.5));
        assert_eq!(parse_num("+3"), Some(3.0));
        assert_eq!(parse_num("1,5e3"), Some(1500.0));
    }

    #[test]
    fn parse_num_rejects_non_finite() {
        assert_eq!(parse_num("inf"), None);
        assert_eq!(parse_num("-inf"), None);
        assert_eq!(parse_num("infinity"), None);
        assert_eq!(parse_num("nan"), None);
        assert_eq!(parse_num("NaN"), None);
        assert_eq!(parse_num("1e999"), None);
    }

    #[test]
    fn parse_num_rejects_mixed_separators() {
        assert_eq!(parse_num("1.234,5"), None);
        assert_eq!(parse_num("1,234.5"), None);
        assert_eq!(parse_num("1,,5"), None);
        assert_eq!(parse_num("1,5,"), None);
    }

    #[test]
    fn invalid_tint_is_translucent_danger() {
        let tint = invalid_tint(Color32::from_rgb(0xdc, 0x26, 0x26));
        assert!(tint.a() > 0 && tint.a() < 128, "alpha {}", tint.a());
    }
}
