"""Testes com asserts do gerador de G-code e do avaliador de fórmulas."""
import json
import math
import os
import re
import sys

import pytest

PROJECT_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
sys.path.insert(0, os.path.join(PROJECT_ROOT, 'src', 'app'))

from gcode_generator import GCodeGenerator  # noqa: E402
from safe_eval import safe_eval  # noqa: E402

BASE_PARAMS = {
    'diametro': 50.0,
    'comprimento_revestir': 60.0,
    'afastamento_tocha': 12.0,
    'num_camadas': 2,
    'espessura_camada': 1.5,
    'largura_cordao': 8.0,
    'sobreposicao': 40.0,
    'velocidade_de_deposicao': 120.0,
    'lead_in': 5.0,
    'lead_out': 5.0,
    'oscilacao_comprimento': 10.0,
    'deslocamento_angular_perc': 50.0,
    'direcao_soldagem': 'esquerda_direita',
    'sentido_rotacao': 'horaria',
}

MODES = [
    ('espiral', None),
    ('oscilacao', 'linear'),
    ('oscilacao', 'quadrada'),
    ('oscilacao', 'quadrada_continua'),
]


def _params(mode, osc_type, **extra):
    p = dict(BASE_PARAMS, welding_mode=mode, **extra)
    if osc_type:
        p['oscillation_type'] = osc_type
    return p


@pytest.mark.parametrize('mode,osc_type', MODES)
@pytest.mark.parametrize('compact', [False, True])
def test_generate_program_structure(mode, osc_type, compact):
    gcode = GCodeGenerator().generate(_params(mode, osc_type, compact_gcode=compact))
    assert gcode, 'gerador retornou vazio'
    assert gcode[0] == '%' and gcode[-1] == '%'
    assert 'M30 (FIM DO PROGRAMA)' in gcode
    assert sum(1 for line in gcode if line.startswith('(--- CAMADA')) == BASE_PARAMS['num_camadas']
    # Tocha ligada e desligada exatamente uma vez
    assert sum(1 for line in gcode if line.startswith('M101')) == 1
    assert sum(1 for line in gcode if line.startswith('M102')) == 1


@pytest.mark.parametrize('mode,osc_type', MODES)
@pytest.mark.parametrize('direction', ['esquerda_direita', 'direita_esquerda'])
@pytest.mark.parametrize('rotation', ['horaria', 'anti_horaria'])
def test_generate_numbers_are_finite(mode, osc_type, direction, rotation):
    gcode = GCodeGenerator().generate(
        _params(mode, osc_type, direcao_soldagem=direction, sentido_rotacao=rotation))
    for line in gcode:
        code = line.split('(')[0]
        assert 'nan' not in code.lower() and 'inf' not in code.lower(), line
        for word in re.findall(r'\b([XYZAF])(-?\d+(?:\.\d+)?)', code):
            assert math.isfinite(float(word[1])), line
        for feed in re.findall(r'\bF(-?\d+(?:\.\d+)?)', code):
            assert float(feed) > 0, line


def test_spiral_compact_is_shorter():
    gen = GCodeGenerator()
    full = gen.generate(_params('espiral', None, compact_gcode=False))
    compact = gen.generate(_params('espiral', None, compact_gcode=True))
    assert len(compact) < len(full)


def test_legacy_speed_key_is_normalized():
    p = dict(BASE_PARAMS, welding_mode='espiral')
    p['velocidade_soldagem'] = p.pop('velocidade_de_deposicao')
    assert GCodeGenerator().generate(p)


def test_unknown_mode_returns_none():
    assert GCodeGenerator().generate(dict(BASE_PARAMS, welding_mode='xyz')) is None
    assert GCodeGenerator().generate(None) is None


# --- safe_eval ---

NAMES = {'math': math, 'max': max, 'min': min, 'abs': abs, 'round': round,
         'ceil': math.ceil, 'floor': math.floor, 'pi': math.pi, 'd': 50.0}


@pytest.mark.parametrize('expr,expected', [
    ('math.pi * d', math.pi * 50.0),
    ('max(d, 1e-6) / 2', 25.0),
    ('ceil(d / 3)', 17),
    ('-d ** 2', -2500.0),
    ('1 if d > 10 else 0', 1),
    ('math.sqrt(d * 2)', 10.0),
])
def test_safe_eval_allows_math(expr, expected):
    assert safe_eval(expr, NAMES) == pytest.approx(expected)


def test_safe_eval_unknown_name_raises_name_error():
    with pytest.raises(NameError):
        safe_eval('d + outro', NAMES)


@pytest.mark.parametrize('expr', [
    "().__class__.__base__.__subclasses__()",
    "__import__('os').system('echo x')",
    "math.__dict__",
    "open('x')",
    "[1, 2]",
    "'texto'",
    "lambda: 1",
    "10 ** 100000",
])
def test_safe_eval_rejects_unsafe(expr):
    with pytest.raises((ValueError, NameError, SyntaxError)):
        safe_eval(expr, NAMES)


def test_default_config_formulas_evaluate():
    with open(os.path.join(PROJECT_ROOT, 'config', 'config.json'), encoding='utf-8') as f:
        cfg = json.load(f)
    exprs = dict(cfg['formulas']['expressions'])
    names = dict(NAMES)
    names.update({
        'diametro': 50.0, 'comprimento_revestir': 100.0, 'lead_in': 5.0, 'lead_out': 5.0,
        'largura_cordao': 8.0, 'sobreposicao': 40.0, 'velocidade_de_deposicao': 120.0,
        'num_camadas': 2, 'espessura_camada': 1.5, 'vazao_gas': 12.0, 'powder_factor': 0.07,
        'powder_cost_brl_kg': 700.0, 'labor_brl_hour': 40.0, 'machine_brl_hour': 250.0,
        'gas_argon_brl_m3': 50.0, 'taxa_deposicao_g_h': 1500.0,
    })
    # Resolve dependências em ordem invertida para exercitar múltiplos passes
    results, pending = {}, dict(reversed(list(exprs.items())))
    for _ in range(len(pending) + 1):
        for key in list(pending):
            try:
                results[key] = safe_eval(pending.pop(key), {**names, **results})
            except NameError:
                pending[key] = exprs[key]
    assert not pending, f'fórmulas não resolvidas: {sorted(pending)}'
    assert results['total_cost_brl'] > 0
