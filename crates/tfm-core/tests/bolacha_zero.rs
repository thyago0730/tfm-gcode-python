//! Zero da peça na Bolacha: no centro (padrão) ou no diâmetro interno. No
//! diâmetro interno o eixo radial escreve `raio − raio interno`, e a tocha não
//! precisa ir ao centro: ganha curso da máquina. A trajetória na peça é a mesma.

use tfm_core::gcode::generate;
use tfm_core::params::{BolachaOscEixo, BolachaTipo, BolachaZero, Operation, Params};
use tfm_core::toolpath::{build_toolpath, machine_time};
use tfm_core::validator::validate_physical;

fn word(line: &str, axis: char) -> Option<f64> {
    line.split('(')
        .next()
        .unwrap_or(line)
        .split_whitespace()
        .find_map(|w| w.strip_prefix(axis).and_then(|v| v.parse().ok()))
}

fn peca(oscilar: bool, eixo: BolachaOscEixo) -> Params {
    let mut p = Params::default();
    p.apply_operation(Operation::Bolacha);
    p.diametro = 439.0;
    p.diametro_interno = 358.0;
    p.bolacha_tipo = BolachaTipo::Aneis;
    p.bolacha_oscilar = oscilar;
    p.bolacha_osc_eixo = eixo;
    p.oscilacao_comprimento = 5.0;
    p
}

/// Valores do eixo radial depois da primeira camada, sem as linhas `G53`.
fn radiais(lines: &[String], eixo: char) -> Vec<f64> {
    let ini = lines
        .iter()
        .position(|l| l.starts_with("(--- CAMADA"))
        .unwrap();
    lines[ini..]
        .iter()
        .filter(|l| l.starts_with("G0") && !l.contains("G53"))
        .filter_map(|l| word(l, eixo))
        .collect()
}

#[test]
fn defaults_to_the_center() {
    assert_eq!(Params::default().bolacha_zero, BolachaZero::Centro);
}

#[test]
fn inner_zero_shifts_only_the_radial_axis() {
    for (oscilar, eixo) in [
        (false, BolachaOscEixo::X),
        (true, BolachaOscEixo::X),
        (true, BolachaOscEixo::Y),
    ] {
        let letra = if oscilar && eixo == BolachaOscEixo::Y {
            'Y'
        } else {
            'X'
        };
        let mut p = peca(oscilar, eixo);
        let centro = generate(&p).expect("gera");
        p.bolacha_zero = BolachaZero::DiametroInterno;
        let interno = generate(&p).expect("gera");
        let (rc, ri) = (radiais(&centro, letra), radiais(&interno, letra));
        assert_eq!(rc.len(), ri.len(), "{oscilar} {eixo:?}");
        for (c, i) in rc.iter().zip(&ri) {
            assert!(
                (c - 179.0 - i).abs() < 1e-6,
                "{oscilar} {eixo:?}: {c} contra {i}"
            );
        }
        assert!(
            ri.iter().all(|v| *v >= -1e-6),
            "nada passa para dentro do furo"
        );
        assert!(
            ri.iter().any(|v| v.abs() < 3.0),
            "o primeiro anel fica perto do zero"
        );
        // Com a tocha em Y, o X continua em 0.
        if letra == 'Y' {
            assert!(radiais(&interno, 'X').iter().all(|v| v.abs() < 1e-9));
        }
        let (tc, ti) = (
            machine_time(&centro).total_min(),
            machine_time(&interno).total_min(),
        );
        assert!((tc - ti).abs() < 1e-6 * tc, "mesmo tempo");
        assert!(
            interno
                .iter()
                .any(|l| l.contains("ZERO") && l.contains("DIAMETRO INTERNO")),
            "o cabeçalho avisa onde está o zero"
        );
    }
}

/// A simulação mostra a mesma trajetória na peça com qualquer zero.
#[test]
fn toolpath_is_the_same_on_the_disc() {
    for (oscilar, eixo) in [(false, BolachaOscEixo::X), (true, BolachaOscEixo::Y)] {
        let mut p = peca(oscilar, eixo);
        let a = build_toolpath(&generate(&p).expect("gera"), &p);
        p.bolacha_zero = BolachaZero::DiametroInterno;
        let b = build_toolpath(&generate(&p).expect("gera"), &p);
        let pa: Vec<_> = a.points().filter(|q| q.torch_on).map(|q| q.pos).collect();
        let pb: Vec<_> = b.points().filter(|q| q.torch_on).map(|q| q.pos).collect();
        assert_eq!(pa.len(), pb.len());
        for (x, y) in pa.iter().zip(&pb) {
            assert!(
                (x.x - y.x).abs() < 1e-3 && (x.y - y.y).abs() < 1e-3,
                "{x:?} contra {y:?}"
            );
        }
    }
}

#[test]
fn inner_zero_needs_an_inner_diameter() {
    let mut p = peca(false, BolachaOscEixo::X);
    p.bolacha_zero = BolachaZero::DiametroInterno;
    assert!(validate_physical(&p).is_valid());
    p.diametro_interno = 0.0;
    assert!(validate_physical(&p)
        .invalid_fields
        .contains(&"bolacha_zero"));
}

#[test]
fn old_procedures_load_with_the_center() {
    let p: Params = serde_json::from_str(r#"{"welding_mode": "bolacha"}"#).expect("carrega");
    assert_eq!(p.bolacha_zero, BolachaZero::Centro);
}
