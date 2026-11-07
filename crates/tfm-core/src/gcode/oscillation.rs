//! Modos de oscilação: porta de `_generate_linear_oscillation`,
//! `_generate_square_oscillation` e `_generate_square_test_oscillation`
//! (`generator.py:986-1479`).
//!
//! * **Linear** (`_build_linear_oscillation_segment`): por passo axial, um
//!   avanço em X seguido de uma volta completa em A; camadas defasadas 15°.
//! * **Quadrada** (`_build_square_oscillation_segment`): por passo axial,
//!   `n` passos angulares em que X vai e volta `oscilacao_comprimento` e A
//!   avança meio passo entre cada perna (movimento sequencial X -> A).
//! * **Quadrada contínua** (`_build_square_test_oscillation_segment`): como a
//!   quadrada, mas cada perna é decomposta em "escada" ([`Staircase`]) com a
//!   granularidade `osc_test_gran_x`/`osc_test_gran_a` e feeds independentes
//!   por eixo; o lead-out desacelera em dois trechos (85% e 60% do feed de X).
//!
//! Porta literal, com um desvio de propósito: o apagamento gira em A, na peça,
//! em vez de prolongar o último movimento em X ([`LayerFrame::rotary_tail`]).
//! No resto, mesma ordem das operações aritméticas, mesmos clamps
//! `max(.., min(.., ..))` (via `py_max`/`py_min`), mesmo idioma `x or padrao`
//! (`py_or`) e as mesmas posições `last_weld_*` que alimentam o apagamento
//! prolongado da tocha. As únicas omissões são atribuições do Python
//! sobrescritas antes de qualquer leitura (`current_x_pos`, `current_x` após o
//! último cordão e os `last_weld_*` intermediários de cada perna), que não
//! alteram a saída e gerariam `unused_assignments` em Rust.

use std::f64::consts::PI;

use super::common::{
    append_torch_disable, append_torch_enable, apply_smart_layers, build_footer, build_header,
    direction_str, effective_base_diameter, extinguish_arc_deg, f1, f3,
    fechamento_uniforme_comment, layer_separator, overlap_metrics, py_max, py_min, py_or, Pos,
};
use super::GenError;
use crate::deposit::{solve_band_factors, BandGroup};
use crate::overlap::{uniform_closing, UniformClosing};
use crate::params::{Direction, OscillationType, Params, WeldingMode};

const M0_CONFIRM: &str = "M0 (Confirme e pressione Start)";
const CLOSE_TO_END: &str = "(Fechamento ate fim do revestimento)";
const LEAD_OUT: &str = "(Movimento lead-out)";
const LINEAR_GEOM_ERROR: &str = "(ERRO: Diametro invalido para calculo angular)";
const SQUARE_GEOM_ERROR: &str = "(ERRO: Diametro ou deslocamento invalido para calculo angular)";

/// Teto de uma reserva antecipada de linhas (`Vec::reserve`): programas
/// maiores crescem por duplicação a partir daí. Evita que uma granularidade
/// minúscula peça de uma vez uma alocação absurda.
const RESERVE_CAP: usize = 4_000_000;
/// Amostras por faixa no cálculo da compensação das quadradas.
const BAND_SAMPLES: usize = 16;
/// Largura de cada faixa de X da compensação das quadradas (mm).
const SLOW_BIN_MM: f64 = 1.0;
/// Nas quadradas o apagamento gira este tanto mais rápido: o mesmo material
/// num arco maior não forma um calombo.
const TAIL_SPREAD: f64 = 4.0;

// ---------------------------------------------------------------------------
// Pontos de entrada
// ---------------------------------------------------------------------------

/// `_generate_linear_oscillation`.
pub fn generate_linear(p: &Params, timestamp: &str) -> Result<Vec<String>, GenError> {
    Ok(generate_layers(p, timestamp, build_linear_segment))
}

/// `_generate_square_oscillation`.
pub fn generate_square(p: &Params, timestamp: &str) -> Result<Vec<String>, GenError> {
    Ok(generate_layers(p, timestamp, build_square_segment))
}

/// `_generate_square_test_oscillation` (operação "Quadrada Contínua").
pub fn generate_square_continuous(p: &Params, timestamp: &str) -> Result<Vec<String>, GenError> {
    Ok(generate_layers(
        p,
        timestamp,
        build_square_continuous_segment,
    ))
}

/// Laço de camadas comum aos três `_generate_*`: cabeçalho, um segmento por
/// camada com o diâmetro de trabalho crescendo (externo) ou diminuindo
/// (interno) `2 * espessura_camada`, rodapé.
fn generate_layers(
    p: &Params,
    timestamp: &str,
    build_segment: fn(&Params, i64, f64) -> Vec<String>,
) -> Vec<String> {
    let mut out = build_header(p, timestamp);
    let mut current_d = effective_base_diameter(p);
    let step = 2.0 * p.espessura_camada;
    let d_sign = if p.is_internal() { -1.0 } else { 1.0 };
    let layers = p.num_camadas.max(0);
    for i in 0..layers {
        let seg = build_segment(p, i, current_d);
        if i == 0 {
            // As camadas têm o mesmo tamanho: reserva tudo de uma vez em vez
            // de duplicar o `Vec` dezenas de vezes num programa de 10⁶ linhas.
            let total = seg
                .len()
                .saturating_mul(usize::try_from(layers).unwrap_or(1));
            out.reserve(total.min(RESERVE_CAP));
        }
        out.extend(seg);
        current_d += d_sign * step;
    }
    out.extend(build_footer(p));
    out
}

/// `math.ceil(part_length / passo) if part_length > 0 else 1`.
fn axial_step_count(part_length: f64, passo: f64) -> i64 {
    if part_length > 0.0 {
        (part_length / passo).ceil() as i64
    } else {
        1
    }
}

// ---------------------------------------------------------------------------
// Geometria comum aos três modos
// ---------------------------------------------------------------------------

/// Bloco inicial repetido nos três `_build_*_segment`: alturas Z, limites em X
/// do revestimento e grandezas derivadas do diâmetro de trabalho da camada.
struct LayerFrame {
    z_layer: f64,
    z_seguranca: f64,
    is_internal: bool,
    direction: Direction,
    part_length: f64,
    /// `x_start_revest`.
    x_start: f64,
    /// `x_end_revest`.
    x_end: f64,
    /// `x_exit`: só existe na operação interna (folga da face).
    x_exit: Option<f64>,
    /// `math.pi * max(current_d, 1e-6)`.
    circunferencia: f64,
    /// `rotary_radius_mm = max(current_d, 1e-6) / 2.0`.
    rotary_radius: f64,
    rotation_sign: f64,
    /// Fechamento uniforme: o comprimento vale de borda a borda do material.
    closing: bool,
    /// Largura efetiva do cordão nesta camada (0 sem o fechamento uniforme).
    bead_width: f64,
    /// Fechamento uniforme: primeira e última posição do centro do cordão
    /// dentro da peça, já com o recuo de meia largura e sem os leads.
    x_first: f64,
    x_last: f64,
}

impl LayerFrame {
    fn new(p: &Params, current_d: f64) -> LayerFrame {
        let base_d = effective_base_diameter(p);
        let radial_delta = (current_d - base_d).abs() / 2.0;
        // Python: z_mag = radial_delta + afastamento + z_offset, com z_offset = 0.0.
        let z_layer = radial_delta + p.afastamento_tocha;
        let z_seguranca = z_layer + p.afastamento_seguranca;
        let is_internal = p.is_internal();
        let part_length = p.comprimento_revestir;
        // Na interna a direção é ignorada: sempre de -L até 0.
        let (mut x_start, mut x_end, x_exit) = if is_internal {
            (-part_length, 0.0, Some(p.face_clearance()))
        } else {
            match p.direcao_soldagem {
                Direction::EsquerdaDireita => (0.0 - p.lead_in, part_length + p.lead_out, None),
                // X0 é o início; o revestimento avança para X negativo.
                Direction::DireitaEsquerda => (0.0 + p.lead_in, -(part_length + p.lead_out), None),
            }
        };
        // Fechamento uniforme: o centro do cordão recua meia largura de cada
        // lado; peça mais estreita que o cordão fica com o centro no meio.
        let closing = p.fechamento_uniforme;
        let bead_width = if closing {
            overlap_metrics(p, 0, Some(current_d), None)
                .bead
                .effective_width_mm
        } else {
            0.0
        };
        let recuo = py_max(py_min(bead_width, part_length), 0.0) / 2.0;
        let sentido = if is_internal || p.direcao_soldagem == Direction::EsquerdaDireita {
            1.0
        } else {
            -1.0
        };
        let origem = if is_internal { -part_length } else { 0.0 };
        if closing {
            x_start += sentido * recuo;
            x_end -= sentido * recuo;
        }
        let d_floor = py_max(current_d, 1e-6);
        LayerFrame {
            z_layer,
            z_seguranca,
            is_internal,
            direction: p.direcao_soldagem,
            part_length,
            x_start,
            x_end,
            x_exit,
            circunferencia: PI * d_floor,
            rotary_radius: d_floor / 2.0,
            rotation_sign: p.rotation_sign(),
            closing,
            bead_width,
            x_first: origem + sentido * recuo,
            x_last: origem + sentido * (part_length - recuo),
        }
    }

    /// Clamp ao trecho revestido: `max(-(L), min(0.0, x))` na operação interna
    /// e em direita->esquerda; `max(0.0, min(L, x))` em esquerda->direita
    /// externa. (A linear escolhe por `passo_sign`/`is_internal`, com o mesmo
    /// resultado.)
    fn clamp_x(&self, x: f64) -> f64 {
        if self.closing {
            let (lo, hi) = (self.x_first.min(self.x_last), self.x_first.max(self.x_last));
            return py_max(lo, py_min(hi, x));
        }
        if self.is_internal || self.direction == Direction::DireitaEsquerda {
            py_max(-self.part_length, py_min(0.0, x))
        } else {
            py_max(0.0, py_min(self.part_length, x))
        }
    }

    /// As três primeiras linhas de todo segmento.
    fn segment_head(&self, layer_num: i64) -> Vec<String> {
        vec![
            format!("(--- CAMADA {} ---)", layer_num + 1),
            format!("(SENTIDO: {})", direction_str(self.direction)),
            format!("G00 Z{}", f3(self.z_seguranca)),
        ]
    }

    /// Comentário do fechamento uniforme, logo depois do sentido, como na
    /// espiral. Cada anel é uma volta inteira.
    fn push_closing_comment(&self, seg: &mut Vec<String>, passo: f64, aneis: i64) {
        let plano = UniformClosing {
            x_start: self.x_first,
            x_end: self.x_last,
            passo,
            voltas: aneis as f64,
        };
        seg.insert(2, fechamento_uniforme_comment(&plano));
    }

    /// O apagamento fica sempre em A, no lugar onde a solda terminou: em X, à
    /// velocidade da oscilação, a tocha ainda acesa andaria 100 mm para fora
    /// da peça. É um desvio de propósito do Python.
    fn rotary_tail(&self, tail: WeldTail) -> WeldTail {
        let (x, a) = (tail.end.x.unwrap_or(0.0), tail.end.a.unwrap_or(0.0));
        WeldTail {
            start: Pos::xa(x, a - self.rotation_sign),
            end: Pos::xa(x, a),
        }
    }

    /// Velocidades do apagamento nas quadradas: o mesmo material sai num arco
    /// maior, sem formar um calombo sobre a última banda.
    fn square_tail_speeds(&self, (feed, speed): (f64, f64)) -> (f64, f64) {
        (feed * TAIL_SPREAD, speed * TAIL_SPREAD)
    }

    /// Final de todo segmento: apagamento da tocha (prolongado ao longo do
    /// último cordão), recuo em Z, saída em X na interna e separador de camadas.
    fn finish_segment(
        &self,
        seg: &mut Vec<String>,
        p: &Params,
        layer_num: i64,
        tail: &WeldTail,
        speeds: (f64, f64),
    ) {
        let (feed, path_speed) = speeds;
        append_torch_disable(
            seg,
            p,
            Some(&tail.start),
            Some(&tail.end),
            Some(feed),
            Some(path_speed),
            Some(self.rotary_radius),
        );
        seg.push(format!("G00 Z{}", f3(self.z_seguranca)));
        if let Some(x_exit) = self.x_exit {
            seg.push(format!("G00 X{}", f3(x_exit)));
        }
        seg.push(layer_separator(p, layer_num));
    }
}

/// Último cordão depositado (`last_weld_start`/`last_weld_end`), que o
/// apagamento prolonga para manter o cordão durante o atraso da tocha.
struct WeldTail {
    start: Pos,
    end: Pos,
}

impl WeldTail {
    /// Início e fim no mesmo ponto (cordão ainda sem deslocamento).
    fn at(x: f64, a: f64) -> WeldTail {
        let pos = Pos::xa(x, a);
        WeldTail {
            start: pos,
            end: pos,
        }
    }

    fn set(&mut self, x0: f64, a0: f64, x1: f64, a1: f64) {
        self.start = Pos::xa(x0, a0);
        self.end = Pos::xa(x1, a1);
    }

    /// `(feed_value, path_speed_mm_min)` do apagamento: os da oscilação em X
    /// quando o último cordão se moveu em X
    /// (`abs(end.get('X', 0.0) - start.get('X', 0.0)) > 1e-9`), senão o feed
    /// angular e a taxa de deposição.
    fn torch_off_speeds(&self, feed_x: f64, feed_a: f64, taxa: f64) -> (f64, f64) {
        let moved_x = (self.end.x.unwrap_or(0.0) - self.start.x.unwrap_or(0.0)).abs() > 1e-9;
        if moved_x {
            (feed_x, feed_x)
        } else {
            (feed_a, taxa)
        }
    }
}

// ---------------------------------------------------------------------------
// Oscilação linear
// ---------------------------------------------------------------------------

/// `_build_linear_oscillation_segment`: uma camada de anéis sucessivos
/// (avanço em X, volta completa em A) defasados `layer_num * 15°`.
fn build_linear_segment(p: &Params, layer_num: i64, current_d: f64) -> Vec<String> {
    let fr = LayerFrame::new(p, current_d);
    let part_length = fr.part_length;
    let taxa_de_deposicao = apply_smart_layers(p, layer_num, p.taxa());

    // Primeiro anel e sentido do avanço axial.
    let (x_primeiro, passo_sign) = if fr.is_internal {
        (-part_length, 1.0)
    } else {
        match fr.direction {
            Direction::EsquerdaDireita => (0.0, 1.0),
            // X0 é o início; decrementa X (0 -> -passo -> -2*passo ...).
            Direction::DireitaEsquerda => (0.0, -1.0),
        }
    };

    let actual_passo_axial = py_max(p.largura_cordao * (1.0 - p.sobreposicao / 100.0), 1e-6);
    let num_passos_axiais = axial_step_count(part_length, actual_passo_axial);
    // Fechamento uniforme: o primeiro e o último anel encostam nas bordas.
    let (x_primeiro, actual_passo_axial, num_passos_axiais) = if fr.closing {
        let (passo, aneis) = linear_rings(&fr, actual_passo_axial);
        (fr.x_first, passo, aneis)
    } else {
        (x_primeiro, actual_passo_axial, num_passos_axiais)
    };
    let circunferencia = fr.circunferencia;
    if circunferencia <= 0.0 {
        return vec![LINEAR_GEOM_ERROR.to_string()];
    }
    let rotation_sign = fr.rotation_sign;
    let rpm_real = if circunferencia > 0.0 && taxa_de_deposicao > 0.0 {
        taxa_de_deposicao / circunferencia
    } else {
        0.0
    };
    let feed_angular = rpm_real * 360.0;
    // `velocidade_oscilacao_mm_min or taxa`: 0.0 conta como não informado.
    let feed_linear = py_or(p.velocidade_oscilacao_mm_min, taxa_de_deposicao);

    let mut seg = fr.segment_head(layer_num);
    if fr.closing {
        fr.push_closing_comment(&mut seg, actual_passo_axial, num_passos_axiais);
    }
    let mut current_a_total = (layer_num as f64 * 15.0) * rotation_sign;
    // Posiciona no início do revestimento.
    seg.push(format!("G00 X{} A{}", f3(fr.x_start), f3(current_a_total)));
    seg.push(format!("G00 Z{}", f3(fr.z_layer)));
    if layer_num == 0 {
        seg.push(M0_CONFIRM.to_string());
    }
    append_torch_enable(&mut seg, p);

    let mut current_x = fr.x_start;
    let mut tail = WeldTail::at(fr.x_start, current_a_total);

    for passo_idx in 0..num_passos_axiais {
        let x_nominal =
            fr.clamp_x(x_primeiro + passo_sign * actual_passo_axial * (passo_idx as f64));
        seg.push(format!(
            "(--- PASSO AXIAL {}/{} ---)",
            passo_idx + 1,
            num_passos_axiais
        ));
        if (x_nominal - current_x).abs() > 1e-6 {
            seg.push(format!("G01 F{} X{}", f1(feed_linear), f3(x_nominal)));
            current_x = x_nominal;
        }
        // No último anel o M102 vem antes: ainda sai material no apagamento,
        // e o cordão deve terminar quando o anel fecha 360°.
        let fecha_no_anel = fr.closing
            && passo_idx == num_passos_axiais - 1
            && (fr.is_internal || p.lead_out.abs() <= 1e-6);
        let arco = if fecha_no_anel {
            let apagamento = extinguish_arc_deg(p, taxa_de_deposicao, fr.rotary_radius);
            py_max(360.0 - apagamento, 0.0)
        } else {
            360.0
        };
        let target_a = current_a_total + rotation_sign * arco;
        seg.push(format!("G01 F{} A{}", f1(feed_angular), f3(target_a)));
        tail.set(current_x, current_a_total, current_x, target_a);
        current_a_total = target_a;
    }

    if fr.is_internal && (fr.x_end - current_x).abs() > 1e-6 {
        seg.push(format!("G01 F{} X{}", f1(feed_linear), f3(fr.x_end)));
        tail.set(current_x, current_a_total, fr.x_end, current_a_total);
    }
    if !fr.is_internal && p.lead_out.abs() > 1e-6 {
        seg.push(LEAD_OUT.to_string());
        seg.push(format!("G01 F{} X{}", f1(feed_linear), f3(fr.x_end)));
        tail.set(current_x, current_a_total, fr.x_end, current_a_total);
    }

    let tail = fr.rotary_tail(tail);
    let speeds = tail.torch_off_speeds(feed_linear, feed_angular, taxa_de_deposicao);
    fr.finish_segment(&mut seg, p, layer_num, &tail, speeds);
    seg
}

/// Fechamento uniforme da linear: `(espaçamento, anéis)` com o espaçamento
/// redistribuído no curso do centro do cordão, sem nunca crescer.
fn linear_rings(fr: &LayerFrame, passo: f64) -> (f64, i64) {
    let plano = uniform_closing(fr.x_first, fr.x_last, passo, 0.0);
    (plano.passo, plano.voltas as i64 + 1)
}

// ---------------------------------------------------------------------------
// Oscilação quadrada (sequencial e contínua)
// ---------------------------------------------------------------------------

/// Grandezas da oscilação quadrada, calculadas de forma idêntica nas
/// variantes sequencial e contínua (`generator.py:1167-1183` e `1334-1348`).
struct SquareGeom {
    osc_length: f64,
    /// Início do primeiro anel (`x_start_anel` do passo 0).
    x_base: f64,
    /// `true` no comportamento do Python: o último passo axial não oscila e
    /// só fecha em X até o fim. O fechamento uniforme dispensa esse passo.
    closing_step: bool,
    actual_passo_axial: f64,
    num_passos_axiais: i64,
    num_passos_angulares_por_volta: i64,
    actual_delta_a_deg: f64,
    feed_x: f64,
    feed_a: f64,
    /// Compensação: quantas vezes mais devagar em cada posição de X.
    slow: Option<SlowProfile>,
}

/// Fator de desaceleração por posição de X, em faixas de [`SLOW_BIN_MM`].
struct SlowProfile {
    x0: f64,
    factors: Vec<f64>,
}

impl SlowProfile {
    fn at(&self, x: f64) -> f64 {
        let i = ((x - self.x0) / SLOW_BIN_MM).max(0.0) as usize;
        self.factors
            .get(i.min(self.factors.len().saturating_sub(1)))
            .copied()
            .unwrap_or(1.0)
    }

    fn max(&self) -> f64 {
        self.factors.iter().copied().fold(1.0, f64::max)
    }
}

impl SquareGeom {
    /// `None` reproduz o retorno antecipado do Python
    /// (`["(ERRO: Diametro ou deslocamento invalido para calculo angular)"]`).
    fn new(p: &Params, fr: &LayerFrame) -> Option<SquareGeom> {
        // `params.get('taxa_de_deposicao', 0.0) or 0.0`, já normalizado.
        let taxa_de_deposicao = p.taxa();
        let osc_length = p.oscilacao_comprimento;
        let actual_passo_axial = py_max(osc_length * (p.sobreposicao / 100.0), 1e-6);
        let num_passos_axiais = axial_step_count(fr.part_length, actual_passo_axial);
        // Fechamento uniforme: a banda inteira cabe no curso do centro do
        // cordão; a oscilação encurta se for maior que ele.
        let (osc_length, actual_passo_axial, num_passos_axiais, x_base, closing_step) =
            if fr.closing {
                let (osc, passo, aneis) = square_bands(fr, osc_length, actual_passo_axial);
                (osc, passo, aneis, fr.x_first, false)
            } else {
                let x_base = if fr.is_internal { -fr.part_length } else { 0.0 };
                (
                    osc_length,
                    actual_passo_axial,
                    num_passos_axiais,
                    x_base,
                    true,
                )
            };
        let circunferencia = fr.circunferencia;
        let desloc_linear_angular = (p.deslocamento_angular_perc / 100.0) * p.largura_cordao;
        if circunferencia <= 0.0 || desloc_linear_angular <= 0.0 {
            return None;
        }
        let num_passos_angulares_por_volta = (circunferencia / desloc_linear_angular).ceil() as i64;
        let actual_delta_a_deg = 360.0 / num_passos_angulares_por_volta as f64;
        let rpm_a = if circunferencia > 0.0 && taxa_de_deposicao > 0.0 {
            taxa_de_deposicao / circunferencia
        } else {
            0.0
        };
        Some(SquareGeom {
            osc_length,
            x_base,
            closing_step,
            actual_passo_axial,
            num_passos_axiais,
            num_passos_angulares_por_volta,
            actual_delta_a_deg,
            // Feed de X pela velocidade da oscilação (`or taxa`: 0.0 = ausente).
            feed_x: py_or(p.velocidade_oscilacao_mm_min, taxa_de_deposicao),
            // Feed de A: RPM -> graus/min.
            feed_a: rpm_a * 360.0,
            slow: None,
        })
    }

    /// Calcula quanto desacelerar em cada posição de X para encher as pontas.
    /// Na sequencial o material sai nas pernas em A, nas duas extremidades da
    /// oscilação; o X, muito mais rápido, fica fixo. Na contínua ele sai na
    /// diagonal, e X e A desaceleram juntos.
    fn plan_slowdown(&mut self, p: &Params, fr: &LayerFrame, continuous: bool) {
        if !(fr.closing && p.compensar_rotacao) {
            return;
        }
        let voltas = self.num_passos_angulares_por_volta as f64;
        let perna_a = voltas * self.actual_delta_a_deg / 2.0 / self.feed_a.max(1e-9);
        let perna_x = voltas * self.osc_length / self.feed_x.max(1e-9);
        let faixa = |a: f64, b: f64, tempo: f64| -> Vec<(f64, f64)> {
            (0..BAND_SAMPLES)
                .map(|i| {
                    let t = (i as f64 + 0.5) / BAND_SAMPLES as f64;
                    (a + (b - a) * t, tempo / BAND_SAMPLES as f64)
                })
                .collect()
        };
        let (lo, hi) = (fr.x_first.min(fr.x_last), fr.x_first.max(fr.x_last));
        let bins = (((hi - lo) / SLOW_BIN_MM).ceil() as usize).max(1);
        let bin = |x: f64| (((x - lo) / SLOW_BIN_MM) as usize).min(bins - 1);
        let mut groups = vec![BandGroup::default(); bins];
        let mut fixed = BandGroup::default();
        for anel in 0..self.num_passos_axiais {
            let (ida, volta) = self.ring_x(fr, anel);
            if continuous {
                for (x, t) in faixa(volta, ida, 2.0 * (perna_a + perna_x)) {
                    groups[bin(x)].samples.push((x, t));
                }
            } else {
                groups[bin(ida)].samples.push((ida, perna_a));
                groups[bin(volta)].samples.push((volta, perna_a));
                fixed.samples.extend(faixa(volta, ida, 2.0 * perna_x));
            }
        }
        let factors = solve_band_factors(&groups, &fixed, lo, hi, fr.bead_width / 2.0);
        self.slow = Some(SlowProfile { x0: lo, factors });
    }

    /// Quantas vezes mais devagar que o nominal em `x`.
    fn slow_at(&self, x: f64) -> f64 {
        self.slow.as_ref().map_or(1.0, |s| s.at(x))
    }

    /// Comentário da compensação, logo depois do fechamento uniforme.
    fn push_slowdown_comment(&self, seg: &mut Vec<String>) {
        let Some(slow) = &self.slow else {
            return;
        };
        let maximo = slow.max();
        seg.insert(
            3,
            format!("(COMPENSACAO DE ROTACAO: ate {maximo:.2} vezes mais lento nas pontas)"),
        );
    }

    /// Defasagem angular entre camadas:
    /// `(layer_num * (actual_delta_A_deg * 0.25)) * rotation_sign`.
    fn stagger_a(&self, layer_num: i64, rotation_sign: f64) -> f64 {
        (layer_num as f64 * (self.actual_delta_a_deg * 0.25)) * rotation_sign
    }

    /// `(x_osc_ida, x_osc_volta)` do anel `passo_idx`, já limitados ao trecho
    /// revestido. O `x_osc_volta` do anel `i + 1` é também o `next_x` do
    /// `G00 X` entre passos axiais (mesma expressão no Python).
    fn ring_x(&self, fr: &LayerFrame, passo_idx: i64) -> (f64, f64) {
        let idx = passo_idx as f64;
        let (x_osc_ida, x_osc_volta) = if fr.is_internal {
            let x_start_anel = self.x_base + idx * self.actual_passo_axial;
            (x_start_anel + self.osc_length, x_start_anel)
        } else {
            match fr.direction {
                Direction::EsquerdaDireita => {
                    let x_start_anel = self.x_base + idx * self.actual_passo_axial;
                    (x_start_anel + self.osc_length, x_start_anel)
                }
                // X vai de 0 para -L: o passo i começa em -i*passo e oscila
                // até -(i*passo + osc_length).
                Direction::DireitaEsquerda => {
                    let x_start_anel = self.x_base - idx * self.actual_passo_axial;
                    (x_start_anel - self.osc_length, x_start_anel)
                }
            }
        };
        (fr.clamp_x(x_osc_ida), fr.clamp_x(x_osc_volta))
    }
}

/// Fechamento uniforme das quadradas: `(oscilação, passo, anéis)`. As bandas
/// vão da primeira à última posição do centro do cordão, com o passo
/// redistribuído sem nunca crescer. Sem os cortes no limite, nenhuma banda
/// acumula material na ponta.
fn square_bands(fr: &LayerFrame, osc_length: f64, passo: f64) -> (f64, f64, i64) {
    let curso = (fr.x_last - fr.x_first).abs();
    let osc = py_min(osc_length, curso);
    let plano = uniform_closing(fr.x_first, fr.x_last, passo, osc);
    (osc, plano.passo, plano.voltas as i64 + 1)
}

/// Fechamento uniforme, pelo diâmetro de base: o que as estimativas mostram.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OscillationClosing {
    /// Passo axial redistribuído (mm).
    pub passo: f64,
    /// Anéis (voltas inteiras).
    pub aneis: i64,
    /// Largura de material de cada anel: o cordão, mais o curso da oscilação
    /// nas quadradas.
    pub band_width: f64,
}

/// `true` nas duas quadradas, seguindo o mesmo despacho de `gcode::generate`.
fn is_square(p: &Params) -> bool {
    match p.welding_mode {
        WeldingMode::OscilacaoQuadrada => true,
        WeldingMode::Oscilacao => matches!(
            p.oscillation_type,
            Some(OscillationType::Quadrada | OscillationType::QuadradaContinua)
        ),
        _ => false,
    }
}

/// Plano do fechamento uniforme da oscilação selecionada em `p`.
pub fn oscillation_closing(p: &Params) -> OscillationClosing {
    let closing = Params {
        fechamento_uniforme: true,
        ..p.clone()
    };
    let fr = LayerFrame::new(&closing, effective_base_diameter(p));
    if is_square(p) {
        let passo = py_max(p.oscilacao_comprimento * (p.sobreposicao / 100.0), 1e-6);
        let (osc, passo, aneis) = square_bands(&fr, p.oscilacao_comprimento, passo);
        OscillationClosing {
            passo,
            aneis,
            band_width: fr.bead_width + osc,
        }
    } else {
        let passo = py_max(p.largura_cordao * (1.0 - p.sobreposicao / 100.0), 1e-6);
        let (passo, aneis) = linear_rings(&fr, passo);
        OscillationClosing {
            passo,
            aneis,
            band_width: fr.bead_width,
        }
    }
}

/// `_build_square_oscillation_segment`: movimento sequencial X -> A, sem
/// granularidade.
fn build_square_segment(p: &Params, layer_num: i64, current_d: f64) -> Vec<String> {
    let fr = LayerFrame::new(p, current_d);
    let Some(mut g) = SquareGeom::new(p, &fr) else {
        return vec![SQUARE_GEOM_ERROR.to_string()];
    };
    g.plan_slowdown(p, &fr, false);
    let taxa_de_deposicao = apply_smart_layers(p, layer_num, p.taxa());
    // Camada raiz mais lenta: o A solda; o X só herda a velocidade de solda
    // quando a da oscilação não foi informada.
    let raiz = apply_smart_layers(p, layer_num, 1.0);
    g.feed_a *= raiz;
    if p.velocidade_oscilacao_mm_min == 0.0 {
        g.feed_x *= raiz;
    }
    let rotation_sign = fr.rotation_sign;
    let n = g.num_passos_axiais;
    let fx = f1(g.feed_x);

    let mut seg = fr.segment_head(layer_num);
    if fr.closing {
        fr.push_closing_comment(&mut seg, g.actual_passo_axial, n);
        g.push_slowdown_comment(&mut seg);
    }
    let mut current_a_total = g.stagger_a(layer_num, rotation_sign);
    let mut tail = WeldTail::at(fr.x_start, current_a_total);
    for passo_idx in 0..n {
        let (x_osc_ida, x_osc_volta) = g.ring_x(&fr, passo_idx);
        let current_x_start_pos = if passo_idx == 0 {
            fr.x_start
        } else {
            x_osc_volta
        };

        seg.push(format!("(--- PASSO AXIAL {}/{} ---)", passo_idx + 1, n));
        seg.push(format!(
            "G00 X{} A{}",
            f3(current_x_start_pos),
            f3(current_a_total)
        ));
        seg.push(format!("G00 Z{}", f3(fr.z_layer)));
        if passo_idx == 0 {
            if layer_num == 0 {
                seg.push(M0_CONFIRM.to_string());
            }
            append_torch_enable(&mut seg, p);
        }
        tail = WeldTail::at(current_x_start_pos, current_a_total);

        if !fr.is_internal && passo_idx == 0 && p.lead_in.abs() > 1e-6 {
            seg.push(format!("G01 F{fx} X{}", f3(x_osc_volta)));
            tail.set(
                current_x_start_pos,
                current_a_total,
                x_osc_volta,
                current_a_total,
            );
        }

        if passo_idx == n - 1 && g.closing_step {
            seg.push(CLOSE_TO_END.to_string());
            if (fr.x_end - current_x_start_pos).abs() > 1e-6 {
                seg.push(format!("G01 F{fx} X{}", f3(fr.x_end)));
                tail.set(
                    current_x_start_pos,
                    current_a_total,
                    fr.x_end,
                    current_a_total,
                );
            }
        } else {
            let delta_a_half = rotation_sign * (g.actual_delta_a_deg / 2.0);
            let fa_ida = f1(g.feed_a / g.slow_at(x_osc_ida));
            let fa_volta = f1(g.feed_a / g.slow_at(x_osc_volta));
            for _ in 0..g.num_passos_angulares_por_volta {
                // Ida (X, depois A).
                let target_a_ida = current_a_total + delta_a_half;
                seg.push(format!("G01 F{fx} X{}", f3(x_osc_ida)));
                seg.push(format!("G01 F{fa_ida} A{}", f3(target_a_ida)));
                current_a_total = target_a_ida;
                // Volta (X, depois A).
                let target_a_volta = current_a_total + delta_a_half;
                seg.push(format!("G01 F{fx} X{}", f3(x_osc_volta)));
                seg.push(format!("G01 F{fa_volta} A{}", f3(target_a_volta)));
                tail.set(x_osc_volta, current_a_total, x_osc_volta, target_a_volta);
                current_a_total = target_a_volta;
            }
        }

        if passo_idx < n - 1 {
            let (_, next_x) = g.ring_x(&fr, passo_idx + 1);
            seg.push(format!("G00 X{}", f3(next_x)));
        }
    }

    // Sem o passo final em X, o fim extra pedido sai depois do último anel.
    if !g.closing_step && !fr.is_internal && p.lead_out.abs() > 1e-6 {
        let (_, x_fim) = g.ring_x(&fr, n - 1);
        seg.push(LEAD_OUT.to_string());
        seg.push(format!("G01 F{fx} X{}", f3(fr.x_end)));
        tail.set(x_fim, current_a_total, fr.x_end, current_a_total);
    }

    let tail = fr.rotary_tail(tail);
    let speeds =
        fr.square_tail_speeds(tail.torch_off_speeds(g.feed_x, g.feed_a, taxa_de_deposicao));
    fr.finish_segment(&mut seg, p, layer_num, &tail, speeds);
    seg
}

// ---------------------------------------------------------------------------
// Oscilação quadrada contínua (escada)
// ---------------------------------------------------------------------------

/// `staircase_move` da quadrada contínua: decompõe cada perna diagonal em
/// `S = max(steps_x, steps_a, 1)` subpassos, cada um `G01 F{feed_x} X..`
/// seguido de `G01 F{feed_a} A..`.
///
/// É o laço quente do gerador (um programa longo passa de 10⁶ linhas, quase
/// todas daqui): os prefixos `G01 F{feed:.1f} X`/`A` são formatados uma vez
/// por camada e cada linha é montada por [`motion_line`], que evita o
/// `core::fmt` de ponto flutuante (3-5x mais lento que o resto da linha).
struct Staircase {
    x_prefix: String,
    a_prefix: String,
    gran_x: f64,
    gran_a: f64,
}

impl Staircase {
    fn new(feed_x: f64, feed_a: f64, gran_x: f64, gran_a: f64) -> Staircase {
        Staircase {
            x_prefix: format!("G01 F{} X", f1(feed_x)),
            a_prefix: format!("G01 F{} A", f1(feed_a)),
            gran_x,
            gran_a,
        }
    }

    /// Estimativa (por cima) das linhas de uma camada, para reservar o `Vec`
    /// de uma vez: cada perna tem até `S` subpassos de duas linhas, mais o
    /// comentário por passo angular e as linhas fixas de cada passo axial.
    fn estimate_lines(&self, g: &SquareGeom) -> usize {
        let s = sub_steps(g.osc_length, self.gran_x)
            .max(sub_steps(g.actual_delta_a_deg / 2.0, self.gran_a))
            .max(1)
            .unsigned_abs() as usize;
        let per_k = 1 + 4 * s;
        let n_ang = usize::try_from(g.num_passos_angulares_por_volta).unwrap_or(0);
        let per_passo = 8usize.saturating_add(n_ang.saturating_mul(per_k));
        let n = usize::try_from(g.num_passos_axiais).unwrap_or(0);
        8usize
            .saturating_add(n.saturating_mul(per_passo))
            .min(RESERVE_CAP)
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &self,
        seg: &mut Vec<String>,
        x0: f64,
        a0: f64,
        x1: f64,
        a1: f64,
        slow: Option<&SlowProfile>,
        feeds: (f64, f64),
    ) {
        let dx = x1 - x0;
        let da = a1 - a0;
        let steps_x = sub_steps(dx, self.gran_x);
        let steps_a = sub_steps(da, self.gran_a);
        let s = steps_x.max(steps_a).max(1);
        for i in 1..=s {
            // Python: `xi = x0 + dx * (i / S)` — a fração antes do produto.
            let frac = i as f64 / s as f64;
            let xi = x0 + dx * frac;
            let ai = a0 + da * frac;
            match slow.map(|s| s.at(xi)).filter(|f| *f > 1.0) {
                Some(f) => {
                    seg.push(motion_line(&format!("G01 F{} X", f1(feeds.0 / f)), xi));
                    seg.push(motion_line(&format!("G01 F{} A", f1(feeds.1 / f)), ai));
                }
                None => {
                    seg.push(motion_line(&self.x_prefix, xi));
                    seg.push(motion_line(&self.a_prefix, ai));
                }
            }
        }
    }
}

/// `math.ceil(abs(d) / max(gran, 1e-6)) if abs(d) > 1e-9 else 0`.
fn sub_steps(d: f64, gran: f64) -> i64 {
    if d.abs() > 1e-9 {
        (d.abs() / py_max(gran, 1e-6)).ceil() as i64
    } else {
        0
    }
}

/// `_build_square_test_oscillation_segment`: pernas em escada com feeds
/// independentes por eixo e lead-out desacelerado em dois trechos.
fn build_square_continuous_segment(p: &Params, layer_num: i64, current_d: f64) -> Vec<String> {
    let fr = LayerFrame::new(p, current_d);
    let Some(mut g) = SquareGeom::new(p, &fr) else {
        return vec![SQUARE_GEOM_ERROR.to_string()];
    };
    g.plan_slowdown(p, &fr, true);
    let taxa_de_deposicao = apply_smart_layers(p, layer_num, p.taxa());
    // Camada raiz mais lenta: o A solda; o X só herda a velocidade de solda
    // quando a da oscilação não foi informada.
    let raiz = apply_smart_layers(p, layer_num, 1.0);
    g.feed_a *= raiz;
    if p.velocidade_oscilacao_mm_min == 0.0 {
        g.feed_x *= raiz;
    }
    let rotation_sign = fr.rotation_sign;
    let n = g.num_passos_axiais;
    let stairs = Staircase::new(g.feed_x, g.feed_a, p.osc_test_gran_x, p.osc_test_gran_a);

    let mut seg = fr.segment_head(layer_num);
    if fr.closing {
        fr.push_closing_comment(&mut seg, g.actual_passo_axial, n);
        g.push_slowdown_comment(&mut seg);
    }
    seg.reserve(stairs.estimate_lines(&g));
    // Stagger angular intercamadas.
    let mut current_a_total = g.stagger_a(layer_num, rotation_sign);
    let mut tail = WeldTail::at(fr.x_start, current_a_total);
    for passo_idx in 0..n {
        let (x_osc_ida, x_osc_volta) = g.ring_x(&fr, passo_idx);
        let mut current_x_start_pos = if passo_idx == 0 {
            fr.x_start
        } else {
            x_osc_volta
        };
        let current_x_end_pos_final_volta = if passo_idx == n - 1 {
            fr.x_end
        } else {
            x_osc_volta
        };

        seg.push(format!("(--- PASSO AXIAL {}/{} ---)", passo_idx + 1, n));
        // Aproxima ao início do anel.
        seg.push(format!(
            "G00 X{} A{}",
            f3(current_x_start_pos),
            f3(current_a_total)
        ));
        seg.push(format!("G00 Z{}", f3(fr.z_layer)));
        if passo_idx == 0 {
            if layer_num == 0 {
                seg.push(M0_CONFIRM.to_string());
            }
            append_torch_enable(&mut seg, p);
            // Lead-in somente em X.
            if !fr.is_internal && p.lead_in.abs() > 1e-6 {
                seg.push(format!("G01 F{} X{}", f1(g.feed_x), f3(x_osc_volta)));
                tail.set(
                    current_x_start_pos,
                    current_a_total,
                    x_osc_volta,
                    current_a_total,
                );
            }
        }

        if passo_idx == n - 1 && g.closing_step {
            // Passo final: fecha somente em X até o término, sem rotação.
            seg.push(CLOSE_TO_END.to_string());
            let dist_ate_fim = (fr.x_end - current_x_start_pos).abs();
            if dist_ate_fim > 1e-6 {
                seg.push(format!("G01 F{} X{}", f1(g.feed_x), f3(fr.x_end)));
                tail.set(
                    current_x_start_pos,
                    current_a_total,
                    fr.x_end,
                    current_a_total,
                );
            }
        } else {
            // Oscilação por passos angulares com decomposição em escada. Perto
            // das pontas cada subpasso tem o F da sua posição em X.
            let slow = g.slow.as_ref();
            let delta_a_half = rotation_sign * (g.actual_delta_a_deg / 2.0);
            for k in 0..g.num_passos_angulares_por_volta {
                seg.push(format!(
                    "(Passo angular {}/{})",
                    k + 1,
                    g.num_passos_angulares_por_volta
                ));
                // Ida em X e A.
                let target_a_ida = current_a_total + delta_a_half;
                stairs.emit(
                    &mut seg,
                    current_x_start_pos,
                    current_a_total,
                    x_osc_ida,
                    target_a_ida,
                    slow,
                    (g.feed_x, g.feed_a),
                );
                current_a_total = target_a_ida;
                current_x_start_pos = x_osc_ida;
                // Volta em X e A.
                let target_a_volta = current_a_total + delta_a_half;
                tail.set(
                    current_x_start_pos,
                    current_a_total,
                    x_osc_volta,
                    target_a_volta,
                );
                stairs.emit(
                    &mut seg,
                    current_x_start_pos,
                    current_a_total,
                    x_osc_volta,
                    target_a_volta,
                    slow,
                    (g.feed_x, g.feed_a),
                );
                current_a_total = target_a_volta;
                current_x_start_pos = x_osc_volta;
            }
        }

        if passo_idx < n - 1 {
            let (_, next_x) = g.ring_x(&fr, passo_idx + 1);
            seg.push(format!("G00 X{}", f3(next_x)));
        }

        if passo_idx == n - 1 && p.lead_out.abs() > 1e-6 {
            seg.push(LEAD_OUT.to_string());
            let back_mid = current_x_end_pos_final_volta
                - (current_x_end_pos_final_volta - current_x_start_pos) * 0.3;
            seg.push(format!("G01 F{} X{}", f1(g.feed_x * 0.85), f3(back_mid)));
            seg.push(format!(
                "G01 F{} X{}",
                f1(g.feed_x * 0.6),
                f3(current_x_end_pos_final_volta)
            ));
            tail.set(
                back_mid,
                current_a_total,
                current_x_end_pos_final_volta,
                current_a_total,
            );
        }
    }

    let tail = fr.rotary_tail(tail);
    let speeds =
        fr.square_tail_speeds(tail.torch_off_speeds(g.feed_x, g.feed_a, taxa_de_deposicao));
    fr.finish_segment(&mut seg, p, layer_num, &tail, speeds);
    seg
}

// ---------------------------------------------------------------------------
// Formatação rápida de `{:.3}` para o laço da escada
// ---------------------------------------------------------------------------

/// Buffer de pilha de [`motion_line`]: prefixo (<= [`MAX_PREFIX`]) + sinal +
/// até 16 dígitos inteiros (|v| < 2^52) + ponto + 3 decimais.
const LINE_BUF: usize = 64;
const MAX_PREFIX: usize = 40;

/// `format!("{prefix}{v:.3}")` com uma única alocação e sem o `core::fmt` de
/// ponto flutuante. Para |v| < 2^52 os dígitos saem de aritmética inteira
/// exata sobre a mantissa (ver [`write_f3`]); fora dessa faixa, ou com prefixo
/// longo demais, cai no `format!` padrão. `tests::motion_line_matches_format`
/// confere a igualdade byte a byte contra o `format!`.
fn motion_line(prefix: &str, v: f64) -> String {
    let mut buf = [0u8; LINE_BUF];
    if prefix.len() <= MAX_PREFIX {
        buf[..prefix.len()].copy_from_slice(prefix.as_bytes());
        if let Some(len) = write_f3(&mut buf, prefix.len(), v) {
            // Prefixo copiado inteiro de um `&str` + dígitos ASCII: UTF-8 válido.
            if let Ok(line) = std::str::from_utf8(&buf[..len]) {
                return line.to_string();
            }
        }
    }
    format!("{prefix}{v:.3}")
}

/// Escreve `v` com três decimais em `buf[n..]` exatamente como
/// `format!("{v:.3}")` — arredondamento do valor binário exato, meio exato
/// para o par, igual ao `flt2dec` do Rust e ao `%.3f` do Python — e devolve o
/// novo comprimento. `None` para |v| >= 2^52, infinito ou NaN.
fn write_f3(buf: &mut [u8; LINE_BUF], mut n: usize, v: f64) -> Option<usize> {
    let bits = v.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i64;
    if exp >= 1075 {
        return None;
    }
    let frac = bits & ((1u64 << 52) - 1);
    // v = mant * 2^e2, com e2 < 0 (subnormais: expoente fixo -1074).
    let (mant, e2) = if exp == 0 {
        (frac, -1074)
    } else {
        (frac | (1u64 << 52), exp - 1075)
    };
    // milli = round_half_even(mant * 1000 / 2^shift); mant * 1000 < 2^63.
    let scaled = mant * 1000;
    let shift = (-e2) as u32;
    let milli = if shift >= 64 {
        0 // scaled < 2^63 <= metade do divisor: arredonda para zero
    } else {
        let q = scaled >> shift;
        let r = scaled & ((1u64 << shift) - 1);
        let half = 1u64 << (shift - 1);
        if r > half || (r == half && q & 1 == 1) {
            q + 1
        } else {
            q
        }
    };
    if bits >> 63 != 0 {
        buf[n] = b'-';
        n += 1;
    }
    // Parte inteira (até 16 dígitos), montada de trás para a frente.
    let mut digits = [0u8; 20];
    let mut i = digits.len();
    let mut int_part = milli / 1000;
    loop {
        i -= 1;
        digits[i] = b'0' + (int_part % 10) as u8;
        int_part /= 10;
        if int_part == 0 {
            break;
        }
    }
    let int_len = digits.len() - i;
    buf[n..n + int_len].copy_from_slice(&digits[i..]);
    n += int_len;
    let f = (milli % 1000) as u32;
    buf[n] = b'.';
    buf[n + 1] = b'0' + (f / 100) as u8;
    buf[n + 2] = b'0' + (f / 10 % 10) as u8;
    buf[n + 3] = b'0' + (f % 10) as u8;
    Some(n + 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xorshift(x: &mut u64) -> u64 {
        *x ^= *x << 13;
        *x ^= *x >> 7;
        *x ^= *x << 17;
        *x
    }

    #[test]
    fn motion_line_matches_format() {
        let prefix = "G01 F3000.0 X";
        let check = |v: f64| {
            assert_eq!(
                motion_line(prefix, v),
                format!("{prefix}{v:.3}"),
                "v={v:e} bits={:#x}",
                v.to_bits()
            );
        };
        // Padrões de bits aleatórios: todos os expoentes, NaN, inf, subnormais.
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..100_000 {
            let bits = xorshift(&mut x);
            check(f64::from_bits(bits));
            // Coordenadas típicas: 1e-6..1e6, ambos os sinais.
            let m = (bits >> 11) as f64 / (1u64 << 53) as f64;
            let e = ((bits >> 3) % 13) as i32 - 6;
            let sign = if bits & 1 == 1 { -1.0 } else { 1.0 };
            check(m * 10f64.powi(e) * sign);
        }
        // Meios exatos (0.0625 = 62.5 milésimos) e vizinhanças de 0.0005.
        for i in -4000..=4000 {
            check(i as f64 * 0.0625);
            check(i as f64 * 0.0005);
            check(i as f64 / 1000.0);
            check(i as f64 * 0.1 + 0.0005);
        }
        // Subpassos `x0 + dx * (i / S)` como os da escada.
        for s in 1..=60u64 {
            for i in 1..=s {
                let frac = i as f64 / s as f64;
                check(-8.0 + 5.0 * frac);
                check(1440.0 + 1.8181818181818181 * frac);
            }
        }
        for v in [
            0.0,
            -0.0,
            1e-9,
            -1e-9,
            0.0005,
            0.0015,
            0.0625,
            0.1875,
            274.95,
            4503599627370495.5,
            4503599627370496.0,
            1e300,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
            f64::MIN_POSITIVE,
            5e-324,
            -5e-324,
            f64::MAX,
        ] {
            check(v);
        }
    }

    #[test]
    fn motion_line_falls_back_for_long_prefix() {
        let long = format!("G01 F{} X", "9".repeat(60));
        assert_eq!(motion_line(&long, 1.5), format!("{long}1.500"));
    }

    #[test]
    fn sub_steps_matches_python_ceil() {
        assert_eq!(sub_steps(0.0, 0.5), 0);
        assert_eq!(sub_steps(1e-10, 0.5), 0);
        assert_eq!(sub_steps(5.0, 0.5), 10);
        assert_eq!(sub_steps(-1.8181818181818181, 0.5), 4);
        // gran <= 0 usa o piso 1e-6 do Python (`max(gran_x, 1e-6)`).
        assert_eq!(sub_steps(1.0, 0.0), sub_steps(1.0, 1e-6));
    }
}
