//! "Matriz de Riscos e Controles - ISO 3834 / ISO 9001": tabela fixa com os
//! riscos mapeados no levantamento ISO 3834 (`_show_risk_matrix` do Python,
//! linhas copiadas verbatim).

use egui::{Label, RichText};

use crate::app::TfmApp;
use crate::dialogs::{self, DialogId};

/// Cabeçalhos das cinco colunas.
pub const HEADERS: [&str; 5] = [
    "Risco Identificado",
    "Causa Potencial",
    "Impacto / Severidade",
    "Controle Lógico (TFM GCODE)",
    "Ação Operacional",
];

/// `(risco, causa, impacto, controle existente no software, ação preventiva)`.
pub const RISKS: [[&str; 5]; 7] = [
    [
        "Colisão por falha de Homing",
        "G53 ativo sem REF ALL HOME",
        "Danos severos à tocha/eixo",
        "Alerta G53 modal antes de salvar. Validação de segurança.",
        "Verificar status de REF no Mach3.",
    ],
    [
        "Parâmetro fora da EPS",
        "Erro de digitação ou ajuste indevido",
        "Não conformidade metalúrgica",
        "Bloqueio lógico via 'Limits' da EPS cadastrada.",
        "Usar apenas EPS qualificada.",
    ],
    [
        "Operador não qualificado",
        "Qualificação vencida ou inexistente",
        "Falha de processo/Rastreabilidade",
        "Validação de data de validade antes de gerar código.",
        "Treinamento e recertificação.",
    ],
    [
        "Geometria Inválida",
        "Oscilação > Diâmetro ou Sobreposição > 100%",
        "Cordão defeituoso / Falha deposição",
        "Validação matemática (_validate_parameters) com bloqueio.",
        "Conferir dimensões da peça.",
    ],
    [
        "Falta de Consumíveis",
        "Campos de lote/fornecedor vazios",
        "Perda de Rastreabilidade (ISO 3834)",
        "Campos obrigatórios condicionados à seleção de EPS.",
        "Preenchimento correto do MOP.",
    ],
    [
        "Alteração não autorizada",
        "Mudança de custos/fórmulas",
        "Erro de orçamentação/processo",
        "Audit Log (Registro de Eventos) automático.",
        "Restrição de acesso à Eng.",
    ],
    [
        "Arco aberto fora da peça",
        "Lead-in/Lead-out incorreto",
        "Danos à peça/Retrabalho",
        "Simulação 3D (visualização) e validação de limites.",
        "Conferir 'Previsão' no gráfico.",
    ],
];

pub fn show(app: &mut TfmApp, ctx: &egui::Context, open: &mut bool) {
    let mut close = false;
    let accent = app.palette.accent;
    dialogs::window(DialogId::RiskMatrix, open)
        .default_size([900.0, 500.0])
        .vscroll(true)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Matriz de Análise de Risco e Controles Operacionais")
                        .strong()
                        .size(15.0),
                );
            });
            ui.add_space(6.0);
            egui::Grid::new("matriz_riscos")
                .num_columns(5)
                .striped(true)
                .min_col_width(120.0)
                .max_col_width(230.0)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    for header in HEADERS {
                        ui.add(Label::new(RichText::new(header).strong().color(accent)).wrap());
                    }
                    ui.end_row();
                    for row in RISKS {
                        for (i, cell) in row.iter().enumerate() {
                            let text = if i == 0 {
                                RichText::new(*cell).strong()
                            } else {
                                RichText::new(*cell)
                            };
                            ui.add(Label::new(text).wrap());
                        }
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
    fn linhas_do_python() {
        assert_eq!(RISKS.len(), 7);
        assert_eq!(RISKS[0][0], "Colisão por falha de Homing");
        assert_eq!(RISKS[1][0], "Parâmetro fora da EPS");
        assert_eq!(RISKS[2][0], "Operador não qualificado");
        assert_eq!(RISKS[3][0], "Geometria Inválida");
        assert_eq!(RISKS[4][0], "Falta de Consumíveis");
        assert!(RISKS.iter().all(|r| r.iter().all(|c| !c.is_empty())));
    }
}
