# -*- coding: utf-8 -*-
# TFM G-Code Generator v11.2 - G-Code Adjust
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

# --- MÓDULO DE GESTÃO DE IDIOMAS ---
class LanguageManager:
    def __init__(self, lang_dir='lang', initial_lang='pt_br'):
        self.lang_dir = resource_path(lang_dir)
        self.languages = self._get_available_languages()
        self.data = {}
        self.set_language(initial_lang)

    def _get_available_languages(self):
        try:
            lang_files = []; base_dir = getattr(sys, '_MEIPASS', os.path.abspath("."))
            lang_path = os.path.join(base_dir, 'lang')
            if os.path.exists(lang_path): lang_files = os.listdir(lang_path)
            elif os.path.exists(self.lang_dir): lang_files = os.listdir(self.lang_dir)
            available = [f.replace('.json', '') for f in lang_files if f.endswith('.json')]
            if not available: return ['pt_br', 'en_us']
            return available
        except Exception as e: print(f"Erro ao listar idiomas: {e}"); return ['pt_br', 'en_us']

    def set_language(self, lang_code):
        filepath_bundle = None; filepath_local = os.path.join(self.lang_dir, f"{lang_code}.json")
        if getattr(sys, 'frozen', False) and hasattr(sys, '_MEIPASS'): filepath_bundle = os.path.join(sys._MEIPASS, 'lang', f"{lang_code}.json")
        try:
            if filepath_bundle and os.path.exists(filepath_bundle):
                with open(filepath_bundle, 'r', encoding='utf-8') as f: self.data = json.load(f)
            elif os.path.exists(filepath_local):
                 with open(filepath_local, 'r', encoding='utf-8') as f: self.data = json.load(f)
            else: raise FileNotFoundError
        except (FileNotFoundError, json.JSONDecodeError):
             self.data = {'error': f"Could not load language file: {lang_code}.json"}; print(self.data['error'])
             if lang_code != 'pt_br': print("Tentando carregar pt_br como fallback..."); self.set_language('pt_br')

    def get(self, key, default=None, **kwargs):
        value = self.data.get(key, default if default is not None else key)
        try: return value.format(**kwargs)
        except Exception: return value

# --- MÓDULO DE GERAÇÃO DE G-CODE ---
class GCodeGenerator:
    def generate(self, params, is_dry_run=False):
        if params is None: return None
        mode = params.get('welding_mode', 'espiral')
        if mode == 'espiral':
            return self._generate_spiral(params, is_dry_run)
        elif mode == 'oscilacao_linear':
            return self._generate_linear_oscillation(params, is_dry_run)
        return None

    def _build_header(self, params, is_dry_run):
        mode = params.get('welding_mode', 'espiral')
        header = ["%", f"(G-CODE GERADO PELO TFM G-CODE)", f"(DRY RUN MODE: {'YES' if is_dry_run else 'NO'})", f"(Data: {datetime.now().strftime('%d/%m/%Y %H:%M:%S')})", "(----------------------------------------)", f"( PROCEDIMENTO: {params.get('nome_procedimento', 'N/A')} )", f"( MODO DE SOLDAGEM: {mode.upper()} )"]
        header.extend([f"( DIAMETRO: {params['diametro']:.3f} mm)", f"( COMPRIMENTO X: {params['comprimento_revestir']:.3f} mm )", f"( SENTIDO: {params.get('direcao_soldagem', 'esquerda_direita')} )"])
        header.extend([f"( CAMADAS: {params['num_camadas']} de {params['espessura_camada']:.2f}mm )"])
        if mode == 'oscilacao_linear':
             # --- ALTERADO: Simplifica comentário ---
             header.extend([f"( OSCILACAO: ATIVADA )", f"(   - COMPRIMENTO OSC.: {params.get('oscilacao_comprimento', 0.0):.2f} mm)", f"(   - DESLOC. ANGULAR: {params.get('deslocamento_angular_perc', 0.0):.1f}% Larg. Cordao)"])
             # --- FIM ALTERAÇÃO ---
        header.extend([f"( LEAD-IN: {params.get('lead_in', 0.0):.2f} mm, LEAD-OUT: {params.get('lead_out', 0.0):.2f} mm )"])
        header.extend(["(----------------------------------------)", "G21 G90 (MM, ABSOLUTO)", ""])
        return header

    def _build_footer(self):
        return ["", "G00 Z0", "M30 (FIM DO PROGRAMA)", "%"] # A0 já havia sido removido

    def _generate_spiral(self, params, is_dry_run):
        gcode_body = ["G94 (FEED/MIN)"]
        current_d = params['diametro']
        for i in range(params['num_camadas']):
            gcode_body.extend(self._build_spiral_segment(params, i, current_d, is_dry_run))
            current_d += 2 * params['espessura_camada']
        return self._build_header(params, is_dry_run) + gcode_body + self._build_footer()

    def _build_spiral_segment(self, params, layer_num, current_d, is_dry_run):
        afastamento = params['afastamento_tocha']; z_offset = 20.0 if is_dry_run else 0.0
        z_layer = current_d / 2.0 + afastamento + z_offset
        z_seguranca = z_layer + 30.0
        part_length = params['comprimento_revestir']; direction = params.get('direcao_soldagem', 'esquerda_direita')
        lead_in = params.get('lead_in', 0.0); lead_out = params.get('lead_out', 0.0)
        if direction == 'esquerda_direita': x_arc_start_calc = 0.0 - lead_in; x_arc_end_calc = part_length + lead_out; angle_sign = 1.0
        else: x_arc_start_calc = part_length + lead_out; x_arc_end_calc = 0.0 - lead_in; angle_sign = -1.0
        comprimento_total_arc = abs(x_arc_end_calc - x_arc_start_calc)
        passo = params['largura_cordao'] * (1.0 - (params['sobreposicao'] / 100.0)); passo = max(passo, 1e-6)
        total_rotacoes = (comprimento_total_arc / passo); angulo_total_A = total_rotacoes * 360.0 * angle_sign
        m8 = "(M8)" if is_dry_run else "M8"; m9 = "(M9)" if is_dry_run else "M9"
        segment_gcode = [f"(--- CAMADA {layer_num + 1} ---)", f"(SENTIDO: {direction})", f"G00 Z{z_seguranca:.3f}",
                         # --- ALTERADO: Remove A0 daqui ---
                         f"G00 X{x_arc_start_calc:.3f}",
                         # --- FIM ALTERAÇÃO ---
                         f"G01 Z{z_layer:.3f} F{params['velocidade_soldagem']*2}", m8, "G04 P1.5"]
        segment_gcode.append(f"G01 X{x_arc_end_calc:.3f} A{angulo_total_A:.3f} F{params['velocidade_soldagem']:.1f}")
        segment_gcode.extend([m9, f"G00 Z{z_seguranca:.3f}", "M01" if layer_num < params['num_camadas']-1 else ""])
        return segment_gcode

    def _generate_linear_oscillation(self, params, is_dry_run):
        gcode_body = ["G94 F{:.1f} (FEED/MIN INICIAL)".format(params['velocidade_soldagem'])]
        current_d = params['diametro']
        for i in range(params['num_camadas']):
            gcode_body.extend(self._build_linear_oscillation_segment(params, i, current_d, is_dry_run))
            current_d += 2 * params['espessura_camada']
        return self._build_header(params, is_dry_run) + gcode_body + self._build_footer()

    def _build_linear_oscillation_segment(self, params, layer_num, current_d, is_dry_run):
        afastamento = params['afastamento_tocha']; z_offset = 20.0 if is_dry_run else 0.0
        z_layer = current_d / 2.0 + afastamento + z_offset
        z_seguranca = z_layer + 30.0
        part_length = params['comprimento_revestir']
        direction = params.get('direcao_soldagem', 'esquerda_direita')
        lead_in = params.get('lead_in', 0.0); lead_out = params.get('lead_out', 0.0)
        velocidade_soldagem = params['velocidade_soldagem']
        osc_length = params['oscilacao_comprimento']
        larg_cordao = params['largura_cordao']
        desloc_perc = params['deslocamento_angular_perc']
        sobreposicao = params['sobreposicao']

        if direction == 'esquerda_direita': x_start_revest = 0.0 - lead_in; x_end_revest = part_length + lead_out
        else: x_start_revest = part_length + lead_out; x_end_revest = 0.0 - lead_in
        actual_passo_axial = larg_cordao * (1.0 - sobreposicao / 100.0); actual_passo_axial = max(actual_passo_axial, 1e-6)
        num_passos_axiais = math.ceil(part_length / actual_passo_axial) if part_length > 0 else 1
        circunferencia = math.pi * current_d
        desloc_linear_angular = (desloc_perc / 100.0) * larg_cordao
        if circunferencia <= 0 or desloc_linear_angular <= 0: return ["(ERRO: Diametro ou deslocamento invalido para calculo angular)"]
        num_passos_angulares_por_volta = math.ceil(circunferencia / desloc_linear_angular)
        actual_delta_A_deg = 360.0 / num_passos_angulares_por_volta
        feed_angular = (velocidade_soldagem / circunferencia) * 360.0 if circunferencia > 0 else 100
        m8 = "(M8)" if is_dry_run else "M8"; m9 = "(M9)" if is_dry_run else "M9"
        segment_gcode = [f"(--- CAMADA {layer_num + 1} ---)", f"(SENTIDO: {direction})", f"G00 Z{z_seguranca:.3f}"]

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
            # --- ALTERADO: Remove A0 daqui ---
            segment_gcode.append(f"G00 X{current_x_start_pos:.3f}")
            # --- FIM ALTERAÇÃO ---
            segment_gcode.append(f"G01 Z{z_layer:.3f} F{velocidade_soldagem*2}")
            if passo_idx == 0:
                 segment_gcode.append(m8); segment_gcode.append("G04 P1.5")
                 if abs(lead_in) > 1e-6: segment_gcode.append(f"G01 X{x_osc_volta:.3f} F{velocidade_soldagem:.1f}")
            current_A = 0.0
            for k in range(num_passos_angulares_por_volta):
                current_x_osc_ida = x_osc_ida
                current_x_osc_volta = x_osc_volta

                segment_gcode.append(f"(Passo angular {k+1}/{num_passos_angulares_por_volta})")
                segment_gcode.append(f"G01 X{current_x_osc_ida:.3f} F{velocidade_soldagem:.1f}")
                segment_gcode.append(f"G01 X{current_x_osc_volta:.3f} F{velocidade_soldagem:.1f}")

                current_A += actual_delta_A_deg
                current_A_mod = current_A % 360 if current_A >= 360 else current_A

                segment_gcode.append(f"G93 G01 A{current_A_mod:.3f} F{feed_angular:.1f}") 
                segment_gcode.append(f"G94 F{velocidade_soldagem:.1f}") 

            # Movimento para o início do PRÓXIMO passo axial (se não for o último)
            if passo_idx < num_passos_axiais - 1:
                 next_x_start_anel = 0.0 + (passo_idx + 1) * actual_passo_axial if direction == 'esquerda_direita' else part_length - (passo_idx + 1) * actual_passo_axial
                 next_x_start_anel = max(0.0, min(part_length, next_x_start_anel))
                 segment_gcode.append(f"G00 X{next_x_start_anel:.3f}")

            # Ao final da última volta (último passo axial), move para a posição final com lead-out
            if passo_idx == num_passos_axiais - 1 and abs(lead_out) > 1e-6:
                  segment_gcode.append(f"(Movimento lead-out)")
                  segment_gcode.append(f"G01 X{current_x_end_pos_final_volta:.3f} F{velocidade_soldagem:.1f}")

        segment_gcode.extend([m9, f"G00 Z{z_seguranca:.3f}", "M01" if layer_num < params['num_camadas']-1 else ""])
        return segment_gcode

# --- JANELA DE CONFIGURAÇÕES ---
class SettingsWindow(Toplevel):
    def __init__(self, parent_app: 'TFM_GCODE', lang, config, settings_type):
        super().__init__(parent_app.root)
        self.parent_app = parent_app; self.lang = lang; self.local_config = config; self.settings_type = settings_type
        self.title(lang.get(settings_type)); self.transient(parent_app.root); self.grab_set(); self.vars = {}
        frame = ttk.Frame(self, padding="10"); frame.pack(expand=True, fill="both")
        fields = ['max_length_x', 'max_rpm_chuck'] if self.settings_type == 'machine_limits' else ['powder_brl_kg', 'gas_argon_brl_m3', 'labor_brl_hour', 'machine_brl_hour']
        for i, field in enumerate(fields): ttk.Label(frame, text=f"{field.replace('_', ' ').capitalize()}:").grid(row=i, column=0, sticky='w', pady=2); self.vars[field] = tk.StringVar(value=self.local_config[self.settings_type].get(field, '0.0')); ttk.Entry(frame, textvariable=self.vars[field]).grid(row=i, column=1, sticky='ew', padx=5)
        btn_frame = ttk.Frame(self); btn_frame.pack(fill='x', padx=10, pady=5); ttk.Button(btn_frame, text=self.lang.get('save'), command=self.save_and_close).pack(side='right'); ttk.Button(btn_frame, text=self.lang.get('cancel'), command=self.destroy).pack(side='right', padx=5)

    def save_and_close(self):
        for key, var in self.vars.items():
            try: self.local_config[self.settings_type][key] = float(var.get())
            except ValueError: self.parent_app.show_notification(self.lang.get('check_parameters'), 'warning'); return
        self.parent_app.save_config(); self.destroy(); self.parent_app.trigger_update()

# --- CLASSE PRINCIPAL ---
class TFM_GCODE:
    def __init__(self, root):
        self.root = root
        self.lang = LanguageManager()
        self.gcode_generator = GCodeGenerator()
        self.config = self.load_config()
        self.anim = None
        self._update_job_id = None
        self._debounce_delay = 300
        self._notification_job_id = None
        self.root.state('zoomed')
        self._create_widgets()
        self._setup_styles()
        self._update_ui_text()
        self.root.after(100, self.trigger_update)

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
        self._update_job_id = None; self.executar_calculos_e_desenho(); self._update_gcode_preview()

    def load_config(self, filepath='config.json'):
        default_config = {"costs": { "powder_brl_kg": 250.0, "gas_argon_brl_m3": 50.0, "labor_brl_hour": 40.0, "machine_brl_hour": 30.0, "currency_symbol": "R$" }, "machine_limits": { "max_length_x": 1500.0, "max_rpm_chuck": 100.0 }, "database": { "procedures_path": "procedures" }}
        config = copy.deepcopy(default_config)
        try:
            with open(filepath, 'r', encoding='utf-8') as f: user_config = json.load(f)
            for key in default_config:
                if key in user_config:
                    if isinstance(config[key], dict) and isinstance(user_config.get(key), dict): config[key].update(user_config[key])
                    else: config[key] = user_config[key]
        except (FileNotFoundError, json.JSONDecodeError): pass
        self.save_config(filepath, config); return config

    def save_config(self, filepath='config.json', config_data=None):
        data_to_save = config_data if config_data is not None else self.config
        try:
            dir_name = os.path.dirname(filepath);
            if dir_name and not os.path.exists(dir_name): os.makedirs(dir_name)
            proc_path = data_to_save.get('database', {}).get('procedures_path', 'procedures')
            if not os.path.exists(proc_path): os.makedirs(proc_path)
            with open(filepath, 'w', encoding='utf-8') as f: json.dump(data_to_save, f, indent=4)
        except Exception as e: self.show_notification(f"Could not save config: {e}", 'error')

    def _create_menu(self):
        self.menubar = tk.Menu(self.root); self.root.config(menu=self.menubar)
        self.file_menu = tk.Menu(self.menubar, tearoff=0); self.settings_menu = tk.Menu(self.menubar, tearoff=0)
        self.language_menu = tk.Menu(self.menubar, tearoff=0); self.help_menu = tk.Menu(self.menubar, tearoff=0)

    def _create_widgets(self):
        self._create_menu(); main_frame = ttk.Frame(self.root, padding="10"); main_frame.grid(row=0, column=0, sticky="nsew")
        self.root.columnconfigure(0, weight=1); self.root.rowconfigure(0, weight=1); self.root.rowconfigure(1, weight=0)
        main_paned_window = PanedWindow(main_frame, orient=tk.HORIZONTAL, sashrelief=tk.RAISED, sashwidth=6); main_paned_window.grid(row=0, column=0, sticky="nsew")
        main_frame.columnconfigure(0, weight=1); main_frame.rowconfigure(0, weight=1)
        controls_outer_frame = ttk.Frame(main_paned_window, width=350); main_paned_window.add(controls_outer_frame, stretch="never")
        view_pane = PanedWindow(main_paned_window, orient=tk.VERTICAL, sashrelief=tk.RAISED, sashwidth=6); main_paned_window.add(view_pane, stretch="always")
        top_view_pane = PanedWindow(view_pane, orient=tk.HORIZONTAL, sashrelief=tk.RAISED, sashwidth=6); view_pane.add(top_view_pane, stretch="always")
        self._sash_configured = False
        def configure_panes(event=None):
             if not self._sash_configured and main_paned_window.winfo_width() > 1 and view_pane.winfo_height() > 1 and top_view_pane.winfo_width() > 1:
                 try:
                     main_pane_width = main_paned_window.winfo_width(); controls_width = int(main_pane_width * 0.25); main_paned_window.sash_place(0, controls_width, 0)
                     view_pane_height = view_pane.winfo_height(); result_rows = 5; cost_rows = 5; padding = 20; line_height_estimate = 25
                     bottom_height = (result_rows + cost_rows) * line_height_estimate + padding * 3; bottom_height = min(bottom_height, int(view_pane_height * 0.4))
                     view_pane.sash_place(0, view_pane_height - bottom_height, 0)
                     top_view_pane_width = top_view_pane.winfo_width(); gcode_width = int(top_view_pane_width * 0.30); top_view_pane.sash_place(0, top_view_pane_width - gcode_width, 0)
                     self._sash_configured = True
                 except tk.TclError: self.root.after(200, configure_panes)
                 except Exception as e: print(f"Erro ao configurar sashes: {e}")
        main_paned_window.bind("<Configure>", configure_panes, add='+')
        scrolled_frame = ScrolledFrame(controls_outer_frame); scrolled_frame.pack(fill="both", expand=True)
        interior = scrolled_frame.interior

        # Notebook para modos de soldagem
        self.notebook = ttk.Notebook(interior); self.notebook.pack(fill="x", expand=True, padx=5, pady=5)
        self.notebook.bind("<<NotebookTabChanged>>", self.trigger_update)
        self.tab_espiral = ttk.Frame(self.notebook, padding="10"); self.tab_osc_linear = ttk.Frame(self.notebook, padding="10")
        self.notebook.add(self.tab_espiral, text="..."); self.notebook.add(self.tab_osc_linear, text="...")

        self.params = {} # Inicializa params aqui
        # Parâmetros comuns (fora do notebook)
        self.params['comprimento'] = tk.StringVar(value="200.0"); self.params['largura_cordao'] = tk.StringVar(value="8.0"); self.params['sobreposicao'] = tk.StringVar(value="40.0")
        self.params['velocidade'] = tk.StringVar(value="120.0"); self.params['afastamento'] = tk.StringVar(value="12.0"); self.params['lead_in'] = tk.StringVar(value="5.0"); self.params['lead_out'] = tk.StringVar(value="5.0")
        self.params['direcao_soldagem'] = tk.StringVar(value='esquerda_direita')
        self.params['nome_procedimento'] = tk.StringVar(value="Novo Procedimento"); self.params['corrente_arco'] = tk.StringVar(value="180.0"); self.params['vazao_gas'] = tk.StringVar(value="15.0")
        self.params['alim_po'] = tk.StringVar(value="50.0"); self.params['preaquecimento'] = tk.StringVar(value="100.0"); self.params['fator_peso'] = tk.StringVar(value="0.16")
        self.params['num_camadas'] = tk.StringVar(value="1"); self.params['espessura_camada'] = tk.StringVar(value="1.5")
        # Parâmetro diâmetro (usado em ambas as abas)
        self.params['diametro'] = tk.StringVar(value="50.0")
        # Parâmetros específicos Oscilação Linear
        self.params['oscilacao_comprimento'] = tk.StringVar(value="10.0")
        self.params['deslocamento_angular_perc'] = tk.StringVar(value="50.0")

        # --- Widgets ABA ESPIRAL ---
        self.espiral_diametro_label = ttk.Label(self.tab_espiral, text="..."); self.espiral_diametro_label.grid(row=0, column=0, sticky="w", pady=3)
        ttk.Entry(self.tab_espiral, textvariable=self.params['diametro']).grid(row=0, column=1, sticky="ew")

        # --- Widgets ABA OSCILAÇÃO LINEAR ---
        self.osc_diametro_label = ttk.Label(self.tab_osc_linear, text="..."); self.osc_diametro_label.grid(row=0, column=0, sticky="w", pady=3)
        ttk.Entry(self.tab_osc_linear, textvariable=self.params['diametro']).grid(row=0, column=1, sticky="ew")
        self.osc_comp_label = ttk.Label(self.tab_osc_linear, text="..."); self.osc_comp_label.grid(row=1, column=0, sticky="w", pady=3)
        ttk.Entry(self.tab_osc_linear, textvariable=self.params['oscilacao_comprimento']).grid(row=1, column=1, sticky="ew")
        self.osc_desloc_label = ttk.Label(self.tab_osc_linear, text="..."); self.osc_desloc_label.grid(row=2, column=0, sticky="w", pady=3)
        ttk.Entry(self.tab_osc_linear, textvariable=self.params['deslocamento_angular_perc']).grid(row=2, column=1, sticky="ew")

        # --- FRAMES COMUNS (fora do notebook) ---
        self.common_params_frame = ttk.LabelFrame(interior, text="...", padding="10"); self.common_params_frame.pack(fill="x", pady=(5,0), expand=True, padx=5)
        self.common_labels = {}; labels_comuns = ["length_to_coat", "weld_bead_width", "overlap", "welding_speed", "torch_standoff", "lead_in", "lead_out"]
        vars_comuns = ['comprimento', 'largura_cordao', 'sobreposicao', 'velocidade', 'afastamento', 'lead_in', 'lead_out']; row_idx = 0
        for i, key in enumerate(labels_comuns): self.common_labels[key] = ttk.Label(self.common_params_frame, text="..."); self.common_labels[key].grid(row=row_idx, column=0, sticky="w", pady=3); ttk.Entry(self.common_params_frame, textvariable=self.params[vars_comuns[i]]).grid(row=row_idx, column=1, sticky="ew"); row_idx += 1
        self.direction_label = ttk.Label(self.common_params_frame, text="..."); self.direction_label.grid(row=row_idx, column=0, sticky="w", pady=3)
        self.direction_combo = ttk.Combobox(self.common_params_frame, textvariable=self.params['direcao_soldagem'], values=['esquerda_direita', 'direita_esquerda'], state='readonly'); self.direction_combo.grid(row=row_idx, column=1, sticky="ew"); row_idx += 1
        self.process_params_frame = ttk.LabelFrame(interior, text="...", padding="10"); self.process_params_frame.pack(fill="x", pady=(5,0), expand=True, padx=5)
        self.process_labels = {}; labels_processo = ["procedure_name", "arc_current", "gas_flow", "powder_feed_rate", "preheat_temp", "weight_factor", "notes"]
        vars_processo = ['nome_procedimento', 'corrente_arco', 'vazao_gas', 'alim_po', 'preaquecimento', 'fator_peso']; row_idx = 0
        for i, key in enumerate(labels_processo[:6]): self.process_labels[key] = ttk.Label(self.process_params_frame, text="..."); self.process_labels[key].grid(row=row_idx, column=0, sticky="w", pady=3); ttk.Entry(self.process_params_frame, textvariable=self.params[vars_processo[i]]).grid(row=row_idx, column=1, sticky="ew"); row_idx += 1
        self.process_labels['notes'] = ttk.Label(self.process_params_frame, text="..."); self.process_labels['notes'].grid(row=row_idx, column=0, sticky="nw", pady=3)
        self.notes_text = Text(self.process_params_frame, height=3, width=20); self.notes_text.grid(row=row_idx, column=1, sticky="ew"); self.notes_text.bind("<KeyRelease>", self.trigger_update)
        self.layer_params_frame = ttk.LabelFrame(interior, text="...", padding="10"); self.layer_params_frame.pack(fill="x", pady=(5,0), expand=True, padx=5)
        self.num_layers_label = ttk.Label(self.layer_params_frame, text="..."); self.num_layers_label.grid(row=0, column=0, sticky="w", pady=3); ttk.Entry(self.layer_params_frame, textvariable=self.params['num_camadas']).grid(row=0, column=1, sticky="ew")
        self.thickness_per_layer_label = ttk.Label(self.layer_params_frame, text="..."); self.thickness_per_layer_label.grid(row=1, column=0, sticky="w", pady=3); ttk.Entry(self.layer_params_frame, textvariable=self.params['espessura_camada']).grid(row=1, column=1, sticky="ew")

        # Resultados e Custos (no rodapé da área de visualização)
        bottom_results_outer_frame = ttk.Frame(view_pane, padding=(0, 10, 0, 0)); view_pane.add(bottom_results_outer_frame, stretch="never")
        bottom_results_outer_frame.columnconfigure(0, weight=1); bottom_results_outer_frame.columnconfigure(1, weight=1)
        self.result_frame = ttk.LabelFrame(bottom_results_outer_frame, text="...", padding="10"); self.result_frame.grid(row=0, column=0, sticky="nsew", padx=(0, 5))
        self.resultados = { 'rpm': tk.StringVar(), 'passo': tk.StringVar(), 'rotacoes': tk.StringVar(), 'angulo': tk.StringVar(), 'tempo': tk.StringVar() }; self.result_labels = {}
        labels_resultados = ["rotation_rpm", "helix_pitch", "total_rotations", "total_angle_A", "estimated_time"];
        for i, key in enumerate(labels_resultados): self.result_labels[key] = ttk.Label(self.result_frame, text="..."); self.result_labels[key].grid(row=i, column=0, sticky="w", pady=1); ttk.Label(self.result_frame, textvariable=self.resultados[list(self.resultados.keys())[i]], font=('Courier', 9, 'bold'), foreground='blue').grid(row=i, column=1, sticky="w", padx=5)
        self.cost_frame = ttk.LabelFrame(bottom_results_outer_frame, text="...", padding="10"); self.cost_frame.grid(row=0, column=1, sticky="nsew", padx=(5, 0))
        self.custos = {'consumiveis': tk.StringVar(), 'operacional': tk.StringVar(), 'total': tk.StringVar(), 'po': tk.StringVar(), 'gas': tk.StringVar()}; self.cost_labels = {}
        labels_custos = ["consumables_cost", "operational_cost", "total_cost", "powder_consumption", "gas_consumption"]
        for i, key in enumerate(labels_custos): self.cost_labels[key] = ttk.Label(self.cost_frame, text="..."); self.cost_labels[key].grid(row=i, column=0, sticky="w", pady=1); font = ('Courier', 9, 'bold') if key == "total_cost" else ('Courier', 9); color = 'darkgreen' if key == "total_cost" else 'black'; ttk.Label(self.cost_frame, textvariable=self.custos[list(self.custos.keys())[i]], font=font, foreground=color).grid(row=i, column=1, sticky="w", padx=5)

        # Botões (fora do scrolled_frame)
        btn_frame = ttk.Frame(controls_outer_frame, padding=(5, 10)); btn_frame.pack(fill='x', side='bottom'); btn_frame.columnconfigure(0, weight=1); btn_frame.columnconfigure(1, weight=1)
        self.generate_button = ttk.Button(btn_frame, text="...", command=lambda: self._generate_gcode_clicked(is_dry_run=False)); self.generate_button.grid(row=0, column=0, sticky="ew", padx=(0, 3), ipady=4)
        self.dry_run_button = ttk.Button(btn_frame, text="...", command=lambda: self._generate_gcode_clicked(is_dry_run=True), style="DryRun.TButton"); self.dry_run_button.grid(row=0, column=1, sticky="ew", padx=(3, 0), ipady=4)

        # Visualizadores (dentro do top_view_pane)
        self.visualizer_frame = ttk.LabelFrame(top_view_pane, text="...", padding=5); top_view_pane.add(self.visualizer_frame, stretch="always")
        self.fig = Figure(figsize=(7, 5), dpi=100); self.ax = self.fig.add_subplot(111, projection='3d'); self.canvas = FigureCanvasTkAgg(self.fig, master=self.visualizer_frame)
        self.canvas_widget = self.canvas.get_tk_widget(); self.canvas_widget.pack(side=tk.TOP, fill=tk.BOTH, expand=True)
        self.toolbar_frame = ttk.Frame(self.visualizer_frame); self.toolbar_frame.pack(side=tk.TOP, fill=tk.X)
        self.toolbar = NavigationToolbar2Tk(self.canvas, self.toolbar_frame); self.toolbar.update()
        self.gcode_viewer_frame = ttk.LabelFrame(top_view_pane, text="...", padding=5); top_view_pane.add(self.gcode_viewer_frame, stretch="never")
        gcode_text_frame = ttk.Frame(self.gcode_viewer_frame); gcode_text_frame.pack(fill=tk.BOTH, expand=True); gcode_text_frame.rowconfigure(0, weight=1); gcode_text_frame.columnconfigure(0, weight=1)
        gcode_vscroll = ttk.Scrollbar(gcode_text_frame, orient=tk.VERTICAL); gcode_hscroll = ttk.Scrollbar(gcode_text_frame, orient=tk.HORIZONTAL)
        self.gcode_text = Text(gcode_text_frame, wrap=tk.NONE, yscrollcommand=gcode_vscroll.set, xscrollcommand=gcode_hscroll.set, font=("Courier New", 9), state='disabled', width=40)
        gcode_vscroll.config(command=self.gcode_text.yview); gcode_hscroll.config(command=self.gcode_text.xview)
        gcode_vscroll.grid(row=0, column=1, sticky='ns'); gcode_hscroll.grid(row=1, column=0, sticky='ew', columnspan=2); self.gcode_text.grid(row=0, column=0, sticky='nsew')
        self.notification_frame = ttk.Frame(main_frame, style='Notification.TFrame'); self.notification_label = ttk.Label(self.notification_frame, text="", anchor='w', padding=(10, 5))
        self.notification_label.pack(side=tk.LEFT, fill=tk.X, expand=True); close_button = ttk.Button(self.notification_frame, text="X", command=self.hide_notification, style='Close.TButton', width=2)
        close_button.pack(side=tk.RIGHT, padx=(0, 5)); self.notification_frame.grid(row=1, column=0, sticky='ew', pady=(5, 0)); self.notification_frame.grid_remove()

        # Adiciona traces
        for var_name, var in self.params.items():
            if isinstance(var, (tk.StringVar, tk.BooleanVar)):
                if var_name not in []: var.trace_add("write", self.trigger_update)

    def _setup_styles(self):
        s = ttk.Style()
        s.configure("TButton", font=('Segoe UI', 10, 'bold'), padding=(10, 6)); s.configure("TButton", background="#107C10", foreground="black")
        s.map("TButton", background=[('active', '#0b5a0b'), ('disabled', '#5f5f5f')], foreground=[('disabled', '#bfbfbf')])
        s.configure("Accent.TButton", font=('Segoe UI', 10, 'bold'), padding=(10, 6), background="#0078D7", foreground="black")
        s.map("Accent.TButton", background=[('active', '#005a9e')])
        s.configure("DryRun.TButton", font=('Segoe UI', 10, 'bold'), padding=(10, 6), background="#FFC300", foreground="black")
        s.map("DryRun.TButton", background=[('active', '#d8a600')])
        s.configure('Notification.TFrame', background='lightgrey')
        s.configure('Info.TLabel', background='#d1ecf1', foreground='#0c5460', font=('Segoe UI', 9))
        s.configure('Success.TLabel', background='#d4edda', foreground='#155724', font=('Segoe UI', 9))
        s.configure('Warning.TLabel', background='#fff3cd', foreground='#856404', font=('Segoe UI', 9))
        s.configure('Error.TLabel', background='#f8d7da', foreground='#721c24', font=('Segoe UI', 9, 'bold'))
        s.configure('Close.TButton', padding=(2,0), font=('Segoe UI', 8))

    # --- REMOVIDO: _toggle_conical e _toggle_oscillation_widgets ---

    def _update_ui_text(self):
        # --- ALTERADO: Usa o novo nome do App ---
        self.root.title(self.lang.get('app_title', default="TFM G-Code Generator"))
        # --- FIM ALTERAÇÃO ---
        for menu in [self.file_menu, self.settings_menu, self.language_menu, self.help_menu]: menu.delete(0, 'end')
        self.menubar.delete(0, 'end')
        self.menubar.add_cascade(label=self.lang.get('file_menu'), menu=self.file_menu)
        self.file_menu.add_command(label=self.lang.get('load_procedure'), command=self._load_procedure); self.file_menu.add_command(label=self.lang.get('save_procedure'), command=self._save_procedure)
        self.file_menu.add_separator(); self.file_menu.add_command(label=self.lang.get('generate_report'), command=self._generate_report, state="normal" if REPORTLAB_AVAILABLE else "disabled")
        self.file_menu.add_separator(); self.file_menu.add_command(label=self.lang.get('exit'), command=self.root.quit)
        self.menubar.add_cascade(label=self.lang.get('settings_menu'), menu=self.settings_menu)
        self.settings_menu.add_command(label=self.lang.get('machine_limits'), command=lambda: SettingsWindow(self, self.lang, self.config, 'machine_limits')); self.settings_menu.add_command(label=self.lang.get('costs'), command=lambda: SettingsWindow(self, self.lang, self.config, 'costs'))
        self.menubar.add_cascade(label=self.lang.get('language_menu'), menu=self.language_menu)
        for lang_code in self.lang.languages: self.language_menu.add_command(label=lang_code.upper(), command=lambda l=lang_code: self._change_language(l))
        self.menubar.add_cascade(label=self.lang.get('help_menu'), menu=self.help_menu); self.help_menu.add_command(label=self.lang.get('about'), command=self._show_about_dialog)

        # Atualiza nomes das abas
        self.notebook.tab(0, text=self.lang.get('spiral_mode', default="Espiral"))
        self.notebook.tab(1, text=self.lang.get('linear_oscillation_mode', default="Oscilação Linear"))

        # Atualiza labels das abas
        self.espiral_diametro_label.config(text=self.lang.get('diameter', default="Diâmetro (mm):"))
        self.osc_diametro_label.config(text=self.lang.get('diameter', default="Diâmetro (mm):"))
        self.osc_comp_label.config(text=self.lang.get('oscillation_length', default="Comprimento Osc. (mm):"))
        self.osc_desloc_label.config(text=self.lang.get('angular_displacement_perc', default="Desloc. Angular (%):"))


        self.common_params_frame.config(text=self.lang.get('general_params'))
        for key, label in self.common_labels.items(): label.config(text=self.lang.get(key, default=f'{key.replace("_", " ").capitalize()}:'))
        self.direction_label.config(text=self.lang.get('welding_direction', default="Sentido da Soldagem:"))
        direction_options = {'esquerda_direita': self.lang.get('left_to_right', default='Esquerda -> Direita'), 'direita_esquerda': self.lang.get('right_to_left', default='Direita -> Esquerda')}
        self.direction_combo['values'] = list(direction_options.values())
        current_internal_value = self.params['direcao_soldagem'].get(); display_value = direction_options.get(current_internal_value, direction_options['esquerda_direita'])
        self.direction_combo.set(display_value)

        # --- REMOVIDO: Atualização de texto da oscilação daqui ---

        self.process_params_frame.config(text=self.lang.get('process_params'))
        for key, label in self.process_labels.items(): label.config(text=self.lang.get(key))
        self.layer_params_frame.config(text=self.lang.get('layer_params'))
        self.num_layers_label.config(text=self.lang.get('num_layers')); self.thickness_per_layer_label.config(text=self.lang.get('thickness_per_layer'))
        self.result_frame.config(text=self.lang.get('calculated_results'))
        for key, label in self.result_labels.items(): label.config(text=self.lang.get(key))
        self.cost_frame.config(text=self.lang.get('cost_estimation'))
        for key, label in self.cost_labels.items(): label.config(text=self.lang.get(key))
        self.generate_button.config(text=self.lang.get('generate_gcode_button')); self.dry_run_button.config(text=self.lang.get('generate_dry_run_button'))
        self.visualizer_frame.config(text=self.lang.get('visualizer_title')); self.gcode_viewer_frame.config(text=self.lang.get('gcode_viewer_title', default="Visualizador G-Code"))

    def _change_language(self, lang_code):
        self.lang.set_language(lang_code); self._update_ui_text(); self.trigger_update()

    def _get_current_params(self):
        data = {}; data['app_title'] = self.lang.get('app_title'); data['nome_procedimento'] = self.params['nome_procedimento'].get()
        try:
            # Parâmetros comuns
            data['diametro'] = float(self.params['diametro'].get()) if self.params['diametro'].get() else 0.0
            data['comprimento_revestir'] = float(self.params['comprimento'].get()) if self.params['comprimento'].get() else 0.0
            data['largura_cordao'] = float(self.params['largura_cordao'].get()) if self.params['largura_cordao'].get() else 0.0
            data['sobreposicao'] = float(self.params['sobreposicao'].get()) if self.params['sobreposicao'].get() else 0.0
            data['velocidade_soldagem'] = float(self.params['velocidade'].get()) if self.params['velocidade'].get() else 0.0
            data['afastamento_tocha'] = float(self.params['afastamento'].get()) if self.params['afastamento'].get() else 0.0
            data['lead_in'] = float(self.params['lead_in'].get()) if self.params['lead_in'].get() else 0.0
            data['lead_out'] = float(self.params['lead_out'].get()) if self.params['lead_out'].get() else 0.0
            display_direction = self.params['direcao_soldagem'].get(); direction_map_reverse = {self.lang.get('left_to_right', default='Esquerda -> Direita'): 'esquerda_direita', self.lang.get('right_to_left', default='Direita -> Esquerda'): 'direita_esquerda'}
            data['direcao_soldagem'] = direction_map_reverse.get(display_direction, 'esquerda_direita')
            data['corrente_arco'] = float(self.params['corrente_arco'].get()) if self.params['corrente_arco'].get() else 0.0; data['vazao_gas'] = float(self.params['vazao_gas'].get()) if self.params['vazao_gas'].get() else 0.0
            data['alim_po'] = float(self.params['alim_po'].get()) if self.params['alim_po'].get() else 0.0; data['preaquecimento'] = float(self.params['preaquecimento'].get()) if self.params['preaquecimento'].get() else 0.0
            data['num_camadas'] = int(self.params['num_camadas'].get()) if self.params['num_camadas'].get() else 1; data['espessura_camada'] = float(self.params['espessura_camada'].get()) if self.params['espessura_camada'].get() else 0.0
            data['fator_peso'] = float(self.params['fator_peso'].get()) if self.params['fator_peso'].get() else 0.0

            # Determina o modo e pega parâmetros específicos
            active_tab_index = self.notebook.index(self.notebook.select())
            if active_tab_index == 0: # Espiral
                data['welding_mode'] = 'espiral'
                data['tipo_peca'] = 'cilindrico' # Sempre cilíndrico agora
                data['diametro_inicial'] = data['diametro']; data['diametro_final'] = data['diametro'] # Mantém compatibilidade
            elif active_tab_index == 1: # Oscilação Linear
                data['welding_mode'] = 'oscilacao_linear'
                data['tipo_peca'] = 'cilindrico' # Sempre cilíndrico
                data['diametro_inicial'] = data['diametro']; data['diametro_final'] = data['diametro']
                data['oscilacao_comprimento'] = float(self.params['oscilacao_comprimento'].get()) if self.params['oscilacao_comprimento'].get() else 0.0
                data['deslocamento_angular_perc'] = float(self.params['deslocamento_angular_perc'].get()) if self.params['deslocamento_angular_perc'].get() else 0.0
                data['oscilacao_enabled'] = True # Implícito neste modo
            else:
                 return None # Aba desconhecida

        except ValueError: return None
        data['notes'] = self.notes_text.get("1.0", tk.END).strip()
        if data['largura_cordao'] <= 0 or data['velocidade_soldagem'] <= 0: return None
        if data.get('sobreposicao') is not None and (data['sobreposicao'] >= 100 or data['sobreposicao'] < 0): return None
        if data.get('num_camadas', 1) < 1: return None
        if data['welding_mode'] == 'oscilacao_linear':
             if data['oscilacao_comprimento'] <= 0: return None
             if data['deslocamento_angular_perc'] <= 0 or data['deslocamento_angular_perc'] > 100: return None # % entre 0 e 100
        return data

    def executar_calculos_e_desenho(self):
         params = self._get_current_params()
         if params is None:
             self.limpar_resultados(); self.desenhar_percurso_3d()
             if hasattr(self, 'gcode_text'): self.gcode_text.config(state='normal'); self.gcode_text.delete('1.0', tk.END); self.show_notification(self.lang.get("check_parameters"), 'warning'); self.gcode_text.insert('1.0', self.lang.get("check_parameters")); self.gcode_text.config(state='disabled')
             return
         try:
             diameter = params['diametro']; diameter = max(diameter, 1e-6)
             # --- CORRIGIDO: Define 'h' aqui ---
             h = params['comprimento_revestir']
             # --- FIM CORREÇÃO ---
             C = params['num_camadas'] * params['espessura_camada']; peso = params['fator_peso']
             lead_in = params.get('lead_in', 0.0); lead_out = params.get('lead_out', 0.0);
             circunferencia = math.pi * diameter
             larg_cordao = params['largura_cordao']
             sobreposicao = params['sobreposicao']
             velocidade_soldagem = params['velocidade_soldagem']

             # Cálculos específicos por modo
             if params['welding_mode'] == 'espiral':
                 comprimento_total_arc = h + lead_in + lead_out
                 rpm = velocidade_soldagem / circunferencia if circunferencia > 0 else 0
                 passo = larg_cordao * (1.0 - (sobreposicao / 100.0)); passo = max(passo, 1e-6)
                 total_rotacoes_part = (h / passo) if h > 0 else 0; angulo_part = total_rotacoes_part * 360.0
                 tempo_min_arc = (comprimento_total_arc / velocidade_soldagem) if velocidade_soldagem > 0 else 0
                 area_camada = circunferencia * h; area_total = area_camada * params['num_camadas']
                 area_leads = circunferencia * (lead_in + lead_out) * params['num_camadas'] if lead_in + lead_out > 0 else 0
                 consumo_po_g = (area_total + area_leads) * peso; consumo_po_kg = consumo_po_g / 1000.0
                 self.resultados['rpm'].set(f"{rpm:.3f}"); self.resultados['passo'].set(f"{passo:.3f} (axial)"); self.resultados['rotacoes'].set(f"{total_rotacoes_part:.2f}")
                 self.resultados['angulo'].set(f"{angulo_part:.2f}"); self.resultados['tempo'].set(f"{tempo_min_arc:.2f} (x{params['num_camadas']})")
                 tempo_total_horas = tempo_min_arc * params['num_camadas'] / 60.0
             elif params['welding_mode'] == 'oscilacao_linear':
                 comp_osc = params['oscilacao_comprimento']; desloc_perc = params['deslocamento_angular_perc']
                 desloc_linear_angular = (desloc_perc / 100.0) * larg_cordao;
                 num_passos_angulares_por_volta = math.ceil(circunferencia / desloc_linear_angular) if circunferencia > 0 and desloc_linear_angular > 0 else 1
                 actual_delta_A_deg = 360.0 / num_passos_angulares_por_volta if num_passos_angulares_por_volta > 0 else 360.0
                 feed_angular = (velocidade_soldagem / circunferencia) * 360.0 if circunferencia > 0 else 0

                 tempo_osc_ida_volta = (comp_osc * 2) / velocidade_soldagem if velocidade_soldagem > 0 else 0
                 tempo_rotacao_passo = actual_delta_A_deg / feed_angular if feed_angular > 0 else 0
                 tempo_por_passo_angular = tempo_osc_ida_volta + tempo_rotacao_passo
                 tempo_por_volta = tempo_por_passo_angular * num_passos_angulares_por_volta

                 passo_sobreposicao = larg_cordao * (1.0 - sobreposicao / 100.0); passo_sobreposicao = max(passo_sobreposicao, 1e-6)
                 num_passos_axiais = math.ceil(h / passo_sobreposicao) if h > 0 else 1
                 tempo_min_total_osc = num_passos_axiais * tempo_por_volta

                 tempo_leads = (lead_in + lead_out) / velocidade_soldagem if velocidade_soldagem > 0 else 0
                 tempo_min_total = tempo_min_total_osc + tempo_leads

                 self.resultados['rpm'].set("N/A"); self.resultados['passo'].set(f"{passo_sobreposicao:.3f} (axial)")
                 self.resultados['rotacoes'].set(f"{num_passos_axiais:.2f} (passos axiais)"); self.resultados['angulo'].set(f"{actual_delta_A_deg:.2f}° (passo)")
                 self.resultados['tempo'].set(f"{tempo_min_total:.2f} (x{params['num_camadas']})")
                 tempo_total_horas = tempo_min_total * params['num_camadas'] / 60.0

                 area_camada = circunferencia * h; area_total = area_camada * params['num_camadas']
                 area_leads = circunferencia * (lead_in + lead_out) * params['num_camadas'] if lead_in + lead_out > 0 else 0
                 consumo_po_g = (area_total + area_leads) * peso; consumo_po_kg = consumo_po_g / 1000.0
             else: self.limpar_resultados(); self.desenhar_percurso_3d(); return

             custo_po = consumo_po_kg * self.config['costs']['powder_brl_kg']
             consumo_gas_m3_hora = params['vazao_gas'] * 60.0 / 1000.0; custo_gas = consumo_gas_m3_hora * tempo_total_horas * self.config['costs']['gas_argon_brl_m3']
             custo_consumiveis = custo_po + custo_gas; custo_operacional = tempo_total_horas * (self.config['costs']['labor_brl_hour'] + self.config['costs']['machine_brl_hour'])
             custo_total = custo_consumiveis + custo_operacional; consumo_gas_m3 = consumo_gas_m3_hora * tempo_total_horas
             symbol = self.config['costs'].get('currency_symbol', '$')
             self.custos['consumiveis'].set(f"{symbol} {custo_consumiveis:.2f}"); self.custos['operacional'].set(f"{symbol} {custo_operacional:.2f}")
             self.custos['total'].set(f"{symbol} {custo_total:.2f}"); self.custos['po'].set(f"{consumo_po_kg:.3f} kg"); self.custos['gas'].set(f"{consumo_gas_m3:.3f} m³")
             self.desenhar_percurso_3d(params)
         except Exception as e:
             error_msg = f"{self.lang.get('error')}: {e}"
             print(f"Erro inesperado em executar_calculos_e_desenho: {e}")
             self.limpar_resultados(); self.desenhar_percurso_3d()
             self.show_notification(error_msg, 'error', duration_ms=0)
             if hasattr(self, 'gcode_text'): self.gcode_text.config(state='normal'); self.gcode_text.delete('1.0', tk.END); self.gcode_text.insert('1.0', error_msg); self.gcode_text.config(state='disabled')
         self.initial_load_done = True

    def limpar_resultados(self):
        for key in self.resultados: self.resultados[key].set("0.00");
        for key in self.custos: self.custos[key].set("...")

    def desenhar_percurso_3d(self, params=None):
        self.ax.clear()
        if params is None: self.ax.set_title(self.lang.get('visualizer_title')); self.canvas.draw(); return
        layer_colors = ['#FF5733', '#FFA500', '#FFC300', '#DAF7A6', '#4ECDC4']
        try:
            d_base = params['diametro']; length_base = params['comprimento_revestir']; direction = params.get('direcao_soldagem', 'esquerda_direita')
            mode = params.get('welding_mode', 'espiral')
            x_base = np.linspace(0, length_base, 30); theta_base = np.linspace(0, 2 * np.pi, 30); xc_base, tc_base = np.meshgrid(x_base, theta_base)
            r_base_vals = d_base / 2.0 ; yc_base = r_base_vals * np.cos(tc_base); zc_base = r_base_vals * np.sin(tc_base); self.ax.plot_surface(xc_base, yc_base, zc_base, alpha=0.1, color='gray')
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
                elif mode == 'oscilacao_linear':
                     num_passos_axiais = math.ceil(length_base / passo) if length_base > 0 else 1
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
            self.ax.set_box_aspect((x_range, y_range, z_range)); self.ax.set_xlabel(self.lang.get("axis_x_length")); self.ax.set_ylabel("Y"); self.ax.set_zlabel(self.lang.get("axis_z_radius"))
            self.ax.set_title(self.lang.get('visualizer_title')); self.canvas.draw()
        except Exception as e:
            print(f"Erro ao desenhar percurso 3D: {e}"); self.ax.set_title(f"{self.lang.get('visualizer_title')} ({self.lang.get('error')})"); self.canvas.draw()

    def _update_gcode_preview(self):
        try:
            params = self._get_current_params()
            if params is None:
                if hasattr(self, 'gcode_text'): self.gcode_text.config(state='normal'); self.gcode_text.delete('1.0', tk.END); self.gcode_text.insert('1.0', self.lang.get("check_parameters")); self.gcode_text.config(state='disabled')
                return
            gcode_list = self.gcode_generator.generate(params, is_dry_run=True)
            if gcode_list:
                full_gcode = "\n".join(gcode_list)
                if hasattr(self, 'gcode_text'): self.gcode_text.config(state='normal'); self.gcode_text.delete('1.0', tk.END); self.gcode_text.insert('1.0', full_gcode); self.gcode_text.config(state='disabled')
            else:
                if hasattr(self, 'gcode_text'): self.gcode_text.config(state='normal'); self.gcode_text.delete('1.0', tk.END); self.gcode_text.insert('1.0', self.lang.get("error_generating_gcode", default="Erro ao gerar G-code preview.")); self.gcode_text.config(state='disabled')
        except Exception as e:
            if hasattr(self, 'gcode_text'): self.gcode_text.config(state='normal'); self.gcode_text.delete('1.0', tk.END); self.gcode_text.insert('1.0', f"{self.lang.get('error')}: {e}"); self.gcode_text.config(state='disabled')
            print(f"Erro ao atualizar G-code preview: {e}")

    def _save_procedure(self):
        proc_path = self.config['database']['procedures_path']; filepath = filedialog.asksaveasfilename(initialdir=proc_path, title=self.lang.get('save_procedure'), defaultextension=".json", filetypes=[("PTA Procedure Files", "*.json")])
        if not filepath: return
        try:
            data_to_save = self._get_current_params();
            if data_to_save is None: self.show_notification(self.lang.get('check_parameters'), 'error'); return
            data_to_save['welding_mode'] = data_to_save.get('welding_mode', 'espiral')
            if data_to_save['welding_mode'] == 'espiral':
                 data_to_save.pop('oscilacao_comprimento', None); data_to_save.pop('deslocamento_angular_perc', None)
            with open(filepath, 'w', encoding='utf-8') as f: json.dump(data_to_save, f, indent=4)
            self.show_notification(self.lang.get('preset_saved'), 'success')
        except Exception as e: self.show_notification(f"{self.lang.get('error')}: {str(e)}", 'error')

    def _load_procedure(self):
        proc_path = self.config['database']['procedures_path']; filepath = filedialog.askopenfilename(initialdir=proc_path, title=self.lang.get('load_procedure'), filetypes=[("PTA Procedure Files", "*.json")])
        if not filepath: return
        try:
            with open(filepath, 'r', encoding='utf-8') as f: loaded_data = json.load(f)
            self.notes_text.delete("1.0", tk.END)
            loaded_data.setdefault('lead_in', '5.0'); loaded_data.setdefault('lead_out', '5.0');
            loaded_data.setdefault('direcao_soldagem', 'esquerda_direita'); loaded_data.setdefault('welding_mode', 'espiral')
            loaded_data.setdefault('oscilacao_comprimento', '10.0'); loaded_data.setdefault('deslocamento_angular_perc', '50.0');
            self._disable_param_traces()
            for key, value in loaded_data.items():
                if key in self.params:
                     if isinstance(self.params[key], tk.BooleanVar): self.params[key].set(bool(value))
                     elif key == 'direcao_soldagem':
                         valid_value = value if value in ['esquerda_direita', 'direita_esquerda'] else 'esquerda_direita'
                         self.params['direcao_soldagem'].set(valid_value)
                         direction_options = {'esquerda_direita': self.lang.get('left_to_right', default='Esquerda -> Direita'), 'direita_esquerda': self.lang.get('right_to_left', default='Direita -> Esquerda')}
                         self.direction_combo.set(direction_options.get(valid_value))
                     elif key == 'd_inicial': self.params['diametro'].set(str(value)) # Carrega no novo campo
                     elif key == 'diametro': self.params['diametro'].set(str(value))
                     elif key != 'd_final': self.params[key].set(str(value)) # Ignora d_final
                elif key == 'notes': self.notes_text.insert("1.0", value)
                elif key == 'welding_mode':
                     if value == 'oscilacao_linear': self.notebook.select(self.tab_osc_linear)
                     else: self.notebook.select(self.tab_espiral)
            self._enable_param_traces(); self.trigger_update()
            self.show_notification(self.lang.get('preset_loaded'), 'success')
        except Exception as e: self.show_notification(f"{self.lang.get('error')} ao carregar: {e}", 'error'); self._enable_param_traces()

    def _disable_param_traces(self):
        for var_name, var in self.params.items():
            if isinstance(var, (tk.StringVar, tk.BooleanVar)) :
                info = var.trace_info()
                if info and info[0]:
                    try: trace_id = info[0][1]; var.trace_remove("write", trace_id)
                    except (IndexError, tk.TclError): pass
        self.notes_text.unbind("<KeyRelease>")

    def _enable_param_traces(self):
        for var_name, var in self.params.items():
            if isinstance(var, (tk.StringVar, tk.BooleanVar)):
                 if var_name not in ['oscilacao_enabled']: var.trace_add("write", self.trigger_update)
        self.notes_text.bind("<KeyRelease>", self.trigger_update)

    def _generate_report(self):
        if not REPORTLAB_AVAILABLE: self.show_notification(self.lang.get('reportlab_missing'), 'warning'); return
        params = self._get_current_params();
        if params is None: self.show_notification(self.lang.get('check_parameters'), 'error'); return
        filepath = filedialog.asksaveasfilename(defaultextension=".pdf", filetypes=[("PDF Files", "*.pdf")])
        if not filepath: return
        try:
            results = {key: var.get() for key, var in self.resultados.items()}; costs = {key: var.get() for key, var in self.custos.items()}
            img_path = "temp_report_img.png"; self.fig.savefig(img_path, dpi=150, bbox_inches='tight', pad_inches=0.1)
            c = pdfcanvas.Canvas(filepath, pagesize=A4); width, height = A4; margin = 20 * mm
            c.setFont("Helvetica-Bold", 18); c.drawString(margin, height - margin, self.lang.get('report_title'))
            logo_path = resource_path('logo.png')
            if os.path.exists(logo_path):
                logo = ImageReader(logo_path); img_w, img_h = logo.getSize(); aspect = img_h / float(img_w) if img_w > 0 else 1
                draw_w = 40 * mm; draw_h = draw_w * aspect; c.drawImage(logo, width - margin - draw_w, height - margin - (draw_h/2), width=draw_w, height=draw_h, preserveAspectRatio=True, mask='auto')
            c.setFont("Helvetica", 8); c.drawString(margin, height - margin - 8*mm, f"Data: {datetime.now().strftime('%d/%m/%Y %H:%M:%S')}"); c.line(margin, height - margin - 10*mm, width - margin, height - margin - 10*mm)
            img_w_px, img_h_px = ImageReader(img_path).getSize(); aspect = img_h_px / float(img_w_px) if img_w_px > 0 else 1
            draw_w = width - 2 * margin; draw_h = draw_w * aspect; img_y_start = height - margin - 15*mm
            if draw_h > img_y_start - 140*mm: draw_h = img_y_start - 140*mm; draw_w = draw_h / aspect if aspect > 0 else 0
            c.drawImage(img_path, (width - draw_w) / 2, img_y_start - draw_h, width=draw_w, height=draw_h, preserveAspectRatio=True)
            y_pos = img_y_start - draw_h - 10*mm; col1_x = margin; col2_x = width / 2 + 5 * mm; line_height = 5 * mm
            def draw_section(title_key, data, x_start, start_y):
                current_y = start_y; c.setFont("Helvetica-Bold", 11); c.drawString(x_start, current_y, self.lang.get(title_key)); current_y -= line_height * 1.5; c.setFont("Helvetica", 9)
                for key, value in data.items(): c.drawString(x_start + 5*mm, current_y, f"{self.lang.get(key)}:"); c.drawString(x_start + 55*mm, current_y, str(value)); current_y -= line_height
                return start_y - current_y
            col1_y = y_pos; geom_data = {"procedure_name": params['nome_procedimento'], "tipo_peca": params['tipo_peca']}
            geom_data.update({"diameter": f"{params['diametro']:.2f} mm", "comprimento_revestir": f"{params['comprimento_revestir']:.2f} mm"})
            col1_y -= draw_section('general_params', geom_data, col1_x, col1_y); col1_y -= line_height
            layer_data = { "num_layers": params['num_camadas'], "thickness_per_layer": f"{params['espessura_camada']:.2f} mm" }; col1_y -= draw_section('layer_params', layer_data, col1_x, col1_y)
            col2_y = y_pos; proc_data = {"arc_current": f"{params['corrente_arco']:.1f} A", "gas_flow": f"{params['vazao_gas']:.1f} l/min", "powder_feed_rate": f"{params['alim_po']:.1f} %", "preheat_temp": f"{params['preaquecimento']:.1f} °C"}; col2_y -= draw_section('process_params', proc_data, col2_x, col2_y); col2_y -= line_height
            results_data = {"rotation_rpm": results['rpm'], "helix_pitch": results['passo'], "total_rotations": results['rotacoes'], "estimated_time": results['tempo']}; col2_y -= draw_section('calculated_results', results_data, col2_x, col2_y)
            c.save(); os.remove(img_path)
            if messagebox.askyesno(self.lang.get('success'), self.lang.get('open_report_prompt')):
                try:
                    if sys.platform == "win32": os.startfile(filepath)
                    else: webbrowser.open(f'file://{os.path.abspath(filepath)}')
                except Exception: webbrowser.open(f'file://{os.path.abspath(filepath)}')
        except Exception as e: self.show_notification(f"Erro ao gerar PDF: {e}", 'error')

    def _generate_gcode_clicked(self, is_dry_run=False):
        params = self._get_current_params()
        if params is None: self.show_notification(self.lang.get('check_parameters'), 'error'); return
        try:
            limites = self.config['machine_limits']
            rpm_calc = float(self.resultados['rpm'].get()) if self.resultados['rpm'].get() != "N/A" else 0.0
            if rpm_calc > limites['max_rpm_chuck']: self.show_notification(self.lang.get('rpm_warning', rpm=rpm_calc, limit=limites['max_rpm_chuck']), 'warning'); return
            comprimento_final = params['comprimento_revestir']
            if comprimento_final > limites['max_length_x']: self.show_notification(self.lang.get('length_warning', length=comprimento_final, limit=limites['max_length_x']), 'warning'); return
        except Exception as e: self.show_notification(f"{self.lang.get('check_parameters')}\n{e}", 'error'); return
        full_gcode_list = self.gcode_generator.generate(params, is_dry_run) # type: ignore
        if not full_gcode_list: self.show_notification(self.lang.get('error_generating_gcode', default="Erro ao gerar G-Code."), 'error'); return # pyright: ignore[reportUnknownMemberType]
        full_gcode = "\n".join(full_gcode_list)
        filepath = filedialog.asksaveasfilename(defaultextension=".tap", filetypes=[("G-Code Files", "*.tap"), ("All Files", "*.*")])
        if not filepath: return
        try:
            with open(filepath, 'w', encoding='utf-8') as f: f.write(full_gcode)
            self.show_notification(self.lang.get('gcode_generated'), 'success') # type: ignore
        except Exception as e: self.show_notification(f"Erro ao salvar G-Code: {e}", 'error')

    def _show_about_dialog(self):
         self.show_notification(self.lang.get('about_text'), 'info', duration_ms=10000)

if __name__ == "__main__":
    root = tk.Tk()
    app = TFM_GCODE(root)
    root.mainloop()

