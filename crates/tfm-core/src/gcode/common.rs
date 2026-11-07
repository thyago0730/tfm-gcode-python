//! Utilidades comuns a todos os modos de geração: formatação numérica igual à
//! do Python, sequências de tocha, geometria rotativa, cabeçalho e rodapé.
//!
//! Porta literal de `GCodeGenerator` (`src/app/core/generator.py`, linhas
//! 34-227 e 303-399). A ordem das operações aritméticas e os idiomas
//! `x or padrao` / `max(x, 0.0)` do Python são preservados para que a saída
//! seja idêntica byte a byte às fixtures golden.

use crate::overlap::{compute_overlap_metrics, OverlapMetrics, UniformClosing};
use crate::params::{
    Direction, MultiperfilStrategy, OscillationType, Params, ProfileShape, Rotation, WeldingMode,
};

// ---------------------------------------------------------------------------
// Formatação numérica (f-strings do Python)
// ---------------------------------------------------------------------------

/// `f"{v:.3f}"`.
pub fn f3(v: f64) -> String {
    format!("{v:.3}")
}

/// `f"{v:.2f}"`.
pub fn f2(v: f64) -> String {
    format!("{v:.2}")
}

/// `f"{v:.1f}"`.
pub fn f1(v: f64) -> String {
    format!("{v:.1}")
}

/// `f"{v:+.3f}"` (sinal sempre explícito).
pub fn f3s(v: f64) -> String {
    format!("{v:+.3}")
}

// ---------------------------------------------------------------------------
// Builtins do Python usados pelo gerador
// ---------------------------------------------------------------------------

/// `max(a, b)` do Python para floats: devolve `b` somente quando `b > a`.
pub(crate) fn py_max(a: f64, b: f64) -> f64 {
    if b > a {
        b
    } else {
        a
    }
}

/// `min(a, b)` do Python para floats: devolve `b` somente quando `b < a`.
pub(crate) fn py_min(a: f64, b: f64) -> f64 {
    if b < a {
        b
    } else {
        a
    }
}

/// `v or padrao` do Python para floats: 0.0 e -0.0 são falsos.
pub(crate) fn py_or(v: f64, default: f64) -> f64 {
    if v == 0.0 {
        default
    } else {
        v
    }
}

// ---------------------------------------------------------------------------
// Strings cruas dos enums, como o Python as imprime
// ---------------------------------------------------------------------------

/// Valor cru de `direcao_soldagem`.
pub fn direction_str(d: Direction) -> &'static str {
    match d {
        Direction::EsquerdaDireita => "esquerda_direita",
        Direction::DireitaEsquerda => "direita_esquerda",
    }
}

/// Valor cru de `sentido_rotacao`.
pub fn rotation_str(r: Rotation) -> &'static str {
    match r {
        Rotation::Horaria => "horaria",
        Rotation::Antihoraria => "antihoraria",
    }
}

/// Valor cru de `multiperfil_profile_shape`.
pub fn profile_shape_str(s: ProfileShape) -> &'static str {
    match s {
        ProfileShape::Retangular => "Retangular",
        ProfileShape::Trapezoidal => "Trapezoidal",
        ProfileShape::Triangular => "Triangular",
    }
}

/// Valor cru de `multiperfil_strategy`.
pub fn strategy_str(s: MultiperfilStrategy) -> &'static str {
    match s {
        MultiperfilStrategy::PassesSeparados => "Passes separados",
        MultiperfilStrategy::PercursoContinuo => "Percurso contínuo",
    }
}

// ---------------------------------------------------------------------------
// Tocha
// ---------------------------------------------------------------------------

/// `max(float(torch_ignite_delay_s or 0.0), 0.0)`.
pub fn torch_ignite_delay(p: &Params) -> f64 {
    py_max(py_or(p.torch_ignite_delay_s, 0.0), 0.0)
}

/// `max(float(torch_extinguish_delay_s or 0.0), 0.0)`.
pub fn torch_extinguish_delay(p: &Params) -> f64 {
    py_max(py_or(p.torch_extinguish_delay_s, 0.0), 0.0)
}

/// `_append_torch_enable_sequence`: liga a tocha e, se houver atraso de
/// ignição, aguarda com `G04`.
pub fn append_torch_enable(lines: &mut Vec<String>, p: &Params) {
    lines.push("M101 (LIGAR Output#1 - tocha)".to_string());
    let ignite_delay = torch_ignite_delay(p);
    if ignite_delay > 0.0 {
        lines.push(format!(
            "G04 P{} (Aguardar abertura da tocha)",
            f2(ignite_delay)
        ));
    }
}

/// Eixos na ordem em que o Python os percorre: `('X', 'Y', 'Z', 'A')`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    X,
    Y,
    Z,
    A,
}

impl Axis {
    const ALL: [Axis; 4] = [Axis::X, Axis::Y, Axis::Z, Axis::A];

    fn letter(self) -> char {
        match self {
            Axis::X => 'X',
            Axis::Y => 'Y',
            Axis::Z => 'Z',
            Axis::A => 'A',
        }
    }
}

/// Posição parcial da máquina: espelha os dicts Python (`{'X': .., 'A': ..}`)
/// que podem não ter todos os eixos. `None` = eixo ausente do dict.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pos {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub z: Option<f64>,
    pub a: Option<f64>,
}

impl Pos {
    /// `{'X': x, 'A': a}`.
    pub fn xa(x: f64, a: f64) -> Pos {
        Pos {
            x: Some(x),
            a: Some(a),
            ..Pos::default()
        }
    }

    /// `not pos` do Python: dict vazio (nenhum eixo presente).
    pub fn is_empty(&self) -> bool {
        self.x.is_none() && self.y.is_none() && self.z.is_none() && self.a.is_none()
    }

    fn get(&self, axis: Axis) -> Option<f64> {
        match axis {
            Axis::X => self.x,
            Axis::Y => self.y,
            Axis::Z => self.z,
            Axis::A => self.a,
        }
    }

    fn set(&mut self, axis: Axis, v: f64) {
        match axis {
            Axis::X => self.x = Some(v),
            Axis::Y => self.y = Some(v),
            Axis::Z => self.z = Some(v),
            Axis::A => self.a = Some(v),
        }
    }
}

/// `_extend_motion_target`: prolonga o movimento `start -> end` em `extra_mm`
/// ao longo da mesma direção, convertendo o eixo A em mm pelo raio rotativo.
///
/// Reproduz os idiomas do Python, inclusive `end_pos.get(axis, start) or start`:
/// um eixo final igual a 0.0 é tratado como ausente e volta ao valor inicial.
pub fn extend_motion_target(
    start: &Pos,
    end: &Pos,
    extra_mm: f64,
    rotary_radius_mm: Option<f64>,
) -> Pos {
    if start.is_empty() || end.is_empty() || extra_mm <= 1e-9 {
        return *end;
    }
    let dx = end.x.or(start.x).unwrap_or(0.0) - start.x.unwrap_or(0.0);
    let dy = end.y.or(start.y).unwrap_or(0.0) - start.y.unwrap_or(0.0);
    let dz = end.z.or(start.z).unwrap_or(0.0) - start.z.unwrap_or(0.0);
    let da_deg = end.a.or(start.a).unwrap_or(0.0) - start.a.unwrap_or(0.0);
    let mut da_mm = 0.0;
    if let Some(radius) = rotary_radius_mm {
        da_mm = da_deg.to_radians() * py_max(py_or(radius, 0.0), 0.0);
    }
    let travel_mm = (dx.powi(2) + dy.powi(2) + dz.powi(2) + da_mm.powi(2)).sqrt();
    if travel_mm <= 1e-9 {
        return *end;
    }
    let scale = 1.0 + (extra_mm / travel_mm);
    let mut extended = *end;
    for axis in Axis::ALL {
        let start_val = py_or(start.get(axis).unwrap_or(0.0), 0.0);
        let end_val = end.get(axis).map_or(start_val, |v| py_or(v, start_val));
        if end.get(axis).is_none() && (end_val - start_val).abs() <= 1e-9 {
            continue;
        }
        extended.set(axis, start_val + ((end_val - start_val) * scale));
    }
    extended
}

/// `_append_torch_disable_sequence`: desliga a tocha e, quando há atraso de
/// apagamento e um trecho válido, prolonga o último movimento para manter o
/// cordão durante o atraso. Devolve a posição final (prolongada ou não).
#[allow(clippy::too_many_arguments)]
pub fn append_torch_disable(
    lines: &mut Vec<String>,
    p: &Params,
    start: Option<&Pos>,
    end: Option<&Pos>,
    feed: Option<f64>,
    path_speed_mm_min: Option<f64>,
    rotary_radius_mm: Option<f64>,
) -> Pos {
    lines.push("M102 (DESLIGAR Output#1 - tocha)".to_string());
    let extinguish_delay = torch_extinguish_delay(p);
    let fallback = end.copied().unwrap_or_default();
    let (start, end, feed, path_speed) = match (start, end, feed, path_speed_mm_min) {
        (Some(s), Some(e), Some(f), Some(v))
            if extinguish_delay > 1e-9 && !s.is_empty() && !e.is_empty() && v > 1e-9 =>
        {
            (s, e, f, v)
        }
        _ => return fallback,
    };
    let extra_distance_mm = path_speed * (extinguish_delay / 60.0);
    let extended = extend_motion_target(start, end, extra_distance_mm, rotary_radius_mm);
    let mut move_parts = vec![format!("G01 F{}", f1(feed))];
    for axis in Axis::ALL {
        if let Some(v) = extended.get(axis) {
            move_parts.push(format!("{}{}", axis.letter(), f3(v)));
        }
    }
    lines.push(format!(
        "(APAGAMENTO DA TOCHA: manter cordao por {}s)",
        f2(extinguish_delay)
    ));
    lines.push(move_parts.join(" "));
    extended
}

// ---------------------------------------------------------------------------
// Geometria rotativa
// ---------------------------------------------------------------------------

/// `_effective_base_diameter`: na operação interna usa o menor entre
/// `diametro` e `diametro_interno` (quando este é positivo).
pub fn effective_base_diameter(p: &Params) -> f64 {
    let d = py_or(p.diametro, 0.0);
    let d_int = py_or(p.diametro_interno, 0.0);
    if p.is_internal() && d_int > 0.0 {
        return py_min(d, d_int);
    }
    d
}

/// Limites em X do arco rotativo: `(x_entry, x_exit, x_start, x_end)` do
/// Python. `x_entry`/`x_exit` só existem na operação interna (folga da face).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcBounds {
    pub x_entry: Option<f64>,
    pub x_exit: Option<f64>,
    pub x_start: f64,
    pub x_end: f64,
}

/// `_get_rotary_arc_bounds`.
pub fn rotary_arc_bounds(p: &Params) -> ArcBounds {
    let part_length = p.comprimento_revestir;
    let lead_in = p.lead_in;
    let lead_out = p.lead_out;
    if p.is_internal() {
        let face_clear = p.face_clearance();
        let (x_start, x_end) = match p.direcao_soldagem {
            Direction::DireitaEsquerda => (0.0, -part_length),
            Direction::EsquerdaDireita => (-part_length, 0.0),
        };
        return ArcBounds {
            x_entry: Some(face_clear),
            x_exit: Some(face_clear),
            x_start,
            x_end,
        };
    }
    let (x_start, x_end) = match p.direcao_soldagem {
        Direction::EsquerdaDireita => (0.0 - lead_in, part_length + lead_out),
        Direction::DireitaEsquerda => (0.0 + lead_in, -(part_length + lead_out)),
    };
    ArcBounds {
        x_entry: None,
        x_exit: None,
        x_start,
        x_end,
    }
}

/// `_get_layer_radius`: raio da superfície na camada `layer_num`, ou metade de
/// `current_d` quando informado. Piso 1e-6.
pub fn layer_radius(p: &Params, layer_num: i64, current_d: Option<f64>) -> f64 {
    if let Some(d) = current_d {
        return py_max(py_or(d, 0.0) / 2.0, 1e-6);
    }
    let base_d = effective_base_diameter(p);
    let layer_height = py_max(py_or(p.espessura_camada, 0.0), 0.0);
    let d_sign = if p.is_internal() { -1.0 } else { 1.0 };
    let d_layer = base_d + (layer_num.max(0) as f64 * 2.0 * layer_height * d_sign);
    py_max(d_layer / 2.0, 1e-6)
}

/// `_get_overlap_metrics`: métricas de sobreposição com `force_min_two_passes = True`.
pub fn overlap_metrics(
    p: &Params,
    layer_num: i64,
    current_d: Option<f64>,
    target_width: Option<f64>,
) -> OverlapMetrics {
    let radius = layer_radius(p, layer_num, current_d);
    compute_overlap_metrics(
        p.largura_cordao,
        p.sobreposicao,
        p.espessura_camada,
        Some(radius),
        target_width,
        true,
        p.bead_width_calibration_factor,
    )
}

/// `_apply_smart_layers`: na camada raiz (0) com `smart_layers` ativo, escala
/// a velocidade por `root_feed_scale / 100`.
pub fn apply_smart_layers(p: &Params, layer_num: i64, feed: f64) -> f64 {
    if p.smart_layers && layer_num == 0 {
        let scale = p.root_feed_scale / 100.0;
        return feed * scale;
    }
    feed
}

// ---------------------------------------------------------------------------
// Cabeçalho e rodapé
// ---------------------------------------------------------------------------

/// `_build_header`: cabeçalho do programa, linha a linha na ordem do Python.
/// `timestamp` já formatado (`%d/%m/%Y %H:%M:%S`).
pub fn build_header(p: &Params, timestamp: &str) -> Vec<String> {
    let mode = p.welding_mode;
    let mut header = vec![
        "%".to_string(),
        "(G-CODE GERADO PELO TFM G-CODE)".to_string(),
        format!("(Data: {timestamp})"),
    ];

    // --- RASTREABILIDADE ISO 3834 ---
    if !p.traceability_eps.is_empty() {
        header.push("( EMPRESA: TFM USINAGEM )".to_string());
        header.push(format!("( EPS: {} )", p.traceability_eps));
    }
    if !p.traceability_operator.is_empty() {
        header.push(format!("( OPERADOR: {} )", p.traceability_operator));
    }
    if !p.traceability_powder_batch.is_empty() {
        header.push(format!("( LOTE DO PO: {} )", p.traceability_powder_batch));
    }
    if !p.traceability_supplier.is_empty() {
        header.push(format!("( FORNECEDOR: {} )", p.traceability_supplier));
    }
    if !p.traceability_certificate.is_empty() {
        header.push(format!("( CERTIFICADO: {} )", p.traceability_certificate));
    }

    header.push(format!(
        "( SOFTWARE: TFM GCODE v{} )",
        super::app_version(p)
    ));
    header.push("(----------------------------------------)".to_string());
    header.push(format!("( PROCEDIMENTO: {} )", p.nome_procedimento));
    header.push(format!(
        "( MODO DE SOLDAGEM: {} )",
        mode.as_str().to_uppercase()
    ));
    header.extend([
        format!("( DIAMETRO: {} mm )", f3(p.diametro)),
        format!("( COMPRIMENTO: {} mm )", f3(p.comprimento_revestir)),
        format!("( SOBREPOSICAO: {}% )", f1(p.sobreposicao)),
        format!(
            "( VELOCIDADE DEP.: {} mm/min )",
            f1(p.velocidade_de_deposicao)
        ),
        format!("( SENTIDO: {} )", direction_str(p.direcao_soldagem)),
    ]);
    if p.is_internal() {
        header.push("( OPERACAO INTERNA: SIM )".to_string());
    }
    header.push(format!(
        "( SENTIDO ROTACAO: {} )",
        rotation_str(p.sentido_rotacao).to_uppercase()
    ));
    header.push(format!(
        "( CAMADAS: {} de {}mm )",
        p.num_camadas,
        f2(p.espessura_camada)
    ));
    header.push(format!(
        "( RESPOSTA TOCHA: LIGAR {}s / DESLIGAR {}s )",
        f2(torch_ignite_delay(p)),
        f2(torch_extinguish_delay(p))
    ));
    if mode.is_oscillation() {
        let tipo_txt = match p.oscillation_type {
            Some(OscillationType::Linear) => "LINEAR",
            Some(OscillationType::Quadrada) => "QUADRADA",
            Some(OscillationType::QuadradaContinua) => "QUADRADA CONTINUA",
            None => "ATIVADA",
        };
        header.extend([
            format!("( OSCILACAO: {tipo_txt} )"),
            format!(
                "(   - COMPRIMENTO OSC.: {} mm)",
                f2(p.oscilacao_comprimento)
            ),
            format!(
                "(   - DESLOC. ANGULAR: {}% Larg. Cordao)",
                f1(p.deslocamento_angular_perc)
            ),
        ]);
    } else if mode == WeldingMode::EspiralMultiperfil {
        // int(params.get('multiperfil_count', 1) or 1)
        let perfis = if p.multiperfil_count == 0 {
            1
        } else {
            p.multiperfil_count
        };
        header.extend([
            format!("( PERFIS: {perfis} )"),
            format!(
                "( SETOR TOTAL: {} deg )",
                f2(py_or(p.multiperfil_total_angle_deg, 360.0))
            ),
            format!(
                "( PASSO POR PERFIL: {} mm )",
                f3(py_or(p.multiperfil_pitch_mm, 0.0))
            ),
            format!("( ESTRATEGIA: {} )", strategy_str(p.multiperfil_strategy)),
            format!(
                "( PONTO A PONTO: {} )",
                if p.multiperfil_point_by_point {
                    "SIM"
                } else {
                    "NAO"
                }
            ),
            format!(
                "( FORMATO PERFIL: {} )",
                profile_shape_str(p.multiperfil_profile_shape)
            ),
            format!(
                "( ALTURA PERFIL: {} mm )",
                f3(py_or(p.multiperfil_profile_height_mm, 0.0))
            ),
            format!(
                "( LARGURA BASE: {} mm )",
                f3(py_or(p.multiperfil_profile_base_mm, 0.0))
            ),
            format!(
                "( LARGURA TOPO: {} mm )",
                f3(py_or(p.multiperfil_profile_top_mm, 0.0))
            ),
        ]);
        if p.multiperfil_end_chamfer_enabled {
            header.extend([
                "( CHANFRO PONTAS: MAXIMO )".to_string(),
                format!(
                    "( ANGULO CHANFRO: {} deg )",
                    f3(py_or(p.multiperfil_end_chamfer_angle_deg, 0.0))
                ),
            ]);
        }
    }
    header.push(format!(
        "( LEAD-IN: {} mm, LEAD-OUT: {} mm )",
        f2(p.lead_in),
        f2(p.lead_out)
    ));
    header.extend([
        "(----------------------------------------)".to_string(),
        "G21 G90 G94".to_string(),
        String::new(),
    ]);
    // Preparo da tocha: opcionalmente recua Z&Y ao zero máquina antes de ligar.
    header.push("(PREPARO DA TOCHA)".to_string());
    if p.is_internal() {
        header.push(format!("G90 G0 X{}", f3(p.face_clearance())));
    }
    if p.torch_retract_on_ignite {
        header.push("G53 G0 Z0 Y0 (Ir ao Z&Y=0 maquina)".to_string());
    }
    header.extend(["G90 G0 Y0 (Ir ao Y=0 peca)".to_string(), String::new()]);
    header
}

/// `_build_footer`: retorno ao zero máquina (Z&Y juntos, tocha desligada,
/// depois X) e fim de programa.
pub fn build_footer(p: &Params) -> Vec<String> {
    let mut footer = vec![
        String::new(),
        "M102 (DESLIGAR Output#1 - tocha)".to_string(),
        "(RETORNO AO ZERO MAQUINA)".to_string(),
        "G90".to_string(),
    ];
    if p.is_internal() {
        footer.push(format!("G90 G0 X{}", f3(p.face_clearance())));
    }
    footer.extend([
        "G53 G0 Z0 Y0 (Ir ao Z&Y=0 maquina)".to_string(),
        "G53 G0 X0 (Ir ao X=0 maquina)".to_string(),
        "M30 (FIM DO PROGRAMA)".to_string(),
        "%".to_string(),
    ]);
    footer
}

/// Separador entre camadas: `"M01" if layer_num < num_camadas - 1 else ""`.
pub fn layer_separator(p: &Params, layer_num: i64) -> String {
    if layer_num < p.num_camadas - 1 {
        "M01".to_string()
    } else {
        String::new()
    }
}

/// Comentário gravado na camada quando o fechamento uniforme está ligado:
/// voltas inteiras, passo redistribuído e o curso do centro do cordão.
pub fn fechamento_uniforme_comment(plano: &UniformClosing) -> String {
    format!(
        "(FECHAMENTO UNIFORME: {:.0} voltas, passo {} mm, centro X{} a X{})",
        plano.voltas,
        f3(plano.passo),
        f3(plano.x_start),
        f3(plano.x_end)
    )
}

/// Quanto a peça gira (graus) durante o atraso de apagamento da tocha, num
/// raio `radius` e à velocidade de trajetória `path_speed_mm_min`: o trecho
/// em que ainda sai material depois do `M102`.
pub fn extinguish_arc_deg(p: &Params, path_speed_mm_min: f64, radius: f64) -> f64 {
    let extra_mm = path_speed_mm_min * (torch_extinguish_delay(p) / 60.0);
    (extra_mm / py_max(radius, 1e-6)).to_degrees()
}

/// Converte um programa em G94 para G93 (tempo inverso). A distância de cada
/// `G01` é a que o Mach3 lê em G94 (X, Y, Z em mm e A em graus, juntos), com o
/// F modal; o F novo é `1 ÷ minutos` da linha, escrito em todo `G01`. O
/// cabeçalho `G94` vira `G93` e o programa volta a `G94` antes do `M30`.
/// Comentários, `G00`, `G04` e linhas com `G53` ficam como estão.
pub fn to_inverse_time(lines: &[String]) -> Vec<String> {
    /// Linha sem deslocamento: 1 ms, só para o G01 ter F.
    const F_SEM_MOVIMENTO: f64 = 60_000.0;
    let word = |l: &str, axis: char| -> Option<f64> {
        l.split_whitespace()
            .find_map(|w| w.strip_prefix(axis).and_then(|v| v.parse().ok()))
    };
    let (mut x, mut y, mut z, mut a, mut f) = (0.0, 0.0, 0.0, 0.0, 0.0_f64);
    let mut out = Vec::with_capacity(lines.len() + 4);
    for raw in lines {
        let l = raw.trim();
        if l.is_empty() || l.starts_with('(') || l.starts_with('%') || l.contains("G53") {
            out.push(raw.clone());
            continue;
        }
        if l.starts_with("M30") {
            out.push("G94 (VOLTA AO AVANCO POR MINUTO)".to_string());
            out.push(raw.clone());
            continue;
        }
        let (codigo, comentario) = match l.find('(') {
            Some(k) => (&l[..k], &l[k..]),
            None => (l, ""),
        };
        let tokens: Vec<&str> = codigo.split_whitespace().collect();
        if tokens.contains(&"G94") {
            let novo: Vec<&str> = tokens
                .iter()
                .map(|t| if *t == "G94" { "G93" } else { t })
                .collect();
            out.push(novo.join(" "));
            out.push("(G93: F EM TEMPO INVERSO, 1 / MINUTOS DE CADA LINHA)".to_string());
            continue;
        }
        let nx = word(codigo, 'X').unwrap_or(x);
        let ny = word(codigo, 'Y').unwrap_or(y);
        let nz = word(codigo, 'Z').unwrap_or(z);
        let na = word(codigo, 'A').unwrap_or(a);
        if let Some(nf) = word(codigo, 'F') {
            f = nf;
        }
        let g01 = tokens.iter().any(|t| *t == "G01" || *t == "G1");
        let g00 = tokens.iter().any(|t| *t == "G00" || *t == "G0");
        if g01 && f > 0.0 {
            let d =
                ((nx - x).powi(2) + (ny - y).powi(2) + (nz - z).powi(2) + (na - a).powi(2)).sqrt();
            let inverso = if d > 1e-9 { f / d } else { F_SEM_MOVIMENTO };
            let mut novo: Vec<String> = tokens
                .iter()
                .filter(|t| !t.starts_with('F'))
                .map(|t| t.to_string())
                .collect();
            novo.push(format!("F{}", f_inverso(inverso)));
            let mut linha = novo.join(" ");
            if !comentario.is_empty() {
                linha.push(' ');
                linha.push_str(comentario);
            }
            out.push(linha);
        } else {
            out.push(raw.clone());
        }
        if g01 || g00 {
            (x, y, z, a) = (nx, ny, nz, na);
        }
    }
    out
}

/// F de tempo inverso com 6 algarismos significativos, sem zeros à direita.
fn f_inverso(v: f64) -> String {
    let casas = (5 - v.abs().log10().floor() as i32).clamp(0, 9) as usize;
    let s = format!("{v:.casas$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    //! Cobre os auxiliares que o modo Espiral não exercita (outros modos os
    //! usam); os valores esperados foram calculados à mão a partir do Python.

    use super::*;
    use crate::params::Direction;

    fn params() -> Params {
        let mut p = Params::default();
        p.normalize();
        p
    }

    #[test]
    fn formatting_matches_python_specs() {
        assert_eq!(f3(50.1224), "50.122");
        assert_eq!(f2(0.0), "0.00");
        assert_eq!(f1(274.95), "274.9"); // meio exato arredonda para par, como o Python
        assert_eq!(f3s(1.5), "+1.500");
        assert_eq!(f3s(-0.25), "-0.250");
    }

    #[test]
    fn layer_radius_grows_outside_and_shrinks_inside() {
        let p = params();
        assert_eq!(layer_radius(&p, 2, None), 28.0); // (50 + 2*2*1.5) / 2
        assert_eq!(layer_radius(&p, -3, None), 25.0); // camada negativa conta como 0
        assert_eq!(layer_radius(&p, 7, Some(0.0)), 1e-6); // current_d manda, com piso

        let mut interno = params();
        interno.operacao_interna = true;
        interno.diametro = 100.0;
        interno.diametro_interno = 60.0;
        assert_eq!(effective_base_diameter(&interno), 60.0);
        assert_eq!(layer_radius(&interno, 1, None), 28.5); // (60 - 2*1.5) / 2
    }

    #[test]
    fn smart_layers_scales_only_root_layer() {
        let mut p = params();
        p.smart_layers = true;
        p.root_feed_scale = 80.0;
        assert_eq!(apply_smart_layers(&p, 0, 120.0), 96.0);
        assert_eq!(apply_smart_layers(&p, 1, 120.0), 120.0);
        p.smart_layers = false;
        assert_eq!(apply_smart_layers(&p, 0, 120.0), 120.0);
    }

    #[test]
    fn arc_bounds_follow_direction_and_leads() {
        let mut p = params();
        p.lead_in = 5.0;
        p.lead_out = 5.0;
        p.direcao_soldagem = Direction::DireitaEsquerda;
        let b = rotary_arc_bounds(&p);
        assert_eq!(
            (b.x_entry, b.x_exit, b.x_start, b.x_end),
            (None, None, 5.0, -55.0)
        );

        p.operacao_interna = true;
        let b = rotary_arc_bounds(&p);
        assert_eq!(
            (b.x_entry, b.x_exit, b.x_start, b.x_end),
            (Some(30.0), Some(30.0), 0.0, -50.0)
        );
    }

    #[test]
    fn extend_motion_target_scales_present_axes_only() {
        let start = Pos {
            x: Some(0.0),
            y: Some(0.0),
            ..Pos::default()
        };
        let end = Pos {
            x: Some(0.0),
            y: Some(30.0),
            ..Pos::default()
        };
        let ext = extend_motion_target(&start, &end, 3.0, None);
        assert_eq!(
            ext,
            Pos {
                x: Some(0.0),
                y: Some(33.0),
                z: None,
                a: None
            }
        );

        // Sem distância extra ou sem início: devolve o fim intacto.
        assert_eq!(extend_motion_target(&start, &end, 0.0, None), end);
        assert_eq!(extend_motion_target(&Pos::default(), &end, 3.0, None), end);
    }

    #[test]
    fn extend_motion_target_keeps_python_zero_end_quirk() {
        // Python: `end_pos.get(axis, start) or start` — X final 0.0 é "falso" e
        // volta ao X inicial, embora o deslocamento real em X tenha sido 50 mm.
        let start = Pos::xa(-50.0, 0.0);
        let end = Pos::xa(0.0, 360.0);
        let ext = extend_motion_target(&start, &end, 4.0, Some(25.0));
        assert_eq!(ext.x, Some(-50.0));
        let a = ext.a.expect("A presente");
        assert!(a > 360.0 && a < 370.0, "A prolongado: {a}");
    }

    #[test]
    fn torch_disable_early_returns_emit_only_m102() {
        let p = params();
        let start = Pos::xa(0.0, 0.0);
        let end = Pos::xa(50.0, 3600.0);

        let mut lines = Vec::new();
        let pos = append_torch_disable(
            &mut lines,
            &p,
            Some(&start),
            Some(&end),
            None,
            Some(120.0),
            None,
        );
        assert_eq!(lines, vec!["M102 (DESLIGAR Output#1 - tocha)".to_string()]);
        assert_eq!(pos, end);

        let mut sem_atraso = params();
        sem_atraso.torch_extinguish_delay_s = 0.0;
        let mut lines = Vec::new();
        let pos = append_torch_disable(
            &mut lines,
            &sem_atraso,
            Some(&start),
            Some(&end),
            Some(100.0),
            Some(120.0),
            None,
        );
        assert_eq!(lines.len(), 1);
        assert_eq!(pos, end);

        let mut lines = Vec::new();
        let pos = append_torch_disable(&mut lines, &p, None, None, Some(100.0), Some(120.0), None);
        assert_eq!(lines.len(), 1);
        assert_eq!(pos, Pos::default());
    }

    #[test]
    fn layer_separator_only_between_layers() {
        let mut p = params();
        p.num_camadas = 3;
        assert_eq!(layer_separator(&p, 0), "M01");
        assert_eq!(layer_separator(&p, 1), "M01");
        assert_eq!(layer_separator(&p, 2), "");
    }
}
