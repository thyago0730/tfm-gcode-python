//! Avaliação de [`Expr`] com a semântica numérica do Python, tudo em `f64`.
//!
//! A ordem de avaliação é a do Python (esquerda para a direita; em chamadas,
//! primeiro o nome da função, depois os argumentos, depois a aridade), para
//! que o **primeiro** erro encontrado seja o devolvido — isso decide, em
//! [`super::evaluate_block`], se uma chave fica pendente (`UnknownName`) ou
//! falha de vez.

use std::f64::consts::{E, PI, TAU};

use super::{Expr, FormulaError, Op};

/// Resolução de um nome em [`eval_lookup`].
pub(super) enum Lookup {
    /// Variável com valor.
    Found(f64),
    /// Variável presente mas `None` (chave que já falhou em `evaluate_block`):
    /// qualquer uso é erro, como `None * 2` levanta `TypeError` no Python.
    Null,
    /// Nome ausente: cai na tabela de constantes e depois em `UnknownName`.
    Missing,
}

/// Avalia `e`. `vars` resolve variáveis (`Some(valor)`); nomes não resolvidos
/// caem nas constantes (`pi`, `math.pi`, `e`, `math.e`, `math.tau`,
/// `math.inf`, `math.nan`) e por fim em [`FormulaError::UnknownName`].
pub fn eval(e: &Expr, vars: &dyn Fn(&str) -> Option<f64>) -> Result<f64, FormulaError> {
    eval_lookup(e, &|name| match vars(name) {
        Some(value) => Lookup::Found(value),
        None => Lookup::Missing,
    })
}

/// Variante de [`eval`] que distingue variável `None` de variável ausente.
pub(super) fn eval_lookup(e: &Expr, lookup: &dyn Fn(&str) -> Lookup) -> Result<f64, FormulaError> {
    match e {
        Expr::Num(value) => Ok(*value),
        Expr::Var(name) => resolve_var(name, lookup),
        Expr::Unary(op, inner) => {
            let value = eval_lookup(inner, lookup)?;
            match op {
                Op::Neg => Ok(-value),
                Op::Pos => Ok(value),
                other => Err(FormulaError::Parse(format!(
                    "operador {other:?} não é unário"
                ))),
            }
        }
        Expr::Binary(op, lhs, rhs) => {
            let a = eval_lookup(lhs, lookup)?;
            let b = eval_lookup(rhs, lookup)?;
            binary(*op, a, b)
        }
        Expr::Call(name, args) => {
            let func =
                Builtin::lookup(name).ok_or_else(|| FormulaError::UnknownName(name.clone()))?;
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                values.push(eval_lookup(arg, lookup)?);
            }
            func.call(name, &values)
        }
    }
}

fn resolve_var(name: &str, lookup: &dyn Fn(&str) -> Lookup) -> Result<f64, FormulaError> {
    match lookup(name) {
        Lookup::Found(value) => Ok(value),
        Lookup::Null => Err(FormulaError::Math(format!(
            "'{name}' é None (a fórmula de que depende falhou)"
        ))),
        Lookup::Missing => {
            constant(name).ok_or_else(|| FormulaError::UnknownName(name.to_string()))
        }
    }
}

fn constant(name: &str) -> Option<f64> {
    match name {
        "pi" | "math.pi" => Some(PI),
        "e" | "math.e" => Some(E),
        "math.tau" => Some(TAU),
        "math.inf" => Some(f64::INFINITY),
        "math.nan" => Some(f64::NAN),
        _ => None,
    }
}

fn math_err(msg: impl Into<String>) -> FormulaError {
    FormulaError::Math(msg.into())
}

fn binary(op: Op, a: f64, b: f64) -> Result<f64, FormulaError> {
    match op {
        Op::Add => Ok(a + b),
        Op::Sub => Ok(a - b),
        Op::Mul => Ok(a * b),
        Op::Div if b == 0.0 => Err(math_err("divisão por zero")),
        Op::Div => Ok(a / b),
        Op::FloorDiv if b == 0.0 => Err(math_err("divisão inteira por zero")),
        Op::FloorDiv => Ok(py_divmod(a, b).0),
        Op::Mod if b == 0.0 => Err(math_err("módulo por zero")),
        Op::Mod => Ok(py_divmod(a, b).1),
        Op::Pow => py_pow(a, b),
        Op::Neg | Op::Pos => Err(FormulaError::Parse(format!(
            "operador {op:?} não é binário"
        ))),
    }
}

/// `float_divmod` do CPython (`floatobject.c`): devolve `(a // b, a % b)` com
/// o módulo levando o sinal do divisor. `b` já é não zero.
fn py_divmod(a: f64, b: f64) -> (f64, f64) {
    let mut modulo = a % b; // fmod: sinal do dividendo
    let mut div = (a - modulo) / b;
    if modulo != 0.0 {
        if (b < 0.0) != (modulo < 0.0) {
            modulo += b;
            div -= 1.0;
        }
    } else {
        modulo = 0.0f64.copysign(b);
    }
    let floordiv = if div != 0.0 {
        let mut floordiv = div.floor();
        if div - floordiv > 0.5 {
            floordiv += 1.0;
        }
        floordiv
    } else {
        0.0f64.copysign(a / b)
    };
    (floordiv, modulo)
}

/// `float_pow` do CPython restrito ao domínio real: `x ** 0 == 1` (mesmo
/// NaN), `1 ** nan == 1`, `0 ** negativo` é erro, base negativa com expoente
/// fracionário seria complexo (erro aqui) e overflow com operandos finitos é
/// erro (`OverflowError`).
fn py_pow(a: f64, b: f64) -> Result<f64, FormulaError> {
    if b == 0.0 {
        return Ok(1.0);
    }
    if a.is_nan() {
        return Ok(f64::NAN);
    }
    if b.is_nan() {
        return Ok(if a == 1.0 { 1.0 } else { f64::NAN });
    }
    if a == 0.0 && b < 0.0 {
        return Err(math_err("0.0 não pode ser elevado a potência negativa"));
    }
    if a < 0.0 && a.is_finite() && b.is_finite() && b.fract() != 0.0 {
        return Err(math_err(format!(
            "{a} ** {b} resultaria em número complexo"
        )));
    }
    let result = a.powf(b);
    if result.is_infinite() && a.is_finite() && b.is_finite() {
        return Err(math_err(format!("overflow em {a} ** {b}")));
    }
    Ok(result)
}

/// `round(x)` do CPython: metade para o par, exato sobre o double.
fn round_half_even(x: f64) -> f64 {
    let rounded = x.round();
    if (x - rounded).abs() == 0.5 {
        2.0 * (x / 2.0).round()
    } else {
        rounded
    }
}

/// `round(x)` / `round(x, ndigits)` do Python.
///
/// Sem `ndigits`, devolve o inteiro mais próximo (metade para o par); `inf` e
/// NaN são erro, como a conversão para `int` no Python. Com `ndigits` (que
/// precisa ser inteiro), arredonda a representação **decimal exata** do
/// double, metade para o par — `round(2.675, 2) == 2.67` porque 2.675 é
/// 2.67499999... — via formatação com precisão fixa (exata em Rust) para
/// `ndigits >= 0`; para `ndigits < 0` usa escala por potência de 10
/// (aproximação adequada para valores usuais). Como no CPython, `ndigits`
/// acima de 323 devolve `x` e abaixo de -308 devolve `0.0 * x`.
fn py_round(x: f64, ndigits: Option<f64>) -> Result<f64, FormulaError> {
    let Some(n) = ndigits else {
        if x.is_nan() {
            return Err(math_err("round(): NaN não pode ser convertido em inteiro"));
        }
        if x.is_infinite() {
            return Err(math_err(
                "round(): infinito não pode ser convertido em inteiro",
            ));
        }
        return Ok(round_half_even(x));
    };
    if !n.is_finite() || n.fract() != 0.0 {
        return Err(math_err(format!(
            "round(): ndigits deve ser inteiro, obtido {n}"
        )));
    }
    if !x.is_finite() {
        return Ok(x);
    }
    if n > 323.0 {
        return Ok(x);
    }
    if n < -308.0 {
        return Ok(0.0 * x);
    }
    if n >= 0.0 {
        let text = format!("{:.*}", n as usize, x);
        return text
            .parse::<f64>()
            .map_err(|_| math_err(format!("round(): falha ao reler {text:?}")));
    }
    let scale = 10f64.powi(-n as i32);
    Ok(round_half_even(x / scale) * scale)
}

/// Funções embutidas. As do grupo "básico" são aceitas com ou sem `math.`
/// (os locais do Python expõem `abs`, `round`, `min`, `max`, `sin`, `cos`,
/// `tan`, `ceil`, `floor` sem prefixo e `math` como módulo); as demais só com
/// `math.`, como no Python, onde não existem como builtin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Builtin {
    Max,
    Min,
    Abs,
    Round,
    Sqrt,
    Sin,
    Cos,
    Tan,
    Ceil,
    Floor,
    Pow,
    Exp,
    Log,
    Log10,
    Log2,
    Fabs,
    Radians,
    Degrees,
    Asin,
    Acos,
    Atan,
    Atan2,
    Hypot,
    Trunc,
}

impl Builtin {
    fn lookup(name: &str) -> Option<Builtin> {
        let (bare, prefixed) = match name.strip_prefix("math.") {
            Some(rest) => (rest, true),
            None => (name, false),
        };
        match (bare, prefixed) {
            ("max", _) => Some(Builtin::Max),
            ("min", _) => Some(Builtin::Min),
            ("abs", _) => Some(Builtin::Abs),
            ("round", _) => Some(Builtin::Round),
            ("sqrt", _) => Some(Builtin::Sqrt),
            ("sin", _) => Some(Builtin::Sin),
            ("cos", _) => Some(Builtin::Cos),
            ("tan", _) => Some(Builtin::Tan),
            ("ceil", _) => Some(Builtin::Ceil),
            ("floor", _) => Some(Builtin::Floor),
            ("pow", true) => Some(Builtin::Pow),
            ("exp", true) => Some(Builtin::Exp),
            ("log", true) => Some(Builtin::Log),
            ("log10", true) => Some(Builtin::Log10),
            ("log2", true) => Some(Builtin::Log2),
            ("fabs", true) => Some(Builtin::Fabs),
            ("radians", true) => Some(Builtin::Radians),
            ("degrees", true) => Some(Builtin::Degrees),
            ("asin", true) => Some(Builtin::Asin),
            ("acos", true) => Some(Builtin::Acos),
            ("atan", true) => Some(Builtin::Atan),
            ("atan2", true) => Some(Builtin::Atan2),
            ("hypot", true) => Some(Builtin::Hypot),
            ("trunc", true) => Some(Builtin::Trunc),
            _ => None,
        }
    }

    /// (mínimo, máximo) de argumentos; `None` = sem limite superior.
    fn arity(self) -> (usize, Option<usize>) {
        match self {
            Builtin::Max | Builtin::Min => (1, None),
            Builtin::Round | Builtin::Log => (1, Some(2)),
            Builtin::Pow | Builtin::Atan2 | Builtin::Hypot => (2, Some(2)),
            _ => (1, Some(1)),
        }
    }

    fn call(self, name: &str, args: &[f64]) -> Result<f64, FormulaError> {
        let (min, max) = self.arity();
        if args.len() < min || max.is_some_and(|m| args.len() > m) {
            let expected = match max {
                None => format!("pelo menos {min}"),
                Some(m) if m == min => format!("exatamente {min}"),
                Some(m) => format!("entre {min} e {m}"),
            };
            return Err(FormulaError::Arity(format!(
                "{name}() recebeu {} argumento(s); esperado {expected}",
                args.len()
            )));
        }
        let x = args[0];
        match self {
            // `max`/`min` do Python: só troca quando a comparação é verdadeira,
            // logo o primeiro vence em empate e um NaN inicial se propaga.
            Builtin::Max => Ok(args[1..]
                .iter()
                .fold(x, |acc, &v| if v > acc { v } else { acc })),
            Builtin::Min => Ok(args[1..]
                .iter()
                .fold(x, |acc, &v| if v < acc { v } else { acc })),
            Builtin::Abs | Builtin::Fabs => Ok(x.abs()),
            Builtin::Round => py_round(x, args.get(1).copied()),
            Builtin::Sqrt => math_1(name, x, f64::sqrt),
            Builtin::Sin => math_1(name, x, f64::sin),
            Builtin::Cos => math_1(name, x, f64::cos),
            Builtin::Tan => math_1(name, x, f64::tan),
            Builtin::Ceil => to_integral(name, x).map(f64::ceil),
            Builtin::Floor => to_integral(name, x).map(f64::floor),
            Builtin::Trunc => to_integral(name, x).map(f64::trunc),
            Builtin::Pow => py_pow(x, args[1]),
            Builtin::Exp => math_1(name, x, f64::exp),
            Builtin::Log => match args.get(1) {
                None => math_1(name, x, f64::ln),
                Some(&base) => {
                    let num = math_1(name, x, f64::ln)?;
                    let den = math_1(name, base, f64::ln)?;
                    if den == 0.0 {
                        return Err(math_err(format!("{name}(): base {base} inválida")));
                    }
                    Ok(num / den)
                }
            },
            Builtin::Log10 => math_1(name, x, f64::log10),
            Builtin::Log2 => math_1(name, x, f64::log2),
            Builtin::Radians => Ok(x.to_radians()),
            Builtin::Degrees => Ok(x.to_degrees()),
            Builtin::Asin => math_1(name, x, f64::asin),
            Builtin::Acos => math_1(name, x, f64::acos),
            Builtin::Atan => math_1(name, x, f64::atan),
            Builtin::Atan2 => Ok(x.atan2(args[1])),
            Builtin::Hypot => {
                let result = x.hypot(args[1]);
                if result.is_infinite() && x.is_finite() && args[1].is_finite() {
                    return Err(math_err(format!("overflow em {name}()")));
                }
                Ok(result)
            }
        }
    }
}

/// `math_1` do CPython (`mathmodule.c`): resultado NaN a partir de entrada
/// não NaN é erro de domínio (`ValueError`); resultado infinito a partir de
/// entrada finita é erro de domínio ou overflow (`OverflowError`).
fn math_1(name: &str, x: f64, f: fn(f64) -> f64) -> Result<f64, FormulaError> {
    let result = f(x);
    if result.is_nan() && !x.is_nan() {
        return Err(math_err(format!("{name}({x}): erro de domínio")));
    }
    if result.is_infinite() && x.is_finite() {
        return Err(math_err(format!(
            "{name}({x}): erro de domínio ou overflow"
        )));
    }
    Ok(result)
}

/// `math.ceil`/`floor`/`trunc` devolvem `int` no Python: infinito é
/// `OverflowError` e NaN é `ValueError`.
fn to_integral(name: &str, x: f64) -> Result<f64, FormulaError> {
    if x.is_nan() {
        Err(math_err(format!(
            "{name}(): NaN não pode ser convertido em inteiro"
        )))
    } else if x.is_infinite() {
        Err(math_err(format!(
            "{name}(): infinito não pode ser convertido em inteiro"
        )))
    } else {
        Ok(x)
    }
}
