//! Avaliador de fórmulas estilo Python.
//!
//! Porta o bloco `formulas` do `config.json`: expressões na sintaxe aritmética
//! do Python (`math.pi * diametro`, `max(x, 1e-6)`, `a // b`, `2 ** 3`) que o
//! original avalia com `eval(expr, {'__builtins__': {}}, locals)` sobre os
//! locais de `build_formula_locals` (`src/app/core/formula_context.py`).
//!
//! * [`parse`] (lexer + parser de precedência) produz uma [`Expr`];
//! * [`eval`] avalia a árvore com a semântica numérica do Python, tudo em
//!   `f64` (a distinção int/float não é reproduzida: `math.ceil` devolve um
//!   `f64` inteiro);
//! * [`formula_locals`] monta as variáveis a partir de [`Params`] e [`Costs`];
//! * [`evaluate_block`] reproduz o laço de até [`MAX_ROUNDS`] rodadas de
//!   `_evaluate_formulas_runtime` (TFM_GCODE.py), com dependências entre
//!   chaves em qualquer ordem.
//!
//! Operadores: `+ - * / // % **`, unários `-`/`+`, parênteses. `/` é divisão
//! real, `//` divisão inteira (piso) e `%` módulo com o sinal do divisor,
//! ambos pelo algoritmo de `float_divmod` do CPython; `**` associa à direita e
//! liga mais forte que o unário (`-2**2 == -4`). Divisão por zero, `0 ** -1`,
//! base negativa com expoente fracionário (complexo no Python), overflow em
//! `**` e erros de domínio de `math` (`sqrt(-1)`, `ceil(inf)`) viram
//! [`FormulaError::Math`]. Comparações e operadores booleanos não são
//! suportados (erro de sintaxe, como qualquer caractere fora da gramática).
//!
//! Funções: `max`/`min` (1+ argumentos, o primeiro vence em empate e NaN se
//! propaga como no `max` do Python), `abs`, `round(x)` e `round(x, n)` com
//! arredondamento "metade para o par", `sqrt`, `sin`, `cos`, `tan`, `ceil`,
//! `floor`, todas com ou sem o prefixo `math.`; e, apenas com o prefixo,
//! `math.pow`, `exp`, `log` (1 ou 2 args), `log10`, `log2`, `fabs`,
//! `radians`, `degrees`, `asin`, `acos`, `atan`, `atan2`, `hypot`, `trunc`.
//! Constantes: `pi`/`math.pi`, `e`/`math.e`, `math.tau`, `math.inf`,
//! `math.nan`. Identificadores podem conter `.` (`math.pi` é um único
//! [`Expr::Var`]); a resolução consulta primeiro as variáveis e depois a
//! tabela de constantes.

mod eval;
mod lexer;
mod parser;

use std::f64::consts::PI;

use indexmap::IndexMap;
use serde_json::Value;

use crate::config::Costs;
use crate::overlap::compute_overlap_metrics;
use crate::params::Params;

pub use eval::eval;
pub use parser::parse;

/// Rodadas máximas de [`evaluate_block`] (`for _ in range(6)` no Python).
pub const MAX_ROUNDS: usize = 6;

/// Operadores unários (`Neg`, `Pos`) e binários (os demais).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    Add,
    Sub,
    Mul,
    /// `/`: divisão real.
    Div,
    /// `//`: divisão inteira (piso).
    FloorDiv,
    /// `%`: módulo com o sinal do divisor.
    Mod,
    /// `**`: potência, associativa à direita.
    Pow,
    /// `-x`.
    Neg,
    /// `+x`.
    Pos,
}

/// Árvore de uma fórmula.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// Literal numérico (`1`, `1.5`, `1e-6`, `.5`, `1_000`).
    Num(f64),
    /// Identificador, possivelmente com pontos (`diametro`, `math.pi`).
    Var(String),
    /// `-x` / `+x`.
    Unary(Op, Box<Expr>),
    /// `a <op> b`.
    Binary(Op, Box<Expr>, Box<Expr>),
    /// Chamada de função embutida (`max(a, b)`, `math.ceil(x)`).
    Call(String, Vec<Expr>),
}

/// Falhas de análise e avaliação.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FormulaError {
    /// Caractere inválido, token inesperado, parêntese sem par, expressão
    /// vazia... (o `SyntaxError` do Python).
    #[error("erro de sintaxe: {0}")]
    Parse(String),
    /// Variável ou função desconhecida (o `NameError` do Python; em
    /// [`evaluate_block`] a chave fica pendente para a próxima rodada).
    #[error("nome desconhecido: {0}")]
    UnknownName(String),
    /// Divisão por zero, domínio inválido, overflow, operando `None`
    /// (`ZeroDivisionError`/`ValueError`/`OverflowError`/`TypeError`).
    #[error("erro matemático: {0}")]
    Math(String),
    /// Quantidade de argumentos inválida para a função (`TypeError`).
    #[error("aridade inválida: {0}")]
    Arity(String),
}

/// `max(a, b)` do Python: devolve `b` só se `b > a` (o primeiro vence em
/// empate e quando a comparação envolve NaN).
fn py_max(a: f64, b: f64) -> f64 {
    if b > a {
        b
    } else {
        a
    }
}

/// `float(value or fallback)` do Python: `0.0`/`-0.0` é falso, NaN é verdadeiro.
fn py_or(value: f64, fallback: f64) -> f64 {
    if value == 0.0 {
        fallback
    } else {
        value
    }
}

/// Porta de `build_formula_locals(params, costs)` (`formula_context.py`).
///
/// Na ordem do Python:
/// 1. `**params`: todo campo numérico ou booleano de [`Params`] sob sua chave
///    JSON (`bool` -> 0.0/1.0, `i64` -> `f64`, `Option<f64>` só quando `Some`).
///    Enums e strings são ignorados: as fórmulas só usam nomes numéricos.
///    A lista vem da serialização serde da struct, para acompanhar
///    automaticamente campos novos e os `rename`/`alias` do JSON.
/// 2. `step_over_mm`, `effective_width_mm`, `overlap_pass_count` do modelo de
///    sobreposição com os mesmos argumentos do Python: nominal =
///    `largura_cordao`, `sobreposicao`, camada = `espessura_camada`, raio =
///    `diametro / 2` quando > 0, alvo = `comprimento_revestir` quando > 0
///    (`largura_alvo` não existe em `Params`), `force_min_two_passes = False`
///    e calibração 1.0 (o Python **não** repassa
///    `bead_width_calibration_factor`). Antes da chamada o passo parte do
///    fallback `largura_cordao * max(0.01, 1 - sobreposicao/100)` (ou `1e-6`
///    quando a largura é <= 0), usado se o modelo devolver zero.
/// 3. `pi` e, dos custos, `gas_argon_brl_m3`, `labor_brl_hour`,
///    `machine_brl_hour`; `powder_cost_brl_kg` é o valor dos params (o Python
///    só cai em `costs.powder_brl_kg` quando a chave falta, o que não ocorre
///    com a struct).
///
/// As funções (`max`, `math`...) **não** entram no mapa: são embutidas em
/// [`eval`].
pub fn formula_locals(p: &Params, costs: &Costs) -> IndexMap<String, f64> {
    let mut out: IndexMap<String, f64> = IndexMap::new();

    // 1. `**params`
    if let Ok(Value::Object(fields)) = serde_json::to_value(p) {
        for (key, value) in fields {
            let number = match value {
                Value::Number(n) => n.as_f64(),
                Value::Bool(b) => Some(if b { 1.0 } else { 0.0 }),
                _ => None,
            };
            if let Some(v) = number {
                out.insert(key, v);
            }
        }
    }

    // 2. Modelo de sobreposição
    let nominal_width = p.largura_cordao;
    let overlap_pct = p.sobreposicao;
    let fallback_step = if nominal_width > 0.0 {
        nominal_width * py_max(0.01, 1.0 - (overlap_pct / 100.0))
    } else {
        1e-6
    };
    let radius_mm = (p.diametro > 0.0).then(|| p.diametro / 2.0);
    let target_width = p.comprimento_revestir;
    let metrics = compute_overlap_metrics(
        nominal_width,
        overlap_pct,
        p.espessura_camada,
        radius_mm,
        (target_width > 0.0).then_some(target_width),
        false,
        1.0,
    );
    let step_over_mm = py_or(metrics.step_over_mm, fallback_step);
    let effective_width_mm = py_or(metrics.bead.effective_width_mm, nominal_width);
    let overlap_pass_count = if metrics.pass_count == 0 {
        1.0
    } else {
        metrics.pass_count as f64
    };
    out.insert("step_over_mm".to_string(), step_over_mm);
    out.insert("effective_width_mm".to_string(), effective_width_mm);
    out.insert("overlap_pass_count".to_string(), overlap_pass_count);

    // 3. Constante e custos
    out.insert("pi".to_string(), PI);
    out.insert("gas_argon_brl_m3".to_string(), costs.gas_argon_brl_m3);
    out.insert("labor_brl_hour".to_string(), costs.labor_brl_hour);
    out.insert("machine_brl_hour".to_string(), costs.machine_brl_hour);
    out.insert("powder_cost_brl_kg".to_string(), p.powder_cost_brl_kg);
    out
}

/// Porta de `_evaluate_formulas_runtime` (TFM_GCODE.py).
///
/// Até [`MAX_ROUNDS`] rodadas; em cada uma, as chaves ainda pendentes são
/// avaliadas na ordem original com `{**locals, **results}` (um resultado fica
/// visível imediatamente para as chaves seguintes da mesma rodada e tem
/// prioridade sobre um local homônimo). [`FormulaError::UnknownName`]
/// (`NameError`) mantém a chave pendente; qualquer outro erro, inclusive de
/// sintaxe e referência a uma chave que já falhou (`None` no Python, que
/// levanta `TypeError` ao operar), grava `None` e encerra a chave. O laço
/// para quando uma rodada não progride.
///
/// O mapa devolvido tem **todas** as chaves de `expressions`, na ordem
/// original; `None` para as que falharam ou ficaram pendentes.
pub fn evaluate_block(
    expressions: &IndexMap<String, String>,
    locals: &IndexMap<String, f64>,
) -> IndexMap<String, Option<f64>> {
    let mut results: IndexMap<String, Option<f64>> = IndexMap::with_capacity(expressions.len());
    // Cada expressão é analisada uma única vez; o erro de sintaxe é aplicado
    // na primeira visita, como o SyntaxError do `eval` do Python.
    let mut pending: Vec<(&String, Result<Expr, FormulaError>)> = expressions
        .iter()
        .map(|(key, src)| (key, parse(src)))
        .collect();

    for _ in 0..MAX_ROUNDS {
        if pending.is_empty() {
            break;
        }
        let mut progressed = false;
        let mut still_pending = Vec::with_capacity(pending.len());
        for (key, parsed) in pending {
            let outcome = match &parsed {
                Err(e) => Err(e.clone()),
                Ok(expr) => {
                    let lookup = |name: &str| match results.get(name) {
                        Some(Some(v)) => eval::Lookup::Found(*v),
                        Some(None) => eval::Lookup::Null,
                        None => match locals.get(name) {
                            Some(v) => eval::Lookup::Found(*v),
                            None => eval::Lookup::Missing,
                        },
                    };
                    eval::eval_lookup(expr, &lookup)
                }
            };
            match outcome {
                Ok(v) => {
                    results.insert(key.clone(), Some(v));
                    progressed = true;
                }
                Err(FormulaError::UnknownName(_)) => still_pending.push((key, parsed)),
                Err(_) => {
                    results.insert(key.clone(), None);
                    progressed = true;
                }
            }
        }
        pending = still_pending;
        if !progressed {
            break;
        }
    }

    expressions
        .keys()
        .map(|key| (key.clone(), results.get(key).copied().flatten()))
        .collect()
}
