//! Painel direito: visualizador de G-code.
//!
//! Lista virtualizada ([`egui::ScrollArea::show_rows`]) com fonte
//! monoespaçada e cor por tipo de linha, busca sem distinção de maiúsculas
//! com navegação ▲/▼, filtros por eixo, LED do estado da tocha (M101/M102)
//! na linha selecionada e o botão "Copiar". O que é
//! caro para programas grandes (filtrar, buscar, estado da tocha) fica em
//! cache e só é refeito quando o resultado, os filtros ou a busca mudam.

use std::borrow::Cow;

use egui::{
    pos2, vec2, Align, Align2, Color32, Key, Layout, Rect, RichText, Sense, TextEdit, TextStyle, Ui,
};

use crate::app::{NotifyKind, TfmApp};
use crate::compute::ComputeResult;
use crate::theme::Palette;

/// Raio do LED da tocha (círculo de 10 px).
const LED_RADIUS: f32 = 5.0;
/// Largura da barra que marca a linha selecionada.
const SELECT_BAR_W: f32 = 3.0;
/// Espaço vertical entre linhas da lista (menor que o padrão do egui).
const ROW_GAP: f32 = 2.0;

/// Tipo de uma linha de G-code, para a cor no visualizador.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Empty,
    /// Começa com `(` ou `;`.
    Comment,
    /// `G0`/`G00`.
    Rapid,
    /// `G1`/`G01`.
    Linear,
    /// `M101` (liga a tocha).
    TorchOn,
    /// `M102` (desliga a tocha).
    TorchOff,
    /// `M0`/`M00`/`M1`/`M01` (parada).
    Pause,
    /// `G53` (coordenadas de máquina).
    Machine,
    Other,
}

/// Estado do visualizador, persistente entre quadros.
#[derive(Clone, Debug, Default)]
pub struct GcodeView {
    pub search: String,
    pub show_x: bool,
    pub show_y: bool,
    pub show_z: bool,
    pub show_a: bool,
    /// Linha selecionada (índice na lista completa).
    pub selected_line: Option<usize>,
    /// Rolar até esta linha no próximo quadro (índice na lista completa).
    pub scroll_to: Option<usize>,
    /// Linhas visíveis que casam com a busca, em ordem crescente.
    pub matches: Vec<usize>,
    /// Posição em `matches` da ocorrência "atual" da navegação ▲/▼.
    pub match_idx: usize,
    /// Linhas que passam pelos filtros de eixo, em ordem crescente.
    visible: Vec<usize>,
    /// O que gerou `visible` e `matches`.
    cache_sig: Option<CacheSig>,
    /// (chave do resultado, linha, tocha ligada) do último LED calculado.
    torch_cache: Option<(u64, usize, bool)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CacheSig {
    key: u64,
    axes: [bool; 4],
    search: String,
}

impl GcodeView {
    fn axes(&self) -> [bool; 4] {
        [self.show_x, self.show_y, self.show_z, self.show_a]
    }

    /// Linhas visíveis após os filtros (índices na lista completa).
    pub fn visible(&self) -> &[usize] {
        &self.visible
    }

    /// Refaz `visible` e `matches` se o resultado, os filtros ou a busca
    /// mudaram; senão não custa nada.
    fn refresh(&mut self, result: &ComputeResult) {
        let sig = CacheSig {
            key: result.key,
            axes: self.axes(),
            search: self.search.trim().to_string(),
        };
        if self.cache_sig.as_ref() == Some(&sig) {
            return;
        }
        let lines = &result.lines;
        self.visible = (0..lines.len())
            .filter(|&i| passes_filters(&lines[i], sig.axes))
            .collect();
        let needle = sig.search.to_lowercase();
        self.matches = if needle.is_empty() {
            Vec::new()
        } else {
            self.visible
                .iter()
                .copied()
                .filter(|&i| contains_ci(&lines[i], &needle))
                .collect()
        };
        if self.match_idx >= self.matches.len() {
            self.match_idx = 0;
        }
        if self.selected_line.is_some_and(|s| s >= lines.len()) {
            self.selected_line = None;
        }
        self.cache_sig = Some(sig);
    }

    /// Vai à próxima (`+1`) ou à anterior (`-1`) ocorrência da busca. Se a
    /// ocorrência atual não é a linha selecionada (o usuário clicou em outra
    /// linha), primeiro volta a ela.
    fn step_match(&mut self, step: i64) {
        let n = self.matches.len();
        if n == 0 {
            return;
        }
        let current = self.matches.get(self.match_idx).copied();
        if current.is_some() && current == self.selected_line {
            self.match_idx = (self.match_idx as i64 + step).rem_euclid(n as i64) as usize;
        } else {
            self.match_idx = self.match_idx.min(n - 1);
        }
        if let Some(&line) = self.matches.get(self.match_idx) {
            self.select(line);
        }
    }

    /// Seleciona uma linha e rola até ela no próximo quadro.
    pub fn select(&mut self, line: usize) {
        self.selected_line = Some(line);
        self.scroll_to = Some(line);
    }

    /// Estado da tocha na linha selecionada (ou na última linha, sem seleção).
    fn torch_on(&mut self, result: &ComputeResult) -> bool {
        let Some(last) = result.lines.len().checked_sub(1) else {
            return false;
        };
        let at = self.selected_line.unwrap_or(last).min(last);
        if let Some((key, line, on)) = self.torch_cache {
            if key == result.key && line == at {
                return on;
            }
        }
        let on = torch_state_at(&result.lines, at);
        self.torch_cache = Some((result.key, at, on));
        on
    }
}

// ---------------------------------------------------------------------------
// Análise de linhas (funções puras)
// ---------------------------------------------------------------------------

/// Parte "código" da linha: sem comentários entre parênteses nem após `;`.
fn code_part(line: &str) -> Cow<'_, str> {
    if !line.contains('(') && !line.contains(';') {
        return Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len());
    let mut depth = 0usize;
    for ch in line.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ';' if depth == 0 => break,
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    Cow::Owned(out)
}

fn is_any(token: &str, names: &[&str]) -> bool {
    names.iter().any(|name| token.eq_ignore_ascii_case(name))
}

/// Classifica a linha para a cor. M101/M102/M0 mandam sobre o resto; G53
/// manda sobre G0/G1 na mesma linha.
pub fn classify(line: &str) -> LineKind {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return LineKind::Empty;
    }
    if trimmed.starts_with('(') || trimmed.starts_with(';') {
        return LineKind::Comment;
    }
    let code = code_part(line);
    let mut kind = LineKind::Other;
    for token in code.split_whitespace() {
        if is_any(token, &["M101"]) {
            return LineKind::TorchOn;
        }
        if is_any(token, &["M102"]) {
            return LineKind::TorchOff;
        }
        if is_any(token, &["M0", "M00", "M1", "M01"]) {
            return LineKind::Pause;
        }
        if is_any(token, &["G53"]) {
            kind = LineKind::Machine;
        } else if kind == LineKind::Other && is_any(token, &["G0", "G00"]) {
            kind = LineKind::Rapid;
        } else if kind == LineKind::Other && is_any(token, &["G1", "G01"]) {
            kind = LineKind::Linear;
        }
    }
    kind
}

/// Linha de movimento: tem `G0`/`G00`/`G1`/`G01` fora de comentários.
fn is_motion(code: &str) -> bool {
    code.split_whitespace()
        .any(|token| is_any(token, &["G0", "G00", "G1", "G01"]))
}

/// A linha tem a palavra do eixo (`X10`, `a-2.5`...) fora de comentários.
fn has_axis(code: &str, axis: u8) -> bool {
    code.split_whitespace().any(|token| {
        let b = token.as_bytes();
        b.len() >= 2
            && b[0].eq_ignore_ascii_case(&axis)
            && matches!(b[1], b'0'..=b'9' | b'-' | b'+' | b'.')
    })
}

/// Filtros por eixo (`[X, Y, Z, A]`): nenhum marcado mostra tudo; com algum
/// marcado, as linhas de movimento só aparecem se tiverem ao menos um dos
/// eixos marcados. Comentários, M-codes e o resto continuam visíveis.
pub fn passes_filters(line: &str, axes: [bool; 4]) -> bool {
    if !axes.iter().any(|on| *on) {
        return true;
    }
    let code = code_part(line);
    if !is_motion(&code) {
        return true;
    }
    const NAMES: [u8; 4] = [b'X', b'Y', b'Z', b'A'];
    axes.iter()
        .zip(NAMES)
        .any(|(on, axis)| *on && has_axis(&code, axis))
}

/// Busca sem distinção de maiúsculas; `needle_lower` já em minúsculas.
pub fn contains_ci(hay: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return false;
    }
    if hay.is_ascii() && needle_lower.is_ascii() {
        let needle = needle_lower.as_bytes();
        hay.as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle))
    } else {
        hay.to_lowercase().contains(needle_lower)
    }
}

/// Estado da tocha logo após a linha `at`: o último M101/M102 até ali.
pub fn torch_state_at(lines: &[String], at: usize) -> bool {
    let mut on = false;
    for line in lines.iter().take(at.saturating_add(1)) {
        match classify(line) {
            LineKind::TorchOn => on = true,
            LineKind::TorchOff => on = false,
            _ => {}
        }
    }
    on
}

/// Cor (e se vai em negrito simulado) de cada tipo de linha.
fn line_color(kind: LineKind, palette: &Palette, ui: &Ui) -> (Color32, bool) {
    match kind {
        LineKind::Empty | LineKind::Comment => (palette.muted, false),
        LineKind::Rapid => (palette.accent, false),
        LineKind::Linear => (palette.success, false),
        LineKind::TorchOn => (palette.success, true),
        LineKind::TorchOff => (palette.danger, false),
        LineKind::Pause | LineKind::Machine => (palette.warning, false),
        LineKind::Other => (ui.visuals().text_color(), false),
    }
}

// ---------------------------------------------------------------------------
// Painel
// ---------------------------------------------------------------------------

/// Ações que precisam do `TfmApp` inteiro (avisos), executadas depois que
/// os empréstimos do painel terminam.
enum Action {
    CopyAll,
    Collapse,
}

/// Desenha o painel direito.
pub fn show_right(app: &mut TfmApp, ui: &mut Ui) {
    let palette = app.palette;
    let busy = app.compute.busy();
    let action = {
        let TfmApp {
            result,
            gcode_view: view,
            ..
        } = app;
        view_ui(ui, view, result.as_ref(), busy, &palette)
    };
    match action {
        Some(Action::CopyAll) => copy_gcode(app, ui.ctx()),
        Some(Action::Collapse) => app.set_right_pinned(false),
        None => {}
    }
}

/// Copia o G-code inteiro para a área de transferência (botão e menu
/// "G-code > Copiar tudo").
pub fn copy_gcode(app: &mut TfmApp, ctx: &egui::Context) {
    match app.result.as_ref().filter(|r| !r.lines.is_empty()) {
        Some(result) => {
            ctx.copy_text(result.lines.join("\n"));
            app.notify(NotifyKind::Sucesso, "G-code copiado");
        }
        None => app.notify(NotifyKind::Aviso, "Não há G-code para copiar"),
    }
}

/// Cabeçalho, busca, filtros, LED, botões e a lista (ou o erro).
fn view_ui(
    ui: &mut Ui,
    view: &mut GcodeView,
    result: Option<&ComputeResult>,
    busy: bool,
    palette: &Palette,
) -> Option<Action> {
    let mut action = None;
    if let Some(r) = result {
        view.refresh(r);
    }
    let total = result.map_or(0, |r| r.lines.len());

    // Título e contagem de linhas.
    ui.horizontal(|ui| {
        ui.strong("G-code");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .small_button("›")
                .on_hover_text("Recolher o G-code")
                .clicked()
            {
                action = Some(Action::Collapse);
            }
            if ui
                .add_enabled(total > 0, egui::Button::new("Copiar").small())
                .on_hover_text("Copiar o G-code inteiro para a área de transferência")
                .clicked()
            {
                action = Some(Action::CopyAll);
            }
            if busy {
                ui.spinner();
                ui.weak("calculando…");
            }
            let count = match result {
                None => "Linhas: -".to_string(),
                Some(_) if view.visible.len() != total => {
                    format!("Linhas: {total} ({} visíveis)", view.visible.len())
                }
                Some(_) => format!("Linhas: {total}"),
            };
            ui.label(count);
        });
    });

    // Busca com navegação ▲/▼ (Enter = próxima).
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let has_matches = !view.matches.is_empty();
            if ui
                .add_enabled(has_matches, egui::Button::new("▼"))
                .on_hover_text("Próxima ocorrência (Enter)")
                .clicked()
            {
                view.step_match(1);
            }
            if ui
                .add_enabled(has_matches, egui::Button::new("▲"))
                .on_hover_text("Ocorrência anterior")
                .clicked()
            {
                view.step_match(-1);
            }
            if !view.search.trim().is_empty() {
                let text = if has_matches {
                    format!("{}/{}", view.match_idx + 1, view.matches.len())
                } else {
                    "0/0".to_string()
                };
                ui.weak(text);
            }
            let response = ui.add(
                TextEdit::singleline(&mut view.search)
                    .hint_text("Buscar no G-code")
                    .desired_width(ui.available_width()),
            );
            if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                view.step_match(1);
                response.request_focus();
            }
        });
    });

    // Filtros por eixo e LED da tocha.
    ui.horizontal(|ui| {
        ui.label("Eixos:");
        let hint = "Mostrar só movimentos com este eixo (nenhum marcado = tudo)";
        ui.toggle_value(&mut view.show_x, "X").on_hover_text(hint);
        ui.toggle_value(&mut view.show_y, "Y").on_hover_text(hint);
        ui.toggle_value(&mut view.show_z, "Z").on_hover_text(hint);
        ui.toggle_value(&mut view.show_a, "A").on_hover_text(hint);
        if let Some(r) = result {
            view.refresh(r);
            let on = view.torch_on(r);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (text, color) = if on {
                    ("Tocha ligada", palette.success)
                } else {
                    ("Tocha desligada", palette.muted)
                };
                ui.label(RichText::new(text).color(color));
                let size = vec2(LED_RADIUS * 2.0 + 2.0, LED_RADIUS * 2.0 + 2.0);
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                ui.painter().circle_filled(rect.center(), LED_RADIUS, color);
            });
        }
    });

    ui.separator();

    match result {
        None => {
            let text = if busy {
                "Calculando o G-code…"
            } else {
                "Nenhum G-code gerado ainda"
            };
            ui.centered_and_justified(|ui| ui.weak(text));
        }
        Some(r) => match &r.gen_error {
            Some(err) => show_error(ui, r, err, palette),
            None if r.lines.is_empty() => {
                ui.centered_and_justified(|ui| ui.weak("Nenhuma linha gerada"));
            }
            None => show_lines(ui, view, r, palette),
        },
    }
    action
}

/// Erro de geração em vermelho e, se houver, a lista de regras violadas
/// (sem repetir o texto quando o erro é exatamente essa lista).
fn show_error(ui: &mut Ui, result: &ComputeResult, err: &str, palette: &Palette) {
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.label(
                RichText::new("Não foi possível gerar o G-code")
                    .strong()
                    .color(palette.danger),
            );
            let errors = &result.field_errors.errors;
            if errors.is_empty() || errors.join("\n") != err {
                ui.label(RichText::new(err).color(palette.danger));
            }
            if !errors.is_empty() {
                ui.add_space(4.0);
                ui.strong("Corrija os campos:");
                for message in errors {
                    ui.label(RichText::new(format!("• {message}")).color(palette.danger));
                }
            }
        });
}

/// Lista virtualizada: só as linhas no viewport são desenhadas. Cada linha
/// tem número (cinza), texto monoespaçado colorido pelo tipo, fundo de
/// seleção/ocorrência e clique para selecionar.
fn show_lines(ui: &mut Ui, view: &mut GcodeView, result: &ComputeResult, palette: &Palette) {
    let lines = &result.lines;
    let font = TextStyle::Monospace.resolve(ui.style());
    let row_h = ui.text_style_height(&TextStyle::Monospace);
    let char_w = ui
        .painter()
        .layout_no_wrap("0".to_string(), font.clone(), Color32::WHITE)
        .size()
        .x
        .max(1.0);
    let digits = lines.len().to_string().len();
    let gutter_w = char_w * digits as f32 + 12.0;
    let has_matches = !view.matches.is_empty();

    let mut clicked: Option<usize> = None;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        let mut scroll = egui::ScrollArea::vertical().auto_shrink(false);
        // Rolagem pedida (busca/seleção): converte a linha em posição na
        // lista visível e centraliza-a, uma vez só.
        if let Some(target) = view.scroll_to.take() {
            let row = match view.visible.binary_search(&target) {
                Ok(i) | Err(i) => i,
            };
            let offset = (row as f32 * (row_h + ROW_GAP) - ui.available_height() * 0.4).max(0.0);
            scroll = scroll.vertical_scroll_offset(offset);
        }
        scroll.show_rows(ui, row_h, view.visible.len(), |ui, range| {
            for row in range {
                let Some(&idx) = view.visible.get(row) else {
                    break;
                };
                let Some(line) = lines.get(idx) else {
                    break;
                };
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                let selected = view.selected_line == Some(idx);
                let is_match = has_matches && view.matches.binary_search(&idx).is_ok();
                let painter = ui.painter();
                if selected {
                    painter.rect_filled(rect, 2.0, palette.accent.gamma_multiply(0.22));
                    let bar = Rect::from_min_size(rect.min, vec2(SELECT_BAR_W, rect.height()));
                    painter.rect_filled(bar, 0.0, palette.accent);
                } else if is_match {
                    painter.rect_filled(rect, 2.0, palette.warning.gamma_multiply(0.22));
                } else if response.hovered() {
                    painter.rect_filled(rect, 2.0, ui.visuals().widgets.hovered.weak_bg_fill);
                }
                painter.text(
                    pos2(rect.left() + gutter_w - 6.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    idx + 1,
                    font.clone(),
                    palette.muted,
                );
                let (color, bold) = line_color(classify(line), palette, ui);
                let galley = painter.layout_no_wrap(line.clone(), font.clone(), color);
                let text_pos = pos2(
                    rect.left() + gutter_w,
                    rect.center().y - galley.size().y * 0.5,
                );
                if bold {
                    // Negrito simulado: a fonte monoespaçada não tem variante.
                    painter.galley(text_pos + vec2(0.6, 0.0), galley.clone(), color);
                }
                let overflow = galley.size().x > rect.width() - gutter_w;
                painter.galley(text_pos, galley, color);
                if response.clicked() {
                    clicked = Some(idx);
                }
                if overflow {
                    response.on_hover_text(RichText::new(line).monospace());
                }
            }
        });
    });
    if let Some(idx) = clicked {
        view.selected_line = Some(idx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn classify_follows_color_rules() {
        assert_eq!(classify("(comentário)"), LineKind::Comment);
        assert_eq!(classify("   "), LineKind::Empty);
        assert_eq!(classify("G00 X0 Z5"), LineKind::Rapid);
        assert_eq!(classify("g0 x0"), LineKind::Rapid);
        assert_eq!(classify("G01 X10 A360 F500"), LineKind::Linear);
        assert_eq!(classify("G1 X10"), LineKind::Linear);
        assert_eq!(classify("M101"), LineKind::TorchOn);
        assert_eq!(classify("M102 (desliga)"), LineKind::TorchOff);
        assert_eq!(classify("M0"), LineKind::Pause);
        assert_eq!(classify("M01"), LineKind::Pause);
        assert_eq!(classify("G53 G0 Z0"), LineKind::Machine);
        assert_eq!(classify("G1 X5 (M101 no comentário)"), LineKind::Linear);
        assert_eq!(classify("F500"), LineKind::Other);
    }

    #[test]
    fn axis_filters_hide_only_motion_lines_without_the_axis() {
        let none = [false; 4];
        let only_a = [false, false, false, true];
        let x_or_a = [true, false, false, true];
        assert!(passes_filters("G1 X10 F500", none));
        assert!(!passes_filters("G1 X10 F500", only_a));
        assert!(passes_filters("G1 X10 A90", only_a));
        assert!(passes_filters("G0 Z5", none));
        assert!(!passes_filters("G0 Z5", x_or_a));
        assert!(passes_filters("G0 x5", x_or_a));
        assert!(passes_filters("(comentário com A)", only_a));
        assert!(passes_filters("M101", only_a));
        assert!(passes_filters("G1 F500 (A360)", none));
        assert!(!passes_filters("G1 F500 (A360)", only_a));
    }

    #[test]
    fn search_is_case_insensitive() {
        assert!(contains_ci("G01 X10 F500", "x10"));
        assert!(contains_ci("(Início da camada)", "início"));
        assert!(!contains_ci("G01 X10", ""));
        assert!(!contains_ci("G01", "G01 X"));
    }

    #[test]
    fn torch_state_follows_last_m101_m102() {
        let lines = s(&["G0 X0", "M101", "G1 X10", "M102", "G0 X0", "(M101)"]);
        assert!(!torch_state_at(&lines, 0));
        assert!(torch_state_at(&lines, 1));
        assert!(torch_state_at(&lines, 2));
        assert!(!torch_state_at(&lines, 3));
        assert!(!torch_state_at(&lines, 5));
        assert!(!torch_state_at(&[], 0));
    }

    #[test]
    fn step_match_cycles_through_matches() {
        let mut view = GcodeView {
            matches: vec![2, 5, 9],
            ..GcodeView::default()
        };
        view.step_match(1);
        assert_eq!(view.selected_line, Some(2));
        view.step_match(1);
        assert_eq!(view.selected_line, Some(5));
        view.step_match(-1);
        assert_eq!(view.selected_line, Some(2));
        view.step_match(-1);
        assert_eq!((view.selected_line, view.match_idx), (Some(9), 2));
        // Clicou em outra linha: ▼ volta à ocorrência atual antes de avançar.
        view.selected_line = Some(0);
        view.step_match(1);
        assert_eq!(view.selected_line, Some(9));
        assert_eq!(view.scroll_to, Some(9));
    }
}
