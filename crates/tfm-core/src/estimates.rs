//! Resultados calculados, sobreposição efetiva e estimativa de custos.
//!
//! Porta das rotinas da UI Python (`TFM_GCODE.py`) que preenchem os painéis
//! "Resultados", "Sobreposição efetiva" e "Custos":
//!
//! * [`compute`] = `executar_calculos_e_desenho` (ramos espiral,
//!   espiral_multiperfil, chapa_plana, oscilação e bolacha; flags de limite
//!   de máquina; tempo arredondado para cima ao minuto; overrides das
//!   fórmulas do `config.json`; custos) + `_update_overlap_results` +
//!   `_update_quality_metrics` (aporte térmico e diluição);
//! * [`multiperfil_pass_plan`] = `_get_multiperfil_profile_pass_plan`.
//!
//! Os textos reproduzem as f-strings do Python caractere a caractere; o que o
//! Python expressava pintando o rótulo de vermelho vira
//! [`ResultText::over_limit`]. Os custos saem como números
//! ([`CostSummary`]); quem formata (`f"{symbol} {valor:.2f}"`,
//! `f"{kg:.3f} kg"`, `f"{m3:.3f} m³"`) é o painel.
//!
//! Nada aqui entra em pânico: as divisões são guardadas como no Python
//! (`x / y if y > 0 else 0`), inteiros vindos do usuário nunca são
//! multiplicados como inteiros e valores não finitos saem como `0.0` nos
//! campos de texto. [`compute`] assume parâmetros já validados pela UI (o
//! Python só chega a `executar_calculos_e_desenho` com `params` válidos; os
//! placeholders `"0.00"`/`"..."` de `limpar_resultados` são responsabilidade
//! do painel). Como no gerador de G-code, `lead_in`/`lead_out` e
//! `powder_factor`/`powder_cost_brl_kg` são lidos tal como estão em
//! [`Params`]: o `_get_current_params` do Python já zerava os leads com
//! `lead_extras_enabled` desligado e resolvia o pó ativo antes de chegar aqui.

use std::f64::consts::PI;

use indexmap::IndexMap;

use crate::config::{AppConfig, MachineLimits};
use crate::deposit::MAX_FEED_FACTOR;
use crate::formula::{evaluate_block, formula_locals};
use crate::gcode::bolacha_osc;
use crate::gcode::common::{
    effective_base_diameter, overlap_metrics, profile_shape_str, py_max, py_min, py_or,
    rotary_arc_bounds,
};
use crate::gcode::multiperfil::layer_profile_widths;
use crate::gcode::oscillation::oscillation_closing;
use crate::overlap::{uniform_closing, OverlapMetrics, UniformClosing, Uniformity};
use crate::params::{BolachaTipo, OscillationType, Params, WeldingMode};
use crate::quality::QualityManager;

// ---------------------------------------------------------------------------
// Tipos públicos
// ---------------------------------------------------------------------------

/// Texto de um resultado e o aviso de limite de máquina excedido (o Python
/// pintava o rótulo de vermelho: RPM acima de `max_rpm_chuck` para a rotação,
/// curso em X acima de `max_length_x` para o passo).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResultText {
    pub text: String,
    pub over_limit: bool,
}

/// Os quatro textos de `_update_overlap_results` e o nível de uniformidade
/// que o Python exprimia pela cor (`Boa` azul, `Atencao` âmbar, `Critica`
/// vermelho).
#[derive(Clone, Debug, PartialEq)]
pub struct OverlapResults {
    /// `effective_bead_width`: `"{:.2}"` ou `"{:.2} - {:.2}"` (primeira e
    /// última camada); multiperfil: média das camadas.
    pub effective_bead_width: String,
    /// `surface_step`: idem para o passo lateral.
    pub surface_step: String,
    /// `coverage_estimate`: `"{:.1}% | curva {:.2} mm"`; multiperfil:
    /// `"{alvo:.2}/{cobertura:.2} mm | {sobreposição:.1}%"`.
    pub coverage: String,
    /// `uniformity_estimate`: `"{label} | var {:.1}%"`; multiperfil:
    /// `"{label} | erro max {:.2} mm"`.
    pub uniformity: String,
    pub uniformity_level: Uniformity,
}

impl Default for OverlapResults {
    /// Estado inicial de `_update_overlap_results`: `"-"` em azul.
    fn default() -> Self {
        OverlapResults {
            effective_bead_width: "-".to_string(),
            surface_step: "-".to_string(),
            coverage: "-".to_string(),
            uniformity: "-".to_string(),
            uniformity_level: Uniformity::Boa,
        }
    }
}

/// Custos e consumos em valores numéricos (o painel formata).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CostSummary {
    /// Pó + gás.
    pub consumiveis: f64,
    /// Mão de obra + máquina.
    pub operacional: f64,
    /// Consumíveis + operacional, salvo override `total_cost_brl`.
    pub total: f64,
    /// Consumo de pó (kg).
    pub powder_kg: f64,
    /// Consumo de gás (m³): `vazao_gas * 60 / 1000 * horas`.
    pub gas_m3: f64,
    pub powder_cost: f64,
    pub gas_cost: f64,
    pub labor_cost: f64,
    pub machine_cost: f64,
    /// `costs.currency_symbol`, ou `"$"` quando vazio
    /// (`.get('currency_symbol', '$')`).
    pub currency: String,
}

/// Tudo o que os painéis de resultados mostram para um conjunto de parâmetros.
#[derive(Clone, Debug, PartialEq)]
pub struct Estimates {
    /// `rotation_rpm`: minutos por volta, `"{:.3}"` (bolacha:
    /// `"{ext:.3} - {int:.3}"` quando diferem). `over_limit` = RPM acima de
    /// `max_rpm_chuck`.
    pub rotation: ResultText,
    /// `helix_pitch`: `"{:.3} (axial)"` / `"(fixo)"` / `"(lat)"` / `"(rad)"`.
    /// `over_limit` = curso em X (`h + leads`, ou `r_ext` na bolacha) acima
    /// de `max_length_x`. A chapa plana não valida limites.
    pub pitch: ResultText,
    /// `total_rotations`.
    pub rotations: String,
    /// `total_angle_A`.
    pub total_angle: String,
    /// `estimated_time`: `"{hh:02}:{mm:02}"` com os minutos arredondados
    /// para cima.
    pub time_hhmm: String,
    /// Tempo total em horas (antes do arredondamento), base dos custos.
    pub time_hours: f64,
    pub overlap: OverlapResults,
    pub costs: CostSummary,
    /// Saída crua de [`evaluate_block`] sobre `config.formulas.expressions`
    /// (`None` = fórmula falhou ou ficou pendente).
    pub formulas: IndexMap<String, Option<f64>>,
    /// `QualityManager::heat_input_kj_mm(tensao, corrente, velocidade, eficiencia)`.
    pub heat_input_kj_mm: f64,
    /// `QualityManager::estimate_dilution(corrente, taxa_deposicao_g_h / 60, velocidade)`.
    pub dilution_pct: f64,
}

/// Uma camada do plano de passes do multiperfil
/// (`_get_multiperfil_profile_pass_plan`).
#[derive(Clone, Debug, PartialEq)]
pub struct PassPlanItem {
    /// Largura alvo do perfil nesta camada.
    pub width: f64,
    /// Largura efetivamente coberta pelos passes (`effective_cover_width_mm`).
    pub effective_width: f64,
    /// Largura efetiva de um cordão (`effective_width_mm`).
    pub bead_effective_width: f64,
    /// Largura do cordão sobre a superfície curva (`surface_width_mm`).
    pub surface_width: f64,
    pub step_over_mm: f64,
    pub pass_count: usize,
    pub offsets: Vec<f64>,
    pub coverage_ratio: f64,
    pub fill_error_mm: f64,
    pub uniformity: Uniformity,
    pub achieved_overlap_pct: f64,
}

// ---------------------------------------------------------------------------
// API
// ---------------------------------------------------------------------------

/// Porta de `executar_calculos_e_desenho` + `_update_overlap_results` +
/// `_update_quality_metrics` para parâmetros válidos.
///
/// Ordem do Python: ramo do modo (textos, flags de limite, minutos totais e
/// pó), sobreposição efetiva, fórmulas; override do pó (`powder_mass_kg`,
/// senão `powder_mass_g`, só valores `>= 0`); fallback do tempo **apenas**
/// quando o modo não produziu tempo (`tempo_total_horas <= 0`): pela massa e
/// taxa de deposição (UI, senão `taxa_deposicao_kg_h`/`taxa_deposicao_g_h`
/// das fórmulas), senão `time_min_mass`, senão `time_min`; custos com os
/// overrides `powder_cost_brl`, `gas_cost_brl`, `labor_cost_brl`,
/// `machine_cost_brl` e `total_cost_brl` (qualquer número, como o
/// `isinstance(x, (int, float))` do Python).
pub fn compute(p: &Params, cfg: &AppConfig) -> Estimates {
    let shared = Shared::new(p, cfg);
    let mode = match p.welding_mode {
        WeldingMode::Espiral => espiral(p, &shared),
        WeldingMode::EspiralMultiperfil => multiperfil(p, &shared),
        WeldingMode::ChapaPlana => chapa_plana(p, &shared),
        WeldingMode::Oscilacao | WeldingMode::OscilacaoLinear | WeldingMode::OscilacaoQuadrada => {
            oscilacao(p, &shared)
        }
        WeldingMode::Bolacha => bolacha(p, &shared),
    };

    let overlap = overlap_results(p);

    let formulas = evaluate_block(&cfg.formulas.expressions, &formula_locals(p, &cfg.costs));
    let num = |key: &str| formulas.get(key).copied().flatten();

    // Override do consumo de pó
    let mut consumo_po_kg = mode.consumo_po_kg;
    if let Some(kg) = num("powder_mass_kg").filter(|v| *v >= 0.0) {
        consumo_po_kg = kg;
    } else if let Some(g) = num("powder_mass_g").filter(|v| *v >= 0.0) {
        consumo_po_kg = g / 1000.0;
    }

    // Tempo: o do modo manda; fórmulas só como fallback quando ele é zero
    let mut time_hhmm = hhmm(mode.total_minutos);
    let mut tempo_total_horas = mode.total_minutos / 60.0;
    if tempo_total_horas <= 0.0 {
        let taxa_kg_h = if p.taxa_deposicao_g_h > 0.0 {
            Some(p.taxa_deposicao_g_h / 1000.0)
        } else if let Some(kg_h) = num("taxa_deposicao_kg_h").filter(|v| *v > 0.0) {
            Some(kg_h)
        } else {
            num("taxa_deposicao_g_h")
                .filter(|v| *v > 0.0)
                .map(|g_h| g_h / 1000.0)
        };
        let time_min_override = if let Some(taxa) = taxa_kg_h.filter(|v| *v > 0.0) {
            Some((consumo_po_kg / py_max(taxa, 1e-6)) * 60.0)
        } else if let Some(m) = num("time_min_mass").filter(|v| *v >= 0.0) {
            Some(m)
        } else {
            num("time_min").filter(|v| *v >= 0.0)
        };
        if let Some(minutes) = time_min_override {
            tempo_total_horas = minutes / 60.0;
            time_hhmm = hhmm(minutes);
        }
    }

    // Custos
    let costs = &cfg.costs;
    let mut custo_po = consumo_po_kg * p.powder_cost_brl_kg;
    let consumo_gas_m3_hora = p.vazao_gas * 60.0 / 1000.0;
    let mut custo_gas = consumo_gas_m3_hora * tempo_total_horas * costs.gas_argon_brl_m3;
    let mut custo_consumiveis = custo_po + custo_gas;
    if let Some(v) = num("powder_cost_brl") {
        custo_po = v;
        custo_consumiveis = custo_po + custo_gas;
    }
    if let Some(v) = num("gas_cost_brl") {
        custo_gas = v;
        custo_consumiveis = custo_po + custo_gas;
    }
    let labor_cost = num("labor_cost_brl").unwrap_or(tempo_total_horas * costs.labor_brl_hour);
    let machine_cost =
        num("machine_cost_brl").unwrap_or(tempo_total_horas * costs.machine_brl_hour);
    let custo_operacional = labor_cost + machine_cost;
    let custo_total = num("total_cost_brl").unwrap_or(custo_consumiveis + custo_operacional);
    let consumo_gas_m3 = consumo_gas_m3_hora * tempo_total_horas;
    let currency = if costs.currency_symbol.is_empty() {
        "$".to_string()
    } else {
        costs.currency_symbol.clone()
    };

    Estimates {
        rotation: mode.rotation,
        pitch: mode.pitch,
        rotations: mode.rotations,
        total_angle: mode.total_angle,
        time_hhmm,
        time_hours: tempo_total_horas,
        overlap,
        costs: CostSummary {
            consumiveis: custo_consumiveis,
            operacional: custo_operacional,
            total: custo_total,
            powder_kg: consumo_po_kg,
            gas_m3: consumo_gas_m3,
            powder_cost: custo_po,
            gas_cost: custo_gas,
            labor_cost,
            machine_cost,
            currency,
        },
        formulas,
        heat_input_kj_mm: QualityManager::heat_input_kj_mm(
            p.tensao_arco,
            p.corrente_arco,
            p.velocidade_de_deposicao,
            p.eficiencia_termica,
        ),
        dilution_pct: QualityManager::estimate_dilution(
            p.corrente_arco,
            p.taxa_deposicao_g_h / 60.0,
            p.velocidade_de_deposicao,
        ),
    }
}

/// Porta de `_get_multiperfil_profile_pass_plan`: uma entrada por camada do
/// perfil ([`layer_profile_widths`]), com as métricas de sobreposição da
/// camada para a largura alvo (`force_min_two_passes = True`).
pub fn multiperfil_pass_plan(p: &Params) -> Vec<PassPlanItem> {
    layer_profile_widths(p)
        .into_iter()
        .enumerate()
        .map(|(layer_idx, width)| {
            let target_width = py_max(py_or(width, 0.0), 0.0);
            let m = overlap_metrics(p, layer_idx as i64, None, Some(target_width));
            PassPlanItem {
                width,
                effective_width: m.effective_cover_width_mm,
                bead_effective_width: m.bead.effective_width_mm,
                surface_width: m.bead.surface_width_mm,
                step_over_mm: m.step_over_mm,
                pass_count: m.pass_count,
                offsets: m.offsets,
                coverage_ratio: m.coverage_ratio,
                fill_error_mm: m.fill_error_mm,
                uniformity: m.uniformity,
                achieved_overlap_pct: m.achieved_overlap_pct,
            }
        })
        .collect()
}

/// `f"{hh:02d}:{mm:02d}"` com `minutos = int(math.ceil(total_minutos))`.
/// Valores não finitos (que no Python derrubariam o cálculo) viram `00:00`.
pub fn hhmm(total_minutos: f64) -> String {
    let minutos = if total_minutos.is_finite() {
        total_minutos.ceil() as i64
    } else {
        0
    };
    format!(
        "{:02}:{:02}",
        minutos.div_euclid(60),
        minutos.rem_euclid(60)
    )
}

// ---------------------------------------------------------------------------
// Sobreposição efetiva (`_update_overlap_results`)
// ---------------------------------------------------------------------------

fn overlap_results(p: &Params) -> OverlapResults {
    if p.welding_mode == WeldingMode::EspiralMultiperfil {
        return multiperfil_overlap_results(p);
    }
    let layer_count = layer_count(p);
    let mut first = overlap_metrics(p, 0, None, None);
    let mut last = overlap_metrics(p, layer_count - 1, None, None);
    // Fechamento uniforme: o passo que sai no G-code é o redistribuído, então
    // é ele que dá a sobreposição real.
    if p.fechamento_uniforme {
        match p.welding_mode {
            WeldingMode::Espiral => {
                for (m, layer) in [(&mut first, 0), (&mut last, layer_count - 1)] {
                    let passo = closing_plan(p, layer, m.step_over_mm).passo;
                    let largura = m.bead.effective_width_mm;
                    set_real_step(m, passo, largura);
                }
            }
            WeldingMode::Oscilacao
            | WeldingMode::OscilacaoLinear
            | WeldingMode::OscilacaoQuadrada => {
                let c = oscillation_closing(p);
                for m in [&mut first, &mut last] {
                    set_real_step(m, c.passo, c.band_width);
                }
            }
            _ => {}
        }
    }
    let (first_eff, last_eff) = (first.bead.effective_width_mm, last.bead.effective_width_mm);
    let eff_min = py_min(first_eff, last_eff);
    let eff_max = py_max(first_eff, last_eff);
    let step_min = py_min(first.step_over_mm, last.step_over_mm);
    let step_max = py_max(first.step_over_mm, last.step_over_mm);
    let coverage = format!(
        "{}% | curva {} mm",
        txt1(first.achieved_overlap_pct),
        txt2(first.bead.surface_width_mm)
    );
    let variation_pct =
        (last.step_over_mm - first.step_over_mm).abs() / py_max(first.step_over_mm, 1e-6) * 100.0;
    let level = if variation_pct <= 3.0 {
        Uniformity::Boa
    } else if variation_pct <= 10.0 {
        Uniformity::Atencao
    } else {
        Uniformity::Critica
    };
    OverlapResults {
        effective_bead_width: range_text(eff_min, eff_max),
        surface_step: range_text(step_min, step_max),
        coverage,
        uniformity: format!("{} | var {}%", level.label(), txt1(variation_pct)),
        uniformity_level: level,
    }
}

/// Grava em `m` o passo que sai no G-code e a sobreposição que ele dá sobre
/// a largura de material `largura`.
fn set_real_step(m: &mut OverlapMetrics, passo: f64, largura: f64) {
    m.step_over_mm = passo;
    m.achieved_overlap_pct = (1.0 - passo / py_max(largura, 1e-6)).clamp(0.0, 0.99) * 100.0;
}

/// Plano do fechamento uniforme da camada `layer` com o passo `passo`.
fn closing_plan(p: &Params, layer: i64, passo: f64) -> UniformClosing {
    let bounds = rotary_arc_bounds(p);
    let largura = overlap_metrics(p, layer, None, None)
        .bead
        .effective_width_mm;
    uniform_closing(bounds.x_start, bounds.x_end, passo, largura)
}

/// `f"{lo:.2f}"` quando `abs(hi - lo) <= 0.05`, senão `f"{lo:.2f} - {hi:.2f}"`.
fn range_text(lo: f64, hi: f64) -> String {
    if (hi - lo).abs() <= 0.05 {
        txt2(lo)
    } else {
        format!("{} - {}", txt2(lo), txt2(hi))
    }
}

fn multiperfil_overlap_results(p: &Params) -> OverlapResults {
    let plan = multiperfil_pass_plan(p);
    if plan.is_empty() {
        return OverlapResults::default();
    }
    let n = plan.len() as f64;
    let mean = |f: fn(&PassPlanItem) -> f64| plan.iter().map(f).sum::<f64>() / n;
    let avg_bead = mean(|i| i.bead_effective_width);
    let avg_step = mean(|i| i.step_over_mm);
    let avg_target_width = mean(|i| i.width);
    let avg_cover_width = mean(|i| i.effective_width);
    let avg_overlap = mean(|i| i.achieved_overlap_pct);
    // max(..., key=severity_rank): Boa < Atencao < Critica
    let worst = plan
        .iter()
        .map(|i| i.uniformity)
        .max_by_key(|u| severity(*u))
        .unwrap_or(Uniformity::Boa);
    // max(abs(fill_error)) com a semântica do builtin (o primeiro vence em
    // empate e NaN não substitui)
    let mut fill_errors = plan.iter().map(|i| i.fill_error_mm.abs());
    let first_fill = fill_errors.next().unwrap_or(0.0);
    let max_fill_error = fill_errors.fold(first_fill, py_max);
    OverlapResults {
        effective_bead_width: txt2(avg_bead),
        surface_step: txt2(avg_step),
        coverage: format!(
            "{}/{} mm | {}%",
            txt2(avg_target_width),
            txt2(avg_cover_width),
            txt1(avg_overlap)
        ),
        uniformity: format!("{} | erro max {} mm", worst.label(), txt2(max_fill_error)),
        uniformity_level: worst,
    }
}

fn severity(u: Uniformity) -> u8 {
    match u {
        Uniformity::Boa => 0,
        Uniformity::Atencao => 1,
        Uniformity::Critica => 2,
    }
}

// ---------------------------------------------------------------------------
// Ramos de `executar_calculos_e_desenho`
// ---------------------------------------------------------------------------

/// Valores comuns a todos os ramos, calculados antes do `if welding_mode`.
struct Shared {
    /// `math.pi * diameter`, com `diameter = max(diâmetro efetivo, 1e-6)`.
    circunferencia: f64,
    /// `comprimento_revestir`.
    h: f64,
    /// `powder_factor` (g/mm²).
    peso: f64,
    lead_in: f64,
    lead_out: f64,
    larg_cordao: f64,
    sobreposicao: f64,
    /// `taxa_de_deposicao` (espelho de `velocidade_de_deposicao`).
    taxa: f64,
    /// `params['num_camadas']` cru, como fator das áreas e dos tempos.
    num_camadas: f64,
    limits: MachineLimits,
}

impl Shared {
    fn new(p: &Params, cfg: &AppConfig) -> Shared {
        let diameter = py_max(effective_base_diameter(p), 1e-6);
        Shared {
            circunferencia: PI * diameter,
            h: p.comprimento_revestir,
            peso: p.powder_factor,
            lead_in: p.lead_in,
            lead_out: p.lead_out,
            larg_cordao: p.largura_cordao,
            sobreposicao: p.sobreposicao,
            taxa: p.taxa(),
            num_camadas: p.num_camadas as f64,
            limits: cfg.active_limits(),
        }
    }

    /// `rpm > max_rpm_chuck`.
    fn rpm_over_limit(&self, rpm: f64) -> bool {
        rpm > self.limits.max_rpm_chuck
    }

    /// `(h + lead_in + lead_out) > max_length_x`.
    fn x_travel_over_limit(&self) -> bool {
        (self.h + self.lead_in + self.lead_out) > self.limits.max_length_x
    }
}

/// Saída de um ramo: textos, flags, minutos totais e pó (kg) antes dos
/// overrides das fórmulas.
struct ModeOutcome {
    rotation: ResultText,
    pitch: ResultText,
    rotations: String,
    total_angle: String,
    total_minutos: f64,
    consumo_po_kg: f64,
}

/// `max(int(params.get('num_camadas', 1) or 1), 1)`.
fn layer_count(p: &Params) -> i64 {
    let n = if p.num_camadas == 0 { 1 } else { p.num_camadas };
    n.max(1)
}

/// `(rot_min_por_volta, rpm)`: minutos por volta e voltas por minuto para um
/// comprimento de volta, zero quando não há volta ou velocidade.
fn helix_rates(comprimento_volta: f64, taxa: f64) -> (f64, f64) {
    if comprimento_volta > 0.0 && taxa > 0.0 {
        (comprimento_volta / taxa, taxa / comprimento_volta)
    } else {
        (0.0, 0.0)
    }
}

/// Pó (kg) dos modos rotativos (espiral e oscilação): área das camadas mais
/// a área dos leads quando existem, vezes o fator, dividido por 1000.
fn rotary_powder_kg(s: &Shared) -> f64 {
    let area_camada = s.circunferencia * s.h;
    let area_total = area_camada * s.num_camadas;
    let area_leads = if s.lead_in + s.lead_out > 0.0 {
        s.circunferencia * (s.lead_in + s.lead_out) * s.num_camadas
    } else {
        0.0
    };
    let consumo_po_g = (area_total + area_leads) * s.peso;
    consumo_po_g / 1000.0
}

/// Não finito -> 0.0 (só nos campos de texto).
fn finite(v: f64) -> f64 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

fn txt0(v: f64) -> String {
    format!("{:.0}", finite(v))
}

fn txt1(v: f64) -> String {
    format!("{:.1}", finite(v))
}

fn txt2(v: f64) -> String {
    format!("{:.2}", finite(v))
}

fn txt3(v: f64) -> String {
    format!("{:.3}", finite(v))
}

/// Ramo `espiral`: passo médio do modelo de sobreposição ao longo das
/// camadas; o ângulo total inclui 1 volta no início e 1 no fim (+720°); o
/// tempo usa o comprimento real da hélice + leads + 2 voltas, vezes as camadas.
fn espiral(p: &Params, s: &Shared) -> ModeOutcome {
    let layer_count = layer_count(p);
    let steps: Vec<f64> = (0..layer_count)
        .map(|i| overlap_metrics(p, i, None, None).step_over_mm)
        .collect();
    let passo = steps.iter().sum::<f64>() / (steps.len().max(1) as f64);
    let passo = py_max(passo, 1e-6);
    let total_rotacoes_part = if s.h > 0.0 { s.h / passo } else { 0.0 };
    // Fechamento uniforme: voltas inteiras no curso do centro do cordão, como
    // no G-code gerado (esse curso já conta os leads).
    let plano = (p.fechamento_uniforme && s.h > 0.0).then(|| closing_plan(p, 0, passo));
    let (passo, total_rotacoes_part) =
        plano.map_or((passo, total_rotacoes_part), |c| (c.passo, c.voltas));
    let leads = if plano.is_some() {
        0.0
    } else {
        s.lead_in + s.lead_out
    };
    let angulo_part = total_rotacoes_part * 360.0 + 720.0;
    let comprimento_helice_por_volta = (s.circunferencia.powi(2) + passo.powi(2)).sqrt();
    let comprimento_total_helice = total_rotacoes_part * comprimento_helice_por_volta;
    let comprimento_total_mov = comprimento_total_helice + leads + (2.0 * s.circunferencia);
    let (rot_min_por_volta, rpm_check) = helix_rates(comprimento_helice_por_volta, s.taxa);
    // A compensação pode acelerar até o teto: é ele que vale contra a máquina.
    let rpm_factor = if p.compensar_rotacao {
        MAX_FEED_FACTOR
    } else {
        1.0
    };
    let tempo_min_arc = if s.taxa > 0.0 {
        comprimento_total_mov / s.taxa
    } else {
        0.0
    };
    ModeOutcome {
        rotation: ResultText {
            text: txt3(rot_min_por_volta),
            over_limit: s.rpm_over_limit(rpm_check * rpm_factor),
        },
        pitch: ResultText {
            text: if p.fechamento_uniforme {
                format!("{} (axial, volta inteira)", txt3(passo))
            } else {
                format!("{} (axial)", txt3(passo))
            },
            over_limit: s.x_travel_over_limit(),
        },
        rotations: txt2(total_rotacoes_part),
        total_angle: txt2(angulo_part),
        total_minutos: tempo_min_arc * s.num_camadas,
        consumo_po_kg: rotary_powder_kg(s),
    }
}

/// Ramo `espiral_multiperfil`: passo fixo (`multiperfil_pitch_mm`), uma
/// hélice por subpasse de cada perfil (plano de passes), fechamento opcional
/// de 2 voltas por subpasse; o tempo **não** é multiplicado por `num_camadas`
/// (o plano já cobre todas as camadas).
fn multiperfil(p: &Params, s: &Shared) -> ModeOutcome {
    let perfil_count = {
        let n = if p.multiperfil_count == 0 {
            1
        } else {
            p.multiperfil_count
        };
        n.max(1)
    };
    let setor_total_deg = py_max(py_or(p.multiperfil_total_angle_deg, 360.0), 0.0);
    let passo = py_max(py_or(p.multiperfil_pitch_mm, 0.0), 1e-6);
    let plan = multiperfil_pass_plan(p);
    let total_subpasses_por_perfil: usize = plan.iter().map(|i| i.pass_count).sum();
    let comprimento_total_arc = s.h + s.lead_in + s.lead_out;
    let rotacoes_por_perfil = if comprimento_total_arc > 0.0 {
        comprimento_total_arc / passo
    } else {
        0.0
    };
    let comprimento_helice_por_volta = (s.circunferencia.powi(2) + passo.powi(2)).sqrt();
    let comprimento_helice_perfil = rotacoes_por_perfil * comprimento_helice_por_volta;
    let fechamento_extra = if p.spiral_close_ends {
        2.0 * s.circunferencia
    } else {
        0.0
    };
    // `perfil_count * total_subpasses` é um produto de inteiros no Python;
    // em f64 para nunca estourar com valores absurdos do usuário.
    let subpasses_totais = perfil_count as f64 * total_subpasses_por_perfil as f64;
    let comprimento_total_mov = subpasses_totais * (comprimento_helice_perfil + fechamento_extra);
    let (rot_min_por_volta, rpm_check) = helix_rates(comprimento_helice_por_volta, s.taxa);
    let tempo_min_arc = if s.taxa > 0.0 {
        comprimento_total_mov / s.taxa
    } else {
        0.0
    };
    let passo_angular_deg = if perfil_count <= 1 {
        0.0
    } else if setor_total_deg >= 360.0 {
        setor_total_deg / perfil_count as f64
    } else {
        setor_total_deg / (perfil_count - 1) as f64
    };
    let area_total_perfil =
        comprimento_helice_perfil * s.larg_cordao * total_subpasses_por_perfil as f64;
    let area_total = area_total_perfil * perfil_count as f64;
    let consumo_po_g = area_total * s.peso;
    ModeOutcome {
        rotation: ResultText {
            text: txt3(rot_min_por_volta),
            over_limit: s.rpm_over_limit(rpm_check),
        },
        pitch: ResultText {
            text: format!("{} (fixo)", txt3(passo)),
            over_limit: s.x_travel_over_limit(),
        },
        rotations: format!(
            "{perfil_count} x {total_subpasses_por_perfil} x {}",
            txt2(rotacoes_por_perfil)
        ),
        total_angle: format!(
            "{}° / {}° ({})",
            txt1(setor_total_deg),
            txt1(passo_angular_deg),
            profile_shape_str(p.multiperfil_profile_shape)
        ),
        total_minutos: tempo_min_arc,
        consumo_po_kg: consumo_po_g / 1000.0,
    }
}

/// Ramo `chapa_plana`: passes laterais em Y (`ceil(largura / passo)`), cada
/// um com o comprimento em X + leads; sem rotação e sem validação de limites.
fn chapa_plana(p: &Params, s: &Shared) -> ModeOutcome {
    let passo = py_max(overlap_metrics(p, 0, None, None).step_over_mm, 1e-6);
    let part_width = p.largura_revestir;
    let num_passes_y = (part_width / passo).ceil();
    let len_x = s.h + s.lead_in + s.lead_out;
    let total_len_x = num_passes_y * len_x;
    let total_len_y = py_max(0.0, (num_passes_y - 1.0) * passo);
    let total_length_mm_per_layer = total_len_x + total_len_y;
    let total_length_mm_all_layers = total_length_mm_per_layer * s.num_camadas;
    let tempo_min = total_length_mm_all_layers / py_max(s.taxa, 1e-6);
    let area_camada = part_width * s.h;
    let area_total = area_camada * s.num_camadas;
    let area_leads = part_width * (s.lead_in + s.lead_out) * s.num_camadas;
    let consumo_po_g = (area_total + area_leads) * s.peso;
    ModeOutcome {
        rotation: ResultText {
            text: "0.000".to_string(),
            over_limit: false,
        },
        pitch: ResultText {
            text: format!("{} (lat)", txt3(passo)),
            over_limit: false,
        },
        rotations: "0".to_string(),
        total_angle: "0".to_string(),
        total_minutos: tempo_min,
        consumo_po_kg: consumo_po_g / 1000.0,
    }
}

/// Ramo `oscilacao` (linear, quadrada e quadrada contínua): passos angulares
/// por volta pelo deslocamento angular, ida e volta da oscilação mais a
/// rotação do passo (zero na quadrada contínua, que gira junto com X), passos
/// axiais pelo passo de sobreposição; tempo vezes as camadas, mais os leads.
fn oscilacao(p: &Params, s: &Shared) -> ModeOutcome {
    let comp_osc = p.oscilacao_comprimento;
    let desloc_perc = p.deslocamento_angular_perc;
    let desloc_linear_angular = (desloc_perc / 100.0) * s.larg_cordao;
    let num_passos_angulares_por_volta = if s.circunferencia > 0.0 && desloc_linear_angular > 0.0 {
        (s.circunferencia / desloc_linear_angular).ceil()
    } else {
        1.0
    };
    let actual_delta_a_deg = if num_passos_angulares_por_volta > 0.0 {
        360.0 / num_passos_angulares_por_volta
    } else {
        360.0
    };
    let (rot_min_por_volta, rpm_a) = helix_rates(s.circunferencia, s.taxa);
    let feed_angular = rpm_a * 360.0;
    let tempo_osc_ida_volta = if s.taxa > 0.0 {
        (comp_osc * 2.0) / s.taxa
    } else {
        0.0
    };
    let mut tempo_rotacao_passo = if feed_angular > 0.0 {
        actual_delta_a_deg / feed_angular
    } else {
        0.0
    };
    if p.oscillation_type == Some(OscillationType::QuadradaContinua) {
        tempo_rotacao_passo = 0.0;
    }
    let tempo_por_passo_angular = tempo_osc_ida_volta + tempo_rotacao_passo;
    let tempo_por_volta = tempo_por_passo_angular * num_passos_angulares_por_volta;
    let passo_sobreposicao = py_max(comp_osc * (s.sobreposicao / 100.0), 1e-6);
    let num_passos_axiais = if s.h > 0.0 {
        (s.h / passo_sobreposicao).ceil()
    } else {
        1.0
    };
    let fechar = p.fechamento_uniforme && s.h > 0.0;
    let (passo_sobreposicao, num_passos_axiais) = if fechar {
        let c = oscillation_closing(p);
        (c.passo, c.aneis as f64)
    } else {
        (passo_sobreposicao, num_passos_axiais)
    };
    let tempo_min_total_osc = num_passos_axiais * tempo_por_volta;
    let tempo_leads = if s.taxa > 0.0 {
        (s.lead_in + s.lead_out) / s.taxa
    } else {
        0.0
    };
    let tempo_min_total = tempo_min_total_osc + tempo_leads;
    ModeOutcome {
        rotation: ResultText {
            text: txt3(rot_min_por_volta),
            over_limit: s.rpm_over_limit(rpm_a),
        },
        pitch: ResultText {
            text: if fechar {
                format!("{} (axial, volta inteira)", txt3(passo_sobreposicao))
            } else {
                format!("{} (axial)", txt3(passo_sobreposicao))
            },
            over_limit: s.x_travel_over_limit(),
        },
        rotations: format!("{} (passos axiais)", txt2(num_passos_axiais)),
        total_angle: format!("{}° (passo)", txt2(actual_delta_a_deg)),
        total_minutos: tempo_min_total * s.num_camadas,
        consumo_po_kg: rotary_powder_kg(s),
    }
}

/// Ramo `bolacha`: voltas radiais entre `r_int` (piso: meio cordão) e
/// `r_ext`; anéis somam uma volta extra e `(360 + overlap)/360` por anel, a
/// espiral contínua usa o raio médio; a rotação mostra o intervalo externo -
/// interno quando difere, o RPM máximo (raio interno) valida o limite e o
/// curso em X é o raio externo.
fn bolacha(p: &Params, s: &Shared) -> ModeOutcome {
    let mut passo = py_max(overlap_metrics(p, 0, None, None).step_over_mm, 1e-6);
    let r_ext = p.diametro / 2.0;
    let r_int = py_max(p.diametro_interno / 2.0, s.larg_cordao / 2.0);
    let mut radial_width = py_max(0.0, r_ext - r_int);
    // Com oscilação, o centro do cordão anda numa faixa menor e o passo usa a
    // largura varrida (ver `gcode::bolacha_osc`).
    let osc = p.bolacha_oscilar.then(|| bolacha_osc::faixa(p));
    let (r_centro_int, r_centro_ext) = match &osc {
        Some(f) => {
            passo = f.passo;
            radial_width = f.c_ext - f.c_int;
            (f.c_int, f.c_ext)
        }
        None => (r_int, r_ext),
    };
    let num_voltas = match &osc {
        Some(f) => f.num_voltas as f64,
        None if radial_width > 0.0 => (radial_width / passo).ceil(),
        None => 1.0,
    };
    let overlap_ang = p.overlap_angular_deg;
    let (comprimento_total_mov, total_rotacoes, total_angulo) = match p.bolacha_tipo {
        BolachaTipo::Aneis => {
            let num_voltas_total = num_voltas + 1.0;
            let fator_volta = (360.0 + overlap_ang) / 360.0;
            let soma_raios = (num_voltas_total * r_centro_int)
                + (passo * (num_voltas_total - 1.0) * num_voltas_total / 2.0);
            (
                2.0 * PI * soma_raios * fator_volta,
                num_voltas_total * fator_volta,
                num_voltas_total * (360.0 + overlap_ang),
            )
        }
        BolachaTipo::EspiralContinua => {
            let total_rotacoes = num_voltas + (overlap_ang / 360.0);
            let total_angulo = (num_voltas * 360.0) + overlap_ang;
            (
                2.0 * PI * ((r_centro_int + r_centro_ext) / 2.0) * total_rotacoes,
                total_rotacoes,
                total_angulo,
            )
        }
    };
    // A onda alonga o caminho e a espera nas pontas soma tempo.
    let (fator_onda, espera_s_por_mm) = if osc.is_some() {
        bolacha_osc::weave_cost(p)
    } else {
        (1.0, 0.0)
    };
    let comprimento_total_mov = comprimento_total_mov * fator_onda;
    let tempo_min_total = comprimento_total_mov / py_max(s.taxa, 1e-6)
        + comprimento_total_mov / fator_onda * espera_s_por_mm / 60.0;
    let area_camada = PI * (r_ext.powi(2) - r_int.powi(2));
    let area_total = area_camada * s.num_camadas;
    let consumo_po_g = area_total * s.peso;
    let rot_min_por_volta_ext = if r_ext > 0.0 && s.taxa > 0.0 {
        (2.0 * PI * r_ext) / s.taxa
    } else {
        0.0
    };
    let rot_min_por_volta_int = if r_int > 0.0 && s.taxa > 0.0 {
        (2.0 * PI * r_int) / s.taxa
    } else {
        0.0
    };
    let rpm_max = if r_int > 0.0 && s.taxa > 0.0 {
        s.taxa / (2.0 * PI * r_int)
    } else {
        0.0
    };
    let rotation_text = if (rot_min_por_volta_ext - rot_min_por_volta_int).abs() < 0.001 {
        txt3(rot_min_por_volta_ext)
    } else {
        format!(
            "{} - {}",
            txt3(rot_min_por_volta_ext),
            txt3(rot_min_por_volta_int)
        )
    };
    ModeOutcome {
        rotation: ResultText {
            text: rotation_text,
            over_limit: s.rpm_over_limit(rpm_max),
        },
        pitch: ResultText {
            text: format!("{} (rad)", txt3(passo)),
            over_limit: r_ext > s.limits.max_length_x,
        },
        rotations: txt2(total_rotacoes),
        total_angle: txt0(total_angulo),
        total_minutos: tempo_min_total * s.num_camadas,
        consumo_po_kg: consumo_po_g / 1000.0,
    }
}
