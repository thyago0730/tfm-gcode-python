//! Tema visual do TFM G-Code Generator.
//!
//! Paleta clara e escura próprias (estilo Prancheta CNC, não o cinza padrão
//! do egui), preferência Automático/Claro/Escuro e carga opcional da fonte
//! Inter a partir de `assets/fonts/`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::style::{Selection, WidgetVisuals, Widgets};
use egui::{Color32, CornerRadius, Margin, Shadow, Stroke, Visuals};

/// Preferência de tema do usuário (chave `ui.theme` do `config.json`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    /// Segue o tema do sistema operacional.
    Auto,
    Claro,
    Escuro,
}

impl Theme {
    /// Rótulo exibido no botão de tema.
    pub fn label(self) -> &'static str {
        match self {
            Theme::Auto => "Automático",
            Theme::Claro => "Claro",
            Theme::Escuro => "Escuro",
        }
    }

    /// Lê a chave gravada no config (`"auto"`, `"claro"`, `"escuro"`; aceita
    /// também `"light"`/`"dark"`). Qualquer outro valor vira `Auto`.
    pub fn from_key(s: &str) -> Theme {
        match s.trim().to_ascii_lowercase().as_str() {
            "claro" | "light" => Theme::Claro,
            "escuro" | "dark" => Theme::Escuro,
            _ => Theme::Auto,
        }
    }

    /// Chave gravada no config.
    pub fn key(self) -> &'static str {
        match self {
            Theme::Auto => "auto",
            Theme::Claro => "claro",
            Theme::Escuro => "escuro",
        }
    }

    /// Próximo tema no ciclo Automático -> Claro -> Escuro -> Automático.
    pub fn next(self) -> Theme {
        match self {
            Theme::Auto => Theme::Claro,
            Theme::Claro => Theme::Escuro,
            Theme::Escuro => Theme::Auto,
        }
    }

    fn preference(self) -> egui::ThemePreference {
        match self {
            Theme::Auto => egui::ThemePreference::System,
            Theme::Claro => egui::ThemePreference::Light,
            Theme::Escuro => egui::ThemePreference::Dark,
        }
    }
}

/// Cores das camadas no 3D e nos gráficos, na ordem do design.
pub const LAYER_COLORS: [u32; 9] = [
    0x2563eb, 0xdc2626, 0x16a34a, 0x9333ea, 0xea580c, 0x0891b2, 0xbe185d, 0x65a30d, 0x854d0e,
];

/// Converte `0xRRGGBB` em [`Color32`] opaco.
pub const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb(
        ((rgb >> 16) & 0xff) as u8,
        ((rgb >> 8) & 0xff) as u8,
        (rgb & 0xff) as u8,
    )
}

/// Cores semânticas do tema em vigor, para widgets desenhados à mão.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub accent: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
    pub muted: Color32,
    pub text: Color32,
    pub border: Color32,
    pub panel_bg: Color32,
    pub card_bg: Color32,
    pub layer_colors: [Color32; 9],
    pub dark: bool,
}

impl Palette {
    pub fn light() -> Palette {
        Palette {
            accent: hex(0x2563eb),
            success: hex(0x16a34a),
            warning: hex(0xca8a04),
            danger: hex(0xdc2626),
            muted: hex(0x6b7280),
            text: hex(0x1f2937),
            border: hex(0xe5e7eb),
            panel_bg: hex(0xf6f7f9),
            card_bg: hex(0xffffff),
            layer_colors: LAYER_COLORS.map(hex),
            dark: false,
        }
    }

    pub fn dark() -> Palette {
        Palette {
            accent: hex(0x3b82f6),
            success: hex(0x22c55e),
            warning: hex(0xeab308),
            danger: hex(0xef4444),
            muted: hex(0x9ca3af),
            text: hex(0xe5e7eb),
            border: hex(0x2a2f38),
            panel_bg: hex(0x15181d),
            card_bg: hex(0x1e2229),
            layer_colors: LAYER_COLORS.map(hex),
            dark: true,
        }
    }

    pub fn for_dark(dark: bool) -> Palette {
        if dark {
            Palette::dark()
        } else {
            Palette::light()
        }
    }

    /// Paleta correspondente ao tema que o egui está usando agora
    /// (resolve `Auto` pelo tema do sistema).
    pub fn resolve(ctx: &egui::Context) -> Palette {
        Palette::for_dark(ctx.theme() == egui::Theme::Dark)
    }

    /// Cor da camada `index` (cicla pela sequência).
    pub fn layer_color(&self, index: usize) -> Color32 {
        self.layer_colors[index % self.layer_colors.len()]
    }
}

const CORNER: CornerRadius = CornerRadius::same(6);

/// Aplica a preferência de tema e os `Visuals` próprios (claro **e** escuro)
/// ao contexto, para que `Auto` siga o sistema sem perder o visual. Devolve a
/// paleta do tema que ficou em vigor.
pub fn apply(ctx: &egui::Context, theme: Theme) -> Palette {
    ctx.options_mut(|opt| {
        opt.theme_preference = theme.preference();
        opt.fallback_theme = egui::Theme::Light;
    });
    ctx.set_visuals_of(egui::Theme::Light, light_visuals());
    ctx.set_visuals_of(egui::Theme::Dark, dark_visuals());
    ctx.all_styles_mut(tune_style);
    Palette::resolve(ctx)
}

fn tune_style(style: &mut egui::Style) {
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.menu_margin = Margin::same(6);
    style.spacing.window_margin = Margin::same(12);
    style.interaction.selectable_labels = false;
    style.visuals.slider_trailing_fill = true;
}

/// Tons de fundo/borda/texto dos widgets de um tema.
struct Tones {
    text: Color32,
    text_strong: Color32,
    border: Color32,
    card: Color32,
    inactive: Color32,
    inactive_weak: Color32,
    hovered: Color32,
    hovered_weak: Color32,
    hovered_border: Color32,
    active: Color32,
    active_weak: Color32,
    open: Color32,
    open_weak: Color32,
}

fn widget(
    bg: Color32,
    weak: Color32,
    border: Color32,
    fg: Stroke,
    expansion: f32,
) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: weak,
        bg_stroke: Stroke::new(1.0, border),
        corner_radius: CORNER,
        fg_stroke: fg,
        expansion,
    }
}

fn widgets(p: &Palette, t: &Tones) -> Widgets {
    Widgets {
        noninteractive: widget(t.card, t.card, t.border, Stroke::new(1.0, t.text), 0.0),
        inactive: widget(
            t.inactive,
            t.inactive_weak,
            t.border,
            Stroke::new(1.0, t.text),
            0.0,
        ),
        hovered: widget(
            t.hovered,
            t.hovered_weak,
            t.hovered_border,
            Stroke::new(1.5, t.text_strong),
            1.0,
        ),
        active: widget(
            t.active,
            t.active_weak,
            p.accent,
            Stroke::new(2.0, t.text_strong),
            1.0,
        ),
        open: widget(t.open, t.open_weak, t.border, Stroke::new(1.0, t.text), 0.0),
    }
}

fn base_visuals(p: &Palette, t: &Tones, mut v: Visuals) -> Visuals {
    v.override_text_color = None;
    v.weak_text_color = Some(p.muted);
    v.widgets = widgets(p, t);
    v.selection = Selection {
        bg_fill: p.accent,
        stroke: Stroke::new(1.0, Color32::WHITE),
    };
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.danger;
    v.panel_fill = p.panel_bg;
    v.window_fill = p.card_bg;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CORNER;
    v.menu_corner_radius = CORNER;
    v.window_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(if p.dark { 96 } else { 36 }),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(if p.dark { 80 } else { 28 }),
    };
    v.text_cursor.stroke = Stroke::new(2.0, p.accent);
    v.button_frame = true;
    v.collapsing_header_frame = false;
    v
}

/// Tema claro: fundo `#f6f7f9`, cartões `#ffffff`, texto `#1f2937`.
pub fn light_visuals() -> Visuals {
    let p = Palette::light();
    let t = Tones {
        text: p.text,
        text_strong: hex(0x111827),
        border: p.border,
        card: p.card_bg,
        inactive: hex(0xe5e9ef),
        inactive_weak: hex(0xeef1f5),
        hovered: hex(0xdde3ea),
        hovered_weak: hex(0xe6ebf1),
        hovered_border: hex(0xc9d1db),
        active: hex(0xcfd7e0),
        active_weak: hex(0xd8dfe7),
        open: hex(0xe4e9ef),
        open_weak: hex(0xebeff4),
    };
    let mut v = base_visuals(&p, &t, Visuals::light());
    v.faint_bg_color = hex(0xf0f2f5);
    v.extreme_bg_color = Color32::WHITE;
    v.text_edit_bg_color = Some(Color32::WHITE);
    v.code_bg_color = hex(0xeef0f3);
    v
}

/// Tema escuro: fundo `#15181d`, cartões `#1e2229`, texto `#e5e7eb`.
pub fn dark_visuals() -> Visuals {
    let p = Palette::dark();
    let t = Tones {
        text: p.text,
        text_strong: hex(0xf9fafb),
        border: p.border,
        card: p.card_bg,
        inactive: hex(0x2a3039),
        inactive_weak: hex(0x262b33),
        hovered: hex(0x333a45),
        hovered_weak: hex(0x2f3540),
        hovered_border: hex(0x444c58),
        active: hex(0x3d4552),
        active_weak: hex(0x39414d),
        open: hex(0x2c323c),
        open_weak: hex(0x2a3039),
    };
    let mut v = base_visuals(&p, &t, Visuals::dark());
    v.faint_bg_color = hex(0x1a1e24);
    v.extreme_bg_color = hex(0x0f1216);
    v.text_edit_bg_color = Some(hex(0x141820));
    v.code_bg_color = hex(0x262b33);
    v
}

/// Registra a fonte Inter como família `"inter"` no topo de `Proportional`,
/// se houver um `.ttf` em `assets/fonts/` (preferindo `Inter*.ttf`). Sem a
/// pasta ou sem arquivo, as fontes padrão do egui permanecem. Nunca baixa nada.
pub fn load_fonts(ctx: &egui::Context) {
    let dir = crate::prefs::app_root().join("assets").join("fonts");
    let Some(path) = pick_font(&dir) else {
        log::info!(
            "Sem fonte em {}; usando as fontes padrão do egui",
            dir.display()
        );
        return;
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            log::warn!("Não foi possível ler a fonte {}: {err}", path.display());
            return;
        }
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "inter".to_owned(),
        Arc::new(egui::FontData::from_owned(bytes)),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "inter".to_owned());
    ctx.set_fonts(fonts);
    log::info!("Fonte {} registrada como família inter", path.display());
}

/// Primeiro `.ttf` cujo nome começa com `inter` (sem distinguir maiúsculas),
/// senão o primeiro `.ttf` em ordem alfabética.
fn pick_font(dir: &Path) -> Option<PathBuf> {
    let mut ttfs: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ttf"))
        })
        .collect();
    ttfs.sort();
    let is_inter = |p: &PathBuf| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.to_ascii_lowercase().starts_with("inter"))
    };
    ttfs.iter().find(|p| is_inter(p)).or(ttfs.first()).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chave_e_rotulo_do_tema() {
        for theme in [Theme::Auto, Theme::Claro, Theme::Escuro] {
            assert_eq!(Theme::from_key(theme.key()), theme);
        }
        assert_eq!(Theme::from_key(" Dark "), Theme::Escuro);
        assert_eq!(Theme::from_key("qualquer"), Theme::Auto);
        assert_eq!(Theme::Auto.label(), "Automático");
    }

    #[test]
    fn ciclo_de_tema() {
        assert_eq!(Theme::Auto.next(), Theme::Claro);
        assert_eq!(Theme::Claro.next(), Theme::Escuro);
        assert_eq!(Theme::Escuro.next(), Theme::Auto);
    }

    #[test]
    fn cores_fixas_do_design() {
        assert_eq!(hex(0x2563eb), Color32::from_rgb(0x25, 0x63, 0xeb));
        let p = Palette::light();
        assert_eq!(p.accent, hex(0x2563eb));
        assert_eq!(p.success, hex(0x16a34a));
        assert_eq!(p.warning, hex(0xca8a04));
        assert_eq!(p.danger, hex(0xdc2626));
        assert_eq!(p.layer_color(9), p.layer_colors[0]);
        assert!(!light_visuals().dark_mode);
        assert!(dark_visuals().dark_mode);
    }
}
