//! Modo Espiral Multiperfil: porta de `_generate_spiral_multiperfil` /
//! `_build_spiral_multiperfil_layer` e dos auxiliares `_get_multiperfil_*`,
//! `_align_rotary_target` e `_offset_helix_segment_within_bounds`
//! (`generator.py:129-264` e `485-593`).
//!
//! Vários perfis (hélices X+A) distribuídos em ângulos iniciais ao longo de um
//! setor; cada perfil é formado por subpasses deslocados lateralmente em X e
//! recortados pelas faces da peça (opcionalmente chanfradas nas pontas). O
//! eixo A acumula entre subpasses e perfis, sempre avançando no sentido de
//! rotação. Antes do cabeçalho, `num_camadas` e `multiperfil_profile_height_mm`
//! são recalculados a partir do perfil, como faz o `generate()` do Python.

use std::f64::consts::PI;

use super::common::{
    append_torch_disable, append_torch_enable, apply_smart_layers, build_footer, build_header,
    effective_base_diameter, f1, f3, f3s, fechamento_uniforme_comment, overlap_metrics,
    profile_shape_str, py_max, py_min, py_or, rotary_arc_bounds, Pos,
};
use super::GenError;
use crate::overlap::uniform_closing;
use crate::params::{MultiperfilStrategy, Params, ProfileShape};

/// Definição geométrica do perfil (`_get_multiperfil_profile_definition`).
///
/// `profile_height` e `total_height` carregam o mesmo valor (o Python devolve
/// ambas as chaves iguais); `layer_count`/`layer_height` são, respectivamente,
/// `num_camadas`/`espessura_camada` do dicionário da UI (`TFM_GCODE.py`).
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileDef {
    pub shape: ProfileShape,
    pub base_width: f64,
    pub top_width: f64,
    pub layer_count: i64,
    pub layer_height: f64,
    pub profile_height: f64,
    pub total_height: f64,
    pub end_chamfer_enabled: bool,
    pub end_chamfer_angle_deg: f64,
}

/// `_get_multiperfil_profile_definition`: largura de base/topo pela forma,
/// número de camadas pela altura do perfil (ou vice-versa) e chanfro de ponta.
pub fn profile_definition(p: &Params) -> ProfileDef {
    let shape = p.multiperfil_profile_shape;
    let base_width = py_or(p.multiperfil_profile_base_mm, 0.0);
    let mut top_width = py_or(p.multiperfil_profile_top_mm, 0.0);
    let layer_height = py_max(py_or(p.espessura_camada, 0.0), 1e-6);
    let mut profile_height = py_or(p.multiperfil_profile_height_mm, 0.0);
    let end_chamfer_enabled = p.multiperfil_end_chamfer_enabled;
    let end_chamfer_angle_deg = py_or(p.multiperfil_end_chamfer_angle_deg, 0.0);
    let layer_count = if profile_height <= 0.0 {
        // max(int(params.get('num_camadas', 1) or 1), 1)
        let n = if p.num_camadas == 0 { 1 } else { p.num_camadas };
        let layer_count = n.max(1);
        profile_height = py_max(layer_count as f64 * layer_height, layer_height);
        layer_count
    } else {
        // `as i64` satura (sem pânico) para alturas absurdas vindas do usuário.
        ((profile_height / layer_height).ceil() as i64).max(1)
    };
    match shape {
        ProfileShape::Retangular => top_width = base_width,
        ProfileShape::Triangular => top_width = 0.0,
        ProfileShape::Trapezoidal => {}
    }
    let total_height = py_max(profile_height, layer_height);
    ProfileDef {
        shape,
        base_width: py_max(base_width, 0.0),
        top_width: py_max(top_width, 0.0),
        layer_count,
        layer_height,
        profile_height: total_height,
        total_height,
        end_chamfer_enabled: end_chamfer_enabled && end_chamfer_angle_deg > 0.0,
        end_chamfer_angle_deg: py_max(end_chamfer_angle_deg, 0.0),
    }
}

/// `_get_multiperfil_start_angles`: ângulos iniciais dos perfis. Setor
/// completo (>= 360°) distribui `total/count`; setor parcial distribui
/// `total/(count-1)` para que o último perfil caia exatamente no fim do setor.
pub fn start_angles(p: &Params) -> Vec<f64> {
    let n = if p.multiperfil_count == 0 {
        1
    } else {
        p.multiperfil_count
    };
    let profile_count = n.max(1);
    let total_angle_deg = py_max(py_or(p.multiperfil_total_angle_deg, 360.0), 0.0);
    if profile_count == 1 {
        return vec![0.0];
    }
    let step_deg = if total_angle_deg >= 360.0 {
        total_angle_deg / profile_count as f64
    } else {
        total_angle_deg / (profile_count - 1) as f64
    };
    (0..profile_count)
        .map(|idx| idx as f64 * step_deg)
        .collect()
}

/// `_align_rotary_target`: avança `target_a` em voltas inteiras (360°) no
/// sentido de rotação até não ficar "atrás" de `current_a`. `None` em
/// `current_a` devolve o alvo intacto (`if current_a is None`).
///
/// Defesa extra ao Python: valores não finitos devolvem o alvo intacto em vez
/// de girar para sempre.
pub fn align_rotary_target(target_a: f64, current_a: Option<f64>, rotation_sign: f64) -> f64 {
    let Some(current_a) = current_a else {
        return target_a;
    };
    if !target_a.is_finite() || !current_a.is_finite() {
        return target_a;
    }
    let mut aligned = target_a;
    let step = 360.0 * if rotation_sign >= 0.0 { 1.0 } else { -1.0 };
    if rotation_sign >= 0.0 {
        while aligned < current_a {
            aligned += step;
        }
    } else {
        while aligned > current_a {
            aligned += step;
        }
    }
    aligned
}

/// `_get_multiperfil_layer_end_chamfer`: recuo em X por lado na camada
/// `layer_num`, para que as pontas do perfil formem o ângulo de chanfro
/// (`y_centro / tan(min(angulo, 89.9°))`). Zero com chanfro desativado.
pub fn layer_end_chamfer(p: &Params, layer_num: i64) -> f64 {
    let profile = profile_definition(p);
    if !profile.end_chamfer_enabled {
        return 0.0;
    }
    let total_height = py_max(py_or(profile.total_height, 0.0), 1e-6);
    let layer_height = py_max(py_or(profile.layer_height, 0.0), 1e-6);
    let y_center = py_min(total_height, (layer_num.max(0) as f64 + 0.5) * layer_height);
    let angle_deg = py_or(profile.end_chamfer_angle_deg, 0.0);
    if angle_deg <= 0.0 {
        return 0.0;
    }
    let angle_rad = py_min(angle_deg, 89.9).to_radians();
    let retreat_by_angle = if angle_rad > 1e-9 {
        y_center / angle_rad.tan()
    } else {
        0.0
    };
    py_max(0.0, retreat_by_angle)
}

/// `_get_multiperfil_pass_offsets`: deslocamentos laterais dos subpasses da
/// camada `layer_num`, a largura alvo do perfil nessa altura e o número de
/// passes. Devolve `(offsets, largura_alvo, pass_count)`.
pub fn pass_offsets(p: &Params, layer_num: i64, current_d: Option<f64>) -> (Vec<f64>, f64, usize) {
    let profile = profile_definition(p);
    let y_center = py_min(
        profile.total_height,
        (layer_num as f64 + 0.5) * profile.layer_height,
    );
    let frac = if profile.total_height > 0.0 {
        y_center / profile.total_height
    } else {
        1.0
    };
    let target_width = profile.base_width + ((profile.top_width - profile.base_width) * frac);
    let metrics = overlap_metrics(p, layer_num, current_d, Some(target_width));
    (
        metrics.offsets,
        py_max(target_width, 0.0),
        metrics.pass_count,
    )
}

/// Porta de `_get_multiperfil_layer_profile_widths` (`TFM_GCODE.py`): largura
/// alvo do perfil em cada uma das `layer_count` camadas, interpolada entre a
/// base e o topo pela altura do centro da camada.
pub fn layer_profile_widths(p: &Params) -> Vec<f64> {
    let profile = profile_definition(p);
    let total_height = profile.total_height;
    let layer_height = profile.layer_height;
    let base_width = profile.base_width;
    let top_width = profile.top_width;
    (0..profile.layer_count.max(0))
        .map(|layer_idx| {
            let y_center = py_min(total_height, (layer_idx as f64 + 0.5) * layer_height);
            let frac = if total_height > 0.0 {
                y_center / total_height
            } else {
                1.0
            };
            let width = base_width + ((top_width - base_width) * frac);
            py_max(width, 0.0)
        })
        .collect()
}

/// `_offset_helix_segment_within_bounds`: desloca o segmento de hélice
/// `(x0, a0) -> (x1, a1)` em `offset_mm` ao longo da sua normal (no plano
/// X × arco desenrolado) e recorta a reta deslocada apenas pelos planos
/// `x_min`/`x_max` (as faces da peça). Devolve `(x0', a0', x1', a1')` em graus.
#[allow(clippy::too_many_arguments)]
pub fn offset_helix_segment_within_bounds(
    x0: f64,
    a0_deg: f64,
    x1: f64,
    a1_deg: f64,
    radius: f64,
    offset_mm: f64,
    x_min: f64,
    x_max: f64,
) -> (f64, f64, f64, f64) {
    let radius = py_max(py_or(radius, 0.0), 1e-9);
    let s0 = a0_deg.to_radians() * radius;
    let s1 = a1_deg.to_radians() * radius;
    let dx = x1 - x0;
    let ds = s1 - s0;
    let seg_len = (dx.powi(2) + ds.powi(2)).sqrt();
    if seg_len <= 1e-9 {
        return (x0, a0_deg, x1, a1_deg);
    }

    let normal_x = -ds / seg_len;
    let normal_s = dx / seg_len;

    let ox0 = x0 + (offset_mm * normal_x);
    let os0 = s0 + (offset_mm * normal_s);
    let ox1 = x1 + (offset_mm * normal_x);
    let os1 = s1 + (offset_mm * normal_s);

    let (t0, t1) = if dx.abs() <= 1e-9 {
        (0.0, 1.0)
    } else {
        // O perfil é definido pelas faces da peça (planos X constantes), não
        // pelo comprimento do segmento deslocado: corta-se só em x_min/x_max.
        let tx0 = (x_min - ox0) / (ox1 - ox0);
        let tx1 = (x_max - ox0) / (ox1 - ox0);
        let t0 = py_min(tx0, tx1);
        let t1 = py_max(tx0, tx1);
        if t0 > t1 {
            let a = (os0 / radius).to_degrees();
            return (ox0, a, ox0, a);
        }
        (t0, t1)
    };

    let cx0 = ox0 + ((ox1 - ox0) * t0);
    let cs0 = os0 + ((os1 - os0) * t0);
    let cx1 = ox0 + ((ox1 - ox0) * t1);
    let cs1 = os0 + ((os1 - os0) * t1);
    (
        cx0,
        (cs0 / radius).to_degrees(),
        cx1,
        (cs1 / radius).to_degrees(),
    )
}

/// `generate()` + `_generate_spiral_multiperfil`: recalcula `num_camadas` e a
/// altura do perfil a partir de [`profile_definition`] (o cabeçalho imprime
/// ambos), depois cabeçalho + uma camada multiperfil por camada + rodapé.
pub fn generate(p: &Params, timestamp: &str) -> Result<Vec<String>, GenError> {
    let mut p = p.clone();
    let profile = profile_definition(&p);
    p.num_camadas = profile.layer_count;
    p.multiperfil_profile_height_mm = profile.profile_height;
    let p = &p;

    let mut body: Vec<String> = Vec::new();
    let base_d = effective_base_diameter(p);
    let mut current_d = base_d;
    let step = 2.0 * p.espessura_camada;
    let d_sign = if p.is_internal() { -1.0 } else { 1.0 };
    for layer_num in 0..p.num_camadas.max(0) {
        body.extend(build_layer(p, layer_num, current_d));
        current_d += d_sign * step;
    }
    let mut out = build_header(p, timestamp);
    out.extend(body);
    out.extend(build_footer(p));
    Ok(out)
}

/// `_build_spiral_multiperfil_layer`: uma camada com todos os perfis e seus
/// subpasses. Não usa `layer_separator`: o `M01` segue a regra própria do
/// multiperfil (só em "Passes separados" sem ponto a ponto, entre perfis ou
/// entre camadas).
fn build_layer(p: &Params, layer_num: i64, current_d: f64) -> Vec<String> {
    let afastamento = p.afastamento_tocha;
    let z_safe_offset = p.afastamento_seguranca;
    let base_d = effective_base_diameter(p);
    let radial_delta = (current_d - base_d).abs() / 2.0;
    let z_layer = radial_delta + afastamento;
    let z_seguranca = z_layer + z_safe_offset;
    let is_internal = p.is_internal();
    let rotation_sign = p.rotation_sign();
    let taxa_de_deposicao = apply_smart_layers(p, layer_num, p.taxa());
    let pitch = py_max(py_or(p.multiperfil_pitch_mm, 0.0), 1e-6);
    let bounds = rotary_arc_bounds(p);
    // Fechamento uniforme: o comprimento vale de borda a borda do material, e
    // as voltas viram inteiras no curso do centro. Sem anéis aqui: um por
    // subpasse empilharia material nas pontas. Desligado, é o cálculo do Python.
    let plano = p.fechamento_uniforme.then(|| {
        let largura = overlap_metrics(p, layer_num, Some(current_d), None)
            .bead
            .effective_width_mm;
        uniform_closing(bounds.x_start, bounds.x_end, pitch, largura)
    });
    let (x_arc_start_calc, x_arc_end_calc, pitch, total_rotacoes) = match &plano {
        Some(c) => (c.x_start, c.x_end, c.passo, c.voltas),
        None => (
            bounds.x_start,
            bounds.x_end,
            pitch,
            (bounds.x_end - bounds.x_start).abs() / pitch,
        ),
    };
    let circunferencia = PI * py_max(current_d, 1e-6);
    let comprimento_helice_por_volta = (circunferencia.powi(2) + pitch.powi(2)).sqrt();
    let rpm_real = if comprimento_helice_por_volta > 0.0 && taxa_de_deposicao > 0.0 {
        taxa_de_deposicao / comprimento_helice_por_volta
    } else {
        0.0
    };
    // O Python também calcula `feed_linear`, mas nunca o imprime.
    let feed_angular = rpm_real * 360.0;
    let fechar = p.spiral_close_ends;
    let strategy = p.multiperfil_strategy;
    let point_by_point = p.multiperfil_point_by_point;
    let angles = start_angles(p);
    let n_profiles = angles.len();
    let (layer_offsets, layer_width, layer_pass_count) =
        pass_offsets(p, layer_num, Some(current_d));
    let end_chamfer_retreat = layer_end_chamfer(p, layer_num);
    let mut x_piece_min = py_min(x_arc_start_calc, x_arc_end_calc) + end_chamfer_retreat;
    let mut x_piece_max = py_max(x_arc_start_calc, x_arc_end_calc) - end_chamfer_retreat;
    if x_piece_min > x_piece_max {
        let x_mid = (x_arc_start_calc + x_arc_end_calc) / 2.0;
        x_piece_min = x_mid;
        x_piece_max = x_mid;
    }
    let rotary_radius = py_max(current_d, 1e-6) / 2.0;

    let mut layer = vec![format!("(--- CAMADA {} / MULTIPERFIL ---)", layer_num + 1)];
    if let Some(c) = &plano {
        layer.push(fechamento_uniforme_comment(c));
    }
    layer.push(format!("G00 Z{}", f3(z_seguranca)));
    // Começa em 0.0 (um valor, não `None`), exatamente como o Python.
    let mut current_a_position = 0.0;
    for (profile_idx, &start_deg) in angles.iter().enumerate() {
        let profile_idx = profile_idx + 1;
        layer.push(format!(
            "(PERFIL {profile_idx}/{n_profiles} - ANGULO INICIAL {} deg)",
            f3(start_deg)
        ));
        layer.push(format!(
            "(FORMACAO DO PERFIL: {} / {layer_pass_count} PASSADAS / {} mm)",
            profile_shape_str(p.multiperfil_profile_shape),
            f3(layer_width)
        ));
        if end_chamfer_retreat > 0.0 {
            layer.push(format!(
                "(CHANFRO NAS PONTAS: {} mm por lado nesta camada)",
                f3(end_chamfer_retreat)
            ));
        }
        for (pass_idx, &x_offset) in layer_offsets.iter().enumerate() {
            let pass_idx = pass_idx + 1;
            let start_a = align_rotary_target(
                rotation_sign * start_deg,
                Some(current_a_position),
                rotation_sign,
            );
            let weld_end_a = start_a + (total_rotacoes * 360.0 * rotation_sign);
            let (x_start, mut start_a, x_end, mut weld_end_a) = offset_helix_segment_within_bounds(
                x_arc_start_calc,
                start_a,
                x_arc_end_calc,
                weld_end_a,
                rotary_radius,
                x_offset,
                x_piece_min,
                x_piece_max,
            );
            let aligned_start_a =
                align_rotary_target(start_a, Some(current_a_position), rotation_sign);
            if (aligned_start_a - start_a).abs() > 1e-9 {
                let a_shift = aligned_start_a - start_a;
                start_a = aligned_start_a;
                weld_end_a += a_shift;
            }
            let mut final_a_position = weld_end_a;
            layer.push(format!(
                "(  SUBPASSE {pass_idx}/{layer_pass_count} - OFFSET X {} mm )",
                f3s(x_offset)
            ));
            layer.push(format!("G00 X{} A{}", f3(x_start), f3(start_a)));
            layer.push(format!("G00 Z{}", f3(z_layer)));
            if layer_num == 0 && profile_idx == 1 && pass_idx == 1 {
                layer.push("M0 (Confirme e pressione Start)".to_string());
            } else if point_by_point {
                layer.push(
                    "M0 (Ponto a ponto: posicionado no proximo cordao. Pressione Start)"
                        .to_string(),
                );
            }
            append_torch_enable(&mut layer, p);
            // (O Python monta tail_start/tail_end aqui e logo os sobrescreve;
            // só as atribuições efetivas são reproduzidas abaixo.)
            if fechar {
                weld_end_a += rotation_sign * 360.0;
                layer.push(format!(
                    "G01 F{} A{}",
                    f1(feed_angular),
                    f3(start_a + (rotation_sign * 360.0))
                ));
            }
            layer.push(format!(
                "G01 F{} X{} A{}",
                f1(feed_angular),
                f3(x_end),
                f3(weld_end_a)
            ));
            let volta_inicial = if fechar { rotation_sign * 360.0 } else { 0.0 };
            let mut tail_start = Pos::xa(x_start, start_a + volta_inicial);
            let mut tail_end = Pos::xa(x_end, weld_end_a);
            if fechar {
                final_a_position = weld_end_a + (rotation_sign * 360.0);
                layer.push(format!(
                    "G01 F{} A{}",
                    f1(feed_angular),
                    f3(final_a_position)
                ));
                tail_start = Pos::xa(x_end, weld_end_a);
                tail_end = Pos::xa(x_end, final_a_position);
            } else if plano.is_some() {
                // Apagamento só em A: o cordão não passa da borda em X.
                tail_start = Pos::xa(x_end, weld_end_a - rotation_sign);
            }
            append_torch_disable(
                &mut layer,
                p,
                Some(&tail_start),
                Some(&tail_end),
                Some(feed_angular),
                Some(taxa_de_deposicao),
                Some(rotary_radius),
            );
            layer.push(format!("G00 Z{}", f3(z_seguranca)));
            if is_internal {
                if let Some(x_exit) = bounds.x_exit {
                    layer.push(format!("G00 X{}", f3(x_exit)));
                }
            }
            current_a_position = final_a_position;
        }
        if !point_by_point
            && strategy == MultiperfilStrategy::PassesSeparados
            && (profile_idx < n_profiles || layer_num < p.num_camadas - 1)
        {
            layer.push("M01".to_string());
        }
    }
    layer
}
