//! "Baixar G-code": porta de `_download_gcode_clicked` (TFM_GCODE.py).
//!
//! No Python o botão gerava o programa, pedia a pasta e gravava
//! `<ordem de serviço>.tap`, com o nome limpo pela regra de
//! [`suggested_tap_name`]. Aqui o fluxo passa pelos mesmos bloqueios de
//! "Gerar G-code" (campos, regras físicas, ISO 3834), confere a checklist de
//! invariantes ([`gcode_checklist`]) num diálogo e só com tudo OK abre o
//! diálogo de salvar já com o nome sugerido. A gravação fica pendente do
//! alerta G53 como as outras ações ([`PendingAction::DownloadTap`]) e, ao
//! gravar, escreve ao lado do `.tap` o side-car `<nome>.audit.json`
//! ([`AuditSidecar`]) e registra no log de auditoria.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use tfm_core::audit::{isoformat, SOFTWARE_VERSION};
use tfm_core::config::json_pretty4;
use tfm_core::params::Params;

use crate::integration::{audit_user, file_name, tap_filename, tap_text};

/// Categoria do log de auditoria e do side-car do "Baixar G-code".
pub const AUDIT_CATEGORY_DOWNLOAD: &str = "GCode Download";

// ---------------------------------------------------------------------------
// Nome do arquivo
// ---------------------------------------------------------------------------

/// Nome sugerido para o `.tap`, pela regra do Python:
/// `re.sub(r"[^A-Za-z0-9._-]+", "_", ordem_servico).strip('_')` (cada
/// sequência de caracteres fora de `A-Z a-z 0-9 . _ -` vira um `_`, e os
/// `_` das pontas são removidos) mais `.tap`; se não sobrar nada,
/// `TFM_AAAAMMDD_HHMMSS.tap` com o instante `now`.
pub fn suggested_tap_name(ordem_servico: &str, now: &NaiveDateTime) -> String {
    let mut safe = String::with_capacity(ordem_servico.len());
    let mut pending_sep = false;
    for c in ordem_servico.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            if pending_sep {
                safe.push('_');
                pending_sep = false;
            }
            safe.push(c);
        } else {
            pending_sep = true;
        }
    }
    if pending_sep {
        safe.push('_');
    }
    let safe = safe.trim_matches('_');
    if safe.is_empty() {
        tap_filename(now)
    } else {
        format!("{safe}.tap")
    }
}

// ---------------------------------------------------------------------------
// Side-car de auditoria
// ---------------------------------------------------------------------------

/// Caminho do side-car: o `.tap` com a extensão trocada por `.audit.json`
/// (`OS-1.tap` -> `OS-1.audit.json`, ao lado).
pub fn sidecar_path(tap: &Path) -> PathBuf {
    let mut name = tap
        .file_stem()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    name.push(".audit.json");
    tap.with_file_name(name)
}

/// FNV-1a de 64 bits dos bytes: impressão digital estável do arquivo gravado,
/// sem dependência externa (não é criptográfica; serve para conferir se o
/// `.tap` na máquina é o que foi baixado).
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes
        .iter()
        .fold(OFFSET, |h, b| (h ^ u64::from(*b)).wrapping_mul(PRIME))
}

/// Conteúdo de `<nome>.audit.json`: o que o log de auditoria guarda de um
/// `.tap` gerado (data em `isoformat()`, categoria, usuário, versão) mais o
/// arquivo, operador, EPS, ordem de serviço, contagem de linhas, a impressão
/// digital dos bytes gravados e os parâmetros inteiros (as mesmas chaves de
/// um procedimento salvo).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuditSidecar {
    pub timestamp: String,
    pub category: String,
    pub file: String,
    pub operator: String,
    pub eps: String,
    pub ordem_servico: String,
    pub lines: usize,
    pub fnv1a64: String,
    pub user: String,
    pub software_version: String,
    pub params: Params,
}

/// Monta o side-car de `tap` para o programa `text` (linhas unidas por
/// quebras de linha simples)
/// gerado com `params` no instante `now`. A impressão digital é dos bytes
/// como vão para o disco ([`tap_text`]).
pub fn sidecar_for(tap: &Path, text: &str, params: &Params, now: &NaiveDateTime) -> AuditSidecar {
    AuditSidecar {
        timestamp: isoformat(now),
        category: AUDIT_CATEGORY_DOWNLOAD.to_string(),
        file: file_name(tap),
        operator: params.traceability_operator.clone(),
        eps: params.traceability_eps.clone(),
        ordem_servico: params.ordem_servico.clone(),
        lines: text.split('\n').count(),
        fnv1a64: format!("{:016x}", fnv1a64(tap_text(text).as_bytes())),
        user: audit_user(&params.traceability_operator).to_string(),
        software_version: SOFTWARE_VERSION.to_string(),
        params: params.clone(),
    }
}

/// O side-car como texto, com indent 4 (`json.dump(indent=4)`).
pub fn sidecar_json(sidecar: &AuditSidecar) -> serde_json::Result<String> {
    json_pretty4(sidecar)
}

/// Grava o side-car em `path`.
pub fn write_sidecar(path: &Path, sidecar: &AuditSidecar) -> io::Result<()> {
    let text = sidecar_json(sidecar).map_err(io::Error::other)?;
    fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::path::Path;
    use tfm_core::params::Params;

    fn fixed_time() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, 31)
            .and_then(|d| d.and_hms_opt(14, 30, 5))
            .expect("data fixa")
    }

    fn traced_params() -> Params {
        Params {
            traceability_operator: "Ana".to_string(),
            traceability_eps: "EPS-7".to_string(),
            ordem_servico: "OS-1".to_string(),
            ..Params::default()
        }
    }

    /// `re.sub(r"[^A-Za-z0-9._-]+", "_", os).strip('_')` + `.tap`, com o
    /// fallback `TFM_AAAAMMDD_HHMMSS.tap` quando não sobra nada.
    #[test]
    fn suggested_name_follows_python_rule() {
        let now = fixed_time();
        assert_eq!(suggested_tap_name("OS-1234", &now), "OS-1234.tap");
        assert_eq!(suggested_tap_name("OS 123/45", &now), "OS_123_45.tap");
        assert_eq!(suggested_tap_name("  OS-07.A ", &now), "OS-07.A.tap");
        assert_eq!(suggested_tap_name("peça nº 9", &now), "pe_a_n_9.tap");
        assert_eq!(suggested_tap_name("a***b", &now), "a_b.tap");
        assert_eq!(suggested_tap_name("", &now), "TFM_20261031_143005.tap");
        assert_eq!(suggested_tap_name("   ", &now), "TFM_20261031_143005.tap");
        assert_eq!(suggested_tap_name("___", &now), "TFM_20261031_143005.tap");
        assert_eq!(suggested_tap_name("***", &now), "TFM_20261031_143005.tap");
    }

    /// O side-car fica ao lado do `.tap`, com o mesmo nome e `.audit.json`.
    #[test]
    fn sidecar_sits_next_to_the_tap() {
        assert_eq!(
            sidecar_path(Path::new("C:/x/OS-1.tap")),
            Path::new("C:/x/OS-1.audit.json")
        );
        assert_eq!(
            sidecar_path(Path::new("OS-1.v2.tap")),
            Path::new("OS-1.v2.audit.json")
        );
        assert_eq!(
            sidecar_path(Path::new("sem_ext")),
            Path::new("sem_ext.audit.json")
        );
    }

    /// FNV-1a de 64 bits com os vetores conhecidos.
    #[test]
    fn fnv1a64_known_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x85944171f73967e8);
    }

    /// Campos do side-car: data no formato `isoformat()` do Python, operador,
    /// EPS, ordem de serviço, contagem de linhas, hash dos bytes gravados,
    /// usuário da auditoria, versão e os parâmetros inteiros.
    #[test]
    fn sidecar_fields_and_date_format() {
        let text = "G21\nM101\nG1 A5\n%";
        let sc = sidecar_for(
            Path::new("C:/x/OS-1.tap"),
            text,
            &traced_params(),
            &fixed_time(),
        );
        assert_eq!(sc.timestamp, "2026-10-31T14:30:05.000000");
        assert_eq!(sc.category, AUDIT_CATEGORY_DOWNLOAD);
        assert_eq!(sc.file, "OS-1.tap");
        assert_eq!(sc.operator, "Ana");
        assert_eq!(sc.eps, "EPS-7");
        assert_eq!(sc.ordem_servico, "OS-1");
        assert_eq!(sc.lines, 4);
        assert_eq!(
            sc.fnv1a64,
            format!("{:016x}", fnv1a64(tap_text(text).as_bytes()))
        );
        assert_eq!(sc.user, "Ana");
        assert_eq!(sc.software_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(sc.params.ordem_servico, "OS-1");

        // Sem operador, o usuário da auditoria é "Engenharia" (como no log).
        let sc = sidecar_for(
            Path::new("OS-1.tap"),
            text,
            &Params::default(),
            &fixed_time(),
        );
        assert_eq!(sc.operator, "");
        assert_eq!(sc.user, "Engenharia");
    }

    /// Serialização: indent 4 como `json.dump(indent=4)`, chaves na ordem
    /// fixa e os parâmetros como objeto aninhado.
    #[test]
    fn sidecar_json_has_fixed_keys_and_indent4() {
        let sc = sidecar_for(
            Path::new("OS-1.tap"),
            "G21\n%",
            &traced_params(),
            &fixed_time(),
        );
        let json = sidecar_json(&sc).expect("serializa");
        assert!(
            json.starts_with("{\n    \"timestamp\": \"2026-10-31T14:30:05.000000\",\n"),
            "{json}"
        );
        let value: serde_json::Value = serde_json::from_str(&json).expect("JSON válido");
        let keys: Vec<&str> = value
            .as_object()
            .expect("objeto")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "timestamp",
                "category",
                "file",
                "operator",
                "eps",
                "ordem_servico",
                "lines",
                "fnv1a64",
                "user",
                "software_version",
                "params",
            ]
        );
        assert_eq!(value["lines"], 2);
        assert_eq!(value["params"]["ordem_servico"], "OS-1");
        assert_eq!(value["params"]["traceability_eps"], "EPS-7");
        let back: AuditSidecar = serde_json::from_str(&json).expect("lê de volta");
        assert_eq!(back, sc);
    }
}
