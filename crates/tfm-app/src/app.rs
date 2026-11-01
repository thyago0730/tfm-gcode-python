//! Estado raiz do aplicativo ([`TfmApp`]) e o layout em painéis:
//! barra superior (menus + ações), painel esquerdo (parâmetros), centro
//! (abas), painel direito (G-code) e painel inferior (resultados).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Align, Color32, Layout, RichText};
use tfm_core::audit::AuditLogger;
use tfm_core::config::AppConfig;
use tfm_core::params::{Operation, Params};
use tfm_core::quality::QualityManager;

use crate::compute::{params_key, ComputeHandle, ComputeResult};
use crate::dialogs::{self, DialogId, Dialogs};
use crate::integration::{self, IntegrationState};
use crate::panels;
use crate::panels::right::GcodeView;
use crate::prefs::{self, UiSnapshot};
use crate::theme::{self, Palette, Theme};
use crate::ui_state::UiState;
use crate::view3d::{self, View3d};

/// Quantidade máxima de avisos guardados no histórico.
pub const MAX_NOTIFICATIONS: usize = 200;
/// Espera após a última mudança de layout antes de gravar o config.
const PREFS_SAVE_DELAY: Duration = Duration::from_millis(1200);
/// Espera após a última mudança de parâmetros antes de recalcular o G-code.
const COMPUTE_DEBOUNCE: Duration = Duration::from_millis(300);
/// Largura da faixa que fica na borda quando um painel está recolhido.
const EDGE_W: f32 = 22.0;

/// Tipo de aviso mostrado no histórico do painel inferior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyKind {
    Info,
    Sucesso,
    Aviso,
    Erro,
}

impl NotifyKind {
    pub fn label(self) -> &'static str {
        match self {
            NotifyKind::Info => "Info",
            NotifyKind::Sucesso => "Sucesso",
            NotifyKind::Aviso => "Aviso",
            NotifyKind::Erro => "Erro",
        }
    }

    pub fn color(self, palette: &Palette) -> Color32 {
        match self {
            NotifyKind::Info => palette.muted,
            NotifyKind::Sucesso => palette.success,
            NotifyKind::Aviso => palette.warning,
            NotifyKind::Erro => palette.danger,
        }
    }
}

/// Um aviso com hora (`HH:MM:SS`), tipo e texto.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub time: String,
    pub kind: NotifyKind,
    pub text: String,
}

/// Abas do painel central.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CenterTab {
    View3d,
    AnaliseTemporal,
    Oscilacao,
    Estatisticas,
    Processo,
}

impl CenterTab {
    pub const ALL: [CenterTab; 5] = [
        CenterTab::View3d,
        CenterTab::AnaliseTemporal,
        CenterTab::Oscilacao,
        CenterTab::Estatisticas,
        CenterTab::Processo,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CenterTab::View3d => "3D Interativo",
            CenterTab::AnaliseTemporal => "Análise Temporal",
            CenterTab::Oscilacao => "Oscilação",
            CenterTab::Estatisticas => "Estatísticas",
            CenterTab::Processo => "Processo",
        }
    }
}

/// Estado raiz do aplicativo.
pub struct TfmApp {
    pub cfg: AppConfig,
    pub cfg_path: PathBuf,
    pub theme: Theme,
    pub palette: Palette,
    pub ui: UiState,
    pub quality: QualityManager,
    pub audit: AuditLogger,
    pub compute: ComputeHandle,
    pub result: Option<ComputeResult>,
    pub notifications: Vec<Notification>,
    pub dialogs: Dialogs,
    /// Ações que atravessam quadros (gravação do .tap pendente do alerta G53).
    pub integration: IntegrationState,
    pub gcode_view: GcodeView,
    pub view3d: View3d,
    pub center_tab: CenterTab,
    /// Painel esquerdo visível agora (fixado, ou aberto pela borda).
    pub left_open: bool,
    /// Painel direito visível agora.
    pub right_open: bool,
    /// Larguras atuais dos painéis laterais, lidas do egui a cada quadro.
    pub left_w: f32,
    pub right_w: f32,
    last_saved: UiSnapshot,
    dirty_since: Option<Instant>,
    /// Chave (parâmetros + config) vista no último quadro.
    last_key: Option<u64>,
    /// Momento da última mudança de chave (base do debounce do cálculo).
    last_change: Instant,
    /// Operação vista no último quadro (mudar de operação recalcula na hora).
    last_operation: Operation,
    /// Chave do último resultado cujos invariantes já viraram aviso.
    last_warned_key: Option<u64>,
}

impl TfmApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let dirs = prefs::dirs();
        let cfg = AppConfig::read(&dirs.config_path);
        // Como o `ConfigManager.load()` do Python: garante o arquivo e as
        // chaves de `ui`, sem reescrever o resto do config.
        if let Err(err) = prefs::save_ui_prefs(&cfg, &dirs.config_path) {
            log::warn!("Não foi possível gravar o config.json: {err}");
        }
        let snapshot = UiSnapshot::from_config(&cfg);
        log::info!("Layout inicial: {snapshot:?}");

        theme::load_fonts(&cc.egui_ctx);
        let palette = theme::apply(&cc.egui_ctx, snapshot.theme);

        let quality = QualityManager::load(&dirs.config_dir, &dirs.quality_dir);
        let audit = AuditLogger::new(dirs.logs_dir.clone());
        let compute = ComputeHandle::new(cc.egui_ctx.clone());
        let ui_state = UiState::default();
        let last_operation = ui_state.operation;

        let mut app = TfmApp {
            cfg,
            cfg_path: dirs.config_path.clone(),
            theme: snapshot.theme,
            palette,
            ui: ui_state,
            quality,
            audit,
            compute,
            result: None,
            notifications: Vec::new(),
            dialogs: Dialogs::default(),
            integration: IntegrationState::default(),
            gcode_view: GcodeView::default(),
            view3d: View3d,
            center_tab: CenterTab::View3d,
            left_open: snapshot.left_pinned,
            right_open: snapshot.right_pinned,
            left_w: snapshot.left_w,
            right_w: snapshot.right_w,
            last_saved: snapshot,
            dirty_since: None,
            last_key: None,
            last_change: Instant::now(),
            last_operation,
            last_warned_key: None,
        };
        app.notify(
            NotifyKind::Info,
            format!("Configuração carregada de {}", dirs.config_path.display()),
        );
        app
    }

    /// Registra um aviso com a hora atual (`HH:MM:SS`), mantendo só os
    /// últimos [`MAX_NOTIFICATIONS`].
    pub fn notify(&mut self, kind: NotifyKind, text: impl Into<String>) {
        let time = chrono::Local::now().format("%H:%M:%S").to_string();
        self.notifications.push(Notification {
            time,
            kind,
            text: text.into(),
        });
        if self.notifications.len() > MAX_NOTIFICATIONS {
            let excess = self.notifications.len() - MAX_NOTIFICATIONS;
            self.notifications.drain(..excess);
        }
    }

    /// Avança o tema (Automático -> Claro -> Escuro) e reaplica os visuais.
    pub fn cycle_theme(&mut self, ctx: &egui::Context) {
        self.theme = self.theme.next();
        self.palette = theme::apply(ctx, self.theme);
    }

    pub fn set_left_pinned(&mut self, pinned: bool) {
        self.cfg.ui.left_pinned = pinned;
        self.left_open = pinned;
    }

    pub fn set_right_pinned(&mut self, pinned: bool) {
        self.cfg.ui.right_pinned = pinned;
        self.right_open = pinned;
    }

    fn snapshot(&self) -> UiSnapshot {
        UiSnapshot {
            theme: self.theme,
            left_w: self.left_w.round(),
            right_w: self.right_w.round(),
            left_pinned: self.cfg.ui.left_pinned,
            right_pinned: self.cfg.ui.right_pinned,
            bottom_collapsed: self.cfg.ui.bottom_collapsed,
        }
    }

    /// Grava tema, larguras e pinos em `cfg.ui` e salva o `config.json`
    /// (só escreve em disco quando algo mudou ou o arquivo não existe).
    pub fn persist_prefs(&mut self) {
        let snapshot = self.snapshot();
        // Pinos e painel inferior já vivem em `cfg.ui`, então `store` sozinho
        // não os vê como mudança: compara também com o último snapshot salvo.
        let cfg_changed = snapshot.store(&mut self.cfg);
        let changed = cfg_changed || snapshot != self.last_saved;
        if changed || !self.cfg_path.exists() {
            match prefs::save_ui_prefs(&self.cfg, &self.cfg_path) {
                Ok(()) => log::info!(
                    "Preferências salvas em {}: {snapshot:?}",
                    self.cfg_path.display()
                ),
                Err(err) => {
                    log::warn!("Falha ao salvar preferências: {err}");
                    self.notify(
                        NotifyKind::Aviso,
                        format!("Não foi possível salvar as preferências: {err}"),
                    );
                }
            }
        }
        self.last_saved = snapshot;
        self.dirty_since = None;
    }

    /// Salva com atraso após mudanças de layout, para não gravar a cada
    /// pixel arrastado.
    fn track_prefs(&mut self, ctx: &egui::Context) {
        if self.snapshot() == self.last_saved {
            self.dirty_since = None;
            return;
        }
        let since = *self.dirty_since.get_or_insert_with(Instant::now);
        let elapsed = since.elapsed();
        if elapsed >= PREFS_SAVE_DELAY {
            self.persist_prefs();
        } else {
            ctx.request_repaint_after(PREFS_SAVE_DELAY - elapsed);
        }
    }

    fn refresh_palette(&mut self, ctx: &egui::Context) {
        let dark = ctx.theme() == egui::Theme::Dark;
        if dark != self.palette.dark {
            self.palette = Palette::for_dark(dark);
        }
    }

    /// Parâmetros atuais da interface (textos convertidos, operação
    /// escolhida aplicada e pó resolvido pelo config).
    pub fn current_params(&self) -> Params {
        self.ui.to_params(&self.cfg)
    }

    /// Decide se pede um cálculo neste quadro: na primeira vez, quando a
    /// operação muda (na hora) ou quando parâmetros/config mudaram e já
    /// passou [`COMPUTE_DEBOUNCE`] desde a última mudança. Nunca repete um
    /// pedido com a mesma chave do último feito.
    fn drive_compute(&mut self, ctx: &egui::Context) {
        let params = self.current_params();
        let key = params_key(&params, &self.cfg);
        let now = Instant::now();
        if self.last_key != Some(key) {
            self.last_key = Some(key);
            self.last_change = now;
            self.ui.dirty = true;
        }
        let op_changed = self.ui.operation != self.last_operation;
        self.last_operation = self.ui.operation;

        let already_requested = self.compute.last_requested() == Some(key);
        let first = self.result.is_none() && !already_requested;
        let since_change = now.duration_since(self.last_change);
        let settled = self.ui.dirty && since_change >= COMPUTE_DEBOUNCE;
        if first || op_changed || settled {
            if !already_requested {
                log::debug!(
                    "Pedido de cálculo {key:#018x} ({})",
                    self.ui.operation.label()
                );
                self.compute.request(params, self.cfg.clone(), key);
            }
            self.ui.dirty = false;
        } else if self.ui.dirty {
            ctx.request_repaint_after(COMPUTE_DEBOUNCE.saturating_sub(since_change));
        }
    }

    /// Recolhe o resultado mais novo do trabalhador; invariantes violados
    /// viram um aviso, uma vez por chave.
    fn receive_result(&mut self) {
        let Some(result) = self.compute.poll() else {
            return;
        };
        log::info!(
            "Resultado {:#018x}: {} linhas, {} aviso(s) de invariantes, erro: {}",
            result.key,
            result.lines.len(),
            result.invariants.len(),
            result.gen_error.as_deref().unwrap_or("nenhum")
        );
        if !result.invariants.is_empty() && self.last_warned_key != Some(result.key) {
            self.last_warned_key = Some(result.key);
            let text = format!(
                "G-code gerado com {} aviso(s): {}",
                result.invariants.len(),
                result.invariants.join(" | ")
            );
            self.notify(NotifyKind::Aviso, text);
        }
        self.result = Some(result);
    }
}

/// Entrada de menu da barra superior.
enum Entry {
    Item(&'static str),
    /// Item desabilitado, com texto de ajuda ao passar o mouse.
    Disabled(&'static str, &'static str),
    Sep,
}

use Entry::{Disabled, Item, Sep};

/// Menus e itens, com os mesmos rótulos do app Python.
const MENUS: &[(&str, &[Entry])] = &[
    (
        "Arquivo",
        &[
            Item("Carregar Procedimento..."),
            Item("Salvar Procedimento..."),
            Sep,
            Disabled("Gerar Relatório PDF...", "em breve"),
            Sep,
            Item("Sair"),
        ],
    ),
    (
        "Configurações",
        &[
            Item("Caminho (Mach3)..."),
            Item("Instalar macropump (autoload)"),
            Sep,
            Item("Custos de Operação..."),
            Item("Tipos de Pó..."),
            Item("Fórmulas..."),
            Sep,
            Item("Perfis de Máquina..."),
        ],
    ),
    (
        "Qualidade",
        &[
            Item("Gerenciar Operadores..."),
            Item("Gerenciar EPS (WPS)..."),
            Item("Matriz de Riscos (ISO 3834)..."),
            Sep,
            Item("Manutenção Preventiva..."),
            Item("Abrir Pasta de Logs"),
        ],
    ),
    (
        "Simulação",
        &[
            Item("Vista Isométrica"),
            Item("Vista Superior"),
            Item("Vista Lateral"),
            Item("Vista Frontal"),
            Item("Vista Traseira"),
            Sep,
            Item("Simular"),
            Item("Pausar"),
            Item("Parar"),
        ],
    ),
    (
        "G-code",
        &[
            Item("Baixar G-code"),
            Item("Abrir no Mach3"),
            Sep,
            Item("Copiar tudo"),
        ],
    ),
    ("Ajuda", &[Item("Tutorial"), Item("Atalhos"), Item("Sobre")]),
];

impl TfmApp {
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("menu").resizable(false).show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                self.menus(ui);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    self.action_buttons(ui)
                });
            });
        });
    }

    fn menus(&mut self, ui: &mut egui::Ui) {
        let mut clicked: Option<(&'static str, &'static str)> = None;
        for (title, entries) in MENUS {
            ui.menu_button(*title, |ui| {
                for entry in entries.iter() {
                    match entry {
                        Item(label) => {
                            if ui.button(*label).clicked() {
                                clicked = Some((title, label));
                            }
                        }
                        Disabled(label, hint) => {
                            ui.add_enabled(false, egui::Button::new(*label))
                                .on_disabled_hover_text(*hint);
                        }
                        Sep => {
                            ui.separator();
                        }
                    }
                }
            });
        }
        if let Some((menu, item)) = clicked {
            self.on_menu(ui.ctx(), menu, item);
        }
    }

    /// Despacho dos itens de menu. Arquivo e G-code chamam as ações de
    /// [`integration`] (procedimentos, .tap, Mach3, copiar); Configurações,
    /// Qualidade e Ajuda abrem os diálogos (ou agem na hora: instalar
    /// macropump, abrir a pasta de logs); Simulação age sobre a aba 3D;
    /// "Sair" fecha.
    fn on_menu(&mut self, ctx: &egui::Context, menu: &str, item: &str) {
        match (menu, item) {
            ("Arquivo", "Carregar Procedimento...") => integration::load_procedure_dialog(self),
            ("Arquivo", "Salvar Procedimento...") => integration::save_procedure_dialog(self),
            ("Arquivo", "Sair") => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            ("G-code", "Baixar G-code") => integration::generate_tap(self),
            ("G-code", "Abrir no Mach3") => integration::open_in_mach3(self),
            ("G-code", "Copiar tudo") => panels::right::copy_gcode(self, ctx),
            ("Configurações", "Caminho (Mach3)...") => self.dialogs.open(DialogId::Mach3Path),
            ("Configurações", "Instalar macropump (autoload)") => {
                dialogs::mach3_path::install_and_notify(self)
            }
            ("Configurações", "Custos de Operação...") => self.dialogs.open(DialogId::Costs),
            ("Configurações", "Tipos de Pó...") => self.dialogs.open(DialogId::Powders),
            ("Configurações", "Fórmulas...") => self.dialogs.open(DialogId::Formulas),
            ("Configurações", "Perfis de Máquina...") => {
                self.dialogs.open(DialogId::MachineProfiles)
            }
            ("Qualidade", "Gerenciar Operadores...") => self.dialogs.open(DialogId::Operators),
            ("Qualidade", "Gerenciar EPS (WPS)...") => self.dialogs.open(DialogId::Wps),
            ("Qualidade", "Matriz de Riscos (ISO 3834)...") => {
                self.dialogs.open(DialogId::RiskMatrix)
            }
            ("Qualidade", "Manutenção Preventiva...") => {
                self.dialogs.open(DialogId::Maintenance)
            }
            ("Qualidade", "Abrir Pasta de Logs") => self.open_logs_dir(),
            ("Ajuda", "Tutorial") => self.dialogs.open(DialogId::Tutorial),
            ("Ajuda", "Atalhos") => self.dialogs.open(DialogId::Shortcuts),
            ("Ajuda", "Sobre") => self.dialogs.open(DialogId::About),
            ("Simulação", item) => {
                if !integration::simulation_menu(ctx, item) {
                    log::debug!("Menu Simulação > {item}: item desconhecido");
                }
            }
            _ => log::debug!("Menu {menu} > {item}: ação ainda não implementada"),
        }
    }

    /// "Abrir Pasta de Logs": garante `data/logs` na raiz do app e abre no
    /// gerenciador de arquivos.
    fn open_logs_dir(&mut self) {
        let dir = prefs::app_root().join("data").join("logs");
        if let Err(err) = std::fs::create_dir_all(&dir) {
            self.notify(
                NotifyKind::Erro,
                format!("Não foi possível criar {}: {err}", dir.display()),
            );
            return;
        }
        match open::that(&dir) {
            Ok(()) => self.notify(
                NotifyKind::Info,
                format!("Pasta de logs: {}", dir.display()),
            ),
            Err(err) => self.notify(
                NotifyKind::Erro,
                format!("Não foi possível abrir {}: {err}", dir.display()),
            ),
        }
    }

    /// Botões de ação à direita da barra. Em `right_to_left`, o primeiro
    /// adicionado fica mais à direita.
    fn action_buttons(&mut self, ui: &mut egui::Ui) {
        let mut right_pinned = self.cfg.ui.right_pinned;
        let hint = if right_pinned {
            "Desafixar painel do G-code"
        } else {
            "Fixar painel do G-code"
        };
        if ui
            .toggle_value(&mut right_pinned, "📌")
            .on_hover_text(hint)
            .changed()
        {
            self.set_right_pinned(right_pinned);
        }
        let mut left_pinned = self.cfg.ui.left_pinned;
        let hint = if left_pinned {
            "Desafixar painel de parâmetros"
        } else {
            "Fixar painel de parâmetros"
        };
        if ui
            .toggle_value(&mut left_pinned, "📌")
            .on_hover_text(hint)
            .changed()
        {
            self.set_left_pinned(left_pinned);
        }
        ui.separator();
        let theme_label = format!("Tema: {}", self.theme.label());
        if ui
            .button(theme_label)
            .on_hover_text("Alternar tema: Automático -> Claro -> Escuro")
            .clicked()
        {
            self.cycle_theme(ui.ctx());
        }
        ui.separator();
        if ui
            .button("Carregar")
            .on_hover_text("Carregar procedimento salvo")
            .clicked()
        {
            self.on_menu(ui.ctx(), "Arquivo", "Carregar Procedimento...");
        }
        if ui
            .button("Salvar")
            .on_hover_text("Salvar procedimento atual")
            .clicked()
        {
            self.on_menu(ui.ctx(), "Arquivo", "Salvar Procedimento...");
        }
        if ui
            .button("Abrir no Mach3")
            .on_hover_text("Salvar .tap e abrir no Mach3")
            .clicked()
        {
            self.on_menu(ui.ctx(), "G-code", "Abrir no Mach3");
        }
        let generate =
            egui::Button::new(RichText::new("Gerar G-code").strong().color(Color32::WHITE))
                .fill(self.palette.accent);
        if ui
            .add(generate)
            .on_hover_text("Gerar o G-code com os parâmetros atuais e salvar o .tap")
            .clicked()
        {
            integration::generate_tap(self);
        }
        if self.compute.busy() {
            ui.spinner();
        }
    }
}

impl TfmApp {
    /// Painel esquerdo (parâmetros): 300 a 480 px quando visível; recolhido
    /// vira uma faixa fina na borda com um botão para reabrir.
    fn left_panel(&mut self, ui: &mut egui::Ui) {
        if self.left_open {
            let pinned = self.cfg.ui.left_pinned;
            egui::Panel::left("params")
                .resizable(true)
                .default_size(self.left_w)
                .size_range(prefs::LEFT_RANGE)
                .show(ui, |ui| {
                    claim_width(ui);
                    if !pinned && collapse_button(ui, "◀", "Recolher painel de parâmetros") {
                        self.left_open = false;
                    }
                    panels::left::show_left(self, ui);
                });
            if let Some(w) = panel_width(ui, "params") {
                if w != self.left_w {
                    log::debug!("Largura do painel de parâmetros: {} -> {w}", self.left_w);
                }
                self.left_w = prefs::clamp(w, &prefs::LEFT_RANGE);
            }
        } else if edge_panel(
            ui,
            egui::Panel::left("params_edge"),
            "▶",
            "Mostrar painel de parâmetros",
        ) {
            self.left_open = true;
        }
    }

    /// Painel direito (G-code): 300 a 520 px quando visível.
    fn right_panel(&mut self, ui: &mut egui::Ui) {
        if self.right_open {
            let pinned = self.cfg.ui.right_pinned;
            egui::Panel::right("gcode")
                .resizable(true)
                .default_size(self.right_w)
                .size_range(prefs::RIGHT_RANGE)
                .show(ui, |ui| {
                    claim_width(ui);
                    if !pinned && collapse_button(ui, "▶", "Recolher painel do G-code") {
                        self.right_open = false;
                    }
                    panels::right::show_right(self, ui);
                });
            if let Some(w) = panel_width(ui, "gcode") {
                if w != self.right_w {
                    log::debug!("Largura do painel do G-code: {} -> {w}", self.right_w);
                }
                self.right_w = prefs::clamp(w, &prefs::RIGHT_RANGE);
            }
        } else if edge_panel(
            ui,
            egui::Panel::right("gcode_edge"),
            "◀",
            "Mostrar painel do G-code",
        ) {
            self.right_open = true;
        }
    }
}

/// No egui 0.37 o tamanho que o painel guarda é o do seu conteúdo (limitado
/// à faixa), não o `default_size`: o conteúdo precisa reivindicar a largura
/// disponível, senão o painel encolhe para o mínimo e o arrasto não "pega".
fn claim_width(ui: &mut egui::Ui) {
    ui.set_min_width(ui.available_width());
}

/// Botão pequeno, alinhado à direita, para recolher um painel desafixado.
fn collapse_button(ui: &mut egui::Ui, icon: &str, hint: &str) -> bool {
    // `horizontal` limita a altura a uma linha; sem ele o `with_layout`
    // tomaria o painel inteiro e empurraria o conteúdo para fora.
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.small_button(icon).on_hover_text(hint).clicked()
        })
        .inner
    })
    .inner
}

/// Largura atual de um painel lateral, lida do estado que o egui guarda
/// (`None` antes do primeiro quadro).
fn panel_width(ui: &egui::Ui, id_salt: &str) -> Option<f32> {
    egui::PanelState::load(ui.ctx(), ui.scope_id().with(id_salt))
        .map(|state| state.outer_rect.width())
}

/// Faixa fina na borda com um botão alto para reabrir um painel recolhido.
fn edge_panel(ui: &mut egui::Ui, panel: egui::Panel, icon: &str, hint: &str) -> bool {
    let frame = egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(2, 4));
    panel
        .resizable(false)
        .exact_size(EDGE_W)
        .frame(frame)
        .show(ui, |ui| {
            let size = egui::vec2(ui.available_width(), ui.available_height().max(40.0));
            let button = egui::Button::new(RichText::new(icon).size(11.0)).frame(false);
            ui.add_sized(size, button).on_hover_text(hint).clicked()
        })
        .inner
}

impl TfmApp {
    /// Painel inferior "Resultados e Custos": recolhido mostra só o título;
    /// expandido é redimensionável e mostra o conteúdo.
    fn bottom_panel(&mut self, ui: &mut egui::Ui) {
        if self.cfg.ui.bottom_collapsed {
            egui::Panel::bottom("results_header")
                .resizable(false)
                .show(ui, |ui| self.bottom_header(ui));
        } else {
            egui::Panel::bottom("results")
                .resizable(true)
                .default_size(220.0)
                .size_range(140.0..=480.0)
                .show(ui, |ui| {
                    // Idem para a altura do painel inferior expandido.
                    ui.set_min_height(ui.available_height());
                    self.bottom_header(ui);
                    ui.separator();
                    panels::bottom::show_bottom(self, ui);
                });
        }
    }

    /// Linha de título do painel inferior: nome, último aviso e botão ▲/▼.
    fn bottom_header(&mut self, ui: &mut egui::Ui) {
        let collapsed = self.cfg.ui.bottom_collapsed;
        ui.horizontal(|ui| {
            ui.strong("Resultados e Custos");
            if let Some(last) = self.notifications.last() {
                ui.separator();
                let color = last.kind.color(&self.palette);
                let text = format!("{}  [{}] {}", last.time, last.kind.label(), last.text);
                let max_w = (ui.available_width() - 40.0).max(0.0);
                ui.scope(|ui| {
                    ui.set_max_width(max_w);
                    ui.add(egui::Label::new(RichText::new(text).color(color)).truncate());
                });
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (icon, hint) = if collapsed {
                    ("▲", "Expandir resultados")
                } else {
                    ("▼", "Recolher resultados")
                };
                if ui.small_button(icon).on_hover_text(hint).clicked() {
                    self.cfg.ui.bottom_collapsed = !collapsed;
                }
            });
        });
    }

    /// Centro: faixa de abas e o conteúdo da aba ativa.
    fn center(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                for tab in CenterTab::ALL {
                    if ui
                        .selectable_label(self.center_tab == tab, tab.label())
                        .clicked()
                    {
                        self.center_tab = tab;
                    }
                }
            });
            ui.separator();
            match self.center_tab {
                CenterTab::View3d => view3d::show_view3d(self, ui),
                _ => panels::center::show_center(self, ui),
            }
        });
    }
}

impl eframe::App for TfmApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.refresh_palette(&ctx);
        // Atalhos antes dos painéis, para consumir as teclas antes dos widgets.
        integration::handle_shortcuts(self, &ctx);
        self.receive_result();
        self.drive_compute(&ctx);

        self.top_bar(ui);
        self.left_panel(ui);
        self.right_panel(ui);
        self.bottom_panel(ui);
        self.center(ui);
        dialogs::show_dialogs(self, &ctx);
        integration::show_modals(self, &ctx);

        self.track_prefs(&ctx);
        if ctx.input(|i| i.viewport().close_requested()) {
            self.persist_prefs();
        }
    }

    /// Chamado pelo eframe no encerramento (e periodicamente quando há
    /// armazenamento): grava larguras, tema e pinos no `config.json`.
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.persist_prefs();
    }

    fn on_exit(&mut self) {
        self.persist_prefs();
    }
}
