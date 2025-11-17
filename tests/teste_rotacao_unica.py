#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Teste rápido para verificar se a rotação única por passe está funcionando.
Gera um G-code simples e conta quantas vezes 'G93 G01 A' aparece por camada.
"""

import sys
import os
# Ajuste do sys.path para funcionar dentro da pasta tests
PROJECT_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
sys.path.insert(0, os.path.join(PROJECT_ROOT, 'src', 'app'))

from TFM_GCODE import GCodeGenerator

def test_rotacao_unica():
    print("=== Teste: Rotação Única por Passe ===")
    
    # Parâmetros de teste
    params = {
        'welding_mode': 'espiral',
        'nome_procedimento': 'TESTE_ROTACAO_UNICA',
        'diametro': 100.0,
        'comprimento_revestir': 200.0,
        'direcao_soldagem': 'esquerda_direita',
        'sentido_rotacao': 'horaria',
        'num_camadas': 3,
        'espessura_camada': 2.0,
        'largura_cordao': 5.0,
        'sobreposicao': 20.0,
        'velocidade_soldagem': 300.0,
        'velocidade_a_mm_min': 200.0,
        'afastamento_tocha': 5.0,
        'lead_in': 10.0,
        'lead_out': 10.0,
        'oscillation_type': 'linear',
        'oscilacao_comprimento': 15.0,
        'deslocamento_angular_perc': 25.0,
        'compact_gcode': False
    }
    
    generator = GCodeGenerator()
    
    # Testar modo espiral
    print("\n1. Testando MODO ESPIRAL...")
    params['welding_mode'] = 'espiral'
    gcode_espiral = generator.generate(params, is_dry_run=True)
    
    if gcode_espiral:
        linhas = gcode_espiral
        camada_atual = 0
        rotacoes_por_camada = {}
        
        for linha in linhas:
            if '(--- CAMADA' in linha:
                camada_atual += 1
                rotacoes_por_camada[camada_atual] = 0
            elif 'G93 G01 A' in linha:
                if camada_atual not in rotacoes_por_camada:
                    rotacoes_por_camada[camada_atual] = 0
                rotacoes_por_camada[camada_atual] += 1
        
        print(f"   Camadas detectadas: {camada_atual}")
        print(f"   Rotações por camada: {rotacoes_por_camada}")
        
        # Verificar se temos exatamente 1 rotação por camada
        ok_espiral = all(count == 1 for count in rotacoes_por_camada.values())
        print(f"   ✓ Rotação única por camada: {'SIM' if ok_espiral else 'NÃO'}")
        
        if not ok_espiral:
            print("   ⚠️  Detalhes das rotações encontradas:")
            for camada, count in rotacoes_por_camada.items():
                print(f"      Camada {camada}: {count} rotação(ões)")
    else:
        print("   ✗ Erro ao gerar G-code espiral")
    
    # Testar modo oscilação linear
    print("\n2. Testando MODO OSCILAÇÃO LINEAR...")
    params['welding_mode'] = 'oscilacao'
    params['oscillation_type'] = 'linear'
    gcode_linear = generator.generate(params, is_dry_run=True)
    
    if gcode_linear:
        linhas = gcode_linear
        camada_atual = 0
        rotacoes_por_camada = {}
        
        for linha in linhas:
            if '(--- CAMADA' in linha:
                camada_atual += 1
                rotacoes_por_camada[camada_atual] = 0
            elif 'G93 G01 A' in linha:
                if camada_atual not in rotacoes_por_camada:
                    rotacoes_por_camada[camada_atual] = 0
                rotacoes_por_camada[camada_atual] += 1
        
        print(f"   Camadas detectadas: {camada_atual}")
        print(f"   Rotações por camada: {rotacoes_por_camada}")
        
        ok_linear = all(count == 1 for count in rotacoes_por_camada.values())
        print(f"   ✓ Rotação única por camada: {'SIM' if ok_linear else 'NÃO'}")
        
        if not ok_linear:
            print("   ⚠️  Detalhes das rotações encontradas:")
            for camada, count in rotacoes_por_camada.items():
                print(f"      Camada {camada}: {count} rotação(ões)")
    else:
        print("   ✗ Erro ao gerar G-code oscilação linear")
    
    # Testar modo oscilação quadrada
    print("\n3. Testando MODO OSCILAÇÃO QUADRADA...")
    params['oscillation_type'] = 'quadrada'
    gcode_quadrada = generator.generate(params, is_dry_run=True)
    
    if gcode_quadrada:
        linhas = gcode_quadrada
        camada_atual = 0
        rotacoes_por_camada = {}
        
        for linha in linhas:
            if '(--- CAMADA' in linha:
                camada_atual += 1
                rotacoes_por_camada[camada_atual] = 0
            elif 'G93 G01 A' in linha:
                if camada_atual not in rotacoes_por_camada:
                    rotacoes_por_camada[camada_atual] = 0
                rotacoes_por_camada[camada_atual] += 1
        
        print(f"   Camadas detectadas: {camada_atual}")
        print(f"   Rotações por camada: {rotacoes_por_camada}")
        
        ok_quadrada = all(count == 1 for count in rotacoes_por_camada.values())
        print(f"   ✓ Rotação única por camada: {'SIM' if ok_quadrada else 'NÃO'}")
        
        if not ok_quadrada:
            print("   ⚠️  Detalhes das rotações encontradas:")
            for camada, count in rotacoes_por_camada.items():
                print(f"      Camada {camada}: {count} rotação(ões)")
    else:
        print("   ✗ Erro ao gerar G-code oscilação quadrada")
    
    # Testar modo oscilação quadrada contínua
    print("\n4. Testando MODO OSCILAÇÃO QUADRADA CONTÍNUA...")
    params['oscillation_type'] = 'quadrada_continua'
    gcode_quad_cont = generator.generate(params, is_dry_run=True)
    
    if gcode_quad_cont:
        linhas = gcode_quad_cont
        camada_atual = 0
        rotacoes_por_camada = {}
        
        for linha in linhas:
            if '(--- CAMADA' in linha:
                camada_atual += 1
                rotacoes_por_camada[camada_atual] = 0
            elif 'G93 G01 A' in linha:
                if camada_atual not in rotacoes_por_camada:
                    rotacoes_por_camada[camada_atual] = 0
                rotacoes_por_camada[camada_atual] += 1
        
        print(f"   Camadas detectadas: {camada_atual}")
        print(f"   Rotações por camada: {rotacoes_por_camada}")
        
        ok_quad_cont = all(count == 1 for count in rotacoes_por_camada.values())
        print(f"   ✓ Rotação única por camada: {'SIM' if ok_quad_cont else 'NÃO'}")
        
        if not ok_quad_cont:
            print("   ⚠️  Detalhes das rotações encontradas:")
            for camada, count in rotacoes_por_camada.items():
                print(f"      Camada {camada}: {count} rotação(ões)")
    else:
        print("   ✗ Erro ao gerar G-code oscilação quadrada contínua")
    
    print("\n=== RESUMO ===")
    print("Espiral:", '✓' if ok_espiral else '✗')
    print("Linear:", '✓' if ok_linear else '✗')
    print("Quadrada:", '✓' if ok_quadrada else '✗')
    print("Quadrada Contínua:", '✓' if ok_quad_cont else '✗')
    
    # Salvar exemplo de G-code para inspeção
    print("\n Salvando exemplo de G-code para inspeção...")
    with open('teste_espiral.nc', 'w', encoding='utf-8') as f:
        f.write('\n'.join(gcode_espiral))
    with open('teste_linear.nc', 'w', encoding='utf-8') as f:
        f.write('\n'.join(gcode_linear))
    print(" Arquivos salvos: teste_espiral.nc, teste_linear.nc")

if __name__ == "__main__":
    test_rotacao_unica()
