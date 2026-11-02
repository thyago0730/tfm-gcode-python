//! Apagamento da tocha na Chapa Plana: não há eixo A para girar no lugar, então
//! o cordão para antes da borda o que a tocha ainda deposita ao apagar. O `M102`
//! sai ali, e o apagamento termina exatamente na borda, em vez de passar dela.

use tfm_core::gcode::generate;
use tfm_core::params::{Direction, FlatOrientation, Operation, Params};
use tfm_core::validator::gcode_invariants;

fn plate(orientation: FlatOrientation, direction: Direction, single: bool) -> Params {
    let mut p = Params::default();
    p.apply_operation(Operation::ChapaPlana);
    p.flat_orientation = orientation;
    p.direcao_soldagem = direction;
    p.flat_single_direction = single;
    p
}

/// Valor do eixo `axis` numa linha de movimento.
fn axis(line: &str, axis: char) -> Option<f64> {
    line.split_whitespace()
        .find_map(|w| w.strip_prefix(axis).and_then(|v| v.parse().ok()))
}

/// Índices das linhas `(APAGAMENTO DA TOCHA`.
fn tails(lines: &[String]) -> Vec<usize> {
    (0..lines.len())
        .filter(|&i| lines[i].starts_with("(APAGAMENTO DA TOCHA"))
        .collect()
}

#[test]
fn torch_goes_out_exactly_at_the_edge() {
    let cases = [
        (FlatOrientation::X, Direction::EsquerdaDireita, 'X', 50.0),
        (FlatOrientation::X, Direction::DireitaEsquerda, 'X', -50.0),
        (FlatOrientation::Y, Direction::EsquerdaDireita, 'Y', 50.0),
        (FlatOrientation::Y, Direction::DireitaEsquerda, 'Y', -50.0),
    ];
    for (orientation, direction, weld, edge) in cases {
        let p = plate(orientation, direction, false);
        let lines = generate(&p).expect("gera a chapa");
        let found = tails(&lines);
        assert_eq!(found.len(), 1, "{orientation:?}: uma camada, um apagamento");
        let i = found[0];
        assert!(lines[i - 1].starts_with("M102"), "{}", lines[i - 1]);
        // O zigue-zague pode acabar em qualquer ponta da passada.
        let parada = axis(&lines[i - 2], weld).expect("fim do cordão");
        let fim = axis(&lines[i + 1], weld).expect("fim do apagamento");
        assert!(
            (fim - edge).abs() < 1e-3 || fim.abs() < 1e-3,
            "{orientation:?} {direction:?}: terminou em {fim}: {}",
            lines[i + 1]
        );
        assert!(
            ((parada - fim).abs() - 4.0).abs() < 1e-3,
            "{orientation:?} {direction:?}: parou em {parada}, terminou em {fim}"
        );
        assert_eq!(gcode_invariants(&lines, &p), Vec::<String>::new());
    }
}

#[test]
fn single_direction_goes_out_at_the_edge_on_every_pass() {
    let p = plate(FlatOrientation::X, Direction::EsquerdaDireita, true);
    let lines = generate(&p).expect("gera a chapa");
    let found = tails(&lines);
    assert!(found.len() > 1, "um apagamento por passe");
    for i in found {
        assert_eq!(axis(&lines[i - 2], 'X'), Some(46.0), "{}", lines[i - 2]);
        assert_eq!(axis(&lines[i + 1], 'X'), Some(50.0), "{}", lines[i + 1]);
    }
}

#[test]
fn nothing_runs_past_the_plate_with_the_extra_start_and_end() {
    let mut p = plate(FlatOrientation::X, Direction::EsquerdaDireita, true);
    p.lead_in = 5.0;
    p.lead_out = 5.0;
    let lines = generate(&p).expect("gera a chapa");
    for l in lines.iter().filter(|l| l.starts_with("G01")) {
        if let Some(x) = axis(l, 'X') {
            assert!(x <= 55.0 + 1e-3, "passou do fim extra: {l}");
        }
    }
}

#[test]
fn plate_shorter_than_the_tail_keeps_a_valid_program() {
    let mut p = plate(FlatOrientation::X, Direction::EsquerdaDireita, false);
    p.comprimento_revestir = 3.0;
    let lines = generate(&p).expect("gera a chapa curta");
    assert_eq!(gcode_invariants(&lines, &p), Vec::<String>::new());
    assert!(!lines.iter().any(|l| l.contains("NaN") || l.contains("inf")));
}
