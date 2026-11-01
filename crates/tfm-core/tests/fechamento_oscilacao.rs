//! Fechamento uniforme nas oscilações: o comprimento vale de borda a borda do
//! material. Na linear, o primeiro e o último anel encostam nas bordas e o
//! espaçamento é redistribuído. Nas quadradas, a banda inteira (curso da
//! oscilação mais a largura do cordão) cabe no comprimento, sem o passo final
//! em X. O apagamento fica só em A: em X ele andaria 100 mm para fora da peça.
//! Desligada, o G-code é o de sempre (paridade com o Python).

use tfm_core::config::AppConfig;
use tfm_core::gcode::common::overlap_metrics;
use tfm_core::params::{Direction, Operation, Params};
use tfm_core::toolpath::build_toolpath;
use tfm_core::validator::gcode_invariants;

const OPS: [Operation; 3] = [
    Operation::OscilacaoLinear,
    Operation::OscilacaoQuadrada,
    Operation::OscilacaoQuadradaContinua,
];
const SQUARE: [Operation; 2] = [
    Operation::OscilacaoQuadrada,
    Operation::OscilacaoQuadradaContinua,
];

fn base(op: Operation, closing: bool) -> Params {
    let mut p = Params::default();
    p.apply_operation(op);
    p.fechamento_uniforme = closing;
    p
}

fn width(p: &Params) -> f64 {
    overlap_metrics(p, 0, None, None).bead.effective_width_mm
}

fn generate(p: &Params) -> Vec<String> {
    tfm_core::gcode::generate(p).expect("gera o programa")
}

/// Valor do eixo `axis` numa linha de movimento.
fn axis(line: &str, axis: char) -> Option<f64> {
    line.split_whitespace()
        .find_map(|w| w.strip_prefix(axis).and_then(|v| v.parse().ok()))
}

/// Menor e maior X do centro do cordão por onde passa material.
fn deposit_x(p: &Params) -> (f64, f64) {
    let lines = generate(p);
    let tp = build_toolpath(&lines, p);
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    let mut prev = None;
    for pt in tp.points() {
        // Cada movimento com material conta dos dois lados: o início extra
        // parte do ponto de chegada do G00, ainda com a tocha desligada.
        if (pt.torch_on || pt.extinguishing) && !pt.rapid {
            for x in [prev.unwrap_or(pt.pos.x), pt.pos.x] {
                lo = lo.min(x as f64);
                hi = hi.max(x as f64);
            }
        }
        prev = Some(pt.pos.x);
    }
    (lo, hi)
}

/// Primeiro movimento depois do primeiro `M102`: o apagamento.
fn tail(lines: &[String]) -> &String {
    let m102 = lines
        .iter()
        .position(|l| l.starts_with("M102"))
        .expect("M102");
    lines[m102..]
        .iter()
        .find(|l| l.starts_with("G01"))
        .expect("apagamento")
}

#[test]
fn material_stays_between_the_edges_in_every_direction() {
    let cases = [
        (Direction::EsquerdaDireita, false, 0.0, 30.0),
        (Direction::DireitaEsquerda, false, -30.0, 0.0),
        (Direction::EsquerdaDireita, true, -30.0, 0.0),
        (Direction::DireitaEsquerda, true, -30.0, 0.0),
    ];
    for op in OPS {
        for (direcao_soldagem, operacao_interna, lo, hi) in cases {
            let p = Params {
                comprimento_revestir: 30.0,
                direcao_soldagem,
                operacao_interna,
                diametro_interno: if operacao_interna { 80.0 } else { 0.0 },
                diametro: if operacao_interna { 100.0 } else { 50.0 },
                ..base(op, true)
            };
            let w = width(&p);
            let (a, b) = deposit_x(&p);
            assert!(
                (a - w / 2.0 - lo).abs() < 0.01 && (b + w / 2.0 - hi).abs() < 0.01,
                "{op:?} {direcao_soldagem:?} interno={operacao_interna}: \
                 centro de {a} a {b}, largura {w}"
            );
        }
    }
}

#[test]
fn leads_extend_the_material_only_by_what_was_asked() {
    for op in OPS {
        let p = Params {
            lead_extras_enabled: true,
            lead_in: 5.0,
            lead_out: 5.0,
            ..base(op, true)
        };
        let w = width(&p);
        let (a, b) = deposit_x(&p);
        assert!(
            (a - w / 2.0 + 5.0).abs() < 0.01 && (b + w / 2.0 - 55.0).abs() < 0.01,
            "{op:?}: centro de {a} a {b}, largura {w}"
        );
    }
}

#[test]
fn linear_rings_touch_both_edges_and_close_on_the_turn() {
    let p = base(Operation::OscilacaoLinear, true);
    let w = width(&p);
    let (x0, x1) = (w / 2.0, 50.0 - w / 2.0);
    let lines = generate(&p);
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("(FECHAMENTO UNIFORME: 10 voltas, passo 4.664 mm")),
        "comentário do fechamento ausente"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with(&format!("G00 X{x0:.3} A"))),
        "o primeiro anel deveria encostar na borda"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("G01") && l.ends_with(&format!(" X{x1:.3}"))),
        "o último anel deveria encostar na borda"
    );
    let a = axis(tail(&lines), 'A').expect("A no apagamento");
    assert!(
        (a - 3600.0).abs() < 0.01,
        "o último anel deveria fechar em A3600: {}",
        tail(&lines)
    );
    assert_eq!(gcode_invariants(&lines, &p), Vec::<String>::new());
}

#[test]
fn square_bands_fill_the_length_without_the_closing_step() {
    for op in SQUARE {
        let p = base(op, true);
        let w = width(&p);
        let (x0, x1) = (w / 2.0, 50.0 - w / 2.0);
        let lines = generate(&p);
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("(FECHAMENTO UNIFORME: 20 voltas, passo 1.946 mm")),
            "{op:?}: comentário do fechamento ausente"
        );
        let n = lines
            .iter()
            .filter(|l| l.starts_with("(--- PASSO AXIAL"))
            .count();
        assert_eq!(n, 20, "{op:?}");
        assert!(
            !lines.iter().any(|l| l.starts_with("(Fechamento ate fim")),
            "{op:?}: o passo final em X deveria sair"
        );
        for l in lines.iter().filter(|l| l.starts_with("G0")) {
            if let Some(x) = axis(l, 'X') {
                assert!(x >= x0 - 1e-3 && x <= x1 + 1e-3, "{op:?}: X fora: {l}");
            }
        }
        assert!(
            lines
                .iter()
                .any(|l| axis(l, 'X').is_some_and(|x| (x - x1).abs() < 1e-3)),
            "{op:?}: a última banda deveria encostar na borda"
        );
        assert_eq!(gcode_invariants(&lines, &p), Vec::<String>::new());
    }
}

#[test]
fn extinguishing_never_runs_along_x() {
    for op in OPS {
        let lines = generate(&base(op, true));
        let tail = tail(&lines);
        assert!(!tail.contains("F3000"), "{op:?}: apagamento em X: {tail}");
    }
    // Sem a caixa, o comportamento do Python continua igual.
    let lines = generate(&base(Operation::OscilacaoQuadrada, false));
    assert!(tail(&lines).contains("F3000"));
    assert!(lines.iter().any(|l| l.starts_with("(Fechamento ate fim")));
    assert!(!lines.iter().any(|l| l.starts_with("(FECHAMENTO UNIFORME")));
}

#[test]
fn parts_shorter_than_the_band_still_generate_valid_programs() {
    for op in OPS {
        for comprimento_revestir in [6.0, 10.0] {
            let p = Params {
                comprimento_revestir,
                ..base(op, true)
            };
            let lines = generate(&p);
            assert!(
                !lines.iter().any(|l| l.contains("NaN") || l.contains("inf")),
                "{op:?} L={comprimento_revestir}"
            );
            assert_eq!(gcode_invariants(&lines, &p), Vec::<String>::new());
            let w = width(&p).min(comprimento_revestir);
            let (a, b) = deposit_x(&p);
            assert!(
                a >= w / 2.0 - 0.01 && b <= comprimento_revestir - w / 2.0 + 0.01,
                "{op:?} L={comprimento_revestir}: centro de {a} a {b}"
            );
        }
    }
}

#[test]
fn estimates_report_the_real_steps_and_coverage() {
    let cfg = AppConfig::default();
    let e = tfm_core::estimates::compute(&base(Operation::OscilacaoLinear, true), &cfg);
    assert_eq!(e.rotations, "10.00 (passos axiais)");
    assert!(e.pitch.text.starts_with("4.664 (axial"), "{}", e.pitch.text);
    assert_eq!(e.overlap.surface_step, "4.66");
    assert!(
        e.overlap.coverage.starts_with("41.9%"),
        "{}",
        e.overlap.coverage
    );

    for op in SQUARE {
        let e = tfm_core::estimates::compute(&base(op, true), &cfg);
        assert_eq!(e.rotations, "20.00 (passos axiais)", "{op:?}");
        assert!(e.pitch.text.starts_with("1.946 (axial"), "{}", e.pitch.text);
        assert_eq!(e.overlap.surface_step, "1.95", "{op:?}");
        assert!(
            e.overlap.coverage.starts_with("85.1%"),
            "{}",
            e.overlap.coverage
        );
    }

    let e0 = tfm_core::estimates::compute(&base(Operation::OscilacaoQuadrada, false), &cfg);
    assert_eq!(e0.rotations, "25.00 (passos axiais)");
    assert_eq!(e0.pitch.text, "2.000 (axial)");
}
