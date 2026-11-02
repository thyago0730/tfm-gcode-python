//! Preferências de interface (`ui` do `config.json`) e pastas do aplicativo.
//!
//! A raiz do app é a pasta que contém `config/` (raiz do workspace em debug,
//! pasta do executável em release), espelhando `AppConfig::resolve_path()`.

use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use tfm_core::config::{self, AppConfig, UiPrefs};

use crate::theme::Theme;

/// Largura permitida do painel esquerdo (parâmetros), em pontos.
pub const LEFT_RANGE: RangeInclusive<f32> = 260.0..=420.0;
/// Largura permitida do painel direito (G-code), em pontos.
pub const RIGHT_RANGE: RangeInclusive<f32> = 260.0..=520.0;

/// Pastas usadas pelo app, todas derivadas do caminho do `config.json`.
#[derive(Clone, Debug)]
pub struct Dirs {
    /// Raiz do app (pai de `config/`).
    pub root: PathBuf,
    /// `config/config.json`.
    pub config_path: PathBuf,
    /// Pasta `config/` (manutenção, materiais).
    pub config_dir: PathBuf,
    /// `data/quality/` (operadores, EPS).
    pub quality_dir: PathBuf,
    /// `data/logs/` (auditoria).
    pub logs_dir: PathBuf,
}

/// Resolve as pastas a partir de [`AppConfig::resolve_path`].
pub fn dirs() -> Dirs {
    let config_path = AppConfig::resolve_path();
    let config_dir = or_dot(config_path.parent().unwrap_or(Path::new(".")));
    let root = app_root_from(&config_path);
    Dirs {
        quality_dir: root.join("data").join("quality"),
        logs_dir: root.join("data").join("logs"),
        root,
        config_path,
        config_dir,
    }
}

/// Raiz do app (pai de `config/`).
pub fn app_root() -> PathBuf {
    app_root_from(&AppConfig::resolve_path())
}

fn app_root_from(config_path: &Path) -> PathBuf {
    let dir = config_path.parent().unwrap_or(Path::new("."));
    if dir.file_name().is_some_and(|n| n == config::DIR_NAME) {
        or_dot(dir.parent().unwrap_or(Path::new(".")))
    } else {
        or_dot(dir)
    }
}

fn or_dot(p: &Path) -> PathBuf {
    if p.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        p.to_path_buf()
    }
}

/// Limita uma largura ao intervalo; valores não finitos viram o mínimo.
pub fn clamp(w: f32, range: &RangeInclusive<f32>) -> f32 {
    if w.is_finite() {
        w.clamp(*range.start(), *range.end())
    } else {
        *range.start()
    }
}

/// Versão atual do layout. Abaixo dela, [`migrate_layout`] aplica os padrões.
pub const LAYOUT_REV: u32 = 1;
/// Largura do painel de parâmetros no layout compacto.
pub const COMPACT_LEFT_W: f32 = 310.0;
/// Largura do painel do G-code no layout compacto.
pub const COMPACT_RIGHT_W: f32 = 340.0;

/// Aplica uma vez os padrões do layout compacto: 3D grande, G-code recolhido e
/// detalhes fechados. Devolve `true` se mudou algo.
pub fn migrate_layout(ui: &mut UiPrefs) -> bool {
    if ui.layout_rev >= LAYOUT_REV {
        return false;
    }
    ui.left_w = COMPACT_LEFT_W;
    ui.right_w = COMPACT_RIGHT_W;
    ui.right_pinned = false;
    ui.bottom_collapsed = true;
    ui.layout_rev = LAYOUT_REV;
    true
}

/// Foto das preferências de layout que o app persiste em `cfg.ui`.
#[derive(Clone, Debug, PartialEq)]
pub struct UiSnapshot {
    pub theme: Theme,
    pub left_w: f32,
    pub right_w: f32,
    pub left_pinned: bool,
    pub right_pinned: bool,
    pub bottom_collapsed: bool,
}

impl UiSnapshot {
    /// Lê (e sanitiza) as preferências gravadas.
    pub fn from_config(cfg: &AppConfig) -> UiSnapshot {
        UiSnapshot {
            theme: Theme::from_key(&cfg.ui.theme),
            left_w: clamp(cfg.ui.left_w, &LEFT_RANGE).round(),
            right_w: clamp(cfg.ui.right_w, &RIGHT_RANGE).round(),
            left_pinned: cfg.ui.left_pinned,
            right_pinned: cfg.ui.right_pinned,
            bottom_collapsed: cfg.ui.bottom_collapsed,
        }
    }

    /// Grava em `cfg.ui`; devolve `true` se algo mudou.
    pub fn store(&self, cfg: &mut AppConfig) -> bool {
        let before = cfg.ui.clone();
        cfg.ui.theme = self.theme.key().to_string();
        cfg.ui.left_w = clamp(self.left_w, &LEFT_RANGE).round();
        cfg.ui.right_w = clamp(self.right_w, &RIGHT_RANGE).round();
        cfg.ui.left_pinned = self.left_pinned;
        cfg.ui.right_pinned = self.right_pinned;
        cfg.ui.bottom_collapsed = self.bottom_collapsed;
        before != cfg.ui
    }
}

/// Salva o config inteiro (indent 4, mesma estrutura do Python).
pub fn save_config(cfg: &AppConfig, path: &Path) -> Result<(), String> {
    cfg.save(path)
        .map_err(|err| format!("{} ({err})", path.display()))
}

/// Aviso devolvido quando um `config.json` ilegível foi renomeado antes de
/// gravar os padrões.
pub const INVALID_CONFIG_WARNING: &str = "config.json inválido; cópia guardada em config.json.bak";

/// Grava **só** as chaves de `ui` que o app controla (tema, larguras, pinos,
/// painel inferior), editando o JSON existente: ordem das chaves, escapes
/// `\uXXXX`, demais seções e a convenção de quebra de linha final são
/// preservados. Sem arquivo, grava o config completo. Com um arquivo que não
/// é um objeto JSON válido, renomeia-o para `config.json.bak`, grava o config
/// completo e devolve `Ok(Some(aviso))` para o chamador mostrar ao usuário;
/// nos demais casos devolve `Ok(None)`.
pub fn save_ui_prefs(cfg: &AppConfig, path: &Path) -> Result<Option<String>, String> {
    let Ok(raw) = std::fs::read(path) else {
        return save_config(cfg, path).map(|()| None);
    };
    let text = String::from_utf8_lossy(&raw);
    let mut root = match serde_json::from_str::<Value>(&text) {
        Ok(root @ Value::Object(_)) => root,
        _ => {
            let warning = back_up_invalid_config(path);
            save_config(cfg, path)?;
            return Ok(Some(warning));
        }
    };
    let Some(map) = root.as_object_mut() else {
        return save_config(cfg, path).map(|()| None);
    };
    let ui = map.entry("ui").or_insert_with(|| Value::Object(Map::new()));
    if !ui.is_object() {
        *ui = Value::Object(Map::new());
    }
    if let Some(ui) = ui.as_object_mut() {
        ui.insert("theme".into(), Value::String(cfg.ui.theme.clone()));
        ui.insert("left_w".into(), Value::from(f64::from(cfg.ui.left_w)));
        ui.insert("right_w".into(), Value::from(f64::from(cfg.ui.right_w)));
        ui.insert("left_pinned".into(), Value::Bool(cfg.ui.left_pinned));
        ui.insert("right_pinned".into(), Value::Bool(cfg.ui.right_pinned));
        ui.insert(
            "bottom_collapsed".into(),
            Value::Bool(cfg.ui.bottom_collapsed),
        );
        ui.insert("layout_rev".into(), Value::from(cfg.ui.layout_rev));
    }
    let mut out =
        config::json_pretty4(&root).map_err(|err| format!("{} ({err})", path.display()))?;
    // Arquivo 100% ASCII => o Python gravou com `ensure_ascii`; todo caractere
    // não ASCII do resultado veio de um escape e volta a ser escape.
    if text.is_ascii() {
        out = escape_non_ascii(&out);
    }
    if text.ends_with('\n') && !out.ends_with('\n') {
        out.push('\n');
    }
    std::fs::write(path, out).map_err(|err| format!("{} ({err})", path.display()))?;
    Ok(None)
}

/// `<arquivo>.bak` ao lado do original (`config.json` -> `config.json.bak`).
fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".bak");
    PathBuf::from(name)
}

/// Renomeia o `config.json` que não é um objeto JSON válido para
/// `config.json.bak` (substituindo um `.bak` anterior) e devolve o aviso para
/// o usuário. Se nem renomear der, o aviso diz isso e o arquivo original é
/// sobrescrito pelos padrões em seguida.
fn back_up_invalid_config(path: &Path) -> String {
    let backup = backup_path(path);
    match std::fs::rename(path, &backup) {
        Ok(()) => {
            log::warn!(
                "{} inválido; cópia guardada em {}",
                path.display(),
                backup.display()
            );
            INVALID_CONFIG_WARNING.to_string()
        }
        Err(err) => {
            log::warn!(
                "{} inválido e não foi possível guardar a cópia em {}: {err}",
                path.display(),
                backup.display()
            );
            format!(
                "config.json inválido; não foi possível guardar a cópia em config.json.bak ({err})"
            )
        }
    }
}

const BACKSLASH: char = 92u8 as char;

/// Reproduz o `ensure_ascii=True` do `json.dump` do Python: todo caractere
/// não ASCII vira `\uXXXX` (pares substitutos para fora do BMP).
fn escape_non_ascii(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut units = [0u16; 2];
    for ch in s.chars() {
        if ch.is_ascii() {
            out.push(ch);
        } else {
            for unit in ch.encode_utf16(&mut units) {
                out.push(BACKSLASH);
                out.push_str(&format!("u{unit:04x}"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raiz_e_o_pai_de_config() {
        let root = app_root_from(Path::new("C:/x/app/config/config.json"));
        assert_eq!(root, PathBuf::from("C:/x/app"));
        let root = app_root_from(Path::new("C:/x/app/config.json"));
        assert_eq!(root, PathBuf::from("C:/x/app"));
        assert_eq!(app_root_from(Path::new("config.json")), PathBuf::from("."));
    }

    #[test]
    fn clamp_respeita_faixa_e_nao_finitos() {
        assert_eq!(clamp(100.0, &LEFT_RANGE), 260.0);
        assert_eq!(clamp(999.0, &RIGHT_RANGE), 520.0);
        assert_eq!(clamp(f32::NAN, &LEFT_RANGE), 260.0);
        assert_eq!(clamp(400.0, &LEFT_RANGE), 400.0);
    }

    /// O layout compacto entra uma vez só: depois disso, o que o usuário
    /// escolher fica valendo.
    #[test]
    fn layout_antigo_migra_uma_vez() {
        let mut ui = UiPrefs::default();
        assert_eq!(ui.layout_rev, 0);
        assert!(migrate_layout(&mut ui));
        assert_eq!(ui.layout_rev, LAYOUT_REV);
        assert_eq!((ui.left_w, ui.right_w), (COMPACT_LEFT_W, COMPACT_RIGHT_W));
        assert!(!ui.right_pinned && ui.bottom_collapsed && ui.left_pinned);
        ui.right_pinned = true;
        ui.bottom_collapsed = false;
        assert!(!migrate_layout(&mut ui));
        assert!(ui.right_pinned && !ui.bottom_collapsed);
    }

    #[test]
    fn store_devolve_se_mudou() {
        let mut cfg = AppConfig::default();
        let snap = UiSnapshot::from_config(&cfg);
        assert!(!snap.store(&mut cfg));
        let changed = UiSnapshot {
            theme: Theme::Escuro,
            left_w: 420.0,
            ..snap
        };
        assert!(changed.store(&mut cfg));
        assert_eq!(cfg.ui.theme, "escuro");
        assert_eq!(cfg.ui.left_w, 420.0);
    }

    #[test]
    fn escape_non_ascii_imita_o_python() {
        assert_eq!(escape_non_ascii("Aço"), r"A\u00e7o");
        assert_eq!(escape_non_ascii("abc"), "abc");
    }

    /// Arquivo no formato que o Python grava: `ensure_ascii`, indent 4, sem
    /// quebra final, com chaves fora da ordem dos defaults.
    const PYTHON_STYLE: &str = r#"{
    "ui": {
        "sashes": {
            "main": 361
        },
        "dpi_scaling": "auto"
    },
    "powders": [
        {
            "name": "A\u00e7o Inox 316L"
        }
    ],
    "zeta": 1,
    "alpha": 2
}"#;

    #[test]
    fn save_ui_prefs_preserva_o_resto_do_arquivo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.json");
        std::fs::write(&path, PYTHON_STYLE).expect("write");
        let mut cfg = AppConfig::default();
        cfg.ui.theme = "escuro".into();
        cfg.ui.left_w = 420.0;
        cfg.ui.left_pinned = false;
        save_ui_prefs(&cfg, &path).expect("save");
        let saved = std::fs::read_to_string(&path).expect("read");
        // Escapes, ordem das seções e ausência de quebra final preservados.
        assert!(saved.contains(r"A\u00e7o Inox 316L"), "{saved}");
        assert!(saved.find("\"zeta\"").unwrap() < saved.find("\"alpha\"").unwrap());
        assert!(!saved.ends_with('\n'));
        // Chaves antigas de `ui` ficam onde estavam; as novas entram depois.
        assert!(saved.find("\"sashes\"").unwrap() < saved.find("\"theme\"").unwrap());
        let value: Value = serde_json::from_str(&saved).expect("json");
        assert_eq!(value["ui"]["theme"], "escuro");
        assert_eq!(value["ui"]["left_w"], 420.0);
        assert_eq!(value["ui"]["left_pinned"], false);
        assert_eq!(value["ui"]["sashes"]["main"], 361);
        assert_eq!(value["powders"][0]["name"], "Aço Inox 316L");
        assert_eq!(value["zeta"], 1);
    }

    #[test]
    fn save_ui_prefs_cria_o_arquivo_quando_falta() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config").join("config.json");
        let warning = save_ui_prefs(&AppConfig::default(), &path).expect("save");
        assert_eq!(warning, None, "arquivo ausente não é inválido");
        let text = std::fs::read_to_string(&path).expect("read");
        let value: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(value["ui"]["left_w"], 360.0);
        assert!(value["costs"].is_object());
    }

    /// `config.json` que não é um objeto JSON válido vai para
    /// `config.json.bak` (substituindo um `.bak` anterior) antes de os
    /// padrões serem gravados, e o chamador recebe o aviso.
    #[test]
    fn save_ui_prefs_guarda_copia_do_config_invalido() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.json");
        let bak = dir.path().join("config.json.bak");
        assert_eq!(backup_path(&path), bak);

        std::fs::write(&path, "{ isto nao e json").expect("write");
        let warning = save_ui_prefs(&AppConfig::default(), &path).expect("save");
        assert_eq!(warning.as_deref(), Some(INVALID_CONFIG_WARNING));
        assert_eq!(
            std::fs::read_to_string(&bak).expect("bak"),
            "{ isto nao e json"
        );
        let text = std::fs::read_to_string(&path).expect("read");
        let value: Value = serde_json::from_str(&text).expect("defaults válidos");
        assert_eq!(value["ui"]["left_w"], 360.0);
        assert!(value["costs"].is_object());

        // JSON válido que não é objeto também é inválido para o app; o .bak
        // anterior é substituído.
        std::fs::write(&path, "[1, 2]").expect("write");
        let warning = save_ui_prefs(&AppConfig::default(), &path).expect("save");
        assert_eq!(warning.as_deref(), Some(INVALID_CONFIG_WARNING));
        assert_eq!(std::fs::read_to_string(&bak).expect("bak"), "[1, 2]");

        // Agora o arquivo é válido: sem aviso e sem mexer no .bak.
        let warning = save_ui_prefs(&AppConfig::default(), &path).expect("save");
        assert_eq!(warning, None);
        assert_eq!(std::fs::read_to_string(&bak).expect("bak"), "[1, 2]");
    }
}

#[cfg(test)]
mod tests_defaults {
    use super::*;

    /// Config gravado pelo Python (sem as chaves novas de `ui`) deve dar os
    /// defaults de largura, não 0 (que o clamp levaria ao mínimo da faixa).
    #[test]
    fn config_sem_chaves_novas_usa_defaults_de_largura() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.json");
        let python_style = r#"{"ui": {"sashes": {"main": {"x": 361}}, "dpi_scaling": "auto", "compact_threshold_px": 560}}"#;
        std::fs::write(&path, python_style).expect("write");
        let cfg = AppConfig::read(&path);
        let snap = UiSnapshot::from_config(&cfg);
        assert_eq!(snap.left_w, 360.0);
        assert_eq!(snap.right_w, 390.0);
        assert_eq!(snap.theme, Theme::Auto);
    }
}
