//! Estado editável do painel esquerdo: textos digitados, flags, enums,
//! operação escolhida e presets por operação.
//!
//! Porta do dicionário `self.params` (StringVar/BooleanVar) do app Tkinter:
//!
//! * [`UiState::fields`] guarda **o que o usuário digitou**, por chave de
//!   [`Params`] (nomes do JSON Python); [`UiState::flags`] as caixas de
//!   seleção; os enums ficam em campos próprios;
//! * [`UiState::from_params`] formata os números como a UI Python os mostra
//!   (`str(float)`: `50.0`, `0.8`; calibração com duas casas: `1.00`;
//!   inteiros sem ponto);
//! * [`UiState::to_params`] porta `_get_current_params`: pó -> fator/custo,
//!   leads só com "Usar início e fim extra", operação interna só onde se
//!   aplica, `taxa_de_deposicao = velocidade`, multiperfil ajusta camadas;
//! * [`UiState::set_operation`] porta `_set_revestimento_mode` (memória de
//!   preset por operação) e [`UiState::reset_defaults`] o botão "Padrão".
//!
//! Texto que não converte **não** derruba a conversão: o campo fica com o
//! valor de [`Params::default`] e entra em [`UiState::invalid`] para ser
//! pintado de vermelho (junto com as regras de `validate_physical`).

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use indexmap::IndexMap;
use tfm_core::config::AppConfig;
use tfm_core::gcode::multiperfil::profile_definition;
use tfm_core::params::{
    BolachaOscEixo, BolachaOscForma, BolachaOscSentido, BolachaTipo, BolachaZero, Direction,
    FlatOrientation, MultiperfilStrategy, Operation, OscillationType, Params, ProfileShape,
    Rotation,
};
use tfm_core::validator::{py_float, validate_physical};

use crate::widgets::parse_num;

/// Abas do painel esquerdo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeftTab {
    Parametros,
    Processo,
    Rastreabilidade,
}

impl LeftTab {
    pub const ALL: [LeftTab; 3] = [
        LeftTab::Parametros,
        LeftTab::Processo,
        LeftTab::Rastreabilidade,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LeftTab::Parametros => "Parâmetros",
            LeftTab::Processo => "Processo",
            LeftTab::Rastreabilidade => "Rastreabilidade",
        }
    }
}

/// Fator de pó quando o nome não está cadastrado (`_get_current_params`).
const FALLBACK_POWDER_FACTOR: f64 = 0.16;
/// Texto dos leads quando desligados e zerados (default do Tkinter: `"5.0"`).
const LEAD_DEFAULT_TEXT: &str = "5.0";

// ---------------------------------------------------------------------------
// Predicados de visibilidade (porta de `_is_*` / `_refresh_*`)
// ---------------------------------------------------------------------------

/// As três oscilações.
pub fn is_oscillation(op: Operation) -> bool {
    op.mode().0.is_oscillation()
}

/// Operação interna e "Afastamento da face": espiral, multiperfil e
/// oscilações (`_is_internal_operation_applicable`).
pub fn internal_applicable(op: Operation) -> bool {
    !matches!(op, Operation::ChapaPlana | Operation::Bolacha)
}

/// Bloco "Avançado" só em Espiral, Espiral Multiperfil e Bolacha
/// (`_refresh_advanced_toggle_visibility`).
pub fn has_advanced(op: Operation) -> bool {
    matches!(
        op,
        Operation::Espiral | Operation::EspiralMultiperfil | Operation::Bolacha
    )
}

// ---------------------------------------------------------------------------
// Descritores dos campos: chave de Params <-> acessores
// ---------------------------------------------------------------------------

/// Campo numérico (`f64`). `decimals = Some(n)` fixa as casas no texto
/// (calibração: `1.00`); `None` usa `str(float)` do Python.
struct NumField {
    key: &'static str,
    get: fn(&Params) -> f64,
    set: fn(&mut Params, f64),
    decimals: Option<usize>,
}

struct IntField {
    key: &'static str,
    get: fn(&Params) -> i64,
    set: fn(&mut Params, i64),
}

struct TextField {
    key: &'static str,
    get: fn(&Params) -> &str,
    set: fn(&mut Params, String),
}

struct FlagField {
    key: &'static str,
    get: fn(&Params) -> bool,
    set: fn(&mut Params, bool),
}

macro_rules! num {
    ($key:literal, $field:ident) => {
        NumField {
            key: $key,
            get: |p| p.$field,
            set: |p, v| p.$field = v,
            decimals: None,
        }
    };
    ($key:literal, $field:ident, $decimals:expr) => {
        NumField {
            key: $key,
            get: |p| p.$field,
            set: |p, v| p.$field = v,
            decimals: Some($decimals),
        }
    };
}

macro_rules! int {
    ($key:literal, $field:ident) => {
        IntField {
            key: $key,
            get: |p| p.$field,
            set: |p, v| p.$field = v,
        }
    };
}

macro_rules! text {
    ($key:literal, $field:ident) => {
        TextField {
            key: $key,
            get: |p| p.$field.as_str(),
            set: |p, v| p.$field = v,
        }
    };
}

macro_rules! flag {
    ($key:literal, $field:ident) => {
        FlagField {
            key: $key,
            get: |p| p.$field,
            set: |p, v| p.$field = v,
        }
    };
}

/// Campos numéricos, na ordem em que a UI Python os declara.
const NUM_FIELDS: &[NumField] = &[
    num!("diametro", diametro),
    num!("diametro_interno", diametro_interno),
    num!("comprimento_revestir", comprimento_revestir),
    num!("largura_revestir", largura_revestir),
    num!("largura_cordao", largura_cordao),
    num!("sobreposicao", sobreposicao),
    num!("velocidade_de_deposicao", velocidade_de_deposicao),
    num!("velocidade_oscilacao_mm_min", velocidade_oscilacao_mm_min),
    num!("afastamento_tocha", afastamento_tocha),
    num!("afastamento_seguranca", afastamento_seguranca),
    num!("torch_ignite_delay_s", torch_ignite_delay_s),
    num!("torch_extinguish_delay_s", torch_extinguish_delay_s),
    num!("overlap_angular_deg", overlap_angular_deg),
    num!("largura_cordao_min", largura_cordao_min),
    num!("largura_cordao_max", largura_cordao_max),
    num!(
        "bead_width_calibration_factor",
        bead_width_calibration_factor,
        2
    ),
    num!("lead_in", lead_in),
    num!("lead_out", lead_out),
    num!("corrente_arco", corrente_arco),
    num!("tensao_arco", tensao_arco),
    num!("vazao_gas", vazao_gas),
    num!("alim_po", alim_po),
    num!("preaquecimento", preaquecimento),
    num!("eficiencia_termica", eficiencia_termica),
    num!("tempo_setup_min", tempo_setup_min),
    num!("taxa_deposicao_g_h", taxa_deposicao_g_h),
    num!("espessura_camada", espessura_camada),
    num!("internal_face_clearance_x", internal_face_clearance_x),
    num!("multiperfil_total_angle_deg", multiperfil_total_angle_deg),
    num!("multiperfil_pitch_mm", multiperfil_pitch_mm),
    num!("multiperfil_profile_base_mm", multiperfil_profile_base_mm),
    num!("multiperfil_profile_top_mm", multiperfil_profile_top_mm),
    num!(
        "multiperfil_profile_height_mm",
        multiperfil_profile_height_mm
    ),
    num!(
        "multiperfil_end_chamfer_angle_deg",
        multiperfil_end_chamfer_angle_deg
    ),
    num!("oscilacao_comprimento", oscilacao_comprimento),
    num!("bolacha_osc_periodo_mm", bolacha_osc_periodo_mm),
    num!("bolacha_osc_fase_camada_perc", bolacha_osc_fase_camada_perc),
    num!("bolacha_osc_espera_s", bolacha_osc_espera_s),
    num!("bolacha_osc_pontos_ciclo", bolacha_osc_pontos_ciclo),
    num!("deslocamento_angular_perc", deslocamento_angular_perc),
    num!("osc_test_gran_x", osc_test_gran_x),
    num!("osc_test_gran_a", osc_test_gran_a),
    num!("root_feed_scale", root_feed_scale),
];

const INT_FIELDS: &[IntField] = &[
    int!("num_camadas", num_camadas),
    int!("multiperfil_count", multiperfil_count),
];

const TEXT_FIELDS: &[TextField] = &[
    text!("nome_procedimento", nome_procedimento),
    text!("ordem_servico", ordem_servico),
    text!("notes", notes),
    text!("traceability_operator", traceability_operator),
    text!("traceability_eps", traceability_eps),
    text!("traceability_powder_batch", traceability_powder_batch),
    text!("traceability_supplier", traceability_supplier),
    text!("traceability_certificate", traceability_certificate),
    text!("traceability_env_temp", traceability_env_temp),
    text!("traceability_env_humidity", traceability_env_humidity),
];

const FLAG_FIELDS: &[FlagField] = &[
    flag!("lead_extras_enabled", lead_extras_enabled),
    flag!("torch_retract_on_ignite", torch_retract_on_ignite),
    flag!("spiral_close_ends", spiral_close_ends),
    flag!("fechamento_uniforme", fechamento_uniforme),
    flag!("compensar_rotacao", compensar_rotacao),
    flag!("defasar_camadas", defasar_camadas),
    flag!("tempo_inverso_g93", tempo_inverso_g93),
    flag!("flat_single_direction", flat_single_direction),
    flag!("operacao_interna", operacao_interna),
    flag!("multiperfil_point_by_point", multiperfil_point_by_point),
    flag!(
        "multiperfil_end_chamfer_enabled",
        multiperfil_end_chamfer_enabled
    ),
    flag!("bolacha_stop_between", bolacha_stop_between),
    flag!("bolacha_oscilar", bolacha_oscilar),
    flag!("bolacha_osc_escada", bolacha_osc_escada),
    flag!("smart_layers", smart_layers),
];

/// Chaves guardadas no preset de cada operação (`self.mode_presets[...]`
/// do Python). Os demais campos são compartilhados entre as operações.
const PRESET_KEYS: &[&str] = &[
    "diametro",
    "comprimento_revestir",
    "largura_revestir",
    "diametro_interno",
    "oscilacao_comprimento",
    "deslocamento_angular_perc",
    "flat_orientation",
    "flat_single_direction",
    "torch_retract_on_ignite",
    "spiral_close_ends",
    "fechamento_uniforme",
    "compensar_rotacao",
    "defasar_camadas",
    "lead_extras_enabled",
    "operacao_interna",
    "internal_face_clearance_x",
    "bolacha_tipo",
    "bolacha_zero",
    "bolacha_stop_between",
    "bolacha_oscilar",
    "bolacha_osc_forma",
    "bolacha_osc_eixo",
    "bolacha_osc_sentido",
    "bolacha_osc_periodo_mm",
    "bolacha_osc_fase_camada_perc",
    "bolacha_osc_espera_s",
    "bolacha_osc_pontos_ciclo",
    "bolacha_osc_escada",
    "largura_cordao_min",
    "largura_cordao_max",
    "overlap_angular_deg",
    "bead_width_calibration_factor",
    "osc_test_gran_x",
    "osc_test_gran_a",
    "multiperfil_count",
    "multiperfil_total_angle_deg",
    "multiperfil_pitch_mm",
    "multiperfil_strategy",
    "multiperfil_point_by_point",
    "multiperfil_profile_shape",
    "multiperfil_profile_base_mm",
    "multiperfil_profile_top_mm",
    "multiperfil_profile_height_mm",
    "multiperfil_end_chamfer_enabled",
    "multiperfil_end_chamfer_angle_deg",
];

const MULTIPERFIL_KEYS: &[&str] = &[
    "multiperfil_count",
    "multiperfil_total_angle_deg",
    "multiperfil_pitch_mm",
    "multiperfil_strategy",
    "multiperfil_point_by_point",
    "multiperfil_profile_shape",
    "multiperfil_profile_base_mm",
    "multiperfil_profile_top_mm",
    "multiperfil_profile_height_mm",
    "multiperfil_end_chamfer_enabled",
    "multiperfil_end_chamfer_angle_deg",
    "fechamento_uniforme",
];

/// Chaves que a primeira entrada numa operação sem preset leva ao padrão
/// (ramo `else` de `_set_revestimento_mode`).
fn first_entry_keys(op: Operation) -> &'static [&'static str] {
    match op {
        Operation::Bolacha => &[
            "diametro_interno",
            "bolacha_tipo",
            "bolacha_stop_between",
            "bolacha_oscilar",
            "bolacha_osc_forma",
            "bolacha_osc_eixo",
            "bolacha_osc_sentido",
            "bolacha_osc_periodo_mm",
            "bolacha_osc_fase_camada_perc",
            "bolacha_osc_espera_s",
            "bolacha_osc_pontos_ciclo",
            "bolacha_osc_escada",
            "oscilacao_comprimento",
        ],
        Operation::ChapaPlana => &[
            "largura_revestir",
            "flat_orientation",
            "flat_single_direction",
        ],
        Operation::EspiralMultiperfil => MULTIPERFIL_KEYS,
        _ => &[],
    }
}

/// Chaves que [`Params::reset_mode_defaults`] toca para cada operação
/// (mais `bead_width_calibration_factor`, sempre).
fn reset_keys(op: Operation) -> &'static [&'static str] {
    match op {
        Operation::Bolacha => &[
            "diametro_interno",
            "bolacha_tipo",
            "bolacha_stop_between",
            "bolacha_oscilar",
            "bolacha_osc_forma",
            "bolacha_osc_eixo",
            "bolacha_osc_sentido",
            "bolacha_osc_periodo_mm",
            "bolacha_osc_fase_camada_perc",
            "bolacha_osc_espera_s",
            "bolacha_osc_pontos_ciclo",
            "bolacha_osc_escada",
            "oscilacao_comprimento",
            "overlap_angular_deg",
        ],
        Operation::ChapaPlana => &[
            "largura_revestir",
            "flat_orientation",
            "flat_single_direction",
        ],
        Operation::EspiralMultiperfil => MULTIPERFIL_KEYS,
        Operation::Espiral
        | Operation::OscilacaoLinear
        | Operation::OscilacaoQuadrada
        | Operation::OscilacaoQuadradaContinua => &[
            "lead_extras_enabled",
            "operacao_interna",
            "spiral_close_ends",
            "fechamento_uniforme",
            "compensar_rotacao",
            "defasar_camadas",
        ],
    }
}

/// Texto de um número como a UI mostra: casas fixas quando pedidas, senão
/// `str(float)` do Python (`50.0`, `0.8`, `3000.0`).
fn fmt_num(v: f64, decimals: Option<usize>) -> String {
    match decimals {
        Some(d) => format!("{v:.d$}"),
        None => py_float(v),
    }
}

/// Inteiro a partir do texto: aceita `"3"`, `" 4 "`, `"4.0"`; recusa fração
/// e valores fora do `i64`.
fn parse_int(text: &str) -> Option<i64> {
    let v = parse_num(text)?;
    (v.fract() == 0.0 && v.abs() < 9.0e15).then_some(v as i64)
}

// ---------------------------------------------------------------------------
// UiState
// ---------------------------------------------------------------------------

/// Estado editável da interface. Veja a documentação do módulo.
#[derive(Clone, Debug, PartialEq)]
pub struct UiState {
    /// Operação escolhida nos cartões.
    pub operation: Operation,
    pub tab: LeftTab,
    /// "Mostrar avançado" (sobreposição angular, atrasos da tocha, calibração).
    pub show_advanced: bool,
    /// Texto de cada campo, por chave de [`Params`] (nomes do JSON Python).
    pub fields: IndexMap<&'static str, String>,
    /// Caixas de seleção, por chave de [`Params`].
    pub flags: IndexMap<&'static str, bool>,
    pub direcao: Direction,
    pub rotacao: Rotation,
    pub flat_orientation: FlatOrientation,
    pub multiperfil_strategy: MultiperfilStrategy,
    pub profile_shape: ProfileShape,
    pub bolacha_tipo: BolachaTipo,
    pub bolacha_osc_forma: BolachaOscForma,
    pub bolacha_osc_eixo: BolachaOscEixo,
    pub bolacha_osc_sentido: BolachaOscSentido,
    pub bolacha_zero: BolachaZero,
    /// Pó escolhido no combo; vazio = o `active_powder` do config.
    pub powder_name: String,
    /// Último estado de cada operação já visitada (`mode_presets` do Python).
    pub presets: HashMap<Operation, Params>,
    /// Campos a pintar de vermelho (texto inválido ou regra física violada);
    /// atualizado por [`UiState::refresh_invalid`].
    pub invalid: HashSet<&'static str>,
    /// Houve edição desde o último cálculo.
    pub dirty: bool,
    /// Momento da última edição (base do debounce do cálculo).
    pub last_edit: Option<Instant>,
}

impl Default for UiState {
    /// Os defaults da UI Python ([`Params::default`]) na Espiral.
    fn default() -> Self {
        UiState::from_params(&Params::default())
    }
}

impl UiState {
    /// Monta o estado a partir de parâmetros (abrir procedimento, EPS, padrão).
    /// Os presets começam vazios e nada está sujo.
    pub fn from_params(p: &Params) -> Self {
        let mut state = UiState {
            operation: Operation::from_params(p),
            tab: LeftTab::Parametros,
            show_advanced: false,
            fields: IndexMap::new(),
            flags: IndexMap::new(),
            direcao: p.direcao_soldagem,
            rotacao: p.sentido_rotacao,
            flat_orientation: p.flat_orientation,
            multiperfil_strategy: p.multiperfil_strategy,
            profile_shape: p.multiperfil_profile_shape,
            bolacha_tipo: p.bolacha_tipo,
            bolacha_osc_forma: p.bolacha_osc_forma,
            bolacha_osc_eixo: p.bolacha_osc_eixo,
            bolacha_osc_sentido: p.bolacha_osc_sentido,
            bolacha_zero: p.bolacha_zero,
            powder_name: p.powder_name.clone(),
            presets: HashMap::new(),
            invalid: HashSet::new(),
            dirty: false,
            last_edit: None,
        };
        for f in NUM_FIELDS {
            state.fields.insert(f.key, fmt_num((f.get)(p), f.decimals));
        }
        for f in INT_FIELDS {
            state.fields.insert(f.key, (f.get)(p).to_string());
        }
        for f in TEXT_FIELDS {
            state.fields.insert(f.key, (f.get)(p).to_string());
        }
        for f in FLAG_FIELDS {
            state.flags.insert(f.key, (f.get)(p));
        }
        // A UI Tkinter mostra "5.0" nos leads desligados (o JSON guarda 0.0).
        if !p.lead_extras_enabled {
            for key in ["lead_in", "lead_out"] {
                if state.fields.get(key).and_then(|t| parse_num(t)) == Some(0.0) {
                    state.fields.insert(key, LEAD_DEFAULT_TEXT.to_string());
                }
            }
        }
        state
    }

    /// Conversão crua dos textos: devolve os parâmetros e as chaves cujo
    /// texto não converteu (ficaram com o valor padrão). Com `cfg`, resolve
    /// também o pó; sem ele (presets) deixa `powder_*` no padrão.
    fn convert(&self, cfg: Option<&AppConfig>) -> (Params, HashSet<&'static str>) {
        let mut p = Params::default();
        let mut invalid = HashSet::new();
        for f in NUM_FIELDS {
            match self.fields.get(f.key).and_then(|t| parse_num(t)) {
                Some(v) => (f.set)(&mut p, v),
                None => {
                    invalid.insert(f.key);
                }
            }
        }
        for f in INT_FIELDS {
            match self.fields.get(f.key).and_then(|t| parse_int(t)) {
                Some(v) => (f.set)(&mut p, v),
                None => {
                    invalid.insert(f.key);
                }
            }
        }
        for f in TEXT_FIELDS {
            if let Some(t) = self.fields.get(f.key) {
                (f.set)(&mut p, t.clone());
            }
        }
        for f in FLAG_FIELDS {
            if let Some(b) = self.flags.get(f.key) {
                (f.set)(&mut p, *b);
            }
        }
        p.direcao_soldagem = self.direcao;
        p.sentido_rotacao = self.rotacao;
        p.flat_orientation = self.flat_orientation;
        p.multiperfil_strategy = self.multiperfil_strategy;
        p.multiperfil_profile_shape = self.profile_shape;
        p.bolacha_tipo = self.bolacha_tipo;
        p.bolacha_osc_forma = self.bolacha_osc_forma;
        p.bolacha_osc_eixo = self.bolacha_osc_eixo;
        p.bolacha_osc_sentido = self.bolacha_osc_sentido;
        p.bolacha_zero = self.bolacha_zero;
        p.apply_operation(self.operation);

        // Leads só com "Usar início e fim extra" (como `_get_current_params`).
        if !p.lead_extras_enabled {
            p.lead_in = 0.0;
            p.lead_out = 0.0;
            invalid.remove("lead_in");
            invalid.remove("lead_out");
        }
        // Operação interna só em espiral/multiperfil/oscilação.
        if !internal_applicable(self.operation) {
            p.operacao_interna = false;
        }
        // A UI tem uma velocidade só: taxa e velocidade do eixo A a espelham.
        p.taxa_de_deposicao = Some(p.velocidade_de_deposicao);
        p.velocidade_a_mm_min = p.velocidade_de_deposicao;

        if let Some(cfg) = cfg {
            let name = if self.powder_name.trim().is_empty() {
                cfg.active_powder.clone()
            } else {
                self.powder_name.clone()
            };
            let padrao = Params::default();
            let valido = |v: Option<f64>| v.filter(|x| x.is_finite() && *x > 0.0);
            let powder = cfg.powder(&name);
            p.densidade_po_g_cm3 = valido(powder.and_then(|w| w.density_g_cm3))
                .or_else(|| tfm_core::config::densidade_conhecida(&name))
                .unwrap_or(padrao.densidade_po_g_cm3);
            p.rendimento_deposicao_perc = valido(powder.and_then(|w| w.yield_perc))
                .map(|v| v.min(100.0))
                .unwrap_or(padrao.rendimento_deposicao_perc);
            match powder {
                Some(powder) => {
                    p.powder_factor = powder.density_factor_g_mm2;
                    p.powder_cost_brl_kg = powder.cost_brl_kg;
                }
                None => {
                    p.powder_factor = FALLBACK_POWDER_FACTOR;
                    p.powder_cost_brl_kg = cfg.costs.powder_brl_kg;
                }
            }
            p.powder_name = name;
            // A Bolacha gira até o limite de rpm do perfil da máquina.
            p.limite_rpm_placa = cfg.active_limits().max_rpm_chuck;
        }

        // Multiperfil: o número de camadas vem da altura do perfil.
        if self.operation == Operation::EspiralMultiperfil {
            p.num_camadas = profile_definition(&p).layer_count.max(1);
        }
        (p, invalid)
    }

    /// Porta de `_get_current_params`: os parâmetros que a interface
    /// descreve agora, com a operação aplicada e o pó resolvido pelo config.
    /// Texto inválido vale o padrão do campo (veja [`UiState::refresh_invalid`]).
    pub fn to_params(&self, cfg: &AppConfig) -> Params {
        self.convert(Some(cfg)).0
    }

    /// Chaves cujo texto **não converte** (só as falhas de `parse`, sem as
    /// regras físicas), na ordem dos campos. É o caso em que o
    /// `_get_current_params()` do Python devolvia `None` e os botões
    /// recusavam com "Por favor, verifique os parâmetros."; aqui
    /// [`UiState::to_params`] usa o padrão do campo, então quem vai gravar
    /// ou gerar deve consultar isto antes. Vazio = tudo converte.
    pub fn unparsable_fields(&self) -> Vec<&'static str> {
        let invalid = self.convert(None).1;
        NUM_FIELDS
            .iter()
            .map(|f| f.key)
            .chain(INT_FIELDS.iter().map(|f| f.key))
            .filter(|key| invalid.contains(key))
            .collect()
    }

    /// Chaves a pintar de vermelho: texto que não converte mais os campos
    /// por trás das regras físicas violadas (`validate_physical`).
    pub fn invalid_fields(&self, cfg: &AppConfig) -> HashSet<&'static str> {
        let (p, mut invalid) = self.convert(Some(cfg));
        invalid.extend(validate_physical(&p).invalid_fields);
        invalid
    }

    /// Recalcula [`UiState::invalid`] (o painel chama a cada quadro).
    pub fn refresh_invalid(&mut self, cfg: &AppConfig) {
        self.invalid = self.invalid_fields(cfg);
    }

    /// Registra uma edição: marca sujo e anota a hora (debounce do cálculo).
    pub fn mark_edited(&mut self) {
        self.dirty = true;
        self.last_edit = Some(Instant::now());
    }

    /// Nome do pó em vigor: o escolhido ou o `active_powder` do config.
    pub fn effective_powder<'a>(&'a self, cfg: &'a AppConfig) -> &'a str {
        if self.powder_name.trim().is_empty() {
            &cfg.active_powder
        } else {
            &self.powder_name
        }
    }

    /// Copia o valor de `key` em `p` para o texto/flag/enum correspondente.
    fn write_key(&mut self, key: &'static str, p: &Params) {
        if let Some(f) = NUM_FIELDS.iter().find(|f| f.key == key) {
            self.fields.insert(key, fmt_num((f.get)(p), f.decimals));
        } else if let Some(f) = INT_FIELDS.iter().find(|f| f.key == key) {
            self.fields.insert(key, (f.get)(p).to_string());
        } else if let Some(f) = TEXT_FIELDS.iter().find(|f| f.key == key) {
            self.fields.insert(key, (f.get)(p).to_string());
        } else if let Some(f) = FLAG_FIELDS.iter().find(|f| f.key == key) {
            self.flags.insert(key, (f.get)(p));
        } else {
            match key {
                "direcao_soldagem" => self.direcao = p.direcao_soldagem,
                "sentido_rotacao" => self.rotacao = p.sentido_rotacao,
                "flat_orientation" => self.flat_orientation = p.flat_orientation,
                "multiperfil_strategy" => self.multiperfil_strategy = p.multiperfil_strategy,
                "multiperfil_profile_shape" => self.profile_shape = p.multiperfil_profile_shape,
                "bolacha_tipo" => self.bolacha_tipo = p.bolacha_tipo,
                "bolacha_osc_forma" => self.bolacha_osc_forma = p.bolacha_osc_forma,
                "bolacha_osc_eixo" => self.bolacha_osc_eixo = p.bolacha_osc_eixo,
                "bolacha_osc_sentido" => self.bolacha_osc_sentido = p.bolacha_osc_sentido,
                "bolacha_zero" => self.bolacha_zero = p.bolacha_zero,
                other => log::debug!("UiState::write_key: chave desconhecida {other:?}"),
            }
        }
    }

    /// Porta de `_set_revestimento_mode`: guarda o preset da operação atual,
    /// troca, e restaura o preset da nova operação se já foi visitada; senão
    /// aplica os padrões de primeira entrada. Fecha a lista e marca sujo.
    pub fn set_operation(&mut self, op: Operation) {
        let current = self.operation;
        if op == current {
            return;
        }
        let snapshot = self.convert(None).0;
        self.presets.insert(current, snapshot);
        self.operation = op;
        match self.presets.get(&op).cloned() {
            Some(preset) => {
                for key in PRESET_KEYS {
                    self.write_key(key, &preset);
                }
            }
            None => {
                let defaults = Params::default();
                for key in first_entry_keys(op) {
                    self.write_key(key, &defaults);
                }
            }
        }
        self.mark_edited();
    }

    /// Botão "Padrão": [`Params::reset_mode_defaults`] da operação atual
    /// aplicado aos textos (só os campos do modo e a calibração).
    pub fn reset_defaults(&mut self) {
        let mut p = self.convert(None).0;
        p.reset_mode_defaults(self.operation);
        for key in reset_keys(self.operation)
            .iter()
            .chain(["bead_width_calibration_factor"].iter())
        {
            self.write_key(key, &p);
        }
        self.mark_edited();
    }

    /// Sincroniza o texto "Nº de Camadas" com o valor derivado do perfil
    /// (multiperfil: `_sync_multiperfil_profile_layers`). Não marca sujo,
    /// porque o valor já é o que [`UiState::to_params`] usa.
    pub fn sync_layer_count(&mut self, layer_count: i64) {
        let text = layer_count.max(1).to_string();
        if self.fields.get("num_camadas") != Some(&text) {
            self.fields.insert("num_camadas", text);
        }
    }

    /// Texto de um campo (vazio se a chave não existir).
    pub fn text(&self, key: &str) -> &str {
        self.fields.get(key).map_or("", String::as_str)
    }

    /// Valor de uma flag (`false` se a chave não existir).
    pub fn flag(&self, key: &str) -> bool {
        self.flags.get(key).copied().unwrap_or(false)
    }
}

/// Tipo de oscilação da operação atual, para quem precisa exibi-lo.
pub fn oscillation_type(op: Operation) -> Option<OscillationType> {
    op.mode().1
}

#[cfg(test)]
mod tests {
    use super::*;
    use tfm_core::config::{AppConfig, Powder};
    use tfm_core::params::{Params, WeldingMode};

    fn cfg() -> AppConfig {
        AppConfig::default()
    }

    /// `from_params(default).to_params(cfg)` devolve os defaults, exceto os
    /// campos que a conversão define a partir do config e da velocidade.
    #[test]
    fn to_params_matches_defaults() {
        let cfg = cfg();
        let state = UiState::from_params(&Params::default());
        let got = state.to_params(&cfg);
        let expected = Params {
            powder_factor: got.powder_factor,
            powder_cost_brl_kg: got.powder_cost_brl_kg,
            powder_name: got.powder_name.clone(),
            taxa_de_deposicao: got.taxa_de_deposicao,
            velocidade_a_mm_min: got.velocidade_a_mm_min,
            limite_rpm_placa: got.limite_rpm_placa,
            ..Params::default()
        };
        assert_eq!(got, expected);
        assert_eq!(got.powder_name, "Pó padrão");
        assert_eq!(got.powder_factor, 0.16);
        assert_eq!(got.powder_cost_brl_kg, 250.0);
        assert_eq!(got.taxa_de_deposicao, Some(120.0));
        assert_eq!(got.velocidade_a_mm_min, 120.0);
    }

    /// Os textos saem como a UI Tkinter os mostra.
    #[test]
    fn from_params_formats_like_python_ui() {
        let s = UiState::default();
        assert_eq!(s.fields["diametro"], "50.0");
        assert_eq!(s.fields["largura_cordao"], "8.0");
        assert_eq!(s.fields["sobreposicao"], "40.0");
        assert_eq!(s.fields["eficiencia_termica"], "0.6");
        assert_eq!(s.fields["velocidade_oscilacao_mm_min"], "3000.0");
        assert_eq!(s.fields["bead_width_calibration_factor"], "1.00");
        assert_eq!(s.fields["num_camadas"], "1");
        assert_eq!(s.fields["multiperfil_count"], "4");
        assert_eq!(s.fields["lead_in"], "5.0");
        assert_eq!(s.fields["lead_out"], "5.0");
        assert_eq!(s.fields["nome_procedimento"], "Novo Procedimento");
        assert!(s.flags["torch_retract_on_ignite"]);
        assert!(!s.flags["lead_extras_enabled"]);
        assert_eq!(s.operation, Operation::Espiral);
        assert_eq!(s.tab, LeftTab::Parametros);
        assert!(!s.dirty);
    }

    /// Muda o diâmetro na Espiral, vai para a Bolacha e volta: diâmetro da
    /// Espiral preservado; a Bolacha também guarda o seu.
    #[test]
    fn set_operation_restores_preset() {
        let cfg = cfg();
        let mut s = UiState::default();
        s.fields.insert("diametro", "75".to_string());
        s.set_operation(Operation::Bolacha);
        assert_eq!(s.operation, Operation::Bolacha);
        assert!(s.dirty);
        assert!(s.last_edit.is_some());
        // Primeira entrada na Bolacha: o diâmetro é compartilhado (continua 75).
        assert_eq!(s.to_params(&cfg).diametro, 75.0);
        s.fields.insert("diametro", "90".to_string());
        s.set_operation(Operation::Espiral);
        assert_eq!(s.to_params(&cfg).diametro, 75.0, "preset da Espiral");
        s.set_operation(Operation::Bolacha);
        assert_eq!(s.to_params(&cfg).diametro, 90.0, "preset da Bolacha");
        let p = s.to_params(&cfg);
        assert_eq!(p.welding_mode, WeldingMode::Bolacha);
        assert_eq!(p.oscillation_type, None);
    }

    /// Primeira entrada numa operação aplica os padrões como o
    /// `_set_revestimento_mode`; a segunda restaura o preset.
    #[test]
    fn first_entry_defaults_follow_python() {
        let cfg = cfg();
        let mut s = UiState::default();
        s.fields.insert("largura_revestir", "250".to_string());
        s.set_operation(Operation::ChapaPlana);
        assert_eq!(s.to_params(&cfg).largura_revestir, 100.0);
        s.fields.insert("largura_revestir", "250".to_string());
        s.set_operation(Operation::Espiral);
        s.set_operation(Operation::ChapaPlana);
        assert_eq!(s.to_params(&cfg).largura_revestir, 250.0);
        // Voltar para a mesma operação não mexe em nada.
        s.dirty = false;
        s.set_operation(Operation::ChapaPlana);
        assert!(!s.dirty);
    }

    /// Texto inválido pinta o campo e deixa o valor padrão; regra física
    /// violada também pinta.
    #[test]
    fn invalid_text_marks_field() {
        let cfg = cfg();
        let mut s = UiState::default();
        s.fields.insert("diametro", "abc".to_string());
        s.refresh_invalid(&cfg);
        assert!(s.invalid.contains("diametro"));
        assert_eq!(s.to_params(&cfg).diametro, Params::default().diametro);

        s.fields.insert("diametro", "50,5".to_string());
        s.fields.insert("sobreposicao", "150".to_string());
        s.fields.insert("num_camadas", "1.5".to_string());
        s.refresh_invalid(&cfg);
        assert!(!s.invalid.contains("diametro"));
        assert!(s.invalid.contains("sobreposicao"));
        assert!(
            s.invalid.contains("num_camadas"),
            "inteiro nao aceita fracao"
        );
        assert_eq!(s.to_params(&cfg).diametro, 50.5);
    }

    /// Leads só entram com "Usar início e fim extra" marcado.
    #[test]
    fn leads_zeroed_unless_enabled() {
        let cfg = cfg();
        let mut s = UiState::default();
        let p = s.to_params(&cfg);
        assert!(!p.lead_extras_enabled);
        assert_eq!((p.lead_in, p.lead_out), (0.0, 0.0));
        s.flags.insert("lead_extras_enabled", true);
        let p = s.to_params(&cfg);
        assert!(p.lead_extras_enabled);
        assert_eq!((p.lead_in, p.lead_out), (5.0, 5.0));
        // Lead inválido só pinta quando o flag está ligado.
        s.fields.insert("lead_in", "x".to_string());
        s.refresh_invalid(&cfg);
        assert!(s.invalid.contains("lead_in"));
        s.flags.insert("lead_extras_enabled", false);
        s.refresh_invalid(&cfg);
        assert!(!s.invalid.contains("lead_in"));
    }

    /// Operação interna só em espiral/multiperfil/oscilações.
    #[test]
    fn operacao_interna_only_where_applicable() {
        let cfg = cfg();
        let mut s = UiState::default();
        s.flags.insert("operacao_interna", true);
        assert!(s.to_params(&cfg).operacao_interna);
        s.set_operation(Operation::OscilacaoQuadrada);
        s.flags.insert("operacao_interna", true);
        assert!(s.to_params(&cfg).operacao_interna);
        s.set_operation(Operation::ChapaPlana);
        s.flags.insert("operacao_interna", true);
        assert!(!s.to_params(&cfg).operacao_interna);
        s.set_operation(Operation::Bolacha);
        s.flags.insert("operacao_interna", true);
        assert!(!s.to_params(&cfg).operacao_interna);
    }

    /// No multiperfil o número de camadas vem do perfil.
    #[test]
    fn multiperfil_layer_count_from_profile() {
        let cfg = cfg();
        let mut s = UiState::default();
        s.set_operation(Operation::EspiralMultiperfil);
        s.fields
            .insert("multiperfil_profile_height_mm", "4.5".to_string());
        s.fields.insert("espessura_camada", "1.5".to_string());
        s.fields.insert("num_camadas", "1".to_string());
        let p = s.to_params(&cfg);
        assert_eq!(p.welding_mode, WeldingMode::EspiralMultiperfil);
        assert_eq!(p.num_camadas, 3);
    }

    /// A caixa "Fechamento uniforme" vai e volta entre a UI e os parâmetros
    /// e é um campo do modo (o "Padrão" a desliga).
    #[test]
    fn fechamento_uniforme_flag_round_trips() {
        let cfg = cfg();
        let mut s = UiState::default();
        assert!(!s.to_params(&cfg).fechamento_uniforme, "padrão é desligado");
        s.flags.insert("fechamento_uniforme", true);
        assert!(s.to_params(&cfg).fechamento_uniforme);
        let p = Params {
            fechamento_uniforme: true,
            ..Params::default()
        };
        assert!(UiState::from_params(&p).flags["fechamento_uniforme"]);
        s.reset_defaults();
        assert!(!s.flags["fechamento_uniforme"]);
    }

    /// "Padrão" restaura só os campos do modo (e a calibração).
    #[test]
    fn reset_defaults_restores_mode_fields_only() {
        let cfg = cfg();
        let mut s = UiState::default();
        s.fields.insert("diametro", "75".to_string());
        s.flags.insert("spiral_close_ends", true);
        s.flags.insert("operacao_interna", true);
        s.fields
            .insert("bead_width_calibration_factor", "0.9".to_string());
        s.dirty = false;
        s.reset_defaults();
        assert!(s.dirty);
        assert_eq!(
            s.to_params(&cfg).diametro,
            75.0,
            "diametro nao e campo de modo"
        );
        assert!(!s.flags["spiral_close_ends"]);
        assert!(!s.flags["operacao_interna"]);
        assert_eq!(s.fields["bead_width_calibration_factor"], "1.00");

        s.set_operation(Operation::Bolacha);
        s.fields.insert("overlap_angular_deg", "33".to_string());
        s.fields.insert("diametro_interno", "20".to_string());
        s.bolacha_tipo = BolachaTipo::Aneis;
        s.reset_defaults();
        assert_eq!(s.fields["overlap_angular_deg"], "10.0");
        assert_eq!(s.fields["diametro_interno"], "0.0");
        assert_eq!(s.bolacha_tipo, BolachaTipo::EspiralContinua);
    }

    /// O pó escolhido define fator e custo; ausente, 0.16 e o custo geral.
    #[test]
    fn powder_resolution_uses_config() {
        let mut cfg = cfg();
        cfg.powders.push(Powder {
            name: "Stellite 6".to_string(),
            density_factor_g_mm2: 0.2,
            cost_brl_kg: 900.0,
            ..Powder::default()
        });
        let mut s = UiState {
            powder_name: "Stellite 6".to_string(),
            ..UiState::default()
        };
        let p = s.to_params(&cfg);
        assert_eq!(p.powder_factor, 0.2);
        assert_eq!(p.powder_cost_brl_kg, 900.0);
        assert_eq!(p.powder_name, "Stellite 6");

        s.powder_name = "inexistente".to_string();
        let p = s.to_params(&cfg);
        assert_eq!(p.powder_factor, 0.16);
        assert_eq!(p.powder_cost_brl_kg, cfg.costs.powder_brl_kg);
        assert_eq!(p.powder_name, "inexistente");

        s.powder_name.clear();
        assert_eq!(s.to_params(&cfg).powder_name, cfg.active_powder);
    }

    /// O teto de rpm da Bolacha vem do perfil da máquina.
    #[test]
    fn plate_rpm_limit_comes_from_the_machine_profile() {
        let cfg = cfg();
        let p = UiState::default().to_params(&cfg);
        assert_eq!(p.limite_rpm_placa, cfg.active_limits().max_rpm_chuck);
    }

    /// Densidade e rendimento: os do pó cadastrado; sem eles, a densidade
    /// conhecida da liga pelo nome; sem nada, os padrões (8,46 e 87%).
    #[test]
    fn powder_density_and_yield_resolution() {
        let mut cfg = cfg();
        cfg.powders.push(Powder {
            name: "WC-Ni (Carbeto)".to_string(),
            ..Powder::default()
        });
        cfg.powders.push(Powder {
            name: "Minha liga".to_string(),
            density_g_cm3: Some(7.5),
            yield_perc: Some(70.0),
            ..Powder::default()
        });
        let mut s = UiState {
            powder_name: "WC-Ni (Carbeto)".to_string(),
            ..UiState::default()
        };
        let p = s.to_params(&cfg);
        assert_eq!(p.densidade_po_g_cm3, 14.5);
        assert_eq!(p.rendimento_deposicao_perc, 87.0);

        s.powder_name = "Minha liga".to_string();
        let p = s.to_params(&cfg);
        assert_eq!(p.densidade_po_g_cm3, 7.5);
        assert_eq!(p.rendimento_deposicao_perc, 70.0);

        s.powder_name = "inexistente".to_string();
        let p = s.to_params(&cfg);
        assert_eq!(p.densidade_po_g_cm3, 8.46);
        assert_eq!(p.rendimento_deposicao_perc, 87.0);
    }

    /// Os campos da oscilação da bolacha fazem a volta, voltam ao padrão e
    /// ficam guardados ao trocar de operação.
    #[test]
    fn bolacha_oscillation_fields_round_trip_and_reset() {
        use tfm_core::params::{BolachaOscEixo, BolachaOscForma, BolachaOscSentido};
        let cfg = cfg();
        let mut p = Params {
            bolacha_oscilar: true,
            bolacha_osc_eixo: BolachaOscEixo::Y,
            bolacha_osc_forma: BolachaOscForma::Senoidal,
            bolacha_osc_sentido: BolachaOscSentido::ParaDentro,
            bolacha_osc_periodo_mm: 7.5,
            bolacha_osc_fase_camada_perc: 20.0,
            bolacha_osc_espera_s: 0.3,
            bolacha_osc_pontos_ciclo: 16.0,
            oscilacao_comprimento: 3.0,
            ..Params::default()
        };
        p.apply_operation(Operation::Bolacha);
        let mut s = UiState::from_params(&p);
        let back = s.to_params(&cfg);
        assert!(back.bolacha_oscilar);
        assert_eq!(back.bolacha_osc_eixo, BolachaOscEixo::Y);
        assert_eq!(back.bolacha_osc_forma, BolachaOscForma::Senoidal);
        assert_eq!(back.bolacha_osc_sentido, BolachaOscSentido::ParaDentro);
        assert_eq!(back.bolacha_osc_periodo_mm, 7.5);
        assert_eq!(back.bolacha_osc_fase_camada_perc, 20.0);
        assert_eq!(back.bolacha_osc_espera_s, 0.3);
        assert_eq!(back.bolacha_osc_pontos_ciclo, 16.0);
        assert_eq!(back.oscilacao_comprimento, 3.0);

        // Trocar de operação e voltar mantém a configuração da bolacha.
        s.set_operation(Operation::Espiral);
        s.set_operation(Operation::Bolacha);
        let volta = s.to_params(&cfg);
        assert!(volta.bolacha_oscilar);
        assert_eq!(volta.bolacha_osc_eixo, BolachaOscEixo::Y);
        assert_eq!(volta.bolacha_osc_forma, BolachaOscForma::Senoidal);
        assert_eq!(volta.bolacha_osc_periodo_mm, 7.5);

        s.reset_defaults();
        let d = s.to_params(&cfg);
        assert!(!d.bolacha_oscilar);
        assert_eq!(d.bolacha_osc_eixo, BolachaOscEixo::X);
        assert_eq!(d.bolacha_osc_forma, BolachaOscForma::Triangular);
        assert_eq!(d.bolacha_osc_sentido, BolachaOscSentido::ParaFora);
        assert_eq!(d.bolacha_osc_periodo_mm, 10.0);
        assert_eq!(d.bolacha_osc_pontos_ciclo, 12.0);
    }

    /// Enums e textos livres fazem a volta.
    #[test]
    fn enums_and_texts_round_trip() {
        let cfg = cfg();
        let mut p = Params {
            direcao_soldagem: Direction::DireitaEsquerda,
            sentido_rotacao: Rotation::Antihoraria,
            flat_orientation: FlatOrientation::Y,
            multiperfil_strategy: MultiperfilStrategy::PercursoContinuo,
            multiperfil_profile_shape: ProfileShape::Trapezoidal,
            bolacha_tipo: BolachaTipo::Aneis,
            traceability_operator: "Ana".to_string(),
            notes: "duas\nlinhas".to_string(),
            ordem_servico: "OS-1".to_string(),
            ..Params::default()
        };
        p.apply_operation(Operation::OscilacaoQuadradaContinua);
        let s = UiState::from_params(&p);
        assert_eq!(s.operation, Operation::OscilacaoQuadradaContinua);
        let back = s.to_params(&cfg);
        assert_eq!(back.direcao_soldagem, Direction::DireitaEsquerda);
        assert_eq!(back.sentido_rotacao, Rotation::Antihoraria);
        assert_eq!(back.flat_orientation, FlatOrientation::Y);
        assert_eq!(
            back.multiperfil_strategy,
            MultiperfilStrategy::PercursoContinuo
        );
        assert_eq!(back.multiperfil_profile_shape, ProfileShape::Trapezoidal);
        assert_eq!(back.bolacha_tipo, BolachaTipo::Aneis);
        assert_eq!(back.traceability_operator, "Ana");
        assert_eq!(back.notes, "duas\nlinhas");
        assert_eq!(back.ordem_servico, "OS-1");
        assert_eq!(back.welding_mode, WeldingMode::Oscilacao);
        assert_eq!(
            back.oscillation_type,
            Some(OscillationType::QuadradaContinua)
        );
    }

    #[test]
    fn visibility_predicates() {
        assert!(is_oscillation(Operation::OscilacaoLinear));
        assert!(is_oscillation(Operation::OscilacaoQuadradaContinua));
        assert!(!is_oscillation(Operation::Espiral));
        assert!(internal_applicable(Operation::Espiral));
        assert!(internal_applicable(Operation::EspiralMultiperfil));
        assert!(internal_applicable(Operation::OscilacaoQuadrada));
        assert!(!internal_applicable(Operation::ChapaPlana));
        assert!(!internal_applicable(Operation::Bolacha));
        assert!(has_advanced(Operation::Espiral));
        assert!(has_advanced(Operation::Bolacha));
        assert!(!has_advanced(Operation::ChapaPlana));
        assert!(!has_advanced(Operation::OscilacaoLinear));
    }

    /// `unparsable_fields` só lista texto que não converte: um valor
    /// fisicamente inválido mas numérico (sobreposição 150%) pinta o campo
    /// (`invalid_fields`) mas não bloqueia como "verifique os parâmetros".
    #[test]
    fn unparsable_fields_exclude_physical_rules() {
        let cfg = cfg();
        let mut s = UiState::default();
        assert!(s.unparsable_fields().is_empty());

        s.fields.insert("sobreposicao", "150".to_string());
        assert!(s.unparsable_fields().is_empty(), "150 converte");
        assert!(s.invalid_fields(&cfg).contains("sobreposicao"));

        s.fields.insert("diametro", "abc".to_string());
        s.fields.insert("num_camadas", "1.5".to_string());
        assert_eq!(s.unparsable_fields(), ["diametro", "num_camadas"]);
        assert!(s.invalid_fields(&cfg).contains("diametro"));

        // Lead inválido só conta com "Usar início e fim extra" ligado.
        s.fields.insert("diametro", "50".to_string());
        s.fields.insert("num_camadas", "2".to_string());
        s.fields.insert("lead_in", "x".to_string());
        assert!(s.unparsable_fields().is_empty());
        s.flags.insert("lead_extras_enabled", true);
        assert_eq!(s.unparsable_fields(), ["lead_in"]);
    }
}
