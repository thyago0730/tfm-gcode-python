//! Harness golden: compara a saída de `tfm_core::gcode::generate_with_timestamp`
//! com os `.gcode` gerados pelo Python original em `tests/golden/` (raiz do repo).
//!
//! Cada `golden_*` cobre um grupo de casos (prefixo do nome em `cases.json`).
//! Os grupos cujo modo ainda não foi portado falham com `GenError::Unsupported`
//! até a tarefa que porta o módulo correspondente; não há `#[ignore]`.
//!
//! O timestamp é fixado em `01/01/2000 00:00:00`, igual à 3ª linha das fixtures.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tfm_core::gcode::generate_with_timestamp;
use tfm_core::params::Params;

const GOLDEN_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/golden");
const TIMESTAMP: &str = "01/01/2000 00:00:00";

/// Prefixos dos oito grupos; juntos devem cobrir cada caso exatamente uma vez.
const GROUPS: [&str; 8] = [
    "espiral_",
    "multiperfil_",
    "bolacha_",
    "chapa_",
    "osc_linear",
    "osc_quadrada",
    "osc_quad_continua",
    "legado_",
];

#[derive(Deserialize)]
struct Case {
    name: String,
    params: serde_json::Value,
}

fn golden_path(file: &str) -> PathBuf {
    Path::new(GOLDEN_DIR).join(file)
}

fn read_golden(file: &str) -> String {
    let path = golden_path(file);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("não foi possível ler {}: {e}", path.display()))
}

fn load_cases() -> Vec<Case> {
    serde_json::from_str(&read_golden("cases.json"))
        .unwrap_or_else(|e| panic!("cases.json inválido: {e}"))
}

/// Linhas esperadas; um `\r` final por linha é descartado (defesa contra
/// checkouts CRLF). O arquivo termina em `%` sem newline final.
fn expected_lines(name: &str) -> Vec<String> {
    read_golden(&format!("{name}.gcode"))
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
        .collect()
}

/// Gera o caso e devolve a descrição do primeiro desvio, se houver.
fn check_case(case: &Case) -> Result<(), String> {
    let p: Params = serde_json::from_value(case.params.clone())
        .map_err(|e| format!("{}: parâmetros inválidos: {e}", case.name))?;
    let got = generate_with_timestamp(&p, TIMESTAMP)
        .map_err(|e| format!("{}: gerador falhou: {e}", case.name))?;
    let expected = expected_lines(&case.name);

    for (i, (e, o)) in expected.iter().zip(got.iter()).enumerate() {
        if e != o {
            return Err(format!(
                "{}: linha {}\n  esperado: {e}\n  obtido:   {o}",
                case.name,
                i + 1
            ));
        }
    }
    if expected.len() != got.len() {
        let first_extra = if got.len() > expected.len() {
            format!("primeira linha extra obtida: {:?}", got[expected.len()])
        } else {
            format!("primeira linha faltante: {:?}", expected[got.len()])
        };
        return Err(format!(
            "{}: número de linhas difere (esperado {}, obtido {}); {first_extra}",
            case.name,
            expected.len(),
            got.len()
        ));
    }
    Ok(())
}

/// Roda todos os casos cujo nome começa com `prefix` e reporta todos os que
/// falharam (cada um com o seu primeiro desvio).
fn run_golden(prefix: &str) {
    let cases: Vec<Case> = load_cases()
        .into_iter()
        .filter(|c| c.name.starts_with(prefix))
        .collect();
    assert!(
        !cases.is_empty(),
        "nenhum caso golden começa com {prefix:?}"
    );

    let failures: Vec<String> = cases.iter().filter_map(|c| check_case(c).err()).collect();
    assert!(
        failures.is_empty(),
        "{} de {} casos golden falharam:\n\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n\n")
    );
}

#[test]
fn golden_groups_partition_all_cases() {
    let cases = load_cases();
    assert_eq!(cases.len(), 30, "cases.json deveria ter 30 casos");
    for case in &cases {
        let hits = GROUPS.iter().filter(|g| case.name.starts_with(**g)).count();
        assert_eq!(
            hits, 1,
            "{} deve pertencer a exatamente um grupo",
            case.name
        );
        assert!(
            golden_path(&format!("{}.gcode", case.name)).is_file(),
            "fixture {}.gcode ausente",
            case.name
        );
    }
}

#[test]
fn golden_espiral() {
    run_golden("espiral_");
}

#[test]
fn golden_multiperfil() {
    run_golden("multiperfil_");
}

#[test]
fn golden_bolacha() {
    run_golden("bolacha_");
}

#[test]
fn golden_chapa() {
    run_golden("chapa_");
}

#[test]
fn golden_osc_linear() {
    run_golden("osc_linear");
}

#[test]
fn golden_osc_quadrada() {
    run_golden("osc_quadrada");
}

#[test]
fn golden_osc_quad_continua() {
    run_golden("osc_quad_continua");
}

#[test]
fn golden_legado() {
    run_golden("legado_");
}
