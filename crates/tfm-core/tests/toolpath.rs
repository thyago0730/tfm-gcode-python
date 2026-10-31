//! Testes de integração de `tfm_core::toolpath`.
//!
//! Os valores de referência foram obtidos rodando o `_parse_gcode_points`
//! original (`src/app/TFM_GCODE.py`) e as regexes de `src/app/ui/charts.py`
//! sobre as fixtures golden de `tests/golden/` (saída do Python, LF):
//! número de trechos separados por `NaN`, pontos por trecho, raio por camada,
//! caixa envolvente e comprimento das séries. Posições são `f32`, então as
//! comparações usam tolerância 1e-3 mm/graus, exceto valores exatamente
//! representáveis (25.0, 26.5, 12.0, ...), que são comparados com `==`.

use std::fs;

use serde_json::Value;
use tfm_core::params::{Params, WeldingMode};
use tfm_core::toolpath::{
    build_toolpath, count_motions, oscillation_series, step_distances, temporal_series, PathPoint,
    Series, Toolpath, Vec3,
};

const GOLDEN_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/golden");

/// Carrega as linhas do G-code golden e os `Params` do caso homônimo.
fn golden(name: &str) -> (Vec<String>, Params) {
    let cases_path = format!("{GOLDEN_DIR}/cases.json");
    let cases: Vec<Value> = serde_json::from_str(
        &fs::read_to_string(&cases_path).unwrap_or_else(|e| panic!("{cases_path}: {e}")),
    )
    .expect("cases.json inválido");
    let case = cases
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("caso golden {name:?} não existe em cases.json"));
    let params: Params =
        serde_json::from_value(case["params"].clone()).expect("params do caso golden");
    let gcode_path = format!("{GOLDEN_DIR}/{name}.gcode");
    let lines = fs::read_to_string(&gcode_path)
        .unwrap_or_else(|e| panic!("{gcode_path}: {e}"))
        .lines()
        .map(|l| l.trim_end_matches('\r').to_string())
        .collect();
    (lines, params)
}

fn lines(src: &[&str]) -> Vec<String> {
    src.iter().map(|s| s.to_string()).collect()
}

fn assert_close(actual: f32, expected: f32, tol: f32, what: &str) {
    assert!(
        (actual - expected).abs() <= tol,
        "{what}: esperado {expected}, obtido {actual} (tol {tol})"
    );
}

fn assert_close64(actual: f64, expected: f64, tol: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tol,
        "{what}: esperado {expected}, obtido {actual} (tol {tol})"
    );
}

fn all_points(tp: &Toolpath) -> Vec<&PathPoint> {
    tp.segments.iter().flatten().collect()
}

fn segment_sizes(tp: &Toolpath) -> Vec<usize> {
    tp.segments.iter().map(Vec::len).collect()
}

fn mean_radius(points: &[&PathPoint]) -> f32 {
    let sum: f64 = points.iter().map(|p| f64::from(p.radius)).sum();
    (sum / points.len() as f64) as f32
}

fn max64(values: &[f64]) -> f64 {
    values.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}

fn min64(values: &[f64]) -> f64 {
    values.iter().copied().fold(f64::INFINITY, f64::min)
}

// ---------------------------------------------------------------------------
// build_toolpath — fixtures golden
// ---------------------------------------------------------------------------

#[test]
fn spiral_toolpath_has_layers_and_radius() {
    let (lines, params) = golden("espiral_3camadas_de_ccw");
    assert_eq!(params.welding_mode, WeldingMode::Espiral);
    let tp = build_toolpath(&lines, &params);

    assert_eq!(tp.layer_count, 3);
    // Python: 13 trechos separados por NaN. Cada G0 e cada mudança de raio
    // quebra; o G01 de -3740.053° vira 747 pontos interpolados + 1, mais o
    // G00 Z12 que abre o trecho e o G01 de apagamento (9.164° < 2 passos).
    assert_eq!(
        segment_sizes(&tp),
        vec![1, 1, 1, 750, 1, 1, 1, 750, 1, 1, 1, 750, 1]
    );
    let points = all_points(&tp);
    assert_eq!(points.len(), 2260);
    assert_eq!(tp.point_count(), 2260);
    assert!(!tp.is_empty());

    // Raio médio com tocha ligada: 25.0 -> 26.5 -> 28.0 (Ø50, Z = 12 + 1.5·n).
    let mut previous_mean: Option<f32> = None;
    for layer in 1..=3usize {
        let on: Vec<&PathPoint> = points
            .iter()
            .copied()
            .filter(|p| p.layer == layer && p.torch_on)
            .collect();
        assert_eq!(on.len(), 748, "camada {layer}: pontos com tocha ligada");
        let expected = 25.0 + 1.5 * (layer as f32 - 1.0);
        let mean = mean_radius(&on);
        assert_close(mean, expected, 1e-4, &format!("camada {layer}: raio médio"));
        assert!(
            on.iter().all(|p| (p.radius - expected).abs() <= 1e-4),
            "camada {layer}: raio constante em {expected}"
        );
        assert!(
            on.iter().all(|p| !p.rapid),
            "camada {layer}: solda só em G1"
        );
        if let Some(prev) = previous_mean {
            assert_close(mean - prev, 1.5, 1e-4, "crescimento do raio entre camadas");
        }
        previous_mean = Some(mean);
    }

    // Primeiro ponto: `G90 G0 Y0` antes da primeira camada; Z = 0 dá r = 25 - 12.
    let first = points[0];
    assert_eq!(first.line_idx, 21);
    assert!(
        lines[21].starts_with("G90 G0 Y0"),
        "linha 21: {:?}",
        lines[21]
    );
    assert_eq!(first.layer, 0);
    assert!(first.rapid && !first.torch_on);
    assert_eq!(first.radius, 13.0);
    assert_eq!(first.feed, 0.0);
    assert_eq!(first.a_deg, 0.0);
    assert_close(first.pos.x, 0.0, 1e-6, "x inicial");
    assert_close(first.pos.y, 0.0, 1e-6, "y inicial (r·cos 90°)");
    assert_close(first.pos.z, 13.0, 1e-6, "z inicial (r·sin 90°)");

    // Trecho do primeiro cordão: G00 Z12 abre, 747 interpolados a 5°, G01 final,
    // G01 de apagamento já com a tocha desligada.
    let weld = &tp.segments[3];
    assert_eq!(weld[0].radius, 25.0);
    assert!(weld[0].rapid && !weld[0].torch_on);
    assert_eq!(weld[0].layer, 1);
    assert_close(
        weld[1].a_deg,
        -3740.053 / 748.0,
        1e-3,
        "1º ponto interpolado",
    );
    assert!(weld[1].torch_on && !weld[1].rapid);
    assert_eq!(
        weld[1].line_idx, weld[748].line_idx,
        "interpolados apontam para o G01"
    );
    assert_close(weld[748].a_deg, -3740.053, 1e-3, "A do fim do G01");
    assert_close(weld[748].pos.x, -50.0, 1e-4, "X do fim do G01");
    let last = weld.last().expect("trecho não vazio");
    assert_close(last.a_deg, -3749.217, 1e-3, "A do apagamento");
    assert_close(last.feed, 274.9, 1e-3, "F do cordão 1");
    assert!(!last.torch_on, "apagamento ocorre após M102");

    // Caixa envolvente do Python: x[-50.122, 0] y[-27.9998, 28.916] z[-50.985, 58].
    assert_close(tp.min.x, -50.122, 1e-3, "min.x");
    assert_close(tp.max.x, 0.0, 1e-3, "max.x");
    assert_close(tp.min.y, -27.9998, 1e-3, "min.y");
    assert_close(tp.max.y, 28.916, 1e-3, "max.y");
    assert_close(tp.min.z, -50.985, 1e-3, "min.z");
    assert_close(tp.max.z, 58.0, 1e-3, "max.z (G00 Z45 -> r = 25 + 33)");
}

#[test]
fn flat_plate_is_planar() {
    let (lines, params) = golden("chapa_x");
    assert_eq!(params.welding_mode, WeldingMode::ChapaPlana);
    let tp = build_toolpath(&lines, &params);

    // O Python não insere quebras na chapa plana: um só trecho com os 47 movimentos.
    assert_eq!(segment_sizes(&tp), vec![47]);
    assert_eq!(tp.layer_count, 1);
    let points = all_points(&tp);
    assert!(
        points.iter().all(|p| p.radius == 0.0 && p.a_deg == 0.0),
        "chapa plana não tem raio nem eixo A"
    );
    let on: Vec<&PathPoint> = points.iter().copied().filter(|p| p.torch_on).collect();
    assert_eq!(on.len(), 41);
    assert!(
        on.iter()
            .all(|p| p.pos.z == 12.0 && !p.rapid && p.layer == 1),
        "cordões todos no plano Z = 12"
    );
    assert_eq!(points.iter().filter(|p| p.rapid).count(), 5);
    assert_eq!(
        tp.min,
        Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0
        }
    );
    assert_eq!(
        tp.max,
        Vec3 {
            x: 54.0,
            y: 96.0,
            z: 42.0
        }
    );

    // Mapeamento direto (x, y, z): `G01 F120.0 Y4.800` com X = 50 vira (50, 4.8, 12).
    let idx = lines
        .iter()
        .position(|l| l == "G01 F120.0 Y4.800")
        .expect("linha Y4.800");
    let p = points
        .iter()
        .find(|p| p.line_idx == idx)
        .expect("ponto da linha Y4.800");
    assert_eq!(p.pos.x, 50.0);
    assert_close(p.pos.y, 4.8, 1e-6, "y direto");
    assert_eq!(p.pos.z, 12.0);
    assert_eq!(p.feed, 120.0);
    assert!(p.torch_on && !p.rapid);
}

#[test]
fn bolacha_maps_x_to_radius() {
    let (lines, params) = golden("bolacha_espiral");
    assert_eq!(params.welding_mode, WeldingMode::Bolacha);
    let tp = build_toolpath(&lines, &params);

    // Python: quebra só em G0 -> [G90 G0 Y0] [G00 Z42] [G00 X4 A0] [G00 Z12 + cordão] [G00 Z42].
    assert_eq!(segment_sizes(&tp), vec![1, 1, 1, 724, 1]);
    assert_eq!(tp.layer_count, 1);
    let points = all_points(&tp);
    assert_eq!(lines[points[0].line_idx], "G90 G0 Y0 (Ir ao Y=0 peca)");

    // raio = X da máquina; o ponto 3D fica na face: hypot(y, z) == |raio|.
    for p in &points {
        let what = format!("raio da linha {}", p.line_idx);
        assert_close(p.pos.y.hypot(p.pos.z), p.radius.abs(), 1e-3, &what);
    }
    // eixo = Z da máquina: cordão a Z = 12, afastamento de segurança a Z = 42.
    let on: Vec<&PathPoint> = points.iter().copied().filter(|p| p.torch_on).collect();
    assert_eq!(on.len(), 722);
    assert!(on.iter().all(|p| p.pos.x == 12.0 && !p.rapid));
    assert_eq!(tp.min.x, 0.0);
    assert_eq!(tp.max.x, 42.0);

    // `G00 X4.000 A0.000` a 42 mm: raio 4, ângulo 0 -> (42, 4·cos 90°, 4·sin 90°).
    let approach = &tp.segments[2][0];
    assert_eq!(approach.radius, 4.0);
    assert_eq!(approach.a_deg, 0.0);
    assert!(approach.rapid);
    assert_eq!(approach.pos.x, 42.0);
    assert_close(approach.pos.y, 0.0, 1e-5, "y da aproximação");
    assert_close(approach.pos.z, 4.0, 1e-5, "z da aproximação");

    // Espiral: raio nunca diminui; cada G01 de 10° gera 1 interpolado (t = 0.5).
    let weld = &tp.segments[3];
    assert!(
        weld.windows(2).all(|w| w[1].radius >= w[0].radius),
        "raio monotônico"
    );
    assert_eq!(weld[0].radius, 4.0);
    assert!(weld[0].rapid && !weld[0].torch_on);
    assert_close(weld[1].radius, 4.0665, 1e-4, "raio interpolado");
    assert_close(weld[1].a_deg, 5.0, 1e-4, "A interpolado");
    assert!(weld[1].torch_on && !weld[1].rapid);
    assert_close(weld[1].feed, 1663.4, 1e-2, "F do primeiro G01");
    assert_close(weld[2].radius, 4.133, 1e-4, "raio do fim do 1º G01");
    assert_close(weld[2].a_deg, 10.0, 1e-4, "A do fim do 1º G01");
    let last = weld.last().expect("trecho não vazio");
    assert_eq!(last.radius, 50.0);
    assert_close(last.a_deg, 3614.584, 1e-3, "A final");
    assert_eq!(last.pos.x, 12.0);
    assert!(!last.torch_on);
    let max_radius = points.iter().map(|p| p.radius).fold(f32::MIN, f32::max);
    assert_eq!(max_radius, 50.0);
}

#[test]
fn internal_spiral_golden_inverts_radius() {
    // Ø100 externo, Ø60 interno, operação interna: base = 30, r = 30 - (|Z| - 12).
    let (lines, params) = golden("espiral_interno");
    assert!(params.is_internal());
    let tp = build_toolpath(&lines, &params);
    assert_eq!(tp.layer_count, 2);
    assert_eq!(
        segment_sizes(&tp),
        vec![1, 1, 1, 1, 751, 1, 1, 1, 1, 750, 1, 1, 1]
    );
    let points = all_points(&tp);
    // Z = 0 -> r = 30 + 12 = 42: o sinal do afastamento inverte na face interna.
    assert_eq!(points[0].radius, 42.0);
    for (layer, expected, n) in [(1usize, 30.0f32, 749usize), (2, 28.5, 748)] {
        let on: Vec<&PathPoint> = points
            .iter()
            .copied()
            .filter(|p| p.layer == layer && p.torch_on)
            .collect();
        assert_eq!(on.len(), n, "camada {layer}: pontos com tocha ligada");
        assert!(
            on.iter().all(|p| p.radius == expected),
            "camada {layer}: raio {expected}"
        );
    }
}

// ---------------------------------------------------------------------------
// Séries para gráficos — fixtures golden
// ---------------------------------------------------------------------------

#[test]
fn temporal_series_lengths_match() {
    let (lines, _) = golden("osc_quadrada");
    let s = temporal_series(&lines);
    let n = count_motions(&lines);
    assert_eq!(n, 1606);
    assert_eq!(s.len(), n);
    assert_eq!(s.x.len(), n);
    assert_eq!(s.z.len(), n);
    assert_eq!(s.a.len(), n);
    assert!(!s.is_empty());
    assert_eq!(oscillation_series(&lines), s.x, "oscilação é a série X");
    assert_eq!(s.x[0], 0.0);
    assert_eq!(min64(&s.x), 0.0);
    assert_eq!(max64(&s.x), 110.0);
    assert_eq!(s.x[n - 1], 0.0);
    assert_eq!(s.z[n - 1], 0.0);
    assert_eq!(s.a[n - 1], 1440.0);

    // A abordagem por regex conta também as linhas `G53 G0 ...` (22 = 19 + 3).
    let (lines, _) = golden("espiral_3camadas_de_ccw");
    let s = temporal_series(&lines);
    assert_eq!(count_motions(&lines), 22);
    assert_eq!(s.len(), 22);
    assert_eq!(s.a[21], -3751.529);
    assert_eq!(min64(&s.x), -50.122);
    assert_eq!(max64(&s.x), 0.0);
    assert_eq!(s.z[21], 0.0, "G53 G0 Z0 Y0 final zera o Z lido");
}

#[test]
fn step_distances_non_negative() {
    let (lines, _) = golden("osc_quadrada");
    let d = step_distances(&lines);
    // Os 3 primeiros movimentos vêm antes do primeiro X: 1606 - 3 = 1603.
    assert_eq!(d.len(), 1603);
    assert_eq!(d[0], 0.0);
    assert!(d.iter().all(|v| *v >= 0.0 && v.is_finite()));
    assert_eq!(max64(&d), 110.0);
    assert_close64(d.iter().sum(), 4084.0, 1e-6, "percurso XZ total");

    let (lines, _) = golden("chapa_x");
    let d = step_distances(&lines);
    assert_eq!(d.len(), 47);
    assert_eq!(d[0], 0.0);
    assert!(d.iter().all(|v| *v >= 0.0));
    assert_eq!(max64(&d), 54.0);
    assert_close64(d.iter().sum(), 1210.0, 1e-6, "percurso XZ total da chapa");
}

// ---------------------------------------------------------------------------
// Séries — semântica das regexes do Python
// ---------------------------------------------------------------------------

#[test]
fn series_mirror_python_regex_reading() {
    let src = lines(&[
        "G21 G90 G94",            // G90/G94 não são movimento
        "G53 G0 Z0 Y0 (maquina)", // G0 conta, mesmo com G53 (a regex não filtra)
        "G1 Z5 (X20)",            // comentário inline NÃO é removido: X20 vale
        "(G1 X3)",                // até comentário puro casa com \bG1\b
        "G04 P3.00",              // dwell: não é movimento
        "G1 X.5 A-3.",            // X.5 -> 0.5; A-3. -> -3 (ponto final não entra)
        "XG1 X7",                 // 'XG1' não tem fronteira antes do G
        "G01 X",                  // X sem número: mantém o anterior
    ]);
    assert_eq!(count_motions(&src), 5);
    assert_eq!(oscillation_series(&src), vec![0.0, 20.0, 3.0, 0.5, 7.0]);
    let s = temporal_series(&src);
    assert_eq!(s.x, vec![0.0, 20.0, 3.0, 0.5, 7.0]);
    assert_eq!(s.z, vec![0.0, 5.0, 5.0, 5.0, 5.0]);
    assert_eq!(s.a, vec![0.0, 0.0, 0.0, -3.0, -3.0]);
    assert_eq!(step_distances(&src), vec![0.0, 17.0, 2.5, 6.5]);
}

#[test]
fn arcs_count_as_motion_in_series_but_not_in_toolpath() {
    let src = lines(&[
        "G2 X1 Z12",
        "G02 X2",
        "G3 X3",
        "G03 X4",
        "G10 X5",
        "G0 X6",
        "G1 X7",
    ]);
    assert_eq!(count_motions(&src), 6);
    assert_eq!(oscillation_series(&src), vec![1.0, 2.0, 3.0, 4.0, 6.0, 7.0]);
    assert_eq!(step_distances(&src), vec![0.0, 1.0, 1.0, 1.0, 2.0, 1.0]);
    // O simulador só aceita G0/G00/G1/G01, como o Python.
    let tp = build_toolpath(&src, &Params::default());
    assert_eq!(segment_sizes(&tp), vec![2]);
    let xs: Vec<f32> = all_points(&tp).iter().map(|p| p.pos.x).collect();
    assert_eq!(xs, vec![6.0, 7.0]);
}

#[test]
fn step_distances_wait_for_both_axes() {
    let src = lines(&["G0 X10", "G0 Z5", "G1 X13 Z9", "G1 X13 Z9"]);
    // O 1º movimento ainda não viu Z: é descartado; o 2º vira 0.0; depois hypot.
    assert_eq!(step_distances(&src), vec![0.0, 5.0, 0.0]);
    assert_eq!(count_motions(&src), 4);
    let s = temporal_series(&src);
    assert_eq!(s.x, vec![10.0, 10.0, 13.0, 13.0]);
    assert_eq!(s.z, vec![0.0, 5.0, 9.0, 9.0]);
    assert_eq!(s.a, vec![0.0; 4]);
    assert_eq!(oscillation_series(&src), s.x);
}

// ---------------------------------------------------------------------------
// build_toolpath — semântica linha a linha do _parse_gcode_points
// ---------------------------------------------------------------------------

#[test]
fn cylindrical_breaks_on_rapid_and_radius_change() {
    let p = Params::default(); // Ø50, afastamento 12 -> r = 25 em Z12
    let src = lines(&[
        "G0 X0 Z12 A0",    // r 25
        "G1 X10 A3",       // r 25: mesmo trecho
        "G1 Z12.0005",     // |Δr| = 0.0005 <= 0.001: sem quebra
        "G1 X10 Z13.5 A3", // r 26.5: quebra mesmo em G1, sem interpolação
        "G1 X20 A4",       // continua no trecho novo
        "G0 X30",          // G0 quebra sempre
        "G1 X40",
    ]);
    let tp = build_toolpath(&src, &p);
    assert_eq!(segment_sizes(&tp), vec![3, 2, 2]);
    let points = all_points(&tp);
    let expected = [25.0, 25.0, 25.0005, 26.5, 26.5, 26.5, 26.5];
    for (p, r) in points.iter().zip(expected) {
        assert_close(p.radius, r, 1e-5, &format!("raio da linha {}", p.line_idx));
    }
    assert!(!tp.segments[1][0].rapid, "quebra por raio não é G0");
    assert!(tp.segments[2][0].rapid);
    assert_eq!(tp.layer_count, 1, "sem marcador de camada, mas com pontos");
    assert!(points.iter().all(|p| p.layer == 0));
    assert_eq!(
        points.iter().map(|p| p.line_idx).collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4, 5, 6]
    );
}

#[test]
fn cylindrical_interpolates_every_five_degrees() {
    let p = Params::default();
    let src = lines(&["G0 Z12 A0", "G1 X9 A17", "G1 A22", "G1 X0 A32"]);
    let tp = build_toolpath(&src, &p);
    // |Δa| = 17 -> int(17/5) = 3 passos -> 2 intermediários; 5 -> nada; 10 -> 1.
    assert_eq!(segment_sizes(&tp), vec![7]);
    let seg = &tp.segments[0];
    let a: Vec<f32> = seg.iter().map(|p| p.a_deg).collect();
    let x: Vec<f32> = seg.iter().map(|p| p.pos.x).collect();
    let expected_a = [0.0, 17.0 / 3.0, 34.0 / 3.0, 17.0, 22.0, 27.0, 32.0];
    let expected_x = [0.0, 3.0, 6.0, 9.0, 9.0, 4.5, 0.0];
    for i in 0..7 {
        assert_close(a[i], expected_a[i], 1e-4, &format!("A do ponto {i}"));
        assert_close(x[i], expected_x[i], 1e-4, &format!("X do ponto {i}"));
        assert_eq!(seg[i].radius, 25.0, "raio do ponto {i}");
    }
    assert!(seg[0].rapid && seg[1..].iter().all(|p| !p.rapid));
    assert!(
        seg[1..4].iter().all(|p| p.line_idx == 1),
        "interpolados herdam a linha do G1"
    );
    assert_eq!(seg[5].line_idx, 3);
    // (x, r·cos(a + 90°), r·sin(a + 90°)) em a = 17°: y = -25·sin 17°, z = 25·cos 17°.
    assert_close(seg[3].pos.y, -7.3093, 1e-3, "y em 17°");
    assert_close(seg[3].pos.z, 23.9076, 1e-3, "z em 17°");
}

#[test]
fn torch_state_and_ignored_lines_follow_python() {
    let p = Params::default();
    let src = lines(&[
        "%",
        "(cabecalho X99 A99)", // comentário: ignorado por completo
        "F300",                // só atualiza F
        "X7",                  // sem G: atualiza o estado, não gera ponto
        "G1 Z12 A1 (X55 A55)", // comentário inline removido
        "M101 (liga)",
        "G01 X8 Xabc",  // 'Xabc' não é número: ignorado
        "M3 X99",       // M*: ignorado
        "G53 G0 X0 Z0", // G53: ignorado
        "M102",
        "G00 F500 X9",
        "G1 X9.5",
    ]);
    let tp = build_toolpath(&src, &p);
    assert_eq!(segment_sizes(&tp), vec![2, 2]);
    let points = all_points(&tp);
    let got: Vec<(f32, bool, f32, bool, usize)> = points
        .iter()
        .map(|p| (p.pos.x, p.torch_on, p.feed, p.rapid, p.line_idx))
        .collect();
    assert_eq!(
        got,
        vec![
            (7.0, false, 300.0, false, 4),
            (8.0, true, 300.0, false, 6),
            (9.0, false, 500.0, true, 10),
            (9.5, false, 500.0, false, 11),
        ]
    );
    assert!(points.iter().all(|p| p.a_deg == 1.0 && p.radius == 25.0));
}

#[test]
fn internal_operation_radius_follows_python() {
    let mut p = Params {
        diametro: 100.0,
        diametro_interno: 60.0,
        operacao_interna: true,
        ..Params::default() // afastamento 12
    };
    let src = lines(&["G0 X0 Z0 A0", "G0 Z12", "G0 Z13.5"]);
    let radii = |p: &Params| -> Vec<f32> {
        all_points(&build_toolpath(&src, p))
            .iter()
            .map(|q| q.radius)
            .collect()
    };
    // base = min(100, 60) / 2 = 30; r = 30 - (|Z| - 12)
    assert_eq!(radii(&p), vec![42.0, 30.0, 28.5]);
    // diametro_interno == 0: cai no diâmetro externo, ainda com o sinal invertido
    p.diametro_interno = 0.0;
    assert_eq!(radii(&p), vec![62.0, 50.0, 48.5]);
    // interno maior que o externo: min() mantém o externo
    p.diametro_interno = 150.0;
    assert_eq!(radii(&p), vec![62.0, 50.0, 48.5]);
    // operação externa ignora o diâmetro interno
    p.diametro_interno = 60.0;
    p.operacao_interna = false;
    assert_eq!(radii(&p), vec![38.0, 50.0, 51.5]);
}

#[test]
fn bolacha_breaks_only_on_rapid_and_interpolates() {
    let p = Params {
        welding_mode: WeldingMode::Bolacha,
        ..Params::default()
    };
    let src = lines(&[
        "G0 X4 Z12 A0",
        "G1 X10 Z13.5 A12", // Z muda, mas bolacha não quebra por isso; 12° -> 1 interpolado
        "G0 X20",           // G0 quebra
        "G1 A17",           // 5°: sem interpolação
    ]);
    let tp = build_toolpath(&src, &p);
    assert_eq!(segment_sizes(&tp), vec![3, 2]);
    let seg = &tp.segments[0];
    // interpolado em t = 0.5: raio 7, eixo 12.75, A 6
    assert_eq!(seg[1].radius, 7.0);
    assert_eq!(seg[1].pos.x, 12.75);
    assert_eq!(seg[1].a_deg, 6.0);
    assert_eq!(seg[1].line_idx, 1);
    assert_eq!(seg[2].radius, 10.0);
    assert_eq!(seg[2].pos.x, 13.5);
    // (eixo, r·cos(a + 90°), r·sin(a + 90°)) em a = 12°, r = 10
    assert_close(seg[2].pos.y, -2.0791, 1e-3, "y em 12°");
    assert_close(seg[2].pos.z, 9.7815, 1e-3, "z em 12°");
    assert_eq!(tp.segments[1][1].a_deg, 17.0);
    assert_eq!(tp.segments[1][1].radius, 20.0);
}

#[test]
fn layer_index_counts_camada_markers() {
    let p = Params::default();
    let src = lines(&[
        "G0 X0 Z12",
        "(--- CAMADA 1 ---)",
        "G1 X1",
        "(--- CAMADA 2 ---)",
        "G1 X2",
        "(--- CAMADA 3 ---)",
    ]);
    let tp = build_toolpath(&src, &p);
    let layers: Vec<usize> = all_points(&tp).iter().map(|p| p.layer).collect();
    assert_eq!(layers, vec![0, 1, 2]);
    assert_eq!(
        tp.layer_count, 3,
        "marcador depois do último ponto também conta"
    );
}

#[test]
fn empty_input_yields_empty_results() {
    let p = Params::default();
    let tp = build_toolpath(&[], &p);
    assert!(tp.segments.is_empty());
    assert!(tp.is_empty());
    assert_eq!(tp.point_count(), 0);
    assert_eq!(tp.points().count(), 0);
    assert_eq!(tp.min, Vec3::default());
    assert_eq!(tp.max, Vec3::default());
    assert_eq!(tp.layer_count, 0);
    assert_eq!(tp, Toolpath::default());

    // Só cabeçalho: nenhum ponto, mas o marcador de camada conta.
    let src = lines(&["%", "(--- CAMADA 1 ---)", "G21 G90", "M30"]);
    let tp = build_toolpath(&src, &p);
    assert!(tp.segments.is_empty());
    assert_eq!(tp.layer_count, 1);
    assert_eq!(tp.min, Vec3::default());

    assert_eq!(temporal_series(&[]), Series::default());
    assert!(temporal_series(&[]).is_empty());
    assert!(oscillation_series(&[]).is_empty());
    assert!(step_distances(&[]).is_empty());
    assert_eq!(count_motions(&[]), 0);
}

#[test]
fn vec3_is_a_plain_copy_value() {
    let v = Vec3::new(1.0, -2.0, 3.5);
    let w = v; // Copy
    assert_eq!(v, w);
    assert_eq!(
        Vec3::default(),
        Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0
        }
    );
    assert_eq!(v.min(Vec3::new(0.0, 0.0, 9.0)), Vec3::new(0.0, -2.0, 3.5));
    assert_eq!(v.max(Vec3::new(0.0, 0.0, 9.0)), Vec3::new(1.0, 0.0, 9.0));
}

#[test]
fn toolpath_points_iterates_all_segments_in_order() {
    let p = Params::default();
    let src = lines(&["G0 X0 Z12", "G1 X1", "G0 X2", "G1 X3"]);
    let tp = build_toolpath(&src, &p);
    assert_eq!(segment_sizes(&tp), vec![2, 2]);
    let xs: Vec<f32> = tp.points().map(|p| p.pos.x).collect();
    assert_eq!(xs, vec![0.0, 1.0, 2.0, 3.0]);
    assert_eq!(tp.point_count(), 4);
    assert_eq!(tp.min.x, 0.0);
    assert_close(tp.min.y, 0.0, 1e-6, "min.y");
    assert_eq!(tp.min.z, 25.0);
    assert_close(tp.max.x, 3.0, 0.0, "max.x");
    assert_close(tp.max.y, 0.0, 1e-6, "max.y");
    assert_eq!(tp.max.z, 25.0);
}

// ---------------------------------------------------------------------------
// Paridade com o Python em todas as fixtures golden
// ---------------------------------------------------------------------------

/// Resumo de cada caso golden obtido do `_parse_gcode_points` original:
/// (nome, pontos, trechos, pontos com tocha ligada, camadas, raio máximo, |A| máximo).
const PYTHON_SUMMARY: &[(&str, usize, usize, usize, usize, f32, f32)] = &[
    ("espiral_padrao", 754, 5, 748, 1, 55.0000, 3749.217),
    (
        "espiral_3camadas_de_ccw",
        2260,
        13,
        2244,
        3,
        58.0000,
        3751.529,
    ),
    ("espiral_fechado_leads", 1047, 5, 1041, 1, 55.0000, 5217.232),
    ("espiral_interno", 1512, 13, 1497, 2, 42.0000, 3752.649),
    ("espiral_sem_atrasos", 753, 5, 748, 1, 55.0000, 3740.053),
    ("espiral_rastreab", 754, 5, 748, 1, 55.0000, 3749.217),
    (
        "multiperfil_retangular",
        2434,
        26,
        2400,
        1,
        55.0000,
        13371.543,
    ),
    (
        "multiperfil_trapezoidal_chanfro",
        6649,
        76,
        6549,
        3,
        58.0000,
        13349.043,
    ),
    (
        "multiperfil_triangular_setor",
        1826,
        20,
        1800,
        1,
        55.0000,
        10041.543,
    ),
    (
        "multiperfil_ponto_a_ponto",
        4867,
        51,
        4800,
        2,
        56.5000,
        13371.543,
    ),
    (
        "multiperfil_continuo_fechado",
        3585,
        26,
        3551,
        1,
        55.0000,
        19131.569,
    ),
    (
        "multiperfil_interno_ccw",
        2444,
        36,
        2400,
        1,
        52.0000,
        13223.639,
    ),
    ("bolacha_espiral", 728, 5, 722, 1, 50.0000, 3614.584),
    ("bolacha_aneis", 747, 6, 741, 1, 50.0000, 3704.584),
    ("bolacha_aneis_parada", 789, 33, 741, 1, 50.0000, 3704.584),
    (
        "bolacha_de_2camadas_smart",
        1185,
        9,
        1156,
        2,
        40.0000,
        2962.296,
    ),
    ("chapa_x", 47, 1, 41, 1, 0.0000, 0.000),
    ("chapa_y_de", 47, 1, 41, 1, 0.0000, 0.000),
    ("chapa_unidirecional_leads", 219, 1, 84, 2, 0.0000, 0.000),
    ("osc_linear", 808, 5, 802, 1, 55.0000, 3969.167),
    ("osc_linear_interno", 812, 8, 803, 1, 42.0000, 3960.000),
    (
        "osc_linear_de_ccw_leads",
        1619,
        9,
        1608,
        2,
        56.5000,
        3975.000,
    ),
    ("osc_quadrada", 1603, 17, 1597, 1, 55.0000, 1440.000),
    ("osc_quadrada_interno", 1910, 20, 1901, 1, 42.0000, 1440.000),
    (
        "osc_quadrada_de_leads",
        1604,
        17,
        1598,
        1,
        55.0000,
        1440.000,
    ),
    ("osc_quad_continua", 15067, 17, 15061, 1, 55.0000, 1440.000),
    (
        "osc_quad_continua_de_leadout",
        15069,
        17,
        15063,
        1,
        55.0000,
        1440.000,
    ),
    (
        "osc_quad_continua_2camadas_gran",
        15541,
        33,
        15530,
        2,
        56.5000,
        1440.857,
    ),
    ("legado_osc_linear", 808, 5, 802, 1, 55.0000, 3969.167),
    ("legado_osc_quadrada", 9583, 77, 9577, 1, 55.0000, 8640.000),
];

#[test]
fn all_golden_cases_match_python_summary() {
    let cases_path = format!("{GOLDEN_DIR}/cases.json");
    let cases: Vec<Value> =
        serde_json::from_str(&fs::read_to_string(&cases_path).expect("cases.json")).expect("JSON");
    assert_eq!(
        cases.len(),
        PYTHON_SUMMARY.len(),
        "cases.json mudou: regenere a tabela com o Python original"
    );
    for (name, points, segments, torch_on, layers, max_r, max_a) in PYTHON_SUMMARY {
        let (lines, params) = golden(name);
        let tp = build_toolpath(&lines, &params);
        let what = format!("caso {name}");
        assert_eq!(tp.point_count(), *points, "{what}: pontos");
        assert_eq!(tp.segments.len(), *segments, "{what}: trechos");
        assert!(
            tp.segments.iter().all(|s| !s.is_empty()),
            "{what}: trecho vazio"
        );
        assert_eq!(
            tp.points().filter(|p| p.torch_on).count(),
            *torch_on,
            "{what}: pontos com tocha ligada"
        );
        assert_eq!(tp.layer_count, *layers, "{what}: camadas");
        let got_r = tp.points().map(|p| p.radius).fold(f32::MIN, f32::max);
        assert_close(got_r, *max_r, 1e-3, &format!("{what}: raio máximo"));
        let got_a = tp.points().map(|p| p.a_deg.abs()).fold(0.0, f32::max);
        assert_close(got_a, *max_a, 1e-2, &format!("{what}: |A| máximo"));

        // Invariantes de todo ponto: nasce de uma linha G0/G00/G1/G01 sem G53 e
        // está dentro da caixa envolvente.
        for p in tp.points() {
            let src = &lines[p.line_idx];
            assert!(!src.contains("G53"), "{what}: ponto vindo de linha G53");
            let has_motion = src
                .split('(')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .any(|w| matches!(w, "G0" | "G00" | "G1" | "G01"));
            assert!(
                has_motion,
                "{what}: linha {} não é movimento: {src:?}",
                p.line_idx
            );
            assert!(
                p.pos.x >= tp.min.x
                    && p.pos.y >= tp.min.y
                    && p.pos.z >= tp.min.z
                    && p.pos.x <= tp.max.x
                    && p.pos.y <= tp.max.y
                    && p.pos.z <= tp.max.z,
                "{what}: ponto fora da caixa envolvente"
            );
        }
    }
}
