//! Toolpath 3D e séries para gráficos, lidos de volta do G-code gerado.
//!
//! Porta de `_parse_gcode_points` (`src/app/TFM_GCODE.py`) e das rotinas de
//! `src/app/ui/charts.py` (`update_temporal`, `update_oscillation`,
//! `update_statistics`). O G-code é a fonte da verdade: a interface nunca
//! recebe geometria do gerador, só o texto, exatamente como no Python.
//!
//! São dois leitores distintos de propósito, porque o Python tem dois:
//!
//! * [`build_toolpath`] é o simulador 3D. Lê palavra a palavra, descarta
//!   comentários (linhas inteiras e trechos inline), `%`, linhas `M*` (exceto
//!   `M101`/`M102`, que ligam e desligam a tocha) e qualquer linha com `G53`.
//!   Só `G0`/`G00` e `G1`/`G01` geram pontos.
//! * As séries ([`temporal_series`], [`oscillation_series`],
//!   [`step_distances`], [`count_motions`]) reproduzem as regexes
//!   `X([-+]?\d*\.?\d+)` e `\bG(?:0|00|1|01|2|02|3|03)\b` aplicadas à linha
//!   inteira: comentários não são removidos e arcos (`G2`/`G3`) contam como
//!   movimento. É assim que os gráficos do Python se comportam, e os gráficos
//!   portados precisam mostrar o mesmo.
//!
//! No lugar dos marcadores `NaN` que o Python intercala na lista de pontos, o
//! percurso vem em [`Toolpath::segments`]: cada quebra abre um `Vec` novo.
//! Os cálculos são feitos em `f64`, na mesma ordem do Python, e só a saída é
//! reduzida a `f32` (o formato que a renderização consome).

use std::f64::consts::FRAC_PI_2;

use crate::params::{Params, WeldingMode};

/// Resolução da interpolação angular em `G1`: um ponto a cada 5°.
const STEP_DEG: f64 = 5.0;
/// Mudança de raio (mm) a partir da qual um `G1` abre um trecho novo.
const RADIUS_EPS: f64 = 0.001;
/// Prefixo do comentário que o gerador escreve no início de cada camada.
const LAYER_MARKER: &str = "(--- CAMADA";

/// Vetor 3D simples, compartilhado com a interface.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    /// Constrói o vetor a partir dos componentes.
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Vec3 { x, y, z }
    }

    /// Mínimo componente a componente (canto inferior da caixa envolvente).
    pub fn min(self, other: Vec3) -> Vec3 {
        Vec3::new(
            self.x.min(other.x),
            self.y.min(other.y),
            self.z.min(other.z),
        )
    }

    /// Máximo componente a componente (canto superior da caixa envolvente).
    pub fn max(self, other: Vec3) -> Vec3 {
        Vec3::new(
            self.x.max(other.x),
            self.y.max(other.y),
            self.z.max(other.z),
        )
    }
}

/// Um ponto do percurso: posição 3D já no espaço da cena mais o estado da
/// máquina na linha que o gerou.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathPoint {
    /// Posição 3D. Cilíndrico: `(x, r·cos(a + 90°), r·sin(a + 90°))`; chapa
    /// plana: `(x, y, z)` da máquina; bolacha: `(z, r·cos(a + 90°), r·sin(a + 90°))`
    /// com `r` igual ao X da máquina.
    pub pos: Vec3,
    /// Índice (base 0) da linha do G-code de origem; pontos interpolados
    /// apontam para a linha do `G1` que os gerou.
    pub line_idx: usize,
    /// Raio de plotagem em mm (0.0 na chapa plana).
    pub radius: f32,
    /// Ângulo A em graus, acumulado como no G-code.
    pub a_deg: f32,
    /// Último `F` visto até esta linha (0.0 antes do primeiro).
    pub feed: f32,
    /// Estado da tocha na linha: `M101` liga, `M102` desliga.
    pub torch_on: bool,
    /// `true` para pontos de `G0`/`G00`.
    pub rapid: bool,
    /// Camada: quantos `(--- CAMADA` apareceram antes da linha (0 antes do
    /// primeiro marcador).
    pub layer: usize,
}

/// Percurso completo, pronto para a renderização.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Toolpath {
    /// Trechos contínuos. Cilíndrico: quebra em `G0` e quando o raio muda mais
    /// de 0.001 mm (camada ou aproximação); bolacha: só em `G0`; chapa plana:
    /// nunca, porque o Python não insere `NaN` nesse modo (use
    /// [`PathPoint::rapid`] para distinguir deslocamentos de cordões).
    pub segments: Vec<Vec<PathPoint>>,
    /// Canto mínimo da caixa envolvente (`Vec3::default()` se vazio).
    pub min: Vec3,
    /// Canto máximo da caixa envolvente (`Vec3::default()` se vazio).
    pub max: Vec3,
    /// Número de marcadores `(--- CAMADA` (mínimo 1 se houver algum ponto).
    pub layer_count: usize,
}

impl Toolpath {
    /// `true` quando não há nenhum ponto.
    pub fn is_empty(&self) -> bool {
        self.segments.iter().all(Vec::is_empty)
    }

    /// Total de pontos em todos os trechos.
    pub fn point_count(&self) -> usize {
        self.segments.iter().map(Vec::len).sum()
    }

    /// Todos os pontos, na ordem do G-code.
    pub fn points(&self) -> impl Iterator<Item = &PathPoint> + '_ {
        self.segments.iter().flatten()
    }
}

/// Séries X, Z e A por movimento, para o gráfico "Análise Temporal".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Series {
    /// Posição X (mm) a cada movimento.
    pub x: Vec<f64>,
    /// Posição Z (mm) a cada movimento.
    pub z: Vec<f64>,
    /// Ângulo A (graus) a cada movimento.
    pub a: Vec<f64>,
}

impl Series {
    /// Número de movimentos (as três séries têm sempre o mesmo tamanho).
    pub fn len(&self) -> usize {
        self.x.len()
    }

    /// `true` quando nenhum movimento foi encontrado.
    pub fn is_empty(&self) -> bool {
        self.x.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Simulador 3D (_parse_gcode_points)
// ---------------------------------------------------------------------------

/// Mapeamento máquina -> cena, escolhido pelo `welding_mode`.
#[derive(Clone, Copy, PartialEq)]
enum Mapping {
    /// Peça cilíndrica: todo modo que não seja chapa plana nem bolacha.
    Cylindrical,
    /// `chapa_plana`: cartesiano direto.
    Flat,
    /// `bolacha`: eixo = Z da máquina, raio = X da máquina.
    Bolacha,
}

/// Estado modal da máquina: última coordenada vista em cada eixo.
#[derive(Clone, Copy, Default)]
struct Machine {
    x: f64,
    y: f64,
    z: f64,
    a: f64,
    f: f64,
}

/// O que uma linha de movimento tem em comum com todos os pontos que gera.
#[derive(Clone, Copy)]
struct Frame {
    line_idx: usize,
    feed: f64,
    torch_on: bool,
    rapid: bool,
    layer: usize,
}

impl Frame {
    fn point(&self, pos: Vec3, radius: f64, a_deg: f64) -> PathPoint {
        PathPoint {
            pos,
            line_idx: self.line_idx,
            radius: radius as f32,
            a_deg: a_deg as f32,
            feed: self.feed as f32,
            torch_on: self.torch_on,
            rapid: self.rapid,
            layer: self.layer,
        }
    }
}

/// Acumula pontos em trechos, no lugar dos marcadores `NaN` do Python.
#[derive(Default)]
struct Builder {
    segments: Vec<Vec<PathPoint>>,
    current: Vec<PathPoint>,
    /// Raio do último ponto emitido; `None` logo após uma quebra, que é o que
    /// o Python testa com `isnan(points[-1][5])`.
    last_radius: Option<f64>,
    total: usize,
    min: Vec3,
    max: Vec3,
}

impl Builder {
    /// `len(points) > 0` do Python.
    fn has_points(&self) -> bool {
        self.total > 0
    }

    /// Fecha o trecho atual (o `NaN` do Python).
    fn split(&mut self) {
        if !self.current.is_empty() {
            self.segments.push(std::mem::take(&mut self.current));
        }
        self.last_radius = None;
    }

    /// Emite um ponto; `radius` em `f64` para a comparação de 0.001 mm não
    /// sofrer o arredondamento para `f32`.
    fn push(&mut self, point: PathPoint, radius: f64) {
        if self.total == 0 {
            self.min = point.pos;
            self.max = point.pos;
        } else {
            self.min = self.min.min(point.pos);
            self.max = self.max.max(point.pos);
        }
        self.total += 1;
        self.last_radius = Some(radius);
        self.current.push(point);
    }

    fn finish(mut self, layer_markers: usize) -> Toolpath {
        self.split();
        let layer_count = if self.total > 0 {
            layer_markers.max(1)
        } else {
            layer_markers
        };
        Toolpath {
            segments: self.segments,
            min: self.min,
            max: self.max,
            layer_count,
        }
    }
}

fn vec3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x as f32, y as f32, z as f32)
}

/// Ponto numa superfície de revolução: o eixo da peça é o X da cena e o
/// ângulo ganha 90° de fase visual (`rotary_visual_phase_rad`), como no Python.
fn revolve(axis: f64, radius: f64, a_deg: f64) -> Vec3 {
    let rad = a_deg.to_radians() + FRAC_PI_2;
    vec3(axis, radius * rad.cos(), radius * rad.sin())
}

/// `float(texto)` do Python dentro de `try/except: pass`: `None` em vez de erro.
fn parse_number(text: &str) -> Option<f64> {
    text.parse().ok()
}

/// Interpolação angular do Python: `int(|Δa| / 5)` passos e um ponto em
/// `t = k / passos` para `k` em `1..passos` (o ponto final é emitido à parte).
/// Devolve `(t, a)` de cada intermediário; vazio quando `|Δa| <= 5`.
fn angular_steps(prev_a: f64, curr_a: f64) -> impl Iterator<Item = (f64, f64)> {
    let delta_a = curr_a - prev_a;
    let steps = if delta_a.abs() > STEP_DEG {
        (delta_a.abs() / STEP_DEG) as usize
    } else {
        0
    };
    (1..steps).map(move |k| {
        let t = k as f64 / steps as f64;
        (t, prev_a + delta_a * t)
    })
}

/// Porta de `_parse_gcode_points`: converte as linhas do G-code no percurso 3D.
/// `lines` é a lista de linhas tal como o gerador a produz (sem `\n`).
///
/// Regras, na ordem do Python: linha vazia, comentário `(` ou `%` -> ignora
/// (contando os marcadores `(--- CAMADA`); `M101`/`M102` -> tocha; outro
/// `M*` -> ignora; `G53` em qualquer posição -> ignora; comentário inline
/// removido; as palavras `G0`/`G00`/`G1`/`G01` definem o movimento e
/// `X`/`Y`/`Z`/`A`/`F` atualizam o estado modal mesmo em linhas sem
/// movimento. Palavra com número inválido é ignorada em silêncio
/// (`except: pass`).
///
/// Raio cilíndrico: `base_r + s·(|Z| - afastamento_tocha)`, com `s = -1` na
/// operação interna (e aí `base_r` vem de `min(diametro, diametro_interno)`
/// quando `diametro_interno > 0`), `s = +1` na externa.
pub fn build_toolpath(lines: &[String], p: &Params) -> Toolpath {
    let internal = p.is_internal();
    let mut d_base = p.diametro;
    if internal && p.diametro_interno > 0.0 && p.diametro_interno < d_base {
        d_base = p.diametro_interno;
    }
    let base_r = d_base / 2.0;
    let afastamento = p.afastamento_tocha;
    let radial_sign = if internal { -1.0 } else { 1.0 };
    let radius_at = |z: f64| base_r + radial_sign * (z.abs() - afastamento);
    let mapping = match p.welding_mode {
        WeldingMode::ChapaPlana => Mapping::Flat,
        WeldingMode::Bolacha => Mapping::Bolacha,
        _ => Mapping::Cylindrical,
    };

    let mut machine = Machine::default();
    let mut torch_on = false;
    let mut layer = 0usize;
    let mut out = Builder::default();

    for (line_idx, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('(') || line.starts_with('%') {
            if line.starts_with(LAYER_MARKER) {
                layer += 1;
            }
            continue;
        }
        if line.starts_with("M101") {
            torch_on = true;
            continue;
        }
        if line.starts_with("M102") {
            torch_on = false;
            continue;
        }
        if line.starts_with('M') || line.contains("G53") {
            continue;
        }
        let clean = line.find('(').map_or(line, |k| &line[..k]);

        let prev = machine;
        let mut motion: Option<bool> = None; // Some(rapid)
        for word in clean.split_whitespace() {
            match word {
                "G0" | "G00" => motion = Some(true),
                "G1" | "G01" => motion = Some(false),
                _ => {
                    let mut chars = word.chars();
                    let target = match chars.next() {
                        Some('X') => &mut machine.x,
                        Some('Z') => &mut machine.z,
                        Some('A') => &mut machine.a,
                        Some('Y') => &mut machine.y,
                        Some('F') => &mut machine.f,
                        _ => continue,
                    };
                    if let Some(v) = parse_number(chars.as_str()) {
                        *target = v;
                    }
                }
            }
        }
        let Some(rapid) = motion else { continue };
        let frame = Frame {
            line_idx,
            feed: machine.f,
            torch_on,
            rapid,
            layer,
        };
        let m = machine;

        match mapping {
            Mapping::Flat => out.push(frame.point(vec3(m.x, m.y, m.z), 0.0, m.a), 0.0),
            Mapping::Bolacha => {
                if rapid && out.has_points() {
                    out.split();
                }
                if !rapid {
                    for (t, a) in angular_steps(prev.a, m.a) {
                        let r = prev.x + (m.x - prev.x) * t;
                        let axis = prev.z + (m.z - prev.z) * t;
                        out.push(frame.point(revolve(axis, r, a), r, a), r);
                    }
                }
                out.push(frame.point(revolve(m.z, m.x, m.a), m.x, m.a), m.x);
            }
            Mapping::Cylindrical => {
                let r = radius_at(m.z);
                let z_changed = out
                    .last_radius
                    .is_some_and(|last| (r - last).abs() > RADIUS_EPS);
                if (rapid || z_changed) && out.has_points() {
                    out.split();
                }
                if !rapid && !z_changed {
                    for (t, a) in angular_steps(prev.a, m.a) {
                        let x = prev.x + (m.x - prev.x) * t;
                        let z = prev.z + (m.z - prev.z) * t;
                        let ri = radius_at(z);
                        out.push(frame.point(revolve(x, ri, a), ri, a), ri);
                    }
                }
                out.push(frame.point(revolve(m.x, r, m.a), r, m.a), r);
            }
        }
    }
    out.finish(layer)
}

// ---------------------------------------------------------------------------
// Séries para gráficos (ui/charts.py)
// ---------------------------------------------------------------------------

/// `\w` do Python em padrões `str`: letras e dígitos Unicode mais `_`.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `re.search(r"<eixo>([-+]?\d*\.?\d+)", line)`: valor do primeiro número
/// colado à letra do eixo, em qualquer posição da linha (comentários inclusos).
fn find_axis_value(line: &str, axis: char) -> Option<f64> {
    let bytes = line.as_bytes();
    for (start, c) in line.char_indices() {
        if c != axis {
            continue;
        }
        let number_start = start + axis.len_utf8();
        if let Some(end) = match_number(bytes, number_start) {
            if let Some(v) = parse_number(&line[number_start..end]) {
                return Some(v);
            }
        }
    }
    None
}

/// Fim do trecho casado por `[-+]?\d*\.?\d+` a partir de `start`, com o mesmo
/// retrocesso da regex: `12.5` -> `12.5`, `12.` -> `12`, `.5` -> `.5`, `-` -> nada.
fn match_number(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    if matches!(bytes.get(i), Some(b'+') | Some(b'-')) {
        i += 1;
    }
    let int_start = i;
    while bytes.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    let int_digits = i - int_start;
    if bytes.get(i) == Some(&b'.') {
        let mut j = i + 1;
        while bytes.get(j).is_some_and(u8::is_ascii_digit) {
            j += 1;
        }
        if j > i + 1 {
            return Some(j);
        }
    }
    (int_digits > 0).then_some(i)
}

/// `re.search(r"\bG(?:0|00|1|01|2|02|3|03)\b", line)`: há um `G` precedido de
/// não-palavra, seguido de `0`..`3` ou `00`..`03` e de uma fronteira de palavra.
fn is_motion_line(line: &str) -> bool {
    let mut prev: Option<char> = None;
    for (i, c) in line.char_indices() {
        if c == 'G' && !prev.is_some_and(is_word) && motion_code_follows(&line[i + 1..]) {
            return true;
        }
        prev = Some(c);
    }
    false
}

/// Alternativas da regex na ordem em que ela as tenta, cada uma seguida de `\b`.
fn motion_code_follows(rest: &str) -> bool {
    let mut chars = rest.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !matches!(first, '0'..='3') {
        return false;
    }
    let second = chars.next();
    // Um dígito ("0", "1", "2", "3") e fronteira.
    if !second.is_some_and(is_word) {
        return true;
    }
    // Dois dígitos ("00".."03") e fronteira.
    first == '0'
        && second.is_some_and(|c| matches!(c, '0'..='3'))
        && !chars.next().is_some_and(is_word)
}

/// Porta de `update_temporal`: X, Z e A acumulados a cada linha de movimento.
/// As três séries saem sempre com o mesmo tamanho (o `pad` do Python nunca
/// tem efeito, porque os três valores são anexados juntos).
pub fn temporal_series(lines: &[String]) -> Series {
    let mut series = Series::default();
    let (mut x, mut z, mut a) = (0.0, 0.0, 0.0);
    for line in lines {
        if let Some(v) = find_axis_value(line, 'X') {
            x = v;
        }
        if let Some(v) = find_axis_value(line, 'Z') {
            z = v;
        }
        if let Some(v) = find_axis_value(line, 'A') {
            a = v;
        }
        if is_motion_line(line) {
            series.x.push(x);
            series.z.push(z);
            series.a.push(a);
        }
    }
    series
}

/// Porta de `update_oscillation`: posição X a cada linha de movimento.
pub fn oscillation_series(lines: &[String]) -> Vec<f64> {
    let mut out = Vec::new();
    let mut x = 0.0;
    for line in lines {
        if let Some(v) = find_axis_value(line, 'X') {
            x = v;
        }
        if is_motion_line(line) {
            out.push(x);
        }
    }
    out
}

/// Porta de `update_statistics`: `hypot(Δx, Δz)` por movimento, 0.0 no
/// primeiro. Movimentos anteriores à primeira leitura de X **e** de Z não
/// entram, por isso a série pode ser mais curta que [`count_motions`].
pub fn step_distances(lines: &[String]) -> Vec<f64> {
    let mut out = Vec::new();
    let (mut x, mut z): (Option<f64>, Option<f64>) = (None, None);
    let mut prev: Option<(f64, f64)> = None;
    for line in lines {
        if let Some(v) = find_axis_value(line, 'X') {
            x = Some(v);
        }
        if let Some(v) = find_axis_value(line, 'Z') {
            z = Some(v);
        }
        if !is_motion_line(line) {
            continue;
        }
        if let (Some(x), Some(z)) = (x, z) {
            out.push(match prev {
                Some((px, pz)) => (x - px).hypot(z - pz),
                None => 0.0,
            });
            prev = Some((x, z));
        }
    }
    out
}

/// Linhas que casam com `\bG(?:0|00|1|01|2|02|3|03)\b`: o eixo "passo" dos
/// gráficos. Inclui `G53 G0 ...` e arcos, como no Python.
pub fn count_motions(lines: &[String]) -> usize {
    lines.iter().filter(|l| is_motion_line(l)).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_number_follows_regex_backtracking() {
        assert_eq!(find_axis_value("G1 X12.5", 'X'), Some(12.5));
        assert_eq!(find_axis_value("X12.", 'X'), Some(12.0));
        assert_eq!(find_axis_value("X.5", 'X'), Some(0.5));
        assert_eq!(find_axis_value("X-.5", 'X'), Some(-0.5));
        assert_eq!(find_axis_value("X+3", 'X'), Some(3.0));
        assert_eq!(find_axis_value("X1e5", 'X'), Some(1.0));
        assert_eq!(find_axis_value("X- X7", 'X'), Some(7.0));
        assert_eq!(find_axis_value("(MAX5)", 'X'), Some(5.0));
        assert_eq!(find_axis_value("X 5", 'X'), None);
        assert_eq!(find_axis_value("x5", 'X'), None);
        assert_eq!(find_axis_value("", 'X'), None);
        assert_eq!(find_axis_value("ção X2", 'X'), Some(2.0));
    }

    #[test]
    fn motion_regex_word_boundaries() {
        let yes = [
            "G0", "G00 Z1", "G1", "G01", "G2", "G02", "G3", "G03", "(G1)", "N10 G1", "G0.5",
        ];
        let no = [
            "G04", "G10", "G21 G90", "XG1", "G00X", "G1_", "çG1", "g1", "G", "",
        ];
        for s in yes {
            assert!(is_motion_line(s), "{s:?} deveria ser movimento");
        }
        for s in no {
            assert!(!is_motion_line(s), "{s:?} não deveria ser movimento");
        }
    }

    #[test]
    fn angular_steps_match_python_range() {
        let a: Vec<(f64, f64)> = angular_steps(0.0, 17.0).collect();
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].0, 1.0 / 3.0);
        assert!((a[0].1 - 17.0 / 3.0).abs() < 1e-12);
        assert_eq!(angular_steps(0.0, 5.0).count(), 0);
        assert_eq!(angular_steps(0.0, 9.9).count(), 0);
        assert_eq!(angular_steps(10.0, 0.0).count(), 1);
        assert_eq!(angular_steps(0.0, f64::NAN).count(), 0);
    }
}
