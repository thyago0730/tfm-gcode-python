//! Testes de integração de `tfm_core::params`.
//!
//! Cobrem: defaults iguais à UI Python, desserialização leniente do JSON
//! gerado pelo Python (números como string, vírgula decimal, chaves legadas),
//! fixtures golden, ida-e-volta `Operation` e a porta de `_reset_mode_defaults`.

use serde_json::{json, Value};
use tfm_core::params::{
    BolachaTipo, Direction, FlatOrientation, MultiperfilStrategy, Operation, OscillationType,
    Params, ProfileShape, Rotation, WeldingMode,
};

const GOLDEN_CASES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/golden/cases.json");

#[test]
fn default_matches_python_ui_defaults() {
    let p = Params::default();
    assert_eq!(p.diametro, 50.0);
    assert_eq!(p.diametro_interno, 0.0);
    assert_eq!(p.comprimento_revestir, 50.0);
    assert_eq!(p.largura_revestir, 100.0);
    assert_eq!(p.largura_cordao, 8.0);
    assert_eq!(p.sobreposicao, 40.0);
    assert_eq!(p.velocidade_de_deposicao, 120.0);
    assert_eq!(p.taxa_de_deposicao, None);
    assert_eq!(p.taxa(), 120.0, "taxa() cai para velocidade_de_deposicao");
    assert_eq!(p.velocidade_a_mm_min, 120.0);
    assert_eq!(p.velocidade_oscilacao_mm_min, 3000.0);
    assert_eq!(p.afastamento_tocha, 12.0);
    assert_eq!(p.afastamento_seguranca, 30.0);
    assert_eq!(p.torch_ignite_delay_s, 3.0);
    assert_eq!(p.torch_extinguish_delay_s, 2.0);
    assert_eq!(p.overlap_angular_deg, 10.0);
    assert_eq!(p.bead_width_calibration_factor, 1.0);
    assert!(!p.lead_extras_enabled);
    assert_eq!(p.lead_in, 0.0);
    assert_eq!(p.lead_out, 0.0);
    assert_eq!(p.direcao_soldagem, Direction::EsquerdaDireita);
    assert_eq!(p.sentido_rotacao, Rotation::Horaria);
    assert_eq!(p.corrente_arco, 180.0);
    assert_eq!(p.tensao_arco, 25.0);
    assert_eq!(p.vazao_gas, 15.0);
    assert_eq!(p.alim_po, 50.0);
    assert_eq!(p.preaquecimento, 100.0);
    assert_eq!(p.eficiencia_termica, 0.8);
    assert_eq!(p.taxa_deposicao_g_h, 700.0);
    assert_eq!(p.num_camadas, 1);
    assert_eq!(p.espessura_camada, 1.5);
    assert_eq!(p.powder_factor, 0.16);
    assert_eq!(p.powder_cost_brl_kg, 450.0);
    assert!(p.torch_retract_on_ignite);
    assert!(!p.spiral_close_ends);
    assert!(!p.flat_single_direction);
    assert_eq!(p.flat_orientation, FlatOrientation::X);
    assert!(!p.operacao_interna);
    assert_eq!(p.internal_face_clearance_x, 30.0);
    assert_eq!(p.multiperfil_count, 4);
    assert_eq!(p.multiperfil_total_angle_deg, 360.0);
    assert_eq!(p.multiperfil_pitch_mm, 12.0);
    assert_eq!(p.multiperfil_strategy, MultiperfilStrategy::PassesSeparados);
    assert!(!p.multiperfil_point_by_point);
    assert_eq!(p.multiperfil_profile_shape, ProfileShape::Retangular);
    assert_eq!(p.multiperfil_profile_base_mm, 8.0);
    assert_eq!(p.multiperfil_profile_top_mm, 4.0);
    assert_eq!(p.multiperfil_profile_height_mm, 1.5);
    assert!(!p.multiperfil_end_chamfer_enabled);
    assert_eq!(p.multiperfil_end_chamfer_angle_deg, 45.0);
    assert_eq!(p.bolacha_tipo, BolachaTipo::EspiralContinua);
    assert!(!p.bolacha_stop_between);
    assert_eq!(p.oscilacao_comprimento, 5.0);
    assert_eq!(p.deslocamento_angular_perc, 20.0);
    assert_eq!(p.osc_test_gran_x, 0.5);
    assert_eq!(p.osc_test_gran_a, 1.0);
    assert!(!p.smart_layers);
    assert_eq!(p.root_feed_scale, 100.0);
    assert_eq!(p.nome_procedimento, "Novo Procedimento");
    assert_eq!(p.welding_mode, WeldingMode::Espiral);
    assert_eq!(
        p.oscillation_type, None,
        "Python só grava a chave em modos de oscilação"
    );
    assert_eq!(p.traceability_operator, "");
    assert_eq!(p.traceability_eps, "");
    assert_eq!(p.traceability_powder_batch, "");
    assert_eq!(p.traceability_supplier, "");
    assert_eq!(p.traceability_certificate, "");
    assert_eq!(p.traceability_env_temp, "");
    assert_eq!(p.traceability_env_humidity, "");
    assert_eq!(p.notes, "");
    assert_eq!(p.ordem_servico, "");
    assert_eq!(p.powder_name, "");
    assert_eq!(p.app_version, None);
    assert_eq!(Operation::from_params(&p), Operation::Espiral);
}

#[test]
fn deserializes_python_strings_and_legacy_keys() {
    let raw = json!({
        "velocidade_soldagem": "150.0",
        "lead_in": "5,5",
        "welding_mode": "oscilacao_linear",
        "internal_entry_x": "25"
    });
    let p: Params = serde_json::from_value(raw).expect("JSON legado deve desserializar");
    assert_eq!(
        p.velocidade_de_deposicao, 150.0,
        "alias velocidade_soldagem"
    );
    assert_eq!(p.lead_in, 5.5, "vírgula decimal");
    assert_eq!(
        p.welding_mode,
        WeldingMode::OscilacaoLinear,
        "modo legado é variante real"
    );
    assert_eq!(p.welding_mode.as_str(), "oscilacao_linear");
    assert!(p.welding_mode.is_oscillation());
    assert_eq!(p.oscillation_type, None, "chave ausente continua ausente");
    assert_eq!(p.face_clearance(), 25.0, "alias internal_entry_x");
    assert_eq!(p.internal_face_clearance_x, 25.0);
    // Campos ausentes continuam com o default da UI.
    assert_eq!(p.diametro, 50.0);
    assert_eq!(Operation::from_params(&p), Operation::OscilacaoLinear);
}

#[test]
fn welding_mode_strings_and_oscillation_flags_match_python() {
    let expected = [
        (WeldingMode::Espiral, "espiral", false),
        (
            WeldingMode::EspiralMultiperfil,
            "espiral_multiperfil",
            false,
        ),
        (WeldingMode::Bolacha, "bolacha", false),
        (WeldingMode::ChapaPlana, "chapa_plana", false),
        (WeldingMode::Oscilacao, "oscilacao", true),
        (WeldingMode::OscilacaoLinear, "oscilacao_linear", true),
        (WeldingMode::OscilacaoQuadrada, "oscilacao_quadrada", true),
    ];
    for (mode, s, osc) in expected {
        assert_eq!(mode.as_str(), s);
        assert_eq!(mode.is_oscillation(), osc, "{s}");
        assert_eq!(mode.to_string(), s);
        assert_eq!(
            serde_json::to_value(mode).unwrap(),
            json!(s),
            "{s}: serialize"
        );
        let back: WeldingMode = serde_json::from_value(json!(s)).unwrap();
        assert_eq!(back, mode, "{s}: deserialize");
    }

    // Modo unificado sem tipo equivale a linear; legados ignoram oscillation_type.
    let p = Params {
        welding_mode: WeldingMode::Oscilacao,
        oscillation_type: None,
        ..Params::default()
    };
    assert_eq!(Operation::from_params(&p), Operation::OscilacaoLinear);
    let p = Params {
        welding_mode: WeldingMode::OscilacaoQuadrada,
        oscillation_type: Some(OscillationType::QuadradaContinua),
        ..Params::default()
    };
    assert_eq!(
        Operation::from_params(&p),
        Operation::OscilacaoQuadrada,
        "legado manda"
    );
    let p = Params {
        welding_mode: WeldingMode::OscilacaoLinear,
        oscillation_type: Some(OscillationType::Quadrada),
        ..Params::default()
    };
    assert_eq!(
        Operation::from_params(&p),
        Operation::OscilacaoLinear,
        "legado manda"
    );
}

#[test]
fn lenient_scalars_cover_number_string_comma_empty_and_bool_variants() {
    let raw = json!({
        "diametro": "12,5",
        "largura_cordao": "",
        "sobreposicao": 40,
        "lead_out": "  7.25 ",
        "num_camadas": "3",
        "multiperfil_count": 2.0,
        "lead_extras_enabled": 1,
        "spiral_close_ends": "true",
        "operacao_interna": "0",
        "smart_layers": "false",
        "torch_retract_on_ignite": "True",
        "taxa_de_deposicao": "",
        "notes": null,
        "ordem_servico": 1234
    });
    let p: Params = serde_json::from_value(raw).expect("valores lenientes devem desserializar");
    assert_eq!(p.diametro, 12.5);
    assert_eq!(p.largura_cordao, 0.0, "string vazia vira 0.0");
    assert_eq!(p.sobreposicao, 40.0, "inteiro JSON vira f64");
    assert_eq!(p.lead_out, 7.25, "espaços são ignorados");
    assert_eq!(p.num_camadas, 3);
    assert_eq!(p.multiperfil_count, 2, "float inteiro vira i64");
    assert!(p.lead_extras_enabled, "1 -> true");
    assert!(p.spiral_close_ends, "\"true\" -> true");
    assert!(!p.operacao_interna, "\"0\" -> false");
    assert!(!p.smart_layers, "\"false\" -> false");
    assert!(p.torch_retract_on_ignite, "\"True\" (Python) -> true");
    assert_eq!(
        p.taxa_de_deposicao, None,
        "string vazia em Option vira None"
    );
    assert_eq!(p.notes, "", "null em String vira vazio");
    assert_eq!(p.ordem_servico, "1234", "número em String vira texto");

    let bad: Result<Params, _> = serde_json::from_value(json!({ "diametro": "abc" }));
    assert!(bad.is_err(), "texto não numérico deve falhar, não virar 0");
    let bad: Result<Params, _> = serde_json::from_value(json!({ "smart_layers": "talvez" }));
    assert!(bad.is_err(), "bool desconhecido deve falhar");
}

#[test]
fn all_golden_cases_deserialize() {
    let text = std::fs::read_to_string(GOLDEN_CASES)
        .unwrap_or_else(|e| panic!("não foi possível ler {GOLDEN_CASES}: {e}"));
    let cases: Vec<Value> = serde_json::from_str(&text).expect("cases.json deve ser um array JSON");
    assert!(!cases.is_empty(), "cases.json não pode estar vazio");

    for case in &cases {
        let name = case["name"].as_str().expect("cada caso tem name");
        let raw = &case["params"];
        assert!(raw.is_object(), "{name}: params deve ser objeto");
        let p: Params = serde_json::from_value(raw.clone())
            .unwrap_or_else(|e| panic!("{name}: params não desserializou: {e}"));
        assert_eq!(
            p.app_version.as_deref(),
            Some("11.3.2"),
            "{name}: app_version"
        );

        let again = serde_json::to_value(&p).unwrap_or_else(|e| panic!("{name}: serialize: {e}"));
        let wm_in = raw["welding_mode"].as_str().unwrap_or("espiral");
        assert_eq!(
            again["welding_mode"].as_str(),
            Some(wm_in),
            "{name}: welding_mode mantido"
        );
        assert_eq!(p.welding_mode.as_str(), wm_in, "{name}: as_str");
        match raw.get("oscillation_type") {
            Some(osc) => assert_eq!(
                again.get("oscillation_type"),
                Some(osc),
                "{name}: oscillation_type mantido"
            ),
            None => assert!(
                again.get("oscillation_type").is_none(),
                "{name}: oscillation_type ausente continua ausente"
            ),
        }
        assert!(
            again.get("operation").is_none(),
            "{name}: Operation não é serializada"
        );

        let back: Params = serde_json::from_value(again)
            .unwrap_or_else(|e| panic!("{name}: re-desserializar: {e}"));
        assert_eq!(back, p, "{name}: ida-e-volta deve ser estável");
    }
}

#[test]
fn operation_round_trip() {
    assert_eq!(
        Operation::ALL,
        [
            Operation::Espiral,
            Operation::EspiralMultiperfil,
            Operation::OscilacaoLinear,
            Operation::OscilacaoQuadrada,
            Operation::OscilacaoQuadradaContinua,
            Operation::ChapaPlana,
            Operation::Bolacha,
        ]
    );
    for op in Operation::ALL {
        let mut p = Params::default();
        p.apply_operation(op);
        assert_eq!(Operation::from_params(&p), op, "{op:?}");
        let (wm, osc) = op.mode();
        assert_eq!(p.welding_mode, wm, "{op:?}: welding_mode");
        assert_eq!(
            p.oscillation_type, osc,
            "{op:?}: oscillation_type (None fora da oscilação)"
        );
        assert_ne!(
            wm,
            WeldingMode::OscilacaoLinear,
            "{op:?}: apply_operation grava a forma moderna"
        );
        assert_ne!(
            wm,
            WeldingMode::OscilacaoQuadrada,
            "{op:?}: apply_operation grava a forma moderna"
        );
    }
    // Voltar de uma oscilação para Espiral limpa oscillation_type.
    let mut p = Params::default();
    p.apply_operation(Operation::OscilacaoQuadrada);
    assert_eq!(p.oscillation_type, Some(OscillationType::Quadrada));
    p.apply_operation(Operation::Espiral);
    assert_eq!(p.oscillation_type, None);
    assert_eq!(Operation::Espiral.mode(), (WeldingMode::Espiral, None));
    assert_eq!(
        Operation::EspiralMultiperfil.mode(),
        (WeldingMode::EspiralMultiperfil, None)
    );
    assert_eq!(
        Operation::OscilacaoLinear.mode(),
        (WeldingMode::Oscilacao, Some(OscillationType::Linear))
    );
    assert_eq!(
        Operation::OscilacaoQuadrada.mode(),
        (WeldingMode::Oscilacao, Some(OscillationType::Quadrada))
    );
    assert_eq!(
        Operation::OscilacaoQuadradaContinua.mode(),
        (
            WeldingMode::Oscilacao,
            Some(OscillationType::QuadradaContinua)
        )
    );
    assert_eq!(
        Operation::ChapaPlana.mode(),
        (WeldingMode::ChapaPlana, None)
    );
    assert_eq!(Operation::Bolacha.mode(), (WeldingMode::Bolacha, None));
}

#[test]
fn operation_labels_match_python_ui() {
    let labels: Vec<&str> = Operation::ALL.iter().map(|o| o.label()).collect();
    assert_eq!(
        labels,
        [
            "Espiral",
            "Espiral Multiperfil",
            "Oscilação Linear",
            "Oscilação Quadrada",
            "Oscilação Quadrada Contínua",
            "Chapa Plana",
            "Bolacha"
        ]
    );
    assert_eq!(Operation::OscilacaoLinear.short_label(), "Osc. Linear");
    assert_eq!(Operation::OscilacaoQuadrada.short_label(), "Osc. Quad.");
    assert_eq!(
        Operation::OscilacaoQuadradaContinua.short_label(),
        "Osc. Quad. Cont."
    );
    assert_eq!(Operation::Espiral.short_label(), "Espiral");
    assert_eq!(Operation::Bolacha.short_label(), "Bolacha");
    assert_eq!(
        Operation::from_label("Oscilação Quadrada"),
        Some(Operation::OscilacaoQuadrada)
    );
    assert_eq!(Operation::from_label("inexistente"), None);
    assert_eq!(Operation::ChapaPlana.to_string(), "Chapa Plana");
}

/// Params com todos os campos tocados por `_reset_mode_defaults` sujos.
fn dirty_params() -> Params {
    Params {
        diametro_interno: 20.0,
        bolacha_tipo: BolachaTipo::Aneis,
        bolacha_stop_between: true,
        overlap_angular_deg: 33.0,
        largura_revestir: 250.0,
        flat_orientation: FlatOrientation::Y,
        flat_single_direction: true,
        multiperfil_count: 9,
        multiperfil_total_angle_deg: 90.0,
        multiperfil_pitch_mm: 3.0,
        multiperfil_strategy: MultiperfilStrategy::PercursoContinuo,
        multiperfil_point_by_point: true,
        multiperfil_profile_shape: ProfileShape::Triangular,
        multiperfil_profile_base_mm: 1.0,
        multiperfil_profile_top_mm: 1.0,
        multiperfil_profile_height_mm: 9.0,
        multiperfil_end_chamfer_enabled: true,
        multiperfil_end_chamfer_angle_deg: 10.0,
        lead_extras_enabled: true,
        operacao_interna: true,
        spiral_close_ends: true,
        bead_width_calibration_factor: 1.37,
        diametro: 999.0,
        ..Params::default()
    }
}

#[test]
fn reset_mode_defaults_bolacha_matches_python() {
    let mut p = dirty_params();
    p.reset_mode_defaults(Operation::Bolacha);
    assert_eq!(p.diametro_interno, 0.0);
    assert_eq!(p.bolacha_tipo, BolachaTipo::EspiralContinua);
    assert!(!p.bolacha_stop_between);
    assert_eq!(p.overlap_angular_deg, 10.0);
    assert_eq!(p.bead_width_calibration_factor, 1.0, "sempre 1.0");
    // Não toca nos campos dos outros modos.
    assert_eq!(p.largura_revestir, 250.0);
    assert_eq!(p.flat_orientation, FlatOrientation::Y);
    assert_eq!(p.multiperfil_count, 9);
    assert!(p.lead_extras_enabled);
    assert!(p.operacao_interna);
    assert!(p.spiral_close_ends);
    assert_eq!(p.diametro, 999.0, "campos fora do reset ficam intactos");
}

#[test]
fn reset_mode_defaults_chapa_plana_matches_python() {
    let mut p = dirty_params();
    p.reset_mode_defaults(Operation::ChapaPlana);
    assert_eq!(p.largura_revestir, 100.0);
    assert_eq!(p.flat_orientation, FlatOrientation::X);
    assert!(!p.flat_single_direction);
    assert_eq!(p.bead_width_calibration_factor, 1.0);
    assert_eq!(p.diametro_interno, 20.0);
    assert_eq!(p.bolacha_tipo, BolachaTipo::Aneis);
    assert_eq!(p.multiperfil_count, 9);
    assert!(p.lead_extras_enabled);
}

#[test]
fn reset_mode_defaults_multiperfil_matches_python() {
    let mut p = dirty_params();
    p.reset_mode_defaults(Operation::EspiralMultiperfil);
    let d = Params::default();
    assert_eq!(p.multiperfil_count, d.multiperfil_count);
    assert_eq!(p.multiperfil_total_angle_deg, d.multiperfil_total_angle_deg);
    assert_eq!(p.multiperfil_pitch_mm, d.multiperfil_pitch_mm);
    assert_eq!(p.multiperfil_strategy, d.multiperfil_strategy);
    assert_eq!(p.multiperfil_point_by_point, d.multiperfil_point_by_point);
    assert_eq!(p.multiperfil_profile_shape, d.multiperfil_profile_shape);
    assert_eq!(p.multiperfil_profile_base_mm, d.multiperfil_profile_base_mm);
    assert_eq!(p.multiperfil_profile_top_mm, d.multiperfil_profile_top_mm);
    assert_eq!(
        p.multiperfil_profile_height_mm,
        d.multiperfil_profile_height_mm
    );
    assert_eq!(
        p.multiperfil_end_chamfer_enabled,
        d.multiperfil_end_chamfer_enabled
    );
    assert_eq!(
        p.multiperfil_end_chamfer_angle_deg,
        d.multiperfil_end_chamfer_angle_deg
    );
    assert_eq!(p.bead_width_calibration_factor, 1.0);
    // Multiperfil NÃO entra no ramo "else" do Python.
    assert!(p.lead_extras_enabled);
    assert!(p.operacao_interna);
    assert!(p.spiral_close_ends);
    assert_eq!(p.diametro_interno, 20.0);
    assert_eq!(p.largura_revestir, 250.0);
}

#[test]
fn reset_mode_defaults_other_operations_match_python() {
    for op in [
        Operation::Espiral,
        Operation::OscilacaoLinear,
        Operation::OscilacaoQuadrada,
        Operation::OscilacaoQuadradaContinua,
    ] {
        let mut p = dirty_params();
        p.reset_mode_defaults(op);
        assert!(!p.lead_extras_enabled, "{op:?}");
        assert!(!p.operacao_interna, "{op:?}");
        assert!(!p.spiral_close_ends, "{op:?}");
        assert_eq!(p.bead_width_calibration_factor, 1.0, "{op:?}");
        assert_eq!(p.diametro_interno, 20.0, "{op:?}");
        assert_eq!(p.largura_revestir, 250.0, "{op:?}");
        assert_eq!(p.multiperfil_count, 9, "{op:?}");
        assert_eq!(p.bolacha_tipo, BolachaTipo::Aneis, "{op:?}");
    }
}

#[test]
fn normalize_mirrors_python_normalize_params() {
    let mut p = Params {
        velocidade_de_deposicao: 150.0,
        velocidade_a_mm_min: 0.0,
        ..Params::default()
    };
    assert_eq!(p.taxa_de_deposicao, None);
    p.normalize();
    assert_eq!(p.taxa_de_deposicao, Some(150.0));
    assert_eq!(p.velocidade_a_mm_min, 150.0);

    let mut q = Params {
        velocidade_de_deposicao: 150.0,
        taxa_de_deposicao: Some(99.0),
        velocidade_a_mm_min: 77.0,
        ..Params::default()
    };
    q.normalize();
    assert_eq!(
        q.taxa_de_deposicao,
        Some(99.0),
        "valor explícito é preservado"
    );
    assert_eq!(q.velocidade_a_mm_min, 77.0, "valor explícito é preservado");
    assert_eq!(q.taxa(), 99.0);
}

#[test]
fn helper_methods_match_generator_semantics() {
    let mut p = Params::default();
    assert!(!p.is_internal());
    assert_eq!(p.rotation_sign(), 1.0);
    assert_eq!(p.face_clearance(), 30.0);
    p.operacao_interna = true;
    p.sentido_rotacao = Rotation::Antihoraria;
    p.internal_face_clearance_x = 0.0;
    assert!(p.is_internal());
    assert_eq!(p.rotation_sign(), -1.0);
    assert_eq!(
        p.face_clearance(),
        30.0,
        "0.0 cai para 30.0 como `or 30.0` no Python"
    );
    p.internal_face_clearance_x = 12.0;
    assert_eq!(p.face_clearance(), 12.0);
}

#[test]
fn serializes_with_python_json_keys_and_enum_strings() {
    let mut p = Params::default();
    p.apply_operation(Operation::OscilacaoQuadradaContinua);
    let v = serde_json::to_value(&p).expect("serialize");
    let obj = v.as_object().expect("objeto");
    assert_eq!(obj["welding_mode"], json!("oscilacao"));
    assert_eq!(obj["oscillation_type"], json!("quadrada_continua"));
    assert_eq!(obj["direcao_soldagem"], json!("esquerda_direita"));
    assert_eq!(obj["sentido_rotacao"], json!("horaria"));
    assert_eq!(obj["flat_orientation"], json!("X"));
    assert_eq!(obj["multiperfil_strategy"], json!("Passes separados"));
    assert_eq!(obj["multiperfil_profile_shape"], json!("Retangular"));
    assert_eq!(obj["bolacha_tipo"], json!("Espiral Contínua"));
    assert_eq!(obj["diametro"], json!(50.0));
    assert_eq!(obj["num_camadas"], json!(1));
    assert_eq!(obj["torch_retract_on_ignite"], json!(true));
    // `None` não é escrito (nem como `null`): o JSON fica como o do Python
    // antes de `_normalize_params`, sem as chaves.
    assert!(
        !obj.contains_key("taxa_de_deposicao"),
        "taxa_de_deposicao ausente quando None"
    );
    assert!(
        !obj.contains_key("app_version"),
        "app_version ausente quando None"
    );
    let with = Params {
        taxa_de_deposicao: Some(120.0),
        app_version: Some("11.3.2".to_string()),
        ..Params::default()
    };
    let v = serde_json::to_value(&with).expect("serialize");
    assert_eq!(v["taxa_de_deposicao"], json!(120.0));
    assert_eq!(v["app_version"], json!("11.3.2"));
    assert!(obj.contains_key("velocidade_de_deposicao"));
    assert!(
        !obj.contains_key("velocidade_soldagem"),
        "alias legado não é escrito"
    );
    assert!(
        !obj.contains_key("internal_entry_x"),
        "alias legado não é escrito"
    );
    assert!(!obj.contains_key("operation"));

    // Sem oscilação, a chave oscillation_type não é escrita (como o Python).
    let d = serde_json::to_value(Params::default()).expect("serialize default");
    assert!(
        d.get("oscillation_type").is_none(),
        "default omite oscillation_type"
    );
    assert_eq!(d["welding_mode"], json!("espiral"));

    // Todos os valores de enum do Python desserializam.
    let all = json!({
        "direcao_soldagem": "direita_esquerda", "sentido_rotacao": "antihoraria",
        "multiperfil_strategy": "Percurso contínuo", "multiperfil_profile_shape": "Trapezoidal",
        "bolacha_tipo": "Anéis (Passo a Passo)", "flat_orientation": "Y",
        "welding_mode": "oscilacao_quadrada", "oscillation_type": "quadrada"
    });
    let q: Params = serde_json::from_value(all).expect("enums Python");
    assert_eq!(q.welding_mode, WeldingMode::OscilacaoQuadrada);
    assert_eq!(q.oscillation_type, Some(OscillationType::Quadrada));
    assert_eq!(q.direcao_soldagem, Direction::DireitaEsquerda);
    assert_eq!(q.sentido_rotacao, Rotation::Antihoraria);
    assert_eq!(
        q.multiperfil_strategy,
        MultiperfilStrategy::PercursoContinuo
    );
    assert_eq!(q.multiperfil_profile_shape, ProfileShape::Trapezoidal);
    assert_eq!(q.bolacha_tipo, BolachaTipo::Aneis);
    assert_eq!(q.flat_orientation, FlatOrientation::Y);
    assert_eq!(Operation::from_params(&q), Operation::OscilacaoQuadrada);
}
