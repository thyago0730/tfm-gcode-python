//! tfm-app: interface gráfica (eframe/egui) do TFM G-Code Generator.
//!
//! O binário `TFM_GCODE` (em `main.rs`) só monta a janela; todo o estado e o
//! layout ficam aqui, em [`app::TfmApp`].

pub mod app;
pub mod compute;
pub mod dialogs;
pub mod icons;
pub mod panels;
pub mod prefs;
pub mod theme;
pub mod ui_state;
pub mod view3d;
pub mod widgets;

/// Título da janela: `TFM G-Code Generator v2.0.0 - Cladding & Hardfacing`.
pub const APP_TITLE: &str = concat!(
    "TFM G-Code Generator v",
    env!("CARGO_PKG_VERSION"),
    " - Cladding & Hardfacing"
);

/// Decodifica `assets/icone.ico` (embutido no binário) para o ícone da janela.
/// Devolve `None` se o arquivo não puder ser decodificado — o app abre sem ícone.
pub fn window_icon() -> Option<egui::IconData> {
    const ICON_BYTES: &[u8] = include_bytes!("../../../assets/icone.ico");
    let decoded = image::load_from_memory_with_format(ICON_BYTES, image::ImageFormat::Ico)
        .map_err(|err| log::warn!("Ícone da janela não pôde ser decodificado: {err}"))
        .ok()?;
    let rgba = decoded.into_rgba8();
    let (width, height) = rgba.dimensions();
    Some(egui::IconData {
        rgba: rgba.into_raw(),
        width,
        height,
    })
}
