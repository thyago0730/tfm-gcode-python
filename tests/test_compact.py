#!/usr/bin/env python3
# Test script to verify compact mode for continuous oscillation

import sys, os
# Ajuste do sys.path para funcionar dentro da pasta tests
PROJECT_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
sys.path.insert(0, os.path.join(PROJECT_ROOT, 'src', 'app'))

from TFM_GCODE import GCodeGenerator

def test_compact_modes():
    """Test both compact and non-compact modes for square continuous oscillation"""
    
    # Create generator instance
    generator = GCodeGenerator()
    
    # Base parameters for square continuous oscillation
    base_params = {
        'afastamento_tocha': 12.0,
        'comprimento_revestir': 200.0,
        'direcao_soldagem': 'esquerda_direita',
        'sentido_rotacao': 'horaria',
        'lead_in': 5.0,
        'lead_out': 5.0,
        'velocidade_soldagem': 120.0,
        'oscilacao_comprimento': 10.0,
        'largura_cordao': 8.0,
        'deslocamento_angular_perc': 50.0,
        'sobreposicao': 40.0,
        'num_camadas': 1
    }
    
    layer_num = 0
    current_d = 50.0
    # dry run removido: geração sempre real
    
    # Test compact mode
    print("=== Testing COMPACT Mode ===")
    compact_params = base_params.copy()
    compact_params['compact_gcode'] = True
    
    try:
        compact_gcode_lines = generator._build_square_continuous_oscillation_segment(
            compact_params, layer_num, current_d
        )
        
        print(f"Total lines: {len(compact_gcode_lines)}")
        
        # Check for dwell commands (should be absent in compact mode)
        compact_dwell_count = sum(1 for line in compact_gcode_lines if "G04" in line)
        print(f"Dwell commands (G04) found: {compact_dwell_count}")
        print(f"Expected: 0 in compact mode")
        
        # Check for continuous motion
        compact_g01_count = sum(1 for line in compact_gcode_lines if "G01" in line)
        print(f"G01 motion commands: {compact_g01_count}")
        
        if compact_dwell_count == 0:
            print("✅ SUCCESS: No dwell commands found in compact mode!")
        else:
            print("❌ ISSUE: Dwell commands still present in compact mode")
            
    except Exception as e:
        print(f"❌ ERROR in compact mode: {e}")
        import traceback
        traceback.print_exc()
        return False
    
    # Test non-compact mode
    print("\n=== Testing NON-Compact Mode ===")
    non_compact_params = base_params.copy()
    non_compact_params['compact_gcode'] = False
    
    try:
        non_compact_gcode_lines = generator._build_square_continuous_oscillation_segment(
            non_compact_params, layer_num, current_d
        )
        
        print(f"Total lines: {len(non_compact_gcode_lines)}")
        
        # Check for dwell commands (should be present in non-compact mode)
        non_compact_dwell_count = sum(1 for line in non_compact_gcode_lines if "G04" in line)
        print(f"Dwell commands (G04) found: {non_compact_dwell_count}")
        print(f"Expected: > 0 in non-compact mode")
        
        # Check for continuous motion
        non_compact_g01_count = sum(1 for line in non_compact_gcode_lines if "G01" in line)
        print(f"G01 motion commands: {non_compact_g01_count}")
        
        if non_compact_dwell_count > 0:
            print("✅ SUCCESS: Dwell commands found in non-compact mode!")
        else:
            print("❌ ISSUE: No dwell commands found in non-compact mode")
            
    except Exception as e:
        print(f"❌ ERROR in non-compact mode: {e}")
        import traceback
        traceback.print_exc()
        return False
    
    # Summary
    print(f"\n=== Summary ===")
    print(f"Compact mode: {len(compact_gcode_lines)} lines, {compact_dwell_count} dwells")
    print(f"Non-compact mode: {len(non_compact_gcode_lines)} lines, {non_compact_dwell_count} dwells")
    
    success = (compact_dwell_count == 0 and non_compact_dwell_count > 0)
    return success

if __name__ == "__main__":
    print("Testing Compact Mode for Square Continuous Oscillation")
    print("=" * 60)
    success = test_compact_modes()
    print("=" * 60)
    if success:
        print("✅ Test PASSED: Compact mode working correctly!")
    else:
        print("❌ Test FAILED: Compact mode needs adjustment")
    sys.exit(0 if success else 1)
