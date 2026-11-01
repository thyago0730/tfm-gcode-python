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

use chrono::{Local, NaiveDateTime};
use serde::{Deserialize, Serialize};
use tfm_core::audit::{isoformat, SOFTWARE_VERSION};
use tfm_core::config::{json_pretty4, Integration};
use tfm_core::params::Params;
use tfm_core::validator::{checklist_passed, gcode_checklist};

use crate::app::{NotifyKind, TfmApp};
use crate::dialogs::checklist::ChecklistState;
use crate::dialogs::DialogId;
use crate::integration::{
    audit_user, current_gcode, file_name, home_gcode_dir, join_lines, or_na, run_or_confirm,
    tap_filename, tap_text, validated_params, with_default_extension, write_text, PendingAction,
};

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

/// Detalhe do log de auditoria do download, no estilo do Python:
/// `Arquivo baixado: {nome} | Auditoria: {side-car} | EPS: {eps} | Op: {operador}`
/// (EPS e operador vazios saem como `N/A`).
pub fn download_audit_detail(file: &str, sidecar: &str, eps: &str, operator: &str) -> String {
    format!(
        "Arquivo baixado: {file} | Auditoria: {sidecar} | EPS: {} | Op: {}",
        or_na(eps),
        or_na(operator)
    )
}

// ---------------------------------------------------------------------------
// Fluxo
// ---------------------------------------------------------------------------

/// Botão/menu "Baixar G-code": porta de `_download_gcode_clicked` com os
/// bloqueios de "Gerar G-code" ([`validated_params`]). Com o programa em
/// mãos, roda a checklist de invariantes e abre o diálogo com o resultado;
/// com falha, salvar fica bloqueado e o erro vai para o histórico. Salvar de
/// fato é [`choose_and_save`], chamado pelo diálogo.
pub fn download_tap(app: &mut TfmApp) {
    let Some(params) = validated_params(app) else {
        return;
    };
    let Some(lines) = current_gcode(app, &params) else {
        return;
    };
    let items = gcode_checklist(&lines, &params);
    if !checklist_passed(&items) {
        let failures: Vec<&str> = items
            .iter()
            .flat_map(|item| item.failures.iter().map(String::as_str))
            .collect();
        let message = format!(
            "Baixar G-code bloqueado pela checklist: {}",
            failures.join(" | ")
        );
        log::warn!("{message}");
        app.notify(NotifyKind::Erro, message);
    }
    app.dialogs.checklist = ChecklistState {
        items,
        text: join_lines(&lines),
        params,
    };
    app.dialogs.open(DialogId::DownloadChecklist);
}

/// Pasta inicial do diálogo de salvar: a `gcode_dir` do config ou, vazia,
/// `~/Mach3/GCode` (o `initialdir` do Python); só quando a pasta já existe.
fn initial_dir(cfg: &Integration) -> Option<PathBuf> {
    let configured = cfg.gcode_dir.trim();
    let dir = if configured.is_empty() {
        home_gcode_dir()?
    } else {
        PathBuf::from(configured)
    };
    dir.is_dir().then_some(dir)
}

/// "Salvar .tap" no diálogo da checklist: abre o diálogo de salvar com o
/// nome sugerido e deixa a gravação pendente do alerta G53 quando o programa
/// usa G53. Cancelar o diálogo não avisa nada (como o Python).
pub(crate) fn choose_and_save(app: &mut TfmApp, state: &ChecklistState) {
    let now = Local::now().naive_local();
    let mut dialog = rfd::FileDialog::new()
        .set_title("Baixar G-code")
        .add_filter("G-Code Files", &["tap"])
        .add_filter("All Files", &["*"])
        .set_file_name(state.suggested_name(&now));
    if let Some(dir) = initial_dir(&app.cfg.integration) {
        dialog = dialog.set_directory(dir);
    }
    let Some(path) = dialog.save_file() else {
        return;
    };
    let action = PendingAction::DownloadTap {
        path: with_default_extension(path, "tap"),
        text: state.text.clone(),
        params: state.params.clone(),
    };
    run_or_confirm(app, action);
}

/// Grava o `.tap` (avisando como o Python: "Arquivo G-Code salvo: ..." ou
/// "Erro ao salvar G-Code: ..."), depois o side-car ao lado e a entrada do
/// log de auditoria. Um side-car que não pôde ser gravado vira aviso, não
/// erro: o `.tap` já está no disco e o log registra "não gravado".
pub(crate) fn commit_download(app: &mut TfmApp, path: PathBuf, text: &str, params: &Params) {
    if let Err(err) = write_text(&path, text) {
        app.notify(NotifyKind::Erro, format!("Erro ao salvar G-Code: {err}"));
        app.audit.log(
            "Critical Error",
            &format!("Falha ao baixar G-Code: {err}"),
            "System",
        );
        return;
    }
    log::info!("G-code baixado em {}", path.display());
    app.notify(
        NotifyKind::Sucesso,
        format!("Arquivo G-Code salvo: {}", path.display()),
    );
    let sidecar = sidecar_for(&path, text, params, &Local::now().naive_local());
    let sidecar_file = sidecar_path(&path);
    let sidecar_label = match write_sidecar(&sidecar_file, &sidecar) {
        Ok(()) => file_name(&sidecar_file),
        Err(err) => {
            app.notify(
                NotifyKind::Aviso,
                format!(
                    "O .tap foi salvo, mas o registro {} não pôde ser gravado: {err}",
                    file_name(&sidecar_file)
                ),
            );
            "não gravado".to_string()
        }
    };
    let detail = download_audit_detail(
        &sidecar.file,
        &sidecar_label,
        &params.traceability_eps,
        &params.traceability_operator,
    );
    app.audit.log(
        AUDIT_CATEGORY_DOWNLOAD,
        &detail,
        audit_user(&params.traceability_operator),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::path::Path;
    use tfm_core::params::Params;
    use tfm_core::validator::ChecklistItem;

    use crate::compute::{params_key, ComputeResult};

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

    /// Detalhe do log de auditoria do download, no estilo do Python
    /// (`N/A` para EPS e operador vazios).
    #[test]
    fn download_audit_detail_format() {
        assert_eq!(
            download_audit_detail("OS-1.tap", "OS-1.audit.json", "EPS-7", "Ana"),
            "Arquivo baixado: OS-1.tap | Auditoria: OS-1.audit.json | EPS: EPS-7 | Op: Ana"
        );
        assert_eq!(
            download_audit_detail("OS-1.tap", "não gravado", "", " "),
            "Arquivo baixado: OS-1.tap | Auditoria: não gravado | EPS: N/A | Op: N/A"
        );
    }

    /// `commit_download` grava o `.tap`, o side-car ao lado, avisa como o
    /// Python ("Arquivo G-Code salvo: ...") e registra na auditoria.
    #[test]
    fn commit_download_writes_tap_sidecar_and_audit() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut app = TfmApp::headless(tmp.path());
        let params = traced_params();
        let text = "G21\nM101\nG1 A5\n%";
        let path = tmp.path().join("saida").join("OS-1.tap");
        commit_download(&mut app, path.clone(), text, &params);

        assert_eq!(
            fs::read(&path).expect(".tap gravado"),
            tap_text(text).as_bytes()
        );
        let sidecar = sidecar_path(&path);
        let json = fs::read_to_string(&sidecar).expect("side-car gravado ao lado");
        let back: AuditSidecar = serde_json::from_str(&json).expect("side-car válido");
        assert_eq!(back.file, "OS-1.tap");
        assert_eq!(back.lines, 4);
        assert_eq!(back.params, params);
        let last = app.notifications.last().expect("aviso registrado");
        assert_eq!(last.kind, NotifyKind::Sucesso);
        assert_eq!(
            last.text,
            format!("Arquivo G-Code salvo: {}", path.display())
        );
        let entries = app.audit.read_entries(chrono::Local::now().date_naive());
        let entry = entries.last().expect("auditoria registrada");
        assert_eq!(entry.category, AUDIT_CATEGORY_DOWNLOAD);
        assert_eq!(entry.user, "Ana");
        assert_eq!(
            entry.detail,
            "Arquivo baixado: OS-1.tap | Auditoria: OS-1.audit.json | EPS: EPS-7 | Op: Ana"
        );
    }

    /// Falha ao gravar o `.tap`: a mensagem do Python, nada de side-car.
    #[test]
    fn commit_download_reports_write_errors() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut app = TfmApp::headless(tmp.path());
        let blocker = tmp.path().join("bloqueio");
        fs::write(&blocker, b"x").expect("arquivo no lugar da pasta");
        let path = blocker.join("OS-1.tap");
        commit_download(&mut app, path.clone(), "G21\n%", &traced_params());
        let last = app.notifications.last().expect("erro registrado");
        assert_eq!(last.kind, NotifyKind::Erro);
        assert!(
            last.text.starts_with("Erro ao salvar G-Code: "),
            "{}",
            last.text
        );
        assert!(!sidecar_path(&path).exists());
    }

    /// Operador com qualificação vencida: o ISO 3834 bloqueia antes da
    /// checklist, como em "Gerar G-code"; nenhum diálogo abre.
    #[test]
    fn download_is_blocked_by_iso3834_like_generate_tap() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut app = TfmApp::headless(tmp.path());
        app.quality.add_operator("Ana", "OP-1", "Q-1", "2000-01-01");
        app.ui
            .fields
            .insert("traceability_operator", "Ana".to_string());
        download_tap(&mut app);
        let last = app.notifications.last().expect("erro registrado");
        assert_eq!(last.kind, NotifyKind::Erro);
        assert!(last.text.starts_with("BLOQUEIO ISO 3834"), "{}", last.text);
        assert!(!app.dialogs.is_open(DialogId::DownloadChecklist));
        assert!(app.integration.pending.is_none());
    }

    /// Checklist com falha: o diálogo abre com os itens marcados, salvar
    /// fica bloqueado e o erro vai para o histórico.
    #[test]
    fn download_blocks_save_when_checklist_fails() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut app = TfmApp::headless(tmp.path());
        let params = app.current_params();
        let key = params_key(&params, &app.cfg);
        let mut result = ComputeResult::failed(params, key, "");
        result.gen_error = None;
        result.lines = ["G1 X1", "M101", "G1 A10", "G1 A5", "M102"]
            .map(String::from)
            .to_vec();
        app.result = Some(result);

        download_tap(&mut app);
        assert!(app.dialogs.is_open(DialogId::DownloadChecklist));
        let state = &app.dialogs.checklist;
        assert_eq!(state.items.len(), 3);
        assert!(state.items[0].is_ok());
        assert!(!state.items[1].is_ok());
        assert!(!state.items[2].is_ok());
        assert!(!state.can_save());
        let last = app.notifications.last().expect("erro registrado");
        assert_eq!(last.kind, NotifyKind::Erro);
        assert!(
            last.text
                .starts_with("Baixar G-code bloqueado pela checklist: "),
            "{}",
            last.text
        );
        assert!(app.integration.pending.is_none());
    }

    /// Tudo OK: o diálogo abre com os três itens em OK, guarda o programa e
    /// os parâmetros, sugere o nome pela ordem de serviço e nada é avisado.
    #[test]
    fn download_opens_checklist_ready_to_save() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut app = TfmApp::headless(tmp.path());
        app.ui.fields.insert("ordem_servico", "OS 42/7".to_string());
        download_tap(&mut app);
        assert!(app.dialogs.is_open(DialogId::DownloadChecklist));
        let state = &app.dialogs.checklist;
        assert!(state.can_save());
        assert!(state.items.iter().all(ChecklistItem::is_ok));
        assert!(state.text.contains("M101"));
        assert_eq!(state.params.ordem_servico, "OS 42/7");
        assert_eq!(state.suggested_name(&fixed_time()), "OS_42_7.tap");
        assert!(app.notifications.is_empty());
        assert!(app.integration.pending.is_none());
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
