//! Paridade do G-code pelo caminho da interface, sem abrir janela.
//!
//! O que o operador faz no app — abrir com `UiState::default()` sobre um
//! `AppConfig::default()`, escolher a operação nos cartões e digitar só os
//! campos que diferem do padrão — precisa gerar, para cada uma das sete
//! operações, exatamente a fixture `tests/golden/<caso>.gcode` produzida pelo
//! Python. O teste golden de `tfm-core` parte dos parâmetros em JSON; este
//! parte dos textos dos campos e passa por `UiState::set_operation` e
//! `UiState::to_params`, cobrindo a troca de operação e a conversão
//! texto -> `Params`.
//!
//! Como nas fixtures, a data é fixada em `01/01/2000 00:00:00` e a versão em
//! `11.3.2` (o app imprime a sua própria). O fator e o custo do pó vêm do
//! config e só entram na estimativa de custos, nunca no G-code.

use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{Map, Value};
use tfm_app::ui_state::UiState;
use tfm_core::config::AppConfig;
use tfm_core::gcode::generate_with_timestamp;
use tfm_core::params::{Operation, Params};

const GOLDEN_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/golden");
const TIMESTAMP: &str = "01/01/2000 00:00:00";
const GOLDEN_VERSION: &str = "11.3.2";

/// Chaves de `cases.json` que o operador não digita: a operação vem dos
/// cartões (`set_operation`), o pó do config, a versão é fixada pelo teste e
/// os espelhos de velocidade são derivados por `to_params`.
const NOT_TYPED: [&str; 7] = [
    "welding_mode",
    "oscillation_type",
    "powder_factor",
    "powder_cost_brl_kg",
    "app_version",
    "taxa_de_deposicao",
    "velocidade_a_mm_min",
];

#[derive(Deserialize)]
struct Case {
    name: String,
    params: Map<String, Value>,
}

fn golden_path(file: &str) -> PathBuf {
    Path::new(GOLDEN_DIR).join(file)
}

fn read_golden(file: &str) -> String {
    let path = golden_path(file);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("não foi possível ler {}: {e}", path.display()))
}

/// O caso `name` de `cases.json`.
fn load_case(name: &str) -> Case {
    let cases: Vec<Case> =
        serde_json::from_str(&read_golden("cases.json")).expect("cases.json válido");
    cases
        .into_iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("caso {name:?} ausente em cases.json"))
}

/// Linhas da fixture; um `\r` final por linha é descartado (defesa contra
/// checkouts CRLF). O arquivo termina em `%` sem newline final.
fn expected_lines(name: &str) -> Vec<String> {
    read_golden(&format!("{name}.gcode"))
        .split('\n')
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

/// Texto que o operador digitaria para um valor do JSON (`50.0` -> `"50.0"`,
/// `100` -> `"100"`, textos como estão).
fn typed_text(key: &str, value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        other => panic!("{key}: valor {other} não cabe num campo de texto"),
    }
}

fn enum_of<T: DeserializeOwned>(key: &str, value: &Value) -> T {
    serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("{key}: {e}"))
}

/// Escreve `value` no campo da interface identificado por `key` (texto,
/// caixa de seleção ou combo).
fn apply_key(ui: &mut UiState, key: &str, value: &Value) {
    if let Some(text) = ui.fields.get_mut(key) {
        *text = typed_text(key, value);
        return;
    }
    if let Some(flag) = ui.flags.get_mut(key) {
        *flag = value
            .as_bool()
            .unwrap_or_else(|| panic!("{key}: {value} não é booleano"));
        return;
    }
    match key {
        "direcao_soldagem" => ui.direcao = enum_of(key, value),
        "sentido_rotacao" => ui.rotacao = enum_of(key, value),
        "flat_orientation" => ui.flat_orientation = enum_of(key, value),
        "multiperfil_strategy" => ui.multiperfil_strategy = enum_of(key, value),
        "multiperfil_profile_shape" => ui.profile_shape = enum_of(key, value),
        "bolacha_tipo" => ui.bolacha_tipo = enum_of(key, value),
        other => panic!("{other}: chave sem campo na interface"),
    }
}

/// Aplica ao estado da interface só os valores do caso que diferem dos
/// padrões (`Params::default()`), como um operador que edita o que precisa.
/// Devolve as chaves digitadas.
fn apply_overrides(ui: &mut UiState, case: &Case) -> Vec<String> {
    let base = serde_json::to_value(Params::default()).expect("Params padrão serializa");
    let mut typed = Vec::new();
    for (key, value) in &case.params {
        if NOT_TYPED.contains(&key.as_str()) || base.get(key) == Some(value) {
            continue;
        }
        apply_key(ui, key, value);
        typed.push(key.clone());
    }
    typed
}

/// Desvio de propósito: no Python o apagamento das oscilações pode andar 100 mm
/// em X, para fora da peça, e aqui ele gira em A. A linha logo depois de
/// `(APAGAMENTO DA TOCHA` vem do programa gerado; todas as outras seguem exatas.
fn with_rotary_tail(name: &str, mut expected: Vec<String>, got: &[String]) -> Vec<String> {
    let plate = name.starts_with("chapa_");
    if !name.starts_with("osc_") && !plate {
        return expected;
    }
    let tails: Vec<usize> = (1..expected.len())
        .filter(|&i| expected[i - 1].starts_with("(APAGAMENTO DA TOCHA"))
        .collect();
    for i in tails {
        // Na chapa o cordão para antes da borda: o fim dele, dois antes do
        // comentário, também difere.
        let stop = plate.then(|| i.checked_sub(3)).flatten();
        for at in stop.into_iter().chain([i]) {
            if let Some(line) = got.get(at) {
                expected[at] = line.clone();
            }
        }
    }
    expected
}

/// Abre o app nos padrões, escolhe `op` nos cartões, digita os desvios do
/// caso e compara o G-code gerado com a fixture, linha a linha.
fn check_case(name: &str, op: Operation) {
    let case = load_case(name);
    let mut ui = UiState::from_params(&Params::default());
    ui.set_operation(op);
    assert_eq!(ui.operation, op, "{name}: operação escolhida");
    let typed = apply_overrides(&mut ui, &case);

    let mut p = ui.to_params(&AppConfig::default());
    assert_eq!(Operation::from_params(&p), op, "{name}: operação aplicada");
    p.app_version = Some(GOLDEN_VERSION.to_string());

    let got = generate_with_timestamp(&p, TIMESTAMP)
        .unwrap_or_else(|e| panic!("{name}: gerador falhou pelos campos da UI: {e}"));
    let expected = with_rotary_tail(name, expected_lines(name), &got);
    if let Some(diff) = first_difference(&expected, &got) {
        panic!(
            "{name}: G-code pela UI difere de {name}.gcode (campos digitados: {typed:?}): {diff}"
        );
    }
}

#[test]
fn ui_padrao_abre_na_espiral_com_os_defaults_do_python() {
    let ui = UiState::default();
    assert_eq!(ui.operation, Operation::Espiral);
    assert_eq!(ui, UiState::from_params(&Params::default()));
}

#[test]
fn espiral_padrao_pela_ui() {
    check_case("espiral_padrao", Operation::Espiral);
}

#[test]
fn multiperfil_retangular_pela_ui() {
    check_case("multiperfil_retangular", Operation::EspiralMultiperfil);
}

#[test]
fn bolacha_espiral_pela_ui() {
    check_case("bolacha_espiral", Operation::Bolacha);
}

#[test]
fn chapa_x_pela_ui() {
    check_case("chapa_x", Operation::ChapaPlana);
}

#[test]
fn osc_linear_pela_ui() {
    check_case("osc_linear", Operation::OscilacaoLinear);
}

#[test]
fn osc_quadrada_pela_ui() {
    check_case("osc_quadrada", Operation::OscilacaoQuadrada);
}

#[test]
fn osc_quad_continua_pela_ui() {
    check_case("osc_quad_continua", Operation::OscilacaoQuadradaContinua);
}

/// Os desvios que cada caso realmente digita, para o teste não passar "por
/// acaso" com todos os campos iguais ao padrão.
#[test]
fn overrides_dos_casos_sao_os_esperados() {
    let typed = |name: &str, op: Operation| {
        let mut ui = UiState::from_params(&Params::default());
        ui.set_operation(op);
        let mut typed = apply_overrides(&mut ui, &load_case(name));
        // Os casos trazem a eficiência térmica 0,8 do Python; o padrão agora é
        // 0,6 (plasma), então todo caso a digita.
        let pos = typed.iter().position(|k| k == "eficiencia_termica");
        assert!(pos.is_some(), "{name}: os casos digitam a eficiência 0,8");
        typed.remove(pos.unwrap());
        typed
    };
    assert!(typed("espiral_padrao", Operation::Espiral).is_empty());
    assert_eq!(typed("bolacha_espiral", Operation::Bolacha), ["diametro"]);
    assert_eq!(
        typed("osc_quadrada", Operation::OscilacaoQuadrada),
        ["comprimento_revestir"]
    );
    assert_eq!(
        typed("osc_quad_continua", Operation::OscilacaoQuadradaContinua),
        ["comprimento_revestir"]
    );
}
