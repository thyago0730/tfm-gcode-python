//! Simulação do material depositado nas peças cilíndricas.
//!
//! A vazão de pó é constante enquanto a tocha está acesa, então cada movimento
//! deposita `vazão × tempo`: onde a máquina anda mais devagar ou repassa, a
//! camada sobe mais. O tempo sai do `F` real de cada linha do programa, inclusive
//! no apagamento, em que ainda há material saindo depois do `M102`.
//!
//! O resultado é um mapa de altura `h(X, A)` na superfície desenrolada, que dá a
//! volta na peça. Cada amostra da trajetória soma um paraboloide com o raio do
//! cordão, normalizado para conservar o volume. A simulação grava as amostras em
//! ordem ([`DepositModel::stamps`]), e a aba 3D as aplica aos poucos para a
//! camada subir junto com a tocha.
//!
//! [`solve_feed_factors`] usa a mesma física para compensar em rotação: acelera
//! só onde o material acumula, nunca desacelera.

use std::f32::consts::TAU;

use crate::gcode::common::{overlap_metrics, rotary_arc_bounds};
use crate::params::{Params, WeldingMode};
use crate::quality::QualityManager;
use crate::toolpath::machine_xy;
use crate::toolpath::{PathPoint, Toolpath};

/// Teto da compensação: a rotação sobe no máximo este fator sobre o nominal.
pub const MAX_FEED_FACTOR: f64 = 2.5;
/// Menor célula do mapa (mm).
const MIN_CELL_MM: f32 = 0.5;
/// Célula do mapa usado no cálculo da compensação (mm).
const SOLVER_CELL_MM: f32 = 1.0;
/// Teto de células do mapa: peças grandes usam células maiores.
const MAX_CELLS: f32 = 60_000.0;
/// O paraboloide nunca é menor que isto, em células.
const MIN_KERNEL_CELLS: f32 = 1.5;
/// Rodadas do cálculo da compensação.
const SOLVER_ROUNDS: usize = 40;
/// Fração da correção aplicada a cada rodada (evita oscilar).
const SOLVER_GAIN: f64 = 0.5;
/// Quanto a falta de material pesa mais que o excesso: quem dita o nível de
/// usinagem é a altura mínima.
const SOLVER_UNDER_WEIGHT: f32 = 3.0;
/// Fator abaixo disto conta como rotação nominal.
const SOLVER_SNAP: f64 = 1.02;
/// Célula do cálculo por faixas em X (mm).
const BAND_CELL_MM: f64 = 0.5;
/// Rodadas do cálculo por faixas em X.
const BAND_ROUNDS: usize = 120;
/// Fração da correção por rodada no cálculo por faixas em X.
const BAND_GAIN: f64 = 0.15;
/// Quanto a falta de material pesa mais que o excesso no cálculo por faixas.
/// A borda chega ao teto de desaceleração antes de encher, e um peso alto só
/// empurra as faixas vizinhas para cima do alvo.
const BAND_UNDER_WEIGHT: f64 = 1.0;
/// Área com altura abaixo desta fração da média conta como sem material.
const UNCOVERED_FRACTION: f32 = 0.1;

/// Uma amostra de material: `volume` mm³ em torno de `(x, a_deg)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stamp {
    pub x: f32,
    pub a_deg: f32,
    pub radius: f32,
    pub volume: f32,
}

/// Altura depositada na superfície desenrolada: colunas em X, linhas em A.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeightMap {
    pub x0: f32,
    pub dx: f32,
    pub nx: usize,
    pub na: usize,
    /// `na × nx` alturas em mm, linha a linha.
    pub heights: Vec<f32>,
    /// Raio de referência da peça (mm).
    pub radius: f32,
    /// Raio do paraboloide (mm).
    pub kernel_radius: f32,
}

impl HeightMap {
    fn new(x_lo: f32, x_hi: f32, radius: f32, bead_radius: f32, min_cell: f32) -> HeightMap {
        let radius = radius.max(1e-3);
        let length = (x_hi - x_lo).max(1.0);
        let circumference = TAU * radius;
        let cell = min_cell.max((length * circumference / MAX_CELLS).sqrt());
        let nx = (length / cell).ceil() as usize + 1;
        let na = ((circumference / cell).round() as usize).max(12);
        HeightMap {
            x0: x_lo,
            dx: cell,
            nx,
            na,
            heights: vec![0.0; nx * na],
            radius,
            kernel_radius: bead_radius.max(MIN_KERNEL_CELLS * cell),
        }
    }

    /// Mesmo mapa, sem material.
    pub fn empty_like(&self) -> HeightMap {
        HeightMap {
            heights: vec![0.0; self.heights.len()],
            ..self.clone()
        }
    }

    pub fn clear(&mut self) {
        self.heights.fill(0.0);
    }

    pub fn height(&self, ix: usize, ia: usize) -> f32 {
        self.heights[ia * self.nx + ix]
    }

    /// X da coluna `ix`.
    pub fn x_at(&self, ix: usize) -> f32 {
        self.x0 + ix as f32 * self.dx
    }

    /// Ângulo (graus) da linha `ia`.
    pub fn a_at(&self, ia: usize) -> f32 {
        ia as f32 * 360.0 / self.na as f32
    }

    /// Chama `f(índice, peso)` para cada célula a menos de `r` mm de
    /// `(x, a_deg)`, dando a volta na peça.
    fn each_cell(&self, x: f32, a_deg: f32, radius: f32, r: f32, mut f: impl FnMut(usize, f32)) {
        let row_mm = TAU * radius.max(1e-3) / self.na as f32;
        let r2 = r * r;
        let ix_lo = (((x - r - self.x0) / self.dx).floor() as i64).max(0);
        let ix_hi = (((x + r - self.x0) / self.dx).ceil() as i64).min(self.nx as i64 - 1);
        let a_row = a_deg.rem_euclid(360.0) / 360.0 * self.na as f32;
        let center = a_row.round() as i64;
        let rows = ((r / row_mm).ceil() as i64).min((self.na as i64 - 1) / 2);
        for row in center - rows..=center + rows {
            let s_mm = (row as f32 - a_row) * row_mm;
            let rest = r2 - s_mm * s_mm;
            if rest <= 0.0 {
                continue;
            }
            let ia = row.rem_euclid(self.na as i64) as usize;
            for ix in ix_lo..=ix_hi {
                let dx = self.x0 + ix as f32 * self.dx - x;
                let w = rest - dx * dx;
                if w > 0.0 {
                    f(ia * self.nx + ix as usize, w / r2);
                }
            }
        }
    }

    /// Soma a amostra ao mapa, conservando o volume.
    pub fn stamp(&mut self, s: &Stamp) {
        let r = self.kernel_radius;
        let mut total = 0.0;
        self.each_cell(s.x, s.a_deg, s.radius, r, |_, w| total += w);
        if total <= 0.0 {
            return;
        }
        let row_mm = TAU * s.radius.max(1e-3) / self.na as f32;
        let k = s.volume / (total * self.dx * row_mm);
        let mut heights = std::mem::take(&mut self.heights);
        self.each_cell(s.x, s.a_deg, s.radius, r, |i, w| heights[i] += k * w);
        self.heights = heights;
    }
}

/// Números do depósito, medidos entre as posições extremas do centro do
/// cordão: fora disso a altura cai até zero na borda e não diz nada.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DepositReport {
    pub min_mm: f32,
    pub mean_mm: f32,
    pub max_mm: f32,
    /// `máxima - mínima`: o que a usinagem precisa tirar para nivelar.
    pub sobremetal_mm: f32,
    pub volume_mm3: f32,
    /// Material acima da altura mínima.
    pub excess_mm3: f32,
    /// `excess_mm3` em % do volume depositado.
    pub excess_pct: f32,
    /// % da área sem material.
    pub uncovered_pct: f32,
    /// Tempo de solda (min).
    pub weld_min: f32,
}

/// Simulação completa de um programa.
#[derive(Clone, Debug, Default)]
pub struct DepositModel {
    /// Mapa final, com todo o programa depositado.
    pub map: HeightMap,
    /// Amostras na ordem do programa.
    pub stamps: Vec<Stamp>,
    /// `point_stamps[k]`: amostras depositadas até o ponto `k` do percurso.
    pub point_stamps: Vec<u32>,
    /// Pontos do percurso.
    pub points: usize,
    /// Operação interna: o material sobe para dentro.
    pub internal: bool,
    pub report: DepositReport,
    /// O mesmo programa sem a compensação, para comparar.
    pub base: Option<DepositReport>,
}

fn deposits(pt: &PathPoint) -> bool {
    (pt.torch_on || pt.extinguishing) && !pt.rapid
}

/// Tempo (min) de `a` até `b`. `F` em unidades por minuto, com A em graus,
/// como o Mach3 interpreta um `G01` que mistura X e A.
fn move_minutes(a: &PathPoint, b: &PathPoint) -> f32 {
    if b.feed <= 0.0 {
        return 0.0;
    }
    let dx = b.pos.x - a.pos.x;
    let da = b.a_deg - a.a_deg;
    (dx * dx + da * da).sqrt() / b.feed
}

/// Velocidade (mm/min sobre a peça) e aporte térmico (kJ/mm) locais do
/// programa, no menor e no maior valor entre os trechos com a tocha ligada.
/// O aporte nominal usa a velocidade pedida; a compensação de rotação muda a
/// velocidade de cada trecho, e com ela o calor por milímetro.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LocalProcess {
    pub speed_min: f64,
    pub speed_max: f64,
    /// Aporte na velocidade máxima.
    pub heat_min: f64,
    /// Aporte na velocidade mínima.
    pub heat_max: f64,
}

/// Trechos menores que isto não dizem nada da velocidade (mm).
const MIN_SPEED_STEP_MM: f64 = 0.05;

/// Velocidade sobre a peça no movimento de `a` até `b` (mm/min). O `F` do
/// `G01` vale para a distância que o Mach3 lê, com os graus de A contando como
/// milímetros; a velocidade real é essa taxa vezes a razão entre a distância
/// sobre a peça e a lida. `None` com a tocha apagada, em `G0` ou num trecho
/// curto demais.
fn step_speed(a: &PathPoint, b: &PathPoint, mode: WeldingMode) -> Option<f64> {
    if !b.torch_on || b.rapid || b.feed.is_nan() || b.feed <= 0.0 {
        return None;
    }
    let feed = f64::from(b.feed);
    if mode == WeldingMode::ChapaPlana {
        let (dx, dy, dz) = (b.pos.x - a.pos.x, b.pos.y - a.pos.y, b.pos.z - a.pos.z);
        let dist = f64::from((dx * dx + dy * dy + dz * dz).sqrt());
        return (dist >= MIN_SPEED_STEP_MM).then_some(feed);
    }
    if mode == WeldingMode::Bolacha {
        // Distância lida pelo Mach3 (X, Y da máquina e graus de A) contra a
        // percorrida na peça, que é o deslocamento do ponto no referencial dela.
        let ((xa, ya), (xb, yb)) = (machine_xy(a), machine_xy(b));
        let da = b.a_deg - a.a_deg;
        let read = f64::from(((xb - xa).powi(2) + (yb - ya).powi(2) + da * da).sqrt());
        let chord = f64::from((b.pos.x - a.pos.x).hypot(b.pos.y - a.pos.y));
        return (read > 1e-6 && chord >= MIN_SPEED_STEP_MM).then(|| feed * chord / read);
    }
    // No cilindro o avanço é X.
    let (along, radius) = (b.pos.x - a.pos.x, b.radius);
    let da = b.a_deg - a.a_deg;
    let read = f64::from((along * along + da * da).sqrt());
    let arc = radius * da.to_radians();
    let surface = f64::from((along * along + arc * arc).sqrt());
    (read > 1e-6 && surface >= MIN_SPEED_STEP_MM).then(|| feed * surface / read)
}

/// Velocidade e aporte térmico locais; `None` sem trecho de solda.
pub fn local_process(tp: &Toolpath, p: &Params) -> Option<LocalProcess> {
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    let mut prev: Option<&PathPoint> = None;
    for pt in tp.points() {
        if let Some(v) = prev.and_then(|a| step_speed(a, pt, p.welding_mode)) {
            lo = lo.min(v);
            hi = hi.max(v);
        }
        prev = Some(pt);
    }
    if lo > hi {
        return None;
    }
    let heat = |speed: f64| {
        QualityManager::heat_input_kj_mm(
            p.tensao_arco,
            p.corrente_arco,
            speed,
            p.eficiencia_termica,
        )
    };
    Some(LocalProcess {
        speed_min: lo,
        speed_max: hi,
        heat_min: heat(hi),
        heat_max: heat(lo),
    })
}

/// Tempo de solda do percurso (min).
fn weld_minutes(tp: &Toolpath) -> f32 {
    let mut prev: Option<&PathPoint> = None;
    let mut total = 0.0;
    for pt in tp.points() {
        if let Some(a) = prev.filter(|_| deposits(pt)) {
            total += move_minutes(a, pt);
        }
        prev = Some(pt);
    }
    total
}

/// Vazão de material (mm³/min). Na espiral e no multiperfil cada cordão tem a
/// seção `espessura × passo` à velocidade nominal, então o platô fica na
/// espessura pedida. Nas oscilações o `F` de cada eixo é independente, e a
/// vazão é a que dá a espessura pedida na média da peça.
pub fn flow_rate(tp: &Toolpath, p: &Params) -> f64 {
    let espessura = p.espessura_camada.max(0.0);
    match p.welding_mode {
        WeldingMode::Espiral | WeldingMode::EspiralMultiperfil => {
            espessura * overlap_metrics(p, 0, None, None).step_over_mm * p.taxa()
        }
        _ => {
            let minutes = weld_minutes(tp) as f64;
            if minutes <= 0.0 {
                return 0.0;
            }
            let area = std::f64::consts::PI * p.diametro * p.comprimento_revestir;
            area * espessura * p.num_camadas.max(1) as f64 / minutes
        }
    }
}

/// Simula o programa com a vazão `q` (mm³/min). `None` em chapa plana e
/// bolacha, que não dão a volta na peça, ou sem material.
pub fn simulate(tp: &Toolpath, p: &Params, q: f64) -> Option<DepositModel> {
    if matches!(
        p.welding_mode,
        WeldingMode::ChapaPlana | WeldingMode::Bolacha
    ) {
        return None;
    }
    let bead_radius =
        (overlap_metrics(p, 0, None, None).bead.effective_width_mm as f32 / 2.0).max(0.1);

    // Faixa do centro do cordão e raio da primeira camada.
    let (mut lo, mut hi, mut radius) = (f32::MAX, f32::MIN, None);
    let mut prev: Option<&PathPoint> = None;
    for pt in tp.points() {
        if let Some(a) = prev.filter(|_| deposits(pt)) {
            lo = lo.min(a.pos.x).min(pt.pos.x);
            hi = hi.max(a.pos.x).max(pt.pos.x);
            radius = radius.or(Some(pt.radius));
        }
        prev = Some(pt);
    }
    let radius = radius?;

    let margin = bead_radius + 1.0;
    let mut map = HeightMap::new(lo - margin, hi + margin, radius, bead_radius, MIN_CELL_MM);
    let spacing = map.dx;
    let mut stamps = Vec::new();
    let mut point_stamps = Vec::with_capacity(tp.point_count() + 1);
    point_stamps.push(0);
    let mut weld_min = 0.0;
    let mut prev: Option<&PathPoint> = None;
    for pt in tp.points() {
        if let Some(a) = prev.filter(|_| deposits(pt)) {
            let minutes = move_minutes(a, pt);
            weld_min += minutes;
            let da = pt.a_deg - a.a_deg;
            let dx = pt.pos.x - a.pos.x;
            let arc = pt.radius * da.to_radians();
            let surface = (dx * dx + arc * arc).sqrt();
            let pieces = ((surface / spacing).ceil() as usize).max(1);
            let volume = q as f32 * minutes / pieces as f32;
            for j in 0..pieces {
                let t = (j as f32 + 0.5) / pieces as f32;
                stamps.push(Stamp {
                    x: a.pos.x + dx * t,
                    a_deg: a.a_deg + da * t,
                    radius: pt.radius,
                    volume,
                });
            }
        }
        prev = Some(pt);
        point_stamps.push(stamps.len() as u32);
    }
    for s in &stamps {
        map.stamp(s);
    }
    // Os números medem a peça: o percurso pode passar dela no apagamento.
    let bounds = rotary_arc_bounds(p);
    let (piece_lo, piece_hi) = (
        bounds.x_start.min(bounds.x_end) as f32,
        bounds.x_start.max(bounds.x_end) as f32,
    );
    let report = report(&map, lo.max(piece_lo), hi.min(piece_hi), weld_min);
    Some(DepositModel {
        map,
        stamps,
        point_stamps,
        points: tp.point_count(),
        internal: p.is_internal(),
        report,
        base: None,
    })
}

fn report(map: &HeightMap, lo: f32, hi: f32, weld_min: f32) -> DepositReport {
    let row_mm = TAU * map.radius / map.na as f32;
    let cell_area = map.dx * row_mm;
    let column = |x: f32| (((x - map.x0) / map.dx).round() as i64).clamp(0, map.nx as i64 - 1);
    let (ix0, ix1) = (column(lo) as usize, column(hi) as usize);
    let (mut min, mut max, mut sum, mut cells) = (f32::MAX, 0.0_f32, 0.0_f64, 0usize);
    for ia in 0..map.na {
        for ix in ix0..=ix1 {
            let h = map.height(ix, ia);
            min = min.min(h);
            max = max.max(h);
            sum += h as f64;
            cells += 1;
        }
    }
    if cells == 0 {
        return DepositReport::default();
    }
    let mean = (sum / cells as f64) as f32;
    let low = mean * UNCOVERED_FRACTION;
    let mut uncovered = 0usize;
    for ia in 0..map.na {
        for ix in ix0..=ix1 {
            if map.height(ix, ia) < low {
                uncovered += 1;
            }
        }
    }
    let excess = ((sum - min as f64 * cells as f64) * cell_area as f64) as f32;
    let volume = (map.heights.iter().map(|&h| h as f64).sum::<f64>() * cell_area as f64) as f32;
    DepositReport {
        min_mm: min,
        mean_mm: mean,
        max_mm: max,
        sobremetal_mm: max - min,
        volume_mm3: volume,
        excess_mm3: excess,
        excess_pct: if volume > 0.0 {
            excess / volume * 100.0
        } else {
            0.0
        },
        uncovered_pct: uncovered as f32 / cells as f32 * 100.0,
        weld_min,
    }
}

/// Um trecho curto da trajetória, todos com o mesmo tempo à rotação nominal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathStep {
    pub x: f64,
    pub a_deg: f64,
}

/// Quanto acelerar a rotação em cada trecho para a altura ficar o mais perto
/// possível do platô nominal: fator em `1.0..=MAX_FEED_FACTOR`. Só acelera,
/// porque mais velocidade é menos material e menos tempo.
///
/// Olhar só a crista do cordão abriria vales nas pontas, e é a altura mínima
/// que dita o nível de usinagem. Por isso a conta usa o resíduo da área inteira
/// entre as bordas: cada trecho é ajustado pela diferença média sob ele, e a
/// falta de material pesa mais que o excesso. O alvo é o platô de uma hélice
/// sem fim, `volume do trecho / (arco do trecho × passo)`, que não depende do
/// comprimento da peça.
pub fn solve_feed_factors(
    steps: &[PathStep],
    radius: f64,
    bead_radius: f64,
    passo: f64,
) -> Vec<f64> {
    let n = steps.len();
    if n < 4 || passo <= 0.0 {
        return vec![1.0; n];
    }
    let (radius, bead_radius) = (radius as f32, bead_radius as f32);
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for s in steps {
        lo = lo.min(s.x as f32);
        hi = hi.max(s.x as f32);
    }
    let margin = bead_radius + 1.0;
    let mut map = HeightMap::new(
        lo - margin,
        hi + margin,
        radius,
        bead_radius,
        SOLVER_CELL_MM,
    );
    let arc = radius * ((steps[1].a_deg - steps[0].a_deg) as f32).to_radians();
    let target = 1.0 / (arc * passo as f32);
    let column = |x: f32| ((x - map.x0) / map.dx).round() as usize;
    let (ix0, ix1) = (column(lo), column(hi));

    // `volumes[i]` = fração do material nominal que o trecho deposita.
    let mut volumes = vec![1.0_f64; n];
    let mut residual = vec![0.0_f32; map.heights.len()];
    let floor = 1.0 / MAX_FEED_FACTOR;
    for _ in 0..SOLVER_ROUNDS {
        map.clear();
        for (s, v) in steps.iter().zip(&volumes) {
            map.stamp(&Stamp {
                x: s.x as f32,
                a_deg: s.a_deg as f32,
                radius,
                volume: *v as f32,
            });
        }
        for (r, h) in residual.iter_mut().zip(&map.heights) {
            let diff = h - target;
            *r = if diff < 0.0 {
                diff * SOLVER_UNDER_WEIGHT
            } else {
                diff
            };
        }
        let r = map.kernel_radius;
        let nx = map.nx;
        for (s, v) in steps.iter().zip(volumes.iter_mut()) {
            let (mut sum, mut total) = (0.0, 0.0);
            map.each_cell(s.x as f32, s.a_deg as f32, radius, r, |i, w| {
                if (ix0..=ix1).contains(&(i % nx)) {
                    sum += residual[i] * w;
                    total += w;
                }
            });
            if total > 0.0 {
                let correction = SOLVER_GAIN * (sum / total / target) as f64;
                *v = (*v * (1.0 - correction)).clamp(floor, 1.0);
            }
        }
        volumes = smooth(&volumes);
    }
    volumes
        .iter()
        .map(|v| {
            let factor = 1.0 / v;
            if factor < SOLVER_SNAP {
                1.0
            } else {
                factor
            }
        })
        .collect()
}

/// Média móvel de 5 trechos: a rotação não muda aos saltos.
fn smooth(values: &[f64]) -> Vec<f64> {
    let n = values.len() as i64;
    (0..n)
        .map(|i| {
            let lo = (i - 2).max(0) as usize;
            let hi = (i + 2).min(n - 1) as usize;
            values[lo..=hi].iter().sum::<f64>() / (hi - lo + 1) as f64
        })
        .collect()
}

/// Material que um grupo de trechos deposita ao longo de X, em `(x, tempo)`.
/// Vale para peças em que cada anel é igual ao longo da volta: a altura só
/// depende de X, e a conta cabe em uma dimensão.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BandGroup {
    pub samples: Vec<(f64, f64)>,
}

/// Quanto desacelerar cada grupo para encher as pontas até o platô: fator de
/// tempo em `1.0..=MAX_FEED_FACTOR`. Só desacelera. `fixed` é o material que
/// não muda, e `x_lo..x_hi` é a faixa do centro do cordão. O alvo é a altura do
/// terço do meio da faixa, sem compensação.
pub fn solve_band_factors(
    groups: &[BandGroup],
    fixed: &BandGroup,
    x_lo: f64,
    x_hi: f64,
    bead_radius: f64,
) -> Vec<f64> {
    let r = bead_radius.max(BAND_CELL_MM);
    let columns = (((x_hi - x_lo) + 2.0 * r) / BAND_CELL_MM).ceil() as usize + 1;
    let x_of = |ix: usize| x_lo - r + ix as f64 * BAND_CELL_MM;
    let profile = |g: &BandGroup| -> Vec<f64> {
        let mut out = vec![0.0; columns];
        for &(x, time) in &g.samples {
            let raw: Vec<f64> = (0..columns)
                .map(|ix| {
                    let u = (x_of(ix) - x) / r;
                    (1.0 - u * u).max(0.0).powf(1.5)
                })
                .collect();
            let total: f64 = raw.iter().sum();
            if total > 0.0 {
                for (o, k) in out.iter_mut().zip(&raw) {
                    *o += time * k / total;
                }
            }
        }
        out
    };
    let shapes: Vec<Vec<f64>> = groups.iter().map(profile).collect();
    let base = profile(fixed);
    let height = |factors: &[f64]| -> Vec<f64> {
        let mut h = base.clone();
        for (shape, f) in shapes.iter().zip(factors) {
            for (a, b) in h.iter_mut().zip(shape) {
                *a += b * f;
            }
        }
        h
    };
    let inside = |ix: usize| (x_lo..=x_hi).contains(&x_of(ix));

    let mut factors = vec![1.0; groups.len()];
    let start = height(&factors);
    let mut middle: Vec<f64> = (0..columns)
        .filter(|&ix| {
            let x = x_of(ix);
            let third = (x_hi - x_lo) / 3.0;
            x >= x_lo + third && x <= x_hi - third
        })
        .map(|ix| start[ix])
        .collect();
    middle.sort_by(f64::total_cmp);
    let Some(&target) = middle.get(middle.len() / 2).filter(|t| **t > 0.0) else {
        return factors;
    };
    for _ in 0..BAND_ROUNDS {
        let h = height(&factors);
        for (shape, f) in shapes.iter().zip(factors.iter_mut()) {
            let (mut sum, mut total) = (0.0, 0.0);
            for ix in (0..columns).filter(|&ix| inside(ix)) {
                let diff = h[ix] - target;
                let weighted = if diff < 0.0 {
                    diff * BAND_UNDER_WEIGHT
                } else {
                    diff
                };
                sum += weighted * shape[ix];
                total += shape[ix];
            }
            if total > 0.0 {
                let correction = BAND_GAIN * sum / total / target;
                *f = (*f * (1.0 - correction)).clamp(1.0, MAX_FEED_FACTOR);
            }
        }
    }
    for f in &mut factors {
        if *f < SOLVER_SNAP {
            *f = 1.0;
        }
    }
    factors
}
