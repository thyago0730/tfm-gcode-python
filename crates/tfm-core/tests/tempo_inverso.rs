//! G93 (tempo inverso): cada `G01` traz `F = 1 ÷ minutos` da própria linha.
//!
//! Com o eixo A, o Mach3 lê o F em G94 de um jeito que depende do "raio do eixo
//! A" configurado na máquina. Em G93 o tempo de cada linha vem escrito nela, e a
//! velocidade real sobre a peça não depende dessa configuração. A opção fica
//! desligada por padrão; ligada, o programa tem o mesmo tempo de máquina.

use tfm_core::gcode::generate_with_timestamp;
use tfm_core::params::{Operation, Params};
use tfm_core::toolpath::{build_toolpath, machine_time};

const TS: &str = "01/01/2026 00:00:00";

fn params(op: Operation) -> Params {
    let mut p = Params::default();
    p.apply_operation(op);
    p
}

fn gera(p: &Params) -> Vec<String> {
    generate_with_timestamp(p, TS).expect("gera")
}

fn codigo(l: &str) -> &str {
    l.split('(').next().unwrap_or(l).trim()
}

fn is_g01(l: &str) -> bool {
    codigo(l)
        .split_whitespace()
        .any(|t| t == "G01" || t == "G1")
}

#[test]
fn off_by_default_and_program_stays_in_g94() {
    let p = params(Operation::Espiral);
    assert!(!p.tempo_inverso_g93);
    let lines = gera(&p);
    assert!(lines.iter().any(|l| l == "G21 G90 G94"));
    assert!(!lines.iter().any(|l| codigo(l).contains("G93")));
}

#[test]
fn every_operation_keeps_its_machine_time_in_g93() {
    for op in Operation::ALL {
        let mut p = params(op);
        let g94 = gera(&p);
        p.tempo_inverso_g93 = true;
        let g93 = gera(&p);
        let what = format!("{op:?}");

        assert!(
            g93.iter().any(|l| l == "G21 G90 G93"),
            "{what}: cabeçalho em G93"
        );
        assert!(!g93.iter().any(|l| l == "G21 G90 G94"), "{what}");
        // Todo G01 em G93 precisa do próprio F.
        for l in g93.iter().filter(|l| is_g01(l)) {
            assert!(
                codigo(l).split_whitespace().any(|t| t.starts_with('F')),
                "{what}: G01 sem F em G93: {l}"
            );
        }
        // O programa volta ao G94 antes do fim.
        let m30 = g93.iter().position(|l| l.starts_with("M30")).expect("M30");
        let g94_fim = g93.iter().rposition(|l| codigo(l) == "G94").expect("G94");
        assert!(g94_fim < m30, "{what}: G94 antes do M30");

        let t94 = machine_time(&g94).total_min();
        let t93 = machine_time(&g93).total_min();
        assert!(
            (t93 - t94).abs() <= t94 * 1e-4,
            "{what}: {t93:.5} min em G93 contra {t94:.5} min em G94"
        );
        // Mesma quantidade de linhas de movimento: só o F muda.
        assert_eq!(
            g93.iter().filter(|l| is_g01(l)).count(),
            g94.iter().filter(|l| is_g01(l)).count(),
            "{what}"
        );
    }
}

/// A simulação lê o F em G93 como tempo e mostra a mesma velocidade.
#[test]
fn toolpath_reads_the_same_feed_in_g93() {
    for op in [
        Operation::Espiral,
        Operation::Bolacha,
        Operation::OscilacaoLinear,
    ] {
        let mut p = params(op);
        let a = build_toolpath(&gera(&p), &p);
        p.tempo_inverso_g93 = true;
        let b = build_toolpath(&gera(&p), &p);
        let fa: Vec<f32> = a
            .points()
            .filter(|pt| !pt.rapid)
            .map(|pt| pt.feed)
            .collect();
        let fb: Vec<f32> = b
            .points()
            .filter(|pt| !pt.rapid)
            .map(|pt| pt.feed)
            .collect();
        assert_eq!(fa.len(), fb.len(), "{op:?}");
        for (i, (x, y)) in fa.iter().zip(&fb).enumerate() {
            // Linhas sem deslocamento não têm velocidade a comparar.
            if *x > 0.0 && *y > 0.0 {
                assert!(
                    (x - y).abs() <= x.abs() * 1e-3,
                    "{op:?} ponto {i}: {x} contra {y}"
                );
            }
        }
    }
}
