#!/usr/bin/env python3
"""Test script to identify duplicate final pass issue"""

import sys
from pathlib import Path

# Permite importar TFM_GCODE do novo local src/app
project_root = Path(__file__).resolve().parent
sys.path.append(str(project_root / 'src' / 'app'))

from TFM_GCODE import GCodeGenerator

def test_duplicate_final_pass():
    """Test to identify if the final pass is duplicated"""
    
    # Create generator instance
    generator = GCodeGenerator()
    
    # Set up test parameters for linear oscillation
    test_params = {
        'diametro': 100.0,
        'comprimento_revestir': 50.0,
        'espessura_camada': 2.0,
        'num_camadas': 2,  # Only 2 layers to easily see the issue
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
        'compact_gcode': False,  # Start with non-compact mode
        'welding_mode': 'oscilacao_linear'
    }
    
    # Generate G-code
    gcode = generator.generate(test_params)
    
    # Save G-code to file for analysis
    with open('test_output.gcode', 'w') as f:
        for line in gcode:
            f.write(line + '\n')
    
    print("=== GENERATED G-CODE (first 50 lines) ===")
    for i, line in enumerate(gcode[:50]):
        print(f"{i+1:3d}: {line}")
    
    if len(gcode) > 50:
        print(f"... and {len(gcode) - 50} more lines")
    
    # Look for duplicate patterns
    print("\n=== ANALYSIS ===")
    
    # Count layers
    layer_count = 0
    layer_lines = []
    for i, line in enumerate(gcode):
        if "(--- CAMADA" in line:
            layer_count += 1
            layer_lines.append((i+1, line.strip()))
    
    print(f"Layers found:")
    for line_num, layer in layer_lines:
        print(f"  Line {line_num}: {layer}")
    
    print(f"\nTotal layers found: {layer_count}")
    print(f"Expected layers: {test_params['num_camadas']}")
    
    # Look for duplicate final positioning or movements
    z_moves = []
    for i, line in enumerate(gcode):
        if "G00 Z" in line and "Z50" not in line and "Z105" not in line and "Z107" not in line:  # Exclude safety moves
            z_moves.append((i+1, line.strip()))
    
    print(f"\nZ-axis work movements:")
    for line_num, z_move in z_moves:
        print(f"  Line {line_num}: {z_move}")
    
    # Look for M01 stops
    m01_stops = []
    for i, line in enumerate(gcode):
        if "M01" in line:
            m01_stops.append((i+1, line.strip()))
    
    print(f"\nM01 program stops found:")
    for line_num, m01 in m01_stops:
        print(f"  Line {line_num}: {m01}")
    
    # Look for lead-out movements
    lead_out_moves = []
    for i, line in enumerate(gcode):
        if "(Movimento lead-out)" in line:
            lead_out_moves.append((i+1, line.strip()))
    
    print(f"\nLead-out movements found:")
    for line_num, lead_out in lead_out_moves:
        print(f"  Line {line_num}: {lead_out}")
    
    # Check for duplicate patterns in the last few lines
    print(f"\n=== LAST 20 LINES ===")
    for i, line in enumerate(gcode[-20:], start=len(gcode)-19):
        print(f"{i:3d}: {line}")
    
    return gcode

if __name__ == "__main__":
    test_duplicate_final_pass()
