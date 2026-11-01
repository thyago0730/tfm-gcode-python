//! "Atalhos": tabela dos atalhos de teclado do app. Este diálogo só os
//! documenta; os manipuladores ficam no app (Task 23).

use egui::RichText;

use crate::app::TfmApp;
use crate::dialogs::{self, DialogId};

/// `(teclas, ação)`, na ordem exibida.
pub const SHORTCUTS: [(&str, &str); 9] = [
    ("Ctrl+S", "Salvar procedimento"),
    ("Ctrl+O", "Carregar procedimento"),
    ("Ctrl+G", "Gerar G-code"),
    ("Ctrl+M", "Abrir no Mach3"),
    ("Ctrl+Shift+C", "Copiar G-code"),
    ("F5", "Recalcular"),
    ("F1", "Ajuda"),
    ("Espaço", "Play/pause da simulação"),
    (
        "1–5",
        "Vistas: Isométrica, Superior, Frontal, Esquerda, Direita",
    ),
];

pub fn show(app: &mut TfmApp, ctx: &egui::Context, open: &mut bool) {
    let mut close = false;
    let accent = app.palette.accent;
    dialogs::window(DialogId::Shortcuts, open)
        .default_width(440.0)
        .show(ctx, |ui| {
            ui.label("Atalhos de teclado disponíveis na janela principal.");
            ui.add_space(4.0);
            egui::Grid::new("atalhos")
                .num_columns(2)
                .striped(true)
                .spacing([18.0, 6.0])
                .show(ui, |ui| {
                    ui.strong("Teclas");
                    ui.strong("Ação");
                    ui.end_row();
                    for (keys, action) in SHORTCUTS {
                        ui.label(RichText::new(keys).monospace().color(accent));
                        ui.label(action);
                        ui.end_row();
                    }
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
    fn lista_completa_de_atalhos() {
        let keys: Vec<&str> = SHORTCUTS.iter().map(|(k, _)| *k).collect();
        assert_eq!(
            keys,
            [
                "Ctrl+S",
                "Ctrl+O",
                "Ctrl+G",
                "Ctrl+M",
                "Ctrl+Shift+C",
                "F5",
                "F1",
                "Espaço",
                "1–5"
            ]
        );
    }
}
