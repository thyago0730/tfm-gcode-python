//! Procedimentos salvos (`*.json` na pasta `database.procedures_path`).
//!
//! Porta de `_save_procedure` / `_load_procedure_file` (TFM_GCODE.py):
//!
//! * [`save_procedure`] grava **todos** os parâmetros com indent 4 e, no modo
//!   unificado `oscilacao`, garante `oscillation_type` (default `linear`);
//! * [`load_procedure`] é leniente como a UI Tkinter: números como string,
//!   chaves legadas e desconhecidas, strings de display da combobox
//!   (`"Horária (CW)"`), valores de enum inválidos (voltam ao default, como a
//!   UI fazia em silêncio) e os `setdefault` do Python para chaves ausentes
//!   ([`LOAD_DEFAULTS`]).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::config::write_json_pretty4;
use crate::params::{
    BolachaTipo, Direction, FlatOrientation, MultiperfilStrategy, OscillationType, Params,
    ProfileShape, Rotation, WeldingMode,
};

/// Erro ao carregar um procedimento.
#[derive(Debug, thiserror::Error)]
pub enum ProcedureError {
    /// O arquivo não pôde ser lido.
    #[error("não foi possível ler {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// O texto não é JSON válido.
    #[error("JSON inválido: {0}")]
    Json(#[source] serde_json::Error),
    /// O JSON é válido mas não é um objeto (`{...}`).
    #[error("o procedimento não é um objeto JSON")]
    NotAnObject,
    /// Algum valor não pôde ser convertido para [`Params`] (ex.: texto não
    /// numérico em um campo numérico).
    #[error("parâmetros inválidos: {0}")]
    Params(#[source] serde_json::Error),
}

/// Defaults que `_load_procedure_file` aplica com `setdefault` às chaves
/// ausentes (valores em texto, como a UI Tkinter guardava).
pub const LOAD_DEFAULTS: [(&str, &str); 7] = [
    ("lead_in", "5.0"),
    ("lead_out", "5.0"),
    ("direcao_soldagem", "esquerda_direita"),
    ("welding_mode", "espiral"),
    ("oscilacao_comprimento", "10.0"),
    ("deslocamento_angular_perc", "50.0"),
    ("sentido_rotacao", "horaria"),
];

/// Grava o procedimento como `_save_procedure`: todos os parâmetros, indent 4,
/// e `oscillation_type = linear` quando o modo é `oscilacao` e o tipo está
/// ausente. Cria a pasta pai se necessário.
pub fn save_procedure(path: &Path, p: &Params) -> io::Result<()> {
    let mut data = p.clone();
    if data.welding_mode == WeldingMode::Oscilacao && data.oscillation_type.is_none() {
        data.oscillation_type = Some(OscillationType::Linear);
    }
    write_json_pretty4(path, &data)
}

/// Lê e interpreta um procedimento (ver [`params_from_value`]).
pub fn load_procedure(path: &Path) -> Result<Params, ProcedureError> {
    let text = fs::read_to_string(path).map_err(|source| ProcedureError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse_procedure(&text)
}

/// Interpreta o texto JSON de um procedimento (ver [`params_from_value`]).
pub fn parse_procedure(text: &str) -> Result<Params, ProcedureError> {
    let value: Value = serde_json::from_str(text).map_err(ProcedureError::Json)?;
    params_from_value(value)
}

/// Converte o JSON já carregado em [`Params`] com a leniência do Python:
///
/// 1. enums com valor desconhecido são removidos (vale o default) e as
///    strings de display da UI antiga são mapeadas para os valores internos;
/// 2. [`LOAD_DEFAULTS`] preenche as chaves ausentes;
/// 3. o restante segue a desserialização leniente de [`Params`] (números como
///    string, `velocidade_soldagem`, chaves desconhecidas ignoradas).
pub fn params_from_value(value: Value) -> Result<Params, ProcedureError> {
    let Value::Object(mut obj) = value else {
        return Err(ProcedureError::NotAnObject);
    };
    sanitize_enums(&mut obj);
    for (key, default) in LOAD_DEFAULTS {
        obj.entry(key)
            .or_insert_with(|| Value::String(default.to_string()));
    }
    serde_json::from_value(Value::Object(obj)).map_err(ProcedureError::Params)
}

/// Strings de display da combobox Tkinter que podem aparecer em arquivos
/// antigos, mapeadas para o valor interno.
fn sanitize_enums(obj: &mut Map<String, Value>) {
    sanitize::<Direction>(
        obj,
        "direcao_soldagem",
        &[
            ("Esquerda -> Direita", "esquerda_direita"),
            ("Direita -> Esquerda", "direita_esquerda"),
        ],
    );
    sanitize::<Rotation>(
        obj,
        "sentido_rotacao",
        &[("Horária (CW)", "horaria"), ("Anti-horária (CCW)", "antihoraria")],
    );
    sanitize::<WeldingMode>(obj, "welding_mode", &[]);
    sanitize::<OscillationType>(
        obj,
        "oscillation_type",
        &[
            ("Linear", "linear"),
            ("Quadrada", "quadrada"),
            ("Quadrada Contínua", "quadrada_continua"),
        ],
    );
    sanitize::<FlatOrientation>(obj, "flat_orientation", &[("x", "X"), ("y", "Y")]);
    sanitize::<MultiperfilStrategy>(obj, "multiperfil_strategy", &[]);
    sanitize::<ProfileShape>(obj, "multiperfil_profile_shape", &[]);
    sanitize::<BolachaTipo>(obj, "bolacha_tipo", &[]);
}

/// Mantém `key` se o valor já é aceito pelo enum `T`; senão tenta o texto
/// aparado, em minúsculas e os `aliases` de display (sem distinguir
/// maiúsculas); sem correspondência, remove a chave para valer o default.
fn sanitize<T: DeserializeOwned>(obj: &mut Map<String, Value>, key: &str, aliases: &[(&str, &str)]) {
    let Some(current) = obj.get(key) else { return };
    if accepts::<T>(current) {
        return;
    }
    let replacement = current.as_str().and_then(|s| {
        let trimmed = s.trim();
        let lower = trimmed.to_lowercase();
        if accepts::<T>(&Value::String(trimmed.to_string())) {
            return Some(trimmed.to_string());
        }
        if accepts::<T>(&Value::String(lower.clone())) {
            return Some(lower);
        }
        aliases
            .iter()
            .find(|(display, _)| display.to_lowercase() == lower)
            .map(|(_, internal)| internal.to_string())
    });
    match replacement {
        Some(s) => {
            obj.insert(key.to_string(), Value::String(s));
        }
        None => {
            obj.remove(key);
        }
    }
}

fn accepts<T: DeserializeOwned>(v: &Value) -> bool {
    serde_json::from_value::<T>(v.clone()).is_ok()
}
