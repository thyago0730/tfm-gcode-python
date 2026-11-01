//! tfm-core: motor de cálculo e geração de G-code do TFM G-Code Generator.
//!
//! Esta crate não tem interface gráfica. Ela porta o motor Python original
//! (`src/app/core/`) e é verificada por fixtures golden em `tests/golden/`.

pub mod audit;
pub mod config;
pub mod deposit;
pub mod estimates;
pub mod formula;
pub mod gcode;
pub mod overlap;
pub mod params;
pub mod procedure;
pub mod quality;
pub mod toolpath;
pub mod validator;
