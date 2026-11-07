//! Largura do cordão ajustada na Bolacha: com a faixa "mín. até máx." ligada,
//! o app escolhe quantos anéis cabem para o passo fechar a largura da peça
//! exatamente, sem o último anel ficar espremido contra a borda. A velocidade
//! muda na proporção inversa do passo, para a espessura não mudar.

use tfm_core::gcode::bolacha::passo_radial;
use tfm_core::gcode::generate;
use tfm_core::params::{BolachaOscEixo, BolachaTipo, Operation, Params};
use tfm_core::validator::validate_physical;

fn word(line: &str, axis: char) -> Option<f64> {
    line.split_whitespace()
        .find_map(|w| w.strip_prefix(axis).and_then(|v| v.parse().ok()))
}

/// A peça da oficina: Ø439 × Ø358, cordão 8 mm, 40% de sobreposição.
fn peca(oscilar: bool) -> Params {
    let mut p = Params::default();
    p.apply_operation(Operation::Bolacha);
    p.diametro = 439.0;
    p.diametro_interno = 358.0;
    p.bolacha_tipo = BolachaTipo::Aneis;
    p.bolacha_oscilar = oscilar;
    p.bolacha_osc_eixo = BolachaOscEixo::Y;
    p.oscilacao_comprimento = 5.0;
    p
}

/// Raios dos anéis, pelos comentários `(--- ANEL i/n em X=r ---)`.
fn aneis(lines: &[String]) -> Vec<f64> {
    lines
        .iter()
        .filter(|l| l.starts_with("(--- ANEL"))
        .filter_map(|l| {
            let k = l.find("em ")? + 5;
            l[k..].trim_end_matches(" ---)").parse().ok()
        })
        .collect()
}

fn passos(r: &[f64]) -> Vec<f64> {
    r.windows(2).map(|w| w[1] - w[0]).collect()
}

#[test]
fn off_by_default() {
    let p = peca(true);
    assert_eq!((p.largura_cordao_min, p.largura_cordao_max), (0.0, 0.0));
    let r = passo_radial(&p, 40.5 - 6.5, 5.0);
    assert_eq!(r.fator_velocidade, 1.0);
}

#[test]
fn rings_close_the_width_with_equal_steps_inside_the_range() {
    for oscilar in [false, true] {
        let mut p = peca(oscilar);
        p.largura_cordao_min = 7.0;
        p.largura_cordao_max = 9.0;
        let r = aneis(&generate(&p).expect("gera"));
        let d = passos(&r);
        assert!(d.len() >= 2, "{r:?}");
        let primeiro = d[0];
        for s in &d {
            assert!(
                (s - primeiro).abs() < 2e-3,
                "oscilar={oscilar}: passos {d:?}"
            );
        }
        let extra = if oscilar { 5.0 } else { 0.0 };
        let (s_min, s_max) = ((7.0 + extra) * 0.6, (9.0 + extra) * 0.6);
        // Com a oscilação de 5 mm nenhum número de anéis cabe entre 7 e 9 mm:
        // vale o mais perto da faixa, com folga de um passo de 0,1 mm.
        let (s_min, s_max) = if oscilar {
            (s_min - 0.15, s_max)
        } else {
            (s_min, s_max)
        };
        assert!(
            (s_min - 1e-3..=s_max + 1e-3).contains(&primeiro),
            "oscilar={oscilar}: passo {primeiro} fora de {s_min}..{s_max}"
        );
    }
}

#[test]
fn without_the_range_the_last_ring_is_squeezed() {
    let r = aneis(&generate(&peca(true)).expect("gera"));
    let d = passos(&r);
    let ultimo = *d.last().unwrap();
    assert!(ultimo < d[0] * 0.8, "hoje o último passo é curto: {d:?}");
}

/// A velocidade de solda muda na razão inversa do passo: com o passo maior, a
/// tocha anda mais devagar e a massa por área fica a mesma.
#[test]
fn speed_compensates_the_step() {
    let mut p = peca(false);
    p.bolacha_osc_eixo = BolachaOscEixo::X;
    p.largura_cordao_min = 7.0;
    p.largura_cordao_max = 9.0;
    let largura = p.diametro / 2.0 - p.diametro_interno / 2.0;
    let r = passo_radial(&p, largura, 0.0);
    let nominal = 8.0 * 0.6;
    assert!((r.fator_velocidade - nominal / r.passo).abs() < 1e-12);
    assert!((r.largura - r.passo / 0.6).abs() < 1e-9);
    let lines = generate(&p).expect("gera");
    // Na Bolacha sem oscilar, o F angular é `taxa ÷ (2π·x) × 360`.
    let (mut x, mut conferidos) = (0.0, 0);
    for l in &lines {
        x = word(l, 'X').unwrap_or(x);
        if l.starts_with("G01 F") && word(l, 'X').is_none() {
            let f = word(l, 'F').unwrap();
            let v = f / 360.0 * std::f64::consts::TAU * x;
            let esperado = p.taxa() * r.fator_velocidade;
            assert!((v - esperado).abs() < 0.2, "{v} contra {esperado}: {l}");
            conferidos += 1;
        }
    }
    assert!(conferidos > 3);
}

#[test]
fn invalid_range_is_a_field_error() {
    let mut p = peca(true);
    p.largura_cordao_min = 9.0;
    p.largura_cordao_max = 7.0;
    let e = validate_physical(&p);
    assert!(e.invalid_fields.contains(&"largura_cordao_min"), "{e:?}");
    p.largura_cordao_min = -1.0;
    p.largura_cordao_max = 9.0;
    assert!(validate_physical(&p)
        .invalid_fields
        .contains(&"largura_cordao_min"));
    p.largura_cordao_min = 7.0;
    assert!(validate_physical(&p).is_valid());
}
