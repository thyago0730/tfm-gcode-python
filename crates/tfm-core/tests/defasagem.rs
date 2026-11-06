//! Defasagem dos passes entre camadas da Espiral.
//!
//! Empilhar camadas iguais soma o relevo entre passes de forma coerente: o
//! cume de uma camada cai sobre o cume da anterior, e o relevo cresce com o
//! número de camadas. Numa hélice, girar o ângulo de partida de uma camada
//! desloca a trama de passes em X sem mudar o passo: com `defasar_camadas`
//! cada camada parte em `360° × k/N`, e os cumes caem nos vales. O X do
//! programa não muda, então os anéis das bordas seguem fechando tudo.

use tfm_core::deposit::{flow_rate, simulate, HeightMap};
use tfm_core::gcode::generate;
use tfm_core::params::{Direction, Operation, Params, Rotation};
use tfm_core::toolpath::build_toolpath;
use tfm_core::validator::gcode_invariants;

fn espiral(camadas: i64, defasar: bool) -> Params {
    let mut p = Params::default();
    p.apply_operation(Operation::Espiral);
    p.fechamento_uniforme = true;
    p.compensar_rotacao = false;
    p.num_camadas = camadas;
    p.defasar_camadas = defasar;
    p
}

/// Valor da palavra `axis` numa linha de movimento.
fn word(line: &str, axis: char) -> Option<f64> {
    line.split_whitespace()
        .find_map(|w| w.strip_prefix(axis).and_then(|v| v.parse().ok()))
}

/// Relevo (máximo menos mínimo) das alturas entre `lo` e `hi` mm de X.
fn relevo(p: &Params, lo: f32, hi: f32) -> f32 {
    let nominal = Params {
        defasar_camadas: false,
        ..p.clone()
    };
    let lines_nominal = generate(&nominal).expect("gera o nominal");
    let q = flow_rate(&build_toolpath(&lines_nominal, &nominal), &nominal);
    let lines = generate(p).expect("gera o programa");
    let model = simulate(&build_toolpath(&lines, p), p, q).expect("modelo de depósito");
    interior(&model.map, lo, hi)
}

fn interior(m: &HeightMap, lo: f32, hi: f32) -> f32 {
    let (mut min, mut max) = (f32::MAX, f32::MIN);
    for ia in 0..m.na {
        for ix in 0..m.nx {
            let x = m.x_at(ix);
            if x >= lo && x <= hi {
                let h = m.height(ix, ia);
                min = min.min(h);
                max = max.max(h);
            }
        }
    }
    max - min
}

/// Linhas de movimento com X (sem as de Z).
fn moves_x(l: &[String]) -> Vec<String> {
    l.iter()
        .filter(|s| s.starts_with("G01") || s.starts_with("G00"))
        .filter(|s| !s.contains('Z') && word(s, 'X').is_some())
        .map(|s| {
            s.split_whitespace()
                .filter(|w| !w.starts_with('F'))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

#[test]
fn off_by_default_keeps_the_program_identical() {
    assert!(!Params::default().defasar_camadas);
    let a = generate(&espiral(3, false)).expect("gera");
    let mut b = espiral(3, false);
    b.defasar_camadas = false;
    assert_eq!(a, generate(&b).expect("gera"));
}

#[test]
fn one_layer_is_not_shifted() {
    let normal = generate(&espiral(1, false)).expect("gera");
    let ligado = generate(&espiral(1, true)).expect("gera");
    assert_eq!(normal, ligado, "uma camada não tem com quem defasar");
}

#[test]
fn shifted_layers_keep_the_ripple_from_piling_up() {
    for camadas in [3, 4, 5] {
        let p = espiral(camadas, true);
        let len = p.comprimento_revestir as f32;
        let sem = relevo(&espiral(camadas, false), 0.24 * len, 0.76 * len);
        let com = relevo(&p, 0.24 * len, 0.76 * len);
        assert!(
            com < sem * 0.35,
            "{camadas} camadas: relevo {com:.2} mm com defasagem contra {sem:.2} mm sem"
        );
    }
}

#[test]
fn shifted_layers_stay_flat_as_layers_grow() {
    let len = espiral(3, true).comprimento_revestir as f32;
    let tres = relevo(&espiral(3, true), 0.24 * len, 0.76 * len);
    let cinco = relevo(&espiral(5, true), 0.24 * len, 0.76 * len);
    assert!(
        cinco < tres * 1.6,
        "o relevo não pode crescer com as camadas: {tres:.2} com 3, {cinco:.2} com 5"
    );
}

/// Sem rampa e sem mexer em X, a peça toda melhora, bordas incluídas.
#[test]
fn the_whole_piece_is_never_worse_than_without() {
    for camadas in [2, 3, 5] {
        let p = espiral(camadas, true);
        let len = p.comprimento_revestir as f32;
        let sem = relevo(&espiral(camadas, false), 0.1 * len, 0.9 * len);
        let com = relevo(&p, 0.1 * len, 0.9 * len);
        assert!(
            com <= sem * 1.02,
            "{camadas} camadas: relevo total {com:.2} mm com defasagem contra {sem:.2} mm sem"
        );
    }
}

/// O passo e o X ficam como estavam: só o ângulo de partida muda.
#[test]
fn x_is_identical_and_only_the_start_angle_changes() {
    for direcao in [Direction::EsquerdaDireita, Direction::DireitaEsquerda] {
        for sentido in [Rotation::Horaria, Rotation::Antihoraria] {
            let mut sem = espiral(3, false);
            sem.direcao_soldagem = direcao;
            sem.sentido_rotacao = sentido;
            let mut com = espiral(3, true);
            com.direcao_soldagem = direcao;
            com.sentido_rotacao = sentido;
            let a = generate(&sem).expect("gera");
            let b = generate(&com).expect("gera");
            assert_eq!(gcode_invariants(&b, &com), Vec::<String>::new());
            let xs = |l: &[String]| -> Vec<f64> {
                moves_x(l).iter().filter_map(|s| word(s, 'X')).collect()
            };
            assert_eq!(xs(&a), xs(&b), "{direcao:?} {sentido:?}: X mudou");
        }
    }
}

#[test]
fn each_layer_starts_at_its_own_angle() {
    let p = espiral(4, true);
    let lines = generate(&p).expect("gera");
    let starts: Vec<f64> = lines
        .iter()
        .filter(|l| l.starts_with("G00 X") && !l.contains('Z'))
        .filter_map(|l| word(l, 'A'))
        .collect();
    assert_eq!(starts, vec![0.0, 90.0, 180.0, 270.0]);
    // Anti-horário: os ângulos seguem o sentido da rotação.
    let mut anti = p.clone();
    anti.sentido_rotacao = Rotation::Antihoraria;
    let lines = generate(&anti).expect("gera");
    let starts: Vec<f64> = lines
        .iter()
        .filter(|l| l.starts_with("G00 X") && !l.contains('Z'))
        .filter_map(|l| word(l, 'A'))
        .collect();
    assert_eq!(starts, vec![0.0, -90.0, -180.0, -270.0]);
}

/// Cada camada gira o mesmo tanto que girava: só começa mais adiante.
#[test]
fn each_layer_turns_the_same_total_angle() {
    let total = |p: &Params| -> Vec<f64> {
        let lines = generate(p).expect("gera");
        let mut out = Vec::new();
        let mut inicio = None;
        let mut ultimo = 0.0;
        for l in &lines {
            if l.starts_with("G00 X") && !l.contains('Z') {
                if let Some(i) = inicio.replace(word(l, 'A').unwrap_or(0.0)) {
                    out.push(ultimo - i);
                }
            }
            if l.starts_with("G01") {
                if let Some(a) = word(l, 'A') {
                    ultimo = a;
                }
            }
        }
        if let Some(i) = inicio {
            out.push(ultimo - i);
        }
        out
    };
    assert_eq!(total(&espiral(3, false)), total(&espiral(3, true)));
}

#[test]
fn works_together_with_rotation_compensation() {
    let mut p = espiral(3, true);
    p.compensar_rotacao = true;
    let lines = generate(&p).expect("gera");
    assert_eq!(gcode_invariants(&lines, &p), Vec::<String>::new());
    let sem = Params {
        defasar_camadas: false,
        ..p.clone()
    };
    let xs =
        |l: &[String]| -> Vec<f64> { moves_x(l).iter().filter_map(|s| word(s, 'X')).collect() };
    assert_eq!(xs(&lines), xs(&generate(&sem).expect("gera")));
    let starts: Vec<f64> = lines
        .iter()
        .filter(|l| l.starts_with("G00 X") && !l.contains('Z'))
        .filter_map(|l| word(l, 'A'))
        .collect();
    assert_eq!(starts, vec![0.0, 120.0, 240.0]);
}

#[test]
fn works_on_a_short_piece_too() {
    let mut curta = espiral(3, true);
    curta.comprimento_revestir = 12.0;
    let lines = generate(&curta).expect("gera");
    assert_eq!(gcode_invariants(&lines, &curta), Vec::<String>::new());
}

#[test]
fn announces_the_layer_phase_in_the_program() {
    let lines = generate(&espiral(3, true)).expect("gera");
    let avisos: Vec<&String> = lines
        .iter()
        .filter(|l| l.contains("DEFASAGEM DE PASSES"))
        .collect();
    assert_eq!(avisos.len(), 3, "uma linha por camada: {avisos:?}");
}
