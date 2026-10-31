//! Modo Bolacha (`_generate_bolacha`). Porta pendente.

use super::GenError;
use crate::params::Params;

/// Ainda não portado: devolve [`GenError::Unsupported`].
pub fn generate(_p: &Params, _timestamp: &str) -> Result<Vec<String>, GenError> {
    Err(GenError::Unsupported("bolacha"))
}
