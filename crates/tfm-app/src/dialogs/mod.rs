//! Diálogos (janelas flutuantes) de configurações, qualidade ISO 3834,
//! manutenção e ajuda.
//!
//! Este arquivo guarda o enum [`DialogId`], o estado [`Dialogs`] (quais
//! diálogos estão abertos e o estado de edição de cada um) e o despachante
//! [`show_dialogs`], chamado pelo app a cada quadro. Cada diálogo vive no seu
//! próprio arquivo e expõe `show(app, state, ctx, open)`.
//!
//! Regras comuns:
//! * toda janela é `egui::Window::new(título).collapsible(false)
//!   .resizable(true).open(&mut flag)` e fecha pelo ✕ ou por "Fechar";
//! * o estado de edição é descartado ao fechar, então reabrir recarrega do
//!   config/qualidade e nada "vaza" entre sessões do diálogo;
//! * Custos, Pós, Fórmulas, Perfis e Mach3 só gravam ao clicar em "Salvar"
//!   (via [`save_config`]); os cadastros de qualidade gravam a cada ação,
//!   como no Python.
//!
//! Empréstimos: o estado é retirado de `app.dialogs` com `std::mem::take`
//! durante o desenho, para que cada diálogo use `&mut app.cfg`,
//! `&mut app.quality` e `app.notify(..)` sem conflito.

use std::collections::HashSet;

use tfm_core::params::Params;

use crate::app::{NotifyKind, TfmApp};
use crate::prefs;

pub mod about;
pub mod costs;
pub mod formulas;
pub mod mach3_path;
pub mod machine_profiles;
pub mod maintenance;
pub mod operators;
pub mod powders;
pub mod risk_matrix;
pub mod shortcuts;
pub mod tutorial;
pub mod wps;

/// Identifica cada diálogo do app.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DialogId {
    Costs,
    Powders,
    Formulas,
    MachineProfiles,
    Mach3Path,
    Operators,
    Wps,
    RiskMatrix,
    Maintenance,
    About,
    Tutorial,
    Shortcuts,
}

impl DialogId {
    pub const ALL: [DialogId; 12] = [
        DialogId::Costs,
        DialogId::Powders,
        DialogId::Formulas,
        DialogId::MachineProfiles,
        DialogId::Mach3Path,
        DialogId::Operators,
        DialogId::Wps,
        DialogId::RiskMatrix,
        DialogId::Maintenance,
        DialogId::About,
        DialogId::Tutorial,
        DialogId::Shortcuts,
    ];

    /// Título da janela (os mesmos do app Python).
    pub fn title(self) -> &'static str {
        match self {
            DialogId::Costs => "Custos de Operação",
            DialogId::Powders => "Tipos de Pó",
            DialogId::Formulas => "Fórmulas",
            DialogId::MachineProfiles => "Perfis de Máquina",
            DialogId::Mach3Path => "Caminho (Mach3)",
            DialogId::Operators => "Gerenciar Operadores",
            DialogId::Wps => "Gerenciar EPS (WPS)",
            DialogId::RiskMatrix => "Matriz de Riscos e Controles - ISO 3834 / ISO 9001",
            DialogId::Maintenance => "Manutenção Preventiva",
            DialogId::About => "Sobre",
            DialogId::Tutorial => "Ajuda",
            DialogId::Shortcuts => "Atalhos",
        }
    }
}

/// Diálogos abertos e o estado de edição de cada um.
#[derive(Default)]
pub struct Dialogs {
    pub open: HashSet<DialogId>,
    pub costs: costs::CostsState,
    pub powders: powders::PowdersState,
    pub formulas: formulas::FormulasState,
    pub machine_profiles: machine_profiles::ProfilesState,
    pub mach3: mach3_path::Mach3State,
    pub operators: operators::OperatorsState,
    pub wps: wps::WpsState,
    pub maintenance: maintenance::MaintenanceState,
    pub tutorial: tutorial::TutorialState,
}

impl Dialogs {
    /// Abre (ou traz de volta) um diálogo.
    pub fn open(&mut self, id: DialogId) {
        self.open.insert(id);
    }

    pub fn is_open(&self, id: DialogId) -> bool {
        self.open.contains(&id)
    }

    /// Descarta o estado de edição de um diálogo (chamado ao fechar).
    fn reset(&mut self, id: DialogId) {
        match id {
            DialogId::Costs => self.costs = Default::default(),
            DialogId::Powders => self.powders = Default::default(),
            DialogId::Formulas => self.formulas = Default::default(),
            DialogId::MachineProfiles => self.machine_profiles = Default::default(),
            DialogId::Mach3Path => self.mach3 = Default::default(),
            DialogId::Operators => self.operators = Default::default(),
            DialogId::Wps => self.wps = Default::default(),
            DialogId::Maintenance => self.maintenance = Default::default(),
            DialogId::Tutorial => self.tutorial = Default::default(),
            DialogId::RiskMatrix | DialogId::About | DialogId::Shortcuts => {}
        }
    }
}

/// Desenha todos os diálogos abertos; os que foram fechados neste quadro
/// saem de `open` e têm o estado descartado.
pub fn show_dialogs(app: &mut TfmApp, ctx: &egui::Context) {
    let mut dialogs = std::mem::take(&mut app.dialogs);
    for id in DialogId::ALL {
        if !dialogs.open.contains(&id) {
            continue;
        }
        let mut open = true;
        match id {
            DialogId::Costs => costs::show(app, &mut dialogs.costs, ctx, &mut open),
            DialogId::Powders => powders::show(app, &mut dialogs.powders, ctx, &mut open),
            DialogId::Formulas => formulas::show(app, &mut dialogs.formulas, ctx, &mut open),
            DialogId::MachineProfiles => {
                machine_profiles::show(app, &mut dialogs.machine_profiles, ctx, &mut open)
            }
            DialogId::Mach3Path => mach3_path::show(app, &mut dialogs.mach3, ctx, &mut open),
            DialogId::Operators => operators::show(app, &mut dialogs.operators, ctx, &mut open),
            DialogId::Wps => wps::show(app, &mut dialogs.wps, ctx, &mut open),
            DialogId::RiskMatrix => risk_matrix::show(app, ctx, &mut open),
            DialogId::Maintenance => {
                maintenance::show(app, &mut dialogs.maintenance, ctx, &mut open)
            }
            DialogId::About => about::show(app, ctx, &mut open),
            DialogId::Tutorial => tutorial::show(app, &mut dialogs.tutorial, ctx, &mut open),
            DialogId::Shortcuts => shortcuts::show(app, ctx, &mut open),
        }
        if !open {
            dialogs.open.remove(&id);
            dialogs.reset(id);
        }
    }
    // Um diálogo pode ter pedido para abrir outro (ex.: Ajuda -> Atalhos).
    dialogs.open.extend(app.dialogs.open.drain());
    app.dialogs = dialogs;
}

/// Janela padrão dos diálogos: título fixo, sem recolher, redimensionável e
/// com o ✕ ligado a `open`.
pub(crate) fn window(id: DialogId, open: &mut bool) -> egui::Window<'_> {
    egui::Window::new(id.title())
        .collapsible(false)
        .resizable(true)
        .open(open)
}

/// Grava o `config.json` inteiro e avisa o resultado. Devolve `true` se
/// conseguiu.
pub(crate) fn save_config(app: &mut TfmApp) -> bool {
    match prefs::save_config(&app.cfg, &app.cfg_path) {
        Ok(()) => {
            app.notify(NotifyKind::Sucesso, "Configurações salvas.");
            true
        }
        Err(err) => {
            app.notify(
                NotifyKind::Erro,
                format!("Erro ao salvar configurações: {err}"),
            );
            false
        }
    }
}

/// Parâmetros do último cálculo (ou os padrões, antes do primeiro).
pub(crate) fn current_params(app: &TfmApp) -> Params {
    app.result
        .as_ref()
        .map(|r| r.params.clone())
        .unwrap_or_default()
}

/// Número para um campo de texto: inteiros saem com `.0` (como o `str(float)`
/// do Python mostrava nos diálogos), os demais com a precisão natural.
pub(crate) fn fmt_num(x: f64) -> String {
    if x.is_finite() && x == x.trunc() && x.abs() < 1e15 {
        format!("{x:.1}")
    } else {
        format!("{x}")
    }
}

/// Rodapé com botões alinhados à direita. Em `right_to_left`, o primeiro
/// botão adicionado fica mais à direita.
pub(crate) fn footer(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.separator();
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titulos_exatos() {
        assert_eq!(DialogId::Costs.title(), "Custos de Operação");
        assert_eq!(DialogId::Powders.title(), "Tipos de Pó");
        assert_eq!(DialogId::Formulas.title(), "Fórmulas");
        assert_eq!(DialogId::MachineProfiles.title(), "Perfis de Máquina");
        assert_eq!(DialogId::Mach3Path.title(), "Caminho (Mach3)");
        assert_eq!(DialogId::Operators.title(), "Gerenciar Operadores");
        assert_eq!(DialogId::Wps.title(), "Gerenciar EPS (WPS)");
        assert_eq!(
            DialogId::RiskMatrix.title(),
            "Matriz de Riscos e Controles - ISO 3834 / ISO 9001"
        );
        assert_eq!(DialogId::Maintenance.title(), "Manutenção Preventiva");
        assert_eq!(DialogId::About.title(), "Sobre");
        assert_eq!(DialogId::Tutorial.title(), "Ajuda");
        assert_eq!(DialogId::Shortcuts.title(), "Atalhos");
        assert_eq!(DialogId::ALL.len(), 12);
    }

    #[test]
    fn abrir_fechar_e_resetar() {
        let mut d = Dialogs::default();
        assert!(!d.is_open(DialogId::Costs));
        d.open(DialogId::Costs);
        assert!(d.is_open(DialogId::Costs));
        d.costs.powder = "1".into();
        d.costs.loaded = true;
        d.reset(DialogId::Costs);
        assert!(!d.costs.loaded);
        assert!(d.costs.powder.is_empty());
    }

    #[test]
    fn fmt_num_como_o_python() {
        assert_eq!(fmt_num(250.0), "250.0");
        assert_eq!(fmt_num(0.16), "0.16");
        assert_eq!(fmt_num(-3.0), "-3.0");
        assert_eq!(fmt_num(1e-6), "0.000001");
    }
}
