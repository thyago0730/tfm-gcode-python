//! Configuração do aplicativo (`config/config.json`).
//!
//! Porta de `src/app/core/config_manager.py`:
//!
//! * mesmos defaults e mesmas chaves do JSON do Python;
//! * [`AppConfig::load`] faz o merge recursivo do arquivo do usuário sobre os
//!   defaults (`_merge_config`: dicionários são mesclados chave a chave,
//!   qualquer outro valor substitui) e regrava o arquivo com a estrutura
//!   completa, como `ConfigManager.load()`;
//! * [`AppConfig::save`] grava com indent 4 (`json.dump(indent=4)`) e garante a
//!   pasta de procedimentos (`database.procedures_path`);
//! * objetos JSON são [`IndexMap`] e as seções `ui` e `costs` guardam as
//!   chaves desconhecidas em `extra`, para que regravar o arquivo do usuário
//!   não perca nada nem mude a ordem das chaves.
//!
//! Números e booleanos são lidos de forma leniente (ver `params::lenient_*`),
//! porque o arquivo pode ter sido editado à mão.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::de::{self, DeserializeOwned, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::params::{
    f64_from_value, i64_from_value, lenient_bool, lenient_f64, lenient_string, string_from_value,
};

// ---------------------------------------------------------------------------
// Utilitários de JSON compartilhados pelos módulos de persistência
// ---------------------------------------------------------------------------

/// Serializa como `json.dump(obj, indent=4)`: quatro espaços, `": "` entre
/// chave e valor, sem newline final. Texto UTF-8 sem escapes `\uXXXX`
/// (o Python lê os dois formatos igualmente).
pub fn json_pretty4<T: Serialize + ?Sized>(value: &T) -> serde_json::Result<String> {
    let mut buf = Vec::with_capacity(1024);
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
    value.serialize(&mut ser)?;
    String::from_utf8(buf).map_err(|e| serde::ser::Error::custom(e.to_string()))
}

/// Grava `value` em `path` com [`json_pretty4`], criando a pasta pai.
pub(crate) fn write_json_pretty4<T: Serialize + ?Sized>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            fs::create_dir_all(dir)?;
        }
    }
    let text = json_pretty4(value).map_err(io::Error::other)?;
    fs::write(path, text)
}

/// Lê e interpreta um arquivo JSON; `None` se não existe, não pode ser lido
/// ou não é JSON válido (o Python trata esses casos como "sem arquivo").
pub(crate) fn read_json_value(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// `deserialize_with` para `f32` leniente (número ou string numérica).
fn lenient_f32<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    f64_from_value(&Value::deserialize(d)?)
        .map(|f| f as f32)
        .map_err(de::Error::custom)
}

/// `deserialize_with` para `u32` leniente (número, float inteiro ou string);
/// negativos viram 0.
fn lenient_u32<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    i64_from_value(&Value::deserialize(d)?)
        .map(|i| u32::try_from(i).unwrap_or(0))
        .map_err(de::Error::custom)
}

/// `deserialize_with` para `IndexMap<String, String>` aceitando valores não
/// textuais (convertidos com `string_from_value`); valores inválidos são
/// descartados em vez de invalidar o mapa inteiro.
fn lenient_string_map<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<IndexMap<String, String>, D::Error> {
    let raw = IndexMap::<String, Value>::deserialize(d)?;
    Ok(raw
        .into_iter()
        .filter_map(|(k, v)| string_from_value(&v).ok().map(|s| (k, s)))
        .collect())
}

/// Desserializa cada elemento de um array separadamente, descartando os
/// inválidos (um pó quebrado não derruba a lista toda).
fn lenient_vec<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Vec<T>, D::Error> {
    let raw = Vec::<Value>::deserialize(d)?;
    Ok(raw
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect())
}

/// Idem para objetos JSON: entradas inválidas são descartadas.
fn lenient_map<'de, D: Deserializer<'de>, T: DeserializeOwned>(
    d: D,
) -> Result<IndexMap<String, T>, D::Error> {
    let raw = IndexMap::<String, Value>::deserialize(d)?;
    Ok(raw
        .into_iter()
        .filter_map(|(k, v)| serde_json::from_value(v).ok().map(|t| (k, t)))
        .collect())
}

// ---------------------------------------------------------------------------
// Seções
// ---------------------------------------------------------------------------

/// Seção `costs`. As chaves extras do arquivo do usuário (p.ex. os custos em
/// EUR) ficam em `extra` e voltam ao JSON na mesma posição relativa.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Costs {
    #[serde(deserialize_with = "lenient_f64")]
    pub powder_brl_kg: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub gas_argon_brl_m3: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub labor_brl_hour: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub machine_brl_hour: f64,
    #[serde(deserialize_with = "lenient_string")]
    pub currency_symbol: String,
    #[serde(flatten)]
    pub extra: IndexMap<String, Value>,
}

impl Default for Costs {
    fn default() -> Self {
        Costs {
            powder_brl_kg: 250.0,
            gas_argon_brl_m3: 50.0,
            labor_brl_hour: 40.0,
            machine_brl_hour: 30.0,
            currency_symbol: "R$".to_string(),
            extra: IndexMap::new(),
        }
    }
}

/// Um pó cadastrado em `powders`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Powder {
    #[serde(deserialize_with = "lenient_string")]
    pub name: String,
    #[serde(deserialize_with = "lenient_f64")]
    pub density_factor_g_mm2: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub cost_brl_kg: f64,
}

impl Default for Powder {
    /// O "Pó padrão" do `config_manager.py`.
    fn default() -> Self {
        Powder {
            name: "Pó padrão".to_string(),
            density_factor_g_mm2: 0.16,
            cost_brl_kg: 250.0,
        }
    }
}

/// As 20 expressões de `get_default_formulas` (TFM_GCODE.py), na mesma ordem
/// e com o mesmo texto. O `config/config.json` do usuário contém exatamente
/// estas expressões.
pub const DEFAULT_FORMULAS: [(&str, &str); 20] = [
    // Geometria e movimento
    ("circumference_mm", "math.pi * diametro"),
    (
        "rotation_rpm",
        "(velocidade_de_deposicao / max(circumference_mm, 1e-6)) * 60",
    ),
    ("rotation_mm_min", "circumference_mm * rotation_rpm"),
    ("helix_pitch_mm", "velocidade_de_deposicao / max(rotation_rpm, 1e-6)"),
    ("effective_step_mm", "step_over_mm"),
    (
        "axial_passes_per_layer",
        "math.ceil(comprimento_revestir / max(step_over_mm, 1e-6))",
    ),
    ("total_length_mm_per_layer", "circumference_mm * axial_passes_per_layer"),
    ("total_length_mm_all_layers", "total_length_mm_per_layer * num_camadas"),
    // Tempo e rotações
    (
        "time_min",
        "total_length_mm_all_layers / max(velocidade_de_deposicao, 1e-6)",
    ),
    ("rotations_total", "rotation_rpm * (time_min/60.0)"),
    // Consumos
    ("gas_consumption_l", "vazao_gas * time_min"),
    (
        "powder_mass_kg",
        "((circumference_mm * (comprimento_revestir + lead_in + lead_out) / 10000) * (num_camadas * espessura_camada) * powder_factor)",
    ),
    ("powder_mass_g", "powder_mass_kg * 1000"),
    ("taxa_deposicao_kg_h", "taxa_deposicao_g_h / 1000.0"),
    (
        "time_min_mass",
        "(powder_mass_kg / max(taxa_deposicao_kg_h, 1e-6)) * 60.0",
    ),
    // Custos
    ("powder_cost_brl", "powder_mass_kg * powder_cost_brl_kg"),
    ("labor_cost_brl", "(time_min/60.0) * labor_brl_hour"),
    ("machine_cost_brl", "(time_min/60.0) * machine_brl_hour"),
    ("gas_cost_brl", "(gas_consumption_l/1000.0) * gas_argon_brl_m3"),
    (
        "total_cost_brl",
        "powder_cost_brl + labor_cost_brl + machine_cost_brl + gas_cost_brl",
    ),
];

/// `notes` de `get_default_formulas`, verbatim.
pub const DEFAULT_FORMULA_NOTES: &str = "Variáveis disponíveis: diametro, comprimento_revestir, lead_in, lead_out, largura_cordao, sobreposicao, velocidade_de_deposicao, num_camadas, espessura_camada, vazao_gas, powder_factor (g/mm²), powder_cost_brl_kg, labor_brl_hour, machine_brl_hour, gas_argon_brl_m3, taxa_deposicao_g_h, taxa_deposicao_kg_h, time_min_mass. Funções: math, pi, sin, cos, tan, abs, round, min, max, ceil, floor. Consumo de pó calculado por área e espessura * fator.";

/// Seção `formulas`: expressões editáveis pelo usuário (avaliadas por
/// `crate::formula`) e a nota explicativa mostrada no editor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Formulas {
    #[serde(deserialize_with = "lenient_string_map")]
    pub expressions: IndexMap<String, String>,
    #[serde(deserialize_with = "lenient_string")]
    pub notes: String,
}

impl Default for Formulas {
    /// As 20 expressões padrão ([`AppConfig::default_formulas`]).
    fn default() -> Self {
        AppConfig::default_formulas()
    }
}

/// Seção `integration` (Mach3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Integration {
    #[serde(deserialize_with = "lenient_string")]
    pub mach3_path: String,
    #[serde(deserialize_with = "lenient_string")]
    pub gcode_dir: String,
    #[serde(deserialize_with = "lenient_bool")]
    pub write_autoload_txt: bool,
    #[serde(deserialize_with = "lenient_bool")]
    pub allow_file_association_fallback: bool,
    #[serde(deserialize_with = "lenient_string")]
    pub mach3_profile: String,
}

impl Default for Integration {
    fn default() -> Self {
        Integration {
            mach3_path: String::new(),
            gcode_dir: "C:/Mach3/GCode".to_string(),
            write_autoload_txt: true,
            allow_file_association_fallback: false,
            mach3_profile: "PTA".to_string(),
        }
    }
}

/// Limites físicos de um perfil de máquina (`machine_profiles.*.limits`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MachineLimits {
    #[serde(deserialize_with = "lenient_f64")]
    pub max_rpm_chuck: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub max_length_x: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub max_diameter: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub max_feed_linear: f64,
}

impl Default for MachineLimits {
    fn default() -> Self {
        MachineLimits {
            max_rpm_chuck: 100.0,
            max_length_x: 1500.0,
            max_diameter: 500.0,
            max_feed_linear: 5000.0,
        }
    }
}

/// Um perfil de máquina (`machine_profiles.<id>`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MachineProfile {
    #[serde(deserialize_with = "lenient_string")]
    pub name: String,
    pub limits: MachineLimits,
}

impl Default for MachineProfile {
    /// O perfil `default` ("Padrão") do Python.
    fn default() -> Self {
        MachineProfile {
            name: "Padrão".to_string(),
            limits: MachineLimits::default(),
        }
    }
}

/// Seção `ui`: preferências da interface egui. As chaves que só o Python
/// usava (`sashes`) ficam em `extra` para não se perderem ao regravar.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPrefs {
    #[serde(deserialize_with = "lenient_string")]
    pub theme: String,
    #[serde(deserialize_with = "lenient_f32")]
    pub left_w: f32,
    #[serde(deserialize_with = "lenient_f32")]
    pub right_w: f32,
    #[serde(deserialize_with = "lenient_bool")]
    pub left_pinned: bool,
    #[serde(deserialize_with = "lenient_bool")]
    pub right_pinned: bool,
    #[serde(deserialize_with = "lenient_bool")]
    pub bottom_collapsed: bool,
    /// `"auto"` ou um fator numérico em texto (`"1.25"`), como no Python.
    #[serde(deserialize_with = "lenient_string")]
    pub dpi_scaling: String,
    #[serde(deserialize_with = "lenient_u32")]
    pub compact_threshold_px: u32,
    #[serde(flatten)]
    pub extra: IndexMap<String, Value>,
}

impl Default for UiPrefs {
    fn default() -> Self {
        UiPrefs {
            theme: "auto".to_string(),
            left_w: 360.0,
            right_w: 390.0,
            left_pinned: true,
            right_pinned: true,
            bottom_collapsed: false,
            dpi_scaling: "auto".to_string(),
            compact_threshold_px: 560,
            extra: IndexMap::new(),
        }
    }
}

/// Seção `database`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Database {
    /// Pasta dos procedimentos salvos; relativa à pasta do `config.json`
    /// quando não é absoluta (ver [`AppConfig::procedures_dir`]).
    #[serde(deserialize_with = "lenient_string")]
    pub procedures_path: String,
}

impl Default for Database {
    fn default() -> Self {
        Database {
            procedures_path: "procedures".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// AppConfig
// ---------------------------------------------------------------------------

/// Nome do arquivo de configuração.
pub const FILE_NAME: &str = "config.json";
/// Nome da pasta que o contém (`config/config.json`).
pub const DIR_NAME: &str = "config";

/// Configuração completa, com as seções na ordem do JSON do Python.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub ui: UiPrefs,
    pub costs: Costs,
    #[serde(deserialize_with = "lenient_vec")]
    pub powders: Vec<Powder>,
    #[serde(deserialize_with = "lenient_string")]
    pub active_powder: String,
    pub formulas: Formulas,
    pub database: Database,
    pub integration: Integration,
    #[serde(deserialize_with = "lenient_map")]
    pub machine_profiles: IndexMap<String, MachineProfile>,
    #[serde(deserialize_with = "lenient_string")]
    pub active_machine_profile: String,
}

impl Default for AppConfig {
    /// `ConfigManager._get_default_config()` (com as 20 fórmulas completas e
    /// `procedures_path = "procedures"`).
    fn default() -> Self {
        let mut machine_profiles = IndexMap::new();
        machine_profiles.insert("default".to_string(), MachineProfile::default());
        AppConfig {
            ui: UiPrefs::default(),
            costs: Costs::default(),
            powders: vec![Powder::default()],
            active_powder: "Pó padrão".to_string(),
            formulas: Formulas::default(),
            database: Database::default(),
            integration: Integration::default(),
            machine_profiles,
            active_machine_profile: "default".to_string(),
        }
    }
}

/// Porta de `ConfigManager._merge_config`: objetos JSON são mesclados chave a
/// chave (uma chave já existente mantém a posição e recebe o novo valor;
/// chaves novas vão para o fim), qualquer outro tipo substitui o alvo.
pub fn merge_value(target: &mut Value, source: Value) {
    match (target, source) {
        (Value::Object(t), Value::Object(s)) => {
            for (k, v) in s {
                let both_objects = t.get(&k).is_some_and(Value::is_object) && v.is_object();
                if both_objects {
                    if let Some(existing) = t.get_mut(&k) {
                        merge_value(existing, v);
                    }
                } else {
                    t.insert(k, v);
                }
            }
        }
        (t, s) => *t = s,
    }
}

/// Desserializa uma seção do mapa mesclado; qualquer problema de tipo devolve
/// o default daquela seção (as demais não são afetadas).
fn section<T: DeserializeOwned + Default>(map: &Map<String, Value>, key: &str) -> T {
    map.get(key)
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

fn section_string(map: &Map<String, Value>, key: &str, fallback: String) -> String {
    map.get(key)
        .and_then(|v| string_from_value(v).ok())
        .unwrap_or(fallback)
}

impl AppConfig {
    /// As 20 expressões e a nota de `get_default_formulas` (TFM_GCODE.py).
    pub fn default_formulas() -> Formulas {
        Formulas {
            expressions: DEFAULT_FORMULAS
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            notes: DEFAULT_FORMULA_NOTES.to_string(),
        }
    }

    /// Caminho do `config.json`.
    ///
    /// * Em build de desenvolvimento (`debug_assertions`): a raiz do workspace
    ///   Cargo + `config/config.json`, como o Python não congelado.
    /// * Em release: ao lado do executável, na ordem do Python congelado
    ///   (`exe/config/config.json`, `exe/config.json`, `pai/config/config.json`,
    ///   `pai/config.json`); se nenhum existe, o primeiro candidato.
    pub fn resolve_path() -> PathBuf {
        if cfg!(debug_assertions) {
            return workspace_root().join(DIR_NAME).join(FILE_NAME);
        }
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf));
        let Some(exe_dir) = exe_dir else {
            return PathBuf::from(DIR_NAME).join(FILE_NAME);
        };
        let first = exe_dir.join(DIR_NAME).join(FILE_NAME);
        let mut candidates = vec![first.clone(), exe_dir.join(FILE_NAME)];
        if let Some(parent) = exe_dir.parent() {
            candidates.push(parent.join(DIR_NAME).join(FILE_NAME));
            candidates.push(parent.join(FILE_NAME));
        }
        candidates.into_iter().find(|c| c.exists()).unwrap_or(first)
    }

    /// Lê `path` e mescla sobre os defaults **sem** regravar o arquivo.
    /// Arquivo ausente, ilegível ou com JSON inválido => defaults.
    pub fn read(path: &Path) -> AppConfig {
        let mut merged = serde_json::to_value(AppConfig::default())
            .unwrap_or_else(|_| Value::Object(Map::new()));
        if let Some(user @ Value::Object(_)) = read_json_value(path) {
            merge_value(&mut merged, user);
        }
        Self::from_merged(merged)
    }

    /// `ConfigManager.load()`: lê, mescla e regrava o arquivo com a estrutura
    /// completa (criando-o quando não existe). Uma falha ao regravar não
    /// impede o uso da configuração lida, como no Python.
    pub fn load(path: &Path) -> AppConfig {
        let cfg = Self::read(path);
        let _ = cfg.save(path);
        cfg
    }

    /// `ConfigManager.save()`: grava com indent 4 e garante a pasta de
    /// procedimentos ([`procedures_dir`](Self::procedures_dir)); não
    /// conseguir criar essa pasta não falha o save.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let _ = fs::create_dir_all(self.procedures_dir(path));
        write_json_pretty4(path, self)
    }

    /// Pasta dos procedimentos: `database.procedures_path` absoluta, ou
    /// relativa à pasta que contém `config_path`.
    pub fn procedures_dir(&self, config_path: &Path) -> PathBuf {
        let p = Path::new(&self.database.procedures_path);
        if p.is_absolute() {
            return p.to_path_buf();
        }
        let base = config_path
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        base.join(p)
    }

    /// Perfil de máquina ativo (`machine_profiles[active_machine_profile]`).
    pub fn active_profile(&self) -> Option<&MachineProfile> {
        self.machine_profiles.get(&self.active_machine_profile)
    }

    /// `_get_active_machine_limits`: limites do perfil ativo; perfil
    /// desconhecido => limites padrão.
    pub fn active_limits(&self) -> MachineLimits {
        self.active_profile()
            .map(|p| p.limits)
            .unwrap_or_default()
    }

    /// Pó cadastrado com este nome.
    pub fn powder(&self, name: &str) -> Option<&Powder> {
        self.powders.iter().find(|p| p.name == name)
    }

    /// O pó ativo (`active_powder`), se cadastrado.
    pub fn active_powder(&self) -> Option<&Powder> {
        self.powder(&self.active_powder)
    }

    fn from_merged(merged: Value) -> AppConfig {
        let Value::Object(map) = merged else {
            return AppConfig::default();
        };
        let defaults = AppConfig::default();
        AppConfig {
            ui: section(&map, "ui"),
            costs: section(&map, "costs"),
            powders: map
                .get("powders")
                .cloned()
                .and_then(vec_from_value)
                .unwrap_or(defaults.powders),
            active_powder: section_string(&map, "active_powder", defaults.active_powder),
            formulas: section(&map, "formulas"),
            database: section(&map, "database"),
            integration: section(&map, "integration"),
            machine_profiles: map
                .get("machine_profiles")
                .cloned()
                .and_then(map_from_value)
                .unwrap_or(defaults.machine_profiles),
            active_machine_profile: section_string(
                &map,
                "active_machine_profile",
                defaults.active_machine_profile,
            ),
        }
    }
}

/// Cada elemento é desserializado separadamente; inválidos são descartados.
fn vec_from_value<T: DeserializeOwned>(v: Value) -> Option<Vec<T>> {
    let Value::Array(items) = v else { return None };
    Some(
        items
            .into_iter()
            .filter_map(|item| serde_json::from_value(item).ok())
            .collect(),
    )
}

/// Cada entrada é desserializada separadamente; inválidas são descartadas.
fn map_from_value<T: DeserializeOwned>(v: Value) -> Option<IndexMap<String, T>> {
    let Value::Object(entries) = v else { return None };
    Some(
        entries
            .into_iter()
            .filter_map(|(k, item)| serde_json::from_value(item).ok().map(|t| (k, t)))
            .collect(),
    )
}

/// Raiz do workspace Cargo (`CARGO_MANIFEST_DIR/../..`), canonicalizada e
/// sem o prefixo verbatim `\?\` do Windows.
fn workspace_root() -> PathBuf {
    let raw = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    match raw.canonicalize() {
        Ok(p) => strip_verbatim_prefix(p),
        Err(_) => normalize_lexically(&raw),
    }
}

fn strip_verbatim_prefix(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\?\UNC\") {
        return PathBuf::from(format!(r"\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(r"\?\") {
        return PathBuf::from(rest);
    }
    p
}

/// Resolve `.` e `..` sem tocar o sistema de arquivos.
fn normalize_lexically(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}
