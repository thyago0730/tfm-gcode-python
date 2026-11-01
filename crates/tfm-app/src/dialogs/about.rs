//! "Sobre": versão e créditos (o texto do `_show_about_dialog` do Python).

use egui::RichText;

use crate::app::TfmApp;
use crate::dialogs::{self, DialogId};

/// Versão exibida (a do crate: 2.0.0).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Linhas do texto do "Sobre", exatamente como no Python.
pub fn about_lines() -> [String; 3] {
    [
        format!("TFM G-Code Generator v{VERSION}"),
        "Desenvolvido para TFM Usinagem & Manutenção Industrial LTDA.".to_string(),
        "Todos os direitos reservados.".to_string(),
    ]
}

pub fn show(app: &mut TfmApp, ctx: &egui::Context, open: &mut bool) {
    let mut close = false;
    let muted = app.palette.muted;
    dialogs::window(DialogId::About, open)
        .default_width(420.0)
        .show(ctx, |ui| {
            let [title, company, rights] = about_lines();
            ui.add_space(6.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(title).heading().strong());
                ui.add_space(4.0);
                ui.label(company);
                ui.label(RichText::new(rights).color(muted));
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "Cladding & Hardfacing — revestimento PTA com rastreabilidade ISO 3834.",
                    )
                    .small()
                    .color(muted),
                );
            });
            dialogs::footer(ui, |ui| {
                if ui.button("Fechar").clicked() {
                    close = true;
                }
            });
        });
    if close {
        *open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texto_do_sobre() {
        let [t, c, r] = about_lines();
        assert_eq!(t, "TFM G-Code Generator v2.0.0");
        assert_eq!(
            c,
            "Desenvolvido para TFM Usinagem & Manutenção Industrial LTDA."
        );
        assert_eq!(r, "Todos os direitos reservados.");
    }
}
