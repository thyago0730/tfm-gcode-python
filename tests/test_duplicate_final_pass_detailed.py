#!/usr/bin/env python3
"""Detailed test to identify duplicate final pass issue"""

import sys
from pathlib import Path

# Permite importar TFM_GCODE do novo local src/app
project_root = Path(__file__).resolve().parent
sys.path.append(str(project_root / 'src' / 'app'))

from TFM_GCODE import GCodeGenerator

def analyze_final_pass_patterns():
    """Analyze the final pass patterns to identify duplication"""
    
    generator = GCodeGenerator()
    
    # Test case that might trigger duplication
    params = {
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
        'lead_out': 5.0,
        'compact_gcode': False,
        'welding_mode': 'oscilacao_linear'
    }
    
    print("Generating G-code for detailed analysis...")
    gcode = generator.generate(params)
    
    # Find the final layer
    layer_starts = []
    axial_step_starts = []
    
    for i, line in enumerate(gcode):
        if "(--- CAMADA" in line:
            layer_num = int(line.split("CAMADA")[1].split("---")[0].strip())
            layer_starts.append((i, layer_num, line))
        elif "(--- PASSO AXIAL" in line:
            step_info = line.split("PASSO AXIAL")[1].split("---")[0].strip()
            step_num = int(step_info.split("/")[0])
            total_steps = int(step_info.split("/")[1])
            axial_step_starts.append((i, step_num, total_steps, line))
    
    print(f"\nFound {len(layer_starts)} layers and {len(axial_step_starts)} axial steps")
    
    # Analyze final layer
    if layer_starts:
        final_layer_num = layer_starts[-1][1]
        final_layer_start = layer_starts[-1][0]
        
        # Find final layer steps
        final_layer_steps = []
        for step_info in axial_step_starts:
            if step_info[0] > final_layer_start:
                final_layer_steps.append(step_info)
        
        print(f"Final layer {final_layer_num} has {len(final_layer_steps)} axial steps")
        
        # Extract final layer G-code
        final_layer_end = len(gcode)
        if len(layer_starts) > 1:
            final_layer_end = layer_starts[-1][0] + 100  # Look ahead 100 lines
        
        final_layer_gcode = gcode[final_layer_start:min(final_layer_end, len(gcode))]
        
        # Look for duplicate patterns
        movement_patterns = []
        current_pattern = []
        
        for line in final_layer_gcode:
            if "(--- PASSO AXIAL" in line:
                if current_pattern:
                    movement_patterns.append(current_pattern)
                current_pattern = [line]
            elif current_pattern:
                current_pattern.append(line)
                if len(current_pattern) > 50:  # Limit pattern size
                    movement_patterns.append(current_pattern)
                    current_pattern = []
        
        if current_pattern:
            movement_patterns.append(current_pattern)
        
        print(f"Found {len(movement_patterns)} movement patterns in final layer")
        
        # Check for duplicates
        if len(movement_patterns) >= 2:
            last_pattern = movement_patterns[-1]
            second_last_pattern = movement_patterns[-2]
            
            # Compare key elements
            last_g01_moves = [line for line in last_pattern if "G01" in line and "X" in line]
            second_last_g01_moves = [line for line in second_last_pattern if "G01" in line and "X" in line]
            
            print(f"\nLast pattern has {len(last_g01_moves)} G01 X moves")
            print(f"Second last pattern has {len(second_last_g01_moves)} G01 X moves")
            
            # Check if they have similar X positions
            if len(last_g01_moves) >= 2 and len(second_last_g01_moves) >= 2:
                last_x_positions = []
                second_last_x_positions = []
                
                for moves, positions in [(last_g01_moves, last_x_positions), (second_last_g01_moves, second_last_x_positions)]:
                    for move in moves:
                        if "X" in move:
                            try:
                                x_val = float(move.split("X")[1].split()[0])
                                positions.append(x_val)
                            except:
                                pass
                
                if last_x_positions and second_last_x_positions:
                    print(f"Last X positions: {last_x_positions}")
                    print(f"Second last X positions: {second_last_x_positions}")
                    
                    # Check if they're very similar (within 0.1mm)
                    if abs(last_x_positions[0] - second_last_x_positions[0]) < 0.1:
                        print("WARNING: Similar X positions detected - potential duplicate pass!")
                        
                        # Show the actual G-code around the suspected duplication
                        print("\n=== SUSPECTED DUPLICATE PASS G-CODE ===")
                        start_line = max(0, len(final_layer_gcode) - 100)
                        for i, line in enumerate(final_layer_gcode[start_line:], start_line + 1):
                            print(f"{i:3d}: {line}")
                            if i > start_line + 50:
                                break
                        
                        return True  # Duplicate detected
    
    return False  # No duplicate detected

def test_edge_case_conditions():
    """Test specific edge cases that might cause duplication"""
    
    edge_cases = [
        {
            'name': 'Very small oscillation length vs part length',
            'params': {
                'diametro': 50.0,
                'comprimento_revestir': 10.0,  # Small part
                'espessura_camada': 1.0,
                'num_camadas': 2,
                'velocidade_soldagem': 100.0,
                'afastamento_tocha': 5.0,
                'oscilacao_comprimento': 8.0,  # Large oscillation vs small part
                'largura_cordao': 2.0,
                'sobreposicao': 50.0,  # High overlap
                'deslocamento_angular_perc': 100.0,  # Full coverage
                'direcao_soldagem': 'esquerda_direita',
                'sentido_rotacao': 'horaria',
                'lead_in': 1.0,
                'lead_out': 1.0,
                'compact_gcode': False,
                'welding_mode': 'oscilacao_linear'
            }
        },
        {
            'name': 'Exact fit oscillation',
            'params': {
                'diametro': 100.0,
                'comprimento_revestir': 30.0,
                'espessura_camada': 2.0,
                'num_camadas': 2,
                'velocidade_soldagem': 100.0,
                'afastamento_tocha': 5.0,
                'oscilacao_comprimento': 10.0,
                'largura_cordao': 3.0,
                'sobreposicao': 33.33,  # Exact fit calculation
                'deslocamento_angular_perc': 50.0,
                'direcao_soldagem': 'esquerda_direita',
                'sentido_rotacao': 'horaria',
                'lead_in': 0.0,  # No lead-in
                'lead_out': 0.0,  # No lead-out
                'compact_gcode': False,
                'welding_mode': 'oscilacao_linear'
            }
        }
    ]
    
    generator = GCodeGenerator()
    
    for case in edge_cases:
        print(f"\n{'='*60}")
        print(f"TESTING EDGE CASE: {case['name']}")
        print(f"{'='*60}")
        
        try:
    gcode = generator.generate(case['params'])
            
            # Quick analysis
            layer_count = sum(1 for line in gcode if "(--- CAMADA" in line)
            axial_count = sum(1 for line in gcode if "(--- PASSO AXIAL" in line)
            
            print(f"Generated {len(gcode)} lines")
            print(f"Layers: {layer_count}, Expected: {case['params']['num_camadas']}")
            print(f"Total axial steps: {axial_count}")
            
            # Check final layer specifically
            duplicate_found = analyze_final_pass_patterns()
            if duplicate_found:
                print("DUPLICATE FINAL PASS DETECTED!")
            else:
                print("No obvious duplication found")
                
        except Exception as e:
            print(f"ERROR: {e}")
            import traceback
            traceback.print_exc()

if __name__ == "__main__":
    print("=== DETAILED DUPLICATE FINAL PASS ANALYSIS ===")
    
    # First run the main analysis
    duplicate_found = analyze_final_pass_patterns()
    
    if duplicate_found:
        print("\n🚨 DUPLICATE FINAL PASS CONFIRMED!")
    else:
        print("\n✅ No duplicate final pass detected in main test")
    
    # Then test edge cases
    print("\n\n=== TESTING EDGE CASE CONDITIONS ===")
    test_edge_case_conditions()
