//! Visualização 3D interativa do percurso da tocha (aba "3D Interativo").
//!
//! Renderização por software com o `Painter` do egui, sem GPU própria: uma
//! câmera orbital com perspectiva simples ([`Camera`]) projeta a cena e a
//! aba desenha, nesta ordem, a grade do chão (só sob a peça deitada), os eixos
//! XYZ, a silhueta da peça (cilindro, chapa ou bolacha) e a trajetória colorida
//! por camada.
//! A simulação ([`Sim`]) desenha o percurso até `progress` e marca a tocha.
//! No canto superior direito fica o cubo de orientação ([`view_cube`]), como
//! o ViewCube do Inventor: as faces nomeadas giram com a câmera, clicar numa
//! face (ou perto de uma aresta ou canto) escolhe a vista, arrastar orbita e
//! ⌂ volta à isométrica.
//!
//! Nas operações cilíndricas (tudo que não é chapa plana nem bolacha) a cena
//! reproduz a máquina: a tocha fica fixa no topo e anda só em X (e em Z por
//! camada); quem gira em torno do centro do eixo A é a peça, levando junto o
//! cordão já depositado. Cada ponto é girado de `-A` ([`rotate_part`]) pelo
//! ângulo do ponto que está sendo depositado ([`current_point`]), que assim
//! cai sempre sob a tocha; as geratrizes da silhueta giram junto. Na bolacha é
//! igual, com a mesa girando no eixo vertical ([`rotate_table`]): a tocha fica
//! parada no raio +X e os raios da silhueta giram junto. Só a chapa plana fica
//! parada. [`Spin`] diz qual é o caso.
//!
//! O `TfmApp` só guarda o marcador [`View3d`]; o estado de verdade
//! ([`ViewState`]) vive na memória do egui e é acessado por [`with_state`]:
//! assim o menu "Simulação" aciona presets e ▶ ⏸ ⏹ sem tocar no `app.rs`.
//!
//! Convenção da cena, a mesma do `tfm_core::toolpath`: X é o eixo da peça
//! (axial) no cilindro e o raio na bolacha, Z aponta para cima (altura da tocha
//! na chapa e na bolacha) e Y completa o triedro. Chapa e bolacha ficam
//! deitadas, com a grade do chão logo abaixo; o cilindro fica na horizontal,
//! sem grade.

use egui::{
    pos2, vec2, Align2, Color32, CornerRadius, FontId, Id, Mesh, Painter, PointerButton, Pos2,
    Rect, Response, Sense, Shape, Stroke, Ui, Vec2,
};
use tfm_core::deposit::{DepositModel, HeightMap};
use tfm_core::gcode::common::overlap_metrics;
use tfm_core::params::{Direction, FlatOrientation, Operation, Params, WeldingMode};
use tfm_core::toolpath::{PathPoint, Toolpath, Vec3};

use crate::app::TfmApp;
use crate::theme::Palette;

/// Velocidades da simulação (multiplicador do ritmo base), na ordem do slider.
pub const SPEEDS: [f32; 10] = [0.5, 1.0, 2.0, 3.0, 5.0, 10.0, 20.0, 30.0, 50.0, 100.0];
/// Pontos por segundo em 1x: ~1000 pontos a cada 10 s.
pub const BASE_POINTS_PER_SEC: f32 = 100.0;
/// Acima disto a trajetória é decimada uniformemente para o desenho.
pub const MAX_DRAW_POINTS: usize = 200_000;
/// Limite de trechos com cordão na malha de "Solda" por quadro; acima disso o
/// percurso é decimado uniformemente.
const MAX_BEAD_SEGMENTS: usize = 60_000;
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
/// Dois trechos do cordão só dividem a aresta da emenda abaixo de 45° entre eles.
const BEAD_JOINT_COS: f32 = std::f32::consts::FRAC_1_SQRT_2;
/// Quanto a linha de centro do eixo A passa de cada face do cilindro, como
/// fração do comprimento.
const AXIS_OVERHANG: f32 = 0.1;

/// Marcador guardado pelo `TfmApp`; o estado real é [`ViewState`], na memória
/// do egui (ver [`with_state`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct View3d;

// ---------------------------------------------------------------------------
// Câmera
// ---------------------------------------------------------------------------

/// Vistas pré-definidas (menu "Simulação", teclas 1–5 e faces do cubo de
/// orientação). Nomes como no Inventor: "Frente" é o lado do operador, com
/// o eixo X da peça para a direita; "Esquerda" e "Direita" olham pelas
/// pontas do eixo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewPreset {
    Iso,
    Topo,
    Base,
    Frente,
    Tras,
    Esquerda,
    Direita,
}

impl ViewPreset {
    /// Ordem do menu "Simulação"; as cinco primeiras são as teclas 1–5.
    pub const ALL: [ViewPreset; 7] = [
        ViewPreset::Iso,
        ViewPreset::Topo,
        ViewPreset::Frente,
        ViewPreset::Esquerda,
        ViewPreset::Direita,
        ViewPreset::Tras,
        ViewPreset::Base,
    ];

    /// Nome curto.
    pub fn label(self) -> &'static str {
        match self {
            ViewPreset::Iso => "ISO",
            ViewPreset::Topo => "Topo",
            ViewPreset::Base => "Base",
            ViewPreset::Frente => "Frente",
            ViewPreset::Tras => "Trás",
            ViewPreset::Esquerda => "Esquerda",
            ViewPreset::Direita => "Direita",
        }
    }

    /// Texto de ajuda (o mesmo nome dos itens do menu "Simulação").
    pub fn hint(self) -> &'static str {
        match self {
            ViewPreset::Iso => "Vista Isométrica",
            ViewPreset::Topo => "Vista Superior",
            ViewPreset::Base => "Vista Inferior",
            ViewPreset::Frente => "Vista Frontal",
            ViewPreset::Tras => "Vista Traseira",
            ViewPreset::Esquerda => "Vista Esquerda",
            ViewPreset::Direita => "Vista Direita",
        }
    }

    /// `(yaw, pitch)` em graus. Yaw 0 = câmera em -Y olhando para +Y (X
    /// para a direita, Z para cima): a frente; yaw -90° = câmera em -X,
    /// olhando a ponta esquerda do eixo; pitch 89° = olhando de cima.
    pub fn angles(self) -> (f32, f32) {
        match self {
            ViewPreset::Iso => (-45.0, 25.0),
            ViewPreset::Topo => (0.0, 89.0),
            ViewPreset::Base => (0.0, -89.0),
            ViewPreset::Frente => (0.0, 0.0),
            ViewPreset::Tras => (180.0, 0.0),
            ViewPreset::Esquerda => (-90.0, 0.0),
            ViewPreset::Direita => (90.0, 0.0),
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

    /// Põe a câmera na direção `dir` (do alvo para a câmera), mantendo alvo
    /// e distância: é o que um clique no cubo de orientação pede. Direção
    /// vertical não define yaw e mantém o atual; zero ou não finita é
    /// ignorada.
    pub fn look_from(&mut self, dir: Vec3) {
        if !is_finite(dir) {
            return;
        }
        let d = normalize(dir);
        if length(d) < 0.5 {
            return;
        }
        self.pitch =
            d.z.clamp(-1.0, 1.0)
                .asin()
                .to_degrees()
                .clamp(-PITCH_LIMIT, PITCH_LIMIT);
        if d.x.abs() > 1e-6 || d.y.abs() > 1e-6 {
            self.yaw = wrap_deg(d.x.atan2(-d.y).to_degrees());
        }
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

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

/// `color` escurecida pelo fator `k` (0..=1), mantendo-a opaca.
fn shade(color: Color32, k: f32) -> Color32 {
    let k = k.clamp(0.0, 1.0);
    let [r, g, b, _] = color.to_array();
    Color32::from_rgb(
        (r as f32 * k) as u8,
        (g as f32 * k) as u8,
        (b as f32 * k) as u8,
    )
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

/// Vetor unitário na direção de `a` (zero se `a` for nulo ou não finito).
fn normalize(a: Vec3) -> Vec3 {
    let l = length(a);
    if l.is_finite() && l > 1e-12 {
        scale(a, 1.0 / l)
    } else {
        Vec3::default()
    }
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
// Cubo de orientação
// ---------------------------------------------------------------------------

/// Lado (px) do quadrado reservado ao cubo de orientação no canto do viewport.
pub const CUBE_SIZE: f32 = 110.0;
/// Margem do cubo até as bordas do viewport.
const CUBE_MARGIN: f32 = 10.0;
/// A partir desta fração da meia-aresta, o clique numa face "pega" a aresta
/// ou o canto vizinho e pede a vista diagonal, como no ViewCube do Inventor.
const CUBE_EDGE_ZONE: f32 = 0.6;
/// Rapidez do deslize da câmera: maior chega antes.
const CAMERA_EASE: f32 = 12.0;
/// Abaixo disto (graus) a câmera encosta na vista pedida.
const CAMERA_SNAP_DEG: f32 = 0.15;
/// Altura da linha do tempo sobre o 3D.
const TIMELINE_H: f32 = 30.0;
/// Lado do botão ⌂ (vista isométrica) ao lado do cubo.
const CUBE_HOME: f32 = 18.0;

/// Uma face do cubo: a normal (também a direção da vista que ela pede), os
/// dois eixos no plano da face (para achar arestas e cantos), o rótulo e a
/// vista correspondente (para a dica).
#[derive(Clone, Copy, Debug)]
struct CubeFace {
    normal: Vec3,
    u: Vec3,
    v: Vec3,
    label: &'static str,
    view: ViewPreset,
}

/// As seis faces, na convenção da cena (X axial, Z para cima).
const CUBE_FACES: [CubeFace; 6] = [
    CubeFace {
        normal: Vec3::new(0.0, 0.0, 1.0),
        u: Vec3::new(1.0, 0.0, 0.0),
        v: Vec3::new(0.0, 1.0, 0.0),
        label: "TOPO",
        view: ViewPreset::Topo,
    },
    CubeFace {
        normal: Vec3::new(0.0, 0.0, -1.0),
        u: Vec3::new(1.0, 0.0, 0.0),
        v: Vec3::new(0.0, -1.0, 0.0),
        label: "BASE",
        view: ViewPreset::Base,
    },
    CubeFace {
        normal: Vec3::new(0.0, -1.0, 0.0),
        u: Vec3::new(1.0, 0.0, 0.0),
        v: Vec3::new(0.0, 0.0, 1.0),
        label: "FRENTE",
        view: ViewPreset::Frente,
    },
    CubeFace {
        normal: Vec3::new(0.0, 1.0, 0.0),
        u: Vec3::new(-1.0, 0.0, 0.0),
        v: Vec3::new(0.0, 0.0, 1.0),
        label: "TRÁS",
        view: ViewPreset::Tras,
    },
    CubeFace {
        normal: Vec3::new(-1.0, 0.0, 0.0),
        u: Vec3::new(0.0, -1.0, 0.0),
        v: Vec3::new(0.0, 0.0, 1.0),
        label: "ESQUERDA",
        view: ViewPreset::Esquerda,
    },
    CubeFace {
        normal: Vec3::new(1.0, 0.0, 0.0),
        u: Vec3::new(0.0, 1.0, 0.0),
        v: Vec3::new(0.0, 0.0, 1.0),
        label: "DIREITA",
        view: ViewPreset::Direita,
    },
];

/// Quadrado do cubo no canto superior direito do viewport (encolhe em
/// viewports pequenos).
fn cube_rect(viewport: Rect) -> Rect {
    let size = CUBE_SIZE
        .min(viewport.width() * 0.4)
        .min(viewport.height() * 0.4)
        .max(24.0);
    Rect::from_min_size(
        pos2(
            viewport.right() - CUBE_MARGIN - size,
            viewport.top() + CUBE_MARGIN,
        ),
        vec2(size, size),
    )
}

/// Projeção ortográfica de um ponto do cubo (meia-aresta 1, centrado na
/// origem) dentro de `rect`: só a rotação da câmera, sem perspectiva.
fn cube_project(cam: &Camera, rect: Rect, p: Vec3) -> Pos2 {
    let b = cam.basis();
    // A diagonal do cubo (√3) tem de caber no quadrado em qualquer ângulo.
    let s = rect.width().min(rect.height()) * 0.5 / 1.8;
    let c = rect.center();
    pos2(c.x + dot(p, b.right) * s, c.y - dot(p, b.up) * s)
}

/// Faces viradas para a câmera.
fn cube_visible(cam: &Camera) -> impl Iterator<Item = &'static CubeFace> {
    let forward = cam.basis().forward;
    CUBE_FACES
        .iter()
        .filter(move |f| dot(f.normal, forward) < -1e-4)
}

/// Face visível sob `pos` e a posição do ponto nela, em `(u, v)` de -1 a 1.
fn cube_hit(cam: &Camera, rect: Rect, pos: Pos2) -> Option<(&'static CubeFace, f32, f32)> {
    for face in cube_visible(cam) {
        let c = cube_project(cam, rect, face.normal);
        let pu = cube_project(cam, rect, add(face.normal, face.u)) - c;
        let pv = cube_project(cam, rect, add(face.normal, face.v)) - c;
        let d = pos - c;
        let det = pu.x * pv.y - pu.y * pv.x;
        if det.abs() < 1e-6 {
            continue;
        }
        let u = (d.x * pv.y - d.y * pv.x) / det;
        let v = (pu.x * d.y - pu.y * d.x) / det;
        if u.abs() <= 1.0 && v.abs() <= 1.0 {
            return Some((face, u, v));
        }
    }
    None
}

/// Direção (do alvo para a câmera) que um clique em `pos` pede: a normal da
/// face, ou a diagonal da aresta/canto quando o clique fica além de
/// [`CUBE_EDGE_ZONE`] do centro. `None` fora do cubo.
fn cube_pick(cam: &Camera, rect: Rect, pos: Pos2) -> Option<Vec3> {
    let (face, u, v) = cube_hit(cam, rect, pos)?;
    let snap = |t: f32| {
        if t > CUBE_EDGE_ZONE {
            1.0
        } else if t < -CUBE_EDGE_ZONE {
            -1.0
        } else {
            0.0
        }
    };
    let dir = add(
        face.normal,
        add(scale(face.u, snap(u)), scale(face.v, snap(v))),
    );
    Some(normalize(dir))
}

/// Largura e altura (px) da face projetada, medidas pelas cordas horizontal
/// e vertical que passam pelo centro dela: é o espaço que um rótulo
/// horizontal centrado tem de verdade.
fn cube_face_span(cam: &Camera, rect: Rect, face: &CubeFace) -> (f32, f32) {
    let c = cube_project(cam, rect, face.normal);
    let pu = cube_project(cam, rect, add(face.normal, face.u)) - c;
    let pv = cube_project(cam, rect, add(face.normal, face.v)) - c;
    let det = (pu.x * pv.y - pu.y * pv.x).abs();
    let chord = |limit: f32| if limit > 1e-6 { 2.0 * det / limit } else { 0.0 };
    (
        chord(pu.y.abs().max(pv.y.abs())),
        chord(pu.x.abs().max(pv.x.abs())),
    )
}

/// Tamanho da fonte do rótulo de uma face com `width` x `height` px: cabe
/// na largura (≈0,6 em por letra, com folga) e na altura, no máximo 10 px;
/// `None` quando ficaria pequeno demais para ler.
fn cube_label_size(width: f32, height: f32, label: &str) -> Option<f32> {
    let chars = label.chars().count().max(1) as f32;
    let size = (0.9 * width / (0.6 * chars)).min(0.6 * height).min(10.0);
    (size >= 6.5).then_some(size)
}

/// Desenha o cubo: faces visíveis preenchidas (a de rótulo `hover` em
/// destaque), arestas e o rótulo das faces onde ele cabe legível.
fn draw_view_cube(
    painter: &Painter,
    cam: &Camera,
    rect: Rect,
    hover: Option<&str>,
    palette: &Palette,
) {
    let forward = cam.basis().forward;
    let edge = Stroke::new(1.0, palette.muted.gamma_multiply(0.9));
    for face in cube_visible(cam) {
        let facing = (-dot(face.normal, forward)).clamp(0.0, 1.0);
        let corners = [
            add(face.normal, add(face.u, face.v)),
            add(face.normal, sub(face.v, face.u)),
            sub(face.normal, add(face.u, face.v)),
            add(face.normal, sub(face.u, face.v)),
        ];
        let pts: Vec<Pos2> = corners
            .iter()
            .map(|&p| cube_project(cam, rect, p))
            .collect();
        let fill = if hover == Some(face.label) {
            palette.accent.gamma_multiply(0.55)
        } else {
            palette.muted.gamma_multiply(0.18 + 0.22 * facing)
        };
        painter.add(Shape::convex_polygon(pts, fill, edge));
        let (width, height) = cube_face_span(cam, rect, face);
        if let Some(size) = cube_label_size(width, height, face.label) {
            painter.text(
                cube_project(cam, rect, face.normal),
                Align2::CENTER_CENTER,
                face.label,
                FontId::proportional(size),
                palette.text,
            );
        }
    }
}

/// Cubo de orientação, como o ViewCube do Inventor: faces nomeadas giram com
/// a câmera; clique numa face = vista dela; perto de aresta ou canto = vista
/// diagonal; arrastar = orbitar; ⌂ ao lado = vista isométrica.
fn view_cube(
    ui: &mut Ui,
    painter: &Painter,
    state: &mut ViewState,
    viewport: Rect,
    palette: &Palette,
) {
    let rect = cube_rect(viewport);
    let id = ui.scope_id().with("cubo_orientacao");
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let hover = response
        .hover_pos()
        .and_then(|p| cube_hit(&state.cam, rect, p))
        .map(|(face, _, _)| face);
    if response.dragged_by(PointerButton::Primary) {
        state.cam.orbit(response.drag_delta());
        state.mark_adjusted();
    } else if response.clicked() {
        if let Some(dir) = response
            .interact_pointer_pos()
            .and_then(|p| cube_pick(&state.cam, rect, p))
        {
            state.look_from(dir);
        }
    }
    draw_view_cube(painter, &state.cam, rect, hover.map(|f| f.label), palette);
    if let Some(face) = hover {
        response.on_hover_text(face.view.hint());
    }

    let home = Rect::from_min_size(
        pos2(rect.left() - CUBE_HOME - 4.0, rect.top()),
        vec2(CUBE_HOME, CUBE_HOME),
    );
    let home_resp = ui
        .interact(home, id.with("inicio"), Sense::click())
        .on_hover_text("Vista Isométrica (tecla 1)");
    let color = if home_resp.hovered() {
        palette.accent
    } else {
        palette.muted
    };
    painter.text(
        home.center(),
        Align2::CENTER_CENTER,
        "⌂",
        FontId::proportional(15.0),
        color,
    );
    if home_resp.clicked() {
        state.set_preset(ViewPreset::Iso);
    }
}

// ---------------------------------------------------------------------------
// Superfície real do depósito ("Solda" nas peças cilíndricas)
// ---------------------------------------------------------------------------

/// Paradas da escala de cor: altura relativa à média e a cor nela.
const HEAT_STOPS: [(f32, [u8; 3]); 5] = [
    (0.6, [37, 99, 235]),
    (0.9, [8, 145, 178]),
    (1.0, [22, 163, 74]),
    (1.25, [202, 138, 4]),
    (1.6, [220, 38, 38]),
];
/// Teto de vértices da superfície por quadro: mapas maiores são amostrados.
const MAX_SURFACE_VERTICES: usize = 60_000;
/// Pontos examinados para achar a altura sob o mouse.
const PROBE_POINTS: f32 = 20_000.0;
/// Abaixo disto (mm) não há material para desenhar.
const SURFACE_MIN_MM: f32 = 0.02;

/// Cor da altura `t` (1.0 = média do programa): frio abaixo, verde no alvo,
/// quente onde sobe mais.
fn heat_color(t: f32) -> Color32 {
    let [(first, c0), .., (last, c1)] = HEAT_STOPS;
    if t <= first {
        return Color32::from_rgb(c0[0], c0[1], c0[2]);
    }
    if t >= last {
        return Color32::from_rgb(c1[0], c1[1], c1[2]);
    }
    for pair in HEAT_STOPS.windows(2) {
        let ((t0, a), (t1, b)) = (pair[0], pair[1]);
        if t <= t1 {
            let k = (t - t0) / (t1 - t0);
            let mix = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * k) as u8;
            return Color32::from_rgb(mix(0), mix(1), mix(2));
        }
    }
    Color32::from_rgb(c1[0], c1[1], c1[2])
}

/// Malha da superfície depositada: cada ponto sobe a altura do mapa, na cor da
/// altura, sombreado pela inclinação para os cordões aparecerem. A peça gira
/// `rotation` graus junto com o depósito. Quadriláteros de costas para a
/// câmera não entram, e é isso que esconde o lado de trás da peça.
fn surface_mesh(
    proj: &Projector,
    map: &HeightMap,
    mean: f32,
    rotation: f32,
    internal: bool,
) -> Mesh {
    let mut mesh = Mesh::default();
    let cells = (map.nx * map.na) as f32;
    let stride = (cells / MAX_SURFACE_VERTICES as f32).sqrt().ceil().max(1.0) as usize;
    let cols = map.nx.div_ceil(stride);
    let rows = (map.na / stride).max(3);
    let mean = mean.max(1e-3);
    let side = if internal { -1.0 } else { 1.0 };
    let (forward, eye) = (proj.basis.forward, proj.basis.eye);
    let light = normalize(add(
        scale(forward, -1.0),
        add(scale(proj.basis.up, 0.6), scale(proj.basis.right, -0.4)),
    ));
    let row_mm = std::f32::consts::TAU * map.radius / rows as f32;
    let h_at = |row: usize, col: usize| map.height(col * stride, row * map.na / rows);

    let mut index = vec![u32::MAX; rows * cols];
    let mut outward = vec![Vec3::default(); rows * cols];
    let mut points = vec![Vec3::default(); rows * cols];
    let mut height = vec![0.0_f32; rows * cols];
    for row in 0..rows {
        let angle = (row as f32 * 360.0 / rows as f32 + 90.0).to_radians();
        let (s, c) = angle.sin_cos();
        let radial = rotate_part(Vec3::new(0.0, c, s), rotation);
        let tangent = rotate_part(Vec3::new(0.0, -s, c), rotation);
        let (up_row, down_row) = ((row + 1) % rows, (row + rows - 1) % rows);
        for col in 0..cols {
            let h = h_at(row, col);
            let r = map.radius + side * h;
            let x = map.x_at(col * stride);
            let at = row * cols + col;
            let (left, right) = (col.saturating_sub(1), (col + 1).min(cols - 1));
            let run = (right - left).max(1) as f32 * stride as f32 * map.dx;
            let slope_x = (h_at(row, right) - h_at(row, left)) / run;
            let slope_a = (h_at(up_row, col) - h_at(down_row, col)) / (2.0 * row_mm);
            let n = normalize(sub(
                scale(radial, side),
                add(
                    scale(Vec3::new(1.0, 0.0, 0.0), slope_x),
                    scale(tangent, slope_a),
                ),
            ));
            let lit = 0.55 + 0.45 * dot(n, light).max(0.0);
            let pos = rotate_part(Vec3::new(x, r * c, r * s), rotation);
            outward[at] = scale(radial, side);
            points[at] = pos;
            height[at] = h;
            if let Some(screen) = proj.project(pos) {
                index[at] = mesh.vertices.len() as u32;
                mesh.colored_vertex(screen, shade(heat_color(h / mean), lit));
            }
        }
    }
    for row in 0..rows {
        let next = (row + 1) % rows;
        for col in 0..cols.saturating_sub(1) {
            let corners = [
                row * cols + col,
                row * cols + col + 1,
                next * cols + col + 1,
                next * cols + col,
            ];
            if corners.iter().all(|&c| height[c] < SURFACE_MIN_MM) {
                continue;
            }
            if corners.iter().any(|&c| index[c] == u32::MAX) {
                continue;
            }
            let [a, ..] = corners;
            if dot(outward[a], sub(points[a], eye)) >= 0.0 {
                continue;
            }
            let [i0, i1, i2, i3] = corners.map(|c| index[c]);
            mesh.add_triangle(i0, i1, i2);
            mesh.add_triangle(i0, i2, i3);
        }
    }
    mesh
}

/// O ponto da superfície sob o mouse.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SurfaceProbe {
    x: f32,
    height: f32,
    screen: Pos2,
}

/// Procura o ponto com material mais perto de `pointer`, só no lado virado para
/// a câmera, a menos de [`PICK_RADIUS`] vezes dois.
fn surface_probe(
    proj: &Projector,
    map: &HeightMap,
    pointer: Pos2,
    rotation: f32,
    internal: bool,
) -> Option<SurfaceProbe> {
    let cells = (map.nx * map.na) as f32;
    let stride = (cells / PROBE_POINTS).sqrt().ceil().max(1.0) as usize;
    let side = if internal { -1.0 } else { 1.0 };
    let reach = PICK_RADIUS * 2.0;
    let mut best: Option<(f32, SurfaceProbe)> = None;
    for ia in (0..map.na).step_by(stride) {
        let angle = (map.a_at(ia) + 90.0).to_radians();
        let (s, c) = angle.sin_cos();
        let radial = rotate_part(Vec3::new(0.0, c, s), rotation);
        for ix in (0..map.nx).step_by(stride) {
            let height = map.height(ix, ia);
            if height < SURFACE_MIN_MM {
                continue;
            }
            let r = map.radius + side * height;
            let x = map.x_at(ix);
            let pos = rotate_part(Vec3::new(x, r * c, r * s), rotation);
            if dot(scale(radial, side), sub(pos, proj.basis.eye)) >= 0.0 {
                continue;
            }
            let Some(screen) = proj.project(pos) else {
                continue;
            };
            let d = screen.distance(pointer);
            if d < reach && best.is_none_or(|(b, _)| d < b) {
                best = Some((d, SurfaceProbe { x, height, screen }));
            }
        }
    }
    best.map(|(_, probe)| probe)
}

/// Marca o ponto e mostra a altura dele e a diferença para a média.
fn draw_probe(painter: &Painter, at: Pos2, probe: SurfaceProbe, mean: f32, palette: &Palette) {
    painter.circle_stroke(probe.screen, 4.0, Stroke::new(1.5, palette.text));
    let text = format!(
        "X {:.1} · altura {:.2} mm ({:+.2})",
        probe.x,
        probe.height,
        probe.height - mean
    );
    let galley = painter.layout_no_wrap(text, FontId::proportional(12.0), palette.text);
    let box_at = at + vec2(14.0, 14.0);
    let rect = Rect::from_min_size(box_at, galley.size()).expand2(vec2(6.0, 3.0));
    painter.rect_filled(rect, CornerRadius::same(4), palette.panel_bg);
    painter.galley(box_at, galley, palette.text);
}

// ---------------------------------------------------------------------------
// Perfil 3D do cordão ("Solda")
// ---------------------------------------------------------------------------

/// Superfície onde o cordão é depositado: dá a normal em cada ponto.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Surface {
    /// Peça cilíndrica (eixo X): normal radial, para dentro na operação
    /// interna.
    Cylinder { internal: bool },
    /// Face deitada, virada para a tocha (chapa plana e bolacha): normal +Z.
    Flat,
}

impl Surface {
    fn normal_at(self, pos: Vec3) -> Vec3 {
        match self {
            Surface::Cylinder { internal } => {
                let radial = normalize(Vec3::new(0.0, pos.y, pos.z));
                let radial = if length(radial) < 0.5 {
                    Vec3::new(0.0, 0.0, 1.0)
                } else {
                    radial
                };
                if internal {
                    scale(radial, -1.0)
                } else {
                    radial
                }
            }
            Surface::Flat => Vec3::new(0.0, 0.0, 1.0),
        }
    }
}

/// Seção do cordão: largura efetiva do motor (a mesma do cálculo do passo) e
/// altura ("Espessura / Camada"), sobre a superfície da operação.
/// `include_tail` conta o trecho de apagamento como material: no fechamento
/// uniforme o anel só fecha com ele.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BeadShape {
    width: f32,
    height: f32,
    surface: Surface,
    include_tail: bool,
}

impl BeadShape {
    fn from_params(p: &Params) -> Self {
        let surface = match p.welding_mode {
            WeldingMode::ChapaPlana | WeldingMode::Bolacha => Surface::Flat,
            _ => Surface::Cylinder {
                internal: p.is_internal(),
            },
        };
        let width = overlap_metrics(p, 0, None, None).bead.effective_width_mm;
        BeadShape {
            width: finite_or(width, 8.0).max(0.2),
            height: finite_or(p.espessura_camada, 1.5).max(0.05),
            surface,
            // Na chapa o cordão para antes da borda e termina no apagamento.
            include_tail: p.fechamento_uniforme || p.welding_mode == WeldingMode::ChapaPlana,
        }
    }
}

/// Tangente do cordão na emenda entre um trecho (`own`) e o vizinho. Na curva é
/// a média dos dois, para eles dividirem a aresta e o lado de fora não abrir em
/// cunha. No canto vivo, ou sem vizinho, o trecho fica com a sua.
fn joint_tangent(own: Vec3, neighbor: Option<Vec3>) -> Vec3 {
    match neighbor {
        Some(t) if dot(own, t) > BEAD_JOINT_COS => normalize(add(own, t)),
        _ => own,
    }
}

/// Malha do perfil 3D do cordão até o ponto `limit`: cada trecho com tocha
/// ligada vira duas facetas (pé esquerdo→crista e crista→pé direito) com a
/// largura e a altura do cordão, na cor da camada sombreada por uma luz presa
/// à câmera. Facetas de costas para a câmera não entram (é o que esconde o
/// lado de trás da peça). Os trechos saem na ordem do percurso, então o
/// cordão novo cobre a borda do anterior, como no depósito real. Acima de
/// [`MAX_BEAD_SEGMENTS`] trechos o percurso é decimado.
fn bead_mesh(
    proj: &Projector,
    tp: &Toolpath,
    limit: usize,
    spin: Spin,
    bead: BeadShape,
    palette: &Palette,
) -> Mesh {
    let mut mesh = Mesh::default();
    let total = tp.point_count();
    if total == 0 || limit == 0 {
        return mesh;
    }
    let stride = total.div_ceil(MAX_BEAD_SEGMENTS).max(1);
    let forward = proj.basis.forward;
    let light = normalize(add(
        scale(forward, -1.0),
        add(scale(proj.basis.up, 0.6), scale(proj.basis.right, -0.4)),
    ));
    let half = bead.width * 0.5;
    let deposits =
        |pt: &PathPoint| !pt.rapid && (pt.torch_on || (bead.include_tail && pt.extinguishing));
    let place = |pt: &PathPoint| spin.apply(pt.pos);
    let mut consumed = 0usize;
    for seg in &tp.segments {
        if consumed >= limit {
            break;
        }
        let n = seg.len().min(limit - consumed);
        let pts = &seg[..n];
        // Direção do trecho entre dois pontos, se ele deposita material.
        let direction = |i: usize, j: usize| {
            let (a, b) = (pts.get(i)?, pts.get(j)?);
            let t = normalize(sub(place(b), place(a)));
            (deposits(a) && deposits(b) && length(t) >= 0.5).then_some(t)
        };
        let mut k = 0;
        while k + stride < n {
            let (i, j) = (k, k + stride);
            k = j;
            let Some(t) = direction(i, j) else { continue };
            let (pa, pc) = (place(&pts[i]), place(&pts[j]));
            let nrm = bead.surface.normal_at(pa);
            let nrm_c = bead.surface.normal_at(pc);
            let s = normalize(cross(nrm, t));
            let before = i.checked_sub(stride).and_then(|h| direction(h, i));
            let s_a = normalize(cross(nrm, joint_tangent(t, before)));
            let after = direction(j, j + stride);
            let s_c = normalize(cross(nrm_c, joint_tangent(t, after)));
            if [s, s_a, s_c].iter().any(|v| length(*v) < 0.5) {
                continue;
            }
            let color = palette.layer_color(pts[i].layer);
            let top_a = add(pa, scale(nrm, bead.height));
            let top_c = add(pc, scale(nrm_c, bead.height));
            let left_a = sub(pa, scale(s_a, half));
            let left_c = sub(pc, scale(s_c, half));
            let right_a = add(pa, scale(s_a, half));
            let right_c = add(pc, scale(s_c, half));
            let facets = [
                (
                    [left_a, top_a, top_c, left_c],
                    normalize(sub(scale(nrm, half), scale(s, bead.height))),
                ),
                (
                    [top_a, right_a, right_c, top_c],
                    normalize(add(scale(nrm, half), scale(s, bead.height))),
                ),
            ];
            for (quad, facet_normal) in facets {
                if dot(facet_normal, forward) > 0.05 {
                    continue;
                }
                let k = 0.45 + 0.55 * dot(facet_normal, light).max(0.0);
                let col = shade(color, k);
                let screen = [
                    proj.project(quad[0]),
                    proj.project(quad[1]),
                    proj.project(quad[2]),
                    proj.project(quad[3]),
                ];
                let [Some(s0), Some(s1), Some(s2), Some(s3)] = screen else {
                    continue;
                };
                let base = mesh.vertices.len() as u32;
                for p in [s0, s1, s2, s3] {
                    mesh.colored_vertex(p, col);
                }
                mesh.add_triangle(base, base + 1, base + 2);
                mesh.add_triangle(base, base + 2, base + 3);
            }
        }
        consumed += n;
    }
    mesh
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
            show_weld: true,
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

    /// Leva a simulação ao ponto `at` (0..=1) e a deixa parada ali: é o que a
    /// linha do tempo faz ao ser arrastada.
    pub fn seek(&mut self, at: f32) {
        self.playing = false;
        let at = if at.is_finite() { at } else { 0.0 };
        self.progress = at.clamp(1e-4, 1.0);
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

/// Depósito acumulado até o ponto atual da simulação.
#[derive(Clone, Debug)]
struct PartialDeposit {
    key: u64,
    stamps: usize,
    map: HeightMap,
}

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
    /// Depósito acumulado na simulação (ver [`Self::partial_map`]).
    partial: Option<PartialDeposit>,
    /// `(yaw, pitch)` para onde a câmera está deslizando.
    cam_goal: Option<(f32, f32)>,
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
            partial: None,
            cam_goal: None,
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
        self.cam_goal = None;
    }

    /// Aproxima a câmera da vista pedida em `dt` segundos, pelo caminho mais
    /// curto. Devolve `true` enquanto ainda está a caminho.
    fn animate_camera(&mut self, dt: f32) -> bool {
        let Some((yaw, pitch)) = self.cam_goal else {
            return false;
        };
        let k = 1.0 - (-CAMERA_EASE * dt).exp();
        let dyaw = wrap_deg(yaw - self.cam.yaw);
        let dpitch = pitch - self.cam.pitch;
        if dyaw.abs().max(dpitch.abs()) < CAMERA_SNAP_DEG {
            self.cam.yaw = yaw;
            self.cam.pitch = pitch;
            self.cam_goal = None;
            return false;
        }
        self.cam.yaw = wrap_deg(self.cam.yaw + dyaw * k);
        self.cam.pitch += dpitch * k;
        true
    }

    /// Menu "Simulação > Vista ...", teclas 1–5 e ⌂ do cubo. Uma vista nova
    /// é um recomeço: o próximo resultado volta a enquadrar.
    pub fn set_preset(&mut self, v: ViewPreset) {
        self.cam_goal = Some(v.angles());
        self.user_adjusted = false;
    }

    /// Clique numa face, aresta ou canto do cubo de orientação: também é um
    /// recomeço, como um preset.
    pub fn look_from(&mut self, dir: Vec3) {
        let mut goal = self.cam;
        goal.look_from(dir);
        self.cam_goal = Some((goal.yaw, goal.pitch));
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

    /// Mapa de altura com o programa depositado até o ponto `limit`. Avança
    /// só com as amostras novas; voltar para trás, ou um resultado novo,
    /// recomeça do zero.
    fn partial_map(&mut self, key: u64, m: &DepositModel, limit: usize) -> &HeightMap {
        let want = m.point_stamps[limit.min(m.points)] as usize;
        if self
            .partial
            .as_ref()
            .is_none_or(|p| p.key != key || p.stamps > want)
        {
            self.partial = None;
        }
        let p = self.partial.get_or_insert_with(|| PartialDeposit {
            key,
            stamps: 0,
            map: m.map.empty_like(),
        });
        for s in &m.stamps[p.stamps..want] {
            p.map.stamp(s);
        }
        p.stamps = want;
        &p.map
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
    // No cilindro a cena é a peça: o recuo de segurança da tocha, dezenas de
    // milímetros acima dela, deixaria a peça pequena na tela.
    if let Some(p) = params.filter(|p| horizontal_cylinder(p)) {
        return reference_bounds(p);
    }
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
            let z = finite_or(p.afastamento_tocha, 0.0);
            Bounds {
                min: Vec3::new(-r, -r, z),
                max: Vec3::new(r, r, z),
            }
        }
        _ => {
            let r = cylinder_radius(p) + finite_or(p.espessura_camada, 0.0).max(0.0);
            let (x0, x1) = cylinder_span(p);
            Bounds {
                min: Vec3::new(x0, -r, -r),
                max: Vec3::new(x1, r, r),
            }
        }
    }
}

/// Trecho da peça em X: o comprimento a revestir, a partir de X0 no sentido da
/// solda; o percurso pode ir além (aproximações, apagamento).
fn cylinder_span(p: &Params) -> (f32, f32) {
    let len = finite_or(p.comprimento_revestir, 0.0).max(1.0);
    match p.direcao_soldagem {
        Direction::EsquerdaDireita => (0.0, len),
        Direction::DireitaEsquerda => (-len, 0.0),
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

/// Peça cilíndrica na horizontal: toda operação que não seja chapa plana nem
/// bolacha, que ficam deitadas.
fn horizontal_cylinder(p: &Params) -> bool {
    !matches!(
        p.welding_mode,
        WeldingMode::ChapaPlana | WeldingMode::Bolacha
    )
}

/// Giro da peça na cena. O ângulo é o A do ponto que está sob a tocha.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Spin {
    /// A peça fica parada e quem anda é a tocha (chapa plana).
    Still,
    /// Cilindro: gira no eixo X e o ponto atual vai para o topo.
    Axis(f32),
    /// Bolacha: a mesa gira no eixo Z e o ponto atual vai para +X.
    Table(f32),
}

impl Spin {
    /// Giro da operação `p` com o ponto `current` sob a tocha.
    fn of(p: &Params, current: Option<&PathPoint>) -> Self {
        let a = current.map_or(0.0, |pt| pt.a_deg);
        match p.welding_mode {
            WeldingMode::ChapaPlana => Spin::Still,
            WeldingMode::Bolacha => Spin::Table(a),
            _ => Spin::Axis(a),
        }
    }

    /// Onde `pos`, dado na posição de A = 0, está com a peça girada.
    fn apply(self, pos: Vec3) -> Vec3 {
        match self {
            Spin::Still => pos,
            Spin::Axis(a) => rotate_part(pos, a),
            Spin::Table(a) => rotate_table(pos, a),
        }
    }
}

/// Gira `pos` em torno do eixo X de `-a_deg`: o ponto que está no ângulo A
/// da peça vai para o topo (sob a tocha), como quando a peça gira na
/// máquina. X e o raio não mudam; ângulo zero ou não finito devolve o ponto
/// como está.
fn rotate_part(pos: Vec3, a_deg: f32) -> Vec3 {
    if a_deg == 0.0 || !a_deg.is_finite() {
        return pos;
    }
    let (s, c) = a_deg.to_radians().sin_cos();
    Vec3::new(pos.x, pos.y * c + pos.z * s, pos.z * c - pos.y * s)
}

/// Gira `pos` em torno do eixo Z de `-a_deg`: o ponto que está no ângulo A da
/// mesa vai para +X (sob a tocha), como quando a mesa gira na máquina. Com A
/// subindo, a peça gira no sentido horário visto de cima. A altura e o raio não
/// mudam; ângulo zero ou não finito devolve o ponto como está.
fn rotate_table(pos: Vec3, a_deg: f32) -> Vec3 {
    if a_deg == 0.0 || !a_deg.is_finite() {
        return pos;
    }
    let (s, c) = a_deg.to_radians().sin_cos();
    Vec3::new(pos.x * c + pos.y * s, pos.y * c - pos.x * s, pos.z)
}

/// Quantos pontos do percurso o quadro desenha: todos fora da simulação; a
/// fração `progress` (ao menos um) durante ela; nenhum sem percurso.
fn draw_limit(sim: &Sim, total: usize) -> usize {
    if total == 0 {
        0
    } else if sim.active() {
        ((sim.progress * total as f32).round() as usize).clamp(1, total)
    } else {
        total
    }
}

/// O ponto que está sendo depositado neste quadro: o último dos `limit`
/// pontos desenhados (`None` sem pontos). Ele dita a rotação da peça e a
/// leitura da tocha.
fn current_point(tp: &Toolpath, limit: usize) -> Option<&PathPoint> {
    let total = tp.point_count();
    if limit == 0 || total == 0 {
        return None;
    }
    let mut index = limit.min(total) - 1;
    for seg in &tp.segments {
        if index < seg.len() {
            return seg.get(index);
        }
        index -= seg.len();
    }
    None
}

fn view_ui(app: &TfmApp, ui: &mut Ui, state: &mut ViewState) -> Option<usize> {
    let palette = app.palette;
    let result = app.result.as_ref();
    if let Some(r) = result {
        state.note_result(r.key, Operation::from_params(&r.params));
    }
    let toolpath = result.map(|r| &r.toolpath).filter(|t| !t.is_empty());
    let total = toolpath.map_or(0, Toolpath::point_count);

    let avail = ui.available_size();
    let size = vec2(avail.x.max(32.0), avail.y.max(32.0));
    let (response, painter) = ui.allocate_painter(size, Sense::click_and_drag());
    let rect = response.rect;
    painter.rect_filled(rect, CornerRadius::same(4), palette.card_bg);

    let bounds = scene_bounds(toolpath, result.map(|r| &r.params));
    interact(ui, &response, state, rect, &bounds, result.is_some());

    let dt = ui.input(|i| i.stable_dt).min(0.1);
    if state.animate_camera(dt) {
        ui.ctx().request_repaint();
    }
    if state.sim.playing {
        state.sim.advance(dt, total);
        ui.ctx().request_repaint();
    }

    // No cilindro e na bolacha quem gira é a peça: o ponto que está sendo
    // depositado fica sempre sob a tocha fixa. Fora da simulação o "atual" é o
    // último ponto, então a peça aparece como termina.
    let current = toolpath.and_then(|tp| current_point(tp, draw_limit(&state.sim, total)));
    let spin = result.map_or(Spin::Still, |r| Spin::of(&r.params, current));
    let rotation = current.map_or(0.0, |pt| pt.a_deg);

    let proj = Projector::new(&state.cam, rect);
    let floor = has_floor(result.map(|r| &r.params));
    draw_scene_base(&painter, &proj, &bounds, floor, &palette);
    if let Some(r) = result {
        draw_reference(&painter, &proj, &r.params, spin, &palette);
    }
    // Com "Solda" ligada as peças cilíndricas mostram o depósito real, que sobe
    // junto com a simulação. Chapa e bolacha não têm mapa e usam o cordão.
    let deposit = result
        .and_then(|r| r.deposit.as_deref())
        .filter(|_| state.sim.show_weld);
    if let (Some(m), Some(r)) = (deposit, result) {
        let limit = draw_limit(&state.sim, total);
        let map = if state.sim.active() {
            state.partial_map(r.key, m, limit)
        } else {
            &m.map
        };
        let mesh = surface_mesh(&proj, map, m.report.mean_mm, rotation, m.internal);
        painter.add(Shape::mesh(mesh));
    }
    let bead = result
        .filter(|_| deposit.is_none())
        .map(|r| BeadShape::from_params(&r.params));
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
                spin,
                bead,
                deposit.is_some(),
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
    let readout = current
        .filter(|_| state.sim.active() && spin != Spin::Still)
        .map(|pt| machine_readout(pt, spin));
    overlay(&painter, rect, toolpath, readout, deposit, &palette);
    view_cube(ui, &painter, state, rect, &palette);
    let bar = (total > 0).then(|| {
        let weld_min = deposit.map(|m| m.report.weld_min);
        timeline(ui, &painter, state, rect, total, weld_min, &palette)
    });
    if let (Some(m), Some(at)) = (deposit, response.hover_pos()) {
        let free = !cube_rect(rect).expand(CUBE_HOME + 4.0).contains(at)
            && !bar.is_some_and(|b| b.contains(at));
        if free {
            let probe = surface_probe(&proj, &m.map, at, rotation, m.internal);
            if let Some(probe) = probe {
                draw_probe(&painter, at, probe, m.report.mean_mm, &palette);
            }
        }
    }
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

/// O que aparece no 3D: fica na mesma linha das abas.
pub fn layer_toggles(ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let (mut trace, mut weld) = with_state(&ctx, |s| (s.sim.show_trace, s.sim.show_weld));
    let solda = ui
        .toggle_value(&mut weld, "Solda")
        .on_hover_text("Material depositado: a cor mostra onde a camada sobe mais");
    let tracado = ui
        .toggle_value(&mut trace, "Traçado")
        .on_hover_text("Trajetória da tocha");
    if solda.changed() || tracado.changed() {
        with_state(&ctx, |s| {
            s.sim.show_trace = trace;
            s.sim.show_weld = weld;
        });
    }
}

/// `mm:ss`, ou `h:mm:ss` a partir de uma hora.
fn fmt_clock(minutes: f32) -> String {
    let total = if minutes.is_finite() {
        (minutes.max(0.0) * 60.0).round() as u64
    } else {
        0
    };
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// Linha do tempo sobre o 3D: play/pausa num botão só, parar, barra arrastável
/// para ir a qualquer ponto, relógio e velocidade. Devolve o retângulo dela.
fn timeline(
    ui: &mut Ui,
    painter: &Painter,
    state: &mut ViewState,
    viewport: Rect,
    total: usize,
    weld_min: Option<f32>,
    palette: &Palette,
) -> Rect {
    let bar = Rect::from_min_max(
        pos2(viewport.left() + 8.0, viewport.bottom() - TIMELINE_H - 8.0),
        pos2(viewport.right() - 8.0, viewport.bottom() - 8.0),
    );
    painter.rect_filled(
        bar,
        CornerRadius::same(8),
        palette.panel_bg.gamma_multiply(0.92),
    );
    let id = ui.scope_id().with("linha_do_tempo");
    let y = bar.center().y;
    let button = |cx: f32| Rect::from_center_size(pos2(cx, y), vec2(26.0, 26.0));

    let play_rect = button(bar.left() + 18.0);
    let playing = state.sim.playing;
    let hint = if playing {
        "Pausar (Espaço)"
    } else {
        "Simular (Espaço)"
    };
    let play = ui
        .interact(play_rect, id.with("play"), Sense::click())
        .on_hover_text(hint);
    if play.clicked() {
        if playing {
            state.sim_pause();
        } else {
            state.sim_play();
        }
    }
    let color = |hovered: bool| {
        if hovered {
            palette.accent
        } else {
            palette.text
        }
    };
    let c = play_rect.center();
    if playing {
        for dx in [-3.5, 3.5] {
            let r = Rect::from_center_size(c + vec2(dx, 0.0), vec2(4.0, 13.0));
            painter.rect_filled(r, CornerRadius::same(1), color(play.hovered()));
        }
    } else {
        // Um triângulo centrado pela caixa parece torto: 1 px à direita.
        let points = vec![
            c + vec2(-4.0, -7.0),
            c + vec2(8.0, 0.0),
            c + vec2(-4.0, 7.0),
        ];
        painter.add(Shape::convex_polygon(
            points,
            color(play.hovered()),
            Stroke::NONE,
        ));
    }

    let stop_rect = button(bar.left() + 46.0);
    let stop = ui
        .interact(stop_rect, id.with("stop"), Sense::click())
        .on_hover_text("Parar e voltar ao início");
    if stop.clicked() {
        state.sim_stop();
    }
    painter.rect_filled(
        Rect::from_center_size(stop_rect.center(), vec2(11.0, 11.0)),
        CornerRadius::same(2),
        color(stop.hovered()),
    );

    let speed_rect = Rect::from_center_size(pos2(bar.right() - 26.0, y), vec2(44.0, 22.0));
    let speed = ui
        .interact(speed_rect, id.with("velocidade"), Sense::click())
        .on_hover_text("Velocidade da simulação: clique para acelerar, botão direito para reduzir");
    let i = state.sim.speed_index();
    if speed.clicked() {
        state.sim.set_speed_index((i + 1) % SPEEDS.len());
    } else if speed.secondary_clicked() {
        state
            .sim
            .set_speed_index((i + SPEEDS.len() - 1) % SPEEDS.len());
    }
    let font = FontId::monospace(11.0);
    painter.text(
        speed_rect.center(),
        Align2::CENTER_CENTER,
        speed_label(state.sim.speed),
        font.clone(),
        color(speed.hovered()),
    );

    let progress = state.sim.progress.clamp(0.0, 1.0);
    let clock = match weld_min {
        Some(m) => format!("{} / {}", fmt_clock(m * progress), fmt_clock(m)),
        None => format!("{} / {total}", (progress * total as f32).round() as usize),
    };
    let clock_w = 104.0;
    painter.text(
        pos2(speed_rect.left() - 6.0, y),
        Align2::RIGHT_CENTER,
        clock,
        font,
        palette.muted,
    );

    let track = Rect::from_min_max(
        pos2(stop_rect.right() + 8.0, y - 13.0),
        pos2(speed_rect.left() - clock_w - 10.0, y + 13.0),
    );
    let drag = ui
        .interact(track, id.with("barra"), Sense::click_and_drag())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if let Some(at) = drag
        .interact_pointer_pos()
        .filter(|_| drag.is_pointer_button_down_on())
    {
        state
            .sim
            .seek((at.x - track.left()) / track.width().max(1.0));
    }
    let rail = Rect::from_center_size(track.center(), vec2(track.width(), 4.0));
    painter.rect_filled(
        rail,
        CornerRadius::same(2),
        palette.muted.gamma_multiply(0.35),
    );
    let x = track.left() + track.width() * state.sim.progress.clamp(0.0, 1.0);
    let done = Rect::from_min_max(rail.min, pos2(x, rail.max.y));
    painter.rect_filled(done, CornerRadius::same(2), palette.accent);
    let grow =
        ui.ctx()
            .animate_bool_with_time(id.with("botao"), drag.hovered() || drag.dragged(), 0.1);
    painter.circle_filled(pos2(x, y), 5.0 + 2.0 * grow, palette.accent);
    bar
}

/// A cena tem plano do chão? Só sob a peça deitada (chapa plana e bolacha): o
/// cilindro fica na horizontal, entre os eixos da máquina.
fn has_floor(p: Option<&Params>) -> bool {
    p.is_some_and(|p| !horizontal_cylinder(p))
}

/// Base da cena: o plano do chão, quando há, e o triedro XYZ.
fn draw_scene_base(
    painter: &Painter,
    proj: &Projector,
    b: &Bounds,
    floor: bool,
    palette: &Palette,
) {
    let spacing = nice_step(b.extent() / 8.0);
    if floor {
        draw_grid(painter, proj, b, spacing, palette);
    }
    draw_axes(painter, proj, spacing, palette);
}

/// Grade do chão: plano XY logo abaixo da peça, em passos de `spacing` mm.
fn draw_grid(painter: &Painter, proj: &Projector, b: &Bounds, spacing: f32, palette: &Palette) {
    let n = ((b.extent() * 0.75 / spacing).ceil() as i32).clamp(4, 40);
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
}

/// Triedro XYZ na origem da máquina: nas peças que giram é o centro do eixo A
/// (X0 na linha de centro); na bolacha, o centro da mesa na altura zero; na
/// chapa plana, o canto X0 Y0 na altura zero.
fn draw_axes(painter: &Painter, proj: &Projector, spacing: f32, palette: &Palette) {
    let origin = Vec3::default();
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

/// Silhueta da peça em cinza fino: cilindro com o comprimento a revestir no
/// sentido da solda (dois círculos, 4 geratrizes e a linha de centro do eixo
/// A passando [`AXIS_OVERHANG`] das faces), chapa (retângulo na altura da
/// tocha) ou bolacha (anéis interno/externo, deitados na altura da tocha).
/// `spin` é o giro atual da peça: as geratrizes do cilindro e os raios da
/// bolacha giram com ela; a chapa fica parada.
fn draw_reference(painter: &Painter, proj: &Projector, p: &Params, spin: Spin, palette: &Palette) {
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
            let z = finite_or(p.afastamento_tocha, 0.0);
            let r_out = finite_or(p.diametro, 0.0) * 0.5;
            let r_in = finite_or(p.diametro_interno, 0.0) * 0.5;
            if r_out > 0.0 {
                draw_circle(painter, proj, stroke, |c, s| {
                    Vec3::new(r_out * c, r_out * s, z)
                });
            }
            let inner = if r_in > 0.0 && r_in < r_out {
                r_in
            } else {
                0.0
            };
            if inner > 0.0 {
                draw_circle(painter, proj, stroke, |c, s| {
                    Vec3::new(inner * c, inner * s, z)
                });
            }
            for k in 0..4 {
                let (s, c) = (k as f32 * std::f32::consts::FRAC_PI_2).sin_cos();
                line(
                    spin.apply(Vec3::new(inner * c, inner * s, z)),
                    spin.apply(Vec3::new(r_out * c, r_out * s, z)),
                );
            }
        }
        _ => {
            let r = cylinder_radius(p);
            let (x0, x1) = cylinder_span(p);
            draw_circle(painter, proj, stroke, |c, s| Vec3::new(x0, r * c, r * s));
            draw_circle(painter, proj, stroke, |c, s| Vec3::new(x1, r * c, r * s));
            for k in 0..4 {
                let (s, c) = (k as f32 * std::f32::consts::FRAC_PI_2).sin_cos();
                line(
                    spin.apply(Vec3::new(x0, r * c, r * s)),
                    spin.apply(Vec3::new(x1, r * c, r * s)),
                );
            }
            // Linha de centro do eixo A: é em torno dela que a peça gira.
            let over = (x1 - x0) * AXIS_OVERHANG;
            line(
                Vec3::new(x0 - over, 0.0, 0.0),
                Vec3::new(x1 + over, 0.0, 0.0),
            );
        }
    }
}

/// Círculo traçado por `at(cos, sin)`, que dá o plano e o raio.
fn draw_circle(painter: &Painter, proj: &Projector, stroke: Stroke, at: impl Fn(f32, f32) -> Vec3) {
    let step = std::f32::consts::TAU / CIRCLE_SEGMENTS as f32;
    for i in 0..CIRCLE_SEGMENTS {
        let (s0, c0) = (i as f32 * step).sin_cos();
        let (s1, c1) = ((i + 1) as f32 * step).sin_cos();
        if let Some(seg) = proj.segment(at(c0, s0), at(c1, s1)) {
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
/// `spin` é o giro atual da peça: cada ponto é girado antes de ser projetado,
/// de modo que o ponto sendo depositado fique sob a tocha.
/// Com "Solda" ligada e `bead` presente, o perfil 3D do cordão
/// ([`bead_mesh`]) é desenhado antes do traçado. Com `surface` ligada, a
/// superfície real já foi desenhada e o traçado sai, porque atravessaria a peça.
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
    spin: Spin,
    bead: Option<BeadShape>,
    surface: bool,
) -> Option<usize> {
    let total = tp.point_count();
    if total == 0 {
        return None;
    }
    // A marca da linha selecionada gira presa à peça: com a simulação andando,
    // pareceria uma segunda tocha. Ela volta na pausa.
    let selected = selected.filter(|_| !state.sim.playing);
    let step = decimation_step(total);
    let limit = draw_limit(&state.sim, total);
    let show_trace = state.sim.show_trace && !surface;
    let show_weld = state.sim.show_weld;
    if let (true, Some(bead)) = (show_weld, bead) {
        let mesh = bead_mesh(proj, tp, limit, spin, bead, palette);
        if !mesh.is_empty() {
            painter.add(Shape::mesh(mesh));
        }
    }

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
            let pos = spin.apply(pt.pos);
            let screen = proj.project(pos);
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
                torch_at = Some((pos, s));
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

/// Números do depósito e a escala de cor, no canto inferior esquerdo.
fn deposit_legend(painter: &Painter, rect: Rect, m: &DepositModel, palette: &Palette) {
    let font = FontId::proportional(11.0);
    let r = &m.report;
    let mut lines = vec![
        format!(
            "Altura: mín {:.2} · média {:.2} · máx {:.2} mm",
            r.min_mm, r.mean_mm, r.max_mm
        ),
        format!(
            "Sobremetal {:.2} mm · {:.1} cm³ · solda {:.1} min",
            r.sobremetal_mm,
            r.volume_mm3 / 1000.0,
            r.weld_min
        ),
    ];
    if let Some(b) = &m.base {
        lines.push(format!(
            "Sem compensação: {:.2} mm · {:.1} cm³ · {:.1} min",
            b.sobremetal_mm,
            b.volume_mm3 / 1000.0,
            b.weld_min
        ));
    }
    if r.uncovered_pct > 0.5 {
        lines.push(format!("Sem material em {:.1}% da área", r.uncovered_pct));
    }
    let left = rect.left() + 8.0;
    let mut y = rect.bottom() - TIMELINE_H - 16.0;
    for line in lines.iter().rev() {
        painter.text(
            pos2(left, y),
            Align2::LEFT_BOTTOM,
            line,
            font.clone(),
            palette.text,
        );
        y -= 15.0;
    }
    // Escala de cor: da altura mais baixa à mais alta.
    let (first, last) = (HEAT_STOPS[0].0, HEAT_STOPS[HEAT_STOPS.len() - 1].0);
    let (w, h) = (2.0, 8.0);
    for i in 0..60 {
        let t = first + (last - first) * i as f32 / 59.0;
        let at = pos2(left + i as f32 * w, y - h - 2.0);
        painter.rect_filled(
            Rect::from_min_size(at, vec2(w, h)),
            CornerRadius::ZERO,
            heat_color(t),
        );
    }
    let label_y = y - h - 4.0;
    for (x, text, align) in [
        (left, "baixo", Align2::LEFT_BOTTOM),
        (left + 60.0, "alvo", Align2::CENTER_BOTTOM),
        (left + 120.0, "alto", Align2::RIGHT_BOTTOM),
    ] {
        painter.text(pos2(x, label_y), align, text, font.clone(), palette.muted);
    }
}

/// Leitura da máquina na simulação: X da tocha e ângulo A da peça. Na bolacha o
/// X da máquina é o raio.
fn machine_readout(pt: &PathPoint, spin: Spin) -> String {
    let x = match spin {
        Spin::Table(_) => pt.radius,
        Spin::Still | Spin::Axis(_) => pt.pos.x,
    };
    format!("Tocha X {x:.3} · Peça A {:.1}°", pt.a_deg)
}

/// Textos sobre o viewport: resumo e, durante a simulação, a leitura da
/// máquina (acima, à esquerda: X da tocha e ângulo A da peça), aviso de
/// decimação (abaixo, à direita). O canto superior direito é do cubo de
/// orientação, e a faixa de baixo, da linha do tempo.
fn overlay(
    painter: &Painter,
    rect: Rect,
    toolpath: Option<&Toolpath>,
    readout: Option<String>,
    deposit: Option<&DepositModel>,
    palette: &Palette,
) {
    let font = FontId::proportional(11.0);
    let pad = vec2(8.0, 6.0);
    if let Some(m) = deposit {
        deposit_legend(painter, rect, m, palette);
    }
    if let Some(text) = readout {
        painter.text(
            rect.left_top() + vec2(pad.x, pad.y + 16.0),
            Align2::LEFT_TOP,
            text,
            font.clone(),
            palette.text,
        );
    }
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
                rect.right_bottom() + vec2(-pad.x, -pad.y - TIMELINE_H - 8.0),
                Align2::RIGHT_BOTTOM,
                "traçado simplificado para exibição",
                font.clone(),
                palette.muted,
            );
        }
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
        cam.preset(ViewPreset::Frente);
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
            (ViewPreset::Frente, 0.0, 0.0),
            (ViewPreset::Esquerda, -90.0, 0.0),
            (ViewPreset::Direita, 90.0, 0.0),
            (ViewPreset::Tras, 180.0, 0.0),
            (ViewPreset::Base, 0.0, -89.0),
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
        while state.animate_camera(1.0 / 60.0) {}
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
        state.set_preset(ViewPreset::Frente);
        assert!(!state.user_adjusted, "preset rearma o enquadramento");
        assert!(state.note_result(4, Operation::ChapaPlana));
        assert!(state.fit_pending);
    }

    #[test]
    fn state_lives_in_egui_memory() {
        let ctx = egui::Context::default();
        with_state(&ctx, |s| {
            s.set_preset(ViewPreset::Esquerda);
            while s.animate_camera(1.0 / 60.0) {}
        });
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
            draw_scene_base(&painter, &proj, &bounds, false, &palette);
            draw_reference(&painter, &proj, &params, Spin::Still, &palette);
            picked = draw_path(
                &painter,
                &proj,
                &tp,
                &mut state,
                &palette,
                Some(first.line_idx),
                click,
                5.0,
                Spin::Still,
                None,
                false,
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

    /// Recolhe círculos preenchidos (centro, cor) e textos pintados num quadro.
    fn collect_shapes(shape: &Shape, circles: &mut Vec<(Pos2, [u8; 4])>, texts: &mut Vec<String>) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| collect_shapes(s, circles, texts)),
            Shape::Circle(c) => circles.push((c.center, c.fill.to_array())),
            Shape::Text(t) => texts.push(t.galley.text().to_owned()),
            _ => {}
        }
    }

    /// A câmera desliza até a vista pedida, pelo caminho mais curto, e para
    /// sozinha ao chegar.
    #[test]
    fn camera_glides_to_the_requested_view() {
        let mut state = ViewState::default();
        state.cam.preset(ViewPreset::Iso);
        state.set_preset(ViewPreset::Topo);
        assert_eq!(state.cam.pitch, ViewPreset::Iso.angles().1, "não salta");
        let mut quadros = 0;
        while state.animate_camera(1.0 / 60.0) {
            quadros += 1;
            assert!(quadros < 200, "a câmera deveria chegar");
        }
        assert!(quadros > 5, "deveria levar alguns quadros: {quadros}");
        assert_eq!((state.cam.yaw, state.cam.pitch), ViewPreset::Topo.angles());

        // De 170° para -170° o caminho curto passa por 180°, não por 0°.
        state.cam.yaw = 170.0;
        state.cam_goal = Some((-170.0, 0.0));
        while state.animate_camera(1.0 / 60.0) {
            assert!(
                state.cam.yaw.abs() > 160.0,
                "deu a volta: {}",
                state.cam.yaw
            );
        }
    }

    /// Mexer na câmera com o mouse cancela o deslize.
    #[test]
    fn dragging_cancels_the_camera_glide() {
        let mut state = ViewState::default();
        state.set_preset(ViewPreset::Topo);
        state.mark_adjusted();
        assert!(!state.animate_camera(1.0 / 60.0));
    }

    /// Arrastar a linha do tempo leva a simulação ao ponto e a deixa parada.
    #[test]
    fn seeking_pauses_at_the_chosen_point() {
        let mut sim = Sim::default();
        sim.play();
        sim.seek(0.4);
        assert!(!sim.playing);
        assert_eq!(sim.progress, 0.4);
        assert!(sim.active());
        sim.seek(7.0);
        assert_eq!(sim.progress, 1.0);
        sim.seek(-1.0);
        assert!(
            sim.progress > 0.0 && sim.active(),
            "o início ainda é simulação"
        );
        sim.seek(f32::NAN);
        assert!(sim.progress.is_finite());
    }

    #[test]
    fn clock_reads_minutes_and_hours() {
        assert_eq!(fmt_clock(0.0), "00:00");
        assert_eq!(fmt_clock(12.9), "12:54");
        assert_eq!(fmt_clock(75.5), "1:15:30");
        assert_eq!(fmt_clock(f32::NAN), "00:00");
    }

    /// Passar o mouse na peça lê a altura do ponto; fora da peça, nada.
    #[test]
    fn surface_probe_reads_the_height_under_the_pointer() {
        let (_, tp, m) = deposit_scene();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0));
        let mut cam = Camera::default();
        cam.fit(tp.min, tp.max);
        let proj = Projector::new(&cam, rect);
        let map = &m.map;
        // A linha da peça mais virada para a câmera.
        let forward = proj.basis.forward;
        let facing = |ia: usize| {
            let (s, c) = (map.a_at(ia) + 90.0).to_radians().sin_cos();
            dot(Vec3::new(0.0, c, s), forward)
        };
        let ia = (0..map.na)
            .min_by(|a, b| facing(*a).total_cmp(&facing(*b)))
            .unwrap_or(0);
        let ix = map.nx / 2;
        let h = map.height(ix, ia);
        let angle = (map.a_at(ia) + 90.0).to_radians();
        let r = map.radius + h;
        let at = Vec3::new(map.x_at(ix), r * angle.cos(), r * angle.sin());
        let screen = proj.project(at).unwrap_or_else(|| panic!("fora da vista"));
        let probe = surface_probe(&proj, map, screen, 0.0, false)
            .unwrap_or_else(|| panic!("deveria achar a peça"));
        assert!((probe.x - map.x_at(ix)).abs() < 2.0, "{probe:?}");
        assert!((probe.height - h).abs() < 0.3, "{probe:?} vs {h}");
        assert!(surface_probe(&proj, map, pos2(2.0, 2.0), 0.0, false).is_none());
    }

    /// Modelo de depósito da espiral com o fechamento uniforme.
    fn deposit_scene() -> (Params, Toolpath, tfm_core::deposit::DepositModel) {
        let p = Params {
            fechamento_uniforme: true,
            ..Params::default()
        };
        let lines = tfm_core::gcode::generate(&p).unwrap_or_else(|e| panic!("{e}"));
        let tp = tfm_core::toolpath::build_toolpath(&lines, &p);
        let q = tfm_core::deposit::flow_rate(&tp, &p);
        let m = tfm_core::deposit::simulate(&tp, &p, q)
            .unwrap_or_else(|| panic!("a espiral deveria ter mapa"));
        (p, tp, m)
    }

    /// A cor mostra a altura relativa à média: frio abaixo, verde no alvo,
    /// quente onde sobe mais.
    #[test]
    fn heat_color_runs_from_low_to_high() {
        let [r0, _, b0, _] = heat_color(0.1).to_array();
        assert!(b0 > r0, "abaixo do alvo deveria ser azul");
        let [r1, g1, _, _] = heat_color(1.0).to_array();
        assert!(g1 > r1, "no alvo deveria ser verde");
        let [r2, g2, _, _] = heat_color(3.0).to_array();
        assert!(r2 > g2 * 2, "acima do alvo deveria ser vermelho");
        assert_eq!(heat_color(0.1), heat_color(0.2), "satura embaixo");
        assert_eq!(heat_color(3.0), heat_color(4.0), "satura em cima");
    }

    /// A superfície real sai como malha, só do lado virado para a câmera.
    #[test]
    fn surface_mesh_draws_only_the_visible_side() {
        let (_, tp, m) = deposit_scene();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0));
        let mut cam = Camera::default();
        cam.fit(tp.min, tp.max);
        let proj = Projector::new(&cam, rect);
        let mesh = surface_mesh(&proj, &m.map, m.report.mean_mm, 0.0, false);
        assert!(!mesh.is_empty(), "a superfície deveria aparecer");
        let quads = mesh.indices.len() / 6;
        let total = m.map.nx * m.map.na;
        assert!(
            quads > total / 5 && quads < total * 3 / 5,
            "{quads} quadriláteros de {total}"
        );
        // Sem material não há o que desenhar.
        let vazio = m.map.empty_like();
        let mesh = surface_mesh(&proj, &vazio, 1.0, 0.0, false);
        assert!(mesh.indices.is_empty());
    }

    /// Durante a simulação o mapa cresce com o progresso; voltando para trás
    /// ele recomeça e dá o mesmo resultado.
    #[test]
    fn partial_map_follows_the_simulation() {
        let (_, _, m) = deposit_scene();
        let soma = |map: &tfm_core::deposit::HeightMap| map.heights.iter().sum::<f32>();
        let mut state = ViewState::default();
        let meio = soma(state.partial_map(7, &m, m.points / 2));
        let fim = soma(state.partial_map(7, &m, m.points));
        assert!(meio > 0.2 * fim && meio < 0.8 * fim, "{meio} de {fim}");
        assert!((fim / soma(&m.map) - 1.0).abs() < 1e-3);
        let de_novo = soma(state.partial_map(7, &m, m.points / 2));
        assert!((de_novo / meio - 1.0).abs() < 1e-3, "{de_novo} vs {meio}");
        // Resultado novo não herda o mapa do anterior.
        let zero = soma(state.partial_map(8, &m, 0));
        assert_eq!(zero, 0.0);
    }

    /// Os números do depósito aparecem sobre a vista, com a comparação quando
    /// a compensação está ligada.
    #[test]
    fn overlay_reports_the_deposit_numbers() {
        let (_, tp, mut m) = deposit_scene();
        m.base = Some(m.report);
        let palette = Palette::light();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0));
        let ctx = egui::Context::default();
        let mut output = ctx.run_pass(egui::RawInput::default(), |ctx| {
            let painter = Painter::new(ctx.clone(), egui::LayerId::background(), rect);
            overlay(&painter, rect, Some(&tp), None, Some(&m), &palette);
        });
        output.textures_delta.clear();
        let (mut circles, mut texts) = (Vec::new(), Vec::new());
        for clipped in &output.shapes {
            collect_shapes(&clipped.shape, &mut circles, &mut texts);
        }
        for want in ["Altura:", "Sobremetal", "Sem compensação:"] {
            assert!(
                texts.iter().any(|t| t.starts_with(want)),
                "{want} ausente: {texts:?}"
            );
        }
    }

    /// `look_from` reproduz os ângulos de cada vista a partir da direção em
    /// que a câmera dela fica em relação ao alvo.
    #[test]
    fn look_from_matches_presets() {
        for preset in ViewPreset::ALL {
            let mut cam = Camera::default();
            cam.preset(preset);
            let b = cam.basis();
            let dir = normalize(sub(b.eye, cam.target));
            let mut other = Camera::default();
            other.look_from(dir);
            let (yaw, pitch) = preset.angles();
            assert!(
                wrap_deg(other.yaw - yaw).abs() < 0.1,
                "{preset:?}: yaw {} vs {yaw}",
                other.yaw
            );
            assert!(
                (other.pitch - pitch).abs() < 0.1,
                "{preset:?}: pitch {} vs {pitch}",
                other.pitch
            );
        }
        // Direção exatamente vertical não tem yaw: mantém o atual.
        let mut cam = Camera::default();
        cam.preset(ViewPreset::Iso);
        cam.look_from(Vec3::new(0.0, 0.0, 1.0));
        assert!((cam.yaw + 45.0).abs() < 1e-3 && cam.pitch > 88.0, "{cam:?}");
    }

    /// A isométrica mostra três faces do cubo: Esquerda, Frente e Topo.
    #[test]
    fn iso_shows_left_front_and_top_faces() {
        let mut cam = Camera::default();
        cam.preset(ViewPreset::Iso);
        let labels: Vec<&str> = cube_visible(&cam).map(|f| f.label).collect();
        assert_eq!(labels.len(), 3, "{labels:?}");
        for l in ["ESQUERDA", "FRENTE", "TOPO"] {
            assert!(labels.contains(&l), "{l} ausente em {labels:?}");
        }
    }

    /// Clicar no centro de uma face visível pede a vista pela normal dela;
    /// fora do cubo não pede nada.
    #[test]
    fn cube_pick_face_center_returns_its_normal() {
        let mut cam = Camera::default();
        cam.preset(ViewPreset::Iso);
        let rect = Rect::from_min_size(pos2(700.0, 60.0), vec2(CUBE_SIZE, CUBE_SIZE));
        for face in cube_visible(&cam) {
            let at = cube_project(&cam, rect, face.normal);
            let dir = cube_pick(&cam, rect, at)
                .unwrap_or_else(|| panic!("face {} não foi atingida", face.label));
            assert!(
                length(sub(dir, face.normal)) < 1e-3,
                "face {}: {dir:?}",
                face.label
            );
        }
        assert!(cube_pick(&cam, rect, pos2(10.0, 10.0)).is_none());
    }

    /// Clicar perto do canto comum de três faces pede a vista diagonal.
    #[test]
    fn cube_pick_corner_snaps_to_diagonal() {
        let mut cam = Camera::default();
        cam.preset(ViewPreset::Iso);
        let rect = Rect::from_min_size(pos2(700.0, 60.0), vec2(CUBE_SIZE, CUBE_SIZE));
        // Canto comum de ESQUERDA (-X), FRENTE (-Y) e TOPO (+Z), um pouco
        // para dentro para cair dentro das três faces.
        let corner = Vec3::new(-0.95, -0.95, 0.95);
        let dir = cube_pick(&cam, rect, cube_project(&cam, rect, corner))
            .unwrap_or_else(|| panic!("canto não atingido"));
        let want = normalize(Vec3::new(-1.0, -1.0, 1.0));
        assert!(length(sub(dir, want)) < 1e-3, "{dir:?}");
    }

    /// O rótulo encolhe para caber na largura da face projetada e some
    /// quando ficaria ilegível; a face de frente para a câmera é um
    /// quadrado de lado 2·s.
    #[test]
    fn cube_labels_shrink_to_fit_the_face() {
        let size = cube_label_size(38.0, 55.0, "ESQUERDA")
            .unwrap_or_else(|| panic!("ESQUERDA deveria caber em 38 px"));
        assert!(
            size * 8.0 * 0.6 <= 38.0 * 0.9 + 1e-3 && size >= 6.5,
            "tamanho {size}"
        );
        assert_eq!(cube_label_size(60.0, 60.0, "TOPO"), Some(10.0));
        assert_eq!(cube_label_size(20.0, 55.0, "ESQUERDA"), None);

        let mut cam = Camera::default();
        cam.preset(ViewPreset::Frente);
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(96.0, 96.0));
        let frente = CUBE_FACES
            .iter()
            .find(|f| f.label == "FRENTE")
            .unwrap_or_else(|| panic!("face FRENTE"));
        let (w, h) = cube_face_span(&cam, rect, frente);
        let side = 2.0 * 48.0 / 1.8;
        assert!(
            (w - side).abs() < 0.5 && (h - side).abs() < 0.5,
            "{w} x {h}"
        );
    }

    /// O cubo fica no canto superior direito do viewport e só rotula as
    /// faces visíveis.
    #[test]
    fn view_cube_draws_visible_face_labels() {
        let palette = Palette::light();
        let viewport = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0));
        let rect = cube_rect(viewport);
        assert!(
            viewport.contains_rect(rect) && rect.right() > 700.0 && rect.top() < 100.0,
            "{rect:?}"
        );
        let mut cam = Camera::default();
        cam.preset(ViewPreset::Iso);
        let ctx = egui::Context::default();
        let mut output = ctx.run_pass(egui::RawInput::default(), |ctx| {
            let painter = Painter::new(ctx.clone(), egui::LayerId::background(), viewport);
            draw_view_cube(&painter, &cam, rect, None, &palette);
        });
        output.textures_delta.clear();
        let (mut circles, mut texts) = (Vec::new(), Vec::new());
        for clipped in &output.shapes {
            collect_shapes(&clipped.shape, &mut circles, &mut texts);
        }
        for l in ["ESQUERDA", "FRENTE", "TOPO"] {
            assert!(
                texts.iter().any(|t| t == l),
                "rótulo {l} ausente: {texts:?}"
            );
        }
        assert!(
            !texts
                .iter()
                .any(|t| t == "TRÁS" || t == "BASE" || t == "DIREITA"),
            "faces ocultas rotuladas: {texts:?}"
        );
    }

    /// Um clique na face TOPO do cubo leva a aba para a vista superior.
    #[test]
    fn clicking_cube_top_face_sets_top_view() {
        use egui::{Event, Modifiers, RawInput};

        let palette = Palette::light();
        let mut state = ViewState::default();
        state.set_preset(ViewPreset::Iso);
        let size = vec2(800.0, 600.0);
        let viewport = Rect::from_min_size(egui::Pos2::ZERO, size);
        let at = cube_project(&state.cam, cube_rect(viewport), Vec3::new(0.0, 0.0, 1.0));
        let ctx = egui::Context::default();
        let frames = [
            vec![Event::PointerMoved(at)],
            vec![Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            }],
            vec![Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
        ];
        for events in frames {
            let input = RawInput {
                screen_rect: Some(viewport),
                events,
                ..RawInput::default()
            };
            let mut output = ctx.run_pass(input, |ctx| {
                let mut ui = Ui::new(
                    ctx.clone(),
                    Id::unique("teste_cubo"),
                    egui::UiBuilder::new().max_rect(viewport),
                );
                let (response, painter) = ui.allocate_painter(size, Sense::click_and_drag());
                view_cube(&mut ui, &painter, &mut state, response.rect, &palette);
            });
            output.textures_delta.clear();
        }
        while state.animate_camera(1.0 / 60.0) {}
        assert!(
            state.cam.pitch > 88.0,
            "esperava a vista superior, veio pitch {}",
            state.cam.pitch
        );
    }

    /// Recolhe segmentos de reta (pontos, cor do traço) pintados num quadro.
    fn collect_segments(shape: &Shape, out: &mut Vec<([Pos2; 2], [u8; 4])>) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| collect_segments(s, out)),
            Shape::LineSegment { points, stroke } => {
                out.push((*points, stroke.color.to_array()));
            }
            _ => {}
        }
    }

    /// Espiral padrão com a câmera enquadrada na cena.
    fn spiral_scene() -> (Params, Toolpath, Camera, Rect) {
        let params = Params::default();
        let lines = tfm_core::gcode::generate(&params).unwrap_or_else(|e| panic!("{e}"));
        let tp = tfm_core::toolpath::build_toolpath(&lines, &params);
        let rect = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0));
        let bounds = scene_bounds(Some(&tp), Some(&params));
        let mut cam = Camera::default();
        cam.fit(bounds.min, bounds.max);
        (params, tp, cam, rect)
    }

    /// Cena da operação com a câmera enquadrada.
    fn scene_of(op: Operation) -> (Params, Bounds, Projector) {
        let mut params = Params::default();
        params.apply_operation(op);
        let lines = tfm_core::gcode::generate(&params).unwrap_or_else(|e| panic!("{e}"));
        let tp = tfm_core::toolpath::build_toolpath(&lines, &params);
        let rect = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0));
        let bounds = scene_bounds(Some(&tp), Some(&params));
        let mut cam = Camera::default();
        cam.fit(bounds.min, bounds.max);
        let proj = Projector::new(&cam, rect);
        (params, bounds, proj)
    }

    /// Roda `draw` numa passada do egui e devolve os segmentos desenhados.
    fn segments_of(rect: Rect, draw: impl Fn(&Painter)) -> Vec<([Pos2; 2], [u8; 4])> {
        let ctx = egui::Context::default();
        let mut output = ctx.run_pass(egui::RawInput::default(), |ctx| {
            draw(&Painter::new(
                ctx.clone(),
                egui::LayerId::background(),
                rect,
            ));
        });
        output.textures_delta.clear();
        let mut segments = Vec::new();
        for clipped in &output.shapes {
            collect_segments(&clipped.shape, &mut segments);
        }
        segments
    }

    /// O plano do chão fica só sob a peça deitada. Cilindro na horizontal fica
    /// entre os eixos da máquina: sobra só o triedro.
    #[test]
    fn floor_shows_only_under_parts_lying_down() {
        let palette = Palette::light();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0));
        let axes = [palette.danger, palette.success, palette.accent].map(|c| c.to_array());
        for op in Operation::ALL {
            let (params, bounds, proj) = scene_of(op);
            let floor = has_floor(Some(&params));
            let drawn = segments_of(rect, |painter| {
                draw_scene_base(painter, &proj, &bounds, floor, &palette);
            });
            let grid = drawn.iter().filter(|(_, c)| !axes.contains(c)).count();
            let lying = matches!(op, Operation::ChapaPlana | Operation::Bolacha);
            assert_eq!(grid > 0, lying, "plano em {op:?}: {grid} linhas");
            assert!(drawn.len() >= grid + 3, "triedro em {op:?}");
        }
        assert!(!has_floor(None), "sem peça ainda, sem plano");
    }

    /// A aba inteira, com a peça calculada de verdade: é o `view_ui` que decide
    /// o plano pela operação do resultado. Sem resultado ainda, nada de plano.
    #[test]
    fn view_tab_draws_the_floor_only_under_parts_lying_down() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let mut app = TfmApp::headless(dir.path());
        let muted = app.palette.muted;
        let grid = [0.25, 0.5].map(|k| muted.gamma_multiply(k).to_array());
        let grid_lines = |app: &TfmApp| {
            let ctx = egui::Context::default();
            let mut state = ViewState::default();
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))),
                ..Default::default()
            };
            let mut output = ctx.run_ui(raw, |ui| {
                view_ui(app, ui, &mut state);
            });
            output.textures_delta.clear();
            let mut segments = Vec::new();
            for clipped in &output.shapes {
                collect_segments(&clipped.shape, &mut segments);
            }
            segments.iter().filter(|(_, c)| grid.contains(c)).count()
        };
        assert_eq!(grid_lines(&app), 0, "sem peça calculada");
        for op in Operation::ALL {
            let mut params = Params::default();
            params.apply_operation(op);
            app.result = Some(crate::compute::compute_all(params, &app.cfg, 1));
            let lying = matches!(op, Operation::ChapaPlana | Operation::Bolacha);
            let lines = grid_lines(&app);
            assert_eq!(lines > 0, lying, "plano em {op:?}: {lines} linhas");
        }
    }

    /// A bolacha fica deitada na mesa, na altura da tocha, com a face para cima.
    #[test]
    fn bolacha_lies_on_the_table() {
        let palette = Palette::light();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0));
        let (params, _, proj) = scene_of(Operation::Bolacha);
        let r = params.diametro as f32 * 0.5;
        let z = params.afastamento_tocha as f32;

        let b = reference_bounds(&params);
        assert_eq!((b.min.x, b.min.y, b.min.z), (-r, -r, z));
        assert_eq!((b.max.x, b.max.y, b.max.z), (r, r, z));

        let shape = BeadShape::from_params(&params);
        let up = shape.surface.normal_at(Vec3::new(r, 0.0, z));
        assert_eq!((up.x, up.y, up.z), (0.0, 0.0, 1.0));

        let drawn = segments_of(rect, |painter| {
            draw_reference(painter, &proj, &params, Spin::Still, &palette);
        });
        for rim in [
            Vec3::new(r, 0.0, z),
            Vec3::new(-r, 0.0, z),
            Vec3::new(0.0, r, z),
            Vec3::new(0.0, -r, z),
        ] {
            let at = proj
                .project(rim)
                .unwrap_or_else(|| panic!("a borda deveria estar visível"));
            let found = drawn
                .iter()
                .any(|(pts, _)| pts.iter().any(|p| p.distance(at) < 1.0));
            assert!(found, "falta a borda da peça em {rim:?}");
        }
    }

    /// O triedro XYZ nasce na origem da máquina: nas peças que giram, o
    /// centro do eixo A, e não o canto da grade.
    #[test]
    fn axes_originate_at_machine_origin() {
        let (params, tp, cam, rect) = spiral_scene();
        let palette = Palette::light();
        let bounds = scene_bounds(Some(&tp), Some(&params));
        let proj = Projector::new(&cam, rect);
        let origin = proj
            .project(Vec3::default())
            .unwrap_or_else(|| panic!("a origem deveria estar visível"));
        let ctx = egui::Context::default();
        let mut output = ctx.run_pass(egui::RawInput::default(), |ctx| {
            let painter = Painter::new(ctx.clone(), egui::LayerId::background(), rect);
            draw_scene_base(&painter, &proj, &bounds, false, &palette);
        });
        output.textures_delta.clear();
        let mut segments = Vec::new();
        for clipped in &output.shapes {
            collect_segments(&clipped.shape, &mut segments);
        }
        for (axis, color) in [
            ("X", palette.danger),
            ("Y", palette.success),
            ("Z", palette.accent),
        ] {
            let c = color.to_array();
            let found = segments
                .iter()
                .any(|(pts, col)| *col == c && pts.iter().any(|p| p.distance(origin) < 1.0));
            assert!(found, "eixo {axis} não parte da origem {origin:?}");
        }
    }

    /// A silhueta do cilindro tem o comprimento a revestir, no sentido da
    /// solda, mesmo quando o percurso vai além (o apagamento da oscilação
    /// quadrada anda 100 mm em X).
    #[test]
    fn reference_cylinder_spans_the_coated_length() {
        let palette = Palette::light();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0));
        for (direction, x0, x1) in [
            (Direction::EsquerdaDireita, 0.0, 50.0),
            (Direction::DireitaEsquerda, -50.0, 0.0),
        ] {
            let params = Params {
                direcao_soldagem: direction,
                ..Params::default()
            };
            let mut cam = Camera::default();
            cam.fit(Vec3::new(-60.0, -30.0, -30.0), Vec3::new(160.0, 30.0, 30.0));
            let proj = Projector::new(&cam, rect);
            let want = |x: f32| {
                proj.project(Vec3::new(x, 25.0, 0.0))
                    .unwrap_or_else(|| panic!("ponto fora da vista"))
            };
            let ctx = egui::Context::default();
            let mut output = ctx.run_pass(egui::RawInput::default(), |ctx| {
                let painter = Painter::new(ctx.clone(), egui::LayerId::background(), rect);
                draw_reference(&painter, &proj, &params, Spin::Still, &palette);
            });
            output.textures_delta.clear();
            let mut segments = Vec::new();
            for clipped in &output.shapes {
                collect_segments(&clipped.shape, &mut segments);
            }
            let has = |a: Pos2, b: Pos2| {
                segments.iter().any(|(pts, _)| {
                    (pts[0].distance(a) < 1.0 && pts[1].distance(b) < 1.0)
                        || (pts[0].distance(b) < 1.0 && pts[1].distance(a) < 1.0)
                })
            };
            assert!(
                has(want(x0), want(x1)),
                "{direction:?}: geratriz de X{x0} a X{x1} ausente"
            );
            assert!(
                !has(want(0.0), want(150.0)),
                "{direction:?}: a silhueta seguiu o percurso até X150"
            );
        }
    }

    /// Percurso mínimo para o perfil do cordão: três pontos sobre o topo de
    /// um cilindro de raio 25, tocha ligada, andando em +X.
    fn bead_path(torch_on: bool) -> Toolpath {
        let pt = |x: f32| PathPoint {
            pos: Vec3::new(x, 0.0, 25.0),
            line_idx: 0,
            radius: 25.0,
            a_deg: 0.0,
            feed: 100.0,
            torch_on,
            extinguishing: false,
            rapid: false,
            layer: 0,
        };
        Toolpath {
            segments: vec![vec![pt(0.0), pt(10.0), pt(20.0)]],
            min: Vec3::new(0.0, -25.0, -25.0),
            max: Vec3::new(20.0, 25.0, 25.0),
            layer_count: 1,
        }
    }

    /// Malha, vista de topo, do cordão sobre a polilinha `xy` numa peça deitada.
    /// Cada trecho sai com 8 vértices: pé esquerdo, crista início e fim, pé
    /// esquerdo final; depois crista, pé direito e seus finais.
    fn flat_bead(xy: &[(f32, f32)]) -> Mesh {
        let points = xy
            .iter()
            .map(|&(x, y)| PathPoint {
                pos: Vec3::new(x, y, 12.0),
                line_idx: 0,
                radius: 0.0,
                a_deg: 0.0,
                feed: 100.0,
                torch_on: true,
                extinguishing: false,
                rapid: false,
                layer: 0,
            })
            .collect();
        let tp = Toolpath {
            segments: vec![points],
            min: Vec3::new(-50.0, -50.0, 12.0),
            max: Vec3::new(50.0, 50.0, 12.0),
            layer_count: 1,
        };
        let bead = BeadShape {
            width: 8.0,
            height: 1.5,
            surface: Surface::Flat,
            include_tail: false,
        };
        let rect = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0));
        let mut cam = Camera::default();
        cam.preset(ViewPreset::Topo);
        cam.target = Vec3::new(0.0, 0.0, 12.0);
        cam.distance = 300.0;
        let proj = Projector::new(&cam, rect);
        bead_mesh(&proj, &tp, xy.len(), Spin::Still, bead, &Palette::light())
    }

    /// Na curva os trechos vizinhos dividem a aresta da emenda. Sem isso o lado
    /// de fora abre em cunha, os dentes pretos na borda da bolacha.
    #[test]
    fn bead_curve_leaves_no_gap_between_segments() {
        let arc: Vec<(f32, f32)> = (0..8)
            .map(|k| {
                let (s, c) = (k as f32 * 10f32.to_radians()).sin_cos();
                (30.0 * c, 30.0 * s)
            })
            .collect();
        let mesh = flat_bead(&arc);
        let quads: Vec<&[egui::epaint::Vertex]> = mesh.vertices.chunks(8).collect();
        assert_eq!(quads.len(), 7);
        for w in quads.windows(2) {
            for (end, start, name) in [(3, 0, "pé esquerdo"), (6, 5, "pé direito")] {
                let gap = w[0][end].pos.distance(w[1][start].pos);
                assert!(gap < 0.01, "{name} abriu {gap} px na emenda");
            }
        }
    }

    /// No canto vivo cada trecho fica com a sua aresta: dividir a emenda
    /// estreitaria o cordão bem ali, como na virada da chapa.
    #[test]
    fn bead_sharp_corner_keeps_its_width() {
        let mesh = flat_bead(&[(0.0, 0.0), (20.0, 0.0), (20.0, 20.0)]);
        let v = &mesh.vertices;
        let start = v[0].pos.distance(v[5].pos);
        let end = v[3].pos.distance(v[6].pos);
        assert!((start - end).abs() < 0.01, "início {start}, canto {end}");
    }

    /// Cada trecho com tocha ligada vira duas facetas (pé esquerdo→crista e
    /// crista→pé direito); a que fica de costas para a câmera é descartada.
    #[test]
    fn bead_mesh_has_two_shaded_facets_per_welding_segment() {
        let palette = Palette::light();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0));
        let bead = BeadShape {
            width: 8.0,
            height: 1.5,
            surface: Surface::Cylinder { internal: false },
            include_tail: false,
        };
        let tp = bead_path(true);
        // Vista de topo: as duas facetas de cada trecho olham para a câmera.
        let mut cam = Camera::default();
        cam.preset(ViewPreset::Topo);
        cam.target = Vec3::new(10.0, 0.0, 25.0);
        cam.distance = 200.0;
        let proj = Projector::new(&cam, rect);
        let mesh = bead_mesh(&proj, &tp, 3, Spin::Still, bead, &palette);
        assert_eq!(
            mesh.vertices.len(),
            16,
            "2 trechos × 2 facetas × 4 vértices"
        );
        assert_eq!(mesh.indices.len(), 24, "2 triângulos por faceta");
        // Só o primeiro trecho quando a simulação parou no segundo ponto.
        let partial = bead_mesh(&proj, &tp, 2, Spin::Still, bead, &palette);
        assert_eq!(partial.vertices.len(), 8);
        // De frente (câmera em -Y) a faceta voltada para +Y fica de costas.
        let mut side = Camera::default();
        side.preset(ViewPreset::Frente);
        side.target = Vec3::new(10.0, 0.0, 25.0);
        side.distance = 200.0;
        let proj_side = Projector::new(&side, rect);
        let mesh_side = bead_mesh(&proj_side, &tp, 3, Spin::Still, bead, &palette);
        assert_eq!(mesh_side.vertices.len(), 8, "uma faceta por trecho");
        // Tocha desligada não deposita nada.
        let off = bead_mesh(&proj, &bead_path(false), 3, Spin::Still, bead, &palette);
        assert!(off.vertices.is_empty());
    }

    /// A largura desenhada é a efetiva do motor, a mesma do cálculo do passo
    /// e das bordas, e o apagamento só conta no fechamento uniforme.
    #[test]
    fn bead_shape_uses_the_engine_width() {
        let p = Params {
            fechamento_uniforme: true,
            ..Params::default()
        };
        let bead = BeadShape::from_params(&p);
        let w = tfm_core::gcode::common::overlap_metrics(&p, 0, None, None)
            .bead
            .effective_width_mm;
        assert!(
            (bead.width as f64 - w).abs() < 1e-3,
            "{} vs {w}",
            bead.width
        );
        assert!(bead.include_tail);
        assert!(!BeadShape::from_params(&Params::default()).include_tail);
        let mut plate = Params::default();
        plate.apply_operation(Operation::ChapaPlana);
        assert!(BeadShape::from_params(&plate).include_tail);
    }

    /// No fechamento uniforme ainda sai material depois do M102: o trecho de
    /// apagamento entra na malha, senão o anel desenhado ficaria aberto.
    #[test]
    fn bead_mesh_counts_the_extinguishing_stretch_when_closing() {
        let palette = Palette::light();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0));
        let mut tp = bead_path(true);
        tp.segments[0][2].torch_on = false;
        tp.segments[0][2].extinguishing = true;
        let mut cam = Camera::default();
        cam.preset(ViewPreset::Topo);
        cam.target = Vec3::new(10.0, 0.0, 25.0);
        cam.distance = 200.0;
        let proj = Projector::new(&cam, rect);
        let mut bead = BeadShape {
            width: 8.0,
            height: 1.5,
            surface: Surface::Cylinder { internal: false },
            include_tail: false,
        };
        assert_eq!(
            bead_mesh(&proj, &tp, 3, Spin::Still, bead, &palette)
                .vertices
                .len(),
            8
        );
        bead.include_tail = true;
        assert_eq!(
            bead_mesh(&proj, &tp, 3, Spin::Still, bead, &palette)
                .vertices
                .len(),
            16
        );
    }

    /// Com "Solda" ligada a trajetória vem com uma malha (o perfil 3D do
    /// cordão) em vez de pontinhos.
    #[test]
    fn draw_path_with_weld_emits_a_bead_mesh() {
        let (params, tp, cam, rect) = spiral_scene();
        let palette = Palette::light();
        let proj = Projector::new(&cam, rect);
        let mut state = ViewState {
            cam,
            ..Default::default()
        };
        state.sim.show_weld = true;
        let bead = Some(BeadShape::from_params(&params));
        let ctx = egui::Context::default();
        let mut output = ctx.run_pass(egui::RawInput::default(), |ctx| {
            let painter = Painter::new(ctx.clone(), egui::LayerId::background(), rect);
            draw_path(
                &painter,
                &proj,
                &tp,
                &mut state,
                &palette,
                None,
                None,
                5.0,
                Spin::Still,
                bead,
                false,
            );
        });
        output.textures_delta.clear();
        let mut mesh_vertices = 0usize;
        for clipped in &output.shapes {
            count_mesh_vertices(&clipped.shape, &mut mesh_vertices);
        }
        assert!(mesh_vertices > 0, "nenhuma malha do cordão foi desenhada");
    }

    fn count_mesh_vertices(shape: &Shape, out: &mut usize) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| count_mesh_vertices(s, out)),
            Shape::Mesh(m) => *out += m.vertices.len(),
            _ => {}
        }
    }

    /// A silhueta do cilindro inclui a linha de centro do eixo A, passando um
    /// pouco das duas faces.
    #[test]
    fn cylinder_reference_draws_axis_centerline() {
        let (params, tp, cam, rect) = spiral_scene();
        let palette = Palette::light();
        let proj = Projector::new(&cam, rect);
        let over = (tp.max.x - tp.min.x) * AXIS_OVERHANG;
        let a = proj
            .project(Vec3::new(tp.min.x - over, 0.0, 0.0))
            .unwrap_or_else(|| panic!("início da linha de centro fora da vista"));
        let b = proj
            .project(Vec3::new(tp.max.x + over, 0.0, 0.0))
            .unwrap_or_else(|| panic!("fim da linha de centro fora da vista"));
        let ctx = egui::Context::default();
        let mut output = ctx.run_pass(egui::RawInput::default(), |ctx| {
            let painter = Painter::new(ctx.clone(), egui::LayerId::background(), rect);
            draw_reference(&painter, &proj, &params, Spin::Still, &palette);
        });
        output.textures_delta.clear();
        let mut segments = Vec::new();
        for clipped in &output.shapes {
            collect_segments(&clipped.shape, &mut segments);
        }
        let found = segments.iter().any(|(pts, _)| {
            (pts[0].distance(a) < 1.0 && pts[1].distance(b) < 1.0)
                || (pts[0].distance(b) < 1.0 && pts[1].distance(a) < 1.0)
        });
        assert!(
            found,
            "linha de centro de {a:?} a {b:?} ausente entre {} segmentos",
            segments.len()
        );
    }

    /// Cilindro gira no eixo X, bolacha gira na mesa e só a chapa fica parada.
    #[test]
    fn spin_follows_the_operation() {
        let pt = PathPoint {
            a_deg: 30.0,
            ..bead_path(true).segments[0][0]
        };
        for op in Operation::ALL {
            let mut p = Params::default();
            p.apply_operation(op);
            let expected = match op {
                Operation::ChapaPlana => Spin::Still,
                Operation::Bolacha => Spin::Table(30.0),
                _ => Spin::Axis(30.0),
            };
            assert_eq!(Spin::of(&p, Some(&pt)), expected, "operação {op:?}");
        }
    }

    /// Bolacha padrão no sentido de rotação pedido.
    fn table_path(sentido: tfm_core::params::Rotation) -> (Params, Toolpath) {
        let mut params = Params::default();
        params.apply_operation(Operation::Bolacha);
        params.sentido_rotacao = sentido;
        let lines = tfm_core::gcode::generate(&params).unwrap_or_else(|e| panic!("{e}"));
        let tp = tfm_core::toolpath::build_toolpath(&lines, &params);
        (params, tp)
    }

    /// Na bolacha quem gira é a mesa: cada ponto do cordão chega a +X, sob a
    /// tocha, quando é a vez dele, nos dois sentidos de rotação.
    #[test]
    fn table_brings_each_point_under_the_torch() {
        use tfm_core::params::Rotation;
        for sentido in [Rotation::Horaria, Rotation::Antihoraria] {
            let (params, tp) = table_path(sentido);
            for pt in tp.points().filter(|q| q.torch_on) {
                let at = Spin::of(&params, Some(pt)).apply(pt.pos);
                assert!((at.x - pt.radius).abs() < 0.01, "{at:?} raio {}", pt.radius);
                assert!(at.y.abs() < 0.01, "{at:?} em A {}", pt.a_deg);
                assert_eq!(at.z, pt.pos.z);
            }
        }
    }

    /// A mesa gira no sentido escolhido, visto de cima, e não sai do lugar sem
    /// ângulo válido.
    #[test]
    fn table_turns_the_chosen_way() {
        let start = Vec3::new(10.0, 0.0, 12.0);
        let close = |v: Vec3, x: f32, y: f32| (v.x - x).abs() < 1e-4 && (v.y - y).abs() < 1e-4;
        let horaria = Spin::Table(90.0).apply(start);
        assert!(close(horaria, 0.0, -10.0), "{horaria:?}");
        let anti = Spin::Table(-90.0).apply(start);
        assert!(close(anti, 0.0, 10.0), "{anti:?}");
        assert_eq!(horaria.z, 12.0);
        assert_eq!(Spin::Table(f32::NAN).apply(start), start);
    }

    /// Os raios da silhueta giram com a mesa: numa peça redonda são eles que
    /// mostram o giro.
    #[test]
    fn table_spokes_turn_with_the_part() {
        let palette = Palette::light();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0));
        let (params, _, proj) = scene_of(Operation::Bolacha);
        let r = params.diametro as f32 * 0.5;
        let z = params.afastamento_tocha as f32;
        // 33° não cai num vértice do círculo: só o raio pode passar ali.
        let spin = Spin::Table(33.0);
        let drawn = segments_of(rect, |painter| {
            draw_reference(painter, &proj, &params, spin, &palette);
        });
        let at = proj
            .project(spin.apply(Vec3::new(r, 0.0, z)))
            .unwrap_or_else(|| panic!("a borda deveria estar visível"));
        let found = drawn
            .iter()
            .any(|(pts, _)| pts.iter().any(|p| p.distance(at) < 1.0));
        assert!(found, "o raio da silhueta não girou com a mesa");
    }

    /// A leitura da tocha é o X da máquina: na bolacha é o raio, não a posição
    /// do ponto na cena.
    #[test]
    fn readout_shows_the_machine_x_on_the_table() {
        let (params, tp) = table_path(tfm_core::params::Rotation::Horaria);
        let pt = tp
            .points()
            .filter(|q| q.torch_on)
            .nth(100)
            .unwrap_or_else(|| panic!("cordão curto"));
        let text = machine_readout(pt, Spin::of(&params, Some(pt)));
        let x = format!("Tocha X {:.3}", pt.radius);
        assert!(text.contains(&x), "{text} deveria conter {x}");
    }

    /// Com a simulação andando só existe um marcador: a tocha. A marca da linha
    /// selecionada gira presa à peça e pareceria uma segunda tocha, então ela
    /// só volta na pausa.
    #[test]
    fn selected_line_marker_hides_while_the_simulation_plays() {
        let (params, tp) = table_path(tfm_core::params::Rotation::Horaria);
        let palette = Palette::light();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0));
        let mut state = ViewState::default();
        state.cam.fit(tp.min, tp.max);
        let line = tp
            .points()
            .filter(|q| q.torch_on)
            .nth(10)
            .map(|q| q.line_idx);
        state.sim.play();
        state.sim.progress = 0.5;
        let cur = current_point(&tp, draw_limit(&state.sim, tp.point_count()))
            .copied()
            .unwrap_or_else(|| panic!("sem ponto atual"));
        let spin = Spin::of(&params, Some(&cur));
        let proj = Projector::new(&state.cam, rect);
        let marks = |state: &mut ViewState| {
            let ctx = egui::Context::default();
            let mut output = ctx.run_pass(egui::RawInput::default(), |ctx| {
                let painter = Painter::new(ctx.clone(), egui::LayerId::background(), rect);
                draw_path(
                    &painter, &proj, &tp, state, &palette, line, None, 5.0, spin, None, false,
                );
            });
            output.textures_delta.clear();
            let (mut circles, mut texts) = (Vec::new(), Vec::new());
            for clipped in &output.shapes {
                collect_shapes(&clipped.shape, &mut circles, &mut texts);
            }
            let count =
                |c: egui::Color32| circles.iter().filter(|(_, k)| *k == c.to_array()).count();
            (count(palette.danger), count(palette.warning))
        };
        assert_eq!(marks(&mut state), (1, 0), "andando: só a tocha");
        state.sim.pause();
        assert_eq!(
            marks(&mut state),
            (1, 1),
            "pausada: a linha selecionada volta"
        );
    }

    /// Na simulação da bolacha a tocha fica no raio X; quem anda é a mesa.
    #[test]
    fn simulated_torch_stays_on_the_x_radius_of_the_table() {
        let (params, tp) = table_path(tfm_core::params::Rotation::Horaria);
        let palette = Palette::light();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0));
        let mut state = ViewState::default();
        state.cam.fit(tp.min, tp.max);
        state.sim.play();
        state.sim.progress = 0.5;
        state.sim.pause();
        let cur = current_point(&tp, draw_limit(&state.sim, tp.point_count()))
            .copied()
            .unwrap_or_else(|| panic!("sem ponto atual"));
        assert!(cur.a_deg.abs() > 1.0, "o meio da espiral já girou a mesa");
        let spin = Spin::of(&params, Some(&cur));
        let proj = Projector::new(&state.cam, rect);
        let expected = proj
            .project(Vec3::new(cur.radius, 0.0, cur.pos.z))
            .unwrap_or_else(|| panic!("o raio deveria estar visível"));
        let ctx = egui::Context::default();
        let mut output = ctx.run_pass(egui::RawInput::default(), |ctx| {
            let painter = Painter::new(ctx.clone(), egui::LayerId::background(), rect);
            draw_path(
                &painter, &proj, &tp, &mut state, &palette, None, None, 5.0, spin, None, false,
            );
        });
        output.textures_delta.clear();
        let (mut circles, mut texts) = (Vec::new(), Vec::new());
        for clipped in &output.shapes {
            collect_shapes(&clipped.shape, &mut circles, &mut texts);
        }
        let danger = palette.danger.to_array();
        let Some((center, _)) = circles.iter().find(|(_, c)| *c == danger) else {
            panic!("marcador da tocha ausente");
        };
        assert!(
            center.distance(expected) < 1.5,
            "tocha em {center:?}, raio projetado em {expected:?}"
        );
    }

    /// Girar a peça de `-A` leva o ponto que está no ângulo `A` para o topo
    /// (sob a tocha), mantendo X e o raio.
    #[test]
    fn rotate_part_brings_current_angle_to_top() {
        // Ponto a A = 90° na convenção do toolpath: (x, r·cos 180°, r·sin 180°).
        let p = Vec3::new(12.5, -40.0, 0.0);
        let top = rotate_part(p, 90.0);
        assert!((top.x - 12.5).abs() < 1e-4, "X mudou: {top:?}");
        assert!(
            top.y.abs() < 1e-3 && (top.z - 40.0).abs() < 1e-3,
            "esperava o topo (0, 40), veio {top:?}"
        );
        // Volta inteira e ângulo zero não mexem no ponto.
        let same = rotate_part(p, 360.0);
        assert!(
            (same.y - p.y).abs() < 1e-3 && same.z.abs() < 1e-3,
            "volta inteira moveu o ponto: {same:?}"
        );
        assert_eq!(rotate_part(p, 0.0), p);
        // O raio é sempre preservado.
        let r = rotate_part(p, 123.4);
        assert!(
            ((r.y * r.y + r.z * r.z).sqrt() - 40.0).abs() < 1e-3,
            "{r:?}"
        );
        // Ângulo não finito não espalha NaN pela cena.
        assert_eq!(rotate_part(p, f32::NAN), p);
    }

    /// O ponto "atual" é o último dos `limit` pontos desenhados: ele dita a
    /// rotação da peça e a leitura da tocha.
    #[test]
    fn current_point_is_last_drawn_point() {
        let params = Params::default();
        let lines = tfm_core::gcode::generate(&params).unwrap_or_else(|e| panic!("{e}"));
        let tp = tfm_core::toolpath::build_toolpath(&lines, &params);
        let total = tp.point_count();
        assert!(total > 2, "percurso curto demais: {total}");
        let all: Vec<PathPoint> = tp.points().copied().collect();
        assert_eq!(current_point(&tp, 0), None);
        assert_eq!(current_point(&tp, 1), Some(&all[0]));
        assert_eq!(current_point(&tp, total / 2), Some(&all[total / 2 - 1]));
        assert_eq!(current_point(&tp, total), Some(&all[total - 1]));
        assert_eq!(current_point(&tp, total + 7), Some(&all[total - 1]));
    }

    /// Quantos pontos o quadro desenha: todos fora da simulação, a fração
    /// `progress` (nunca zero) durante ela, e zero sem percurso.
    #[test]
    fn draw_limit_follows_simulation_progress() {
        let mut sim = Sim::default();
        assert_eq!(draw_limit(&sim, 100), 100);
        assert_eq!(draw_limit(&sim, 0), 0);
        sim.play();
        assert_eq!(
            draw_limit(&sim, 100),
            1,
            "no início desenha ao menos um ponto"
        );
        sim.progress = 0.5;
        assert_eq!(draw_limit(&sim, 100), 50);
        sim.pause();
        assert_eq!(
            draw_limit(&sim, 100),
            50,
            "pausada no meio continua parcial"
        );
        sim.progress = 1.0;
        assert_eq!(draw_limit(&sim, 100), 100);
        assert_eq!(draw_limit(&sim, 0), 0);
    }

    /// Na peça que gira, a cena é a peça: o recuo de segurança da tocha não
    /// entra no enquadramento.
    #[test]
    fn cylindrical_scene_is_the_part_and_not_the_safe_height() {
        let tp = Toolpath {
            segments: Vec::new(),
            min: Vec3::new(0.0, -55.0, -55.0),
            max: Vec3::new(150.0, 55.0, 55.0),
            layer_count: 1,
        };
        let b = scene_bounds(Some(&tp), Some(&Params::default()));
        assert_eq!(b.min, Vec3::new(0.0, -26.5, -26.5));
        assert_eq!(b.max, Vec3::new(50.0, 26.5, 26.5));
        let mut flat = Params::default();
        flat.apply_operation(Operation::ChapaPlana);
        let f = scene_bounds(Some(&tp), Some(&flat));
        assert_eq!(
            (f.min, f.max),
            (tp.min, tp.max),
            "chapa plana segue o percurso"
        );
    }

    /// Durante a simulação da espiral a tocha fica no topo da peça: o
    /// marcador vermelho aparece onde o ponto atual cai depois de girar a
    /// peça, e a leitura mostra o X da tocha e o A da peça.
    #[test]
    fn simulated_torch_stays_on_top_of_rotating_part() {
        let params = Params::default();
        let lines = tfm_core::gcode::generate(&params).unwrap_or_else(|e| panic!("{e}"));
        let tp = tfm_core::toolpath::build_toolpath(&lines, &params);
        let total = tp.point_count();
        let palette = Palette::light();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0));
        let mut state = ViewState::default();
        state.cam.fit(tp.min, tp.max);
        state.sim.play();
        state.sim.progress = 0.5;
        state.sim.pause();
        let limit = draw_limit(&state.sim, total);
        let cur = current_point(&tp, limit)
            .copied()
            .unwrap_or_else(|| panic!("sem ponto atual"));
        assert!(cur.a_deg.abs() > 1.0, "o meio da espiral já girou a peça");
        let top = Vec3::new(cur.pos.x, 0.0, cur.radius);
        let spin = Spin::of(&params, Some(&cur));
        let proj = Projector::new(&state.cam, rect);
        let expected = proj
            .project(top)
            .unwrap_or_else(|| panic!("o topo da peça deveria estar visível"));

        let ctx = egui::Context::default();
        let mut output = ctx.run_pass(egui::RawInput::default(), |ctx| {
            let painter = Painter::new(ctx.clone(), egui::LayerId::background(), rect);
            draw_path(
                &painter, &proj, &tp, &mut state, &palette, None, None, 5.0, spin, None, false,
            );
            let readout = machine_readout(&cur, spin);
            overlay(&painter, rect, Some(&tp), Some(readout), None, &palette);
        });
        output.textures_delta.clear();

        let (mut circles, mut texts) = (Vec::new(), Vec::new());
        for clipped in &output.shapes {
            collect_shapes(&clipped.shape, &mut circles, &mut texts);
        }
        let danger = palette.danger.to_array();
        let Some((center, _)) = circles.iter().find(|(_, c)| *c == danger) else {
            panic!("marcador da tocha ausente");
        };
        assert!(
            center.distance(expected) < 1.5,
            "tocha em {center:?}, topo projetado em {expected:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t.contains("Peça A") && t.contains('°')),
            "leitura do ângulo da peça ausente: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("Tocha X")),
            "leitura da posição da tocha ausente: {texts:?}"
        );
    }
}
