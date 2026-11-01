//! "Custos de Operação": os quatro custos unitários e o símbolo da moeda
//! (seção `costs` do config). Nada é gravado até "Salvar".

use tfm_core::config::AppConfig;

use crate::app::{NotifyKind, TfmApp};
use crate::dialogs::{self, fmt_num, DialogId};
use crate::widgets::{num_field, parse_num, text_field};

/// Campos do diálogo, em texto (o usuário pode digitar vírgula).
#[derive(Clone, Debug, Default)]
pub struct CostsState {
    pub loaded: bool,
    pub powder: String,
    pub gas: String,
    pub labor: String,
    pub machine: String,
    pub symbol: String,
}

/// Valores já convertidos.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CostValues {
    pub powder: f64,
    pub gas: f64,
    pub labor: f64,
    pub machine: f64,
}

impl CostsState {
    pub const EMPTY: Self = CostsState {
        loaded: false,
        powder: String::new(),
        gas: String::new(),
        labor: String::new(),
        machine: String::new(),
        symbol: String::new(),
    };

    pub fn load(&mut self, cfg: &AppConfig) {
        let c = &cfg.costs;
        self.powder = fmt_num(c.powder_brl_kg);
        self.gas = fmt_num(c.gas_argon_brl_m3);
        self.labor = fmt_num(c.labor_brl_hour);
        self.machine = fmt_num(c.machine_brl_hour);
        self.symbol = c.currency_symbol.clone();
        self.loaded = true;
    }

    /// Símbolo da moeda digitado (vazio => `R$`).
    pub fn symbol(&self) -> String {
        let s = self.symbol.trim();
        if s.is_empty() {
            "R$".to_string()
        } else {
            s.to_string()
        }
    }

    /// Converte os campos; `Err` com o rótulo do primeiro campo inválido.
    /// Custos negativos também são rejeitados.
    pub fn parse(&self) -> Result<CostValues, &'static str> {
        let field = |s: &str, label: &'static str| {
            parse_num(s).filter(|v| *v >= 0.0).ok_or(label)
        };
        Ok(CostValues {
            powder: field(&self.powder, "Pó")?,
            gas: field(&self.gas, "Gás argônio")?,
            labor: field(&self.labor, "Mão de obra")?,
            machine: field(&self.machine, "Máquina")?,
        })
    }
}

/// Aplica os valores em `cfg.costs` e descreve o que mudou (para o audit
/// log), no formato `chave: antes -> depois`.
pub fn apply(cfg: &mut AppConfig, values: CostValues, symbol: &str) -> Vec<String> {
    let mut changes = Vec::new();
    let costs = &mut cfg.costs;
    let mut set = |name: &str, slot: &mut f64, new: f64| {
        if *slot != new {
            changes.push(format!("{name}: {} -> {}", fmt_num(*slot), fmt_num(new)));
            *slot = new;
        }
    };
    set("powder_brl_kg", &mut costs.powder_brl_kg, values.powder);
    set("gas_argon_brl_m3", &mut costs.gas_argon_brl_m3, values.gas);
    set("labor_brl_hour", &mut costs.labor_brl_hour, values.labor);
    set("machine_brl_hour", &mut costs.machine_brl_hour, values.machine);
    if costs.currency_symbol != symbol {
        changes.push(format!(
            "currency_symbol: {} -> {symbol}",
            costs.currency_symbol
        ));
        costs.currency_symbol = symbol.to_string();
    }
    changes
}

pub fn show(app: &mut TfmApp, state: &mut CostsState, ctx: &egui::Context, open: &mut bool) {
    if !state.loaded {
        state.load(&app.cfg);
    }
    let mut close = false;
    let mut save = false;
    dialogs::window(DialogId::Costs, open)
        .default_width(400.0)
        .show(ctx, |ui| {
            let sym = state.symbol();
            ui.label("Custos unitários usados nas estimativas (fórmulas de custo).");
            ui.add_space(4.0);
            let bad = parse_num(&state.powder).is_none();
            num_field(
                ui,
                &format!("Pó ({sym}/kg):"),
                &mut state.powder,
                "Custo padrão do pó por quilograma (o pó ativo em Tipos de Pó pode sobrescrever).",
                bad,
            );
            let bad = parse_num(&state.gas).is_none();
            num_field(
                ui,
                &format!("Gás argônio ({sym}/m³):"),
                &mut state.gas,
                "Custo do argônio por metro cúbico.",
                bad,
            );
            let bad = parse_num(&state.labor).is_none();
            num_field(
                ui,
                &format!("Mão de obra ({sym}/hora):"),
                &mut state.labor,
                "Custo da mão de obra por hora de processo.",
                bad,
            );
            let bad = parse_num(&state.machine).is_none();
            num_field(
                ui,
                &format!("Máquina ({sym}/hora):"),
                &mut state.machine,
                "Custo da hora-máquina (energia, depreciação, consumíveis).",
                bad,
            );
            text_field(
                ui,
                "Símbolo da moeda:",
                &mut state.symbol,
                "Símbolo mostrado junto aos custos (ex.: R$).",
            );
            ui.add_space(4.0);
            ui.weak("As alterações só são gravadas ao clicar em Salvar.");
            dialogs::footer(ui, |ui| {
                if ui.button("Salvar").clicked() {
                    save = true;
                }
                if ui.button("Cancelar").clicked() {
                    close = true;
                }
            });
        });
    if save {
        match state.parse() {
            Ok(values) => {
                let symbol = state.symbol();
                let changes = apply(&mut app.cfg, values, &symbol);
                if dialogs::save_config(app) {
                    if !changes.is_empty() {
                        app.audit.log(
                            "Cost/Config Edit",
                            &format!("Alterações em costs: {}", changes.join(", ")),
                            "Engenharia",
                        );
                    }
                    close = true;
                }
            }
            Err(label) => app.notify(
                NotifyKind::Aviso,
                format!("Por favor, verifique os parâmetros. Valor numérico inválido em \"{label}\"."),
            ),
        }
    }
    if close {
        *open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carrega_e_converte() {
        let mut cfg = AppConfig::default();
        cfg.costs.powder_brl_kg = 700.0;
        let mut st = CostsState::default();
        st.load(&cfg);
        assert!(st.loaded);
        assert_eq!(st.powder, "700.0");
        assert_eq!(st.symbol(), "R$");
        st.gas = "12,5".into();
        let v = st.parse().expect("válido");
        assert_eq!(v.gas, 12.5);
        assert_eq!(v.powder, 700.0);
    }

    #[test]
    fn rejeita_invalido_e_negativo() {
        let mut st = CostsState::default();
        st.load(&AppConfig::default());
        st.labor = "abc".into();
        assert_eq!(st.parse(), Err("Mão de obra"));
        st.labor = "-1".into();
        assert_eq!(st.parse(), Err("Mão de obra"));
        st.symbol = "   ".into();
        assert_eq!(st.symbol(), "R$");
    }

    #[test]
    fn apply_descreve_mudancas() {
        let mut cfg = AppConfig::default();
        let values = CostValues {
            powder: 250.0,
            gas: 50.0,
            labor: 45.0,
            machine: 30.0,
        };
        let changes = apply(&mut cfg, values, "US$");
        assert_eq!(cfg.costs.labor_brl_hour, 45.0);
        assert_eq!(cfg.costs.currency_symbol, "US$");
        assert_eq!(
            changes,
            vec![
                "labor_brl_hour: 40.0 -> 45.0".to_string(),
                "currency_symbol: R$ -> US$".to_string()
            ]
        );
        assert!(apply(&mut cfg, values, "US$").is_empty());
    }
}
