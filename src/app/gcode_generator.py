# -*- coding: utf-8 -*-
"""Geração de G-code (independente de UI).

Mantido separado de TFM_GCODE.py para poder ser importado e testado sem Tkinter.
"""
import math
from datetime import datetime


class GCodeGenerator:
    def _scurve_fractions(self, n_steps: int):
        # Retorna frações [0..1] com perfil S-curve (ease-in-out) usando coseno
        # Inclui apenas pontos de avanço (exclui 0.0), inclui 1.0
        n = max(int(n_steps), 2)
        fracs = []
        for i in range(1, n + 1):
            t = i / n
            s = 0.5 - 0.5 * math.cos(math.pi * t)  # ease-in-out
            fracs.append(s)
        return fracs
    def _normalize_params(self, params: dict):
        # Compatibilidade com presets/testes antigos e novo nome
        # Mapear velocidade_soldagem (antigo) -> velocidade_de_deposicao
        if 'velocidade_de_deposicao' not in params and 'velocidade_soldagem' in params:
            params['velocidade_de_deposicao'] = params['velocidade_soldagem']
        # Bidirecional: garantir que ambas as chaves existam
        if 'taxa_de_deposicao' in params and 'velocidade_de_deposicao' not in params:
            params['velocidade_de_deposicao'] = params['taxa_de_deposicao']
        if 'velocidade_de_deposicao' in params and 'taxa_de_deposicao' not in params:
            params['taxa_de_deposicao'] = params['velocidade_de_deposicao']
        # Preencher velocidade_a_mm_min a partir da velocidade de deposição
        if 'velocidade_a_mm_min' not in params and 'velocidade_de_deposicao' in params:
            params['velocidade_a_mm_min'] = params['velocidade_de_deposicao']

    def generate(self, params):
        # Normaliza parâmetros antes de gerar
        if params is not None:
            self._normalize_params(params)
        if params is None: return None
        mode = params.get('welding_mode', 'espiral')
        if mode == 'espiral':
            return self._generate_spiral(params)
        elif mode == 'oscilacao':
            # Método unificado: escolhe entre linear e quadrada via parâmetro
            osc_type = params.get('oscillation_type', 'linear')
            if osc_type == 'quadrada':
                return self._generate_square_oscillation(params)
            elif osc_type == 'quadrada_continua':
                # A operação "Quadrada Teste" foi renomeada para "Quadrada Contínua"
                return self._generate_square_test_oscillation(params)
            else:
                return self._generate_linear_oscillation(params)
        elif mode == 'oscilacao_linear':
            # Compatibilidade retroativa com presets antigos
            return self._generate_linear_oscillation(params)
        elif mode == 'oscilacao_quadrada':
            # Compatibilidade retroativa com presets antigos
            return self._generate_square_oscillation(params)
        return None

    def _build_header(self, params):
        mode = params.get('welding_mode', 'espiral')
        header = ["%", f"(G-CODE GERADO PELO TFM G-CODE)", f"(Data: {datetime.now().strftime('%d/%m/%Y %H:%M:%S')})", "(----------------------------------------)", f"( PROCEDIMENTO: {params.get('nome_procedimento', 'N/A')} )", f"( MODO DE SOLDAGEM: {mode.upper()} )"]
        header.extend([f"( DIAMETRO: {params['diametro']:.3f} mm)", f"( COMPRIMENTO: {params['comprimento_revestir']:.3f} mm )", f"( SENTIDO: {params.get('direcao_soldagem', 'esquerda_direita')} )"])
        rot = params.get('sentido_rotacao', 'horaria')
        header.append(f"( SENTIDO ROTACAO: {rot.upper()} )")
        header.extend([f"( CAMADAS: {params['num_camadas']} de {params['espessura_camada']:.2f}mm )"])
        if mode in ('oscilacao','oscilacao_linear','oscilacao_quadrada'):
             osc_type = params.get('oscillation_type')
             if osc_type == 'linear': tipo_txt = 'LINEAR'
             elif osc_type == 'quadrada': tipo_txt = 'QUADRADA'
             elif osc_type == 'quadrada_continua': tipo_txt = 'QUADRADA CONTINUA'
             else: tipo_txt = None
             osc_label = f" ( OSCILACAO: {tipo_txt if tipo_txt else 'ATIVADA'} )"
             header.extend([osc_label.strip(), f"(   - COMPRIMENTO OSC.: {params.get('oscilacao_comprimento', 0.0):.2f} mm)", f"(   - DESLOC. ANGULAR: {params.get('deslocamento_angular_perc', 0.0):.1f}% Larg. Cordao)"])
        header.extend([f"( LEAD-IN: {params.get('lead_in', 0.0):.2f} mm, LEAD-OUT: {params.get('lead_out', 0.0):.2f} mm )"])
        header.extend(["(----------------------------------------)", "G21 G90 G94", ""]) 
        # Sequencia de preparação da tocha: opcionalmente recuar Z&Y ao zero maquina antes de ligar
        header.append("(PREPARO DA TOCHA)")
        if bool(params.get('torch_retract_on_ignite', True)):
            header.append("G53 G0 Z0 Y0 (Ir ao Z&Y=0 maquina)")
        header.extend([
            "M101 (LIGAR Output#1 - tocha)",
            "M0 (Confirme no Mach3 e pressione Cycle Start)",
            "G90 G0 Y0 (Ir ao Y=0 peca)",
            ""
        ])
        return header

    def _build_footer(self):
        # Retorno ao zero maquina: mover Z&Y juntos e desligar tocha, depois X&Y juntos
        return [
            "",
            "(RETORNO AO ZERO MAQUINA)",
            "G90",
            "G53 G0 Z0 Y0 (Ir ao Z&Y=0 maquina)",
            "M102 (DESLIGAR Output#1 - tocha)",
            "G53 G0 X0 Y0 (Ir ao X&Y=0 maquina)",
            "M30 (FIM DO PROGRAMA)",
            "%"
        ]

    def _generate_spiral(self, params):
        gcode_body = []
        current_d = params['diametro']
        for i in range(params['num_camadas']):
            gcode_body.extend(self._build_spiral_segment(params, i, current_d))
            current_d += 2 * params['espessura_camada']
        return self._build_header(params) + gcode_body + self._build_footer()

    def _build_spiral_segment(self, params, layer_num, current_d):
        # Normaliza para chamadas diretas em testes
        self._normalize_params(params)
        afastamento = params['afastamento_tocha']; z_offset = 0.0
        z_layer = current_d / 2.0 + afastamento + z_offset
        z_seguranca = z_layer + 30.0
        part_length = params['comprimento_revestir']; direction = params.get('direcao_soldagem', 'esquerda_direita')
        rotation_dir = params.get('sentido_rotacao', 'horaria')
        # Controle de rotação fixo em eixo A
        lead_in = params.get('lead_in', 0.0); lead_out = params.get('lead_out', 0.0)
        taxa_de_deposicao = params['taxa_de_deposicao']
        if direction == 'esquerda_direita': x_arc_start_calc = 0.0 - lead_in; x_arc_end_calc = part_length + lead_out
        else: x_arc_start_calc = part_length + lead_out; x_arc_end_calc = 0.0 - lead_in
        comprimento_total_arc = abs(x_arc_end_calc - x_arc_start_calc)
        passo = params['largura_cordao'] * (1.0 - (params['sobreposicao'] / 100.0)); passo = max(passo, 1e-6)
        total_rotacoes = (comprimento_total_arc / passo)
        rotation_sign = 1.0 if rotation_dir == 'horaria' else -1.0
        angulo_total_A = total_rotacoes * 360.0 * rotation_sign
        # Calcula feed angular baseado na velocidade A separada
        circunferencia = math.pi * current_d
        # RPM derivado diretamente: 1/((D*pi)/taxa) = taxa/(D*pi)
        rpm_a = (taxa_de_deposicao / circunferencia) if circunferencia > 0 and taxa_de_deposicao > 0 else 0.0
        # Removido suporte a spindle e RPM alvo; usar RPM derivado
        rpm_real = rpm_a
        # Feed rate = taxa de deposição * (1/RPM)
        feed_linear = (taxa_de_deposicao / rpm_real) if rpm_real > 0 else taxa_de_deposicao
        feed_angular = rpm_real * 360.0
        
        compact = bool(params.get('compact_gcode', False))
        n_scurve = int(params.get('n_scurve_steps', 6) or 6)
        segment_gcode = [f"(--- CAMADA {layer_num + 1} ---)", f"(SENTIDO: {direction})", f"G00 Z{z_seguranca:.3f}", f"G00 X{x_arc_start_calc:.3f} A0", f"G01 F{feed_linear*2:.1f} Z{z_layer:.3f}"]
        # Removido ramp-up de lead-in: rotação inicial com X parado conforme solicitado
        # ---------- VOLTA COMPLETA INICIAL NO DIÂMETRO ----------
        if compact:
            segment_gcode.append(f"G01 F{feed_angular:.1f} A{(rotation_sign*360.0):.3f}")
        else:
            # Rotação inicial com rampa S-curve
            target_A_init = rotation_sign * 360.0
            for f in self._scurve_fractions(n_scurve):
                a_pos = target_A_init * f
                segment_gcode.append(f"G01 F{feed_angular:.1f} A{a_pos:.3f}")
        # ---------- PASSE HELICOIDAL: X e A simultâneos ----------
        # Move de X inicial até X final enquanto rotaciona o A pelo ângulo total do passe
        # Alvo acumulado: rotação do passe (já com sinal) + 1 volta inicial
        if compact:
            segment_gcode.append(f"G01 F{feed_linear:.1f} X{x_arc_end_calc:.3f} A{(angulo_total_A + rotation_sign*360.0):.3f}")
        else:
            # Passo helicoidal com rampa S-curve em X e A
            start_X = x_arc_start_calc
            end_X = x_arc_end_calc
            start_A = rotation_sign * 360.0
            end_A = angulo_total_A + rotation_sign * 360.0
            for f in self._scurve_fractions(n_scurve):
                x_pos = start_X + (end_X - start_X) * f
                a_pos = start_A + (end_A - start_A) * f
                segment_gcode.append(f"G01 F{feed_linear:.1f} X{x_pos:.3f} A{a_pos:.3f}")
        # ---------- VOLTA COMPLETA FINAL NO DIÂMETRO ----------
        if compact:
            segment_gcode.append(f"G01 F{feed_angular:.1f} A{(angulo_total_A + rotation_sign*720.0):.3f}")
        else:
            # Rotação final com rampa S-curve
            start_A2 = angulo_total_A + rotation_sign * 360.0
            end_A2 = angulo_total_A + rotation_sign * 720.0
            for f in self._scurve_fractions(n_scurve):
                a_pos = start_A2 + (end_A2 - start_A2) * f
                segment_gcode.append(f"G01 F{feed_angular:.1f} A{a_pos:.3f}")
        # Ramp down no lead-out (se houver)
        if abs(lead_out) > 1e-6:
            back_mid1 = x_arc_end_calc - (x_arc_end_calc - x_arc_start_calc) * 0.3
            segment_gcode.append(f"G01 F{feed_linear*0.85:.1f} X{back_mid1:.3f}")
            
        segment_gcode.extend([f"G00 Z{z_seguranca:.3f}", "M01" if layer_num < params['num_camadas']-1 else ""]) 
        return segment_gcode

    def _generate_linear_oscillation(self, params):
        gcode_body = []
        current_d = params['diametro']
        for i in range(params['num_camadas']):
            gcode_body.extend(self._build_linear_oscillation_segment(params, i, current_d))
            current_d += 2 * params['espessura_camada']
        return self._build_header(params) + gcode_body + self._build_footer()

    def _build_linear_oscillation_segment(self, params, layer_num, current_d):
        # Normaliza para chamadas diretas em testes
        self._normalize_params(params)
        afastamento = params['afastamento_tocha']; z_offset = 0.0
        z_layer = current_d / 2.0 + afastamento + z_offset
        z_seguranca = z_layer + 30.0
        part_length = params['comprimento_revestir']
        direction = params.get('direcao_soldagem', 'esquerda_direita')
        rotation_dir = params.get('sentido_rotacao', 'horaria')
        lead_in = params.get('lead_in', 0.0); lead_out = params.get('lead_out', 0.0)
        taxa_de_deposicao = params['taxa_de_deposicao']
        # Controle de rotação: sempre eixo A
        osc_length = params['oscilacao_comprimento']
        larg_cordao = params['largura_cordao']
        desloc_perc = params['deslocamento_angular_perc']
        sobreposicao = params['sobreposicao']
        compact = bool(params.get('compact_gcode', False))

        if direction == 'esquerda_direita': x_start_revest = 0.0 - lead_in; x_end_revest = part_length + lead_out
        else: x_start_revest = part_length + lead_out; x_end_revest = 0.0 - lead_in
        actual_passo_axial = osc_length * (sobreposicao / 100.0); actual_passo_axial = max(actual_passo_axial, 1e-6)
        num_passos_axiais = math.ceil(part_length / actual_passo_axial) if part_length > 0 else 1
        circunferencia = math.pi * current_d
        desloc_linear_angular = (desloc_perc / 100.0) * larg_cordao
        if circunferencia <= 0 or desloc_linear_angular <= 0: return ["(ERRO: Diametro ou deslocamento invalido para calculo angular)"]
        num_passos_angulares_por_volta = math.ceil(circunferencia / desloc_linear_angular)
        actual_delta_A_deg = 360.0 / num_passos_angulares_por_volta
        rotation_sign = 1.0 if rotation_dir == 'horaria' else -1.0
        # RPM derivado diretamente: taxa/(D*pi)
        rpm_a = (taxa_de_deposicao / circunferencia) if circunferencia > 0 and taxa_de_deposicao > 0 else 0
        rpm_real = rpm_a
        feed_angular = rpm_real * 360.0
        # Feed X definido pela velocidade da oscilação (mm/min)
        feed_linear = float(params.get('velocidade_oscilacao_mm_min', taxa_de_deposicao) or taxa_de_deposicao)
        
        segment_gcode = [f"(--- CAMADA {layer_num + 1} ---)", f"(SENTIDO: {direction})", f"G00 Z{z_seguranca:.3f}"]
        # Stagger angular intercamadas para melhor repartição
        current_A_total = (layer_num * (actual_delta_A_deg * 0.25)) * (1.0 if rotation_dir == 'horaria' else -1.0)
        n_scurve = int(params.get('n_scurve_steps', 6) or 6)

        for passo_idx in range(num_passos_axiais):
            if direction == 'esquerda_direita':
                x_start_anel = 0.0 + passo_idx * actual_passo_axial
                x_osc_ida = x_start_anel + osc_length; x_osc_volta = x_start_anel
            else:
                x_start_anel = part_length - passo_idx * actual_passo_axial
                x_osc_ida = x_start_anel - osc_length; x_osc_volta = x_start_anel
            
            x_osc_ida = max(0.0, min(part_length, x_osc_ida))
            x_osc_volta = max(0.0, min(part_length, x_osc_volta)) 

            current_x_start_pos = x_osc_volta
            if passo_idx == 0: current_x_start_pos = x_start_revest
            current_x_end_pos_final_volta = x_osc_volta
            if passo_idx == num_passos_axiais -1: current_x_end_pos_final_volta = x_end_revest
            segment_gcode.append(f"(--- PASSO AXIAL {passo_idx + 1}/{num_passos_axiais} ---)")
            segment_gcode.append(f"G00 X{current_x_start_pos:.3f} A{current_A_total:.3f}") # Modificado para A contínuo
            segment_gcode.append(f"G01 F{feed_linear*2:.1f} Z{z_layer:.3f}")
            if passo_idx == 0:
                 if not compact:
                     pass
                 # ---------- ROTAÇÃO ÚNICA POR PASSE (com rampa S-curve no modo detailed) ----------
                 target_A_total = current_A_total + rotation_sign * actual_delta_A_deg * num_passos_angulares_por_volta * num_passos_axiais
                 if compact:
                     segment_gcode.append(f"G01 F{feed_angular:.1f} A{target_A_total:.3f}")
                 else:
                     for f in self._scurve_fractions(n_scurve):
                         a_pos = current_A_total + (target_A_total - current_A_total) * f
                         segment_gcode.append(f"G01 F{feed_angular:.1f} A{a_pos:.3f}")
                 # (G94 já ativo no cabeçalho)
                 if abs(lead_in) > 1e-6:
                     # Ramp up no lead-in até a posição inicial de oscilação
                     mid1 = current_x_start_pos + (x_osc_volta - current_x_start_pos) * 0.3
                     mid2 = current_x_start_pos + (x_osc_volta - current_x_start_pos) * 0.7
                     segment_gcode.append(f"G01 F{feed_linear*0.6:.1f} X{mid1:.3f}")
                     segment_gcode.append(f"G01 F{feed_linear*0.85:.1f} X{mid2:.3f}")
                     segment_gcode.append(f"G01 F{feed_linear:.1f} X{x_osc_volta:.3f}")
            
            # Handle final axial step differently to prevent duplication
            if passo_idx == num_passos_axiais - 1:
                # Passo final: apenas move até o fim do revestimento se ainda houver distância
                dist_ate_fim = abs(x_end_revest - current_x_start_pos)
                if dist_ate_fim > 1e-6:
                    segment_gcode.append(f"(Fechamento ate fim do revestimento)")
                    segment_gcode.append(f"G01 F{feed_linear:.1f} X{x_end_revest:.3f}")
                # Rotação única já foi emitida – não repetir "A"
            else:
                # Normal oscillation for non-final steps
                for k in range(num_passos_angulares_por_volta):
                    current_x_osc_ida = x_osc_ida
                    current_x_osc_volta = x_osc_volta
                    segment_gcode.append(f"(Passo angular {k+1}/{num_passos_angulares_por_volta})")
                    
                    if compact:
                        # Modo compacto: movimento contínuo sem paradas
                        # Ida contínua - uma linha com velocidade constante
                        segment_gcode.append(f"G01 F{feed_linear:.1f} X{current_x_osc_ida:.3f}")
                        # Volta contínua - uma linha com velocidade constante
                        segment_gcode.append(f"G01 F{feed_linear:.1f} X{current_x_osc_volta:.3f}")
                    else:
                        # Perfil S-curve ida
                        for f in self._scurve_fractions(n_scurve):
                            x_pos = current_x_osc_volta + (current_x_osc_ida - current_x_osc_volta) * f
                            segment_gcode.append(f"G01 F{feed_linear:.1f} X{x_pos:.3f}")
                        # Perfil S-curve volta
                        for f in self._scurve_fractions(n_scurve):
                            x_pos = current_x_osc_ida + (current_x_osc_volta - current_x_osc_ida) * f
                            segment_gcode.append(f"G01 F{feed_linear:.1f} X{x_pos:.3f}")
                        

                    current_A_total += rotation_sign * actual_delta_A_deg
                    # Rotação já foi emitida no início – não repetir "A" aqui 

            if passo_idx < num_passos_axiais - 1:
                 next_x_start_anel = 0.0 + (passo_idx + 1) * actual_passo_axial if direction == 'esquerda_direita' else part_length - (passo_idx + 1) * actual_passo_axial
                 next_x_start_anel = max(0.0, min(part_length, next_x_start_anel))
                 segment_gcode.append(f"G00 X{next_x_start_anel:.3f}")

            if passo_idx == num_passos_axiais - 1 and abs(lead_out) > 1e-6:
                  segment_gcode.append(f"(Movimento lead-out)")
                  # Ramp down para finalizar
                  back_mid = current_x_end_pos_final_volta - (current_x_end_pos_final_volta - current_x_start_pos) * 0.3
                  segment_gcode.append(f"G01 F{feed_linear*0.85:.1f} X{back_mid:.3f}")
                  segment_gcode.append(f"G01 F{feed_linear*0.6:.1f} X{current_x_end_pos_final_volta:.3f}")
                  if not compact:
                      pass

        segment_gcode.extend([f"G00 Z{z_seguranca:.3f}", "M01" if layer_num < params['num_camadas']-1 else ""]) 
        return segment_gcode
        
    def _generate_square_oscillation(self, params):
        gcode_body = []
        current_d = params['diametro']
        for i in range(params['num_camadas']):
            gcode_body.extend(self._build_square_oscillation_segment(params, i, current_d))
            current_d += 2 * params['espessura_camada']
        return self._build_header(params) + gcode_body + self._build_footer()

    def _generate_square_test_oscillation(self, params):
        # Variante de teste: feeds independentes por eixo e decomposição em "escada"
        self._normalize_params(params)
        gcode_body = []
        current_d = params['diametro']
        for i in range(params['num_camadas']):
            gcode_body.extend(self._build_square_test_oscillation_segment(params, i, current_d))
            current_d += 2 * params['espessura_camada']
        return self._build_header(params) + gcode_body + self._build_footer()

    def _build_square_oscillation_segment(self, params, layer_num, current_d):
        # Normaliza para chamadas diretas em testes
        self._normalize_params(params)
        def calc_feed(v, rpm):
            return v / rpm if rpm else v  # fallback para v se rpm=0
        afastamento = params['afastamento_tocha']; z_offset = 0.0
        z_layer = current_d / 2.0 + afastamento + z_offset
        z_seguranca = z_layer + 30.0
        part_length = params['comprimento_revestir']
        direction = params.get('direcao_soldagem', 'esquerda_direita')
        rotation_dir = params.get('sentido_rotacao', 'horaria')
        lead_in = params.get('lead_in', 0.0); lead_out = params.get('lead_out', 0.0)
        taxa_de_deposicao = params['taxa_de_deposicao']
        # Sempre usar controle de rotação pelo eixo A
        osc_length = params['oscilacao_comprimento']
        larg_cordao = params['largura_cordao']
        desloc_perc = params['deslocamento_angular_perc']
        sobreposicao = params['sobreposicao']

        if direction == 'esquerda_direita': x_start_revest = 0.0 - lead_in; x_end_revest = part_length + lead_out
        else: x_start_revest = part_length + lead_out; x_end_revest = 0.0 - lead_in
        actual_passo_axial = osc_length * (sobreposicao / 100.0); actual_passo_axial = max(actual_passo_axial, 1e-6)
        num_passos_axiais = math.ceil(part_length / actual_passo_axial) if part_length > 0 else 1
        circunferencia = math.pi * current_d
        desloc_linear_angular = (desloc_perc / 100.0) * larg_cordao
        if circunferencia <= 0 or desloc_linear_angular <= 0: return ["(ERRO: Diametro ou deslocamento invalido para calculo angular)"]
        num_passos_angulares_por_volta = math.ceil(circunferencia / desloc_linear_angular)
        actual_delta_A_deg = 360.0 / num_passos_angulares_por_volta
        rotation_sign = 1.0 if rotation_dir == 'horaria' else -1.0
        # RPM derivado diretamente: taxa/(D*pi)
        rpm_a = (taxa_de_deposicao / circunferencia) if circunferencia > 0 and taxa_de_deposicao > 0 else 0
        rpm_real = rpm_a
        feed_angular = rpm_real * 360.0
        # Feed X definido pela velocidade da oscilação (mm/min)
        feed_linear = float(params.get('velocidade_oscilacao_mm_min', taxa_de_deposicao) or taxa_de_deposicao)
        
        segment_gcode = [f"(--- CAMADA {layer_num + 1} ---)", f"(SENTIDO: {direction})", f"G00 Z{z_seguranca:.3f}"]
        # Stagger angular intercamadas
        current_A_total = (layer_num * (actual_delta_A_deg * 0.25)) * (1.0 if rotation_dir == 'horaria' else -1.0)
        n_scurve = int(params.get('n_scurve_steps', 6) or 6)

        for passo_idx in range(num_passos_axiais):
            if direction == 'esquerda_direita':
                x_start_anel = 0.0 + passo_idx * actual_passo_axial
                x_osc_ida = x_start_anel + osc_length; x_osc_volta = x_start_anel
            else:
                x_start_anel = part_length - passo_idx * actual_passo_axial
                x_osc_ida = x_start_anel - osc_length; x_osc_volta = x_start_anel

            x_osc_ida = max(0.0, min(part_length, x_osc_ida))
            x_osc_volta = max(0.0, min(part_length, x_osc_volta))

            current_x_start_pos = x_osc_volta
            if passo_idx == 0: current_x_start_pos = x_start_revest
            current_x_end_pos_final_volta = x_osc_volta
            if passo_idx == num_passos_axiais -1: current_x_end_pos_final_volta = x_end_revest
            segment_gcode.append(f"(--- PASSO AXIAL {passo_idx + 1}/{num_passos_axiais} ---)")
            segment_gcode.append(f"G00 X{current_x_start_pos:.3f} A{current_A_total:.3f}")
            segment_gcode.append(f"G01 F{feed_linear*2:.1f} Z{z_layer:.3f}")
            if passo_idx == 0:
                 # ---------- ROTAÇÃO ÚNICA POR PASSE (S-curve quando detalhado) ----------
                 target_A_total = current_A_total + rotation_sign * actual_delta_A_deg * num_passos_angulares_por_volta * num_passos_axiais
                 if bool(params.get('compact_gcode', False)):
                     segment_gcode.append(f"G01 F{feed_angular:.1f} A{target_A_total:.3f}")
                 else:
                     for f in self._scurve_fractions(n_scurve):
                         a_pos = current_A_total + (target_A_total - current_A_total) * f
                         segment_gcode.append(f"G01 F{feed_angular:.1f} A{a_pos:.3f}")
                 # (G94 já ativo no cabeçalho)
                 if abs(lead_in) > 1e-6:
                     mid1 = current_x_start_pos + (x_osc_volta - current_x_start_pos) * 0.3
                     mid2 = current_x_start_pos + (x_osc_volta - current_x_start_pos) * 0.7
                     segment_gcode.append(f"G01 F{feed_linear*0.6:.1f} X{mid1:.3f}")
                     segment_gcode.append(f"G01 F{feed_linear*0.85:.1f} X{mid2:.3f}")
                     segment_gcode.append(f"G01 F{feed_linear:.1f} X{x_osc_volta:.3f}")
                     

            # Handle final axial step differently to prevent duplication
            if passo_idx == num_passos_axiais - 1:
                # For final step, do a single pass to end position without oscillation
                segment_gcode.append(f"(Passo final - posicao de termino)")
                # Single smooth movement to final position
                segment_gcode.append(f"G01 F{feed_linear:.1f} X{x_end_revest:.3f}")
                # Rotação única já foi emitida – não repetir "A"
            else:
                # Normal oscillation for non-final steps
                for k in range(num_passos_angulares_por_volta):
                    current_x_osc_ida = x_osc_ida
                    current_x_osc_volta = x_osc_volta
                    segment_gcode.append(f"(Passo angular {k+1}/{num_passos_angulares_por_volta})")
                    # Perfil S-curve em X
                    for f in self._scurve_fractions(n_scurve):
                        x_pos = current_x_osc_volta + (current_x_osc_ida - current_x_osc_volta) * f
                        segment_gcode.append(f"G01 F{feed_linear:.1f} X{x_pos:.3f}")
                    
                    # Rotação única já foi emitida – não repetir "A"

            if passo_idx < num_passos_axiais - 1:
                 next_x_start_anel = 0.0 + (passo_idx + 1) * actual_passo_axial if direction == 'esquerda_direita' else part_length - (passo_idx + 1) * actual_passo_axial
                 next_x_start_anel = max(0.0, min(part_length, next_x_start_anel))
                 segment_gcode.append(f"G00 X{next_x_start_anel:.3f}")

            if passo_idx == num_passos_axiais - 1 and abs(lead_out) > 1e-6:
                  segment_gcode.append(f"(Movimento lead-out)")
                  back_mid = current_x_end_pos_final_volta - (current_x_end_pos_final_volta - current_x_start_pos) * 0.3
                  segment_gcode.append(f"G01 F{feed_linear*0.85:.1f} X{back_mid:.3f}")
                  segment_gcode.append(f"G01 F{feed_linear*0.6:.1f} X{current_x_end_pos_final_volta:.3f}")
                  

        segment_gcode.extend([f"G00 Z{z_seguranca:.3f}", "M01" if layer_num < params['num_camadas']-1 else ""]) 
        return segment_gcode


    def _build_square_test_oscillation_segment(self, params, layer_num, current_d):
        # Normaliza para chamadas diretas em testes
        self._normalize_params(params)
        afastamento = params['afastamento_tocha']; z_offset = 0.0
        z_layer = current_d / 2.0 + afastamento + z_offset
        z_seguranca = z_layer + 30.0
        part_length = params['comprimento_revestir']
        direction = params.get('direcao_soldagem', 'esquerda_direita')
        rotation_dir = params.get('sentido_rotacao', 'horaria')
        lead_in = params.get('lead_in', 0.0); lead_out = params.get('lead_out', 0.0)
        taxa_de_deposicao = params.get('taxa_de_deposicao', 0.0) or 0.0
        osc_length = params['oscilacao_comprimento']
        larg_cordao = params['largura_cordao']
        desloc_perc = params['deslocamento_angular_perc']
        sobreposicao = params['sobreposicao']
        compact = bool(params.get('compact_gcode', False))

        # Granularidade da "escada" (por passo)
        gran_x = float(params.get('osc_test_gran_x', 1.0))
        gran_a = float(params.get('osc_test_gran_a', 1.0))

        if direction == 'esquerda_direita':
            x_start_revest = 0.0 - lead_in; x_end_revest = part_length + lead_out
        else:
            x_start_revest = part_length + lead_out; x_end_revest = 0.0 - lead_in

        actual_passo_axial = osc_length * (sobreposicao / 100.0); actual_passo_axial = max(actual_passo_axial, 1e-6)
        num_passos_axiais = math.ceil(part_length / actual_passo_axial) if part_length > 0 else 1
        circunferencia = math.pi * current_d
        desloc_linear_angular = (desloc_perc / 100.0) * larg_cordao
        if circunferencia <= 0 or desloc_linear_angular <= 0:
            return ["(ERRO: Diametro ou deslocamento invalido para calculo angular)"]
        num_passos_angulares_por_volta = math.ceil(circunferencia / desloc_linear_angular)
        actual_delta_A_deg = 360.0 / num_passos_angulares_por_volta
        rotation_sign = 1.0 if rotation_dir == 'horaria' else -1.0

        # Feeds independentes: X usa taxa de deposição; A usa RPM -> deg/min
        rpm_a = (taxa_de_deposicao / circunferencia) if circunferencia > 0 and taxa_de_deposicao > 0 else 0.0
        # Feed X definido pela velocidade da oscilação (mm/min)
        feed_x = float(params.get('velocidade_oscilacao_mm_min', taxa_de_deposicao) or taxa_de_deposicao)
        feed_a = rpm_a * 360.0

        
        segment_gcode = [f"(--- CAMADA {layer_num + 1} ---)", f"(SENTIDO: {direction})", f"G00 Z{z_seguranca:.3f}"]
        # Stagger angular intercamadas
        current_A_total = (layer_num * (actual_delta_A_deg * 0.25)) * (1.0 if rotation_dir == 'horaria' else -1.0)
        current_x_pos = None

        def staircase_move(x0, a0, x1, a1):
            """Decompõe movimento diagonal em passos X e A com feeds próprios."""
            dx = x1 - x0; da = a1 - a0
            steps_x = math.ceil(abs(dx) / max(gran_x, 1e-6)) if abs(dx) > 1e-9 else 0
            steps_a = math.ceil(abs(da) / max(gran_a, 1e-6)) if abs(da) > 1e-9 else 0
            S = max(steps_x, steps_a, 1)
            for i in range(1, S + 1):
                xi = x0 + dx * (i / S)
                ai = a0 + da * (i / S)
                # Primeiro X, depois A (ordem pode ser ajustada no futuro)
                segment_gcode.append(f"G01 F{feed_x:.1f} X{xi:.3f}")
                segment_gcode.append(f"G01 F{feed_a:.1f} A{ai:.3f}")
            return x1, a1

        for passo_idx in range(num_passos_axiais):
            if direction == 'esquerda_direita':
                x_start_anel = 0.0 + passo_idx * actual_passo_axial
                x_osc_ida = x_start_anel + osc_length; x_osc_volta = x_start_anel
            else:
                x_start_anel = part_length - passo_idx * actual_passo_axial
                x_osc_ida = x_start_anel - osc_length; x_osc_volta = x_start_anel

            x_osc_ida = max(0.0, min(part_length, x_osc_ida))
            x_osc_volta = max(0.0, min(part_length, x_osc_volta))

            current_x_start_pos = x_osc_volta
            if passo_idx == 0:
                current_x_start_pos = x_start_revest
            current_x_end_pos_final_volta = x_osc_volta
            if passo_idx == num_passos_axiais - 1:
                current_x_end_pos_final_volta = x_end_revest

            segment_gcode.append(f"(--- PASSO AXIAL {passo_idx + 1}/{num_passos_axiais} ---)")
            # Aproxima ao início do anel
            if current_x_pos is None:
                current_x_pos = current_x_start_pos
            segment_gcode.append(f"G00 X{current_x_start_pos:.3f} A{current_A_total:.3f}")
            segment_gcode.append(f"G01 F{feed_x*2:.1f} Z{z_layer:.3f}")
            if passo_idx == 0:
                if not compact:
                    pass
                # Lead-in somente X
                if abs(lead_in) > 1e-6:
                    segment_gcode.append(f"G01 F{feed_x:.1f} X{x_osc_volta:.3f}")
                    if not compact:
                        pass

            # Passo final: fechar somente em X até o término, sem rotação
            if passo_idx == num_passos_axiais - 1:
                segment_gcode.append(f"(Fechamento ate fim do revestimento)")
                dist_ate_fim = abs(x_end_revest - current_x_start_pos)
                if dist_ate_fim > 1e-6:
                    segment_gcode.append(f"G01 F{feed_x:.1f} X{x_end_revest:.3f}")
            else:
                # Oscilação por passos angulares com decomposição em escada
                for k in range(num_passos_angulares_por_volta):
                    segment_gcode.append(f"(Passo angular {k+1}/{num_passos_angulares_por_volta})")
                    deltaA_half = rotation_sign * (actual_delta_A_deg / 2.0)
                    # Ida em X e A
                    target_x_ida = x_osc_ida
                    target_A_ida = current_A_total + deltaA_half
                    current_x_pos, current_A_total = staircase_move(current_x_start_pos, current_A_total, target_x_ida, target_A_ida)
                    current_x_start_pos = target_x_ida
                    if not compact:
                        pass
                    # Volta em X e A
                    target_x_volta = x_osc_volta
                    target_A_volta = current_A_total + deltaA_half
                    current_x_pos, current_A_total = staircase_move(current_x_start_pos, current_A_total, target_x_volta, target_A_volta)
                    current_x_start_pos = target_x_volta
                    if not compact:
                        pass

            if passo_idx < num_passos_axiais - 1:
                next_x_start_anel = 0.0 + (passo_idx + 1) * actual_passo_axial if direction == 'esquerda_direita' else part_length - (passo_idx + 1) * actual_passo_axial
                next_x_start_anel = max(0.0, min(part_length, next_x_start_anel))
                segment_gcode.append(f"G00 X{next_x_start_anel:.3f}")

            if passo_idx == num_passos_axiais - 1 and abs(lead_out) > 1e-6:
                segment_gcode.append(f"(Movimento lead-out)")
                back_mid = current_x_end_pos_final_volta - (current_x_end_pos_final_volta - current_x_start_pos) * 0.3
                segment_gcode.append(f"G01 F{feed_x*0.85:.1f} X{back_mid:.3f}")
                segment_gcode.append(f"G01 F{feed_x*0.6:.1f} X{current_x_end_pos_final_volta:.3f}")
                if not compact:
                    pass

        segment_gcode.extend([f"G00 Z{z_seguranca:.3f}", "M01" if layer_num < params['num_camadas']-1 else ""]) 
        return segment_gcode
