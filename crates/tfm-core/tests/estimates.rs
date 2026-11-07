//! Testes de integração de `tfm_core::estimates`.
//!
//! Porta de `executar_calculos_e_desenho`, `_update_overlap_results`,
//! `_get_multiperfil_profile_pass_plan` e `_update_quality_metrics`
//! (TFM_GCODE.py). Os valores esperados foram calculados à mão a partir do
//! Python; os que dependem do modelo de sobreposição (`asin`) são derivados de
//! `gcode::common::overlap_metrics` do próprio crate, com a mesma ordem de
//! operações do Python. A fixture `tests/fixtures/config_exemplo.json` (cópia de um `config.json` real) é apenas lida.

use std::f64::consts::PI;
use std::path::Path;

use tfm_core::config::AppConfig;
use tfm_core::estimates::{compute_with_program, multiperfil_pass_plan, Estimates, PassPlanItem};

/// A conta de cada operação herdada do Python, sem o programa gerado: é o que
/// estes testes conferem. Com programa, o tempo é o de máquina do G-code
/// (`tests/tempo_programa.rs`).
fn compute(p: &Params, cfg: &AppConfig) -> Estimates {
    compute_with_program(p, cfg, None)
}
use tfm_core::formula::{evaluate_block, formula_locals};
use tfm_core::gcode::common::overlap_metrics;
use tfm_core::overlap::Uniformity;
use tfm_core::params::{BolachaTipo, Operation, Params, ProfileShape};
use tfm_core::quality::QualityManager;

const FIXTURE_CONFIG: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/config_exemplo.json"
);

fn fixture_config() -> AppConfig {
    assert!(
        Path::new(FIXTURE_CONFIG).exists(),
        "{FIXTURE_CONFIG} deve existir"
    );
    AppConfig::read(Path::new(FIXTURE_CONFIG))
}

/// Config padrão sem fórmulas: isola a aritmética própria de cada modo
/// (sem override de pó/tempo/custos).
fn bare_config() -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.formulas.expressions.clear();
    cfg
}

fn config_with(exprs: &[(&str, &str)]) -> AppConfig {
    let mut cfg = bare_config();
    for (k, v) in exprs {
        cfg.formulas
            .expressions
            .insert((*k).to_string(), (*v).to_string());
    }
    cfg
}

/// Defaults da UI com a operação aplicada (`taxa_de_deposicao` ausente, como
/// nos presets; `Params::taxa()` cai em `velocidade_de_deposicao`).
fn params(op: Operation) -> Params {
    let mut p = Params::default();
    p.apply_operation(op);
    p
}

fn is_hhmm(s: &str) -> bool {
    s.len() == 5
        && s.as_bytes()[2] == b':'
        && s.chars()
            .enumerate()
            .all(|(i, c)| i == 2 || c.is_ascii_digit())
}

/// `int(math.ceil(min))` -> `f"{hh:02d}:{mm:02d}"`.
fn hhmm(minutes: f64) -> String {
    let m = minutes.ceil() as i64;
    format!("{:02}:{:02}", m / 60, m % 60)
}

fn assert_close(actual: f64, expected: f64, tol: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tol,
        "{what}: esperado {expected:?}, obtido {actual:?} (tol {tol:e})"
    );
}

fn assert_finite_text(what: &str, text: &str) {
    assert!(
        !text.contains("NaN") && !text.contains("inf"),
        "{what} deve ser finito, obtido {text:?}"
    );
}

/// Custos sem override de fórmula, a partir do pó (kg) e do tempo (h), com os
/// custos padrão (250/50/40/30) e `powder_cost_brl_kg` dos params (450).
fn assert_default_costs(e: &Estimates, kg: f64, hours: f64) {
    let c = &e.costs;
    let po = kg * 450.0;
    let m3_hora = 15.0 * 60.0 / 1000.0;
    let gas = m3_hora * hours * 50.0;
    let labor = hours * 40.0;
    let machine = hours * 30.0;
    assert_close(c.powder_kg, kg, 1e-9, "powder_kg");
    assert_close(c.powder_cost, po, 1e-9, "powder_cost");
    assert_close(c.gas_cost, gas, 1e-9, "gas_cost");
    assert_close(c.labor_cost, labor, 1e-9, "labor_cost");
    assert_close(c.machine_cost, machine, 1e-9, "machine_cost");
    assert_close(c.consumiveis, po + gas, 1e-9, "consumiveis");
    assert_close(c.operacional, labor + machine, 1e-9, "operacional");
    assert_close(c.total, (po + gas) + (labor + machine), 1e-9, "total");
    assert_close(c.gas_m3, m3_hora * hours, 1e-9, "gas_m3");
    assert_eq!(c.currency, "R$");
}

// ---------------------------------------------------------------------------
// Testes do brief
// ---------------------------------------------------------------------------

#[test]
fn espiral_default_time_and_costs() {
    let e = compute(&params(Operation::Espiral), &fixture_config());
    assert!(is_hhmm(&e.time_hhmm), "time_hhmm = {:?}", e.time_hhmm);
    assert!(e.costs.total > 0.0, "total = {}", e.costs.total);
    assert!(e.time_hours > 0.0);
    assert!(e.pitch.text.ends_with(" (axial)"), "{:?}", e.pitch.text);
    assert!(!e.rotation.over_limit);
    assert!(!e.pitch.over_limit);
}

#[test]
fn chapa_zero_rotation() {
    let e = compute(&params(Operation::ChapaPlana), &fixture_config());
    assert_eq!(e.rotation.text, "0.000");
    assert_eq!(e.rotations, "0");
    assert_eq!(e.total_angle, "0");
    assert!(e.pitch.text.ends_with(" (lat)"), "{:?}", e.pitch.text);
}

#[test]
fn bolacha_rotation_range_text() {
    let mut p = params(Operation::Bolacha);
    p.diametro = 100.0;
    p.diametro_interno = 20.0;
    let e = compute(&p, &fixture_config());
    assert!(e.rotation.text.contains(" - "), "{:?}", e.rotation.text);
    // r_ext = 50, r_int = max(10, 4) = 10: 2*pi*50/120 e 2*pi*10/120
    assert_eq!(e.rotation.text, "2.618 - 0.524");
    assert!(e.pitch.text.ends_with(" (rad)"), "{:?}", e.pitch.text);
}

#[test]
fn rpm_over_limit_flag() {
    let mut p = params(Operation::Espiral);
    p.velocidade_de_deposicao = 50000.0;
    let e = compute(&p, &fixture_config());
    assert!(e.rotation.over_limit, "rpm 50000/helice >> 100");
    assert!(!e.pitch.over_limit);
}

#[test]
fn formula_override_powder_cost() {
    let cfg = config_with(&[("powder_cost_brl", "123.0")]);
    let e = compute(&params(Operation::Espiral), &cfg);
    assert_eq!(e.costs.powder_cost, 123.0);
    assert_close(
        e.costs.consumiveis,
        123.0 + e.costs.gas_cost,
        1e-12,
        "consumiveis",
    );
    assert_eq!(e.formulas.get("powder_cost_brl"), Some(&Some(123.0)));
}

#[test]
fn multiperfil_pass_plan_len_equals_layers() {
    let mut p = params(Operation::EspiralMultiperfil);
    p.multiperfil_profile_height_mm = 4.5;
    p.espessura_camada = 1.5;
    let plan = multiperfil_pass_plan(&p);
    assert_eq!(plan.len(), 3);
    for (idx, item) in plan.iter().enumerate() {
        // Retangular: largura alvo constante = base (8 mm)
        assert_eq!(item.width, 8.0, "camada {idx}");
        assert!(item.pass_count >= 2, "force_min_two_passes na camada {idx}");
        assert_eq!(item.offsets.len(), item.pass_count);
        let m = overlap_metrics(&p, idx as i64, None, Some(8.0));
        assert_eq!(item.step_over_mm, m.step_over_mm);
        assert_eq!(item.bead_effective_width, m.bead.effective_width_mm);
        assert_eq!(item.surface_width, m.bead.surface_width_mm);
        assert_eq!(item.effective_width, m.effective_cover_width_mm);
        assert_eq!(item.coverage_ratio, m.coverage_ratio);
        assert_eq!(item.fill_error_mm, m.fill_error_mm);
        assert_eq!(item.achieved_overlap_pct, m.achieved_overlap_pct);
        assert_eq!(item.uniformity, m.uniformity);
    }
}

// ---------------------------------------------------------------------------
// Aritmética de cada modo (ramos de `executar_calculos_e_desenho`)
// ---------------------------------------------------------------------------

#[test]
fn espiral_arithmetic_matches_python() {
    let mut p = params(Operation::Espiral);
    p.lead_extras_enabled = true;
    p.lead_in = 5.0;
    p.lead_out = 3.0;
    p.num_camadas = 2;
    let e = compute(&p, &bare_config());

    // passo = média do step_over das camadas 0..num_camadas
    let m0 = overlap_metrics(&p, 0, None, None);
    let m1 = overlap_metrics(&p, 1, None, None);
    let passo = (m0.step_over_mm + m1.step_over_mm) / 2.0;
    let circ = PI * 50.0;
    let h = 50.0;
    let total_rot = h / passo;
    let angulo = total_rot * 360.0 + 720.0;
    let helice_volta = (circ.powi(2) + passo.powi(2)).sqrt();
    let mov = total_rot * helice_volta + (5.0 + 3.0) + (2.0 * circ);
    let minutos = (mov / 120.0) * 2.0;

    assert_eq!(e.rotation.text, format!("{:.3}", helice_volta / 120.0));
    assert_eq!(e.pitch.text, format!("{passo:.3} (axial)"));
    assert_eq!(e.rotations, format!("{total_rot:.2}"));
    assert_eq!(e.total_angle, format!("{angulo:.2}"));
    assert_eq!(e.time_hhmm, hhmm(minutos));
    assert_close(e.time_hours, minutos / 60.0, 1e-9, "time_hours");
    assert!(!e.rotation.over_limit && !e.pitch.over_limit);

    // pó: (área das camadas + área dos leads) * fator / 1000
    let area_total = circ * h * 2.0;
    let area_leads = circ * (5.0 + 3.0) * 2.0;
    let kg = (area_total + area_leads) * 0.16 / 1000.0;
    assert_default_costs(&e, kg, minutos / 60.0);
}

#[test]
fn espiral_x_travel_over_limit_flag() {
    let mut p = params(Operation::Espiral);
    p.comprimento_revestir = 1400.0;
    p.lead_extras_enabled = true;
    p.lead_in = 60.0;
    p.lead_out = 60.0;
    let e = compute(&p, &bare_config());
    assert!(e.pitch.over_limit, "1400 + 60 + 60 > max_length_x 1500");
    assert!(!e.rotation.over_limit);

    let mut cfg = bare_config();
    if let Some(profile) = cfg.machine_profiles.get_mut("default") {
        profile.limits.max_length_x = 2000.0;
    }
    assert!(
        !compute(&p, &cfg).pitch.over_limit,
        "limite do perfil ativo manda"
    );
}

#[test]
fn chapa_arithmetic_matches_python() {
    let p = params(Operation::ChapaPlana);
    let e = compute(&p, &bare_config());
    let passo = overlap_metrics(&p, 0, None, None).step_over_mm.max(1e-6);
    let num_passes_y = (100.0_f64 / passo).ceil();
    let total_len = num_passes_y * 50.0 + (num_passes_y - 1.0) * passo;
    let minutos = total_len / 120.0;
    assert_eq!(e.pitch.text, format!("{passo:.3} (lat)"));
    assert_eq!(e.time_hhmm, hhmm(minutos));
    assert_close(e.time_hours, minutos / 60.0, 1e-9, "time_hours");
    assert_default_costs(&e, 100.0 * 50.0 * 0.16 / 1000.0, minutos / 60.0);
    // A chapa não valida limites de máquina (o Python não toca as cores)
    let mut longa = p.clone();
    longa.comprimento_revestir = 5000.0;
    longa.velocidade_de_deposicao = 50000.0;
    let e2 = compute(&longa, &bare_config());
    assert!(!e2.rotation.over_limit && !e2.pitch.over_limit);
}

#[test]
fn bolacha_aneis_arithmetic_matches_python() {
    let mut p = params(Operation::Bolacha);
    p.diametro = 100.0;
    p.diametro_interno = 20.0;
    p.bolacha_tipo = BolachaTipo::Aneis;
    p.overlap_angular_deg = 10.0;
    let e = compute(&p, &bare_config());
    let passo = overlap_metrics(&p, 0, None, None).step_over_mm.max(1e-6);
    let (r_ext, r_int) = (50.0_f64, 10.0_f64);
    let num_voltas = ((r_ext - r_int) / passo).ceil();
    let n_total = num_voltas + 1.0;
    let fator = 370.0 / 360.0;
    let soma_raios = (n_total * r_int) + (passo * (n_total - 1.0) * n_total / 2.0);
    let mov = 2.0 * PI * soma_raios * fator;
    let minutos = mov / 120.0;
    assert_eq!(e.rotations, format!("{:.2}", n_total * fator));
    assert_eq!(e.total_angle, format!("{:.0}", n_total * 370.0));
    assert_eq!(e.pitch.text, format!("{passo:.3} (rad)"));
    assert_eq!(e.time_hhmm, hhmm(minutos));
    assert_close(e.time_hours, minutos / 60.0, 1e-9, "time_hours");
    let kg = PI * (r_ext.powi(2) - r_int.powi(2)) * 0.16 / 1000.0;
    assert_default_costs(&e, kg, minutos / 60.0);
    assert!(!e.rotation.over_limit && !e.pitch.over_limit);
}

#[test]
fn bolacha_espiral_continua_and_limits() {
    let mut p = params(Operation::Bolacha);
    p.diametro = 100.0;
    p.diametro_interno = 20.0;
    p.overlap_angular_deg = 10.0;
    let e = compute(&p, &bare_config());
    let passo = overlap_metrics(&p, 0, None, None).step_over_mm.max(1e-6);
    let num_voltas = (40.0 / passo).ceil();
    let total_rot = num_voltas + (10.0 / 360.0);
    let mov = 2.0 * PI * ((10.0 + 50.0) / 2.0) * total_rot;
    assert_eq!(e.rotations, format!("{total_rot:.2}"));
    assert_eq!(e.total_angle, format!("{:.0}", num_voltas * 360.0 + 10.0));
    assert_eq!(e.time_hhmm, hhmm(mov / 120.0));

    // rpm_max = taxa / (2*pi*r_int) > 100 pinta a rotação; r_ext > max_x pinta o passo
    let mut rapida = p.clone();
    rapida.velocidade_de_deposicao = 10000.0;
    assert!(compute(&rapida, &bare_config()).rotation.over_limit);
    let mut enorme = p.clone();
    enorme.diametro = 4000.0;
    assert!(compute(&enorme, &bare_config()).pitch.over_limit);

    // raios iguais (|ext - int| < 0.001): texto único
    let mut iguais = params(Operation::Bolacha);
    iguais.diametro = 8.0;
    iguais.diametro_interno = 8.0;
    let e3 = compute(&iguais, &bare_config());
    assert_eq!(e3.rotation.text, "0.209");
    assert_eq!(e3.rotations, format!("{:.2}", 1.0 + 10.0 / 360.0));
}

#[test]
fn oscillation_linear_exact_values() {
    let e = compute(&params(Operation::OscilacaoLinear), &bare_config());
    // circ = 50*pi; desloc = 0.2*8 = 1.6; passos/volta = ceil(98.17) = 99
    // Linear: uma volta inteira em A por passo axial, passo do cordão
    // 8 × (1 − 40%) = 4,8 mm e ceil(50 ÷ 4,8) = 11 passos. O Python usava o
    // modelo da quadrada aqui (passo 2,0 e 03:59), 16 vezes acima do programa.
    assert_eq!(e.rotation.text, "1.309");
    assert_eq!(e.pitch.text, "4.800 (axial)");
    assert_eq!(e.rotations, "11.00 (passos axiais)");
    assert_eq!(e.total_angle, "360.00° (passo)");
    let circ = PI * 50.0;
    let minutos = 11.0 * (circ / 120.0);
    assert_eq!(e.time_hhmm, hhmm(minutos));
    assert_close(e.time_hours, minutos / 60.0, 1e-9, "time_hours");
    assert_default_costs(&e, circ * 50.0 * 0.16 / 1000.0, minutos / 60.0);
    assert!(!e.rotation.over_limit && !e.pitch.over_limit);

    // Quadrada: modelo das pernas em X e passos angulares; contínua zera o giro.
    let tempo_osc = 10.0 / 120.0;
    let tempo_rot = (360.0 / 99.0) / ((120.0 / circ) * 360.0);
    let q = compute(&params(Operation::OscilacaoQuadrada), &bare_config());
    assert_eq!(q.pitch.text, "2.000 (axial)");
    assert_eq!(q.time_hhmm, "03:59");
    assert_close(
        q.time_hours,
        25.0 * ((tempo_osc + tempo_rot) * 99.0) / 60.0,
        1e-9,
        "quadrada",
    );
    let qc = compute(
        &params(Operation::OscilacaoQuadradaContinua),
        &bare_config(),
    );
    assert_eq!(qc.time_hhmm, "03:27");
    assert_close(
        qc.time_hours,
        25.0 * (tempo_osc * 99.0) / 60.0,
        1e-9,
        "continua",
    );
}

#[test]
fn multiperfil_texts_and_time() {
    let p = params(Operation::EspiralMultiperfil);
    let e = compute(&p, &bare_config());
    let plan = multiperfil_pass_plan(&p);
    assert_eq!(plan.len(), 1);
    let subpasses: usize = plan.iter().map(|i| i.pass_count).sum();
    assert_eq!(
        subpasses, 2,
        "largura 8 em cordao de ~8 mm: force_min_two_passes"
    );
    let circ = PI * 50.0;
    let helice_volta = (circ.powi(2) + 12.0_f64.powi(2)).sqrt();
    let rot_perfil = 50.0 / 12.0;
    assert_eq!(e.rotation.text, format!("{:.3}", helice_volta / 120.0));
    assert_eq!(e.rotation.text, "1.313");
    assert_eq!(e.pitch.text, "12.000 (fixo)");
    assert_eq!(e.rotations, format!("4 x {subpasses} x {rot_perfil:.2}"));
    assert_eq!(e.total_angle, "360.0° / 90.0° (Retangular)");
    let helice_perfil = rot_perfil * helice_volta;
    let mov = (4 * subpasses) as f64 * helice_perfil;
    let minutos = mov / 120.0; // sem multiplicar por num_camadas
    assert_eq!(e.time_hhmm, hhmm(minutos));
    assert_close(e.time_hours, minutos / 60.0, 1e-9, "time_hours");
    let kg = helice_perfil * 8.0 * subpasses as f64 * 4.0 * 0.16 / 1000.0;
    assert_default_costs(&e, kg, minutos / 60.0);

    // Fechamento das pontas soma 2 voltas por subpasse
    let mut fechado = p.clone();
    fechado.spiral_close_ends = true;
    let ef = compute(&fechado, &bare_config());
    let mov_f = (4 * subpasses) as f64 * (helice_perfil + 2.0 * circ);
    assert_close(ef.time_hours, mov_f / 120.0 / 60.0, 1e-9, "fechamento");

    // Setor parcial divide por (count - 1); a forma entra no texto
    let mut trap = p.clone();
    trap.multiperfil_count = 3;
    trap.multiperfil_total_angle_deg = 180.0;
    trap.multiperfil_profile_shape = ProfileShape::Trapezoidal;
    let et = compute(&trap, &bare_config());
    assert_eq!(et.total_angle, "180.0° / 90.0° (Trapezoidal)");
    let mut unico = p.clone();
    unico.multiperfil_count = 1;
    assert_eq!(
        compute(&unico, &bare_config()).total_angle,
        "360.0° / 0.0° (Retangular)"
    );
}

#[test]
fn overlap_results_multiperfil_averages() {
    let mut p = params(Operation::EspiralMultiperfil);
    p.multiperfil_profile_height_mm = 4.5;
    p.multiperfil_profile_shape = ProfileShape::Trapezoidal;
    let e = compute(&p, &bare_config());
    let plan = multiperfil_pass_plan(&p);
    let n = plan.len() as f64;
    let avg = |f: &dyn Fn(&PassPlanItem) -> f64| plan.iter().map(f).sum::<f64>() / n;
    let worst = plan
        .iter()
        .map(|i| i.uniformity)
        .max()
        .unwrap_or(Uniformity::Boa);
    let max_fill = plan
        .iter()
        .map(|i| i.fill_error_mm.abs())
        .fold(f64::MIN, f64::max);
    assert_eq!(
        e.overlap.effective_bead_width,
        format!("{:.2}", avg(&|i| i.bead_effective_width))
    );
    assert_eq!(
        e.overlap.surface_step,
        format!("{:.2}", avg(&|i| i.step_over_mm))
    );
    assert_eq!(
        e.overlap.coverage,
        format!(
            "{:.2}/{:.2} mm | {:.1}%",
            avg(&|i| i.width),
            avg(&|i| i.effective_width),
            avg(&|i| i.achieved_overlap_pct)
        )
    );
    assert_eq!(
        e.overlap.uniformity,
        format!("{} | erro max {max_fill:.2} mm", worst.label())
    );
    assert_eq!(e.overlap.uniformity_level, worst);
}

#[test]
fn overlap_results_single_layer_strings() {
    let p = params(Operation::Espiral);
    let e = compute(&p, &bare_config());
    let m = overlap_metrics(&p, 0, None, None);
    assert_eq!(
        e.overlap.effective_bead_width,
        format!("{:.2}", m.bead.effective_width_mm)
    );
    assert_eq!(e.overlap.surface_step, format!("{:.2}", m.step_over_mm));
    assert_eq!(
        e.overlap.coverage,
        format!(
            "{:.1}% | curva {:.2} mm",
            m.achieved_overlap_pct, m.bead.surface_width_mm
        )
    );
    assert_eq!(e.overlap.uniformity, "Boa | var 0.0%");
    assert_eq!(e.overlap.uniformity_level, Uniformity::Boa);
}

#[test]
fn overlap_results_range_and_critica() {
    let mut p = params(Operation::Espiral);
    p.diametro = 10.0;
    p.num_camadas = 10;
    let e = compute(&p, &bare_config());
    let first = overlap_metrics(&p, 0, None, None);
    let last = overlap_metrics(&p, 9, None, None);
    let eff_min = first
        .bead
        .effective_width_mm
        .min(last.bead.effective_width_mm);
    let eff_max = first
        .bead
        .effective_width_mm
        .max(last.bead.effective_width_mm);
    let step_min = first.step_over_mm.min(last.step_over_mm);
    let step_max = first.step_over_mm.max(last.step_over_mm);
    assert!(
        eff_max - eff_min > 0.05,
        "cenario deve variar: {eff_min} .. {eff_max}"
    );
    assert_eq!(
        e.overlap.effective_bead_width,
        format!("{eff_min:.2} - {eff_max:.2}")
    );
    assert_eq!(
        e.overlap.surface_step,
        format!("{step_min:.2} - {step_max:.2}")
    );
    let variation =
        (last.step_over_mm - first.step_over_mm).abs() / first.step_over_mm.max(1e-6) * 100.0;
    assert!(variation > 10.0, "variacao {variation}");
    assert_eq!(
        e.overlap.uniformity,
        format!("Critica | var {variation:.1}%")
    );
    assert_eq!(e.overlap.uniformity_level, Uniformity::Critica);
    assert_eq!(
        e.overlap.coverage,
        format!(
            "{:.1}% | curva {:.2} mm",
            first.achieved_overlap_pct, first.bead.surface_width_mm
        )
    );
}

// ---------------------------------------------------------------------------
// Fórmulas: overrides de pó, tempo (fallback) e custos
// ---------------------------------------------------------------------------

#[test]
fn formula_powder_mass_override_and_cost_chain() {
    let cfg = config_with(&[
        ("powder_mass_kg", "0.5"),
        ("gas_cost_brl", "1.5"),
        ("labor_cost_brl", "2.5"),
        ("machine_cost_brl", "3.5"),
        ("total_cost_brl", "999"),
    ]);
    let e = compute(&params(Operation::Espiral), &cfg);
    assert_eq!(e.costs.powder_kg, 0.5);
    assert_eq!(e.costs.powder_cost, 0.5 * 450.0);
    assert_eq!(e.costs.gas_cost, 1.5);
    assert_eq!(e.costs.consumiveis, 0.5 * 450.0 + 1.5);
    assert_eq!(e.costs.labor_cost, 2.5);
    assert_eq!(e.costs.machine_cost, 3.5);
    assert_eq!(e.costs.operacional, 6.0);
    assert_eq!(e.costs.total, 999.0);
    // gas_m3 nao e afetado pelo override de custo
    assert_close(e.costs.gas_m3, 0.9 * e.time_hours, 1e-9, "gas_m3");

    // powder_mass_g so entra sem powder_mass_kg; negativos sao ignorados
    let g = compute(
        &params(Operation::Espiral),
        &config_with(&[("powder_mass_g", "250")]),
    );
    assert_eq!(g.costs.powder_kg, 0.25);
    let neg = compute(
        &params(Operation::Espiral),
        &config_with(&[("powder_mass_kg", "-1")]),
    );
    let bare = compute(&params(Operation::Espiral), &bare_config());
    assert_eq!(neg.costs.powder_kg, bare.costs.powder_kg);
}

#[test]
fn formula_time_fallback_only_when_mode_time_is_zero() {
    // Chapa sem area nem comprimento: tempo do modo = 0 -> fallback pela massa
    let mut p = params(Operation::ChapaPlana);
    p.comprimento_revestir = 0.0;
    p.largura_revestir = 0.0;
    let e = compute(&p, &config_with(&[("powder_mass_kg", "0.35")]));
    // taxa_deposicao_g_h = 700 -> 0.7 kg/h -> 0.35/0.7*60 = 30 min
    assert_eq!(e.time_hhmm, "00:30");
    assert_close(e.time_hours, 0.5, 1e-12, "time_hours");
    assert_close(e.costs.labor_cost, 0.5 * 40.0, 1e-12, "labor");

    // Sem taxa na UI: time_min_mass, depois time_min
    p.taxa_deposicao_g_h = 0.0;
    let e2 = compute(&p, &config_with(&[("time_min_mass", "90")]));
    assert_eq!(e2.time_hhmm, "01:30");
    assert_close(e2.time_hours, 1.5, 1e-12, "time_min_mass");
    let e3 = compute(&p, &config_with(&[("time_min", "61")]));
    assert_eq!(e3.time_hhmm, "01:01");
    // taxa vinda da formula (kg/h) tem prioridade sobre time_min_mass
    let e4 = compute(
        &p,
        &config_with(&[
            ("powder_mass_kg", "1.0"),
            ("taxa_deposicao_kg_h", "2.0"),
            ("time_min_mass", "90"),
        ]),
    );
    assert_eq!(e4.time_hhmm, "00:30");
    // ... e taxa_deposicao_g_h da formula entra dividida por 1000
    let e6 = compute(
        &p,
        &config_with(&[("powder_mass_kg", "1.0"), ("taxa_deposicao_g_h", "4000")]),
    );
    assert_eq!(e6.time_hhmm, "00:15");

    // Com tempo do modo > 0 a formula de tempo e ignorada
    let e5 = compute(
        &params(Operation::Espiral),
        &config_with(&[("time_min", "1")]),
    );
    let bare = compute(&params(Operation::Espiral), &bare_config());
    assert_eq!(e5.time_hhmm, bare.time_hhmm);
    assert_eq!(e5.time_hours, bare.time_hours);
}

#[test]
fn formulas_are_raw_evaluate_block_output() {
    let cfg = fixture_config();
    let p = params(Operation::Espiral);
    let e = compute(&p, &cfg);
    let expected = evaluate_block(&cfg.formulas.expressions, &formula_locals(&p, &cfg.costs));
    assert_eq!(e.formulas, expected);
    assert_eq!(e.formulas.len(), 20);
    for key in [
        "powder_cost_brl",
        "gas_cost_brl",
        "labor_cost_brl",
        "machine_cost_brl",
        "powder_mass_kg",
        "gas_consumption_l",
        "time_min_mass",
        "time_min",
    ] {
        assert!(
            matches!(e.formulas.get(key), Some(Some(_))),
            "{key} deve avaliar"
        );
    }
    // As fórmulas avaliam, mas as padrão (herdadas do Python, de conta de
    // cilindro) não sobrescrevem o cálculo: só uma expressão alterada manda.
    let kg = e.formulas["powder_mass_kg"].unwrap_or(f64::NAN);
    assert!(kg.is_finite());
    assert_eq!(
        e.costs.powder_cost,
        e.costs.powder_kg * p.powder_cost_brl_kg
    );
    assert_eq!(e.costs.currency, "R$");
}

// ---------------------------------------------------------------------------
// Qualidade, moeda, robustez
// ---------------------------------------------------------------------------

#[test]
fn quality_metrics_follow_update_quality_metrics() {
    let p = params(Operation::Espiral);
    let e = compute(&p, &bare_config());
    // (25 V * 180 A * 60 * 0.8) / 120 mm/min / 1000 = 1.8 kJ/mm
    assert_close(e.heat_input_kj_mm, 1.8, 1e-12, "heat input");
    // po em g/min = taxa_deposicao_g_h / 60
    let dil = QualityManager::estimate_dilution(180.0, 700.0 / 60.0, 120.0);
    assert_eq!(e.dilution_pct, dil);
    assert!(dil > 0.0);
}

#[test]
fn currency_defaults_to_dollar_when_empty() {
    let mut cfg = bare_config();
    cfg.costs.currency_symbol = String::new();
    assert_eq!(
        compute(&params(Operation::Espiral), &cfg).costs.currency,
        "$"
    );
    cfg.costs.currency_symbol = "EUR".to_string();
    assert_eq!(
        compute(&params(Operation::Espiral), &cfg).costs.currency,
        "EUR"
    );
}

#[test]
fn normalized_params_give_identical_results() {
    for op in Operation::ALL {
        let p = params(op);
        let mut n = p.clone();
        n.normalize();
        assert_eq!(
            compute(&p, &fixture_config()),
            compute(&n, &fixture_config()),
            "{op}"
        );
    }
}

#[test]
fn never_panics_on_degenerate_params() {
    let cfg = fixture_config();
    for op in Operation::ALL {
        let mut p = params(op);
        p.diametro = 0.0;
        p.diametro_interno = 0.0;
        p.comprimento_revestir = 0.0;
        p.largura_revestir = 0.0;
        p.largura_cordao = 0.0;
        p.sobreposicao = 0.0;
        p.velocidade_de_deposicao = 0.0;
        p.num_camadas = 0;
        p.multiperfil_profile_height_mm = 0.0;
        p.multiperfil_pitch_mm = 0.0;
        p.multiperfil_count = 0;
        p.oscilacao_comprimento = 0.0;
        p.deslocamento_angular_perc = 0.0;
        p.vazao_gas = 0.0;
        p.taxa_deposicao_g_h = 0.0;
        let e = compute(&p, &cfg);
        assert!(is_hhmm(&e.time_hhmm), "{op}: {:?}", e.time_hhmm);
        for (what, text) in [
            ("rotation", &e.rotation.text),
            ("pitch", &e.pitch.text),
            ("rotations", &e.rotations),
            ("total_angle", &e.total_angle),
            ("effective_bead_width", &e.overlap.effective_bead_width),
            ("surface_step", &e.overlap.surface_step),
            ("coverage", &e.overlap.coverage),
            ("uniformity", &e.overlap.uniformity),
        ] {
            assert_finite_text(&format!("{op}: {what}"), text);
        }
        assert_eq!(e.heat_input_kj_mm, 0.0, "{op}: velocidade 0");
        assert_eq!(e.dilution_pct, 0.0, "{op}: sem po");
    }
}

#[test]
fn non_finite_inputs_become_zero_in_text_fields() {
    for op in [
        Operation::Espiral,
        Operation::OscilacaoLinear,
        Operation::Bolacha,
        Operation::EspiralMultiperfil,
    ] {
        let mut p = params(op);
        // (comprimento infinito dispararia um overflow em overlap.rs via formula_locals; ver relatório)
        p.diametro = f64::INFINITY;
        p.velocidade_de_deposicao = f64::NAN;
        let e = compute(&p, &bare_config());
        assert_eq!(e.time_hhmm, "00:00", "{op}");
        for text in [
            &e.rotation.text,
            &e.pitch.text,
            &e.rotations,
            &e.total_angle,
        ] {
            assert_finite_text(&op.to_string(), text);
        }
    }
}
