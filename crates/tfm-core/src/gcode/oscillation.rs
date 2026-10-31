//! Modos de oscilação (`_generate_linear_oscillation`,
//! `_generate_square_oscillation`, `_generate_square_test_oscillation`).
//! Porta pendente.

use super::GenError;
use crate::params::Params;

/// Oscilação linear. Ainda não portado: devolve [`GenError::Unsupported`].
pub fn generate_linear(_p: &Params, _timestamp: &str) -> Result<Vec<String>, GenError> {
    Err(GenError::Unsupported("oscilacao_linear"))
}

/// Oscilação quadrada. Ainda não portado: devolve [`GenError::Unsupported`].
pub fn generate_square(_p: &Params, _timestamp: &str) -> Result<Vec<String>, GenError> {
    Err(GenError::Unsupported("oscilacao_quadrada"))
}

/// Oscilação quadrada contínua. Ainda não portado: devolve [`GenError::Unsupported`].
pub fn generate_square_continuous(_p: &Params, _timestamp: &str) -> Result<Vec<String>, GenError> {
    Err(GenError::Unsupported("oscilacao_quadrada_continua"))
}
