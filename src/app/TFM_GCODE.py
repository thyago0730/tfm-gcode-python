# -*- coding: utf-8 -*-
# TFM G-Code Generator v11.3 - Sem Dependência de Idioma (KeyError Fix)
# Desenvolvido para TFM Usinagem & Manutenção Industrial LTDA

import tkinter as tk
from tkinter import filedialog, messagebox, ttk, Toplevel, Text, PanedWindow
import numpy as np
import math
import json
import os
import sys
import copy
from datetime import datetime
import webbrowser
import subprocess
import re
import threading
import urllib.request
import hashlib
import tempfile
from pathlib import Path

from matplotlib.figure import Figure
from matplotlib.backends.backend_tkagg import FigureCanvasTkAgg, NavigationToolbar2Tk

# --- MÓDULO DE GERAÇÃO DE PDF ---
try:
    from reportlab.pdfgen import canvas as pdfcanvas
    from reportlab.lib.pagesizes import A4
    from reportlab.lib.units import mm
    from reportlab.lib.utils import ImageReader
    from reportlab.lib import colors
    REPORTLAB_AVAILABLE = True
except ImportError:
    REPORTLAB_AVAILABLE = False

# --- FUNÇÃO PARA LOCALIZAR ARQUIVOS NO EXECUTÁVEL ---
def resource_path(relative_path):
    """ Obtém o caminho absoluto para o recurso, funciona para dev e para PyInstaller """
    try:
        base_path = sys._MEIPASS
    except Exception:
        base_path = os.path.abspath(".")
    return os.path.join(base_path, relative_path)

# --- WIDGET DE FRAME COM ROLAGEM ---
class ScrolledFrame(ttk.Frame):
    def __init__(self, parent, *args, **kw):
        super().__init__(parent, *args, **kw)
        vscrollbar = ttk.Scrollbar(self, orient="vertical")
        vscrollbar.pack(fill="y", side="right", expand=False)
        self.canvas = tk.Canvas(self, bd=0, highlightthickness=0, yscrollcommand=vscrollbar.set)
        self.canvas.pack(side="left", fill="both", expand=True)
        vscrollbar.config(command=self.canvas.yview)
        self.canvas.xview_moveto(0); self.canvas.yview_moveto(0)
        self.interior = ttk.Frame(self.canvas)
        self.interior_id = self.canvas.create_window(0, 0, window=self.interior, anchor="nw")
        self.interior.bind('<Configure>', self._configure_interior)
        self.canvas.bind('<Configure>', self._configure_canvas)

    def _configure_interior(self, event):
        size = (self.interior.winfo_reqwidth(), self.interior.winfo_reqheight())
        self.canvas.config(scrollregion="0 0 %s %s" % size)
        if self.interior.winfo_reqwidth() != self.canvas.winfo_width():
            self.canvas.config(width=self.interior.winfo_reqwidth())

    def _configure_canvas(self, event):
        if self.interior.winfo_reqwidth() != self.canvas.winfo_width():
            self.canvas.itemconfigure(self.interior_id, width=self.canvas.winfo_width())

# --- CLASSE Tooltip ---
class ToolTip:
    """Tooltip leve e reutilizável com atraso e offset dinâmico por DPI."""
    _shared_tw = None
    _shared_label = None

    def __init__(self, widget, text, delay_ms: int = 180):
        self.widget = widget
        self.text = text
        self._delay_ms = delay_ms
        self._job_id = None
        widget.bind("<Enter>", self._schedule_show)
        widget.bind("<Leave>", self._cancel_and_hide)

    def _schedule_show(self, event=None):
        try:
            if self._job_id:
                self.widget.after_cancel(self._job_id)
            self._job_id = self.widget.after(self._delay_ms, self._show)
        except Exception:
            pass

    def _show(self, event=None):
        self._job_id = None
        try:
            # Offset baseado em escala do Tk (aproxima DPI)
            try:
                scaling = float(self.widget.tk.call('tk', 'scaling'))
            except Exception:
                scaling = 1.0
            offset = int(16 * scaling)
            x = self.widget.winfo_rootx() + offset
            y = self.widget.winfo_rooty() + offset

            if ToolTip._shared_tw is None:
                parent = self.widget.winfo_toplevel()
                ToolTip._shared_tw = tk.Toplevel(parent)
                ToolTip._shared_tw.wm_overrideredirect(True)
                ToolTip._shared_label = ttk.Label(ToolTip._shared_tw, background="#ffffe0", relief="solid", borderwidth=1, padding=(6,4))
                ToolTip._shared_label.pack()

            ToolTip._shared_label.configure(text=self.text)
            ToolTip._shared_tw.wm_geometry(f"+{x}+{y}")
            ToolTip._shared_tw.deiconify()
        except Exception:
            pass

    def _cancel_and_hide(self, event=None):
        try:
            if self._job_id:
                self.widget.after_cancel(self._job_id)
                self._job_id = None
        except Exception:
            pass
        try:
            if ToolTip._shared_tw is not None:
                ToolTip._shared_tw.withdraw()
        except Exception:
            pass

class SplashScreen(tk.Toplevel):
    def __init__(self, master, image_path=None, message="Carregando...", bg_color="#ffffff", transparent_bg=True, transparent_color="#ffffff"):
        super().__init__(master)
        try:
            self.overrideredirect(True)
        except Exception:
            pass
        # Cor de fundo: se transparente, usa uma cor "chave" rara
        use_bg = transparent_color if transparent_bg else bg_color
        self.configure(bg=use_bg)
        try:
            self.attributes("-topmost", True)
        except Exception:
            pass
        # Ativa transparência por cor-chave (Windows) para remover fundo da janela
        try:
            if transparent_bg:
                self.attributes("-transparentcolor", transparent_color)
        except Exception:
            pass
        # Container com a mesma cor de fundo
        container = tk.Frame(self, bg=use_bg, highlightthickness=0, bd=0)
        container.pack(fill="both", expand=True, padx=12, pady=12)
        try:
            if image_path and os.path.exists(image_path):
                self._logo_img = tk.PhotoImage(file=image_path)
                tk.Label(container, image=self._logo_img, bg=use_bg, highlightthickness=0, bd=0).pack(pady=(6, 6))
            else:
                self._logo_img = None
        except Exception:
            self._logo_img = None
        if message:
            tk.Label(container, text=message, bg=use_bg).pack()
        self.update_idletasks()
        # Centraliza na tela
        try:
            w = self.winfo_reqwidth(); h = self.winfo_reqheight()
            sw = self.winfo_screenwidth(); sh = self.winfo_screenheight()
            x = int((sw - w) / 2); y = int((sh - h) / 2)
            self.geometry(f"{w}x{h}+{x}+{y}")
        except Exception:
            pass

# Toolbar personalizada para ajustar "Home" ao percurso atual
class CustomToolbar(NavigationToolbar2Tk):
    def __init__(self, canvas, window, app_ref):
        self._app_ref = app_ref
        super().__init__(canvas, window)
        # Botões rápidos integrados à barra com quebra de linha quando necessário
        try:
            # Linha 1 (dentro da toolbar padrão)
            self._btn_top_r1 = ttk.Button(self, text="Top", style='Toolbutton', command=self._app_ref._view_top)
            self._btn_side_r1 = ttk.Button(self, text="Side", style='Toolbutton', command=self._app_ref._view_side)
            self._btn_top_r1.pack(side=tk.LEFT, padx=(6, 0))
            self._btn_side_r1.pack(side=tk.LEFT, padx=(4, 0))

            # Linha 2 (overflow abaixo da barra)
            self._row2 = ttk.Frame(window)
            self._btn_top_r2 = ttk.Button(self._row2, text="Top", style='Toolbutton', command=self._app_ref._view_top)
            self._btn_side_r2 = ttk.Button(self._row2, text="Side", style='Toolbutton', command=self._app_ref._view_side)

            def _update_toolbar_layout(event=None):
                try:
                    container_w = window.winfo_width()
                    # Largura requerida pelos filhos atuais da barra (linha 1)
                    total_children_w = 0
                    for child in self.winfo_children():
                        try:
                            total_children_w += child.winfo_reqwidth()
                        except Exception:
                            pass
                    # Margem de segurança
                    total_children_w += 8

                    if total_children_w > container_w:
                        # mover os extras para a segunda linha
                        try:
                            self._btn_top_r1.pack_forget(); self._btn_side_r1.pack_forget()
                        except Exception:
                            pass
                        try:
                            if not getattr(self, '_row2_packed', False):
                                self._row2.pack(side=tk.TOP, fill=tk.X)
                                self._row2_packed = True
                            if not getattr(self, '_row2_buttons_packed', False):
                                self._btn_top_r2.pack(side=tk.LEFT, padx=(6, 0))
                                self._btn_side_r2.pack(side=tk.LEFT, padx=(4, 0))
                                self._row2_buttons_packed = True
                        except Exception:
                            pass
                    else:
                        # manter extras na primeira linha e ocultar a segunda
                        try:
                            if not self._btn_top_r1.winfo_ismapped():
                                self._btn_top_r1.pack(side=tk.LEFT, padx=(6, 0))
                                self._btn_side_r1.pack(side=tk.LEFT, padx=(4, 0))
                        except Exception:
                            pass
                        try:
                            if getattr(self, '_row2_buttons_packed', False):
                                self._btn_top_r2.pack_forget(); self._btn_side_r2.pack_forget()
                                self._row2_buttons_packed = False
                            if getattr(self, '_row2_packed', False):
                                self._row2.pack_forget(); self._row2_packed = False
                        except Exception:
                            pass
                except Exception:
                    pass

            # Atualiza ao redimensionar
            try:
                window.bind("<Configure>", _update_toolbar_layout, add='+')
                self.bind("<Configure>", _update_toolbar_layout, add='+')
                # executa uma vez após inicialização
                try:
                    window.after(50, _update_toolbar_layout)
                except Exception:
                    _update_toolbar_layout()
            except Exception:
                pass
            # Removido: botão Isométrico conforme solicitado
        except Exception:
            pass

    def home(self, *args):
        # Recalcula desenho e aplica vista espelhada (comportamento antigo desejado)
        try:
            params = self._app_ref._get_current_params()
            self._app_ref.desenhar_percurso_3d(params)
            try:
                elev, azim = getattr(self._app_ref, '_initial_view', (self._app_ref.ax.elev, self._app_ref.ax.azim))
                self._app_ref.ax.view_init(elev=elev, azim=azim)
                self._app_ref.canvas.draw_idle()
            except Exception:
                pass
        except Exception:
            # Fallback para home padrão caso algo falhe
            try:
                super().home(*args)
            except Exception:
                pass

# --- REMOVIDO: MÓDULO DE GESTÃO DE IDIOMAS ---

# --- MÓDULO DE GERAÇÃO DE G-CODE ---
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


# --- JANELA DE CONFIGURAÇÕES ---
class SettingsWindow(Toplevel):
    def __init__(self, parent_app: 'TFM_GCODE', config, settings_type):
        super().__init__(parent_app.root)
        self.parent_app = parent_app; self.local_config = config; self.settings_type = settings_type
        # --- ALTERADO: Textos hard-coded ---
        self.title('Custos de Operação' if self.settings_type == 'costs' else ('Tipos de Pó' if self.settings_type == 'powders' else ('Fórmulas' if self.settings_type == 'formulas' else 'Configurações')))
        self.transient(parent_app.root); self.grab_set(); self.vars = {}
        # UI para tipos de pó
        if self.settings_type == 'powders':
            container = ttk.Frame(self, padding="10"); container.pack(expand=True, fill="both")
            left = ttk.Frame(container); right = ttk.Frame(container)
            left.grid(row=0, column=0, sticky='nsew', padx=(0,10)); right.grid(row=0, column=1, sticky='nsew')
            container.columnconfigure(0, weight=1); container.columnconfigure(1, weight=2); container.rowconfigure(0, weight=1)

            ttk.Label(left, text='Tipos de Pó').pack(anchor='w')
            self.powder_list = tk.Listbox(left, height=10)
            self.powder_list.pack(expand=True, fill='both')
            btns = ttk.Frame(left); btns.pack(fill='x', pady=5)
            ttk.Button(btns, text='Adicionar', command=self._powder_add).pack(side='left')
            ttk.Button(btns, text='Remover', command=self._powder_remove).pack(side='left', padx=5)

            # Campos de edição
            self.vars['name'] = tk.StringVar()
            self.vars['density_factor_g_mm2'] = tk.StringVar()
            self.vars['cost_brl_kg'] = tk.StringVar()
            ttk.Label(right, text='Nome:').grid(row=0, column=0, sticky='w', pady=2)
            ttk.Entry(right, textvariable=self.vars['name']).grid(row=0, column=1, sticky='ew', padx=5)
            ttk.Label(right, text='Peso específico (fator):').grid(row=1, column=0, sticky='w', pady=2)
            ttk.Entry(right, textvariable=self.vars['density_factor_g_mm2']).grid(row=1, column=1, sticky='ew', padx=5)
            ttk.Label(right, text='Custo (R$/kg):').grid(row=2, column=0, sticky='w', pady=2)
            ttk.Entry(right, textvariable=self.vars['cost_brl_kg']).grid(row=2, column=1, sticky='ew', padx=5)
            right.columnconfigure(1, weight=1)

            # Popular lista
            self._powders = list(self.local_config.get('powders', []))
            active = self.local_config.get('active_powder')
            for p in self._powders:
                self.powder_list.insert(tk.END, p.get('name', 'Sem nome'))
            if active:
                try:
                    idx = next(i for i,p in enumerate(self._powders) if p.get('name') == active)
                    self.powder_list.selection_set(idx); self.powder_list.activate(idx);
                    self._powder_load(idx)
                except StopIteration:
                    if self._powders:
                        self.powder_list.selection_set(0); self.powder_list.activate(0); self._powder_load(0)
            self.powder_list.bind('<<ListboxSelect>>', self._powder_on_select)

            btn_frame = ttk.Frame(self); btn_frame.pack(fill='x', padx=10, pady=5)
            ttk.Button(btn_frame, text='Salvar', command=self.save_and_close).pack(side='right')
            ttk.Button(btn_frame, text='Cancelar', command=self.destroy).pack(side='right', padx=5)
        elif self.settings_type == 'formulas':
            # Layout: esquerda (lista de variáveis), direita (editor JSON)
            container = ttk.PanedWindow(self, orient='horizontal'); container.pack(expand=True, fill='both')
            left = ttk.Frame(container, padding="10"); right = ttk.Frame(container, padding="10")
            container.add(left, weight=1); container.add(right, weight=2)

            ttk.Label(left, text='Variáveis de Cálculo').grid(row=0, column=0, sticky='w')
            cols = ('variavel', 'descricao', 'uso')
            self.vars_tree = ttk.Treeview(left, columns=cols, show='headings', height=18)
            self.vars_tree.heading('variavel', text='Variável'); self.vars_tree.heading('descricao', text='Descrição'); self.vars_tree.heading('uso', text='Uso')
            self.vars_tree.column('variavel', width=180, anchor='w'); self.vars_tree.column('descricao', width=380, anchor='w'); self.vars_tree.column('uso', width=40, anchor='center')
            self.vars_tree.grid(row=1, column=0, sticky='nsew', pady=5)
            left.rowconfigure(1, weight=1); left.columnconfigure(0, weight=1)

            # Ações rápidas para variáveis
            var_btns = ttk.Frame(left); var_btns.grid(row=2, column=0, sticky='ew')
            ttk.Button(var_btns, text='Copiar nome', command=self._copy_selected_var).pack(side='left')
            ttk.Label(var_btns, text='Dê duplo clique para inserir no editor.').pack(side='left', padx=10)
            self.vars_tree.bind('<Double-1>', self._on_var_dblclick)

            # Mapa de variáveis existentes e suas descrições
            variables_map = [
                ('diametro', 'Diâmetro da peça (mm)'),
                ('comprimento_revestir', 'Comprimento a revestir (mm)'),
                ('largura_cordao', 'Largura do cordão (mm)'),
                ('sobreposicao', 'Sobreposição (%)'),
                ('velocidade_de_deposicao', 'Velocidade de deposição (mm/min)'),
                ('velocidade_a_mm_min', 'Velocidade do eixo A (mm/min) — padrão igual à taxa'),
                ('afastamento_tocha', 'Afastamento da tocha (mm)'),
                ('lead_in', 'Início extra (mm)'),
                ('lead_out', 'Término extra (mm)'),
                ('direcao_soldagem', 'Sentido da soldagem (esquerda_direita | direita_esquerda)'),
                ('sentido_rotacao', 'Sentido da rotação (horaria | antihoraria)'),
                ('corrente_arco', 'Corrente de arco (A)'),
                ('vazao_gas', 'Vazão de gás (l/min)'),
                ('alim_po', 'Alimentação de pó (%)'),
                ('preaquecimento', 'Pré-aquecimento (°C)'),
                ('num_camadas', 'Número de camadas'),
                ('espessura_camada', 'Espessura por camada (mm)'),
                ('powder_name', 'Nome do pó ativo'),
                ('powder_factor', 'Fator de densidade do pó (g/mm²)'),
                ('powder_cost_brl_kg', 'Custo do pó (R$/kg)'),
                ('powder_mass_g', 'Consumo de pó (g) — resultado de fórmula'),
                ('powder_mass_kg', 'Consumo de pó (kg) — resultado de fórmula'),
                ('taxa_deposicao_g_h', 'Taxa de deposição (g/h)'),
                ('taxa_deposicao_kg_h', 'Taxa de deposição (kg/h) — derivada'),
                ('welding_mode', 'Modo de soldagem (espiral | oscilacao)'),
                ('oscilacao_comprimento', 'Comprimento da oscilação (mm)'),
                ('deslocamento_angular_perc', 'Deslocamento angular (% da largura do cordão)'),
                ('oscillation_type', 'Tipo de oscilação (linear | quadrada | quadrada_continua)'),
                ('osc_test_gran_x', 'Granularidade X para Quadrada Contínua (mm)'),
                ('compact_gcode', 'Modo compacto de G-code (booleano)'),
                ('torch_retract_on_ignite', 'Recuar tocha ao ligar (booleano)'),
                ('diametro_inicial', 'Diâmetro inicial (mm)'),
                ('diametro_final', 'Diâmetro final (mm)'),
                ('tipo_peca', 'Tipo de peça (cilindrico)')
            ]
            self._variables_list = [var for var, _ in variables_map]
            for var, desc in variables_map:
                self.vars_tree.insert('', tk.END, values=(var, desc, ''))
            # Atualização inicial de uso e sintaxe
            try:
                self._update_variable_usage(); self._apply_syntax_highlight()
            except Exception:
                pass

            ttk.Label(right, text='Editor de Fórmulas (JSON)').grid(row=0, column=0, sticky='w')
            editor_btns = ttk.Frame(right); editor_btns.grid(row=1, column=0, sticky='ew', pady=(2,0))
            ttk.Button(editor_btns, text='Validar', command=self._validate_json).pack(side='left')
            ttk.Button(editor_btns, text='Formatar', command=self._format_json).pack(side='left', padx=5)
            ttk.Button(editor_btns, text='Avaliar', command=self._evaluate_formulas).pack(side='left')
            # Indicador de status do editor
            self._editor_status_var = tk.StringVar(value='Válido')
            self._editor_status_lbl = ttk.Label(editor_btns, textvariable=self._editor_status_var)
            self._editor_status_lbl.pack(side='left', padx=10)
            ttk.Button(editor_btns, text='Exportar', command=self._export_formulas).pack(side='right')
            ttk.Button(editor_btns, text='Importar', command=self._import_formulas).pack(side='right', padx=5)
            ttk.Button(editor_btns, text='Restaurar padrão', command=self._restore_default_formulas).pack(side='right')
            # Editor JSON com conteúdo atual
            self.formulas_text = Text(right, height=16, width=60)
            try:
                initial_json = json.dumps(self.local_config.get('formulas', {}), indent=2, ensure_ascii=False)
            except Exception:
                initial_json = "{}"
            self.formulas_text.insert('1.0', initial_json)
            # Configurar tag de erro para realce
            self.formulas_text.tag_configure('error', background='#ffdddd')
            # Tags para realce de sintaxe
            self.formulas_text.tag_configure('key', foreground='#1a73e8')
            self.formulas_text.tag_configure('number', foreground='#b00020')
            self.formulas_text.tag_configure('function', foreground='#6f42c1')
            self.formulas_text.tag_configure('variable', foreground='#2b8a3e')
            self.formulas_text.tag_configure('string', foreground='#c47f17')
            self.formulas_text.grid(row=2, column=0, sticky='nsew', pady=5)
            # Atalhos
            self.formulas_text.bind('<Control-Return>', lambda e: (self._evaluate_formulas(), 'break'))
            self.formulas_text.bind('<Control-Shift-F>', lambda e: (self._format_json(), self._apply_syntax_highlight(), 'break'))
            self.formulas_text.bind('<Control-l>', lambda e: (self._validate_json(), 'break'))
            # Realce ao digitar
            self.formulas_text.bind('<KeyRelease>', lambda e: (self._apply_syntax_highlight(), self._update_variable_usage(), self._set_editor_status('Não salvo', 'pending')))
            right.rowconfigure(2, weight=1); right.columnconfigure(0, weight=1)

            # Painel de resultados da avaliação
            ttk.Label(right, text='Resultados (avaliados com parâmetros atuais)').grid(row=3, column=0, sticky='w')
            self.results_tree = ttk.Treeview(right, columns=('chave','valor'), show='headings', height=8)
            self.results_tree.heading('chave', text='Chave'); self.results_tree.heading('valor', text='Valor')
            self.results_tree.column('chave', width=180, anchor='w'); self.results_tree.column('valor', width=260, anchor='w')
            self.results_tree.grid(row=4, column=0, sticky='nsew')
            # Copiar resultados com Ctrl+C e menu de contexto
            try:
                self.results_tree.bind('<Control-c>', self._copy_selected_results)
                self._results_menu = tk.Menu(self.results_tree, tearoff=0)
                self._results_menu.add_command(label='Copiar resultado', command=self._copy_selected_results)
                def _show_results_menu(e):
                    try:
                        self.results_tree.focus_set()
                        self._results_menu.tk_popup(e.x_root, e.y_root)
                    finally:
                        try: self._results_menu.grab_release()
                        except Exception: pass
                self.results_tree.bind('<Button-3>', _show_results_menu)
            except Exception:
                pass

            btn_frame = ttk.Frame(self); btn_frame.pack(fill='x', padx=10, pady=5)
            ttk.Button(btn_frame, text='Salvar', command=self.save_and_close).pack(side='right')
            ttk.Button(btn_frame, text='Cancelar', command=self.destroy).pack(side='right', padx=5)
        else:
            frame = ttk.Frame(self, padding="10"); frame.pack(expand=True, fill="both")
            # Removido: seção "Limites da Máquina"; apenas custos permanecem
            fields = ['gas_argon_brl_m3', 'labor_brl_hour', 'machine_brl_hour']
            labels = {
                'gas_argon_brl_m3': 'Gás argônio (R$/m³):',
                'labor_brl_hour': 'Mão de obra (R$/hora):',
                'machine_brl_hour': 'Máquina (R$/hora):'
            }
            for i, field in enumerate(fields):
                ttk.Label(frame, text=labels.get(field, f"{field.replace('_', ' ').capitalize()}:"))\
                    .grid(row=i, column=0, sticky='w', pady=2)
                self.vars[field] = tk.StringVar(value=self.local_config.get('costs', {}).get(field, '0.0'))
                ttk.Entry(frame, textvariable=self.vars[field]).grid(row=i, column=1, sticky='ew', padx=5)
            btn_frame = ttk.Frame(self); btn_frame.pack(fill='x', padx=10, pady=5)
            ttk.Button(btn_frame, text='Salvar', command=self.save_and_close).pack(side='right')
            ttk.Button(btn_frame, text='Cancelar', command=self.destroy).pack(side='right', padx=5)
        # --- FIM ALTERAÇÃO ---

    def save_and_close(self):
        if self.settings_type == 'powders':
            # Validar e salvar lista e ativo
            sel = self.powder_list.curselection()
            active_idx = sel[0] if sel else (0 if self._powders else None)
            # Atualizar item selecionado com campos atuais
            if active_idx is not None and 0 <= active_idx < len(self._powders):
                try:
                    name = self.vars['name'].get().strip() or 'Sem nome'
                    density = float(self.vars['density_factor_g_mm2'].get())
                    cost = float(self.vars['cost_brl_kg'].get())
                except ValueError:
                    self.parent_app.show_notification('Por favor, verifique os parâmetros. Valores numéricos inválidos.', 'warning'); return
                self._powders[active_idx]['name'] = name
                self._powders[active_idx]['density_factor_g_mm2'] = density
                self._powders[active_idx]['cost_brl_kg'] = cost
                # Atualizar lista visual
                self.powder_list.delete(active_idx)
                self.powder_list.insert(active_idx, name)
                self.powder_list.selection_clear(0, tk.END); self.powder_list.selection_set(active_idx)
                self.powder_list.activate(active_idx)
            # Validação: nomes de pó não vazios e únicos
            names = [p.get('name', '').strip() for p in self._powders]
            if any(not n for n in names):
                self.parent_app.show_notification('Todos os tipos de pó devem ter nome.', 'warning'); return
            duplicates = sorted({n for n in names if names.count(n) > 1})
            if duplicates:
                self.parent_app.show_notification(f'Nomes de pó duplicados: {", ".join(duplicates)}. Renomeie para continuar.', 'warning'); return
            # Persistir
            self.local_config['powders'] = self._powders
            if active_idx is not None and self._powders:
                self.local_config['active_powder'] = self._powders[active_idx].get('name')
            self.parent_app.save_config();
            # Atualiza imediatamente o combobox de Tipo de Pó na tela principal
            try:
                self.parent_app.refresh_powder_selector()
            except Exception:
                pass
            self.destroy(); self.parent_app.trigger_update()
        elif self.settings_type == 'formulas':
            # Ler JSON da área de texto e salvar em config['formulas']
            try:
                raw = self.formulas_text.get('1.0', 'end').strip()
                parsed = json.loads(raw) if raw else {}
                if not isinstance(parsed, dict):
                    raise ValueError('Estrutura inválida: esperado objeto JSON na raiz.')
                self.local_config['formulas'] = parsed
            except Exception as e:
                self.parent_app.show_notification(f"Erro ao salvar fórmulas: {e}", 'error'); return
            self.parent_app.save_config(); self.destroy(); self.parent_app.trigger_update()
        else:
            for key, var in self.vars.items():
                try: self.local_config[self.settings_type][key] = float(var.get())
                except ValueError: self.parent_app.show_notification("Por favor, verifique os parâmetros. Valores numéricos inválidos.", 'warning'); return
            self.parent_app.save_config(); self.destroy(); self.parent_app.trigger_update()

    # --- Métodos auxiliares para powders ---
    def _powder_on_select(self, event=None):
        sel = self.powder_list.curselection()
        if not sel: return
        idx = sel[0]
        self._powder_load(idx)

    def _powder_load(self, idx: int):
        p = self._powders[idx]
        self.vars['name'].set(p.get('name', ''))
        self.vars['density_factor_g_mm2'].set(str(p.get('density_factor_g_mm2', '')))
        self.vars['cost_brl_kg'].set(str(p.get('cost_brl_kg', '')))

    def _powder_add(self):
        # Gerar um nome único no formato "Pó N"
        base_idx = len(self._powders) + 1
        candidate = f'Pó {base_idx}'
        existing = {p.get('name', '') for p in self._powders}
        while candidate in existing:
            base_idx += 1
            candidate = f'Pó {base_idx}'
        new = {'name': candidate, 'density_factor_g_mm2': 0.16, 'cost_brl_kg': self.local_config.get('costs', {}).get('powder_brl_kg', 250.0)}
        self._powders.append(new)
        self.powder_list.insert(tk.END, new['name'])
        idx = len(self._powders)-1
        self.powder_list.selection_clear(0, tk.END); self.powder_list.selection_set(idx); self.powder_list.activate(idx)
        self._powder_load(idx)

    def _powder_remove(self):
        sel = self.powder_list.curselection()
        if not sel: return
        idx = sel[0]
        if 0 <= idx < len(self._powders):
            self._powders.pop(idx)
            self.powder_list.delete(idx)
            if self._powders:
                new_idx = max(0, idx-1)
                self.powder_list.selection_set(new_idx); self.powder_list.activate(new_idx); self._powder_load(new_idx)
            else:
                self.vars['name'].set(''); self.vars['density_factor_g_mm2'].set(''); self.vars['cost_brl_kg'].set('')

    # --- Métodos auxiliares para Fórmulas ---
    def _copy_selected_var(self):
        sel = self.vars_tree.selection()
        if not sel:
            self.parent_app.show_notification('Selecione uma variável para copiar.', 'warning'); return
        item = sel[0]
        var_name = self.vars_tree.item(item, 'values')[0]
        try:
            self.clipboard_clear(); self.clipboard_append(var_name)
            self.parent_app.show_notification(f'Variável "{var_name}" copiada para o clipboard.', 'success')
        except Exception:
            self.parent_app.show_notification('Não foi possível copiar para o clipboard.', 'error')

    def _on_var_dblclick(self, event=None):
        sel = self.vars_tree.selection()
        if not sel:
            return
        var_name = self.vars_tree.item(sel[0], 'values')[0]
        try:
            # Inserir no cursor atual
            self.formulas_text.insert('insert', var_name)
        except Exception:
            pass

    def _clear_error_highlight(self):
        try:
            self.formulas_text.tag_remove('error', '1.0', 'end')
        except Exception:
            pass

    def _apply_syntax_highlight(self):
        try:
            raw = self.formulas_text.get('1.0', 'end')
            # Limpar tags
            for tag in ('key','number','function','variable','string'):
                self.formulas_text.tag_remove(tag, '1.0', 'end')
            # Chaves JSON: "...":
            for m in re.finditer(r'\"([^\"\\]|\\.)*\"\s*:', raw):
                start_idx = f"1.0+{m.start()}c"; end_idx = f"1.0+{m.end()-1}c"
                self.formulas_text.tag_add('key', start_idx, end_idx)
            # Strings JSON
            for m in re.finditer(r'\"([^\"\\]|\\.)*\"', raw):
                start_idx = f"1.0+{m.start()}c"; end_idx = f"1.0+{m.end()}c"
                self.formulas_text.tag_add('string', start_idx, end_idx)
            # Números
            for m in re.finditer(r'\b\d+(?:\.\d+)?\b', raw):
                start_idx = f"1.0+{m.start()}c"; end_idx = f"1.0+{m.end()}c"
                self.formulas_text.tag_add('number', start_idx, end_idx)
            # Funções
            for m in re.finditer(r'\b(?:math\.[a-zA-Z_]+|min|max|abs|round|ceil|floor)\b', raw):
                start_idx = f"1.0+{m.start()}c"; end_idx = f"1.0+{m.end()}c"
                self.formulas_text.tag_add('function', start_idx, end_idx)
            # Variáveis conhecidas
            variables = set(getattr(self, '_variables_list', []) or [])
            variables.update(['gas_argon_brl_m3','labor_brl_hour','machine_brl_hour'])
            if variables:
                pattern = r'\b(?:' + '|'.join(re.escape(v) for v in sorted(variables)) + r')\b'
                for m in re.finditer(pattern, raw):
                    start_idx = f"1.0+{m.start()}c"; end_idx = f"1.0+{m.end()}c"
                    self.formulas_text.tag_add('variable', start_idx, end_idx)
        except Exception:
            pass

    def _update_variable_usage(self):
        try:
            raw = self.formulas_text.get('1.0', 'end')
            items = self.vars_tree.get_children()
            for item in items:
                vals = list(self.vars_tree.item(item, 'values'))
                var_name = vals[0]
                used = bool(re.search(r'\b' + re.escape(var_name) + r'\b', raw))
                vals = [vals[0], vals[1], '✓' if used else '']
                self.vars_tree.item(item, values=vals)
        except Exception:
            pass

    def _copy_selected_results(self, event=None):
        try:
            sel = self.results_tree.selection()
            items = sel if sel else self.results_tree.get_children()
            lines = []
            for item in items:
                vals = self.results_tree.item(item, 'values')
                if not vals:
                    continue
                if len(vals) >= 2:
                    lines.append(f"{vals[0]}\t{vals[1]}")
                else:
                    lines.append("\t".join(map(str, vals)))
            text = "\n".join(lines)
            if not text:
                self.parent_app.show_notification('Nenhum resultado para copiar.', 'warning')
                return 'break'
            self.clipboard_clear(); self.clipboard_append(text)
            self.parent_app.show_notification('Resultados copiados para o clipboard.', 'success')
        except Exception:
            self.parent_app.show_notification('Não foi possível copiar resultados.', 'error')
        return 'break'

    def _validate_json(self):
        raw = self.formulas_text.get('1.0', 'end').strip()
        self._clear_error_highlight()
        try:
            parsed = json.loads(raw) if raw else {}
            if not isinstance(parsed, dict):
                raise ValueError('Estrutura inválida: esperado objeto JSON na raiz.')
            # Validação adicional: chaves string não vazias; valores str|int|float
            bad_keys = [k for k in parsed.keys() if not isinstance(k, str) or not k]
            bad_vals = [k for k, v in parsed.items() if not isinstance(v, (str, int, float))]
            if bad_keys:
                raise ValueError(f"Chaves inválidas (devem ser strings não vazias): {', '.join(map(str,bad_keys))}")
            if bad_vals:
                raise ValueError(f"Valores inválidos em chaves: {', '.join(map(str,bad_vals))}. Use string de expressão ou número.")
            self.parent_app.show_notification('JSON válido.', 'success')
            try: self._set_editor_status('Válido', 'success')
            except Exception: pass
            return True
        except json.JSONDecodeError as e:
            # Realçar linha com erro
            lineno = getattr(e, 'lineno', None); colno = getattr(e, 'colno', 1)
            if lineno:
                start = f"{lineno}.0"; end = f"{lineno}.end"
                try: self.formulas_text.tag_add('error', start, end)
                except Exception: pass
            self.parent_app.show_notification(f"Erro de JSON: linha {getattr(e,'lineno', '?')}, coluna {getattr(e,'colno','?')}: {e.msg}", 'error')
            return False
        except Exception as e:
            self.parent_app.show_notification(f"Erro: {e}", 'error')
            try: self._set_editor_status('Erro', 'error')
            except Exception: pass
            return False

    def _format_json(self):
        raw = self.formulas_text.get('1.0', 'end').strip()
        self._clear_error_highlight()
        try:
            parsed = json.loads(raw) if raw else {}
            formatted = json.dumps(parsed, indent=2, ensure_ascii=False)
            self.formulas_text.delete('1.0', 'end'); self.formulas_text.insert('1.0', formatted + "\n")
            self.parent_app.show_notification('JSON formatado.', 'success')
            try:
                self._set_editor_status('Válido', 'success') if self._validate_json() else self._set_editor_status('Erro', 'error')
            except Exception:
                pass
        except Exception as e:
            self.parent_app.show_notification(f"Falha ao formatar: {e}", 'error')
            try: self._set_editor_status('Erro', 'error')
            except Exception: pass

    def _evaluate_formulas(self):
        # Tentar obter e interpretar o bloco de fórmulas
        raw = self.formulas_text.get('1.0', 'end').strip()
        self._clear_error_highlight()
        try:
            parsed = json.loads(raw) if raw else {}
        except json.JSONDecodeError as e:
            lineno = getattr(e, 'lineno', None)
            if lineno:
                start = f"{lineno}.0"; end = f"{lineno}.end"
                try: self.formulas_text.tag_add('error', start, end)
                except Exception: pass
            self.parent_app.show_notification(f"Erro de JSON: linha {getattr(e,'lineno','?')} coluna {getattr(e,'colno','?')}: {e.msg}", 'error')
            try: self._set_editor_status('Erro', 'error')
            except Exception: pass
            return
        except Exception as e:
            self.parent_app.show_notification(f"Erro: {e}", 'error'); return

        # Extrair o dicionário de expressões
        if isinstance(parsed, dict) and 'expressions' in parsed and isinstance(parsed['expressions'], dict):
            expressions = dict(parsed['expressions'])
        elif isinstance(parsed, dict):
            expressions = {k: v for k, v in parsed.items() if isinstance(v, str)}
        else:
            self.parent_app.show_notification('Estrutura de fórmulas inválida.', 'warning'); return

        # Obter parâmetros atuais da aplicação
        try:
            params = self.parent_app._get_current_params()
        except Exception:
            params = {}

        # Atualizar uso de variáveis na UI
        try:
            self._update_variable_usage()
        except Exception:
            pass

        # Contexto seguro para avaliação
        safe_globals = {'__builtins__': {}}
        # Custos e pó ativos vindos do config
        costs = self.parent_app.config.get('costs', {})
        # Garantir variáveis de custo no ambiente
        safe_locals = {
            **params,
            'math': math,
            'pi': math.pi,
            'sin': math.sin,
            'cos': math.cos,
            'tan': math.tan,
            'abs': abs,
            'round': round,
            'min': min,
            'max': max,
            'ceil': math.ceil,
            'floor': math.floor,
            'gas_argon_brl_m3': costs.get('gas_argon_brl_m3', 0.0),
            'labor_brl_hour': costs.get('labor_brl_hour', 0.0),
            'machine_brl_hour': costs.get('machine_brl_hour', 0.0)
        }

        # Detectar variáveis desconhecidas por expressão (considerando dependências entre fórmulas)
        known_names = set(list(safe_locals.keys())) | set(expressions.keys())
        unknown_by_key = {}
        for key, expr in expressions.items():
            try:
                tokens = set(re.findall(r'[A-Za-z_][A-Za-z0-9_\.]*', expr))
                # Remover nomes do módulo math e funções
                tokens.discard('math')
                for t in list(tokens):
                    if t.startswith('math.'):
                        tokens.discard(t)
                # Ignorar 'e'/'E' usado em notação científica (ex.: 1e-6)
                tokens.discard('e'); tokens.discard('E')
                unknowns = {t for t in tokens if t not in known_names}
                if unknowns:
                    unknown_by_key[key] = sorted(unknowns)
            except Exception:
                pass

        # Avaliação iterativa para resolver dependências entre fórmulas
        results = {}
        pending = dict(expressions)
        for _ in range(6):  # até 6 passes
            progressed = False
            keys = list(pending.keys())
            for key in keys:
                expr = pending[key]
                try:
                    val = eval(expr, safe_globals, {**safe_locals, **results})
                    results[key] = val
                    del pending[key]
                    progressed = True
                except NameError:
                    # Pode depender de outra fórmula ainda não resolvida; tentar em próximo passe
                    continue
                except Exception as e:
                    results[key] = f"Erro: {e}"
                    del pending[key]
                    progressed = True
            if not progressed:
                break

        # Preencher a tabela de resultados
        try:
            for i in self.results_tree.get_children():
                self.results_tree.delete(i)
        except Exception:
            pass
        for k, v in results.items():
            self.results_tree.insert('', tk.END, values=(k, v))
        # Itens pendentes não resolvidos
        for k in pending.keys():
            self.results_tree.insert('', tk.END, values=(k, 'Não resolvido (ver dependências)'))
        # Adicionar avisos de variáveis desconhecidas
        if unknown_by_key:
            for k, unk in unknown_by_key.items():
                self.results_tree.insert('', tk.END, values=(f"{k} (aviso)", f"Variáveis desconhecidas: {', '.join(unk)}"))
            self.parent_app.show_notification('Avaliação concluída com avisos de variáveis.', 'warning')
        else:
            self.parent_app.show_notification('Avaliação concluída.', 'success')
        try: self._set_editor_status('Válido', 'success')
        except Exception: pass

    def _restore_default_formulas(self):
        # Confirmação antes de restaurar
        try:
            if not messagebox.askyesno('Confirmar', 'Restaurar o template padrão de fórmulas no editor? (não salva automaticamente)'):
                return
        except Exception:
            pass
        try:
            default_block = self.parent_app.get_default_formulas()
        except Exception:
            default_block = {}
        try:
            rendered = json.dumps(default_block, indent=2, ensure_ascii=False)
            self.formulas_text.delete('1.0', 'end')
            self.formulas_text.insert('1.0', rendered + "\n")
            self._clear_error_highlight()
            self.parent_app.show_notification('Fórmulas padrão restauradas no editor. Clique em Salvar para persistir.', 'info')
            try: self._set_editor_status('Não salvo', 'pending')
            except Exception: pass
        except Exception as e:
            self.parent_app.show_notification(f'Falha ao restaurar padrão: {e}', 'error')

    def _set_editor_status(self, text: str, kind: str = 'info'):
        """Atualiza o status do editor (texto e cor). kind: success|error|pending|info"""
        try:
            self._editor_status_var.set(text)
            color_map = {
                'success': '#2b8a3e',
                'error': '#b00020',
                'pending': '#c47f17',
                'info': '#1a73e8'
            }
            fg = color_map.get(kind, '#1a73e8')
            try:
                self._editor_status_lbl.configure(foreground=fg)
            except Exception:
                pass
        except Exception:
            pass

    def _export_formulas(self):
        """Exporta apenas o bloco de fórmulas para um arquivo JSON."""
        try:
            raw = self.formulas_text.get('1.0', 'end').strip()
            parsed = json.loads(raw) if raw else {}
        except Exception as e:
            self.parent_app.show_notification(f'Não é possível exportar: JSON inválido ({e})', 'error'); return
        try:
            filepath = filedialog.asksaveasfilename(defaultextension='.json', filetypes=[('JSON','*.json')], title='Exportar fórmulas')
            if not filepath:
                return
            with open(filepath, 'w', encoding='utf-8') as f:
                json.dump(parsed, f, indent=2, ensure_ascii=False)
            self.parent_app.show_notification('Fórmulas exportadas.', 'success')
        except Exception as e:
            self.parent_app.show_notification(f'Erro ao exportar: {e}', 'error')

    def _import_formulas(self):
        """Importa um arquivo JSON e carrega no editor (não salva automaticamente)."""
        try:
            filepath = filedialog.askopenfilename(filetypes=[('JSON','*.json')], title='Importar fórmulas')
            if not filepath:
                return
            with open(filepath, 'r', encoding='utf-8') as f:
                data = json.load(f)
            if not isinstance(data, dict):
                raise ValueError('Arquivo inválido: esperado objeto JSON na raiz.')
            rendered = json.dumps(data, indent=2, ensure_ascii=False)
            self.formulas_text.delete('1.0', 'end'); self.formulas_text.insert('1.0', rendered + "\n")
            self._apply_syntax_highlight(); self._update_variable_usage()
            self.parent_app.show_notification('Fórmulas importadas no editor. Clique em Salvar para persistir.', 'info')
            try: self._set_editor_status('Não salvo', 'pending')
            except Exception: pass
        except Exception as e:
            self.parent_app.show_notification(f'Erro ao importar: {e}', 'error')

# Versão do aplicativo para controle de atualização
APP_VERSION = "1.0.4"

# --- CLASSE PRINCIPAL ---
class TFM_GCODE:
    def __init__(self, root, on_ready=None):
        self.root = root
        # --- REMOVIDO: self.lang = LanguageManager() ---
        self.gcode_generator = GCodeGenerator()
        self.config = self.load_config()
        self.anim = None
        self._update_job_id = None
        self._debounce_delay = 300
        self._notification_job_id = None
        self._last_gcode_line_count = 0
        self._gcode_preview_thread_running = False
        # Aplica escala dinâmica de DPI antes de construir a UI
        try:
            self._apply_dynamic_scaling()
        except Exception:
            pass
        self.root.state('zoomed')
        self._create_widgets()
        self._setup_styles()
        self._update_ui_text() # Agora apenas define os textos
        # Restaura sashes quando a janela já tem dimensões
        try:
            self.root.after(200, self._restore_sash_positions)
        except Exception:
            pass
        self.root.after(100, self.trigger_update)
        # Checagem de atualização após inicialização
        try:
            self.root.after(2000, lambda: self._check_for_updates(silent=True))
        except Exception:
            pass
        # Notifica que a UI está pronta para esconder Splash
        try:
            if callable(on_ready):
                # Usa after_idle para garantir que primeiros frames já renderizaram
                self.root.after_idle(on_ready)
        except Exception:
            pass

    def get_default_formulas(self):
        # Template padrão de fórmulas do aplicativo (completo)
        return {
            "expressions": {
                # Geometria e movimento
                "circumference_mm": "math.pi * diametro",
                "rotation_rpm": "(velocidade_de_deposicao / max(circumference_mm, 1e-6)) * 60",
                "rotation_mm_min": "circumference_mm * rotation_rpm",
                "helix_pitch_mm": "velocidade_de_deposicao / max(rotation_rpm, 1e-6)",
                "effective_step_mm": "largura_cordao * (1 - (sobreposicao/100))",
                "axial_passes_per_layer": "math.ceil(comprimento_revestir / max(effective_step_mm, 1e-6))",
                "total_length_mm_per_layer": "circumference_mm * axial_passes_per_layer",
                "total_length_mm_all_layers": "total_length_mm_per_layer * num_camadas",

                # Tempo e rotações
                "time_min": "total_length_mm_all_layers / max(velocidade_de_deposicao, 1e-6)",
                "rotations_total": "rotation_rpm * (time_min/60.0)",

                # Consumos
                "gas_consumption_l": "vazao_gas * time_min",
                # Consumo de pó (kg) — alinhado com a UI
                # área coberta = circunferência * (comprimento + leads) * num_camadas (mm²)
                # massa (g) = área (mm²) * powder_factor (g/mm²)
                # massa (kg) = massa_g / 1000
                "powder_mass_kg": "((circumference_mm * (comprimento_revestir + lead_in + lead_out) / 10000) * (num_camadas * espessura_camada) * powder_factor)",
                # Também expor em gramas para conveniência
                "powder_mass_g": "powder_mass_kg * 1000",

                # Taxa de deposição por massa: entrada em g/h e derivada em kg/h
                "taxa_deposicao_kg_h": "taxa_deposicao_g_h / 1000.0",
                # Tempo baseado em consumo de pó e taxa (minutos)
                "time_min_mass": "(powder_mass_kg / max(taxa_deposicao_kg_h, 1e-6)) * 60.0",

                # Custos
                "powder_cost_brl": "powder_mass_kg * powder_cost_brl_kg",
                "labor_cost_brl": "(time_min/60.0) * labor_brl_hour",
                "machine_cost_brl": "(time_min/60.0) * machine_brl_hour",
                "gas_cost_brl": "(gas_consumption_l/1000.0) * gas_argon_brl_m3",
                "total_cost_brl": "powder_cost_brl + labor_cost_brl + machine_cost_brl + gas_cost_brl"
            },
            "notes": "Variáveis disponíveis: diametro, comprimento_revestir, lead_in, lead_out, largura_cordao, sobreposicao, velocidade_de_deposicao, num_camadas, espessura_camada, vazao_gas, powder_factor (g/mm²), powder_cost_brl_kg, labor_brl_hour, machine_brl_hour, gas_argon_brl_m3, taxa_deposicao_g_h, taxa_deposicao_kg_h, time_min_mass. Funções: math, pi, sin, cos, tan, abs, round, min, max, ceil, floor. Consumo de pó calculado por área e espessura * fator."
        }

    def show_notification(self, message, msg_type='info', duration_ms=5000):
        if self._notification_job_id: self.root.after_cancel(self._notification_job_id); self._notification_job_id = None
        style_map = {'info': 'Info.TLabel', 'success': 'Success.TLabel', 'warning': 'Warning.TLabel', 'error': 'Error.TLabel'}; style_name = style_map.get(msg_type, 'Info.TLabel')
        self.notification_label.config(text=message, style=style_name); self.notification_frame.grid()
        if msg_type != 'error' and duration_ms > 0: self._notification_job_id = self.root.after(duration_ms, self.hide_notification)

    def hide_notification(self):
        self.notification_frame.grid_remove(); self.notification_label.config(text="")
        if self._notification_job_id: self.root.after_cancel(self._notification_job_id); self._notification_job_id = None

    def trigger_update(self, *args):
        if self._update_job_id: self.root.after_cancel(self._update_job_id)
        self._update_job_id = self.root.after(self._debounce_delay, self._perform_update)

    def _perform_update(self):
        self._update_job_id = None; self.executar_calculos_e_desenho(); self._update_gcode_preview_async()
        self._update_temporal_plot(); self._update_oscillation_plot(); self._update_statistics_plot(); self._update_process_plots()

    def load_config(self, filepath=None):
        # Resolve caminho padrão conforme estrutura de pastas nova e executável congelado
        from pathlib import Path
        if filepath is None:
            if getattr(sys, 'frozen', False):
                base_dir = Path(sys.executable).resolve().parent
                # Preferir arquivo em subpasta 'config/' distribuído pela .spec; cair para raiz se não existir
                primary = base_dir / 'config.json'
                secondary = base_dir / 'config' / 'config.json'
                if secondary.exists():
                    filepath = str(secondary)
                elif primary.exists():
                    filepath = str(primary)
                else:
                    filepath = str(secondary)
            else:
                # src/app/TFM_GCODE.py -> projeto raiz está 2 níveis acima
                base_dir = Path(__file__).resolve().parents[2]
                filepath = str(base_dir / 'config' / 'config.json')

        default_config = {
            "ui": {
                "sashes": {
                    "main": None,
                    "top_view": None
                },
                "dpi_scaling": "auto"
            },
            "update": {
                "enabled": True,
                "check_on_start": True,
                # Em produção, apontar para URL http(s); aqui definimos um padrão seguro
                "feed_url": "https://thyago0730.github.io/tfm-gcode-python/latest.json"
            },
            "costs": {
                "powder_brl_kg": 250.0,
                "gas_argon_brl_m3": 50.0,
                "labor_brl_hour": 40.0,
                "machine_brl_hour": 30.0,
                "currency_symbol": "R$"
            },
            "powders": [
                {
                    "name": "Pó padrão",
                    "density_factor_g_mm2": 0.16,
                    "cost_brl_kg": 250.0
                }
            ],
            "active_powder": "Pó padrão",
            "formulas": {
                "expressions": {
                    "circumference_mm": "math.pi * diametro",
                    "rotation_rpm": "(velocidade_de_deposicao / max(circumference_mm, 1e-6)) * 60",
                    "rotation_mm_min": "circumference_mm * rotation_rpm",
                "helix_pitch_mm": "velocidade_de_deposicao / max(rotation_rpm, 1e-6)",
                    "powder_mass_kg": "((circumference_mm * (comprimento_revestir + lead_in + lead_out) / 10000) * (num_camadas * espessura_camada) * powder_factor)",
                    "powder_mass_g": "powder_mass_kg * 1000"
                },
                "notes": "Edite as expressões em JSON. Variáveis disponíveis: diametro, comprimento_revestir, lead_in, lead_out, largura_cordao, sobreposicao, velocidade_de_deposicao, num_camadas, espessura_camada, vazao_gas, powder_factor (g/mm²), powder_cost_brl_kg, labor_brl_hour, machine_brl_hour, gas_argon_brl_m3."
            },
            "database": {
                # Novo padrão: data/procedures
                "procedures_path": "data/procedures"
            },
            "integration": {
                "mach3_path": "",
                "gcode_dir": "C:/Mach3/GCode",
                "write_autoload_txt": True,
                "allow_file_association_fallback": False,
                "mach3_profile": "PTA"
            }
        }

        config = copy.deepcopy(default_config)
        try:
            with open(filepath, 'r', encoding='utf-8') as f:
                user_config = json.load(f)
            for key in default_config:
                if key in user_config:
                    if isinstance(config[key], dict) and isinstance(user_config.get(key), dict):
                        config[key].update(user_config[key])
                    else:
                        config[key] = user_config[key]
            # Remover chave obsoleta se existir no arquivo do usuário
            if 'machine_limits' in config:
                try:
                    del config['machine_limits']
                except Exception:
                    pass
        except (FileNotFoundError, json.JSONDecodeError):
            pass
        # Normaliza feed de atualização: se vazio, aplica padrão seguro
        try:
            upd = config.get('update') or {}
            if not (upd.get('feed_url') or '').strip():
                upd['feed_url'] = "https://thyago0730.github.io/tfm-gcode-python/latest.json"
                config['update'] = upd
        except Exception:
            pass

        self.save_config(filepath, config)
        return config

    # --- Atualização automática ---
    def _parse_version(self, v: str):
        try:
            return tuple(int(x) for x in str(v).strip().split('.'))
        except Exception:
            return (0,)

    def _get_update_feed_url(self):
        try:
            url = (self.config.get('update') or {}).get('feed_url') or ''
        except Exception:
            url = ''
        if url:
            return url
        # Fallback: arquivo local em desenvolvimento
        try:
            base = self._project_root()
            local_json = base / 'docs' / 'latest.json'
            if local_json.exists():
                return str(local_json)
        except Exception:
            pass
        return ''

    def _check_for_updates(self, silent: bool = False):
        try:
            upd = self.config.get('update') or {}
            if not upd.get('enabled', True):
                return
            if not upd.get('check_on_start', True) and silent:
                return
            feed = self._get_update_feed_url()
            if not feed:
                if not silent:
                    try:
                        self.show_notification('Feed de atualização não configurado.', 'warning')
                    except Exception:
                        pass
                return

            # Carrega latest.json (arquivo local ou via HTTP)
            data = None
            try:
                if '://' in feed and feed.lower().startswith(('http://', 'https://')):
                    with urllib.request.urlopen(feed, timeout=5) as resp:
                        raw = resp.read().decode('utf-8', errors='replace')
                        data = json.loads(raw)
                else:
                    # caminho local
                    with open(feed, 'r', encoding='utf-8') as f:
                        data = json.load(f)
            except Exception:
                if not silent:
                    try:
                        self.show_notification('Falha ao checar atualização.', 'error')
                    except Exception:
                        pass
                return

            remote_version = str(data.get('version') or '')
            installer_url = str(data.get('url') or '')
            sha256_expected = (data.get('sha256') or '').strip()
            if not remote_version:
                return
            if self._parse_version(remote_version) <= self._parse_version(APP_VERSION):
                if not silent:
                    try:
                        self.show_notification('Você já está na última versão.', 'info')
                    except Exception:
                        pass
                return

            # Há atualização disponível
            try:
                if not messagebox.askyesno('Atualização disponível', f'Nova versão {remote_version} disponível. Deseja atualizar agora?'):
                    return
            except Exception:
                # Se não puder mostrar o diálogo, segue silencioso
                pass

            if not installer_url:
                try:
                    self.show_notification('URL do instalador ausente.', 'error')
                except Exception:
                    pass
                return

            # Baixa instalador para pasta temporária
            try:
                tmp_dir = Path(tempfile.gettempdir())
                fname = 'TFM_GCODE_Setup.exe'
                dest = tmp_dir / fname
                with urllib.request.urlopen(installer_url, timeout=30) as resp:
                    content = resp.read()
                with open(dest, 'wb') as outf:
                    outf.write(content)
                # Valida SHA-256, se fornecido
                if sha256_expected:
                    h = hashlib.sha256()
                    h.update(content)
                    digest = h.hexdigest()
                    if digest.lower() != sha256_expected.lower():
                        try:
                            self.show_notification('Falha de integridade do instalador (hash).', 'error')
                        except Exception:
                            pass
                        return
                # Executa instalador e encerra app
                try:
                    subprocess.Popen([str(dest)], shell=True)
                except Exception:
                    try:
                        self.show_notification('Não foi possível iniciar o instalador.', 'error')
                    except Exception:
                        pass
                    return
                try:
                    self.root.quit()
                except Exception:
                    pass
            except Exception:
                try:
                    self.show_notification('Falha ao baixar o instalador.', 'error')
                except Exception:
                    pass
        except Exception:
            # Não interrompe a aplicação por falha de update
            pass

    def save_config(self, filepath=None, config_data=None):
        from pathlib import Path
        data_to_save = config_data if config_data is not None else self.config
        try:
            if filepath is None:
                if getattr(sys, 'frozen', False):
                    base_dir = Path(sys.executable).resolve().parent
                    filepath = str(base_dir / 'config.json')
                else:
                    base_dir = Path(__file__).resolve().parents[2]
                    filepath = str(base_dir / 'config' / 'config.json')

            dir_name = os.path.dirname(filepath)
            if dir_name and not os.path.exists(dir_name):
                os.makedirs(dir_name)

            # Garantir existência de data/procedures (absoluto)
            proc_conf = data_to_save.get('database', {}).get('procedures_path', 'data/procedures')
            proc_path = Path(proc_conf)
            if not proc_path.is_absolute():
                if getattr(sys, 'frozen', False):
                    root_dir = Path(sys.executable).resolve().parent
                else:
                    root_dir = Path(__file__).resolve().parents[2]
                proc_path = root_dir / proc_path
            os.makedirs(proc_path, exist_ok=True)

            with open(filepath, 'w', encoding='utf-8') as f:
                json.dump(data_to_save, f, indent=4)
        except Exception as e:
            self.show_notification(f"Não foi possível salvar a configuração: {e}", 'error')

    # --- Escala dinâmica de DPI ---
    def _apply_dynamic_scaling(self):
        """Ajusta automaticamente a escala do Tk com base no DPI real.
        Usa winfo_fpixels('1i') para obter pixels por polegada e define tk scaling
        como pixels por ponto (1/72 polegada).
        """
        try:
            fpixels = self.root.winfo_fpixels('1i')  # pixels por polegada
            if fpixels and fpixels > 0:
                scaling = fpixels / 72.0
                self.root.tk.call('tk', 'scaling', scaling)
        except Exception:
            pass

    def _evaluate_formulas_runtime(self, params):
        """Avalia o bloco de fórmulas presente em self.config e retorna um dict.
        Usa o mesmo ambiente seguro do editor, permitindo dependências entre chaves.
        """
        block = self.config.get('formulas', {}) or {}
        # Extrair expressões
        if isinstance(block, dict) and 'expressions' in block and isinstance(block['expressions'], dict):
            expressions = dict(block['expressions'])
        elif isinstance(block, dict):
            expressions = {k: v for k, v in block.items() if isinstance(v, str)}
        else:
            return {}

        costs = self.config.get('costs', {})
        safe_globals = {'__builtins__': {}}
        safe_locals = {
            **params,
            'math': math,
            'pi': math.pi,
            'sin': math.sin,
            'cos': math.cos,
            'tan': math.tan,
            'abs': abs,
            'round': round,
            'min': min,
            'max': max,
            'ceil': math.ceil,
            'floor': math.floor,
            'gas_argon_brl_m3': costs.get('gas_argon_brl_m3', 0.0),
            'labor_brl_hour': costs.get('labor_brl_hour', 0.0),
            'machine_brl_hour': costs.get('machine_brl_hour', 0.0),
            'powder_cost_brl_kg': params.get('powder_cost_brl_kg', costs.get('powder_brl_kg', 0.0)),
        }

        results = {}
        pending = dict(expressions)
        for _ in range(6):
            progressed = False
            for key in list(pending.keys()):
                expr = pending[key]
                try:
                    val = eval(expr, safe_globals, {**safe_locals, **results})
                    results[key] = val
                    del pending[key]
                    progressed = True
                except NameError:
                    continue
                except Exception:
                    # Armazena o erro para diagnóstico, mas não interrompe
                    results[key] = None
                    del pending[key]
                    progressed = True
            if not progressed:
                break
        return results

    def refresh_powder_selector(self):
        """Atualiza os valores do combobox de Tipo de Pó com base em self.config.
        Reajusta a seleção para o pó ativo se existir.
        """
        try:
            # Se o combobox ainda não foi criado, retorna silenciosamente
            if not hasattr(self, 'powder_combo'):
                return
            powder_names = [p.get('name') for p in self.config.get('powders', []) if p.get('name')]
            self.powder_combo['values'] = powder_names
            active_name = self.config.get('active_powder')
            current = self.params.get('powder_name').get() if 'powder_name' in self.params else None
            if active_name and active_name in powder_names:
                self.params['powder_name'].set(active_name)
            elif current in powder_names:
                # mantém o atual se ainda existir na lista
                self.params['powder_name'].set(current)
            elif powder_names:
                # define o primeiro da lista quando não há ativo
                self.params['powder_name'].set(powder_names[0])
            self.trigger_update()
        except Exception:
            # Evita quebrar o fluxo por erros de UI durante refresh
            pass

    def _create_menu(self):
        self.menubar = tk.Menu(self.root); self.root.config(menu=self.menubar)
        self.file_menu = tk.Menu(self.menubar, tearoff=0); self.settings_menu = tk.Menu(self.menubar, tearoff=0)
        # --- REMOVIDO: self.language_menu ---
        self.help_menu = tk.Menu(self.menubar, tearoff=0)
        # Botão 'Simuladores' removido (menus e funcionalidades de simulação desativados)

        # Menu Configurações
        self.settings_menu.add_command(label="Caminho (Mach3)...", command=self._select_mach3_path)
        self.settings_menu.add_command(label="Instalar macropump (autoload)", command=self._install_macropump)
        self.settings_menu.add_separator()
        self.settings_menu.add_command(label="Custos de Operação...", command=lambda: SettingsWindow(self, self.config, 'costs'))
        self.settings_menu.add_command(label="Tipos de Pó...", command=lambda: SettingsWindow(self, self.config, 'powders'))
        self.settings_menu.add_command(label="Fórmulas...", command=lambda: SettingsWindow(self, self.config, 'formulas'))
        self.menubar.add_cascade(label="Configurações", menu=self.settings_menu)

    # --- Funções de validação ---
    def _validate_float(self, proposed: str) -> bool:
        if proposed == "" or proposed == "-": return True
        try: float(proposed); return True
        except ValueError: return False

    def _validate_int(self, proposed: str) -> bool:
        if proposed == "": return True
        return proposed.isdigit()

    def _create_widgets(self):
        self._create_menu(); main_frame = ttk.Frame(self.root, padding="10"); main_frame.grid(row=0, column=0, sticky="nsew")
        self.root.columnconfigure(0, weight=1); self.root.rowconfigure(0, weight=1); self.root.rowconfigure(1, weight=0)
        self.main_paned_window = PanedWindow(main_frame, orient=tk.HORIZONTAL, sashrelief=tk.RAISED, sashwidth=6); self.main_paned_window.grid(row=0, column=0, sticky="nsew")
        main_frame.columnconfigure(0, weight=1); main_frame.rowconfigure(0, weight=1)
        controls_outer_frame = ttk.Frame(self.main_paned_window, width=350); self.main_paned_window.add(controls_outer_frame, stretch="never")
        # Referência para alternar visibilidade
        self.controls_outer_frame = controls_outer_frame
        # Área de visualização sem PanedWindow vertical - layout fixo
        self.view_area = ttk.Frame(self.main_paned_window); self.main_paned_window.add(self.view_area, stretch="always")
        self.view_area.columnconfigure(0, weight=1); self.view_area.rowconfigure(0, weight=1); self.view_area.rowconfigure(1, weight=0)
        self.top_view_pane = PanedWindow(self.view_area, orient=tk.HORIZONTAL, sashrelief=tk.RAISED, sashwidth=6); self.top_view_pane.grid(row=0, column=0, sticky="nsew")
        # Cabeçalho removido: sem seta para alternar painel esquerdo
        self._sash_configured = False
        def configure_panes(event=None):
             if not self._sash_configured and self.main_paned_window.winfo_width() > 1 and self.top_view_pane.winfo_width() > 1:
                 try:
                     main_pane_width = self.main_paned_window.winfo_width(); controls_width = int(main_pane_width * 0.25); self.main_paned_window.sash_place(0, controls_width, 0)
                     top_view_pane_width = self.top_view_pane.winfo_width(); gcode_width = int(top_view_pane_width * 0.30); self.top_view_pane.sash_place(0, top_view_pane_width - gcode_width, 0)
                     try:
                         self._sash_top = self.top_view_pane.sash_coord(0)
                     except tk.TclError:
                         self._sash_top = None
                     self._sash_configured = True
                 except tk.TclError: self.root.after(200, configure_panes)
                 except Exception as e: print(f"Erro ao configurar sashes: {e}")
        self.main_paned_window.bind("<Configure>", configure_panes, add='+')
        scrolled_frame = ScrolledFrame(controls_outer_frame); scrolled_frame.pack(fill="both", expand=True)
        interior = scrolled_frame.interior
        self.notebook = ttk.Notebook(interior); self.notebook.pack(fill="x", expand=True, padx=5, pady=5)
        self.notebook.bind("<<NotebookTabChanged>>", self.trigger_update)
        self.tab_espiral = ttk.Frame(self.notebook, padding="10"); self.tab_osc = ttk.Frame(self.notebook, padding="10")
        # Responsividade: entradas (coluna 1) expandem horizontalmente
        try:
            self.tab_espiral.columnconfigure(0, weight=0); self.tab_espiral.columnconfigure(1, weight=1)
            self.tab_osc.columnconfigure(0, weight=0); self.tab_osc.columnconfigure(1, weight=1)
        except Exception:
            pass
        self.notebook.add(self.tab_espiral, text="Espiral"); self.notebook.add(self.tab_osc, text="Oscilação")
        self.params = {}
        self.params['comprimento'] = tk.StringVar(value="200.0"); self.params['largura_cordao'] = tk.StringVar(value="8.0"); self.params['sobreposicao'] = tk.StringVar(value="40.0")
        # Renomeado: taxa_de_deposicao -> velocidade_de_deposicao
        self.params['velocidade_de_deposicao'] = tk.StringVar(value="120.0"); self.params['afastamento'] = tk.StringVar(value="12.0"); self.params['lead_in'] = tk.StringVar(value="5.0"); self.params['lead_out'] = tk.StringVar(value="5.0")
        self.params['direcao_soldagem'] = tk.StringVar(value='Esquerda -> Direita') # Usa valor de display direto
        self.params['sentido_rotacao'] = tk.StringVar(value='Horária (CW)') # Usa valor de display direto
        self.params['nome_procedimento'] = tk.StringVar(value="Novo Procedimento"); self.params['corrente_arco'] = tk.StringVar(value="180.0"); self.params['vazao_gas'] = tk.StringVar(value="15.0")
        self.params['alim_po'] = tk.StringVar(value="50.0"); self.params['preaquecimento'] = tk.StringVar(value="100.0")
        # Nova taxa de deposição por massa (g/h)
        self.params['taxa_deposicao_g_h'] = tk.StringVar(value="700.0")
        # Velocidade da oscilação (mm/min) desacoplada — padrão 1000.0
        self.params['velocidade_oscilacao_mm_min'] = tk.StringVar(value="1000.0")
        self.params['num_camadas'] = tk.StringVar(value="1"); self.params['espessura_camada'] = tk.StringVar(value="1.5")
        self.params['diametro'] = tk.StringVar(value="50.0")
        self.params['oscilacao_comprimento'] = tk.StringVar(value="10.0")
        self.params['deslocamento_angular_perc'] = tk.StringVar(value="50.0")
        # Tipo de oscilação (Linear/Quadrada)
        self.params['tipo_oscilacao'] = tk.StringVar(value="Linear")
        # Modo compacto de G-code
        self.params['compact_gcode'] = tk.BooleanVar(value=False)
        # Controle: recuar tocha ao ligar
        self.params['torch_retract_on_ignite'] = tk.BooleanVar(value=True)
        # Removido: controle de rotação por spindle; sempre usar Eixo A

        vcmd_float = (self.root.register(self._validate_float), '%P')
        vcmd_int = (self.root.register(self._validate_int), '%P')

        self.espiral_diametro_label = ttk.Label(self.tab_espiral, text="Diâmetro (mm):"); self.espiral_diametro_label.grid(row=0, column=0, sticky="w", padx=8, pady=6)
        ttk.Entry(self.tab_espiral, textvariable=self.params['diametro'], validate='key', validatecommand=vcmd_float).grid(row=0, column=1, sticky="ew", padx=8, pady=6)

        self.osc_diametro_label = ttk.Label(self.tab_osc, text="Diâmetro (mm):"); self.osc_diametro_label.grid(row=0, column=0, sticky="w", padx=8, pady=6)
        ttk.Entry(self.tab_osc, textvariable=self.params['diametro'], validate='key', validatecommand=vcmd_float).grid(row=0, column=1, sticky="ew", padx=8, pady=6)
        self.osc_comp_label = ttk.Label(self.tab_osc, text="Comprimento Osc. (mm):"); self.osc_comp_label.grid(row=1, column=0, sticky="w", padx=8, pady=6)
        ttk.Entry(self.tab_osc, textvariable=self.params['oscilacao_comprimento'], validate='key', validatecommand=vcmd_float).grid(row=1, column=1, sticky="ew", padx=8, pady=6)
        self.osc_desloc_label = ttk.Label(self.tab_osc, text="Desloc. Angular (%):"); self.osc_desloc_label.grid(row=2, column=0, sticky="w", padx=8, pady=6)
        ttk.Entry(self.tab_osc, textvariable=self.params['deslocamento_angular_perc'], validate='key', validatecommand=vcmd_float).grid(row=2, column=1, sticky="ew", padx=8, pady=6)
        # Seletor do tipo de oscilação
        self.osc_tipo_label = ttk.Label(self.tab_osc, text="Tipo de Oscilação:"); self.osc_tipo_label.grid(row=3, column=0, sticky="w", padx=8, pady=6)
        self.osc_tipo_combo = ttk.Combobox(self.tab_osc, textvariable=self.params['tipo_oscilacao'], values=['Linear', 'Quadrada', 'Quadrada Contínua'], state='readonly')
        self.osc_tipo_combo.grid(row=3, column=1, sticky="ew", padx=8, pady=6)
        # Checkbox para modo compacto de G-code
        self.compact_gcode_check = ttk.Checkbutton(self.tab_osc, text="Compactar G-code", variable=self.params['compact_gcode'])
        self.compact_gcode_check.grid(row=4, column=0, columnspan=2, sticky="w", padx=8, pady=6)

        # Granularidade (X) para Oscilação Quadrada Contínua
        # Permite controlar o deslocamento por subpasso na decomposição em "escada"
        self.params['osc_test_gran_x'] = tk.StringVar(value="0.5")
        self.osc_granx_label = ttk.Label(self.tab_osc, text="Granularidade X (mm):")
        self.osc_granx_label.grid(row=5, column=0, sticky="w", padx=8, pady=6)
        ttk.Entry(self.tab_osc, textvariable=self.params['osc_test_gran_x'], validate='key', validatecommand=vcmd_float).grid(row=5, column=1, sticky="ew", padx=8, pady=6)
        ToolTip(self.osc_granx_label, "Usado em 'Quadrada Contínua': tamanho do passo X por submovimento.")

        self.common_params_frame = ttk.LabelFrame(interior, text="Parâmetros Gerais", padding="10"); self.common_params_frame.pack(fill="x", pady=(5,0), expand=True, padx=5)
        try:
            self.common_params_frame.columnconfigure(0, weight=0); self.common_params_frame.columnconfigure(1, weight=1)
        except Exception:
            pass
        # Removido daqui: Velocidade de deposição (mm/min) — movida para Parâmetros do Processo
        self.common_labels = {}; labels_comuns = ["Comprimento (mm):", "Largura Cordão (mm):", "Sobreposição (%):", "Afastamento Tocha (mm):", "Início Extra (mm):", "Término Extra (mm):"]
        vars_comuns = ['comprimento', 'largura_cordao', 'sobreposicao', 'afastamento', 'lead_in', 'lead_out']; row_idx = 0
        for i, key_text in enumerate(labels_comuns):
            self.common_labels[key_text] = ttk.Label(self.common_params_frame, text=key_text); self.common_labels[key_text].grid(row=row_idx, column=0, sticky="w", padx=8, pady=6)
            ttk.Entry(self.common_params_frame, textvariable=self.params[vars_comuns[i]], validate='key', validatecommand=vcmd_float).grid(row=row_idx, column=1, sticky="ew", padx=8, pady=6); row_idx += 1
        
        self.direction_label = ttk.Label(self.common_params_frame, text="Sentido da Soldagem:"); self.direction_label.grid(row=row_idx, column=0, sticky="w", padx=8, pady=6)
        self.direction_combo = ttk.Combobox(self.common_params_frame, textvariable=self.params['direcao_soldagem'], values=['Esquerda -> Direita', 'Direita -> Esquerda'], state='readonly'); self.direction_combo.grid(row=row_idx, column=1, sticky="ew", padx=8, pady=6); row_idx += 1
        
        self.rotation_label = ttk.Label(self.common_params_frame, text="Sentido da Rotação:"); self.rotation_label.grid(row=row_idx, column=0, sticky="w", padx=8, pady=6)
        self.rotation_combo = ttk.Combobox(self.common_params_frame, textvariable=self.params['sentido_rotacao'], values=['Horária (CW)', 'Anti-horária (CCW)'], state='readonly'); self.rotation_combo.grid(row=row_idx, column=1, sticky="ew", padx=8, pady=6); row_idx += 1
        ToolTip(self.rotation_label, "Diferença: Sentido da Soldagem move em X; Sentido da Rotação define o sentido do eixo A (CW/CCW).")

        # Removido seletor de controle de rotação (Spindle vs Eixo A)

        self.process_params_frame = ttk.LabelFrame(interior, text="Parâmetros do Processo", padding="10"); self.process_params_frame.pack(fill="x", pady=(5,0), expand=True, padx=5)
        try:
            self.process_params_frame.columnconfigure(0, weight=0); self.process_params_frame.columnconfigure(1, weight=1)
        except Exception:
            pass
        self.process_labels = {}; labels_processo = ["Nome Procedimento:", "Corrente Arco (A):", "Vazão Gás (l/min):", "Alimentação Pó (%):", "Pré-aquecimento (°C):", "Notas:"]
        vars_processo = ['nome_procedimento', 'corrente_arco', 'vazao_gas', 'alim_po', 'preaquecimento']; row_idx = 0
        for i, key_text in enumerate(labels_processo[:-1]):
            self.process_labels[key_text] = ttk.Label(self.process_params_frame, text=key_text); self.process_labels[key_text].grid(row=row_idx, column=0, sticky="w", padx=8, pady=6)
            entry = ttk.Entry(self.process_params_frame, textvariable=self.params[vars_processo[i]])
            if key_text != "Nome Procedimento:": entry.config(validate='key', validatecommand=vcmd_float)
            else: entry.config(validate='none')
            entry.grid(row=row_idx, column=1, sticky="ew", padx=8, pady=6); row_idx += 1

        # Seleção de Tipo de Pó (combobox)
        try:
            active_powder_name = self.config.get('active_powder') or ""
        except Exception:
            active_powder_name = ""
        self.params['powder_name'] = tk.StringVar(value=active_powder_name)
        powder_names = [p.get('name') for p in self.config.get('powders', []) if p.get('name')]
        self.process_labels['Tipo de Pó:'] = ttk.Label(self.process_params_frame, text="Tipo de Pó:")
        self.process_labels['Tipo de Pó:'].grid(row=row_idx, column=0, sticky="w", padx=8, pady=6)
        self.powder_combo = ttk.Combobox(self.process_params_frame, textvariable=self.params['powder_name'], values=powder_names, state='readonly')
        self.powder_combo.grid(row=row_idx, column=1, sticky="ew", padx=8, pady=6)
        # Persistir seleção ao trocar o tipo de pó (sem exibir label)
        def _on_powder_selected(event=None):
            try:
                selected = self.params['powder_name'].get()
                if selected:
                    self.config['active_powder'] = selected
                    self.save_config()
            except Exception:
                pass
            self.trigger_update()
        self.powder_combo.bind("<<ComboboxSelected>>", _on_powder_selected)
        row_idx += 1

        # Campo OS (Ordem de Serviço)
        self.params['ordem_servico'] = tk.StringVar(value="")
        self.process_labels['OS:'] = ttk.Label(self.process_params_frame, text="OS:")
        self.process_labels['OS:'].grid(row=row_idx, column=0, sticky="w", padx=8, pady=6)
        ttk.Entry(self.process_params_frame, textvariable=self.params['ordem_servico']).grid(row=row_idx, column=1, sticky="ew", padx=8, pady=6); row_idx += 1

        # Campo para taxa de deposição em massa
        self.process_labels['TaxaDep'] = ttk.Label(self.process_params_frame, text="Taxa de deposição (g/h):")
        self.process_labels['TaxaDep'].grid(row=row_idx, column=0, sticky="w", padx=8, pady=6)
        ttk.Entry(self.process_params_frame, textvariable=self.params['taxa_deposicao_g_h'], validate='key', validatecommand=vcmd_float).grid(row=row_idx, column=1, sticky="ew", padx=8, pady=6); row_idx += 1

        # Nova posição: Velocidade de deposição (mm/min)
        self.process_labels['VelDep'] = ttk.Label(self.process_params_frame, text="Velocidade de deposição (mm/min):")
        self.process_labels['VelDep'].grid(row=row_idx, column=0, sticky="w", padx=8, pady=6)
        ttk.Entry(self.process_params_frame, textvariable=self.params['velocidade_de_deposicao'], validate='key', validatecommand=vcmd_float).grid(row=row_idx, column=1, sticky="ew", padx=8, pady=6); row_idx += 1

        # Novo campo: Velocidade da oscilação (mm/min)
        self.process_labels['VelOsc'] = ttk.Label(self.process_params_frame, text="Velocidade da oscilação (mm/min):")
        self.process_labels['VelOsc'].grid(row=row_idx, column=0, sticky="w", padx=8, pady=6)
        ttk.Entry(self.process_params_frame, textvariable=self.params['velocidade_oscilacao_mm_min'], validate='key', validatecommand=vcmd_float).grid(row=row_idx, column=1, sticky="ew", padx=8, pady=6); row_idx += 1

        # Botao: habilitar/desabilitar recuo da tocha ao ligar
        self.process_labels['TorchRetract'] = ttk.Checkbutton(
            self.process_params_frame,
            text="Recuar tocha ao ligar",
            variable=self.params['torch_retract_on_ignite'],
            command=self.trigger_update
        )
        self.process_labels['TorchRetract'].grid(row=row_idx, column=0, columnspan=2, sticky="w", padx=8, pady=6); row_idx += 1

        self.process_labels['Notes:'] = ttk.Label(self.process_params_frame, text="Notas:"); self.process_labels['Notes:'].grid(row=row_idx, column=0, sticky="nw", padx=8, pady=6)
        self.notes_text = Text(self.process_params_frame, height=3, width=20); self.notes_text.grid(row=row_idx, column=1, sticky="ew", padx=8, pady=6); self.notes_text.bind("<KeyRelease>", self.trigger_update)
        
        self.layer_params_frame = ttk.LabelFrame(interior, text="Parâmetros de Camada", padding="10"); self.layer_params_frame.pack(fill="x", pady=(5,0), expand=True, padx=5)
        try:
            self.layer_params_frame.columnconfigure(0, weight=0); self.layer_params_frame.columnconfigure(1, weight=1)
        except Exception:
            pass
        # Reordenar: colocar "Parâmetros de Camada" acima de "Parâmetros do Processo"
        try:
            self.process_params_frame.pack_forget()
            self.layer_params_frame.pack_forget()
            self.layer_params_frame.pack(fill="x", pady=(5,0), expand=True, padx=5)
            self.process_params_frame.pack(fill="x", pady=(5,0), expand=True, padx=5)
        except Exception:
            pass
        # Ativa modo responsivo: quebra linha sob o título quando estreito
        try:
            thr = self._get_compact_threshold()
            self._make_frame_responsive(self.common_params_frame, threshold_px=thr)
            self._make_frame_responsive(self.process_params_frame, threshold_px=thr)
            self._make_frame_responsive(self.layer_params_frame, threshold_px=thr)
            self._make_frame_responsive(self.tab_espiral, threshold_px=thr)
            self._make_frame_responsive(self.tab_osc, threshold_px=thr)
            # Estender responsividade para o 3D e o visualizador de G-code
            self._make_frame_responsive(self.tab_3d, threshold_px=thr)
            self._make_frame_responsive(self.gcode_viewer_frame, threshold_px=thr)
        except Exception:
            pass
        self.num_layers_label = ttk.Label(self.layer_params_frame, text="Nº de Camadas:"); self.num_layers_label.grid(row=0, column=0, sticky="w", padx=8, pady=6); ttk.Entry(self.layer_params_frame, textvariable=self.params['num_camadas'], validate='key', validatecommand=vcmd_int).grid(row=0, column=1, sticky="ew", padx=8, pady=6)
        self.thickness_per_layer_label = ttk.Label(self.layer_params_frame, text="Espessura / Camada (mm):"); self.thickness_per_layer_label.grid(row=1, column=0, sticky="w", padx=8, pady=6); ttk.Entry(self.layer_params_frame, textvariable=self.params['espessura_camada'], validate='key', validatecommand=vcmd_float).grid(row=1, column=1, sticky="ew", padx=8, pady=6)

        # Resultados e Custos (rodapé fixo da área de visualização)
        bottom_results_outer_frame = ttk.Frame(self.view_area, padding=(0, 10, 0, 0)); bottom_results_outer_frame.grid(row=1, column=0, sticky="ew")
        bottom_results_outer_frame.columnconfigure(0, weight=1); bottom_results_outer_frame.columnconfigure(1, weight=1)
        self.result_frame = ttk.LabelFrame(bottom_results_outer_frame, text="Resultados Calculados", padding="10"); self.result_frame.grid(row=0, column=0, sticky="nsew", padx=(0, 5))
        # Removido: velocidade da placa (rotation_mm_min)
        self.resultados = { 'rotation_rpm': tk.StringVar(), 'helix_pitch': tk.StringVar(), 'total_rotations': tk.StringVar(), 'total_angle_A': tk.StringVar(), 'estimated_time': tk.StringVar() }; self.result_labels = {}
        labels_resultados = ["Rotação (RPM):", "Passo Axial (mm):", "Rotações/Passos:", "Ângulo Total/Passo (A):", "Tempo Total (hh:mm):"];
        keys_resultados = ["rotation_rpm", "helix_pitch", "total_rotations", "total_angle_A", "estimated_time"]
        for i, key_text in enumerate(labels_resultados):
            self.result_labels[key_text] = ttk.Label(self.result_frame, text=key_text); self.result_labels[key_text].grid(row=i, column=0, sticky="w", pady=1)
            # --- ALTERADO: Acessa self.resultados com a chave correta de keys_resultados ---
            ttk.Label(self.result_frame, textvariable=self.resultados[keys_resultados[i]], font=('Courier', 9, 'bold'), foreground='blue').grid(row=i, column=1, sticky="w", padx=5)
            # --- FIM ALTERAÇÃO ---
        
        self.cost_frame = ttk.LabelFrame(bottom_results_outer_frame, text="Estimativa de Custo", padding="10"); self.cost_frame.grid(row=0, column=1, sticky="nsew", padx=(5, 0))
        self.custos = {'consumiveis': tk.StringVar(), 'operacional': tk.StringVar(), 'total': tk.StringVar(), 'po': tk.StringVar(), 'gas': tk.StringVar()}; self.cost_labels = {}
        labels_custos = ["Custo Consumíveis:", "Custo Operacional:", "CUSTO TOTAL:", "Consumo Pó (kg):", "Consumo Gás (m³):"]
        keys_custos = ["consumiveis", "operacional", "total", "po", "gas"]
        for i, key_text in enumerate(labels_custos):
            self.cost_labels[key_text] = ttk.Label(self.cost_frame, text=key_text); self.cost_labels[key_text].grid(row=i, column=0, sticky="w", pady=1)
            is_total = keys_custos[i] == "total"
            font = ('Courier', 9, 'bold') if is_total else ('Courier', 9); color = 'darkgreen' if is_total else 'black'
            # --- ALTERADO: Acessa self.custos com a chave correta de keys_custos ---
            ttk.Label(self.cost_frame, textvariable=self.custos[keys_custos[i]], font=font, foreground=color).grid(row=i, column=1, sticky="w", padx=5)
            # --- FIM ALTERAÇÃO ---

        # Ações do G-Code (novo botão com menu, posicionado à direita abaixo do visualizador)

        # Visualizadores (dentro do top_view_pane)
        # Agora com navegação por abas dentro da área do visualizador
        # Remove o título: usa Frame simples em vez de LabelFrame
        self.visualizer_frame = ttk.Frame(self.top_view_pane, padding=5)
        self.top_view_pane.add(self.visualizer_frame, stretch="always")

        # Notebook de abas para navegação entre visualizações
        self.visualizer_notebook = ttk.Notebook(self.visualizer_frame)
        self.tab_3d = ttk.Frame(self.visualizer_notebook)
        self.tab_temporal = ttk.Frame(self.visualizer_notebook)
        self.tab_oscilacao = ttk.Frame(self.visualizer_notebook)
        self.tab_estatisticas = ttk.Frame(self.visualizer_notebook)
        self.tab_processo = ttk.Frame(self.visualizer_notebook)

        # Títulos sem emojis para compatibilidade total de fontes
        self.visualizer_notebook.add(self.tab_3d, text="3D Interativo")
        self.visualizer_notebook.add(self.tab_temporal, text="Análise Temporal")
        self.visualizer_notebook.add(self.tab_oscilacao, text="Oscilação")
        self.visualizer_notebook.add(self.tab_estatisticas, text="Estatísticas")
        self.visualizer_notebook.add(self.tab_processo, text="Processo")
        self.visualizer_notebook.pack(fill=tk.BOTH, expand=True)
        # Seleciona explicitamente a aba 3D como padrão (índice 0)
        try:
            self.visualizer_notebook.select(0)
        except Exception:
            pass
        # Botão seta da coluna direita será criado após o frame do G-code

        # Figura e canvas 3D embutidos na aba 3D
        self.fig = Figure(figsize=(7, 5), dpi=100)
        self.ax = self.fig.add_subplot(111, projection='3d')
        self.canvas = FigureCanvasTkAgg(self.fig, master=self.tab_3d)
        self.canvas_widget = self.canvas.get_tk_widget()
        self.canvas_widget.pack(side=tk.TOP, fill=tk.BOTH, expand=True)
        self.toolbar_frame = ttk.Frame(self.tab_3d)
        self.toolbar_frame.pack(side=tk.TOP, fill=tk.X)
        self.toolbar = CustomToolbar(self.canvas, self.toolbar_frame, self)
        self.toolbar.update()
        # Captura vista inicial para espelhamento do Home
        try:
            self._home_view = (self.ax.elev, self.ax.azim)
        except Exception:
            self._home_view = (25, 45)
        # Define posição inicial padrão (não espelhada)
        try:
            self._initial_view = (self._home_view[0], self._home_view[1])
            self.ax.view_init(elev=self._initial_view[0], azim=self._initial_view[1])
        except Exception:
            pass
        # Barra de coordenadas do 3D removida
        # Desenha imediatamente usando os parâmetros atuais
        try:
            initial_params = self._get_current_params()
            self.desenhar_percurso_3d(initial_params)
        except Exception:
            pass
        # Garante um primeiro desenho para evitar tela vazia
        try:
            self.canvas.draw()
        except Exception:
            pass

        # Aba Análise Temporal: cria figure/canvas para gráficos
        self.fig_temporal = Figure(figsize=(7, 4), dpi=100)
        self.ax_temporal = self.fig_temporal.add_subplot(111)
        self.canvas_temporal = FigureCanvasTkAgg(self.fig_temporal, master=self.tab_temporal)
        self.canvas_temporal_widget = self.canvas_temporal.get_tk_widget()
        self.canvas_temporal_widget.pack(side=tk.TOP, fill=tk.BOTH, expand=True)
        try:
            self.ax_temporal.set_title("Análise Temporal (aguardando dados)")
            self.ax_temporal.set_xlabel("Passo"); self.ax_temporal.set_ylabel("Valor")
            self.canvas_temporal.draw()
        except Exception:
            pass
        # Aba Oscilação: cria figure/canvas para gráficos
        self.fig_oscilacao = Figure(figsize=(7, 4), dpi=100)
        self.ax_oscilacao = self.fig_oscilacao.add_subplot(111)
        self.canvas_oscilacao = FigureCanvasTkAgg(self.fig_oscilacao, master=self.tab_oscilacao)
        self.canvas_oscilacao_widget = self.canvas_oscilacao.get_tk_widget()
        self.canvas_oscilacao_widget.pack(side=tk.TOP, fill=tk.BOTH, expand=True)
        try:
            self.ax_oscilacao.set_title("Oscilação (aguardando dados)"); self.ax_oscilacao.set_xlabel("Passo"); self.ax_oscilacao.set_ylabel("Posição"); self.canvas_oscilacao.draw()
        except Exception:
            pass
        # Aba Estatísticas: cria figure/canvas com dois subplots (distância por passo e histograma)
        self.fig_estatisticas = Figure(figsize=(7, 4), dpi=100)
        self.ax_est_top = self.fig_estatisticas.add_subplot(211)
        self.ax_est_bottom = self.fig_estatisticas.add_subplot(212)
        self.canvas_estatisticas = FigureCanvasTkAgg(self.fig_estatisticas, master=self.tab_estatisticas)
        self.canvas_estatisticas_widget = self.canvas_estatisticas.get_tk_widget()
        self.canvas_estatisticas_widget.pack(side=tk.TOP, fill=tk.BOTH, expand=True)
        try:
            self.ax_est_top.set_title("Distância por passo (aguardando dados)")
            self.ax_est_top.set_xlabel("Passo"); self.ax_est_top.set_ylabel("Distância")
            self.ax_est_bottom.set_title("Histograma das distâncias")
            self.ax_est_bottom.set_xlabel("Distância"); self.ax_est_bottom.set_ylabel("Contagem")
            self.fig_estatisticas.tight_layout()
            self.canvas_estatisticas.draw()
        except Exception:
            pass

        # Aba Processo: custos, consumo e métricas agregadas
        self.fig_processo = Figure(figsize=(7, 4), dpi=100)
        self.ax_proc_cost = self.fig_processo.add_subplot(221)
        self.ax_proc_cons = self.fig_processo.add_subplot(222)
        self.ax_proc_len = self.fig_processo.add_subplot(223)
        self.ax_proc_time = self.fig_processo.add_subplot(224)
        self.canvas_processo = FigureCanvasTkAgg(self.fig_processo, master=self.tab_processo)
        self.canvas_processo_widget = self.canvas_processo.get_tk_widget()
        self.canvas_processo_widget.pack(side=tk.TOP, fill=tk.BOTH, expand=True)
        try:
            self.ax_proc_cost.set_title("Custos — % por categoria")
            self.ax_proc_cons.set_title("Consumo (pó e gás)")
            self.ax_proc_len.set_title("Comprimento acumulado por camada")
            self.ax_proc_time.set_title("Tempo total")
            self.fig_processo.tight_layout()
            self.canvas_processo.draw()
        except Exception:
            pass

        # Redesenha o 3D ao trocar para a aba correspondente
        def _on_visualizer_tab_changed(event=None):
            try:
                current_index = self.visualizer_notebook.index(self.visualizer_notebook.select())
                if current_index == 0:
                    self.desenhar_percurso_3d(self._get_current_params())
                elif current_index == 1:
                    self._update_temporal_plot()
                elif current_index == 2:
                    self._update_oscillation_plot()
                elif current_index == 3:
                    self._update_statistics_plot()
                elif current_index == 4:
                    self._update_process_plots()
            except Exception:
                pass
        self.visualizer_notebook.bind("<<NotebookTabChanged>>", _on_visualizer_tab_changed)
        
        # Remove o título: usa Frame simples em vez de LabelFrame
        self.gcode_viewer_frame = ttk.Frame(self.top_view_pane, padding=5); self.top_view_pane.add(self.gcode_viewer_frame, stretch="never")
        # Cabeçalho removido: sem seta para alternar painel direito
        self.gcode_line_count_var = tk.StringVar(value="Linhas: -")
        self.gcode_line_count_label = ttk.Label(self.gcode_viewer_frame, textvariable=self.gcode_line_count_var, font=("Courier New", 9, 'bold'))
        self.gcode_line_count_label.pack(side=tk.TOP, fill=tk.X, padx=2, pady=(0, 4))
        # Barra de busca e filtros
        gcode_search_frame = ttk.Frame(self.gcode_viewer_frame)
        gcode_search_frame.pack(side=tk.TOP, fill=tk.X, padx=2, pady=(0, 4))
        ttk.Label(gcode_search_frame, text="Buscar:").pack(side=tk.LEFT)
        self.gcode_search_var = tk.StringVar()
        self.gcode_search_entry = ttk.Entry(gcode_search_frame, textvariable=self.gcode_search_var, width=20)
        self.gcode_search_entry.pack(side=tk.LEFT, padx=(4, 4))
        # Botões com ícones por imagem (fallback para emoji se imagem não existir)
        try:
            self._img_search = tk.PhotoImage(file=resource_path('assets/search.png'))
            self.gcode_search_btn = ttk.Button(gcode_search_frame, image=self._img_search, command=self._search_gcode)
        except Exception:
            self.gcode_search_btn = ttk.Button(gcode_search_frame, text="🔍", command=self._search_gcode, width=2)
        self.gcode_search_btn.pack(side=tk.LEFT, padx=(0, 4))
        try:
            self._img_clear = tk.PhotoImage(file=resource_path('assets/clear.png'))
            self.gcode_clear_btn = ttk.Button(gcode_search_frame, image=self._img_clear, command=self._clear_gcode_search)
        except Exception:
            self.gcode_clear_btn = ttk.Button(gcode_search_frame, text="🧹", command=self._clear_gcode_search, width=2)
        self.gcode_clear_btn.pack(side=tk.LEFT, padx=(0, 8))
        try:
            ToolTip(self.gcode_search_btn, "Buscar")
            ToolTip(self.gcode_clear_btn, "Limpar")
        except Exception:
            pass
        # Filtros por eixos
        self.filter_x = tk.BooleanVar(value=False)
        self.filter_z = tk.BooleanVar(value=False)
        self.filter_a = tk.BooleanVar(value=False)
        ttk.Checkbutton(gcode_search_frame, text="X", variable=self.filter_x, command=self._apply_axis_filters).pack(side=tk.LEFT)
        ttk.Checkbutton(gcode_search_frame, text="Z", variable=self.filter_z, command=self._apply_axis_filters).pack(side=tk.LEFT)
        ttk.Checkbutton(gcode_search_frame, text="A", variable=self.filter_a, command=self._apply_axis_filters).pack(side=tk.LEFT)
        gcode_text_frame = ttk.Frame(self.gcode_viewer_frame); gcode_text_frame.pack(fill=tk.BOTH, expand=True); gcode_text_frame.rowconfigure(0, weight=1); gcode_text_frame.columnconfigure(0, weight=1)
        gcode_vscroll = ttk.Scrollbar(gcode_text_frame, orient=tk.VERTICAL); gcode_hscroll = ttk.Scrollbar(gcode_text_frame, orient=tk.HORIZONTAL)
        self.gcode_text = Text(gcode_text_frame, wrap=tk.NONE, yscrollcommand=gcode_vscroll.set, xscrollcommand=gcode_hscroll.set, font=("Courier New", 9), state='disabled', width=40)
        gcode_vscroll.config(command=self.gcode_text.yview); gcode_hscroll.config(command=self.gcode_text.xview)
        gcode_vscroll.grid(row=0, column=1, sticky='ns'); gcode_hscroll.grid(row=1, column=0, sticky='ew', columnspan=2); self.gcode_text.grid(row=0, column=0, sticky='nsew')
        # Configura tags de coloração semântica
        self._init_gcode_viewer_tags()

        # Rodapé de ações do G-Code no visualizador (à direita)
        gcode_actions_frame = ttk.Frame(self.gcode_viewer_frame)
        gcode_actions_frame.pack(side=tk.BOTTOM, fill=tk.X, pady=(6, 0))
        # Ações de cópia
        ttk.Button(gcode_actions_frame, text="Copiar tudo", command=self._copy_gcode_all).pack(side=tk.LEFT, padx=(0, 6))
        # Botão para abrir diretamente no Mach3
        self.open_mach3_btn = ttk.Button(gcode_actions_frame, text="Abrir no Mach3", command=self._open_in_mach3_clicked)
        self.open_mach3_btn.pack(side=tk.RIGHT, padx=(0, 6))
        # Botão de download simplificado: apenas ícone, salva com nome da OS
        self.download_btn = ttk.Button(gcode_actions_frame, text="⤓", style="Icon.TButton", command=self._download_gcode_clicked)
        self.download_btn.config(width=2)
        self.download_btn.pack(side=tk.RIGHT, padx=(0, 6))
        ToolTip(self.download_btn, "Baixar G-code")
        ToolTip(self.open_mach3_btn, "Salvar e abrir no Mach3")
        
        # Barra de Notificação
        self.notification_frame = ttk.Frame(main_frame, style='Notification.TFrame'); self.notification_label = ttk.Label(self.notification_frame, text="", anchor='w', padding=(10, 5), style='Notification.TLabel')
        self.notification_label.pack(side=tk.LEFT, fill=tk.X, expand=True); close_button = ttk.Button(self.notification_frame, text="X", command=self.hide_notification, style='Close.TButton', width=2)
        close_button.pack(side=tk.RIGHT, padx=(0, 5)); self.notification_frame.grid(row=1, column=0, sticky='ew', pady=(5, 0)); self.notification_frame.grid_remove()

        # Adiciona traces
        for var_name, var in self.params.items():
            if isinstance(var, (tk.StringVar, tk.BooleanVar)):
                if var_name not in []: var.trace_add("write", self.trigger_update)

    def _save_sash_positions(self):
        try:
            self._sash_top = self.top_view_pane.sash_coord(0)
            # Persiste também no config.json
            try:
                tx, ty = self._sash_top
                self.config.setdefault('ui', {}).setdefault('sashes', {})
                self.config['ui']['sashes']['top_view'] = {"x": tx, "y": ty}
            except Exception:
                pass
            try:
                mx, my = self.main_paned_window.sash_coord(0)
                self.config.setdefault('ui', {}).setdefault('sashes', {})
                self.config['ui']['sashes']['main'] = {"x": mx, "y": my}
            except tk.TclError:
                pass
            except Exception:
                pass
            try:
                self.save_config()
            except Exception:
                pass
        except tk.TclError:
            pass

    def _restore_sash_positions(self):
        try:
            # Primeiro tenta restaurar de config.json
            ui_conf = self.config.get('ui', {}).get('sashes', {}) if isinstance(self.config.get('ui', {}), dict) else {}
            try:
                tv = ui_conf.get('top_view')
                if tv and 'x' in tv and 'y' in tv:
                    self.top_view_pane.sash_place(0, int(tv['x']), int(tv['y']))
            except tk.TclError:
                pass
            try:
                mn = ui_conf.get('main')
                if mn and 'x' in mn:
                    self.main_paned_window.sash_place(0, int(mn['x']), 0)
            except tk.TclError:
                pass
            # Fallback: usa valores em memória coletados nesta sessão
            if not ui_conf and getattr(self, '_sash_top', None):
                try:
                    x, y = self._sash_top
                    self.top_view_pane.sash_place(0, x, y)
                except tk.TclError:
                    pass
        except Exception:
            pass
    
    # Removidas funções de alternância de painéis esquerdo e direito

    # --- Layout responsivo: quebra linha abaixo do título quando necessário ---
    def _make_frame_responsive(self, frame, threshold_px: int = 520):
        if not hasattr(self, '_grid_original_by_frame'):
            self._grid_original_by_frame = {}

        def _capture_original():
            if frame in self._grid_original_by_frame:
                return
            info = {}
            for child in frame.grid_slaves():
                gi = child.grid_info()
                try:
                    row = int(gi.get('row', 0)); col = int(gi.get('column', 0))
                    colspan = int(gi.get('columnspan', 1))
                except Exception:
                    row = int(gi['row']); col = int(gi['column']); colspan = int(gi.get('columnspan', 1) or 1)
                info[child] = {'row': row, 'column': col, 'columnspan': colspan}
            self._grid_original_by_frame[frame] = info

        def _apply_compact(width_now: int):
            # Usa posição original como base
            originals = self._grid_original_by_frame.get(frame, {})
            if not originals:
                return
            # Descobre linhas existentes
            rows = sorted({v['row'] for v in originals.values()})
            added = 0
            # Para cada linha, se houver widget na coluna 1, cria quebra
            for r in rows:
                # Widgets desta linha
                row_widgets = [w for w, v in originals.items() if v['row'] == r]
                # mede necessidade real de largura
                total_req = 0
                for w in row_widgets:
                    try:
                        total_req += int(w.winfo_reqwidth())
                    except Exception:
                        pass
                has_col1 = any(originals[w]['column'] == 1 for w in row_widgets)
                need_break = has_col1 and (total_req + 24 > width_now)
                # Reposiciona widgets desta e das próximas linhas
                for w, v in originals.items():
                    base_row = v['row']; base_col = v['column']; base_span = v['columnspan']
                    new_row = base_row + added
                    new_col = base_col
                    new_span = base_span
                    if base_row == r and need_break:
                        if base_col == 1:
                            # mover entrada para a linha de baixo, ocupar 2 colunas
                            new_row = base_row + added + 1
                            new_col = 0
                            new_span = 2
                        elif base_col == 0:
                            # rótulo: reduzir largura com wrap para evitar corte
                            try:
                                if hasattr(w, 'configure'):
                                    w.configure(wraplength=max(width_now - 24, 100))
                            except Exception:
                                pass
                    try:
                        w.grid_configure(row=new_row, column=new_col, columnspan=new_span, sticky='ew' if (base_col == 1 or new_span == 2) else 'w')
                    except Exception:
                        pass
                if need_break:
                    added += 1

        def _restore_wide():
            originals = self._grid_original_by_frame.get(frame, {})
            if not originals:
                return
            for w, v in originals.items():
                try:
                    # remove wraplength se existir
                    try:
                        w.configure(wraplength=0)
                    except Exception:
                        pass
                    w.grid_configure(row=v['row'], column=v['column'], columnspan=v['columnspan'], sticky='ew' if v['column'] == 1 else 'w')
                except Exception:
                    pass

        def _on_config(event=None):
            try:
                # captura primeira vez
                _capture_original()
                width = frame.winfo_width()
                # Largura disponível considerando ancestrais relevantes
                try:
                    top_w = frame.winfo_toplevel().winfo_width()
                except Exception:
                    top_w = width
                try:
                    parent_w = frame.master.winfo_width() if hasattr(frame, 'master') else width
                except Exception:
                    parent_w = width
                avail_w = min(width, top_w, parent_w)
                if width <= 1:
                    return
                # reforça expansão da coluna de entrada
                try:
                    frame.columnconfigure(0, weight=0, minsize=0)
                    frame.columnconfigure(1, weight=1, minsize=80)
                except Exception:
                    pass
                # ativa compacto se qualquer largura disponível for menor que o limite
                if avail_w < threshold_px:
                    try:
                        frame.columnconfigure(0, minsize=80)
                        frame.columnconfigure(1, minsize=120)
                    except Exception:
                        pass
                    _apply_compact(width_now=avail_w)
                else:
                    try:
                        frame.columnconfigure(0, minsize=0)
                        frame.columnconfigure(1, minsize=0)
                    except Exception:
                        pass
                    _restore_wide()
            except Exception:
                pass

        try:
            frame.bind('<Configure>', _on_config, add='+')
            # Primeira avaliação
            frame.after(100, _on_config)
        except Exception:
            pass

        # Aplicar também em frames aninhados diretos, para cobrir grids internos
        try:
            for child in frame.winfo_children():
                # evita duplicar binding
                if isinstance(child, (tk.Frame, ttk.Frame)) and not getattr(child, '_responsive_bound', False):
                    setattr(child, '_responsive_bound', True)
                    self._make_frame_responsive(child, threshold_px=threshold_px)
        except Exception:
            pass

    def _get_compact_threshold(self) -> int:
        try:
            ui = self.config.get('ui', {}) if isinstance(self.config.get('ui', {}), dict) else {}
            val = int(ui.get('compact_threshold_px', 560))
            # Mantém limites razoáveis
            return max(360, min(1400, val))
        except Exception:
            return 560

    # Removido: largura mínima para alça de colapso

    def _setup_styles(self):
        s = ttk.Style()
        # Paleta corporativa
        ORANGE = '#FF6A00'
        ORANGE_DARK = '#CC5600'
        BLACK = '#000000'
        WHITE = '#FFFFFF'

        # Usar tema que permite customizar cores de fundo
        try:
            s.theme_use('clam')
        except tk.TclError:
            pass

        # Plano de fundo e elementos básicos
        self.root.configure(bg=WHITE)
        s.configure('TFrame', background=WHITE)
        s.configure('TLabelframe', background=WHITE)
        s.configure('TLabelframe.Label', background=WHITE, foreground=BLACK)
        s.configure('TLabel', background=WHITE, foreground=BLACK)

        # Notebook/Tabs
        s.configure('TNotebook', background=WHITE)
        s.configure('TNotebook.Tab', padding=(8, 4))
        s.map('TNotebook.Tab',
              background=[('selected', WHITE), ('!selected', '#F2F2F2')],
              foreground=[('selected', BLACK), ('!selected', BLACK)])

        # Botões
        s.configure('TButton', font=('Segoe UI', 10, 'bold'), padding=(10, 6), background=ORANGE, foreground=WHITE)
        s.map('TButton', background=[('active', ORANGE_DARK), ('disabled', '#BFBFBF')], foreground=[('disabled', '#E0E0E0')])

        s.configure('Accent.TButton', font=('Segoe UI', 10, 'bold'), padding=(10, 6), background=BLACK, foreground=WHITE)
        s.map('Accent.TButton', background=[('active', '#222222')])

        s.configure('DryRun.TButton', font=('Segoe UI', 10, 'bold'), padding=(10, 6), background='#FFC26A', foreground=BLACK)
        s.map('DryRun.TButton', background=[('active', '#FFB14D')])

        s.configure('Close.TButton', padding=(2,0), font=('Segoe UI', 8), background=BLACK, foreground=WHITE)
        s.map('Close.TButton', background=[('active', '#222222')])

        # Combobox
        s.configure('TCombobox', fieldbackground=WHITE, background=WHITE, foreground=BLACK)
        s.map('TCombobox', fieldbackground=[('readonly', WHITE)], foreground=[('readonly', BLACK)])

        # Menubutton de ícone (download)
        s.configure('Icon.TMenubutton', font=('Segoe UI Symbol', 13, 'bold'), padding=(6, 3), background=ORANGE, foreground=WHITE)
        s.map('Icon.TMenubutton', background=[('active', ORANGE_DARK)])
        # Versão para Button com ícone
        # Botão de download: altura alinhada ao TButton
        s.configure('Icon.TButton', font=('Segoe UI', 10, 'bold'), padding=(10, 6), background=ORANGE, foreground=WHITE)
        s.map('Icon.TButton', background=[('active', ORANGE_DARK)])

        # Barra de notificação
        s.configure('Notification.TFrame', background=BLACK)
        s.configure('Notification.TLabel', background=BLACK, foreground=WHITE, font=('Segoe UI', 9))
        s.configure('Info.TLabel', background='#d1ecf1', foreground='#0c5460', font=('Segoe UI', 9))
        s.configure('Success.TLabel', background='#d4edda', foreground='#155724', font=('Segoe UI', 9))
        s.configure('Warning.TLabel', background='#fff3cd', foreground='#856404', font=('Segoe UI', 9))
        s.configure('Error.TLabel', background='#f8d7da', foreground='#721c24', font=('Segoe UI', 9, 'bold'))

    def _update_ui_text(self):
        # Esta função agora é chamada apenas uma vez para definir os textos
        self.root.title("TFM G-Code Generator")
        self.menubar.add_cascade(label="Arquivo", menu=self.file_menu)
        self.file_menu.add_command(label="Carregar Procedimento...", command=self._load_procedure); self.file_menu.add_command(label="Salvar Procedimento...", command=self._save_procedure)
        self.file_menu.add_separator(); self.file_menu.add_command(label="Gerar Relatório PDF...", command=self._generate_report, state="normal" if REPORTLAB_AVAILABLE else "disabled")
        self.file_menu.add_separator(); self.file_menu.add_command(label="Sair", command=self.root.quit)
        # Configurações já adicionadas em _create_menu; evitar duplicidade
        # Menu 'Simuladores' removido
        self.menubar.add_cascade(label="Ajuda", menu=self.help_menu); self.help_menu.add_command(label="Sobre", command=self._show_about_dialog)
        try:
            self.help_menu.add_separator()
            self.help_menu.add_command(label="Verificar atualização...", command=lambda: self._check_for_updates(silent=False))
        except Exception:
            pass
        # Nomes das abas, labels, etc., já definidos em _create_widgets
        # Atualiza o prefixo do contador de linhas (caso o idioma fosse trocado, mas agora é fixo)
        if hasattr(self, 'gcode_line_count_var'):
             prefix = "Linhas"
             display_count = '-' if getattr(self, '_last_gcode_line_count', 0) == 0 else str(self._last_gcode_line_count)
             self.gcode_line_count_var.set(f"{prefix}: {display_count}")
        pass 

    # --- REMOVIDO: _change_language ---

    def _get_current_params(self):
        data = {}; data['nome_procedimento'] = self.params['nome_procedimento'].get()
        data['app_title'] = "TFM G-Code Generator" # Texto fixo
        try:
            data['diametro'] = float(self.params['diametro'].get()) if self.params['diametro'].get() else 0.0
            data['comprimento_revestir'] = float(self.params['comprimento'].get()) if self.params['comprimento'].get() else 0.0
            data['largura_cordao'] = float(self.params['largura_cordao'].get()) if self.params['largura_cordao'].get() else 0.0
            data['sobreposicao'] = float(self.params['sobreposicao'].get()) if self.params['sobreposicao'].get() else 0.0
            # Novo nome: velocidade_de_deposicao
            try:
                vel_str = self.params['velocidade_de_deposicao'].get()
            except Exception:
                vel_str = ''
            vel = float(vel_str) if vel_str else 0.0
            data['velocidade_de_deposicao'] = vel
            # Compatibilidade retroativa: também preencher taxa_de_deposicao
            data['taxa_de_deposicao'] = vel
            # Compatibilidade: manter velocidade do eixo A igual à velocidade de deposição por padrão.
            data['velocidade_a_mm_min'] = vel
            # Removido: RPM alvo (A). RPM é derivado exclusivamente da taxa de deposição e diâmetro.
            # Velocidade da oscilação (mm/min) — campo da UI
            try:
                vel_osc_str = self.params['velocidade_oscilacao_mm_min'].get()
            except Exception:
                vel_osc_str = ''
            data['velocidade_oscilacao_mm_min'] = float(vel_osc_str) if vel_osc_str else 0.0
            data['afastamento_tocha'] = float(self.params['afastamento'].get()) if self.params['afastamento'].get() else 0.0
            data['lead_in'] = float(self.params['lead_in'].get()) if self.params['lead_in'].get() else 0.0
            data['lead_out'] = float(self.params['lead_out'].get()) if self.params['lead_out'].get() else 0.0
            direction_map_reverse = {'Esquerda -> Direita': 'esquerda_direita', 'Direita -> Esquerda': 'direita_esquerda'}
            data['direcao_soldagem'] = direction_map_reverse.get(self.params['direcao_soldagem'].get(), 'esquerda_direita')
            rotation_map_reverse = {'Horária (CW)': 'horaria', 'Anti-horária (CCW)': 'antihoraria'}
            data['sentido_rotacao'] = rotation_map_reverse.get(self.params['sentido_rotacao'].get(), 'horaria')
            # Removido: controle de rotação; sempre eixo A
            data['corrente_arco'] = float(self.params['corrente_arco'].get()) if self.params['corrente_arco'].get() else 0.0; data['vazao_gas'] = float(self.params['vazao_gas'].get()) if self.params['vazao_gas'].get() else 0.0
            data['alim_po'] = float(self.params['alim_po'].get()) if self.params['alim_po'].get() else 0.0; data['preaquecimento'] = float(self.params['preaquecimento'].get()) if self.params['preaquecimento'].get() else 0.0
            data['num_camadas'] = int(self.params['num_camadas'].get()) if self.params['num_camadas'].get() else 1; data['espessura_camada'] = float(self.params['espessura_camada'].get()) if self.params['espessura_camada'].get() else 0.0
            # Seleção de pó e OS (se presentes na UI)
            selected_powder_name = None
            try:
                if 'powder_name' in self.params:
                    selected_powder_name = self.params['powder_name'].get() or None
            except Exception:
                selected_powder_name = None
            try:
                data['ordem_servico'] = self.params['ordem_servico'].get() if 'ordem_servico' in self.params else ""
            except Exception:
                data['ordem_servico'] = ""

            # Fator de pó e custo: tentam usar o pó selecionado; caso contrário, o ativo nas Configurações
            powders = self.config.get('powders', [])
            active_name = self.config.get('active_powder')
            target_powder_name = selected_powder_name or active_name
            data['powder_name'] = target_powder_name
            powder_factor = None; powder_cost = None
            for p in powders:
                if p.get('name') == target_powder_name:
                    try:
                        powder_factor = float(p.get('density_factor_g_mm2', 0.0))
                    except (TypeError, ValueError):
                        powder_factor = 0.0
                    try:
                        powder_cost = float(p.get('cost_brl_kg', self.config.get('costs', {}).get('powder_brl_kg', 0.0)))
                    except (TypeError, ValueError):
                        powder_cost = self.config.get('costs', {}).get('powder_brl_kg', 0.0)
                    break
            if powder_factor is None:
                powder_factor = 0.16
            if powder_cost is None:
                powder_cost = self.config.get('costs', {}).get('powder_brl_kg', 0.0)
            data['powder_factor'] = powder_factor
            data['powder_cost_brl_kg'] = powder_cost
            # Sinalizador para modo compacto de G-code
            try:
                data['compact_gcode'] = bool(self.params['compact_gcode'].get())
            except Exception:
                data['compact_gcode'] = False
            # Recuo da tocha ao ligar
            try:
                data['torch_retract_on_ignite'] = bool(self.params['torch_retract_on_ignite'].get())
            except Exception:
                data['torch_retract_on_ignite'] = True

            active_tab_index = self.notebook.index(self.notebook.select())
            if active_tab_index == 0: # Espiral
                data['welding_mode'] = 'espiral'; data['tipo_peca'] = 'cilindrico'
                data['diametro_inicial'] = data['diametro']; data['diametro_final'] = data['diametro']
            elif active_tab_index == 1: # Oscilação (unificada)
                data['welding_mode'] = 'oscilacao'; data['tipo_peca'] = 'cilindrico'
                data['diametro_inicial'] = data['diametro']; data['diametro_final'] = data['diametro']
                data['oscilacao_comprimento'] = float(self.params['oscilacao_comprimento'].get()) if self.params['oscilacao_comprimento'].get() else 0.0
                data['deslocamento_angular_perc'] = float(self.params['deslocamento_angular_perc'].get()) if self.params['deslocamento_angular_perc'].get() else 0.0
                # Mapear tipo para valor interno
                osc_map_reverse = {'Linear': 'linear', 'Quadrada': 'quadrada', 'Quadrada Contínua': 'quadrada_continua'}
                data['oscillation_type'] = osc_map_reverse.get(self.params['tipo_oscilacao'].get(), 'linear')
                data['oscilacao_enabled'] = True
                # Granularidade X para Quadrada Contínua
                try:
                    data['osc_test_gran_x'] = float(self.params.get('osc_test_gran_x', tk.StringVar(value='0.5')).get())
                except Exception:
                    data['osc_test_gran_x'] = 0.5
            else: return None
        except ValueError: return None
        # Taxa de deposição por massa (g/h) para uso em fórmulas
        try:
            taxa_g_h_str = self.params.get('taxa_deposicao_g_h', tk.StringVar(value='700.0')).get()
        except Exception:
            taxa_g_h_str = '0.0'
        try:
            data['taxa_deposicao_g_h'] = float(taxa_g_h_str) if taxa_g_h_str else 0.0
        except ValueError:
            data['taxa_deposicao_g_h'] = 0.0
        data['notes'] = self.notes_text.get("1.0", tk.END).strip()
        if data['largura_cordao'] <= 0 or data['velocidade_de_deposicao'] <= 0: return None
        if data.get('sobreposicao') is not None and (data['sobreposicao'] >= 100 or data['sobreposicao'] < 0): return None
        if data.get('num_camadas', 1) < 1: return None
        if data['welding_mode'] in ('oscilacao','oscilacao_linear','oscilacao_quadrada'):
             if data['oscilacao_comprimento'] <= 0: return None
             if data['deslocamento_angular_perc'] <= 0 or data['deslocamento_angular_perc'] > 100: return None
        return data

    def executar_calculos_e_desenho(self):
        params = self._get_current_params()
        if params is None:
            self.limpar_resultados(); self.desenhar_percurso_3d()
            if hasattr(self, 'gcode_text'):
                self.gcode_text.config(state='normal')
                self.gcode_text.delete('1.0', tk.END)
                self.show_notification("Por favor, verifique os parâmetros.", 'warning')
                self.gcode_text.insert('1.0', "Por favor, verifique os parâmetros.")
                self.gcode_text.config(state='disabled')
            return
        try:
            diameter = params['diametro']; diameter = max(diameter, 1e-6)
            h = params['comprimento_revestir']
            C = params['num_camadas'] * params['espessura_camada']; peso = params['powder_factor']
            lead_in = params.get('lead_in', 0.0); lead_out = params.get('lead_out', 0.0)
            circunferencia = math.pi * diameter
            larg_cordao = params['largura_cordao']
            sobreposicao = params['sobreposicao']
            taxa_de_deposicao = params['taxa_de_deposicao']
            tempo_total_horas = 0.0; consumo_po_kg = 0.0

            if params['welding_mode'] == 'espiral':
                # Comprimento real da hélice por volta e total (não apenas axial)
                passo = larg_cordao * (1.0 - (sobreposicao / 100.0)); passo = max(passo, 1e-6)
                total_rotacoes_part = (h / passo) if h > 0 else 0
                # Ângulo total inclui 1 volta no início e 1 no final
                angulo_part = total_rotacoes_part * 360.0 + 720.0
                comprimento_helice_por_volta = math.sqrt((circunferencia ** 2) + (passo ** 2))
                comprimento_total_helice = total_rotacoes_part * comprimento_helice_por_volta
                # Tempo: comprimento da hélice + leads + 1 volta inicial + 1 volta final
                comprimento_total_mov = comprimento_total_helice + (lead_in + lead_out) + (2 * circunferencia)
                # RPM aproximado considerando componente circumferencial; mantido simples
                rpm = (taxa_de_deposicao / circunferencia) if circunferencia > 0 and taxa_de_deposicao > 0 else 0
                # Tempo baseado no comprimento real percorrido
                tempo_min_arc = (comprimento_total_mov / taxa_de_deposicao) if taxa_de_deposicao > 0 else 0
                area_camada = circunferencia * h; area_total = area_camada * params['num_camadas']
                area_leads = circunferencia * (lead_in + lead_out) * params['num_camadas'] if lead_in + lead_out > 0 else 0
                consumo_po_g = (area_total + area_leads) * peso; consumo_po_kg = consumo_po_g / 1000.0
                # Atualização de resultados sem velocidade da placa
                self.resultados['rotation_rpm'].set(f"{rpm:.3f}"); self.resultados['helix_pitch'].set(f"{passo:.3f} (axial)"); self.resultados['total_rotations'].set(f"{total_rotacoes_part:.2f}")
                self.resultados['total_angle_A'].set(f"{angulo_part:.2f}")
                total_minutos = tempo_min_arc * params['num_camadas']
                # Arredonda para cima ao minuto para evitar subestimar
                minutos_arred = int(math.ceil(total_minutos))
                hh = minutos_arred // 60; mm = minutos_arred % 60
                self.resultados['estimated_time'].set(f"{hh:02d}:{mm:02d}")
                tempo_total_horas = total_minutos / 60.0
            elif params['welding_mode'] in ('oscilacao', 'oscilacao_linear', 'oscilacao_quadrada'):
                comp_osc = params['oscilacao_comprimento']; desloc_perc = params['deslocamento_angular_perc']
                desloc_linear_angular = (desloc_perc / 100.0) * larg_cordao
                num_passos_angulares_por_volta = math.ceil(circunferencia / desloc_linear_angular) if circunferencia > 0 and desloc_linear_angular > 0 else 1
                actual_delta_A_deg = 360.0 / num_passos_angulares_por_volta if num_passos_angulares_por_volta > 0 else 360.0
                # RPM do eixo A: derivado da taxa de deposição e circunferência
                rpm_a = (taxa_de_deposicao / circunferencia) if circunferencia > 0 and taxa_de_deposicao > 0 else 0
                feed_angular = rpm_a * 360.0

                tempo_osc_ida_volta = (comp_osc * 2) / taxa_de_deposicao if taxa_de_deposicao > 0 else 0
                tempo_rotacao_passo = actual_delta_A_deg / feed_angular if feed_angular > 0 else 0
                # Se for quadrada contínua, rotação ocorre junto ao X; considerar tempo de rotação como 0 adicional
                if params.get('oscillation_type') == 'quadrada_continua':
                    tempo_rotacao_passo = 0
                tempo_por_passo_angular = tempo_osc_ida_volta + tempo_rotacao_passo
                tempo_por_volta = tempo_por_passo_angular * num_passos_angulares_por_volta
                passo_sobreposicao = comp_osc * (sobreposicao / 100.0)
                passo_sobreposicao = max(passo_sobreposicao, 1e-6)
                num_passos_axiais = math.ceil(h / passo_sobreposicao) if h > 0 else 1
                tempo_min_total_osc = num_passos_axiais * tempo_por_volta

                tempo_leads = (lead_in + lead_out) / taxa_de_deposicao if taxa_de_deposicao > 0 else 0
                tempo_min_total = tempo_min_total_osc + tempo_leads

                # Atualização de resultados sem velocidade da placa
                self.resultados['rotation_rpm'].set(f"{rpm_a:.3f}"); self.resultados['helix_pitch'].set(f"{passo_sobreposicao:.3f} (axial)")
                self.resultados['total_rotations'].set(f"{num_passos_axiais:.2f} (passos axiais)"); self.resultados['total_angle_A'].set(f"{actual_delta_A_deg:.2f}° (passo)")
                total_minutos = tempo_min_total * params['num_camadas']
                # Arredonda para cima ao minuto para evitar subestimar
                minutos_arred = int(math.ceil(total_minutos))
                hh = minutos_arred // 60; mm = minutos_arred % 60
                self.resultados['estimated_time'].set(f"{hh:02d}:{mm:02d}")
                tempo_total_horas = total_minutos / 60.0

                area_camada = circunferencia * h; area_total = area_camada * params['num_camadas']
                area_leads = circunferencia * (lead_in + lead_out) * params['num_camadas'] if lead_in + lead_out > 0 else 0
                consumo_po_g = (area_total + area_leads) * peso; consumo_po_kg = consumo_po_g / 1000.0
            else:
                self.limpar_resultados(); self.desenhar_percurso_3d(); return

            # Avaliar fórmulas de runtime e permitir override de consumo/custos
            formula_res = {}
            try:
                formula_res = self._evaluate_formulas_runtime(params)
            except Exception:
                formula_res = {}

            # Override consumo de pó, se definido em fórmulas
            pm_kg = formula_res.get('powder_mass_kg')
            pm_g = formula_res.get('powder_mass_g')
            if isinstance(pm_kg, (int, float)) and pm_kg >= 0:
                consumo_po_kg = float(pm_kg)
            elif isinstance(pm_g, (int, float)) and pm_g >= 0:
                consumo_po_kg = float(pm_g) / 1000.0

            # Override de tempo por massa (prioriza taxa de deposição da UI)
            time_min_override = None
            # 1) Se a UI informar taxa em g/h (>0), usa para calcular tempo
            taxa_g_h_ui = params.get('taxa_deposicao_g_h', 0.0)
            taxa_kg_h_calc = None
            if isinstance(taxa_g_h_ui, (int, float)) and taxa_g_h_ui > 0:
                taxa_kg_h_calc = float(taxa_g_h_ui) / 1000.0
            else:
                # 2) Caso contrário, tenta pelas fórmulas: kg/h ou g/h
                taxa_kg_h_form = formula_res.get('taxa_deposicao_kg_h')
                taxa_g_h_form = formula_res.get('taxa_deposicao_g_h')
                if isinstance(taxa_kg_h_form, (int, float)) and taxa_kg_h_form > 0:
                    taxa_kg_h_calc = float(taxa_kg_h_form)
                elif isinstance(taxa_g_h_form, (int, float)) and taxa_g_h_form > 0:
                    taxa_kg_h_calc = float(taxa_g_h_form) / 1000.0
            if isinstance(taxa_kg_h_calc, (int, float)) and taxa_kg_h_calc > 0:
                time_min_override = (consumo_po_kg / max(taxa_kg_h_calc, 1e-6)) * 60.0
            elif isinstance(formula_res.get('time_min_mass'), (int, float)) and formula_res['time_min_mass'] >= 0:
                # 3) Se houver tempo por massa explícito nas fórmulas, usa
                time_min_override = float(formula_res['time_min_mass'])
            elif isinstance(formula_res.get('time_min'), (int, float)) and formula_res['time_min'] >= 0:
                # 4) Por último, tempo direto das fórmulas
                time_min_override = float(formula_res['time_min'])

            if time_min_override is not None:
                tempo_total_horas = time_min_override / 60.0
                minutos_arred = int(math.ceil(time_min_override))
                hh = minutos_arred // 60; mm = minutos_arred % 60
                self.resultados['estimated_time'].set(f"{hh:02d}:{mm:02d}")

            custo_po = consumo_po_kg * params.get('powder_cost_brl_kg', self.config['costs']['powder_brl_kg'])
            # Custos de gás/operacionais (com possibilidade de override por fórmula)
            consumo_gas_m3_hora = params['vazao_gas'] * 60.0 / 1000.0
            custo_gas = consumo_gas_m3_hora * tempo_total_horas * self.config['costs']['gas_argon_brl_m3']
            custo_operacional = tempo_total_horas * (self.config['costs']['labor_brl_hour'] + self.config['costs']['machine_brl_hour'])
            custo_consumiveis = custo_po + custo_gas

            # Overrides diretos se presentes
            if isinstance(formula_res.get('powder_cost_brl'), (int, float)):
                custo_po = float(formula_res['powder_cost_brl'])
                # recalcula consumíveis para refletir custo_po
                custo_consumiveis = custo_po + custo_gas
            if isinstance(formula_res.get('gas_cost_brl'), (int, float)):
                custo_gas = float(formula_res['gas_cost_brl'])
                custo_consumiveis = custo_po + custo_gas
            if isinstance(formula_res.get('labor_cost_brl'), (int, float)):
                # se vier pronto por fórmula, usa
                labor_cost = float(formula_res['labor_cost_brl'])
            else:
                labor_cost = tempo_total_horas * self.config['costs']['labor_brl_hour']
            if isinstance(formula_res.get('machine_cost_brl'), (int, float)):
                machine_cost = float(formula_res['machine_cost_brl'])
            else:
                machine_cost = tempo_total_horas * self.config['costs']['machine_brl_hour']
            custo_operacional = labor_cost + machine_cost

            custo_total = custo_consumiveis + custo_operacional
            if isinstance(formula_res.get('total_cost_brl'), (int, float)):
                custo_total = float(formula_res['total_cost_brl'])
            consumo_gas_m3 = consumo_gas_m3_hora * tempo_total_horas
            symbol = self.config['costs'].get('currency_symbol', '$')
            self.custos['consumiveis'].set(f"{symbol} {custo_consumiveis:.2f}"); self.custos['operacional'].set(f"{symbol} {custo_operacional:.2f}")
            self.custos['total'].set(f"{symbol} {custo_total:.2f}"); self.custos['po'].set(f"{consumo_po_kg:.3f} kg"); self.custos['gas'].set(f"{consumo_gas_m3:.3f} m³")
            self.desenhar_percurso_3d(params)
        except Exception as e:
            error_msg = f"Erro: {e}"; print(f"Erro inesperado em executar_calculos_e_desenho: {e}")
            self.limpar_resultados(); self.desenhar_percurso_3d()
            self.show_notification(error_msg, 'error', duration_ms=0)
            if hasattr(self, 'gcode_text'):
                self.gcode_text.config(state='normal'); self.gcode_text.delete('1.0', tk.END); self.gcode_text.insert('1.0', error_msg); self.gcode_text.config(state='disabled')
        self.initial_load_done = True

    def limpar_resultados(self):
        for key in self.resultados: self.resultados[key].set("0.00");
        for key in self.custos: self.custos[key].set("...")

    def desenhar_percurso_3d(self, params=None):
        self.ax.clear()
        if params is None:
            self.canvas.draw(); return
        # Paleta amigável a daltonismo (Set2/Paired)
        layer_colors = ['#66c2a5', '#fc8d62', '#8da0cb', '#e78ac3', '#a6d854', '#ffd92f', '#e5c494', '#b3b3b3']
        try:
            d_base = params['diametro']; length_base = params['comprimento_revestir']; direction = params.get('direcao_soldagem', 'esquerda_direita')
            mode = params.get('welding_mode', 'espiral')
            x_base = np.linspace(0, length_base, 30); theta_base = np.linspace(0, 2 * np.pi, 30); xc_base, tc_base = np.meshgrid(x_base, theta_base)
            r_base_vals = d_base / 2.0 ; yc_base = r_base_vals * np.cos(tc_base); zc_base = r_base_vals * np.sin(tc_base); self.ax.plot_surface(xc_base, yc_base, zc_base, alpha=0.1, color='gray')
            # Grid leve para percepção espacial
            try:
                self.ax.grid(True, alpha=0.2, color="#888")
            except Exception:
                pass
            for i in range(params['num_camadas']):
                d_layer = params['diametro'] + i * 2 * params['espessura_camada']; length = params['comprimento_revestir']
                lead_in = params.get('lead_in', 0.0); lead_out = params.get('lead_out', 0.0)
                r_layer = d_layer / 2.0; passo = params['largura_cordao'] * (1.0 - (params['sobreposicao'] / 100.0)); passo = max(passo, 1e-6)
                if direction == 'esquerda_direita': x_arc_start_calc = 0.0 - lead_in; x_arc_end_calc = length + lead_out
                else: x_arc_start_calc = length + lead_out; x_arc_end_calc = 0.0 - lead_in
                comprimento_total_arc = abs(x_arc_end_calc - x_arc_start_calc)
                angle_sign = 1.0 if direction == 'esquerda_direita' else -1.0
                if mode == 'espiral':
                    total_rotacoes = (comprimento_total_arc / passo)
                    angulo_total_rad = math.radians(total_rotacoes * 360.0 * angle_sign)
                    if abs(angulo_total_rad) < 1e-6: continue
                    num_points = int(50 * abs(total_rotacoes)) + 2;
                    if num_points < 2: num_points = 2
                    theta_helice = np.linspace(0, angulo_total_rad, num_points); x_final_helice = np.linspace(x_arc_start_calc, x_arc_end_calc, len(theta_helice))
                    r_base_helice = np.full_like(x_final_helice, r_layer)
                    r_helice = r_base_helice; safe_r_helice = np.where(np.abs(r_helice) < 1e-9, 1e-9, r_helice); delta_theta = params['largura_cordao'] / safe_r_helice
                    t_fita_var = np.linspace(-0.5, 0.5, 3); TH, T_FITA_MULT = np.meshgrid(theta_helice, t_fita_var); XH, _ = np.meshgrid(x_final_helice, t_fita_var)
                    R_HELICE_MESH, _ = np.meshgrid(r_helice, t_fita_var); Y_solda = R_HELICE_MESH * np.cos(TH + T_FITA_MULT * delta_theta); Z_solda = R_HELICE_MESH * np.sin(TH + T_FITA_MULT * delta_theta)
                    X_solda = XH; self.ax.plot_surface(X_solda, Y_solda, Z_solda, color=layer_colors[i % len(layer_colors)], alpha=0.9)
                elif mode in ('oscilacao', 'oscilacao_linear', 'oscilacao_quadrada'):
                     passo_axial_osc = params['oscilacao_comprimento'] * (params['sobreposicao'] / 100.0); passo_axial_osc = max(passo_axial_osc, 1e-6)
                     num_passos_axiais = math.ceil(length_base / passo_axial_osc) if length_base > 0 else 1
                     x_positions = np.linspace(0, length_base, num_passos_axiais + 1) if direction == 'esquerda_direita' else np.linspace(length_base, 0, num_passos_axiais + 1)
                     theta_ring = np.linspace(0, 2*np.pi, 50)
                     r_ring = np.full_like(theta_ring, r_layer)
                     y_ring = r_ring * np.cos(theta_ring)
                     z_ring = r_ring * np.sin(theta_ring)

                     for k in range(num_passos_axiais):
                         x_start_band = x_positions[k]
                         x_end_band = x_positions[k+1]
                         X_band, T_band = np.meshgrid([x_start_band, x_end_band], theta_ring)
                         R_band = np.full_like(T_band, r_layer)
                         Y_band = R_band * np.cos(T_band)
                         Z_band = R_band * np.sin(T_band)
                         self.ax.plot_surface(X_band, Y_band, Z_band, color=layer_colors[i % len(layer_colors)], alpha=0.7)
            d_final_plot = params['diametro'] + params['num_camadas'] * 2 * params['espessura_camada']
            max_radius_plot = max(d_final_plot/2.0, d_base/2.0); max_radius_plot = max(max_radius_plot, 1e-6)
            lead_in_val = params.get('lead_in', 0.0); lead_out_val = params.get('lead_out', 0.0); x_min_plot = 0 - lead_in_val; x_max_plot = length_base + lead_out_val
            x_range = x_max_plot - x_min_plot; x_range = x_range if x_range > 0 else 1; y_range = max_radius_plot * 2; z_range = y_range
            self.ax.set_xlim(x_min_plot, x_max_plot); self.ax.set_ylim(-max_radius_plot, max_radius_plot); self.ax.set_zlim(-max_radius_plot, max_radius_plot)
            # Aspect ratio equivalente para evitar distorção
            try:
                self.ax.set_aspect('equal', adjustable='box')
            except Exception:
                self.ax.set_box_aspect((x_range, y_range, z_range))
            # Eixos e marcadores
            try:
                self.ax.plot([x_min_plot, x_max_plot], [0, 0], [0, 0], color='#111', lw=1)
                self.ax.plot([0, 0], [-max_radius_plot, max_radius_plot], [0, 0], color='#111', lw=1)
                self.ax.plot([0, 0], [0, 0], [-max_radius_plot, max_radius_plot], color='#111', lw=1)
                self.ax.text(x_max_plot, 0, 0, 'X', color='#111')
                self.ax.text(0, max_radius_plot, 0, 'Y', color='#111')
                self.ax.text(0, 0, max_radius_plot, 'Z', color='#111')
            except Exception:
                pass
            self.ax.set_xlabel("Eixo X (Comprimento)"); self.ax.set_ylabel("Y"); self.ax.set_zlabel("Eixo Z (Raio)")
            # Título removido
            try:
                self.canvas.draw_idle()
            except Exception:
                self.canvas.draw()
        except Exception as e:
            print(f"Erro ao desenhar percurso 3D: {e}"); self.canvas.draw()

    def _on_motion_3d(self, event):
        try:
            if event.inaxes == self.ax and hasattr(self, 'coord_hint_var'):
                # Usa format_coord do Axes3D para mostrar x, y, z estimados
                coord_text = self.ax.format_coord(event.xdata, event.ydata)
                # Normaliza texto para etiqueta
                self.coord_hint_var.set(coord_text)
        except Exception:
            pass

    def _view_top(self):
        try:
            self.ax.view_init(elev=90, azim=0)
            self.canvas.draw_idle()
        except Exception:
            pass

    def _view_side(self):
        try:
            self.ax.view_init(elev=0, azim=0)
            self.canvas.draw_idle()
        except Exception:
            pass

    def _view_iso(self):
        try:
            self.ax.view_init(elev=25, azim=45)
            self.canvas.draw_idle()
        except Exception:
            pass

    def _update_gcode_preview(self):
        try:
            params = self._get_current_params()
            if params is None:
                if hasattr(self, 'gcode_text'): self.gcode_text.config(state='normal'); self.gcode_text.delete('1.0', tk.END); self.gcode_text.insert('1.0', "Por favor, verifique os parâmetros."); self.gcode_text.config(state='disabled')
                if hasattr(self, 'gcode_line_count_var'): self._last_gcode_line_count = 0; self.gcode_line_count_var.set("Linhas: -")
                return
            gcode_output = self.gcode_generator.generate(params)
            if gcode_output:
                if isinstance(gcode_output, (list, tuple)): full_gcode = "\n".join(map(str, gcode_output))
                else: full_gcode = str(gcode_output)
                if hasattr(self, 'gcode_text'):
                    self.gcode_text.config(state='normal'); self.gcode_text.delete('1.0', tk.END); self.gcode_text.insert('1.0', full_gcode)
                    # Aplica coloração semântica e destaca eixo/linhas
                    self._apply_gcode_syntax_highlight()
                    self.gcode_text.config(state='disabled')
                if hasattr(self, 'gcode_line_count_var'):
                    self._last_gcode_line_count = len(full_gcode.splitlines())
                    self.gcode_line_count_var.set(f"Linhas: {self._last_gcode_line_count}")
            else:
                if hasattr(self, 'gcode_text'): self.gcode_text.config(state='normal'); self.gcode_text.delete('1.0', tk.END); self.gcode_text.insert('1.0', "Erro ao gerar G-code preview."); self.gcode_text.config(state='disabled')
        except Exception as e:
            try:
                if hasattr(self, 'gcode_text'):
                    self.gcode_text.config(state='normal'); self.gcode_text.delete('1.0', tk.END); self.gcode_text.insert('1.0', f"Erro: {e}"); self.gcode_text.config(state='disabled')
                if hasattr(self, 'gcode_line_count_var'):
                    self._last_gcode_line_count = 0
                    self.gcode_line_count_var.set("Linhas: -")
            except Exception:
                pass

    def _update_gcode_preview_async(self):
        try:
            if hasattr(self, 'gcode_line_count_var'):
                self.gcode_line_count_var.set("Gerando…")
        except Exception:
            pass
        if getattr(self, '_gcode_preview_thread_running', False):
            return
        self._gcode_preview_thread_running = True

        def _worker():
            res = {}
            try:
                params = self._get_current_params()
                if params is None:
                    res = {'error': 'params'}
                else:
                    gcode_output = self.gcode_generator.generate(params)
                    if isinstance(gcode_output, (list, tuple)):
                        full_gcode = "\n".join(map(str, gcode_output))
                    else:
                        full_gcode = str(gcode_output) if gcode_output else ""
                    res = {'text': full_gcode, 'count': len(full_gcode.splitlines()) if full_gcode else 0}
            except Exception as e:
                res = {'error': str(e)}

            def _apply():
                try:
                    if res.get('error'):
                        if hasattr(self, 'gcode_text'):
                            self.gcode_text.config(state='normal'); self.gcode_text.delete('1.0', tk.END); self.gcode_text.insert('1.0', "Erro ao gerar G-code preview."); self.gcode_text.config(state='disabled')
                        if hasattr(self, 'gcode_line_count_var'):
                            self._last_gcode_line_count = 0; self.gcode_line_count_var.set("Linhas: -")
                    else:
                        full_gcode = res.get('text', '')
                        if hasattr(self, 'gcode_text'):
                            self.gcode_text.config(state='normal'); self.gcode_text.delete('1.0', tk.END); self.gcode_text.insert('1.0', full_gcode)
                            self._apply_gcode_syntax_highlight()
                            self.gcode_text.config(state='disabled')
                        if hasattr(self, 'gcode_line_count_var'):
                            self._last_gcode_line_count = res.get('count', 0)
                            self.gcode_line_count_var.set(f"Linhas: {self._last_gcode_line_count}")
                finally:
                    self._gcode_preview_thread_running = False

            try:
                self.root.after(0, _apply)
            except Exception:
                _apply()

        try:
            threading.Thread(target=_worker, daemon=True).start()
        except Exception:
            # Fallback para modo síncrono
            try:
                self._update_gcode_preview()
            except Exception:
                pass

    def _init_gcode_viewer_tags(self):
        try:
            # Cores de alto contraste
            self.gcode_text.tag_config('rapid', foreground='#e31a1c')   # G00
            self.gcode_text.tag_config('linear', foreground='#33a02c')  # G01
            self.gcode_text.tag_config('axis_x', foreground='#1f78b4')
            self.gcode_text.tag_config('axis_z', foreground='#6a3d9a')
            self.gcode_text.tag_config('axis_a', foreground='#ff7f00')
            self.gcode_text.tag_config('comment', foreground='#888888')
            self.gcode_text.tag_config('current', background='#ffff99')
            self.gcode_text.tag_config('search', background='#ffd92f')
        except Exception:
            pass

    def _apply_gcode_syntax_highlight(self):
        try:
            text = self.gcode_text.get('1.0', tk.END)
            lines = text.splitlines()
            # Limpa tags anteriores
            for tag in ('rapid','linear','axis_x','axis_z','axis_a','comment','search','current'):
                self.gcode_text.tag_remove(tag, '1.0', tk.END)
            for i, line in enumerate(lines, start=1):
                start = f"{i}.0"; end = f"{i}.end"
                if line.strip().startswith(('(', ';')):
                    self.gcode_text.tag_add('comment', start, end)
                    continue
                if 'G00' in line:
                    self.gcode_text.tag_add('rapid', start, end)
                elif 'G01' in line:
                    self.gcode_text.tag_add('linear', start, end)
                if ' X' in line or line.strip().startswith('X'):
                    self.gcode_text.tag_add('axis_x', start, end)
                if ' Z' in line or line.strip().startswith('Z'):
                    self.gcode_text.tag_add('axis_z', start, end)
                if ' A' in line or line.strip().startswith('A'):
                    self.gcode_text.tag_add('axis_a', start, end)
        except Exception:
            pass

    def _search_gcode(self):
        try:
            query = self.gcode_search_var.get().strip()
            if not query:
                return
            self.gcode_text.config(state='normal')
            self.gcode_text.tag_remove('search', '1.0', tk.END)
            idx = '1.0'
            while True:
                idx = self.gcode_text.search(query, idx, nocase=True, stopindex=tk.END)
                if not idx:
                    break
                lineend = f"{idx.split('.')[0]}.end"
                self.gcode_text.tag_add('search', idx, lineend)
                idx = f"{int(idx.split('.')[0]) + 1}.0"
            self.gcode_text.config(state='disabled')
        except Exception:
            pass

    def _clear_gcode_search(self):
        try:
            self.gcode_text.config(state='normal')
            self.gcode_text.tag_remove('search', '1.0', tk.END)
            self.gcode_text.config(state='disabled')
            # Desmarca filtros de eixo e atualiza destaques
            if hasattr(self, 'filter_x'): self.filter_x.set(False)
            if hasattr(self, 'filter_z'): self.filter_z.set(False)
            if hasattr(self, 'filter_a'): self.filter_a.set(False)
            try:
                self._apply_axis_filters()
            except Exception:
                pass
        except Exception:
            pass

    def _apply_axis_filters(self):
        try:
            # Em vez de ocultar linhas, aplicamos destaque leve às linhas dos eixos selecionados
            self.gcode_text.config(state='normal')
            self.gcode_text.tag_remove('search', '1.0', tk.END)
            text = self.gcode_text.get('1.0', tk.END)
            lines = text.splitlines()
            for i, line in enumerate(lines, start=1):
                add = False
                if self.filter_x.get() and (' X' in line or line.strip().startswith('X')):
                    add = True
                if self.filter_z.get() and (' Z' in line or line.strip().startswith('Z')):
                    add = True
                if self.filter_a.get() and (' A' in line or line.strip().startswith('A')):
                    add = True
                if add:
                    self.gcode_text.tag_add('search', f"{i}.0", f"{i}.end")
            self.gcode_text.config(state='disabled')
        except Exception:
            pass

    def _copy_gcode_all(self):
        try:
            all_text = self.gcode_text.get('1.0', tk.END)
            self.root.clipboard_clear(); self.root.clipboard_append(all_text)
        except Exception:
            pass

    def _highlight_gcode_line(self, line_no: int):
        try:
            self.gcode_text.config(state='normal')
            self.gcode_text.tag_remove('current', '1.0', tk.END)
            self.gcode_text.tag_add('current', f"{line_no}.0", f"{line_no}.end")
            self.gcode_text.see(f"{line_no}.0")
            self.gcode_text.config(state='disabled')
        except Exception:
            pass

    def _update_temporal_plot(self):
        try:
            params = self._get_current_params()
            if params is None:
                return
            gcode_output = self.gcode_generator.generate(params)
            if not gcode_output:
                # Limpa gráfico se não houver dados
                if hasattr(self, 'ax_temporal'):
                    self.ax_temporal.clear(); self.ax_temporal.set_title("Análise Temporal (sem dados)"); self.ax_temporal.set_xlabel("Passo"); self.ax_temporal.set_ylabel("Valor")
                    if hasattr(self, 'canvas_temporal'): self.canvas_temporal.draw()
                return
            lines = list(map(str, gcode_output)) if isinstance(gcode_output, (list, tuple)) else str(gcode_output).splitlines()
            x_vals, z_vals, a_vals = [], [], []
            current_x = current_z = current_a = 0.0
            for line in lines:
                x_match = re.search(r"X([-+]?\d*\.?\d+)", line)
                z_match = re.search(r"Z([-+]?\d*\.?\d+)", line)
                a_match = re.search(r"A([-+]?\d*\.?\d+)", line)
                if x_match: current_x = float(x_match.group(1))
                if z_match: current_z = float(z_match.group(1))
                if a_match: current_a = float(a_match.group(1))
                # Detecta movimentos G0/G00, G1/G01, G2/G02, G3/G03
                if re.search(r"\bG(?:0|00|1|01|2|02|3|03)\b", line):
                    x_vals.append(current_x); z_vals.append(current_z); a_vals.append(current_a)

            if hasattr(self, 'ax_temporal'):
                self.ax_temporal.clear()
                if x_vals or z_vals or a_vals:
                    n = max(len(x_vals), len(z_vals), len(a_vals))
                    steps = range(1, n + 1)
                    def pad(seq, n):
                        return seq + ([seq[-1]] if seq else [0.0]) * (n - len(seq))
                    self.ax_temporal.plot(steps, pad(x_vals, n), label="X")
                    self.ax_temporal.plot(steps, pad(z_vals, n), label="Z")
                    self.ax_temporal.plot(steps, pad(a_vals, n), label="A")
                    self.ax_temporal.set_title("Análise Temporal (X, Z, A)"); self.ax_temporal.set_xlabel("Passo"); self.ax_temporal.set_ylabel("Valor"); self.ax_temporal.legend(loc="upper right")
                else:
                    self.ax_temporal.set_title("Análise Temporal (sem movimentos)"); self.ax_temporal.set_xlabel("Passo"); self.ax_temporal.set_ylabel("Valor")
                if hasattr(self, 'canvas_temporal'): self.canvas_temporal.draw()
        except Exception as e:
            try:
                if hasattr(self, 'ax_temporal'):
                    self.ax_temporal.set_title(f"Análise Temporal (erro: {e})")
                    if hasattr(self, 'canvas_temporal'): self.canvas_temporal.draw()
            except Exception:
                pass

    def _update_oscillation_plot(self):
        try:
            params = self._get_current_params()
            if params is None:
                return
            gcode_output = self.gcode_generator.generate(params)
            if not gcode_output:
                if hasattr(self, 'ax_oscilacao'):
                    self.ax_oscilacao.clear(); self.ax_oscilacao.set_title("Oscilação (sem dados)"); self.ax_oscilacao.set_xlabel("Passo"); self.ax_oscilacao.set_ylabel("Posição")
                    if hasattr(self, 'canvas_oscilacao'): self.canvas_oscilacao.draw()
                return
            lines = list(map(str, gcode_output)) if isinstance(gcode_output, (list, tuple)) else str(gcode_output).splitlines()
            x_vals = []
            current_x = 0.0
            for line in lines:
                # Captura últimos valores de A e X encontrados
                x_match = re.search(r"X([-+]?\d*\.?\d+)", line)
                if x_match: current_x = float(x_match.group(1))
                # Adiciona amostra quando há movimento G0/G00/G1/G01/G2/G02/G3/G03
                if re.search(r"\bG(?:0|00|1|01|2|02|3|03)\b", line):
                    x_vals.append(current_x)

            if hasattr(self, 'ax_oscilacao'):
                self.ax_oscilacao.clear()
                if x_vals:
                    n = len(x_vals)
                    steps = range(1, n + 1)
                    def pad(seq, n):
                        return seq + ([seq[-1]] if seq else [0.0]) * (n - len(seq))
                    self.ax_oscilacao.plot(steps, pad(x_vals, n), label="Posição X")
                    self.ax_oscilacao.set_title("Oscilação (X por passo)"); self.ax_oscilacao.set_xlabel("Passo"); self.ax_oscilacao.set_ylabel("Posição"); self.ax_oscilacao.legend(loc="upper right")
                else:
                    self.ax_oscilacao.set_title("Oscilação (sem movimentos)"); self.ax_oscilacao.set_xlabel("Passo"); self.ax_oscilacao.set_ylabel("Posição")
                if hasattr(self, 'canvas_oscilacao'): self.canvas_oscilacao.draw()
        except Exception as e:
            try:
                if hasattr(self, 'ax_oscilacao'):
                    self.ax_oscilacao.set_title(f"Oscilação (erro: {e})")
                    if hasattr(self, 'canvas_oscilacao'): self.canvas_oscilacao.draw()
            except Exception:
                pass

    def _update_statistics_plot(self):
        try:
            params = self._get_current_params()
            if params is None:
                return
            gcode_output = self.gcode_generator.generate(params)
            if not gcode_output:
                if hasattr(self, 'ax_est_top') and hasattr(self, 'ax_est_bottom'):
                    self.ax_est_top.clear(); self.ax_est_bottom.clear()
                    self.ax_est_top.set_title("Distância por passo (sem dados)"); self.ax_est_top.set_xlabel("Passo"); self.ax_est_top.set_ylabel("Distância")
                    self.ax_est_bottom.set_title("Histograma das distâncias"); self.ax_est_bottom.set_xlabel("Distância"); self.ax_est_bottom.set_ylabel("Contagem")
                    if hasattr(self, 'canvas_estatisticas'): self.canvas_estatisticas.draw()
                return
            lines = list(map(str, gcode_output)) if isinstance(gcode_output, (list, tuple)) else str(gcode_output).splitlines()
            x_vals, z_vals = [], []
            current_x = current_z = None
            prev_x = prev_z = None
            dist_steps = []
            for line in lines:
                x_match = re.search(r"X([-+]?\d*\.?\d+)", line)
                z_match = re.search(r"Z([-+]?\d*\.?\d+)", line)
                if x_match: current_x = float(x_match.group(1))
                if z_match: current_z = float(z_match.group(1))
                if re.search(r"\bG(?:0|00|1|01|2|02|3|03)\b", line):
                    if current_x is not None and current_z is not None:
                        x_vals.append(current_x); z_vals.append(current_z)
                        if prev_x is None or prev_z is None:
                            dist_steps.append(0.0)
                        else:
                            dist_steps.append(math.hypot(current_x - prev_x, current_z - prev_z))
                        prev_x, prev_z = current_x, current_z

            if hasattr(self, 'ax_est_top') and hasattr(self, 'ax_est_bottom'):
                self.ax_est_top.clear(); self.ax_est_bottom.clear()
                if dist_steps:
                    steps = range(1, len(dist_steps) + 1)
                    cum = []
                    total = 0.0
                    for d in dist_steps:
                        total += d; cum.append(total)
                    self.ax_est_top.plot(steps, dist_steps, label="Distância")
                    self.ax_est_top.plot(steps, cum, label="Acumulado")
                    self.ax_est_top.set_title("Distância por passo e acumulado"); self.ax_est_top.set_xlabel("Passo"); self.ax_est_top.set_ylabel("Distância"); self.ax_est_top.legend(loc="upper right")
                    try:
                        self.ax_est_bottom.hist(dist_steps, bins=max(10, min(40, len(dist_steps)//3)), color="#4c72b0", edgecolor="black")
                    except Exception:
                        self.ax_est_bottom.hist(dist_steps, bins=10, color="#4c72b0", edgecolor="black")
                    self.ax_est_bottom.set_title("Histograma das distâncias"); self.ax_est_bottom.set_xlabel("Distância"); self.ax_est_bottom.set_ylabel("Contagem")
                else:
                    self.ax_est_top.set_title("Distância por passo (sem movimentos)"); self.ax_est_top.set_xlabel("Passo"); self.ax_est_top.set_ylabel("Distância")
                    self.ax_est_bottom.set_title("Histograma das distâncias"); self.ax_est_bottom.set_xlabel("Distância"); self.ax_est_bottom.set_ylabel("Contagem")
                try:
                    self.fig_estatisticas.tight_layout()
                except Exception:
                    pass
                if hasattr(self, 'canvas_estatisticas'): self.canvas_estatisticas.draw()
        except Exception as e:
            try:
                if hasattr(self, 'ax_est_top') and hasattr(self, 'ax_est_bottom'):
                    self.ax_est_top.set_title(f"Estatísticas (erro: {e})")
                    if hasattr(self, 'canvas_estatisticas'): self.canvas_estatisticas.draw()
            except Exception:
                pass

    def _update_process_plots(self):
        try:
            params = self._get_current_params()
            if params is None:
                # Limpa e titula eixos
                if hasattr(self, 'ax_proc_cost'):
                    self.ax_proc_cost.clear(); self.ax_proc_cost.set_title("Custos — % por categoria")
                if hasattr(self, 'ax_proc_cons'):
                    self.ax_proc_cons.clear(); self.ax_proc_cons.set_title("Consumo (pó e gás)")
                if hasattr(self, 'ax_proc_len'):
                    self.ax_proc_len.clear(); self.ax_proc_len.set_title("Comprimento acumulado por camada")
                if hasattr(self, 'ax_proc_time'):
                    self.ax_proc_time.clear(); self.ax_proc_time.set_title("Tempo total")
                if hasattr(self, 'canvas_processo'): self.canvas_processo.draw()
                return

            # Avalia fórmulas no runtime
            formula_res = {}
            try:
                formula_res = self._evaluate_formulas_runtime(params) or {}
            except Exception:
                formula_res = {}

            # --- Gráfico de pizza de custos ---
            if hasattr(self, 'ax_proc_cost'):
                self.ax_proc_cost.clear()
                labels_cost = ["Pó", "Gás", "Mão de Obra", "Máquina"]
                keys_cost = ["powder_cost_brl", "gas_cost_brl", "labor_cost_brl", "machine_cost_brl"]
                values = []
                for k in keys_cost:
                    v = formula_res.get(k)
                    values.append(float(v) if isinstance(v, (int, float)) and v >= 0 else 0.0)
                total_cost = sum(values)
                if total_cost > 0:
                    self.ax_proc_cost.pie(values, labels=labels_cost, autopct='%1.1f%%', startangle=90)
                    self.ax_proc_cost.axis('equal')
                    self.ax_proc_cost.set_title("Custos — % por categoria")
                else:
                    self.ax_proc_cost.text(0.5, 0.5, "Sem dados de custo", ha='center', va='center')
                    self.ax_proc_cost.axis('off')

            # --- Barras de consumo ---
            if hasattr(self, 'ax_proc_cons'):
                self.ax_proc_cons.clear()
                pm_kg = formula_res.get('powder_mass_kg')
                gas_l = formula_res.get('gas_consumption_l')
                pm_kg_val = float(pm_kg) if isinstance(pm_kg, (int, float)) and pm_kg >= 0 else 0.0
                gas_m3_val = (float(gas_l) / 1000.0) if isinstance(gas_l, (int, float)) and gas_l >= 0 else 0.0
                cats = ["Pó (kg)", "Gás (m³)"]
                vals = [pm_kg_val, gas_m3_val]
                bars = self.ax_proc_cons.bar(cats, vals, color=["#6baed6", "#31a354"])
                for rect, v in zip(bars, vals):
                    self.ax_proc_cons.text(rect.get_x() + rect.get_width()/2.0, rect.get_height(), f"{v:.3f}", ha='center', va='bottom')
                self.ax_proc_cons.set_ylim(0, max(vals + [1e-6]) * 1.2)
                self.ax_proc_cons.set_ylabel("Quantidade")
                self.ax_proc_cons.set_title("Consumo (pó e gás)")

            # --- Comprimento acumulado por camada ---
            if hasattr(self, 'ax_proc_len'):
                self.ax_proc_len.clear()
                per_layer_len = formula_res.get('total_length_mm_per_layer')
                num_camadas = params.get('num_camadas')
                try:
                    C = int(num_camadas) if num_camadas is not None else 0
                except Exception:
                    C = 0
                if isinstance(per_layer_len, (int, float)) and per_layer_len > 0 and C > 0:
                    steps = list(range(1, C + 1))
                    cum = [i * per_layer_len for i in steps]
                    self.ax_proc_len.plot(steps, cum, label="Acumulado (mm)")
                    self.ax_proc_len.set_xlabel("Camada")
                    self.ax_proc_len.set_ylabel("Comprimento (mm)")
                    self.ax_proc_len.legend(loc='upper left')
                    self.ax_proc_len.set_title("Comprimento acumulado por camada")
                else:
                    self.ax_proc_len.text(0.5, 0.5, "Sem dados de comprimento", ha='center', va='center')
                    self.ax_proc_len.axis('off')

            # --- Tempo total ---
            if hasattr(self, 'ax_proc_time'):
                self.ax_proc_time.clear()
                time_min_mass = formula_res.get('time_min_mass')
                time_min = formula_res.get('time_min')
                # Prioriza tempo por massa se disponível
                t_minutes = None
                if isinstance(time_min_mass, (int, float)) and time_min_mass >= 0:
                    t_minutes = float(time_min_mass)
                    label = "Tempo por massa"
                elif isinstance(time_min, (int, float)) and time_min >= 0:
                    t_minutes = float(time_min)
                    label = "Tempo estimado"
                if t_minutes is not None:
                    bars = self.ax_proc_time.bar([label], [t_minutes], color="#ff7f0e")
                    self.ax_proc_time.set_ylabel("Minutos")
                    self.ax_proc_time.set_title("Tempo total")
                    # rótulo
                    hh = int(t_minutes // 60)
                    mm = int(round(t_minutes % 60))
                    self.ax_proc_time.text(0, t_minutes, f"{hh:02d}h{mm:02d}", ha='center', va='bottom')
                    self.ax_proc_time.set_ylim(0, max(t_minutes, 1e-6) * 1.2)
                else:
                    self.ax_proc_time.text(0.5, 0.5, "Sem dados de tempo", ha='center', va='center')
                    self.ax_proc_time.axis('off')

            try:
                if hasattr(self, 'fig_processo'):
                    self.fig_processo.tight_layout()
            except Exception:
                pass
            if hasattr(self, 'canvas_processo'):
                self.canvas_processo.draw()
        except Exception as e:
            try:
                if hasattr(self, 'ax_proc_cost'):
                    self.ax_proc_cost.set_title(f"Processo (erro: {e})")
                if hasattr(self, 'canvas_processo'):
                    self.canvas_processo.draw()
            except Exception:
                pass

    def _save_procedure(self):
        from pathlib import Path
        proc_path_conf = self.config['database']['procedures_path']
        proc_path = Path(proc_path_conf)
        if not proc_path.is_absolute():
            root_dir = Path(sys.executable).resolve().parent if getattr(sys, 'frozen', False) else Path(__file__).resolve().parents[2]
            proc_path = root_dir / proc_path
        filepath = filedialog.asksaveasfilename(initialdir=str(proc_path), title="Salvar Procedimento...", defaultextension=".json", filetypes=[("TFM Procedure Files", "*.json")])
        if not filepath: return
        try:
            data_to_save = self._get_current_params();
            if data_to_save is None: self.show_notification("Por favor, verifique os parâmetros.", 'error'); return
            data_to_save['welding_mode'] = data_to_save.get('welding_mode', 'espiral')
            if data_to_save['welding_mode'] == 'espiral':
                 data_to_save.pop('oscilacao_comprimento', None); data_to_save.pop('deslocamento_angular_perc', None)
                 data_to_save.pop('oscillation_type', None)
            elif data_to_save['welding_mode'] == 'oscilacao':
                 # Garantir que o tipo seja salvo
                 data_to_save['oscillation_type'] = data_to_save.get('oscillation_type', 'linear')
            with open(filepath, 'w', encoding='utf-8') as f: json.dump(data_to_save, f, indent=4)
            self.show_notification("Procedimento salvo com sucesso!", 'success')
        except Exception as e: self.show_notification(f"Erro: {str(e)}", 'error')

    def _load_procedure(self):
        from pathlib import Path
        proc_path_conf = self.config['database']['procedures_path']
        proc_path = Path(proc_path_conf)
        if not proc_path.is_absolute():
            root_dir = Path(sys.executable).resolve().parent if getattr(sys, 'frozen', False) else Path(__file__).resolve().parents[2]
            proc_path = root_dir / proc_path
        filepath = filedialog.askopenfilename(initialdir=str(proc_path), title="Carregar Procedimento...", filetypes=[("TFM Procedure Files", "*.json")])
        if not filepath: return
        try:
            with open(filepath, 'r', encoding='utf-8') as f: loaded_data = json.load(f)
            self.notes_text.delete("1.0", tk.END)
            loaded_data.setdefault('lead_in', '5.0'); loaded_data.setdefault('lead_out', '5.0');
            loaded_data.setdefault('direcao_soldagem', 'esquerda_direita'); loaded_data.setdefault('welding_mode', 'espiral')
            loaded_data.setdefault('oscilacao_comprimento', '10.0'); loaded_data.setdefault('deslocamento_angular_perc', '50.0');
            loaded_data.setdefault('sentido_rotacao', 'horaria')
            self._disable_param_traces()
            for key, value in loaded_data.items():
                if key in self.params:
                    if isinstance(self.params[key], tk.BooleanVar):
                        self.params[key].set(bool(value))
                    elif key == 'direcao_soldagem':
                        # Converte valor interno para valor de display
                        direction_options = {
                            'esquerda_direita': 'Esquerda -> Direita',
                            'direita_esquerda': 'Direita -> Esquerda'
                        }
                        self.params['direcao_soldagem'].set(direction_options.get(value, 'Esquerda -> Direita'))
                    elif key == 'sentido_rotacao':
                        rotation_options = {
                            'horaria': 'Horária (CW)',
                            'antihoraria': 'Anti-horária (CCW)'
                        }
                        self.params['sentido_rotacao'].set(rotation_options.get(value, 'Horária (CW)'))
                    elif key == 'oscillation_type':
                        # Define o valor de display do tipo de oscilação
                        display_map = {
                            'linear': 'Linear',
                            'quadrada': 'Quadrada',
                            'quadrada_continua': 'Quadrada Contínua'
                        }
                        self.params['tipo_oscilacao'].set(display_map.get(value, 'Linear'))
                    # Removido: controle de rotação via spindle; sempre eixo A
                    elif key == 'd_inicial':
                        self.params['diametro'].set(str(value))
                    elif key == 'diametro':
                        self.params['diametro'].set(str(value))
                    elif key != 'd_final':
                        self.params[key].set(str(value))
                elif key == 'notes': self.notes_text.insert("1.0", value)
                elif key == 'welding_mode':
                     if value in ('oscilacao', 'oscilacao_linear', 'oscilacao_quadrada'):
                         # Seleciona a aba unificada e ajusta tipo conforme modo antigo
                         self.notebook.select(self.tab_osc)
                         if value == 'oscilacao_quadrada': self.params['tipo_oscilacao'].set('Quadrada')
                         elif value == 'oscilacao_linear': self.params['tipo_oscilacao'].set('Linear')
                     else:
                         self.notebook.select(self.tab_espiral)
            self._enable_param_traces(); self.trigger_update()
            self.show_notification("Procedimento carregado com sucesso!", 'success')
        except Exception as e: self.show_notification(f"Erro ao carregar: {e}", 'error'); self._enable_param_traces()

    def _disable_param_traces(self):
        for var_name, var in self.params.items():
            if isinstance(var, (tk.StringVar, tk.BooleanVar)) :
                info = var.trace_info()
                if info and info[0]:
                    try: trace_id = info[0][1]; var.trace_remove("write", trace_id)
                    except (IndexError, tk.TclError): pass
        self.notes_text.unbind("<KeyRelease>")
        self.notebook.unbind("<<NotebookTabChanged>>")

    def _enable_param_traces(self):
        for var_name, var in self.params.items():
            if isinstance(var, (tk.StringVar, tk.BooleanVar)):
                 if var_name not in []: var.trace_add("write", self.trigger_update)
        self.notes_text.bind("<KeyRelease>", self.trigger_update)
        self.notebook.bind("<<NotebookTabChanged>>", self.trigger_update)

    def _generate_report(self):
        if not REPORTLAB_AVAILABLE: self.show_notification("O módulo 'reportlab' não foi encontrado. A geração de PDF está desabilitada.", 'warning'); return
        params = self._get_current_params();
        if params is None: self.show_notification("Por favor, verifique os parâmetros.", 'error'); return
        filepath = filedialog.asksaveasfilename(defaultextension=".pdf", filetypes=[("PDF Files", "*.pdf")])
        if not filepath: return
        try:
            results = {key: var.get() for key, var in self.resultados.items()}; costs = {key: var.get() for key, var in self.custos.items()}
            img_path = "temp_report_img.png"; self.fig.savefig(img_path, dpi=150, bbox_inches='tight', pad_inches=0.1)
            c = pdfcanvas.Canvas(filepath, pagesize=A4); width, height = A4; margin = 20 * mm
            c.setFont("Helvetica-Bold", 18); c.drawString(margin, height - margin, "Relatório de Procedimento PTA")
            logo_path = resource_path('assets/logo.png')
            if os.path.exists(logo_path):
                try:
                    logo = ImageReader(logo_path)
                    img_w, img_h = logo.getSize(); aspect = img_h / float(img_w) if img_w > 0 else 1
                    draw_w = 40 * mm; draw_h = draw_w * aspect
                    c.drawImage(logo, width - margin - draw_w, height - margin - (draw_h/2), width=draw_w, height=draw_h, preserveAspectRatio=True, mask='auto')
                except Exception as _logo_err:
                    # Se o arquivo estiver corrompido ou em formato incompatível, continua sem o logo
                    self.show_notification(f"Logo inválido: {os.path.basename(logo_path)}. Gerando PDF sem logo.", 'warning')
            c.setFont("Helvetica", 8); c.drawString(margin, height - margin - 8*mm, f"Data: {datetime.now().strftime('%d/%m/%Y %H:%M:%S')}"); c.line(margin, height - margin - 10*mm, width - margin, height - margin - 10*mm)
            img_w_px, img_h_px = ImageReader(img_path).getSize(); aspect = img_h_px / float(img_w_px) if img_w_px > 0 else 1
            draw_w = width - 2 * margin; draw_h = draw_w * aspect; img_y_start = height - margin - 15*mm
            if draw_h > img_y_start - 140*mm: draw_h = img_y_start - 140*mm; draw_w = draw_h / aspect if aspect > 0 else 0
            c.drawImage(img_path, (width - draw_w) / 2, img_y_start - draw_h, width=draw_w, height=draw_h, preserveAspectRatio=True)
            y_pos = img_y_start - draw_h - 10*mm; col1_x = margin; col2_x = width / 2 + 5 * mm; line_height = 5 * mm
            
            def draw_section(title_text, data, x_start, start_y):
                current_y = start_y; c.setFont("Helvetica-Bold", 11); c.drawString(x_start, current_y, title_text); current_y -= line_height * 1.5; c.setFont("Helvetica", 9)
                for key_text, value in data.items(): c.drawString(x_start + 5*mm, current_y, f"{key_text}:"); c.drawString(x_start + 55*mm, current_y, str(value)); current_y -= line_height
                return start_y - current_y

            col1_y = y_pos
            geom_data = {
                "Nome Procedimento": params['nome_procedimento'], "Modo de Soldagem": params['welding_mode'],
                "Diâmetro": f"{params['diametro']:.2f} mm", "Comprimento": f"{params['comprimento_revestir']:.2f} mm"
            }
            col1_y -= draw_section("Parâmetros Gerais", geom_data, col1_x, col1_y); col1_y -= line_height
            layer_data = { "Nº de Camadas": params['num_camadas'], "Espessura / Camada": f"{params['espessura_camada']:.2f} mm" }; col1_y -= draw_section("Parâmetros de Camada", layer_data, col1_x, col1_y)
            
            col2_y = y_pos
            proc_data = {
                "Corrente Arco": f"{params['corrente_arco']:.1f} A",
                "Vazão Gás": f"{params['vazao_gas']:.1f} l/min",
                "Alimentação Pó": f"{params['alim_po']:.1f} %",
                "Pré-aquecimento": f"{params['preaquecimento']:.1f} °C",
                "Tipo de Pó": params.get('powder_name', '') or '',
                "OS": params.get('ordem_servico', '') or ''
            }
            col2_y -= draw_section("Parâmetros do Processo", proc_data, col2_x, col2_y); col2_y -= line_height
            
            # Resultados sem velocidade da placa
            results_data = {
                "Rotação (RPM)": results['rotation_rpm'],
                "Passo Axial (mm)": results['helix_pitch'],
                "Rotações/Passos": results['total_rotations'],
                "Ângulo Total/Passo (A)": results['total_angle_A'],
                "Tempo Total (hh:mm)": results['estimated_time']
            }
            col2_y -= draw_section("Resultados Calculados", results_data, col2_x, col2_y)

            # Rodapé: Notas do procedimento (fixo na parte inferior, texto não sobe)
            try:
                notes_text_val = self.notes_text.get("1.0", tk.END).strip()
            except Exception:
                notes_text_val = ""
            if notes_text_val:
                # Linha separadora e título
                c.setFillColor(colors.grey)
                c.line(margin, margin + 17*mm, width - margin, margin + 17*mm)
                c.setFillColor(colors.black)
                c.setFont("Helvetica-Bold", 10)
                c.drawString(margin, margin + 22*mm, "Notas:")
                # Texto das notas: renderiza abaixo do título (para baixo), com word-wrap e limite de área
                c.setFont("Helvetica", 9)
                max_w = width - 2 * margin
                line_h = 4.5 * mm
                # Constrói linhas quebradas por largura
                words = notes_text_val.split()
                wrapped_lines = []
                current = ""
                for w in words:
                    candidate = current + (" " if current else "") + w
                    if c.stringWidth(candidate, "Helvetica", 9) <= max_w:
                        current = candidate
                    else:
                        wrapped_lines.append(current)
                        current = w
                if current:
                    wrapped_lines.append(current)
                # Área disponível: do y_start para baixo até próximo do rodapé
                y_curr = margin + 12*mm
                min_y = margin + 3*mm
                for i, ln in enumerate(wrapped_lines):
                    if y_curr - line_h < min_y:
                        c.drawString(margin, y_curr, "...")
                        break
                    c.drawString(margin, y_curr, ln)
                    y_curr -= line_h
            
            c.save(); os.remove(img_path)
            if messagebox.askyesno("Sucesso", "Relatório PDF gerado com sucesso. Deseja abri-lo?"):
                try:
                    if sys.platform == "win32": os.startfile(filepath)
                    else: webbrowser.open(f'file://{os.path.abspath(filepath)}')
                except Exception: webbrowser.open(f'file://{os.path.abspath(filepath)}')
        except Exception as e: self.show_notification(f"Erro ao gerar PDF: {e}", 'error')

    def _generate_gcode_clicked(self):
        self._save_sash_positions()
        params = self._get_current_params()
        if params is None:
            self.show_notification("Por favor, verifique os parâmetros.", 'error')
            return
        # Removido: checagens de limites de máquina (gera sempre)
        try:
            full_gcode_list = self.gcode_generator.generate(params)
            if not full_gcode_list: self.show_notification("Erro ao gerar G-Code.", 'error'); return
            full_gcode = "\n".join(full_gcode_list)
            filepath = filedialog.asksaveasfilename(defaultextension=".tap", filetypes=[("G-Code Files", "*.tap"), ("All Files", "*.*")])
            if not filepath: return
            try:
                with open(filepath, 'w', encoding='utf-8') as f: f.write(full_gcode)
                self.show_notification("Arquivo G-Code gerado com sucesso!", 'success')
            except Exception as e: self.show_notification(f"Erro ao salvar G-Code: {e}", 'error')
        finally:
            # Restaura as posições dos sashes para evitar pulo da aba
            self._restore_sash_positions()

    def _download_gcode_clicked(self):
        # Salva G-code automaticamente com o nome da OS
        self._save_sash_positions()
        params = self._get_current_params()
        if params is None:
            self.show_notification("Por favor, verifique os parâmetros.", 'error')
            return
        # Removido: checagens de limites de máquina (baixa sempre)
        try:
            full_gcode_list = self.gcode_generator.generate(params)
            if not full_gcode_list:
                self.show_notification("Erro ao gerar G-Code.", 'error')
                return
            full_gcode = "\n".join(full_gcode_list)
            # Deixa o usuário escolher a pasta de destino
            initial_dir = self.config.get('integration', {}).get('gcode_dir', os.path.join(os.path.expanduser("~"), "Mach3", "GCode"))
            base_dir = filedialog.askdirectory(title="Escolha a pasta para salvar o G-code", initialdir=initial_dir, mustexist=False)
            if not base_dir:
                # Usuário cancelou
                return
            try:
                os.makedirs(base_dir, exist_ok=True)
            except Exception:
                self.show_notification(f"Não foi possível acessar a pasta selecionada: {base_dir}", 'error')
                return
            os_name = params.get('ordem_servico', '') or ''
            safe = re.sub(r"[^A-Za-z0-9._-]+", "_", os_name).strip('_')
            if not safe:
                safe = f"TFM_{datetime.now().strftime('%Y%m%d_%H%M%S')}"
            filename = f"{safe}.tap"
            filepath = os.path.join(base_dir, filename)
            try:
                with open(filepath, 'w', encoding='utf-8') as f:
                    f.write(full_gcode)
                self.show_notification(f"Arquivo G-Code salvo: {filepath}", 'success')
            except Exception as e:
                self.show_notification(f"Erro ao salvar G-Code: {e}", 'error')
        finally:
            self._restore_sash_positions()

    def _show_about_dialog(self):
         try:
             ver = APP_VERSION
         except Exception:
             ver = "(desconhecida)"
         self.show_notification(f"TFM G-Code Generator v{ver}\nDesenvolvido para TFM Usinagem & Manutenção Industrial LTDA.\nTodos os direitos reservados.", 'info', duration_ms=10000)

    def _select_mach3_path(self):
        initial_dir = r"C:\\Mach3" if os.path.exists(r"C:\\Mach3") else os.path.expanduser("~")
        path = filedialog.askopenfilename(title="Selecione o executável do Mach3",
                                          initialdir=initial_dir,
                                          filetypes=[("Executável", "*.exe"), ("Todos os arquivos", "*.*")])
        if not path:
            return
        try:
            self.config.setdefault('integration', {})
            self.config['integration']['mach3_path'] = path
            self.save_config()
            self.show_notification(f"Caminho do Mach3 atualizado:\n{path}", 'success')
        except Exception as e:
            self.show_notification(f"Erro ao salvar caminho do Mach3: {e}", 'error')

    def _install_macropump(self):
        try:
            mach3_path = self.config.get('integration', {}).get('mach3_path')
            profile = self.config.get('integration', {}).get('mach3_profile', 'PTA')
            base_dir = os.path.dirname(mach3_path) if mach3_path else r"C:\\Mach3"
            # Suporte a 'macros' e 'Macros'
            macro_root = os.path.join(base_dir, 'macros')
            if not os.path.isdir(macro_root):
                macro_root_alt = os.path.join(base_dir, 'Macros')
                macro_root = macro_root_alt if os.path.isdir(macro_root_alt) else macro_root
            # Alvos possíveis: perfil informado e perfil padrão Mach3Mill
            targets = []
            for prof in {profile, 'Mach3Mill'}:
                tdir = os.path.join(macro_root, prof)
                try:
                    os.makedirs(tdir, exist_ok=True)
                    targets.append(os.path.join(tdir, 'macropump.m1s'))
                except Exception:
                    # continuar para outros destinos
                    pass
            gcode_dir = self.config.get('integration', {}).get('gcode_dir', r"C:\\Mach3\\GCode")
            gcode_dir_vb = gcode_dir.replace('\\', '\\\\')
            vb = (
                "' macropump.m1s - Autoload de G-Code\n"
                "Dim fso, f, path\n"
                "Set fso = CreateObject(\"Scripting.FileSystemObject\")\n"
                f"If fso.FileExists(\"{gcode_dir_vb}\\\\autoload.txt\") Then\n"+
                "    On Error Resume Next\n"
                f"    Set f = fso.OpenTextFile(\"{gcode_dir_vb}\\\\autoload.txt\", 1)\n"+
                "    path = Trim(f.ReadLine)\n"
                "    f.Close\n"
                "    If (path <> \"\") Then\n"
                "        If fso.FileExists(path) Then\n"
                "            Message(\"Autoload: \" & path)\n"
                "            LoadFile(path)\n"
                "            ' DoOEMButton(1000) ' Cycle Start opcional\n"
                "        Else\n"
                "            Message(\"Autoload: arquivo não existe \" & path)\n"
                "        End If\n"
                "    Else\n"
                "        Message(\"Autoload: caminho vazio\")\n"
                "    End If\n"
                f"    fso.DeleteFile(\"{gcode_dir_vb}\\\\autoload.txt\")\n"+
                "    On Error GoTo 0\n"
                "End If\n"
            )
            written = []
            for tf in targets:
                try:
                    with open(tf, 'w', encoding='utf-8') as f:
                        f.write(vb)
                    written.append(tf)
                except Exception:
                    pass
            self.config.setdefault('integration', {})
            self.config['integration']['write_autoload_txt'] = True
            self.save_config()
            if written:
                paths = "\n".join(written)
                self.show_notification(
                    f"macropump.m1s instalado em:\n{paths}\nNo Mach3, marque \"Run Macro Pump\" em General Config e reinicie.",
                    'success'
                )
            else:
                raise PermissionError("Não foi possível escrever o macropump.m1s em nenhuma pasta de perfil. Execute o app como administrador ou instale manualmente.")
        except Exception as e:
            self.show_notification(f"Erro ao instalar macropump: {e}", 'error')

    def _ensure_mach3_macros(self):
        """Garante que os macros M101/M102 existam no perfil do Mach3.
        M101: ligar tocha (ActivateSignal OUTPUT1)
        M102: desligar tocha (DeactivateSignal OUTPUT1)
        """
        try:
            mach3_path = self.config.get('integration', {}).get('mach3_path')
            profile = self.config.get('integration', {}).get('mach3_profile', 'PTA')
            base_dir = os.path.dirname(mach3_path) if mach3_path else r"C:\\Mach3"
            macro_root = os.path.join(base_dir, 'macros')
            if not os.path.isdir(macro_root):
                macro_root_alt = os.path.join(base_dir, 'Macros')
                macro_root = macro_root_alt if os.path.isdir(macro_root_alt) else macro_root
            targets = []
            for prof in {profile, 'Mach3Mill'}:
                tdir = os.path.join(macro_root, prof)
                try:
                    os.makedirs(tdir, exist_ok=True)
                    targets.append(tdir)
                except Exception:
                    pass
            vb_on = (
                "ActivateSignal(Output1)\n"
            )
            vb_off = (
                "DeactivateSignal(Output1)\n"
            )
            created = []
            for tdir in targets:
                try:
                    m101 = os.path.join(tdir, 'M101.m1s')
                    m102 = os.path.join(tdir, 'M102.m1s')
                    # Se não existir, cria; se existir mas estiver errado (ex.: "ActivateSinal"), corrige com backup
                    def _needs_fix(path, expected_snippet):
                        try:
                            with open(path, 'r', encoding='utf-8') as f:
                                content = f.read()
                            if 'ActivateSinal' in content or 'DeactivateSinal' in content:
                                return True
                            return expected_snippet not in content
                        except Exception:
                            return True
                    if not os.path.exists(m101):
                        with open(m101, 'w', encoding='utf-8') as f:
                            f.write(vb_on)
                        created.append(m101)
                    elif _needs_fix(m101, 'ActivateSignal(Output1)'):
                        try:
                            os.replace(m101, m101 + '.bak')
                        except Exception:
                            pass
                        with open(m101, 'w', encoding='utf-8') as f:
                            f.write(vb_on)
                        created.append(m101)
                    if not os.path.exists(m102):
                        with open(m102, 'w', encoding='utf-8') as f:
                            f.write(vb_off)
                        created.append(m102)
                    elif _needs_fix(m102, 'DeactivateSignal(Output1)'):
                        try:
                            os.replace(m102, m102 + '.bak')
                        except Exception:
                            pass
                        with open(m102, 'w', encoding='utf-8') as f:
                            f.write(vb_off)
                        created.append(m102)
                except Exception:
                    pass
            if created:
                self.show_notification(
                    "Macros M101/M102 instalados para acionar OUTPUT1:\n" + "\n".join(created),
                    'success'
                )
        except Exception as e:
            self.show_notification(f"Erro ao instalar macros do Mach3: {e}", 'error')

    def _open_in_mach3_clicked(self):
        # Salva G-code no caminho padrão e tenta abrir no Mach3
        self._save_sash_positions()
        params = self._get_current_params()
        if params is None:
            self.show_notification("Por favor, verifique os parâmetros.", 'error')
            return
        try:
            full_gcode_list = self.gcode_generator.generate(params)
            if not full_gcode_list:
                self.show_notification("Erro ao gerar G-Code.", 'error')
                return
            full_gcode = "\n".join(full_gcode_list)
            base_dir = self.config.get('integration', {}).get('gcode_dir', r"C:\\Mach3\\GCode")
            try:
                os.makedirs(base_dir, exist_ok=True)
            except Exception:
                # Se não conseguir criar, usa a pasta do usuário como fallback
                base_dir = os.path.join(os.path.expanduser("~"), "Mach3", "GCode")
                os.makedirs(base_dir, exist_ok=True)
            filename = f"TFM_{datetime.now().strftime('%Y%m%d_%H%M%S')}.tap"
            filepath = os.path.join(base_dir, filename)
            try:
                with open(filepath, 'w', encoding='utf-8') as f:
                    f.write(full_gcode)
            except Exception as e:
                self.show_notification(f"Erro ao salvar G-Code para Mach3: {e}", 'error')
                return
            # Opcional: escrever arquivo autoload.txt para uso com macropump
            try:
                if self.config.get('integration', {}).get('write_autoload_txt', False):
                    with open(os.path.join(base_dir, 'autoload.txt'), 'w', encoding='utf-8') as auto:
                        auto.write(filepath)
            except Exception:
                pass
            # Tenta abrir o Mach3 com o arquivo como argumento
            configured = self.config.get('integration', {}).get('mach3_path')
            mach3_candidates = [configured] if configured else [r"C:\\Mach3\\Mach3.exe", r"C:\\Mach3\\Mach3Mill.exe"]
            mach3_path = next((p for p in mach3_candidates if p and os.path.exists(p)), None)
            if mach3_path:
                try:
                    # Antes de iniciar o Mach3, garantir macros M101/M102
                    self._ensure_mach3_macros()
                    mach3_dir = os.path.dirname(mach3_path)
                    profile = self.config.get('integration', {}).get('mach3_profile')
                    args = [mach3_path]
                    if profile:
                        args += ['/p', profile]
                    # Tenta abrir diretamente passando o arquivo G-code como argumento
                    args += [filepath]
                    subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, cwd=mach3_dir)
                    # Regrava autoload.txt com pequeno atraso para garantir leitura pelo macropump após inicialização
                    if self.config.get('integration', {}).get('write_autoload_txt', False):
                        def _rewrite_autoload():
                            try:
                                with open(os.path.join(base_dir, 'autoload.txt'), 'w', encoding='utf-8') as auto:
                                    auto.write(filepath)
                            except Exception:
                                pass
                        try:
                            # agenda em ~2s para dar tempo ao Mach3 subir o macropump
                            self.after(2000, _rewrite_autoload)
                        except Exception:
                            _rewrite_autoload()
                    # Fallback opcional: abrir arquivo via associação do Windows após o Mach3 subir
                    if self.config.get('integration', {}).get('allow_file_association_fallback', False):
                        def _startfile_later():
                            try:
                                os.startfile(filepath)
                            except Exception:
                                pass
                        try:
                            self.after(3000, _startfile_later)
                        except Exception:
                            _startfile_later()
                    self.show_notification(f"Mach3 iniciado (perfil: {profile or 'padrão'}). Abrindo arquivo:\n{filepath}", 'success')
                except Exception:
                    try:
                        os.startfile(mach3_path)
                        self.show_notification(f"G-Code salvo. Abra o arquivo no Mach3:\n{filepath}", 'warning')
                    except Exception:
                        self.show_notification(f"G-Code salvo. Não foi possível abrir o Mach3 automaticamente.\nArquivo: {filepath}", 'warning')
            else:
                # Se não encontrar o executável do Mach3, informa e abre o arquivo
                self.show_notification(f"G-Code salvo. Mach3 não encontrado.\nArquivo: {filepath}", 'warning')
                if self.config.get('integration', {}).get('allow_file_association_fallback', False):
                    try:
                        os.startfile(filepath)
                    except Exception:
                        webbrowser.open(f'file://{os.path.abspath(filepath)}')
        except Exception as e:
            self.show_notification(f"Erro ao preparar G-Code para Mach3: {e}", 'error')
        finally:
            self._restore_sash_positions()

    # --- Integração de Simuladores no Menu ---
    def _project_root(self):
        from pathlib import Path
        # Se estiver em modo frozen (PyInstaller), usa a pasta do executável
        return Path(sys.executable).resolve().parent if getattr(sys, 'frozen', False) else Path(__file__).resolve().parents[2]

    def _run_python_script(self, relative_path, input_text=None):
        try:
            base = self._project_root()
            script_path = base / relative_path
            if not script_path.exists():
                raise FileNotFoundError(f"Script não encontrado: {script_path}")
            # Executa de forma silenciosa (sem poluir terminal) e opcionalmente não interativo
            creationflags = subprocess.CREATE_NO_WINDOW if os.name == 'nt' and hasattr(subprocess, 'CREATE_NO_WINDOW') else 0
            if input_text is None:
                subprocess.run([sys.executable, str(script_path)], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=creationflags)
            else:
                proc = subprocess.Popen([sys.executable, str(script_path)], stdin=subprocess.PIPE, text=True,
                                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=creationflags)
                proc.communicate(input=input_text)
            self.show_notification("Simulação executada com sucesso.", 'success', 3000)
        except Exception as e:
            self.show_notification(f"Erro ao executar simulação: {e}", 'error', 6000)

    def _sim_run_demo(self):
        self._run_python_script('src/simulators/simulador_demo.py')

    def _sim_run_detailed(self):
        self._run_python_script('src/simulators/simulador_real.py')

    def _sim_run_final(self):
        # Usa opção '1' para G-code de exemplo por padrão
        self._run_python_script('src/simulators/simulador_final.py', input_text='1\n')

    def _sim_run_visual(self):
        # Seleciona opção padrão (1: exemplo) e velocidade 50
        self._run_python_script('src/simulators/simulador_visual.py', input_text='1\n50\n')

    def _open_visualizador_3d(self):
        # Abre o visualizador 3D navegável
        self._run_python_script('src/visualizers/visualizador_3d_navegacao_v2.py')

    def _open_simulations_png(self):
        try:
            base = self._project_root()
            arquivos = [
                base / 'simulacao_soldagem_demo.png',
                base / 'analise_gcode_detalhada.png',
                base / 'analise_gcode_completa.png'
            ]
            abertos = 0
            for arquivo in arquivos:
                if arquivo.exists():
                    try:
                        if os.name == 'nt':
                            os.startfile(str(arquivo))
                        else:
                            webbrowser.open(arquivo.as_uri())
                        abertos += 1
                    except Exception:
                        pass
            if abertos > 0:
                self.show_notification(f"{abertos} simulação(ões) aberta(s).", 'success', 3000)
            else:
                self.show_notification("Nenhuma simulação PNG encontrada.", 'warning', 3000)
        except Exception as e:
            self.show_notification(f"Erro ao abrir simulações: {e}", 'error', 6000)

if __name__ == "__main__":
    root = tk.Tk()
    is_frozen = getattr(sys, 'frozen', False)
    # Exibe uma Splash leve (Tk) tanto em dev quanto em executável
    try:
        root.withdraw()
    except Exception:
        pass
    # Resolve caminho do logo
    try:
        logo_path = resource_path('assets/logo.png')
    except Exception:
        logo_path = None
    # Exibe splash
    try:
        splash = SplashScreen(
            root,
            image_path=logo_path,
            message=None,
            bg_color="#ffffff",
            transparent_bg=True,
            transparent_color="#ffffff",
        )
        # Força pintura imediata para evitar atraso perceptível
        try:
            splash.update_idletasks()
            splash.update()
        except Exception:
            pass
    except Exception:
        splash = None
    # Fallback: oculta o splash caso algo impeça on_ready
    try:
        def _force_hide_splash():
            try:
                if splash is not None and splash.winfo_exists():
                    try:
                        splash.withdraw()
                    except Exception:
                        pass
                    splash.destroy()
                try:
                    root.deiconify()
                except Exception:
                    pass
            except Exception:
                pass
        root.after(5000, _force_hide_splash)
    except Exception:
        pass
    # Inicializa app e fecha Splash assim que a UI terminar de carregar
    def _on_ready():
        try:
            if splash is not None and splash.winfo_exists():
                try:
                    splash.withdraw()
                except Exception:
                    pass
                splash.destroy()
        except Exception:
            pass
        try:
            root.deiconify()
        except Exception:
            pass

    def _init_app():
        TFM_GCODE(root, on_ready=_on_ready)

    # Chama init imediatamente (Splash já foi pintada)
    _init_app()
    root.mainloop()
