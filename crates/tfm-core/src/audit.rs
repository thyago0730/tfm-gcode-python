//! Log de auditoria de engenharia (ISO 3834).
//!
//! Porta da classe `AuditLogger` (TFM_GCODE.py): um arquivo JSON por dia
//! (`LOG_ENG_AAAA-MM-DD.json`) com um array de entradas
//! `{timestamp, category, detail, user, software_version}`, gravado com
//! indent 4. Como no Python, registrar nunca falha: [`AuditLogger::log`]
//! engole erros de E/S (use [`AuditLogger::try_log`] para vê-los) e um
//! arquivo corrompido é recomeçado do zero.

use std::fs;
use std::io;
use std::path::PathBuf;

use chrono::{Local, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{read_json_value, write_json_pretty4};

/// Versão gravada em `software_version`: a do crate (o Python gravava
/// `APP_VERSION`, "11.3.3").
pub const SOFTWARE_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Usuário padrão das entradas (`user="Engenharia"` no Python).
pub const DEFAULT_USER: &str = "Engenharia";
/// Pasta padrão dos logs (`logs_dir="data/logs"` no Python).
pub const DEFAULT_LOGS_DIR: &str = "data/logs";

/// `datetime.isoformat()` do Python para um instante local sem fuso:
/// `AAAA-MM-DDTHH:MM:SS.ffffff` (microssegundos sempre com seis dígitos).
pub fn isoformat(now: &NaiveDateTime) -> String {
    now.format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
}

/// Uma entrada do log, na ordem de chaves do Python.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    /// `datetime.now().isoformat()`: `AAAA-MM-DDTHH:MM:SS.ffffff` (hora local,
    /// sem fuso).
    pub timestamp: String,
    pub category: String,
    pub detail: String,
    pub user: String,
    pub software_version: String,
}

/// Registrador de auditoria: um arquivo por dia em `dir`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditLogger {
    pub dir: PathBuf,
}

impl Default for AuditLogger {
    /// `AuditLogger()` do Python: `data/logs` relativo ao diretório atual.
    fn default() -> Self {
        AuditLogger::new(DEFAULT_LOGS_DIR)
    }
}

impl AuditLogger {
    /// Cria o registrador e garante a pasta (falha ao criar é ignorada, como
    /// no Python; o `log` tentará de novo).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let _ = fs::create_dir_all(&dir);
        AuditLogger { dir }
    }

    /// `LOG_ENG_AAAA-MM-DD.json`.
    pub fn file_name_for(date: NaiveDate) -> String {
        format!("LOG_ENG_{}.json", date.format("%Y-%m-%d"))
    }

    /// Caminho do arquivo de um dia.
    pub fn path_for(&self, date: NaiveDate) -> PathBuf {
        self.dir.join(Self::file_name_for(date))
    }

    /// Caminho do arquivo de hoje.
    pub fn path_for_today(&self) -> PathBuf {
        self.path_for(Local::now().date_naive())
    }

    /// Registra uma entrada; erros são engolidos (`AuditLogger.log` do Python).
    pub fn log(&self, category: &str, detail: &str, user: &str) {
        let _ = self.try_log(category, detail, user);
    }

    /// Registra uma entrada com a hora atual e devolve-a, ou o erro de E/S.
    pub fn try_log(&self, category: &str, detail: &str, user: &str) -> io::Result<AuditEntry> {
        self.try_log_at(Local::now().naive_local(), category, detail, user)
    }

    /// Registra uma entrada com o instante `now` (útil em testes). O arquivo
    /// do dia é lido, a entrada é acrescentada e o array é regravado; um
    /// arquivo ilegível ou corrompido é tratado como vazio.
    pub fn try_log_at(
        &self,
        now: NaiveDateTime,
        category: &str,
        detail: &str,
        user: &str,
    ) -> io::Result<AuditEntry> {
        let path = self.path_for(now.date());
        let entry = AuditEntry {
            timestamp: isoformat(&now),
            category: category.to_string(),
            detail: detail.to_string(),
            user: user.to_string(),
            software_version: SOFTWARE_VERSION.to_string(),
        };
        let mut logs = match read_json_value(&path) {
            Some(Value::Array(items)) => items,
            _ => Vec::new(),
        };
        logs.push(serde_json::to_value(&entry).map_err(io::Error::other)?);
        write_json_pretty4(&path, &logs)?;
        Ok(entry)
    }

    /// Entradas registradas em um dia (as que não puderem ser interpretadas
    /// são ignoradas); vazio se o arquivo não existe.
    pub fn read_entries(&self, date: NaiveDate) -> Vec<AuditEntry> {
        match read_json_value(&self.path_for(date)) {
            Some(Value::Array(items)) => items
                .into_iter()
                .filter_map(|v| serde_json::from_value(v).ok())
                .collect(),
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `datetime.isoformat()`: microssegundos sempre com seis dígitos.
    #[test]
    fn isoformat_matches_python() {
        let t = NaiveDate::from_ymd_opt(2026, 10, 31)
            .and_then(|d| d.and_hms_micro_opt(14, 30, 5, 120))
            .expect("data fixa");
        assert_eq!(isoformat(&t), "2026-10-31T14:30:05.000120");
    }
}
