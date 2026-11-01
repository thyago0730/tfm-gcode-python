//! "Perfis de Máquina": combo do perfil, quatro limites físicos, "+ Novo",
//! "Salvar" e "Excluir". Tudo é editado numa cópia dos perfis e só vai para o
//! config (e para o disco) em "Salvar", que também torna o perfil
//! selecionado o ativo (`active_machine_profile`), como o Python.

use egui::RichText;
use indexmap::IndexMap;
use tfm_core::config::{AppConfig, MachineLimits, MachineProfile};

use crate::app::{NotifyKind, TfmApp};
use crate::dialogs::{self, fmt_num, DialogId};
use crate::widgets::{num_field, parse_num, text_field};

/// Chave do perfil padrão, que não pode ser excluído.
pub const DEFAULT_KEY: &str = "default";

/// Cópia de trabalho dos perfis (mesma ordem do config).
pub type ProfileMap = IndexMap<String, MachineProfile>;

#[derive(Clone, Debug, Default)]
pub struct ProfilesState {
    pub loaded: bool,
    /// Cópia de trabalho de `cfg.machine_profiles`.
    pub profiles: ProfileMap,
    pub selected: String,
    pub rpm: String,
    pub length_x: String,
    pub diameter: String,
    pub feed: String,
    pub creating: bool,
    pub new_name: String,
    pub confirm_delete: bool,
}

impl ProfilesState {
    pub fn load(&mut self, cfg: &AppConfig) {
        self.profiles = cfg.machine_profiles.clone();
        if self.profiles.is_empty() {
            self.profiles
                .insert(DEFAULT_KEY.to_string(), MachineProfile::default());
        }
        self.selected = if self.profiles.contains_key(&cfg.active_machine_profile) {
            cfg.active_machine_profile.clone()
        } else {
            self.first_key()
        };
        self.load_limits();
        self.loaded = true;
    }

    fn first_key(&self) -> String {
        self.profiles.keys().next().cloned().unwrap_or_default()
    }

    /// Copia os limites do perfil selecionado para os campos.
    pub fn load_limits(&mut self) {
        let limits = self
            .profiles
            .get(&self.selected)
            .map(|p| p.limits)
            .unwrap_or_default();
        self.rpm = fmt_num(limits.max_rpm_chuck);
        self.length_x = fmt_num(limits.max_length_x);
        self.diameter = fmt_num(limits.max_diameter);
        self.feed = fmt_num(limits.max_feed_linear);
    }

    /// Converte os campos; `Err` com o rótulo do primeiro inválido
    /// (limites precisam ser > 0).
    pub fn parse_limits(&self) -> Result<MachineLimits, &'static str> {
        let field = |s: &str, label: &'static str| {
            parse_num(s).filter(|v| *v > 0.0).ok_or(label)
        };
        Ok(MachineLimits {
            max_rpm_chuck: field(&self.rpm, "Rotação Máx (RPM)")?,
            max_length_x: field(&self.length_x, "Curso X Máx (mm)")?,
            max_diameter: field(&self.diameter, "Diâmetro Máx (mm)")?,
            max_feed_linear: field(&self.feed, "Avanço Linear Máx (mm/min)")?,
        })
    }

    /// "+ Novo": cria o perfil copiando os limites dos campos e o seleciona.
    pub fn create(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Informe o nome do novo perfil.".to_string());
        }
        if self.profiles.contains_key(name) {
            return Err("Perfil já existe!".to_string());
        }
        let limits = self
            .parse_limits()
            .map_err(|f| format!("Valor inválido em \"{f}\"."))?;
        self.profiles.insert(
            name.to_string(),
            MachineProfile {
                name: name.to_string(),
                limits,
            },
        );
        self.selected = name.to_string();
        self.load_limits();
        Ok(())
    }

    /// "Salvar": grava os campos no perfil selecionado da cópia.
    pub fn store_selected(&mut self) -> Result<(), &'static str> {
        let limits = self.parse_limits()?;
        match self.profiles.get_mut(&self.selected) {
            Some(p) => {
                p.limits = limits;
                Ok(())
            }
            None => Err("Perfil"),
        }
    }

    /// "Excluir": remove o selecionado (nunca o `default`) e volta ao
    /// `default` (ou ao primeiro). Devolve a chave removida.
    pub fn delete_selected(&mut self) -> Result<String, String> {
        if self.selected == DEFAULT_KEY {
            return Err("Não é possível excluir o perfil padrão.".to_string());
        }
        let removed = self.selected.clone();
        if self.profiles.shift_remove(&removed).is_none() {
            return Err(format!("Perfil '{removed}' não existe."));
        }
        self.selected = if self.profiles.contains_key(DEFAULT_KEY) {
            DEFAULT_KEY.to_string()
        } else {
            self.first_key()
        };
        self.load_limits();
        Ok(removed)
    }

    /// Aplica a cópia no config: perfis e perfil ativo = selecionado.
    pub fn apply(&self, cfg: &mut AppConfig) {
        cfg.machine_profiles = self.profiles.clone();
        cfg.active_machine_profile = self.selected.clone();
    }
}

/// Texto do combo: a chave e, quando difere, o nome amigável.
fn profile_label(profiles: &ProfileMap, key: &str) -> String {
    match profiles.get(key) {
        Some(p) if !p.name.is_empty() && p.name != key => format!("{key} ({})", p.name),
        _ => key.to_string(),
    }
}

pub fn show(app: &mut TfmApp, state: &mut ProfilesState, ctx: &egui::Context, open: &mut bool) {
    if !state.loaded {
        state.load(&app.cfg);
    }
    let mut close = false;
    let mut save = false;
    let mut create = false;
    let mut delete = false;
    let danger = app.palette.danger;
    dialogs::window(DialogId::MachineProfiles, open)
        .default_width(470.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Perfil Ativo:");
                let keys: Vec<String> = state.profiles.keys().cloned().collect();
                let current = profile_label(&state.profiles, &state.selected);
                let mut changed = None;
                egui::ComboBox::from_id_salt("perfil_maquina")
                    .selected_text(current)
                    .width(190.0)
                    .show_ui(ui, |ui| {
                        for key in &keys {
                            let label = profile_label(&state.profiles, key);
                            if ui.selectable_label(state.selected == *key, label).clicked() {
                                changed = Some(key.clone());
                            }
                        }
                    });
                if let Some(key) = changed {
                    state.selected = key;
                    state.load_limits();
                    state.confirm_delete = false;
                }
                if ui.button("+ Novo").clicked() {
                    state.creating = true;
                    state.confirm_delete = false;
                    state.new_name.clear();
                }
                if ui.button("Excluir").clicked() {
                    state.confirm_delete = true;
                    state.creating = false;
                }
            });
            if state.creating {
                ui.group(|ui| {
                    text_field(
                        ui,
                        "Nome do novo perfil:",
                        &mut state.new_name,
                        "Identificador do perfil; copia os limites dos campos abaixo.",
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Criar").clicked() {
                            create = true;
                        }
                        if ui.button("Cancelar").clicked() {
                            state.creating = false;
                        }
                    });
                });
            }
            if state.confirm_delete {
                ui.group(|ui| {
                    ui.label(
                        RichText::new(format!("Excluir perfil '{}'?", state.selected)).color(danger),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Sim, excluir").clicked() {
                            delete = true;
                        }
                        if ui.button("Não").clicked() {
                            state.confirm_delete = false;
                        }
                    });
                });
            }
            ui.separator();
            ui.strong("Limites e Parâmetros");
            let bad = parse_num(&state.rpm).is_none();
            num_field(
                ui,
                "Rotação Máx (RPM):",
                &mut state.rpm,
                "Rotação máxima da placa (eixo A).",
                bad,
            );
            let bad = parse_num(&state.length_x).is_none();
            num_field(
                ui,
                "Curso X Máx (mm):",
                &mut state.length_x,
                "Curso útil máximo do eixo X.",
                bad,
            );
            let bad = parse_num(&state.diameter).is_none();
            num_field(
                ui,
                "Diâmetro Máx (mm):",
                &mut state.diameter,
                "Maior diâmetro de peça que a máquina aceita.",
                bad,
            );
            let bad = parse_num(&state.feed).is_none();
            num_field(
                ui,
                "Avanço Linear Máx (mm/min):",
                &mut state.feed,
                "Avanço linear máximo (G1) dos eixos.",
                bad,
            );
            ui.add_space(4.0);
            ui.weak("Salvar grava os limites do perfil selecionado e o torna o perfil ativo.");
            dialogs::footer(ui, |ui| {
                if ui.button("Salvar").clicked() {
                    save = true;
                }
                if ui.button("Fechar").clicked() {
                    close = true;
                }
            });
        });
    if create {
        let name = state.new_name.clone();
        match state.create(&name) {
            Ok(()) => {
                state.creating = false;
                app.notify(
                    NotifyKind::Info,
                    format!(
                        "Perfil '{}' criado. Clique em Salvar para gravar.",
                        state.selected
                    ),
                );
            }
            Err(err) => app.notify(NotifyKind::Aviso, err),
        }
    }
    if delete {
        state.confirm_delete = false;
        match state.delete_selected() {
            Ok(removed) => app.notify(
                NotifyKind::Info,
                format!("Perfil '{removed}' removido. Clique em Salvar para gravar."),
            ),
            Err(err) => app.notify(NotifyKind::Aviso, err),
        }
    }
    if save {
        match state.store_selected() {
            Ok(()) => {
                state.apply(&mut app.cfg);
                if dialogs::save_config(app) {
                    app.audit.log(
                        "Cost/Config Edit",
                        &format!("Perfil de máquina '{}' salvo e ativado.", state.selected),
                        "Engenharia",
                    );
                }
            }
            Err(label) => app.notify(
                NotifyKind::Aviso,
                format!("Por favor, verifique os limites. Valor inválido em \"{label}\"."),
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

    fn state() -> ProfilesState {
        let mut st = ProfilesState::default();
        st.load(&AppConfig::default());
        st
    }

    #[test]
    fn carrega_o_perfil_ativo() {
        let st = state();
        assert_eq!(st.selected, "default");
        assert_eq!(st.rpm, "100.0");
        assert_eq!(st.feed, "5000.0");
        assert_eq!(profile_label(&st.profiles, "default"), "default (Padrão)");
    }

    #[test]
    fn novo_perfil_copia_limites_e_rejeita_duplicado() {
        let mut st = state();
        st.rpm = "120".into();
        st.create("torno2").expect("cria");
        assert_eq!(st.selected, "torno2");
        assert_eq!(st.profiles["torno2"].limits.max_rpm_chuck, 120.0);
        assert_eq!(st.create("torno2"), Err("Perfil já existe!".to_string()));
        assert!(st.create("  ").is_err());
    }

    #[test]
    fn excluir_protege_o_padrao_e_volta_para_ele() {
        let mut st = state();
        assert_eq!(
            st.delete_selected(),
            Err("Não é possível excluir o perfil padrão.".to_string())
        );
        st.create("x").expect("cria");
        assert_eq!(st.delete_selected(), Ok("x".to_string()));
        assert_eq!(st.selected, "default");
        assert!(!st.profiles.contains_key("x"));
    }

    #[test]
    fn salvar_valida_e_aplica_no_config() {
        let mut st = state();
        st.diameter = "0".into();
        assert_eq!(st.parse_limits(), Err("Diâmetro Máx (mm)"));
        st.diameter = "650,5".into();
        st.store_selected().expect("grava");
        let mut cfg = AppConfig::default();
        st.apply(&mut cfg);
        assert_eq!(cfg.active_limits().max_diameter, 650.5);
        assert_eq!(cfg.active_machine_profile, "default");
    }
}
