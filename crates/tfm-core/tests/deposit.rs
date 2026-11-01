//! Simulação do material depositado: cada movimento com a tocha ligada deposita
//! `vazão × tempo`, então onde a máquina anda mais devagar ou repassa, a camada
//! sobe mais. A compensação acelera a rotação onde o material acumula.

use tfm_core::deposit::{flow_rate, simulate, DepositModel, DepositReport, MAX_FEED_FACTOR};
use tfm_core::gcode::generate;
use tfm_core::params::{Operation, Params};
use tfm_core::toolpath::build_toolpath;
use tfm_core::validator::gcode_invariants;

fn model(p: &Params) -> DepositModel {
    let lines = generate(p).expect("gera o programa");
    let tp = build_toolpath(&lines, p);
    simulate(&tp, p, flow_rate(&tp, p)).expect("simula o depósito")
}

fn closing() -> Params {
    Params {
        fechamento_uniforme: true,
        ..Params::default()
    }
}

fn compensated() -> Params {
    Params {
        compensar_rotacao: true,
        ..closing()
    }
}

/// Valores de `F` distintos nos `G01` do programa.
fn feeds(lines: &[String]) -> Vec<f64> {
    let mut out: Vec<f64> = lines
        .iter()
        .filter(|l| l.starts_with("G01"))
        .filter_map(|l| l.split_whitespace().nth(1))
        .filter_map(|w| w.strip_prefix('F').and_then(|v| v.parse().ok()))
        .collect();
    out.sort_by(f64::total_cmp);
    out.dedup();
    out
}

#[test]
fn deposited_volume_matches_flow_times_time() {
    let p = closing();
    let lines = generate(&p).expect("gera");
    let tp = build_toolpath(&lines, &p);
    let q = flow_rate(&tp, &p);
    let m = simulate(&tp, &p, q).expect("simula");
    let esperado = q * m.report.weld_min as f64;
    let volume = m.report.volume_mm3 as f64;
    assert!(
        (volume / esperado - 1.0).abs() < 0.01,
        "volume {volume} vs vazão × tempo {esperado}"
    );
}

#[test]
fn plain_spiral_plateau_is_the_requested_layer_thickness() {
    let m = model(&Params::default());
    assert!(
        (m.report.mean_mm - 1.5).abs() < 0.08,
        "média {} deveria ser a espessura da camada",
        m.report.mean_mm
    );
}

#[test]
fn layers_stack_up() {
    let m = model(&Params {
        num_camadas: 3,
        ..Params::default()
    });
    assert!(
        (m.report.mean_mm - 4.5).abs() < 0.25,
        "três camadas de 1,5 mm: média {}",
        m.report.mean_mm
    );
}

#[test]
fn slower_feed_raises_the_layer() {
    let rapido = Params::default();
    let lento = Params {
        velocidade_de_deposicao: rapido.velocidade_de_deposicao / 2.0,
        ..rapido.clone()
    };
    let tp_rapido = build_toolpath(&generate(&rapido).expect("gera"), &rapido);
    let tp_lento = build_toolpath(&generate(&lento).expect("gera"), &lento);
    // Mesma vazão: com metade da velocidade sai o dobro por milímetro.
    let q = flow_rate(&tp_rapido, &rapido);
    let a = simulate(&tp_rapido, &rapido, q).expect("simula").report;
    let b = simulate(&tp_lento, &lento, q).expect("simula").report;
    let razao = b.mean_mm / a.mean_mm;
    assert!((razao - 2.0).abs() < 0.1, "razão de alturas {razao}");
}

#[test]
fn closing_rings_show_where_the_layer_rises_more() {
    let m = model(&closing());
    let r = m.report;
    assert!(
        r.max_mm > r.mean_mm * 1.4,
        "o anel sobre a hélice deveria subir: máx {} vs média {}",
        r.max_mm,
        r.mean_mm
    );
    assert!(r.excess_pct > 0.0 && r.sobremetal_mm > 0.0, "{r:?}");
    // O pico fica nas pontas, não no meio da peça.
    let map = &m.map;
    let (mut best, mut at) = (0.0_f32, 0.0_f32);
    for ia in 0..map.na {
        for ix in 0..map.nx {
            let h = map.height(ix, ia);
            if h > best {
                best = h;
                at = map.x_at(ix);
            }
        }
    }
    assert!(!(14.0..=36.0).contains(&at), "pico em X{at}");
}

#[test]
fn partial_simulation_grows_to_the_final_map() {
    let m = model(&closing());
    assert_eq!(m.point_stamps.len(), m.points + 1);
    assert!(m.point_stamps.windows(2).all(|w| w[0] <= w[1]));
    let mut grid = m.map.empty_like();
    let meio = m.point_stamps[m.points / 2] as usize;
    for s in &m.stamps[..meio] {
        grid.stamp(s);
    }
    let parcial: f32 = grid.heights.iter().sum();
    let total: f32 = m.map.heights.iter().sum();
    assert!(parcial > 0.2 * total && parcial < 0.8 * total, "{parcial}");
    for s in &m.stamps[meio..] {
        grid.stamp(s);
    }
    let fim: f32 = grid.heights.iter().sum();
    assert!((fim / total - 1.0).abs() < 1e-3, "{fim} vs {total}");
}

#[test]
fn every_cylindrical_operation_has_a_map_and_the_others_do_not() {
    for op in [
        Operation::Espiral,
        Operation::EspiralMultiperfil,
        Operation::OscilacaoLinear,
        Operation::OscilacaoQuadrada,
        Operation::OscilacaoQuadradaContinua,
    ] {
        let mut p = Params::default();
        p.apply_operation(op);
        p.comprimento_revestir = 20.0;
        let m = model(&p);
        assert!(m.report.volume_mm3 > 0.0 && m.report.max_mm > 0.0, "{op:?}");
    }
    for op in [Operation::ChapaPlana, Operation::Bolacha] {
        let mut p = Params::default();
        p.apply_operation(op);
        let lines = generate(&p).expect("gera");
        let tp = build_toolpath(&lines, &p);
        assert!(simulate(&tp, &p, 1.0).is_none(), "{op:?}");
    }
}

/// O apagamento da quadrada sem a caixa anda 100 mm em X: o material existe,
/// mas os números medem a peça, não o caminho até X150.
#[test]
fn report_measures_the_part_and_not_the_run_out_past_it() {
    let mut p = Params::default();
    p.apply_operation(Operation::OscilacaoQuadrada);
    let m = model(&p);
    assert!(m.map.x_at(m.map.nx - 1) > 140.0, "o mapa deveria chegar lá");
    assert!(m.report.uncovered_pct < 5.0, "{:?}", m.report);
    assert!((m.report.mean_mm - 1.5).abs() < 0.3, "{:?}", m.report);
}

#[test]
fn compensation_flattens_the_ends_and_saves_material_and_time() {
    let base = model(&closing()).report;
    let comp = model(&compensated()).report;
    assert!(
        comp.sobremetal_mm < 0.7 * base.sobremetal_mm,
        "sobremetal {} vs {}",
        comp.sobremetal_mm,
        base.sobremetal_mm
    );
    assert!(comp.excess_mm3 < base.excess_mm3, "{comp:?} vs {base:?}");
    assert!(comp.volume_mm3 < base.volume_mm3, "{comp:?} vs {base:?}");
    assert!(comp.weld_min < base.weld_min, "{comp:?} vs {base:?}");
}

#[test]
fn compensation_only_speeds_up_and_keeps_the_program_shape() {
    let p = compensated();
    let nominal = feeds(&generate(&closing()).expect("gera"));
    let lines = generate(&p).expect("gera");
    let comp = feeds(&lines);
    assert_eq!(nominal.len(), 1, "{nominal:?}");
    assert!(comp.len() > 3, "a rotação deveria variar: {comp:?}");
    let f0 = nominal[0];
    let (lo, hi) = (comp[0], comp[comp.len() - 1]);
    assert!(lo >= f0 - 0.11, "desacelerou: {lo} < {f0}");
    assert!(hi <= f0 * MAX_FEED_FACTOR + 0.11, "acima do teto: {hi}");
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("(COMPENSACAO DE ROTACAO")),
        "comentário da compensação ausente"
    );
    // Mesmas bordas e o mesmo fechamento em A.
    let m102 = lines
        .iter()
        .position(|l| l.starts_with("M102"))
        .expect("M102");
    let tail = lines[m102..]
        .iter()
        .find(|l| l.starts_with("G01"))
        .expect("apagamento");
    let a: f64 = tail
        .split_whitespace()
        .find_map(|w| w.strip_prefix('A').and_then(|v| v.parse().ok()))
        .expect("A");
    assert!((a - 3960.0).abs() < 0.05, "o anel deveria fechar: {tail}");
    assert_eq!(gcode_invariants(&lines, &p), Vec::<String>::new());
}

fn square(op: Operation, compensar_rotacao: bool) -> Params {
    let mut p = Params::default();
    p.apply_operation(op);
    p.fechamento_uniforme = true;
    p.compensar_rotacao = compensar_rotacao;
    p
}

/// Simula `p` com a vazão do programa sem compensação, como o app faz: é o que
/// mostra o material a mais.
fn with_nominal_flow(p: &Params) -> DepositReport {
    let nominal = Params {
        compensar_rotacao: false,
        ..p.clone()
    };
    let base = build_toolpath(&generate(&nominal).expect("gera"), &nominal);
    let q = flow_rate(&base, &nominal);
    let tp = build_toolpath(&generate(p).expect("gera"), p);
    simulate(&tp, p, q).expect("simula").report
}

#[test]
fn slowing_the_ends_fills_the_square_oscillations() {
    // Na contínua a borda precisaria de 4 vezes para chegar ao platô e para no
    // teto de 2,5, então o sobremetal cai menos que na sequencial.
    for (op, ganho) in [
        (Operation::OscilacaoQuadrada, 0.7),
        (Operation::OscilacaoQuadradaContinua, 0.85),
    ] {
        let base = with_nominal_flow(&square(op, false));
        let comp = with_nominal_flow(&square(op, true));
        assert!(
            comp.min_mm > 1.6 * base.min_mm,
            "{op:?}: a ponta deveria subir: {} vs {}",
            comp.min_mm,
            base.min_mm
        );
        assert!(
            comp.sobremetal_mm < ganho * base.sobremetal_mm,
            "{op:?}: sobremetal {} vs {}",
            comp.sobremetal_mm,
            base.sobremetal_mm
        );
        // O calombo do apagamento agora cai sobre a ponta já cheia, e não sobre
        // a rampa de antes: a margem é para ele, não para o platô.
        assert!(
            comp.max_mm < base.max_mm * 1.12,
            "{op:?}: o meio não deveria subir: {} vs {}",
            comp.max_mm,
            base.max_mm
        );
        assert!(
            comp.weld_min > base.weld_min,
            "{op:?}: {comp:?} vs {base:?}"
        );
        assert!(comp.volume_mm3 > base.volume_mm3, "{op:?}");
    }
}

#[test]
fn slowing_only_lowers_feeds_and_keeps_every_position() {
    for op in [
        Operation::OscilacaoQuadrada,
        Operation::OscilacaoQuadradaContinua,
    ] {
        let nominal = generate(&square(op, false)).expect("gera");
        let p = square(op, true);
        let lines = generate(&p).expect("gera");
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("(COMPENSACAO DE ROTACAO")),
            "{op:?}: comentário ausente"
        );
        let sem_comentario: Vec<&String> = lines
            .iter()
            .filter(|l| !l.starts_with("(COMPENSACAO"))
            .collect();
        assert_eq!(sem_comentario.len(), nominal.len(), "{op:?}: linhas");
        let posicao = |l: &str| {
            l.split_whitespace()
                .filter(|w| !w.starts_with('F'))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let (mut mudou, mut f_max, mut f_min) = (0, 0.0_f64, f64::MAX);
        for (a, b) in sem_comentario.iter().zip(&nominal) {
            if a.contains("(Data:") {
                continue;
            }
            assert_eq!(posicao(a), posicao(b), "{op:?}: posição mudou");
            let feed = |l: &str| -> Option<f64> {
                l.split_whitespace().nth(1)?.strip_prefix('F')?.parse().ok()
            };
            if let (Some(fa), Some(fb)) = (feed(a), feed(b)) {
                assert!(fa <= fb + 1e-9, "{op:?}: acelerou: {a} vs {b}");
                if fa < fb {
                    mudou += 1;
                    f_max = f_max.max(fb / fa);
                }
                f_min = f_min.min(fa / fb);
            }
        }
        assert!(mudou > 0, "{op:?}: nenhum F mudou");
        assert!(f_max <= MAX_FEED_FACTOR + 0.02, "{op:?}: {f_max}");
        assert_eq!(gcode_invariants(&lines, &p), Vec::<String>::new());
    }
}

#[test]
fn compensation_needs_the_edges_and_skips_the_flat_linear() {
    // Sem as bordas não existe a banda inteira para calcular.
    let mut p = Params::default();
    p.apply_operation(Operation::OscilacaoQuadrada);
    p.compensar_rotacao = true;
    assert!(!generate(&p)
        .expect("gera")
        .iter()
        .any(|l| l.starts_with("(COMPENSACAO")));
    // A linear já fica plana: não há o que compensar.
    let p = square(Operation::OscilacaoLinear, true);
    assert!(!generate(&p)
        .expect("gera")
        .iter()
        .any(|l| l.starts_with("(COMPENSACAO")));
}

#[test]
fn compensation_off_does_not_touch_the_program() {
    let lines = generate(&Params::default()).expect("gera");
    assert!(lines.iter().any(|l| l == "G01 F274.9 X50.000 A3740.053"));
    assert!(!lines.iter().any(|l| l.starts_with("(COMPENSACAO")));
}
