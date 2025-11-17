#!/usr/bin/env python3
"""Test to identify issues in final axial step processing"""

import sys
from pathlib import Path

# Permite importar TFM_GCODE do novo local src/app
project_root = Path(__file__).resolve().parent
sys.path.append(str(project_root / 'src' / 'app'))

from TFM_GCODE import GCodeGenerator

def analyze_final_axial_step():
    """Analyze the final axial step for potential duplication issues"""
    
    generator = GCodeGenerator()
    
    # Test case that might trigger the issue
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
        'lead_out': 5.0,  # This might be the key parameter
        'compact_gcode': False,
        'welding_mode': 'oscilacao_linear'
    }
    
    print("Generating G-code for final axial step analysis...")
    gcode = generator.generate(params)
    
    # Find all axial steps in the final layer
    layer_starts = []
    axial_steps = []
    
    for i, line in enumerate(gcode):
        if "(--- CAMADA" in line:
            layer_num = int(line.split("CAMADA")[1].split("---")[0].strip())
            layer_starts.append((i, layer_num))
        elif "(--- PASSO AXIAL" in line:
            step_info = line.split("PASSO AXIAL")[1].split("---")[0].strip()
            step_num = int(step_info.split("/")[0])
            total_steps = int(step_info.split("/")[1])
            axial_steps.append((i, step_num, total_steps, line))
    
    if not layer_starts or not axial_steps:
        print("No layers or axial steps found!")
        return
    
    # Focus on final layer
    final_layer_start = layer_starts[-1][0]
    final_layer_steps = []
    
    for step_info in axial_steps:
        if step_info[0] > final_layer_start:
            final_layer_steps.append(step_info)
    
    print(f"Final layer has {len(final_layer_steps)} axial steps")
    
    # Analyze each step in final layer
    for i, (step_line_num, step_num, total_steps, step_header) in enumerate(final_layer_steps):
        print(f"\n--- Analyzing Step {step_num}/{total_steps} ---")
        
        # Find the end of this step (next step or end of file)
        next_step_start = len(gcode)
        if i < len(final_layer_steps) - 1:
            next_step_start = final_layer_steps[i + 1][0]
        
        step_gcode = gcode[step_line_num:next_step_start]
        
        # Look for movement patterns
        g00_moves = []
        g01_moves = []
        lead_out_moves = []
        
        for j, line in enumerate(step_gcode):
            if "G00 X" in line and "A" in line:
                g00_moves.append((j, line.strip()))
            elif "G01 X" in line:
                g01_moves.append((j, line.strip()))
            elif "Movimento lead-out" in line:
                lead_out_moves.append((j, line.strip()))
        
        print(f"  G00 moves: {len(g00_moves)}")
        print(f"  G01 moves: {len(g01_moves)}")
        print(f"  Lead-out moves: {len(lead_out_moves)}")
        
        # Check for suspicious patterns
        if len(g01_moves) > 20:  # Too many moves for one step
            print(f"  ⚠️  WARNING: Unusually high number of G01 moves ({len(g01_moves)})")
            
            # Look for duplicate X positions
            x_positions = []
            for _, move in g01_moves:
                try:
                    x_val = float(move.split("X")[1].split()[0])
                    x_positions.append(x_val)
                except:
                    pass
            
            if x_positions:
                # Check for exact duplicates
                seen_positions = set()
                duplicates = []
                for pos in x_positions:
                    if pos in seen_positions:
                        duplicates.append(pos)
                    seen_positions.add(pos)
                
                if duplicates:
                    print(f"  🚨 DUPLICATE X POSITIONS FOUND: {duplicates}")
                    
                    # Show the problematic section
                    print("\n  Problematic G-code section:")
                    start_idx = max(0, len(step_gcode) - 50)
                    for k, line in enumerate(step_gcode[start_idx:], start_idx):
                        print(f"    {k+step_line_num:3d}: {line}")
                        if k > start_idx + 30:
                            break
        
        # Special check for final step
        if step_num == total_steps:
            print(f"  🔍 This is the FINAL step - checking for double lead-out")
            
            # Look for multiple lead-out sections
            lead_out_count = len([line for line in step_gcode if "Movimento lead-out" in line])
            if lead_out_count > 1:
                print(f"  🚨 MULTIPLE LEAD-OUT SECTIONS: {lead_out_count}")
                
                # Show the entire final step
                print("\n  Complete final step G-code:")
                for k, line in enumerate(step_gcode):
                    print(f"    {k+step_line_num:3d}: {line}")
                    if k > 100:  # Limit output
                        print("    ... (truncated)")
                        break

def test_specific_edge_case():
    """Test a specific edge case that might trigger duplication"""
    
    generator = GCodeGenerator()
    
    # Edge case: very specific parameters that might cause issues
    params = {
        'diametro': 80.0,
        'comprimento_revestir': 25.0,  # Exact multiple of oscillation
        'espessura_camada': 1.5,
        'num_camadas': 1,  # Single layer - more likely to show issues
        'velocidade_soldagem': 80.0,
        'afastamento_tocha': 4.0,
        'oscilacao_comprimento': 8.33,  # Weird decimal
        'largura_cordao': 2.5,
        'sobreposicao': 33.33,  # Weird percentage
        'deslocamento_angular_perc': 66.67,  # Weird percentage
        'direcao_soldagem': 'direita_esquerda',  # Reverse direction
        'sentido_rotacao': 'anti_horaria',
        'lead_in': 2.5,
        'lead_out': 2.5,
        'compact_gcode': False,
        'welding_mode': 'oscilacao_linear'
    }
    
    print("\n" + "="*60)
    print("TESTING SPECIFIC EDGE CASE")
    print("="*60)
    
    gcode = generator.generate(params)
    
    # Quick analysis
    layer_count = sum(1 for line in gcode if "(--- CAMADA" in line)
    axial_count = sum(1 for line in gcode if "(--- PASSO AXIAL" in line)
    lead_out_count = sum(1 for line in gcode if "Movimento lead-out" in line)
    
    print(f"Layers: {layer_count}, Expected: {params['num_camadas']}")
    print(f"Axial steps: {axial_count}")
    print(f"Lead-out movements: {lead_out_count}")
    
    if lead_out_count > layer_count:
        print("🚨 MORE LEAD-OUTS THAN LAYERS - POTENTIAL ISSUE!")
        
        # Show the end of the G-code
        print("\nFinal 30 lines of G-code:")
        for i, line in enumerate(gcode[-30:], len(gcode) - 29):
            print(f"{i:3d}: {line}")

if __name__ == "__main__":
    print("=== FINAL AXIAL STEP ANALYSIS ===")
    analyze_final_axial_step()
    test_specific_edge_case()
