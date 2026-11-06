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
use crate::deposit::{solve_feed_factors, PathStep};
use crate::overlap::uniform_closing;
use crate::params::Params;

/// Comprimento máximo de um trecho do cálculo da compensação (mm).
const COMP_STEP_MM: f64 = 2.0;
/// Maior trecho do cálculo da compensação (graus).
const COMP_MAX_STEP_DEG: f64 = 5.0;
/// Voltas de hélice simuladas: o platô do meio não muda com o comprimento.
const COMP_MAX_TURNS: f64 = 12.0;
/// Menor trecho do programa com rotação própria (graus).
const COMP_BIN_DEG: f64 = 15.0;

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

/// Ângulo de partida da camada, para defasar os passes entre camadas. Numa
/// hélice, girar a partida em `θ` desloca a trama de passes em X por `passo ×
/// θ/360` sem mudar o passo nem o X do programa: com `N` camadas, partir em
/// `360° × k/N` leva os cumes de cada camada para os vales das outras, e o
/// relevo entre passes não soma. Os anéis das bordas seguem fechando tudo.
/// Zero sem a opção ou com uma só camada.
fn partida_defasada(p: &Params, camada: i64, rotation_sign: f64) -> f64 {
    let n = p.num_camadas;
    if !p.defasar_camadas || n < 2 {
        return 0.0;
    }
    rotation_sign * 360.0 * (camada % n) as f64 / n as f64
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
    let a_ini = partida_defasada(p, layer_num, rotation_sign);
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
    if p.defasar_camadas && p.num_camadas >= 2 {
        seg.push(format!(
            "(DEFASAGEM DE PASSES: camada {}/{}, partida em A{})",
            layer_num + 1,
            p.num_camadas,
            f3(a_ini)
        ));
    }
    seg.push(format!("G00 Z{}", f3(z_seguranca)));
    // Sem o anel a hélice só toca a borda em um ponto da volta: o fechamento
    // uniforme sempre fecha as duas pontas.
    let fechar = p.spiral_close_ends || p.fechamento_uniforme;
    let raio = py_max(current_d, 1e-6) / 2.0;
    let anel = if fechar { 360.0 } else { 0.0 };
    let perfil = p.compensar_rotacao.then(|| {
        RotationProfile::new(
            x_arc_start_calc,
            x_arc_end_calc,
            total_rotacoes,
            anel,
            raio,
            metrics.bead.effective_width_mm / 2.0,
        )
    });
    if let Some(perfil) = &perfil {
        seg.insert(
            seg.len() - 1,
            format!(
                "(COMPENSACAO DE ROTACAO: ate {:.2} vezes nas pontas)",
                perfil.max_factor()
            ),
        );
    }
    let a_partida = if a_ini == 0.0 {
        "0".to_string()
    } else {
        f3(a_ini)
    };
    seg.push(format!("G00 X{} A{a_partida}", f3(x_arc_start_calc)));
    seg.push(format!("G00 Z{}", f3(z_layer)));
    if layer_num == 0 {
        seg.push("M0 (Confirme e pressione Start)".to_string());
    }
    append_torch_enable(&mut seg, p);

    let volta_fechamento = if fechar { rotation_sign * 360.0 } else { 0.0 };
    if fechar && perfil.is_none() {
        let volta_a_ini = a_ini + rotation_sign * 360.0;
        seg.push(format!("G01 F{} A{}", f1(feed_angular), f3(volta_a_ini)));
    }
    let end_x = x_arc_end_calc;
    let end_a = a_ini + angulo_total_a + volta_fechamento;
    if perfil.is_none() {
        seg.push(format!(
            "G01 F{} X{} A{}",
            f1(feed_angular),
            f3(end_x),
            f3(end_a)
        ));
    }
    let mut tail_start = Pos::xa(x_arc_start_calc, a_ini + volta_fechamento);
    let mut tail_end = Pos::xa(end_x, end_a);
    let mut fator_fim = 1.0;
    if let Some(perfil) = &perfil {
        // Mesmas posições, com a rotação de cada trecho. O apagamento roda na
        // rotação do fim, então é ela que diz onde o M102 precisa sair.
        fator_fim = perfil.at(perfil.total_deg - 1e-6);
        let helice_fim = anel + total_rotacoes * 360.0;
        let arco_final = if !fechar {
            0.0
        } else if p.fechamento_uniforme {
            let apagamento = extinguish_arc_deg(p, taxa_de_deposicao * fator_fim, raio);
            py_max(360.0 - apagamento, 0.0)
        } else {
            360.0
        };
        let (inicio, fim) = emit_compensated(
            &mut seg,
            perfil,
            helice_fim + arco_final,
            (a_ini, rotation_sign),
            feed_angular,
            |phi| {
                let t = (phi - anel) / (helice_fim - anel).max(1e-9);
                x_arc_start_calc + (end_x - x_arc_start_calc) * t.clamp(0.0, 1.0)
            },
        );
        tail_start = inicio;
        tail_end = fim;
    } else if fechar {
        // No fechamento uniforme o material ainda sai durante o apagamento:
        // o M102 vem antes, para o cordão terminar quando o anel fecha 360°.
        let arco_final = if p.fechamento_uniforme {
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
        Some(feed_angular * fator_fim),
        Some(taxa_de_deposicao * fator_fim),
        Some(raio),
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

/// Fator de rotação ao longo da espiral, por ângulo girado desde o início.
struct RotationProfile {
    step_deg: f64,
    factors: Vec<f64>,
    /// Graus simulados: em peças longas, só as duas pontas e um platô.
    sim_deg: f64,
    /// Graus de todo o caminho, com os anéis.
    total_deg: f64,
    /// Fim do anel inicial e fim da hélice: pontos em que X muda de regra.
    knots: [f64; 2],
}

impl RotationProfile {
    fn new(x0: f64, x1: f64, voltas: f64, anel: f64, raio: f64, bead_radius: f64) -> Self {
        let voltas_sim = voltas.min(COMP_MAX_TURNS);
        let helice_sim = voltas_sim * 360.0;
        let sim_deg = 2.0 * anel + helice_sim;
        let step_deg = (COMP_STEP_MM / py_max(raio, 1e-6))
            .to_degrees()
            .min(COMP_MAX_STEP_DEG);
        let n = (sim_deg / step_deg).ceil() as usize;
        let passo = if voltas > 0.0 {
            (x1 - x0) / voltas
        } else {
            0.0
        };
        let steps: Vec<PathStep> = (0..n)
            .map(|i| {
                let phi = (i as f64 + 0.5) * step_deg;
                let helice = (phi - anel).clamp(0.0, helice_sim);
                PathStep {
                    x: x0 + passo * helice / 360.0,
                    a_deg: phi,
                }
            })
            .collect();
        let helice = voltas * 360.0;
        RotationProfile {
            step_deg,
            factors: solve_feed_factors(&steps, raio, bead_radius, passo.abs()),
            sim_deg,
            total_deg: 2.0 * anel + helice,
            knots: [anel, anel + helice],
        }
    }

    /// Fator a `phi` graus do início; no meio de peças longas, a rotação nominal.
    fn at(&self, phi: f64) -> f64 {
        let meio = self.sim_deg / 2.0;
        let sim = if phi <= meio {
            phi
        } else if self.total_deg - phi <= meio {
            self.sim_deg - (self.total_deg - phi)
        } else {
            return 1.0;
        };
        let i = ((sim / self.step_deg) as usize).min(self.factors.len().saturating_sub(1));
        self.factors.get(i).copied().unwrap_or(1.0)
    }

    fn max_factor(&self) -> f64 {
        self.factors.iter().copied().fold(1.0, f64::max)
    }

    /// Média entre `from` e `to`, em passos de 2% para não gerar uma linha por
    /// diferença invisível na máquina.
    fn quantized(&self, from: f64, to: f64) -> f64 {
        let media = (self.at(from) + self.at((from + to) / 2.0) + self.at(to - 1e-6)) / 3.0;
        (media * 50.0).round() / 50.0
    }
}

/// Grava a passada até `parada` graus, com uma linha `G01` por trecho de mesma
/// rotação. Devolve início e fim do último trecho, de onde sai o apagamento.
fn emit_compensated(
    seg: &mut Vec<String>,
    perfil: &RotationProfile,
    parada: f64,
    (a_ini, rotation_sign): (f64, f64),
    feed: f64,
    x_at: impl Fn(f64) -> f64,
) -> (Pos, Pos) {
    let mut cortes: Vec<f64> = (1..)
        .map(|i| i as f64 * COMP_BIN_DEG)
        .take_while(|c| *c < parada)
        .collect();
    cortes.extend(perfil.knots.iter().copied().filter(|k| *k < parada));
    cortes.push(parada);
    cortes.sort_by(f64::total_cmp);
    cortes.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    cortes.retain(|c| *c > 1e-6);

    let e_ponto_de_regra = |c: f64| perfil.knots.iter().any(|k| (c - k).abs() < 1e-6);
    let (mut de, mut trecho_de) = (0.0, 0.0);
    let mut fator = perfil.quantized(0.0, cortes.first().copied().unwrap_or(parada));
    let mut trecho = (Pos::xa(x_at(0.0), a_ini), Pos::xa(x_at(0.0), a_ini));
    let mut grava = |seg: &mut Vec<String>, ate: f64, fator: f64, trecho_de: f64| {
        let (x0, x1) = (x_at(trecho_de), x_at(ate));
        let (a0, a1) = (
            a_ini + rotation_sign * trecho_de,
            a_ini + rotation_sign * ate,
        );
        if (x1 - x0).abs() > 1e-9 {
            seg.push(format!("G01 F{} X{} A{}", f1(feed * fator), f3(x1), f3(a1)));
        } else {
            seg.push(format!("G01 F{} A{}", f1(feed * fator), f3(a1)));
        }
        trecho = (Pos::xa(x0, a0), Pos::xa(x1, a1));
    };
    for (i, &ate) in cortes.iter().enumerate() {
        let f = perfil.quantized(de, ate);
        if (f - fator).abs() > 1e-9 && de > trecho_de {
            grava(seg, de, fator, trecho_de);
            trecho_de = de;
        }
        fator = f;
        if i + 1 == cortes.len() || e_ponto_de_regra(ate) {
            grava(seg, ate, fator, trecho_de);
            trecho_de = ate;
        }
        de = ate;
    }
    trecho
}
