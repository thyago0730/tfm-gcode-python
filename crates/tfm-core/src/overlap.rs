//! Modelo de sobreposição efetiva do cordão.
//!
//! Porta literal de `src/app/core/overlap_model.py`. A ordem das operações em
//! ponto flutuante é preservada para que os resultados sejam idênticos bit a bit
//! aos do Python: o gerador construído sobre este módulo é comparado com a saída
//! Python com 3 casas decimais, e qualquer desvio aqui se propaga para lá.
//!
//! Os auxiliares `py_max`/`py_min`/`py_or` reproduzem a semântica dos builtins
//! `max`/`min` de dois argumentos e do idioma `valor or padrao` do Python, para
//! que casos de borda (NaN, zero com sinal, calibração zero) saiam iguais.

use std::fmt;

/// Estimativa da largura real do cordão depositado.
#[derive(Debug, Clone, PartialEq)]
pub struct BeadEstimate {
    /// Largura nominal informada, com piso de 1e-6 mm.
    pub nominal_width_mm: f64,
    /// Largura medida sobre a superfície (arco da corda quando há raio).
    pub surface_width_mm: f64,
    /// Largura efetiva: superfície × fator de processo × calibração.
    pub effective_width_mm: f64,
    /// Fator de calibração aplicado (piso 0.5; zero recai em 1.0).
    pub calibration_factor: f64,
    /// Fator de processo derivado da razão altura/largura, em [0.82, 1.04].
    pub process_factor: f64,
    /// Razão superfície/nominal (1.0 em superfície plana).
    pub curvature_gain: f64,
    /// Razão altura de camada / largura nominal.
    pub aspect_ratio: f64,
    /// Raio efetivamente usado, ou `None` em superfície plana.
    pub radius_mm: Option<f64>,
}

/// Classificação da uniformidade do revestimento (rótulos do Python).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Uniformity {
    Boa,
    Atencao,
    Critica,
}

impl Uniformity {
    /// Rótulo textual idêntico ao campo `uniformity_label` do Python.
    pub fn label(self) -> &'static str {
        match self {
            Uniformity::Boa => "Boa",
            Uniformity::Atencao => "Atencao",
            Uniformity::Critica => "Critica",
        }
    }
}

impl fmt::Display for Uniformity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Métricas de sobreposição e cobertura para um conjunto de passes.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlapMetrics {
    /// Estimativa do cordão usada no cálculo.
    pub bead: BeadEstimate,
    /// Sobreposição alvo em %, fixada em [0, 99].
    pub target_overlap_pct: f64,
    /// Sobreposição obtida em %, fixada em [0, 99].
    pub achieved_overlap_pct: f64,
    /// Passo lateral entre cordões (mm), piso 1e-6.
    pub step_over_mm: f64,
    /// Largura alvo (mm, ≥ 0) ou `None` quando não informada.
    pub target_width_mm: Option<f64>,
    /// Número de passes (≥ 1).
    pub pass_count: usize,
    /// Deslocamentos laterais de cada passe, simétricos em torno de 0.
    pub offsets: Vec<f64>,
    /// Largura total coberta pelos passes (mm).
    pub effective_cover_width_mm: f64,
    /// Razão alvo / cobertura (1.0 sem alvo).
    pub coverage_ratio: f64,
    /// Cobertura − alvo (mm; 0.0 sem alvo).
    pub fill_error_mm: f64,
    /// Pontuação 0..100 da uniformidade.
    pub uniformity_score: f64,
    /// Classificação da uniformidade.
    pub uniformity: Uniformity,
}

/// `max(a, b)` do Python: devolve `b` somente quando `b > a`.
#[inline]
fn py_max(a: f64, b: f64) -> f64 {
    if b > a {
        b
    } else {
        a
    }
}

/// `min(a, b)` do Python: devolve `b` somente quando `b < a`.
#[inline]
fn py_min(a: f64, b: f64) -> f64 {
    if b < a {
        b
    } else {
        a
    }
}

/// `value or default` do Python para floats: 0.0 e -0.0 são falsos, NaN é verdadeiro.
#[inline]
fn py_or(value: f64, default: f64) -> f64 {
    if value == 0.0 {
        default
    } else {
        value
    }
}

/// `_clamp(value, low, high)` do Python: `max(low, min(high, value))`.
#[inline]
fn clamp(value: f64, low: f64, high: f64) -> f64 {
    py_max(low, py_min(high, value))
}

/// Estima a largura efetiva do cordão a partir da largura nominal, da altura de
/// camada, do raio da superfície (se curva) e de um fator de calibração.
pub fn estimate_effective_bead_width(
    nominal_width_mm: f64,
    layer_height_mm: f64,
    radius_mm: Option<f64>,
    calibration_factor: f64,
) -> BeadEstimate {
    let nominal = py_max(py_or(nominal_width_mm, 0.0), 1e-6);
    let layer_height = py_max(py_or(layer_height_mm, 0.0), 0.0);
    let calibration = py_max(py_or(calibration_factor, 1.0), 0.5);

    let (radius, surface_width) = match radius_mm {
        Some(r) if py_or(r, 0.0) > 1e-6 => {
            let radius = py_max(r, (nominal / 2.0) + 1e-6);
            let chord_ratio = clamp(nominal / (2.0 * radius), 0.0, 0.999999);
            (Some(radius), 2.0 * radius * chord_ratio.asin())
        }
        _ => (None, nominal),
    };

    let aspect_ratio = if nominal > 1e-9 {
        layer_height / nominal
    } else {
        0.0
    };
    let mut process_factor = 1.0;
    if aspect_ratio > 0.18 {
        process_factor -= py_min((aspect_ratio - 0.18) * 0.22, 0.18);
    } else if aspect_ratio < 0.08 {
        process_factor += py_min((0.08 - aspect_ratio) * 0.08, 0.04);
    }
    let process_factor = clamp(process_factor, 0.82, 1.04);

    let effective_width = py_max(surface_width * process_factor * calibration, 1e-6);
    let curvature_gain = if nominal > 1e-9 {
        surface_width / nominal
    } else {
        1.0
    };

    BeadEstimate {
        nominal_width_mm: nominal,
        surface_width_mm: py_max(surface_width, 1e-6),
        effective_width_mm: effective_width,
        calibration_factor: calibration,
        process_factor,
        curvature_gain,
        aspect_ratio,
        radius_mm: radius,
    }
}

/// Número máximo de passes que `compute_overlap_metrics` calcula.
///
/// Uma largura-alvo enorme ou um cordão com largura ~0 pediriam milhões de passes
/// (e um `offsets` de centenas de MB); acima deste limite o número de passes é
/// fixado aqui. Abaixo dele o resultado é idêntico ao modelo Python.
pub const MAX_PASSES: usize = 100_000;

/// Calcula passo lateral, número de passes, deslocamentos, cobertura e
/// uniformidade para uma sobreposição alvo e (opcionalmente) uma largura alvo.
pub fn compute_overlap_metrics(
    nominal_width_mm: f64,
    overlap_pct: f64,
    layer_height_mm: f64,
    radius_mm: Option<f64>,
    target_width_mm: Option<f64>,
    force_min_two_passes: bool,
    calibration_factor: f64,
) -> OverlapMetrics {
    let overlap_target = clamp(py_or(overlap_pct, 0.0), 0.0, 99.0);
    let bead = estimate_effective_bead_width(
        nominal_width_mm,
        layer_height_mm,
        radius_mm,
        calibration_factor,
    );
    let effective_width = bead.effective_width_mm;
    let step_over_mm = py_max(effective_width * (1.0 - (overlap_target / 100.0)), 1e-6);
    let achieved_overlap_pct = clamp((1.0 - (step_over_mm / effective_width)) * 100.0, 0.0, 99.0);

    // Alvo não finito ("inf"/"nan" vindos de um campo de texto) é tratado
    // exatamente como ausência de alvo: um passe, sem métricas de cobertura.
    let target_width = target_width_mm
        .filter(|t| t.is_finite())
        .map(|t| py_max(py_or(t, 0.0), 0.0));
    let (pass_count, offsets, effective_cover_width_mm) = match target_width {
        None => (1usize, vec![0.0], effective_width),
        Some(target_width) => {
            let mut pass_count: usize = if target_width <= effective_width {
                1
            } else {
                let extra_span = py_max(target_width - effective_width, 0.0);
                // `as usize` satura (+inf -> usize::MAX), então a soma também satura.
                1usize.saturating_add((extra_span / step_over_mm).ceil() as usize)
            };
            if force_min_two_passes && target_width > 0.0 && target_width >= (step_over_mm * 0.5) {
                pass_count = pass_count.max(2);
            }
            let pass_count = pass_count.min(MAX_PASSES);
            if pass_count <= 1 {
                (pass_count, vec![0.0], effective_width)
            } else {
                let span = step_over_mm * ((pass_count - 1) as f64);
                let offsets = (0..pass_count)
                    .map(|idx| (-span / 2.0) + ((idx as f64) * step_over_mm))
                    .collect();
                (pass_count, offsets, effective_width + span)
            }
        }
    };

    let (coverage_ratio, fill_error_mm) = match target_width {
        None => (1.0, 0.0),
        Some(target_width) => {
            let coverage_ratio = if effective_cover_width_mm > 1e-9 {
                target_width / effective_cover_width_mm
            } else {
                1.0
            };
            (coverage_ratio, effective_cover_width_mm - target_width)
        }
    };

    let overlap_error = (achieved_overlap_pct - overlap_target).abs();
    let coverage_error = (1.0 - coverage_ratio).abs();
    let curvature_penalty = (bead.curvature_gain - 1.0).abs() * 35.0;
    let score = 100.0 - (overlap_error * 3.5) - (coverage_error * 140.0) - curvature_penalty;
    let score = clamp(score, 0.0, 100.0);

    let uniformity = if overlap_error <= 4.0 && coverage_error <= 0.04 {
        Uniformity::Boa
    } else if overlap_error <= 10.0 && coverage_error <= 0.12 {
        Uniformity::Atencao
    } else {
        Uniformity::Critica
    };

    OverlapMetrics {
        bead,
        target_overlap_pct: overlap_target,
        achieved_overlap_pct,
        step_over_mm,
        target_width_mm: target_width,
        pass_count,
        offsets,
        effective_cover_width_mm,
        coverage_ratio,
        fill_error_mm,
        uniformity_score: score,
        uniformity,
    }
}

/// Fechamento uniforme da espiral: arredonda as voltas para cima até um
/// número inteiro e redistribui o passo no comprimento, de modo que a espiral
/// termine em volta cheia (sem o "ângulo aberto" da última passada). O passo
/// nunca cresce, então a sobreposição só aumenta. Devolve `(passo, voltas)`;
/// entradas não positivas ou não finitas voltam como vieram, com as voltas
/// que elas dariam (0 quando o passo é inválido).
pub fn uniform_closing(comprimento: f64, passo: f64) -> (f64, f64) {
    if !(comprimento > 0.0 && passo > 0.0) || !comprimento.is_finite() || !passo.is_finite() {
        let voltas = if passo > 0.0 && comprimento.is_finite() && comprimento > 0.0 {
            comprimento / passo
        } else {
            0.0
        };
        return (passo, voltas);
    }
    let voltas = (comprimento / passo - 1e-9).ceil().max(1.0);
    (comprimento / voltas, voltas)
}
