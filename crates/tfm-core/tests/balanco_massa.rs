//! Espessura pelo balanço de massa: o pó depositado no tempo de arco, espalhado
//! na área revestida, dá a espessura que a máquina realmente faz.
//!
//! `espessura = taxa (g/min) × tempo de arco (min) ÷ (densidade × área × camadas)`.
//! O Z sobe pela espessura pedida; se o balanço diferir mais de 15%, o
//! afastamento da tocha muda a cada camada, e o app avisa.

use std::f64::consts::PI;

use tfm_core::config::AppConfig;
use tfm_core::estimates::{compute, thickness_balance_alert};
use tfm_core::gcode::generate;
use tfm_core::params::{Operation, Params};
use tfm_core::toolpath::machine_time;

fn params(op: Operation) -> Params {
    let mut p = Params::default();
    p.apply_operation(op);
    p
}

fn balanco(p: &Params) -> Option<f64> {
    compute(p, &AppConfig::default()).espessura_balanco_mm
}

#[test]
fn spiral_balance_is_deposited_mass_over_density_and_area() {
    let p = params(Operation::Espiral);
    let arco = machine_time(&generate(&p).expect("gera")).feed_min;
    let massa_g = p.taxa_deposicao_g_h / 60.0 * arco;
    let area = PI * 50.0 * 50.0;
    let esperado = massa_g / (p.densidade_po_g_cm3 / 1000.0 * area);
    let got = balanco(&p).expect("tem balanço");
    assert!((got - esperado).abs() < 1e-6, "{got} contra {esperado}");
    // Com o padrão o balanço dá perto de 2,4 mm, contra 1,5 mm pedidos.
    assert!((got - 2.39).abs() < 0.05, "{got}");
}

#[test]
fn balance_scales_with_rate_and_density() {
    let p = params(Operation::ChapaPlana);
    let base = balanco(&p).expect("tem");
    let mut q = p.clone();
    q.taxa_deposicao_g_h *= 2.0;
    assert!((balanco(&q).unwrap() - 2.0 * base).abs() < 1e-9);
    let mut q = p.clone();
    q.densidade_po_g_cm3 = 14.5;
    assert!((balanco(&q).unwrap() - base * 8.46 / 14.5).abs() < 1e-9);
}

#[test]
fn every_operation_has_a_finite_balance() {
    for op in Operation::ALL {
        let b = balanco(&params(op));
        assert!(b.is_some_and(|v| v.is_finite() && v > 0.0), "{op:?}: {b:?}");
    }
}

#[test]
fn no_balance_without_rate_or_thickness() {
    let mut p = params(Operation::Espiral);
    p.taxa_deposicao_g_h = 0.0;
    assert_eq!(balanco(&p), None);
    let mut p = params(Operation::Espiral);
    p.espessura_camada = 0.0;
    assert_eq!(balanco(&p), None);
}

#[test]
fn alert_only_beyond_fifteen_percent() {
    assert_eq!(thickness_balance_alert(1.5, 1.6), None);
    assert_eq!(thickness_balance_alert(1.5, 1.3), None);
    let a = thickness_balance_alert(1.5, 2.39).expect("avisa");
    assert!(a.contains("2.39") && a.contains("1.50"), "{a}");
    assert!(thickness_balance_alert(1.5, 1.2).is_some());
    assert_eq!(thickness_balance_alert(0.0, 2.0), None);
}
