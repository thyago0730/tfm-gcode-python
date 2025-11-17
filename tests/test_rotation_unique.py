#!/usr/bin/env python3
"""
Teste rápido: verifica se o G-code gerado contém
APENAS UMA linha G93 G01 A... por camada (rotação única).
"""
import sys, os
# Ajuste do sys.path para funcionar dentro da pasta tests
PROJECT_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
sys.path.insert(0, os.path.join(PROJECT_ROOT, 'src', 'app'))

from TFM_GCODE import GCodeGenerator

def test_rotation_unique():
    gen = GCodeGenerator()
    params = {
        'tipo_oscilacao': 'linear',
        'diametro': 200,
        'comprimento_revestir': 300,
        'num_camadas': 2,
        'espessura_camada': 2,
        'largura_cordao': 12,
        'sobreposicao': 30,
        'oscilacao_comprimento': 60,
        'deslocamento_angular_perc': 50,
        'velocidade_soldagem': 300,
        'sentido_rotacao': 'horaria',
        'direcao_soldagem': 'esquerda_direita',
        'lead_in': 10,
        'lead_out': 10,
        'afastamento_tocha': 5,
        'compact_gcode': False,
        'velocidade_a_mm_min': 150,
    }
    # Ajuste: usar o método correto de geração
    # E definir modo/tipo de oscilação conforme nova API
    params['welding_mode'] = 'oscilacao'
    params['oscillation_type'] = 'linear'
    gcode = gen.generate(params)
    lines = [l.strip() for l in gcode if l.strip()]
    # Conta quantas linhas G93 G01 A... existem
    rot_lines = [l for l in lines if l.startswith('G93 G01 A')]
    print('Linhas G93 G01 A encontradas:')
    for l in rot_lines:
        print(' ', l)
    print(f'Total: {len(rot_lines)} linhas.')
    # Esperamos apenas 2 (uma por camada)
    if len(rot_lines) == 2:
        print('✅ Sucesso: apenas uma rotação por camada!')
    else:
        print('❌ Ainda há múltiplas rotações por camada.')

if __name__ == '__main__':
    test_rotation_unique()
