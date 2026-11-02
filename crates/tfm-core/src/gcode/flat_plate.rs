//! Modo Chapa Plana: porta de `_generate_flat_plate` /
//! `_build_flat_plate_segment` (`generator.py:790-984`).
//!
//! Cordões retos paralelos sobre uma chapa: o eixo de solda é X (padrão) ou Y
//! (`flat_orientation`), e o passo lateral acontece no outro eixo. Em
//! zigue-zague a tocha fica acesa a camada inteira e os cordões alternam de
//! sentido; em sentido único (`flat_single_direction`) cada cordão é soldado no
//! mesmo sentido, com a tocha religada e apagada a cada passe.
//!
//! O Python tem dois ramos quase idênticos (orientação X e Y) que só diferem
//! nas letras dos eixos e na ordem em que as coordenadas entram no `Pos`;
//! aqui ambos passam por [`to_xy`], que faz esse mapeamento, mantendo a ordem
//! de emissão e de operações aritméticas do original. O único desvio de
//! propósito é o fim do cordão ([`stop_short`]).

use super::common::{
    append_torch_disable, append_torch_enable, apply_smart_layers, build_footer, build_header, f1,
    f3, layer_separator, py_max, torch_extinguish_delay, Pos,
};
use super::GenError;
use crate::params::{Direction, FlatOrientation, Params};

/// `_generate_flat_plate`: cabeçalho + um segmento por camada + rodapé.
pub fn generate(p: &Params, timestamp: &str) -> Result<Vec<String>, GenError> {
    let mut body: Vec<String> = Vec::new();
    for i in 0..p.num_camadas.max(0) {
        body.extend(build_segment(p, i)?);
    }
    let mut out = build_header(p, timestamp);
    out.extend(body);
    out.extend(build_footer(p));
    Ok(out)
}

/// Letras dos eixos `(solda, passo)` para a orientação.
fn axis_letters(o: FlatOrientation) -> (char, char) {
    match o {
        FlatOrientation::X => ('X', 'Y'),
        FlatOrientation::Y => ('Y', 'X'),
    }
}

/// Converte coordenadas `(solda, passo)` em `(X, Y)` da máquina.
fn to_xy(o: FlatOrientation, weld: f64, cross: f64) -> (f64, f64) {
    match o {
        FlatOrientation::X => (weld, cross),
        FlatOrientation::Y => (cross, weld),
    }
}

/// Dict Python `{'X': .., 'Y': ..}` a partir de coordenadas `(solda, passo)`.
fn pos(o: FlatOrientation, weld: f64, cross: f64) -> Pos {
    let (x, y) = to_xy(o, weld, cross);
    Pos {
        x: Some(x),
        y: Some(y),
        ..Pos::default()
    }
}

/// `G00 X{..} Y{..}` (sempre na ordem X, Y) a partir de `(solda, passo)`.
fn g00_xy(o: FlatOrientation, weld: f64, cross: f64) -> String {
    let (x, y) = to_xy(o, weld, cross);
    format!("G00 X{} Y{}", f3(x), f3(y))
}

/// Onde o cordão de `from` a `to` para, para o apagamento de `tail_mm` terminar
/// exatamente em `to`, em vez de passar da borda. Na chapa não há eixo A para
/// girar no lugar. Desvio de propósito do Python. Um cordão mais curto que o
/// apagamento vai até o fim, porque não há onde recuar.
fn stop_short(from: f64, to: f64, tail_mm: f64) -> f64 {
    if tail_mm <= 0.0 || tail_mm >= (to - from).abs() {
        return to;
    }
    to - (to - from).signum() * tail_mm
}

/// `_build_flat_plate_segment`: uma camada da chapa.
fn build_segment(p: &Params, layer_num: i64) -> Result<Vec<String>, GenError> {
    let afastamento = p.afastamento_tocha;
    let z_safe_offset = p.afastamento_seguranca;
    let z_layer = afastamento + (layer_num as f64 * p.espessura_camada);
    let z_seguranca = z_layer + z_safe_offset;

    let part_length = p.comprimento_revestir;
    let part_width = p.largura_revestir;

    let lead_in = p.lead_in;
    let lead_out = p.lead_out;

    // Mapeamento de eixos: por padrão solda em X e passo em Y.
    let orientation = p.flat_orientation;
    let (weld_letter, cross_letter) = axis_letters(orientation);

    // Passo lateral (largura).
    let larg_cordao = p.largura_cordao;
    let sobreposicao = p.sobreposicao;
    let step_cross = larg_cordao * (1.0 - (sobreposicao / 100.0));
    let step_cross = py_max(step_cross, 1e-6);

    let taxa_de_deposicao = p.taxa();
    let feed_rate = apply_smart_layers(p, layer_num, taxa_de_deposicao);
    // O que a tocha ainda deposita depois do M102.
    let tail_mm = feed_rate * (torch_extinguish_delay(p) / 60.0);

    // Limites do movimento de solda (mesma fórmula nas duas orientações).
    let (weld_start, weld_end) = match p.direcao_soldagem {
        Direction::EsquerdaDireita => (0.0 - lead_in, part_length + lead_out),
        // Direita -> Esquerda: começa em 0 e vai para -comprimento.
        Direction::DireitaEsquerda => (0.0 + lead_in, -(part_length + lead_out)),
    };

    // Limites do passo.
    let cross_start = 0.0;
    let passo_sign = 1.0;

    // math.ceil(part_width / step_cross): o Python levanta exceção em inf/NaN.
    let passes = (part_width / step_cross).ceil();
    if !passes.is_finite() {
        return Err(GenError::Invalid(format!(
            "número de passes da chapa plana não finito (largura {part_width}, passo {step_cross})"
        )));
    }
    let num_passes = passes as i64;
    let mut current_cross = cross_start;

    let mut seg = vec![
        format!("(--- CAMADA {} ---)", layer_num + 1),
        format!("(MODO: CHAPA PLANA - Orientação {})", weld_letter),
        format!("G00 Z{}", f3(z_seguranca)),
    ];

    // Posição inicial e descida para Z.
    seg.push(g00_xy(orientation, weld_start, current_cross));
    seg.push(format!("G00 Z{}", f3(z_layer)));

    if layer_num == 0 {
        seg.push("M0 (Confirme e pressione Start)".to_string());
    }

    let single_dir = p.flat_single_direction;
    if !single_dir {
        append_torch_enable(&mut seg, p);
    }
    let mut last_weld_start = pos(orientation, weld_start, current_cross);
    let mut last_weld_end = pos(orientation, weld_start, current_cross);

    // Alvo do cordão no zigue-zague; só é lido quando `!single_dir`.
    let mut target = weld_end;
    for i in 0..num_passes {
        if single_dir {
            // Sempre weld_start -> weld_end, religando a tocha a cada passe.
            seg.push(g00_xy(orientation, weld_start, current_cross));
            append_torch_enable(&mut seg, p);
            seg.push(format!("G00 Z{}", f3(z_layer)));
            let parada = stop_short(weld_start, weld_end, tail_mm);
            seg.push(format!(
                "G01 F{} {}{}",
                f1(feed_rate),
                weld_letter,
                f3(parada)
            ));
            last_weld_start = pos(orientation, weld_start, current_cross);
            last_weld_end = pos(orientation, parada, current_cross);
            append_torch_disable(
                &mut seg,
                p,
                Some(&last_weld_start),
                Some(&last_weld_end),
                Some(feed_rate),
                Some(feed_rate),
                None,
            );
            seg.push(format!("G00 Z{}", f3(z_seguranca)));
        } else {
            // Zigue-zague no eixo de solda.
            target = if i % 2 == 0 { weld_end } else { weld_start };
            let from = if i % 2 == 0 { weld_start } else { weld_end };
            // A tocha só apaga no último cordão da camada.
            let parada = if i == num_passes - 1 {
                stop_short(from, target, tail_mm)
            } else {
                target
            };
            seg.push(format!(
                "G01 F{} {}{}",
                f1(feed_rate),
                weld_letter,
                f3(parada)
            ));
            last_weld_start = pos(orientation, from, current_cross);
            last_weld_end = pos(orientation, parada, current_cross);
        }

        // Passo lateral.
        if i < num_passes - 1 {
            current_cross += step_cross * passo_sign;
            if !single_dir {
                seg.push(format!(
                    "G01 F{} {}{}",
                    f1(feed_rate),
                    cross_letter,
                    f3(current_cross)
                ));
                last_weld_start = pos(
                    orientation,
                    target,
                    current_cross - (step_cross * passo_sign),
                );
                last_weld_end = pos(orientation, target, current_cross);
            }
        }
    }

    // Retração e fim da camada.
    if !single_dir {
        append_torch_disable(
            &mut seg,
            p,
            Some(&last_weld_start),
            Some(&last_weld_end),
            Some(feed_rate),
            Some(feed_rate),
            None,
        );
    }
    seg.push(format!("G00 Z{}", f3(z_seguranca)));
    seg.push(layer_separator(p, layer_num));
    Ok(seg)
}
