//! Fechamento uniforme da espiral: com a caixa ligada, as voltas viram um
//! número inteiro e o passo é redistribuído no comprimento, de modo que a
//! última passada termine em volta cheia, sem o "ângulo aberto". Desligada,
//! o G-code é o de sempre (paridade com o Python).

use tfm_core::config::AppConfig;
use tfm_core::overlap::uniform_closing;
use tfm_core::params::{Operation, Params};

#[test]
fn uniform_closing_rounds_turns_up_and_shrinks_the_pitch() {
    let (passo, voltas) = uniform_closing(50.0, 4.8);
    assert_eq!(voltas, 11.0);
    assert!((passo - 50.0 / 11.0).abs() < 1e-9, "passo {passo}");
    // Já inteiro: nada muda (tolerância numérica).
    let (passo, voltas) = uniform_closing(48.0, 4.8);
    assert_eq!(voltas, 10.0);
    assert!((passo - 4.8).abs() < 1e-9, "passo {passo}");
    // Passo maior que o comprimento: uma volta inteira.
    assert_eq!(uniform_closing(3.0, 4.8), (3.0, 1.0));
    // Entradas inválidas voltam como vieram, sem NaN nem divisão por zero.
    assert_eq!(uniform_closing(50.0, 0.0), (0.0, 0.0));
    assert_eq!(uniform_closing(0.0, 4.8), (4.8, 0.0));
    assert_eq!(uniform_closing(f64::NAN, 4.8).0, 4.8);
}

#[test]
fn spiral_ends_on_a_whole_turn_when_enabled() {
    let p = Params {
        fechamento_uniforme: true,
        ..Params::default()
    };
    let lines = tfm_core::gcode::generate(&p).expect("gera a espiral");
    let weld = lines
        .iter()
        .find(|l| l.starts_with("G01 F") && l.contains(" X50.000 A"))
        .expect("passada da espiral");
    assert!(
        weld.ends_with(" A3960.000"),
        "esperava 11 voltas exatas (A3960): {weld}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l == "(FECHAMENTO UNIFORME: 11 voltas, passo 4.545 mm)"),
        "comentário do fechamento ausente"
    );

    // Desligada: 10,39 voltas, como o Python, e sem o comentário.
    let lines = tfm_core::gcode::generate(&Params::default()).expect("gera a espiral");
    assert!(
        lines.iter().any(|l| l == "G01 F274.9 X50.000 A3740.053"),
        "paridade com o Python quebrada"
    );
    assert!(!lines.iter().any(|l| l.starts_with("(FECHAMENTO UNIFORME")));
}

#[test]
fn multiperfil_also_closes_on_whole_turns() {
    let mut p = Params::default();
    p.apply_operation(Operation::EspiralMultiperfil);
    p.multiperfil_pitch_mm = 4.8;
    let plain = tfm_core::gcode::generate(&p).expect("gera o multiperfil");
    p.fechamento_uniforme = true;
    let closed = tfm_core::gcode::generate(&p).expect("gera o multiperfil fechado");
    assert_ne!(plain, closed, "a caixa não mudou o multiperfil");
    assert!(
        closed
            .iter()
            .any(|l| l.starts_with("(FECHAMENTO UNIFORME: ") && l.contains(" voltas, passo ")),
        "comentário do fechamento ausente no multiperfil"
    );
    assert!(!plain.iter().any(|l| l.starts_with("(FECHAMENTO UNIFORME")));
}

#[test]
fn estimates_report_whole_turns_when_enabled() {
    let cfg = AppConfig::default();
    let p = Params {
        fechamento_uniforme: true,
        ..Params::default()
    };
    let e = tfm_core::estimates::compute(&p, &cfg);
    assert_eq!(e.rotations, "11.00");
    assert!(
        e.pitch.text.starts_with("4.545 (axial"),
        "passo: {}",
        e.pitch.text
    );
    let e0 = tfm_core::estimates::compute(&Params::default(), &cfg);
    assert_eq!(e0.rotations, "10.39");
}
