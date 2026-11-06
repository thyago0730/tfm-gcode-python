//! Oscilação na Bolacha: o X (raio) vai e volta em torno da trajetória enquanto
//! a mesa gira.
//!
//! Configuração: ligar, comprimento (pico a pico), forma da onda, sentido do
//! primeiro movimento, período ao longo do arco, defasagem entre camadas, espera
//! nas pontas e pontos por ciclo da senoide. O envelope da oscilação fica dentro
//! do raio da peça, e a velocidade real sobre a peça segue a pedida.

use tfm_core::config::AppConfig;
use tfm_core::deposit::local_process;
use tfm_core::estimates;
use tfm_core::gcode::generate;
use tfm_core::params::{
    BolachaOscForma, BolachaOscSentido, BolachaTipo, Direction, Operation, Params, Rotation,
};
use tfm_core::quality::QualityManager;
use tfm_core::toolpath::build_toolpath;
use tfm_core::validator::{gcode_invariants, validate_physical};

const FORMAS: [BolachaOscForma; 3] = [
    BolachaOscForma::Triangular,
    BolachaOscForma::Senoidal,
    BolachaOscForma::Quadrada,
];

fn bolacha(forma: BolachaOscForma) -> Params {
    let mut p = Params::default();
    p.apply_operation(Operation::Bolacha);
    p.bolacha_oscilar = true;
    p.bolacha_osc_forma = forma;
    p.oscilacao_comprimento = 4.0;
    p.bolacha_osc_periodo_mm = 8.0;
    p.velocidade_oscilacao_mm_min = 0.0;
    p
}

fn word(line: &str, axis: char) -> Option<f64> {
    line.split_whitespace()
        .find_map(|w| w.strip_prefix(axis).and_then(|v| v.parse().ok()))
}

/// Movimentos de solda (`G01`) com X e A, na ordem.
fn welds(lines: &[String]) -> Vec<&String> {
    lines
        .iter()
        .filter(|l| l.starts_with("G01") && !l.contains('Z'))
        .collect()
}

/// Vértices `(x, a relativo)` do anel `v` (base 1) dentro da primeira volta.
fn ring(lines: &[String], v: usize) -> Vec<(f64, f64)> {
    let marker = format!("(--- ANEL {v}/");
    let mut out = Vec::new();
    let (mut inside, mut a_now, mut a0): (bool, f64, Option<f64>) = (false, 0.0, None);
    for l in lines {
        if l.starts_with("(--- ANEL") {
            inside = l.starts_with(&marker);
        }
        if l.starts_with("G00 X") && !l.contains('Z') {
            a_now = word(l, 'A').unwrap_or(a_now);
        }
        if l.starts_with("G01") {
            let a_before = a_now;
            if let Some(a) = word(l, 'A') {
                a_now = a;
            }
            if inside {
                let start = *a0.get_or_insert(a_before);
                let rel = (a_now - start).abs();
                if rel <= 360.0 + 1e-6 {
                    if let Some(x) = word(l, 'X') {
                        out.push((x, rel));
                    }
                }
            }
        }
    }
    out
}

fn aneis(forma: BolachaOscForma) -> Params {
    let mut p = bolacha(forma);
    p.bolacha_tipo = BolachaTipo::Aneis;
    p
}

#[test]
fn off_by_default_keeps_the_disc_program_unchanged() {
    let mut p = Params::default();
    p.apply_operation(Operation::Bolacha);
    assert!(!p.bolacha_oscilar);
    let lines = generate(&p).expect("gera");
    assert!(!lines.iter().any(|l| l.to_uppercase().contains("OSCILA")));
    // Os outros campos da oscilação não vazam para o programa desligado.
    let mut q = p.clone();
    q.bolacha_osc_forma = BolachaOscForma::Quadrada;
    q.bolacha_osc_periodo_mm = 3.0;
    assert_eq!(lines, generate(&q).expect("gera"));
}

#[test]
fn x_stays_inside_the_disc_for_every_configuration() {
    for forma in FORMAS {
        for tipo in [BolachaTipo::EspiralContinua, BolachaTipo::Aneis] {
            for direcao in [Direction::EsquerdaDireita, Direction::DireitaEsquerda] {
                for sentido in [BolachaOscSentido::ParaFora, BolachaOscSentido::ParaDentro] {
                    for rot in [Rotation::Horaria, Rotation::Antihoraria] {
                        for parada in [false, true] {
                            let mut p = bolacha(forma);
                            p.bolacha_tipo = tipo;
                            p.direcao_soldagem = direcao;
                            p.bolacha_osc_sentido = sentido;
                            p.sentido_rotacao = rot;
                            p.bolacha_stop_between = parada;
                            p.num_camadas = 2;
                            let cfg = format!("{forma:?} {tipo:?} {direcao:?} {sentido:?} {rot:?}");
                            let lines = generate(&p).expect("gera");
                            for l in welds(&lines) {
                                if let Some(x) = word(l, 'X') {
                                    assert!(
                                        (4.0 - 1e-3..=25.0 + 1e-3).contains(&x),
                                        "{cfg}: X {x} fora do raio 4 a 25: {l}"
                                    );
                                }
                            }
                            assert_eq!(gcode_invariants(&lines, &p), Vec::<String>::new(), "{cfg}");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn ring_swings_by_the_configured_length() {
    for forma in FORMAS {
        let p = aneis(forma);
        let lines = generate(&p).expect("gera");
        let r = ring(&lines, 2);
        let (min, max) = r.iter().fold((f64::MAX, f64::MIN), |(a, b), (x, _)| {
            (a.min(*x), b.max(*x))
        });
        assert!(
            (max - min - 4.0).abs() < 0.2,
            "{forma:?}: oscilou {:.2} mm em vez de 4",
            max - min
        );
    }
}

fn reversals(xs: &[f64]) -> usize {
    xs.windows(3)
        .filter(|w| (w[1] - w[0]) * (w[2] - w[1]) < 0.0)
        .count()
}

#[test]
fn cycles_per_turn_follow_the_period_along_the_arc() {
    let p = aneis(BolachaOscForma::Triangular);
    let lines = generate(&p).expect("gera");
    let xs: Vec<f64> = ring(&lines, 2).iter().map(|(x, _)| *x).collect();
    let reversos = reversals(&xs);
    let raio = xs.iter().sum::<f64>() / xs.len() as f64;
    let esperado = 2.0 * std::f64::consts::PI * raio / 8.0 * 2.0;
    assert!(
        (reversos as f64 - esperado).abs() <= 2.5,
        "{reversos} inversões contra {esperado:.1} esperadas"
    );
    // Período maior, menos ciclos.
    let mut longo = p.clone();
    longo.bolacha_osc_periodo_mm = 16.0;
    let xs2: Vec<f64> = ring(&generate(&longo).expect("gera"), 2)
        .iter()
        .map(|(x, _)| *x)
        .collect();
    let reversos2 = reversals(&xs2);
    assert!(
        reversos2 * 2 <= reversos + 3,
        "{reversos2} contra {reversos}"
    );
}

#[test]
fn first_move_follows_the_chosen_direction() {
    for forma in [BolachaOscForma::Triangular, BolachaOscForma::Senoidal] {
        for (direcao, sentido) in [
            (Direction::EsquerdaDireita, BolachaOscSentido::ParaFora),
            (Direction::EsquerdaDireita, BolachaOscSentido::ParaDentro),
            (Direction::DireitaEsquerda, BolachaOscSentido::ParaFora),
            (Direction::DireitaEsquerda, BolachaOscSentido::ParaDentro),
        ] {
            let mut p = bolacha(forma);
            p.direcao_soldagem = direcao;
            p.bolacha_osc_sentido = sentido;
            let lines = generate(&p).expect("gera");
            let inicio = lines
                .iter()
                .find(|l| l.starts_with("G00 X") && !l.contains('Z'))
                .and_then(|l| word(l, 'X'))
                .expect("posição inicial");
            let primeiro = welds(&lines)
                .iter()
                .find_map(|l| word(l, 'X'))
                .expect("primeiro movimento");
            match sentido {
                BolachaOscSentido::ParaFora => assert!(primeiro > inicio, "{forma:?} {direcao:?}"),
                BolachaOscSentido::ParaDentro => {
                    assert!(primeiro < inicio, "{forma:?} {direcao:?}")
                }
            }
        }
    }
}

#[test]
fn layer_phase_shifts_where_each_layer_starts() {
    let mut p = bolacha(BolachaOscForma::Triangular);
    p.num_camadas = 3;
    let starts = |p: &Params| -> Vec<f64> {
        generate(p)
            .expect("gera")
            .iter()
            .filter(|l| l.starts_with("G00 X") && !l.contains('Z'))
            .filter_map(|l| word(l, 'X'))
            .collect()
    };
    let sem = starts(&p);
    assert_eq!(sem.len(), 3);
    assert!(sem.iter().all(|x| (x - sem[0]).abs() < 1e-6), "{sem:?}");
    p.bolacha_osc_fase_camada_perc = 25.0;
    let com = starts(&p);
    assert!(
        (com[1] - com[0] - 2.0).abs() < 0.05,
        "um quarto de ciclo leva ao pico (2 mm): {com:?}"
    );
    assert!(
        (com[2] - com[0]).abs() < 0.05,
        "meio ciclo volta ao centro: {com:?}"
    );
}

#[test]
fn only_the_square_wave_has_moves_in_x_alone() {
    for forma in FORMAS {
        let lines = generate(&bolacha(forma)).expect("gera");
        let so_x = welds(&lines)
            .iter()
            .filter(|l| word(l, 'X').is_some() && word(l, 'A').is_none())
            .count();
        if forma == BolachaOscForma::Quadrada {
            assert!(so_x > 10, "a quadrada sobe e desce em X parada: {so_x}");
        } else {
            assert_eq!(so_x, 0, "{forma:?} anda X e A juntos");
        }
    }
}

#[test]
fn dwell_at_the_ends_is_optional() {
    let mut p = bolacha(BolachaOscForma::Triangular);
    let sem = generate(&p).expect("gera");
    assert!(!sem.iter().any(|l| l.contains("Espera na ponta")));
    p.bolacha_osc_espera_s = 0.4;
    let com = generate(&p).expect("gera");
    let esperas: Vec<&String> = com
        .iter()
        .filter(|l| l.contains("Espera na ponta"))
        .collect();
    assert!(
        esperas.len() > 20,
        "uma espera em cada pico: {}",
        esperas.len()
    );
    assert!(esperas[0].starts_with("G04 P0.40"), "{}", esperas[0]);
}

#[test]
fn sine_points_per_cycle_set_the_resolution() {
    let mut p = bolacha(BolachaOscForma::Senoidal);
    p.bolacha_osc_pontos_ciclo = 8.0;
    let poucos = welds(&generate(&p).expect("gera")).len();
    p.bolacha_osc_pontos_ciclo = 24.0;
    let muitos = welds(&generate(&p).expect("gera")).len();
    assert!(muitos > poucos * 2, "{muitos} contra {poucos}");
}

/// O F de cada trecho é escalado pela distância real sobre a peça: a velocidade
/// de solda segue a pedida mesmo com o X indo e voltando.
#[test]
fn surface_speed_stays_on_target_while_weaving() {
    for forma in FORMAS {
        for tipo in [BolachaTipo::EspiralContinua, BolachaTipo::Aneis] {
            let mut p = bolacha(forma);
            p.bolacha_tipo = tipo;
            let lines = generate(&p).expect("gera");
            let l = local_process(&build_toolpath(&lines, &p), &p).expect("há solda");
            let taxa = p.taxa();
            assert!(
                l.speed_min > taxa * 0.9 && l.speed_max < taxa * 1.1,
                "{forma:?} {tipo:?}: velocidade {:.0} a {:.0} contra {taxa:.0}",
                l.speed_min,
                l.speed_max
            );
        }
    }
}

#[test]
fn radial_step_uses_the_swept_width() {
    // Largura do cordão 8 + 4 de oscilação: passo (8 + 4) × (1 − 40%) = 7,2.
    let p = aneis(BolachaOscForma::Triangular);
    let lines = generate(&p).expect("gera");
    let centros: Vec<f64> = lines
        .iter()
        .filter(|l| l.starts_with("(--- ANEL"))
        .filter_map(|l| l.split("X=").nth(1))
        .filter_map(|s| s.trim_end_matches(" ---)").parse().ok())
        .collect();
    assert!(centros.len() >= 3, "{centros:?}");
    assert!(
        (centros[0] - 6.0).abs() < 1e-6,
        "borda interna: {centros:?}"
    );
    assert!((centros[1] - centros[0] - 7.2).abs() < 1e-3, "{centros:?}");
    assert!(
        centros.iter().all(|c| *c <= 23.0 + 1e-6),
        "o centro deixa metade da oscilação até a borda: {centros:?}"
    );
}

#[test]
fn field_errors_name_the_oscillation_fields() {
    let ok = bolacha(BolachaOscForma::Triangular);
    assert!(validate_physical(&ok).is_valid());
    let invalid =
        |p: &Params, key: &'static str| validate_physical(p).invalid_fields.contains(&key);

    let mut p = ok.clone();
    p.oscilacao_comprimento = 0.0;
    assert!(invalid(&p, "oscilacao_comprimento"));

    p = ok.clone();
    p.oscilacao_comprimento = 30.0;
    assert!(invalid(&p, "oscilacao_comprimento"));

    p = ok.clone();
    p.bolacha_osc_periodo_mm = 0.5;
    assert!(invalid(&p, "bolacha_osc_periodo_mm"));

    p = ok.clone();
    p.bolacha_osc_forma = BolachaOscForma::Senoidal;
    p.bolacha_osc_pontos_ciclo = 4.0;
    assert!(invalid(&p, "bolacha_osc_pontos_ciclo"));

    p = ok.clone();
    p.bolacha_osc_espera_s = -1.0;
    assert!(invalid(&p, "bolacha_osc_espera_s"));

    // Desligada, nada disso importa.
    p = ok;
    p.bolacha_oscilar = false;
    p.oscilacao_comprimento = 0.0;
    p.bolacha_osc_periodo_mm = 0.0;
    assert!(validate_physical(&p).is_valid());
}

#[test]
fn estimated_time_follows_the_real_weld_path() {
    for forma in FORMAS {
        for tipo in [BolachaTipo::EspiralContinua, BolachaTipo::Aneis] {
            let mut p = bolacha(forma);
            p.bolacha_tipo = tipo;
            let lines = generate(&p).expect("gera");
            let tp = build_toolpath(&lines, &p);
            // Tempo de solda do percurso: distância sobre a peça pela velocidade.
            let pts: Vec<_> = tp.points().collect();
            let mut min = 0.0f64;
            for w in pts.windows(2) {
                if !w[1].torch_on || w[1].rapid {
                    continue;
                }
                let r = f64::from((w[0].radius + w[1].radius) / 2.0);
                let dr = f64::from(w[1].radius - w[0].radius);
                let da = f64::from(w[1].a_deg - w[0].a_deg).to_radians();
                min += (dr * dr + (r * da).powi(2)).sqrt() / p.taxa();
            }
            let est = estimates::compute(&p, &AppConfig::default()).time_hours * 60.0;
            assert!(
                (est - min).abs() / min < 0.12,
                "{forma:?} {tipo:?}: estimado {est:.2} min contra {min:.2} min do percurso"
            );
        }
    }
}

#[test]
fn too_fast_a_weave_raises_a_process_warning() {
    let mut p = bolacha(BolachaOscForma::Triangular);
    let (avisos, _) = QualityManager::process_warnings(&p);
    assert!(
        !avisos.iter().any(|a| a.contains("oscilação")),
        "{avisos:?}"
    );
    // 600 mm/min são 10 mm/s: num período de 1 mm, 10 ciclos por segundo.
    p.velocidade_de_deposicao = 600.0;
    p.bolacha_osc_periodo_mm = 1.0;
    let (avisos, _) = QualityManager::process_warnings(&p);
    assert!(
        avisos.iter().any(|a| a.contains("Frequência de oscilação")),
        "{avisos:?}"
    );
}

#[test]
fn old_procedures_without_the_new_fields_still_load() {
    let p: Params = serde_json::from_str(r#"{"welding_mode": "bolacha"}"#).expect("carrega");
    assert!(!p.bolacha_oscilar);
    assert_eq!(p.bolacha_osc_forma, BolachaOscForma::Triangular);
    assert_eq!(p.bolacha_osc_sentido, BolachaOscSentido::ParaFora);
    let json = serde_json::to_string(&bolacha(BolachaOscForma::Senoidal)).expect("grava");
    let back: Params = serde_json::from_str(&json).expect("relê");
    assert_eq!(back.bolacha_osc_forma, BolachaOscForma::Senoidal);
    assert!(back.bolacha_oscilar);
}
