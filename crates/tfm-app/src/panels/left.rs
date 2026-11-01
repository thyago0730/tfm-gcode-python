//! Painel esquerdo: cartão da operação atual (ícone, nome, "Padrão", ▼/▲),
//! lista recolhível das sete operações e as abas Parâmetros / Processo /
//! Rastreabilidade, com os mesmos rótulos, defaults e regras de visibilidade
//! da UI Python (`_create_widgets`, `_refresh_*`) e um tooltip didático por
//! campo. Qualquer edição marca o estado como sujo ([`UiState::mark_edited`]).

use egui::{
    pos2, vec2, Align, Align2, CollapsingHeader, ComboBox, CornerRadius, CursorIcon, FontId, Frame,
    Label, Layout, Margin, Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui,
    Vec2,
};
use tfm_core::config::AppConfig;
use tfm_core::params::{
    BolachaTipo, Direction, FlatOrientation, MultiperfilStrategy, Operation, ProfileShape, Rotation,
};
use tfm_core::quality::QualityManager;

use crate::app::TfmApp;
use crate::icons::{draw_operation_icon, operation_icon};
use crate::theme::Palette;
use crate::ui_state::{has_advanced, internal_applicable, is_oscillation, LeftTab, UiState};
use crate::widgets::{
    check_field, combo_field, kpi, num_field, section, text_field, NUM_FIELD_WIDTH,
};

/// Tamanho do ícone da operação (o Canvas Tk tinha 40 × 35).
const ICON_SIZE: Vec2 = vec2(40.0, 35.0);
/// Altura de cada cartão da lista de operações.
const CARD_H: f32 = 47.0;
/// Margem interna dos cartões.
const CARD_PAD: f32 = 8.0;
/// Raio dos cantos dos cartões.
const CARD_RADIUS: CornerRadius = CornerRadius::same(6);

/// Tooltips didáticos, um por campo (textos fixados no brief da task).
mod tip {
    pub const DIAMETRO: &str = "Diâmetro externo da peça onde o cordão será depositado. Define a circunferência e a rotação.";
    pub const DIAMETRO_INTERNO: &str = "Diâmetro interno da peça: o furo da bolacha ou a superfície revestida na operação interna.";
    pub const COMPRIMENTO: &str =
        "Trecho em X que recebe revestimento. Maior = mais voltas e mais tempo.";
    pub const LARGURA_CHAPA: &str = "Largura da chapa, no eixo do passo, que recebe revestimento.";
    pub const LARGURA_CORDAO: &str =
        "Largura real de um cordão depositado. Meça num teste; define o passo.";
    pub const SOBREPOSICAO: &str =
        "Quanto um cordão cobre o vizinho, em %. Maior = passo menor, mais uniforme, mais tempo.";
    pub const AFASTAMENTO_TOCHA: &str = "Distância da tocha até a peça em Z (stand-off).";
    pub const AFAST_SEGURANCA: &str = "Altura extra em Z para movimentos rápidos entre camadas.";
    pub const VELOCIDADE: &str =
        "Velocidade do cordão em mm/min. Maior = menos aporte térmico por mm.";
    pub const NUM_CAMADAS: &str = "Quantas camadas sobrepostas. Cada camada sobe Z pela espessura.";
    pub const ESPESSURA: &str = "Altura que cada camada acrescenta.";
    pub const SENTIDO_SOLDAGEM: &str = "Direção do avanço em X.";
    pub const SENTIDO_ROTACAO: &str = "Sentido do eixo A (CW/CCW), independente do avanço.";
    pub const COMPRIMENTO_OSC: &str = "Curso da oscilação em X dentro de cada volta.";
    pub const DESLOC_ANGULAR: &str = "Avanço angular entre oscilações, em % da largura do cordão.";
    pub const GRAN_X: &str =
        "Tamanho do subpasso em X na quadrada contínua. Menor = mais linhas, movimento mais suave.";
    pub const GRAN_A: &str = "Tamanho do subpasso em graus na quadrada contínua.";
    pub const QTD_PERFIS: &str = "Quantos cordões helicoidais paralelos ao redor do eixo.";
    pub const ANGULO_SETOR: &str = "Setor angular ocupado pelos perfis. 360° distribui igualmente.";
    pub const PASSO_PERFIL: &str = "Avanço axial por volta de cada perfil.";
    pub const ALTURA_PERFIL: &str = "Altura total do perfil; define quantas camadas.";
    pub const ESTRATEGIA: &str =
        "Passes separados para entre perfis (M01); Percurso contínuo não para.";
    pub const PONTO_A_PONTO: &str = "Para (M0) antes de cada cordão para conferência.";
    pub const FORMATO_PERFIL: &str = "Retangular, trapezoidal (base e topo) ou triangular.";
    pub const LARGURA_BASE: &str = "Largura do perfil na base, em mm.";
    pub const LARGURA_TOPO: &str = "Largura do perfil no topo, em mm.";
    pub const CHANFRO: &str =
        "Encurta as camadas superiores nas pontas para formar o ângulo informado.";
    pub const TIPO_BOLACHA: &str =
        "Espiral contínua de Arquimedes ou anéis concêntricos passo a passo.";
    pub const PARAR_ANEIS: &str = "Desliga a tocha e sobe Z entre um anel e o próximo.";
    pub const SOBREPOSICAO_ANG: &str = "Graus extras ao fim de cada volta para fechar a costura.";
    pub const ATRASO_LIGAR: &str =
        "Liga a saída da tocha e espera esse tempo antes de iniciar o cordão.";
    pub const ATRASO_DESLIGAR: &str = "Desliga a saída da tocha e mantém o movimento do cordão por esse tempo para compensar o apagamento.";
    pub const CALIBRACAO: &str =
        "Ajusta a largura efetiva do cordão (ex.: 0,95 reduz 5%, 1,05 aumenta 5%).";
    pub const OPERACAO_INTERNA: &str =
        "Reveste o diâmetro interno; a tocha entra pela face e Z cresce para dentro.";
    pub const AFASTAMENTO_FACE: &str =
        "Posição X segura fora da peça para entrar e sair em operações internas.";
    pub const FECHAMENTO_UNIFORME: &str = "O comprimento passa a valer de borda a borda do \
        material depositado: o centro do cordão anda meia largura para dentro de cada lado e as \
        voltas viram inteiras nesse curso. Na Espiral, cada ponta fecha num anel de 360° (já \
        inclui \"Fechar início/fim\"). Nas oscilações, a banda inteira cabe no comprimento e o \
        apagamento gira só em A. A sobreposição só aumenta; o passo real aparece nos resultados.";
    pub const COMPENSAR_ROTACAO: &str = "Simula o material depositado e ajusta a rotação por trecho. \
        Na Espiral acelera onde o material acumula, com menos sobremetal, pó e tempo. Nas quadradas, \
        junto com o fechamento uniforme, desacelera nas pontas, onde falta material, gastando mais \
        pó e tempo. A rotação muda até 2,5 vezes: confira se ela continua dentro da EPS.";
    pub const FECHAR_PONTAS: &str =
        "Dá uma volta completa parada no início e no fim para fechar o cordão.";
    pub const RECUAR: &str = "Vai ao Z e Y zero máquina (G53) antes de ligar a tocha.";
    pub const UNIDIRECIONAL: &str =
        "Cada cordão no mesmo sentido, com retorno rápido e tocha desligada.";
    pub const ORIENTACAO: &str = "X: cordão em X, passo em Y. Y: cordão em Y, passo em X.";
    pub const LEAD_EXTRAS: &str =
        "Acrescenta um trecho extra em X antes do início e depois do término do revestimento.";
    pub const LEAD_IN: &str = "Trecho extra em X antes do início do revestimento.";
    pub const LEAD_OUT: &str = "Trecho extra em X depois do término do revestimento.";
    pub const CAMADAS_INTELIGENTES: &str = "Primeira camada (raiz) mais lenta, pelo % informado.";
    pub const ROOT_FEED: &str =
        "Percentual da velocidade aplicado à camada raiz com as camadas inteligentes ativas.";
    pub const CORRENTE: &str = "Corrente do arco em A, usada no aporte térmico.";
    pub const TENSAO: &str = "Tensão do arco em V.";
    pub const VAZAO_GAS: &str = "Argônio em l/min; entra no consumo e no custo.";
    pub const ALIM_PO: &str = "Ajuste do alimentador em %, só para registro.";
    pub const PREAQUECIMENTO: &str = "Temperatura de pré-aquecimento da peça, só para registro.";
    pub const EFICIENCIA: &str = "Fator de rendimento do arco (0 a 1) no cálculo do heat input.";
    pub const TAXA_G_H: &str =
        "Massa de pó depositada por hora; usada nas fórmulas de tempo por massa.";
    pub const VEL_OSC: &str = "Velocidade dos movimentos em X nas oscilações.";
    pub const CONSUMIVEL: &str = "Pó ativo: define fator de massa (g/mm²) e custo por kg.";
    pub const IDENTIFICACAO: &str = "Identificação gravada no cabeçalho e no procedimento salvo.";
    pub const RASTREABILIDADE: &str = "Registrado no cabeçalho do G-code (ISO 3834).";
    pub const AMBIENTE: &str = "Condições ambientais registradas no procedimento.";
    pub const NOTAS: &str = "Observações livres gravadas no procedimento.";
    pub const PADRAO: &str = "Restaura os valores padrão do modo atual.";
    pub const HEAT_INPUT: &str = "Aporte térmico: tensão × corrente × eficiência ÷ velocidade. Fora de 0,1 a 3,0 kJ/mm fica em vermelho.";
    pub const DILUICAO: &str =
        "Diluição estimada pela corrente e pela taxa de pó. Fora de 2 a 15 % fica em vermelho.";
}

/// Listas e valores calculados antes de emprestar `app.ui` de forma mutável.
struct Context {
    palette: Palette,
    /// `(heat input kJ/mm, diluição %)` do último resultado válido.
    kpis: Option<(f64, f64)>,
    powders: Vec<String>,
    operators: Vec<String>,
    wps: Vec<String>,
}

/// Painel esquerdo completo.
pub fn show_left(app: &mut TfmApp, ui: &mut Ui) {
    app.ui.refresh_invalid(&app.cfg);
    let ctx = Context {
        palette: app.palette,
        kpis: app
            .result
            .as_ref()
            .filter(|r| r.field_errors.is_valid())
            .map(|r| (r.estimates.heat_input_kj_mm, r.estimates.dilution_pct)),
        powders: powder_names(&app.cfg, &app.quality),
        operators: app.quality.operator_names(),
        wps: app.quality.wps_names(),
    };
    // No multiperfil o número de camadas é derivado do perfil (campo "auto").
    if app.ui.operation == Operation::EspiralMultiperfil {
        let layers = app.ui.to_params(&app.cfg).num_camadas;
        app.ui.sync_layer_count(layers);
    }

    let cfg = &app.cfg;
    let state = &mut app.ui;
    operation_selector(state, &ctx.palette, ui);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        for tab in LeftTab::ALL {
            ui.selectable_value(&mut state.tab, tab, tab.label());
        }
    });
    ui.separator();

    let mut form = Form {
        state,
        changed: false,
    };
    ScrollArea::vertical()
        .id_salt("left_panel_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| match form.state.tab {
            LeftTab::Parametros => tab_parametros(&mut form, ui),
            LeftTab::Processo => tab_processo(&mut form, &ctx, cfg, ui),
            LeftTab::Rastreabilidade => tab_rastreabilidade(&mut form, &ctx, ui),
        });
    if form.changed {
        form.state.mark_edited();
    }
}

/// Pós do config mais os materiais padrão da qualidade, sem repetição, em
/// ordem alfabética (como o combo "Consumível (Pó)" do Python).
fn powder_names(cfg: &AppConfig, quality: &QualityManager) -> Vec<String> {
    let mut names: Vec<String> = cfg
        .powders
        .iter()
        .map(|p| p.name.clone())
        .filter(|name| !name.trim().is_empty())
        .chain(quality.material_names())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Acesso aos campos do estado com registro de mudança.
struct Form<'a> {
    state: &'a mut UiState,
    changed: bool,
}

impl Form<'_> {
    /// Campo numérico; vermelho quando a chave está em `invalid`.
    fn num(&mut self, ui: &mut Ui, label: &str, key: &'static str, tip: &str) {
        let invalid = self.state.invalid.contains(key);
        if let Some(text) = self.state.fields.get_mut(key) {
            if num_field(ui, label, text, tip, invalid) {
                self.changed = true;
            }
        }
    }

    fn text(&mut self, ui: &mut Ui, label: &str, key: &'static str, tip: &str) {
        if let Some(text) = self.state.fields.get_mut(key) {
            if text_field(ui, label, text, tip) {
                self.changed = true;
            }
        }
    }

    /// Caixa de seleção; devolve o valor atual.
    fn flag(&mut self, ui: &mut Ui, label: &str, key: &'static str, tip: &str) -> bool {
        let Some(value) = self.state.flags.get_mut(key) else {
            return false;
        };
        if check_field(ui, label, value, tip) {
            self.changed = true;
        }
        *value
    }

    fn combo<T: PartialEq + Copy>(
        &mut self,
        ui: &mut Ui,
        label: &str,
        pick: fn(&mut UiState) -> &mut T,
        options: &[(T, &str)],
        tip: &str,
    ) {
        let value = pick(self.state);
        if combo_field(ui, label, value, options, tip) {
            self.changed = true;
        }
    }

    /// Campo de texto com um menu ▼ que preenche a partir de uma lista
    /// (operadores e EPS cadastrados), mantendo a digitação livre.
    fn combo_text(
        &mut self,
        ui: &mut Ui,
        label: &str,
        key: &'static str,
        options: &[String],
        tip: &str,
    ) {
        let mut chosen: Option<String> = None;
        row(ui, label, tip, |ui| {
            let menu = ui.menu_button("▼", |ui| {
                if options.is_empty() {
                    ui.weak("Nenhum cadastrado (menu Qualidade)");
                }
                for name in options {
                    if ui.button(name).clicked() {
                        chosen = Some(name.clone());
                        ui.close();
                    }
                }
            });
            menu.response.on_hover_text("Escolher da lista cadastrada");
            if let Some(text) = self.state.fields.get_mut(key) {
                let response = TextEdit::singleline(text)
                    .desired_width(ui.available_width())
                    .show(ui)
                    .response
                    .response;
                if response.changed() {
                    self.changed = true;
                }
                response.on_hover_text(tip);
            }
        });
        if let Some(name) = chosen {
            self.state.fields.insert(key, name);
            self.changed = true;
        }
    }
}

/// Linha padrão: rótulo (com tooltip) à esquerda e o controle da direita
/// para a esquerda no espaço restante (mesmo desenho dos campos de `widgets`).
fn row<R>(ui: &mut Ui, label: &str, tip: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.label(label).on_hover_text(tip);
        ui.with_layout(Layout::right_to_left(Align::Center), add)
            .inner
    })
    .inner
}

/// Grupo com título em negrito (porta dos `LabelFrame` aninhados do Python).
fn group(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    ui.add_space(2.0);
    Frame::group(ui.style())
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(title).strong());
            add(ui);
        });
    ui.add_space(2.0);
}

// ---------------------------------------------------------------------------
// Seletor de operação
// ---------------------------------------------------------------------------

/// Cartão da operação atual (ícone 40 × 35, nome em negrito, "Padrão" e
/// ▼/▲) e, quando aberta, a lista dos sete cartões.
fn operation_selector(state: &mut UiState, palette: &Palette, ui: &mut Ui) {
    let op = state.operation;
    let (arrow, hint) = if state.list_expanded {
        ("▲", "Recolher a lista de operações")
    } else {
        ("▼", "Escolher outra operação")
    };
    Frame::new()
        .fill(palette.card_bg)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(CARD_RADIUS)
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            // `horizontal` limita a altura a uma linha; sem ele o `with_layout`
            // centralizaria a linha no painel inteiro e empurraria as abas e os
            // campos para fora da tela.
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button(arrow).on_hover_text(hint).clicked() {
                        state.list_expanded = !state.list_expanded;
                    }
                    if ui.button("Padrão").on_hover_text(tip::PADRAO).clicked() {
                        state.reset_defaults();
                    }
                    // O resto da linha (ícone + nome) também abre/fecha a lista.
                    let size = vec2(ui.available_width().max(ICON_SIZE.x), ICON_SIZE.y);
                    let inner = ui.allocate_ui_with_layout(
                        size,
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            operation_icon(ui, op, ICON_SIZE, palette.text, palette.accent);
                            ui.add_space(4.0);
                            ui.add(
                                Label::new(RichText::new(op.label()).strong().size(14.0))
                                    .selectable(false)
                                    .truncate(),
                            );
                        },
                    );
                    let response = ui
                        .interact(
                            inner.response.rect,
                            ui.scope_id().with("op_header"),
                            Sense::click(),
                        )
                        .on_hover_text(hint)
                        .on_hover_cursor(CursorIcon::PointingHand);
                    if response.clicked() {
                        state.list_expanded = !state.list_expanded;
                    }
                });
            });
        });

    if state.list_expanded {
        ui.add_space(4.0);
        let mut chosen = None;
        for candidate in Operation::ALL {
            if operation_card(ui, palette, candidate, candidate == op) {
                chosen = Some(candidate);
            }
            ui.add_space(3.0);
        }
        if let Some(candidate) = chosen {
            state.set_operation(candidate);
        }
    }
}

/// Um cartão da lista: ícone + rótulo curto, destacado quando selecionado.
/// Devolve `true` ao clicar.
fn operation_card(ui: &mut Ui, palette: &Palette, op: Operation, selected: bool) -> bool {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, CARD_H), Sense::click());
    let response = response
        .on_hover_text(op.label())
        .on_hover_cursor(CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            palette.accent.gamma_multiply(0.18)
        } else if response.hovered() {
            ui.visuals().widgets.hovered.weak_bg_fill
        } else {
            palette.card_bg
        };
        let stroke = if selected {
            Stroke::new(1.5, palette.accent)
        } else {
            Stroke::new(1.0, palette.border)
        };
        let painter = ui.painter();
        painter.rect(rect, CARD_RADIUS, fill, stroke, StrokeKind::Inside);
        let icon_rect = Rect::from_min_size(
            pos2(rect.min.x + CARD_PAD, rect.center().y - ICON_SIZE.y / 2.0),
            ICON_SIZE,
        );
        draw_operation_icon(painter, icon_rect, op, palette.text, palette.accent);
        let color = if selected {
            palette.accent
        } else {
            palette.text
        };
        painter.text(
            pos2(icon_rect.max.x + CARD_PAD, rect.center().y),
            Align2::LEFT_CENTER,
            op.short_label(),
            FontId::proportional(13.0),
            color,
        );
    }
    response.clicked()
}

// ---------------------------------------------------------------------------
// Aba Parâmetros
// ---------------------------------------------------------------------------

fn tab_parametros(f: &mut Form, ui: &mut Ui) {
    let op = f.state.operation;
    section(ui, "Parâmetros de Revestimento", true, |ui| {
        match op {
            Operation::ChapaPlana => {
                f.num(ui, "Largura (mm):", "largura_revestir", tip::LARGURA_CHAPA);
                f.num(
                    ui,
                    "Comprimento (mm):",
                    "comprimento_revestir",
                    tip::COMPRIMENTO,
                );
            }
            Operation::Bolacha => {
                f.num(ui, "Diâm. Ext. (mm):", "diametro", tip::DIAMETRO);
                f.num(
                    ui,
                    "Diâm. Int. (mm):",
                    "diametro_interno",
                    tip::DIAMETRO_INTERNO,
                );
            }
            _ => {
                f.num(ui, "Diâmetro (mm):", "diametro", tip::DIAMETRO);
                f.num(
                    ui,
                    "Comprimento (mm):",
                    "comprimento_revestir",
                    tip::COMPRIMENTO,
                );
                if internal_applicable(op) && f.state.flag("operacao_interna") {
                    f.num(
                        ui,
                        "Diâmetro interno (mm):",
                        "diametro_interno",
                        tip::DIAMETRO_INTERNO,
                    );
                }
            }
        }

        group(ui, "Opções de Soldagem", |ui| {
            f.flag(
                ui,
                "Recuar tocha ao ligar",
                "torch_retract_on_ignite",
                tip::RECUAR,
            );
            if matches!(op, Operation::Espiral | Operation::EspiralMultiperfil) {
                f.flag(
                    ui,
                    "Fechar início/fim da espiral",
                    "spiral_close_ends",
                    tip::FECHAR_PONTAS,
                );
            }
            if matches!(
                op,
                Operation::Espiral
                    | Operation::EspiralMultiperfil
                    | Operation::OscilacaoLinear
                    | Operation::OscilacaoQuadrada
                    | Operation::OscilacaoQuadradaContinua
            ) {
                f.flag(
                    ui,
                    "Fechamento uniforme (borda a borda)",
                    "fechamento_uniforme",
                    tip::FECHAMENTO_UNIFORME,
                );
            }
            if matches!(
                op,
                Operation::Espiral
                    | Operation::OscilacaoQuadrada
                    | Operation::OscilacaoQuadradaContinua
            ) {
                f.flag(
                    ui,
                    "Compensar rotação pela densidade",
                    "compensar_rotacao",
                    tip::COMPENSAR_ROTACAO,
                );
            }
            if op == Operation::ChapaPlana {
                f.flag(
                    ui,
                    "Sentido Único (Puxando)",
                    "flat_single_direction",
                    tip::UNIDIRECIONAL,
                );
                f.combo(
                    ui,
                    "Orientação da Solda:",
                    |s| &mut s.flat_orientation,
                    &[(FlatOrientation::X, "X"), (FlatOrientation::Y, "Y")],
                    tip::ORIENTACAO,
                );
            }
            if internal_applicable(op) {
                let internal = f.flag(
                    ui,
                    "Operação interna",
                    "operacao_interna",
                    tip::OPERACAO_INTERNA,
                );
                if internal {
                    f.num(
                        ui,
                        "Afastamento da face (mm):",
                        "internal_face_clearance_x",
                        tip::AFASTAMENTO_FACE,
                    );
                }
            }
        });

        if is_oscillation(op) {
            f.num(
                ui,
                "Comprimento Osc. (mm):",
                "oscilacao_comprimento",
                tip::COMPRIMENTO_OSC,
            );
            f.num(
                ui,
                "Desloc. Angular (%):",
                "deslocamento_angular_perc",
                tip::DESLOC_ANGULAR,
            );
        }
        if op == Operation::OscilacaoQuadradaContinua {
            f.num(ui, "Granularidade X (mm):", "osc_test_gran_x", tip::GRAN_X);
            f.num(ui, "Granularidade A (°):", "osc_test_gran_a", tip::GRAN_A);
        }
        if op == Operation::EspiralMultiperfil {
            multiperfil_group(f, ui);
        }
        if op == Operation::Bolacha {
            f.combo(
                ui,
                "Tipo de Bolacha:",
                |s| &mut s.bolacha_tipo,
                &[
                    (BolachaTipo::EspiralContinua, "Espiral Contínua"),
                    (BolachaTipo::Aneis, "Anéis (Passo a Passo)"),
                ],
                tip::TIPO_BOLACHA,
            );
            f.flag(
                ui,
                "Desligar entre passes",
                "bolacha_stop_between",
                tip::PARAR_ANEIS,
            );
        }
        f.num(
            ui,
            "Afast. Segurança (mm):",
            "afastamento_seguranca",
            tip::AFAST_SEGURANCA,
        );

        if has_advanced(op) {
            advanced_group(f, ui);
        }
    });

    section(ui, "Parâmetros Gerais", true, |ui| {
        f.num(
            ui,
            "Largura Cordão (mm):",
            "largura_cordao",
            tip::LARGURA_CORDAO,
        );
        if !matches!(op, Operation::Bolacha | Operation::EspiralMultiperfil) {
            f.num(ui, "Sobreposição (%):", "sobreposicao", tip::SOBREPOSICAO);
        }
        f.num(
            ui,
            "Afastamento Tocha (mm):",
            "afastamento_tocha",
            tip::AFASTAMENTO_TOCHA,
        );
        let leads = f.flag(
            ui,
            "Usar início e fim extra",
            "lead_extras_enabled",
            tip::LEAD_EXTRAS,
        );
        if leads && op != Operation::Bolacha {
            f.num(ui, "Início Extra (mm):", "lead_in", tip::LEAD_IN);
            f.num(ui, "Término Extra (mm):", "lead_out", tip::LEAD_OUT);
        }
        f.combo(
            ui,
            "Sentido da Soldagem:",
            |s| &mut s.direcao,
            &[
                (Direction::EsquerdaDireita, "Esquerda -> Direita"),
                (Direction::DireitaEsquerda, "Direita -> Esquerda"),
            ],
            tip::SENTIDO_SOLDAGEM,
        );
        if op != Operation::ChapaPlana {
            f.combo(
                ui,
                "Sentido da Rotação:",
                |s| &mut s.rotacao,
                &[
                    (Rotation::Horaria, "Horária (CW)"),
                    (Rotation::Antihoraria, "Anti-horária (CCW)"),
                ],
                tip::SENTIDO_ROTACAO,
            );
        }
    });

    section(ui, "Parâmetros de Camada", true, |ui| {
        if op == Operation::EspiralMultiperfil {
            ui.add_enabled_ui(false, |ui| {
                f.num(ui, "Nº de Camadas (auto):", "num_camadas", tip::NUM_CAMADAS);
            });
        } else {
            f.num(ui, "Nº de Camadas:", "num_camadas", tip::NUM_CAMADAS);
        }
        f.num(
            ui,
            "Espessura / Camada (mm):",
            "espessura_camada",
            tip::ESPESSURA,
        );
    });
}

/// "Perfis ao Redor do Eixo" (só Espiral Multiperfil).
fn multiperfil_group(f: &mut Form, ui: &mut Ui) {
    group(ui, "Perfis ao Redor do Eixo", |ui| {
        f.num(
            ui,
            "Quantidade de perfis:",
            "multiperfil_count",
            tip::QTD_PERFIS,
        );
        f.num(
            ui,
            "Ângulo total do setor (°):",
            "multiperfil_total_angle_deg",
            tip::ANGULO_SETOR,
        );
        f.num(
            ui,
            "Passo por perfil (mm):",
            "multiperfil_pitch_mm",
            tip::PASSO_PERFIL,
        );
        f.num(
            ui,
            "Altura do perfil (mm):",
            "multiperfil_profile_height_mm",
            tip::ALTURA_PERFIL,
        );
        f.combo(
            ui,
            "Estratégia:",
            |s| &mut s.multiperfil_strategy,
            &[
                (MultiperfilStrategy::PassesSeparados, "Passes separados"),
                (MultiperfilStrategy::PercursoContinuo, "Percurso contínuo"),
            ],
            tip::ESTRATEGIA,
        );
        f.flag(
            ui,
            "Ponto a ponto",
            "multiperfil_point_by_point",
            tip::PONTO_A_PONTO,
        );
        f.combo(
            ui,
            "Formato do perfil:",
            |s| &mut s.profile_shape,
            &[
                (ProfileShape::Retangular, "Retangular"),
                (ProfileShape::Trapezoidal, "Trapezoidal"),
                (ProfileShape::Triangular, "Triangular"),
            ],
            tip::FORMATO_PERFIL,
        );
        f.num(
            ui,
            "Largura da base (mm):",
            "multiperfil_profile_base_mm",
            tip::LARGURA_BASE,
        );
        if f.state.profile_shape == ProfileShape::Trapezoidal {
            f.num(
                ui,
                "Largura do topo (mm):",
                "multiperfil_profile_top_mm",
                tip::LARGURA_TOPO,
            );
        }
        let chamfer = f.flag(
            ui,
            "Aplicar chanfro nas pontas",
            "multiperfil_end_chamfer_enabled",
            tip::CHANFRO,
        );
        if chamfer {
            f.num(
                ui,
                "Ângulo do chanfro (graus):",
                "multiperfil_end_chamfer_angle_deg",
                tip::CHANFRO,
            );
        }
    });
}

/// "Avançado": recolhido por padrão, aberto por `show_advanced`
/// (sobreposição angular só na Bolacha, atrasos da tocha e calibração).
fn advanced_group(f: &mut Form, ui: &mut Ui) {
    let open = f.state.show_advanced;
    let bolacha = f.state.operation == Operation::Bolacha;
    let response = CollapsingHeader::new(RichText::new("Avançado").strong())
        .id_salt("left_advanced")
        .open(Some(open))
        .show(ui, |ui| {
            if bolacha {
                f.num(
                    ui,
                    "Sobreposição Ang. (°):",
                    "overlap_angular_deg",
                    tip::SOBREPOSICAO_ANG,
                );
            }
            f.num(
                ui,
                "Atraso para ligar (s):",
                "torch_ignite_delay_s",
                tip::ATRASO_LIGAR,
            );
            f.num(
                ui,
                "Atraso para desligar (s):",
                "torch_extinguish_delay_s",
                tip::ATRASO_DESLIGAR,
            );
            f.num(
                ui,
                "Calibração Largura (x):",
                "bead_width_calibration_factor",
                tip::CALIBRACAO,
            );
        });
    if response.header_response.clicked() {
        f.state.show_advanced = !open;
    }
}

// ---------------------------------------------------------------------------
// Aba Processo
// ---------------------------------------------------------------------------

fn tab_processo(f: &mut Form, ctx: &Context, cfg: &AppConfig, ui: &mut Ui) {
    section(ui, "Parâmetros do Processo", true, |ui| {
        f.text(
            ui,
            "Nome Procedimento:",
            "nome_procedimento",
            tip::IDENTIFICACAO,
        );
        f.text(ui, "OS:", "ordem_servico", tip::IDENTIFICACAO);
        powder_combo(f, cfg, &ctx.powders, ui);
        f.num(ui, "Corrente Arco (A):", "corrente_arco", tip::CORRENTE);
        f.num(ui, "Tensão Arco (V):", "tensao_arco", tip::TENSAO);
        f.num(ui, "Vazão Gás (l/min):", "vazao_gas", tip::VAZAO_GAS);
        f.num(ui, "Alimentação Pó (%):", "alim_po", tip::ALIM_PO);
        f.num(
            ui,
            "Pré-aquecimento (°C):",
            "preaquecimento",
            tip::PREAQUECIMENTO,
        );
        f.num(
            ui,
            "Eficiência Térmica (0-1):",
            "eficiencia_termica",
            tip::EFICIENCIA,
        );
        f.num(
            ui,
            "Taxa de deposição (g/h):",
            "taxa_deposicao_g_h",
            tip::TAXA_G_H,
        );
        f.num(
            ui,
            "Velocidade de deposição (mm/min):",
            "velocidade_de_deposicao",
            tip::VELOCIDADE,
        );
        f.num(
            ui,
            "Velocidade da oscilação (mm/min):",
            "velocidade_oscilacao_mm_min",
            tip::VEL_OSC,
        );

        ui.separator();
        f.flag(
            ui,
            "Camadas Inteligentes",
            "smart_layers",
            tip::CAMADAS_INTELIGENTES,
        );
        f.num(ui, "% Vel. Raiz:", "root_feed_scale", tip::ROOT_FEED);

        ui.separator();
        process_kpis(ctx, ui);

        ui.separator();
        ui.label("Notas:").on_hover_text(tip::NOTAS);
        if let Some(notes) = f.state.fields.get_mut("notes") {
            let response = TextEdit::multiline(notes)
                .desired_rows(4)
                .desired_width(f32::INFINITY)
                .hint_text("Observações do procedimento")
                .show(ui)
                .response
                .response;
            if response.changed() {
                f.changed = true;
            }
            response.on_hover_text(tip::NOTAS);
        }
    });
}

/// Heat Input (kJ/mm) e Diluição Est. (%) do último resultado, com as mesmas
/// faixas de alerta do Python (0,1 a 3,0 kJ/mm; 2 a 15 %).
fn process_kpis(ctx: &Context, ui: &mut Ui) {
    let palette = &ctx.palette;
    match ctx.kpis {
        Some((heat, dilution)) => {
            let heat_color = if !(0.1..=3.0).contains(&heat) {
                palette.danger
            } else {
                palette.success
            };
            let dilution_color = if !(2.0..=15.0).contains(&dilution) {
                palette.danger
            } else {
                palette.success
            };
            kpi(
                ui,
                "Heat Input:",
                &format!("{heat:.2} kJ/mm"),
                Some(heat_color),
            );
            kpi(
                ui,
                "Diluição Est.:",
                &format!("{dilution:.1} %"),
                Some(dilution_color),
            );
        }
        None => {
            kpi(ui, "Heat Input:", "- kJ/mm", None);
            kpi(ui, "Diluição Est.:", "- %", None);
        }
    }
    ui.weak(RichText::new(tip::HEAT_INPUT).small());
    ui.weak(RichText::new(tip::DILUICAO).small());
}

/// Combo "Consumível (Pó)": pós do config e materiais padrão; vazio no
/// estado significa o pó ativo do config.
fn powder_combo(f: &mut Form, cfg: &AppConfig, powders: &[String], ui: &mut Ui) {
    let current = f.state.effective_powder(cfg).to_string();
    let mut chosen: Option<String> = None;
    row(ui, "Consumível (Pó):", tip::CONSUMIVEL, |ui| {
        let response = ComboBox::from_id_salt("left_powder")
            .width(ui.available_width().max(NUM_FIELD_WIDTH))
            .selected_text(current.as_str())
            .show_ui(ui, |ui| {
                if powders.is_empty() {
                    ui.weak("Nenhum pó cadastrado (Configurações > Tipos de Pó)");
                }
                for name in powders {
                    if ui.selectable_label(*name == current, name).clicked() {
                        chosen = Some(name.clone());
                    }
                }
            })
            .response;
        response.on_hover_text(tip::CONSUMIVEL);
    });
    if let Some(name) = chosen {
        if name != current {
            f.state.powder_name = name;
            f.changed = true;
        }
    }
}

// ---------------------------------------------------------------------------
// Aba Rastreabilidade
// ---------------------------------------------------------------------------

fn tab_rastreabilidade(f: &mut Form, ctx: &Context, ui: &mut Ui) {
    section(
        ui,
        "Rastreabilidade & Condições Ambientais (ISO 3834)",
        true,
        |ui| {
            f.combo_text(
                ui,
                "Operador:",
                "traceability_operator",
                &ctx.operators,
                tip::RASTREABILIDADE,
            );
            f.combo_text(
                ui,
                "EPS (WPS):",
                "traceability_eps",
                &ctx.wps,
                tip::RASTREABILIDADE,
            );
            f.text(
                ui,
                "Lote Pó:",
                "traceability_powder_batch",
                tip::RASTREABILIDADE,
            );
            f.text(
                ui,
                "Fornecedor:",
                "traceability_supplier",
                tip::RASTREABILIDADE,
            );
            f.text(
                ui,
                "Certificado:",
                "traceability_certificate",
                tip::RASTREABILIDADE,
            );
            ui.separator();
            f.text(
                ui,
                "Temp. Amb. (°C):",
                "traceability_env_temp",
                tip::AMBIENTE,
            );
            f.text(
                ui,
                "Umidade (%):",
                "traceability_env_humidity",
                tip::AMBIENTE,
            );
            if ctx.operators.is_empty() || ctx.wps.is_empty() {
                ui.add_space(4.0);
                ui.weak("Cadastre operadores e EPS no menu Qualidade para preencher pela lista.");
            }
        },
    );
}
