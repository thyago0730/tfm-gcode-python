//! Bolacha sem oscilação (espiral ou anéis) com a tocha em X ou em Y. Com a
//! tocha em Y, o Y faz o raio e o X fica em 0; a trajetória na peça é a mesma.

use tfm_core::gcode::generate;
use tfm_core::params::{BolachaOscEixo, BolachaTipo, BolachaZero, Operation, Params};
use tfm_core::toolpath::{build_toolpath, machine_time};

fn word(line: &str, axis: char) -> Option<f64> {
    line.split('(')
        .next()
        .unwrap_or(line)
        .split_whitespace()
        .find_map(|w| w.strip_prefix(axis).and_then(|v| v.parse().ok()))
}

fn peca(tipo: BolachaTipo, eixo: BolachaOscEixo) -> Params {
    let mut p = Params::default();
    p.apply_operation(Operation::Bolacha);
    p.diametro = 439.0;
    p.diametro_interno = 358.0;
    p.bolacha_tipo = tipo;
    p.bolacha_oscilar = false;
    p.bolacha_osc_eixo = eixo;
    p
}

/// Linhas de movimento depois da primeira camada, sem as `G53`.
fn movimentos(lines: &[String]) -> Vec<&String> {
    let ini = lines
        .iter()
        .position(|l| l.starts_with("(--- CAMADA"))
        .unwrap();
    lines[ini..]
        .iter()
        .filter(|l| l.starts_with("G0") && !l.contains("G53"))
        .collect()
}

#[test]
fn spiral_and_rings_follow_the_torch_axis() {
    for tipo in [BolachaTipo::EspiralContinua, BolachaTipo::Aneis] {
        let em_x = generate(&peca(tipo, BolachaOscEixo::X)).expect("gera");
        let em_y = generate(&peca(tipo, BolachaOscEixo::Y)).expect("gera");
        let (mx, my) = (movimentos(&em_x), movimentos(&em_y));
        assert_eq!(mx.len(), my.len(), "{tipo:?}");
        for (a, b) in mx.iter().zip(&my) {
            assert_eq!(word(a, 'X'), word(b, 'Y'), "{tipo:?}: {a} contra {b}");
            assert!(
                word(b, 'X').is_none_or(|v| v.abs() < 1e-9),
                "{tipo:?}: X fica em 0: {b}"
            );
            if b.starts_with("G00") && word(b, 'Y').is_some() {
                assert_eq!(word(b, 'X'), Some(0.0), "{tipo:?}: G00 põe X0: {b}");
            }
        }
        assert!(
            my.iter().any(|l| word(l, 'Y').is_some_and(|v| v > 170.0)),
            "{tipo:?}: o Y faz o raio"
        );
        if tipo == BolachaTipo::Aneis {
            assert!(em_y
                .iter()
                .any(|l| l.starts_with("(--- ANEL") && l.contains(" em Y=")));
        }
        let (tx, ty) = (
            machine_time(&em_x).total_min(),
            machine_time(&em_y).total_min(),
        );
        assert!((tx - ty).abs() < 1e-6 * tx, "{tipo:?}: mesmo tempo");
    }
}

/// A simulação mostra o mesmo disco revestido com a tocha em X ou em Y.
#[test]
fn toolpath_radius_is_the_same_with_the_torch_in_y() {
    let raios = |p: &Params| -> Vec<f64> {
        build_toolpath(&generate(p).expect("gera"), p)
            .points()
            .filter(|q| q.torch_on)
            .map(|q| f64::from((q.pos.x.powi(2) + q.pos.y.powi(2)).sqrt()))
            .collect()
    };
    for tipo in [BolachaTipo::EspiralContinua, BolachaTipo::Aneis] {
        let rx = raios(&peca(tipo, BolachaOscEixo::X));
        let ry = raios(&peca(tipo, BolachaOscEixo::Y));
        assert_eq!(rx.len(), ry.len());
        for (a, b) in rx.iter().zip(&ry) {
            assert!((a - b).abs() < 1e-3, "{tipo:?}: {a} contra {b}");
        }
    }
}

#[test]
fn inner_zero_shifts_y_when_the_torch_is_in_y() {
    let mut p = peca(BolachaTipo::EspiralContinua, BolachaOscEixo::Y);
    p.bolacha_zero = BolachaZero::DiametroInterno;
    let g = generate(&p).expect("gera");
    let ys: Vec<f64> = movimentos(&g).iter().filter_map(|l| word(l, 'Y')).collect();
    assert!(!ys.is_empty());
    assert!(ys.iter().all(|v| *v >= -1e-6 && *v < 45.0), "{ys:?}");
    assert!(g.iter().any(|l| l.contains("ZERO Y NO DIAMETRO INTERNO")));
}
