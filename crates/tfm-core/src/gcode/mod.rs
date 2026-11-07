//! Geração de G-code por operação.
//!
//! Porta de `GCodeGenerator.generate` (`generator.py:266-301`): clona os
//! parâmetros, aplica [`Params::normalize`] e despacha para o módulo do modo
//! de soldagem. Cada módulo devolve o programa completo (cabeçalho + corpo +
//! rodapé) como uma linha por `String`, sem `\n`; linhas vazias do Python são
//! `String`s vazias.

pub mod bolacha;
pub mod bolacha_osc;
pub mod common;
pub mod flat_plate;
pub mod multiperfil;
pub mod oscillation;
pub mod spiral;

use crate::params::{OscillationType, Params, WeldingMode};

/// Falha na geração.
#[derive(Debug, thiserror::Error)]
pub enum GenError {
    /// O modo ainda não foi portado do Python.
    #[error("operação ainda não implementada: {0}")]
    Unsupported(&'static str),
    /// Parâmetros que tornam a geração impossível.
    #[error("parâmetros inválidos: {0}")]
    Invalid(String),
}

/// Gera o G-code com a data/hora atual no cabeçalho
/// (`datetime.now().strftime('%d/%m/%Y %H:%M:%S')`).
pub fn generate(p: &Params) -> Result<Vec<String>, GenError> {
    let timestamp = chrono::Local::now().format("%d/%m/%Y %H:%M:%S").to_string();
    generate_with_timestamp(p, &timestamp)
}

/// Gera o G-code com um timestamp fixo (usado pelas fixtures golden).
pub fn generate_with_timestamp(p: &Params, timestamp: &str) -> Result<Vec<String>, GenError> {
    // Parâmetros degenerados (largura 0, sobreposição 100%, passo 0, oscilação 0)
    // levam vários geradores a laços de dezenas de milhões de passes; a validação
    // física barra antes, como `_generate_gcode_clicked` faz no Python.
    let errors = crate::validator::validate_physical(p).errors;
    if !errors.is_empty() {
        return Err(GenError::Invalid(errors.join("\n")));
    }
    let mut p = p.clone();
    p.normalize();
    let lines = match p.welding_mode {
        WeldingMode::Espiral => spiral::generate(&p, timestamp),
        WeldingMode::EspiralMultiperfil => multiperfil::generate(&p, timestamp),
        WeldingMode::Bolacha => bolacha::generate(&p, timestamp),
        WeldingMode::ChapaPlana => flat_plate::generate(&p, timestamp),
        WeldingMode::OscilacaoLinear => oscillation::generate_linear(&p, timestamp),
        WeldingMode::OscilacaoQuadrada => oscillation::generate_square(&p, timestamp),
        WeldingMode::Oscilacao => match p.oscillation_type {
            None | Some(OscillationType::Linear) => oscillation::generate_linear(&p, timestamp),
            Some(OscillationType::Quadrada) => oscillation::generate_square(&p, timestamp),
            Some(OscillationType::QuadradaContinua) => {
                oscillation::generate_square_continuous(&p, timestamp)
            }
        },
    }?;
    let lines = bolacha::zero_no_diametro_interno(lines, &p);
    Ok(if p.tempo_inverso_g93 {
        common::to_inverse_time(&lines)
    } else {
        lines
    })
}

/// Versão impressa no cabeçalho: `app_version` dos parâmetros ou, ausente, a
/// versão desta crate (`params.get('app_version', DEFAULT_APP_VERSION)`).
pub fn app_version(p: &Params) -> String {
    p.app_version
        .clone()
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}
