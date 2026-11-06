//! Modo Bolacha: porta de `_generate_bolacha` / `_build_bolacha_segment`
//! (`generator.py:595-788`).
//!
//! Revestimento de uma face circular (disco). O eixo X é o **raio** da peça e
//! o eixo A gira a mesa; cada camada sobe `espessura_camada` em Z e começa 15°
//! adiante da anterior para não alinhar as costuras. Dois traçados:
//!
//! * **Espiral Contínua** (Arquimedes): X avança `passo_radial` por volta de
//!   360°, discretizada em passos de 10°, com passos extras no fim para a
//!   sobreposição angular;
//! * **Anéis (Passo a Passo)**: `num_voltas + 1` anéis concêntricos de 360°,
//!   ligados por uma transição em espiral durante o overlap angular ou, com
//!   `bolacha_stop_between`, com a tocha apagada e Z recuado entre anéis.

use std::f64::consts::PI;

use super::common::{
    append_torch_disable, append_torch_enable, apply_smart_layers, build_footer, build_header, f1,
    f3, layer_separator, py_max, py_min, Pos,
};
use super::GenError;
use crate::params::{BolachaTipo, Direction, Params};

/// Resolução angular da espiral contínua (`deg_per_step`).
const DEG_PER_STEP: f64 = 10.0;

/// `_generate_bolacha`: cabeçalho + um segmento por camada + rodapé.
///
/// O Python passa `current_d = params['diametro']` a cada segmento, mas o
/// argumento (`face_diameter`) nunca é lido — o segmento usa `diametro`
/// diretamente — por isso não há parâmetro equivalente aqui.
pub fn generate(p: &Params, timestamp: &str) -> Result<Vec<String>, GenError> {
    let mut body: Vec<String> = Vec::new();
    for i in 0..p.num_camadas.max(0) {
        body.extend(build_segment(p, i));
    }
    let mut out = build_header(p, timestamp);
    out.extend(body);
    out.extend(build_footer(p));
    Ok(out)
}

/// Avanço angular (graus/min) que mantém a velocidade tangencial no raio `x_pos`:
/// `rpm = min(taxa / (2π·max(x, 2.0)), 20.0)`, devolvendo `rpm * 360`.
/// Mantém a guarda `if current_circ > 0 else 1.0` do Python.
fn angular_feed(taxa_de_deposicao: f64, x_pos: f64) -> f64 {
    let current_circ = 2.0 * PI * py_max(x_pos, 2.0);
    let rpm = if current_circ > 0.0 {
        taxa_de_deposicao / current_circ
    } else {
        1.0
    };
    let rpm = py_min(rpm, 20.0);
    rpm * 360.0
}

/// `_build_bolacha_segment`: uma camada da bolacha.
fn build_segment(p: &Params, layer_num: i64) -> Vec<String> {
    if p.bolacha_oscilar {
        return super::bolacha_osc::build_segment(p, layer_num);
    }
    let afastamento = p.afastamento_tocha;
    let z_safe_offset = p.afastamento_seguranca;
    let z_layer = (layer_num as f64 * p.espessura_camada) + afastamento;
    let z_seguranca = z_layer + z_safe_offset;

    // Na bolacha X é o raio da peça; o avanço em X é o passo radial.
    let r_ext = p.diametro / 2.0;
    // O X mínimo é meia largura de cordão, para não sobrepor demais no centro.
    let r_int = py_max(p.diametro_interno / 2.0, p.largura_cordao / 2.0);
    let radial_width = py_max(0.0, r_ext - r_int);

    let taxa_de_deposicao = apply_smart_layers(p, layer_num, p.taxa());
    let larg_cordao = p.largura_cordao;
    let sobreposicao = p.sobreposicao;
    let angular_overlap = p.overlap_angular_deg;

    // Passo radial baseado na sobreposição.
    let passo_radial = larg_cordao * (1.0 - (sobreposicao / 100.0));
    let passo_radial = py_max(passo_radial, 1e-6);

    // Número de voltas (passes radiais); `math.ceil` devolve int no Python.
    let num_voltas: i64 = if radial_width > 0.0 {
        (radial_width / passo_radial).ceil() as i64
    } else {
        1
    };
    let rotation_sign = p.rotation_sign();

    // Início em X (raio) e sentido do avanço radial. O Python também calcula
    // `x_end`, mas nunca o lê.
    let (x_start, x_sign) = match p.direcao_soldagem {
        // Do interior para o exterior.
        Direction::EsquerdaDireita => (r_int, 1.0),
        // Do exterior para o interior.
        Direction::DireitaEsquerda => (r_ext, -1.0),
    };
    // `max(r_int, min(r_ext, x))`: garante limites entre r_int e r_ext.
    let clamp_r = |x: f64| py_max(r_int, py_min(r_ext, x));

    let mut seg = vec![
        format!("(--- CAMADA {} ---)", layer_num + 1),
        "(MODO: BOLACHA - FACE CIRCULAR)".to_string(),
        format!("G00 Z{}", f3(z_seguranca)),
    ];

    // Início deslocado 15° por camada para evitar costura alinhada.
    let mut current_a_total = (layer_num as f64 * 15.0) * rotation_sign;

    // Posição inicial em X e A, depois desce para o Z de trabalho.
    seg.push(format!("G00 X{} A{}", f3(x_start), f3(current_a_total)));
    seg.push(format!("G00 Z{}", f3(z_layer)));
    if layer_num == 0 {
        seg.push("M0 (Confirme e pressione Start)".to_string());
    }
    append_torch_enable(&mut seg, p);
    let mut last_weld_start = Pos::xa(x_start, current_a_total);
    let mut last_weld_end = Pos::xa(x_start, current_a_total);

    // Avanço angular da última interpolação; alimenta o apagamento final.
    // Os dois laços abaixo executam ao menos uma vez (`num_voltas >= 1`), então
    // este valor inicial nunca chega ao `append_torch_disable` — existe só para
    // o compilador, pois no Python a variável nasce dentro do laço.
    let mut feed_angular = 0.0;

    let stop_between = p.bolacha_stop_between;
    match p.bolacha_tipo {
        BolachaTipo::Aneis => {
            // Anéis concêntricos: N+1 anéis para cobrir o raio total.
            for v in 0..=num_voltas {
                let x_pos = clamp_r(x_start + (v as f64 * passo_radial * x_sign));

                seg.push(format!(
                    "(--- ANEL {}/{} em X={} ---)",
                    v + 1,
                    num_voltas + 1,
                    f3(x_pos)
                ));

                // Primeiro anel, ou retomada após parada: posiciona e liga.
                if v == 0 || stop_between {
                    seg.push(format!("G00 X{} A{}", f3(x_pos), f3(current_a_total)));
                    if stop_between && v > 0 {
                        // Desce Z (veio do Z de segurança) e religa a tocha.
                        seg.push(format!("G00 Z{}", f3(z_layer)));
                        append_torch_enable(&mut seg, p);
                    }
                }

                // Uma volta completa (360°) com X constante.
                feed_angular = angular_feed(taxa_de_deposicao, x_pos);
                let target_a_360 = current_a_total + (rotation_sign * 360.0);
                seg.push(format!("G01 F{} A{}", f1(feed_angular), f3(target_a_360)));
                // O Python grava aqui `last_weld_start/end = {X: x_pos, A: ..}`, mas
                // todos os ramos abaixo os sobrescrevem antes de qualquer leitura;
                // a atribuição morta é omitida para não gerar warning.
                current_a_total = target_a_360;

                // Sobreposição angular.
                let target_a_overlap = current_a_total + (rotation_sign * angular_overlap);

                if v < num_voltas {
                    let next_v = v + 1;
                    let next_x = clamp_r(x_start + (next_v as f64 * passo_radial * x_sign));

                    if stop_between {
                        // Overlap no mesmo raio (X constante), depois desliga e sobe.
                        seg.push(format!("G01 A{}", f3(target_a_overlap)));
                        last_weld_start = Pos::xa(x_pos, current_a_total);
                        last_weld_end = Pos::xa(x_pos, target_a_overlap);
                        current_a_total = target_a_overlap;
                        append_torch_disable(
                            &mut seg,
                            p,
                            Some(&last_weld_start),
                            Some(&last_weld_end),
                            Some(feed_angular),
                            Some(taxa_de_deposicao),
                            Some(py_max(x_pos, 1e-6)),
                        );
                        seg.push(format!("G00 Z{}", f3(z_seguranca)));
                    } else {
                        // Transição em espiral (step-over suave) durante o overlap.
                        seg.push(format!("G01 X{} A{}", f3(next_x), f3(target_a_overlap)));
                        last_weld_start = Pos::xa(x_pos, current_a_total);
                        last_weld_end = Pos::xa(next_x, target_a_overlap);
                        current_a_total = target_a_overlap;
                    }
                } else {
                    // Último anel: overlap final no mesmo raio.
                    seg.push(format!("G01 A{}", f3(target_a_overlap)));
                    last_weld_start = Pos::xa(x_pos, current_a_total);
                    last_weld_end = Pos::xa(x_pos, target_a_overlap);
                    current_a_total = target_a_overlap;
                }
            }
        }
        BolachaTipo::EspiralContinua => {
            // Espiral de Arquimedes: a cada volta de 360° X avança `passo_radial`;
            // a volta é dividida em passos angulares para manter a trajetória.
            let steps_per_rev = (360.0 / DEG_PER_STEP) as i64;
            // Passos extras no fim da última volta para a sobreposição angular
            // (pelo menos 1 se angular_overlap > 0).
            let extra_steps: i64 = if angular_overlap > 0.0 {
                (angular_overlap / DEG_PER_STEP).ceil() as i64
            } else {
                0
            };
            let total_rev_steps = num_voltas * steps_per_rev + extra_steps;

            for s in 0..total_rev_steps {
                // Progresso total (0.0 a num_voltas).
                let total_progress = (s + 1) as f64 / steps_per_rev as f64;

                // Raio interpolado continuamente; trava no raio final na sobreposição.
                let x_pos = if total_progress <= num_voltas as f64 {
                    x_start + (total_progress * passo_radial * x_sign)
                } else {
                    x_start + (num_voltas as f64 * passo_radial * x_sign)
                };
                let x_pos = clamp_r(x_pos);

                // O avanço angular depende do raio local (velocidade tangencial).
                feed_angular = angular_feed(taxa_de_deposicao, x_pos);

                let a_pos = current_a_total + (rotation_sign * DEG_PER_STEP);
                seg.push(format!(
                    "G01 F{} X{} A{}",
                    f1(feed_angular),
                    f3(x_pos),
                    f3(a_pos)
                ));
                // Python: `x_start if s == 0 else last_weld_end.get('X', x_pos)`.
                let start_x = if s == 0 {
                    x_start
                } else {
                    last_weld_end.x.unwrap_or(x_pos)
                };
                last_weld_start = Pos::xa(start_x, current_a_total);
                last_weld_end = Pos::xa(x_pos, a_pos);
                current_a_total = a_pos;
            }
        }
    }

    append_torch_disable(
        &mut seg,
        p,
        Some(&last_weld_start),
        Some(&last_weld_end),
        Some(feed_angular),
        Some(taxa_de_deposicao),
        Some(py_max(last_weld_end.x.unwrap_or(x_start), 1e-6)),
    );
    seg.push(format!("G00 Z{}", f3(z_seguranca)));
    seg.push(layer_separator(p, layer_num));
    seg
}
