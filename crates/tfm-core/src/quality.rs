//! Gestão da qualidade (ISO 3834): operadores, EPS/WPS, manutenção
//! preventiva, materiais e cálculos de processo.
//!
//! Porta de `src/app/core/quality_manager.py`. Arquivos:
//!
//! * `operators.json` e `wps.json` em `data_dir` (`data/quality` no Python);
//! * `maintenance_log.json` e `materials.json` em `config_dir`.
//!
//! As mensagens de bloqueio (`BLOQUEIO ISO 3834: ...`) e os textos de alerta
//! são idênticos aos do Python, inclusive na formatação dos números
//! (`repr(float)`: `180.0`, `100.0 - 140.0`). Como no Python, os `save_*`
//! engolem erros de E/S.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDate};
use indexmap::IndexMap;
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{read_json_value, write_json_pretty4};
use crate::params::{lenient_f64, lenient_string, BolachaOscEixo, Params, WeldingMode};

/// Operadores (em `data_dir`).
pub const OPERATORS_FILE: &str = "operators.json";
/// EPS/WPS (em `data_dir`).
pub const WPS_FILE: &str = "wps.json";
/// Contadores de manutenção (em `config_dir`).
pub const MAINTENANCE_FILE: &str = "maintenance_log.json";
/// Materiais (em `config_dir`).
pub const MATERIALS_FILE: &str = "materials.json";
/// Formato da validade da qualificação (`strptime("%Y-%m-%d")`).
pub const DATE_FMT: &str = "%Y-%m-%d";

// ---------------------------------------------------------------------------
// Formatação/conversão com a semântica do Python
// ---------------------------------------------------------------------------

/// `repr(float)` do Python nas mensagens: valores inteiros saem com `.0`.
fn py_float(x: f64) -> String {
    if x.is_nan() {
        "nan".to_string()
    } else if x.is_infinite() {
        (if x > 0.0 { "inf" } else { "-inf" }).to_string()
    } else if x == x.trunc() && x.abs() < 1e16 {
        format!("{x:.1}")
    } else {
        format!("{x}")
    }
}

/// `float(x)` do Python: número, string numérica (ponto decimal) ou bool;
/// qualquer outra coisa (`""`, `None`, objeto) é erro => `None`.
fn py_float_of(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// `{limit_hours}` do Python: o default é `int` (`20h`), então valores
/// inteiros saem sem casas decimais.
fn fmt_limit(hours: f64) -> String {
    if hours == hours.trunc() && hours.abs() < 1e16 {
        format!("{hours:.0}")
    } else {
        format!("{hours}")
    }
}

/// `deserialize_with` para um objeto JSON qualquer; valores que não são
/// objeto (ex.: `null`) viram mapa vazio (`params or {}`).
fn lenient_object<'de, D: Deserializer<'de>>(d: D) -> Result<IndexMap<String, Value>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Object(map) => map.into_iter().collect(),
        _ => IndexMap::new(),
    })
}

/// Array JSON de um arquivo, ou vazio se ausente/inválido/não-array.
fn read_array(path: &Path) -> Vec<Value> {
    match read_json_value(path) {
        Some(Value::Array(items)) => items,
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Operadores
// ---------------------------------------------------------------------------

/// Operador qualificado (`operators.json`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "OperatorFile", into = "OperatorFile")]
pub struct Operator {
    pub name: String,
    pub code: String,
    pub qualification_id: String,
    /// Validade da qualificação, `AAAA-MM-DD` (vazia = sem validade).
    pub qualification_validity: String,
}

/// Forma no arquivo: o Python grava a validade em `qualification_validity` e
/// na chave legada `validity`; na leitura vale a primeira não vazia.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct OperatorFile {
    #[serde(deserialize_with = "lenient_string")]
    name: String,
    #[serde(deserialize_with = "lenient_string")]
    code: String,
    #[serde(deserialize_with = "lenient_string")]
    qualification_id: String,
    #[serde(
        deserialize_with = "lenient_string",
        skip_serializing_if = "String::is_empty"
    )]
    qualification_validity: String,
    #[serde(
        deserialize_with = "lenient_string",
        skip_serializing_if = "String::is_empty"
    )]
    validity: String,
}

impl From<OperatorFile> for Operator {
    fn from(f: OperatorFile) -> Self {
        let validity = [f.qualification_validity.as_str(), f.validity.as_str()]
            .into_iter()
            .map(str::trim)
            .find(|s| !s.is_empty())
            .unwrap_or("")
            .to_string();
        Operator {
            name: f.name,
            code: f.code,
            qualification_id: f.qualification_id,
            qualification_validity: validity,
        }
    }
}

impl From<Operator> for OperatorFile {
    fn from(o: Operator) -> Self {
        OperatorFile {
            name: o.name,
            code: o.code,
            qualification_id: o.qualification_id,
            qualification_validity: o.qualification_validity.clone(),
            validity: o.qualification_validity,
        }
    }
}

// ---------------------------------------------------------------------------
// EPS / WPS
// ---------------------------------------------------------------------------

/// Faixa admissível de um parâmetro na EPS (`limits.<param>`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Limit {
    pub min: f64,
    pub max: f64,
}

/// EPS/WPS cadastrada (`wps.json`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Wps {
    #[serde(deserialize_with = "lenient_string")]
    pub name: String,
    /// Parâmetros de referência da EPS (livres, como no Python).
    #[serde(deserialize_with = "lenient_object")]
    pub params: IndexMap<String, Value>,
    /// Faixas por nome de parâmetro de [`Params`]; só gravado se não vazio.
    #[serde(
        deserialize_with = "lenient_limits",
        skip_serializing_if = "IndexMap::is_empty"
    )]
    pub limits: IndexMap<String, Limit>,
}

/// `limits` do arquivo: entradas sem `min`/`max` numéricos são descartadas
/// (o Python as ignora com `except (ValueError, TypeError, KeyError)`).
fn lenient_limits<'de, D: Deserializer<'de>>(d: D) -> Result<IndexMap<String, Limit>, D::Error> {
    let raw = match Value::deserialize(d)? {
        Value::Object(map) => map,
        _ => return Ok(IndexMap::new()),
    };
    Ok(raw
        .into_iter()
        .filter_map(|(k, v)| {
            let min = py_float_of(v.get("min")?)?;
            let max = py_float_of(v.get("max")?)?;
            Some((k, Limit { min, max }))
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Manutenção preventiva
// ---------------------------------------------------------------------------

/// Componente de desgaste com contador de horas de arco.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Component {
    #[serde(deserialize_with = "lenient_f64")]
    pub limit_hours: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub current_hours: f64,
}

impl Component {
    /// Componente zerado com este limite de horas.
    pub fn with_limit(limit_hours: f64) -> Self {
        Component {
            limit_hours,
            current_hours: 0.0,
        }
    }

    /// `current_hours >= limit_hours`.
    pub fn needs_maintenance(&self) -> bool {
        self.current_hours >= self.limit_hours
    }

    /// `current_hours >= 90 %` do limite.
    pub fn near_limit(&self) -> bool {
        self.current_hours >= self.limit_hours * 0.9
    }
}

/// Registro do histórico de manutenção (`history[]`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryEntry {
    /// `AAAA-MM-DD HH:MM`.
    #[serde(deserialize_with = "lenient_string")]
    pub date: String,
    /// Ex.: `Reset bico_constritor`.
    #[serde(deserialize_with = "lenient_string")]
    pub action: String,
    /// `total_arc_time_minutes` no momento da ação.
    #[serde(deserialize_with = "lenient_f64")]
    pub total_time: f64,
}

/// Conteúdo de `maintenance_log.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Maintenance {
    #[serde(deserialize_with = "lenient_f64")]
    pub total_arc_time_minutes: f64,
    /// `AAAA-MM-DD`; o default é a data de hoje, como no Python.
    #[serde(deserialize_with = "lenient_string")]
    pub last_maintenance_date: String,
    /// `bico_constritor` (20 h), `eletrodo` (40 h), `mangueiras` (500 h).
    pub components: IndexMap<String, Component>,
    pub history: Vec<HistoryEntry>,
}

impl Default for Maintenance {
    fn default() -> Self {
        Maintenance {
            total_arc_time_minutes: 0.0,
            last_maintenance_date: Local::now().format(DATE_FMT).to_string(),
            components: Maintenance::default_components(),
            history: Vec::new(),
        }
    }
}

impl Maintenance {
    /// Os três componentes padrão do Python, na mesma ordem.
    pub fn default_components() -> IndexMap<String, Component> {
        [
            ("bico_constritor", 20.0),
            ("eletrodo", 40.0),
            ("mangueiras", 500.0),
        ]
        .into_iter()
        .map(|(name, hours)| (name.to_string(), Component::with_limit(hours)))
        .collect()
    }

    /// Soma `minutes` ao total e `minutes / 60` às horas de cada componente.
    pub fn log_arc_time(&mut self, minutes: f64) {
        self.total_arc_time_minutes += minutes;
        let hours = minutes / 60.0;
        for component in self.components.values_mut() {
            component.current_hours += hours;
        }
    }

    /// Zera o componente e registra no histórico; `false` se não existe.
    pub fn reset_component(&mut self, name: &str) -> bool {
        let Some(component) = self.components.get_mut(name) else {
            return false;
        };
        component.current_hours = 0.0;
        self.history.push(HistoryEntry {
            date: Local::now().format("%Y-%m-%d %H:%M").to_string(),
            action: format!("Reset {name}"),
            total_time: self.total_arc_time_minutes,
        });
        true
    }

    /// `check_maintenance_alerts`: um texto por componente no limite
    /// (`Manutenção Necessária: ...`) ou acima de 90 % (`Atenção: ...`).
    pub fn alerts(&self) -> Vec<String> {
        self.components
            .iter()
            .filter_map(|(name, c)| {
                if c.needs_maintenance() {
                    Some(format!(
                        "Manutenção Necessária: {name} ({:.1}/{}h)",
                        c.current_hours,
                        fmt_limit(c.limit_hours)
                    ))
                } else if c.near_limit() {
                    Some(format!(
                        "Atenção: {name} próximo do limite ({:.1}/{}h)",
                        c.current_hours,
                        fmt_limit(c.limit_hours)
                    ))
                } else {
                    None
                }
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Materiais
// ---------------------------------------------------------------------------

/// Material/pó de referência do módulo de qualidade (`materials.json`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Material {
    #[serde(deserialize_with = "lenient_f64")]
    pub density: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub cost_per_kg: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub hardness_hrc: f64,
    #[serde(deserialize_with = "lenient_string")]
    pub description: String,
}

impl Material {
    fn new(density: f64, cost_per_kg: f64, hardness_hrc: f64, description: &str) -> Self {
        Material {
            density,
            cost_per_kg,
            hardness_hrc,
            description: description.to_string(),
        }
    }
}

/// Os quatro materiais padrão de `_load_materials`, na mesma ordem.
pub fn default_materials() -> IndexMap<String, Material> {
    [
        (
            "Inconel 625",
            Material::new(8.44, 350.0, 22.0, "Liga de Níquel resistente à corrosão"),
        ),
        (
            "Stellite 6",
            Material::new(8.46, 450.0, 42.0, "Liga de Cobalto resistente ao desgaste"),
        ),
        (
            "WC-Ni (Carbeto)",
            Material::new(14.5, 600.0, 55.0, "Compósito metal-cerâmico"),
        ),
        (
            "Aço Inox 316L",
            Material::new(7.99, 80.0, 18.0, "Aço inoxidável austenítico"),
        ),
    ]
    .into_iter()
    .map(|(name, m)| (name.to_string(), m))
    .collect()
}

// ---------------------------------------------------------------------------
// QualityManager
// ---------------------------------------------------------------------------

/// Estado do módulo de qualidade: operadores, EPS, manutenção e materiais,
/// com as pastas de onde foram lidos.
#[derive(Clone, Debug, PartialEq)]
pub struct QualityManager {
    /// Pasta de `maintenance_log.json` e `materials.json`.
    pub config_dir: PathBuf,
    /// Pasta de `operators.json` e `wps.json`.
    pub data_dir: PathBuf,
    pub operators: Vec<Operator>,
    pub wps: Vec<Wps>,
    pub maintenance: Maintenance,
    pub materials: IndexMap<String, Material>,
}

impl QualityManager {
    /// `QualityManager(config_path, data_dir)`: cria as duas pastas (erros
    /// ignorados) e lê os quatro arquivos; cada um ausente ou inválido vale o
    /// default. Entradas de operador/EPS sem nome ou malformadas são
    /// descartadas (`_normalize_operators`).
    pub fn load(config_dir: &Path, data_dir: &Path) -> Self {
        let _ = fs::create_dir_all(config_dir);
        let _ = fs::create_dir_all(data_dir);
        let operators = read_array(&data_dir.join(OPERATORS_FILE))
            .into_iter()
            .filter_map(|v| serde_json::from_value::<Operator>(v).ok())
            .filter(|op| !op.name.is_empty())
            .collect();
        let wps = read_array(&data_dir.join(WPS_FILE))
            .into_iter()
            .filter_map(|v| serde_json::from_value::<Wps>(v).ok())
            .filter(|w| !w.name.is_empty())
            .collect();
        let maintenance = read_json_value(&config_dir.join(MAINTENANCE_FILE))
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        let materials = read_json_value(&config_dir.join(MATERIALS_FILE))
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_else(default_materials);
        QualityManager {
            config_dir: config_dir.to_path_buf(),
            data_dir: data_dir.to_path_buf(),
            operators,
            wps,
            maintenance,
            materials,
        }
    }

    /// `data_dir/operators.json`.
    pub fn operators_path(&self) -> PathBuf {
        self.data_dir.join(OPERATORS_FILE)
    }

    /// `data_dir/wps.json`.
    pub fn wps_path(&self) -> PathBuf {
        self.data_dir.join(WPS_FILE)
    }

    /// `config_dir/maintenance_log.json`.
    pub fn maintenance_path(&self) -> PathBuf {
        self.config_dir.join(MAINTENANCE_FILE)
    }

    /// `config_dir/materials.json`.
    pub fn materials_path(&self) -> PathBuf {
        self.config_dir.join(MATERIALS_FILE)
    }

    /// Grava `operators.json` (indent 4); erros de E/S são engolidos.
    pub fn save_operators(&self) {
        let _ = write_json_pretty4(&self.operators_path(), &self.operators);
    }

    /// Grava `wps.json` (indent 4); erros de E/S são engolidos.
    pub fn save_wps(&self) {
        let _ = write_json_pretty4(&self.wps_path(), &self.wps);
    }

    /// Grava `maintenance_log.json` (indent 4); erros de E/S são engolidos.
    pub fn save_maintenance(&self) {
        let _ = write_json_pretty4(&self.maintenance_path(), &self.maintenance);
    }

    /// Grava `materials.json`; `true` se conseguiu (como o Python).
    pub fn save_materials(&self) -> bool {
        write_json_pretty4(&self.materials_path(), &self.materials).is_ok()
    }

    // --- Operadores --------------------------------------------------------

    /// Operador pelo nome exato.
    pub fn operator(&self, name: &str) -> Option<&Operator> {
        self.operators.iter().find(|op| op.name == name)
    }

    /// Nomes dos operadores em ordem alfabética.
    pub fn operator_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.operators.iter().map(|op| op.name.clone()).collect();
        names.sort();
        names
    }

    /// `add_operator`: atualiza os campos **não vazios** de um operador
    /// existente ou cadastra um novo; grava o arquivo.
    pub fn add_operator(
        &mut self,
        name: &str,
        code: &str,
        qualification_id: &str,
        qualification_validity: &str,
    ) {
        match self.operators.iter_mut().find(|op| op.name == name) {
            Some(op) => {
                if !code.is_empty() {
                    op.code = code.to_string();
                }
                if !qualification_id.is_empty() {
                    op.qualification_id = qualification_id.to_string();
                }
                if !qualification_validity.is_empty() {
                    op.qualification_validity = qualification_validity.to_string();
                }
            }
            None => self.operators.push(Operator {
                name: name.to_string(),
                code: code.to_string(),
                qualification_id: qualification_id.to_string(),
                qualification_validity: qualification_validity.to_string(),
            }),
        }
        self.save_operators();
    }

    /// `upsert_operator`: sobrescreve `qualification_id` (mesmo vazio) e a
    /// validade se informada; cadastra se não existe; grava o arquivo.
    pub fn upsert_operator(
        &mut self,
        name: &str,
        qualification_id: &str,
        qualification_validity: &str,
    ) {
        match self.operators.iter_mut().find(|op| op.name == name) {
            Some(op) => {
                op.qualification_id = qualification_id.to_string();
                if !qualification_validity.is_empty() {
                    op.qualification_validity = qualification_validity.to_string();
                }
            }
            None => self.operators.push(Operator {
                name: name.to_string(),
                code: String::new(),
                qualification_id: qualification_id.to_string(),
                qualification_validity: qualification_validity.to_string(),
            }),
        }
        self.save_operators();
    }

    /// Remove o operador (se existir) e grava o arquivo.
    pub fn remove_operator(&mut self, name: &str) {
        self.operators.retain(|op| op.name != name);
        self.save_operators();
    }

    /// `check_operator_qualification` com a data de hoje.
    pub fn check_operator_qualification(&self, name: &str) -> Result<(), String> {
        self.check_operator_qualification_on(name, Local::now().date_naive())
    }

    /// Bloqueia só quando o operador existe, tem validade no formato
    /// `AAAA-MM-DD` e ela é anterior a `today`. Nome vazio, operador
    /// desconhecido, validade vazia ou ilegível não bloqueiam.
    pub fn check_operator_qualification_on(
        &self,
        name: &str,
        today: NaiveDate,
    ) -> Result<(), String> {
        if name.is_empty() {
            return Ok(());
        }
        let Some(op) = self.operator(name) else {
            return Ok(());
        };
        let validity_str = op.qualification_validity.trim();
        if validity_str.is_empty() {
            return Ok(());
        }
        match NaiveDate::parse_from_str(validity_str, DATE_FMT) {
            Ok(validity) if validity < today => Err(format!(
                "BLOQUEIO ISO 3834: Qualificação do operador '{name}' vencida em {validity_str}."
            )),
            _ => Ok(()),
        }
    }

    // --- EPS / WPS ---------------------------------------------------------

    /// EPS pelo nome exato.
    pub fn wps(&self, name: &str) -> Option<&Wps> {
        self.wps.iter().find(|w| w.name == name)
    }

    /// Nomes das EPS em ordem alfabética.
    pub fn wps_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.wps.iter().map(|w| w.name.clone()).collect();
        names.sort();
        names
    }

    /// `add_wps`: cadastra a EPS ou, se já existe, substitui `params` quando
    /// informado e não vazio; grava o arquivo.
    pub fn add_wps(&mut self, name: &str, params: Option<IndexMap<String, Value>>) {
        let params = params.filter(|p| !p.is_empty());
        match self.wps.iter_mut().find(|w| w.name == name) {
            Some(w) => {
                if let Some(p) = params {
                    w.params = p;
                }
            }
            None => self.wps.push(Wps {
                name: name.to_string(),
                params: params.unwrap_or_default(),
                limits: IndexMap::new(),
            }),
        }
        self.save_wps();
    }

    /// Remove a EPS (se existir) e grava o arquivo.
    pub fn remove_wps(&mut self, name: &str) {
        self.wps.retain(|w| w.name != name);
        self.save_wps();
    }

    // --- ISO 3834 ----------------------------------------------------------

    /// `validate_iso3834`: na ordem do Python,
    ///
    /// 1. qualificação do operador (`traceability_operator`);
    /// 2. sem EPS informada ou EPS não cadastrada => liberado;
    /// 3. cada `limits.<param>` da EPS é comparado ao campo homônimo de
    ///    [`Params`] (campos ausentes/nulos/não numéricos são pulados);
    /// 4. com EPS ativa, lote do pó, fornecedor e certificado são
    ///    obrigatórios (espaços contam como vazio).
    ///
    /// As mensagens começam com `BLOQUEIO ISO 3834:` e são as do Python.
    pub fn validate_iso3834(&self, p: &Params) -> Result<(), String> {
        self.check_operator_qualification(&p.traceability_operator)?;

        let eps_name = p.traceability_eps.as_str();
        if eps_name.is_empty() {
            return Ok(());
        }
        let Some(wps) = self.wps(eps_name) else {
            return Ok(());
        };

        let values = serde_json::to_value(p).unwrap_or(Value::Null);
        for (param, limit) in &wps.limits {
            let Some(raw) = values.get(param) else {
                continue;
            };
            let Some(f_val) = py_float_of(raw) else {
                continue;
            };
            if f_val < limit.min || f_val > limit.max {
                return Err(format!(
                    "BLOQUEIO ISO 3834: Parâmetro '{param}' ({}) fora da faixa da EPS ({} - {}).",
                    py_float(f_val),
                    py_float(limit.min),
                    py_float(limit.max)
                ));
            }
        }

        let mandatory = [
            (&p.traceability_powder_batch, "Lote do Pó"),
            (&p.traceability_supplier, "Fornecedor"),
            (&p.traceability_certificate, "Certificado"),
        ];
        let missing: Vec<&str> = mandatory
            .iter()
            .filter(|(value, _)| value.trim().is_empty())
            .map(|(_, label)| *label)
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "BLOQUEIO ISO 3834: Campos obrigatórios para EPS ativa: {}.",
                missing.join(", ")
            ));
        }
        Ok(())
    }

    // --- Manutenção --------------------------------------------------------

    /// `log_arc_time`: acumula minutos de arco em todos os componentes e grava.
    pub fn log_arc_time(&mut self, minutes: f64) {
        self.maintenance.log_arc_time(minutes);
        self.save_maintenance();
    }

    /// `reset_component`: zera o componente, registra no histórico e grava;
    /// nome desconhecido não faz nada.
    pub fn reset_component(&mut self, name: &str) {
        if self.maintenance.reset_component(name) {
            self.save_maintenance();
        }
    }

    /// `check_maintenance_alerts` (ver [`Maintenance::alerts`]).
    pub fn maintenance_alerts(&self) -> Vec<String> {
        self.maintenance.alerts()
    }

    // --- Materiais ---------------------------------------------------------

    /// Material pelo nome.
    pub fn material(&self, name: &str) -> Option<&Material> {
        self.materials.get(name)
    }

    /// Nomes dos materiais na ordem do arquivo (`list(materials.keys())`).
    pub fn material_names(&self) -> Vec<String> {
        self.materials.keys().cloned().collect()
    }

    /// `add_material`: cadastra/substitui e grava `materials.json`.
    pub fn add_material(
        &mut self,
        name: &str,
        density: f64,
        cost_per_kg: f64,
        hardness_hrc: f64,
        description: &str,
    ) {
        self.materials.insert(
            name.to_string(),
            Material::new(density, cost_per_kg, hardness_hrc, description),
        );
        self.save_materials();
    }

    // --- Cálculos de qualidade ---------------------------------------------

    /// Aporte térmico em kJ/mm: `(V * I * 60 * eta) / v / 1000`, com `v` em
    /// mm/min; `0.0` se `v <= 0`.
    pub fn heat_input_kj_mm(voltage: f64, current: f64, speed_mm_min: f64, efficiency: f64) -> f64 {
        if speed_mm_min <= 0.0 {
            return 0.0;
        }
        let heat_j_mm = (voltage * current * 60.0 * efficiency) / speed_mm_min;
        heat_j_mm / 1000.0
    }

    /// Avisos do aporte térmico e da velocidade LOCAIS do programa, que
    /// `process_warnings` não vê porque usa a velocidade pedida. Com EPS, valem
    /// as faixas dela (`aporte_termico` em kJ/mm e `velocidade_de_deposicao` em
    /// mm/min); sem faixa de aporte, as gerais de `process_warnings` (0,1 a
    /// 3,0 kJ/mm). Chaves desconhecidas da EPS são ignoradas.
    pub fn local_alerts(
        limits: Option<&IndexMap<String, Limit>>,
        local: &crate::deposit::LocalProcess,
    ) -> Vec<String> {
        let mut out = Vec::new();
        match limits.and_then(|m| m.get("aporte_termico")) {
            Some(lim) => {
                if local.heat_max > lim.max {
                    out.push(format!(
                        "Aporte térmico local de {:.2} kJ/mm fora da faixa da EPS ({:.2} - {:.2}).",
                        local.heat_max, lim.min, lim.max
                    ));
                }
                if local.heat_min < lim.min {
                    out.push(format!(
                        "Aporte térmico local de {:.2} kJ/mm fora da faixa da EPS ({:.2} - {:.2}).",
                        local.heat_min, lim.min, lim.max
                    ));
                }
            }
            None => {
                if local.heat_max > 3.0 {
                    out.push(format!(
                        "Aporte térmico local muito alto ({:.2} kJ/mm). Risco de distorção/diluição excessiva.",
                        local.heat_max
                    ));
                }
                if local.heat_min < 0.1 {
                    out.push(format!(
                        "Aporte térmico local muito baixo ({:.2} kJ/mm). Risco de falta de fusão.",
                        local.heat_min
                    ));
                }
            }
        }
        if let Some(lim) = limits.and_then(|m| m.get("velocidade_de_deposicao")) {
            for speed in [local.speed_max, local.speed_min] {
                if speed > lim.max || speed < lim.min {
                    out.push(format!(
                        "Velocidade local de {speed:.0} mm/min fora da faixa da EPS ({:.0} - {:.0}).",
                        lim.min, lim.max
                    ));
                }
            }
        }
        out
    }

    /// Diluição estimada (%) pela heurística do Python:
    /// `clamp(2.5 * (I / pó_g_min) - 2.0, 0, 100)`; `0.0` se não há pó.
    /// A velocidade não entra no modelo, mas é mantida pela assinatura.
    pub fn estimate_dilution(current: f64, powder_g_min: f64, _speed_mm_min: f64) -> f64 {
        if powder_g_min <= 0.0 {
            return 0.0;
        }
        let ratio = current / powder_g_min;
        (2.5 * ratio - 2.0).clamp(0.0, 100.0)
    }

    /// `validate_parameters`: `(warnings, errors)` com os textos do Python.
    /// A velocidade avaliada é `taxa_de_deposicao` (espelho de
    /// `velocidade_de_deposicao`, ver [`Params::taxa`]); o aporte térmico usa
    /// a eficiência padrão 0.8, como o Python.
    pub fn process_warnings(p: &Params) -> (Vec<String>, Vec<String>) {
        const MAX_SPEED: f64 = 5000.0;
        let mut warnings = Vec::new();
        let mut errors = Vec::new();

        let feed = p.taxa();
        if feed > MAX_SPEED {
            errors.push(format!(
                "Velocidade de soldagem ({} mm/min) excede o limite da máquina ({}).",
                py_float(feed),
                py_float(MAX_SPEED)
            ));
        }

        let osc_width = p.oscilacao_comprimento;
        if osc_width > 0.0 {
            let freq = feed / (2.0 * osc_width * 60.0);
            if freq > 5.0 {
                warnings.push(format!(
                    "Frequência de oscilação estimada ({freq:.1} Hz) pode ser muito alta para a mesa."
                ));
            }
        }

        if p.welding_mode == WeldingMode::Bolacha
            && p.bolacha_oscilar
            && p.bolacha_osc_periodo_mm > 0.0
        {
            let freq = feed / (60.0 * p.bolacha_osc_periodo_mm);
            if freq > 5.0 {
                warnings.push(format!(
                    "Frequência de oscilação estimada ({freq:.1} Hz) pode ser muito alta para a mesa."
                ));
            }
            // Em Y a tocha corre junto da mesa: a velocidade relativa à peça
            // vai de `1 - 2L/P` a `1 + 2L/P` da pedida, e com `2L/P >= 1` ela
            // chega a zero ou muda de sentido.
            let razao = 2.0 * p.oscilacao_comprimento / p.bolacha_osc_periodo_mm;
            if p.bolacha_osc_eixo == BolachaOscEixo::Y && razao >= 1.0 {
                warnings.push(format!(
                    "Oscilação em Y: a tocha chega a parar ou recuar em relação à peça                      (2 × comprimento ÷ período = {razao:.1}). Aumente o período ou reduza o comprimento."
                ));
            }
        }

        let current = p.corrente_arco;
        if current > 0.0 && feed > 0.0 {
            let hi = Self::heat_input_kj_mm(p.tensao_arco, current, feed, 0.8);
            if hi > 3.0 {
                warnings.push(format!(
                    "Aporte Térmico muito alto ({hi:.2} kJ/mm). Risco de distorção/diluição excessiva."
                ));
            } else if hi < 0.1 {
                warnings.push(format!(
                    "Aporte Térmico muito baixo ({hi:.2} kJ/mm). Risco de falta de fusão."
                ));
            }
        }

        (warnings, errors)
    }
}
