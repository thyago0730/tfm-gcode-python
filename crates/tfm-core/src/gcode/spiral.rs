//! Modo Espiral: porta de `_generate_spiral` / `_build_spiral_segment`
//! (`generator.py:401-483`).
//!
//! Hélice contínua X+A por camada; o diâmetro de trabalho cresce (externo) ou
//! diminui (interno) `2 * espessura_camada` por camada.

use std::f64::consts::PI;

use super::common::{
    append_torch_disable, append_torch_enable, build_footer, build_header, direction_str,
    effective_base_diameter, extinguish_arc_deg, f1, f3, fechamento_uniforme_comment,
    layer_separator, overlap_metrics, py_max, rotary_arc_bounds, Pos,
};
use super::GenError;
use crate::overlap::uniform_closing;
use crate::params::Params;

/// `_generate_spiral`: cabeçalho + uma hélice por camada + rodapé.
pub fn generate(p: &Params, timestamp: &str) -> Result<Vec<String>, GenError> {
    let mut body: Vec<String> = Vec::new();
    let base_d = effective_base_diameter(p);
    let mut current_d = base_d;
    let step = 2.0 * p.espessura_camada;
    let d_sign = if p.is_internal() { -1.0 } else { 1.0 };
    for i in 0..p.num_camadas.max(0) {
        body.extend(build_segment(p, i, current_d));
        current_d += d_sign * step;
    }
    let mut out = build_header(p, timestamp);
    out.extend(body);
    out.extend(build_footer(p));
    Ok(out)
}

/// `_build_spiral_segment`: uma camada da espiral.
fn build_segment(p: &Params, layer_num: i64, current_d: f64) -> Vec<String> {
    let afastamento = p.afastamento_tocha;
    let z_safe_offset = p.afastamento_seguranca;
    let base_d = effective_base_diameter(p);
    let radial_delta = (current_d - base_d).abs() / 2.0;
    // Python: z_mag = radial_delta + afastamento + z_offset, com z_offset = 0.0.
    let z_mag = radial_delta + afastamento;
    let is_internal = p.is_internal();
    let z_layer = z_mag;
    let z_seguranca = z_layer + z_safe_offset;
    let direction = direction_str(p.direcao_soldagem);
    let taxa_de_deposicao = p.taxa();
    let bounds = rotary_arc_bounds(p);
    let metrics = overlap_metrics(p, layer_num, Some(current_d), None);
    let passo = metrics.step_over_mm;
    // Fechamento uniforme: o comprimento vale de borda a borda do material,
    // então o centro do cordão recua meia largura de cada lado e as voltas
    // viram inteiras nesse curso. Desligado, é o cálculo do Python.
    let plano = p.fechamento_uniforme.then(|| {
        uniform_closing(
            bounds.x_start,
            bounds.x_end,
            passo,
            metrics.bead.effective_width_mm,
        )
    });
    let (x_arc_start_calc, x_arc_end_calc, passo, total_rotacoes) = match &plano {
        Some(c) => (c.x_start, c.x_end, c.passo, c.voltas),
        None => (
            bounds.x_start,
            bounds.x_end,
            passo,
            (bounds.x_end - bounds.x_start).abs() / passo,
        ),
    };
    let rotation_sign = p.rotation_sign();
    let angulo_total_a = total_rotacoes * 360.0 * rotation_sign;
    // Comprimento real da hélice por volta, para manter a velocidade de
    // deposição ao longo da trajetória combinada X+A.
    let circunferencia = PI * py_max(current_d, 1e-6);
    let comprimento_helice_por_volta = (circunferencia.powi(2) + passo.powi(2)).sqrt();
    let rpm_real = if comprimento_helice_por_volta > 0.0 && taxa_de_deposicao > 0.0 {
        taxa_de_deposicao / comprimento_helice_por_volta
    } else {
        0.0
    };
    let feed_angular = rpm_real * 360.0;

    let mut seg = vec![
        format!("(--- CAMADA {} ---)", layer_num + 1),
        format!("(SENTIDO: {direction})"),
    ];
    if let Some(c) = &plano {
        seg.push(fechamento_uniforme_comment(c));
    }
    seg.push(format!("G00 Z{}", f3(z_seguranca)));
    seg.push(format!("G00 X{} A0", f3(x_arc_start_calc)));
    seg.push(format!("G00 Z{}", f3(z_layer)));
    if layer_num == 0 {
        seg.push("M0 (Confirme e pressione Start)".to_string());
    }
    append_torch_enable(&mut seg, p);

    // Sem o anel a hélice só toca a borda em um ponto da volta: o fechamento
    // uniforme sempre fecha as duas pontas.
    let fechar = p.spiral_close_ends || p.fechamento_uniforme;
    let volta_fechamento = if fechar { rotation_sign * 360.0 } else { 0.0 };
    if fechar {
        let volta_a_ini = rotation_sign * 360.0;
        seg.push(format!("G01 F{} A{}", f1(feed_angular), f3(volta_a_ini)));
    }
    let end_x = x_arc_end_calc;
    let end_a = angulo_total_a + volta_fechamento;
    seg.push(format!(
        "G01 F{} X{} A{}",
        f1(feed_angular),
        f3(end_x),
        f3(end_a)
    ));
    let mut tail_start = Pos::xa(x_arc_start_calc, volta_fechamento);
    let mut tail_end = Pos::xa(end_x, end_a);
    if fechar {
        // No fechamento uniforme o material ainda sai durante o apagamento:
        // o M102 vem antes, para o cordão terminar quando o anel fecha 360°.
        let arco_final = if p.fechamento_uniforme {
            let raio = py_max(current_d, 1e-6) / 2.0;
            py_max(360.0 - extinguish_arc_deg(p, taxa_de_deposicao, raio), 0.0)
        } else {
            360.0
        };
        let volta_a_fim = end_a + rotation_sign * arco_final;
        seg.push(format!("G01 F{} A{}", f1(feed_angular), f3(volta_a_fim)));
        tail_start = Pos::xa(end_x, end_a);
        tail_end = Pos::xa(end_x, volta_a_fim);
    }
    append_torch_disable(
        &mut seg,
        p,
        Some(&tail_start),
        Some(&tail_end),
        Some(feed_angular),
        Some(taxa_de_deposicao),
        Some(py_max(current_d, 1e-6) / 2.0),
    );
    seg.push(format!("G00 Z{}", f3(z_seguranca)));
    if is_internal {
        if let Some(x_exit) = bounds.x_exit {
            seg.push(format!("G00 X{}", f3(x_exit)));
        }
    }
    seg.push(layer_separator(p, layer_num));
    seg
}
