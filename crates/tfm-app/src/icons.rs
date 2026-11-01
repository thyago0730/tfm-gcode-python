//! Ícones das operações desenhados com [`Painter`] (porta dos Canvas do Python).
//!
//! Cada ícone é descrito nas coordenadas do Canvas Tk original (40 × 35 px),
//! normalizado para 0..1 e escalado ao [`Rect`] pedido preservando a proporção
//! (o desenho fica centralizado quando o retângulo tem outra proporção).
//! Contorno da peça em `stroke`; cordão/percurso em `accent`.

use egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};
use tfm_core::params::Operation;

/// Dimensões do Canvas Tk de referência.
const CANVAS_W: f32 = 40.0;
const CANVAS_H: f32 = 35.0;

/// Corpo do cilindro (coordenadas do Canvas): 30 × 18 centralizado.
const X0: f32 = 5.0;
const X1: f32 = 35.0;
const Y0: f32 = 8.5;
const Y1: f32 = 26.5;
const Y_MID: f32 = 17.5;
/// Largura das elipses das tampas do cilindro.
const CAP: f32 = 10.0;

/// Espessuras (px no tamanho de referência de 40 px; escalam com o ícone).
const OUTLINE: f32 = 1.5;
const BEAD: f32 = 2.0;
const THIN: f32 = 1.25;

/// Desenha o ícone de `op` dentro de `rect`.
pub fn draw_operation_icon(
    painter: &Painter,
    rect: Rect,
    op: Operation,
    stroke: Color32,
    accent: Color32,
) {
    let c = Canvas::new(painter, rect);
    match op {
        Operation::Espiral => {
            cylinder(&c, stroke);
            spiral(&c, accent);
        }
        Operation::EspiralMultiperfil => {
            cylinder(&c, stroke);
            spiral_multiperfil(&c, accent);
        }
        Operation::OscilacaoLinear => {
            cylinder(&c, stroke);
            osc_linear(&c, accent);
        }
        Operation::OscilacaoQuadrada => {
            cylinder(&c, stroke);
            osc_square(&c, accent, 10.0, (Y1 - Y0) / 2.0 - 2.0, BEAD);
        }
        Operation::OscilacaoQuadradaContinua => {
            cylinder(&c, stroke);
            osc_square(&c, accent, 6.0, (Y1 - Y0) / 2.0 - 3.0, THIN);
        }
        Operation::ChapaPlana => flat(&c, stroke, accent),
        Operation::Bolacha => bolacha(&c, stroke, accent),
    }
}

/// Aloca `size` na `ui` e desenha o ícone de `op` nela (cartões do painel
/// esquerdo). Devolve a resposta da área, para hover/tooltip.
pub fn operation_icon(
    ui: &mut Ui,
    op: Operation,
    size: Vec2,
    stroke: Color32,
    accent: Color32,
) -> Response {
    let (response, painter) = ui.allocate_painter(size, Sense::hover());
    draw_operation_icon(&painter, response.rect, op, stroke, accent);
    response
}

/// Área útil do ícone com proporção 40:35 e conversão de coordenadas.
struct Canvas<'a> {
    painter: &'a Painter,
    area: Rect,
    /// Pixels de tela por pixel do Canvas de referência.
    unit: f32,
}

impl<'a> Canvas<'a> {
    fn new(painter: &'a Painter, rect: Rect) -> Self {
        let unit = (rect.width() / CANVAS_W)
            .min(rect.height() / CANVAS_H)
            .max(0.0);
        let area = Rect::from_center_size(rect.center(), vec2(CANVAS_W, CANVAS_H) * unit);
        Self {
            painter,
            area,
            unit,
        }
    }

    /// Ponto em coordenadas normalizadas (0..1 nos dois eixos).
    fn at(&self, u: f32, v: f32) -> Pos2 {
        pos2(
            self.area.min.x + u * self.area.width(),
            self.area.min.y + v * self.area.height(),
        )
    }

    /// Ponto nas coordenadas do Canvas de referência (40 × 35).
    fn px(&self, x: f32, y: f32) -> Pos2 {
        self.at(x / CANVAS_W, y / CANVAS_H)
    }

    fn stroke(&self, width: f32, color: Color32) -> Stroke {
        Stroke::new((width * self.unit).max(1.0), color)
    }

    fn segment(&self, a: (f32, f32), b: (f32, f32), width: f32, color: Color32) {
        self.painter.line_segment(
            [self.px(a.0, a.1), self.px(b.0, b.1)],
            self.stroke(width, color),
        );
    }

    fn polyline(&self, points: Vec<Pos2>, width: f32, color: Color32) {
        self.painter
            .add(Shape::line(points, self.stroke(width, color)));
    }

    fn circle(&self, cx: f32, cy: f32, r: f32, width: f32, color: Color32) {
        self.painter
            .circle_stroke(self.px(cx, cy), r * self.unit, self.stroke(width, color));
    }

    fn ellipse(&self, cx: f32, cy: f32, rx: f32, ry: f32, width: f32, color: Color32) {
        self.painter.add(Shape::ellipse_stroke(
            self.px(cx, cy),
            vec2(rx, ry) * self.unit,
            self.stroke(width, color),
        ));
    }

    /// Arco de elipse entre `start_deg` e `end_deg` (graus, sentido
    /// anti-horário a partir das 3 h, como no `create_arc` do Tk).
    #[allow(clippy::too_many_arguments)]
    fn arc(
        &self,
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        start_deg: f32,
        end_deg: f32,
        color: Color32,
    ) {
        const SEGMENTS: usize = 16;
        let points = (0..=SEGMENTS)
            .map(|i| {
                let t = start_deg + (end_deg - start_deg) * i as f32 / SEGMENTS as f32;
                let (sin, cos) = t.to_radians().sin_cos();
                self.px(cx + rx * cos, cy - ry * sin)
            })
            .collect();
        self.polyline(points, OUTLINE, color);
    }

    fn polygon(&self, corners: &[(f32, f32)], fill: Color32, color: Color32) {
        let points = corners.iter().map(|&(x, y)| self.px(x, y)).collect();
        self.painter.add(Shape::convex_polygon(
            points,
            fill,
            self.stroke(OUTLINE, color),
        ));
    }
}

/// Peça cilíndrica: geratrizes, tampa esquerda inteira e meia tampa direita.
fn cylinder(c: &Canvas, color: Color32) {
    let (rx, ry) = (CAP / 2.0, (Y1 - Y0) / 2.0);
    c.segment((X0 + CAP, Y0), (X1 - CAP, Y0), OUTLINE, color);
    c.segment((X0 + CAP, Y1), (X1 - CAP, Y1), OUTLINE, color);
    c.ellipse(X0 + rx, Y_MID, rx, ry, OUTLINE, color);
    c.arc(X1 - rx, Y_MID, rx, ry, -90.0, 90.0, color);
}

/// Espiral: passes verticais a cada 5 px a partir de `x0 + 3`.
fn spiral(c: &Canvas, accent: Color32) {
    let mut x = X0 + 3.0;
    while x < X1 - 2.0 {
        c.segment((x, Y0), (x, Y1), BEAD, accent);
        x += 5.0;
    }
}

/// Espiral multiperfil: três passes (20 %, 50 %, 80 %) e a diagonal do perfil.
fn spiral_multiperfil(c: &Canvas, accent: Color32) {
    for frac in [0.2, 0.5, 0.8] {
        let x = X0 + (X1 - X0) * frac;
        c.segment((x, Y0), (x, Y1), BEAD, accent);
    }
    c.segment((X0 + 2.0, Y0 + 3.0), (X1 - 2.0, Y1 - 3.0), THIN, accent);
}

/// Oscilação linear: zigue-zague com 7 passes ao longo do corpo.
fn osc_linear(c: &Canvas, accent: Color32) {
    const PASSES: usize = 7;
    let amp = (Y1 - Y0) / 2.0 - 2.0;
    let step = (X1 - X0) / PASSES as f32;
    let points = (0..=PASSES)
        .map(|i| {
            let y = if i % 2 == 0 { Y_MID - amp } else { Y_MID + amp };
            c.px(X0 + step * i as f32, y)
        })
        .collect();
    c.polyline(points, BEAD, accent);
}

/// Oscilação quadrada: onda quadrada de período `step` e amplitude `amp`.
fn osc_square(c: &Canvas, accent: Color32, step: f32, amp: f32, width: f32) {
    let mut points = vec![c.px(X0, Y_MID)];
    let mut x = X0;
    while x < X1 - step {
        points.extend([
            c.px(x, Y_MID - amp),
            c.px(x + step / 2.0, Y_MID - amp),
            c.px(x + step / 2.0, Y_MID + amp),
            c.px(x + step, Y_MID + amp),
        ]);
        x += step;
    }
    c.polyline(points, width, accent);
}

/// Chapa plana: paralelepípedo (topo, frente, lateral) e zigue-zague no topo.
fn flat(c: &Canvas, stroke: Color32, accent: Color32) {
    c.polygon(
        &[(5.0, 15.0), (15.0, 5.0), (35.0, 5.0), (25.0, 15.0)],
        stroke.gamma_multiply(0.10),
        stroke,
    );
    c.polygon(
        &[(5.0, 15.0), (25.0, 15.0), (25.0, 25.0), (5.0, 25.0)],
        Color32::TRANSPARENT,
        stroke,
    );
    c.polygon(
        &[(25.0, 15.0), (35.0, 5.0), (35.0, 15.0), (25.0, 25.0)],
        stroke.gamma_multiply(0.28),
        stroke,
    );
    c.polyline(
        vec![
            c.px(8.0, 12.0),
            c.px(32.0, 12.0),
            c.px(8.0, 8.0),
            c.px(32.0, 8.0),
        ],
        THIN,
        accent,
    );
}

/// Bolacha: disco externo e três círculos concêntricos do percurso.
fn bolacha(c: &Canvas, stroke: Color32, accent: Color32) {
    let (cx, cy) = (CANVAS_W / 2.0, CANVAS_H / 2.0);
    c.circle(cx, cy, 15.0, OUTLINE, stroke);
    for r in [4.0, 8.0, 12.0] {
        c.circle(cx, cy, r, THIN, accent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, LayerId};

    fn painter_for(ctx: &Context, layer: LayerId, rect: Rect) -> Painter {
        Painter::new(ctx.clone(), layer, rect)
    }

    #[test]
    fn todos_os_icones_produzem_formas() {
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(40.0, 35.0));
        for op in Operation::ALL {
            // Contexto novo por operação: a lista de formas começa vazia.
            let ctx = Context::default();
            let layer = LayerId::background();
            let painter = painter_for(&ctx, layer, rect);
            draw_operation_icon(&painter, rect, op, Color32::BLACK, Color32::RED);
            let drew = ctx.graphics_mut(|g| !g.entry(layer).is_empty());
            assert!(drew, "{} não desenhou nada", op.label());
        }
    }

    #[test]
    fn canvas_preserva_proporcao_e_normaliza() {
        let ctx = Context::default();
        let layer = LayerId::background();
        let square = Rect::from_min_size(pos2(10.0, 20.0), vec2(70.0, 70.0));
        let painter = painter_for(&ctx, layer, square);
        let c = Canvas::new(&painter, square);
        assert!((c.unit - 1.75).abs() < 1e-5, "unit {}", c.unit);
        assert_eq!(c.at(0.5, 0.5), square.center());
        assert_eq!(c.px(CANVAS_W, CANVAS_H), c.at(1.0, 1.0));
        assert!((c.area.width() - 70.0).abs() < 1e-4);
        assert!((c.area.height() - 61.25).abs() < 1e-4);
        assert!(c.area.min.y > square.min.y && c.area.max.y < square.max.y);
    }

    #[test]
    fn espessura_minima_de_um_pixel() {
        let ctx = Context::default();
        let tiny = Rect::from_min_size(Pos2::ZERO, vec2(8.0, 7.0));
        let painter = painter_for(&ctx, LayerId::background(), tiny);
        let c = Canvas::new(&painter, tiny);
        assert_eq!(c.stroke(THIN, Color32::RED).width, 1.0);
        let full = Rect::from_min_size(Pos2::ZERO, vec2(80.0, 70.0));
        let painter = painter_for(&ctx, LayerId::background(), full);
        let c = Canvas::new(&painter, full);
        assert_eq!(c.stroke(BEAD, Color32::RED).width, 4.0);
    }
}
