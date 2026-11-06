//! Aporte térmico e velocidade LOCAIS do programa gerado.
//!
//! O aporte nominal usa a velocidade pedida. A compensação de rotação muda a
//! velocidade de cada trecho (até 2,5 vezes), e com ela o aporte: onde a placa
//! desacelera, o calor por milímetro sobe na mesma proporção. Estes testes
//! medem isso no percurso real e conferem as faixas da EPS contra o valor local.

use indexmap::IndexMap;
use tfm_core::config::AppConfig;
use tfm_core::deposit::{local_process, LocalProcess};
use tfm_core::estimates;
use tfm_core::gcode::generate;
use tfm_core::params::{Operation, Params};
use tfm_core::quality::{Limit, QualityManager};
use tfm_core::toolpath::build_toolpath;

fn local(p: &Params) -> LocalProcess {
    let lines = generate(p).expect("gera o programa");
    local_process(&build_toolpath(&lines, p), p).expect("há solda")
}

fn nominal_heat(p: &Params) -> f64 {
    estimates::compute(p, &AppConfig::default()).heat_input_kj_mm
}

fn assert_near(actual: f64, expected: f64, tol_pct: f64, what: &str) {
    let err = (actual - expected).abs() / expected.abs().max(1e-9) * 100.0;
    assert!(
        err <= tol_pct,
        "{what}: esperado {expected:.3}, obtido {actual:.3} ({err:.1}% de diferença)"
    );
}

fn espiral(compensar: bool) -> Params {
    let mut p = Params::default();
    p.apply_operation(Operation::Espiral);
    p.fechamento_uniforme = true;
    p.compensar_rotacao = compensar;
    p
}

#[test]
fn without_compensation_the_local_heat_is_the_nominal_one() {
    let p = espiral(false);
    let l = local(&p);
    let nominal = nominal_heat(&p);
    assert_near(l.heat_min, nominal, 4.0, "aporte mínimo");
    assert_near(l.heat_max, nominal, 4.0, "aporte máximo");
    assert_near(l.speed_min, p.taxa(), 4.0, "velocidade mínima");
    assert_near(l.speed_max, p.taxa(), 4.0, "velocidade máxima");
}

/// A Espiral compensada só acelera: o aporte local fica abaixo do nominal.
#[test]
fn spiral_compensation_lowers_the_local_heat() {
    let p = espiral(true);
    let l = local(&p);
    let nominal = nominal_heat(&p);
    assert!(l.speed_max > p.taxa() * 1.2, "acelera: {:?}", l);
    assert!(l.speed_max <= p.taxa() * 2.55, "teto de 2,5 vezes: {:?}", l);
    assert!(
        l.heat_min < nominal * 0.85,
        "aporte cai: {l:?} contra {nominal}"
    );
    assert_near(l.heat_max, nominal, 4.0, "o máximo segue o nominal");
    assert_near(
        l.heat_min * l.speed_max,
        nominal * p.taxa(),
        3.0,
        "aporte × velocidade é constante",
    );
}

/// A quadrada compensada só desacelera: é aí que o aporte local sobe.
#[test]
fn square_compensation_raises_the_local_heat() {
    let mut p = Params::default();
    p.apply_operation(Operation::OscilacaoQuadrada);
    p.fechamento_uniforme = true;
    p.compensar_rotacao = true;
    let l = local(&p);
    let sem = Params {
        compensar_rotacao: false,
        ..p.clone()
    };
    let sem_l = local(&sem);
    assert!(
        l.heat_max > sem_l.heat_max * 1.1,
        "a desaceleração sobe o aporte: {l:?} contra {sem_l:?}"
    );
    assert!(
        l.speed_min < sem_l.speed_min * 0.9,
        "{l:?} contra {sem_l:?}"
    );
}

#[test]
fn flat_plate_speed_is_the_programmed_feed() {
    let mut p = Params::default();
    p.apply_operation(Operation::ChapaPlana);
    let l = local(&p);
    let nominal = nominal_heat(&p);
    assert_near(l.heat_min, nominal, 4.0, "aporte mínimo");
    assert_near(l.heat_max, nominal, 4.0, "aporte máximo");
}

#[test]
fn disc_has_a_local_speed_too() {
    let mut p = Params::default();
    p.apply_operation(Operation::Bolacha);
    let l = local(&p);
    assert!(l.speed_min > 0.0 && l.speed_max >= l.speed_min, "{l:?}");
    assert!(l.heat_min > 0.0 && l.heat_max >= l.heat_min, "{l:?}");
}

fn faixas(pairs: &[(&str, f64, f64)]) -> IndexMap<String, Limit> {
    pairs
        .iter()
        .map(|(k, min, max)| {
            (
                (*k).to_string(),
                Limit {
                    min: *min,
                    max: *max,
                },
            )
        })
        .collect()
}

fn amostra() -> LocalProcess {
    LocalProcess {
        speed_min: 600.0,
        speed_max: 1500.0,
        heat_min: 0.8,
        heat_max: 2.1,
    }
}

#[test]
fn eps_range_is_checked_against_the_local_values() {
    let dentro = faixas(&[
        ("aporte_termico", 0.5, 2.5),
        ("velocidade_de_deposicao", 500.0, 1600.0),
    ]);
    assert!(QualityManager::local_alerts(Some(&dentro), &amostra()).is_empty());

    let aporte = faixas(&[("aporte_termico", 0.5, 1.8)]);
    let avisos = QualityManager::local_alerts(Some(&aporte), &amostra());
    assert_eq!(avisos.len(), 1, "{avisos:?}");
    assert!(avisos[0].contains("2.10 kJ/mm"), "{}", avisos[0]);
    assert!(avisos[0].contains("0.50 - 1.80"), "{}", avisos[0]);

    let velocidade = faixas(&[("velocidade_de_deposicao", 700.0, 1400.0)]);
    let avisos = QualityManager::local_alerts(Some(&velocidade), &amostra());
    assert_eq!(avisos.len(), 2, "mínima e máxima fora: {avisos:?}");
    assert!(avisos.iter().any(|a| a.contains("600")), "{avisos:?}");
    assert!(avisos.iter().any(|a| a.contains("1500")), "{avisos:?}");
}

/// Sem EPS, valem as faixas gerais do aporte (0,1 a 3,0 kJ/mm).
#[test]
fn without_eps_the_general_heat_range_applies() {
    let alto = LocalProcess {
        heat_max: 3.4,
        ..amostra()
    };
    let avisos = QualityManager::local_alerts(None, &alto);
    assert_eq!(avisos.len(), 1, "{avisos:?}");
    assert!(avisos[0].contains("3.40"), "{}", avisos[0]);
    let baixo = LocalProcess {
        heat_min: 0.05,
        ..amostra()
    };
    assert_eq!(QualityManager::local_alerts(None, &baixo).len(), 1);
    assert!(QualityManager::local_alerts(None, &amostra()).is_empty());
}

#[test]
fn unknown_eps_keys_are_ignored() {
    let outras = faixas(&[("diametro", 10.0, 20.0)]);
    assert!(QualityManager::local_alerts(Some(&outras), &amostra()).is_empty());
}
