# Documentação do projeto

Visão geral, build, testes e fixtures golden estão no [`README.md`](../README.md) da raiz. Esta página mapeia o código: um módulo por linha. Os cabeçalhos de cada módulo (`//!`) dizem qual função do Python ele porta.

## Pastas

| Pasta | Conteúdo |
|---|---|
| `crates/tfm-core/` | Motor de cálculo e geração de G-code, sem interface |
| `crates/tfm-app/` | Interface eframe/egui e o binário `TFM_GCODE` |
| `tests/golden/` | Fixtures de paridade (`cases.json` + um `.gcode` por caso) geradas pelo Python |
| `scripts/golden/gen_golden.py` | Regenera as fixtures com o motor Python (`.venv/Scripts/python.exe -I scripts/golden/gen_golden.py`) |
| `src/app/` | Aplicativo Python/Tkinter original: implementação de referência (`TFM_GCODE.py`, `core/`, `ui/`) |
| `config/`, `data/`, `procedures/`, `assets/` | Configuração, dados de qualidade e logs, procedimentos salvos, ícone e fontes |
| `docs/superpowers/` | Especificação de design (`specs/`) e plano de implementação (`plans/`) do port |
| `.github/workflows/rust.yml` | CI: testes e build release no Windows, com o `.exe` como artefato |

## `tfm-core` — módulos

| Módulo | Responsabilidade |
|---|---|
| `lib.rs` | Raiz da crate; só declara os módulos. |
| `params.rs` | `Params` (serde, chaves iguais ao JSON do Python), enums de operação, direção, perfil e bolacha, defaults e `apply_operation`/`normalize`. |
| `overlap.rs` | Modelo de sobreposição efetiva do cordão (passo, largura efetiva, nº de passes); porta literal de `overlap_model.py`. |
| `gcode/mod.rs` | `generate()`, `generate_with_timestamp()`, `GenError`; valida, normaliza e despacha para o módulo do modo. |
| `gcode/common.rs` | Cabeçalho e rodapé, sequências de tocha (M101/M102 e atrasos), formatação numérica igual à do Python, geometria rotativa. |
| `gcode/spiral.rs` | Modo Espiral (hélice X+A por camada, externo ou interno). |
| `gcode/multiperfil.rs` | Modo Espiral Multiperfil (vários perfis por setor, formas retangular/trapezoidal/triangular, estratégias). |
| `gcode/bolacha.rs` | Modo Bolacha (face circular: anéis ou espiral, com paradas). |
| `gcode/flat_plate.rs` | Modo Chapa Plana (cordões retos em X ou Y, zigue-zague ou unidirecional, lead-in/out). |
| `gcode/oscillation.rs` | Oscilação linear, quadrada e quadrada contínua, mais os modos legados. |
| `validator.rs` | Regras físicas dos parâmetros (mensagens da UI) e invariantes do G-code gerado. |
| `estimates.rs` | Resultados calculados, sobreposição efetiva, tempo, consumos e custos (via fórmulas do config). |
| `formula/mod.rs` | Avaliador de fórmulas estilo Python (API e contexto de variáveis). |
| `formula/lexer.rs` | Tokens: números, identificadores com `.`, operadores. |
| `formula/parser.rs` | Parser de precedência (Pratt) para a gramática aritmética do Python. |
| `formula/eval.rs` | Avaliação com a semântica numérica do Python (`math.*`, `min`/`max`, divisão, potência). |
| `config.rs` | `AppConfig` (`config/config.json`): defaults, merge recursivo, `resolve_path`/`app_root`, pós, perfis de máquina, fórmulas. |
| `quality.rs` | Operadores, EPS/WPS, manutenção preventiva, materiais, heat input, diluição e validação ISO 3834. |
| `toolpath.rs` | Lê o G-code gerado de volta em pontos 3D por operação e nas séries dos gráficos. |
| `procedure.rs` | Salvar e carregar procedimento (`*.json` compatível com o Python). |
| `audit.rs` | Log de auditoria em `data/logs/LOG_ENG_AAAA-MM-DD.json`. |

Testes de integração em `crates/tfm-core/tests/`: `golden.rs` (paridade com as fixtures), `params.rs`, `overlap.rs`, `formula.rs`, `estimates.rs`, `toolpath.rs`, `config_procedure.rs`.

## `tfm-app` — módulos

| Módulo | Responsabilidade |
|---|---|
| `main.rs` | Ponto de entrada do `TFM_GCODE.exe`: janela maximizada com ícone, log, `eframe::run_native`. |
| `lib.rs` | Raiz da crate; `APP_TITLE` e o ícone da janela. |
| `app.rs` | Estado raiz (`TfmApp`), barra superior (menus e ações), layout em painéis, debounce do recálculo, avisos. |
| `ui_state.rs` | Estado editável do painel esquerdo: textos digitados, flags, enums, operação, presets por operação; conversão para `Params`. |
| `compute.rs` | Geração em segundo plano: thread única que calcula G-code, validações, estimativas, toolpath e séries, devolvendo por canal. |
| `prefs.rs` | Preferências de interface (`ui` do `config.json`) e as pastas do aplicativo (`config/`, `data/quality/`, `data/logs/`). |
| `theme.rs` | Paleta clara e escura, preferência Automático/Claro/Escuro e carga opcional da fonte Inter. |
| `widgets.rs` | Widgets reutilizáveis: campos com rótulo e tooltip, seções, KPIs. |
| `icons.rs` | Ícones das sete operações desenhados com `Painter` (porta dos Canvas do Tk). |
| `integration.rs` | Ações: gerar o `.tap` (com alerta G53), abrir no Mach3, salvar/carregar procedimento, atalhos de teclado. |
| `view3d.rs` | 3D interativo: câmera orbital, grade, eixos, silhueta da peça, trajetória por camada, presets de vista e simulação. |
| `charts.rs` | Gráficos do centro: Análise Temporal, Oscilação, Estatísticas e o dashboard de Processo. |
| `panels/mod.rs` | Declara os quatro painéis fixos. |
| `panels/left.rs` | Painel esquerdo: cartão e lista de operações, abas Parâmetros / Processo / Rastreabilidade. |
| `panels/center.rs` | Centro: abas de gráficos (o 3D é despachado pelo `app.rs`). |
| `panels/right.rs` | Painel direito: visualizador de G-code com busca, filtros por eixo, LED da tocha, Copiar tudo e Salvar `.tap`. |
| `panels/bottom.rs` | Painel inferior: Resultados Calculados, Estimativa de Custo e Avisos. |
| `dialogs/mod.rs` | `DialogId`, estado `Dialogs` e o despachante `show_dialogs`. |
| `dialogs/costs.rs` | "Custos de Operação". |
| `dialogs/powders.rs` | "Tipos de Pó". |
| `dialogs/formulas.rs` | "Fórmulas" (editor JSON das expressões). |
| `dialogs/machine_profiles.rs` | "Perfis de Máquina" (limites físicos). |
| `dialogs/mach3_path.rs` | "Caminho (Mach3)" e instalação dos macros M101/M102. |
| `dialogs/operators.rs` | "Gerenciar Operadores". |
| `dialogs/wps.rs` | "Gerenciar EPS (WPS)". |
| `dialogs/risk_matrix.rs` | "Matriz de Riscos e Controles" (ISO 3834 / ISO 9001). |
| `dialogs/maintenance.rs` | "Manutenção Preventiva". |
| `dialogs/about.rs` | "Sobre". |
| `dialogs/tutorial.rs` | "Ajuda": tutorial em abas. |
| `dialogs/shortcuts.rs` | "Atalhos": tabela dos atalhos de teclado. |

Teste de integração em `crates/tfm-app/tests/ui_parity.rs`: os defaults da interface geram, para a Espiral, o mesmo G-code de `tests/golden/espiral_padrao.gcode`.
