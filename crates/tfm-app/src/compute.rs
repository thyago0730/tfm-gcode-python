//! Geração de G-code em segundo plano.
//!
//! Um único trabalhador (thread) recebe sempre o pedido mais recente — um
//! pedido novo substitui o que ainda não começou — e calcula tudo o que os
//! painéis mostram: G-code, validação física, invariantes, estimativas,
//! trajetória 3D e séries dos gráficos. O resultado volta por canal; a UI só
//! faz `poll()` a cada quadro e nunca bloqueia. Ao terminar, o trabalhador
//! pede um repaint ao egui para o resultado aparecer sem esperar um evento.

use std::collections::hash_map::DefaultHasher;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::Instant;

use indexmap::IndexMap;
use serde::Serialize;
use tfm_core::config::AppConfig;
use tfm_core::estimates::{self, Estimates};
use tfm_core::gcode::{self, GenError};
use tfm_core::params::Params;
use tfm_core::toolpath::{self, Series, Toolpath};
use tfm_core::validator::{self, FieldErrors};

/// Pilha do trabalhador: árvores de fórmula profundas e programas grandes
/// (oscilação contínua com granularidade fina) nunca estouram a pilha padrão.
const WORKER_STACK: usize = 16 * 1024 * 1024;

/// Tudo o que uma rodada de cálculo produz para um conjunto de parâmetros.
#[derive(Clone, Debug)]
pub struct ComputeResult {
    /// Parâmetros usados nesta rodada (já com a operação aplicada).
    pub params: Params,
    /// Linhas do G-code (vazio quando a geração falhou).
    pub lines: Vec<String>,
    /// Motivo da falha de geração, em português, quando houve.
    pub gen_error: Option<String>,
    /// Regras físicas violadas e os campos por trás delas.
    pub field_errors: FieldErrors,
    /// Avisos dos invariantes do G-code gerado (vazio = tudo certo).
    pub invariants: Vec<String>,
    /// Resultados calculados e custos (placeholders vazios se inválido).
    pub estimates: Estimates,
    pub toolpath: Toolpath,
    pub series: Series,
    /// Série da oscilação (deslocamento lateral por passo).
    pub osc: Vec<f64>,
    /// Distância percorrida em cada movimento.
    pub dists: Vec<f64>,
    /// [`params_key`] dos parâmetros + config que geraram este resultado.
    pub key: u64,
}

impl ComputeResult {
    /// Resultado vazio com uma mensagem de erro (geração impossível).
    pub fn failed(params: Params, key: u64, message: impl Into<String>) -> Self {
        ComputeResult {
            field_errors: validator::validate_physical(&params),
            params,
            lines: Vec::new(),
            gen_error: Some(message.into()),
            invariants: Vec::new(),
            estimates: empty_estimates(),
            toolpath: Toolpath::default(),
            series: Series::default(),
            osc: Vec::new(),
            dists: Vec::new(),
            key,
        }
    }

    /// `true` quando há G-code gerado sem erro.
    pub fn is_ok(&self) -> bool {
        self.gen_error.is_none() && !self.lines.is_empty()
    }
}

/// Estimativas "em branco" para parâmetros que não validam: textos vazios e
/// zeros, no lugar dos placeholders `"0.00"`/`"..."` que o painel decide.
pub fn empty_estimates() -> Estimates {
    Estimates {
        rotation: Default::default(),
        pitch: Default::default(),
        rotations: String::new(),
        total_angle: String::new(),
        time_hhmm: String::new(),
        time_hours: 0.0,
        overlap: Default::default(),
        costs: Default::default(),
        formulas: IndexMap::new(),
        heat_input_kj_mm: 0.0,
        dilution_pct: 0.0,
    }
}

/// Faz toda a rodada de cálculo de forma síncrona (é o que o trabalhador
/// executa; também serve a testes e a quem precise do resultado na hora).
///
/// Quando a geração falha, `lines` fica vazio e `gen_error` traz o motivo;
/// `estimates` só é calculado com parâmetros fisicamente válidos (o motor
/// assume isso), senão vai em branco. Trajetória e séries saem do que houver.
pub fn compute_all(params: Params, cfg: &AppConfig, key: u64) -> ComputeResult {
    let field_errors = validator::validate_physical(&params);
    let (lines, gen_error) = match gcode::generate(&params) {
        Ok(lines) => (lines, None),
        Err(GenError::Invalid(message)) => (Vec::new(), Some(message)),
        Err(err) => (Vec::new(), Some(err.to_string())),
    };
    let invariants = if lines.is_empty() {
        Vec::new()
    } else {
        validator::gcode_invariants(&lines, &params)
    };
    let estimates = if field_errors.is_valid() {
        estimates::compute(&params, cfg)
    } else {
        empty_estimates()
    };
    let toolpath = toolpath::build_toolpath(&lines, &params);
    let series = toolpath::temporal_series(&lines);
    let osc = toolpath::oscillation_series(&lines);
    let dists = toolpath::step_distances(&lines);
    ComputeResult {
        params,
        lines,
        gen_error,
        field_errors,
        invariants,
        estimates,
        toolpath,
        series,
        osc,
        dists,
        key,
    }
}

/// Chave que identifica uma rodada: hash do JSON dos parâmetros e da parte
/// do config que entra no cálculo (custos, fórmulas, pós e pó ativo, perfis
/// de máquina e perfil ativo). Preferências de interface não entram.
pub fn params_key(p: &Params, cfg: &AppConfig) -> u64 {
    let mut hasher = DefaultHasher::new();
    hash_json(&mut hasher, p);
    hash_json(&mut hasher, &cfg.costs);
    hash_json(&mut hasher, &cfg.formulas);
    hash_json(&mut hasher, &cfg.powders);
    cfg.active_powder.hash(&mut hasher);
    hash_json(&mut hasher, &cfg.machine_profiles);
    cfg.active_machine_profile.hash(&mut hasher);
    hasher.finish()
}

/// Alimenta o hasher com o JSON do valor; se a serialização falhar (não
/// deveria), usa a representação `Debug` para a chave continuar estável.
fn hash_json<T: Serialize + Debug + ?Sized>(hasher: &mut DefaultHasher, value: &T) {
    match serde_json::to_string(value) {
        Ok(json) => json.hash(hasher),
        Err(_) => format!("{value:?}").hash(hasher),
    }
}

// ---------------------------------------------------------------------------
// Trabalhador
// ---------------------------------------------------------------------------

/// Um pedido de cálculo.
struct Job {
    params: Params,
    cfg: AppConfig,
    key: u64,
}

/// Fila de um só lugar: o pedido novo substitui o pendente.
#[derive(Default)]
struct Queue {
    pending: Option<Job>,
    /// O trabalhador está no meio de um cálculo.
    running: bool,
    /// A alça foi descartada: o trabalhador deve encerrar.
    shutdown: bool,
}

#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
}

/// Um mutex envenenado (pânico com a trava presa) ainda guarda dados
/// coerentes aqui: seguimos com eles em vez de derrubar a UI.
fn lock(queue: &Mutex<Queue>) -> MutexGuard<'_, Queue> {
    queue
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Alça para o trabalhador de cálculo: faz pedidos e recolhe resultados.
pub struct ComputeHandle {
    shared: Arc<Shared>,
    rx: Receiver<ComputeResult>,
    /// Só quando a thread não pôde ser criada: calcula na própria chamada e
    /// entrega pelo mesmo canal, para o app continuar funcionando.
    inline_tx: Option<Sender<ComputeResult>>,
    last_requested: Option<u64>,
}

impl ComputeHandle {
    /// Cria o trabalhador. `ctx` é usado para pedir repaint ao concluir.
    pub fn new(ctx: egui::Context) -> Self {
        let shared = Arc::new(Shared::default());
        let (tx, rx) = mpsc::channel();
        let worker_shared = Arc::clone(&shared);
        let worker_tx = tx.clone();
        let spawned = thread::Builder::new()
            .name("tfm-compute".to_string())
            .stack_size(WORKER_STACK)
            .spawn(move || worker_loop(worker_shared, worker_tx, ctx));
        let inline_tx = match spawned {
            Ok(_) => None,
            Err(err) => {
                log::error!("Não foi possível criar a thread de cálculo ({err}); calculando na UI");
                Some(tx)
            }
        };
        ComputeHandle {
            shared,
            rx,
            inline_tx,
            last_requested: None,
        }
    }

    /// Pede um cálculo. Substitui qualquer pedido ainda não iniciado; um
    /// cálculo em andamento termina e o seu resultado é descartado pelo
    /// `poll()` se um mais novo já tiver chegado.
    pub fn request(&mut self, params: Params, cfg: AppConfig, key: u64) {
        self.last_requested = Some(key);
        if let Some(tx) = &self.inline_tx {
            let result = run_job(Job { params, cfg, key });
            if tx.send(result).is_err() {
                log::error!("Canal de resultados fechado");
            }
            return;
        }
        {
            let mut queue = lock(&self.shared.queue);
            if queue.pending.is_some() {
                log::debug!("Pedido de cálculo {key:#018x} substitui o pendente");
            }
            queue.pending = Some(Job { params, cfg, key });
        }
        self.shared.wake.notify_one();
    }

    /// Resultado mais novo pronto, se houver (resultados mais antigos que
    /// ainda estavam na fila são descartados).
    pub fn poll(&mut self) -> Option<ComputeResult> {
        let mut newest = None;
        while let Ok(result) = self.rx.try_recv() {
            newest = Some(result);
        }
        newest
    }

    /// Há pedido na fila ou cálculo em andamento.
    pub fn busy(&self) -> bool {
        let queue = lock(&self.shared.queue);
        queue.pending.is_some() || queue.running
    }

    /// Chave do último pedido feito (para saber se um resultado é o atual).
    pub fn last_requested(&self) -> Option<u64> {
        self.last_requested
    }
}

impl Drop for ComputeHandle {
    fn drop(&mut self) {
        lock(&self.shared.queue).shutdown = true;
        self.shared.wake.notify_all();
    }
}

/// Laço do trabalhador: espera um pedido, calcula, entrega, pede repaint.
fn worker_loop(shared: Arc<Shared>, tx: Sender<ComputeResult>, ctx: egui::Context) {
    loop {
        let job = {
            let mut queue = lock(&shared.queue);
            loop {
                if queue.shutdown {
                    return;
                }
                if let Some(job) = queue.pending.take() {
                    queue.running = true;
                    break job;
                }
                queue = shared
                    .wake
                    .wait(queue)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        };
        let key = job.key;
        let started = Instant::now();
        let result = run_job(job);
        log::debug!(
            "Cálculo {key:#018x} concluído em {:?}: {} linhas, {} avisos",
            started.elapsed(),
            result.lines.len(),
            result.invariants.len()
        );
        let delivered = tx.send(result).is_ok();
        lock(&shared.queue).running = false;
        ctx.request_repaint();
        if !delivered {
            // A alça foi descartada: ninguém mais vai ler.
            return;
        }
    }
}

/// Executa um pedido protegendo a thread: um pânico no motor vira um
/// resultado com erro em vez de deixar a UI esperando para sempre.
fn run_job(job: Job) -> ComputeResult {
    let Job { params, cfg, key } = job;
    let params_backup = params.clone();
    match panic::catch_unwind(AssertUnwindSafe(|| compute_all(params, &cfg, key))) {
        Ok(result) => result,
        Err(payload) => {
            let message = panic_message(payload.as_ref());
            log::error!("Pânico no cálculo {key:#018x}: {message}");
            ComputeResult::failed(
                params_backup,
                key,
                format!("Falha interna no cálculo: {message}"),
            )
        }
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "erro desconhecido".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::{Duration, Instant};
    use tfm_core::config::AppConfig;
    use tfm_core::params::Params;

    /// Pede um cálculo com os parâmetros padrão e espera o resultado chegar
    /// pelo `poll()` (até 2 s, de 20 em 20 ms).
    #[test]
    fn request_then_poll_returns_result() {
        let mut handle = ComputeHandle::new(egui::Context::default());
        let params = Params::default();
        let cfg = AppConfig::default();
        let key = params_key(&params, &cfg);
        handle.request(params, cfg, key);

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut result = None;
        while Instant::now() < deadline {
            if let Some(r) = handle.poll() {
                result = Some(r);
                break;
            }
            sleep(Duration::from_millis(20));
        }
        let result = result.expect("o trabalhador não devolveu resultado em 2 s");
        assert!(
            result.lines.len() > 20,
            "esperava mais de 20 linhas, veio {}",
            result.lines.len()
        );
        assert!(
            result.gen_error.is_none(),
            "gen_error = {:?}",
            result.gen_error
        );
        assert_eq!(result.key, key);
        assert!(
            !handle.busy(),
            "sem pedido pendente o trabalhador deve ficar ocioso"
        );
    }

    /// Dois pedidos seguidos: o `poll()` devolve só o mais novo.
    #[test]
    fn poll_drops_stale_results() {
        let mut handle = ComputeHandle::new(egui::Context::default());
        let cfg = AppConfig::default();
        let a = Params::default();
        let mut b = Params::default();
        b.comprimento_revestir += 10.0;
        let key_a = params_key(&a, &cfg);
        let key_b = params_key(&b, &cfg);
        assert_ne!(
            key_a, key_b,
            "parâmetros diferentes devem ter chaves diferentes"
        );
        handle.request(a, cfg.clone(), key_a);
        handle.request(b, cfg, key_b);

        let deadline = Instant::now() + Duration::from_secs(4);
        let mut newest = None;
        while Instant::now() < deadline {
            if let Some(r) = handle.poll() {
                newest = Some(r);
            }
            if newest.is_some() && !handle.busy() {
                break;
            }
            sleep(Duration::from_millis(20));
        }
        let newest = newest.expect("nenhum resultado em 4 s");
        assert_eq!(
            newest.key, key_b,
            "o último resultado deve ser do pedido mais novo"
        );
    }

    /// Cenário pesado do roteiro (Quadrada Contínua, granularidade 0,1 mm,
    /// comprimento 300 mm): o trabalhador termina sem estourar a pilha e o
    /// programa sai grande. Ignorado por padrão por ser lento em debug;
    /// rode com `cargo test -p tfm-app heavy -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn heavy_square_continuous_completes() {
        use tfm_core::params::Operation;
        let mut params = Params::default();
        params.apply_operation(Operation::OscilacaoQuadradaContinua);
        params.osc_test_gran_x = 0.1;
        params.comprimento_revestir = 300.0;
        let cfg = AppConfig::default();
        let key = params_key(&params, &cfg);
        let mut handle = ComputeHandle::new(egui::Context::default());
        let started = Instant::now();
        handle.request(params, cfg, key);
        let deadline = Instant::now() + Duration::from_secs(600);
        let mut result = None;
        while Instant::now() < deadline {
            if let Some(r) = handle.poll() {
                result = Some(r);
                break;
            }
            sleep(Duration::from_millis(50));
        }
        let result = result.expect("cenário pesado não terminou em 10 min");
        eprintln!(
            "cenário pesado: {} linhas, {} pontos 3D, {:?}, erro: {:?}",
            result.lines.len(),
            result.toolpath.point_count(),
            started.elapsed(),
            result.gen_error
        );
        assert!(
            result.gen_error.is_none(),
            "gen_error = {:?}",
            result.gen_error
        );
        assert!(
            result.lines.len() > 1000,
            "veio só {} linhas",
            result.lines.len()
        );
    }

    /// A chave muda com os parâmetros e com a parte relevante do config, mas
    /// não com as preferências de interface.
    #[test]
    fn params_key_tracks_params_and_relevant_config() {
        let p = Params::default();
        let cfg = AppConfig::default();
        let base = params_key(&p, &cfg);
        assert_eq!(
            base,
            params_key(&p, &cfg),
            "a chave deve ser determinística"
        );

        let mut cfg_ui = cfg.clone();
        cfg_ui.ui.left_w += 10.0;
        assert_eq!(
            base,
            params_key(&p, &cfg_ui),
            "preferências de UI não entram na chave"
        );

        let mut cfg_costs = cfg.clone();
        cfg_costs.costs.powder_brl_kg += 1.0;
        assert_ne!(base, params_key(&p, &cfg_costs), "custos entram na chave");

        let mut cfg_powder = cfg.clone();
        cfg_powder.active_powder = "outro-po".to_string();
        assert_ne!(base, params_key(&p, &cfg_powder), "pó ativo entra na chave");
    }
}
