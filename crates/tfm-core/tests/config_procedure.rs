//! Testes de integração de `tfm_core::{config, procedure, audit, quality}`.
//!
//! Tudo roda em diretórios temporários (`tempfile`). O `config/config.json`
//! do repositório é apenas lido, nunca gravado, e só recebe asserções
//! estruturais; os arquivos de qualidade são fixtures próprias (nada em
//! `data/` é lido).
//! Os valores esperados vêm do Python original (`config_manager.py`,
//! `quality_manager.py`, `TFM_GCODE.py`).

use std::fs;
use std::path::Path;

use chrono::{Local, NaiveDate};
use serde_json::{json, Value};
use tfm_core::audit::{AuditLogger, SOFTWARE_VERSION};
use tfm_core::config::{AppConfig, Database, Formulas, MachineLimits};
use tfm_core::params::{
    BolachaTipo, Direction, Operation, OscillationType, Params, Rotation, WeldingMode,
};
use tfm_core::procedure::{load_procedure, save_procedure, ProcedureError};
use tfm_core::quality::{Maintenance, QualityManager};

const REPO_CONFIG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/config.json");

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).expect("ler arquivo")).expect("JSON válido")
}

fn write(path: &Path, text: &str) {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).expect("criar pasta");
    }
    fs::write(path, text).expect("gravar arquivo");
}

// ---------------------------------------------------------------------------
// config
// ---------------------------------------------------------------------------

#[test]
fn config_partial_file_fills_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    write(&path, r#"{"costs": {"labor_brl_hour": 99}}"#);

    let cfg = AppConfig::load(&path);

    // seção parcial: a chave informada vence, as demais vêm do default
    assert_eq!(cfg.costs.labor_brl_hour, 99.0);
    assert_eq!(cfg.costs.powder_brl_kg, 250.0);
    assert_eq!(cfg.costs.gas_argon_brl_m3, 50.0);
    assert_eq!(cfg.costs.machine_brl_hour, 30.0);
    assert_eq!(cfg.costs.currency_symbol, "R$");
    assert!(cfg.costs.extra.is_empty());

    // seções ausentes: defaults do config_manager.py
    assert_eq!(cfg.powders.len(), 1);
    assert_eq!(cfg.powders[0].name, "Pó padrão");
    assert_eq!(cfg.powders[0].density_factor_g_mm2, 0.16);
    assert_eq!(cfg.powders[0].cost_brl_kg, 250.0);
    assert_eq!(cfg.active_powder, "Pó padrão");
    assert_eq!(cfg.integration.mach3_path, "");
    assert_eq!(cfg.integration.gcode_dir, "C:/Mach3/GCode");
    assert!(cfg.integration.write_autoload_txt);
    assert!(!cfg.integration.allow_file_association_fallback);
    assert_eq!(cfg.integration.mach3_profile, "PTA");
    assert_eq!(cfg.database.procedures_path, "procedures");
    assert_eq!(cfg.active_machine_profile, "default");
    assert_eq!(cfg.machine_profiles.len(), 1);
    assert_eq!(cfg.machine_profiles["default"].name, "Padrão");
    assert_eq!(
        cfg.active_limits(),
        MachineLimits {
            max_rpm_chuck: 100.0,
            max_length_x: 1500.0,
            max_diameter: 500.0,
            max_feed_linear: 5000.0,
        }
    );
    assert_eq!(cfg.ui.theme, "auto");
    assert_eq!(cfg.ui.left_w, 360.0);
    assert_eq!(cfg.ui.right_w, 390.0);
    assert!(cfg.ui.left_pinned);
    assert!(cfg.ui.right_pinned);
    assert!(!cfg.ui.bottom_collapsed);
    assert_eq!(cfg.ui.dpi_scaling, "auto");
    assert_eq!(cfg.ui.compact_threshold_px, 560);
    assert_eq!(cfg.formulas, Formulas::default());
    assert_eq!(cfg.formulas.expressions.len(), 20);
}

#[test]
fn config_missing_file_saves_full_structure() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub").join("config.json");
    assert!(!path.exists());

    let cfg = AppConfig::load(&path);

    assert_eq!(cfg, AppConfig::default());
    assert!(
        path.exists(),
        "load grava o arquivo, como ConfigManager.load()"
    );
    let text = fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "ui",
            "costs",
            "powders",
            "active_powder",
            "formulas",
            "database",
            "integration",
            "machine_profiles",
            "active_machine_profile",
        ]
    );
    assert_eq!(v["formulas"]["expressions"].as_object().unwrap().len(), 20);
    assert_eq!(v["powders"].as_array().unwrap().len(), 1);
    assert_eq!(
        v["machine_profiles"]["default"]["limits"]["max_feed_linear"],
        json!(5000.0)
    );
    assert_eq!(v["integration"]["write_autoload_txt"], json!(true));
    assert!(
        text.contains("\n    \"costs\": {\n        \"powder_brl_kg\": 250.0,\n"),
        "indent 4 como json.dump(indent=4):\n{text}"
    );
    assert!(!text.ends_with('\n'), "json.dump não escreve newline final");
    // o Python garante a pasta de procedimentos ao salvar: relativa à raiz do
    // app (`root_dir / proc_path`), não à pasta do config.json
    assert!(!dir.path().join("sub").join("procedures").exists());
    assert!(
        cfg.procedures_dir().is_dir(),
        "{}",
        cfg.procedures_dir().display()
    );
}

#[test]
fn repo_config_json_round_trips() {
    let original = fs::read_to_string(REPO_CONFIG).expect("config/config.json do repositório");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    write(&path, &original);

    let cfg = AppConfig::load(&path);

    // Só asserções estruturais: o conteúdo é do usuário e muda pelo app.
    assert_eq!(cfg.formulas.expressions.len(), 20);
    assert_eq!(cfg.powders.len(), 9);

    // O arquivo regravado tem as nove seções e mantém todas as chaves e
    // valores do usuário (inclusive extras de `costs` e `ui.sashes`).
    let saved = read_json(&path);
    let saved_keys: Vec<&str> = saved
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        saved_keys,
        [
            "ui",
            "costs",
            "powders",
            "active_powder",
            "formulas",
            "database",
            "integration",
            "machine_profiles",
            "active_machine_profile",
        ]
    );
    let orig: Value = serde_json::from_str(&original).unwrap();
    for (key, value) in orig.as_object().unwrap() {
        if key == "ui" {
            for (ui_key, ui_value) in value.as_object().unwrap() {
                assert_eq!(&saved["ui"][ui_key], ui_value, "ui.{ui_key} preservado");
            }
        } else {
            assert_eq!(&saved[key], value, "seção '{key}' preservada");
        }
    }
    // Ordem das chaves do usuário preservada (IndexMap / preserve_order).
    let orig_powders: Vec<&Value> = orig["powders"].as_array().unwrap().iter().collect();
    let saved_powders: Vec<&Value> = saved["powders"].as_array().unwrap().iter().collect();
    assert_eq!(saved_powders, orig_powders);
    let orig_costs: Vec<&String> = orig["costs"].as_object().unwrap().keys().collect();
    let saved_costs: Vec<&String> = saved["costs"].as_object().unwrap().keys().collect();
    assert_eq!(saved_costs, orig_costs);
}

#[test]
fn default_formulas_match_python_get_default_formulas() {
    let f = AppConfig::default_formulas();
    assert_eq!(f.expressions.len(), 20);
    let keys: Vec<&str> = f.expressions.keys().map(String::as_str).collect();
    assert_eq!(keys[0], "circumference_mm");
    assert_eq!(keys[4], "effective_step_mm");
    assert_eq!(keys[19], "total_cost_brl");
    assert_eq!(f.expressions["circumference_mm"], "math.pi * diametro");
    assert_eq!(
        f.expressions["powder_mass_kg"],
        "((circumference_mm * (comprimento_revestir + lead_in + lead_out) / 10000) * (num_camadas * espessura_camada) * powder_factor)"
    );
    assert!(f
        .notes
        .starts_with("Variáveis disponíveis: diametro, comprimento_revestir"));
    assert!(f
        .notes
        .ends_with("Consumo de pó calculado por área e espessura * fator."));
    // O config.json do repositório contém exatamente essas 20 expressões.
    let repo: Value = serde_json::from_str(&fs::read_to_string(REPO_CONFIG).unwrap()).unwrap();
    let repo_expr = repo["formulas"]["expressions"].as_object().unwrap();
    assert_eq!(repo_expr.len(), f.expressions.len());
    for (k, expr) in &f.expressions {
        assert_eq!(repo_expr[k], json!(expr), "expressão '{k}' igual ao Python");
    }
}

#[test]
fn config_resolve_path_in_debug_points_to_workspace_config() {
    let path = AppConfig::resolve_path();
    assert!(
        path.ends_with(Path::new("config").join("config.json")),
        "{}",
        path.display()
    );
    if cfg!(debug_assertions) {
        assert!(path.is_absolute());
        assert!(
            path.exists(),
            "{} deve ser o config/config.json do repositório",
            path.display()
        );
        assert!(
            !path.to_string_lossy().starts_with(r"\\?\"),
            "sem prefixo verbatim: {}",
            path.display()
        );
        // mesma coisa que canonicalize() devolve, só sem o prefixo verbatim
        let canon = path.canonicalize().unwrap().to_string_lossy().into_owned();
        assert_eq!(canon.trim_start_matches(r"\\?\"), path.to_string_lossy());
    }
}

#[test]
fn procedures_dir_is_relative_to_app_root() {
    // Python: `root_dir / proc_path`, com root_dir = pasta do exe (congelado)
    // ou raiz do projeto (dev) — nunca a pasta que contém o config.json.
    let root = AppConfig::app_root();
    assert!(root.is_absolute(), "{}", root.display());
    assert_eq!(
        root.join("config").join("config.json"),
        AppConfig::resolve_path()
    );

    // "procedures", como no config.json real do usuário
    let cfg = AppConfig::default();
    assert_eq!(cfg.database.procedures_path, "procedures");
    let dir = cfg.procedures_dir();
    assert!(dir.ends_with("procedures"), "{}", dir.display());
    assert_eq!(dir.parent().unwrap(), root);
    assert_ne!(
        dir.parent().and_then(Path::file_name).unwrap(),
        "config",
        "não deve cair em <raiz>/config/procedures: {}",
        dir.display()
    );

    // caminho absoluto fica como está
    let abs = std::env::temp_dir().join("tfm_procedimentos_abs");
    let cfg_abs = AppConfig {
        database: Database {
            procedures_path: abs.to_string_lossy().into_owned(),
        },
        ..AppConfig::default()
    };
    assert_eq!(cfg_abs.procedures_dir(), abs);
}

#[test]
fn config_unknown_active_profile_falls_back_to_default_limits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    write(
        &path,
        r#"{"active_machine_profile": "torno_grande",
            "machine_profiles": {"torno_grande": {"name": "Grande", "limits": {"max_length_x": "3000"}}}}"#,
    );
    let cfg = AppConfig::load(&path);
    // merge recursivo como _merge_config: o perfil "default" continua existindo
    assert_eq!(cfg.machine_profiles.len(), 2);
    let lim = cfg.active_limits();
    assert_eq!(lim.max_length_x, 3000.0, "string numérica aceita");
    assert_eq!(lim.max_rpm_chuck, 100.0);
    let mut cfg2 = cfg.clone();
    cfg2.active_machine_profile = "nao_existe".into();
    assert_eq!(cfg2.active_limits(), MachineLimits::default());
}

#[test]
fn config_invalid_json_falls_back_to_defaults_and_rewrites() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    write(&path, "{ isto não é json");
    let cfg = AppConfig::load(&path);
    assert_eq!(cfg, AppConfig::default());
    assert_eq!(read_json(&path)["costs"]["powder_brl_kg"], json!(250.0));
}

// ---------------------------------------------------------------------------
// procedure
// ---------------------------------------------------------------------------

#[test]
fn legacy_python_procedure_loads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legado.json");
    // Procedimento antigo: números como string, chave legada, modo legado e
    // strings de display da combobox Tkinter.
    write(
        &path,
        r#"{
            "nome_procedimento": "Eixo 01",
            "lead_in": "5.0",
            "welding_mode": "oscilacao_linear",
            "velocidade_soldagem": 150,
            "direcao_soldagem": "Esquerda -> Direita",
            "sentido_rotacao": "Anti-horária (CCW)",
            "oscillation_type": "Quadrada Contínua",
            "torch_retract_on_ignite": "1",
            "d_inicial": "80",
            "tipo_revestimento": "Oscilação Linear"
        }"#,
    );

    let p = load_procedure(&path).expect("procedimento legado carrega");

    assert_eq!(Operation::from_params(&p), Operation::OscilacaoLinear);
    assert_eq!(
        p.welding_mode,
        WeldingMode::OscilacaoLinear,
        "valor cru preservado"
    );
    assert_eq!(
        p.velocidade_de_deposicao, 150.0,
        "alias velocidade_soldagem"
    );
    assert_eq!(p.nome_procedimento, "Eixo 01");
    assert_eq!(p.lead_in, 5.0);
    assert_eq!(p.direcao_soldagem, Direction::EsquerdaDireita);
    assert_eq!(
        p.sentido_rotacao,
        Rotation::Antihoraria,
        "string de display mapeada"
    );
    assert_eq!(p.oscillation_type, Some(OscillationType::QuadradaContinua));
    assert!(p.torch_retract_on_ignite);
    assert_eq!(
        p.diametro, 50.0,
        "d_inicial é ignorado (chave desconhecida)"
    );
    // setdefault do _load_procedure_file para chaves ausentes
    assert_eq!(p.lead_out, 5.0);
    assert_eq!(p.oscilacao_comprimento, 10.0);
    assert_eq!(p.deslocamento_angular_perc, 50.0);
}

#[test]
fn procedure_setdefaults_do_not_override_present_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.json");
    write(
        &path,
        r#"{"lead_in": 0, "lead_out": "2,5", "oscilacao_comprimento": 7}"#,
    );
    let p = load_procedure(&path).unwrap();
    assert_eq!(p.lead_in, 0.0);
    assert_eq!(p.lead_out, 2.5);
    assert_eq!(p.oscilacao_comprimento, 7.0);
    assert_eq!(p.deslocamento_angular_perc, 50.0);
    assert_eq!(p.welding_mode, WeldingMode::Espiral);
}

#[test]
fn procedure_unknown_enum_values_fall_back_to_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.json");
    write(
        &path,
        r#"{"welding_mode": "modo_inexistente", "bolacha_tipo": "Foo",
            "direcao_soldagem": null, "sentido_rotacao": 3,
            "multiperfil_strategy": "???", "flat_orientation": "y"}"#,
    );
    let p = load_procedure(&path).unwrap();
    assert_eq!(p.welding_mode, WeldingMode::Espiral);
    assert_eq!(p.bolacha_tipo, BolachaTipo::EspiralContinua);
    assert_eq!(p.direcao_soldagem, Direction::EsquerdaDireita);
    assert_eq!(p.sentido_rotacao, Rotation::Horaria);
    assert_eq!(p.multiperfil_strategy, Default::default());
    assert_eq!(
        p.flat_orientation,
        tfm_core::params::FlatOrientation::Y,
        "x/y minúsculos aceitos"
    );
}

#[test]
fn procedure_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub").join("round_trip.json");
    let mut p = Params::default();
    p.apply_operation(Operation::OscilacaoQuadrada);
    p.nome_procedimento = "Rolo de laminação".into();
    p.notes = "Observações com acentuação: ção, ã, é".into();
    p.diametro = 123.45;
    p.num_camadas = 3;
    p.sentido_rotacao = Rotation::Antihoraria;
    p.traceability_operator = "Thiago".into();
    p.lead_in = 0.0; // o setdefault "5.0" não pode alterar um valor presente

    save_procedure(&path, &p).expect("salvar cria a pasta e grava");
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("{\n    \""), "indent 4:\n{text}");
    assert!(text.contains("\"oscillation_type\": \"quadrada\""));
    assert!(text.contains("ção"), "UTF-8 sem escapes");

    let back = load_procedure(&path).unwrap();
    assert_eq!(back, p);
    assert_eq!(Operation::from_params(&back), Operation::OscilacaoQuadrada);
}

#[test]
fn procedure_oscilacao_without_type_is_saved_as_linear() {
    // _save_procedure: welding_mode == 'oscilacao' => oscillation_type default 'linear'
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("osc.json");
    let p = Params {
        welding_mode: WeldingMode::Oscilacao,
        oscillation_type: None,
        ..Params::default()
    };
    save_procedure(&path, &p).unwrap();
    assert_eq!(read_json(&path)["oscillation_type"], json!("linear"));
    let back = load_procedure(&path).unwrap();
    assert_eq!(back.oscillation_type, Some(OscillationType::Linear));

    // Espiral: a chave não é gravada (o Python só a escreve em oscilação)
    let path2 = dir.path().join("esp.json");
    save_procedure(&path2, &Params::default()).unwrap();
    assert!(read_json(&path2).get("oscillation_type").is_none());
}

#[test]
fn procedure_errors_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nao_existe.json");
    assert!(matches!(
        load_procedure(&missing),
        Err(ProcedureError::Io { .. })
    ));

    let bad = dir.path().join("bad.json");
    write(&bad, "{ nope");
    assert!(matches!(load_procedure(&bad), Err(ProcedureError::Json(_))));

    let array = dir.path().join("array.json");
    write(&array, "[1, 2, 3]");
    assert!(matches!(
        load_procedure(&array),
        Err(ProcedureError::NotAnObject)
    ));

    let garbage = dir.path().join("garbage.json");
    write(&garbage, r#"{"diametro": "abc"}"#);
    let err = load_procedure(&garbage).unwrap_err();
    assert!(matches!(err, ProcedureError::Params(_)));
    assert!(err.to_string().contains("abc"), "{err}");
}

// ---------------------------------------------------------------------------
// audit
// ---------------------------------------------------------------------------

#[test]
fn audit_log_appends_to_daily_file() {
    let dir = tempfile::tempdir().unwrap();
    let logs_dir = dir.path().join("data").join("logs");
    let logger = AuditLogger::new(&logs_dir);
    assert!(logs_dir.is_dir(), "construtor cria a pasta como o Python");

    logger.log("GERACAO", "G-code gerado: peca.tap", "Engenharia");
    logger.log("CONFIG", "Pó alterado para Stellite 6", "Thiago");

    let today = Local::now().date_naive();
    let path = logs_dir.join(format!("LOG_ENG_{}.json", today.format("%Y-%m-%d")));
    assert!(path.exists(), "{}", path.display());
    let v = read_json(&path);
    let entries = v.as_array().expect("array de entradas");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["category"], json!("GERACAO"));
    assert_eq!(entries[0]["detail"], json!("G-code gerado: peca.tap"));
    assert_eq!(entries[0]["user"], json!("Engenharia"));
    assert_eq!(entries[0]["software_version"], json!(SOFTWARE_VERSION));
    assert_eq!(entries[1]["user"], json!("Thiago"));
    let ts = entries[0]["timestamp"].as_str().unwrap();
    // datetime.now().isoformat(): AAAA-MM-DDTHH:MM:SS.ffffff
    assert!(
        ts.starts_with(&today.format("%Y-%m-%dT").to_string()),
        "{ts}"
    );
    assert_eq!(ts.len(), 26, "{ts}");
    let keys: Vec<&str> = entries[0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "timestamp",
            "category",
            "detail",
            "user",
            "software_version"
        ]
    );

    // arquivo corrompido: recomeça do zero sem falhar (como o Python)
    write(&path, "{ corrompido");
    logger.log("X", "y", "z");
    assert_eq!(read_json(&path).as_array().unwrap().len(), 1);
    assert_eq!(
        AuditLogger::file_name_for(NaiveDate::from_ymd_opt(2026, 1, 5).unwrap()),
        "LOG_ENG_2026-01-05.json"
    );
}

// ---------------------------------------------------------------------------
// quality
// ---------------------------------------------------------------------------

/// `QualityManager` em pastas temporárias, com `operators.json`/`wps.json`
/// opcionais já gravados em `data_dir`.
fn quality(dir: &Path, operators: Option<&str>, wps: Option<&str>) -> QualityManager {
    let config_dir = dir.join("config");
    let data_dir = dir.join("data").join("quality");
    if let Some(text) = operators {
        write(&data_dir.join("operators.json"), text);
    }
    if let Some(text) = wps {
        write(&data_dir.join("wps.json"), text);
    }
    QualityManager::load(&config_dir, &data_dir)
}

#[test]
fn iso3834_blocks_expired_operator() {
    let dir = tempfile::tempdir().unwrap();
    let qm = quality(
        dir.path(),
        Some(
            r#"[{"name": "Thiago", "qualification_id": "001", "qualification_validity": "2000-01-01"}]"#,
        ),
        None,
    );
    let err = qm.check_operator_qualification("Thiago").unwrap_err();
    assert!(err.contains("vencida"), "{err}");
    assert_eq!(
        err,
        "BLOQUEIO ISO 3834: Qualificação do operador 'Thiago' vencida em 2000-01-01."
    );
    // operador vazio ou desconhecido não bloqueia
    assert_eq!(qm.check_operator_qualification(""), Ok(()));
    assert_eq!(qm.check_operator_qualification("Ninguém"), Ok(()));

    let p = Params {
        traceability_operator: "Thiago".into(),
        ..Params::default()
    };
    assert!(qm.validate_iso3834(&p).unwrap_err().contains("vencida"));
}

#[test]
fn iso3834_requires_batch_when_eps_set() {
    let dir = tempfile::tempdir().unwrap();
    let qm = quality(dir.path(), None, Some(r#"[{"name": "E1", "params": {}}]"#));
    assert_eq!(qm.wps.len(), 1);
    assert!(qm.wps("E1").is_some());

    let mut p = Params {
        traceability_eps: "E1".into(),
        ..Params::default()
    };
    let err = qm.validate_iso3834(&p).unwrap_err();
    assert!(err.contains("Lote do Pó"), "{err}");
    assert_eq!(
        err,
        "BLOQUEIO ISO 3834: Campos obrigatórios para EPS ativa: Lote do Pó, Fornecedor, Certificado."
    );

    // só o fornecedor em branco (espaços contam como vazio)
    p.traceability_powder_batch = "L-123".into();
    p.traceability_supplier = "   ".into();
    p.traceability_certificate = "C-9".into();
    assert_eq!(
        qm.validate_iso3834(&p),
        Err("BLOQUEIO ISO 3834: Campos obrigatórios para EPS ativa: Fornecedor.".to_string())
    );

    // EPS não cadastrada ou vazia: não bloqueia
    p.traceability_eps = "E-desconhecida".into();
    assert_eq!(qm.validate_iso3834(&p), Ok(()));
    p.traceability_eps = String::new();
    assert_eq!(qm.validate_iso3834(&p), Ok(()));
}

#[test]
fn iso3834_blocks_parameter_outside_wps_limits() {
    let dir = tempfile::tempdir().unwrap();
    let qm = quality(
        dir.path(),
        None,
        Some(
            r#"[{"name": "E2", "params": {"corrente_arco": 120},
                 "limits": {"corrente_arco": {"min": 100, "max": "140"},
                            "nome_procedimento": {"min": 0, "max": 1},
                            "chave_inexistente": {"min": 0, "max": 1},
                            "quebrado": {"min": "x"}}}]"#,
        ),
    );
    let wps = qm.wps("E2").unwrap();
    assert_eq!(
        wps.limits.len(),
        3,
        "limite sem max/min numérico é descartado"
    );
    assert_eq!(wps.limits["corrente_arco"].max, 140.0);

    let mut p = Params {
        traceability_eps: "E2".into(),
        traceability_powder_batch: "L".into(),
        traceability_supplier: "F".into(),
        traceability_certificate: "C".into(),
        corrente_arco: 180.0,
        ..Params::default()
    };
    assert_eq!(
        qm.validate_iso3834(&p),
        Err("BLOQUEIO ISO 3834: Parâmetro 'corrente_arco' (180.0) fora da faixa da EPS (100.0 - 140.0).".to_string())
    );
    p.corrente_arco = 125.5;
    assert_eq!(qm.validate_iso3834(&p), Ok(()));
}

#[test]
fn operators_accept_legacy_validity_key_and_save_both_keys() {
    let dir = tempfile::tempdir().unwrap();
    let mut qm = quality(
        dir.path(),
        Some(
            r#"[{"name": "Thiago", "qualification_id": "001", "validity": " 2099-12-31 "},
                 {"code": "sem-nome"}, "lixo", {"name": ""}]"#,
        ),
        None,
    );
    assert_eq!(qm.operators.len(), 1, "entradas sem nome são ignoradas");
    assert_eq!(qm.operators[0].qualification_validity, "2099-12-31");
    assert_eq!(qm.check_operator_qualification("Thiago"), Ok(()));

    qm.add_operator("Maria", "M1", "Q2", "2099-01-01");
    qm.add_operator("Thiago", "", "", "");
    assert_eq!(qm.operator_names(), ["Maria", "Thiago"]);
    assert_eq!(
        qm.operator("Thiago").unwrap().qualification_id,
        "001",
        "campos vazios não sobrescrevem"
    );

    let saved = read_json(
        &dir.path()
            .join("data")
            .join("quality")
            .join("operators.json"),
    );
    let first = &saved.as_array().unwrap()[0];
    assert_eq!(first["name"], json!("Thiago"));
    assert_eq!(first["qualification_validity"], json!("2099-12-31"));
    assert_eq!(
        first["validity"],
        json!("2099-12-31"),
        "chave legada gravada como no Python"
    );

    qm.remove_operator("Maria");
    let again = QualityManager::load(
        &dir.path().join("config"),
        &dir.path().join("data").join("quality"),
    );
    assert_eq!(again.operator_names(), ["Thiago"]);
}

#[test]
fn maintenance_defaults_and_alerts() {
    let dir = tempfile::tempdir().unwrap();
    let mut qm = quality(dir.path(), None, None);
    assert!(dir.path().join("config").is_dir(), "load cria config_dir");
    assert!(
        dir.path().join("data").join("quality").is_dir(),
        "load cria data_dir"
    );

    let m = &qm.maintenance;
    assert_eq!(m.total_arc_time_minutes, 0.0);
    assert_eq!(
        m.last_maintenance_date,
        Local::now().format("%Y-%m-%d").to_string()
    );
    let names: Vec<&str> = m.components.keys().map(String::as_str).collect();
    assert_eq!(names, ["bico_constritor", "eletrodo", "mangueiras"]);
    assert_eq!(m.components["bico_constritor"].limit_hours, 20.0);
    assert_eq!(m.components["eletrodo"].limit_hours, 40.0);
    assert_eq!(m.components["mangueiras"].limit_hours, 500.0);
    assert!(m.components.values().all(|c| c.current_hours == 0.0));
    assert!(m.history.is_empty());
    assert_eq!(*m, Maintenance::default());
    assert!(qm.maintenance_alerts().is_empty());

    qm.log_arc_time(18.5 * 60.0);
    assert_eq!(qm.maintenance.total_arc_time_minutes, 1110.0);
    assert_eq!(qm.maintenance.components["eletrodo"].current_hours, 18.5);
    assert_eq!(
        qm.maintenance_alerts(),
        ["Atenção: bico_constritor próximo do limite (18.5/20h)"]
    );

    qm.log_arc_time(90.0);
    assert_eq!(
        qm.maintenance_alerts(),
        ["Manutenção Necessária: bico_constritor (20.0/20h)"]
    );

    // gravado em config_dir/maintenance_log.json e relido igual
    let path = dir.path().join("config").join("maintenance_log.json");
    assert_eq!(read_json(&path)["total_arc_time_minutes"], json!(1200.0));
    let again = QualityManager::load(
        &dir.path().join("config"),
        &dir.path().join("data").join("quality"),
    );
    assert_eq!(again.maintenance, qm.maintenance);
}

#[test]
fn reset_component_records_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut qm = quality(dir.path(), None, None);
    qm.log_arc_time(600.0);
    qm.reset_component("bico_constritor");
    qm.reset_component("nao_existe");

    assert_eq!(
        qm.maintenance.components["bico_constritor"].current_hours,
        0.0
    );
    assert_eq!(qm.maintenance.components["eletrodo"].current_hours, 10.0);
    assert_eq!(qm.maintenance.history.len(), 1);
    let h = &qm.maintenance.history[0];
    assert_eq!(h.action, "Reset bico_constritor");
    assert_eq!(h.total_time, 600.0);
    assert_eq!(h.date.len(), "AAAA-MM-DD HH:MM".len(), "{}", h.date);

    // arquivo parcial: chaves ausentes vêm do default (merge do _load_maintenance)
    let path = dir.path().join("config").join("maintenance_log.json");
    write(&path, r#"{"total_arc_time_minutes": 42.0}"#);
    let again = QualityManager::load(
        &dir.path().join("config"),
        &dir.path().join("data").join("quality"),
    );
    assert_eq!(again.maintenance.total_arc_time_minutes, 42.0);
    assert_eq!(again.maintenance.components.len(), 3);
}

#[test]
fn heat_input_formula() {
    let hi = QualityManager::heat_input_kj_mm(25.0, 180.0, 120.0, 0.8);
    assert!((hi - 1.8).abs() < 1e-9, "{hi}");
    assert_eq!(QualityManager::heat_input_kj_mm(25.0, 180.0, 0.0, 0.8), 0.0);
    assert_eq!(
        QualityManager::heat_input_kj_mm(25.0, 180.0, -5.0, 0.8),
        0.0
    );
}

#[test]
fn estimate_dilution_is_clamped() {
    assert!((QualityManager::estimate_dilution(100.0, 20.0, 120.0) - 10.5).abs() < 1e-9);
    assert_eq!(QualityManager::estimate_dilution(10.0, 20.0, 120.0), 0.0);
    assert_eq!(
        QualityManager::estimate_dilution(1000.0, 10.0, 120.0),
        100.0
    );
    assert_eq!(QualityManager::estimate_dilution(100.0, 0.0, 120.0), 0.0);
}

#[test]
fn process_warnings_match_python_validate_parameters() {
    let (w, e) = QualityManager::process_warnings(&Params::default());
    assert!(w.is_empty() && e.is_empty(), "{w:?} {e:?}");

    let fast = Params {
        taxa_de_deposicao: Some(6000.0), // espelho de velocidade_de_deposicao
        ..Params::default()
    };
    let (w, e) = QualityManager::process_warnings(&fast);
    assert_eq!(
        e,
        ["Velocidade de soldagem (6000.0 mm/min) excede o limite da máquina (5000.0)."]
    );
    assert_eq!(
        w,
        [
            "Frequência de oscilação estimada (10.0 Hz) pode ser muito alta para a mesa.",
            "Aporte Térmico muito baixo (0.04 kJ/mm). Risco de falta de fusão.",
        ]
    );

    let slow = Params {
        velocidade_de_deposicao: 10.0,
        oscilacao_comprimento: 0.0,
        ..Params::default()
    };
    let (w, e) = QualityManager::process_warnings(&slow);
    assert!(e.is_empty());
    assert_eq!(
        w,
        ["Aporte Térmico muito alto (21.60 kJ/mm). Risco de distorção/diluição excessiva."]
    );
}

#[test]
fn python_shaped_quality_files_load() {
    // Mesmo formato dos arquivos que o Python grava em data/quality
    // (operators.json com as duas chaves de validade; wps.json só com
    // name/params), com valores sintéticos para não depender dos dados do usuário.
    let dir = tempfile::tempdir().unwrap();
    let qm = quality(
        dir.path(),
        Some(
            r#"[
    {
        "name": "Operador Exemplo",
        "qualification_id": "QX-001",
        "qualification_validity": "2099-06-30",
        "validity": "2099-06-30"
    }
]"#,
        ),
        Some(
            r#"[
    {
        "name": "EPS-Exemplo",
        "params": {}
    }
]"#,
        ),
    );
    assert_eq!(qm.operator_names(), ["Operador Exemplo"]);
    let op = qm.operator("Operador Exemplo").unwrap();
    assert_eq!(op.qualification_id, "QX-001");
    assert_eq!(op.qualification_validity, "2099-06-30");
    assert_eq!(op.code, "");
    assert_eq!(qm.check_operator_qualification("Operador Exemplo"), Ok(()));
    assert_eq!(qm.wps_names(), ["EPS-Exemplo"]);
    let wps = qm.wps("EPS-Exemplo").unwrap();
    assert!(wps.params.is_empty());
    assert!(wps.limits.is_empty());
}
