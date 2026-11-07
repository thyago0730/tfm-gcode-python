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
    BolachaOscEixo, BolachaOscForma, BolachaOscSentido, BolachaTipo, Direction, Operation, Params,
    Rotation,
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

// ---------------------------------------------------------------------------
// Oscilação em Y: a mesa gira e a tocha vai e volta no eixo Y.
// ---------------------------------------------------------------------------

fn bolacha_y(forma: BolachaOscForma) -> Params {
    let mut p = bolacha(forma);
    p.bolacha_osc_eixo = BolachaOscEixo::Y;
    p
}

fn aneis_y(forma: BolachaOscForma) -> Params {
    let mut p = bolacha_y(forma);
    p.bolacha_tipo = BolachaTipo::Aneis;
    p
}

/// Um movimento de solda como o Mach3 lê: deslocamento em X e Y (mm), em A
/// (graus) e o F. `x` é o raio médio do trecho.
struct Mov {
    dx: f64,
    dy: f64,
    da: f64,
    f: f64,
    x: f64,
    linha: String,
}

fn movimentos(lines: &[String]) -> Vec<Mov> {
    let (mut x, mut y, mut a) = (0.0, 0.0, 0.0);
    let mut out = Vec::new();
    for l in lines {
        let is_move = l.starts_with("G00") || l.starts_with("G01");
        if !is_move || l.contains('Z') {
            continue;
        }
        let (nx, ny, na) = (
            word(l, 'X').unwrap_or(x),
            word(l, 'Y').unwrap_or(y),
            word(l, 'A').unwrap_or(a),
        );
        if l.starts_with("G01") {
            if let Some(f) = word(l, 'F') {
                out.push(Mov {
                    dx: nx - x,
                    dy: ny - y,
                    da: na - a,
                    f,
                    x: (nx + x) / 2.0,
                    linha: l.clone(),
                });
            }
        }
        (x, y, a) = (nx, ny, na);
    }
    out
}

/// `(y, a relativo)` dos vértices do anel `v` dentro da primeira volta, com Y
/// modal como o controlador.
fn ring_y(lines: &[String], v: usize) -> Vec<(f64, f64)> {
    let marker = format!("(--- ANEL {v}/");
    let (mut inside, mut y_now, mut a_now, mut a0) = (false, 0.0, 0.0, None);
    let mut out = Vec::new();
    for l in lines {
        if l.starts_with("(--- ANEL") {
            inside = l.starts_with(&marker);
        }
        if (l.starts_with("G00") || l.starts_with("G01")) && !l.contains('Z') {
            let a_before = a_now;
            y_now = word(l, 'Y').unwrap_or(y_now);
            a_now = word(l, 'A').unwrap_or(a_now);
            if inside && l.starts_with("G01") {
                let start = *a0.get_or_insert(a_before);
                let rel = (a_now - start).abs();
                if rel <= 360.0 + 1e-6 {
                    out.push((y_now, rel));
                }
            }
        }
    }
    out
}

#[test]
fn y_axis_swings_around_y_zero_by_the_configured_length() {
    for forma in FORMAS {
        let lines = generate(&aneis_y(forma)).expect("gera");
        let ys: Vec<f64> = ring_y(&lines, 2).iter().map(|(y, _)| *y).collect();
        let (min, max) = ys
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), y| (a.min(*y), b.max(*y)));
        assert!(
            (max - min - 4.0).abs() < 0.2,
            "{forma:?}: Y oscilou {:.2} mm em vez de 4",
            max - min
        );
        assert!(
            (max + min).abs() < 0.3,
            "{forma:?}: a onda fica centrada em Y0: de {min:.2} a {max:.2}"
        );
    }
}

/// Com a oscilação em Y o X segue a trajetória da bolacha sem oscilar: o
/// passo radial é o mesmo e nada se afasta do raio da peça.
#[test]
fn y_axis_leaves_the_radial_path_alone() {
    for forma in FORMAS {
        let mut sem = aneis_y(forma);
        sem.bolacha_oscilar = false;
        let plano = generate(&sem).expect("gera");
        let osc = generate(&aneis_y(forma)).expect("gera");
        let centros = |l: &[String]| -> Vec<String> {
            l.iter()
                .filter(|s| s.starts_with("(--- ANEL"))
                .cloned()
                .collect()
        };
        assert_eq!(centros(&plano), centros(&osc), "{forma:?}: anéis iguais");
        for m in movimentos(&osc) {
            if let Some(x) = word(&m.linha, 'X') {
                assert!((4.0 - 1e-3..=25.0 + 1e-3).contains(&x), "{}", m.linha);
            }
        }
        let l = ring_y(&osc, 2);
        assert!(l.len() > 10);
    }
}

#[test]
fn y_returns_to_zero_when_each_layer_ends() {
    let mut p = bolacha_y(BolachaOscForma::Triangular);
    p.num_camadas = 3;
    let lines = generate(&p).expect("gera");
    let mut ultimo = None;
    let mut fins = Vec::new();
    for l in &lines {
        if l.starts_with("(--- CAMADA") {
            if let Some(y) = ultimo.take() {
                fins.push(y);
            }
        }
        if (l.starts_with("G00") || l.starts_with("G01")) && !l.contains('Z') {
            if let Some(y) = word(l, 'Y') {
                ultimo = Some(y);
            }
        }
    }
    fins.extend(ultimo);
    assert_eq!(fins.len(), 3, "{fins:?}");
    assert!(fins.iter().all(|y| y.abs() < 1e-6), "{fins:?}");
}

/// A mesa segue a velocidade pedida: o F de cada linha faz o arco que a mesa
/// percorre por minuto ser o da solda, e o Y corre junto no mesmo tempo.
#[test]
fn table_keeps_its_speed_while_y_moves() {
    for forma in FORMAS {
        for tipo in [BolachaTipo::EspiralContinua, BolachaTipo::Aneis] {
            let mut p = bolacha_y(forma);
            p.bolacha_tipo = tipo;
            let lines = generate(&p).expect("gera");
            let taxa = p.taxa();
            let mut contadas = 0;
            for m in movimentos(&lines) {
                if m.da.abs() < 0.2 {
                    continue;
                }
                let lido = (m.dx * m.dx + m.dy * m.dy + m.da * m.da).sqrt();
                let tempo = lido / m.f;
                let arco = m.x * m.da.abs().to_radians();
                let v = arco / tempo;
                assert!(
                    (v - taxa).abs() / taxa < 0.03,
                    "{forma:?} {tipo:?}: mesa a {v:.1} mm/min contra {taxa}: {}",
                    m.linha
                );
                contadas += 1;
            }
            assert!(contadas > 20);
        }
    }
}

#[test]
fn y_only_moves_use_the_oscillation_feed() {
    let mut p = bolacha_y(BolachaOscForma::Quadrada);
    p.velocidade_oscilacao_mm_min = 300.0;
    let lines = generate(&p).expect("gera");
    let so_y: Vec<Mov> = movimentos(&lines)
        .into_iter()
        .filter(|m| m.dy.abs() > 1e-6 && m.da.abs() < 1e-9 && m.dx.abs() < 1e-9)
        .collect();
    assert!(so_y.len() > 10, "a quadrada sobe e desce em Y parada");
    assert!(so_y.iter().all(|m| (m.f - 300.0).abs() < 1e-6));
}

#[test]
fn first_y_move_follows_the_chosen_direction() {
    for forma in [BolachaOscForma::Triangular, BolachaOscForma::Senoidal] {
        for (sentido, sinal) in [
            (BolachaOscSentido::ParaFora, 1.0),
            (BolachaOscSentido::ParaDentro, -1.0),
        ] {
            let mut p = bolacha_y(forma);
            p.bolacha_osc_sentido = sentido;
            let lines = generate(&p).expect("gera");
            let primeiro = movimentos(&lines)
                .into_iter()
                .find(|m| m.dy.abs() > 1e-6)
                .expect("primeiro movimento em Y");
            assert!(primeiro.dy * sinal > 0.0, "{forma:?} {sentido:?}");
        }
    }
}

/// O ponto na peça é o `(X, Y)` da tocha girado pelo ângulo da mesa: o raio
/// na peça passa a ser `hypot(X, Y)`.
#[test]
fn toolpath_places_the_torch_offset_on_the_disc() {
    let p = aneis_y(BolachaOscForma::Triangular);
    let lines = generate(&p).expect("gera");
    let tp = build_toolpath(&lines, &p);
    let maior = tp
        .points()
        .filter(|q| q.torch_on)
        .map(|q| (q.pos.x.hypot(q.pos.y) - q.radius).abs())
        .fold(0.0f32, f32::max);
    assert!(
        maior > 0.3,
        "o Y afasta o ponto do raio da máquina: {maior}"
    );
    for q in tp.points().filter(|q| q.torch_on) {
        let (_, y) = tfm_core::toolpath::machine_xy(q);
        assert!(
            y.abs() <= 2.0 + 0.05,
            "Y da máquina {y} passou de meia onda"
        );
    }
}

/// Com a tocha correndo junto com a mesa a velocidade relativa à peça cai, e
/// no outro sentido sobe: é o que o aporte térmico local precisa enxergar.
#[test]
fn local_speed_reflects_the_torch_moving_with_the_table() {
    let mut p = bolacha_y(BolachaOscForma::Triangular);
    p.bolacha_tipo = BolachaTipo::Aneis;
    p.oscilacao_comprimento = 4.0;
    p.bolacha_osc_periodo_mm = 8.0;
    let lines = generate(&p).expect("gera");
    let l = local_process(&build_toolpath(&lines, &p), &p).expect("há solda");
    let taxa = p.taxa();
    assert!(l.speed_max > taxa * 1.5, "{l:?} contra {taxa}");
    assert!(l.speed_min < taxa * 0.5, "{l:?} contra {taxa}");
}

#[test]
fn y_amplitude_is_not_limited_by_the_radius_and_stalls_are_warned() {
    let mut p = bolacha_y(BolachaOscForma::Triangular);
    p.oscilacao_comprimento = 30.0;
    assert!(
        validate_physical(&p).is_valid(),
        "em Y não há limite radial"
    );
    p.oscilacao_comprimento = 0.0;
    assert!(validate_physical(&p)
        .invalid_fields
        .contains(&"oscilacao_comprimento"));

    // 2 × comprimento ÷ período = 1: a tocha chega a parar em relação à peça.
    let mut q = bolacha_y(BolachaOscForma::Triangular);
    q.oscilacao_comprimento = 4.0;
    q.bolacha_osc_periodo_mm = 8.0;
    let (avisos, _) = QualityManager::process_warnings(&q);
    assert!(
        avisos.iter().any(|a| a.contains("parar ou recuar")),
        "{avisos:?}"
    );
    q.oscilacao_comprimento = 1.0;
    q.bolacha_osc_periodo_mm = 20.0;
    let (avisos, _) = QualityManager::process_warnings(&q);
    assert!(
        !avisos.iter().any(|a| a.contains("parar ou recuar")),
        "{avisos:?}"
    );
}

#[test]
fn estimated_time_follows_the_machine_time_with_y() {
    for forma in FORMAS {
        for tipo in [BolachaTipo::EspiralContinua, BolachaTipo::Aneis] {
            let mut p = bolacha_y(forma);
            p.bolacha_tipo = tipo;
            let lines = generate(&p).expect("gera");
            let min: f64 = movimentos(&lines)
                .iter()
                .map(|m| (m.dx * m.dx + m.dy * m.dy + m.da * m.da).sqrt() / m.f)
                .sum();
            let est = estimates::compute(&p, &AppConfig::default()).time_hours * 60.0;
            assert!(
                (est - min).abs() / min < 0.12,
                "{forma:?} {tipo:?}: estimado {est:.2} min contra {min:.2} min da máquina"
            );
        }
    }
}

#[test]
fn the_axis_defaults_to_x_and_old_procedures_load() {
    let p: Params = serde_json::from_str(r#"{"welding_mode": "bolacha"}"#).expect("carrega");
    assert_eq!(p.bolacha_osc_eixo, BolachaOscEixo::X);
    let json = serde_json::to_string(&bolacha_y(BolachaOscForma::Senoidal)).expect("grava");
    let back: Params = serde_json::from_str(&json).expect("relê");
    assert_eq!(back.bolacha_osc_eixo, BolachaOscEixo::Y);
}

/// A rotação da placa na Bolacha vai até o limite dos parâmetros (o da máquina
/// na tela), não mais até 20 rpm fixos. Padrão 20, como o Python.
#[test]
fn bolacha_rpm_cap_follows_the_param_limit() {
    use tfm_core::toolpath::plate_rpm;
    let max_rpm = |p: &Params| {
        let t = build_toolpath(&tfm_core::gcode::generate(p).expect("gera"), p);
        let pts: Vec<_> = t.points().cloned().collect();
        pts.windows(2)
            .filter_map(|w| plate_rpm(&w[0], &w[1], p.welding_mode))
            .fold(0.0_f32, f32::max)
    };
    // Sem oscilação: na oscilação as linhas da onda levam F em mm/min ao longo
    // do caminho e não passam pelo teto.
    let oscilar = false;
    {
        let mut p = Params::default();
        p.apply_operation(Operation::Bolacha);
        p.velocidade_de_deposicao = 3000.0;
        p.taxa_de_deposicao = Some(3000.0);
        assert_eq!(p.limite_rpm_placa, 20.0);
        let a = max_rpm(&p);
        assert!(a <= 20.0 * 1.02, "oscilar {oscilar}: {a} rpm com teto 20");
        p.limite_rpm_placa = 100.0;
        let b = max_rpm(&p);
        assert!(
            b > 25.0 && b <= 100.0 * 1.02,
            "oscilar {oscilar}: {b} rpm com teto 100"
        );
    }
}
