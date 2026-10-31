//! Testes de integração de `tfm_core::formula`.
//!
//! Os valores esperados vêm do Python original: `eval()` com
//! `{'__builtins__': {}}` sobre `build_formula_locals(params, costs)`
//! (`formula_context.py`) e o laço de 6 rodadas de
//! `_evaluate_formulas_runtime` (TFM_GCODE.py). Valores que dependem só de
//! `+ - * /` e `math.pi` são comparados com `assert_eq!` (idênticos bit a bit);
//! os que passam por `asin` (modelo de sobreposição) usam tolerância 1e-9.
//! O `config/config.json` do repositório é apenas lido (`AppConfig::read`).

use std::path::Path;

use indexmap::IndexMap;
use tfm_core::config::AppConfig;
use tfm_core::formula::{eval, evaluate_block, formula_locals, parse, Expr, FormulaError, Op};
use tfm_core::params::Params;

const REPO_CONFIG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/config.json");

type Vars<'a> = &'a dyn Fn(&str) -> Option<f64>;

fn ev_with(src: &str, vars: Vars) -> Result<f64, FormulaError> {
    eval(&parse(src)?, vars)
}

fn ev(src: &str) -> Result<f64, FormulaError> {
    ev_with(src, &|_| None)
}

fn ok(src: &str) -> f64 {
    match ev(src) {
        Ok(v) => v,
        Err(e) => panic!("{src:?}: esperado sucesso, obtido {e}"),
    }
}

fn exprs(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

fn locals(pairs: &[(&str, f64)]) -> IndexMap<String, f64> {
    pairs.iter().map(|(k, v)| ((*k).to_string(), *v)).collect()
}

fn assert_close(actual: f64, expected: f64, tol: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tol,
        "{what}: esperado {expected:?}, obtido {actual:?} (tol {tol:e})"
    );
}

fn repo_config() -> AppConfig {
    assert!(
        Path::new(REPO_CONFIG).exists(),
        "{REPO_CONFIG} deve existir"
    );
    AppConfig::read(Path::new(REPO_CONFIG))
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

#[test]
fn parses_python_formula() {
    let e = parse("(velocidade_de_deposicao / max(circumference_mm, 1e-6)) * 60").expect("parse");
    let expected = Expr::Binary(
        Op::Mul,
        Box::new(Expr::Binary(
            Op::Div,
            Box::new(Expr::Var("velocidade_de_deposicao".to_string())),
            Box::new(Expr::Call(
                "max".to_string(),
                vec![Expr::Var("circumference_mm".to_string()), Expr::Num(1e-6)],
            )),
        )),
        Box::new(Expr::Num(60.0)),
    );
    assert_eq!(e, expected);

    // `math.pi` é um único identificador com ponto.
    let e = parse("math.pi * diametro").expect("parse");
    assert_eq!(
        e,
        Expr::Binary(
            Op::Mul,
            Box::new(Expr::Var("math.pi".to_string())),
            Box::new(Expr::Var("diametro".to_string())),
        )
    );

    // Unário e chamada com prefixo.
    let e = parse("-math.ceil(x)").expect("parse");
    assert_eq!(
        e,
        Expr::Unary(
            Op::Neg,
            Box::new(Expr::Call(
                "math.ceil".to_string(),
                vec![Expr::Var("x".to_string())]
            )),
        )
    );
}

#[test]
fn math_prefix_and_bare() {
    assert_eq!(ok("math.ceil(2.1)"), 3.0);
    assert_eq!(ok("ceil(2.1)"), 3.0);
    assert_eq!(ok("math.floor(-2.1)"), -3.0);
    assert_eq!(ok("floor(-2.1)"), -3.0);
    assert_eq!(ok("math.sqrt(16)"), 4.0);
    assert_eq!(ok("sqrt(16)"), 4.0);
    assert_eq!(ok("abs(-3.5)"), 3.5);
    assert_eq!(ok("math.abs(-3.5)"), 3.5);
    assert_eq!(ok("math.sin(0)"), 0.0);
    assert_eq!(ok("cos(0)"), 1.0);
    assert_eq!(ok("math.tan(0)"), 0.0);
    assert_eq!(ok("math.pi"), std::f64::consts::PI);
    assert_eq!(ok("pi"), std::f64::consts::PI);
    assert_eq!(ok("math.e"), std::f64::consts::E);
    assert_eq!(ok("e"), std::f64::consts::E);
    assert_eq!(ok("math.pi * 2"), ok("pi * 2"));
}

#[test]
fn power_and_floordiv() {
    assert_eq!(ok("2**10"), 1024.0);
    assert_eq!(ok("7 // 2"), 3.0);
    assert_eq!(ok("-7 // 2"), -4.0);
    assert_eq!(ok("-7.5 // 2"), -4.0);
    assert_eq!(ok("7.5 // -2"), -4.0);
    assert_eq!(ok("2**-1"), 0.5);
    assert_eq!(ok("2 ** 0.5"), std::f64::consts::SQRT_2);
    assert_eq!(ok("0 ** 0"), 1.0);
}

#[test]
fn precedence_matches_python() {
    assert_eq!(ok("2 + 3 * 4"), 14.0);
    assert_eq!(ok("-2**2"), -4.0);
    assert_eq!(ok("2**3**2"), 512.0);
    assert_eq!(ok("2**-1**2"), 0.5);
    assert_eq!(ok("-(2 + 3) * 4"), -20.0);
    assert_eq!(ok("--2"), 2.0);
    assert_eq!(ok("+2"), 2.0);
    assert_eq!(ok("2 * -3"), -6.0);
    assert_eq!(ok("1 - 2 - 3"), -4.0);
    assert_eq!(ok("8 / 2 / 2"), 2.0);
    assert_eq!(ok("2 * 3 % 4"), 2.0);
    assert_eq!(ok("(1 + 2) * (3 + 4)"), 21.0);
    assert_eq!(ok("  2\t+\n3  "), 5.0);
}

#[test]
fn modulo_takes_sign_of_divisor() {
    assert_eq!(ok("-7 % 3"), 2.0);
    assert_eq!(ok("7.5 % -2"), -0.5);
    assert_eq!(ok("5.5 % 2.5"), 0.5);
    assert_eq!(ok("-5.5 % 2.5"), 2.0);
    assert_eq!(ok("10 % 3"), 1.0);
    assert_eq!(ok("6 % 3"), 0.0);
}

#[test]
fn division_by_zero_is_math_error() {
    for src in [
        "1 / 0",
        "1 // 0",
        "1 % 0",
        "0 / 0",
        "1.5 / (2 - 2)",
        "0 ** -1",
    ] {
        assert!(
            matches!(ev(src), Err(FormulaError::Math(_))),
            "{src:?} deveria dar FormulaError::Math, obtido {:?}",
            ev(src)
        );
    }
}

#[test]
fn scientific_and_fractional_literals() {
    assert_eq!(ok("1e-6"), 1e-6);
    assert_eq!(ok("1.5E3"), 1500.0);
    assert_eq!(ok("1E+2"), 100.0);
    assert_eq!(ok(".5 + 5."), 5.5);
    assert_eq!(ok("1.e2"), 100.0);
    assert_eq!(ok("1_000"), 1000.0);
    assert_eq!(ok("0.1 + 0.2"), 0.1 + 0.2);
    assert_eq!(ok("1e-6 * 1e6"), 1e-6 * 1e6);
}

// ---------------------------------------------------------------------------
// Funções e nomes
// ---------------------------------------------------------------------------

#[test]
fn round_is_half_to_even() {
    assert_eq!(ok("round(2.5)"), 2.0);
    assert_eq!(ok("round(0.5)"), 0.0);
    assert_eq!(ok("round(1.5)"), 2.0);
    assert_eq!(ok("round(-0.5)"), 0.0);
    assert_eq!(ok("round(-1.5)"), -2.0);
    assert_eq!(ok("round(2.4)"), 2.0);
    assert_eq!(ok("round(2.6)"), 3.0);
    // Com `ndigits`, o Python arredonda o valor decimal exato do double:
    // 2.675 é na verdade 2.67499999... -> 2.67; 0.125 e 0.375 são exatos.
    assert_eq!(ok("round(2.675, 2)"), 2.67);
    assert_eq!(ok("round(0.125, 2)"), 0.12);
    assert_eq!(ok("round(0.375, 2)"), 0.38);
    assert_eq!(ok("round(1.23456, 3)"), 1.235);
    assert_eq!(ok("round(1250.0, -2)"), 1200.0);
    assert_eq!(ok("round(1350.0, -2)"), 1400.0);
    assert_eq!(ok("round(7, 0)"), 7.0);
    // `ndigits` precisa ser inteiro (TypeError no Python).
    assert!(matches!(ev("round(1.5, 0.5)"), Err(FormulaError::Math(_))));
}

#[test]
fn max_min_python_semantics() {
    assert_eq!(ok("max(3, 7, 5)"), 7.0);
    assert_eq!(ok("min(3, 7, 5)"), 3.0);
    assert_eq!(ok("max(1, 2)"), 2.0);
    assert_eq!(ok("min(1, 2)"), 1.0);
    assert_eq!(ok("max(5)"), 5.0);
    assert_eq!(ok("max(-1, -1e-6)"), -1e-6);
    // Propagação de NaN como no Python (`max` compara com `>`: o primeiro
    // argumento vence quando a comparação é falsa).
    let nan = |name: &str| (name == "nan").then_some(f64::NAN);
    assert!(ev_with("max(nan, 1)", &nan).unwrap().is_nan());
    assert_eq!(ev_with("max(1, nan)", &nan).unwrap(), 1.0);
    assert!(ev_with("min(nan, 1)", &nan).unwrap().is_nan());
    assert_eq!(ev_with("min(1, nan)", &nan).unwrap(), 1.0);
}

#[test]
fn variables_come_from_closure_then_constants() {
    let vars = |name: &str| match name {
        "diametro" => Some(50.0),
        "pi" => Some(3.0),
        _ => None,
    };
    assert_eq!(ev_with("diametro * 2", &vars).unwrap(), 100.0);
    // A closure tem prioridade sobre a tabela de constantes.
    assert_eq!(ev_with("pi", &vars).unwrap(), 3.0);
    assert_eq!(ev_with("math.pi", &vars).unwrap(), std::f64::consts::PI);
    assert_eq!(ev_with("x", &|_| Some(1.5)).unwrap(), 1.5);
}

#[test]
fn unknown_names_and_functions() {
    assert_eq!(
        ev("xyz + 1"),
        Err(FormulaError::UnknownName("xyz".to_string()))
    );
    assert_eq!(
        ev("foo(1)"),
        Err(FormulaError::UnknownName("foo".to_string()))
    );
    assert_eq!(
        ev("math.nope(1)"),
        Err(FormulaError::UnknownName("math.nope".to_string()))
    );
    assert_eq!(
        ev("a.b * 2"),
        Err(FormulaError::UnknownName("a.b".to_string()))
    );
    // Como no Python, o nome da função é resolvido antes dos argumentos.
    assert_eq!(
        ev("foo(1/0)"),
        Err(FormulaError::UnknownName("foo".to_string()))
    );
}

#[test]
fn arity_errors() {
    for src in [
        "sqrt(1, 2)",
        "max()",
        "min()",
        "round(1, 2, 3)",
        "abs()",
        "ceil(1, 2)",
        "round()",
    ] {
        assert!(
            matches!(ev(src), Err(FormulaError::Arity(_))),
            "{src:?} deveria dar FormulaError::Arity, obtido {:?}",
            ev(src)
        );
    }
}

#[test]
fn math_domain_errors() {
    let inf = |name: &str| (name == "inf").then_some(f64::INFINITY);
    let cases: [(&str, Vars); 7] = [
        ("sqrt(-1)", &|_| None),
        ("math.sqrt(-4)", &|_| None),
        ("(-8) ** 0.5", &|_| None),
        ("10.0 ** 400", &|_| None),
        ("math.ceil(inf)", &inf),
        ("floor(inf)", &inf),
        ("round(inf)", &inf),
    ];
    for (src, vars) in cases {
        let got = ev_with(src, vars);
        assert!(
            matches!(got, Err(FormulaError::Math(_))),
            "{src:?} deveria dar FormulaError::Math, obtido {got:?}"
        );
    }
    // Operações que não levantam erro no Python continuam sem erro.
    assert!(ev_with("inf - inf", &inf).unwrap().is_nan());
    assert!(ev_with("inf * 2", &inf).unwrap().is_infinite());
    assert_eq!(ok("1 ** 1000000"), 1.0);
}

#[test]
fn lexer_rejects_garbage() {
    for src in [
        "2 $ 3",
        "2 +",
        "",
        "   ",
        "(2",
        "2)",
        "2 3",
        "1e",
        "math.",
        "a == b",
        "a < b",
        "max(1,",
        "max(1 2)",
        "2 ** ** 3",
        "x = 1",
        "'abc'",
        "2 ! 3",
        "[1, 2]",
        "a.1",
        "1..2",
        ",",
        "()",
    ] {
        let got = parse(src);
        assert!(
            matches!(got, Err(FormulaError::Parse(_))),
            "{src:?} deveria dar FormulaError::Parse, obtido {got:?}"
        );
    }
    // Vírgula final em chamada é aceita, como no Python.
    assert_eq!(ok("max(1, 2,)"), 2.0);
    // Aninhamento absurdo é rejeitado, nunca estoura a pilha.
    let deep = format!("{}1{}", "(".repeat(100_000), ")".repeat(100_000));
    assert!(matches!(parse(&deep), Err(FormulaError::Parse(_))));
    let minus = format!("{}1", "-".repeat(100_000));
    assert!(matches!(parse(&minus), Err(FormulaError::Parse(_))));
    // Cadeias associativas à esquerda não recursam no parser, mas a árvore
    // cresce um nível por termo (eval e Drop são recursivos nela): também
    // precisam ser rejeitadas, e rápido.
    let chain = format!("{}1", "1+".repeat(100_000));
    assert!(matches!(parse(&chain), Err(FormulaError::Parse(_))));
    let products = format!("{}1", "2*".repeat(100_000));
    assert!(matches!(parse(&products), Err(FormulaError::Parse(_))));
    let powers = format!("{}1", "2**".repeat(100_000));
    assert!(matches!(parse(&powers), Err(FormulaError::Parse(_))));
}

#[test]
fn tree_height_limit_is_256() {
    // 256 termos -> altura 256: aceito, avaliado e descartado sem problema.
    let max_ok = format!("{}1", "1+".repeat(255));
    assert_eq!(ok(&max_ok), 256.0);
    // 257 termos -> altura 257: rejeitado com mensagem que cita o limite.
    let too_deep = format!("{}1", "1+".repeat(256));
    match parse(&too_deep) {
        Err(FormulaError::Parse(msg)) => assert!(msg.contains("256"), "{msg}"),
        other => panic!("esperado FormulaError::Parse, obtido {other:?}"),
    }
    // Chamadas e unários aninhados contam níveis do mesmo jeito.
    let calls_ok = format!("{}1{}", "abs(".repeat(200), ")".repeat(200));
    assert_eq!(ok(&calls_ok), 1.0);
    let calls_too_deep = format!("{}1{}", "max(".repeat(300), ")".repeat(300));
    assert!(matches!(
        parse(&calls_too_deep),
        Err(FormulaError::Parse(_))
    ));
    let unary_ok = format!("{}1", "-".repeat(200));
    assert_eq!(ok(&unary_ok), 1.0);
    // Altura é a do ramo mais profundo: muitos argumentos rasos não contam.
    let wide = format!("max({})", vec!["1"; 5_000].join(", "));
    assert_eq!(ok(&wide), 1.0);
}

#[test]
fn errors_are_reported_left_to_right() {
    // O Python avalia da esquerda para a direita: o primeiro erro vence.
    assert_eq!(
        ev("xyz + 1/0"),
        Err(FormulaError::UnknownName("xyz".to_string()))
    );
    assert!(matches!(ev("1/0 + xyz"), Err(FormulaError::Math(_))));
    assert!(matches!(ev("max(1/0, xyz)"), Err(FormulaError::Math(_))));
    assert_eq!(
        ev("max(xyz, 1/0)"),
        Err(FormulaError::UnknownName("xyz".to_string()))
    );
    // Argumentos são avaliados antes da checagem de aridade (TypeError depois).
    assert!(matches!(ev("sqrt(1/0, 2)"), Err(FormulaError::Math(_))));
}

// ---------------------------------------------------------------------------
// evaluate_block
// ---------------------------------------------------------------------------

#[test]
fn dependency_resolution_order_independent() {
    let out = evaluate_block(&exprs(&[("b", "a * 2"), ("a", "3")]), &locals(&[]));
    assert_eq!(out.get("b"), Some(&Some(6.0)));
    assert_eq!(out.get("a"), Some(&Some(3.0)));
    // A saída segue a ordem ORIGINAL das expressões, não a de resolução.
    assert_eq!(out.keys().collect::<Vec<_>>(), vec!["b", "a"]);
}

#[test]
fn error_isolated_to_key() {
    let out = evaluate_block(
        &exprs(&[
            ("bad", "1/0"),
            ("good", "2 + 2"),
            ("dep", "bad * 2"),
            ("syntax", "2 +"),
            ("missing", "nope + 1"),
        ]),
        &locals(&[]),
    );
    assert_eq!(out.get("bad"), Some(&None));
    assert_eq!(out.get("good"), Some(&Some(4.0)));
    // No Python, `bad` vale None e `None * 2` levanta TypeError -> None.
    assert_eq!(out.get("dep"), Some(&None));
    assert_eq!(out.get("syntax"), Some(&None));
    // Nome nunca resolvido fica pendente até o fim das rodadas -> None.
    assert_eq!(out.get("missing"), Some(&None));
    assert_eq!(out.len(), 5);
}

#[test]
fn locals_feed_expressions_and_results_shadow_them() {
    let loc = locals(&[("diametro", 50.0), ("x", 10.0)]);
    let out = evaluate_block(
        &exprs(&[
            ("circ", "math.pi * diametro"),
            ("y", "x + 1"),
            ("x", "x * 2"),
            ("z", "x + 1"),
        ]),
        &loc,
    );
    assert_eq!(out.get("circ"), Some(&Some(std::f64::consts::PI * 50.0)));
    // `y` é avaliado antes de `x` ser redefinido: usa o local (10).
    assert_eq!(out.get("y"), Some(&Some(11.0)));
    // `x` usa o local 10 ao ser definido; `z`, depois, vê o resultado (20),
    // como `{**safe_locals, **results}` no Python.
    assert_eq!(out.get("x"), Some(&Some(20.0)));
    assert_eq!(out.get("z"), Some(&Some(21.0)));
}

#[test]
fn six_rounds_limit_matches_python() {
    // Cadeia invertida de 7 chaves: cada rodada resolve exatamente uma a mais.
    // Em 6 rodadas, k1..k6 resolvem e k7 continua pendente -> None.
    let chain7 = exprs(&[
        ("k7", "k6 + 1"),
        ("k6", "k5 + 1"),
        ("k5", "k4 + 1"),
        ("k4", "k3 + 1"),
        ("k3", "k2 + 1"),
        ("k2", "k1 + 1"),
        ("k1", "1"),
    ]);
    let out = evaluate_block(&chain7, &locals(&[]));
    assert_eq!(out.get("k1"), Some(&Some(1.0)));
    assert_eq!(out.get("k6"), Some(&Some(6.0)));
    assert_eq!(out.get("k7"), Some(&None));

    // A mesma cadeia em ordem direta resolve tudo na primeira rodada, porque
    // um resultado fica visível imediatamente para as chaves seguintes.
    let mut forward: Vec<(String, String)> =
        chain7.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    forward.reverse();
    let forward: IndexMap<String, String> = forward.into_iter().collect();
    let out = evaluate_block(&forward, &locals(&[]));
    assert!(out.values().all(|v| v.is_some()), "{out:?}");
    assert_eq!(out.get("k7"), Some(&Some(7.0)));

    // Dependência circular nunca progride: para cedo, ambas None.
    let out = evaluate_block(&exprs(&[("a", "b + 1"), ("b", "a + 1")]), &locals(&[]));
    assert_eq!(out.get("a"), Some(&None));
    assert_eq!(out.get("b"), Some(&None));

    // Bloco vazio -> mapa vazio.
    assert!(evaluate_block(&exprs(&[]), &locals(&[])).is_empty());
}

#[test]
fn pending_key_fails_later_when_dependency_errors() {
    // Rodada 1: `b` pendente (a desconhecido), `a` -> 1. Rodada 2: `b` -> 1/0 -> None.
    let out = evaluate_block(&exprs(&[("b", "a + 1/0"), ("a", "1")]), &locals(&[]));
    assert_eq!(out.get("a"), Some(&Some(1.0)));
    assert_eq!(out.get("b"), Some(&None));
}

// ---------------------------------------------------------------------------
// formula_locals
// ---------------------------------------------------------------------------

#[test]
fn formula_locals_matches_python() {
    let cfg = repo_config();
    let p = Params::default();
    let loc = formula_locals(&p, &cfg.costs);

    // Campos numéricos/bool de Params sob as chaves do Python, na ordem da struct.
    assert_eq!(loc.get_index(0), Some((&"diametro".to_string(), &50.0)));
    assert_eq!(
        loc.get_index(1),
        Some((&"diametro_interno".to_string(), &0.0))
    );
    assert_eq!(loc["comprimento_revestir"], 50.0);
    assert_eq!(loc["largura_cordao"], 8.0);
    assert_eq!(loc["sobreposicao"], 40.0);
    assert_eq!(loc["velocidade_de_deposicao"], 120.0);
    assert_eq!(loc["num_camadas"], 1.0); // i64 -> f64
    assert_eq!(loc["multiperfil_count"], 4.0);
    assert_eq!(loc["torch_retract_on_ignite"], 1.0); // bool -> 1.0
    assert_eq!(loc["smart_layers"], 0.0); // bool -> 0.0
    assert_eq!(loc["lead_in"], 0.0);
    assert_eq!(loc["taxa_deposicao_g_h"], 700.0);
    // `taxa_de_deposicao` é None por padrão: ausente, como a chave ausente no Python.
    assert!(!loc.contains_key("taxa_de_deposicao"));
    // Enums e strings não são expostos.
    for k in [
        "nome_procedimento",
        "welding_mode",
        "direcao_soldagem",
        "powder_name",
        "flat_orientation",
    ] {
        assert!(!loc.contains_key(k), "{k} não deveria estar nos locais");
    }
    // Funções não são locais (são embutidas no avaliador).
    for k in [
        "math", "max", "min", "abs", "round", "ceil", "floor", "sin", "cos", "tan", "sqrt",
    ] {
        assert!(!loc.contains_key(k), "{k} não deveria estar nos locais");
    }

    // Modelo de sobreposição: nominal 8, 40 %, camada 1.5, raio 25, alvo 50
    // (Python: build_formula_locals com os defaults da UI).
    assert_close(loc["step_over_mm"], 4.812765401236198, 1e-9, "step_over_mm");
    assert_close(
        loc["effective_width_mm"],
        8.021275668726997,
        1e-9,
        "effective_width_mm",
    );
    assert_eq!(loc["overlap_pass_count"], 10.0);
    assert_eq!(loc["pi"], std::f64::consts::PI);

    // Custos do config.json do repositório.
    assert_eq!(loc["gas_argon_brl_m3"], 50.0);
    assert_eq!(loc["labor_brl_hour"], 40.0);
    assert_eq!(loc["machine_brl_hour"], 250.0);
    // O valor dos params (450) tem prioridade sobre costs.powder_brl_kg (700).
    assert_eq!(cfg.costs.powder_brl_kg, 700.0);
    assert_eq!(loc["powder_cost_brl_kg"], 450.0);
    // `powder_cost_brl_kg` mantém a posição original (chave de Params).
    let idx_powder = loc.get_index_of("powder_cost_brl_kg").unwrap();
    let idx_step = loc.get_index_of("step_over_mm").unwrap();
    assert!(
        idx_powder < idx_step,
        "powder_cost_brl_kg deve preceder step_over_mm"
    );
    let tail: Vec<&str> = loc.keys().rev().take(7).map(String::as_str).collect();
    assert_eq!(
        tail,
        vec![
            "machine_brl_hour",
            "labor_brl_hour",
            "gas_argon_brl_m3",
            "pi",
            "overlap_pass_count",
            "effective_width_mm",
            "step_over_mm"
        ]
    );

    // `taxa_de_deposicao` presente quando Some.
    let mut p2 = p.clone();
    p2.taxa_de_deposicao = Some(99.0);
    assert_eq!(formula_locals(&p2, &cfg.costs)["taxa_de_deposicao"], 99.0);
}

#[test]
fn formula_locals_overlap_edge_cases() {
    let cfg = repo_config();

    // Superfície plana (diametro 0 -> raio None): sem asin, valores exatos do Python.
    let flat = Params {
        diametro: 0.0,
        ..Params::default()
    };
    let loc = formula_locals(&flat, &cfg.costs);
    assert_eq!(loc["step_over_mm"], 4.7920799999999995);
    assert_eq!(loc["effective_width_mm"], 7.9868);
    assert_eq!(loc["overlap_pass_count"], 10.0);

    // Largura de cordão zero e comprimento zero: o Python parte do fallback
    // 1e-6 e o modelo (nominal clampado em 1e-6, alvo None) devolve 1e-6 / 1 passe.
    let zero = Params {
        largura_cordao: 0.0,
        comprimento_revestir: 0.0,
        ..Params::default()
    };
    let loc = formula_locals(&zero, &cfg.costs);
    assert_eq!(loc["step_over_mm"], 1e-6);
    assert_eq!(loc["effective_width_mm"], 1e-6);
    assert_eq!(loc["overlap_pass_count"], 1.0);

    // O fator de calibração NÃO entra no modelo (build_formula_locals não o
    // repassa; compute_overlap_metrics usa o default 1.0).
    let cal = Params {
        bead_width_calibration_factor: 1.5,
        ..Params::default()
    };
    let loc_cal = formula_locals(&cal, &cfg.costs);
    let loc_ref = formula_locals(&Params::default(), &cfg.costs);
    assert_eq!(loc_cal["step_over_mm"], loc_ref["step_over_mm"]);
    assert_eq!(loc_cal["effective_width_mm"], loc_ref["effective_width_mm"]);
    assert_eq!(loc_cal["bead_width_calibration_factor"], 1.5);

    // Custos distintos chegam intactos.
    let mut costs = cfg.costs.clone();
    costs.gas_argon_brl_m3 = 1.0;
    costs.labor_brl_hour = 2.0;
    costs.machine_brl_hour = 3.0;
    let loc = formula_locals(&Params::default(), &costs);
    assert_eq!(
        (
            loc["gas_argon_brl_m3"],
            loc["labor_brl_hour"],
            loc["machine_brl_hour"]
        ),
        (1.0, 2.0, 3.0)
    );
}

// ---------------------------------------------------------------------------
// Bloco padrão do config.json
// ---------------------------------------------------------------------------

/// Valores do Python (`_evaluate_formulas_runtime`) para `Params::default()`
/// e os custos do `config/config.json` do repositório, na ordem do arquivo.
/// `effective_step_mm` depende de `asin` (tolerância); os demais são exatos.
const PYTHON_DEFAULT_RESULTS: [(&str, f64); 20] = [
    ("circumference_mm", 157.07963267948966),
    ("rotation_rpm", 45.83662361046586),
    ("rotation_mm_min", 7200.0),
    ("helix_pitch_mm", 2.6179938779914944),
    ("powder_mass_kg", 0.1884955592153876),
    ("powder_mass_g", 188.4955592153876),
    ("effective_step_mm", 4.812765401236198),
    ("axial_passes_per_layer", 11.0),
    ("total_length_mm_per_layer", 1727.8759594743863),
    ("total_length_mm_all_layers", 1727.8759594743863),
    ("time_min", 14.398966328953218),
    ("rotations_total", 11.0),
    ("gas_consumption_l", 215.98449493429828),
    ("taxa_deposicao_kg_h", 0.7),
    ("time_min_mass", 16.156762218461797),
    ("powder_cost_brl", 84.82300164692442),
    ("labor_cost_brl", 9.599310885968812),
    ("machine_cost_brl", 59.99569303730508),
    ("gas_cost_brl", 10.799224746714915),
    ("total_cost_brl", 165.21723031691323),
];

#[test]
fn default_config_formulas_all_evaluate() {
    let cfg = repo_config();
    assert_eq!(cfg.formulas.expressions.len(), 20);
    let loc = formula_locals(&Params::default(), &cfg.costs);
    let out = evaluate_block(&cfg.formulas.expressions, &loc);

    assert_eq!(out.len(), 20);
    // Mesma ordem das expressões do arquivo.
    assert_eq!(
        out.keys().collect::<Vec<_>>(),
        cfg.formulas.expressions.keys().collect::<Vec<_>>()
    );
    for (key, value) in &out {
        assert!(
            value.is_some(),
            "{key} deveria avaliar para Some, obtido None"
        );
    }
    for (key, expected) in PYTHON_DEFAULT_RESULTS {
        let got = out[key].expect("Some");
        if key == "effective_step_mm" {
            assert_close(got, expected, 1e-9, key);
        } else {
            assert_eq!(
                got, expected,
                "{key}: esperado {expected:?}, obtido {got:?}"
            );
        }
    }
}

#[test]
fn builtin_default_formulas_all_evaluate() {
    // As 20 expressões embutidas (`AppConfig::default_formulas`) têm o mesmo
    // texto do config.json, em outra ordem: todas resolvem igualmente.
    let cfg = repo_config();
    let formulas = AppConfig::default_formulas();
    assert_eq!(formulas.expressions.len(), 20);
    let loc = formula_locals(&Params::default(), &cfg.costs);
    let out = evaluate_block(&formulas.expressions, &loc);
    assert_eq!(out.len(), 20);
    for (key, expected) in PYTHON_DEFAULT_RESULTS {
        let got = out[key].unwrap_or_else(|| panic!("{key} deveria ser Some"));
        assert_close(got, expected, 1e-9, key);
    }
    assert_eq!(
        out.keys().collect::<Vec<_>>(),
        formulas.expressions.keys().collect::<Vec<_>>()
    );
}
