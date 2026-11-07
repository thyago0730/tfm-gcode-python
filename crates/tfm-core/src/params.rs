//! Parâmetros do procedimento de revestimento (`Params`).
//!
//! Porta do dicionário `params` do TFM G-Code Generator em Python:
//!
//! * os nomes dos campos são exatamente as chaves do JSON gravado pelo Python
//!   (presets, procedimentos salvos, fixtures golden);
//! * chaves legadas continuam aceitas na leitura (`velocidade_soldagem`,
//!   `internal_entry_x`) mas nunca são escritas; os modos legados
//!   `welding_mode = oscilacao_linear|oscilacao_quadrada` são variantes reais
//!   de [`WeldingMode`] e voltam ao JSON exatamente como entraram, porque o
//!   cabeçalho do G-code imprime o valor cru;
//! * números podem chegar como string (`"5.0"`, `"12,5"`, `""`) e booleanos
//!   como `0`/`1` ou `"true"`/`"false"`, porque a UI Tkinter guardava tudo em
//!   `StringVar`. Veja [`lenient_f64`], [`lenient_i64`], [`lenient_bool`].
//!
//! [`Operation`] é a visão da UI (sete operações do combobox) e **não** é
//! serializada: deriva de `welding_mode` + `oscillation_type`.

use std::fmt;

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Enums serializados com os valores de string do Python
// ---------------------------------------------------------------------------

/// Modo de soldagem gravado em `welding_mode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WeldingMode {
    #[default]
    #[serde(rename = "espiral")]
    Espiral,
    #[serde(rename = "espiral_multiperfil")]
    EspiralMultiperfil,
    #[serde(rename = "bolacha")]
    Bolacha,
    #[serde(rename = "chapa_plana")]
    ChapaPlana,
    /// Modo unificado de oscilação; o tipo vem de `oscillation_type`.
    #[serde(rename = "oscilacao")]
    Oscilacao,
    /// Legado: presets antigos gravavam o tipo no próprio modo. Mantido como
    /// variante real porque o cabeçalho do G-code imprime o valor cru.
    #[serde(rename = "oscilacao_linear")]
    OscilacaoLinear,
    /// Legado, ver [`WeldingMode::OscilacaoLinear`].
    #[serde(rename = "oscilacao_quadrada")]
    OscilacaoQuadrada,
}

impl WeldingMode {
    /// Valor de string exatamente como gravado no JSON do Python.
    pub fn as_str(self) -> &'static str {
        match self {
            WeldingMode::Espiral => "espiral",
            WeldingMode::EspiralMultiperfil => "espiral_multiperfil",
            WeldingMode::Bolacha => "bolacha",
            WeldingMode::ChapaPlana => "chapa_plana",
            WeldingMode::Oscilacao => "oscilacao",
            WeldingMode::OscilacaoLinear => "oscilacao_linear",
            WeldingMode::OscilacaoQuadrada => "oscilacao_quadrada",
        }
    }

    /// `true` para o modo unificado de oscilação e para os dois legados
    /// (o Python testa `mode in ('oscilacao', 'oscilacao_linear', 'oscilacao_quadrada')`).
    pub fn is_oscillation(self) -> bool {
        matches!(
            self,
            WeldingMode::Oscilacao | WeldingMode::OscilacaoLinear | WeldingMode::OscilacaoQuadrada
        )
    }
}

impl fmt::Display for WeldingMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Tipo de oscilação gravado em `oscillation_type`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OscillationType {
    #[default]
    #[serde(rename = "linear")]
    Linear,
    #[serde(rename = "quadrada")]
    Quadrada,
    #[serde(rename = "quadrada_continua")]
    QuadradaContinua,
}

/// Direção de soldagem ao longo do eixo X (`direcao_soldagem`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    #[default]
    #[serde(rename = "esquerda_direita")]
    EsquerdaDireita,
    #[serde(rename = "direita_esquerda")]
    DireitaEsquerda,
}

/// Sentido de rotação do eixo A (`sentido_rotacao`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Rotation {
    #[default]
    #[serde(rename = "horaria")]
    Horaria,
    #[serde(rename = "antihoraria")]
    Antihoraria,
}

/// Forma do perfil no modo Espiral Multiperfil (`multiperfil_profile_shape`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProfileShape {
    #[default]
    Retangular,
    Trapezoidal,
    Triangular,
}

/// Estratégia de percurso no modo Espiral Multiperfil (`multiperfil_strategy`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MultiperfilStrategy {
    #[default]
    #[serde(rename = "Passes separados")]
    PassesSeparados,
    #[serde(rename = "Percurso contínuo")]
    PercursoContinuo,
}

/// Tipo de bolacha (`bolacha_tipo`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BolachaTipo {
    #[default]
    #[serde(rename = "Espiral Contínua")]
    EspiralContinua,
    #[serde(rename = "Anéis (Passo a Passo)")]
    Aneis,
}

/// Onde fica o zero do eixo radial na Bolacha (`bolacha_zero`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BolachaZero {
    /// Zero no centro da peça: o eixo radial escreve o raio.
    #[default]
    #[serde(rename = "Centro")]
    Centro,
    /// Zero na borda do furo: o eixo radial escreve `raio − raio interno`, e a
    /// tocha não precisa ir ao centro.
    #[serde(rename = "Diâmetro interno")]
    DiametroInterno,
}

/// Forma da onda da oscilação da bolacha (`bolacha_osc_forma`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BolachaOscForma {
    /// Vai e volta em retas, com vértices nos picos.
    #[default]
    #[serde(rename = "Triangular")]
    Triangular,
    /// Senoide, em `bolacha_osc_pontos_ciclo` pontos por ciclo.
    #[serde(rename = "Senoidal")]
    Senoidal,
    /// Degraus: o X sobe e desce com a mesa parada.
    #[serde(rename = "Quadrada")]
    Quadrada,
}

/// Eixo em que a tocha oscila na bolacha (`bolacha_osc_eixo`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BolachaOscEixo {
    /// A tocha fica à direita, sobre o eixo X: o X faz o raio e a oscilação.
    #[default]
    #[serde(rename = "Radial (X)")]
    X,
    /// A tocha fica atrás, sobre o eixo Y (X = 0): o Y faz o raio e a
    /// oscilação, para frente e para trás. "Tangencial (Y)" é o nome antigo.
    #[serde(rename = "Radial (Y)", alias = "Tangencial (Y)")]
    Y,
}

/// Sentido do primeiro movimento radial da oscilação da bolacha.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BolachaOscSentido {
    /// Primeiro rumo ao raio externo.
    #[default]
    #[serde(rename = "Para fora")]
    ParaFora,
    /// Primeiro rumo ao centro.
    #[serde(rename = "Para dentro")]
    ParaDentro,
}

/// Orientação dos cordões na chapa plana (`flat_orientation`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FlatOrientation {
    #[default]
    X,
    Y,
}

// ---------------------------------------------------------------------------
// Operation: as sete operações do combobox da UI (não serializada)
// ---------------------------------------------------------------------------

/// Operação escolhida na UI. Deriva de `welding_mode` + `oscillation_type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Operation {
    Espiral,
    EspiralMultiperfil,
    OscilacaoLinear,
    OscilacaoQuadrada,
    OscilacaoQuadradaContinua,
    ChapaPlana,
    Bolacha,
}

impl Operation {
    /// Todas as operações, na ordem em que a UI Python as lista.
    pub const ALL: [Operation; 7] = [
        Operation::Espiral,
        Operation::EspiralMultiperfil,
        Operation::OscilacaoLinear,
        Operation::OscilacaoQuadrada,
        Operation::OscilacaoQuadradaContinua,
        Operation::ChapaPlana,
        Operation::Bolacha,
    ];

    /// Rótulo completo, igual ao combobox `tipo_revestimento` do Python.
    pub fn label(self) -> &'static str {
        match self {
            Operation::Espiral => "Espiral",
            Operation::EspiralMultiperfil => "Espiral Multiperfil",
            Operation::OscilacaoLinear => "Oscilação Linear",
            Operation::OscilacaoQuadrada => "Oscilação Quadrada",
            Operation::OscilacaoQuadradaContinua => "Oscilação Quadrada Contínua",
            Operation::ChapaPlana => "Chapa Plana",
            Operation::Bolacha => "Bolacha",
        }
    }

    /// Rótulo curto para espaços apertados (abas, status). Igual a
    /// [`label`](Self::label) para as operações sem oscilação.
    pub fn short_label(self) -> &'static str {
        match self {
            Operation::OscilacaoLinear => "Osc. Linear",
            Operation::OscilacaoQuadrada => "Osc. Quad.",
            Operation::OscilacaoQuadradaContinua => "Osc. Quad. Cont.",
            other => other.label(),
        }
    }

    /// Localiza a operação pelo rótulo completo ou curto.
    pub fn from_label(label: &str) -> Option<Operation> {
        let wanted = label.trim();
        Operation::ALL
            .into_iter()
            .find(|op| op.label() == wanted || op.short_label() == wanted)
    }

    /// Par (`welding_mode`, `oscillation_type`) que o Python grava para esta
    /// operação. `None` quando a operação não usa oscilação.
    pub fn mode(self) -> (WeldingMode, Option<OscillationType>) {
        match self {
            Operation::Espiral => (WeldingMode::Espiral, None),
            Operation::EspiralMultiperfil => (WeldingMode::EspiralMultiperfil, None),
            Operation::OscilacaoLinear => (WeldingMode::Oscilacao, Some(OscillationType::Linear)),
            Operation::OscilacaoQuadrada => {
                (WeldingMode::Oscilacao, Some(OscillationType::Quadrada))
            }
            Operation::OscilacaoQuadradaContinua => (
                WeldingMode::Oscilacao,
                Some(OscillationType::QuadradaContinua),
            ),
            Operation::ChapaPlana => (WeldingMode::ChapaPlana, None),
            Operation::Bolacha => (WeldingMode::Bolacha, None),
        }
    }

    /// Operação implícita nos parâmetros (mesma regra do `generate` Python):
    /// os modos legados já carregam o tipo; o modo unificado olha
    /// `oscillation_type`, e ausente equivale a linear.
    pub fn from_params(p: &Params) -> Operation {
        match p.welding_mode {
            WeldingMode::Espiral => Operation::Espiral,
            WeldingMode::EspiralMultiperfil => Operation::EspiralMultiperfil,
            WeldingMode::Bolacha => Operation::Bolacha,
            WeldingMode::ChapaPlana => Operation::ChapaPlana,
            WeldingMode::OscilacaoLinear => Operation::OscilacaoLinear,
            WeldingMode::OscilacaoQuadrada => Operation::OscilacaoQuadrada,
            WeldingMode::Oscilacao => match p.oscillation_type {
                None | Some(OscillationType::Linear) => Operation::OscilacaoLinear,
                Some(OscillationType::Quadrada) => Operation::OscilacaoQuadrada,
                Some(OscillationType::QuadradaContinua) => Operation::OscilacaoQuadradaContinua,
            },
        }
    }
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

// ---------------------------------------------------------------------------
// Params
// ---------------------------------------------------------------------------

/// Todos os parâmetros do procedimento, com os mesmos nomes do JSON Python.
///
/// Campos ausentes no JSON recebem o valor de [`Params::default`] (os defaults
/// da UI). Campos presentes são lidos de forma leniente (string, vírgula
/// decimal, `0`/`1`), ver os `lenient_*` deste módulo.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Params {
    // --- Geometria ---------------------------------------------------------
    #[serde(deserialize_with = "lenient_f64")]
    pub diametro: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub diametro_interno: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub comprimento_revestir: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub largura_revestir: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub largura_cordao: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub sobreposicao: f64,

    // --- Velocidades -------------------------------------------------------
    /// Velocidade de deposição (mm/min). Presets antigos: `velocidade_soldagem`.
    #[serde(alias = "velocidade_soldagem", deserialize_with = "lenient_f64")]
    pub velocidade_de_deposicao: f64,
    /// Espelho de `velocidade_de_deposicao`; `None` até [`Params::normalize`]
    /// (e então ausente do JSON, como a chave ausente no Python).
    #[serde(
        deserialize_with = "lenient_opt_f64",
        skip_serializing_if = "Option::is_none"
    )]
    pub taxa_de_deposicao: Option<f64>,
    #[serde(deserialize_with = "lenient_f64")]
    pub velocidade_a_mm_min: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub velocidade_oscilacao_mm_min: f64,

    // --- Tocha -------------------------------------------------------------
    #[serde(deserialize_with = "lenient_f64")]
    pub afastamento_tocha: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub afastamento_seguranca: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub torch_ignite_delay_s: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub torch_extinguish_delay_s: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub overlap_angular_deg: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub bead_width_calibration_factor: f64,
    #[serde(deserialize_with = "lenient_bool")]
    pub lead_extras_enabled: bool,
    #[serde(deserialize_with = "lenient_f64")]
    pub lead_in: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub lead_out: f64,
    pub direcao_soldagem: Direction,
    pub sentido_rotacao: Rotation,

    // --- Processo / energia ------------------------------------------------
    #[serde(deserialize_with = "lenient_f64")]
    pub corrente_arco: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub tensao_arco: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub vazao_gas: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub alim_po: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub preaquecimento: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub eficiencia_termica: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub taxa_deposicao_g_h: f64,
    #[serde(deserialize_with = "lenient_i64")]
    pub num_camadas: i64,
    #[serde(deserialize_with = "lenient_f64")]
    pub espessura_camada: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub powder_factor: f64,
    /// Densidade do pó (g/cm³): a massa depositada é o volume vezes ela.
    /// Vem do pó escolhido. Não existia no Python.
    #[serde(deserialize_with = "lenient_f64")]
    pub densidade_po_g_cm3: f64,
    /// Quanto do pó alimentado vira cordão (%). O resto se perde fora da poça.
    /// Vem do pó escolhido. Não existia no Python.
    #[serde(deserialize_with = "lenient_f64")]
    pub rendimento_deposicao_perc: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub powder_cost_brl_kg: f64,
    #[serde(deserialize_with = "lenient_bool")]
    pub torch_retract_on_ignite: bool,

    // --- Espiral / chapa / interno -----------------------------------------
    #[serde(deserialize_with = "lenient_bool")]
    pub spiral_close_ends: bool,
    /// Fechamento uniforme: o comprimento vale de borda a borda do material,
    /// com voltas inteiras no curso do centro do cordão e, na espiral, anéis
    /// nas pontas (ver `overlap::uniform_closing`). Não existia no Python.
    #[serde(deserialize_with = "lenient_bool")]
    pub fechamento_uniforme: bool,
    /// Compensa em rotação: acelera onde a simulação do depósito mostra que o
    /// material acumula (ver `deposit::solve_feed_factors`). Não existia no
    /// Python.
    #[serde(deserialize_with = "lenient_bool")]
    pub compensar_rotacao: bool,
    /// Defasa os passes entre camadas: cada camada desloca a trama de passes
    /// em uma fração do passo, para os cumes caírem nos vales da anterior. Sem
    /// isso o relevo entre passes soma a cada camada. Só na Espiral. Não
    /// existia no Python.
    #[serde(deserialize_with = "lenient_bool")]
    pub defasar_camadas: bool,
    /// Gera o programa em G93 (tempo inverso): cada `G01` traz `F = 1 ÷
    /// minutos` da linha, e a velocidade não depende do "raio do eixo A"
    /// configurado no Mach3. Não existia no Python.
    #[serde(deserialize_with = "lenient_bool")]
    pub tempo_inverso_g93: bool,
    /// Teto da rotação da placa na Bolacha (rpm), perto do centro. Padrão 20,
    /// o fixo do Python; a tela usa o `max_rpm_chuck` do perfil da máquina.
    #[serde(deserialize_with = "lenient_f64")]
    pub limite_rpm_placa: f64,
    /// Setup, preaquecimento e esperas fora do arco (min), para o orçamento:
    /// entra na mão de obra e na máquina. Não existia no Python.
    #[serde(deserialize_with = "lenient_f64")]
    pub tempo_setup_min: f64,
    /// Bolacha: faixa aceita da largura do cordão (mm). Ligada (mínimo > 0), o
    /// número de anéis é escolhido para o passo fechar a largura da peça, com
    /// a largura dentro da faixa e a velocidade compensando o passo. Zero:
    /// desligada. Não existia no Python.
    #[serde(deserialize_with = "lenient_f64")]
    pub largura_cordao_min: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub largura_cordao_max: f64,
    #[serde(deserialize_with = "lenient_bool")]
    pub flat_single_direction: bool,
    pub flat_orientation: FlatOrientation,
    #[serde(deserialize_with = "lenient_bool")]
    pub operacao_interna: bool,
    /// Folga em X até a face interna (mm). Presets antigos: `internal_entry_x`.
    #[serde(alias = "internal_entry_x", deserialize_with = "lenient_f64")]
    pub internal_face_clearance_x: f64,

    // --- Espiral Multiperfil -----------------------------------------------
    #[serde(deserialize_with = "lenient_i64")]
    pub multiperfil_count: i64,
    #[serde(deserialize_with = "lenient_f64")]
    pub multiperfil_total_angle_deg: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub multiperfil_pitch_mm: f64,
    pub multiperfil_strategy: MultiperfilStrategy,
    #[serde(deserialize_with = "lenient_bool")]
    pub multiperfil_point_by_point: bool,
    pub multiperfil_profile_shape: ProfileShape,
    #[serde(deserialize_with = "lenient_f64")]
    pub multiperfil_profile_base_mm: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub multiperfil_profile_top_mm: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub multiperfil_profile_height_mm: f64,
    #[serde(deserialize_with = "lenient_bool")]
    pub multiperfil_end_chamfer_enabled: bool,
    #[serde(deserialize_with = "lenient_f64")]
    pub multiperfil_end_chamfer_angle_deg: f64,

    // --- Bolacha -----------------------------------------------------------
    pub bolacha_tipo: BolachaTipo,
    #[serde(deserialize_with = "lenient_bool")]
    pub bolacha_stop_between: bool,
    /// Oscila na bolacha: o X (raio) vai e volta em torno da trajetória
    /// enquanto a mesa gira. O comprimento (pico a pico) é
    /// `oscilacao_comprimento`; a velocidade dos movimentos só em X é
    /// `velocidade_oscilacao_mm_min`. Não existia no Python.
    #[serde(deserialize_with = "lenient_bool")]
    pub bolacha_oscilar: bool,
    pub bolacha_osc_forma: BolachaOscForma,
    /// Eixo da oscilação: X (radial) ou Y (a tocha para frente e para trás).
    pub bolacha_osc_eixo: BolachaOscEixo,
    pub bolacha_osc_sentido: BolachaOscSentido,
    /// Período ao longo do arco (mm): quanto da trajetória um ciclo percorre.
    #[serde(deserialize_with = "lenient_f64")]
    pub bolacha_osc_periodo_mm: f64,
    /// Quanto a onda avança de uma camada para a seguinte, em % do ciclo.
    #[serde(deserialize_with = "lenient_f64")]
    pub bolacha_osc_fase_camada_perc: f64,
    /// Espera em cada pico (s); zero não espera.
    #[serde(deserialize_with = "lenient_f64")]
    pub bolacha_osc_espera_s: f64,
    /// Pontos por ciclo da senoide.
    #[serde(deserialize_with = "lenient_f64")]
    pub bolacha_osc_pontos_ciclo: f64,
    /// Oscilação granulada, como a quadrada contínua: cada trecho em que o
    /// raio e a mesa andam juntos vira uma escada de subpassos `G01 X` (no
    /// avanço da oscilação, até `osc_test_gran_x` mm) e `G01 A` (até
    /// `osc_test_gran_a` graus). Não existia no Python.
    #[serde(deserialize_with = "lenient_bool")]
    pub bolacha_osc_escada: bool,
    /// Zero do eixo radial da Bolacha: centro ou diâmetro interno. Não existia
    /// no Python.
    pub bolacha_zero: BolachaZero,

    // --- Oscilação ---------------------------------------------------------
    #[serde(deserialize_with = "lenient_f64")]
    pub oscilacao_comprimento: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub deslocamento_angular_perc: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub osc_test_gran_x: f64,
    #[serde(deserialize_with = "lenient_f64")]
    pub osc_test_gran_a: f64,

    // --- Camadas inteligentes ----------------------------------------------
    #[serde(deserialize_with = "lenient_bool")]
    pub smart_layers: bool,
    #[serde(deserialize_with = "lenient_f64")]
    pub root_feed_scale: f64,

    // --- Identificação / modo ----------------------------------------------
    #[serde(deserialize_with = "lenient_string")]
    pub nome_procedimento: String,
    pub welding_mode: WeldingMode,
    /// Só existe no JSON para modos de oscilação; ausente, o cabeçalho do
    /// G-code imprime `( OSCILACAO: ATIVADA )`, então "ausente" é preservado.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oscillation_type: Option<OscillationType>,

    // --- Rastreabilidade ---------------------------------------------------
    #[serde(deserialize_with = "lenient_string")]
    pub traceability_operator: String,
    #[serde(deserialize_with = "lenient_string")]
    pub traceability_eps: String,
    #[serde(deserialize_with = "lenient_string")]
    pub traceability_powder_batch: String,
    #[serde(deserialize_with = "lenient_string")]
    pub traceability_supplier: String,
    #[serde(deserialize_with = "lenient_string")]
    pub traceability_certificate: String,
    #[serde(deserialize_with = "lenient_string")]
    pub traceability_env_temp: String,
    #[serde(deserialize_with = "lenient_string")]
    pub traceability_env_humidity: String,
    #[serde(deserialize_with = "lenient_string")]
    pub notes: String,
    #[serde(deserialize_with = "lenient_string")]
    pub ordem_servico: String,
    #[serde(deserialize_with = "lenient_string")]
    pub powder_name: String,
    /// Versão do app que gravou o JSON (`"11.3.2"` nas fixtures golden);
    /// ausente do JSON quando `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
}

impl Default for Params {
    /// Defaults da UI Python (o `BASE` das fixtures golden), com
    /// `taxa_de_deposicao`, `oscillation_type` e `app_version` ausentes.
    fn default() -> Self {
        Params {
            diametro: 50.0,
            diametro_interno: 0.0,
            comprimento_revestir: 50.0,
            largura_revestir: 100.0,
            largura_cordao: 8.0,
            sobreposicao: 40.0,
            velocidade_de_deposicao: 120.0,
            taxa_de_deposicao: None,
            velocidade_a_mm_min: 120.0,
            velocidade_oscilacao_mm_min: 3000.0,
            afastamento_tocha: 12.0,
            afastamento_seguranca: 30.0,
            torch_ignite_delay_s: 3.0,
            torch_extinguish_delay_s: 2.0,
            overlap_angular_deg: 10.0,
            bead_width_calibration_factor: 1.0,
            lead_extras_enabled: false,
            lead_in: 0.0,
            lead_out: 0.0,
            direcao_soldagem: Direction::EsquerdaDireita,
            sentido_rotacao: Rotation::Horaria,
            corrente_arco: 180.0,
            tensao_arco: 25.0,
            vazao_gas: 15.0,
            alim_po: 50.0,
            preaquecimento: 100.0,
            eficiencia_termica: 0.6, // plasma (PTA): EN 1011-1 e TWI
            taxa_deposicao_g_h: 700.0,
            num_camadas: 1,
            espessura_camada: 1.5,
            powder_factor: 0.16,
            densidade_po_g_cm3: 8.46,
            rendimento_deposicao_perc: 87.0,
            powder_cost_brl_kg: 450.0,
            torch_retract_on_ignite: true,
            spiral_close_ends: false,
            fechamento_uniforme: false,
            compensar_rotacao: false,
            defasar_camadas: false,
            tempo_inverso_g93: false,
            limite_rpm_placa: 20.0,
            tempo_setup_min: 0.0,
            largura_cordao_min: 0.0,
            largura_cordao_max: 0.0,
            flat_single_direction: false,
            flat_orientation: FlatOrientation::X,
            operacao_interna: false,
            internal_face_clearance_x: 30.0,
            multiperfil_count: 4,
            multiperfil_total_angle_deg: 360.0,
            multiperfil_pitch_mm: 12.0,
            multiperfil_strategy: MultiperfilStrategy::PassesSeparados,
            multiperfil_point_by_point: false,
            multiperfil_profile_shape: ProfileShape::Retangular,
            multiperfil_profile_base_mm: 8.0,
            multiperfil_profile_top_mm: 4.0,
            multiperfil_profile_height_mm: 1.5,
            multiperfil_end_chamfer_enabled: false,
            multiperfil_end_chamfer_angle_deg: 45.0,
            bolacha_tipo: BolachaTipo::EspiralContinua,
            bolacha_stop_between: false,
            bolacha_oscilar: false,
            bolacha_osc_forma: BolachaOscForma::Triangular,
            bolacha_osc_eixo: BolachaOscEixo::X,
            bolacha_osc_sentido: BolachaOscSentido::ParaFora,
            bolacha_osc_periodo_mm: 10.0,
            bolacha_osc_fase_camada_perc: 0.0,
            bolacha_osc_espera_s: 0.0,
            bolacha_osc_pontos_ciclo: 12.0,
            bolacha_osc_escada: false,
            bolacha_zero: BolachaZero::Centro,
            oscilacao_comprimento: 5.0,
            deslocamento_angular_perc: 20.0,
            osc_test_gran_x: 0.5,
            osc_test_gran_a: 1.0,
            smart_layers: false,
            root_feed_scale: 100.0,
            nome_procedimento: "Novo Procedimento".to_string(),
            welding_mode: WeldingMode::Espiral,
            oscillation_type: None,
            traceability_operator: String::new(),
            traceability_eps: String::new(),
            traceability_powder_batch: String::new(),
            traceability_supplier: String::new(),
            traceability_certificate: String::new(),
            traceability_env_temp: String::new(),
            traceability_env_humidity: String::new(),
            notes: String::new(),
            ordem_servico: String::new(),
            powder_name: String::new(),
            app_version: None,
        }
    }
}

impl Params {
    /// Porta de `GCodeGenerator._normalize_params`: espelha
    /// `taxa_de_deposicao` em `velocidade_de_deposicao` quando ausente e
    /// preenche `velocidade_a_mm_min` (0.0 = não informado) com a velocidade
    /// de deposição.
    pub fn normalize(&mut self) {
        if self.taxa_de_deposicao.is_none() {
            self.taxa_de_deposicao = Some(self.velocidade_de_deposicao);
        }
        if self.velocidade_a_mm_min == 0.0 {
            self.velocidade_a_mm_min = self.velocidade_de_deposicao;
        }
    }

    /// Taxa de deposição efetiva: `taxa_de_deposicao` ou, ausente,
    /// `velocidade_de_deposicao`.
    pub fn taxa(&self) -> f64 {
        self.taxa_de_deposicao
            .unwrap_or(self.velocidade_de_deposicao)
    }

    /// `bool(params.get('operacao_interna', False))`.
    pub fn is_internal(&self) -> bool {
        self.operacao_interna
    }

    /// Folga em X até a face interna: `internal_face_clearance_x or 30.0`
    /// (zero conta como não informado, igual ao Python).
    pub fn face_clearance(&self) -> f64 {
        if self.internal_face_clearance_x == 0.0 {
            30.0
        } else {
            self.internal_face_clearance_x
        }
    }

    /// Quanto o zero radial da Bolacha está afastado do centro (mm): o raio
    /// interno com o zero no diâmetro interno, senão zero.
    pub fn bolacha_zero_mm(&self) -> f64 {
        let r = self.diametro_interno / 2.0;
        if self.welding_mode == WeldingMode::Bolacha
            && self.bolacha_zero == BolachaZero::DiametroInterno
            && r.is_finite()
            && r > 0.0
        {
            r
        } else {
            0.0
        }
    }

    /// Eixo da máquina que faz o raio na Bolacha: Y com a tocha em Y.
    pub fn bolacha_eixo_radial(&self) -> char {
        if self.bolacha_osc_eixo == BolachaOscEixo::Y {
            'Y'
        } else {
            'X'
        }
    }

    /// `1.0 if rotation_dir == 'horaria' else -1.0`.
    pub fn rotation_sign(&self) -> f64 {
        match self.sentido_rotacao {
            Rotation::Horaria => 1.0,
            Rotation::Antihoraria => -1.0,
        }
    }

    /// Grava a operação na forma moderna: `welding_mode` + `oscillation_type`
    /// (`Some` só para as três oscilações; `None` nas demais, como o Python
    /// que só escreve a chave nos modos de oscilação).
    pub fn apply_operation(&mut self, op: Operation) {
        let (mode, osc) = op.mode();
        self.welding_mode = mode;
        self.oscillation_type = osc;
    }

    /// Porta exata de `_reset_mode_defaults` (TFM_GCODE.py): restaura os
    /// campos específicos do modo escolhido ao padrão da UI e, para qualquer
    /// operação, `bead_width_calibration_factor = 1.0`. Os demais campos não
    /// são tocados.
    pub fn reset_mode_defaults(&mut self, op: Operation) {
        let d = Params::default();
        match op {
            Operation::Bolacha => {
                self.diametro_interno = d.diametro_interno;
                self.bolacha_tipo = d.bolacha_tipo;
                self.bolacha_stop_between = d.bolacha_stop_between;
                self.bolacha_oscilar = d.bolacha_oscilar;
                self.bolacha_osc_forma = d.bolacha_osc_forma;
                self.bolacha_osc_eixo = d.bolacha_osc_eixo;
                self.bolacha_osc_sentido = d.bolacha_osc_sentido;
                self.bolacha_osc_periodo_mm = d.bolacha_osc_periodo_mm;
                self.bolacha_osc_fase_camada_perc = d.bolacha_osc_fase_camada_perc;
                self.bolacha_osc_espera_s = d.bolacha_osc_espera_s;
                self.bolacha_osc_pontos_ciclo = d.bolacha_osc_pontos_ciclo;
                self.bolacha_osc_escada = d.bolacha_osc_escada;
                self.bolacha_zero = d.bolacha_zero;
                self.oscilacao_comprimento = d.oscilacao_comprimento;
                self.overlap_angular_deg = d.overlap_angular_deg;
            }
            Operation::ChapaPlana => {
                self.largura_revestir = d.largura_revestir;
                self.flat_orientation = d.flat_orientation;
                self.flat_single_direction = d.flat_single_direction;
            }
            Operation::EspiralMultiperfil => {
                self.multiperfil_count = d.multiperfil_count;
                self.multiperfil_total_angle_deg = d.multiperfil_total_angle_deg;
                self.multiperfil_pitch_mm = d.multiperfil_pitch_mm;
                self.multiperfil_strategy = d.multiperfil_strategy;
                self.multiperfil_point_by_point = d.multiperfil_point_by_point;
                self.multiperfil_profile_shape = d.multiperfil_profile_shape;
                self.multiperfil_profile_base_mm = d.multiperfil_profile_base_mm;
                self.multiperfil_profile_top_mm = d.multiperfil_profile_top_mm;
                self.multiperfil_profile_height_mm = d.multiperfil_profile_height_mm;
                self.multiperfil_end_chamfer_enabled = d.multiperfil_end_chamfer_enabled;
                self.multiperfil_end_chamfer_angle_deg = d.multiperfil_end_chamfer_angle_deg;
                self.fechamento_uniforme = d.fechamento_uniforme;
            }
            Operation::Espiral
            | Operation::OscilacaoLinear
            | Operation::OscilacaoQuadrada
            | Operation::OscilacaoQuadradaContinua => {
                self.lead_extras_enabled = d.lead_extras_enabled;
                self.operacao_interna = d.operacao_interna;
                self.spiral_close_ends = d.spiral_close_ends;
                self.fechamento_uniforme = d.fechamento_uniforme;
                self.compensar_rotacao = d.compensar_rotacao;
                self.defasar_camadas = d.defasar_camadas;
            }
        }
        self.bead_width_calibration_factor = d.bead_width_calibration_factor;
    }
}

// ---------------------------------------------------------------------------
// Leitura leniente: a UI Tkinter guardava tudo em StringVar
// ---------------------------------------------------------------------------

/// Erro de conversão leniente de um valor JSON.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LenientError {
    #[error("valor numérico inválido: {0:?}")]
    InvalidNumber(String),
    #[error("valor booleano inválido: {0:?}")]
    InvalidBool(String),
    #[error("tipo JSON inesperado ({0}); esperado {1}")]
    UnexpectedType(&'static str, &'static str),
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// `"12.5"`, `"12,5"`, `" 7 "` -> número; `""` -> 0.0. Igual a `float(x or 0)`.
fn parse_f64_text(s: &str) -> Result<f64, LenientError> {
    let t = s.trim();
    if t.is_empty() {
        return Ok(0.0);
    }
    t.replace(',', ".")
        .parse::<f64>()
        .map_err(|_| LenientError::InvalidNumber(s.to_string()))
}

/// Converte um valor JSON em `f64`: número, string numérica (vírgula ou
/// ponto), `""`/`null` -> 0.0, bool -> 0.0/1.0.
pub fn f64_from_value(v: &Value) -> Result<f64, LenientError> {
    match v {
        Value::Null => Ok(0.0),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::Number(n) => n
            .as_f64()
            .ok_or_else(|| LenientError::InvalidNumber(n.to_string())),
        Value::String(s) => parse_f64_text(s),
        other => Err(LenientError::UnexpectedType(kind(other), "número")),
    }
}

/// Como [`f64_from_value`], mas `null` e `""` viram `None`.
pub fn opt_f64_from_value(v: &Value) -> Result<Option<f64>, LenientError> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) if s.trim().is_empty() => Ok(None),
        other => f64_from_value(other).map(Some),
    }
}

/// Converte um valor JSON em `i64`: inteiro, float inteiro (`4.0`), string
/// (`"3"`, `"4.0"`), `""`/`null` -> 0, bool -> 0/1. Floats são truncados.
pub fn i64_from_value(v: &Value) -> Result<i64, LenientError> {
    match v {
        Value::Null => Ok(0),
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f.trunc() as i64))
            .ok_or_else(|| LenientError::InvalidNumber(n.to_string())),
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                Ok(0)
            } else if let Ok(i) = t.parse::<i64>() {
                Ok(i)
            } else {
                parse_f64_text(t).map(|f| f.trunc() as i64)
            }
        }
        other => Err(LenientError::UnexpectedType(kind(other), "inteiro")),
    }
}

/// Converte um valor JSON em `bool`: bool, número (`!= 0`), strings
/// `"true"/"false"`, `"1"/"0"`, `"sim"/"não"`, `"yes"/"no"`, `"on"/"off"`
/// (sem distinguir maiúsculas), `""`/`null` -> false.
pub fn bool_from_value(v: &Value) -> Result<bool, LenientError> {
    match v {
        Value::Null => Ok(false),
        Value::Bool(b) => Ok(*b),
        Value::Number(n) => Ok(n.as_f64().is_some_and(|f| f != 0.0)),
        Value::String(s) => {
            let t = s.trim().to_lowercase();
            match t.as_str() {
                "" | "0" | "false" | "f" | "no" | "n" | "nao" | "não" | "off" => Ok(false),
                "1" | "true" | "t" | "yes" | "y" | "sim" | "s" | "on" => Ok(true),
                _ => parse_f64_text(&t)
                    .map(|f| f != 0.0)
                    .map_err(|_| LenientError::InvalidBool(s.to_string())),
            }
        }
        other => Err(LenientError::UnexpectedType(kind(other), "booleano")),
    }
}

/// Converte um valor JSON em `String`: `null` -> `""`, número/bool -> texto.
pub fn string_from_value(v: &Value) -> Result<String, LenientError> {
    match v {
        Value::Null => Ok(String::new()),
        Value::String(s) => Ok(s.clone()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        other => Err(LenientError::UnexpectedType(kind(other), "texto")),
    }
}

/// `deserialize_with` para `f64`: número, `"12.5"`, `"12,5"`; `""` -> 0.0.
pub fn lenient_f64<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    f64_from_value(&Value::deserialize(d)?).map_err(de::Error::custom)
}

/// `deserialize_with` para `Option<f64>`: `null`/`""` -> `None`.
pub fn lenient_opt_f64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    opt_f64_from_value(&Value::deserialize(d)?).map_err(de::Error::custom)
}

/// `deserialize_with` para `i64`: inteiro, float inteiro, `"3"`; `""` -> 0.
pub fn lenient_i64<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    i64_from_value(&Value::deserialize(d)?).map_err(de::Error::custom)
}

/// `deserialize_with` para `bool`: bool, `0`/`1`, `"true"`/`"false"`.
pub fn lenient_bool<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    bool_from_value(&Value::deserialize(d)?).map_err(de::Error::custom)
}

/// `deserialize_with` para `String`: `null` -> `""`, número -> texto.
pub fn lenient_string<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    string_from_value(&Value::deserialize(d)?).map_err(de::Error::custom)
}
