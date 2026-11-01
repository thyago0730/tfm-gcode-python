//! Ações da janela principal: gerar o `.tap`, abrir no Mach3, salvar e
//! carregar procedimentos, e os atalhos de teclado.
//!
//! Porta de `_generate_gcode_clicked`, `_open_in_mach3_clicked`,
//! `_save_procedure`/`_load_procedure` do app Python (`TFM_GCODE.py`), com
//! as mesmas mensagens e a mesma ordem de validação
//! (`validate_physical` -> ISO 3834 -> geração -> diálogo de arquivo).
//!
//! O alerta de segurança G53 era um `messagebox.askyesno`; aqui ele precisa
//! sobreviver a vários quadros, então a ação pendente (gravar o `.tap` ou
//! abrir no Mach3) fica em [`IntegrationState::pending`] (campo `integration`
//! do `TfmApp`) e o modal é desenhado por [`show_modals`] a cada quadro.
//! "Abrir no Mach3" passa pelos mesmos bloqueios de "Gerar G-code" (campos,
//! física, ISO 3834 e o alerta G53) e também registra auditoria; "Baixar
//! G-code" (módulo `download`) reutiliza os mesmos portões e a mesma ação
//! pendente. Os macros do
//! Mach3 (`_ensure_mach3_macros`) são [`mach3_path::install_macros`],
//! reutilizado.
//!
//! Nada aqui bloqueia a interface além dos diálogos nativos de arquivo: a
//! regravação atrasada do `autoload.txt` e o fallback de associação de
//! arquivo rodam em threads próprias.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use chrono::NaiveDateTime;
use egui::{Key, Modifiers};
use tfm_core::config::Integration;
use tfm_core::gcode::{self, GenError};
use tfm_core::params::Params;
use tfm_core::procedure::{load_procedure, save_procedure};
use tfm_core::validator::validate_physical;

use crate::app::{NotifyKind, TfmApp};
use crate::compute::params_key;
use crate::dialogs::{mach3_path, DialogId};
use crate::panels::right::copy_gcode;
use crate::ui_state::UiState;
use crate::view3d::{self, ViewPreset};

const BACKSLASH: char = 92u8 as char;

/// Título do alerta de segurança G53 (o `messagebox.askyesno` do Python).
pub const G53_TITLE: &str = "SEGURANÇA DE MÁQUINA";
/// Corpo do alerta de segurança G53.
pub const G53_BODY: &str = "Este programa usa G53 (Coordenadas de Máquina) para recuo seguro.\n\n\
A MÁQUINA ESTÁ REFERENCIADA (REF ALL HOME)?\n\n\
Se não estiver, pode haver COLISÃO.\n\
Deseja continuar?";
/// Categoria do log de auditoria da geração manual.
pub const AUDIT_CATEGORY: &str = "GCode Generation (Manual)";
/// Categoria do log de auditoria de "Abrir no Mach3".
pub const AUDIT_CATEGORY_MACH3: &str = "GCode Generation (Mach3)";
/// Aviso quando o trabalhador ainda calcula estes parâmetros.
pub const BUSY_MESSAGE: &str = "Aguarde o cálculo terminar e tente de novo.";
/// Erro quando a geração síncrona entra em pânico.
pub const INTERNAL_ERROR: &str = "Falha interna ao gerar o G-code";
/// Filtro dos diálogos de procedimento (`("TFM Procedure Files", "*.json")`).
const PROCEDURE_FILTER: &str = "TFM Procedure Files";
/// Atraso até regravar o `autoload.txt` depois de iniciar o Mach3
/// (`self.after(2000, ...)` no Python).
const AUTOLOAD_REWRITE_DELAY: Duration = Duration::from_secs(2);
/// Atraso até abrir o `.tap` pela associação de arquivo (`self.after(3000, ...)`).
const ASSOCIATION_DELAY: Duration = Duration::from_secs(3);

// ---------------------------------------------------------------------------
// Estado entre quadros
// ---------------------------------------------------------------------------

/// Ação à espera da resposta ao alerta G53. Em ambas, `text` é o programa
/// completo (linhas unidas por `\n`) e `params` os parâmetros usados (EPS e
/// operador vão para o log de auditoria).
#[derive(Clone, Debug, PartialEq)]
pub enum PendingAction {
    /// "Gerar G-code": gravar o `.tap` no caminho escolhido no diálogo.
    SaveTap {
        path: PathBuf,
        text: String,
        params: Params,
    },
    /// "Abrir no Mach3": gravar `TFM_<data>_<hora>.tap` e iniciar o Mach3.
    OpenMach3 { text: String, params: Params },
    /// "Baixar G-code": gravar o `.tap` no caminho escolhido e, ao lado, o
    /// side-car `.audit.json` ([`crate::download::commit_download`]).
    DownloadTap {
        path: PathBuf,
        text: String,
        params: Params,
    },
}

impl PendingAction {
    /// Aviso de cancelamento ("Não", Esc ou clique fora do modal).
    fn cancel_message(&self) -> &'static str {
        match self {
            PendingAction::SaveTap { .. } => "Geração cancelada.",
            PendingAction::OpenMach3 { .. } => "Abertura no Mach3 cancelada.",
            PendingAction::DownloadTap { .. } => "Download cancelado.",
        }
    }
}

/// Estado das ações que atravessam quadros (campo `integration` do `TfmApp`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IntegrationState {
    pub pending: Option<PendingAction>,
}

// ---------------------------------------------------------------------------
// Auxiliares puros
// ---------------------------------------------------------------------------

/// Junta as linhas com `\n` (o `"\n".join(...)` do Python), sem `\r`.
pub fn join_lines(lines: &[String]) -> String {
    lines.join("\n")
}

/// O programa usa G53 (recuo em coordenadas de máquina)?
pub fn uses_g53(text: &str) -> bool {
    text.contains("G53")
}

/// Fim de linha do `.tap`: CRLF no Windows (como o Python gravava lá), LF
/// nos demais sistemas.
pub fn tap_line_ending() -> &'static str {
    if cfg!(windows) {
        "\r\n"
    } else {
        "\n"
    }
}

/// Texto do `.tap` como vai para o disco: `text` com quebras `\n`
/// convertidas para [`tap_line_ending`].
pub fn tap_text(text: &str) -> String {
    let ending = tap_line_ending();
    if ending == "\n" {
        text.to_string()
    } else {
        text.replace('\n', ending)
    }
}

/// Grava o `.tap` em UTF-8 com o fim de linha do sistema
/// ([`tap_line_ending`]), criando a pasta pai se preciso. Só o `.tap` passa
/// por aqui: procedimentos JSON e `autoload.txt` têm os seus próprios
/// gravadores.
pub fn write_text(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, tap_text(text).as_bytes())
}

/// `TFM_AAAAMMDD_HHMMSS.tap` (`_open_in_mach3_clicked`).
pub fn tap_filename(now: &NaiveDateTime) -> String {
    format!("TFM_{}.tap", now.format("%Y%m%d_%H%M%S"))
}

pub(crate) fn or_na(value: &str) -> &str {
    if value.trim().is_empty() {
        "N/A"
    } else {
        value
    }
}

/// Detalhe do log de auditoria da geração manual, no formato do Python:
/// `Arquivo gerado manualmente: {nome} | EPS: {eps} | Op: {operador}`
/// (campos vazios saem como `N/A`).
pub fn audit_detail(file_name: &str, eps: &str, operator: &str) -> String {
    format!(
        "Arquivo gerado manualmente: {file_name} | EPS: {} | Op: {}",
        or_na(eps),
        or_na(operator)
    )
}

/// Usuário do log de auditoria: o operador informado ou `Engenharia`.
pub fn audit_user(operator: &str) -> &str {
    if operator.trim().is_empty() {
        "Engenharia"
    } else {
        operator
    }
}

/// Completa a extensão quando o usuário digitou o nome sem nenhuma
/// (`defaultextension` do Tkinter).
pub fn with_default_extension(mut path: PathBuf, ext: &str) -> PathBuf {
    if path.extension().is_none() {
        path.set_extension(ext);
    }
    path
}

/// Nome do arquivo (`os.path.basename`), ou o caminho inteiro se não houver.
pub(crate) fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

// ---------------------------------------------------------------------------
// Mach3: pastas, executável, argumentos, autoload
// ---------------------------------------------------------------------------

/// `C:\Mach3\GCode`, usada quando `gcode_dir` está vazio.
fn default_gcode_dir() -> PathBuf {
    PathBuf::from(format!("C:{BACKSLASH}Mach3{BACKSLASH}GCode"))
}

/// `~/Mach3/GCode` (fallback quando a pasta configurada não pode ser criada);
/// `None` sem pasta do usuário conhecida.
pub fn home_gcode_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|home| PathBuf::from(home).join("Mach3").join("GCode"))
}

/// Pasta onde o `.tap` vai: `primary` (criada se preciso; vazia =
/// `C:\Mach3\GCode`) ou, se não der para criá-la, `fallback`.
pub fn resolve_gcode_dir(primary: &str, fallback: Option<PathBuf>) -> Result<PathBuf, String> {
    let primary = primary.trim();
    let primary = if primary.is_empty() {
        default_gcode_dir()
    } else {
        PathBuf::from(primary)
    };
    let first = match fs::create_dir_all(&primary) {
        Ok(()) => return Ok(primary),
        Err(err) => err,
    };
    let Some(fallback) = fallback else {
        return Err(format!(
            "não foi possível criar {} ({first})",
            primary.display()
        ));
    };
    fs::create_dir_all(&fallback).map_err(|err| {
        format!(
            "não foi possível criar {} ({first}) nem {} ({err})",
            primary.display(),
            fallback.display()
        )
    })?;
    log::warn!(
        "Pasta de G-code {} indisponível ({first}); usando {}",
        primary.display(),
        fallback.display()
    );
    Ok(fallback)
}

/// Pasta de G-code do config, com o fallback `~/Mach3/GCode`.
pub fn gcode_dir_with_fallback(cfg: &Integration) -> Result<PathBuf, String> {
    resolve_gcode_dir(&cfg.gcode_dir, home_gcode_dir())
}

/// Candidatos a executável do Mach3: só o configurado quando há um; senão
/// `C:\Mach3\Mach3.exe` e `C:\Mach3\Mach3Mill.exe`.
pub fn mach3_candidates(cfg: &Integration) -> Vec<PathBuf> {
    let configured = cfg.mach3_path.trim();
    if configured.is_empty() {
        let base = mach3_path::default_mach3_dir();
        vec![base.join("Mach3.exe"), base.join("Mach3Mill.exe")]
    } else {
        vec![PathBuf::from(configured)]
    }
}

/// Primeiro candidato que existe no disco.
pub fn find_mach3(cfg: &Integration) -> Option<PathBuf> {
    mach3_candidates(cfg).into_iter().find(|p| p.exists())
}

/// Argumentos do Mach3: `/p <perfil>` (quando há perfil) e o arquivo.
pub fn mach3_args(profile: &str, file: &Path) -> Vec<OsString> {
    let mut args = Vec::new();
    let profile = profile.trim();
    if !profile.is_empty() {
        args.push(OsString::from("/p"));
        args.push(OsString::from(profile));
    }
    args.push(file.as_os_str().to_owned());
    args
}

/// Nome do perfil para o aviso (`profile or 'padrão'`).
fn profile_label(profile: &str) -> &str {
    let profile = profile.trim();
    if profile.is_empty() {
        "padrão"
    } else {
        profile
    }
}

/// Grava `<dir>/autoload.txt` com o caminho do `.tap` (lido pelo macropump).
pub fn write_autoload(dir: &Path, file: &Path) -> io::Result<()> {
    fs::write(dir.join("autoload.txt"), file.display().to_string())
}

/// Regrava o `autoload.txt` depois de `delay`, numa thread, para o macropump
/// ler depois que o Mach3 subiu.
fn schedule_autoload_rewrite(dir: PathBuf, file: PathBuf, delay: Duration) {
    let (dir_now, file_now) = (dir.clone(), file.clone());
    let spawned = thread::Builder::new()
        .name("tfm-autoload".to_string())
        .spawn(move || {
            thread::sleep(delay);
            rewrite_autoload(&dir, &file);
        });
    // Sem thread, regrava já (o `except: _rewrite_autoload()` do Python).
    if let Err(err) = spawned {
        log::warn!("Thread do autoload não iniciou: {err}; regravando agora");
        rewrite_autoload(&dir_now, &file_now);
    }
}

/// Regrava o `autoload.txt`, só registrando a falha.
fn rewrite_autoload(dir: &Path, file: &Path) {
    if let Err(err) = write_autoload(dir, file) {
        log::warn!("autoload.txt não regravado em {}: {err}", dir.display());
    }
}

/// Abre o arquivo pela associação do Windows (`os.startfile`); se falhar,
/// tenta como URL `file:///` (o `webbrowser.open` do Python).
fn open_with_association(file: &Path) {
    if open::that(file).is_ok() {
        return;
    }
    let url = format!(
        "file:///{}",
        file.display().to_string().replace(BACKSLASH, "/")
    );
    if let Err(err) = open::that(&url) {
        log::warn!("Não foi possível abrir {}: {err}", file.display());
    }
}

/// Abre o `.tap` pela associação depois de `delay`, numa thread.
fn schedule_open_with_association(file: PathBuf, delay: Duration) {
    let file_now = file.clone();
    let spawned = thread::Builder::new()
        .name("tfm-abrir-tap".to_string())
        .spawn(move || {
            thread::sleep(delay);
            open_with_association(&file);
        });
    // Sem thread, abre já (o `except: _startfile_later()` do Python).
    if let Err(err) = spawned {
        log::warn!("Thread de abertura do .tap não iniciou: {err}; abrindo agora");
        open_with_association(&file_now);
    }
}

// ---------------------------------------------------------------------------
// G-code atual
// ---------------------------------------------------------------------------

/// Geração síncrona protegida: um pânico do motor vira [`INTERNAL_ERROR`]
/// em vez de derrubar a interface; `Err` já é a mensagem para o usuário.
fn generate_guarded(
    generate: impl FnOnce() -> Result<Vec<String>, GenError>,
) -> Result<Vec<String>, String> {
    let outcome = panic::catch_unwind(AssertUnwindSafe(generate));
    let lines = match outcome {
        Ok(Ok(lines)) => lines,
        Ok(Err(err)) => return Err(format!("Erro ao gerar G-Code: {err}")),
        Err(_) => {
            log::error!("Pânico na geração síncrona do G-code");
            return Err(INTERNAL_ERROR.to_string());
        }
    };
    if lines.is_empty() {
        return Err("Erro ao gerar G-Code.".to_string());
    }
    Ok(lines)
}

/// Linhas do G-code para `params`: reaproveita o resultado do trabalhador
/// quando ele é destes mesmos parâmetros (e deu certo). Se o trabalhador
/// ainda está calculando (resultado defasado), avisa e devolve `None` em vez
/// de gerar na thread da interface; ocioso, gera agora, protegido contra
/// pânico. Qualquer falha já foi notificada quando devolve `None`.
pub(crate) fn current_gcode(app: &mut TfmApp, params: &Params) -> Option<Vec<String>> {
    let key = params_key(params, &app.cfg);
    let cached = app
        .result
        .as_ref()
        .filter(|r| r.key == key && r.gen_error.is_none() && !r.lines.is_empty());
    if let Some(result) = cached {
        return Some(result.lines.clone());
    }
    if app.compute.busy() {
        log::info!("Ação adiada: cálculo {key:#018x} ainda em andamento");
        app.notify(NotifyKind::Aviso, BUSY_MESSAGE);
        return None;
    }
    match generate_guarded(|| gcode::generate(params)) {
        Ok(lines) => Some(lines),
        Err(message) => {
            app.notify(NotifyKind::Erro, message);
            None
        }
    }
}

/// Primeiro portão do Python: `_get_current_params()` devolvia `None` quando
/// algum texto não convertia (`float()` falhou) e a ação era recusada com
/// "Por favor, verifique os parâmetros.". Aqui `to_params` usaria o padrão
/// do campo, então gerar/salvar/abrir precisam checar isto antes.
fn fields_ok(app: &mut TfmApp) -> bool {
    let bad = app.ui.unparsable_fields();
    if bad.is_empty() {
        return true;
    }
    log::warn!("Ação recusada: campos não numéricos: {}", bad.join(", "));
    app.notify(NotifyKind::Erro, "Por favor, verifique os parâmetros.");
    false
}

/// Os três bloqueios comuns a "Gerar G-code", "Abrir no Mach3" e "Baixar
/// G-code", na ordem do Python: campos que convertem, regras físicas
/// (`validate_physical`) e ISO 3834. Devolve os parâmetros liberados; `None`
/// já foi notificado.
pub(crate) fn validated_params(app: &mut TfmApp) -> Option<Params> {
    if !fields_ok(app) {
        return None;
    }
    let params = app.current_params();
    let errors = validate_physical(&params).errors;
    if !errors.is_empty() {
        app.notify(
            NotifyKind::Erro,
            format!("ERRO DE PARÂMETROS:\n{}", errors.join("\n")),
        );
        return None;
    }
    if let Err(message) = app.quality.validate_iso3834(&params) {
        app.notify(NotifyKind::Erro, message);
        return None;
    }
    Some(params)
}

// ---------------------------------------------------------------------------
// Gerar G-code (.tap)
// ---------------------------------------------------------------------------

/// Botão "Gerar G-code", menu "G-code > Gerar G-code", "Salvar .tap" e
/// Ctrl+G: porta de `_generate_gcode_clicked`. Valida (física, depois ISO
/// 3834), gera, pede o arquivo e grava — ou deixa a gravação pendente do
/// alerta G53 ([`show_modals`]).
pub fn generate_tap(app: &mut TfmApp) {
    let Some(params) = validated_params(app) else {
        return;
    };
    let Some(lines) = current_gcode(app, &params) else {
        return;
    };
    let text = join_lines(&lines);

    let Some(path) = rfd::FileDialog::new()
        .add_filter("G-Code Files", &["tap"])
        .add_filter("All Files", &["*"])
        .set_file_name("programa.tap")
        .save_file()
    else {
        return;
    };
    let action = PendingAction::SaveTap {
        path: with_default_extension(path, "tap"),
        text,
        params,
    };
    run_or_confirm(app, action);
}

/// Executa a ação, ou a deixa pendente do alerta G53 quando o programa usa
/// G53 ([`show_modals`] executa no "Sim").
pub(crate) fn run_or_confirm(app: &mut TfmApp, action: PendingAction) {
    let text = match &action {
        PendingAction::SaveTap { text, .. }
        | PendingAction::OpenMach3 { text, .. }
        | PendingAction::DownloadTap { text, .. } => text,
    };
    if uses_g53(text) {
        app.integration.pending = Some(action);
    } else {
        run_action(app, action);
    }
}

/// Executa uma ação já liberada (sem G53, ou G53 confirmado).
fn run_action(app: &mut TfmApp, action: PendingAction) {
    match action {
        PendingAction::SaveTap { path, text, params } => commit_save(app, path, &text, &params),
        PendingAction::OpenMach3 { text, params } => launch_mach3(app, &text, &params),
        PendingAction::DownloadTap { path, text, params } => {
            crate::download::commit_download(app, path, &text, &params)
        }
    }
}

/// Grava o `.tap`, avisa e registra na auditoria (ou avisa o erro crítico,
/// registrando-o como o Python).
fn commit_save(app: &mut TfmApp, path: PathBuf, text: &str, params: &Params) {
    match write_text(&path, text) {
        Ok(()) => {
            log::info!("G-code gravado em {}", path.display());
            app.notify(NotifyKind::Sucesso, "Arquivo G-Code gerado com sucesso!");
            let detail = audit_detail(
                &file_name(&path),
                &params.traceability_eps,
                &params.traceability_operator,
            );
            app.audit.log(
                AUDIT_CATEGORY,
                &detail,
                audit_user(&params.traceability_operator),
            );
        }
        Err(err) => {
            app.notify(
                NotifyKind::Erro,
                format!("Erro crítico ao gerar G-Code: {err}"),
            );
            app.audit.log(
                "Critical Error",
                &format!("Falha na geração de G-Code: {err}"),
                "System",
            );
        }
    }
}

/// Desenha o alerta G53 enquanto há ação pendente. "Sim" executa (grava o
/// `.tap` ou abre no Mach3); "Não", Esc ou clicar fora cancelam ("Geração
/// cancelada." / "Abertura no Mach3 cancelada.").
pub fn show_modals(app: &mut TfmApp, ctx: &egui::Context) {
    if app.integration.pending.is_none() {
        return;
    }
    let mut decision: Option<bool> = None;
    let response = egui::Modal::new(egui::Id::unique("tfm_g53_modal")).show(ctx, |ui| {
        ui.set_max_width(460.0);
        ui.heading(G53_TITLE);
        ui.separator();
        ui.label(G53_BODY);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Não").clicked() {
                    decision = Some(false);
                }
                if ui.button("Sim").clicked() {
                    decision = Some(true);
                }
            });
        });
    });
    if decision.is_none() && response.should_close() {
        decision = Some(false);
    }
    match decision {
        Some(true) => {
            if let Some(action) = app.integration.pending.take() {
                run_action(app, action);
            }
        }
        Some(false) => {
            if let Some(action) = app.integration.pending.take() {
                app.notify(NotifyKind::Info, action.cancel_message());
            }
        }
        None => {}
    }
}

// ---------------------------------------------------------------------------
// Abrir no Mach3
// ---------------------------------------------------------------------------

/// Botão/menu "Abrir no Mach3" e Ctrl+M: porta de `_open_in_mach3_clicked`
/// com os mesmos bloqueios de [`generate_tap`] (campos, física, ISO 3834 e
/// o alerta G53). Liberado, [`launch_mach3`] grava e inicia o Mach3.
pub fn open_in_mach3(app: &mut TfmApp) {
    let Some(params) = validated_params(app) else {
        return;
    };
    let Some(lines) = current_gcode(app, &params) else {
        return;
    };
    let text = join_lines(&lines);
    run_or_confirm(app, PendingAction::OpenMach3 { text, params });
}

/// Grava `TFM_<data>_<hora>.tap` na pasta de G-code (ou `~/Mach3/GCode`),
/// registra na auditoria, grava o `autoload.txt` (se ligado), garante os
/// macros e inicia o `Mach3.exe /p <perfil> <arquivo>` com a pasta do Mach3
/// como `cwd`.
fn launch_mach3(app: &mut TfmApp, text: &str, params: &Params) {
    let integration = app.cfg.integration.clone();

    let base_dir = match gcode_dir_with_fallback(&integration) {
        Ok(dir) => dir,
        Err(err) => {
            app.notify(
                NotifyKind::Erro,
                format!("Erro ao preparar G-Code para Mach3: {err}"),
            );
            return;
        }
    };
    let filepath = base_dir.join(tap_filename(&chrono::Local::now().naive_local()));
    if let Err(err) = write_text(&filepath, text) {
        app.notify(
            NotifyKind::Erro,
            format!("Erro ao salvar G-Code para Mach3: {err}"),
        );
        return;
    }
    log::info!("G-code para o Mach3 gravado em {}", filepath.display());
    let detail = audit_detail(
        &file_name(&filepath),
        &params.traceability_eps,
        &params.traceability_operator,
    );
    app.audit.log(
        AUDIT_CATEGORY_MACH3,
        &detail,
        audit_user(&params.traceability_operator),
    );
    if integration.write_autoload_txt {
        if let Err(err) = write_autoload(&base_dir, &filepath) {
            log::warn!("autoload.txt não gravado em {}: {err}", base_dir.display());
        }
    }

    let shown = filepath.display().to_string();
    let Some(mach3) = find_mach3(&integration) else {
        app.notify(
            NotifyKind::Aviso,
            format!("G-Code salvo. Mach3 não encontrado.\nArquivo: {shown}"),
        );
        if integration.allow_file_association_fallback {
            open_with_association(&filepath);
        }
        return;
    };

    ensure_macros(app, &integration);
    let mach3_dir = mach3
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| mach3_path::mach3_dir(&integration));
    let profile = integration.mach3_profile.trim();
    let spawned = Command::new(&mach3)
        .args(mach3_args(profile, &filepath))
        .current_dir(&mach3_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match spawned {
        Ok(_child) => {
            log::info!(
                "Mach3 iniciado: {} (perfil {})",
                mach3.display(),
                profile_label(profile)
            );
            if integration.write_autoload_txt {
                schedule_autoload_rewrite(base_dir, filepath.clone(), AUTOLOAD_REWRITE_DELAY);
            }
            if integration.allow_file_association_fallback {
                schedule_open_with_association(filepath, ASSOCIATION_DELAY);
            }
            app.notify(
                NotifyKind::Sucesso,
                format!(
                    "Mach3 iniciado (perfil: {}). Abrindo arquivo:\n{shown}",
                    profile_label(profile)
                ),
            );
        }
        Err(err) => {
            log::warn!("Mach3 não iniciou ({}): {err}", mach3.display());
            match open::that(&mach3) {
                Ok(()) => app.notify(
                    NotifyKind::Aviso,
                    format!("G-Code salvo. Abra o arquivo no Mach3:\n{shown}"),
                ),
                Err(_) => app.notify(
                    NotifyKind::Aviso,
                    format!(
                        "G-Code salvo. Não foi possível abrir o Mach3 automaticamente.\nArquivo: {shown}"
                    ),
                ),
            }
        }
    }
}

/// Antes de iniciar o Mach3, garante os macros M101/M102 (porta de
/// `_ensure_mach3_macros`, via [`mach3_path::install_macros`]). Falhas
/// avisam mas não impedem o Mach3 de abrir.
fn ensure_macros(app: &mut TfmApp, cfg: &Integration) {
    match mach3_path::install_macros(cfg) {
        Ok(written) if written.is_empty() => {}
        Ok(written) => {
            let paths: Vec<String> = written.iter().map(|p| p.display().to_string()).collect();
            app.notify(
                NotifyKind::Sucesso,
                format!(
                    "Macros M101/M102 instalados para acionar OUTPUT1:\n{}",
                    paths.join("\n")
                ),
            );
        }
        Err(err) => app.notify(
            NotifyKind::Erro,
            format!("Erro ao instalar macros do Mach3: {err}"),
        ),
    }
}

// ---------------------------------------------------------------------------
// Procedimentos
// ---------------------------------------------------------------------------

/// Menu "Arquivo > Salvar Procedimento...", botão "Salvar" e Ctrl+S: grava
/// os parâmetros atuais como JSON (chaves do Python) na pasta de
/// procedimentos.
pub fn save_procedure_dialog(app: &mut TfmApp) {
    if !fields_ok(app) {
        return;
    }
    let dir = app.cfg.procedures_dir();
    if let Err(err) = fs::create_dir_all(&dir) {
        log::warn!("Pasta de procedimentos {} não criada: {err}", dir.display());
    }
    let mut dialog = rfd::FileDialog::new()
        .set_title("Salvar Procedimento...")
        .add_filter(PROCEDURE_FILTER, &["json"])
        .set_file_name("procedimento.json");
    if dir.is_dir() {
        dialog = dialog.set_directory(&dir);
    }
    let Some(path) = dialog.save_file() else {
        return;
    };
    let path = with_default_extension(path, "json");
    let params = app.current_params();
    match save_procedure(&path, &params) {
        Ok(()) => {
            log::info!("Procedimento salvo em {}", path.display());
            app.notify(NotifyKind::Sucesso, "Procedimento salvo com sucesso!");
        }
        Err(err) => app.notify(NotifyKind::Erro, format!("Erro: {err}")),
    }
}

/// Menu "Arquivo > Carregar Procedimento...", botão "Carregar" e Ctrl+O.
pub fn load_procedure_dialog(app: &mut TfmApp) {
    let dir = app.cfg.procedures_dir();
    let mut dialog = rfd::FileDialog::new()
        .set_title("Carregar Procedimento...")
        .add_filter(PROCEDURE_FILTER, &["json"]);
    if dir.is_dir() {
        dialog = dialog.set_directory(&dir);
    }
    let Some(path) = dialog.pick_file() else {
        return;
    };
    load_procedure_file(app, &path);
}

/// Carrega um procedimento de `path` para a interface (`_load_procedure_file`).
pub fn load_procedure_file(app: &mut TfmApp, path: &Path) {
    match load_procedure(path) {
        Ok(params) => {
            apply_loaded_params(&mut app.ui, &params);
            log::info!("Procedimento carregado de {}", path.display());
            app.notify(NotifyKind::Sucesso, "Procedimento carregado com sucesso!");
        }
        Err(err) => app.notify(NotifyKind::Erro, format!("Erro ao carregar: {err}")),
    }
}

/// Substitui o estado da interface pelos parâmetros carregados, preservando
/// a aba, a lista de operações, "avançado" e os presets das outras
/// operações; marca sujo para o cálculo refazer o G-code.
pub fn apply_loaded_params(ui: &mut UiState, params: &Params) {
    let mut fresh = UiState::from_params(params);
    fresh.list_expanded = ui.list_expanded;
    fresh.tab = ui.tab;
    fresh.show_advanced = ui.show_advanced;
    fresh.presets = std::mem::take(&mut ui.presets);
    fresh.mark_edited();
    *ui = fresh;
}

// ---------------------------------------------------------------------------
// Simulação (menu e teclas)
// ---------------------------------------------------------------------------

/// Vista do item de menu "Simulação > Vista ..." (pelo nome do item).
pub fn simulation_view(item: &str) -> Option<ViewPreset> {
    ViewPreset::ALL.into_iter().find(|v| v.hint() == item)
}

/// Vista das teclas 1–5.
pub fn digit_preset(digit: u8) -> Option<ViewPreset> {
    match digit {
        1 => Some(ViewPreset::Iso),
        2 => Some(ViewPreset::Topo),
        3 => Some(ViewPreset::Lado),
        4 => Some(ViewPreset::Frente),
        5 => Some(ViewPreset::Tras),
        _ => None,
    }
}

/// Aplica uma vista pré-definida à aba 3D.
pub fn set_view(ctx: &egui::Context, preset: ViewPreset) {
    view3d::with_state(ctx, |s| s.set_preset(preset));
    ctx.request_repaint();
}

/// Espaço: alterna play/pause da simulação.
pub fn toggle_simulation(ctx: &egui::Context) {
    view3d::with_state(ctx, |s| {
        if s.sim.playing {
            s.sim_pause();
        } else {
            s.sim_play();
        }
    });
    ctx.request_repaint();
}

/// Itens do menu "Simulação": as cinco vistas, "Simular", "Pausar" e
/// "Parar". Devolve `false` para um item desconhecido.
pub fn simulation_menu(ctx: &egui::Context, item: &str) -> bool {
    if let Some(preset) = simulation_view(item) {
        set_view(ctx, preset);
        return true;
    }
    match item {
        "Simular" => view3d::with_state(ctx, |s| s.sim_play()),
        "Pausar" => view3d::with_state(ctx, |s| s.sim_pause()),
        "Parar" => view3d::with_state(ctx, |s| s.sim_stop()),
        _ => return false,
    }
    ctx.request_repaint();
    true
}

// ---------------------------------------------------------------------------
// Atalhos
// ---------------------------------------------------------------------------

/// Atalhos da janela principal (os listados em "Ajuda > Atalhos").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    SaveProcedure,
    LoadProcedure,
    GenerateTap,
    OpenMach3,
    CopyGcode,
    Recompute,
    Help,
    PlayPause,
    View(ViewPreset),
}

const DIGIT_KEYS: [(Key, u8); 5] = [
    (Key::Num1, 1),
    (Key::Num2, 2),
    (Key::Num3, 3),
    (Key::Num4, 4),
    (Key::Num5, 5),
];

/// Lê (e consome) o atalho pressionado neste quadro. Os mais específicos
/// vêm antes (Ctrl+Shift+C antes de qualquer Ctrl+letra). Espaço e 1–5 só
/// valem sem widget focado (`focused`), para não roubar teclas de um campo
/// de texto.
fn poll_shortcut(input: &mut egui::InputState, focused: bool) -> Option<Shortcut> {
    if input.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::C) {
        return Some(Shortcut::CopyGcode);
    }
    if input.consume_key(Modifiers::COMMAND, Key::S) {
        return Some(Shortcut::SaveProcedure);
    }
    if input.consume_key(Modifiers::COMMAND, Key::O) {
        return Some(Shortcut::LoadProcedure);
    }
    if input.consume_key(Modifiers::COMMAND, Key::G) {
        return Some(Shortcut::GenerateTap);
    }
    if input.consume_key(Modifiers::COMMAND, Key::M) {
        return Some(Shortcut::OpenMach3);
    }
    if input.consume_key(Modifiers::NONE, Key::F5) {
        return Some(Shortcut::Recompute);
    }
    if input.consume_key(Modifiers::NONE, Key::F1) {
        return Some(Shortcut::Help);
    }
    if focused {
        return None;
    }
    if input.consume_key(Modifiers::NONE, Key::Space) {
        return Some(Shortcut::PlayPause);
    }
    DIGIT_KEYS
        .into_iter()
        .find(|(key, _)| input.consume_key(Modifiers::NONE, *key))
        .and_then(|(_, digit)| digit_preset(digit))
        .map(Shortcut::View)
}

/// Chamado uma vez por quadro, antes dos painéis. Com o alerta G53 aberto
/// as teclas ficam para o modal.
pub fn handle_shortcuts(app: &mut TfmApp, ctx: &egui::Context) {
    if app.integration.pending.is_some() {
        return;
    }
    let focused = ctx.memory(|m| m.focused().is_some());
    let Some(shortcut) = ctx.input_mut(|i| poll_shortcut(i, focused)) else {
        return;
    };
    log::debug!("Atalho: {shortcut:?}");
    run_shortcut(app, ctx, shortcut);
}

/// Executa um atalho.
pub fn run_shortcut(app: &mut TfmApp, ctx: &egui::Context, shortcut: Shortcut) {
    match shortcut {
        Shortcut::SaveProcedure => save_procedure_dialog(app),
        Shortcut::LoadProcedure => load_procedure_dialog(app),
        Shortcut::GenerateTap => generate_tap(app),
        Shortcut::OpenMach3 => open_in_mach3(app),
        Shortcut::CopyGcode => copy_gcode(app, ctx),
        Shortcut::Recompute => force_recompute(app),
        Shortcut::Help => app.dialogs.open(DialogId::Tutorial),
        Shortcut::PlayPause => toggle_simulation(ctx),
        Shortcut::View(preset) => set_view(ctx, preset),
    }
}

/// F5: pede o cálculo de novo, mesmo com a mesma chave (o debounce do app
/// nunca repete um pedido por conta própria).
pub fn force_recompute(app: &mut TfmApp) {
    let params = app.current_params();
    let key = params_key(&params, &app.cfg);
    app.compute.request(params, app.cfg.clone(), key);
    app.ui.dirty = false;
    app.notify(NotifyKind::Info, "Recalculando G-code...");
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use tfm_core::params::Operation;

    use crate::ui_state::LeftTab;

    /// `TFM_AAAAMMDD_HHMMSS.tap`, como `_open_in_mach3_clicked`.
    #[test]
    fn tap_filename_has_timestamp_format() {
        let fixed = NaiveDate::from_ymd_opt(2026, 10, 31)
            .and_then(|d| d.and_hms_opt(14, 30, 5))
            .expect("data fixa");
        assert_eq!(tap_filename(&fixed), "TFM_20261031_143005.tap");

        let now = tap_filename(&chrono::Local::now().naive_local());
        assert_eq!(now.len(), "TFM_AAAAMMDD_HHMMSS.tap".len());
        assert!(now.starts_with("TFM_") && now.ends_with(".tap"), "{now}");
        let stamp = &now[4..now.len() - 4];
        let (date, time) = stamp.split_once('_').expect("separador _");
        assert!(
            date.len() == 8 && date.chars().all(|c| c.is_ascii_digit()),
            "{date}"
        );
        assert!(
            time.len() == 6 && time.chars().all(|c| c.is_ascii_digit()),
            "{time}"
        );
    }

    /// Detalhe e usuário do log de auditoria, no formato do Python.
    #[test]
    fn audit_detail_matches_python_format() {
        assert_eq!(
            audit_detail("peca.tap", "EPS-07", "Ana"),
            "Arquivo gerado manualmente: peca.tap | EPS: EPS-07 | Op: Ana"
        );
        assert_eq!(
            audit_detail("peca.tap", "", "  "),
            "Arquivo gerado manualmente: peca.tap | EPS: N/A | Op: N/A"
        );
        assert_eq!(audit_user(""), "Engenharia");
        assert_eq!(audit_user("   "), "Engenharia");
        assert_eq!(audit_user("Ana"), "Ana");
        assert_eq!(AUDIT_CATEGORY, "GCode Generation (Manual)");
    }

    /// Textos do alerta G53 iguais ao `messagebox.askyesno` do Python.
    #[test]
    fn g53_texts_and_detection() {
        assert_eq!(G53_TITLE, "SEGURANÇA DE MÁQUINA");
        assert!(G53_BODY
            .starts_with("Este programa usa G53 (Coordenadas de Máquina) para recuo seguro.\n\n"));
        assert!(G53_BODY.contains("A MÁQUINA ESTÁ REFERENCIADA (REF ALL HOME)?\n\n"));
        assert!(G53_BODY.ends_with("Se não estiver, pode haver COLISÃO.\nDeseja continuar?"));
        assert!(uses_g53("G90\nG53 G0 Z0\nM30"));
        assert!(!uses_g53("G90\nG0 Z5\nM30"));
    }

    /// O `.tap` sai em UTF-8 com CRLF no Windows (como o Python gravava lá)
    /// e LF nos demais sistemas, criando a pasta se preciso.
    #[test]
    fn write_text_uses_crlf_on_windows_and_lf_elsewhere() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("sub").join("programa.tap");
        let lines = vec![
            "(TFM – Revestimento)".to_string(),
            "G21".to_string(),
            "M30".to_string(),
        ];
        let text = join_lines(&lines);
        assert_eq!(text, "(TFM – Revestimento)\nG21\nM30", "join só com LF");
        write_text(&path, &text).expect("grava");
        let bytes = fs::read(&path).expect("lê");
        let expected = if cfg!(windows) {
            "(TFM – Revestimento)\r\nG21\r\nM30"
        } else {
            "(TFM – Revestimento)\nG21\nM30"
        };
        assert_eq!(bytes, expected.as_bytes());
        assert_eq!(tap_text(&text), expected);
        assert_eq!(tap_line_ending(), if cfg!(windows) { "\r\n" } else { "\n" });
        // Nunca duplica o CR: cada `\n` vira exatamente um fim de linha.
        assert_eq!(
            tap_text(&text).matches('\n').count(),
            2,
            "três linhas = duas quebras"
        );
    }

    /// A geração síncrona é protegida: pânico vira [`INTERNAL_ERROR`], erro
    /// do motor vira "Erro ao gerar G-Code: …", programa vazio é erro, e o
    /// caminho normal devolve as linhas.
    #[test]
    fn generate_guarded_maps_panic_and_errors() {
        let err = generate_guarded(|| panic!("motor explodiu")).expect_err("pânico vira Err");
        assert_eq!(err, INTERNAL_ERROR);
        let err = generate_guarded(|| Err(GenError::Invalid("x".to_string())))
            .expect_err("erro do motor vira Err");
        assert!(err.starts_with("Erro ao gerar G-Code: "), "{err}");
        assert_eq!(
            generate_guarded(|| Ok(Vec::new())).expect_err("vazio é erro"),
            "Erro ao gerar G-Code."
        );
        let lines = generate_guarded(|| gcode::generate(&Params::default())).expect("padrão gera");
        assert!(lines.len() > 20, "veio só {} linhas", lines.len());
    }

    /// Com o trabalhador ocupado e sem resultado destes parâmetros, a ação
    /// não gera na thread da interface: avisa [`BUSY_MESSAGE`] e devolve
    /// `None`. Ocioso, gera na hora.
    #[test]
    fn current_gcode_waits_for_busy_worker() {
        use crate::compute::{compute_all, ComputeHandle, JobRunner};
        use std::sync::Arc;
        use tfm_core::config::AppConfig;

        let tmp = tempfile::tempdir().expect("tempdir");
        let mut app = TfmApp::headless(tmp.path());
        let params = app.current_params();
        let lines = current_gcode(&mut app, &params).expect("ocioso gera na hora");
        assert!(lines.len() > 20);
        assert!(app.notifications.is_empty(), "sem aviso quando ocioso");

        // Trabalhador que demora: qualquer pedido o deixa ocupado por 2 s.
        let slow: JobRunner = Arc::new(
            |params: Params, cfg: &AppConfig, key: u64| -> crate::compute::ComputeResult {
                thread::sleep(Duration::from_secs(2));
                compute_all(params, cfg, key)
            },
        );
        app.compute = ComputeHandle::with_runner(egui::Context::default(), slow);
        let mut other = params.clone();
        other.comprimento_revestir += 5.0;
        let other_key = params_key(&other, &app.cfg);
        app.compute.request(other, app.cfg.clone(), other_key);
        assert!(app.compute.busy());
        assert!(current_gcode(&mut app, &params).is_none());
        let last = app.notifications.last().expect("aviso registrado");
        assert_eq!(last.kind, NotifyKind::Aviso);
        assert_eq!(last.text, BUSY_MESSAGE);
    }

    /// Operador com qualificação vencida (QualityManager em pasta
    /// temporária): "Abrir no Mach3" e "Gerar G-code" são bloqueados pelo
    /// ISO 3834 antes de gravar qualquer arquivo ou abrir o alerta G53.
    #[test]
    fn open_in_mach3_is_blocked_by_iso3834_like_generate_tap() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut app = TfmApp::headless(tmp.path());
        app.quality.add_operator("Ana", "OP-1", "Q-1", "2000-01-01");
        assert!(
            app.quality.operators_path().is_file(),
            "operators.json gravado na pasta temporária"
        );
        app.ui
            .fields
            .insert("traceability_operator", "Ana".to_string());
        let gcode_dir = tmp.path().join("gcode");
        app.cfg.integration.gcode_dir = gcode_dir.display().to_string();

        open_in_mach3(&mut app);
        let last = app.notifications.last().expect("erro registrado");
        assert_eq!(last.kind, NotifyKind::Erro);
        assert!(
            last.text
                .starts_with("BLOQUEIO ISO 3834: Qualificação do operador 'Ana' vencida"),
            "{}",
            last.text
        );
        assert!(app.integration.pending.is_none(), "nada pendente do G53");
        assert!(!gcode_dir.exists(), "nenhum .tap gravado");

        generate_tap(&mut app);
        let last = app.notifications.last().expect("erro registrado");
        assert_eq!(last.kind, NotifyKind::Erro);
        assert!(last.text.starts_with("BLOQUEIO ISO 3834"), "{}", last.text);
        assert_eq!(app.notifications.len(), 2, "um erro por ação");
    }

    /// Regra física violada bloqueia "Abrir no Mach3" com a mesma mensagem
    /// "ERRO DE PARÂMETROS:" de "Gerar G-code"; campo que não converte, com
    /// "Por favor, verifique os parâmetros.".
    #[test]
    fn open_in_mach3_is_blocked_by_physical_rules_and_bad_fields() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut app = TfmApp::headless(tmp.path());
        app.ui.fields.insert("diametro", "0".to_string());
        open_in_mach3(&mut app);
        let last = app.notifications.last().expect("erro registrado");
        assert_eq!(last.kind, NotifyKind::Erro);
        assert!(
            last.text.starts_with("ERRO DE PARÂMETROS:\n"),
            "{}",
            last.text
        );

        app.ui.fields.insert("diametro", "abc".to_string());
        open_in_mach3(&mut app);
        let last = app.notifications.last().expect("erro registrado");
        assert_eq!(last.kind, NotifyKind::Erro);
        assert_eq!(last.text, "Por favor, verifique os parâmetros.");
        assert!(app.integration.pending.is_none());
    }

    /// "Não" no alerta G53 cancela com a mensagem da ação pendente.
    #[test]
    fn pending_action_cancel_messages() {
        let save = PendingAction::SaveTap {
            path: PathBuf::from("peca.tap"),
            text: "G53".to_string(),
            params: Params::default(),
        };
        assert_eq!(save.cancel_message(), "Geração cancelada.");
        let open = PendingAction::OpenMach3 {
            text: "G53".to_string(),
            params: Params::default(),
        };
        assert_eq!(open.cancel_message(), "Abertura no Mach3 cancelada.");
        let download = PendingAction::DownloadTap {
            path: PathBuf::from("OS-1.tap"),
            text: "G53".to_string(),
            params: Params::default(),
        };
        assert_eq!(download.cancel_message(), "Download cancelado.");
        assert_eq!(AUDIT_CATEGORY_MACH3, "GCode Generation (Mach3)");
    }

    /// `defaultextension` do Tkinter: só completa quando falta extensão.
    #[test]
    fn default_extension_only_when_missing() {
        assert_eq!(
            with_default_extension(PathBuf::from("peca"), "tap"),
            PathBuf::from("peca.tap")
        );
        assert_eq!(
            with_default_extension(PathBuf::from("peca.nc"), "tap"),
            PathBuf::from("peca.nc")
        );
        assert_eq!(
            with_default_extension(PathBuf::from("proc"), "json"),
            PathBuf::from("proc.json")
        );
    }

    /// Candidatos a executável e argumentos como no Python.
    #[test]
    fn mach3_candidates_and_args_follow_python() {
        let configured = Integration {
            mach3_path: "D:/Maq/Mach3.exe".to_string(),
            ..Integration::default()
        };
        assert_eq!(
            mach3_candidates(&configured),
            [PathBuf::from("D:/Maq/Mach3.exe")]
        );
        let empty = Integration {
            mach3_path: String::new(),
            ..Integration::default()
        };
        let base = mach3_path::default_mach3_dir();
        assert_eq!(
            mach3_candidates(&empty),
            [base.join("Mach3.exe"), base.join("Mach3Mill.exe")]
        );

        let file = Path::new("C:/Mach3/GCode/TFM_1.tap");
        assert_eq!(
            mach3_args("PTA", file),
            [
                OsString::from("/p"),
                OsString::from("PTA"),
                file.as_os_str().to_owned()
            ]
        );
        assert_eq!(mach3_args("  ", file), [file.as_os_str().to_owned()]);
        assert_eq!(profile_label(""), "padrão");
        assert_eq!(profile_label("PTA"), "PTA");
    }

    /// Pasta configurada quando dá para criar; senão o fallback; vazio =
    /// `C:\Mach3\GCode`.
    #[test]
    fn gcode_dir_falls_back_when_primary_cannot_be_created() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let primary = tmp.path().join("ok");
        let got = resolve_gcode_dir(&primary.display().to_string(), None).expect("cria");
        assert_eq!(got, primary);
        assert!(primary.is_dir());

        // Um arquivo no lugar da pasta: não dá para criar -> fallback.
        let blocked = tmp.path().join("bloqueado");
        fs::write(&blocked, b"x").expect("arquivo");
        let fallback = tmp.path().join("home").join("Mach3").join("GCode");
        let got = resolve_gcode_dir(&blocked.display().to_string(), Some(fallback.clone()))
            .expect("fallback");
        assert_eq!(got, fallback);
        assert!(fallback.is_dir());
        assert!(resolve_gcode_dir(&blocked.display().to_string(), None).is_err());

        assert_eq!(
            default_gcode_dir(),
            PathBuf::from(format!("C:{BACKSLASH}Mach3{BACKSLASH}GCode"))
        );
        assert!(home_gcode_dir().is_some_and(|p| p.ends_with(Path::new("Mach3").join("GCode"))));
    }

    /// `autoload.txt` guarda o caminho do `.tap`.
    #[test]
    fn autoload_holds_the_tap_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("TFM_20261031_143005.tap");
        write_autoload(tmp.path(), &file).expect("grava");
        assert_eq!(
            fs::read_to_string(tmp.path().join("autoload.txt")).expect("lê"),
            file.display().to_string()
        );
    }

    /// Carregar um procedimento troca os parâmetros, preserva aba/lista/
    /// avançado e os presets, e marca sujo (recalcula).
    #[test]
    fn loaded_params_keep_tab_and_mark_dirty() {
        let mut ui = UiState::default();
        ui.set_operation(Operation::ChapaPlana);
        ui.set_operation(Operation::Espiral);
        // Depois de trocar de operação (que fecha a lista): estado de tela.
        ui.tab = LeftTab::Processo;
        ui.list_expanded = true;
        ui.show_advanced = true;
        ui.dirty = false;
        let presets_before = ui.presets.clone();
        assert!(!presets_before.is_empty());

        let mut p = Params {
            diametro: 123.0,
            ..Params::default()
        };
        p.apply_operation(Operation::Bolacha);
        apply_loaded_params(&mut ui, &p);
        assert_eq!(ui.operation, Operation::Bolacha);
        assert_eq!(ui.fields["diametro"], "123.0");
        assert_eq!(ui.tab, LeftTab::Processo);
        assert!(ui.list_expanded);
        assert!(ui.show_advanced);
        assert!(ui.dirty);
        assert!(ui.last_edit.is_some());
        assert_eq!(ui.presets, presets_before);
    }

    /// Salvar grava as chaves do JSON Python e carregar devolve o mesmo.
    #[test]
    fn procedure_round_trip_uses_python_keys() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("procedimento.json");
        let mut p = Params::default();
        p.apply_operation(Operation::OscilacaoQuadrada);
        p.nome_procedimento = "Teste".to_string();
        save_procedure(&path, &p).expect("salva");
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("lê")).expect("json");
        assert_eq!(json["welding_mode"], "oscilacao");
        assert_eq!(json["oscillation_type"], "quadrada");
        assert_eq!(json["nome_procedimento"], "Teste");
        assert_eq!(load_procedure(&path).expect("carrega"), p);
    }

    /// Teclas 1–5 = Isométrica, Superior, Lateral, Frontal, Traseira; os
    /// itens do menu "Simulação" mapeiam para as mesmas vistas.
    #[test]
    fn digit_presets_are_the_five_views() {
        assert_eq!(digit_preset(1), Some(ViewPreset::Iso));
        assert_eq!(digit_preset(2), Some(ViewPreset::Topo));
        assert_eq!(digit_preset(3), Some(ViewPreset::Lado));
        assert_eq!(digit_preset(4), Some(ViewPreset::Frente));
        assert_eq!(digit_preset(5), Some(ViewPreset::Tras));
        assert_eq!(digit_preset(0), None);
        assert_eq!(digit_preset(6), None);
        for v in ViewPreset::ALL {
            assert_eq!(simulation_view(v.hint()), Some(v));
        }
        assert_eq!(simulation_view("Simular"), None);
    }
}
