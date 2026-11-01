//! Paridade do G-code pelo caminho da interface, sem abrir janela.
//!
//! O que o operador vê ao abrir o app — `UiState::default()` sobre um
//! `AppConfig::default()` — precisa gerar, para a Espiral, exatamente a fixture
//! `tests/golden/espiral_padrao.gcode` produzida pelo Python. O teste golden de
//! `tfm-core` parte dos parâmetros em JSON; este parte dos textos dos campos e
//! passa por `UiState::to_params`, cobrindo a conversão texto -> `Params`.
//!
//! Como nas fixtures, a data é fixada em `01/01/2000 00:00:00` e a versão em
//! `11.3.2` (o app imprime a sua própria). O fator e o custo do pó vêm do
//! config e só entram na estimativa de custos, nunca no G-code.

use std::fs;

use tfm_app::ui_state::UiState;
use tfm_core::config::AppConfig;
use tfm_core::gcode::generate_with_timestamp;
use tfm_core::params::{Operation, Params};

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/golden/espiral_padrao.gcode"
);
const TIMESTAMP: &str = "01/01/2000 00:00:00";
const GOLDEN_VERSION: &str = "11.3.2";

/// Linhas da fixture; um `\r` final por linha é descartado (defesa contra
/// checkouts CRLF). O arquivo termina em `%` sem newline final.
fn expected_lines() -> Vec<String> {
    let text =
        fs::read_to_string(GOLDEN).unwrap_or_else(|e| panic!("não foi possível ler {GOLDEN}: {e}"));
    text.split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
        .collect()
}

/// Descreve o primeiro desvio entre o esperado e o obtido, se houver.
fn first_difference(expected: &[String], got: &[String]) -> Option<String> {
    for (i, (e, o)) in expected.iter().zip(got.iter()).enumerate() {
        if e != o {
            return Some(format!("linha {}\n  esperado: {e}\n  obtido:   {o}", i + 1));
        }
    }
    if expected.len() != got.len() {
        let (which, line) = if got.len() > expected.len() {
            ("obtido tem linha extra", &got[expected.len()])
        } else {
            ("obtido termina antes; faltou", &expected[got.len()])
        };
        return Some(format!(
            "número de linhas: esperado {}, obtido {} ({which}: {line:?})",
            expected.len(),
            got.len()
        ));
    }
    None
}

#[test]
fn ui_padrao_abre_na_espiral_com_os_defaults_do_python() {
    let ui = UiState::default();
    assert_eq!(ui.operation, Operation::Espiral);
    assert_eq!(ui, UiState::from_params(&Params::default()));
}

#[test]
fn espiral_padrao_pela_ui_e_identica_ao_golden() {
    let cfg = AppConfig::default();
    let mut p = UiState::default().to_params(&cfg);
    p.app_version = Some(GOLDEN_VERSION.to_string());

    let got = generate_with_timestamp(&p, TIMESTAMP)
        .unwrap_or_else(|e| panic!("gerador falhou com os defaults da UI: {e}"));
    let expected = expected_lines();

    if let Some(diff) = first_difference(&expected, &got) {
        panic!("G-code pela UI difere de espiral_padrao.gcode: {diff}");
    }
}
