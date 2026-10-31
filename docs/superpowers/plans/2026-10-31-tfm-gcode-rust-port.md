# TFM G-Code Generator em Rust + egui — Plano de Implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reescrever o TFM G-Code Generator (Python/Tkinter) como um workspace Rust com motor de G-code idêntico ao atual e interface egui no estilo do Prancheta CNC.

**Architecture:** Workspace Cargo com duas crates: `tfm-core` (parâmetros, gerador de G-code por operação, sobreposição, fórmulas, config, qualidade, estimativas, toolpath; sem UI) e `tfm-app` (eframe/egui: painéis esquerdo/centro/direito/inferior, tema claro/escuro/auto, 3D por software, gráficos egui_plot, diálogos). O Python atual gera fixtures golden que os testes Rust comparam linha a linha.

**Tech Stack:** Rust 1.95, eframe/egui 0.37, egui_extras 0.37, egui_plot (compatível com egui 0.37), serde/serde_json (`preserve_order`), indexmap, rfd 0.17, chrono, image 0.25, log/env_logger. Python 3.13 em `.venv` só para gerar fixtures.

**Spec:** `docs/superpowers/specs/2026-10-31-tfm-gcode-rust-egui-design.md`

## Global Constraints

- Textos visíveis ao usuário em português, mesmo tom do app atual; identificadores de parâmetros iguais aos do Python (`diametro`, `comprimento_revestir`, `largura_cordao`, `sobreposicao`, `velocidade_de_deposicao`, `espessura_camada`, `num_camadas`, ...).
- Paridade exata de G-code com o Python para todos os casos em `tests/golden/`, linha a linha, incluindo linhas vazias; única exceção é a linha `(Data: ...)`, fixada pelo timestamp `01/01/2000 00:00:00` nos testes.
- Formatação numérica: `format!("{:.3}")`, `{:.1}`, `{:.2}`, `{:+.3}`, `{:.0}`; `math.ceil` -> `f64::ceil`, `int()` -> `as i64` (trunca).
- Leitura leniente: números em JSON podem vir como string (`"5.0"`); chaves legadas `velocidade_soldagem`, `internal_entry_x`, modos `oscilacao_linear`/`oscilacao_quadrada` são aceitas.
- Nenhum `panic!` em código de produção por entrada do usuário: valores degenerados (0, negativo, vazio) viram erro de validação, não crash.
- Versão do pacote `tfm-app`: `2.0.0`. Executável `TFM_GCODE.exe`.
- Commits pequenos após cada tarefa, mensagens em português no padrão do repositório (`feat:`, `test:`, `docs:`, `build:`); sem linhas de atribuição.
- Comandos: `cargo test -p tfm-core`, `cargo build -p tfm-app --release`; Python das fixtures: `.venv/Scripts/python.exe -I scripts/golden/gen_golden.py`.

## Review Focus

1. Vírgula decimal: operador brasileiro digita `12,5`. Campos numéricos da UI devem aceitar vírgula como separador decimal. Teste em `widgets.rs` (`parse_num("12,5") == Some(12.5)`), Tarefa 16.
2. Procedimento antigo gravado pelo Python com strings numéricas e chave `oscilacao_linear`/`velocidade_soldagem` deve abrir sem erro. Teste em `procedure.rs`, Tarefa 12.
3. `config.json` parcial (só `costs`) ou ausente: app abre com defaults e grava o arquivo completo. Teste em `config.rs`, Tarefa 12.
4. Parâmetros degenerados (diâmetro 0, largura 0, sobreposição 100, camadas 0): `generate` não entra em pânico nem em loop infinito; a UI mostra o erro de validação. Teste `degenerate_params_never_panic` em `validator.rs`, Tarefa 10.
5. Programa longo (quadrada contínua, granularidade 0,1 mm, 3 camadas, 300 mm): geração termina em menos de 2 s e o visualizador de G-code usa `ScrollArea::show_rows` (virtualizado). Teste de tempo em `tests/golden.rs` (Tarefa 9); verificação manual da rolagem na Tarefa 18.

---

## Mapa de arquivos

Criar:
- `Cargo.toml` (workspace), `rust-toolchain.toml`, `.gitignore` (adicionar `target/`)
- `crates/tfm-core/Cargo.toml`, `crates/tfm-core/src/{lib,params,overlap,validator,estimates,config,quality,toolpath,procedure,audit}.rs`
- `crates/tfm-core/src/gcode/{mod,common,spiral,multiperfil,bolacha,flat_plate,oscillation}.rs`
- `crates/tfm-core/src/formula/{mod,lexer,parser,eval}.rs`
- `crates/tfm-core/tests/{golden,overlap,formula,params,config_procedure,estimates,toolpath}.rs`
- `crates/tfm-app/Cargo.toml`, `crates/tfm-app/build.rs` (ícone do .exe), `crates/tfm-app/src/{main,app,theme,prefs,widgets,icons,ui_state,compute,view3d,charts,integration}.rs`
- `crates/tfm-app/src/panels/{mod,left,center,right,bottom}.rs`
- `crates/tfm-app/src/dialogs/{mod,costs,powders,formulas,machine_profiles,mach3_path,operators,wps,risk_matrix,maintenance,about,tutorial,shortcuts}.rs`
- `scripts/golden/gen_golden.py`, `tests/golden/cases.json`, `tests/golden/*.gcode`
- `.github/workflows/rust.yml`

Modificar: `README.md`, `docs/README.md`.

Não tocar: `src/` (Python), `config/config.json` (lido, nunca sobrescrito pelos testes).

---

### Task 1: Workspace Cargo e crate core vazia

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `crates/tfm-core/Cargo.toml`, `crates/tfm-core/src/lib.rs` e um arquivo vazio por módulo, `.github/workflows/rust.yml`
- Modify: `.gitignore` (adicionar `target/`)

**Interfaces:**
- Produces: workspace com membro `crates/tfm-core` (lib `tfm_core`), dependências `serde = { features = ["derive"] }`, `serde_json = { features = ["preserve_order"] }`, `indexmap = { features = ["serde"] }`, `thiserror`, `chrono`; dev-dep `tempfile`.

- [ ] **Step 1: Criar `Cargo.toml` do workspace** com `[workspace] members = ["crates/tfm-core"] resolver = "2"` (adicionar `crates/tfm-app` na Tarefa 15), `[workspace.package] version = "2.0.0" edition = "2021"`, `[profile.release] opt-level = 3, lto = "thin", codegen-units = 1, strip = true`.
- [ ] **Step 2: Criar `crates/tfm-core/Cargo.toml`** e `src/lib.rs` com `pub mod params; pub mod overlap; pub mod gcode; pub mod validator; pub mod estimates; pub mod formula; pub mod config; pub mod quality; pub mod toolpath; pub mod procedure; pub mod audit;` (cada arquivo só com um comentário `//!`; `gcode/mod.rs` e `formula/mod.rs` idem).
- [ ] **Step 3: Rodar** `cargo build -p tfm-core` — Expected: compila sem erros.
- [ ] **Step 4: Criar `.github/workflows/rust.yml`**: `cargo build --workspace --release` + `cargo test --workspace` em `windows-latest`, `actions/checkout@v4`, `dtolnay/rust-toolchain@stable`.
- [ ] **Step 5: Commit** `build: workspace Cargo com crate tfm-core`.

---

### Task 2: Fixtures golden geradas pelo Python

**Files:**
- Create: `scripts/golden/gen_golden.py`, `tests/golden/cases.json`, `tests/golden/<nome>.gcode` (um por caso)

**Interfaces:**
- Produces: `tests/golden/cases.json` = lista de `{"name": str, "params": dict}`; `tests/golden/<name>.gcode` = linhas do Python unidas por `\n`, com a 3ª linha substituída por `(Data: 01/01/2000 00:00:00)`. Todo caso inclui `"app_version": "11.3.2"`.

- [ ] **Step 1: Escrever `gen_golden.py`** que insere `src/app` no `sys.path`, importa `core.generator.GCodeGenerator`, define `BASE` com os defaults da UI atual: `diametro 50.0, diametro_interno 0.0, comprimento_revestir 50.0, largura_revestir 100.0, largura_cordao 8.0, sobreposicao 40.0, velocidade_de_deposicao 120.0, taxa_de_deposicao 120.0, velocidade_a_mm_min 120.0, velocidade_oscilacao_mm_min 3000.0, afastamento_tocha 12.0, afastamento_seguranca 30.0, torch_ignite_delay_s 3.0, torch_extinguish_delay_s 2.0, overlap_angular_deg 10.0, bead_width_calibration_factor 1.0, lead_extras_enabled False, lead_in 0.0, lead_out 0.0, direcao_soldagem 'esquerda_direita', sentido_rotacao 'horaria', corrente_arco 180.0, tensao_arco 25.0, vazao_gas 15.0, alim_po 50.0, preaquecimento 100.0, eficiencia_termica 0.8, taxa_deposicao_g_h 700.0, num_camadas 1, espessura_camada 1.5, powder_factor 0.16, powder_cost_brl_kg 450.0, torch_retract_on_ignite True, spiral_close_ends False, flat_single_direction False, flat_orientation 'X', operacao_interna False, internal_face_clearance_x 30.0, multiperfil_count 4, multiperfil_total_angle_deg 360.0, multiperfil_pitch_mm 12.0, multiperfil_strategy 'Passes separados', multiperfil_point_by_point False, multiperfil_profile_shape 'Retangular', multiperfil_profile_base_mm 8.0, multiperfil_profile_top_mm 4.0, multiperfil_profile_height_mm 1.5, multiperfil_end_chamfer_enabled False, multiperfil_end_chamfer_angle_deg 45.0, bolacha_tipo 'Espiral Contínua', bolacha_stop_between False, oscilacao_comprimento 5.0, deslocamento_angular_perc 20.0, osc_test_gran_x 0.5, osc_test_gran_a 1.0, smart_layers False, root_feed_scale 100.0, nome_procedimento 'Novo Procedimento', traceability_operator/eps/powder_batch/supplier/certificate '', app_version '11.3.2'`, e a matriz `CASES` abaixo; para cada caso grava `.gcode` e acumula em `cases.json` (indent 2, `ensure_ascii=False`).
- [ ] **Step 2: Matriz de casos** (nome -> overrides sobre BASE):
  - `espiral_padrao` {welding_mode espiral}; `espiral_3camadas_de_ccw` {num_camadas 3, direcao_soldagem direita_esquerda, sentido_rotacao antihoraria}; `espiral_fechado_leads` {spiral_close_ends True, lead_extras_enabled True, lead_in 5, lead_out 5}; `espiral_interno` {operacao_interna True, diametro 100, diametro_interno 60, num_camadas 2}; `espiral_sem_atrasos` {torch_ignite_delay_s 0, torch_extinguish_delay_s 0, torch_retract_on_ignite False}; `espiral_rastreab` {traceability_eps 'EPS-01', traceability_operator 'Op', traceability_powder_batch 'L1', traceability_supplier 'F', traceability_certificate 'C1'}.
  - `multiperfil_retangular` {welding_mode espiral_multiperfil}; `multiperfil_trapezoidal_chanfro` {+ multiperfil_profile_shape Trapezoidal, multiperfil_end_chamfer_enabled True, multiperfil_profile_height_mm 4.5}; `multiperfil_triangular_setor` {+ shape Triangular, multiperfil_count 3, multiperfil_total_angle_deg 180}; `multiperfil_ponto_a_ponto` {+ multiperfil_point_by_point True, multiperfil_profile_height_mm 3.0}; `multiperfil_continuo_fechado` {+ multiperfil_strategy 'Percurso contínuo', spiral_close_ends True}; `multiperfil_interno_ccw` {+ operacao_interna True, diametro 120, diametro_interno 80, sentido_rotacao antihoraria}.
  - `bolacha_espiral` {welding_mode bolacha, diametro 100}; `bolacha_aneis` {+ bolacha_tipo 'Anéis (Passo a Passo)', diametro_interno 20}; `bolacha_aneis_parada` {+ idem, bolacha_stop_between True}; `bolacha_de_2camadas_smart` {welding_mode bolacha, direcao_soldagem direita_esquerda, num_camadas 2, smart_layers True, root_feed_scale 80, diametro 80}.
  - `chapa_x` {welding_mode chapa_plana}; `chapa_y_de` {+ flat_orientation Y, direcao_soldagem direita_esquerda}; `chapa_unidirecional_leads` {+ flat_single_direction True, lead_extras_enabled True, lead_in 5, lead_out 5, num_camadas 2}.
  - `osc_linear` {welding_mode oscilacao, oscillation_type linear}; `osc_linear_interno` {+ operacao_interna True, diametro 100, diametro_interno 60}; `osc_linear_de_ccw_leads` {+ direcao_soldagem direita_esquerda, sentido_rotacao antihoraria, lead_extras_enabled True, lead_in 5, lead_out 5, num_camadas 2}.
  - `osc_quadrada` {welding_mode oscilacao, oscillation_type quadrada}; `osc_quadrada_interno` {+ operacao_interna True, diametro 100, diametro_interno 60}; `osc_quadrada_de_leads` {+ direcao_soldagem direita_esquerda, lead_extras_enabled True, lead_in 5, lead_out 5}.
  - `osc_quad_continua` {welding_mode oscilacao, oscillation_type quadrada_continua}; `osc_quad_continua_de_leadout` {+ direcao_soldagem direita_esquerda, lead_extras_enabled True, lead_out 5}; `osc_quad_continua_2camadas_gran` {+ num_camadas 2, osc_test_gran_x 1.0, osc_test_gran_a 2.0}.
  - Modos legados: `legado_osc_linear` {welding_mode oscilacao_linear}; `legado_osc_quadrada` {welding_mode oscilacao_quadrada, oscillation_type quadrada}.
- [ ] **Step 3: Rodar** `.venv/Scripts/python.exe -I scripts/golden/gen_golden.py` — Expected: imprime `30 casos gravados`, cria 30 `.gcode` não vazios; `grep -c "M101" tests/golden/espiral_3camadas_de_ccw.gcode` == 3.
- [ ] **Step 4: Commit** `test: fixtures golden de G-code geradas pelo Python (30 casos)`.

---

### Task 3: Parâmetros (`params.rs`)

**Files:**
- Create: `crates/tfm-core/src/params.rs`, `crates/tfm-core/tests/params.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum WeldingMode { Espiral, EspiralMultiperfil, Bolacha, ChapaPlana, Oscilacao }  // serde "espiral","espiral_multiperfil","bolacha","chapa_plana","oscilacao"; aliases "oscilacao_linear","oscilacao_quadrada" -> Oscilacao
  pub enum OscillationType { Linear, Quadrada, QuadradaContinua }       // "linear","quadrada","quadrada_continua"
  pub enum Direction { EsquerdaDireita, DireitaEsquerda }               // "esquerda_direita","direita_esquerda"
  pub enum Rotation { Horaria, Antihoraria }                            // "horaria","antihoraria"
  pub enum ProfileShape { Retangular, Trapezoidal, Triangular }         // "Retangular","Trapezoidal","Triangular"
  pub enum MultiperfilStrategy { PassesSeparados, PercursoContinuo }    // "Passes separados","Percurso contínuo"
  pub enum BolachaTipo { EspiralContinua, Aneis }                       // "Espiral Contínua","Anéis (Passo a Passo)"
  pub enum FlatOrientation { X, Y }
  pub enum Operation { Espiral, EspiralMultiperfil, OscilacaoLinear, OscilacaoQuadrada, OscilacaoQuadradaContinua, ChapaPlana, Bolacha }
  impl Operation { pub const ALL: [Operation; 7]; pub fn label(self) -> &'static str /* "Espiral","Espiral Multiperfil","Oscilação Linear","Oscilação Quadrada","Oscilação Quadrada Contínua","Chapa Plana","Bolacha" */; pub fn short_label(self) -> &'static str /* "Osc. Linear","Osc. Quad.","Osc. Quad. Cont." */; pub fn mode(self) -> (WeldingMode, Option<OscillationType>); pub fn from_params(p: &Params) -> Operation }
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)] #[serde(default)]
  pub struct Params { /* todos os campos do BASE da Tarefa 2 como f64/i64/bool/String/enums; f64 com #[serde(deserialize_with = "lenient_f64")], i64 com lenient_i64, bool com lenient_bool; velocidade_de_deposicao com #[serde(alias = "velocidade_soldagem")]; internal_face_clearance_x com alias "internal_entry_x"; taxa_de_deposicao: Option<f64>; app_version: Option<String>; notes, ordem_servico, powder_name, traceability_env_temp, traceability_env_humidity: String */ }
  impl Params { pub fn normalize(&mut self) /* porta de _normalize_params */; pub fn taxa(&self) -> f64; pub fn is_internal(&self) -> bool; pub fn face_clearance(&self) -> f64 /* internal_face_clearance_x ou 30.0 */; pub fn rotation_sign(&self) -> f64; pub fn apply_operation(&mut self, op: Operation); pub fn reset_mode_defaults(&mut self, op: Operation) /* porta de _reset_mode_defaults */ }
  impl Default for Params  // BASE da Tarefa 2 (app_version None)
  pub fn lenient_f64<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error>  // número, "12.5", "12,5"; "" -> 0.0
  ```

- [ ] **Step 1: Escrever `tests/params.rs`**: `default_matches_python_ui_defaults` (diametro 50.0, largura_cordao 8.0, torch_ignite_delay_s 3.0, num_camadas 1); `deserializes_python_strings_and_legacy_keys` (JSON `{"velocidade_soldagem": "150.0", "lead_in": "5,5", "welding_mode": "oscilacao_linear", "internal_entry_x": "25"}` -> velocidade 150.0, lead_in 5.5, mode Oscilacao, face_clearance 25.0); `all_golden_cases_deserialize` (lê `tests/golden/cases.json` via `concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/golden/cases.json")`; cada `params` desserializa e re-serializa mantendo `welding_mode`); `operation_round_trip` (para cada `Operation::ALL`: `apply_operation` e `from_params` devolvem a mesma).
- [ ] **Step 2: Rodar** `cargo test -p tfm-core --test params` — Expected: FAIL (tipos não existem).
- [ ] **Step 3: Implementar `params.rs`**. `lenient_*` desserializam para `serde_json::Value` e convertem. `reset_mode_defaults` porta exata de `_reset_mode_defaults` (TFM_GCODE.py:1289-1345), incluindo `bead_width_calibration_factor = 1.0` sempre.
- [ ] **Step 4: Rodar** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): struct Params compatível com o JSON do Python`.

---

### Task 4: Modelo de sobreposição (`overlap.rs`)

**Files:**
- Create: `crates/tfm-core/src/overlap.rs`, `crates/tfm-core/tests/overlap.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct BeadEstimate { pub nominal_width_mm: f64, pub surface_width_mm: f64, pub effective_width_mm: f64, pub calibration_factor: f64, pub process_factor: f64, pub curvature_gain: f64, pub aspect_ratio: f64, pub radius_mm: Option<f64> }
  pub fn estimate_effective_bead_width(nominal_width_mm: f64, layer_height_mm: f64, radius_mm: Option<f64>, calibration_factor: f64) -> BeadEstimate
  #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)] pub enum Uniformity { Boa, Atencao, Critica }  impl Uniformity { pub fn label(self) -> &'static str }
  pub struct OverlapMetrics { pub bead: BeadEstimate, pub target_overlap_pct: f64, pub achieved_overlap_pct: f64, pub step_over_mm: f64, pub target_width_mm: Option<f64>, pub pass_count: usize, pub offsets: Vec<f64>, pub effective_cover_width_mm: f64, pub coverage_ratio: f64, pub fill_error_mm: f64, pub uniformity_score: f64, pub uniformity: Uniformity }
  pub fn compute_overlap_metrics(nominal_width_mm: f64, overlap_pct: f64, layer_height_mm: f64, radius_mm: Option<f64>, target_width_mm: Option<f64>, force_min_two_passes: bool, calibration_factor: f64) -> OverlapMetrics
  ```

- [ ] **Step 1: Escrever `tests/overlap.rs`** portando `tests/test_overlap_model.py`: `flat_surface_matches_nominal`, `curved_surface_expands_width`, `high_aspect_ratio_reduces_effective_width`, `overlap_reduces_step_over` (10, 40 -> achieved ≈ 40 ±0.5), `single_pass_when_target_narrow` (10, 30, target 8 -> 1 passe, offsets [0.0]), `multiple_passes_when_target_wide` (8, 50, target 30 -> ≥2, len(offsets)==pass_count), `uniformity_good_for_balanced` (8, 35, target 8 -> Boa, score ≥ 80), `zero_overlap_clamped`, `force_min_two_passes` (8, 40, target 6, force -> 2 passes, offsets ±step/2). Mais `step_over_matches_python_reference`: valor de `compute_overlap_metrics(8.0, 40.0, 1.5, Some(25.0), None, true, 1.0).step_over_mm` obtido rodando o Python uma vez (`.venv/Scripts/python.exe -I -c`), colado com 9 casas, tolerância 1e-9.
- [ ] **Step 2: Rodar** — Expected: FAIL.
- [ ] **Step 3: Implementar `overlap.rs`** porta literal de `overlap_model.py`.
- [ ] **Step 4: Rodar** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): modelo de sobreposição efetiva do cordão`.

---

### Task 5: Base do gerador + Espiral (`gcode/common.rs`, `gcode/mod.rs`, `gcode/spiral.rs`, harness golden)

**Files:**
- Create: `crates/tfm-core/src/gcode/{mod,common,spiral,multiperfil,bolacha,flat_plate,oscillation}.rs`, `crates/tfm-core/tests/golden.rs`

**Interfaces:**
- Consumes: `Params`, `Operation`, `compute_overlap_metrics`.
- Produces:
  ```rust
  // gcode/mod.rs
  #[derive(Debug, thiserror::Error)] pub enum GenError { #[error("operação ainda não implementada: {0}")] Unsupported(&'static str), #[error("parâmetros inválidos: {0}")] Invalid(String) }
  pub fn generate(p: &Params) -> Result<Vec<String>, GenError>            // chrono::Local::now().format("%d/%m/%Y %H:%M:%S")
  pub fn generate_with_timestamp(p: &Params, timestamp: &str) -> Result<Vec<String>, GenError>  // clona, normalize(), no multiperfil ajusta num_camadas = profile.layer_count e profile_height, despacha
  pub fn app_version(p: &Params) -> String  // p.app_version ou env!("CARGO_PKG_VERSION")
  // gcode/common.rs
  pub fn f3(v: f64) -> String; pub fn f2(v: f64) -> String; pub fn f1(v: f64) -> String; pub fn f3s(v: f64) -> String /* {:+.3} */
  #[derive(Clone, Default)] pub struct Pos { pub x: Option<f64>, pub y: Option<f64>, pub z: Option<f64>, pub a: Option<f64> }
  pub fn torch_ignite_delay(p: &Params) -> f64; pub fn torch_extinguish_delay(p: &Params) -> f64
  pub fn append_torch_enable(lines: &mut Vec<String>, p: &Params)
  pub fn append_torch_disable(lines: &mut Vec<String>, p: &Params, start: Option<&Pos>, end: Option<&Pos>, feed: Option<f64>, path_speed_mm_min: Option<f64>, rotary_radius_mm: Option<f64>) -> Pos
  pub fn extend_motion_target(start: &Pos, end: &Pos, extra_mm: f64, rotary_radius_mm: Option<f64>) -> Pos
  pub fn effective_base_diameter(p: &Params) -> f64
  pub struct ArcBounds { pub x_entry: Option<f64>, pub x_exit: Option<f64>, pub x_start: f64, pub x_end: f64 }
  pub fn rotary_arc_bounds(p: &Params) -> ArcBounds
  pub fn layer_radius(p: &Params, layer_num: i64, current_d: Option<f64>) -> f64
  pub fn overlap_metrics(p: &Params, layer_num: i64, current_d: Option<f64>, target_width: Option<f64>) -> OverlapMetrics  // force_min_two_passes = true
  pub fn apply_smart_layers(p: &Params, layer_num: i64, feed: f64) -> f64
  pub fn build_header(p: &Params, timestamp: &str) -> Vec<String>
  pub fn build_footer(p: &Params) -> Vec<String>
  pub fn layer_separator(p: &Params, layer_num: i64) -> String  // "M01" se layer_num < num_camadas-1, senão ""
  // cada módulo de modo:
  pub fn generate(p: &Params, timestamp: &str) -> Result<Vec<String>, GenError>
  ```
  Módulos ainda não portados devolvem `Err(GenError::Unsupported("..."))`.

- [ ] **Step 1: Escrever `tests/golden.rs`**: `run_golden(filter: &str)` lê `cases.json`; para cada caso cujo nome contém `filter`: desserializa `Params`, chama `generate_with_timestamp(&p, "01/01/2000 00:00:00")`, compara com `expected.split('\n')` e no primeiro desvio falha com `"{name}: linha {i}\n  esperado: {e}\n  obtido:   {o}"`; também falha se o número de linhas diferir. Testes: `golden_espiral` ("espiral_"), `golden_multiperfil`, `golden_bolacha`, `golden_chapa`, `golden_osc_linear`, `golden_osc_quadrada` ("osc_quadrada"), `golden_osc_quad_continua`, `golden_legado`. Os sem gerador ficam `#[ignore]` até a tarefa deles.
- [ ] **Step 2: Rodar** `cargo test -p tfm-core --test golden golden_espiral` — Expected: FAIL.
- [ ] **Step 3: Implementar `common.rs`** porta literal de generator.py:34-227 e 303-399 (todas as linhas condicionais do cabeçalho na mesma ordem; `extend_motion_target` com A convertido em mm pelo raio).
- [ ] **Step 4: Implementar `spiral.rs`** porta de `_generate_spiral`/`_build_spiral_segment` (generator.py:401-483).
- [ ] **Step 5: Implementar `mod.rs`** (despacho, `GenError`, stubs).
- [ ] **Step 6: Rodar** `cargo test -p tfm-core --test golden golden_espiral` — Expected: PASS (6 casos).
- [ ] **Step 7: Commit** `feat(core): gerador base, cabeçalho/rodapé e modo Espiral com paridade golden`.

---

### Task 6: Espiral Multiperfil (`gcode/multiperfil.rs`)

**Files:**
- Modify: `crates/tfm-core/src/gcode/multiperfil.rs`, `tests/golden.rs` (remover `#[ignore]` de `golden_multiperfil`)

**Interfaces:**
- Produces (públicos, usados por `estimates.rs` e pela UI):
  ```rust
  pub struct ProfileDef { pub shape: ProfileShape, pub base_width: f64, pub top_width: f64, pub layer_count: i64, pub layer_height: f64, pub profile_height: f64, pub total_height: f64, pub end_chamfer_enabled: bool, pub end_chamfer_angle_deg: f64 }
  pub fn profile_definition(p: &Params) -> ProfileDef
  pub fn start_angles(p: &Params) -> Vec<f64>
  pub fn align_rotary_target(target_a: f64, current_a: Option<f64>, rotation_sign: f64) -> f64
  pub fn layer_end_chamfer(p: &Params, layer_num: i64) -> f64
  pub fn pass_offsets(p: &Params, layer_num: i64, current_d: Option<f64>) -> (Vec<f64>, f64, usize)
  pub fn layer_profile_widths(p: &Params) -> Vec<f64>     // porta de _get_multiperfil_layer_profile_widths (TFM_GCODE.py)
  pub fn offset_helix_segment_within_bounds(x0: f64, a0_deg: f64, x1: f64, a1_deg: f64, radius: f64, offset_mm: f64, x_min: f64, x_max: f64) -> (f64, f64, f64, f64)
  ```

- [ ] **Step 1: Designorar `golden_multiperfil`**; rodar — Expected: FAIL `Unsupported`.
- [ ] **Step 2: Implementar** porta de generator.py:129-264 e 485-593. `current_a_position` acumula entre perfis; `M01` só quando `!point_by_point && strategy == PassesSeparados && (profile_idx < n || layer_num < num_camadas-1)`.
- [ ] **Step 3: Rodar** — Expected: PASS (6 casos).
- [ ] **Step 4: Commit** `feat(core): modo Espiral Multiperfil com paridade golden`.

---

### Task 7: Bolacha (`gcode/bolacha.rs`)

**Files:**
- Modify: `crates/tfm-core/src/gcode/bolacha.rs`, `tests/golden.rs` (designorar `golden_bolacha`)

- [ ] **Step 1: Designorar e rodar** — Expected: FAIL.
- [ ] **Step 2: Implementar** porta de generator.py:595-788: `r_int = max(d_int/2, largura/2)`, `passo_radial = largura*(1-sobrep/100)` (mín 1e-6), `num_voltas = ceil(radial_width/passo)` ou 1, `rpm = min(taxa/circ, 20)`, anéis `v in 0..=num_voltas`, espiral `deg_per_step = 10`, `extra_steps = ceil(overlap/10)`. O `feed_angular` da última iteração alimenta o `append_torch_disable` final.
- [ ] **Step 3: Rodar** — Expected: PASS (4 casos).
- [ ] **Step 4: Commit** `feat(core): modo Bolacha com paridade golden`.

---

### Task 8: Chapa Plana (`gcode/flat_plate.rs`)

**Files:**
- Modify: `crates/tfm-core/src/gcode/flat_plate.rs`, `tests/golden.rs` (designorar `golden_chapa`)

- [ ] **Step 1: Designorar e rodar** — FAIL.
- [ ] **Step 2: Implementar** porta de generator.py:790-984 (orientações X/Y, zigue-zague vs unidirecional; `M0` só na camada 0; torch enable fora do laço só no zigue-zague).
- [ ] **Step 3: Rodar** — PASS (3 casos).
- [ ] **Step 4: Commit** `feat(core): modo Chapa Plana com paridade golden`.

---

### Task 9: Oscilações linear, quadrada e quadrada contínua (`gcode/oscillation.rs`)

**Files:**
- Modify: `crates/tfm-core/src/gcode/oscillation.rs`, `tests/golden.rs` (designorar `golden_osc_linear`, `golden_osc_quadrada`, `golden_osc_quad_continua`, `golden_legado`)

**Interfaces:**
- Produces: `pub fn generate_linear`, `generate_square`, `generate_square_continuous` (mesma assinatura dos modos), despachadas por `mod.rs` segundo `oscillation_type`.

- [ ] **Step 1: Designorar os 4 testes e rodar** — FAIL.
- [ ] **Step 2: Implementar linear** (generator.py:986-1108).
- [ ] **Step 3: Implementar quadrada** (1134-1294).
- [ ] **Step 4: Implementar quadrada contínua** (1296-1479): `staircase_move` como fn interna que emite `G01 F{feed_x} X..` e `G01 F{feed_a} A..` por subpasso; lead-out com feeds `feed_x*0.85` e `feed_x*0.6`.
- [ ] **Step 5: Teste de desempenho** em `tests/golden.rs`: `square_continuous_long_program_under_2s` (`comprimento_revestir 300, num_camadas 3, osc_test_gran_x 0.1, osc_test_gran_a 0.5`) — `elapsed < 2s` e `lines.len() > 10_000`.
- [ ] **Step 6: Rodar** `cargo test -p tfm-core --test golden` — Expected: 30 casos PASS, nenhum `#[ignore]`.
- [ ] **Step 7: Commit** `feat(core): oscilações linear, quadrada e quadrada contínua com paridade golden`.

---

### Task 10: Validação (`validator.rs`)

**Files:**
- Create: `crates/tfm-core/src/validator.rs` (testes inline)

**Interfaces:**
- Produces:
  ```rust
  pub fn gcode_invariants(lines: &[String], p: &Params) -> Vec<String>   // porta de validate_gcode_invariants; mensagens iguais ("G-code vazio", "Movimento de solda antes de ligar a tocha (M101)", "Rotação A invertida: {:.3} -> {:.3} ({prev} -> {line})")
  pub struct FieldErrors { pub errors: Vec<String>, pub invalid_fields: Vec<&'static str> }  // "largura_cordao","sobreposicao","velocidade_de_deposicao","num_camadas","oscilacao_comprimento","deslocamento_angular_perc","multiperfil_count","multiperfil_total_angle_deg","multiperfil_pitch_mm","multiperfil_profile_base_mm","multiperfil_profile_top_mm","multiperfil_end_chamfer_angle_deg","diametro","comprimento_revestir","osc_test_gran_x"
  pub fn validate_physical(p: &Params) -> FieldErrors   // união de _validate_parameters e das regras de _get_current_params
  pub fn can_generate(p: &Params) -> bool
  ```

- [ ] **Step 1: Testes**: `invariants_accept_golden_spiral`, `invariants_detect_reversed_a` (`M101`, `G01 A10`, `G01 A5`, horaria -> 1 issue), `physical_blocks_zero_width` ("Largura do cordão deve ser maior que zero." e `invalid_fields` contém "largura_cordao"), `degenerate_params_never_panic` (para cada `Operation::ALL` × {diametro 0, largura 0, sobreposicao 100, num_camadas 0, comprimento 0, oscilacao 0, deslocamento 0, multiperfil_pitch 0}: `catch_unwind(generate_with_timestamp)` sem pânico e < 1 s).
- [ ] **Step 2: Rodar** — FAIL.
- [ ] **Step 3: Implementar**. Se o teste revelar laço infinito em algum gerador, `generate_with_timestamp` passa a chamar `validate_physical` primeiro e devolver `GenError::Invalid` (golden continua passando porque os casos são válidos).
- [ ] **Step 4: Rodar** `cargo test -p tfm-core` — PASS.
- [ ] **Step 5: Commit** `feat(core): validação física e invariantes do G-code`.

---

### Task 11: Avaliador de fórmulas (`formula/`)

**Files:**
- Create: `crates/tfm-core/src/formula/{mod,lexer,parser,eval}.rs`, `crates/tfm-core/tests/formula.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum Expr { Num(f64), Var(String) /* "math.pi" é Var */, Unary(Op, Box<Expr>), Binary(Op, Box<Expr>, Box<Expr>), Call(String, Vec<Expr>) }
  pub enum FormulaError { Parse(String), UnknownName(String), Math(String), Arity(String) }
  pub fn parse(src: &str) -> Result<Expr, FormulaError>
  pub fn eval(e: &Expr, vars: &dyn Fn(&str) -> Option<f64>) -> Result<f64, FormulaError>
  pub fn formula_locals(p: &Params, costs: &Costs) -> IndexMap<String, f64>   // porta de build_formula_locals: campos numéricos/bool de Params (bool -> 0/1) + step_over_mm, effective_width_mm, overlap_pass_count, gas_argon_brl_m3, labor_brl_hour, machine_brl_hour, powder_cost_brl_kg
  pub fn evaluate_block(expressions: &IndexMap<String, String>, locals: &IndexMap<String, f64>) -> IndexMap<String, Option<f64>>  // até 6 rodadas; UnknownName -> pendente; outro erro -> None
  ```
  Suporte: literais (`1`, `1.5`, `1e-6`), `+ - * / // % **`, unário `-`, parênteses, identificadores com `.`, chamadas `max min abs round ceil floor sqrt sin cos tan` com ou sem prefixo `math.`; `math.pi`, `pi`, `math.e`. Precedência Python: `**` > unário > `* / // %` > `+ -`. Divisão por zero -> `Math`.

- [ ] **Step 1: Testes**: `parses_python_formula`, `math_prefix_and_bare`, `dependency_resolution_order_independent` (`b = a * 2`, `a = 3` -> b 6), `error_isolated_to_key` (`"1/0"` -> None), `default_config_formulas_all_evaluate` (`config/config.json` do repo com `Params::default()` -> 20 chaves `Some`), `power_and_floordiv` (`2**10 == 1024`, `7 // 2 == 3`).
- [ ] **Step 2: Rodar** — FAIL. **Step 3: Implementar**. **Step 4: Rodar** — PASS.
- [ ] **Step 5: Commit** `feat(core): avaliador de fórmulas compatível com a sintaxe Python do config.json`.

---

### Task 12: Config, procedimentos, auditoria e qualidade

**Files:**
- Create: `crates/tfm-core/src/{config,procedure,audit,quality}.rs`, `crates/tfm-core/tests/config_procedure.rs`

**Interfaces:**
- Produces:
  ```rust
  // config.rs (todo #[serde(default)])
  pub struct Costs { pub powder_brl_kg: f64, pub gas_argon_brl_m3: f64, pub labor_brl_hour: f64, pub machine_brl_hour: f64, pub currency_symbol: String /* "R$" */, #[serde(flatten)] pub extra: IndexMap<String, serde_json::Value> }
  pub struct Powder { pub name: String, pub density_factor_g_mm2: f64, pub cost_brl_kg: f64 }
  pub struct Formulas { pub expressions: IndexMap<String, String>, pub notes: String }
  pub struct Integration { pub mach3_path: String, pub gcode_dir: String /* "C:/Mach3/GCode" */, pub write_autoload_txt: bool /* true */, pub allow_file_association_fallback: bool, pub mach3_profile: String /* "PTA" */ }
  pub struct MachineLimits { pub max_rpm_chuck: f64 /* 100 */, pub max_length_x: f64 /* 1500 */, pub max_diameter: f64 /* 500 */, pub max_feed_linear: f64 /* 5000 */ }
  pub struct MachineProfile { pub name: String, pub limits: MachineLimits }
  pub struct UiPrefs { pub theme: String /* "auto" */, pub left_w: f32 /* 360 */, pub right_w: f32 /* 390 */, pub left_pinned: bool /* true */, pub right_pinned: bool /* true */, pub bottom_collapsed: bool, pub dpi_scaling: String, pub compact_threshold_px: u32, #[serde(flatten)] pub extra: IndexMap<String, Value> }
  pub struct Database { pub procedures_path: String /* "procedures" */ }
  pub struct AppConfig { pub ui: UiPrefs, pub costs: Costs, pub powders: Vec<Powder>, pub active_powder: String, pub formulas: Formulas, pub database: Database, pub integration: Integration, pub machine_profiles: IndexMap<String, MachineProfile>, pub active_machine_profile: String }
  impl AppConfig { pub fn default_formulas() -> Formulas /* as 20 expressões de get_default_formulas */; pub fn resolve_path() -> PathBuf /* ao lado do exe: config/config.json, config.json; pasta pai idem; em debug a raiz do workspace */; pub fn load(path: &Path) -> AppConfig; pub fn save(&self, path: &Path) -> io::Result<()> /* indent 4 */; pub fn active_limits(&self) -> MachineLimits; pub fn powder(&self, name: &str) -> Option<&Powder> }
  // procedure.rs
  pub fn save_procedure(path: &Path, p: &Params) -> io::Result<()>
  pub fn load_procedure(path: &Path) -> Result<Params, ProcedureError>  // leniente; defaults do Python para chaves ausentes: lead_in/out 5.0, direcao esquerda_direita, welding_mode espiral, oscilacao_comprimento 10.0, deslocamento 50.0, sentido horaria
  // audit.rs
  pub struct AuditLogger { pub dir: PathBuf }  impl AuditLogger { pub fn log(&self, category: &str, detail: &str, user: &str) }  // LOG_ENG_%Y-%m-%d.json: array de {timestamp (ISO 8601), category, detail, user, software_version}
  // quality.rs
  pub struct Operator { pub name: String, pub code: String, pub qualification_id: String, pub qualification_validity: String /* AAAA-MM-DD */ }
  pub struct Limit { pub min: f64, pub max: f64 }
  pub struct Wps { pub name: String, pub params: IndexMap<String, Value>, pub limits: IndexMap<String, Limit> }
  pub struct Component { pub limit_hours: f64, pub current_hours: f64 }
  pub struct HistoryEntry { pub date: String, pub action: String, pub total_time: f64 }
  pub struct Maintenance { pub total_arc_time_minutes: f64, pub last_maintenance_date: String, pub components: IndexMap<String, Component> /* bico_constritor 20, eletrodo 40, mangueiras 500 */, pub history: Vec<HistoryEntry> }
  pub struct QualityManager { pub config_dir: PathBuf, pub data_dir: PathBuf, pub operators: Vec<Operator>, pub wps: Vec<Wps>, pub maintenance: Maintenance }
  impl QualityManager { pub fn load(config_dir: &Path, data_dir: &Path) -> Self /* operators.json, wps.json em data_dir; maintenance_log.json em config_dir */; pub fn save_operators(&self); pub fn save_wps(&self); pub fn save_maintenance(&self); pub fn check_operator_qualification(&self, name: &str) -> Result<(), String>; pub fn validate_iso3834(&self, p: &Params) -> Result<(), String> /* mensagens "BLOQUEIO ISO 3834: ..." iguais ao Python */; pub fn log_arc_time(&mut self, minutes: f64); pub fn reset_component(&mut self, name: &str); pub fn maintenance_alerts(&self) -> Vec<String>; pub fn heat_input_kj_mm(voltage: f64, current: f64, speed_mm_min: f64, efficiency: f64) -> f64; pub fn estimate_dilution(current: f64, powder_g_min: f64, speed_mm_min: f64) -> f64; pub fn process_warnings(p: &Params) -> (Vec<String>, Vec<String>) }
  ```

- [ ] **Step 1: Testes** (com `tempfile`): `config_partial_file_fills_defaults` (`{"costs": {"labor_brl_hour": 99}}` -> labor 99, 1 pó default, `gcode_dir == "C:/Mach3/GCode"`); `config_missing_file_saves_full_structure`; `repo_config_json_round_trips` (`formulas.expressions.len() == 20`, `powders.len() == 9`); `legacy_python_procedure_loads` (`"lead_in": "5.0"`, `"welding_mode": "oscilacao_linear"`, `"velocidade_soldagem": 150` -> `Operation::OscilacaoLinear`, velocidade 150); `procedure_round_trip`; `iso3834_blocks_expired_operator` (validade `2000-01-01` -> Err com "vencida"); `iso3834_requires_batch_when_eps_set` (EPS "E1" cadastrada, lote vazio -> Err com "Lote do Pó"); `heat_input_formula` (25 V, 180 A, 120 mm/min, 0.8 -> 1.8 kJ/mm ±1e-9).
- [ ] **Step 2: Rodar** — FAIL. **Step 3: Implementar**. **Step 4: Rodar** `cargo test -p tfm-core` — PASS.
- [ ] **Step 5: Commit** `feat(core): config, procedimentos, auditoria e gestão da qualidade compatíveis com o Python`.

---

### Task 13: Estimativas, resultados e custos (`estimates.rs`)

**Files:**
- Create: `crates/tfm-core/src/estimates.rs`, `crates/tfm-core/tests/estimates.rs`

**Interfaces:**
- Consumes: `Params`, `AppConfig`, `common::overlap_metrics`, `multiperfil::{profile_definition, layer_profile_widths}`, `formula::{formula_locals, evaluate_block}`, `QualityManager::{heat_input_kj_mm, estimate_dilution}`.
- Produces:
  ```rust
  pub struct ResultText { pub text: String, pub over_limit: bool }
  pub struct OverlapResults { pub effective_bead_width: String, pub surface_step: String, pub coverage: String, pub uniformity: String, pub uniformity_level: Uniformity }
  pub struct CostSummary { pub consumiveis: f64, pub operacional: f64, pub total: f64, pub powder_kg: f64, pub gas_m3: f64, pub powder_cost: f64, pub gas_cost: f64, pub labor_cost: f64, pub machine_cost: f64, pub currency: String }
  pub struct Estimates { pub rotation: ResultText, pub pitch: ResultText /* over_limit = curso X excedido */, pub rotations: String, pub total_angle: String, pub time_hhmm: String, pub time_hours: f64, pub overlap: OverlapResults, pub costs: CostSummary, pub formulas: IndexMap<String, Option<f64>>, pub heat_input_kj_mm: f64, pub dilution_pct: f64 }
  pub fn compute(p: &Params, cfg: &AppConfig) -> Estimates   // porta de executar_calculos_e_desenho (ramos espiral, multiperfil, chapa, oscilação, bolacha), _update_overlap_results, overrides de fórmulas, custos; tempo com ceil ao minuto
  pub struct PassPlanItem { pub width: f64, pub effective_width: f64, pub bead_effective_width: f64, pub surface_width: f64, pub step_over_mm: f64, pub pass_count: usize, pub offsets: Vec<f64>, pub coverage_ratio: f64, pub fill_error_mm: f64, pub uniformity: Uniformity, pub achieved_overlap_pct: f64 }
  pub fn multiperfil_pass_plan(p: &Params) -> Vec<PassPlanItem>
  ```
  A UI do Python calcula a sobreposição com `_get_overlap_metrics(params, layer_idx)`, mesma matemática de `common::overlap_metrics(p, layer_idx, None, target)`.

- [ ] **Step 1: Testes**: `espiral_default_time_and_costs` (`time_hhmm` casa com `^\d{2}:\d{2}$`, `costs.total > 0`), `chapa_zero_rotation` (`rotation.text == "0.000"`, `rotations == "0"`), `bolacha_rotation_range_text` (d100/d_int 20 -> contém `" - "`), `rpm_over_limit_flag` (velocidade 50000 -> `rotation.over_limit`), `formula_override_powder_cost` (config com `powder_cost_brl = "123.0"` -> `costs.powder_cost == 123.0`), `multiperfil_pass_plan_len_equals_layers` (height 4.5, espessura 1.5 -> 3 itens).
- [ ] **Step 2: Rodar** — FAIL. **Step 3: Implementar**. **Step 4: Rodar** — PASS.
- [ ] **Step 5: Commit** `feat(core): resultados calculados, sobreposição efetiva e estimativa de custos`.

---

### Task 14: Toolpath 3D e séries de gráficos (`toolpath.rs`)

**Files:**
- Create: `crates/tfm-core/src/toolpath.rs`, `crates/tfm-core/tests/toolpath.rs`

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Copy)] pub struct Vec3 { pub x: f32, pub y: f32, pub z: f32 }
  pub struct PathPoint { pub pos: Vec3, pub line_idx: usize, pub radius: f32, pub a_deg: f32, pub feed: f32, pub torch_on: bool, pub rapid: bool, pub layer: usize }
  pub struct Toolpath { pub segments: Vec<Vec<PathPoint>> /* separados em G0 ou mudança de Z */, pub min: Vec3, pub max: Vec3, pub layer_count: usize }
  pub fn build_toolpath(lines: &[String], p: &Params) -> Toolpath  // porta de _parse_gcode_points: cilíndrico (x, r*cos(a+90°), r*sin(a+90°)); chapa (x, y, z); bolacha (eixo = z, raio = x); interpola a cada 5° em G1; ignora G53, M*, comentários; camada = contagem de "(--- CAMADA"
  pub struct Series { pub x: Vec<f64>, pub z: Vec<f64>, pub a: Vec<f64> }
  pub fn temporal_series(lines: &[String]) -> Series
  pub fn oscillation_series(lines: &[String]) -> Vec<f64>
  pub fn step_distances(lines: &[String]) -> Vec<f64>        // hypot(dx, dz) por movimento, 0.0 no primeiro
  pub fn count_motions(lines: &[String]) -> usize
  ```

- [ ] **Step 1: Testes**: `spiral_toolpath_has_layers_and_radius` (golden `espiral_3camadas_de_ccw` -> `layer_count == 3`, raio médio por camada cresce 1.5), `flat_plate_is_planar`, `bolacha_maps_x_to_radius`, `temporal_series_lengths_match`, `step_distances_non_negative`.
- [ ] **Step 2: Rodar** — FAIL. **Step 3: Implementar**. **Step 4: Rodar** — PASS.
- [ ] **Step 5: Commit** `feat(core): toolpath 3D e séries para gráficos a partir do G-code`.

---

### Task 15: Esqueleto do app egui, tema, preferências e layout

**Files:**
- Create: `crates/tfm-app/Cargo.toml`, `crates/tfm-app/build.rs`, `crates/tfm-app/src/{main,app,theme,prefs}.rs`, `crates/tfm-app/src/panels/{mod,left,center,right,bottom}.rs` (placeholders)
- Modify: `Cargo.toml` (adicionar membro)

**Interfaces:**
- Produces:
  ```rust
  // theme.rs
  #[derive(Clone, Copy, PartialEq)] pub enum Theme { Auto, Claro, Escuro }  impl Theme { pub fn label(self) -> &'static str /* "Automático","Claro","Escuro" */; pub fn from_key(s: &str) -> Theme; pub fn key(self) -> &'static str; pub fn next(self) -> Theme }
  pub struct Palette { pub accent: Color32 /* #2563eb */, pub success: Color32 /* #16a34a */, pub warning: Color32 /* #ca8a04 */, pub danger: Color32 /* #dc2626 */, pub muted: Color32, pub panel_bg: Color32, pub card_bg: Color32, pub layer_colors: [Color32; 9], pub dark: bool }
  pub fn apply(ctx: &egui::Context, theme: Theme) -> Palette   // Visuals próprios (claro: fundo #f6f7f9, cartões #ffffff; escuro: fundo #15181d, cartões #1e2229), rounding 6, selection = accent
  pub fn load_fonts(ctx: &egui::Context)   // assets/fonts/*.ttf -> família "inter" no topo de Proportional, se existir
  // app.rs
  pub struct TfmApp { pub cfg: AppConfig, pub cfg_path: PathBuf, pub theme: Theme, pub palette: Palette, pub ui: UiState, pub quality: QualityManager, pub audit: AuditLogger, pub compute: ComputeHandle, pub result: Option<ComputeResult>, pub notifications: Vec<Notification>, pub dialogs: Dialogs, pub gcode_view: GcodeView, pub view3d: View3d, pub center_tab: CenterTab, pub left_open: bool, pub right_open: bool }
  pub enum NotifyKind { Info, Sucesso, Aviso, Erro }  pub struct Notification { pub time: String, pub kind: NotifyKind, pub text: String }
  impl TfmApp { pub fn new(cc: &eframe::CreationContext) -> Self; pub fn notify(&mut self, kind: NotifyKind, text: impl Into<String>) }
  impl eframe::App for TfmApp { fn update(..) /* TopBottomPanel::top("menu"); SidePanel::left("params").resizable(true).default_width(cfg.ui.left_w).width_range(300.0..=480.0) quando left_open; SidePanel::right("gcode").width_range(300.0..=520.0); TopBottomPanel::bottom("results"); CentralPanel */; fn save(..) /* grava larguras, tema e pinos em cfg.ui e salva config */ }
  ```
  `main.rs`: `eframe::run_native("TFM G-Code Generator v2.0.0 - Cladding & Hardfacing", NativeOptions { viewport: ViewportBuilder::default().with_maximized(true).with_min_inner_size([1024.0, 680.0]).with_icon(ícone de assets/icone.ico via image) })`. `build.rs` com `winres` para o ícone do `.exe` (só em `cfg(windows)`); `[[bin]] name = "TFM_GCODE" path = "src/main.rs"`.

- [ ] **Step 1: Criar a crate**: deps `tfm-core`, `eframe = "0.37"`, `egui_extras = { version = "0.37", features = ["image"] }`, `egui_plot` (versão compatível verificada com `cargo tree -i egui`), `rfd = "0.17"`, `chrono`, `image = { version = "0.25", default-features = false, features = ["ico","png"] }`, `log`, `env_logger`, `open = "5"`, `indexmap`; build-dep `winres = "0.1"`.
- [ ] **Step 2: Implementar `theme.rs`, `prefs.rs`, `app.rs`, `main.rs`** com os painéis mostrando placeholders, barra superior com os 6 menus (itens ainda sem ação), botão de tema (ciclo) e pinos (desfixado = painel recolhe para um botão na borda).
- [ ] **Step 3: Rodar** `cargo run -p tfm-app` em background por ~8 s e encerrar — Expected: janela abre sem pânico; log sem erros; após fechar, `config.json` tem `ui.theme` e `ui.left_w`.
- [ ] **Step 4: Commit** `feat(app): esqueleto eframe com tema claro/escuro/auto e painéis fixáveis`.

---

### Task 16: Widgets e ícones (`widgets.rs`, `icons.rs`)

**Files:**
- Create: `crates/tfm-app/src/widgets.rs` (testes inline), `crates/tfm-app/src/icons.rs`

**Interfaces:**
- Produces:
  ```rust
  pub fn parse_num(s: &str) -> Option<f64>    // "12,5" e "12.5", espaços; vazio/inválido -> None
  pub fn num_field(ui: &mut Ui, label: &str, value: &mut String, tip: &str, invalid: bool) -> bool  // rótulo + TextEdit 90 px; tooltip no rótulo e no campo; borda danger se invalid; retorna changed
  pub fn text_field(ui, label, &mut String, tip) -> bool
  pub fn check_field(ui, label, &mut bool, tip) -> bool
  pub fn combo_field<T: PartialEq + Copy>(ui, label, &mut T, options: &[(T, &str)], tip) -> bool
  pub fn section(ui: &mut Ui, title: &str, default_open: bool, add: impl FnOnce(&mut Ui))  // CollapsingHeader com título em negrito
  pub fn kpi(ui: &mut Ui, label: &str, value: &str, color: Option<Color32>)
  pub fn draw_operation_icon(painter: &Painter, rect: Rect, op: Operation, stroke: Color32, accent: Color32)  // porta dos 7 _draw_icon_* do Python em coordenadas 0..1
  ```

- [ ] **Step 1: Teste** `parse_num_accepts_comma` (`"12,5"` -> 12.5, `" 7 "` -> 7.0, `""` -> None, `"abc"` -> None, `"1e-3"` -> 0.001).
- [ ] **Step 2: Rodar** `cargo test -p tfm-app` — FAIL. **Step 3: Implementar**. **Step 4: Rodar** — PASS.
- [ ] **Step 5: Commit** `feat(app): widgets de campo com tooltip e ícones das 7 operações`.

---

### Task 17: Painel esquerdo — operações e abas Parâmetros / Processo / Rastreabilidade

**Files:**
- Create: `crates/tfm-app/src/ui_state.rs`; Modify: `crates/tfm-app/src/panels/left.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum LeftTab { Parametros, Processo, Rastreabilidade }
  pub struct UiState { pub operation: Operation, pub list_expanded: bool, pub tab: LeftTab, pub show_advanced: bool, pub fields: IndexMap<&'static str, String> /* chave de Params -> texto */, pub flags: IndexMap<&'static str, bool>, pub direcao: Direction, pub rotacao: Rotation, pub flat_orientation: FlatOrientation, pub multiperfil_strategy: MultiperfilStrategy, pub profile_shape: ProfileShape, pub bolacha_tipo: BolachaTipo, pub powder_name: String, pub presets: HashMap<Operation, Params>, pub invalid: HashSet<&'static str>, pub dirty: bool, pub last_edit: Option<Instant> }
  impl UiState { pub fn from_params(p: &Params) -> Self; pub fn to_params(&self, cfg: &AppConfig) -> Params /* porta de _get_current_params: pó -> powder_factor/powder_cost_brl_kg (0.16 e costs.powder_brl_kg se não achar); leads só se lead_extras_enabled; operacao_interna só em espiral/multiperfil/oscilação; multiperfil ajusta num_camadas */; pub fn set_operation(&mut self, op: Operation) /* porta de _set_revestimento_mode */; pub fn reset_defaults(&mut self) /* reset_mode_defaults */ }
  pub fn show_left(app: &mut TfmApp, ui: &mut Ui)
  ```
  Visibilidade (porta de `_refresh_*`): Diâmetro em todos menos Chapa; Largura (mm) só Chapa; Diâmetro interno só Bolacha e operações internas; Orientação da Solda e Unidirecional só Chapa; Comprimento Osc. e Desloc. Angular só oscilações; Granularidade X e A só Quadrada Contínua; bloco Multiperfil só Multiperfil; Tipo de Bolacha e Parar entre anéis só Bolacha; Operação interna + Afastamento da face só Espiral/Multiperfil/Oscilações; Fechar pontas só Espiral/Multiperfil; Início/Término Extra só com "Extras de entrada/saída" marcado; Avançado (Sobreposição Ang., atrasos da tocha, calibração) recolhido por padrão.
  Tooltips fixados (um por campo): Diâmetro "Diâmetro externo da peça onde o cordão será depositado. Define a circunferência e a rotação."; Comprimento "Trecho em X que recebe revestimento. Maior = mais voltas e mais tempo."; Largura Cordão "Largura real de um cordão depositado. Meça num teste; define o passo."; Sobreposição "Quanto um cordão cobre o vizinho, em %. Maior = passo menor, mais uniforme, mais tempo."; Afastamento Tocha "Distância da tocha até a peça em Z (stand-off)."; Afast. Segurança "Altura extra em Z para movimentos rápidos entre camadas."; Velocidade de deposição "Velocidade do cordão em mm/min. Maior = menos aporte térmico por mm."; Nº de Camadas "Quantas camadas sobrepostas. Cada camada sobe Z pela espessura."; Espessura / Camada "Altura que cada camada acrescenta."; Sentido da Soldagem "Direção do avanço em X."; Sentido da Rotação "Sentido do eixo A (CW/CCW), independente do avanço."; Comprimento Osc. "Curso da oscilação em X dentro de cada volta."; Desloc. Angular "Avanço angular entre oscilações, em % da largura do cordão."; Granularidade X "Tamanho do subpasso em X na quadrada contínua. Menor = mais linhas, movimento mais suave."; Granularidade A "Tamanho do subpasso em graus na quadrada contínua."; Quantidade de perfis "Quantos cordões helicoidais paralelos ao redor do eixo."; Ângulo total do setor "Setor angular ocupado pelos perfis. 360° distribui igualmente."; Passo por perfil "Avanço axial por volta de cada perfil."; Altura do perfil "Altura total do perfil; define quantas camadas."; Estratégia "Passes separados para entre perfis (M01); Percurso contínuo não para."; Ponto a ponto "Para (M0) antes de cada cordão para conferência."; Formato do perfil "Retangular, trapezoidal (base e topo) ou triangular."; Largura da base / do topo "Largura do perfil na base / no topo, em mm."; Chanfro nas pontas / Ângulo "Encurta as camadas superiores nas pontas para formar o ângulo informado."; Tipo de Bolacha "Espiral contínua de Arquimedes ou anéis concêntricos passo a passo."; Parar entre anéis "Desliga a tocha e sobe Z entre um anel e o próximo."; Sobreposição Ang. "Graus extras ao fim de cada volta para fechar a costura."; Atraso para ligar "Liga a saída da tocha e espera esse tempo antes de iniciar o cordão."; Atraso para desligar "Desliga a saída da tocha e mantém o movimento do cordão por esse tempo para compensar o apagamento."; Calibração Largura "Ajusta a largura efetiva do cordão (ex.: 0,95 reduz 5%, 1,05 aumenta 5%)."; Operação interna "Reveste o diâmetro interno; a tocha entra pela face e Z cresce para dentro."; Afastamento da face "Posição X segura fora da peça para entrar e sair em operações internas."; Fechar pontas "Dá uma volta completa parada no início e no fim para fechar o cordão."; Recuar Z e Y ao ligar "Vai ao Z e Y zero máquina (G53) antes de ligar a tocha."; Unidirecional "Cada cordão no mesmo sentido, com retorno rápido e tocha desligada."; Orientação da Solda "X: cordão em X, passo em Y. Y: cordão em Y, passo em X."; Camadas Inteligentes "Primeira camada (raiz) mais lenta, pelo % informado."; Corrente "Corrente do arco em A, usada no aporte térmico."; Tensão "Tensão do arco em V."; Vazão Gás "Argônio em l/min; entra no consumo e no custo."; Alimentação Pó "Ajuste do alimentador em %, só para registro."; Pré-aquecimento "Temperatura de pré-aquecimento da peça, só para registro."; Eficiência Térmica "Fator de rendimento do arco (0 a 1) no cálculo do heat input."; Taxa de deposição (g/h) "Massa de pó depositada por hora; usada nas fórmulas de tempo por massa."; Velocidade da oscilação "Velocidade dos movimentos em X nas oscilações."; Consumível (Pó) "Pó ativo: define fator de massa (g/mm²) e custo por kg."; Nome Procedimento / OS "Identificação gravada no cabeçalho e no procedimento salvo."; campos de rastreabilidade "Registrado no cabeçalho do G-code (ISO 3834)."; Temp. Amb. / Umidade "Condições ambientais registradas no procedimento."
  Aba Processo mostra também Heat Input (kJ/mm) e Diluição Est. (%) do `Estimates`.

- [ ] **Step 1: Implementar `ui_state.rs`** com testes inline `to_params_matches_defaults` (`UiState::from_params(&Params::default()).to_params(&cfg)` igual a `Params::default()` exceto `powder_*`) e `set_operation_restores_preset` (muda diâmetro na Espiral, vai para Bolacha e volta -> diâmetro preservado).
- [ ] **Step 2: Rodar** — FAIL; implementar; PASS.
- [ ] **Step 3: Implementar `left.rs`**: cartão da operação atual (ícone 40×35 + nome + botão "Padrão" + seta), lista recolhível com 7 cartões, abas com `selectable_value`, `ScrollArea::vertical`.
- [ ] **Step 4: Rodar o app**: trocar operação muda os campos; campo inválido fica vermelho; tooltips aparecem.
- [ ] **Step 5: Commit** `feat(app): painel de operações e parâmetros com tooltips didáticos`.

---

### Task 18: Pipeline de cálculo em thread e visualizador de G-code (painel direito)

**Files:**
- Create: `crates/tfm-app/src/compute.rs`; Modify: `crates/tfm-app/src/panels/right.rs`, `app.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct ComputeResult { pub params: Params, pub lines: Vec<String>, pub gen_error: Option<String>, pub field_errors: FieldErrors, pub invariants: Vec<String>, pub estimates: Estimates, pub toolpath: Toolpath, pub series: Series, pub osc: Vec<f64>, pub dists: Vec<f64>, pub key: u64 }
  pub struct ComputeHandle { .. }  impl ComputeHandle { pub fn new(ctx: egui::Context) -> Self /* thread + mpsc; chama ctx.request_repaint() ao concluir */; pub fn request(&mut self, params: Params, cfg: AppConfig, key: u64) /* substitui pedido pendente */; pub fn poll(&mut self) -> Option<ComputeResult>; pub fn busy(&self) -> bool }
  pub fn params_key(p: &Params, cfg: &AppConfig) -> u64
  // right.rs
  pub struct GcodeView { pub search: String, pub show_x: bool, pub show_y: bool, pub show_z: bool, pub show_a: bool, pub selected_line: Option<usize>, pub scroll_to: Option<usize> }
  pub fn show_right(app: &mut TfmApp, ui: &mut Ui)   // "Linhas: N"; busca com ▲ ▼; filtros X Y Z A (escondem linhas de movimento sem o eixo); LED da tocha ("Tocha ligada" success / "Tocha desligada" muted) conforme a linha selecionada; ScrollArea::vertical().show_rows com fonte monoespaçada; cores: comentário muted, G00 accent, G01 success, M101 success forte, M102 danger, M0/M01 warning, G53 warning; clique seleciona e destaca; botões Copiar tudo, Salvar .tap
  ```
  `app.rs`: se `ui.dirty` e `last_edit` há mais de 300 ms -> `compute.request(...)`; `poll()` atualiza `result`; `invariants` não vazias geram notificação de Aviso uma vez por `key`.

- [ ] **Step 1: Teste inline** em `compute.rs`: `request_then_poll_returns_result` (`Params::default()` + `AppConfig::default()` -> `lines.len() > 20` em até 2 s de polling).
- [ ] **Step 2: Rodar** — FAIL; implementar; PASS.
- [ ] **Step 3: Implementar `right.rs`** e ligar em `app.rs`.
- [ ] **Step 4: Rodar o app** com Quadrada Contínua, granularidade 0,1 e comprimento 300: UI responde durante o cálculo; rolagem fluida.
- [ ] **Step 5: Commit** `feat(app): cálculo em thread com debounce e visualizador de G-code`.

---

### Task 19: Painel inferior — resultados, custos e avisos

**Files:**
- Modify: `crates/tfm-app/src/panels/bottom.rs`

- [ ] **Step 1: Implementar** cabeçalho "Resultados e Custos" com ▲/▼ (persistido em `cfg.ui.bottom_collapsed`); três colunas: Resultados Calculados (rótulos "Rotação (min/volta):", "Passo Axial (mm):", "Rotações/Passos:", "Ângulo Total/Passo (A):", "Tempo Total (hh:mm):", "Largura Efetiva (mm):", "Passo Real (mm):", "Cobertura Estimada:", "Uniformidade:"; valor em danger quando `over_limit`; uniformidade colorida por nível), Estimativa de Custo ("Custo Consumíveis:", "Custo Operacional:", "CUSTO TOTAL:" em success negrito, "Consumo Pó (kg):", "Consumo Gás (m³):" com `{symbol} {:.2}`, `{:.3} kg`, `{:.3} m³`), Avisos (lista rolável `HH:MM:SS [INFO|SUCESSO|AVISO|ERRO] texto`, botão Limpar).
- [ ] **Step 2: Rodar o app**: valores mudam ao editar; recolher/expandir persiste.
- [ ] **Step 3: Commit** `feat(app): painel de resultados, custos e avisos`.

---

### Task 20: Visualizador 3D por software (`view3d.rs`)

**Files:**
- Create: `crates/tfm-app/src/view3d.rs`; Modify: `panels/center.rs` (aba "3D Interativo")

**Interfaces:**
- Produces:
  ```rust
  pub enum ViewPreset { Iso, Topo, Lado, Frente, Tras }
  pub struct Camera { pub yaw: f32, pub pitch: f32, pub distance: f32, pub target: Vec3 }
  impl Camera { pub fn preset(&mut self, v: ViewPreset); pub fn project(&self, p: Vec3, rect: Rect) -> Option<Pos2>; pub fn fit(&mut self, min: Vec3, max: Vec3) }
  pub struct Sim { pub playing: bool, pub progress: f32 /* 0..1 */, pub speed: f32, pub show_trace: bool, pub show_weld: bool }
  pub struct View3d { pub cam: Camera, pub sim: Sim, pub fit_pending: bool }
  pub fn show_view3d(ui: &mut Ui, view: &mut View3d, result: Option<&ComputeResult>, palette: &Palette, selected_line: Option<usize>) -> Option<usize>  // devolve linha clicada no 3D, se houver
  ```
  Interação: arrastar esquerdo = orbitar; direito/meio = pan; roda = zoom; duplo clique = enquadrar. Desenha grade, eixos XYZ (X danger, Y success, Z accent) com rótulos, silhueta da peça (cilindro: elipses + 4 geratrizes; chapa: retângulo; bolacha: anéis r_int/r_ext), trajetória por segmento colorida pela camada (`layer_colors[layer % 9]`), G0 em muted tracejado, tocha desligada mais clara, ponto da linha selecionada em destaque; simulação desenha até `progress` com marcador da tocha; barra: ▶ ⏸ ⏹, ISO Topo Lado Frente Trás, Traçado, Solda, Velocidade (0.5, 1, 2, 3, 5, 10, 20, 30, 50, 100).
- [ ] **Step 1: Testes inline** `project_target_lands_in_rect_center` (±1 px) e `preset_top_looks_down` (pitch ≈ 90°).
- [ ] **Step 2: Rodar** — FAIL; implementar; PASS.
- [ ] **Step 3: Rodar o app**: espiral de 3 camadas mostra 3 cores; simulação avança e para; presets funcionam.
- [ ] **Step 4: Commit** `feat(app): visualizador 3D interativo com simulação`.

---

### Task 21: Gráficos (`charts.rs`)

**Files:**
- Create: `crates/tfm-app/src/charts.rs`; Modify: `panels/center.rs` (abas Análise Temporal, Oscilação, Estatísticas, Processo)

**Interfaces:**
- Produces: `pub fn temporal(ui, &Series, &Palette)` (linhas X, Z, A tracejada; título "Análise Temporal (Coordenadas)"), `pub fn oscillation(ui, &[f64], &Palette)` (texto "Amplitude Aprox: {:.2} mm"), `pub fn statistics(ui, &[f64], &Palette)` (distância por passo + acumulado normalizado; histograma `bins = clamp(n/5, 10, 50)`; "Média: {:.2}mm  Máx: {:.2}mm"), `pub fn process_dashboard(ui, &Estimates, &Params, &Palette)` (rosca Pó/Gás/Mão de Obra/Máquina com total no centro e legenda em %; barras Pó (kg) e Gás (m³); esquema de camadas empilhadas com cordões sobrepostos pelo passo; cartão "HHh MMm" com barra).
- [ ] **Step 1: Implementar** com `egui_plot::{Plot, Line, BarChart}` e `Painter`.
- [ ] **Step 2: Rodar o app**: 4 abas corretas com Espiral e com Quadrada.
- [ ] **Step 3: Commit** `feat(app): gráficos de análise temporal, oscilação, estatísticas e dashboard de processo`.

---

### Task 22: Diálogos de configuração e qualidade

**Files:**
- Create: `crates/tfm-app/src/dialogs/{mod,costs,powders,formulas,machine_profiles,mach3_path,operators,wps,risk_matrix,maintenance,about,tutorial,shortcuts}.rs`

**Interfaces:**
- Produces: `pub enum DialogId { Costs, Powders, Formulas, MachineProfiles, Mach3Path, Operators, Wps, RiskMatrix, Maintenance, About, Tutorial, Shortcuts }`, `pub struct Dialogs { pub open: HashSet<DialogId>, /* estados de edição por diálogo */ }`, `pub fn show_dialogs(app: &mut TfmApp, ctx: &Context)` (cada um em `egui::Window::new(título).collapsible(false).resizable(true)`).
  Conteúdo: Custos (4 valores + símbolo, Salvar); Pós (tabela nome / fator g/mm² / custo R$/kg; Adicionar, Editar, Remover, Ativar, "Padrões PTA" = os 9 pós do config atual); Fórmulas (editor do JSON `expressions`, lista de variáveis com clique para inserir, Avaliar com tabela chave/valor e erros em danger, Formatar, Restaurar padrão, Importar, Exportar); Perfis de Máquina (combo + 4 limites, Novo, Salvar, Excluir); Caminho Mach3 (rfd para o exe, pasta de G-code, perfil, autoload, fallback; botão "Instalar macropump (autoload)" que chama `ensure_mach3_macros`); Operadores (lista; Nome, ID Qualificação, Validade AAAA-MM-DD; Adicionar, Remover); EPS (lista; "Nova vazia", "Nova a partir dos parâmetros atuais", limites min/max por parâmetro, Remover); Matriz de Riscos (5 linhas do Python: colisão por falha de homing, parâmetro fora da EPS, operador não qualificado, geometria inválida, falta de consumíveis); Manutenção (cards por componente com uso/limite e barra, Zerar, Editar limite, Adicionar componente; aba Histórico); Sobre ("TFM G-Code Generator v2.0.0\nDesenvolvido para TFM Usinagem & Manutenção Industrial LTDA.\nTodos os direitos reservados."); Tutorial (abas Introdução, Parâmetros, Qualidade, G-code com os textos do Python); Atalhos (Ctrl+S salvar procedimento, Ctrl+O carregar, Ctrl+G gerar G-code, Ctrl+M abrir no Mach3, Ctrl+Shift+C copiar G-code, F5 recalcular, F1 ajuda, Espaço play/pause, 1–5 vistas).
- [ ] **Step 1: Implementar** e ligar aos menus da barra superior com os rótulos do Python (Arquivo: Carregar Procedimento..., Salvar Procedimento..., Gerar Relatório PDF... (desabilitado, "em breve"), Sair; Configurações: Caminho (Mach3)..., Instalar macropump (autoload), Custos de Operação..., Tipos de Pó..., Fórmulas..., Perfis de Máquina...; Qualidade: Gerenciar Operadores..., Gerenciar EPS (WPS)..., Matriz de Riscos (ISO 3834)..., Manutenção Preventiva..., Abrir Pasta de Logs; Simulação: Vista Isométrica/Superior/Lateral/Frontal/Traseira, Simular, Pausar, Parar; G-code: Baixar G-code, Abrir no Mach3, Copiar tudo; Ajuda: Tutorial, Atalhos, Sobre).
- [ ] **Step 2: Rodar o app**: cada menu abre seu diálogo; salvar em Custos muda o custo total; Avaliar fórmulas lista 20 chaves.
- [ ] **Step 3: Commit** `feat(app): diálogos de configurações, qualidade ISO 3834, manutenção e ajuda`.

---

### Task 23: Ações — gerar .tap, Mach3, procedimentos, atalhos (`integration.rs`)

**Files:**
- Create: `crates/tfm-app/src/integration.rs`; Modify: `app.rs`

**Interfaces:**
- Produces:
  ```rust
  pub fn generate_tap(app: &mut TfmApp)  // validate_physical -> validate_iso3834 -> generate -> rfd save (.tap) -> se contém "G53": modal "SEGURANÇA DE MÁQUINA" com o texto do Python (Sim/Não) -> grava UTF-8 -> notify Sucesso -> audit.log("GCode Generation (Manual)", "Arquivo gerado manualmente: {nome} | EPS: {eps} | Op: {op}", operador ou "Engenharia")
  pub fn open_in_mach3(app: &mut TfmApp)  // porta de _open_in_mach3_clicked
  pub fn ensure_mach3_macros(cfg: &Integration) -> Vec<PathBuf>  // M101.m1s/M102.m1s em <Mach3>/macros/{perfil, Mach3Mill}; corrige "ActivateSinal" com backup .bak
  pub fn save_procedure_dialog(app: &mut TfmApp); pub fn load_procedure_dialog(app: &mut TfmApp)  // rfd em database.procedures_path; filtro "TFM Procedure Files" *.json
  pub fn copy_gcode(app: &mut TfmApp, ctx: &Context)
  pub fn handle_shortcuts(app: &mut TfmApp, ctx: &Context)
  ```
- [ ] **Step 1: Teste inline** `ensure_macros_writes_expected_content` (dir temporário -> `M101.m1s` contém `ActivateSignal(Output1)`; conteúdo com `ActivateSinal` é corrigido e `.bak` criado).
- [ ] **Step 2: Rodar** — FAIL; implementar; PASS.
- [ ] **Step 3: Rodar o app**: Ctrl+S salva JSON que o Python lê (`.venv/Scripts/python.exe -I -c "import json,sys; d=json.load(open(sys.argv[1])); print(d['welding_mode'])" arquivo`); Ctrl+G salva `.tap` idêntico ao painel.
- [ ] **Step 4: Commit** `feat(app): geração de .tap com alertas, integração Mach3, procedimentos e atalhos`.

---

### Task 24: Build release, documentação e revisão final

**Files:**
- Modify: `README.md`, `docs/README.md`

- [ ] **Step 1:** `cargo test --workspace` — todos PASS. `cargo clippy --workspace -- -D warnings` — limpo.
- [ ] **Step 2:** `cargo build -p tfm-app --release` — `target/release/TFM_GCODE.exe` existe, com ícone.
- [ ] **Step 3:** Rodar o exe; gerar `.tap` da Espiral padrão com os defaults; comparar com `tests/golden/espiral_padrao.gcode` ignorando as linhas `(Data:` e `( SOFTWARE:` — idêntico.
- [ ] **Step 4:** Atualizar `README.md` (o que é, `cargo build --release`, onde fica o exe, estrutura das crates, como regenerar fixtures, Python em `src/` é referência) e `docs/README.md` (tabela de módulos).
- [ ] **Step 5:** Capturar tela do app nos temas claro e escuro e enviar ao usuário.
- [ ] **Step 6: Commit** `docs: README da versão Rust + egui e instruções de build`.

---

## Ordem de execução e paralelismo

Sequencial: 1 -> 2 -> 3 -> 4 -> 5. Em paralelo (arquivos distintos): {6, 7, 8, 9}, {10}, {11}, {12}. Depois 13 (depende de 6, 11, 12) e 14 (depende de 3). UI: 15 -> 16 -> 17 -> 18 -> {19, 20, 21} -> 22 -> 23 -> 24.
