"""Gera as fixtures golden de G-code usando o motor Python (oráculo do port Rust).

Uso:  .venv/Scripts/python.exe -I scripts/golden/gen_golden.py

Saída (idempotente: sobrescreve os arquivos existentes):
  tests/golden/cases.json    lista de {"name", "params"}; params = BASE + overrides do caso
  tests/golden/<name>.gcode  linhas do Python unidas por "\\n"; a 3a linha é fixada em
                             "(Data: 01/01/2000 00:00:00)" porque o Python grava a data atual
"""
import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "src" / "app"))

from core.generator import GCodeGenerator  # noqa: E402

OUT_DIR = REPO / "tests" / "golden"
FIXED_DATE_LINE = "(Data: 01/01/2000 00:00:00)"

# Defaults da UI atual.
BASE = {
    "diametro": 50.0,
    "diametro_interno": 0.0,
    "comprimento_revestir": 50.0,
    "largura_revestir": 100.0,
    "largura_cordao": 8.0,
    "sobreposicao": 40.0,
    "velocidade_de_deposicao": 120.0,
    "taxa_de_deposicao": 120.0,
    "velocidade_a_mm_min": 120.0,
    "velocidade_oscilacao_mm_min": 3000.0,
    "afastamento_tocha": 12.0,
    "afastamento_seguranca": 30.0,
    "torch_ignite_delay_s": 3.0,
    "torch_extinguish_delay_s": 2.0,
    "overlap_angular_deg": 10.0,
    "bead_width_calibration_factor": 1.0,
    "lead_extras_enabled": False,
    "lead_in": 0.0,
    "lead_out": 0.0,
    "direcao_soldagem": "esquerda_direita",
    "sentido_rotacao": "horaria",
    "corrente_arco": 180.0,
    "tensao_arco": 25.0,
    "vazao_gas": 15.0,
    "alim_po": 50.0,
    "preaquecimento": 100.0,
    "eficiencia_termica": 0.8,
    "taxa_deposicao_g_h": 700.0,
    "num_camadas": 1,
    "espessura_camada": 1.5,
    "powder_factor": 0.16,
    "powder_cost_brl_kg": 450.0,
    "torch_retract_on_ignite": True,
    "spiral_close_ends": False,
    "flat_single_direction": False,
    "flat_orientation": "X",
    "operacao_interna": False,
    "internal_face_clearance_x": 30.0,
    "multiperfil_count": 4,
    "multiperfil_total_angle_deg": 360.0,
    "multiperfil_pitch_mm": 12.0,
    "multiperfil_strategy": "Passes separados",
    "multiperfil_point_by_point": False,
    "multiperfil_profile_shape": "Retangular",
    "multiperfil_profile_base_mm": 8.0,
    "multiperfil_profile_top_mm": 4.0,
    "multiperfil_profile_height_mm": 1.5,
    "multiperfil_end_chamfer_enabled": False,
    "multiperfil_end_chamfer_angle_deg": 45.0,
    "bolacha_tipo": "Espiral Contínua",
    "bolacha_stop_between": False,
    "oscilacao_comprimento": 5.0,
    "deslocamento_angular_perc": 20.0,
    "osc_test_gran_x": 0.5,
    "osc_test_gran_a": 1.0,
    "smart_layers": False,
    "root_feed_scale": 100.0,
    "nome_procedimento": "Novo Procedimento",
    "traceability_operator": "",
    "traceability_eps": "",
    "traceability_powder_batch": "",
    "traceability_supplier": "",
    "traceability_certificate": "",
    "app_version": "11.3.2",
}

# Bases compartilhadas por grupo de casos (o "+" da matriz).
MULTI = {"welding_mode": "espiral_multiperfil"}
BOLACHA = {"welding_mode": "bolacha", "diametro": 100}
BOLACHA_ANEIS = {**BOLACHA, "bolacha_tipo": "Anéis (Passo a Passo)", "diametro_interno": 20}
CHAPA = {"welding_mode": "chapa_plana"}
OSC_LIN = {"welding_mode": "oscilacao", "oscillation_type": "linear"}
# Quadrada e quadrada contínua usam comprimento_revestir 10.0 para manter cada .gcode < ~300 KB.
OSC_QUAD = {"welding_mode": "oscilacao", "oscillation_type": "quadrada", "comprimento_revestir": 10.0}
OSC_QUAD_CONT = {"welding_mode": "oscilacao", "oscillation_type": "quadrada_continua", "comprimento_revestir": 10.0}
LEADS = {"lead_extras_enabled": True, "lead_in": 5, "lead_out": 5}

# Matriz: nome -> overrides sobre BASE.
CASES = [
    # Espiral
    ("espiral_padrao", {"welding_mode": "espiral"}),
    ("espiral_3camadas_de_ccw", {"num_camadas": 3, "direcao_soldagem": "direita_esquerda", "sentido_rotacao": "antihoraria"}),
    ("espiral_fechado_leads", {"spiral_close_ends": True, **LEADS}),
    ("espiral_interno", {"operacao_interna": True, "diametro": 100, "diametro_interno": 60, "num_camadas": 2}),
    ("espiral_sem_atrasos", {"torch_ignite_delay_s": 0, "torch_extinguish_delay_s": 0, "torch_retract_on_ignite": False}),
    ("espiral_rastreab", {"traceability_eps": "EPS-01", "traceability_operator": "Op", "traceability_powder_batch": "L1",
                          "traceability_supplier": "F", "traceability_certificate": "C1"}),
    # Espiral multiperfil
    ("multiperfil_retangular", MULTI),
    ("multiperfil_trapezoidal_chanfro", {**MULTI, "multiperfil_profile_shape": "Trapezoidal",
                                         "multiperfil_end_chamfer_enabled": True, "multiperfil_profile_height_mm": 4.5}),
    ("multiperfil_triangular_setor", {**MULTI, "multiperfil_profile_shape": "Triangular",
                                      "multiperfil_count": 3, "multiperfil_total_angle_deg": 180}),
    ("multiperfil_ponto_a_ponto", {**MULTI, "multiperfil_point_by_point": True, "multiperfil_profile_height_mm": 3.0}),
    ("multiperfil_continuo_fechado", {**MULTI, "multiperfil_strategy": "Percurso contínuo", "spiral_close_ends": True}),
    ("multiperfil_interno_ccw", {**MULTI, "operacao_interna": True, "diametro": 120, "diametro_interno": 80,
                                 "sentido_rotacao": "antihoraria"}),
    # Bolacha
    ("bolacha_espiral", BOLACHA),
    ("bolacha_aneis", BOLACHA_ANEIS),
    ("bolacha_aneis_parada", {**BOLACHA_ANEIS, "bolacha_stop_between": True}),
    ("bolacha_de_2camadas_smart", {"welding_mode": "bolacha", "direcao_soldagem": "direita_esquerda", "num_camadas": 2,
                                   "smart_layers": True, "root_feed_scale": 80, "diametro": 80}),
    # Chapa plana
    ("chapa_x", CHAPA),
    ("chapa_y_de", {**CHAPA, "flat_orientation": "Y", "direcao_soldagem": "direita_esquerda"}),
    ("chapa_unidirecional_leads", {**CHAPA, "flat_single_direction": True, **LEADS, "num_camadas": 2}),
    # Oscilação linear
    ("osc_linear", OSC_LIN),
    ("osc_linear_interno", {**OSC_LIN, "operacao_interna": True, "diametro": 100, "diametro_interno": 60}),
    ("osc_linear_de_ccw_leads", {**OSC_LIN, "direcao_soldagem": "direita_esquerda", "sentido_rotacao": "antihoraria",
                                 **LEADS, "num_camadas": 2}),
    # Oscilação quadrada
    ("osc_quadrada", OSC_QUAD),
    ("osc_quadrada_interno", {**OSC_QUAD, "operacao_interna": True, "diametro": 100, "diametro_interno": 60}),
    ("osc_quadrada_de_leads", {**OSC_QUAD, "direcao_soldagem": "direita_esquerda", **LEADS}),
    # Oscilação quadrada contínua
    ("osc_quad_continua", OSC_QUAD_CONT),
    ("osc_quad_continua_de_leadout", {**OSC_QUAD_CONT, "direcao_soldagem": "direita_esquerda",
                                      "lead_extras_enabled": True, "lead_out": 5}),
    ("osc_quad_continua_2camadas_gran", {**OSC_QUAD_CONT, "num_camadas": 2, "osc_test_gran_x": 1.0, "osc_test_gran_a": 2.0}),
    # Modos legados
    ("legado_osc_linear", {"welding_mode": "oscilacao_linear"}),
    ("legado_osc_quadrada", {"welding_mode": "oscilacao_quadrada", "oscillation_type": "quadrada"}),
]


def main():
    names = [name for name, _ in CASES]
    if len(names) != len(set(names)):
        raise RuntimeError("nomes de caso duplicados em CASES")
    OUT_DIR.mkdir(parents=True, exist_ok=True)

    cases = []
    for name, overrides in CASES:
        params = {**BASE, **overrides}
        # generate() normaliza o dict in place: passa uma cópia para cases.json guardar a entrada bruta.
        lines = GCodeGenerator().generate(dict(params))
        if not lines:
            raise RuntimeError(f"{name}: generate() não retornou linhas")
        if not lines[2].startswith("(Data: "):
            raise RuntimeError(f"{name}: 3a linha inesperada: {lines[2]!r}")
        lines[2] = FIXED_DATE_LINE
        with open(OUT_DIR / f"{name}.gcode", "w", encoding="utf-8", newline="") as f:
            f.write("\n".join(lines))
        cases.append({"name": name, "params": params})

    with open(OUT_DIR / "cases.json", "w", encoding="utf-8", newline="") as f:
        json.dump(cases, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print(f"{len(cases)} casos gravados")


if __name__ == "__main__":
    main()
