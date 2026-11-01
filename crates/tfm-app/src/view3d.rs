//! Visualização 3D interativa do percurso da tocha (aba "3D Interativo").
//!
//! Renderização por software com o `Painter` do egui, sem GPU própria: uma
//! câmera orbital com perspectiva simples ([`Camera`]) projeta a cena e a
//! aba desenha, nesta ordem, a grade do chão, os eixos XYZ, a silhueta da
//! peça (cilindro, chapa ou bolacha) e a trajetória colorida por camada.
//! A simulação ([`Sim`]) desenha o percurso até `progress` e marca a tocha.
//!
//! O `TfmApp` só guarda o marcador [`View3d`]; o estado de verdade
//! ([`ViewState`]) vive na memória do egui e é acessado por [`with_state`]:
//! assim o menu "Simulação" aciona presets e ▶ ⏸ ⏹ sem tocar no `app.rs`.
//!
//! Convenção da cena, a mesma do `tfm_core::toolpath`: X é o eixo da peça
//! (axial), Z aponta para cima (altura da tocha na chapa plana) e Y completa
//! o triedro. A grade do chão fica no plano XY logo abaixo da peça.

use egui::{
    pos2, vec2, Align2, CornerRadius, FontId, Id, Painter, PointerButton, Pos2, Rect, Response,
    Sense, Shape, Slider, Stroke, Ui, Vec2,
};
use tfm_core::params::{FlatOrientation, Operation, Params, WeldingMode};
use tfm_core::toolpath::{PathPoint, Toolpath, Vec3};

use crate::app::TfmApp;
use crate::theme::Palette;

/// Velocidades da simulação (multiplicador do ritmo base), na ordem do slider.
pub const SPEEDS: [f32; 10] = [0.5, 1.0, 2.0, 3.0, 5.0, 10.0, 20.0, 30.0, 50.0, 100.0];
/// Pontos por segundo em 1x: ~1000 pontos a cada 10 s.
pub const BASE_POINTS_PER_SEC: f32 = 100.0;
/// Acima disto a trajetória é decimada uniformemente para o desenho.
pub const MAX_DRAW_POINTS: usize = 200_000;
/// Limite de círculos de "Solda" por quadro.
const MAX_WELD_DOTS: usize = 20_000;
/// Distâncias permitidas da câmera ao alvo (mm).
pub const MIN_DISTANCE: f32 = 1.0;
pub const MAX_DISTANCE: f32 = 1.0e6;
/// Campo de visão (graus) na menor dimensão do retângulo.
const FOV_DEG: f32 = 40.0;
/// Folga do enquadramento em torno da esfera envolvente.
const FIT_MARGIN: f32 = 1.15;
/// Graus de órbita por pixel arrastado.
const ORBIT_DEG_PER_PX: f32 = 0.4;
/// Limite do pitch (graus): evita o salto no polo.
const PITCH_LIMIT: f32 = 89.0;
/// Expoente do zoom por unidade de rolagem (multiplicativo).
const WHEEL_ZOOM: f32 = 0.0025;
/// Raio (px) dentro do qual um clique seleciona um ponto.
const PICK_RADIUS: f32 = 10.0;
/// Segmentos de reta por círculo da silhueta.
const CIRCLE_SEGMENTS: usize = 48;

/// Marcador guardado pelo `TfmApp`; o estado real é [`ViewState`], na memória
/// do egui (ver [`with_state`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct View3d;

// ---------------------------------------------------------------------------
// Câmera
// ---------------------------------------------------------------------------

/// Vistas pré-definidas (botões da barra e menu "Simulação").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewPreset {
    Iso,
    Topo,
    Lado,
    Frente,
    Tras,
}

impl ViewPreset {
    pub const ALL: [ViewPreset; 5] = [
        ViewPreset::Iso,
        ViewPreset::Topo,
        ViewPreset::Lado,
        ViewPreset::Frente,
        ViewPreset::Tras,
    ];

    /// Rótulo do botão da barra.
    pub fn label(self) -> &'static str {
        match self {
            ViewPreset::Iso => "ISO",
            ViewPreset::Topo => "Topo",
            ViewPreset::Lado => "Lado",
            ViewPreset::Frente => "Frente",
            ViewPreset::Tras => "Trás",
        }
    }

    /// Texto de ajuda (o mesmo nome dos itens do menu "Simulação").
    pub fn hint(self) -> &'static str {
        match self {
            ViewPreset::Iso => "Vista Isométrica",
            ViewPreset::Topo => "Vista Superior",
            ViewPreset::Lado => "Vista Lateral",
            ViewPreset::Frente => "Vista Frontal",
            ViewPreset::Tras => "Vista Traseira",
        }
    }

    /// `(yaw, pitch)` em graus. Yaw 0 = câmera em -Y olhando para +Y (X
    /// para a direita, Z para cima); yaw -90° = câmera em -X (vê a face da
    /// peça); pitch 89° = olhando de cima.
    pub fn angles(self) -> (f32, f32) {
        match self {
            ViewPreset::Iso => (-45.0, 25.0),
            ViewPreset::Topo => (0.0, 89.0),
            ViewPreset::Lado => (0.0, 0.0),
            ViewPreset::Frente => (-90.0, 0.0),
            ViewPreset::Tras => (90.0, 0.0),
        }
    }
}

/// Câmera orbital: gira em torno de `target` a `distance` mm, com `yaw`
/// (em torno de Z) e `pitch` (elevação), ambos em graus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: Vec3,
}

impl Default for Camera {
    fn default() -> Self {
        let (yaw, pitch) = ViewPreset::Iso.angles();
        Camera {
            yaw,
            pitch,
            distance: 300.0,
            target: Vec3::default(),
        }
    }
}

/// Posição e triedro da câmera, derivados de yaw/pitch/distance/target.
#[derive(Clone, Copy, Debug)]
struct Basis {
    eye: Vec3,
    right: Vec3,
    up: Vec3,
    forward: Vec3,
}

impl Camera {
    /// Aplica os ângulos de uma vista pré-definida (mantém alvo e distância).
    pub fn preset(&mut self, v: ViewPreset) {
        let (yaw, pitch) = v.angles();
        self.yaw = yaw;
        self.pitch = pitch;
    }

    fn basis(&self) -> Basis {
        let (sy, cy) = self.yaw.to_radians().sin_cos();
        let (sp, cp) = self.pitch.to_radians().sin_cos();
        let forward = Vec3::new(-sy * cp, cy * cp, -sp);
        let right = Vec3::new(cy, sy, 0.0);
        let up = Vec3::new(-sy * sp, cy * sp, cp);
        let eye = sub(self.target, scale(forward, self.distance));
        Basis {
            eye,
            right,
            up,
            forward,
        }
    }

    /// Plano próximo: o que fica mais perto da câmera que isto não é desenhado.
    fn near(&self) -> f32 {
        (self.distance * 1e-3).max(1e-3)
    }

    /// Projeta `p` em coordenadas de tela dentro de `rect`; `None` quando o
    /// ponto fica atrás da câmera (ou não é finito). O alvo cai no centro.
    pub fn project(&self, p: Vec3, rect: Rect) -> Option<Pos2> {
        Projector::new(self, rect).project(p)
    }

    /// Enquadra a caixa `min..max`: alvo no centro e distância tal que a
    /// esfera envolvente caiba no campo de visão com folga.
    pub fn fit(&mut self, min: Vec3, max: Vec3) {
        let center = scale(add(min, max), 0.5);
        let half = length(sub(max, min)) * 0.5;
        let radius = if half.is_finite() && half > 1e-3 {
            half
        } else {
            10.0
        };
        let distance = radius / (FOV_DEG * 0.5).to_radians().sin() * FIT_MARGIN;
        if is_finite(center) {
            self.target = center;
        }
        self.distance = distance.clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    /// Órbita por arrasto (pixels): arrastar para a direita gira a cena para
    /// a direita; para baixo, inclina a câmera para cima. Pitch limitado a
    /// ±89° e yaw mantido em [-180°, 180°).
    pub fn orbit(&mut self, delta: Vec2) {
        if !delta.is_finite() {
            return;
        }
        self.yaw = wrap_deg(self.yaw - delta.x * ORBIT_DEG_PER_PX);
        self.pitch = (self.pitch + delta.y * ORBIT_DEG_PER_PX).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// Desloca o alvo no plano da tela, de modo que a cena acompanhe o
    /// ponteiro (pixels convertidos em mm na profundidade do alvo).
    pub fn pan(&mut self, delta: Vec2, rect: Rect) {
        if !delta.is_finite() {
            return;
        }
        let b = self.basis();
        let k = self.distance / focal(rect);
        let shift = add(scale(b.right, -delta.x * k), scale(b.up, delta.y * k));
        let target = add(self.target, shift);
        if is_finite(target) {
            self.target = target;
        }
    }

    /// Zoom multiplicativo: `factor < 1` aproxima, `> 1` afasta.
    pub fn zoom(&mut self, factor: f32) {
        let distance = self.distance * factor;
        if distance.is_nan() {
            return;
        }
        self.distance = distance.clamp(MIN_DISTANCE, MAX_DISTANCE);
    }
}

/// Distância focal (px) para `rect`: o campo de visão vale na menor dimensão,
/// então o que o `fit` enquadra cabe tanto na largura quanto na altura.
fn focal(rect: Rect) -> f32 {
    let side = rect.width().min(rect.height()).max(1.0);
    0.5 * side / (FOV_DEG * 0.5).to_radians().tan()
}

/// Projeção de uma câmera num retângulo, calculada uma vez por quadro.
#[derive(Clone, Copy, Debug)]
pub struct Projector {
    basis: Basis,
    focal: f32,
    center: Pos2,
    near: f32,
}

impl Projector {
    pub fn new(cam: &Camera, rect: Rect) -> Self {
        Projector {
            basis: cam.basis(),
            focal: focal(rect),
            center: rect.center(),
            near: cam.near(),
        }
    }

    /// Coordenadas no espaço da câmera: `(direita, cima, profundidade)`.
    fn cam_coords(&self, p: Vec3) -> Vec3 {
        let v = sub(p, self.basis.eye);
        Vec3::new(
            dot(v, self.basis.right),
            dot(v, self.basis.up),
            dot(v, self.basis.forward),
        )
    }

    /// Coordenadas de tela de um ponto já no espaço da câmera.
    fn screen_pos(&self, c: Vec3) -> Option<Pos2> {
        if c.z.is_nan() || c.z <= self.near {
            return None;
        }
        let s = pos2(
            self.center.x + c.x * self.focal / c.z,
            self.center.y - c.y * self.focal / c.z,
        );
        (s.x.is_finite() && s.y.is_finite()).then_some(s)
    }

    /// Ponto em coordenadas de tela; `None` atrás da câmera.
    pub fn project(&self, p: Vec3) -> Option<Pos2> {
        self.screen_pos(self.cam_coords(p))
    }

    /// Segmento em coordenadas de tela. A parte que fica atrás da câmera é
    /// recortada no plano próximo; `None` quando os dois extremos ficam atrás.
    pub fn segment(&self, a: Vec3, b: Vec3) -> Option<[Pos2; 2]> {
        let (mut ca, mut cb) = (self.cam_coords(a), self.cam_coords(b));
        let (front_a, front_b) = (ca.z > self.near, cb.z > self.near);
        match (front_a, front_b) {
            (false, false) => return None,
            (true, true) => {}
            _ => {
                let t = (self.near - ca.z) / (cb.z - ca.z);
                if !t.is_finite() {
                    return None;
                }
                let clip = lerp(ca, cb, t);
                let clip = Vec3::new(clip.x, clip.y, self.near * 1.001);
                if front_a {
                    cb = clip;
                } else {
                    ca = clip;
                }
            }
        }
        Some([self.screen_pos(ca)?, self.screen_pos(cb)?])
    }
}

// ---------------------------------------------------------------------------
// Vetores (o `Vec3` do núcleo não tem operadores)
// ---------------------------------------------------------------------------

fn add(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

fn scale(a: Vec3, k: f32) -> Vec3 {
    Vec3::new(a.x * k, a.y * k, a.z * k)
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn length(a: Vec3) -> f32 {
    dot(a, a).sqrt()
}

fn lerp(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    add(a, scale(sub(b, a), t))
}

fn is_finite(a: Vec3) -> bool {
    a.x.is_finite() && a.y.is_finite() && a.z.is_finite()
}

/// Leva um ângulo em graus para [-180, 180).
fn wrap_deg(a: f32) -> f32 {
    (a + 180.0).rem_euclid(360.0) - 180.0
}

/// `v` como `f32`, ou `default` se não for finito.
fn finite_or(v: f64, default: f32) -> f32 {
    let v = v as f32;
    if v.is_finite() {
        v
    } else {
        default
    }
}

/// Passo "redondo" (1, 2 ou 5 × 10^k) próximo de `raw`; 10 para entradas
/// inválidas.
fn nice_step(raw: f32) -> f32 {
    if !(raw.is_finite() && raw > 0.0) {
        return 10.0;
    }
    let base = 10f32.powf(raw.log10().floor());
    let m = raw / base;
    let mult = if m < 1.5 {
        1.0
    } else if m < 3.5 {
        2.0
    } else if m < 7.5 {
        5.0
    } else {
        10.0
    };
    base * mult
}

/// Passo da decimação uniforme: 1 até [`MAX_DRAW_POINTS`], senão o menor
/// passo que deixa no máximo esse total.
pub fn decimation_step(points: usize) -> usize {
    if points <= MAX_DRAW_POINTS {
        1
    } else {
        points.div_ceil(MAX_DRAW_POINTS)
    }
}

/// Texto da velocidade: `0,5x`, `1x`, `100x`.
pub fn speed_label(speed: f32) -> String {
    if speed.fract() == 0.0 {
        format!("{}x", speed as i64)
    } else {
        format!("{speed:.1}x").replace('.', ",")
    }
}

// ---------------------------------------------------------------------------
// Simulação
// ---------------------------------------------------------------------------

/// Estado da simulação. `progress` em 0..=1 é a fração dos pontos do
/// percurso já "soldados"; `speed` é um dos valores de [`SPEEDS`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sim {
    pub playing: bool,
    pub progress: f32,
    pub speed: f32,
    pub show_trace: bool,
    pub show_weld: bool,
}

impl Default for Sim {
    fn default() -> Self {
        Sim {
            playing: false,
            progress: 0.0,
            speed: 1.0,
            show_trace: true,
            show_weld: false,
        }
    }
}

impl Sim {
    /// ▶: começa (ou continua); se já chegou ao fim, recomeça do zero.
    pub fn play(&mut self) {
        if self.progress >= 1.0 {
            self.progress = 0.0;
        }
        self.playing = true;
    }

    /// ⏸: congela onde está.
    pub fn pause(&mut self) {
        self.playing = false;
    }

    /// ⏹: para e volta ao início (o percurso inteiro volta a aparecer).
    pub fn stop(&mut self) {
        self.playing = false;
        self.progress = 0.0;
    }

    /// Em andamento ou pausada no meio: o desenho vai só até `progress`.
    /// Parada no zero, a aba mostra o percurso inteiro.
    pub fn active(&self) -> bool {
        self.playing || self.progress > 0.0
    }

    /// Avança `dt` segundos: 1x percorre [`BASE_POINTS_PER_SEC`] pontos por
    /// segundo. Ao chegar ao fim, para.
    pub fn advance(&mut self, dt: f32, total_points: usize) {
        if !(self.playing && dt.is_finite() && dt > 0.0) {
            return;
        }
        if total_points == 0 {
            self.playing = false;
            return;
        }
        let step = dt * self.speed * BASE_POINTS_PER_SEC / total_points as f32;
        self.progress = (self.progress + step).min(1.0);
        if self.progress >= 1.0 {
            self.playing = false;
        }
    }

    /// Posição de `speed` em [`SPEEDS`] (1x se não estiver na tabela).
    pub fn speed_index(&self) -> usize {
        SPEEDS
            .iter()
            .position(|s| (*s - self.speed).abs() < 1e-6)
            .unwrap_or(1)
    }

    /// Escolhe a velocidade pela posição na tabela (saturando no último).
    pub fn set_speed_index(&mut self, idx: usize) {
        self.speed = SPEEDS[idx.min(SPEEDS.len() - 1)];
    }
}

// ---------------------------------------------------------------------------
// Estado do visualizador
// ---------------------------------------------------------------------------

/// Estado completo da aba, persistente entre quadros na memória do egui.
#[derive(Clone, Debug)]
pub struct ViewState {
    pub cam: Camera,
    pub sim: Sim,
    /// Enquadrar a cena no próximo quadro em que houver resultado. Começa
    /// ligado e religa a cada resultado novo enquanto o usuário não ajustou
    /// a câmera (ver [`Self::user_adjusted`]) ou quando a operação muda.
    pub fit_pending: bool,
    /// O usuário orbitou, moveu ou deu zoom: recálculos do mesmo tipo de peça
    /// não reenquadram mais (o zoom/pan sobrevive a cada ajuste de parâmetro).
    /// Volta a `false` no duplo clique, em [`Self::request_fit`], nos presets
    /// e quando a operação muda.
    pub user_adjusted: bool,
    /// Chave do último resultado visto.
    last_key: Option<u64>,
    /// Operação do último resultado visto (peça diferente = enquadrar de novo).
    last_operation: Option<Operation>,
    /// Buffer reaproveitado para a polilinha em desenho.
    scratch: Vec<Pos2>,
}

impl Default for ViewState {
    fn default() -> Self {
        ViewState {
            cam: Camera::default(),
            sim: Sim::default(),
            fit_pending: true,
            user_adjusted: false,
            last_key: None,
            last_operation: None,
            scratch: Vec::new(),
        }
    }
}

impl ViewState {
    /// Registra o resultado atual; devolve `true` quando a chave mudou. Aí a
    /// simulação para (o marcador da tocha não pula no meio de um programa
    /// novo) e o enquadramento é pedido se o usuário ainda não ajustou a
    /// câmera — ou de qualquer modo quando a operação muda, porque é outra
    /// peça (e o ajuste anterior deixa de valer). O primeiro resultado conta
    /// como mudança de operação: sempre enquadra.
    pub fn note_result(&mut self, key: u64, operation: Operation) -> bool {
        if self.last_key == Some(key) {
            return false;
        }
        self.last_key = Some(key);
        let op_changed = self.last_operation != Some(operation);
        self.last_operation = Some(operation);
        self.sim.stop();
        if op_changed {
            self.user_adjusted = false;
        }
        if !self.user_adjusted {
            self.fit_pending = true;
        }
        true
    }

    /// O usuário mexeu na câmera (órbita, pan ou zoom): cancela um
    /// enquadramento pendente e passa a preservar a vista nos recálculos.
    pub fn mark_adjusted(&mut self) {
        self.user_adjusted = true;
        self.fit_pending = false;
    }

    /// Menu "Simulação > Vista ..." e botões da barra. Uma vista nova é um
    /// recomeço: o próximo resultado volta a enquadrar.
    pub fn set_preset(&mut self, v: ViewPreset) {
        self.cam.preset(v);
        self.user_adjusted = false;
    }

    /// Menu "Simulação > Simular" (e botão ▶).
    pub fn sim_play(&mut self) {
        self.sim.play();
    }

    /// Menu "Simulação > Pausar" (e botão ⏸).
    pub fn sim_pause(&mut self) {
        self.sim.pause();
    }

    /// Menu "Simulação > Parar" (e botão ⏹).
    pub fn sim_stop(&mut self) {
        self.sim.stop();
    }

    /// Pede um novo enquadramento (duplo clique) e volta a enquadrar nos
    /// próximos recálculos.
    pub fn request_fit(&mut self) {
        self.fit_pending = true;
        self.user_adjusted = false;
    }
}

fn state_id() -> Id {
    Id::unique("tfm_view3d_state")
}

/// Acesso ao [`ViewState`] guardado na memória do egui. É assim que o menu
/// "Simulação" (Task 23) deve agir sobre a aba, de qualquer lugar que tenha
/// o `egui::Context`:
///
/// ```ignore
/// view3d::with_state(ctx, |s| s.set_preset(ViewPreset::Topo));
/// view3d::with_state(ctx, |s| s.sim_play());
/// ```
///
/// A closure roda com a memória do egui travada: não chame nada do `ctx`
/// dentro dela (nem `request_repaint`), só mexa no estado.
pub fn with_state<R>(ctx: &egui::Context, f: impl FnOnce(&mut ViewState) -> R) -> R {
    ctx.data_mut(|d| f(d.get_temp_mut_or_default::<ViewState>(state_id())))
}

// ---------------------------------------------------------------------------
// Aba "3D Interativo"
// ---------------------------------------------------------------------------

/// Desenha a aba: barra de controles e o viewport interativo. Um clique num
/// ponto do percurso seleciona a linha correspondente no painel do G-code.
pub fn show_view3d(app: &mut TfmApp, ui: &mut Ui) {
    let id = state_id();
    // Tira o estado da memória do egui enquanto desenha (o buffer de pontos
    // é movido, não copiado) e devolve no fim.
    let mut state: ViewState = ui
        .ctx()
        .data_mut(|d| std::mem::take(d.get_temp_mut_or_default::<ViewState>(id)));
    let picked = view_ui(app, ui, &mut state);
    ui.ctx().data_mut(|d| d.insert_temp(id, state));
    if let Some(line) = picked {
        app.gcode_view.select(line);
    }
}

/// Caixa envolvente da cena (percurso, ou referência da peça sem percurso).
#[derive(Clone, Copy, Debug)]
struct Bounds {
    min: Vec3,
    max: Vec3,
}

impl Bounds {
    fn center(&self) -> Vec3 {
        scale(add(self.min, self.max), 0.5)
    }

    /// Maior dimensão (mínimo 1 mm).
    fn extent(&self) -> f32 {
        let d = sub(self.max, self.min);
        d.x.max(d.y).max(d.z).max(1.0)
    }
}

fn scene_bounds(toolpath: Option<&Toolpath>, params: Option<&Params>) -> Bounds {
    if let Some(tp) = toolpath {
        if is_finite(tp.min) && is_finite(tp.max) {
            return Bounds {
                min: tp.min,
                max: tp.max,
            };
        }
    }
    match params {
        Some(p) => reference_bounds(p),
        None => Bounds {
            min: Vec3::new(-100.0, -100.0, 0.0),
            max: Vec3::new(100.0, 100.0, 0.0),
        },
    }
}

/// Caixa da geometria de referência da peça, pelos parâmetros.
fn reference_bounds(p: &Params) -> Bounds {
    match p.welding_mode {
        WeldingMode::ChapaPlana => {
            let (lx, ly) = flat_size(p);
            let z = finite_or(p.afastamento_tocha, 0.0);
            Bounds {
                min: Vec3::new(0.0, 0.0, z),
                max: Vec3::new(lx, ly, z),
            }
        }
        WeldingMode::Bolacha => {
            let r = (finite_or(p.diametro, 0.0) * 0.5).max(1.0);
            let x = finite_or(p.afastamento_tocha, 0.0);
            Bounds {
                min: Vec3::new(x, -r, -r),
                max: Vec3::new(x, r, r),
            }
        }
        _ => {
            let r = cylinder_radius(p);
            let len = finite_or(p.comprimento_revestir, 0.0).max(1.0);
            Bounds {
                min: Vec3::new(0.0, -r, -r),
                max: Vec3::new(len, r, r),
            }
        }
    }
}

/// Raio do cilindro de referência: `diametro_interno` na operação interna.
fn cylinder_radius(p: &Params) -> f32 {
    let d = if p.is_internal() && p.diametro_interno > 0.0 {
        p.diametro_interno
    } else {
        p.diametro
    };
    (finite_or(d, 0.0) * 0.5).max(0.5)
}

/// Lados da chapa plana em `(X, Y)`: o comprimento fica no eixo de solda.
fn flat_size(p: &Params) -> (f32, f32) {
    let c = finite_or(p.comprimento_revestir, 0.0).max(1.0);
    let l = finite_or(p.largura_revestir, 0.0).max(1.0);
    match p.flat_orientation {
        FlatOrientation::X => (c, l),
        FlatOrientation::Y => (l, c),
    }
}

fn view_ui(app: &TfmApp, ui: &mut Ui, state: &mut ViewState) -> Option<usize> {
    let palette = app.palette;
    let result = app.result.as_ref();
    if let Some(r) = result {
        state.note_result(r.key, Operation::from_params(&r.params));
    }
    let toolpath = result.map(|r| &r.toolpath).filter(|t| !t.is_empty());
    let total = toolpath.map_or(0, Toolpath::point_count);

    toolbar(ui, state, total);

    let avail = ui.available_size();
    let size = vec2(avail.x.max(32.0), avail.y.max(32.0));
    let (response, painter) = ui.allocate_painter(size, Sense::click_and_drag());
    let rect = response.rect;
    painter.rect_filled(rect, CornerRadius::same(4), palette.card_bg);

    let bounds = scene_bounds(toolpath, result.map(|r| &r.params));
    interact(ui, &response, state, rect, &bounds, result.is_some());

    if state.sim.playing {
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        state.sim.advance(dt, total);
        ui.ctx().request_repaint();
    }

    let proj = Projector::new(&state.cam, rect);
    draw_grid(&painter, &proj, &bounds, &palette);
    if let Some(r) = result {
        draw_reference(&painter, &proj, &r.params, toolpath, &palette);
    }
    let picked = match toolpath {
        Some(tp) => {
            let stick = (bounds.extent() * 0.08).max(2.0);
            // Cada clique de um duplo clique também conta como `clicked()`:
            // enquadrar não pode trocar a linha selecionada.
            let pointer = if response.clicked() && !response.double_clicked() {
                response.interact_pointer_pos()
            } else {
                None
            };
            draw_path(
                &painter,
                &proj,
                tp,
                state,
                &palette,
                app.gcode_view.selected_line,
                pointer,
                stick,
            )
        }
        None => {
            let text = if result.is_some() {
                "Sem trajetória para exibir"
            } else {
                "Calculando a trajetória…"
            };
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                text,
                FontId::proportional(14.0),
                palette.muted,
            );
            None
        }
    };
    overlay(&painter, rect, toolpath, &palette);
    picked
}

/// Órbita (botão esquerdo), pan (direito ou meio), zoom (roda / pinça) e
/// enquadramento (duplo clique). Mexer na câmera cancela um enquadramento
/// pendente; o pendente é consumido aqui quando já há resultado.
fn interact(
    ui: &Ui,
    response: &Response,
    state: &mut ViewState,
    rect: Rect,
    bounds: &Bounds,
    has_result: bool,
) {
    let mut touched = false;
    if response.dragged_by(PointerButton::Primary) {
        state.cam.orbit(response.drag_delta());
        touched = true;
    } else if response.dragged_by(PointerButton::Secondary)
        || response.dragged_by(PointerButton::Middle)
    {
        state.cam.pan(response.drag_delta(), rect);
        touched = true;
    }
    if response.hovered() {
        let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        if scroll != 0.0 && scroll.is_finite() {
            state.cam.zoom((-scroll * WHEEL_ZOOM).exp());
            touched = true;
        }
        if zoom != 1.0 && zoom.is_finite() && zoom > 0.0 {
            state.cam.zoom(1.0 / zoom);
            touched = true;
        }
    }
    if touched {
        state.mark_adjusted();
    }
    if response.double_clicked() {
        state.request_fit();
    }
    if state.fit_pending && has_result {
        state.cam.fit(bounds.min, bounds.max);
        state.fit_pending = false;
    }
}

/// Barra de controles: ▶ ⏸ ⏹, vistas, Traçado, Solda e Velocidade.
fn toolbar(ui: &mut Ui, state: &mut ViewState, total: usize) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("▶").on_hover_text("Simular").clicked() {
            state.sim_play();
        }
        if ui.button("⏸").on_hover_text("Pausar").clicked() {
            state.sim_pause();
        }
        if ui.button("⏹").on_hover_text("Parar").clicked() {
            state.sim_stop();
        }
        ui.separator();
        for preset in ViewPreset::ALL {
            if ui
                .button(preset.label())
                .on_hover_text(preset.hint())
                .clicked()
            {
                state.set_preset(preset);
            }
        }
        ui.separator();
        ui.checkbox(&mut state.sim.show_trace, "Traçado")
            .on_hover_text("Mostrar a trajetória da tocha");
        ui.checkbox(&mut state.sim.show_weld, "Solda")
            .on_hover_text("Marcar os pontos com a tocha ligada");
        ui.separator();
        ui.label("Velocidade:");
        let mut idx = state.sim.speed_index();
        let slider = Slider::new(&mut idx, 0..=SPEEDS.len() - 1).show_value(false);
        if ui
            .add(slider)
            .on_hover_text("Velocidade da simulação")
            .changed()
        {
            state.sim.set_speed_index(idx);
        }
        ui.monospace(speed_label(state.sim.speed));
        if total > 0 && state.sim.active() {
            ui.separator();
            let shown = (state.sim.progress * total as f32).round() as usize;
            ui.weak(format!("ponto {shown} de {total}"));
        }
    });
}

/// Grade do chão (plano XY logo abaixo da peça) e o triedro XYZ no canto.
fn draw_grid(painter: &Painter, proj: &Projector, b: &Bounds, palette: &Palette) {
    let extent = b.extent();
    let spacing = nice_step(extent / 8.0);
    let n = ((extent * 0.75 / spacing).ceil() as i32).clamp(4, 40);
    let c = b.center();
    let cx = (c.x / spacing).round() * spacing;
    let cy = (c.y / spacing).round() * spacing;
    let z = b.min.z - spacing * 0.02;
    let half = n as f32 * spacing;
    let faint = Stroke::new(1.0, palette.muted.gamma_multiply(0.25));
    let strong = Stroke::new(1.0, palette.muted.gamma_multiply(0.5));
    for i in -n..=n {
        let stroke = if i % 5 == 0 { strong } else { faint };
        let x = cx + i as f32 * spacing;
        if let Some(seg) = proj.segment(Vec3::new(x, cy - half, z), Vec3::new(x, cy + half, z)) {
            painter.line_segment(seg, stroke);
        }
        let y = cy + i as f32 * spacing;
        if let Some(seg) = proj.segment(Vec3::new(cx - half, y, z), Vec3::new(cx + half, y, z)) {
            painter.line_segment(seg, stroke);
        }
    }

    let origin = Vec3::new(cx - half, cy - half, z);
    let len = spacing * 2.0;
    let axes = [
        (Vec3::new(len, 0.0, 0.0), palette.danger, "X"),
        (Vec3::new(0.0, len, 0.0), palette.success, "Y"),
        (Vec3::new(0.0, 0.0, len), palette.accent, "Z"),
    ];
    for (dir, color, label) in axes {
        if let Some(seg) = proj.segment(origin, add(origin, dir)) {
            painter.line_segment(seg, Stroke::new(2.0, color));
        }
        if let Some(tip) = proj.project(add(origin, scale(dir, 1.18))) {
            painter.text(
                tip,
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(13.0),
                color,
            );
        }
    }
}

/// Silhueta da peça em cinza fino: cilindro (dois círculos + 4 geratrizes),
/// chapa (retângulo na altura da tocha) ou bolacha (anéis interno/externo).
fn draw_reference(
    painter: &Painter,
    proj: &Projector,
    p: &Params,
    toolpath: Option<&Toolpath>,
    palette: &Palette,
) {
    let stroke = Stroke::new(1.0, palette.muted.gamma_multiply(0.8));
    let line = |a: Vec3, b: Vec3| {
        if let Some(seg) = proj.segment(a, b) {
            painter.line_segment(seg, stroke);
        }
    };
    match p.welding_mode {
        WeldingMode::ChapaPlana => {
            let (lx, ly) = flat_size(p);
            let z = finite_or(p.afastamento_tocha, 0.0);
            let corners = [
                Vec3::new(0.0, 0.0, z),
                Vec3::new(lx, 0.0, z),
                Vec3::new(lx, ly, z),
                Vec3::new(0.0, ly, z),
            ];
            for i in 0..4 {
                line(corners[i], corners[(i + 1) % 4]);
            }
        }
        WeldingMode::Bolacha => {
            let x = finite_or(p.afastamento_tocha, 0.0);
            let r_out = finite_or(p.diametro, 0.0) * 0.5;
            let r_in = finite_or(p.diametro_interno, 0.0) * 0.5;
            if r_out > 0.0 {
                draw_circle(painter, proj, x, r_out, stroke);
            }
            let inner = if r_in > 0.0 && r_in < r_out {
                r_in
            } else {
                0.0
            };
            if inner > 0.0 {
                draw_circle(painter, proj, x, inner, stroke);
            }
            for k in 0..4 {
                let (s, c) = (k as f32 * std::f32::consts::FRAC_PI_2).sin_cos();
                line(
                    Vec3::new(x, inner * c, inner * s),
                    Vec3::new(x, r_out * c, r_out * s),
                );
            }
        }
        _ => {
            let r = cylinder_radius(p);
            let len = finite_or(p.comprimento_revestir, 0.0).max(1.0);
            let (x0, x1) = match toolpath {
                Some(tp) if tp.max.x - tp.min.x > 1e-3 => (tp.min.x, tp.max.x),
                _ => (0.0, len),
            };
            draw_circle(painter, proj, x0, r, stroke);
            draw_circle(painter, proj, x1, r, stroke);
            for k in 0..4 {
                let (s, c) = (k as f32 * std::f32::consts::FRAC_PI_2).sin_cos();
                line(Vec3::new(x0, r * c, r * s), Vec3::new(x1, r * c, r * s));
            }
        }
    }
}

/// Círculo de raio `r` no plano YZ, em `x`.
fn draw_circle(painter: &Painter, proj: &Projector, x: f32, r: f32, stroke: Stroke) {
    let step = std::f32::consts::TAU / CIRCLE_SEGMENTS as f32;
    for i in 0..CIRCLE_SEGMENTS {
        let (s0, c0) = (i as f32 * step).sin_cos();
        let (s1, c1) = ((i + 1) as f32 * step).sin_cos();
        if let Some(seg) = proj.segment(Vec3::new(x, r * c0, r * s0), Vec3::new(x, r * c1, r * s1))
        {
            painter.line_segment(seg, stroke);
        }
    }
}

/// O que decide cor e traço de um trecho da trajetória.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Style {
    layer: usize,
    torch_on: bool,
    rapid: bool,
}

impl Style {
    fn of(pt: &PathPoint) -> Self {
        Style {
            layer: pt.layer,
            torch_on: pt.torch_on,
            rapid: pt.rapid,
        }
    }
}

/// Desenha a trajetória (até `progress` quando a simulação está ativa), os
/// pontos de solda, a linha selecionada e a tocha (vareta de `stick` mm).
/// `pointer` é a posição de um clique neste quadro; devolve a linha do ponto
/// mais próximo dele, se ficou a menos de [`PICK_RADIUS`].
#[allow(clippy::too_many_arguments)]
fn draw_path(
    painter: &Painter,
    proj: &Projector,
    tp: &Toolpath,
    state: &mut ViewState,
    palette: &Palette,
    selected: Option<usize>,
    pointer: Option<Pos2>,
    stick: f32,
) -> Option<usize> {
    let total = tp.point_count();
    if total == 0 {
        return None;
    }
    let step = decimation_step(total);
    let weld_stride = total.div_ceil(MAX_WELD_DOTS).max(step);
    let limit = if state.sim.active() {
        ((state.sim.progress * total as f32).round() as usize).clamp(1, total)
    } else {
        total
    };
    let (show_trace, show_weld) = (state.sim.show_trace, state.sim.show_weld);

    let mut best: Option<(f32, usize)> = None;
    let mut selected_at: Option<Pos2> = None;
    let mut torch_at: Option<(Vec3, Pos2)> = None;
    let mut consumed = 0usize;
    let scratch = &mut state.scratch;
    scratch.clear();

    for seg in &tp.segments {
        if consumed >= limit {
            break;
        }
        let n = seg.len().min(limit - consumed);
        let mut run: Option<Style> = None;
        for (k, pt) in seg[..n].iter().enumerate() {
            // Decimação uniforme, mantendo sempre o último ponto do trecho.
            if step > 1 && k % step != 0 && k + 1 != n {
                continue;
            }
            let style = Style::of(pt);
            let screen = proj.project(pt.pos);
            if let Some(s) = screen {
                if let Some(ptr) = pointer {
                    let d = s.distance_sq(ptr);
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, pt.line_idx));
                    }
                }
                if selected_at.is_none() && selected == Some(pt.line_idx) {
                    selected_at = Some(s);
                }
                torch_at = Some((pt.pos, s));
                if show_weld
                    && pt.torch_on
                    && !pt.rapid
                    && (consumed + k).is_multiple_of(weld_stride)
                {
                    painter.circle_filled(s, 1.5, palette.layer_color(pt.layer));
                }
            }
            if run != Some(style) {
                // O movimento até este ponto tem o estilo deste ponto: o
                // trecho novo começa no ponto anterior.
                if show_trace {
                    flush_run(painter, scratch, run, palette);
                }
                let last = scratch.last().copied();
                scratch.clear();
                scratch.extend(last);
                run = Some(style);
            }
            match screen {
                Some(s) => scratch.push(s),
                None => {
                    if show_trace {
                        flush_run(painter, scratch, run, palette);
                    }
                    scratch.clear();
                }
            }
        }
        if show_trace {
            flush_run(painter, scratch, run, palette);
        }
        scratch.clear();
        consumed += n;
    }

    if let (Some(s), Some(line)) = (selected_at, selected) {
        painter.circle_filled(s, 4.5, palette.warning);
        painter.circle_stroke(s, 4.5, Stroke::new(1.0, palette.card_bg));
        painter.text(
            s + vec2(7.0, -7.0),
            Align2::LEFT_BOTTOM,
            format!("L{}", line + 1),
            FontId::monospace(11.0),
            palette.warning,
        );
    }
    if state.sim.active() {
        if let Some((pos, s)) = torch_at {
            let top = add(pos, Vec3::new(0.0, 0.0, stick));
            if let Some(seg) = proj.segment(pos, top) {
                painter.line_segment(seg, Stroke::new(2.0, palette.danger));
            }
            painter.circle_filled(s, 5.0, palette.danger);
        }
    }
    best.filter(|(d, _)| *d <= PICK_RADIUS * PICK_RADIUS)
        .map(|(_, line)| line)
}

/// Desenha a polilinha acumulada em `points` com o estilo do trecho: G0 em
/// cinza tracejado; cordão na cor da camada (tocha desligada a 40%).
fn flush_run(painter: &Painter, points: &[Pos2], style: Option<Style>, palette: &Palette) {
    let Some(style) = style else { return };
    if points.len() < 2 {
        return;
    }
    if style.rapid {
        let mut shapes = Vec::new();
        Shape::dashed_line_many(
            points,
            Stroke::new(1.0, palette.muted),
            6.0,
            4.0,
            &mut shapes,
        );
        painter.extend(shapes);
    } else {
        let base = palette.layer_color(style.layer);
        let (color, width) = if style.torch_on {
            (base, 2.0)
        } else {
            (base.gamma_multiply(0.4), 1.2)
        };
        painter.add(Shape::line(points.to_vec(), Stroke::new(width, color)));
    }
}

/// Textos sobre o viewport: resumo (acima, à esquerda), aviso de decimação
/// (abaixo, à esquerda) e dica de uso (abaixo, à direita).
fn overlay(painter: &Painter, rect: Rect, toolpath: Option<&Toolpath>, palette: &Palette) {
    let font = FontId::proportional(11.0);
    let pad = vec2(8.0, 6.0);
    if let Some(tp) = toolpath {
        let total = tp.point_count();
        let camadas = if tp.layer_count == 1 {
            "camada"
        } else {
            "camadas"
        };
        painter.text(
            rect.left_top() + pad,
            Align2::LEFT_TOP,
            format!("{} {camadas} · {total} pontos", tp.layer_count),
            font.clone(),
            palette.muted,
        );
        if decimation_step(total) > 1 {
            painter.text(
                rect.left_bottom() + vec2(pad.x, -pad.y),
                Align2::LEFT_BOTTOM,
                "traçado simplificado para exibição",
                font.clone(),
                palette.muted,
            );
        }
    }
    if rect.width() > 420.0 {
        painter.text(
            rect.right_bottom() - pad,
            Align2::RIGHT_BOTTOM,
            "arrastar: girar · botão direito: mover · roda: zoom · duplo clique: enquadrar",
            font,
            palette.muted.gamma_multiply(0.8),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2, Rect};
    use tfm_core::toolpath::Vec3;

    fn rect() -> Rect {
        Rect::from_min_size(pos2(100.0, 50.0), vec2(640.0, 480.0))
    }

    /// Projeta `p` e falha o teste se o ponto cair atrás da câmera.
    fn proj(cam: &Camera, p: Vec3) -> egui::Pos2 {
        cam.project(p, rect())
            .unwrap_or_else(|| panic!("ponto {p:?} deveria estar à frente da câmera"))
    }

    fn side_cam() -> Camera {
        let mut cam = Camera::default();
        cam.preset(ViewPreset::Lado);
        cam.target = Vec3::default();
        cam.distance = 100.0;
        cam
    }

    #[test]
    fn project_target_lands_in_rect_center() {
        let cam = Camera {
            yaw: 33.0,
            pitch: 21.0,
            distance: 250.0,
            target: Vec3::new(12.0, -7.0, 3.5),
        };
        let p = proj(&cam, cam.target);
        let c = rect().center();
        assert!(
            (p.x - c.x).abs() <= 1.0 && (p.y - c.y).abs() <= 1.0,
            "alvo projetado em {p:?}, centro em {c:?}"
        );
    }

    #[test]
    fn preset_top_looks_down() {
        let mut cam = Camera::default();
        cam.preset(ViewPreset::Topo);
        assert!(
            (cam.pitch - 89.0).abs() < 0.5,
            "vista de topo deveria ter pitch ~ 89°, veio {}",
            cam.pitch
        );
    }

    #[test]
    fn fit_sets_distance_from_bounds() {
        let mut cam = Camera::default();
        cam.fit(Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 100.0, 100.0));
        assert!(cam.distance.is_finite(), "distância não finita");
        assert!(
            cam.distance > 100.0,
            "distância {} pequena demais",
            cam.distance
        );
        assert_eq!(cam.target, Vec3::new(50.0, 50.0, 50.0));
        // Caixa degenerada e valores não finitos não quebram nem geram NaN.
        cam.fit(Vec3::default(), Vec3::default());
        assert!(cam.distance.is_finite() && cam.distance > 0.0);
        cam.fit(Vec3::new(f32::NAN, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0));
        assert!(cam.distance.is_finite() && cam.target.x.is_finite());
    }

    #[test]
    fn fit_keeps_whole_box_inside_rect() {
        let mut cam = Camera::default();
        let (min, max) = (Vec3::new(-30.0, -10.0, -5.0), Vec3::new(170.0, 10.0, 5.0));
        cam.fit(min, max);
        for p in [
            min,
            max,
            Vec3::new(min.x, max.y, min.z),
            Vec3::new(max.x, min.y, max.z),
        ] {
            let s = proj(&cam, p);
            assert!(
                rect().contains(s),
                "canto {p:?} caiu fora do retângulo: {s:?}"
            );
        }
    }

    #[test]
    fn project_behind_camera_is_none() {
        let cam = side_cam();
        // A câmera da vista lateral fica em Y negativo olhando para +Y.
        assert!(cam.project(Vec3::new(0.0, -200.0, 0.0), rect()).is_none());
        assert!(cam.project(Vec3::new(f32::NAN, 0.0, 0.0), rect()).is_none());
    }

    #[test]
    fn side_view_has_x_right_and_z_up() {
        let cam = side_cam();
        let c = rect().center();
        let px = proj(&cam, Vec3::new(10.0, 0.0, 0.0));
        let pz = proj(&cam, Vec3::new(0.0, 0.0, 10.0));
        assert!(
            px.x > c.x + 5.0 && (px.y - c.y).abs() < 1.0,
            "+X deveria ir para a direita: {px:?}"
        );
        assert!(
            pz.y < c.y - 5.0 && (pz.x - c.x).abs() < 1.0,
            "+Z deveria ir para cima: {pz:?}"
        );
    }

    #[test]
    fn presets_have_expected_angles() {
        let mut cam = Camera::default();
        let cases = [
            (ViewPreset::Iso, -45.0, 25.0),
            (ViewPreset::Lado, 0.0, 0.0),
            (ViewPreset::Frente, -90.0, 0.0),
            (ViewPreset::Tras, 90.0, 0.0),
        ];
        for (preset, yaw, pitch) in cases {
            cam.preset(preset);
            assert!((cam.yaw - yaw).abs() < 1e-3, "{preset:?}: yaw {}", cam.yaw);
            assert!(
                (cam.pitch - pitch).abs() < 1e-3,
                "{preset:?}: pitch {}",
                cam.pitch
            );
        }
        assert_eq!(ViewPreset::Tras.label(), "Trás");
    }

    #[test]
    fn orbit_clamps_pitch_and_wraps_yaw() {
        let mut cam = side_cam();
        cam.orbit(vec2(0.0, 10_000.0));
        assert!(
            cam.pitch <= 89.0 && cam.pitch >= -89.0,
            "pitch {}",
            cam.pitch
        );
        cam.orbit(vec2(0.0, -20_000.0));
        assert!(cam.pitch >= -89.0, "pitch {}", cam.pitch);
        cam.orbit(vec2(10_000.0, 0.0));
        assert!(cam.yaw.abs() <= 180.0, "yaw {}", cam.yaw);
    }

    #[test]
    fn zoom_is_multiplicative_and_bounded() {
        let mut cam = side_cam();
        cam.zoom(0.5);
        assert!((cam.distance - 50.0).abs() < 1e-3);
        cam.zoom(2.0);
        assert!((cam.distance - 100.0).abs() < 1e-3);
        cam.zoom(0.0);
        assert!(
            cam.distance >= MIN_DISTANCE,
            "zoom(0) não pode zerar a distância"
        );
        cam.zoom(f32::INFINITY);
        assert!(cam.distance <= MAX_DISTANCE && cam.distance.is_finite());
    }

    #[test]
    fn pan_moves_target_in_view_plane() {
        let mut cam = side_cam();
        cam.pan(vec2(40.0, 0.0), rect());
        assert!(
            cam.target.x < 0.0,
            "arrastar para a direita move o alvo para -X: {:?}",
            cam.target
        );
        assert!(cam.target.y.abs() < 1e-4 && cam.target.z.abs() < 1e-4);
        cam.pan(vec2(0.0, 40.0), rect());
        assert!(
            cam.target.z > 0.0,
            "arrastar para baixo sobe o alvo: {:?}",
            cam.target
        );
    }

    #[test]
    fn sim_advances_and_stops_at_end() {
        let mut sim = Sim::default();
        assert!(!sim.active(), "simulação começa parada");
        sim.play();
        sim.advance(0.1, 1000);
        // 1x percorre 100 pontos/s: 0,1 s = 10 pontos de 1000.
        assert!(
            (sim.progress - 0.01).abs() < 1e-4,
            "progress {}",
            sim.progress
        );
        assert!(sim.active());
        sim.advance(100.0, 1000);
        assert_eq!(sim.progress, 1.0);
        assert!(!sim.playing, "ao chegar ao fim a simulação para");
        sim.pause();
        sim.stop();
        assert_eq!(sim.progress, 0.0);
        assert!(!sim.playing && !sim.active());
        // Sem pontos não há divisão por zero.
        sim.play();
        sim.advance(1.0, 0);
        assert!(sim.progress.is_finite());
    }

    #[test]
    fn speed_table_roundtrip() {
        let mut sim = Sim::default();
        assert_eq!(sim.speed, 1.0);
        assert_eq!(sim.speed_index(), 1);
        sim.set_speed_index(4);
        assert_eq!(sim.speed, 5.0);
        sim.set_speed_index(999);
        assert_eq!(sim.speed, 100.0, "índice fora da tabela usa o último");
        assert_eq!(speed_label(0.5), "0,5x");
        assert_eq!(speed_label(1.0), "1x");
        assert_eq!(speed_label(100.0), "100x");
    }

    #[test]
    fn decimation_keeps_at_most_max_points() {
        assert_eq!(decimation_step(0), 1);
        assert_eq!(decimation_step(1000), 1);
        assert_eq!(decimation_step(MAX_DRAW_POINTS), 1);
        assert_eq!(decimation_step(MAX_DRAW_POINTS + 1), 2);
        for n in [250_000usize, 1_000_000, 3_333_333] {
            let step = decimation_step(n);
            assert!(
                n.div_ceil(step) <= MAX_DRAW_POINTS,
                "{n} pontos com passo {step}"
            );
        }
    }

    #[test]
    fn new_result_requests_fit_once() {
        let op = Operation::Espiral;
        let mut state = ViewState::default();
        assert!(state.fit_pending, "estado inicial pede enquadramento");
        state.fit_pending = false;
        assert!(
            state.note_result(42, op),
            "resultado novo pede enquadramento"
        );
        assert!(state.fit_pending);
        state.fit_pending = false;
        assert!(
            !state.note_result(42, op),
            "mesmo resultado não pede de novo"
        );
        assert!(!state.fit_pending);
        state.set_preset(ViewPreset::Topo);
        assert!((state.cam.pitch - 89.0).abs() < 0.5);
        state.sim_play();
        assert!(state.sim.playing);
        state.sim_pause();
        assert!(!state.sim.playing);
        state.sim_stop();
        assert_eq!(state.sim.progress, 0.0);
        // Depois de o usuário ajustar a câmera, um recálculo da mesma
        // operação não reenquadra; a simulação em curso é parada.
        state.mark_adjusted();
        state.sim_play();
        state.sim.advance(1.0, 1000);
        assert!(
            state.note_result(43, op),
            "chave nova continua sendo novidade"
        );
        assert!(
            !state.fit_pending,
            "ajuste do usuário sobrevive ao recálculo"
        );
        assert!(
            !state.sim.playing && state.sim.progress == 0.0,
            "resultado novo para a simulação"
        );
    }

    /// O primeiro resultado enquadra mesmo que o usuário já tenha mexido na
    /// câmera (ainda sem peça): conta como mudança de operação.
    #[test]
    fn first_result_always_fits() {
        let mut state = ViewState {
            fit_pending: false,
            ..ViewState::default()
        };
        state.cam.zoom(0.5);
        state.mark_adjusted();
        assert!(state.user_adjusted && !state.fit_pending);
        assert!(state.note_result(7, Operation::Espiral));
        assert!(state.fit_pending, "primeiro resultado sempre enquadra");
        assert!(!state.user_adjusted, "ajuste sem peça é esquecido");
    }

    #[test]
    fn user_zoom_survives_recompute() {
        let mut state = ViewState::default();
        assert!(state.note_result(1, Operation::Espiral));
        state.fit_pending = false; // enquadramento consumido no quadro
        state.cam.zoom(0.5);
        state.mark_adjusted();
        assert!(state.user_adjusted && !state.fit_pending);

        // Mesma operação, parâmetros novos: nada de reenquadrar.
        assert!(state.note_result(2, Operation::Espiral));
        assert!(
            !state.fit_pending,
            "zoom do operador descartado por um recálculo"
        );
        assert!(state.user_adjusted);

        // Operação diferente é outra peça: enquadra e esquece o ajuste.
        assert!(state.note_result(3, Operation::ChapaPlana));
        assert!(state.fit_pending, "operação nova deveria enquadrar");
        assert!(!state.user_adjusted);
        state.fit_pending = false;

        // Duplo clique (request_fit) e presets rearmam o enquadramento
        // automático para os próximos recálculos.
        state.mark_adjusted();
        state.request_fit();
        assert!(state.fit_pending && !state.user_adjusted);
        state.fit_pending = false;
        state.mark_adjusted();
        state.set_preset(ViewPreset::Lado);
        assert!(!state.user_adjusted, "preset rearma o enquadramento");
        assert!(state.note_result(4, Operation::ChapaPlana));
        assert!(state.fit_pending);
    }

    #[test]
    fn state_lives_in_egui_memory() {
        let ctx = egui::Context::default();
        with_state(&ctx, |s| s.set_preset(ViewPreset::Frente));
        let yaw = with_state(&ctx, |s| s.cam.yaw);
        assert!((yaw + 90.0).abs() < 1e-3, "yaw {yaw}");
    }

    #[test]
    fn nice_step_is_1_2_5() {
        assert_eq!(nice_step(0.0), 10.0);
        assert_eq!(nice_step(f32::NAN), 10.0);
        assert_eq!(nice_step(12.0), 10.0);
        assert_eq!(nice_step(27.0), 20.0);
        assert_eq!(nice_step(60.0), 50.0);
        assert!((nice_step(0.3) - 0.2).abs() < 1e-6);
    }

    /// Recolhe as cores sólidas de traços e polilinhas pintados num quadro.
    fn stroke_colors(shape: &Shape, out: &mut std::collections::HashSet<[u8; 4]>) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| stroke_colors(s, out)),
            Shape::Path(p) => {
                if let egui::epaint::ColorMode::Solid(c) = p.stroke.color {
                    out.insert(c.to_array());
                }
            }
            Shape::LineSegment { stroke, .. } => {
                out.insert(stroke.color.to_array());
            }
            _ => {}
        }
    }

    /// Percurso real (Espiral padrão com 3 camadas) desenhado sem janela: a
    /// saída do egui tem traços nas três cores de camada e nos três eixos, e
    /// um clique sobre o primeiro ponto devolve a linha dele.
    #[test]
    fn default_spiral_paints_three_layer_colors() {
        use tfm_core::params::Params;

        let params = Params {
            num_camadas: 3,
            ..Params::default()
        };
        let lines = tfm_core::gcode::generate(&params)
            .unwrap_or_else(|e| panic!("geração do G-code padrão falhou: {e}"));
        let tp = tfm_core::toolpath::build_toolpath(&lines, &params);
        assert!(
            tp.layer_count >= 3,
            "esperava 3 camadas, veio {}",
            tp.layer_count
        );
        let first = tp
            .points()
            .next()
            .copied()
            .unwrap_or_else(|| panic!("percurso vazio"));

        let palette = Palette::light();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0));
        let mut state = ViewState::default();
        state.cam.fit(tp.min, tp.max);
        let bounds = scene_bounds(Some(&tp), Some(&params));
        let proj = Projector::new(&state.cam, rect);
        let click = proj.project(first.pos);
        assert!(click.is_some(), "o primeiro ponto deveria estar visível");

        let ctx = egui::Context::default();
        let mut picked = None;
        let mut output = ctx.run_pass(egui::RawInput::default(), |ctx| {
            let painter = Painter::new(ctx.clone(), egui::LayerId::background(), rect);
            draw_grid(&painter, &proj, &bounds, &palette);
            draw_reference(&painter, &proj, &params, Some(&tp), &palette);
            picked = draw_path(
                &painter,
                &proj,
                &tp,
                &mut state,
                &palette,
                Some(first.line_idx),
                click,
                5.0,
            );
        });
        // Sem renderizador ninguém aplica o atlas de fontes: o egui exige
        // que a entrega seja limpa antes de ser descartada.
        output.textures_delta.clear();
        assert_eq!(
            picked,
            Some(first.line_idx),
            "clique deveria escolher a linha do ponto"
        );

        let mut colors = std::collections::HashSet::new();
        for clipped in &output.shapes {
            stroke_colors(&clipped.shape, &mut colors);
        }
        for layer in 0..3 {
            let c = palette.layer_color(layer);
            assert!(
                colors.contains(&c.to_array()),
                "cor da camada {layer} ausente"
            );
        }
        for (axis, c) in [
            ("X", palette.danger),
            ("Y", palette.success),
            ("Z", palette.accent),
        ] {
            assert!(colors.contains(&c.to_array()), "eixo {axis} ausente");
        }
    }
}
