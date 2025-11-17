#!/usr/bin/env python3
"""Test script to identify final layer duplication issue"""

import sys
from pathlib import Path

# Permite importar TFM_GCODE do novo local src/app
project_root = Path(__file__).resolve().parent
sys.path.append(str(project_root / 'src' / 'app'))

from TFM_GCODE import GCodeGenerator

def test_final_layer_edge_cases():
    """Test various edge cases that might cause final pass duplication"""
    
    generator = GCodeGenerator()
    
    test_cases = [
        {
            'name': 'Normal 1 layer',
            'params': {
                'diametro': 100.0,
                'comprimento_revestir': 50.0,
                'espessura_camada': 2.0,
                'num_camadas': 1,  # Single layer - potential issue?
                'velocidade_soldagem': 100.0,
                'afastamento_tocha': 5.0,
                'oscilacao_comprimento': 10.0,
                'largura_cordao': 3.0,
                'sobreposicao': 30.0,
                'deslocamento_angular_perc': 50.0,
                'direcao_soldagem': 'esquerda_direita',
                'sentido_rotacao': 'horaria',
                'lead_in': 5.0,
                'lead_out': 5.0,
                'compact_gcode': False,
                'welding_mode': 'oscilacao_linear'
            }
        },
        {
            'name': 'Zero lead-out',
            'params': {
                'diametro': 100.0,
                'comprimento_revestir': 50.0,
                'espessura_camada': 2.0,
                'num_camadas': 2,
                'velocidade_soldagem': 100.0,
                'afastamento_tocha': 5.0,
                'oscilacao_comprimento': 10.0,
                'largura_cordao': 3.0,
                'sobreposicao': 30.0,
                'deslocamento_angular_perc': 50.0,
                'direcao_soldagem': 'esquerda_direita',
                'sentido_rotacao': 'horaria',
                'lead_in': 5.0,
                'lead_out': 0.0,  # Zero lead-out - potential issue?
                'compact_gcode': False,
                'welding_mode': 'oscilacao_linear'
            }
        },
        {
            'name': 'Very small part length',
            'params': {
                'diametro': 100.0,
                'comprimento_revestir': 5.0,  # Very small - potential issue?
                'espessura_camada': 2.0,
                'num_camadas': 2,
                'velocidade_soldagem': 100.0,
                'afastamento_tocha': 5.0,
                'oscilacao_comprimento': 10.0,  # Larger than part!
                'largura_cordao': 3.0,
                'sobreposicao': 30.0,
                'deslocamento_angular_perc': 50.0,
                'direcao_soldagem': 'esquerda_direita',
                'sentido_rotacao': 'horaria',
                'lead_in': 1.0,
                'lead_out': 1.0,
                'compact_gcode': False,
                'welding_mode': 'oscilacao_linear'
            }
        }
    ]
    
    for test_case in test_cases:
        print(f"\n{'='*60}")
        print(f"TESTING: {test_case['name']}")
        print(f"{'='*60}")
        
        try:
    gcode = generator.generate(test_case['params'])
            
            # Analyze the G-code
            layer_count = sum(1 for line in gcode if "(--- CAMADA" in line)
            axial_step_count = sum(1 for line in gcode if "(--- PASSO AXIAL" in line)
            lead_out_count = sum(1 for line in gcode if "(Movimento lead-out)" in line)
            m01_count = sum(1 for line in gcode if "M01" in line and line.strip() != "")
            
            print(f"Generated {len(gcode)} lines of G-code")
            print(f"Layers found: {layer_count}")
            print(f"Expected layers: {test_case['params']['num_camadas']}")
            print(f"Axial steps: {axial_step_count}")
            print(f"Lead-out movements: {lead_out_count}")
            print(f"M01 stops: {m01_count}")
            
            # Check for potential issues
            issues = []
            
            if layer_count != test_case['params']['num_camadas']:
                issues.append(f"Layer count mismatch: expected {test_case['params']['num_camadas']}, got {layer_count}")
            
            if lead_out_count != layer_count:
                issues.append(f"Lead-out count mismatch: expected {layer_count}, got {lead_out_count}")
            
            if m01_count != max(0, layer_count - 1):
                issues.append(f"M01 count mismatch: expected {max(0, layer_count - 1)}, got {m01_count}")
            
            # Check final positioning
            final_z_moves = [line for line in gcode[-10:] if "G00 Z" in line]
            if final_z_moves:
                print(f"Final Z moves: {final_z_moves}")
            
            # Check for duplicate patterns in final layer
            final_layer_lines = []
            in_final_layer = False
            for line in gcode:
                if "(--- CAMADA" in line:
                    if f"CAMADA {layer_count}" in line:
                        in_final_layer = True
                        final_layer_lines = [line]
                    else:
                        in_final_layer = False
                elif in_final_layer:
                    final_layer_lines.append(line)
            
            if final_layer_lines:
                final_axial_steps = sum(1 for line in final_layer_lines if "(--- PASSO AXIAL" in line)
                print(f"Final layer axial steps: {final_axial_steps}")
                
                # Check if the last axial step appears twice
                last_axial_step = None
                step_pattern = []
                for line in final_layer_lines:
                    if "(--- PASSO AXIAL" in line:
                        step_pattern.append(line.strip())
                
                if len(step_pattern) > 1:
                    last_step = step_pattern[-1]
                    if step_pattern.count(last_step) > 1:
                        issues.append(f"Last axial step '{last_step}' appears {step_pattern.count(last_step)} times")
            
            if issues:
                print(f"ISSUES FOUND:")
                for issue in issues:
                    print(f"  - {issue}")
            else:
                print("No obvious issues found")
                
        except Exception as e:
            print(f"ERROR generating G-code: {e}")
            import traceback
            traceback.print_exc()

if __name__ == "__main__":
    test_final_layer_edge_cases()
