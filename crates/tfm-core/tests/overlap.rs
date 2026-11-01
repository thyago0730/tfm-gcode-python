//! Testes do modelo de sobreposição (`tfm_core::overlap`).
//!
//! Porta `tests/test_overlap_model.py` e acrescenta valores de referência obtidos
//! rodando o Python original (`.venv/Scripts/python.exe -I`, repr completo).
//! Valores que dependem só de `+ - * /` são comparados com `assert_eq!` (idênticos
//! bit a bit); os que passam por `asin` usam tolerância 1e-9.

use tfm_core::overlap::{compute_overlap_metrics, estimate_effective_bead_width, Uniformity};

fn assert_close(actual: f64, expected: f64, tol: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tol,
        "{what}: esperado {expected:?}, obtido {actual:?} (tol {tol:e})"
    );
}

// ---------------------------------------------------------------------------
// estimate_effective_bead_width
// ---------------------------------------------------------------------------

#[test]
fn flat_surface_matches_nominal() {
    let bead = estimate_effective_bead_width(8.0, 0.0, None, 1.0);
    assert_eq!(bead.nominal_width_mm, 8.0);
    assert_eq!(bead.surface_width_mm, 8.0);
    assert_close(
        bead.effective_width_mm,
        8.0,
        8.0 * 0.02,
        "effective_width_mm",
    );
    // Python: aspecto 0 -> process_factor 1.0 + min(0.08*0.08, 0.04) = 1.0064
    assert_eq!(bead.aspect_ratio, 0.0);
    assert_eq!(bead.process_factor, 1.0064);
    assert_eq!(bead.effective_width_mm, 8.0512);
    assert_eq!(bead.curvature_gain, 1.0);
    assert_eq!(bead.calibration_factor, 1.0);
    assert_eq!(bead.radius_mm, None);
}

#[test]
fn curved_surface_expands_width() {
    let bead = estimate_effective_bead_width(8.0, 0.0, Some(50.0), 1.0);
    assert!(bead.surface_width_mm > bead.nominal_width_mm);
    assert!(bead.curvature_gain > 1.0);
    assert_eq!(bead.radius_mm, Some(50.0));
    assert_close(
        bead.surface_width_mm,
        8.0085580033659,
        1e-9,
        "surface_width_mm",
    );
    assert_close(
        bead.curvature_gain,
        1.0010697504207375,
        1e-9,
        "curvature_gain",
    );
    assert_close(
        bead.effective_width_mm,
        8.059812774587442,
        1e-9,
        "effective_width_mm",
    );
}

#[test]
fn high_aspect_ratio_reduces_effective_width() {
    let flat = estimate_effective_bead_width(8.0, 0.5, None, 1.0);
    let tall = estimate_effective_bead_width(8.0, 2.0, None, 1.0);
    assert!(tall.effective_width_mm < flat.effective_width_mm);
    assert_eq!(flat.effective_width_mm, 8.0112);
    // aspecto 0.25 -> 1.0 - min((0.25-0.18)*0.22, 0.18) = 0.9846
    assert_eq!(tall.aspect_ratio, 0.25);
    assert_eq!(tall.process_factor, 0.9846);
    assert_eq!(tall.effective_width_mm, 7.8768);
}

#[test]
fn process_factor_neutral_in_mid_band() {
    // 0.08 <= aspecto <= 0.18 -> fator 1.0, largura efetiva igual à nominal
    let bead = estimate_effective_bead_width(8.0, 1.0, None, 1.0);
    assert_eq!(bead.aspect_ratio, 0.125);
    assert_eq!(bead.process_factor, 1.0);
    assert_eq!(bead.effective_width_mm, 8.0);
}

#[test]
fn process_factor_clamped_at_floor() {
    // aspecto 1.0 -> 1.0 - min(0.1804, 0.18); o clamp em 0.82 mantém o
    // arredondamento do Python (0.8200000000000001).
    let bead = estimate_effective_bead_width(8.0, 8.0, None, 1.0);
    assert_eq!(bead.aspect_ratio, 1.0);
    assert_eq!(bead.process_factor, 0.8200000000000001);
    assert_eq!(bead.effective_width_mm, 6.5600000000000005);
}

#[test]
fn calibration_factor_has_floor_and_zero_fallback() {
    assert_eq!(
        estimate_effective_bead_width(8.0, 0.0, None, 0.1).calibration_factor,
        0.5
    );
    // `calibration_factor or 1.0` no Python: zero recai em 1.0, não no piso 0.5.
    let zero = estimate_effective_bead_width(8.0, 0.0, None, 0.0);
    assert_eq!(zero.calibration_factor, 1.0);
    assert_eq!(zero.effective_width_mm, 8.0512);
    let scaled = estimate_effective_bead_width(8.0, 1.0, None, 1.25);
    assert_eq!(scaled.calibration_factor, 1.25);
    assert_eq!(scaled.effective_width_mm, 10.0);
}

#[test]
fn degenerate_inputs_are_floored() {
    assert_eq!(
        estimate_effective_bead_width(0.0, 0.0, None, 1.0).nominal_width_mm,
        1e-6
    );
    // raio <= 1e-6 é tratado como superfície plana
    let tiny = estimate_effective_bead_width(8.0, 0.0, Some(1e-9), 1.0);
    assert_eq!(tiny.radius_mm, None);
    assert_eq!(tiny.surface_width_mm, 8.0);
    assert_eq!(tiny.curvature_gain, 1.0);
    // raio menor que meia largura é elevado a nominal/2 + 1e-6 (corda ~ semicírculo)
    let small = estimate_effective_bead_width(8.0, 0.0, Some(2.0), 1.0);
    assert_eq!(small.radius_mm, Some(4.000001));
    assert_close(
        small.surface_width_mm,
        12.555060043681442,
        1e-9,
        "surface_width_mm",
    );
    // altura de camada negativa é fixada em 0
    let neg = estimate_effective_bead_width(8.0, -3.0, None, 1.0);
    assert_eq!(neg.aspect_ratio, 0.0);
    assert_eq!(neg.process_factor, 1.0064);
}

// ---------------------------------------------------------------------------
// compute_overlap_metrics
// ---------------------------------------------------------------------------

#[test]
fn overlap_reduces_step_over() {
    let m = compute_overlap_metrics(10.0, 40.0, 0.0, None, None, false, 1.0);
    assert!(m.step_over_mm < m.bead.effective_width_mm);
    assert_close(m.achieved_overlap_pct, 40.0, 0.5, "achieved_overlap_pct");
    // valores exatos do Python
    assert_eq!(m.bead.effective_width_mm, 10.064);
    assert_eq!(m.step_over_mm, 6.0384);
    assert_eq!(m.achieved_overlap_pct, 40.0);
    assert_eq!(m.target_overlap_pct, 40.0);
    assert_eq!(m.target_width_mm, None);
    assert_eq!(m.pass_count, 1);
    assert_eq!(m.offsets, vec![0.0]);
    assert_eq!(m.effective_cover_width_mm, 10.064);
    assert_eq!(m.coverage_ratio, 1.0);
    assert_eq!(m.fill_error_mm, 0.0);
}

#[test]
fn single_pass_when_target_narrow() {
    let m = compute_overlap_metrics(10.0, 30.0, 0.0, None, Some(8.0), false, 1.0);
    assert_eq!(m.pass_count, 1);
    assert_eq!(m.offsets, vec![0.0]);
    assert_eq!(m.target_width_mm, Some(8.0));
    assert_eq!(m.effective_cover_width_mm, m.bead.effective_width_mm);
    assert_eq!(m.coverage_ratio, 0.794912559618442);
    assert_eq!(m.fill_error_mm, 2.064);
    // cobertura 20% abaixo do alvo -> Critica
    assert_eq!(m.uniformity, Uniformity::Critica);
    assert_eq!(m.uniformity_score, 71.28775834658187);
}

#[test]
fn multiple_passes_when_target_wide() {
    let m = compute_overlap_metrics(8.0, 50.0, 0.0, None, Some(30.0), false, 1.0);
    assert!(m.pass_count >= 2);
    assert_eq!(m.offsets.len(), m.pass_count);
    // 1 + ceil((30 - 8.0512) / 4.0256) = 7 passes, simétricos em torno de 0
    assert_eq!(m.pass_count, 7);
    assert_eq!(m.step_over_mm, 4.0256);
    let expected = [
        -12.076799999999999,
        -8.051199999999998,
        -4.025599999999999,
        0.0,
        4.025600000000001,
        8.051200000000001,
        12.076799999999999,
    ];
    assert_eq!(m.offsets, expected.to_vec());
    assert_eq!(m.effective_cover_width_mm, 32.2048);
    assert_eq!(m.coverage_ratio, 0.9315381558028617);
    assert_eq!(m.uniformity, Uniformity::Atencao);
    assert_eq!(m.uniformity_score, 90.41534181240064);
}

#[test]
fn uniformity_good_for_balanced() {
    let m = compute_overlap_metrics(8.0, 35.0, 0.0, None, Some(8.0), false, 1.0);
    assert_eq!(m.uniformity, Uniformity::Boa);
    assert_eq!(m.uniformity.label(), "Boa");
    assert!(m.uniformity_score >= 80.0);
    assert_eq!(m.uniformity_score, 99.10969793322734);
}

#[test]
fn zero_overlap_clamped() {
    let m = compute_overlap_metrics(8.0, 0.0, 0.0, None, None, false, 1.0);
    assert_close(m.achieved_overlap_pct, 0.0, 0.1, "achieved_overlap_pct");
    assert_close(
        m.step_over_mm,
        m.bead.effective_width_mm,
        m.bead.effective_width_mm * 0.01,
        "step_over_mm",
    );
    assert_eq!(m.achieved_overlap_pct, 0.0);
    assert_eq!(m.step_over_mm, 8.0512);
    assert_eq!(m.target_overlap_pct, 0.0);
}

#[test]
fn overlap_pct_is_clamped_to_0_99() {
    let high = compute_overlap_metrics(8.0, 150.0, 0.0, None, None, false, 1.0);
    assert_eq!(high.target_overlap_pct, 99.0);
    assert_eq!(high.achieved_overlap_pct, 99.0);
    let low = compute_overlap_metrics(8.0, -5.0, 0.0, None, None, false, 1.0);
    assert_eq!(low.target_overlap_pct, 0.0);
    assert_eq!(low.step_over_mm, low.bead.effective_width_mm);
}

#[test]
fn force_min_two_passes() {
    let m = compute_overlap_metrics(8.0, 40.0, 0.0, None, Some(6.0), true, 1.0);
    assert_eq!(m.pass_count, 2);
    assert_eq!(m.offsets.len(), 2);
    let half = m.step_over_mm / 2.0;
    assert_close(m.offsets[0], -half, 1e-12, "offsets[0]");
    assert_close(m.offsets[1], half, 1e-12, "offsets[1]");
    assert_eq!(m.step_over_mm, 4.8307199999999995);
    assert_eq!(m.offsets, vec![-2.4153599999999997, 2.4153599999999997]);
    assert_eq!(m.effective_cover_width_mm, 8.0512 + 4.8307199999999995);
    // caso do teste Python original: alvo 8.0 também força 2 passes
    let m8 = compute_overlap_metrics(8.0, 40.0, 0.0, None, Some(8.0), true, 1.0);
    assert!(m8.pass_count >= 2);
    assert_eq!(m8.offsets, m.offsets);
}

#[test]
fn force_min_two_passes_requires_target_at_least_half_step() {
    // step 4.83072 -> meio passo 2.41536
    let below = compute_overlap_metrics(8.0, 40.0, 0.0, None, Some(2.0), true, 1.0);
    assert_eq!(below.pass_count, 1);
    let above = compute_overlap_metrics(8.0, 40.0, 0.0, None, Some(2.5), true, 1.0);
    assert_eq!(above.pass_count, 2);
    // alvo zero não força nada; cobertura 0 e erro de preenchimento = largura efetiva
    let zero = compute_overlap_metrics(8.0, 40.0, 0.0, None, Some(0.0), true, 1.0);
    assert_eq!(zero.pass_count, 1);
    assert_eq!(zero.target_width_mm, Some(0.0));
    assert_eq!(zero.coverage_ratio, 0.0);
    assert_eq!(zero.fill_error_mm, 8.0512);
    assert_eq!(zero.uniformity, Uniformity::Critica);
    assert_eq!(zero.uniformity_score, 0.0);
    // alvo negativo é fixado em 0.0 (continua Some)
    let neg = compute_overlap_metrics(8.0, 40.0, 0.0, None, Some(-3.0), false, 1.0);
    assert_eq!(neg.target_width_mm, Some(0.0));
    assert_eq!(neg.pass_count, 1);
    // sem alvo, force_min_two_passes não se aplica
    let none = compute_overlap_metrics(8.0, 40.0, 0.0, None, None, true, 1.0);
    assert_eq!(none.pass_count, 1);
}

#[test]
fn uniformity_atencao_band_and_ordering() {
    let m = compute_overlap_metrics(8.0, 40.0, 0.0, None, Some(7.5), false, 1.0);
    assert_eq!(m.uniformity, Uniformity::Atencao);
    assert_eq!(m.uniformity.label(), "Atencao");
    assert_eq!(m.coverage_ratio, 0.9315381558028617);
    assert_eq!(m.fill_error_mm, 0.5511999999999997);
    assert_eq!(m.uniformity_score, 90.41534181240064);
    assert_eq!(Uniformity::Critica.label(), "Critica");
    assert_eq!(Uniformity::Boa.to_string(), "Boa");
    assert!(Uniformity::Boa < Uniformity::Atencao);
    assert!(Uniformity::Atencao < Uniformity::Critica);
}

#[test]
fn step_over_matches_python_reference() {
    // .venv/Scripts/python.exe -I -c "... compute_overlap_metrics(8.0, 40.0,
    //   layer_height_mm=1.5, radius_mm=25.0, target_width_mm=None,
    //   force_min_two_passes=True, calibration_factor=1.0)['step_over_mm']"
    // -> 4.812765401236198
    let m = compute_overlap_metrics(8.0, 40.0, 1.5, Some(25.0), None, true, 1.0);
    assert_close(m.step_over_mm, 4.812765401, 1e-9, "step_over_mm");
    assert_close(
        m.step_over_mm,
        4.812765401236198,
        1e-12,
        "step_over_mm (repr)",
    );
    // demais campos da mesma chamada
    assert_eq!(m.bead.aspect_ratio, 0.1875);
    assert_eq!(m.bead.process_factor, 0.99835);
    assert_eq!(m.bead.radius_mm, Some(25.0));
    assert_close(
        m.bead.surface_width_mm,
        8.03453264759553,
        1e-9,
        "surface_width_mm",
    );
    assert_close(
        m.bead.effective_width_mm,
        8.021275668726997,
        1e-9,
        "effective_width_mm",
    );
    assert_close(
        m.bead.curvature_gain,
        1.0043165809494412,
        1e-9,
        "curvature_gain",
    );
    assert_eq!(m.achieved_overlap_pct, 40.0);
    assert_eq!(m.pass_count, 1);
    assert_eq!(m.offsets, vec![0.0]);
    assert_eq!(m.coverage_ratio, 1.0);
    assert_eq!(m.uniformity, Uniformity::Boa);
    assert_close(
        m.uniformity_score,
        99.84891966676956,
        1e-9,
        "uniformity_score",
    );
}

// ---------------------------------------------------------------------------
// Robustez: largura-alvo não finita e limite de passes
// ---------------------------------------------------------------------------

#[test]
fn non_finite_target_is_single_pass() {
    // "inf"/"nan" vindos de um campo de texto não podem derrubar o cálculo:
    // alvo não finito é tratado exatamente como ausência de alvo.
    let reference = compute_overlap_metrics(8.0, 40.0, 0.0, None, None, false, 1.0);
    for target in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        let m = compute_overlap_metrics(8.0, 40.0, 0.0, None, Some(target), true, 1.0);
        assert_eq!(m.pass_count, 1, "target {target:?}");
        assert_eq!(m.offsets, vec![0.0], "target {target:?}");
        assert_eq!(m.target_width_mm, None, "target {target:?}");
        assert_eq!(m.coverage_ratio, 1.0, "target {target:?}");
        assert_eq!(m.fill_error_mm, 0.0, "target {target:?}");
        assert_eq!(m, reference, "target {target:?}");
    }
}

#[test]
fn pass_count_is_clamped() {
    // nominal 1e-3 e alvo 1e9 pediriam ~1e14 passes; o limite evita a alocação.
    let m = compute_overlap_metrics(1e-3, 99.0, 0.0, None, Some(1e9), false, 1.0);
    assert_eq!(m.pass_count, 100_000);
    assert_eq!(m.pass_count, tfm_core::overlap::MAX_PASSES);
    assert_eq!(m.offsets.len(), 100_000);
    // offsets continuam simétricos e com o passo correto
    let span = m.step_over_mm * 99_999.0;
    assert_eq!(m.offsets[0], -span / 2.0);
    assert_eq!(
        m.offsets[99_999],
        (-span / 2.0) + (99_999.0 * m.step_over_mm)
    );
    assert_eq!(m.effective_cover_width_mm, m.bead.effective_width_mm + span);
    assert_eq!(m.target_width_mm, Some(1e9));
    // abaixo do limite nada muda: 7 passes como antes
    let small = compute_overlap_metrics(8.0, 50.0, 0.0, None, Some(30.0), false, 1.0);
    assert_eq!(small.pass_count, 7);
}
