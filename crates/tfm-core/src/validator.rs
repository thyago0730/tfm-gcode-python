//! Validação: regras físicas dos parâmetros e invariantes do G-code gerado.
//!
//! Porta de `validate_gcode_invariants` (`src/app/core/gcode_validator.py`) e
//! da união de `_validate_parameters` com as regras `valid_*` do fim de
//! `_get_current_params` (`src/app/TFM_GCODE.py`). As mensagens são as que a
//! UI Python exibe, em português.
//!
//! Os regexes do Python (`\bA([-+]?\d*\.?\d+)`, `\bM101\b`, `\bM102\b`,
//! `\bG0?1\b`) são reproduzidos à mão, sem a crate `regex`.

use crate::params::{Operation, Params, ProfileShape, WeldingMode};

/// Resultado de [`validate_physical`]: mensagens para o usuário e as chaves de
/// [`Params`] (nomes do JSON Python) por trás de cada regra violada.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldErrors {
    /// Mensagens em português: as de `_validate_parameters`, na ordem do
    /// Python, seguidas das regras `valid_*` de `_get_current_params` que no
    /// Python só pintam o campo de vermelho.
    pub errors: Vec<String>,
    /// Chaves de `Params` com problema, sem repetição, na ordem da primeira
    /// ocorrência (uma chave pode estar atrás de mais de uma regra).
    pub invalid_fields: Vec<&'static str>,
}

impl FieldErrors {
    /// `true` quando nenhuma regra falhou.
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    fn fail(&mut self, message: impl Into<String>, fields: &[&'static str]) {
        self.errors.push(message.into());
        for field in fields {
            if !self.invalid_fields.contains(field) {
                self.invalid_fields.push(field);
            }
        }
    }
}

/// União de `_validate_parameters` (mensagens do Python, na mesma ordem) com
/// as regras `valid_*` do fim de `_get_current_params`. Regras presentes nos
/// dois lugares (largura, velocidade, sobreposição, multiperfil) produzem uma
/// mensagem só; as que no Python bloqueiam sem mensagem (número de camadas,
/// comprimento e deslocamento da oscilação) ganham mensagem no mesmo estilo,
/// no fim da lista.
///
/// As regras por modo usam `welding_mode`/`oscillation_type` como o Python:
/// multiperfil só em `espiral_multiperfil`; oscilação nos três modos de
/// oscilação (inclusive os legados). A granularidade X vale para a quadrada
/// contínua efetiva ([`Operation::from_params`]) e lê `osc_test_gran_x`: o
/// Python lê a chave inexistente `granularidade_x` (bug conhecido), aqui é
/// portada a intenção.
pub fn validate_physical(p: &Params) -> FieldErrors {
    let mut fe = FieldErrors::default();

    // 1. Valores físicos básicos.
    if p.diametro <= 0.0 {
        fe.fail("Diâmetro deve ser maior que zero.", &["diametro"]);
    }
    if p.comprimento_revestir <= 0.0 {
        fe.fail(
            "Comprimento de revestimento deve ser maior que zero.",
            &["comprimento_revestir"],
        );
    }
    if p.velocidade_de_deposicao <= 0.0 {
        fe.fail(
            "Velocidade de deposição deve ser positiva.",
            &["velocidade_de_deposicao"],
        );
    }
    if p.largura_cordao <= 0.0 {
        fe.fail(
            "Largura do cordão deve ser maior que zero.",
            &["largura_cordao"],
        );
    }

    // 2. Consistência geométrica.
    let overlap = p.sobreposicao;
    if !(0.0..100.0).contains(&overlap) {
        fe.fail(
            format!(
                "Sobreposição ({}%) inválida. Deve estar entre 0 e 99%.",
                py_float(overlap)
            ),
            &["sobreposicao"],
        );
    }
    if p.welding_mode == WeldingMode::EspiralMultiperfil {
        if p.multiperfil_count < 1 {
            fe.fail(
                "Quantidade de perfis deve ser pelo menos 1.",
                &["multiperfil_count"],
            );
        }
        if p.multiperfil_total_angle_deg <= 0.0 {
            fe.fail(
                "Ângulo total do setor deve ser maior que zero.",
                &["multiperfil_total_angle_deg"],
            );
        }
        if p.multiperfil_pitch_mm <= 0.0 {
            fe.fail(
                "Passo por perfil deve ser maior que zero.",
                &["multiperfil_pitch_mm"],
            );
        }
        if p.multiperfil_profile_height_mm <= 0.0 {
            fe.fail(
                "Altura do perfil deve ser maior que zero.",
                &["multiperfil_profile_height_mm"],
            );
        }
        if p.multiperfil_profile_base_mm <= 0.0 {
            fe.fail(
                "Largura da base do perfil deve ser maior que zero.",
                &["multiperfil_profile_base_mm"],
            );
        }
        if p.multiperfil_profile_shape == ProfileShape::Trapezoidal
            && p.multiperfil_profile_top_mm <= 0.0
        {
            fe.fail(
                "Largura do topo do perfil trapezoidal deve ser maior que zero.",
                &["multiperfil_profile_top_mm"],
            );
        }
        if p.multiperfil_end_chamfer_enabled {
            let angle = p.multiperfil_end_chamfer_angle_deg;
            if angle <= 0.0 || angle >= 90.0 {
                fe.fail(
                    "Ângulo do chanfro deve estar entre 0 e 90 graus.",
                    &["multiperfil_end_chamfer_angle_deg"],
                );
            }
        }
    }

    // 3. Oscilação vs diâmetro (regra empírica do Python).
    let is_oscillation = p.welding_mode.is_oscillation();
    if is_oscillation && p.oscilacao_comprimento > p.diametro {
        fe.fail(
            format!(
                "Comprimento de oscilação ({}mm) maior que o diâmetro da peça ({}mm).",
                py_float(p.oscilacao_comprimento),
                py_float(p.diametro)
            ),
            &["oscilacao_comprimento", "diametro"],
        );
    }

    // Oscilação da bolacha: o envelope da onda precisa caber no raio da peça.
    if p.welding_mode == WeldingMode::Bolacha && p.bolacha_oscilar {
        bolacha_oscilation_rules(p, &mut fe);
    }

    // 4. Granularidade X da quadrada contínua.
    if Operation::from_params(p) == Operation::OscilacaoQuadradaContinua && p.osc_test_gran_x <= 0.0
    {
        fe.fail(
            "Granularidade X deve ser maior que zero.",
            &["osc_test_gran_x"],
        );
    }

    // Regras `valid_*` de `_get_current_params` sem mensagem no Python.
    if p.num_camadas < 1 {
        fe.fail("Número de camadas deve ser pelo menos 1.", &["num_camadas"]);
    }
    if is_oscillation {
        if p.oscilacao_comprimento <= 0.0 {
            fe.fail(
                "Comprimento de oscilação deve ser maior que zero.",
                &["oscilacao_comprimento"],
            );
        }
        let perc = p.deslocamento_angular_perc;
        if perc <= 0.0 || perc > 100.0 {
            fe.fail(
                format!(
                    "Deslocamento angular ({}%) inválido. Deve ser maior que 0 e no máximo 100%.",
                    py_float(perc)
                ),
                &["deslocamento_angular_perc"],
            );
        }
    }
    fe
}

/// Regras da oscilação da bolacha, só com ela ligada.
fn bolacha_oscilation_rules(p: &Params, fe: &mut FieldErrors) {
    use crate::gcode::bolacha_osc::{MIN_PERIODO_MM, MIN_PONTOS_CICLO};
    use crate::params::BolachaOscForma;
    let r_ext = p.diametro / 2.0;
    let r_int = (p.diametro_interno / 2.0).max(p.largura_cordao / 2.0);
    if p.oscilacao_comprimento <= 0.0 {
        fe.fail(
            "Comprimento de oscilação deve ser maior que zero.",
            &["oscilacao_comprimento"],
        );
    } else if p.oscilacao_comprimento >= r_ext - r_int {
        fe.fail(
            format!(
                "Comprimento de oscilação ({}mm) não cabe na largura radial da bolacha ({}mm).",
                py_float(p.oscilacao_comprimento),
                py_float((r_ext - r_int).max(0.0))
            ),
            &["oscilacao_comprimento", "diametro", "diametro_interno"],
        );
    }
    if p.bolacha_osc_periodo_mm < MIN_PERIODO_MM {
        fe.fail(
            format!("Período da oscilação deve ser de pelo menos {MIN_PERIODO_MM} mm."),
            &["bolacha_osc_periodo_mm"],
        );
    }
    if p.bolacha_osc_forma == BolachaOscForma::Senoidal
        && p.bolacha_osc_pontos_ciclo.round() < MIN_PONTOS_CICLO
    {
        fe.fail(
            format!("A senoide precisa de pelo menos {MIN_PONTOS_CICLO} pontos por ciclo."),
            &["bolacha_osc_pontos_ciclo"],
        );
    }
    if p.bolacha_osc_espera_s < 0.0 {
        fe.fail(
            "A espera na ponta não pode ser negativa.",
            &["bolacha_osc_espera_s"],
        );
    }
}

/// `true` quando [`validate_physical`] não encontra erro: os parâmetros podem
/// ir para `gcode::generate` (que também valida e devolve `GenError::Invalid`).
pub fn can_generate(p: &Params) -> bool {
    validate_physical(p).is_valid()
}

/// Formata um `f64` como `str()` do Python 3, para as mensagens que
/// interpolam valores (`f"Sobreposição ({overlap}%) ..."`): menor
/// representação que faz a volta (`0.1`, `12.5`), inteiros com `.0` (`100.0`),
/// notação científica com expoente de pelo menos dois dígitos fora de
/// `1e-4 <= |x| < 1e16` (`1e-05`, `1.5e+20`) e `inf`/`-inf`/`nan`.
pub fn py_float(v: f64) -> String {
    if v.is_nan() {
        return "nan".to_string();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    let sign = if v.is_sign_negative() { "-" } else { "" };
    if v == 0.0 {
        return format!("{sign}0.0");
    }
    // `{:e}` dá a mantissa mais curta que faz a volta e o expoente decimal.
    let sci = format!("{:e}", v.abs());
    let Some((mantissa, exp)) = sci.split_once('e') else {
        return format!("{sign}{sci}");
    };
    let Ok(exp) = exp.parse::<i32>() else {
        return format!("{sign}{sci}");
    };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    if !(-4..16).contains(&exp) {
        let (first, rest) = digits.split_at(1);
        let mant = if rest.is_empty() {
            first.to_string()
        } else {
            format!("{first}.{rest}")
        };
        let exp_sign = if exp < 0 { '-' } else { '+' };
        return format!("{sign}{mant}e{exp_sign}{:02}", exp.abs());
    }
    // Posição do ponto decimal em relação a `digits`.
    let point = exp + 1;
    let n = digits.len() as i32;
    let body = if point <= 0 {
        format!("0.{}{digits}", "0".repeat(point.unsigned_abs() as usize))
    } else if point >= n {
        format!("{digits}{}.0", "0".repeat((point - n) as usize))
    } else {
        let (int_part, frac_part) = digits.split_at(point as usize);
        format!("{int_part}.{frac_part}")
    };
    format!("{sign}{body}")
}

const EMPTY_MSG: &str = "G-code vazio";
const MISSING_ENABLE_MSG: &str = "Movimento de solda antes de ligar a tocha (M101)";
const REVERSED_A_PREFIX: &str = "Rotação A invertida: ";

/// Nomes dos três itens da checklist de [`gcode_checklist`], na ordem em que
/// ela os devolve.
pub const CHECKLIST_NAMES: [&str; 3] = [
    "Programa com linhas de G-code",
    "Tocha ligada (M101) antes do primeiro movimento de solda",
    "Eixo A sempre no sentido de rotação escolhido",
];

/// Um item da checklist de invariantes: o nome do que foi conferido e as
/// mensagens de falha (vazio = OK).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChecklistItem {
    pub name: &'static str,
    pub failures: Vec<String>,
}

impl ChecklistItem {
    /// O item passou (nenhuma falha).
    pub fn is_ok(&self) -> bool {
        self.failures.is_empty()
    }
}

/// Os mesmos invariantes de [`gcode_invariants`], agrupados nos três itens
/// de [`CHECKLIST_NAMES`] para mostrar como checklist (OK ou a mensagem de
/// falha). Achatar as falhas na ordem dos itens dá as mesmas mensagens de
/// `gcode_invariants`, que percorre o programa uma vez e produz no máximo
/// uma falha do item 1, ou falhas do item 2 (todas antes do primeiro `M101`)
/// seguidas das do item 3.
pub fn gcode_checklist(lines: &[String], p: &Params) -> Vec<ChecklistItem> {
    let mut items: Vec<ChecklistItem> = CHECKLIST_NAMES
        .iter()
        .map(|name| ChecklistItem {
            name,
            failures: Vec::new(),
        })
        .collect();
    for message in gcode_invariants(lines, p) {
        let index = if message == EMPTY_MSG {
            0
        } else if message == MISSING_ENABLE_MSG {
            1
        } else if message.starts_with(REVERSED_A_PREFIX) {
            2
        } else {
            debug_assert!(false, "invariante sem item na checklist: {message}");
            continue;
        };
        items[index].failures.push(message);
    }
    items
}

/// Todos os itens da checklist passaram.
pub fn checklist_passed(items: &[ChecklistItem]) -> bool {
    items.iter().all(ChecklistItem::is_ok)
}

/// Porta de `validate_gcode_invariants`: confere o programa gerado contra os
/// invariantes de processo. Devolve as mensagens (vazio = tudo certo):
///
/// * `"G-code vazio"` para uma lista sem linhas;
/// * `"Movimento de solda antes de ligar a tocha (M101)"` para cada `G1`/`G01`
///   antes do primeiro `M101` do programa (no máximo 3 mensagens);
/// * `"Rotação A invertida: {prev:.3} -> {a:.3} ({linha anterior} -> {linha})"`
///   quando, com a tocha ligada, o eixo A recua contra `sentido_rotacao`
///   (tolerância 1e-6; no máximo 3 mensagens). `M101`/`M102` zeram a
///   referência; linhas vazias e comentários `(...)` são ignorados.
pub fn gcode_invariants(lines: &[String], p: &Params) -> Vec<String> {
    if lines.is_empty() {
        return vec![EMPTY_MSG.to_string()];
    }
    let expected_sign = p.rotation_sign();
    let mut issues = Vec::new();
    let mut torch_on = false;
    let mut seen_any_enable = false;
    let mut prev: Option<(f64, &str)> = None;
    let mut a_violations = 0u32;
    let mut missing_enable_weld = 0u32;

    for raw in lines {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('(') {
            continue;
        }
        if contains_word(line, "M101") {
            torch_on = true;
            seen_any_enable = true;
            prev = None;
            continue;
        }
        if contains_word(line, "M102") {
            torch_on = false;
            prev = None;
            continue;
        }
        if !seen_any_enable && contains_g01(line) {
            missing_enable_weld += 1;
            if missing_enable_weld <= 3 {
                issues.push(MISSING_ENABLE_MSG.to_string());
            }
        }
        if !torch_on {
            continue;
        }
        let Some(a_val) = first_a_word(line) else {
            continue;
        };
        if let Some((prev_a, prev_line)) = prev {
            let delta = a_val - prev_a;
            let reversed = if expected_sign >= 0.0 {
                delta < -1e-6
            } else {
                delta > 1e-6
            };
            if reversed {
                a_violations += 1;
                if a_violations <= 3 {
                    issues.push(format!(
                        "{REVERSED_A_PREFIX}{prev_a:.3} -> {a_val:.3} ({prev_line} -> {line})"
                    ));
                }
            }
        }
        prev = Some((a_val, line));
    }
    issues
}

/// `\w` do Python em `str`: letras, dígitos e `_`.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `\b` imediatamente antes do byte `at` (fronteira de char) de uma palavra.
fn boundary_before(line: &str, at: usize) -> bool {
    !line[..at].chars().next_back().is_some_and(is_word_char)
}

/// `\b` imediatamente depois do byte `at` (fronteira de char) de uma palavra.
fn boundary_after(line: &str, at: usize) -> bool {
    !line[at..].chars().next().is_some_and(is_word_char)
}

/// `re.search(r"\bWORD\b", line)` para uma palavra ASCII alfanumérica.
fn contains_word(line: &str, word: &str) -> bool {
    line.match_indices(word)
        .any(|(i, _)| boundary_before(line, i) && boundary_after(line, i + word.len()))
}

/// `re.search(r"\bG0?1\b", line)`: a palavra `G1` ou `G01` (não `G010`, `XG1`).
fn contains_g01(line: &str) -> bool {
    line.match_indices('G').any(|(i, _)| {
        if !boundary_before(line, i) {
            return false;
        }
        let rest = &line[i + 1..];
        let len = if rest.starts_with("01") {
            3
        } else if rest.starts_with('1') {
            2
        } else {
            return false;
        };
        boundary_after(line, i + len)
    })
}

/// `float(re.search(r"\bA([-+]?\d*\.?\d+)", line).group(1))`: o primeiro `A`
/// em fronteira de palavra seguido de um número. Um `A` sem número é pulado e
/// a busca continua na linha; `None` se nenhum casar.
fn first_a_word(line: &str) -> Option<f64> {
    line.match_indices('A').find_map(|(i, _)| {
        if !boundary_before(line, i) {
            return None;
        }
        py_number_prefix(&line[i + 1..]).and_then(|s| s.parse::<f64>().ok())
    })
}

/// Prefixo de `rest` que casa `[-+]?\d*\.?\d+` (dígitos ASCII), com o mesmo
/// retrocesso do regex: `"5."` casa `"5"`, `".5"` casa `".5"`, `"-"` não casa.
fn py_number_prefix(rest: &str) -> Option<&str> {
    let b = rest.as_bytes();
    let mut j = usize::from(matches!(b.first(), Some(b'-' | b'+')));
    let int_start = j;
    while j < b.len() && b[j].is_ascii_digit() {
        j += 1;
    }
    let has_int = j > int_start;
    if j + 1 < b.len() && b[j] == b'.' && b[j + 1].is_ascii_digit() {
        j += 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        return Some(&rest[..j]);
    }
    has_int.then(|| &rest[..j])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gcode::generate_with_timestamp;
    use crate::params::{Operation, OscillationType, Params, ProfileShape, Rotation, WeldingMode};
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    const GOLDEN_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/golden");
    const TIMESTAMP: &str = "01/01/2000 00:00:00";

    fn golden_cases() -> Vec<serde_json::Value> {
        let text = std::fs::read_to_string(format!("{GOLDEN_DIR}/cases.json"))
            .expect("tests/golden/cases.json deve existir");
        let cases: serde_json::Value = serde_json::from_str(&text).expect("cases.json válido");
        cases.as_array().expect("cases.json é uma lista").clone()
    }

    fn golden_case(name: &str) -> (Params, Vec<String>) {
        let case = golden_cases()
            .into_iter()
            .find(|c| c["name"] == name)
            .unwrap_or_else(|| panic!("caso golden {name} não encontrado"));
        let p: Params = serde_json::from_value(case["params"].clone()).expect("params válidos");
        let lines = std::fs::read_to_string(format!("{GOLDEN_DIR}/{name}.gcode"))
            .expect("fixture .gcode deve existir")
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
            .collect();
        (p, lines)
    }

    fn lines(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // --- gcode_invariants ---------------------------------------------------

    #[test]
    fn invariants_accept_golden_spiral() {
        let (p, fixture) = golden_case("espiral_3camadas_de_ccw");
        assert_eq!(p.sentido_rotacao, Rotation::Antihoraria);
        assert!(
            gcode_invariants(&fixture, &p).is_empty(),
            "fixture ccw deve passar"
        );
        let generated = generate_with_timestamp(&p, TIMESTAMP).expect("espiral gera");
        assert!(
            gcode_invariants(&generated, &p).is_empty(),
            "saída gerada deve passar"
        );

        let (p, fixture) = golden_case("espiral_padrao");
        assert_eq!(p.sentido_rotacao, Rotation::Horaria);
        assert!(
            gcode_invariants(&fixture, &p).is_empty(),
            "fixture horária deve passar"
        );
    }

    #[test]
    fn invariants_detect_reversed_a() {
        let p = Params::default(); // horaria
        let issues = gcode_invariants(&lines(&["M101", "G01 A10", "G01 A5"]), &p);
        assert_eq!(
            issues,
            vec!["Rotação A invertida: 10.000 -> 5.000 (G01 A10 -> G01 A5)".to_string()]
        );

        // No sentido anti-horário a mesma sequência é válida...
        let ccw = Params {
            sentido_rotacao: Rotation::Antihoraria,
            ..Params::default()
        };
        assert!(gcode_invariants(&lines(&["M101", "G01 A10", "G01 A5"]), &ccw).is_empty());
        // ...e a sequência crescente é a invertida.
        let issues = gcode_invariants(&lines(&["M101", "G01 A-10", "G01 A-5"]), &ccw);
        assert_eq!(
            issues,
            vec!["Rotação A invertida: -10.000 -> -5.000 (G01 A-10 -> G01 A-5)".to_string()]
        );
    }

    #[test]
    fn invariants_limit_reversals_to_three_and_reset_on_torch() {
        let p = Params::default();
        let seq = lines(&[
            "M101", "G01 A50", "G01 A40", "G01 A30", "G01 A20", "G01 A10", "G01 A0",
        ]);
        let issues = gcode_invariants(&seq, &p);
        assert_eq!(
            issues.len(),
            3,
            "no máximo 3 mensagens de rotação invertida"
        );
        assert!(issues
            .iter()
            .all(|m| m.starts_with("Rotação A invertida: ")));

        // M101/M102 zeram a referência anterior.
        assert!(gcode_invariants(&lines(&["M101", "G01 A10", "M101", "G01 A5"]), &p).is_empty());
        assert!(gcode_invariants(
            &lines(&["M101", "G01 A10", "M102", "G00 A0", "M101", "G01 A5"]),
            &p
        )
        .is_empty());
        // Com a tocha desligada o eixo A não é verificado.
        assert!(gcode_invariants(&lines(&["M101", "M102", "G01 A10", "G01 A5"]), &p).is_empty());
    }

    #[test]
    fn invariants_empty_and_missing_enable() {
        let p = Params::default();
        assert_eq!(gcode_invariants(&[], &p), vec!["G-code vazio".to_string()]);
        // Lista só com linhas vazias/comentários não é "vazia" para o Python.
        assert!(gcode_invariants(&lines(&["", "   ", "(G01 A5)"]), &p).is_empty());

        let msg = "Movimento de solda antes de ligar a tocha (M101)".to_string();
        let issues = gcode_invariants(&lines(&["G01 X1", "G1 X2", "G01 X3", " G01 X4 "]), &p);
        assert_eq!(
            issues,
            vec![msg.clone(), msg.clone(), msg.clone()],
            "máximo de 3"
        );

        // Só conta antes de QUALQUER M101; depois, com a tocha desligada, não.
        assert!(gcode_invariants(&lines(&["M101", "M102", "G01 X5"]), &p).is_empty());
        // G00 não é movimento de solda; G010 e XG1 não são palavras G1/G01.
        assert!(gcode_invariants(&lines(&["G00 X1", "G010 X1", "XG1 X1", "G1X"]), &p).is_empty());
        assert_eq!(gcode_invariants(&lines(&["G1"]), &p), vec![msg.clone()]);
        assert_eq!(gcode_invariants(&lines(&["X5 G01"]), &p), vec![msg]);
    }

    /// A checklist estruturada tem três itens fixos, na ordem de
    /// [`CHECKLIST_NAMES`]; num programa golden todos passam, e as falhas
    /// achatadas são exatamente as mensagens de `gcode_invariants`.
    #[test]
    fn checklist_groups_invariants_by_item() {
        let (p, golden) = golden_case("espiral_padrao");
        let items = gcode_checklist(&golden, &p);
        assert_eq!(items.len(), 3);
        let names: Vec<&str> = items.iter().map(|i| i.name).collect();
        assert_eq!(names, CHECKLIST_NAMES);
        assert!(items.iter().all(ChecklistItem::is_ok));
        assert!(checklist_passed(&items));

        // G1 antes do M101 e, com a tocha ligada, o eixo A recuando (horário).
        let bad = lines(&["G1 X1", "M101", "G1 A10", "G1 A5", "M102"]);
        let items = gcode_checklist(&bad, &p);
        assert!(items[0].is_ok(), "programa não está vazio");
        assert_eq!(
            items[1].failures,
            vec!["Movimento de solda antes de ligar a tocha (M101)".to_string()]
        );
        assert_eq!(items[2].failures.len(), 1);
        assert!(
            items[2].failures[0].starts_with("Rotação A invertida: 10.000 -> 5.000"),
            "{}",
            items[2].failures[0]
        );
        assert!(!checklist_passed(&items));
        let flat: Vec<String> = items.iter().flat_map(|i| i.failures.clone()).collect();
        assert_eq!(flat, gcode_invariants(&bad, &p));

        let items = gcode_checklist(&[], &p);
        assert_eq!(items[0].failures, vec!["G-code vazio".to_string()]);
        assert!(items[1].is_ok() && items[2].is_ok());
        assert!(!checklist_passed(&items));
    }

    #[test]
    fn invariants_parse_a_word_like_python_regex() {
        let p = Params::default();
        // "XA5" não é palavra A (precedida por caractere de palavra); " A10" é.
        let issues = gcode_invariants(&lines(&["M101", "G01 XA5 A10", "G01 A5"]), &p);
        assert_eq!(
            issues,
            vec!["Rotação A invertida: 10.000 -> 5.000 (G01 XA5 A10 -> G01 A5)".to_string()]
        );
        // Formas numéricas: "5." casa "5", "-.5" casa "-.5", "+5" casa "+5".
        let issues = gcode_invariants(&lines(&["M101", "G01 A5.", "G01 A-.5"]), &p);
        assert_eq!(
            issues,
            vec!["Rotação A invertida: 5.000 -> -0.500 (G01 A5. -> G01 A-.5)".to_string()]
        );
        let issues = gcode_invariants(&lines(&["M101", "G01 A+5", "G01 A4.9999"]), &p);
        assert_eq!(
            issues,
            vec!["Rotação A invertida: 5.000 -> 5.000 (G01 A+5 -> G01 A4.9999)".to_string()]
        );
        // "A" sem número é ignorado; um "A" válido mais à frente é usado.
        let issues = gcode_invariants(&lines(&["M101", "G01 A- A10", "G01 A"]), &p);
        assert!(issues.is_empty());
        let issues = gcode_invariants(&lines(&["M101", "G01 A- A10", "G01 A..5 A5"]), &p);
        assert_eq!(issues.len(), 1);
        // Tolerância de 1e-6 e sem falso positivo em movimentos iguais.
        assert!(gcode_invariants(
            &lines(&["M101", "G01 A10", "G01 A10", "G01 A9.9999995"]),
            &p
        )
        .is_empty());
    }

    // --- validate_physical ---------------------------------------------------

    #[test]
    fn physical_accepts_defaults_and_all_golden_cases() {
        let p = Params::default();
        let fe = validate_physical(&p);
        assert!(fe.errors.is_empty(), "{:?}", fe.errors);
        assert!(fe.invalid_fields.is_empty());
        assert!(can_generate(&p));
        for case in golden_cases() {
            let p: Params = serde_json::from_value(case["params"].clone()).expect("params");
            let fe = validate_physical(&p);
            assert!(fe.errors.is_empty(), "{}: {:?}", case["name"], fe.errors);
        }
    }

    #[test]
    fn physical_blocks_zero_width() {
        let p = Params {
            largura_cordao: 0.0,
            ..Params::default()
        };
        let fe = validate_physical(&p);
        assert_eq!(
            fe.errors,
            vec!["Largura do cordão deve ser maior que zero.".to_string()]
        );
        assert_eq!(fe.invalid_fields, vec!["largura_cordao"]);
        assert!(!can_generate(&p));
    }

    #[test]
    fn physical_basic_messages_match_python() {
        let p = Params {
            diametro: 0.0,
            comprimento_revestir: -1.0,
            velocidade_de_deposicao: 0.0,
            sobreposicao: 100.0,
            num_camadas: 0,
            ..Params::default()
        };
        let fe = validate_physical(&p);
        assert_eq!(
            fe.errors,
            vec![
                "Diâmetro deve ser maior que zero.".to_string(),
                "Comprimento de revestimento deve ser maior que zero.".to_string(),
                "Velocidade de deposição deve ser positiva.".to_string(),
                "Sobreposição (100.0%) inválida. Deve estar entre 0 e 99%.".to_string(),
                "Número de camadas deve ser pelo menos 1.".to_string(),
            ]
        );
        assert_eq!(
            fe.invalid_fields,
            vec![
                "diametro",
                "comprimento_revestir",
                "velocidade_de_deposicao",
                "sobreposicao",
                "num_camadas"
            ]
        );

        let mut p = Params {
            sobreposicao: -0.5,
            ..Params::default()
        };
        assert_eq!(
            validate_physical(&p).errors,
            vec!["Sobreposição (-0.5%) inválida. Deve estar entre 0 e 99%.".to_string()]
        );
        p.sobreposicao = 99.0;
        assert!(can_generate(&p));
    }

    #[test]
    fn physical_multiperfil_rules() {
        let mut p = Params::default();
        p.apply_operation(Operation::EspiralMultiperfil);
        assert!(can_generate(&p));
        p.multiperfil_count = 0;
        p.multiperfil_total_angle_deg = 0.0;
        p.multiperfil_pitch_mm = 0.0;
        p.multiperfil_profile_height_mm = 0.0;
        p.multiperfil_profile_base_mm = 0.0;
        p.multiperfil_profile_shape = ProfileShape::Trapezoidal;
        p.multiperfil_profile_top_mm = 0.0;
        p.multiperfil_end_chamfer_enabled = true;
        p.multiperfil_end_chamfer_angle_deg = 90.0;
        let fe = validate_physical(&p);
        assert_eq!(
            fe.errors,
            vec![
                "Quantidade de perfis deve ser pelo menos 1.".to_string(),
                "Ângulo total do setor deve ser maior que zero.".to_string(),
                "Passo por perfil deve ser maior que zero.".to_string(),
                "Altura do perfil deve ser maior que zero.".to_string(),
                "Largura da base do perfil deve ser maior que zero.".to_string(),
                "Largura do topo do perfil trapezoidal deve ser maior que zero.".to_string(),
                "Ângulo do chanfro deve estar entre 0 e 90 graus.".to_string(),
            ]
        );
        assert_eq!(
            fe.invalid_fields,
            vec![
                "multiperfil_count",
                "multiperfil_total_angle_deg",
                "multiperfil_pitch_mm",
                "multiperfil_profile_height_mm",
                "multiperfil_profile_base_mm",
                "multiperfil_profile_top_mm",
                "multiperfil_end_chamfer_angle_deg",
            ]
        );
        // Topo só importa no trapezoidal; chanfro só quando habilitado.
        let mut p = Params::default();
        p.apply_operation(Operation::EspiralMultiperfil);
        p.multiperfil_profile_shape = ProfileShape::Retangular;
        p.multiperfil_profile_top_mm = 0.0;
        p.multiperfil_end_chamfer_enabled = false;
        p.multiperfil_end_chamfer_angle_deg = 0.0;
        assert!(can_generate(&p));
        // As regras multiperfil não se aplicam fora do modo.
        p.apply_operation(Operation::Espiral);
        p.multiperfil_pitch_mm = 0.0;
        assert!(can_generate(&p));
    }

    #[test]
    fn physical_oscillation_rules() {
        let mut p = Params::default();
        p.apply_operation(Operation::OscilacaoLinear);
        assert!(can_generate(&p));
        p.oscilacao_comprimento = 60.0; // > diâmetro 50
        p.deslocamento_angular_perc = 0.0;
        let fe = validate_physical(&p);
        assert_eq!(
            fe.errors,
            vec![
                "Comprimento de oscilação (60.0mm) maior que o diâmetro da peça (50.0mm)."
                    .to_string(),
                "Deslocamento angular (0.0%) inválido. Deve ser maior que 0 e no máximo 100%."
                    .to_string(),
            ]
        );
        assert_eq!(
            fe.invalid_fields,
            vec![
                "oscilacao_comprimento",
                "diametro",
                "deslocamento_angular_perc"
            ]
        );
        p.deslocamento_angular_perc = 100.0;
        p.oscilacao_comprimento = 0.0;
        let fe = validate_physical(&p);
        assert_eq!(
            fe.errors,
            vec!["Comprimento de oscilação deve ser maior que zero.".to_string()]
        );
        assert_eq!(fe.invalid_fields, vec!["oscilacao_comprimento"]);
        p.deslocamento_angular_perc = 100.5;
        p.oscilacao_comprimento = 5.0;
        assert_eq!(
            validate_physical(&p).errors,
            vec![
                "Deslocamento angular (100.5%) inválido. Deve ser maior que 0 e no máximo 100%."
                    .to_string()
            ]
        );
        // Os extremos: 100% é aceito, 0% e negativo não.
        p.deslocamento_angular_perc = 100.0;
        assert!(validate_physical(&p).errors.is_empty(), "100% é válido");
        p.deslocamento_angular_perc = -5.0;
        assert_eq!(
            validate_physical(&p).errors,
            vec![
                "Deslocamento angular (-5.0%) inválido. Deve ser maior que 0 e no máximo 100%."
                    .to_string()
            ]
        );

        // Diâmetro zero em oscilação: a mesma chave aparece uma vez só.
        let mut p = Params::default();
        p.apply_operation(Operation::OscilacaoQuadrada);
        p.diametro = 0.0;
        let fe = validate_physical(&p);
        assert_eq!(
            fe.errors,
            vec![
                "Diâmetro deve ser maior que zero.".to_string(),
                "Comprimento de oscilação (5.0mm) maior que o diâmetro da peça (0.0mm)."
                    .to_string(),
            ]
        );
        assert_eq!(fe.invalid_fields, vec!["diametro", "oscilacao_comprimento"]);

        // Modos legados também são oscilação.
        let p = Params {
            welding_mode: WeldingMode::OscilacaoLinear,
            oscilacao_comprimento: 0.0,
            ..Params::default()
        };
        assert!(!can_generate(&p));
        // Fora da oscilação as regras não se aplicam.
        let mut p = Params::default();
        p.apply_operation(Operation::ChapaPlana);
        p.oscilacao_comprimento = 0.0;
        p.deslocamento_angular_perc = 0.0;
        assert!(can_generate(&p));
    }

    #[test]
    fn physical_granularity_only_for_square_continuous() {
        let mut p = Params::default();
        p.apply_operation(Operation::OscilacaoQuadradaContinua);
        p.osc_test_gran_x = 0.0;
        let fe = validate_physical(&p);
        assert_eq!(
            fe.errors,
            vec!["Granularidade X deve ser maior que zero.".to_string()]
        );
        assert_eq!(fe.invalid_fields, vec!["osc_test_gran_x"]);

        p.apply_operation(Operation::OscilacaoQuadrada);
        assert!(can_generate(&p));
        // Tipo quadrada_continua pendurado num modo que não é oscilação não conta.
        p.apply_operation(Operation::Espiral);
        p.oscillation_type = Some(OscillationType::QuadradaContinua);
        assert!(can_generate(&p));
    }

    #[test]
    fn py_float_formats_like_python_str() {
        for (v, s) in [
            (100.0, "100.0"),
            (5.0, "5.0"),
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (12.5, "12.5"),
            (0.1, "0.1"),
            (-0.5, "-0.5"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1.5e-7, "1.5e-07"),
            (1e15, "1000000000000000.0"),
            (1234.5678, "1234.5678"),
            (1e16, "1e+16"),
            (1.5e20, "1.5e+20"),
            (1e100, "1e+100"),
            (f64::INFINITY, "inf"),
            (f64::NEG_INFINITY, "-inf"),
            (f64::NAN, "nan"),
        ] {
            assert_eq!(py_float(v), s, "py_float({v:?})");
        }
    }

    // --- robustez do gerador ----------------------------------------------------

    fn run_degenerate(label: &str, p: &Params) {
        let p = p.clone();
        let (tx, rx) = mpsc::channel();
        let start = Instant::now();
        thread::spawn(move || {
            let res = catch_unwind(AssertUnwindSafe(|| generate_with_timestamp(&p, TIMESTAMP)));
            let _ = tx.send(res);
        });
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Ok(_)) => {
                let elapsed = start.elapsed();
                assert!(
                    elapsed < Duration::from_secs(1),
                    "{label}: levou {elapsed:?}"
                );
            }
            Ok(Err(_)) => panic!("{label}: generate_with_timestamp entrou em pânico"),
            Err(_) => {
                panic!("{label}: generate_with_timestamp não terminou em 1 s (laço infinito?)")
            }
        }
    }

    #[test]
    fn degenerate_params_never_panic() {
        type Override = (&'static str, fn(&mut Params));
        let overrides: [Override; 8] = [
            ("diametro 0", |p| p.diametro = 0.0),
            ("largura 0", |p| p.largura_cordao = 0.0),
            ("sobreposicao 100", |p| p.sobreposicao = 100.0),
            ("num_camadas 0", |p| p.num_camadas = 0),
            ("comprimento 0", |p| p.comprimento_revestir = 0.0),
            ("oscilacao 0", |p| p.oscilacao_comprimento = 0.0),
            ("deslocamento 0", |p| p.deslocamento_angular_perc = 0.0),
            ("multiperfil_pitch 0", |p| p.multiperfil_pitch_mm = 0.0),
        ];
        for op in Operation::ALL {
            for (name, apply) in overrides {
                let mut p = Params::default();
                p.apply_operation(op);
                apply(&mut p);
                run_degenerate(&format!("{op} / {name}"), &p);
            }
        }
    }
}
