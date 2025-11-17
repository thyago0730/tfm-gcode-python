#!/usr/bin/env python3
"""Teste rápido do modo spindle na oscilação quadrada contínua."""

import sys
import os
# Garantir import do app a partir da raiz do projeto
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'src', 'app'))

from TFM_GCODE import GCodeGenerator

def test_spindle_mode():
    generator = GCodeGenerator()
    
    params = {
        'diametro': 200.0,  # Diâmetro atual da camada
        'comprimento_revestir': 100.0,
        'largura_cordao': 10.0,
        'espessura_camada': 2.0,
        'num_camadas': 1,
        'velocidade_soldagem': 120.0,
        'oscilacao_comprimento': 15.0,
        'oscilacao_largura': 8.0,
        'oscilacao_frequencia': 1.0,
        'afastamento_tocha': 5.0,
        'deslocamento_angular_perc': 50.0,
        'sobreposicao': 30.0,
        'direcao_soldagem': 'esquerda_direita',
        'sentido_rotacao': 'horaria',
        'lead_in': 5.0,
        'lead_out': 5.0,
        'velocidade_a_mm_min': 197.080,  # Para ~157 RPM com diâmetro 200 mm
        'rotation_control': 'spindle',  # Modo spindle
        'compact_gcode': False,
        'welding_mode': 'oscilacao',
        'oscillation_type': 'quadrada_continua'
    }
    
    try:
        gcode = generator.generate(params)
        print("✓ G-code gerado com sucesso!")
        print("\nPrimeiras 30 linhas do G-code:")
        for i, line in enumerate(gcode[:30]):
            print(f"{i+1:2d}: {line}")
        
        # Verificar se M3 aparece
        m3_found = any('M3 S' in line for line in gcode)
        m5_found = any('M5' in line for line in gcode)
        
        print(f"\nM3 S encontrado: {'✓' if m3_found else '✗'}")
        print(f"M5 encontrado: {'✓' if m5_found else '✗'}")
        
        # Salvar arquivo para inspeção
        with open('teste_spindle_quadrada_continua.nc', 'w') as f:
            f.write('\n'.join(gcode))
        print("\nArquivo salvo: teste_spindle_quadrada_continua.nc")
        
    except Exception as e:
        print(f"✗ Erro: {e}")
        import traceback
        traceback.print_exc()

if __name__ == '__main__':
    test_spindle_mode()
