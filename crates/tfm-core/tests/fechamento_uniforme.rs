//! Fechamento uniforme: o comprimento a revestir vale de borda a borda do
//! material depositado, não pela linha de centro. Com a caixa ligada, o centro
//! do cordão anda meia largura para dentro de cada lado, as voltas viram
//! inteiras nesse curso e, na espiral, cada ponta fecha num anel de 360°.
//! Desligada, o G-code é o de sempre (paridade com o Python).

use tfm_core::config::AppConfig;
use tfm_core::gcode::common::overlap_metrics;
use tfm_core::overlap::uniform_closing;
use tfm_core::params::{Direction, Operation, Params};
use tfm_core::toolpath::build_toolpath;
use tfm_core::validator::gcode_invariants;

fn closing() -> Params {
    Params {
        fechamento_uniforme: true,
        ..Params::default()
    }
}

fn width(p: &Params) -> f64 {
    overlap_metrics(p, 0, None, None).bead.effective_width_mm
}

/// Valor do eixo `axis` numa linha de movimento.
fn axis(line: &str, axis: char) -> Option<f64> {
    line.split_whitespace()
        .find_map(|w| w.strip_prefix(axis).and_then(|v| v.parse().ok()))
}

/// Menor e maior X do centro do cordão por onde passa material.
fn deposit_x(p: &Params) -> (f64, f64) {
    let lines = tfm_core::gcode::generate(p).expect("gera o programa");
    let tp = build_toolpath(&lines, p);
    let xs = tp
        .points()
        .filter(|pt| (pt.torch_on || pt.extinguishing) && !pt.rapid)
        .map(|pt| pt.pos.x as f64);
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for x in xs {
        lo = lo.min(x);
        hi = hi.max(x);
    }
    (lo, hi)
}

#[test]
fn closing_plan_keeps_the_bead_inside_the_span() {
    let c = uniform_closing(0.0, 50.0, 4.8, 8.0);
    assert_eq!((c.x_start, c.x_end), (4.0, 46.0));
    assert_eq!(c.voltas, 9.0);
    assert!((c.passo - 42.0 / 9.0).abs() < 1e-9, "passo {}", c.passo);
    // Soldando para a esquerda, o recuo é para o outro lado.
    let c = uniform_closing(0.0, -50.0, 4.8, 8.0);
    assert_eq!((c.x_start, c.x_end), (-4.0, -46.0));
    // Curso já múltiplo do passo: nada muda.
    let c = uniform_closing(0.0, 50.0, 4.2, 8.0);
    assert_eq!(c.voltas, 10.0);
    assert!((c.passo - 4.2).abs() < 1e-9, "passo {}", c.passo);
    // Peça mais estreita que o cordão: fica no meio, sem hélice.
    let c = uniform_closing(0.0, 6.0, 4.8, 8.0);
    assert_eq!((c.x_start, c.x_end, c.voltas), (3.0, 3.0, 0.0));
    // Entradas inválidas não espalham NaN nem dividem por zero.
    let c = uniform_closing(0.0, 50.0, 0.0, 8.0);
    assert_eq!((c.x_start, c.x_end, c.voltas), (4.0, 46.0, 0.0));
    let c = uniform_closing(0.0, 50.0, 4.8, f64::NAN);
    assert_eq!((c.x_start, c.x_end), (0.0, 50.0));
}

#[test]
fn spiral_material_runs_from_edge_to_edge_and_closes_in_rings() {
    let p = closing();
    let w = width(&p);
    let (x0, x1) = (w / 2.0, 50.0 - w / 2.0);
    let lines = tfm_core::gcode::generate(&p).expect("gera a espiral");
    assert!(
        lines.iter().any(|l| l == &format!("G00 X{x0:.3} A0")),
        "a passada deveria começar meia largura para dentro"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("G01 F") && l.ends_with(" A360.000")),
        "anel inicial ausente"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.ends_with(&format!(" X{x1:.3} A3600.000"))),
        "esperava 9 voltas inteiras entre os anéis"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("(FECHAMENTO UNIFORME: 9 voltas, passo 4.664 mm")),
        "comentário do fechamento ausente"
    );
    // O cordão termina onde o anel fecha: o M102 sai antes, para o apagamento
    // chegar exatamente a 360°, e nenhum movimento passa da borda em X.
    let m102 = lines
        .iter()
        .position(|l| l.starts_with("M102"))
        .expect("M102");
    let tail = lines[m102..]
        .iter()
        .find(|l| l.starts_with("G01"))
        .expect("apagamento");
    let a = axis(tail, 'A').expect("A no apagamento");
    assert!(
        (a - 3960.0).abs() < 0.01,
        "o anel deveria fechar em A3960: {tail}"
    );
    for l in lines.iter().filter(|l| l.starts_with("G01")) {
        if let Some(x) = axis(l, 'X') {
            assert!(x >= x0 - 1e-3 && x <= x1 + 1e-3, "X fora do curso: {l}");
        }
    }
    assert_eq!(gcode_invariants(&lines, &p), Vec::<String>::new());
}

#[test]
fn deposit_edges_match_the_length_in_every_direction() {
    let cases = [
        (Direction::EsquerdaDireita, false, 0.0, 50.0),
        (Direction::DireitaEsquerda, false, -50.0, 0.0),
        (Direction::EsquerdaDireita, true, -50.0, 0.0),
        (Direction::DireitaEsquerda, true, -50.0, 0.0),
    ];
    for (direcao_soldagem, operacao_interna, lo, hi) in cases {
        let p = Params {
            direcao_soldagem,
            operacao_interna,
            diametro_interno: if operacao_interna { 80.0 } else { 0.0 },
            diametro: if operacao_interna { 100.0 } else { 50.0 },
            ..closing()
        };
        let w = width(&p);
        let (a, b) = deposit_x(&p);
        assert!(
            (a - w / 2.0 - lo).abs() < 0.01 && (b + w / 2.0 - hi).abs() < 0.01,
            "{direcao_soldagem:?} interno={operacao_interna}: centro de {a} a {b}, largura {w}"
        );
    }
}

#[test]
fn every_layer_gets_its_own_plan_and_box_off_keeps_python_parity() {
    let p = Params {
        num_camadas: 3,
        ..closing()
    };
    let lines = tfm_core::gcode::generate(&p).expect("gera 3 camadas");
    let n = lines
        .iter()
        .filter(|l| l.starts_with("(FECHAMENTO UNIFORME"))
        .count();
    assert_eq!(n, 3);

    let lines = tfm_core::gcode::generate(&Params::default()).expect("gera a espiral");
    assert!(lines.iter().any(|l| l == "G00 X0.000 A0"));
    assert!(lines.iter().any(|l| l == "G01 F274.9 X50.000 A3740.053"));
    assert!(!lines.iter().any(|l| l.starts_with("(FECHAMENTO UNIFORME")));
}

#[test]
fn part_narrower_than_the_bead_still_generates_a_valid_program() {
    let p = Params {
        comprimento_revestir: 6.0,
        ..closing()
    };
    let lines = tfm_core::gcode::generate(&p).expect("gera a peça estreita");
    assert!(
        lines.iter().any(|l| l == "G00 X3.000 A0"),
        "deveria centrar o cordão"
    );
    assert!(!lines.iter().any(|l| l.contains("NaN") || l.contains("inf")));
    assert_eq!(gcode_invariants(&lines, &p), Vec::<String>::new());
}

#[test]
fn multiperfil_stays_inside_the_length_without_extra_rings() {
    let mut p = Params::default();
    p.apply_operation(Operation::EspiralMultiperfil);
    p.multiperfil_pitch_mm = 4.8;
    p.fechamento_uniforme = true;
    let w = width(&p);
    let lines = tfm_core::gcode::generate(&p).expect("gera o multiperfil");
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("(FECHAMENTO UNIFORME: ")),
        "comentário do fechamento ausente no multiperfil"
    );
    for l in lines.iter().filter(|l| l.starts_with("G01")) {
        if let Some(x) = axis(l, 'X') {
            assert!(
                x >= w / 2.0 - 1e-3 && x <= 50.0 - w / 2.0 + 1e-3,
                "X fora: {l}"
            );
        }
    }
    // Um anel por subpasse empilharia material nas pontas: não há G01 só em A
    // fora do apagamento.
    let rings = lines
        .windows(2)
        .filter(|w| {
            w[1].starts_with("G01")
                && axis(&w[1], 'X').is_none()
                && !w[0].starts_with("(APAGAMENTO")
        })
        .count();
    assert_eq!(rings, 0, "o multiperfil não deveria ganhar anéis");
}

#[test]
fn estimates_report_the_real_pitch_and_coverage() {
    let cfg = AppConfig::default();
    let e = tfm_core::estimates::compute(&closing(), &cfg);
    assert_eq!(e.rotations, "9.00");
    assert_eq!(e.total_angle, "3960.00");
    assert!(
        e.pitch.text.starts_with("4.664 (axial"),
        "passo: {}",
        e.pitch.text
    );
    assert_eq!(e.overlap.surface_step, "4.66");
    assert!(
        e.overlap.coverage.starts_with("41.9%"),
        "{}",
        e.overlap.coverage
    );

    let e0 = tfm_core::estimates::compute(&Params::default(), &cfg);
    assert_eq!(e0.rotations, "10.39");
    assert_eq!(e0.overlap.surface_step, "4.81");
}
