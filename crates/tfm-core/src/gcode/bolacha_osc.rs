//! Oscilação na Bolacha, em torno da trajetória (espiral ou anéis) enquanto a
//! mesa gira. Não existia no Python.
//!
//! O raio vai e volta e a onda varre a largura do cordão. O passo radial usa a
//! largura varrida (cordão + oscilação) e a trajetória central fica meia
//! oscilação para dentro de cada borda. O `F` de cada trecho é escalado pela
//! distância real sobre a peça, então a velocidade de solda segue a pedida.
//!
//! O eixo ([`BolachaOscEixo`]) é onde a tocha está montada: em X ela fica à
//! direita e o X faz o raio; em Y ela fica atrás, com X = 0, e o Y faz o raio e
//! a oscilação. O programa em Y é o do X com o X escrito como Y.
//!
//! * **Onda** ([`BolachaOscForma`]): triangular (retas com vértice nos picos),
//!   senoidal (`bolacha_osc_pontos_ciclo` pontos por ciclo) ou quadrada (o eixo
//!   sobe e desce com a mesa parada, na velocidade da oscilação).
//! * **Período** em mm ao longo do arco da mesa: um ciclo percorre `periodo`,
//!   então perto do centro, onde o arco é curto, cabem menos ciclos por volta.
//! * **Fase entre camadas**: cada camada começa a onda mais adiante, para os
//!   picos não empilharem.

use std::f64::consts::TAU;

use super::common::{
    append_torch_disable, append_torch_enable, apply_smart_layers, f1, f3, layer_separator, py_max,
    py_min, Pos,
};
use crate::params::{
    BolachaOscEixo, BolachaOscForma, BolachaOscSentido, BolachaTipo, Direction, Params,
};

/// Passo fino em que a fase da onda é integrada (graus).
const DEG_STEP: f64 = 0.5;
/// Menor período aceito (mm).
pub const MIN_PERIODO_MM: f64 = 1.0;
/// Menor número de pontos por ciclo da senoide.
pub const MIN_PONTOS_CICLO: f64 = 6.0;
/// Resolução angular da espiral contínua, a mesma da bolacha sem oscilação.
const DEG_PER_STEP: f64 = 10.0;
/// Tolerância para considerar dois valores iguais.
const EPS: f64 = 1e-9;
/// No eixo X o Mach3 interpola em reta em X e A, então o raio muda ao longo de
/// cada linha e a velocidade sobre a peça varia junto. Cada linha muda o raio em
/// no máximo esta fração dele, o que segura a variação em torno de metade disso.
const MAX_DX_REL: f64 = 0.08;

/// Faixa radial da oscilação: limites da peça, faixa do centro do cordão e o
/// passo entre voltas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Faixa {
    pub r_int: f64,
    pub r_ext: f64,
    /// Menor raio do centro do cordão: `r_int` mais meia oscilação.
    pub c_int: f64,
    /// Maior raio do centro do cordão: `r_ext` menos meia oscilação.
    pub c_ext: f64,
    /// Passo radial: `(largura + oscilação) × (1 − sobreposição)`, ou o ajustado
    /// à largura da peça com a faixa de largura ligada.
    pub passo: f64,
    pub num_voltas: i64,
    /// Fator da velocidade de solda que compensa o passo ajustado (1 sem ajuste).
    pub fator_velocidade: f64,
}

pub fn faixa(p: &Params) -> Faixa {
    let r_ext = p.diametro / 2.0;
    let r_int = py_max(p.diametro_interno / 2.0, p.largura_cordao / 2.0);
    let amp = py_max(p.oscilacao_comprimento, 0.0);
    let (c_int, c_ext) = if r_ext - r_int > amp {
        (r_int + amp / 2.0, r_ext - amp / 2.0)
    } else {
        let meio = (r_int + r_ext) / 2.0;
        (meio, meio)
    };
    let ajuste = super::bolacha::passo_radial(p, c_ext - c_int, amp);
    Faixa {
        r_int,
        r_ext,
        c_int,
        c_ext,
        passo: ajuste.passo,
        num_voltas: ajuste.num_voltas,
        fator_velocidade: ajuste.fator_velocidade,
    }
}

/// Quanto o tempo do caminho com a onda é maior que o sem ela, e quanto tempo de
/// espera cada milímetro de trajetória ainda traz (s/mm), para as estimativas.
///
/// A onda alonga o caminho; na quadrada, os degraus andam com a mesa parada,
/// na velocidade da oscilação.
pub fn weave_cost(p: &Params) -> (f64, f64) {
    let h = py_max(p.oscilacao_comprimento, 0.0) / 2.0;
    let periodo = py_max(p.bolacha_osc_periodo_mm, MIN_PERIODO_MM);
    let taxa = py_max(p.taxa(), 1e-6);
    let feed_x = if p.velocidade_oscilacao_mm_min != 0.0 {
        p.velocidade_oscilacao_mm_min
    } else {
        taxa
    };
    let fator = match p.bolacha_osc_forma {
        BolachaOscForma::Quadrada => 1.0 + 4.0 * h / periodo * taxa / feed_x,
        BolachaOscForma::Triangular => (1.0 + (4.0 * h / periodo).powi(2)).sqrt(),
        BolachaOscForma::Senoidal => {
            let n = 64;
            (0..n)
                .map(|k| {
                    let c = (TAU * (k as f64 + 0.5) / n as f64).cos();
                    (1.0 + (TAU * h / periodo * c).powi(2)).sqrt()
                })
                .sum::<f64>()
                / n as f64
        }
    };
    (fator, 2.0 * py_max(p.bolacha_osc_espera_s, 0.0) / periodo)
}

/// Um ponto em que a onda muda de regra, dentro de um ciclo.
#[derive(Clone, Copy)]
struct Quebra {
    fase: f64,
    /// Pico: ali vale a espera.
    pico: bool,
    /// Salto: o valor muda sem a mesa andar.
    salto: bool,
}

#[derive(Clone, Copy)]
struct Onda {
    forma: BolachaOscForma,
    meia: f64,
    /// `+1` começa para fora; `-1`, para dentro.
    sentido: f64,
    periodo: f64,
    pontos: f64,
}

impl Onda {
    fn new(p: &Params) -> Onda {
        Onda {
            forma: p.bolacha_osc_forma,
            meia: py_max(p.oscilacao_comprimento, 0.0) / 2.0,
            sentido: match p.bolacha_osc_sentido {
                BolachaOscSentido::ParaFora => 1.0,
                BolachaOscSentido::ParaDentro => -1.0,
            },
            periodo: py_max(p.bolacha_osc_periodo_mm, MIN_PERIODO_MM),
            pontos: py_max(p.bolacha_osc_pontos_ciclo.round(), MIN_PONTOS_CICLO),
        }
    }

    /// Deslocamento (mm) na fase `fase` (ciclos), no eixo da oscilação.
    fn deslocamento(&self, fase: f64) -> f64 {
        let f = fase.rem_euclid(1.0);
        let v = match self.forma {
            BolachaOscForma::Triangular => {
                if f < 0.25 {
                    4.0 * f
                } else if f < 0.75 {
                    2.0 - 4.0 * f
                } else {
                    4.0 * f - 4.0
                }
            }
            BolachaOscForma::Senoidal => (TAU * f).sin(),
            BolachaOscForma::Quadrada => {
                if f < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
        };
        self.sentido * self.meia * v
    }

    fn quebras(&self) -> Vec<Quebra> {
        match self.forma {
            BolachaOscForma::Triangular => vec![
                Quebra {
                    fase: 0.25,
                    pico: true,
                    salto: false,
                },
                Quebra {
                    fase: 0.75,
                    pico: true,
                    salto: false,
                },
            ],
            BolachaOscForma::Senoidal => {
                let n = self.pontos as usize;
                (0..n)
                    .map(|k| {
                        let fase = k as f64 / n as f64;
                        Quebra {
                            fase,
                            pico: (fase - 0.25).abs() < EPS || (fase - 0.75).abs() < EPS,
                            salto: false,
                        }
                    })
                    .collect()
            }
            BolachaOscForma::Quadrada => vec![
                Quebra {
                    fase: 0.0,
                    pico: true,
                    salto: true,
                },
                Quebra {
                    fase: 0.5,
                    pico: true,
                    salto: true,
                },
            ],
        }
    }
}

/// Vértice do caminho: ângulo percorrido desde o início da sessão (graus, sem
/// sinal) e raio absoluto.
#[derive(Clone, Copy, Debug)]
struct Vertice {
    theta: f64,
    x: f64,
    pico: bool,
}

/// Percorre a trajetória central e solta um vértice onde a onda muda de regra.
struct Caminhante<'a> {
    onda: &'a Onda,
    limites: (f64, f64),
    vertices: Vec<Vertice>,
    theta: f64,
    fase: f64,
}

impl<'a> Caminhante<'a> {
    fn novo(onda: &'a Onda, limites: (f64, f64), fase: f64, x0: f64) -> Self {
        let mut c = Caminhante {
            onda,
            limites,
            vertices: Vec::new(),
            theta: 0.0,
            fase,
        };
        c.empurra(0.0, x0, onda.deslocamento(fase), false);
        c
    }

    /// Soma o deslocamento `desloc` da onda ao raio.
    fn empurra(&mut self, theta: f64, x_central: f64, desloc: f64, pico: bool) {
        let x = py_max(self.limites.0, py_min(self.limites.1, x_central + desloc));
        if let Some(ultimo) = self.vertices.last() {
            if (ultimo.theta - theta).abs() < EPS && (ultimo.x - x).abs() < EPS {
                return;
            }
        }
        self.vertices.push(Vertice { theta, x, pico });
    }

    /// Anda até `ate` graus com a trajetória central `x_central(theta)`.
    fn anda(&mut self, ate: f64, x_central: impl Fn(f64) -> f64) {
        let quebras = self.onda.quebras();
        while self.theta < ate - EPS {
            let proximo = (self.theta + DEG_STEP).min(ate);
            let raio = py_max(x_central((self.theta + proximo) / 2.0), 2.0);
            let dfase = raio * (proximo - self.theta).to_radians() / self.onda.periodo;
            let fim = self.fase + dfase;
            let primeiro_ciclo = self.fase.floor() as i64;
            let ultimo_ciclo = fim.floor() as i64;
            for k in primeiro_ciclo..=ultimo_ciclo {
                for q in &quebras {
                    let bf = k as f64 + q.fase;
                    if bf > self.fase + 1e-12 && bf <= fim + 1e-12 {
                        let t = self.theta + (bf - self.fase) / dfase * (proximo - self.theta);
                        let nominal = x_central(t);
                        if q.salto {
                            let antes = self.onda.deslocamento(bf - 1e-9);
                            self.empurra(t, nominal, antes, false);
                        }
                        self.empurra(t, nominal, self.onda.deslocamento(bf), q.pico);
                    }
                }
            }
            self.theta = proximo;
            self.fase = fim;
        }
        let central = x_central(self.theta);
        self.empurra(
            self.theta,
            central,
            self.onda.deslocamento(self.fase),
            false,
        );
    }

    /// Anda até `ate` graus em reta até o raio `x_destino`, sem onda: a fase
    /// fica parada, e a onda continua dali no trecho seguinte.
    fn transita(&mut self, ate: f64, x_destino: f64) {
        let desloc = self.onda.deslocamento(self.fase);
        self.theta = py_max(self.theta, ate);
        self.empurra(self.theta, x_destino, desloc, false);
    }

    /// Entrega os vértices novos, mantendo o último para a continuidade.
    fn drena(&mut self) -> Vec<Vertice> {
        let n = self.vertices.len();
        let novos = self.vertices.clone();
        let ultimo = self.vertices[n - 1];
        self.vertices = vec![ultimo];
        novos
    }
}

/// Grava os movimentos entre vértices e guarda onde a solda terminou.
struct Emissor {
    a_ini: f64,
    sinal: f64,
    taxa: f64,
    feed_x: f64,
    espera: f64,
    /// Ângulo da sessão no início do trecho atual (graus).
    base: f64,
    ultimo: (f64, f64),
    /// Início do último movimento em que a mesa girou.
    cauda: (f64, f64),
}

impl Emissor {
    fn a_de(&self, theta: f64) -> f64 {
        self.a_ini + self.sinal * (self.base + theta)
    }

    /// `vertices[0]` é onde já estamos; os demais viram `G01`.
    fn emite(&mut self, seg: &mut Vec<String>, vertices: &[Vertice]) {
        for par in vertices.windows(2) {
            let (a, b) = (par[0], par[1]);
            let dtheta = b.theta - a.theta;
            let dx = b.x - a.x;
            if dtheta.abs() < EPS && dx.abs() < EPS {
                continue;
            }
            let destino = (b.x, self.a_de(b.theta));
            if dtheta.abs() < EPS {
                // Só o raio anda, com a mesa parada.
                seg.push(format!("G01 F{} X{}", f1(self.feed_x), f3(b.x)));
            } else {
                let raio = py_max((a.x + b.x) / 2.0, 2.0);
                let partes = ((dx.abs() / (MAX_DX_REL * raio)).ceil() as usize).max(1);
                let (mut x0, mut t0) = (a.x, a.theta);
                for k in 1..=partes {
                    let u = k as f64 / partes as f64;
                    let (x1, t1) = (a.x + dx * u, a.theta + dtheta * u);
                    let (sdx, sdt) = (x1 - x0, t1 - t0);
                    let raio = py_max((x0 + x1) / 2.0, 2.0);
                    let lido = sdx.hypot(sdt);
                    let real = sdx.hypot(raio * sdt.to_radians());
                    let feed = self.taxa * lido / real;
                    let a1 = self.a_de(t1);
                    if sdx.abs() < 5e-4 {
                        seg.push(format!("G01 F{} A{}", f1(feed), f3(a1)));
                    } else {
                        seg.push(format!("G01 F{} X{} A{}", f1(feed), f3(x1), f3(a1)));
                    }
                    x0 = x1;
                    t0 = t1;
                }
                self.cauda = (b.x, self.a_de(a.theta));
            }
            if b.pico && self.espera > 0.0 {
                seg.push(format!(
                    "G04 P{:.2} (Espera na ponta da oscilacao)",
                    self.espera
                ));
            }
            self.ultimo = destino;
        }
    }
}

fn nome_da_forma(f: BolachaOscForma) -> &'static str {
    match f {
        BolachaOscForma::Triangular => "triangular",
        BolachaOscForma::Senoidal => "senoidal",
        BolachaOscForma::Quadrada => "quadrada",
    }
}

fn nome_do_primeiro(eixo: BolachaOscEixo, s: BolachaOscSentido) -> &'static str {
    match (eixo, s) {
        (BolachaOscEixo::X, BolachaOscSentido::ParaFora) => "para fora",
        (BolachaOscEixo::X, BolachaOscSentido::ParaDentro) => "para dentro",
        (BolachaOscEixo::Y, BolachaOscSentido::ParaFora) => "para fora (Y positivo)",
        (BolachaOscEixo::Y, BolachaOscSentido::ParaDentro) => "para dentro (Y negativo)",
    }
}

/// Avanço angular (graus/min) que mantém a velocidade tangencial no raio `x`,
/// com a rotação até `limite_rpm`.
fn angular_feed(taxa: f64, x: f64, limite_rpm: f64) -> f64 {
    super::bolacha::angular_feed(taxa, x, limite_rpm)
}

/// `G00` que leva a tocha ao início da onda (raio e A).
fn posiciona(v: &Vertice, a: f64) -> String {
    format!("G00 X{} A{}", f3(v.x), f3(a))
}

/// Tocha montada sobre o eixo Y: o raio que o programa escreve em X vai para
/// o Y, e cada `G00` põe o X em 0.
fn raio_em_y(seg: Vec<String>) -> Vec<String> {
    seg.into_iter()
        .map(|l| {
            if let Some(resto) = l.strip_prefix("(--- ANEL ") {
                return format!("(--- ANEL {}", resto.replace(" em X=", " em Y="));
            }
            let (codigo, comentario) = match l.find('(') {
                Some(k) => (l[..k].trim_end().to_string(), format!(" {}", &l[k..])),
                None => (l.clone(), String::new()),
            };
            if !codigo.starts_with("G0") || !codigo.contains('X') {
                return l;
            }
            let mut tokens: Vec<String> = codigo
                .split_whitespace()
                .map(|t| match t.strip_prefix('X') {
                    Some(v) => format!("Y{v}"),
                    None => t.to_string(),
                })
                .collect();
            if tokens[0] == "G00" {
                tokens.insert(1, format!("X{}", f3(0.0)));
            }
            format!("{}{comentario}", tokens.join(" "))
        })
        .collect()
}

/// Uma camada da bolacha com oscilação.
pub fn build_segment(p: &Params, layer_num: i64) -> Vec<String> {
    let f = faixa(p);
    let onda = Onda::new(p);
    let eixo = p.bolacha_osc_eixo;
    let z_layer = (layer_num as f64 * p.espessura_camada) + p.afastamento_tocha;
    let z_seguranca = z_layer + p.afastamento_seguranca;
    let taxa = apply_smart_layers(p, layer_num, p.taxa()) * f.fator_velocidade;
    let feed_x = if p.velocidade_oscilacao_mm_min != 0.0 {
        p.velocidade_oscilacao_mm_min
    } else {
        taxa
    };
    let sinal = p.rotation_sign();
    let ov = p.overlap_angular_deg;
    let limites = (f.r_int, f.r_ext);
    let clamp_c = |x: f64| py_max(f.c_int, py_min(f.c_ext, x));
    let (x_start, x_sinal) = match p.direcao_soldagem {
        Direction::EsquerdaDireita => (f.c_int, 1.0),
        Direction::DireitaEsquerda => (f.c_ext, -1.0),
    };
    let a_ini = (layer_num as f64 * 15.0) * sinal;
    let fase0 = layer_num as f64 * p.bolacha_osc_fase_camada_perc / 100.0;

    let mut seg = vec![
        format!("(--- CAMADA {} ---)", layer_num + 1),
        "(MODO: BOLACHA - FACE CIRCULAR COM OSCILACAO)".to_string(),
        format!(
            "(OSCILACAO: {}, eixo {}, {:.1} mm, periodo {:.1} mm, primeiro {})",
            nome_da_forma(p.bolacha_osc_forma),
            match eixo {
                BolachaOscEixo::X => "X",
                BolachaOscEixo::Y => "Y",
            },
            p.oscilacao_comprimento,
            onda.periodo,
            nome_do_primeiro(eixo, p.bolacha_osc_sentido)
        ),
        format!("G00 Z{}", f3(z_seguranca)),
    ];

    let mut cam = Caminhante::novo(&onda, limites, fase0, x_start);
    seg.push(posiciona(&cam.vertices[0], a_ini));
    seg.push(format!("G00 Z{}", f3(z_layer)));
    if layer_num == 0 {
        seg.push("M0 (Confirme e pressione Start)".to_string());
    }
    append_torch_enable(&mut seg, p);

    let mut em = Emissor {
        a_ini,
        sinal,
        taxa,
        feed_x,
        espera: py_max(p.bolacha_osc_espera_s, 0.0),
        base: 0.0,
        ultimo: (cam.vertices[0].x, a_ini),
        cauda: (cam.vertices[0].x, a_ini),
    };

    match p.bolacha_tipo {
        BolachaTipo::Aneis => {
            for v in 0..=f.num_voltas {
                let x_pos = clamp_c(x_start + v as f64 * f.passo * x_sinal);
                seg.push(format!(
                    "(--- ANEL {}/{} em X={} ---)",
                    v + 1,
                    f.num_voltas + 1,
                    f3(x_pos)
                ));
                if v > 0 && p.bolacha_stop_between {
                    // Nova sessão: a onda recomeça na fase da camada.
                    em.base += cam.theta;
                    cam = Caminhante::novo(&onda, limites, fase0, x_pos);
                    seg.push(posiciona(&cam.vertices[0], em.a_de(0.0)));
                    seg.push(format!("G00 Z{}", f3(z_layer)));
                    append_torch_enable(&mut seg, p);
                    em.ultimo = (cam.vertices[0].x, em.a_de(0.0));
                } else if v == 0 {
                    seg.push(posiciona(&cam.vertices[0], a_ini));
                }
                let inicio = cam.theta;
                cam.anda(inicio + 360.0, |_| x_pos);
                let vs = cam.drena();
                em.emite(&mut seg, &vs);
                let inicio = cam.theta;
                // Sobreposição angular e troca de raio em reta, sem oscilar: a
                // onda não volta por cima do começo do anel.
                if v < f.num_voltas && !p.bolacha_stop_between {
                    let proximo = clamp_c(x_start + (v + 1) as f64 * f.passo * x_sinal);
                    cam.transita(inicio + ov, proximo);
                } else {
                    cam.transita(inicio + ov, x_pos);
                }
                let vs = cam.drena();
                em.emite(&mut seg, &vs);
                if v < f.num_voltas && p.bolacha_stop_between {
                    append_torch_disable(
                        &mut seg,
                        p,
                        Some(&Pos::xa(em.ultimo.0, em.cauda.1)),
                        Some(&Pos::xa(em.ultimo.0, em.ultimo.1)),
                        Some(angular_feed(taxa, x_pos, p.limite_rpm_placa)),
                        Some(taxa),
                        Some(py_max(x_pos, 1e-6)),
                    );
                    seg.push(format!("G00 Z{}", f3(z_seguranca)));
                }
            }
        }
        BolachaTipo::EspiralContinua => {
            let passos_extra = if ov > 0.0 {
                (ov / DEG_PER_STEP).ceil()
            } else {
                0.0
            };
            let total = f.num_voltas as f64 * 360.0 + passos_extra * DEG_PER_STEP;
            let voltas = f.num_voltas as f64;
            cam.anda(total, |th| {
                clamp_c(x_start + x_sinal * f.passo * (th / 360.0).min(voltas))
            });
            let vs = cam.drena();
            em.emite(&mut seg, &vs);
        }
    }

    append_torch_disable(
        &mut seg,
        p,
        Some(&Pos::xa(em.ultimo.0, em.cauda.1)),
        Some(&Pos::xa(em.ultimo.0, em.ultimo.1)),
        Some(angular_feed(taxa, em.ultimo.0, p.limite_rpm_placa)),
        Some(taxa),
        Some(py_max(em.ultimo.0, 1e-6)),
    );
    seg.push(format!("G00 Z{}", f3(z_seguranca)));
    seg.push(layer_separator(p, layer_num));
    match eixo {
        BolachaOscEixo::X => seg,
        BolachaOscEixo::Y => raio_em_y(seg),
    }
}
