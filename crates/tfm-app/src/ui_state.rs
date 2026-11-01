//! Estado editável da interface (textos digitados, operação escolhida).
//! Esqueleto: a task dos painéis de parâmetros completa esta struct.

use tfm_core::params::Operation;

#[derive(Clone, Debug, PartialEq)]
pub struct UiState {
    pub operation: Operation,
    /// Houve edição desde o último cálculo.
    pub dirty: bool,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            operation: Operation::Espiral,
            dirty: false,
        }
    }
}
