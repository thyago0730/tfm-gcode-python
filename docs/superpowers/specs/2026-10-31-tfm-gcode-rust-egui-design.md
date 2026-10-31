# TFM G-Code Generator — Reescrita em Rust + egui (design)

Data: 2026-10-31. Decisões tomadas por Claude a pedido do usuário ("mete marcha do seu jeito, capriche").
Pedido original: "pegue esse método [do Prancheta CNC] e transforme o meu software, com o mesmo design".

## 1. Intenção e porquê

O TFM G-Code Generator hoje é um app Python/Tkinter + Matplotlib (~12,7 mil linhas) que gera
G-code para revestimento PTA (plasma por arco transferido) em peças cilíndricas, chapas e faces
("bolacha"), com rastreabilidade ISO 3834, custos, fórmulas, integração com Mach3 e atualizador.
Funciona, mas abre devagar, tem visual datado (Tkinter), é pesado para distribuir
(PyInstaller + VTK + Matplotlib) e difícil de manter num arquivo de 7 mil linhas.

O "método" do Prancheta CNC, extraído por análise estática do executável (sem executá-lo):

- Rust compilado em release, um único `.exe`, sem instalador nem dependências externas.
- Interface egui/eframe (modo imediato, leve, OpenGL via glow), fonte Inter, tema Automático/Claro/Escuro.
- Painéis laterais esquerdo e direito redimensionáveis e fixáveis (`left_w`, `right_w`, `left_pinned`, `right_pinned` persistidos).
- Vista 3D própria com iluminação simples; ícones com nomes em português.
- Cada parâmetro tem um tooltip em português simples explicando o efeito. Exemplo real do Prancheta:
  "Quanto a fresa desce a cada passada. Menor = mais passadas, menos esforço."
- Monorepo com crates separadas: motor de cálculo isolado da interface (`crates/cam`, `crates/io`, `apps/prancheta`).

A reescrita aplica esse método ao TFM: motor de G-code em uma crate pura e testável, app egui em
outra, visual limpo e moderno, mesmo fluxo de trabalho do app atual.

## 2. Metas

1. Paridade de G-code. Para o mesmo conjunto de parâmetros, o motor Rust gera exatamente as mesmas
   linhas que o Python (exceto a linha de data). Verificado por fixtures "golden" geradas pelo
   Python e comparadas linha a linha em testes Rust. Isso protege a produção.
2. Mesmo fluxo: escolher operação, ajustar parâmetros, ver 3D/gráficos/G-code ao vivo, ler
   resultados e custos, salvar `.tap`, abrir no Mach3, salvar procedimento.
3. Design Prancheta: layout em painéis (esquerdo: parâmetros; centro: visualizações; direito:
   G-code; inferior: resultados, custos e avisos), tema claro/escuro/auto, painéis fixáveis e
   redimensionáveis, tooltips didáticos em todos os campos, ícones desenhados para as 7 operações,
   atalhos de teclado com diálogo de ajuda.
4. Compatibilidade de dados: `config/config.json`, procedimentos `.json`, `data/quality/*.json`
   e `config/maintenance_log.json` continuam legíveis pelo app novo.
5. Um executável: `cargo build --release` gera `TFM_GCODE.exe` com ícone, sem instalador obrigatório.

## 3. Não-metas da v1 e anti-metas

Deixado explicitamente para depois (cada item vira um plano próprio):
- Relatório PDF (item de menu mostra "em breve"; os dados continuam no procedimento salvo).
- Atualizador automático via `latest.json`.
- 3D com sombreamento tipo VTK. A v1 usa renderizador por software (linhas projetadas) no
  `Painter` do egui: suficiente para traçado, camadas, referência da peça e simulação.
- Relato de erros para GitHub. O Prancheta embute um token no exe, o que é inseguro; não copiar.

Anti-metas (seria falha mesmo funcionando):
- G-code diferente do Python para os mesmos parâmetros, em qualquer casa decimal.
- Interface travada durante a geração de programas longos (quadrada contínua com granularidade fina).
- Procedimentos antigos que não abrem.
- Textos em inglês ou com jargão. Tom igual ao app atual, em português.

## 4. Restrições

- Rust estável 1.95 (instalado). Crates: `eframe`/`egui` 0.37, `egui_extras` 0.37, `egui_plot`
  compatível com egui 0.37, `serde` + `serde_json` (feature `preserve_order`), `rfd` 0.17,
  `chrono`, `image` 0.25 (ícone .ico), `log` + `env_logger`.
- Sem `eval`: as fórmulas do `config.json` usam sintaxe Python (`math.pi * diametro`,
  `max(a, 1e-6)`, `math.ceil(x)`) e são avaliadas por um parser próprio na crate core, para que o
  arquivo atual continue válido sem conversão.
- Fonte Inter: carregada em tempo de execução de `assets/fonts/*.ttf` se a pasta existir; senão,
  fontes padrão do egui. Nenhum download durante o build.
- Alvo: Windows 10 x64. O código não deve impedir Linux (eframe e rfd são multiplataforma), mas a
  integração Mach3 é só Windows.
- O código Python permanece em `src/` como referência e gerador das fixtures. Não é apagado nesta fase.

## 5. Arquitetura decidida

Workspace Cargo na raiz do repositório (`C:\tfm-gcode-rust`):

```
Cargo.toml                       workspace: crates/tfm-core, crates/tfm-app
crates/tfm-core/src/
  lib.rs
  params.rs                      Params (serde, chaves iguais ao Python), enums, defaults por operação
  overlap.rs                     modelo de sobreposição (porta de overlap_model.py)
  gcode/mod.rs                   generate(), generate_with_timestamp(), GenError
  gcode/common.rs                cabeçalho, rodapé, tocha, formatação, limites rotativos, raio de camada
  gcode/spiral.rs  multiperfil.rs  bolacha.rs  flat_plate.rs  oscillation.rs
  validator.rs                   invariantes do G-code + validação física dos parâmetros
  estimates.rs                   resultados calculados, sobreposição efetiva, tempo, custos
  formula/{mod,lexer,parser,eval}.rs   avaliador de expressões estilo Python
  config.rs                      AppConfig (config.json) com merge de defaults e preferências de UI
  quality.rs                     operadores, EPS/WPS, manutenção, heat input, diluição, ISO 3834
  toolpath.rs                    G-code -> pontos 3D por operação + séries para gráficos
  procedure.rs                   salvar/carregar procedimento (compatível com o Python)
  audit.rs                       log de auditoria em data/logs (LOG_ENG_AAAA-MM-DD.json)
crates/tfm-app/src/
  main.rs  app.rs  theme.rs  prefs.rs  widgets.rs  icons.rs  compute.rs
  panels/{left,center,right,bottom}.rs
  view3d.rs  charts.rs  integration.rs
  dialogs/*.rs                   custos, pós, fórmulas, perfis de máquina, caminho Mach3, operadores,
                                 EPS, matriz de riscos, manutenção, sobre, tutorial, atalhos
scripts/golden/gen_golden.py     gera tests/golden/*.gcode com o Python atual
tests/golden/                    cases.json + um .gcode por caso (fixtures de paridade)
```

Fluxo de dados no app: `UiState` (textos digitados) -> `Params` (validação; campos inválidos ficam
em vermelho) -> `generate()` -> `Vec<String>` -> `estimates`, `toolpath`, séries de gráficos.
Recalcula com debounce de 300 ms após qualquer edição; a geração roda em thread separada e o
resultado volta por canal, para a UI nunca travar.

Layout, de cima para baixo:
- Barra superior: menus (Arquivo, Configurações, Qualidade, Simulação, G-code, Ajuda), botões de
  ação (Gerar G-code, Abrir no Mach3, Salvar e Carregar procedimento), alternador de tema, pinos
  dos painéis.
- Painel esquerdo (300 a 480 px): seletor de operação (7 cartões com ícone, lista recolhível) e
  abas Parâmetros / Processo / Rastreabilidade.
- Centro: abas 3D Interativo / Análise Temporal / Oscilação / Estatísticas / Processo.
- Painel direito (300 a 520 px): G-code com busca, filtros por eixo, contagem de linhas, LED da
  tocha, realce de sintaxe, Copiar tudo.
- Painel inferior recolhível: Resultados Calculados | Estimativa de Custo | Avisos (histórico com
  hora e tipo).

Tema: paleta clara e escura próprias (não a cinza padrão do egui), cantos levemente arredondados,
destaque azul `#2563eb`, sucesso verde `#16a34a`, atenção âmbar `#ca8a04`, erro vermelho `#dc2626`,
a mesma família de cores encontrada no Prancheta. Camadas no 3D usam a sequência
`#2563eb #dc2626 #16a34a #9333ea #ea580c #0891b2 #be185d #65a30d #854d0e`.

## 6. Decisões já tomadas (o construtor não reabre)

- Chaves de parâmetros e de config são as do Python (`diametro`, `comprimento_revestir`,
  `largura_cordao`, `sobreposicao`, `velocidade_de_deposicao`, ...). Chaves legadas aceitas na
  leitura: `velocidade_soldagem`, modos `oscilacao_linear`/`oscilacao_quadrada`, `internal_entry_x`.
  Números gravados como texto (`"5.0"`) são aceitos na leitura.
- Formatação numérica do G-code: `{:.3}`, `{:.1}`, `{:.2}`, `{:+.3}` do Rust equivalem aos
  f-strings do Python; a comparação golden é exata, linha a linha, incluindo linhas vazias.
- A linha `(Data: ...)` recebe o timestamp por parâmetro (`generate_with_timestamp`); o app passa
  a hora atual no formato `dd/mm/aaaa HH:MM:SS`.
- Versão no cabeçalho: `app_version` dos parâmetros quando presente; senão `CARGO_PKG_VERSION`
  (começa em `2.0.0`).
- Tooltips: um por campo, frase curta em português dizendo o efeito e a direção ("maior = ...").
  Os já existentes no Python são reaproveitados.
- Ícones das operações desenhados com `Painter` (porta dos desenhos em Canvas do Python). Sem SVG na v1.
- Gráficos com `egui_plot`. Dashboard de processo (custos em rosca, consumo em barras, esquema de
  camadas, tempo) desenhado com `Painter` e `egui_plot`.
- Abrir no Mach3: salva `TFM_AAAAMMDD_HHMMSS.tap` em `integration.gcode_dir`, escreve `autoload.txt`
  se configurado, garante as macros `M101.m1s` (`ActivateSignal(Output1)`) e `M102.m1s`
  (`DeactivateSignal(Output1)`) e inicia `Mach3.exe /p <perfil> <arquivo>`.
- Alerta de segurança G53 antes de salvar `.tap`: diálogo modal perguntando se a máquina está
  referenciada (REF ALL HOME).
- Bloqueios ISO 3834 iguais ao Python: operador com qualificação vencida, parâmetro fora dos
  limites da EPS, campos obrigatórios (lote, fornecedor, certificado) quando há EPS selecionada.
- Nome do executável: `TFM_GCODE.exe`. Título da janela: `TFM G-Code Generator v2.0.0 - Cladding & Hardfacing`.

## 7. O que fica a critério do construtor

- Espaçamentos, tamanhos de fonte e animações, respeitando paleta e layout acima.
- Estrutura interna dos módulos da UI além do que o plano fixa.
- Estratégia de cache dos resultados, desde que o recálculo não bloqueie a UI.
