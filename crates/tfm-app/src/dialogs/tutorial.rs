//! "Ajuda": tutorial em abas com os textos do `_show_tutorial_dialog` do
//! Python, copiados verbatim (a menção ao relatório PDF fica como está).

use egui::RichText;

use crate::app::TfmApp;
use crate::dialogs::{self, DialogId};

#[derive(Clone, Debug, Default)]
pub struct TutorialState {
    /// Índice da aba ativa em [`TABS`].
    pub tab: usize,
}

impl TutorialState {
    pub const EMPTY: Self = TutorialState { tab: 0 };
}

const INTRO: &str = "TFM G-Code Generator

Gera G-code para revestimento PTA em peças cilíndricas com suporte a ISO 3834.

Fluxo:
- Configure os parâmetros de processo e rastreabilidade
- Visualize e analise o percurso
- Gere e revise o G-code (bloqueio automático por regras ISO)
- Salve/Carregue procedimentos
- Exporte relatório PDF com dados de rastreabilidade";

const PARAMS: &str = "Parâmetros de Revestimento
- Tipos: Espiral, Oscilação Linear, Oscilação Quadrada, Oscilação Quadrada Contínua
- Oscilações: informe Comprimento de Oscilação e Deslocamento Angular
- Quadrada Contínua: use Granularidade X para tamanho dos subpassos

Parâmetros Gerais
- Geometria e processo: Comprimento, Largura do cordão, Sobreposição, Afastamento, Leads
- Sentidos: Soldagem (X) e Rotação (A)

Camadas e Processo
- Número de camadas, espessura, corrente, vazão, alimentação de pó, pré-aquecimento.
- Tipo de Pó: define fator e custo.

Rastreabilidade (ISO 3834)
- Operador: Selecione o operador qualificado (validação de data).
- EPS (WPS): Selecione a Especificação de Procedimento de Soldagem.
- Consumíveis: Lote do Pó, Fornecedor e Certificado (obrigatórios se EPS ativa).";

const VIEWS: &str = "Visualizações
- 3D Interativo: cilindro e percurso; botões Top/Side para orientação
- Análise Temporal: gráficos auxiliares do movimento
- Oscilação: perfil de oscilação conforme parâmetros
- Estatísticas: visão geral do processo
- Processo: gráficos complementares

Visualizador de G-code
- Busca e filtros por eixos X/Y/Z/A (os filtros ficam abaixo do campo de busca)
- Ações: salvar, baixar, abrir no Mach3";

const SAVE_LOAD: &str = "Salvar e Carregar
- Arquivo > Salvar Procedimento: grava um .json com todos os parâmetros
- Arquivo > Carregar Procedimento: restaura exatamente o que foi salvo
- Duplo clique em .json associado: carrega automaticamente o procedimento";

const QUALITY: &str = "Gestão da Qualidade (ISO 3834)

Menu Qualidade:
- Gerenciar Operadores: Cadastro com controle de validade de qualificação.
- Gerenciar EPS (WPS): Cadastro de procedimentos com limites de parâmetros (Min/Max).

Regras de Bloqueio:
- Geração de G-code bloqueada se qualificação do operador estiver vencida.
- Geração bloqueada se parâmetros fugirem dos limites da EPS selecionada.
- Auditoria: Logs automáticos de alterações em materiais, fórmulas e custos.";

const REPORTS: &str = "Relatórios
- Arquivo > Gerar Relatório PDF
- Inclui todos os parâmetros, estatísticas calculadas e seção de Rastreabilidade ISO 3834.";

const RISK_MATRIX: &str = "Matriz de Risco (ISO 3834 + ISO 9001)

Riscos Mapeados e Controles:

1. Colisão por falha de homing
   - Causa: G53 ativo sem referenciamento
   - Impacto: Danos à tocha
   - Controle: Alerta no MOP, Bloqueio lógico no software

2. Arco aberto fora da peça
   - Causa: Lead-in incorreto
   - Impacto: Retrabalho
   - Controle: Simulação 3D, Validação geométrica automática

3. Parâmetro fora da EPS
   - Causa: Erro de operador
   - Impacto: Não conformidade
   - Controle: Travamento por faixa aceitável (ISO 3834), Validação de limites

4. Qualificação Vencida
   - Causa: Gestão administrativa
   - Impacto: Auditoria reprovada
   - Controle: Bloqueio de geração de G-code se validade expirada";

const TIPS: &str = "Dicas
- Ajuste Sobreposição para controlar o passo axial
- Em Quadrada Contínua, Granularidade X regula o tamanho dos subpassos
- Custos usam Configurações > Custos de Operação e o Tipo de Pó selecionado
- Use filtros por eixo no G-code para focar em movimentos específicos
- Em Operação interna, ajuste o Afastamento da face (X) para aproximação/retirada segura";

/// `(título da aba, texto)`, na ordem do Python.
pub const TABS: [(&str, &str); 8] = [
    ("Introdução", INTRO),
    ("Parâmetros", PARAMS),
    ("Visualizações", VIEWS),
    ("Salvar/Carregar", SAVE_LOAD),
    ("Qualidade ISO", QUALITY),
    ("Relatórios", REPORTS),
    ("Matriz de Risco", RISK_MATRIX),
    ("Dicas", TIPS),
];

pub fn show(app: &mut TfmApp, state: &mut TutorialState, ctx: &egui::Context, open: &mut bool) {
    let mut close = false;
    let mut open_shortcuts = false;
    let accent = app.palette.accent;
    dialogs::window(DialogId::Tutorial, open)
        .default_size([820.0, 600.0])
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (i, (title, _)) in TABS.iter().enumerate() {
                    if ui.selectable_label(state.tab == i, *title).clicked() {
                        state.tab = i;
                    }
                }
            });
            ui.separator();
            let (title, text) = TABS.get(state.tab).copied().unwrap_or(TABS[0]);
            egui::ScrollArea::vertical()
                .id_salt("tutorial_texto")
                .auto_shrink([false, false])
                .max_height(ui.available_height() - 40.0)
                .show(ui, |ui| {
                    ui.label(RichText::new(title).strong().color(accent));
                    ui.add_space(4.0);
                    ui.label(text);
                });
            dialogs::footer(ui, |ui| {
                if ui.button("Fechar").clicked() {
                    close = true;
                }
                if ui
                    .button("Atalhos")
                    .on_hover_text("Abrir a lista de atalhos de teclado")
                    .clicked()
                {
                    open_shortcuts = true;
                }
            });
        });
    if open_shortcuts {
        app.dialogs.open(DialogId::Shortcuts);
    }
    if close {
        *open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abas_do_python() {
        let titles: Vec<&str> = TABS.iter().map(|(t, _)| *t).collect();
        assert_eq!(
            titles,
            [
                "Introdução",
                "Parâmetros",
                "Visualizações",
                "Salvar/Carregar",
                "Qualidade ISO",
                "Relatórios",
                "Matriz de Risco",
                "Dicas"
            ]
        );
        assert!(INTRO.starts_with("TFM G-Code Generator\n\nGera G-code"));
        assert!(INTRO.ends_with("Exporte relatório PDF com dados de rastreabilidade"));
        assert!(TIPS.contains("Afastamento da face (X)"));
        assert!(RISK_MATRIX.contains("4. Qualificação Vencida"));
    }
}
