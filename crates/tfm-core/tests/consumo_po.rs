//! Consumo de pó: volume depositado × densidade do pó ÷ rendimento de deposição.
//!
//! A conta herdada do Python multiplicava a área de um cilindro por um fator sem
//! unidade definida (0,16) e dava 0,188 kg em todas as operações. Agora o volume
//! vem da geometria de cada operação (área revestida × espessura × camadas), a
//! densidade é a do pó escolhido e o rendimento é o que o pó realmente vira
//! cordão (87% no padrão, valor típico de PTA).

use std::f64::consts::PI;

use tfm_core::config::AppConfig;
use tfm_core::estimates::compute;
use tfm_core::params::{Operation, Params};

fn params(op: Operation) -> Params {
    let mut p = Params::default();
    p.apply_operation(op);
    p
}

fn kg(p: &Params) -> f64 {
    compute(p, &AppConfig::default()).costs.powder_kg
}

/// kg de pó para um volume depositado (mm³) com a densidade e o rendimento de `p`.
fn fisico(volume_mm3: f64, p: &Params) -> f64 {
    volume_mm3 * p.densidade_po_g_cm3 / 1000.0 / (p.rendimento_deposicao_perc / 100.0) / 1000.0
}

fn assert_near(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() <= b.abs() * 1e-6, "{what}: {a} contra {b}");
}

#[test]
fn defaults_are_stellite_density_and_typical_pta_yield() {
    let p = Params::default();
    assert_eq!(p.densidade_po_g_cm3, 8.46);
    assert_eq!(p.rendimento_deposicao_perc, 87.0);
}

#[test]
fn each_operation_uses_its_own_coated_area() {
    let esp = params(Operation::Espiral);
    let area = PI * 50.0 * 50.0;
    assert_near(
        kg(&esp),
        fisico(area * 1.5, &esp),
        "espiral: lateral do cilindro",
    );
    // Cerca de 115 g de pó para 100 g depositados.
    assert!((kg(&esp) - 0.1146).abs() < 0.001, "{}", kg(&esp));

    let chapa = params(Operation::ChapaPlana);
    assert_near(
        kg(&chapa),
        fisico(100.0 * 50.0 * 1.5, &chapa),
        "chapa: largura × comprimento",
    );

    let mut bolacha = params(Operation::Bolacha);
    assert_near(
        kg(&bolacha),
        fisico(PI * 25.0 * 25.0 * 1.5, &bolacha),
        "bolacha: face do disco",
    );
    bolacha.diametro_interno = 20.0;
    assert_near(
        kg(&bolacha),
        fisico(PI * (25.0 * 25.0 - 10.0 * 10.0) * 1.5, &bolacha),
        "bolacha com furo",
    );

    for op in [Operation::OscilacaoLinear, Operation::OscilacaoQuadrada] {
        let p = params(op);
        assert_near(kg(&p), fisico(area * 1.5, &p), &format!("{op:?}"));
    }
    let multi = params(Operation::EspiralMultiperfil);
    assert!(kg(&multi) > 0.0 && kg(&multi).is_finite());
}

#[test]
fn mass_scales_with_thickness_layers_density_and_yield() {
    let p = params(Operation::Espiral);
    let base = kg(&p);
    let mut q = p.clone();
    q.espessura_camada = 3.0;
    assert_near(kg(&q), base * 2.0, "espessura em dobro");
    let mut q = p.clone();
    q.num_camadas = 3;
    assert_near(kg(&q), base * 3.0, "três camadas");
    let mut q = p.clone();
    q.densidade_po_g_cm3 = 14.5;
    assert_near(kg(&q), base * 14.5 / 8.46, "carbeto mais denso");
    let mut q = p.clone();
    q.rendimento_deposicao_perc = 43.5;
    assert_near(kg(&q), base * 2.0, "metade do rendimento");
}

/// O custo do pó segue a massa.
#[test]
fn powder_cost_follows_the_mass() {
    let p = params(Operation::Bolacha);
    let e = compute(&p, &AppConfig::default());
    assert_near(
        e.costs.powder_cost,
        e.costs.powder_kg * p.powder_cost_brl_kg,
        "custo",
    );
}

/// Rendimento ou densidade inválidos não viram infinito: valem o padrão.
#[test]
fn invalid_yield_or_density_fall_back_to_defaults() {
    let mut p = params(Operation::Espiral);
    let padrao = kg(&p);
    p.rendimento_deposicao_perc = 0.0;
    p.densidade_po_g_cm3 = -1.0;
    let v = kg(&p);
    assert!(v.is_finite());
    assert_near(v, padrao, "volta ao padrão");
}

/// Densidades conhecidas dos pós padrão de PTA, pelo nome.
#[test]
fn known_densities_by_powder_name() {
    use tfm_core::config::densidade_conhecida;
    assert_eq!(densidade_conhecida("WC-Ni (Carbeto)"), Some(14.5));
    assert_eq!(densidade_conhecida("stellite 6"), Some(8.46));
    assert_eq!(densidade_conhecida("Inconel 625"), Some(8.44));
    assert_eq!(densidade_conhecida("Aço Inox 316L"), Some(7.99));
    assert_eq!(densidade_conhecida("Pó padrão"), None);
}

/// Configs antigos, sem densidade nem rendimento, continuam válidos; os campos
/// vazios não são gravados.
#[test]
fn powder_without_density_or_yield_loads_and_saves_clean() {
    use tfm_core::config::Powder;
    let p: Powder =
        serde_json::from_str(r#"{"name":"X","density_factor_g_mm2":0.2,"cost_brl_kg":10}"#)
            .expect("lê");
    assert_eq!(p.density_g_cm3, None);
    assert_eq!(p.yield_perc, None);
    let json = serde_json::to_string(&p).expect("grava");
    assert!(
        !json.contains("density_g_cm3") && !json.contains("yield_perc"),
        "{json}"
    );
    let q: Powder =
        serde_json::from_str(r#"{"name":"Y","density_g_cm3":14.5,"yield_perc":80}"#).expect("lê");
    assert_eq!((q.density_g_cm3, q.yield_perc), (Some(14.5), Some(80.0)));
}
