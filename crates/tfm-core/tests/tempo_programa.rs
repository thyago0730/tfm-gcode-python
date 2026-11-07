//! O tempo das estimativas é o tempo de máquina do G-code gerado.
//!
//! O modelo herdado do Python estimava cada operação por uma conta própria, e
//! nas oscilações errava de 6 a 16 vezes. Agora o tempo é a soma, linha a linha
//! do programa, da distância lida pelo Mach3 dividida pelo F, mais as esperas
//! `G04`. Custos de mão de obra, máquina e gás saem desse tempo.

use tfm_core::config::AppConfig;
use tfm_core::estimates::compute;
use tfm_core::gcode::generate;
use tfm_core::params::{BolachaOscForma, Operation, Params};

fn word(line: &str, axis: char) -> Option<f64> {
    line.split_whitespace()
        .find_map(|w| w.strip_prefix(axis).and_then(|v| v.parse().ok()))
}

/// Tempo de máquina, escrito à parte do motor para conferir o motor.
fn maquina(lines: &[String]) -> f64 {
    let (mut x, mut y, mut z, mut a, mut f) = (0.0, 0.0, 0.0, 0.0, 0.0);
    let mut min = 0.0;
    for raw in lines {
        let l = raw.trim();
        // Comentários e G53 (coordenada de máquina) não mexem na posição da peça.
        if l.starts_with('(') || l.contains("G53") {
            continue;
        }
        if l.starts_with("G04") {
            min += word(l, 'P').unwrap_or(0.0) / 60.0;
            continue;
        }
        let nx = word(l, 'X').unwrap_or(x);
        let ny = word(l, 'Y').unwrap_or(y);
        let nz = word(l, 'Z').unwrap_or(z);
        let na = word(l, 'A').unwrap_or(a);
        if let Some(nf) = word(l, 'F') {
            f = nf;
        }
        let g01 = l.starts_with("G01") || l.starts_with("G1 ");
        if g01 && f > 0.0 {
            let d =
                ((nx - x).powi(2) + (ny - y).powi(2) + (nz - z).powi(2) + (na - a).powi(2)).sqrt();
            min += d / f;
        }
        if l.starts_with("G0") || l.starts_with("G1") {
            (x, y, z, a) = (nx, ny, nz, na);
        }
    }
    min
}

fn params(op: Operation) -> Params {
    let mut p = Params::default();
    p.apply_operation(op);
    p
}

fn minutos(p: &Params) -> f64 {
    compute(p, &AppConfig::default()).time_hours * 60.0
}

#[test]
fn every_operation_time_is_the_machine_time_of_its_program() {
    for op in Operation::ALL {
        let p = params(op);
        let real = maquina(&generate(&p).expect("gera"));
        let est = minutos(&p);
        assert!(
            (est - real).abs() <= real * 0.005,
            "{op:?}: estimado {est:.3} min contra {real:.3} min de máquina"
        );
    }
}

#[test]
fn linear_oscillation_shows_the_pitch_of_its_program() {
    let p = params(Operation::OscilacaoLinear);
    let e = compute(&p, &AppConfig::default());
    assert_eq!(e.pitch.text, "4.800 (axial)");
    let lines = generate(&p).expect("gera");
    let passos = lines
        .iter()
        .filter(|l| l.starts_with("(--- PASSO AXIAL"))
        .count();
    assert_eq!(e.rotations, format!("{passos}.00 (passos axiais)"));
}

/// A camada raiz mais lenta vale em todas as operações, e o tempo mostra.
#[test]
fn slower_root_layer_takes_longer() {
    for op in Operation::ALL {
        let mut p = params(op);
        if op != Operation::EspiralMultiperfil {
            // No multiperfil o número de camadas vem do perfil.
            p.num_camadas = 2;
        }
        let base = minutos(&p);
        p.smart_layers = true;
        p.root_feed_scale = 50.0;
        let lenta = minutos(&p);
        assert!(
            lenta > base * 1.1,
            "{op:?}: raiz a 50%: {lenta:.2} contra {base:.2}"
        );
    }
}

#[test]
fn rotation_compensation_shortens_the_spiral() {
    let mut p = params(Operation::Espiral);
    p.fechamento_uniforme = true;
    let sem = minutos(&p);
    p.compensar_rotacao = true;
    let com = minutos(&p);
    assert!(com < sem, "compensada acelera: {com:.2} contra {sem:.2}");
}

#[test]
fn dwells_count_in_the_time() {
    let mut p = params(Operation::Bolacha);
    p.bolacha_oscilar = true;
    p.bolacha_osc_forma = BolachaOscForma::Triangular;
    p.oscilacao_comprimento = 4.0;
    let sem = minutos(&p);
    p.bolacha_osc_espera_s = 0.5;
    let lines = generate(&p).expect("gera");
    let esperas = lines
        .iter()
        .filter(|l| l.contains("Espera na ponta"))
        .count();
    let com = minutos(&p);
    let extra = esperas as f64 * 0.5 / 60.0;
    assert!(
        (com - sem - extra).abs() < 0.01,
        "{esperas} esperas: {com:.3} contra {sem:.3} + {extra:.3}"
    );
}

/// Com o config padrão, mão de obra, máquina e gás saem do tempo de máquina:
/// as fórmulas padrão herdadas do Python, de conta de cilindro, não mandam.
#[test]
fn costs_follow_the_machine_time_with_the_default_formulas() {
    let cfg = AppConfig::default();
    for op in Operation::ALL {
        let p = params(op);
        let e = compute(&p, &cfg);
        let h = e.time_hours;
        let c = &e.costs;
        let what = format!("{op:?}");
        assert!(
            (c.labor_cost - h * cfg.costs.labor_brl_hour).abs() < 1e-6,
            "{what} mão de obra"
        );
        assert!(
            (c.machine_cost - h * cfg.costs.machine_brl_hour).abs() < 1e-6,
            "{what} máquina"
        );
        let gas = p.vazao_gas * 60.0 / 1000.0 * h;
        assert!((c.gas_m3 - gas).abs() < 1e-9, "{what} gás");
        assert!(
            (c.gas_cost - gas * cfg.costs.gas_argon_brl_m3).abs() < 1e-6,
            "{what} custo do gás"
        );
    }
}

/// Uma fórmula que o usuário mudou continua mandando.
#[test]
fn a_customized_formula_still_overrides() {
    let mut cfg = AppConfig::default();
    cfg.formulas
        .expressions
        .insert("labor_cost_brl".to_string(), "123.0".to_string());
    let e = compute(&params(Operation::Espiral), &cfg);
    assert_eq!(e.costs.labor_cost, 123.0);
}

/// O setup (opcional) entra na mão de obra e na máquina, não no tempo de arco
/// nem no gás.
#[test]
fn setup_time_adds_to_labor_and_machine_only() {
    let cfg = AppConfig::default();
    let p = params(Operation::Espiral);
    assert_eq!(p.tempo_setup_min, 0.0);
    let base = compute(&p, &cfg);
    let mut q = p.clone();
    q.tempo_setup_min = 30.0;
    let e = compute(&q, &cfg);
    assert_eq!(e.time_hours, base.time_hours, "tempo de arco igual");
    assert_eq!(e.costs.gas_m3, base.costs.gas_m3, "gás igual");
    let mo = e.costs.labor_cost - base.costs.labor_cost;
    let mq = e.costs.machine_cost - base.costs.machine_cost;
    assert!((mo - 0.5 * cfg.costs.labor_brl_hour).abs() < 1e-9, "{mo}");
    assert!((mq - 0.5 * cfg.costs.machine_brl_hour).abs() < 1e-9, "{mq}");
    // Negativo ou inválido não desconta.
    q.tempo_setup_min = -10.0;
    assert_eq!(compute(&q, &cfg).costs.labor_cost, base.costs.labor_cost);
}
