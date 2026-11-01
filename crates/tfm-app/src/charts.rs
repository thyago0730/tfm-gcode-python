//! Gráficos do painel central: Análise Temporal, Oscilação, Estatísticas e o
//! dashboard de Processo (porte de `src/app/ui/charts.py`).
//!
//! As três primeiras abas usam `egui_plot`; o dashboard é desenhado com
//! [`egui::Painter`] em uma grade 2×2 (rosca de custos | barras de consumo /
//! esquema de camadas | cartão de tempo). Os ajudantes puros (binning do
//! histograma, acumulado, amplitude, geometria do esquema) ficam separados
//! do desenho para serem testáveis sem janela.
//!
//! Regras: textos em português com os títulos do Python; cores dos dados pela
//! [`Palette`] (X = `accent`, Z = `success`, A = `warning`); texto sempre pelas
//! cores do tema (`ui.visuals()`), nunca preto/branco fixo; entradas vazias
//! nunca entram em pânico.

use std::f32::consts::{FRAC_PI_2, TAU};

use egui::epaint::{Mesh, TextShape};
use egui::{
    pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Layout, Margin, Pos2, Rect, RichText,
    Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};
use egui_plot::{Bar, BarChart, Corner, Legend, Line, LineStyle, Plot, PlotPoint, Text};
use tfm_core::estimates::{CostSummary, Estimates};
use tfm_core::overlap;
use tfm_core::params::{Params, WeldingMode};
use tfm_core::toolpath::Series;

use crate::theme::Palette;

// ---------------------------------------------------------------------------
// Ajudantes puros (testados sem janela)
// ---------------------------------------------------------------------------

/// Quantidade de bins do histograma: `max(10, min(50, n // 5))` do Python.
pub fn histogram_bins(n: usize) -> usize {
    (n / 5).clamp(10, 50)
}

/// Um bin do histograma: centro e largura em mm, e quantos passos caíram nele.
#[derive(Clone, Debug, PartialEq)]
pub struct HistBin {
    pub center: f64,
    pub width: f64,
    pub count: usize,
}

/// Histograma de largura uniforme sobre `[min, max]`, com o máximo incluído no
/// último bin (como `numpy.histogram`). Valores não finitos são ignorados;
/// valores todos iguais produzem um único bin.
pub fn histogram(values: &[f64], bins: usize) -> Vec<HistBin> {
    let finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() || bins == 0 {
        return Vec::new();
    }
    let min = finite.iter().copied().fold(f64::INFINITY, f64::min);
    let max = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let span = max - min;
    if span <= 0.0 {
        let width = (min.abs() * 0.1).max(1.0);
        return vec![HistBin {
            center: min,
            width,
            count: finite.len(),
        }];
    }
    let width = span / bins as f64;
    let mut counts = vec![0usize; bins];
    for v in finite {
        let idx = ((v - min) / width).floor();
        let idx = if idx.is_finite() && idx >= 0.0 {
            (idx as usize).min(bins - 1)
        } else {
            0
        };
        counts[idx] += 1;
    }
    counts
        .into_iter()
        .enumerate()
        .map(|(i, count)| HistBin {
            center: min + (i as f64 + 0.5) * width,
            width,
            count,
        })
        .collect()
}

/// Soma acumulada; não finitos contam como zero.
pub fn cumulative(values: &[f64]) -> Vec<f64> {
    let mut acc = 0.0;
    values
        .iter()
        .map(|v| {
            if v.is_finite() {
                acc += v;
            }
            acc
        })
        .collect()
}

/// Reescala `values` para que o seu máximo vire `target_max` (curva do
/// acumulado desenhada na escala da distância por passo). Máximo ou alvo não
/// positivos devolvem zeros.
pub fn normalize_to(values: &[f64], target_max: f64) -> Vec<f64> {
    let max = values
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(0.0, f64::max);
    if max <= 0.0 || !target_max.is_finite() || target_max <= 0.0 {
        return vec![0.0; values.len()];
    }
    let k = target_max / max;
    values
        .iter()
        .map(|v| if v.is_finite() { v * k } else { 0.0 })
        .collect()
}

/// `(max - min) / 2` dos valores finitos; zero com menos de dois valores.
pub fn amplitude(values: &[f64]) -> f64 {
    let finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.len() < 2 {
        return 0.0;
    }
    let min = finite.iter().copied().fold(f64::INFINITY, f64::min);
    let max = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    (max - min) / 2.0
}

/// Média e máximo dos valores finitos (`None` se não houver nenhum).
pub fn mean_max(values: &[f64]) -> Option<(f64, f64)> {
    let mut n = 0usize;
    let mut sum = 0.0;
    let mut max = f64::NEG_INFINITY;
    for v in values.iter().copied().filter(|v| v.is_finite()) {
        n += 1;
        sum += v;
        max = max.max(v);
    }
    (n > 0).then(|| (sum / n as f64, max))
}

/// Pontos `[passo, valor]` com o passo começando em 1 (eixo "Passo" do
/// Python); valores não finitos ficam de fora.
pub fn step_points(values: &[f64]) -> Vec<[f64; 2]> {
    values
        .iter()
        .enumerate()
        .filter(|(_, v)| v.is_finite())
        .map(|(i, v)| [(i + 1) as f64, *v])
        .collect()
}

/// `"{hh:02}h {mm:02}m"` do cartão de tempo; `None` quando não há tempo
/// (zero, negativo ou não finito). Minutos arredondados que chegam a 60 viram
/// uma hora a mais (o Python mostrava "01h 60m").
pub fn hours_label(time_hours: f64) -> Option<String> {
    if !time_hours.is_finite() || time_hours <= 0.0 {
        return None;
    }
    let total_min = time_hours * 60.0;
    let mut hh = (total_min / 60.0).floor() as u64;
    let mut mm = (total_min % 60.0).round() as u64;
    if mm >= 60 {
        hh += 1;
        mm = 0;
    }
    Some(format!("{hh:02}h {mm:02}m"))
}

/// `Some(v)` só para números finitos.
pub fn finite_or_none(v: f64) -> Option<f64> {
    v.is_finite().then_some(v)
}

/// Formata com `decimals` casas, ou "—" quando o valor está ausente.
pub fn fmt_opt(v: Option<f64>, decimals: usize) -> String {
    match v {
        Some(v) => format!("{v:.decimals$}"),
        None => "—".to_string(),
    }
}

/// Rótulos das quatro fatias da rosca de custos, na ordem de [`cost_parts`].
pub const COST_LABELS: [&str; 4] = ["Pó", "Gás", "Mão de Obra", "Máquina"];

/// Cores das fatias (as do Python): Pó, Gás, Mão de Obra, Máquina.
pub const COST_COLORS: [Color32; 4] = [
    Color32::from_rgb(0x6b, 0xae, 0xd6),
    Color32::from_rgb(0xfd, 0x8d, 0x3c),
    Color32::from_rgb(0x74, 0xc4, 0x76),
    Color32::from_rgb(0x9e, 0x9a, 0xc8),
];

/// Pó, Gás, Mão de Obra e Máquina: `None` quando o valor não é um número
/// (fórmula não resolvida); negativos contam como zero, como no Python.
pub fn cost_parts(costs: &CostSummary) -> [Option<f64>; 4] {
    [
        costs.powder_cost,
        costs.gas_cost,
        costs.labor_cost,
        costs.machine_cost,
    ]
    .map(|v| finite_or_none(v).map(|v| v.max(0.0)))
}

/// Total (ausentes contam como zero) e percentual de cada parte; com total
/// zero as partes presentes ficam em 0 %.
pub fn percentages(parts: &[Option<f64>; 4]) -> (f64, [Option<f64>; 4]) {
    let total: f64 = parts.iter().flatten().sum();
    let pct = parts.map(|p| p.map(|v| if total > 0.0 { v / total * 100.0 } else { 0.0 }));
    (total, pct)
}

// ---------------------------------------------------------------------------
// Abas com egui_plot
// ---------------------------------------------------------------------------

/// Altura reservada a um título + espaçamento (para dividir a aba em dois
/// gráficos).
const HEADING_SPACE: f32 = 30.0;

/// `color` com o alfa trocado por `alpha` (0..=1).
fn with_alpha(color: Color32, alpha: f32) -> Color32 {
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), a)
}

/// Título do gráfico (o `set_title` do matplotlib), na cor forte do tema.
fn heading(ui: &mut Ui, title: &str) {
    let color = ui.visuals().strong_text_color();
    ui.label(RichText::new(title).size(15.0).color(color));
}

/// Caixinha de anotação (o `bbox` branco do matplotlib), com o fundo de
/// cartão do tema. Aceita texto de várias linhas.
fn badge(ui: &mut Ui, text: &str, palette: &Palette) {
    let color = ui.visuals().text_color();
    egui::Frame::new()
        .fill(palette.card_bg)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).color(color));
        });
}

/// Gráfico sem dados (`_set_empty_plot`): título, eixos nomeados e
/// "Sem dados" no centro.
fn empty_plot(
    ui: &mut Ui,
    id: &str,
    title: &str,
    x_label: &str,
    y_label: &str,
    height: Option<f32>,
) {
    heading(ui, title);
    let weak = ui.visuals().weak_text_color();
    let mut plot = Plot::new(id)
        .x_axis_label(x_label)
        .y_axis_label(y_label)
        .include_x(0.0)
        .include_x(1.0)
        .include_y(0.0)
        .include_y(1.0)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false);
    if let Some(h) = height {
        plot = plot.height(h);
    }
    plot.show(ui, |plot_ui| {
        let text = RichText::new("Sem dados").size(14.0).color(weak);
        plot_ui.text(Text::new("", PlotPoint::new(0.5, 0.5), text).anchor(Align2::CENTER_CENTER));
    });
}

/// Aba "Análise Temporal": X, Z e A (tracejada) por passo do G-code.
pub fn temporal(ui: &mut Ui, series: &Series, palette: &Palette) {
    if series.is_empty() {
        empty_plot(
            ui,
            "chart_temporal",
            "Análise Temporal (sem dados)",
            "Passo",
            "Valor",
            None,
        );
        return;
    }
    heading(ui, "Análise Temporal (Coordenadas)");
    let x = step_points(&series.x);
    let z = step_points(&series.z);
    let a = step_points(&series.a);
    Plot::new("chart_temporal")
        .legend(Legend::default().position(Corner::RightTop))
        .x_axis_label("Passo (G-Code)")
        .y_axis_label("Valor (mm / graus)")
        .show(ui, |plot_ui| {
            plot_ui.line(Line::new("X", x).color(palette.accent).width(1.5));
            plot_ui.line(Line::new("Z", z).color(palette.success).width(1.5));
            plot_ui.line(
                Line::new("A", a)
                    .color(palette.warning)
                    .width(1.5)
                    .style(LineStyle::dashed_loose()),
            );
        });
}

/// Aba "Oscilação": posição X por passo e a amplitude aproximada
/// (`(max - min) / 2`) em uma anotação ao lado do título.
pub fn oscillation(ui: &mut Ui, osc: &[f64], palette: &Palette) {
    let points = step_points(osc);
    if points.is_empty() {
        empty_plot(
            ui,
            "chart_osc",
            "Oscilação (sem dados)",
            "Passo",
            "Posição X",
            None,
        );
        return;
    }
    ui.horizontal(|ui| {
        heading(ui, "Padrão de Oscilação (Eixo X)");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            badge(
                ui,
                &format!("Amplitude Aprox: {:.2} mm", amplitude(osc)),
                palette,
            );
        });
    });
    Plot::new("chart_osc")
        .legend(Legend::default().position(Corner::RightTop))
        .x_axis_label("Passo")
        .y_axis_label("Posição X (mm)")
        .show(ui, |plot_ui| {
            plot_ui.line(
                Line::new("Posição X", points)
                    .color(palette.accent)
                    .width(1.2),
            );
        });
}

/// Aba "Estatísticas": distância por passo com o acumulado normalizado ao
/// máximo dos passos (eixo secundário do Python), e o histograma das
/// distâncias com média e máximo.
pub fn statistics(ui: &mut Ui, dists: &[f64], palette: &Palette) {
    let per_step = step_points(dists);
    let plot_h = ((ui.available_height() - 2.0 * HEADING_SPACE) / 2.0).max(80.0);
    if per_step.is_empty() {
        empty_plot(
            ui,
            "chart_dist",
            "Distância por passo (sem dados)",
            "Passo",
            "Distância",
            Some(plot_h),
        );
        empty_plot(
            ui,
            "chart_hist",
            "Histograma",
            "Distância",
            "Contagem",
            Some(plot_h),
        );
        return;
    }

    heading(ui, "Distância Percorrida");
    let max_dist = per_step.iter().map(|p| p[1]).fold(0.0, f64::max);
    let cumulative_norm = step_points(&normalize_to(&cumulative(dists), max_dist));
    let faded = with_alpha(palette.accent, 0.7);
    Plot::new("chart_dist")
        .legend(Legend::default().position(Corner::LeftTop))
        .x_axis_label("Passo")
        .y_axis_label("Distância do Passo (mm)")
        .height(plot_h)
        .show(ui, |plot_ui| {
            plot_ui.line(
                Line::new("Passo Individual", per_step)
                    .color(faded)
                    .width(1.2),
            );
            plot_ui.line(
                Line::new("Acumulado (normalizado)", cumulative_norm)
                    .color(palette.warning)
                    .width(2.0),
            );
        });

    ui.horizontal(|ui| {
        heading(ui, "Distribuição do Tamanho dos Passos");
        if let Some((mean, max)) = mean_max(dists) {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                badge(ui, &format!("Média: {mean:.2}mm\nMáx: {max:.2}mm"), palette);
            });
        }
    });
    let bins = histogram(dists, histogram_bins(dists.len()));
    let fill = with_alpha(palette.accent, 0.85);
    let edge = Stroke::new(0.5, palette.card_bg);
    let bars: Vec<Bar> = bins
        .iter()
        .map(|b| {
            Bar::new(b.center, b.count as f64)
                .width(b.width)
                .fill(fill)
                .stroke(edge)
        })
        .collect();
    Plot::new("chart_hist")
        .x_axis_label("Distância (mm)")
        .y_axis_label("Frequência")
        .include_y(0.0)
        .show(ui, |plot_ui| {
            plot_ui.bar_chart(BarChart::new("Passos", bars).color(fill));
        });
}

// ---------------------------------------------------------------------------
// Dashboard de processo (Painter, grade 2×2)
// ---------------------------------------------------------------------------

const CARD_GAP: f32 = 8.0;
const CARD_PAD: f32 = 10.0;
/// `colors["time"]` do Python: destaque do cartão de tempo.
const TIME_COLOR: Color32 = Color32::from_rgb(0xff, 0x7f, 0x0e);
/// `bar_1` / `bar_2` do Python: Pó (kg) e Gás (m³).
const BAR_COLORS: [Color32; 2] = [
    Color32::from_rgb(0x6b, 0xae, 0xd6),
    Color32::from_rgb(0x31, 0xa3, 0x54),
];
/// Zona de sobreposição e penetração (diluição) do esquema, como no Python.
const OVERLAP_COLOR: Color32 = Color32::from_rgb(0xff, 0x20, 0x28);
const DILUTION_COLOR: Color32 = Color32::from_rgb(0x7a, 0x3b, 0x1a);

/// Cores de texto do tema, lidas uma vez por quadro.
#[derive(Clone, Copy)]
struct Ink {
    text: Color32,
    weak: Color32,
    strong: Color32,
}

impl Ink {
    fn of(ui: &Ui) -> Self {
        let v = ui.visuals();
        Ink {
            text: v.text_color(),
            weak: v.weak_text_color(),
            strong: v.strong_text_color(),
        }
    }
}

/// Aba "Processo": rosca de custos, barras de consumo, esquema de camadas e
/// cartão de tempo, em uma grade 2×2 que ocupa toda a aba.
pub fn process_dashboard(ui: &mut Ui, est: &Estimates, params: &Params, palette: &Palette) {
    let ink = Ink::of(ui);
    let size = ui.available_size().max(vec2(1.0, 1.0));
    let (response, painter) = ui.allocate_painter(size, Sense::hover());
    let rect = response.rect;
    let cell_w = ((rect.width() - CARD_GAP) / 2.0).max(1.0);
    let cell_h = ((rect.height() - CARD_GAP) / 2.0).max(1.0);
    let cell = |col: f32, row: f32| {
        let min = rect.min + vec2(col * (cell_w + CARD_GAP), row * (cell_h + CARD_GAP));
        Rect::from_min_size(min, vec2(cell_w, cell_h))
    };

    let (p, area) = card(
        &painter,
        cell(0.0, 0.0),
        "Composicao de Custos",
        palette,
        ink,
    );
    draw_cost_donut(&p, area, est, palette, ink);
    let (p, area) = card(&painter, cell(1.0, 0.0), "Consumo Estimado", palette, ink);
    draw_consumption_bars(&p, area, est, palette, ink);
    let (p, area) = card(
        &painter,
        cell(0.0, 1.0),
        "Faixas e Sobreposicao",
        palette,
        ink,
    );
    draw_schematic(&p, area, params, est.dilution_pct, palette, ink);
    let (p, area) = card(&painter, cell(1.0, 1.0), "Tempo Estimado", palette, ink);
    draw_time_card(&p, area, est.time_hours, palette, ink);
}

/// Fundo, borda e título de um cartão (`_style_dashboard_axis`). Devolve um
/// painter recortado ao cartão e a área livre abaixo do título.
fn card(
    painter: &egui::Painter,
    rect: Rect,
    title: &str,
    palette: &Palette,
    ink: Ink,
) -> (egui::Painter, Rect) {
    painter.rect_filled(rect, CornerRadius::same(6), palette.card_bg);
    painter.rect_stroke(
        rect,
        CornerRadius::same(6),
        Stroke::new(1.0, palette.border),
        StrokeKind::Inside,
    );
    let clipped = painter.with_clip_rect(rect);
    let title_rect = clipped.text(
        rect.min + vec2(CARD_PAD, CARD_PAD),
        Align2::LEFT_TOP,
        title,
        FontId::proportional(13.0),
        ink.strong,
    );
    let min = pos2(rect.min.x + CARD_PAD, title_rect.max.y + 6.0);
    let max = rect.max - vec2(CARD_PAD, CARD_PAD);
    let size = (max - min).max(Vec2::ZERO);
    (clipped, Rect::from_min_size(min, size))
}

/// Setor de anel (fatia da rosca) como malha de triângulos, com um traço na
/// cor do cartão separando-o do vizinho (o `edgecolor` branco do Python).
#[allow(clippy::too_many_arguments)]
fn add_ring_segment(
    painter: &egui::Painter,
    center: Pos2,
    r_in: f32,
    r_out: f32,
    start: f32,
    sweep: f32,
    color: Color32,
    edge: Color32,
) {
    let steps = ((sweep.abs() / TAU * 96.0).ceil() as usize).max(2);
    let mut mesh = Mesh::default();
    for i in 0..=steps {
        let angle = start + sweep * (i as f32 / steps as f32);
        let (s, c) = angle.sin_cos();
        mesh.colored_vertex(center + vec2(c * r_out, s * r_out), color);
        mesh.colored_vertex(center + vec2(c * r_in, s * r_in), color);
    }
    for i in 0..steps as u32 {
        let (o0, i0, o1, i1) = (2 * i, 2 * i + 1, 2 * i + 2, 2 * i + 3);
        mesh.add_triangle(o0, i0, o1);
        mesh.add_triangle(i0, i1, o1);
    }
    painter.add(Shape::mesh(mesh));
    let (s, c) = start.sin_cos();
    painter.line_segment(
        [
            center + vec2(c * r_in, s * r_in),
            center + vec2(c * r_out, s * r_out),
        ],
        Stroke::new(1.5, edge),
    );
}

/// Rosca Pó/Gás/Mão de Obra/Máquina com o total no centro e a legenda em %
/// (duas colunas) abaixo. Sem custo: "Sem dados de custo".
fn draw_cost_donut(
    painter: &egui::Painter,
    area: Rect,
    est: &Estimates,
    palette: &Palette,
    ink: Ink,
) {
    let parts = cost_parts(&est.costs);
    let (total, pct) = percentages(&parts);
    if total <= 0.0 {
        painter.text(
            area.center(),
            Align2::CENTER_CENTER,
            "Sem dados de custo",
            FontId::proportional(13.0),
            ink.weak,
        );
        return;
    }
    const ROW_H: f32 = 18.0;
    let legend_h = 2.0 * ROW_H + 4.0;
    let ring_bottom = (area.max.y - legend_h).max(area.min.y);
    let ring_area = Rect::from_min_max(area.min, pos2(area.max.x, ring_bottom));
    let radius = (ring_area.width().min(ring_area.height()) / 2.0 - 2.0).max(0.0);
    let center = ring_area.center();
    if radius > 6.0 {
        let inner = radius * 0.67;
        // Começa em cima e anda no sentido anti-horário visual (matplotlib
        // `startangle=90`, `counterclock=True`): na tela o ângulo decresce.
        let mut start = -FRAC_PI_2;
        for (i, part) in parts.iter().enumerate() {
            let value = part.unwrap_or(0.0);
            if value <= 0.0 {
                continue;
            }
            let sweep = (value / total) as f32 * TAU;
            add_ring_segment(
                painter,
                center,
                inner,
                radius,
                start,
                -sweep,
                COST_COLORS[i],
                palette.card_bg,
            );
            start -= sweep;
        }
        let symbol = if est.costs.currency.is_empty() {
            "R$"
        } else {
            est.costs.currency.as_str()
        };
        painter.text(
            center - vec2(0.0, radius * 0.13),
            Align2::CENTER_CENTER,
            "Total",
            FontId::proportional(11.0),
            ink.weak,
        );
        painter.text(
            center + vec2(0.0, radius * 0.14),
            Align2::CENTER_CENTER,
            format!("{symbol}{total:.2}"),
            FontId::proportional(14.0),
            ink.strong,
        );
    }
    let col_w = area.width() / 2.0;
    for (i, label) in COST_LABELS.iter().enumerate() {
        let col = (i % 2) as f32;
        let row = (i / 2) as f32;
        let y = ring_bottom + 4.0 + row * ROW_H + ROW_H / 2.0;
        let x = area.min.x + col * col_w;
        let swatch = Rect::from_center_size(pos2(x + 6.0, y), vec2(10.0, 10.0));
        painter.rect_filled(swatch, CornerRadius::same(2), COST_COLORS[i]);
        let suffix = if pct[i].is_some() { "%" } else { "" };
        let text = format!("{label}  {}{suffix}", fmt_opt(pct[i], 1));
        painter.text(
            pos2(x + 16.0, y),
            Align2::LEFT_CENTER,
            text,
            FontId::proportional(11.0),
            ink.text,
        );
    }
}

/// Barras horizontais "Pó (kg)" e "Gás (m³)" com o valor (`{:.3}`) ao lado;
/// a primeira categoria fica embaixo, como no `barh` do matplotlib.
fn draw_consumption_bars(
    painter: &egui::Painter,
    area: Rect,
    est: &Estimates,
    palette: &Palette,
    ink: Ink,
) {
    let values = [
        finite_or_none(est.costs.powder_kg),
        finite_or_none(est.costs.gas_m3),
    ];
    let categories = ["Pó (kg)", "Gás (m³)"];
    let max = values.iter().flatten().copied().fold(1e-6, f64::max);
    let x_lim = max * 1.35;
    const LABEL_W: f32 = 62.0;
    const AXIS_H: f32 = 18.0;
    let plot = Rect::from_min_max(
        pos2(area.min.x + LABEL_W, area.min.y),
        pos2(area.max.x, area.max.y - AXIS_H),
    );
    if plot.width() <= 1.0 || plot.height() <= 1.0 {
        return;
    }
    let row_h = plot.height() / 2.0;
    let bar_h = (row_h * 0.52).max(2.0);
    for (i, (category, value)) in categories.iter().zip(values).enumerate() {
        let cy = plot.max.y - row_h * (i as f32 + 0.5);
        painter.text(
            pos2(plot.min.x - 8.0, cy),
            Align2::RIGHT_CENTER,
            category,
            FontId::proportional(11.0),
            ink.text,
        );
        let v = value.unwrap_or(0.0).max(0.0);
        let w = ((v / x_lim) as f32 * plot.width()).clamp(0.0, plot.width());
        let bar = Rect::from_min_size(pos2(plot.min.x, cy - bar_h / 2.0), vec2(w, bar_h));
        painter.rect_filled(bar, CornerRadius::same(2), with_alpha(BAR_COLORS[i], 0.86));
        painter.text(
            pos2(plot.min.x + w + plot.width() * 0.02, cy),
            Align2::LEFT_CENTER,
            fmt_opt(value, 3),
            FontId::proportional(11.5),
            ink.strong,
        );
    }
    painter.line_segment(
        [pos2(plot.min.x, plot.max.y), pos2(plot.max.x, plot.max.y)],
        Stroke::new(1.0, palette.border),
    );
    painter.text(
        pos2(plot.center().x, area.max.y),
        Align2::CENTER_BOTTOM,
        "Quantidade",
        FontId::proportional(10.5),
        ink.weak,
    );
}

/// Cartão "Tempo Estimado": `HHh MMm` grande, "Tempo total previsto", barra e
/// `{:.2} h`; sem tempo mostra "--:--".
fn draw_time_card(
    painter: &egui::Painter,
    area: Rect,
    time_hours: f64,
    palette: &Palette,
    ink: Ink,
) {
    let Some(label) = hours_label(time_hours) else {
        painter.text(
            area.center(),
            Align2::CENTER_CENTER,
            "--:--",
            FontId::proportional(26.0),
            ink.weak,
        );
        return;
    };
    let x0 = area.min.x + area.width() * 0.08;
    let x1 = area.min.x + area.width() * 0.92;
    // Frações medidas de baixo para cima, como `transAxes`.
    let y = |f: f32| area.max.y - area.height() * f;
    painter.text(
        pos2(x0, y(0.67)),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(26.0),
        TIME_COLOR,
    );
    painter.text(
        pos2(x0, y(0.47)),
        Align2::LEFT_CENTER,
        "Tempo total previsto",
        FontId::proportional(11.5),
        ink.weak,
    );
    let bar_y = y(0.22);
    let track = Rect::from_min_max(pos2(x0, bar_y - 3.5), pos2(x1, bar_y + 3.5));
    painter.rect_filled(track, CornerRadius::same(4), palette.border);
    // O Python preenche de 0.08 a 0.68 do eixo (60/84 da trilha), fixo.
    let fill_end = x0 + (x1 - x0) * (0.60 / 0.84);
    let fill = Rect::from_min_max(pos2(x0, bar_y - 3.5), pos2(fill_end, bar_y + 3.5));
    painter.rect_filled(fill, CornerRadius::same(4), TIME_COLOR);
    painter.text(
        pos2(x0, y(0.08)),
        Align2::LEFT_CENTER,
        format!("{time_hours:.2} h"),
        FontId::proportional(11.5),
        ink.weak,
    );
}

// ---------------------------------------------------------------------------
// Esquema "Faixas e Sobreposicao" (porte de `_draw_layer_fill_schematic`)
// ---------------------------------------------------------------------------

/// Cordões desenhados no esquema (o Python mostra sempre três).
pub const SCHEMATIC_BEADS: usize = 3;
/// Amostras do perfil ao longo do comprimento.
const PROFILE_SAMPLES: usize = 320;

/// Geometria do esquema em mm: três cordões de corpo `bead_body_width`
/// afastados `spacing_mm`, empilhados em `num_layers` camadas de
/// `layer_height`.
#[derive(Clone, Debug, PartialEq)]
pub struct Schematic {
    pub spacing_mm: f64,
    pub bead_body_width: f64,
    pub bead_starts: [f64; SCHEMATIC_BEADS],
    pub display_length: f64,
    pub margin_x: f64,
    pub layer_height: f64,
    pub num_layers: usize,
    pub stack_height: f64,
    /// Faixas `(início, fim)` onde cordões vizinhos se sobrepõem.
    pub overlap_ranges: Vec<(f64, f64)>,
    /// Diluição em %, fixada em [0, 100].
    pub dilution_pct: f64,
    /// `dilution_pct / 20` fixado em [0, 1]: controla pé e penetração do cordão.
    pub dilution_norm: f64,
}

/// Monta a geometria a partir dos parâmetros; `None` sem comprimento ou sem
/// espessura de camada (o Python mostra "Sem dados"). O passo lateral vem do
/// passo do multiperfil, senão do modelo de sobreposição do núcleo, senão de
/// `largura * (1 - sobreposição)`.
pub fn schematic_geometry(params: &Params, dilution_pct: f64) -> Option<Schematic> {
    let raw = [
        params.comprimento_revestir,
        params.espessura_camada,
        params.largura_cordao,
        params.sobreposicao,
        params.multiperfil_pitch_mm,
    ];
    if raw.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let part_length = params.comprimento_revestir.max(0.0);
    let layer_height = params.espessura_camada.max(0.0);
    let bead_width = params.largura_cordao.max(0.0);
    let overlap_pct = params.sobreposicao;
    let pitch = params.multiperfil_pitch_mm;
    if part_length <= 0.0 || layer_height <= 0.0 {
        return None;
    }
    let num_layers = usize::try_from(params.num_camadas.max(1)).unwrap_or(1);

    let mut spacing = 0.0;
    if params.welding_mode == WeldingMode::EspiralMultiperfil && pitch > 0.0 {
        spacing = pitch;
    } else if bead_width > 0.0 {
        let calib = params.bead_width_calibration_factor;
        let calib = if calib.is_finite() && calib > 0.0 {
            calib
        } else {
            1.0
        };
        let metrics = overlap::compute_overlap_metrics(
            bead_width,
            overlap_pct,
            layer_height,
            None,
            None,
            false,
            calib,
        );
        if metrics.step_over_mm.is_finite() {
            spacing = metrics.step_over_mm.max(0.0);
        }
    }
    if spacing <= 0.0 && bead_width > 0.0 {
        spacing = bead_width * (1.0 - overlap_pct / 100.0).max(0.1);
    }
    if spacing <= 0.0 {
        spacing = part_length;
    }

    let bead_body_width = bead_width.max(spacing * 1.16).max(1.0);
    let bead_starts = [0.0, spacing, 2.0 * spacing];
    let display_length = bead_starts[SCHEMATIC_BEADS - 1] + bead_body_width;
    let margin_x = (display_length * 0.16).max(2.8);
    let stack_height = (num_layers as f64 * layer_height).max(layer_height);
    let overlap_ranges = bead_starts
        .windows(2)
        .filter_map(|pair| {
            let (left, right) = (pair[0], pair[1]);
            let start = left.max(right);
            let end = (left + bead_body_width).min(right + bead_body_width);
            (end > start).then_some((start, end))
        })
        .collect();
    let dilution_pct = if dilution_pct.is_finite() {
        dilution_pct.clamp(0.0, 100.0)
    } else {
        0.0
    };
    let dilution_norm = (dilution_pct / 20.0).clamp(0.0, 1.0);

    Some(Schematic {
        spacing_mm: spacing,
        bead_body_width,
        bead_starts,
        display_length,
        margin_x,
        layer_height,
        num_layers,
        stack_height,
        overlap_ranges,
        dilution_pct,
        dilution_norm,
    })
}

/// Uma amostra do perfil: topo do revestimento, profundidade de penetração
/// (abaixo da base) e quantos cordões cobrem este `x`.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileSample {
    pub x: f64,
    pub top: f64,
    pub bottom: f64,
    pub overlaps: u32,
}

/// Perfil do revestimento amostrado em `samples` pontos de `0` a
/// `display_length`: cada cordão contribui com uma coroa
/// `stack * (pé + (1 - pé) * (1 - u^1.8)^0.85)` e uma penetração
/// `prof * (1 - u^1.35)^1.15`, com `u` a distância normalizada ao centro.
pub fn clad_profile(geo: &Schematic, samples: usize) -> Vec<ProfileSample> {
    if samples == 0 {
        return Vec::new();
    }
    let half = (geo.bead_body_width / 2.0).max(1e-6);
    let toe = 0.16 + 0.06 * geo.dilution_norm;
    let penetration = geo.layer_height * (0.10 + 0.22 * geo.dilution_norm);
    let centers = geo.bead_starts.map(|s| s + geo.bead_body_width / 2.0);
    (0..samples)
        .map(|i| {
            let t = if samples > 1 {
                i as f64 / (samples - 1) as f64
            } else {
                0.0
            };
            let x = geo.display_length * t;
            let mut top = 0.0;
            let mut bottom = 0.0;
            let mut overlaps = 0u32;
            for c in centers {
                let u = ((x - c) / half).abs();
                if u <= 1.0 {
                    overlaps += 1;
                    let crown = (1.0 - u.powf(1.8)).clamp(0.0, 1.0).powf(0.85);
                    top += geo.stack_height * (toe + (1.0 - toe) * crown);
                    let pen = (1.0 - u.powf(1.35)).clamp(0.0, 1.0).powf(1.15);
                    bottom = f64::max(bottom, penetration * pen);
                }
            }
            ProfileSample {
                x,
                top,
                bottom,
                overlaps,
            }
        })
        .collect()
}

/// Mapeia coordenadas de dados (mm) para a tela, com y crescendo para cima.
struct Frame2d {
    rect: Rect,
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
}

impl Frame2d {
    fn new(rect: Rect, x0: f64, x1: f64, y0: f64, y1: f64) -> Self {
        let x1 = if x1 > x0 { x1 } else { x0 + 1.0 };
        let y1 = if y1 > y0 { y1 } else { y0 + 1.0 };
        Frame2d {
            rect,
            x0,
            x1,
            y0,
            y1,
        }
    }

    fn at(&self, x: f64, y: f64) -> Pos2 {
        let fx = ((x - self.x0) / (self.x1 - self.x0)) as f32;
        let fy = ((y - self.y0) / (self.y1 - self.y0)) as f32;
        pos2(
            self.rect.min.x + fx * self.rect.width(),
            self.rect.max.y - fy * self.rect.height(),
        )
    }
}

/// Preenche, entre amostras consecutivas aceitas por `keep`, a faixa vertical
/// `range(amostra) = (baixo, alto)` como quadriláteros de uma malha.
fn fill_band(
    painter: &egui::Painter,
    frame: &Frame2d,
    prof: &[ProfileSample],
    range: impl Fn(&ProfileSample) -> (f64, f64),
    keep: impl Fn(&ProfileSample) -> bool,
    color: Color32,
) {
    let mut mesh = Mesh::default();
    for pair in prof.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if !(keep(a) && keep(b)) {
            continue;
        }
        let (lo_a, hi_a) = range(a);
        let (lo_b, hi_b) = range(b);
        if hi_a <= lo_a && hi_b <= lo_b {
            continue;
        }
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(frame.at(a.x, lo_a), color);
        mesh.colored_vertex(frame.at(a.x, hi_a.max(lo_a)), color);
        mesh.colored_vertex(frame.at(b.x, hi_b.max(lo_b)), color);
        mesh.colored_vertex(frame.at(b.x, lo_b), color);
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    if !mesh.is_empty() {
        painter.add(Shape::mesh(mesh));
    }
}

/// Texto girado 90° (de baixo para cima), centrado em `center`.
fn rotated_text(painter: &egui::Painter, center: Pos2, text: &str, size: f32, color: Color32) {
    let galley = painter.layout_no_wrap(text.to_string(), FontId::proportional(size), color);
    let pos = center - galley.size() / 2.0;
    painter.add(
        TextShape::new(pos, galley, color).with_angle_and_anchor(-FRAC_PI_2, Align2::CENTER_CENTER),
    );
}

/// Cota horizontal: linha com dois traços nas pontas e o rótulo acima.
#[allow(clippy::too_many_arguments)]
fn dimension(
    painter: &egui::Painter,
    frame: &Frame2d,
    x0: f64,
    x1: f64,
    y: f64,
    tick: f64,
    label: &str,
    ink: Ink,
) {
    let stroke = Stroke::new(1.2, ink.text);
    painter.line_segment([frame.at(x0, y), frame.at(x1, y)], stroke);
    painter.line_segment([frame.at(x0, y - tick), frame.at(x0, y + tick)], stroke);
    painter.line_segment([frame.at(x1, y - tick), frame.at(x1, y + tick)], stroke);
    let mid = frame.at((x0 + x1) / 2.0, y + tick * 0.6);
    painter.text(
        mid,
        Align2::CENTER_BOTTOM,
        label,
        FontId::proportional(9.5),
        ink.text,
    );
}

/// Comprimento fixo, em px, da ponta das setas dos eixos do esquema. O
/// `Painter::arrow` do egui faz a ponta com 25 % do comprimento do eixo, o que
/// aqui atravessaria o desenho; o Python usa uma ponta `->` pequena e fixa.
pub const ARROW_HEAD_PX: f32 = 7.0;
// Garantia em tempo de compilação: a ponta nunca volta a crescer.
const _: () = assert!(
    ARROW_HEAD_PX <= 8.0,
    "ponta da seta do esquema deve ter no máximo 8 px"
);

/// Abertura de cada farpa da seta em relação ao eixo (a mesma do egui).
const ARROW_BARB_DEG: f32 = 36.0;

/// Pontas das duas farpas de uma seta de `from` a `to`, com `head` px de
/// comprimento. Um vetor nulo ou não finito devolve a própria ponta (sem NaN).
pub fn arrow_barbs(from: Pos2, to: Pos2, head: f32) -> [Pos2; 2] {
    let dir = to - from;
    let len = dir.length();
    if !len.is_finite() || len <= f32::EPSILON {
        return [to, to];
    }
    let back = dir / len * head;
    let rot = egui::emath::Rot2::from_angle(ARROW_BARB_DEG.to_radians());
    [to - rot * back, to - rot.inverse() * back]
}

/// Seta de eixo: haste em `line_segment` e ponta de [`ARROW_HEAD_PX`].
fn axis_arrow(painter: &egui::Painter, from: Pos2, to: Pos2, stroke: Stroke) {
    painter.line_segment([from, to], stroke);
    let [a, b] = arrow_barbs(from, to, ARROW_HEAD_PX);
    painter.line_segment([to, a], stroke);
    painter.line_segment([to, b], stroke);
}

/// Trechos `[início, fim)` de amostras consecutivas cujo topo passa de `y`,
/// com pelo menos duas amostras (uma divisória vira uma linha por trecho, em
/// vez de um segmento por par de amostras).
pub fn runs_above(prof: &[ProfileSample], y: f64) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start: Option<usize> = None;
    for (i, sample) in prof.iter().enumerate() {
        match (sample.top > y, start) {
            (true, None) => start = Some(i),
            (false, Some(begin)) => {
                runs.push((begin, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(begin) = start {
        runs.push((begin, prof.len()));
    }
    runs.retain(|(begin, end)| end - begin >= 2);
    runs
}

/// Desenha o esquema: eixos, camadas coloridas pela paleta recortadas pelo
/// perfil, zonas de sobreposição, penetração, divisórias, cotas e legenda.
fn draw_schematic(
    painter: &egui::Painter,
    area: Rect,
    params: &Params,
    dilution_pct: f64,
    palette: &Palette,
    ink: Ink,
) {
    let Some(geo) = schematic_geometry(params, dilution_pct) else {
        painter.text(
            area.center(),
            Align2::CENTER_CENTER,
            "Sem dados",
            FontId::proportional(13.0),
            ink.weak,
        );
        painter.text(
            pos2(area.center().x, area.max.y),
            Align2::CENTER_BOTTOM,
            "Comprimento (mm)",
            FontId::proportional(10.5),
            ink.weak,
        );
        rotated_text(
            painter,
            pos2(area.min.x + 7.0, area.center().y),
            "Altura",
            10.5,
            ink.weak,
        );
        return;
    };
    if area.width() < 40.0 || area.height() < 40.0 {
        return;
    }
    let prof = clad_profile(&geo, PROFILE_SAMPLES);
    let (Some(first), Some(last)) = (prof.first(), prof.last()) else {
        return;
    };
    let h = geo.layer_height;
    let length = geo.display_length;
    let top_y = prof.iter().map(|s| s.top).fold(geo.stack_height, f64::max);
    // Folga acima do perfil para as cotas e seus rótulos.
    let y_max = top_y + (h * 0.62).max(0.92) + (h * 0.30).max(0.30);
    let y_min = -(h * 0.72).max(0.42);
    let x_min = -geo.margin_x;
    let x_max = length + geo.margin_x * 0.48;
    let frame = Frame2d::new(area, x_min, x_max, y_min, y_max);
    let thin = Stroke::new(1.15, ink.text);

    // Eixos com seta de ponta fixa (comprimento e altura).
    let axis = Stroke::new(1.4, ink.text);
    axis_arrow(
        painter,
        frame.at(-geo.margin_x * 0.10, 0.0),
        frame.at(x_max, 0.0),
        axis,
    );
    axis_arrow(
        painter,
        frame.at(-geo.margin_x * 0.02, 0.0),
        frame.at(-geo.margin_x * 0.02, y_max * 0.88),
        axis,
    );

    // Penetração (diluição) abaixo da base.
    let dilution = with_alpha(DILUTION_COLOR, 0.42);
    fill_band(
        painter,
        &frame,
        &prof,
        |s| (-s.bottom, 0.0),
        |s| s.bottom > 1e-6,
        dilution,
    );
    // Camadas, da base ao topo, recortadas pelo perfil; a última sobe até a coroa.
    for k in 0..geo.num_layers {
        let lo = k as f64 * h;
        let hi = if k + 1 == geo.num_layers {
            f64::INFINITY
        } else {
            (k + 1) as f64 * h
        };
        let color = palette.layer_color(k);
        fill_band(
            painter,
            &frame,
            &prof,
            move |s| (lo, s.top.min(hi)),
            |_| true,
            color,
        );
    }
    // Zonas onde dois cordões se sobrepõem.
    let overlap = with_alpha(OVERLAP_COLOR, 0.96);
    fill_band(
        painter,
        &frame,
        &prof,
        |s| (0.0, s.top),
        |s| s.overlaps >= 2,
        overlap,
    );
    // Contorno do revestimento.
    let outline: Vec<Pos2> = prof.iter().map(|s| frame.at(s.x, s.top)).collect();
    painter.add(Shape::line(outline, thin));
    painter.line_segment([frame.at(0.0, 0.0), frame.at(0.0, first.top)], thin);
    painter.line_segment([frame.at(length, 0.0), frame.at(length, last.top)], thin);

    // Divisórias entre camadas (no máximo 24), recortadas pelo perfil.
    if geo.num_layers > 1 {
        let step = ((geo.num_layers as f64 / 24.0).ceil() as usize).max(1);
        let split = Stroke::new(0.85, with_alpha(ink.text, 0.9));
        for k in (step..geo.num_layers).step_by(step) {
            let y = k as f64 * h;
            if y >= top_y - 1e-6 {
                continue;
            }
            // Uma linha por trecho do perfil acima da divisória, não um
            // segmento por par de amostras.
            for (begin, end) in runs_above(&prof, y) {
                let points: Vec<Pos2> = prof[begin..end].iter().map(|s| frame.at(s.x, y)).collect();
                painter.add(Shape::line(points, split));
            }
            let tick = Stroke::new(0.8, ink.weak);
            painter.line_segment([frame.at(-geo.margin_x * 0.04, y), frame.at(0.0, y)], tick);
        }
    }
    // Linhas de centro dos cordões, tracejadas, da base ao perfil.
    let n = prof.len();
    for start in geo.bead_starts {
        let c = start + geo.bead_body_width / 2.0;
        let idx = ((c / length) * (n - 1) as f64)
            .round()
            .clamp(0.0, (n - 1) as f64) as usize;
        let top = prof.get(idx).map_or(0.0, |s| s.top);
        let dashed = Shape::dashed_line(
            &[frame.at(c, 0.0), frame.at(c, top)],
            Stroke::new(0.9, ink.weak),
            3.0,
            3.0,
        );
        painter.extend(dashed);
    }

    // Cotas: trechos de solda e de sobreposição em dois níveis alternados.
    let mut segments: Vec<(f64, f64)> = Vec::new();
    let mut cursor = 0.0;
    for &(s, e) in &geo.overlap_ranges {
        if s > cursor {
            segments.push((cursor, s));
        }
        segments.push((s, e));
        cursor = e;
    }
    if cursor < length {
        segments.push((cursor, length));
    }
    let base_dim = top_y + (h * 0.22).max(0.26);
    let levels = [base_dim, base_dim + (h * 0.22).max(0.18)];
    let tick = (h * 0.12).max(0.12);
    for (i, &(s, e)) in segments.iter().enumerate() {
        if e <= s {
            continue;
        }
        let label = format!("{:.2} mm", e - s);
        dimension(painter, &frame, s, e, levels[i % 2], tick, &label, ink);
    }
    // Passo entre os dois primeiros centros.
    let c0 = geo.bead_starts[0] + geo.bead_body_width / 2.0;
    let c1 = geo.bead_starts[1] + geo.bead_body_width / 2.0;
    let pitch_y = top_y + (h * 0.10).max(0.14);
    let label = format!("Passo {:.2} mm", geo.spacing_mm);
    dimension(
        painter,
        &frame,
        c0,
        c1,
        pitch_y,
        (h * 0.11).max(0.10),
        &label,
        ink,
    );
    // Altura total, à esquerda, com o rótulo em pé.
    let hx = -geo.margin_x * 0.055;
    let htick = (length * 0.015).max(0.10);
    let stroke = Stroke::new(1.2, ink.text);
    painter.line_segment([frame.at(hx, 0.0), frame.at(hx, top_y)], stroke);
    painter.line_segment(
        [frame.at(hx - htick, 0.0), frame.at(hx + htick, 0.0)],
        stroke,
    );
    painter.line_segment(
        [frame.at(hx - htick, top_y), frame.at(hx + htick, top_y)],
        stroke,
    );
    let label_x = hx - (geo.margin_x * 0.08).max(0.22);
    let label = format!("{:.2} mm", geo.stack_height);
    rotated_text(
        painter,
        frame.at(label_x, top_y / 2.0),
        &label,
        9.5,
        ink.text,
    );

    // Rótulos dos eixos e das camadas.
    let bold = FontId::proportional(11.0);
    let small = FontId::proportional(9.5);
    painter.text(
        frame.at(-geo.margin_x * 0.45, y_max * 0.90),
        Align2::LEFT_CENTER,
        "Altura",
        bold.clone(),
        ink.strong,
    );
    // Rótulo logo abaixo do eixo e legenda numa linha própria mais abaixo
    // (em px), para um não cobrir o outro.
    let axis_px = frame.at(0.0, 0.0).y;
    let axis_label = pos2(frame.at(length * 0.70, 0.0).x, axis_px + 4.0);
    painter.text(
        axis_label,
        Align2::LEFT_TOP,
        "Comprimento (mm)",
        bold,
        ink.strong,
    );
    painter.text(
        frame.at(-geo.margin_x * 0.74, top_y * 0.5),
        Align2::LEFT_CENTER,
        "camadas",
        small.clone(),
        ink.weak,
    );
    painter.text(
        frame.at(-geo.margin_x * 0.12, 0.0),
        Align2::RIGHT_CENTER,
        "0",
        small.clone(),
        ink.weak,
    );
    let layers = geo.num_layers.to_string();
    painter.text(
        frame.at(-geo.margin_x * 0.12, top_y),
        Align2::RIGHT_CENTER,
        layers,
        small.clone(),
        ink.text,
    );
    let tick = Stroke::new(0.9, ink.weak);
    painter.line_segment(
        [frame.at(-geo.margin_x * 0.05, top_y), frame.at(0.0, top_y)],
        tick,
    );
    painter.line_segment(
        [frame.at(-geo.margin_x * 0.05, 0.0), frame.at(0.0, 0.0)],
        tick,
    );

    // Legenda abaixo do eixo (em pixels): Solda / Sobreposicao / Diluicao.
    let legend_y = (axis_px + 28.0).min(area.max.y - 6.0);
    let mut x = frame.at(length * 0.02, 0.0).x;
    let items = [
        ("Solda".to_string(), palette.layer_color(0)),
        ("Sobreposicao".to_string(), OVERLAP_COLOR),
        (format!("Diluicao {:.1}%", geo.dilution_pct), dilution),
    ];
    for (label, color) in items {
        let swatch = Rect::from_center_size(pos2(x + 6.0, legend_y), vec2(12.0, 7.0));
        painter.rect_filled(swatch, CornerRadius::ZERO, color);
        let used = painter.text(
            pos2(x + 16.0, legend_y),
            Align2::LEFT_CENTER,
            label,
            small.clone(),
            ink.weak,
        );
        x = used.max.x + 14.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compute::empty_estimates;
    use egui::{Context, RawInput};

    const EPS: f64 = 1e-9;

    #[test]
    fn bins_do_histograma_seguem_clamp_10_50() {
        // bins = max(10, min(50, n // 5)) do Python.
        assert_eq!(histogram_bins(0), 10);
        assert_eq!(histogram_bins(49), 10);
        assert_eq!(histogram_bins(50), 10);
        assert_eq!(histogram_bins(100), 20);
        assert_eq!(histogram_bins(249), 49);
        assert_eq!(histogram_bins(250), 50);
        assert_eq!(histogram_bins(10_000), 50);
    }

    #[test]
    fn histograma_conta_todos_os_valores() {
        let values: Vec<f64> = (0..100).map(f64::from).collect();
        let bins = histogram(&values, 10);
        assert_eq!(bins.len(), 10);
        assert_eq!(bins.iter().map(|b| b.count).sum::<usize>(), 100);
        // Último bin inclui o máximo (como numpy): 90..=99 -> 10 valores.
        assert_eq!(bins[9].count, 10);
        assert!((bins[0].width - 9.9).abs() < EPS);
        assert!((bins[0].center - 4.95).abs() < EPS);
    }

    #[test]
    fn histograma_de_valores_iguais_ou_vazio() {
        let same = histogram(&[2.5, 2.5, 2.5], 10);
        assert_eq!(same.len(), 1);
        assert_eq!(same[0].count, 3);
        assert!(same[0].width > 0.0);
        assert!((same[0].center - 2.5).abs() < EPS);
        assert!(histogram(&[], 10).is_empty());
        assert!(histogram(&[1.0], 0).is_empty());
        // Não finitos são ignorados, nunca derrubam o binning.
        let mixed = histogram(&[1.0, f64::NAN, 3.0, f64::INFINITY], 10);
        assert_eq!(mixed.iter().map(|b| b.count).sum::<usize>(), 2);
    }

    #[test]
    fn soma_acumulada() {
        assert_eq!(cumulative(&[1.0, 2.0, 3.0]), vec![1.0, 3.0, 6.0]);
        assert!(cumulative(&[]).is_empty());
        // NaN conta como zero para o acumulado não "sumir" do gráfico.
        assert_eq!(cumulative(&[1.0, f64::NAN, 2.0]), vec![1.0, 1.0, 3.0]);
    }

    #[test]
    fn normaliza_para_o_maximo_dos_passos() {
        let norm = normalize_to(&[1.0, 3.0, 6.0], 3.0);
        assert!((norm[0] - 0.5).abs() < EPS);
        assert!((norm[1] - 1.5).abs() < EPS);
        assert!((norm[2] - 3.0).abs() < EPS);
        assert_eq!(normalize_to(&[0.0, 0.0], 5.0), vec![0.0, 0.0]);
        assert_eq!(normalize_to(&[1.0, 2.0], 0.0), vec![0.0, 0.0]);
        assert!(normalize_to(&[], 1.0).is_empty());
    }

    #[test]
    fn amplitude_e_metade_da_excursao() {
        assert!((amplitude(&[0.0, 4.0, 2.0]) - 2.0).abs() < EPS);
        assert_eq!(amplitude(&[7.0]), 0.0);
        assert_eq!(amplitude(&[]), 0.0);
        assert!((amplitude(&[-1.0, f64::NAN, 1.0]) - 1.0).abs() < EPS);
    }

    #[test]
    fn media_e_maximo() {
        let (mean, max) = mean_max(&[1.0, 2.0, 6.0]).expect("tem valores");
        assert!((mean - 3.0).abs() < EPS);
        assert!((max - 6.0).abs() < EPS);
        assert!(mean_max(&[]).is_none());
        assert!(mean_max(&[f64::NAN]).is_none());
    }

    #[test]
    fn pontos_por_passo_comecam_em_1_e_pulam_nao_finitos() {
        let pts = step_points(&[5.0, f64::NAN, 7.0]);
        assert_eq!(pts, vec![[1.0, 5.0], [3.0, 7.0]]);
        assert!(step_points(&[]).is_empty());
    }

    #[test]
    fn rotulo_de_horas_hh_mm() {
        assert_eq!(hours_label(1.5).as_deref(), Some("01h 30m"));
        assert_eq!(hours_label(0.25).as_deref(), Some("00h 15m"));
        // 1.9999 h -> 119.994 min: os minutos arredondam para 60 e viram 1 h.
        assert_eq!(hours_label(1.9999).as_deref(), Some("02h 00m"));
        assert_eq!(hours_label(0.0), None);
        assert_eq!(hours_label(-1.0), None);
        assert_eq!(hours_label(f64::NAN), None);
    }

    #[test]
    fn valores_ausentes_viram_travessao() {
        assert_eq!(finite_or_none(1.5), Some(1.5));
        assert_eq!(finite_or_none(f64::NAN), None);
        assert_eq!(finite_or_none(f64::INFINITY), None);
        assert_eq!(fmt_opt(Some(1.23456), 2), "1.23");
        assert_eq!(fmt_opt(Some(0.5), 3), "0.500");
        assert_eq!(fmt_opt(None, 2), "—");
    }

    #[test]
    fn partes_de_custo_e_percentuais() {
        let costs = CostSummary {
            powder_cost: 10.0,
            gas_cost: 0.0,
            labor_cost: 30.0,
            machine_cost: f64::NAN,
            ..Default::default()
        };
        let parts = cost_parts(&costs);
        assert_eq!(parts, [Some(10.0), Some(0.0), Some(30.0), None]);
        let (total, pct) = percentages(&parts);
        assert!((total - 40.0).abs() < EPS);
        assert_eq!(pct[0], Some(25.0));
        assert_eq!(pct[1], Some(0.0));
        assert_eq!(pct[2], Some(75.0));
        assert_eq!(pct[3], None);
        // Negativos contam como zero (Python: `v > 0 else 0.0`).
        let neg = cost_parts(&CostSummary {
            powder_cost: -5.0,
            ..Default::default()
        });
        assert_eq!(neg[0], Some(0.0));
        let (total, pct) = percentages(&[None, None, None, None]);
        assert_eq!(total, 0.0);
        assert_eq!(pct, [None, None, None, None]);
    }

    #[test]
    fn geometria_do_esquema_com_defaults() {
        let p = Params::default(); // 50 mm, h 1.5, cordão 8, sobreposição 40 %, 1 camada
        let geo = schematic_geometry(&p, 0.0).expect("defaults desenham");
        let step =
            overlap::compute_overlap_metrics(8.0, 40.0, 1.5, None, None, false, 1.0).step_over_mm;
        assert!(
            (geo.spacing_mm - step).abs() < EPS,
            "passo {}",
            geo.spacing_mm
        );
        assert!(geo.spacing_mm > 0.0 && geo.spacing_mm < 8.0);
        assert!(geo.bead_body_width >= 8.0);
        assert_eq!(geo.bead_starts, [0.0, step, 2.0 * step]);
        assert!((geo.display_length - (2.0 * step + geo.bead_body_width)).abs() < EPS);
        assert_eq!(geo.num_layers, 1);
        assert!((geo.stack_height - 1.5).abs() < EPS);
        assert_eq!(geo.overlap_ranges.len(), 2, "cordões vizinhos se sobrepõem");
        assert!(geo.margin_x >= 2.8);
    }

    #[test]
    fn geometria_do_esquema_sem_dados_e_multiperfil() {
        let p = Params {
            espessura_camada: 0.0,
            ..Default::default()
        };
        assert!(schematic_geometry(&p, 0.0).is_none());
        let p = Params {
            comprimento_revestir: 0.0,
            ..Default::default()
        };
        assert!(schematic_geometry(&p, 0.0).is_none());
        let p = Params {
            comprimento_revestir: f64::NAN,
            ..Default::default()
        };
        assert!(schematic_geometry(&p, 0.0).is_none());

        let p = Params {
            welding_mode: WeldingMode::EspiralMultiperfil,
            multiperfil_pitch_mm: 12.0,
            num_camadas: 3,
            ..Default::default()
        };
        let geo = schematic_geometry(&p, 50.0).expect("multiperfil desenha");
        assert!((geo.spacing_mm - 12.0).abs() < EPS);
        assert_eq!(geo.num_layers, 3);
        assert!((geo.stack_height - 4.5).abs() < EPS);
        // Cordão de 8 mm com passo 12: corpo = 12 * 1.16 = 13.92, então os
        // vizinhos ainda se sobrepõem em 1.92 mm (o esquema sempre mostra
        // sobreposição, como no Python).
        assert!((geo.bead_body_width - 13.92).abs() < 1e-6);
        assert_eq!(geo.overlap_ranges.len(), 2);
        for (start, end) in &geo.overlap_ranges {
            assert!((end - start - 1.92).abs() < 1e-6, "faixa {start}..{end}");
        }
        assert!((geo.dilution_pct - 50.0).abs() < EPS);
        assert!((geo.dilution_norm - 1.0).abs() < EPS);
        // Diluição fora de [0, 100] (ou NaN) é fixada.
        let geo = schematic_geometry(&p, -4.0).expect("desenha");
        assert_eq!(geo.dilution_pct, 0.0);
        let geo = schematic_geometry(&p, 250.0).expect("desenha");
        assert_eq!(geo.dilution_pct, 100.0);
        let geo = schematic_geometry(&p, f64::NAN).expect("desenha");
        assert_eq!(geo.dilution_pct, 0.0);
    }

    #[test]
    fn perfil_do_revestimento() {
        let geo = schematic_geometry(&Params::default(), 10.0).expect("defaults");
        let prof = clad_profile(&geo, 320);
        assert_eq!(prof.len(), 320);
        assert!((prof[0].x).abs() < EPS);
        assert!((prof[319].x - geo.display_length).abs() < EPS);
        let top_max = prof.iter().map(|s| s.top).fold(0.0, f64::max);
        assert!(top_max >= geo.stack_height - 1e-6, "coroa alcança a altura");
        assert!(prof.iter().all(|s| s.top >= 0.0 && s.bottom >= 0.0));
        assert!(
            prof.iter().any(|s| s.overlaps >= 2),
            "há zona de sobreposição"
        );
        assert!(prof.iter().any(|s| s.bottom > 0.0), "há penetração");
        assert!(clad_profile(&geo, 0).is_empty());
    }

    fn run_headless(mut f: impl FnMut(&mut Ui)) {
        let ctx = Context::default();
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))),
            ..Default::default()
        };
        // Dois quadros: o segundo já tem os limites automáticos do plot. As
        // texturas (fonte) não vão a GPU nenhuma: descartadas de propósito.
        for _ in 0..2 {
            ctx.run_ui(input.clone(), |ui| f(ui)).textures_delta.clear();
        }
    }

    fn sample_series() -> Series {
        let n = 400;
        Series {
            x: (0..n).map(|i| (i as f64 * 0.3).sin() * 4.0).collect(),
            z: (0..n).map(|i| i as f64 * 0.05).collect(),
            a: (0..n).map(|i| i as f64 * 1.5).collect(),
        }
    }

    #[test]
    fn graficos_renderizam_sem_panico_com_dados_e_vazios() {
        for palette in [Palette::light(), Palette::dark()] {
            let series = sample_series();
            run_headless(|ui| temporal(ui, &series, &palette));
            run_headless(|ui| temporal(ui, &Series::default(), &palette));
            run_headless(|ui| oscillation(ui, &series.x, &palette));
            run_headless(|ui| oscillation(ui, &[], &palette));
            run_headless(|ui| oscillation(ui, &[1.0], &palette));
            let dists: Vec<f64> = (0..300).map(|i| 0.5 + (i % 7) as f64 * 0.1).collect();
            run_headless(|ui| statistics(ui, &dists, &palette));
            run_headless(|ui| statistics(ui, &[], &palette));
            run_headless(|ui| statistics(ui, &[0.0, 0.0], &palette));
            run_headless(|ui| statistics(ui, &[f64::NAN], &palette));
        }
    }

    #[test]
    fn dashboard_renderiza_com_e_sem_custos() {
        let palette = Palette::light();
        let params = Params::default();
        let empty = empty_estimates();
        run_headless(|ui| process_dashboard(ui, &empty, &params, &palette));

        let mut est = empty_estimates();
        est.time_hours = 2.75;
        est.dilution_pct = 12.0;
        est.costs = CostSummary {
            powder_cost: 120.0,
            gas_cost: 15.5,
            labor_cost: 80.0,
            machine_cost: 40.0,
            powder_kg: 0.42,
            gas_m3: 2.1,
            currency: "R$".to_string(),
            ..Default::default()
        };
        let mut layered = Params {
            num_camadas: 4,
            ..Default::default()
        };
        run_headless(|ui| process_dashboard(ui, &est, &layered, &palette));
        // Muitas camadas: as divisórias são decimadas (máx. 24 linhas).
        layered.num_camadas = 120;
        run_headless(|ui| process_dashboard(ui, &est, &layered, &Palette::dark()));
        // Painel minúsculo: nada divide por zero nem estoura.
        let ctx = Context::default();
        let tiny = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(40.0, 30.0))),
            ..Default::default()
        };
        ctx.run_ui(tiny, |ui| {
            process_dashboard(ui, &est, &layered, &Palette::dark())
        })
        .textures_delta
        .clear();
    }

    #[test]
    fn setas_dos_eixos_tem_ponta_fixa() {
        // `ARROW_HEAD_PX <= 8` é checado em tempo de compilação ao lado da constante.
        let tip = pos2(400.0, 100.0);
        let [a, b] = arrow_barbs(pos2(0.0, 100.0), tip, ARROW_HEAD_PX);
        for barb in [a, b] {
            assert!(
                ((barb - tip).length() - ARROW_HEAD_PX).abs() < 1e-3,
                "{barb:?}"
            );
            assert!(barb.x < tip.x, "farpa atrás da ponta: {barb:?}");
        }
        // Farpas simétricas em relação ao eixo.
        assert!((a.y - 100.0 + (b.y - 100.0)).abs() < 1e-3);
        assert!((a.y - b.y).abs() > 1.0);
        // Eixo vertical: a ponta não cresce com o comprimento do eixo.
        let tip = pos2(50.0, 10.0);
        let [a, b] = arrow_barbs(pos2(50.0, 900.0), tip, ARROW_HEAD_PX);
        assert!(((a - tip).length() - ARROW_HEAD_PX).abs() < 1e-3);
        assert!(b.y > tip.y);
        // Vetor nulo: nada de NaN.
        let [a, b] = arrow_barbs(tip, tip, ARROW_HEAD_PX);
        assert_eq!([a, b], [tip, tip]);
    }

    #[test]
    fn divisorias_coalescidas_por_trecho() {
        let prof: Vec<ProfileSample> = [0.0, 2.0, 2.0, 0.0, 2.0, 2.0, 2.0, 0.0, 2.0]
            .iter()
            .enumerate()
            .map(|(i, top)| ProfileSample {
                x: i as f64,
                top: *top,
                bottom: 0.0,
                overlaps: 1,
            })
            .collect();
        // Dois trechos acima de y = 1; a amostra isolada no fim não vira linha.
        assert_eq!(runs_above(&prof, 1.0), vec![(1, 3), (4, 7)]);
        assert_eq!(runs_above(&prof, 3.0), Vec::<(usize, usize)>::new());
        assert_eq!(runs_above(&prof, -1.0), vec![(0, 9)]);
        assert!(runs_above(&[], 1.0).is_empty());
    }

    #[test]
    fn dashboard_fica_dentro_dos_cartoes() {
        let palette = Palette::light();
        let mut est = empty_estimates();
        est.time_hours = 2.75;
        est.dilution_pct = 12.0;
        est.costs = CostSummary {
            powder_cost: 120.0,
            gas_cost: 15.5,
            labor_cost: 80.0,
            machine_cost: 40.0,
            powder_kg: 0.42,
            gas_m3: 2.1,
            currency: "R$".to_string(),
            ..Default::default()
        };
        let params = Params {
            num_camadas: 4,
            ..Default::default()
        };
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(900.0, 620.0));
        let ctx = Context::default();
        let input = RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input.clone(), |ui| {
            process_dashboard(ui, &est, &params, &palette)
        });
        out.textures_delta.clear();
        // Cada cartão pinta com um painter recortado ao seu retângulo: setas,
        // cotas e textos têm de caber nele (nada vaza para o vizinho).
        let mut shapes_in_cards = 0usize;
        for clipped in &out.shapes {
            let clip = clipped.clip_rect;
            if clip.width() >= screen.width() * 0.9 {
                continue; // painter sem recorte: fundos dos cartões / raiz
            }
            let bounds = clipped.shape.visual_bounding_rect();
            if !bounds.is_finite() || !bounds.is_positive() {
                continue;
            }
            shapes_in_cards += 1;
            assert!(
                clip.expand(2.0).contains_rect(bounds),
                "forma {bounds:?} vaza do cartão {clip:?}"
            );
        }
        assert!(
            shapes_in_cards > 50,
            "poucas formas nos cartões: {shapes_in_cards}"
        );
    }
}
