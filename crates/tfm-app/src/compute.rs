//! Geração de G-code em segundo plano (thread + canal), com debounce.
//! Esqueleto: a task de cálculo substitui estes placeholders.

/// Resultado de uma geração: as linhas do G-code.
#[derive(Clone, Debug, Default)]
pub struct ComputeResult {
    pub lines: Vec<String>,
}

/// Alça para o trabalhador de cálculo.
pub struct ComputeHandle;

impl ComputeHandle {
    /// `ctx` é guardado pela implementação real para pedir repaint ao terminar.
    pub fn new(_ctx: egui::Context) -> Self {
        ComputeHandle
    }

    /// Resultado pronto, se houver.
    pub fn poll(&mut self) -> Option<ComputeResult> {
        None
    }

    /// Há cálculo em andamento.
    pub fn busy(&self) -> bool {
        false
    }
}
