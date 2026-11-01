//! "Caminho (Mach3)": executável do Mach3, pasta de G-code, perfil, autoload
//! e fallback (seção `integration`), mais o instalador dos macros:
//! `M101.m1s`/`M102.m1s` (liga/desliga OUTPUT1, a tocha) e `macropump.m1s`
//! (autoload do G-code). Só grava o config em "Salvar".
//!
//! [`install_macros`] é a porta de `_ensure_mach3_macros` do Python e é
//! reutilizada pela integração (abrir no Mach3); [`install_macropump`] é a de
//! `_install_macropump`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tfm_core::config::{AppConfig, Integration};

use crate::app::{NotifyKind, TfmApp};
use crate::dialogs::{self, DialogId};
use crate::widgets::{check_field, text_field};

const BACKSLASH: char = 92u8 as char;
/// Conteúdo esperado de `M101.m1s` (liga a tocha).
pub const MACRO_ON: &str = "ActivateSignal(Output1)";
/// Conteúdo esperado de `M102.m1s` (desliga a tocha).
pub const MACRO_OFF: &str = "DeactivateSignal(Output1)";
/// Perfil usado quando `mach3_profile` está vazio.
pub const DEFAULT_PROFILE: &str = "PTA";
/// Perfil padrão do Mach3, sempre incluído.
pub const MILL_PROFILE: &str = "Mach3Mill";

#[derive(Clone, Debug, Default)]
pub struct Mach3State {
    pub loaded: bool,
    /// Cópia de trabalho de `cfg.integration`.
    pub integration: Integration,
}

impl Mach3State {
    pub const EMPTY: Self = Mach3State {
        loaded: false,
        integration: Integration {
            mach3_path: String::new(),
            gcode_dir: String::new(),
            write_autoload_txt: false,
            allow_file_association_fallback: false,
            mach3_profile: String::new(),
        },
    };

    pub fn load(&mut self, cfg: &AppConfig) {
        self.integration = cfg.integration.clone();
        self.loaded = true;
    }
}

/// `C:\Mach3`.
pub fn default_mach3_dir() -> PathBuf {
    PathBuf::from(format!("C:{BACKSLASH}Mach3"))
}

/// Pasta do Mach3: pai de `mach3_path`, ou `C:\Mach3` quando vazio.
pub fn mach3_dir(cfg: &Integration) -> PathBuf {
    let path = cfg.mach3_path.trim();
    if path.is_empty() {
        return default_mach3_dir();
    }
    Path::new(path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(default_mach3_dir)
}

/// `macros/` dentro da pasta do Mach3 (ou `Macros/`, se só ela existir).
pub fn macro_root(base: &Path) -> PathBuf {
    let lower = base.join("macros");
    if lower.is_dir() {
        return lower;
    }
    let upper = base.join("Macros");
    if upper.is_dir() {
        upper
    } else {
        lower
    }
}

/// Perfis-alvo: o configurado (ou `PTA`) e `Mach3Mill`, sem repetir.
pub fn target_profiles(cfg: &Integration) -> Vec<String> {
    let profile = cfg.mach3_profile.trim();
    let profile = if profile.is_empty() {
        DEFAULT_PROFILE
    } else {
        profile
    };
    let mut out = vec![profile.to_string()];
    if profile != MILL_PROFILE {
        out.push(MILL_PROFILE.to_string());
    }
    out
}

/// Cria as pastas de perfil; `Err` se nenhuma pôde ser criada.
fn target_dirs(cfg: &Integration) -> Result<Vec<PathBuf>, String> {
    let root = macro_root(&mach3_dir(cfg));
    let mut dirs = Vec::new();
    let mut last_err = None;
    for profile in target_profiles(cfg) {
        let dir = root.join(&profile);
        match fs::create_dir_all(&dir) {
            Ok(()) => dirs.push(dir),
            Err(err) => last_err = Some(format!("{} ({err})", dir.display())),
        }
    }
    if dirs.is_empty() {
        return Err(format!(
            "Não foi possível criar a pasta de macros do Mach3: {}",
            last_err.unwrap_or_else(|| root.display().to_string())
        ));
    }
    Ok(dirs)
}

/// O macro precisa ser (re)escrito? Sim quando não dá para ler, quando tem o
/// erro de digitação `ActivateSinal`/`DeactivateSinal` ou quando não contém a
/// linha esperada.
pub fn needs_fix(path: &Path, expected: &str) -> bool {
    match fs::read_to_string(path) {
        Ok(content) => {
            content.contains("ActivateSinal")
                || content.contains("DeactivateSinal")
                || !content.contains(expected)
        }
        Err(_) => true,
    }
}

/// Garante o macro com `expected`: cria se falta; se existe errado, renomeia
/// para `.bak` e reescreve. Devolve `true` se escreveu.
fn ensure_macro(path: &Path, expected: &str) -> io::Result<bool> {
    if path.exists() {
        if !needs_fix(path, expected) {
            return Ok(false);
        }
        let mut bak = path.as_os_str().to_owned();
        bak.push(".bak");
        if let Err(err) = fs::rename(path, &bak) {
            log::warn!("Backup de {} falhou: {err}", path.display());
        }
    }
    fs::write(path, format!("{expected}\n"))?;
    Ok(true)
}

/// Junta o resultado: `Err` só quando nada foi escrito **e** houve falhas.
fn finish(written: Vec<PathBuf>, errors: Vec<String>) -> Result<Vec<PathBuf>, String> {
    if written.is_empty() && !errors.is_empty() {
        return Err(format!(
            "Não foi possível escrever os macros em nenhuma pasta de perfil ({}). Execute o app como administrador ou instale manualmente.",
            errors.join("; ")
        ));
    }
    for err in errors {
        log::warn!("Macro do Mach3 não escrito: {err}");
    }
    Ok(written)
}

/// Instala `M101.m1s`/`M102.m1s` em `<Mach3>/macros/<perfil>` e
/// `<Mach3>/macros/Mach3Mill`, criando as pastas, com backup `.bak` dos
/// arquivos errados. Devolve os arquivos escritos (vazio = já estavam
/// corretos).
pub fn install_macros(cfg: &Integration) -> Result<Vec<PathBuf>, String> {
    let dirs = target_dirs(cfg)?;
    let mut written = Vec::new();
    let mut errors = Vec::new();
    for dir in &dirs {
        for (file, expected) in [("M101.m1s", MACRO_ON), ("M102.m1s", MACRO_OFF)] {
            let path = dir.join(file);
            match ensure_macro(&path, expected) {
                Ok(true) => written.push(path),
                Ok(false) => {}
                Err(err) => errors.push(format!("{} ({err})", path.display())),
            }
        }
    }
    finish(written, errors)
}

/// VBScript do `macropump.m1s`: a cada ciclo do Macro Pump, se existir
/// `<gcode_dir>\autoload.txt`, carrega o G-code indicado nele e apaga o
/// arquivo (o mesmo script do Python).
pub fn macropump_script(gcode_dir: &str) -> String {
    let dir = gcode_dir.trim().trim_end_matches(['/', BACKSLASH]);
    let dir = if dir.is_empty() {
        format!("C:{BACKSLASH}Mach3{BACKSLASH}GCode")
    } else {
        dir.to_string()
    };
    let autoload = format!("{dir}{BACKSLASH}autoload.txt");
    format!(
        "' macropump.m1s - Autoload de G-Code
Dim fso, f, path
Set fso = CreateObject(\"Scripting.FileSystemObject\")
If fso.FileExists(\"{autoload}\") Then
    On Error Resume Next
    Set f = fso.OpenTextFile(\"{autoload}\", 1)
    path = Trim(f.ReadLine)
    f.Close
    If (path <> \"\") Then
        If fso.FileExists(path) Then
            Message(\"Autoload: \" & path)
            LoadFile(path)
            ' DoOEMButton(1000) ' Cycle Start opcional
        Else
            Message(\"Autoload: arquivo não existe \" & path)
        End If
    Else
        Message(\"Autoload: caminho vazio\")
    End If
    fso.DeleteFile(\"{autoload}\")
    On Error GoTo 0
End If
"
    )
}

/// Instala (sobrescrevendo) o `macropump.m1s` do autoload em cada
/// perfil-alvo. Devolve os arquivos escritos.
pub fn install_macropump(cfg: &Integration) -> Result<Vec<PathBuf>, String> {
    let dirs = target_dirs(cfg)?;
    let script = macropump_script(&cfg.gcode_dir);
    let mut written = Vec::new();
    let mut errors = Vec::new();
    for dir in &dirs {
        let path = dir.join("macropump.m1s");
        match fs::write(&path, &script) {
            Ok(()) => written.push(path),
            Err(err) => errors.push(format!("{} ({err})", path.display())),
        }
    }
    finish(written, errors)
}

/// "Instalar macropump (autoload)": macros da tocha + macropump.
pub fn install_all(cfg: &Integration) -> Result<Vec<PathBuf>, String> {
    let mut written = install_macros(cfg)?;
    written.extend(install_macropump(cfg)?);
    Ok(written)
}

/// Texto do aviso após a instalação.
pub fn install_message(written: &[PathBuf]) -> String {
    if written.is_empty() {
        return "Macros do Mach3 já estavam instalados e corretos.".to_string();
    }
    let paths: Vec<String> = written.iter().map(|p| p.display().to_string()).collect();
    format!(
        "Macros instalados em: {}. No Mach3, marque \"Run Macro Pump\" em General Config e reinicie.",
        paths.join(" | ")
    )
}

/// Item de menu "Instalar macropump (autoload)": instala com o config atual,
/// avisa e, como o Python, liga `write_autoload_txt` (gravando o config se
/// ele mudou).
pub fn install_and_notify(app: &mut TfmApp) {
    match install_all(&app.cfg.integration) {
        Ok(written) => {
            app.notify(NotifyKind::Sucesso, install_message(&written));
            if !app.cfg.integration.write_autoload_txt {
                app.cfg.integration.write_autoload_txt = true;
                dialogs::save_config(app);
            }
        }
        Err(err) => app.notify(
            NotifyKind::Erro,
            format!("Erro ao instalar macropump: {err}"),
        ),
    }
}

fn pick_exe(cfg: &Integration) -> Option<String> {
    let mut dlg = rfd::FileDialog::new()
        .set_title("Selecione o executável do Mach3")
        .add_filter("Executável", &["exe"])
        .add_filter("Todos os arquivos", &["*"]);
    let dir = mach3_dir(cfg);
    if dir.is_dir() {
        dlg = dlg.set_directory(dir);
    }
    dlg.pick_file().map(|p| p.display().to_string())
}

fn pick_dir(current: &str) -> Option<String> {
    let mut dlg = rfd::FileDialog::new().set_title("Pasta onde o G-code (.tap) é salvo para o Mach3");
    if Path::new(current).is_dir() {
        dlg = dlg.set_directory(current);
    }
    dlg.pick_folder().map(|p| p.display().to_string())
}

/// Linha "rótulo + campo largo + botão de procurar".
fn path_row(ui: &mut egui::Ui, label: &str, value: &mut String, tip: &str, button: &str) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.label(label).on_hover_text(tip);
        let width = (ui.available_width() - 96.0).max(120.0);
        ui.add(egui::TextEdit::singleline(value).desired_width(width))
            .on_hover_text(tip);
        clicked = ui.button(button).clicked();
    });
    clicked
}

pub fn show(app: &mut TfmApp, state: &mut Mach3State, ctx: &egui::Context, open: &mut bool) {
    if !state.loaded {
        state.load(&app.cfg);
    }
    let mut close = false;
    let mut save = false;
    let mut install = false;
    dialogs::window(DialogId::Mach3Path, open)
        .default_width(540.0)
        .show(ctx, |ui| {
            ui.label("Integração com o Mach3: abrir o .tap, autoload e macros da tocha.");
            ui.add_space(4.0);
            if path_row(
                ui,
                "Executável do Mach3:",
                &mut state.integration.mach3_path,
                "Caminho do Mach3.exe (ex.: C:/Mach3/Mach3.exe). A pasta dele recebe os macros.",
                "Procurar...",
            ) {
                if let Some(path) = pick_exe(&state.integration) {
                    state.integration.mach3_path = path;
                }
            }
            if path_row(
                ui,
                "Pasta do G-code:",
                &mut state.integration.gcode_dir,
                "Pasta onde o .tap é salvo antes de abrir no Mach3 (e onde fica o autoload.txt).",
                "Pasta...",
            ) {
                if let Some(dir) = pick_dir(&state.integration.gcode_dir) {
                    state.integration.gcode_dir = dir;
                }
            }
            text_field(
                ui,
                "Perfil do Mach3:",
                &mut state.integration.mach3_profile,
                "Perfil do Mach3 cuja pasta de macros recebe M101/M102 e macropump (ex.: PTA).",
            );
            check_field(
                ui,
                "Gravar autoload.txt ao abrir no Mach3",
                &mut state.integration.write_autoload_txt,
                "Escreve <pasta do G-code>/autoload.txt com o caminho do .tap; o macropump carrega o arquivo automaticamente.",
            );
            check_field(
                ui,
                "Permitir abrir pela associação de arquivo",
                &mut state.integration.allow_file_association_fallback,
                "Se o executável não for encontrado, abre o .tap pelo programa associado no Windows.",
            );
            ui.add_space(4.0);
            if ui
                .button("Instalar macropump (autoload)")
                .on_hover_text(
                    "Grava M101/M102 (tocha) e macropump.m1s (autoload) nas pastas de macros do perfil e do Mach3Mill, usando os valores acima.",
                )
                .clicked()
            {
                install = true;
            }
            ui.weak("As alterações só são gravadas ao clicar em Salvar.");
            dialogs::footer(ui, |ui| {
                if ui.button("Salvar").clicked() {
                    save = true;
                }
                if ui.button("Cancelar").clicked() {
                    close = true;
                }
            });
        });
    if install {
        match install_all(&state.integration) {
            Ok(written) => {
                state.integration.write_autoload_txt = true;
                app.notify(NotifyKind::Sucesso, install_message(&written));
            }
            Err(err) => app.notify(
                NotifyKind::Erro,
                format!("Erro ao instalar macropump: {err}"),
            ),
        }
    }
    if save {
        app.cfg.integration = state.integration.clone();
        if dialogs::save_config(app) {
            close = true;
        }
    }
    if close {
        *open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_in(root: &Path) -> Integration {
        Integration {
            mach3_path: root.join("Mach3").join("Mach3.exe").display().to_string(),
            gcode_dir: root.join("Mach3").join("GCode").display().to_string(),
            mach3_profile: "PTA".to_string(),
            ..Integration::default()
        }
    }

    #[test]
    fn instala_m101_m102_nos_dois_perfis() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cfg = cfg_in(tmp.path());
        let written = install_macros(&cfg).expect("instala");
        assert_eq!(written.len(), 4, "{written:?}");
        let m101 = tmp.path().join("Mach3/macros/PTA/M101.m1s");
        let m102 = tmp.path().join("Mach3/macros/Mach3Mill/M102.m1s");
        assert_eq!(
            fs::read_to_string(&m101).expect("lê"),
            "ActivateSignal(Output1)\n"
        );
        assert_eq!(
            fs::read_to_string(&m102).expect("lê"),
            "DeactivateSignal(Output1)\n"
        );
        // Segunda chamada: tudo correto, nada a escrever.
        assert!(install_macros(&cfg).expect("de novo").is_empty());
    }

    #[test]
    fn corrige_macro_errado_com_backup() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cfg = cfg_in(tmp.path());
        let dir = tmp.path().join("Mach3/macros/PTA");
        fs::create_dir_all(&dir).expect("pasta");
        let m101 = dir.join("M101.m1s");
        fs::write(&m101, "ActivateSinal(Output1)\n").expect("escreve");
        let written = install_macros(&cfg).expect("instala");
        assert!(written.contains(&m101));
        assert_eq!(
            fs::read_to_string(&m101).expect("lê"),
            "ActivateSignal(Output1)\n"
        );
        let bak = dir.join("M101.m1s.bak");
        assert_eq!(
            fs::read_to_string(&bak).expect("backup"),
            "ActivateSinal(Output1)\n"
        );
        assert!(needs_fix(&bak, MACRO_ON));
        assert!(!needs_fix(&m101, MACRO_ON));
    }

    #[test]
    fn usa_pasta_macros_maiuscula_quando_so_ela_existe() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cfg = cfg_in(tmp.path());
        fs::create_dir_all(tmp.path().join("Mach3/Macros")).expect("pasta");
        let root = macro_root(&mach3_dir(&cfg));
        // Em sistemas sem distinção de caixa (Windows) `macros` também
        // "existe"; o que importa é cair na mesma pasta já criada.
        assert!(root
            .file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("macros")));
        let written = install_macros(&cfg).expect("instala");
        assert_eq!(written.len(), 4);
        assert!(tmp.path().join("Mach3/Macros/PTA/M101.m1s").exists());
        assert!(tmp.path().join("Mach3/Macros/Mach3Mill/M102.m1s").exists());
    }

    #[test]
    fn pasta_padrao_e_perfis_alvo() {
        let empty = Integration {
            mach3_path: String::new(),
            mach3_profile: String::new(),
            ..Integration::default()
        };
        assert_eq!(mach3_dir(&empty), PathBuf::from(format!("C:{BACKSLASH}Mach3")));
        assert_eq!(target_profiles(&empty), ["PTA", "Mach3Mill"]);
        let mill = Integration {
            mach3_profile: "Mach3Mill".to_string(),
            ..Integration::default()
        };
        assert_eq!(target_profiles(&mill), ["Mach3Mill"]);
        let pta = Integration {
            mach3_path: "C:/Mach3/Mach3.exe".to_string(),
            ..Integration::default()
        };
        assert_eq!(mach3_dir(&pta), PathBuf::from("C:/Mach3"));
    }

    #[test]
    fn macropump_usa_a_pasta_de_gcode() {
        let script = macropump_script("D:/GC/");
        assert!(script.contains(&format!("D:/GC{BACKSLASH}autoload.txt")));
        assert!(script.contains("LoadFile(path)"));
        assert!(script.starts_with("' macropump.m1s - Autoload de G-Code"));
        let tmp = tempfile::tempdir().expect("tempdir");
        let cfg = cfg_in(tmp.path());
        let written = install_macropump(&cfg).expect("instala");
        assert_eq!(written.len(), 2);
        let text = fs::read_to_string(&written[0]).expect("lê");
        assert!(text.contains("autoload.txt"));
        assert!(install_message(&written).contains("Run Macro Pump"));
        assert_eq!(
            install_message(&[]),
            "Macros do Mach3 já estavam instalados e corretos."
        );
    }
}
