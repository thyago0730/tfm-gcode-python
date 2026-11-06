# TFM G-Code Generator

Aplicativo desktop (Windows) da TFM Usinagem & Manutenção Industrial para gerar, visualizar e analisar G-code de revestimento por PTA (cladding e hardfacing) em tornos CNC com Mach3: Espiral, Espiral Multiperfil, Oscilação (linear, quadrada e quadrada contínua), Chapa Plana e Bolacha.

Esta é a versão **Rust + egui** (v2.0.0), reescrita a partir do aplicativo Python/Tkinter, que continua em `src/` como implementação de referência. Para os mesmos parâmetros o G-code gerado é idêntico ao do Python, linha a linha — isso é verificado por fixtures golden (veja abaixo).

## Estrutura

Workspace Cargo com duas crates:

| Crate | Pasta | O que faz |
|---|---|---|
| `tfm-core` | `crates/tfm-core/` | Motor sem interface: parâmetros, modelo de sobreposição, geração de G-code por operação, validação física e de invariantes, estimativas e custos, avaliador de fórmulas, configuração, procedimentos, qualidade ISO 3834, toolpath 3D e auditoria. |
| `tfm-app` | `crates/tfm-app/` | Interface gráfica (eframe/egui 0.37): painéis, gráficos, 3D interativo, diálogos, tema claro/escuro, geração do `.tap` e integração com o Mach3. Produz o binário `TFM_GCODE`. |

A tabela módulo a módulo está em [`docs/README.md`](docs/README.md). A especificação de design e o plano do port ficam em `docs/superpowers/`.

## Compilar

Requisito: [Rust estável](https://rustup.rs) (`rust-toolchain.toml` fixa o canal `stable`). No Windows, o `build.rs` incorpora `assets/icone.ico` e as informações de versão ao `.exe`; isso usa o `rc.exe` do Windows SDK (vem com o Visual Studio Build Tools). Sem ele o build **não falha**: emite um aviso e o executável sai sem ícone.

```
cargo build --release
```

Para testar com um clique, `scripts\run_app.cmd` (ou `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\run_app.ps1`) compila em release, atualiza `dist\TFM_GCODE.exe` (fechando uma instância aberta de lá) e abre o programa; o exe em `dist\` usa o `config\config.json`, os procedimentos e os logs da raiz do repositório. Opções: `-SemAbrir` (só compila e copia) e `-SemBuild` (só abre).

O executável fica em `target/release/TFM_GCODE.exe` (cerca de 18 MB, sem dependências externas). Em release ele abre só a janela, sem console; em debug (`cargo run -p tfm-app`) o console mostra o log, controlado por `RUST_LOG` (padrão `warn,tfm_app=info,tfm_core=info`).

## Testes

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

A suíte cobre o motor (unitários por módulo e integração em `crates/tfm-core/tests/`, incluindo as fixtures golden) e a interface (estado dos campos, preferências, cálculo em segundo plano, diálogos e ações). O teste `crates/tfm-app/tests/ui_parity.rs` confere, sem abrir janela, que a interface — padrões, escolha da operação nos cartões e os campos digitados — gera, para cada uma das sete operações, o mesmo G-code das fixtures golden correspondentes. Os testes que precisam de um `config.json` completo leem a fixture `tests/fixtures/config_exemplo.json`; o `config/config.json` do repositório é só carregado, sem asserções sobre o conteúdo.

## Fixtures golden (paridade com o Python)

`tests/golden/` guarda `cases.json` (os parâmetros de cada caso) e um `.gcode` por caso, gerados pelo motor Python original. `crates/tfm-core/tests/golden.rs` gera cada caso com o Rust e compara linha a linha.

- **Nunca edite um `.gcode` à mão.** Se o comportamento esperado mudar, regenere todas as fixtures com o Python:

  ```
  .venv/Scripts/python.exe -I scripts/golden/gen_golden.py
  ```

- Para acrescentar um caso, inclua-o em `scripts/golden/gen_golden.py` e regenere.
- A data do cabeçalho é fixada em `01/01/2000 00:00:00` e a versão em `11.3.2` para o resultado ser determinístico; o app imprime a data atual e a própria versão.

## Configuração e dados

Os caminhos são relativos à **raiz do aplicativo**: a raiz do repositório em desenvolvimento (`cargo run`) e a pasta do executável em release. Em release o app também aceita `config.json` ao lado do `.exe` ou na pasta pai, na mesma ordem do Python congelado; se nenhum existe, cria `config/config.json` com os padrões.

| Caminho | Conteúdo |
|---|---|
| `config/config.json` | Configuração completa: preferências de interface (`ui`), custos, pós, fórmulas, integração com o Mach3, perfis de máquina. O app regrava só as chaves `ui.*` (tema, larguras e pinos dos painéis); o resto só muda pelos diálogos de Configurações. |
| `data/quality/` | `operators.json` e `wps.json` (operadores qualificados e EPS/WPS). |
| `data/logs/` | Log de auditoria de engenharia, um arquivo por dia (`LOG_ENG_AAAA-MM-DD.json`). |
| `procedures/` | Procedimentos salvos (`*.json`, compatíveis com os do Python). Pasta configurável em `database.procedures_path`. |
| `assets/` | `icone.ico` (ícone da janela e do `.exe`). Uma fonte Inter opcional em `assets/fonts/*.ttf` é usada se existir; sem ela, as fontes padrão do egui. |

## Implementação de referência (Python)

`src/app/` contém o aplicativo Python/Tkinter original (`TFM_GCODE.py`, `core/`, `ui/`). Ele não é mais o produto, mas é o **oráculo** do port: as fixtures golden saem dele e qualquer dúvida de comportamento se resolve lendo a função Python correspondente — os módulos Rust citam, no cabeçalho, o que portam. O ambiente Python (`.venv/` com `requirements.txt`) é opcional: só é preciso para regenerar as fixtures golden ou rodar os testes Python de referência.

## Diferenças em relação à versão Python

- **"Camadas Inteligentes" agora funciona.** No Python a caixa existia, mas `_get_current_params` não copiava `smart_layers`/`root_feed_scale`, então ela não tinha efeito. No Rust o valor chega ao gerador e a camada de raiz usa o "% Vel. Raiz". Com a caixa desmarcada (padrão) o G-code é idêntico ao do Python.
- **"Gerar G-code" na Oscilação Quadrada Contínua agora funciona.** No Python a validação lia a chave inexistente `granularidade_x` e sempre bloqueava a geração desse modo; no Rust a validação usa `osc_test_gran_x`, o campo que a interface realmente preenche.
- **"Abrir no Mach3" passa pelos mesmos bloqueios de "Gerar G-code"** (campos, regras físicas, ISO 3834 e o alerta de segurança G53) e registra o arquivo gravado no log de auditoria ("GCode Generation (Mach3)"). No Python ele gravava e abria o Mach3 sem essas verificações.
- **"Baixar G-code" foi portado** (o botão ⤓ do visualizador do Python; aqui no menu G-code e na barra de botões). Passa pelos mesmos bloqueios de "Gerar G-code", mostra a checklist dos invariantes do G-code (programa com linhas, M101 antes da solda, eixo A no sentido escolhido) e só salva com tudo OK; sugere o nome `<ordem de serviço>.tap` pela regra do Python (`TFM_AAAAMMDD_HHMMSS.tap` sem OS), grava ao lado o `<nome>.audit.json` (data, operador, EPS, OS, linhas, impressão digital do arquivo e parâmetros) e registra "GCode Download" no log de auditoria. No Python ele só gravava o arquivo na pasta escolhida, sem checklist nem registro.
- **Tema escuro** usa tons mais claros da mesma paleta (`#3b82f6`, `#22c55e`, `#eab308`, `#ef4444`) por contraste sobre o fundo escuro; o tema claro usa exatamente as cores do design (`#2563eb`, `#16a34a`, `#ca8a04`, `#dc2626`).
- **Relatório PDF** e **atualizador automático** (`latest.json`) ainda não foram portados; o item de menu do relatório aparece desabilitado ("em breve").
- O `.tap` é gravado com fim de linha **CRLF** no Windows (como o Python gravava lá) e LF nos demais sistemas; o Mach3 aceita os dois.
- **No 3D das operações cilíndricas quem gira é a peça.** Como na máquina, a tocha fica fixa no topo e anda só em X (e em Z por camada); a peça gira em torno do centro do eixo A levando o cordão já depositado, e durante a simulação o viewport mostra a leitura "Tocha X … · Peça A …". No Python a tocha orbitava a peça parada. O G-code não muda (o Mach3 continua controlando o Z); chapa plana e bolacha seguem desenhadas como antes.
- **Cubo de orientação no 3D**, como o ViewCube do Inventor, no lugar dos botões ISO/Topo/Lado/Frente/Trás: as faces nomeadas giram com a câmera, clicar numa face escolhe a vista, perto de uma aresta ou canto dá a vista diagonal, arrastar orbita e ⌂ volta à isométrica. Os nomes seguem o Inventor: Frente é o lado do operador (eixo X para a direita), Esquerda e Direita olham pelas pontas do eixo, Topo, Base e Trás completam. As teclas 1–5 e o menu Simulação mantêm os mesmos ângulos de antes (1 ISO, 2 Topo, 3 Frente, 4 Esquerda, 5 Direita).
- **No 3D a Bolacha fica deitada na mesa, como na máquina.** X é o raio, Z é a altura da tocha e a mesa gira num eixo vertical. Na simulação a tocha fica parada no raio e anda só em X, e quem gira é a mesa, levando o cordão junto, no sentido escolhido em "Sentido da Rotação". A leitura "Tocha X" mostra o X da máquina. O Python desenhava a peça em pé e parada. Só a visualização muda: o G-code é o mesmo.
- **A simulação mostra a rotação da placa ao vivo**, em rpm e em min/volta, logo abaixo de "Tocha X" e "Peça A". O valor sai do F de cada G01 como o Mach3 lê, com os graus de A contando como milímetros, e bate com o painel "Rotação (min/volta)". Ele fica vermelho acima do limite da placa do perfil da máquina. Em G0 aparece "Placa em G0", porque a máquina vai na velocidade dela.
- **O plano do chão só aparece sob a peça deitada**, na Chapa Plana e na Bolacha. Nas peças cilíndricas, que ficam na horizontal, o plano saiu e o triedro XYZ continua no centro do eixo A.
- **"Solda" no 3D simula o material depositado**, em vez de repassar uma camada. Nas operações cilíndricas, cada movimento com a tocha ligada deposita `vazão × tempo`, com o tempo do `F` real de cada linha, inclusive no apagamento: onde a máquina anda mais devagar ou repassa, a camada sobe mais. O resultado é um mapa de altura na superfície da peça, colorido da altura mais baixa à mais alta, que sobe junto com a simulação. O canto da vista mostra as alturas mínima, média e máxima, o sobremetal a usinar, o volume e o tempo de solda, medidos entre as posições extremas do centro do cordão, porque nas bordas a altura cai até zero no pé do cordão. Na Espiral e no Multiperfil a vazão é a que dá a espessura pedida no platô; nas oscilações, a que dá essa espessura na média da peça. Chapa plana e bolacha mantêm o perfil do cordão. A silhueta da peça cilíndrica tem sempre o comprimento a revestir, no sentido da solda; o percurso da Espiral e do Multiperfil pode passar dela por frações de milímetro no apagamento.
- **"Compensar rotação pela densidade"**, caixa nova nas Opções de Soldagem da Espiral e das duas quadradas: usa a mesma simulação para ajustar a rotação por trecho, gravando o `F` no G-code e o comentário `(COMPENSACAO DE ROTACAO: ...)`, sem mudar nenhuma posição. Na Espiral o material acumula nas pontas, então a rotação sobe, até 2,5 vezes: na peça Ø50 × 50 mm com fechamento uniforme, o sobremetal cai de 1,60 para 0,59 mm, com 10% menos material e tempo. Nas quadradas falta material nas pontas, então a máquina desacelera pela posição em X, até 2,5 vezes, e aqui a caixa precisa do fechamento uniforme: na sequencial o mínimo sobe de 0,62 para 1,41 mm e o sobremetal cai de 1,41 para 0,70 mm; na contínua o mínimo sobe de 0,47 para 1,06 mm e o sobremetal cai de 1,58 para 1,20 mm, porque a borda precisaria de 4 vezes e para no teto. O preço é de 10% a 14% mais pó e tempo. A vista compara com o programa sem compensação, na mesma vazão. Confira se a velocidade nas pontas continua dentro da EPS. O "Tempo Total" dos Resultados não conta a compensação; o tempo real de solda está na vista 3D. Desligada, o G-code não muda.
- **"Fechamento uniforme (borda a borda)"**, caixa nova nas Opções de Soldagem da Espiral, do Multiperfil e das três oscilações: o comprimento passa a valer de borda a borda do material depositado, não pela linha de centro. O centro do cordão anda meia largura efetiva para dentro de cada lado, as voltas sobem até um inteiro nesse curso e o passo é redistribuído, então a sobreposição só aumenta. Na Espiral, cada ponta fecha num anel de 360° (sem o anel a hélice só tocaria a borda em um ponto da volta) e o `M102` sai antes do fim do anel, para o cordão terminar ao fechar a volta, contando o apagamento. No Multiperfil não há anéis, que empilhariam material nas pontas, e o apagamento fica só em A. Na oscilação linear o primeiro e o último anel encostam nas bordas e o espaçamento é redistribuído. Nas quadradas a banda inteira (curso da oscilação mais a largura do cordão) cabe no comprimento, sem cortes no limite, e o passo final em X sai. O G-code ganha o comentário `(FECHAMENTO UNIFORME: N voltas, passo X mm, centro Xa a Xb)` e os resultados mostram voltas, passo e cobertura reais. Na primeira e na última volta o anel e a hélice se sobrepõem mais, então ali o depósito fica mais alto. Desligada (padrão, e em todo procedimento antigo), o G-code segue o do Python.
- **O apagamento da tocha não sai mais da peça nas oscilações nem na Chapa Plana.** No Python, o movimento de "manter cordão" depois do `M102` prolongava o último movimento: na oscilação quadrada eram 100 mm em X a F3000, com a tocha ainda depositando. Agora ele gira em A, no ponto onde a solda terminou, nas três oscilações, com ou sem o fechamento uniforme; nas quadradas gira 4 vezes mais rápido para não formar um calombo. Na Chapa Plana não há eixo A para girar, então o cordão para antes da borda o que a tocha ainda deposita ao apagar (4 mm em 2 s a 120 mm/min), o `M102` sai ali e o apagamento termina exatamente na borda, em vez de passar dela. São os únicos desvios do Python com as caixas desligadas: os testes de paridade deixam de comparar só a linha logo depois de `(APAGAMENTO DA TOCHA` e, na chapa, o fim do cordão antes do `M102`.
- **Layout compacto para o dia a dia:** o 3D é o centro da tela. A barra de cima tem só "Gerar G-code", "Abrir no Mach3", salvar e carregar procedimento e o tema; "Baixar G-code" e "Copiar tudo" ficam no menu G-code. As sete operações ficam sempre à vista, em ícones de um clique. O rodapé é uma linha de status (tempo de solda, pó, custo, sobremetal, passo e rotação) cujos valores acendem quando mudam; os detalhes abrem por ▲. O G-code e os parâmetros recolhem numa faixa na borda e deslizam ao abrir. A simulação tem linha do tempo arrastável, relógio e velocidade sobre o 3D, a câmera desliza até a vista pedida no cubo, e passar o mouse na peça mostra a altura do ponto. O alerta G53 não abre janela: a barra de cima se transforma na pergunta, com "Confirmar" (Ctrl+Enter) e "Voltar" (Esc); só Ctrl+Enter confirma, para um Enter solto não liberar a máquina. O layout novo entra uma vez só, pela chave `ui.layout_rev` do `config.json`, e depois o que você abrir ou fechar fica salvo.
- O 3D é um renderizador por software sobre o `Painter` do egui (linhas projetadas), não VTK.

## Integração contínua

`.github/workflows/rust.yml` roda `cargo test --workspace` e `cargo build -p tfm-app --release` no Windows a cada push e pull request em `main` e `rust-egui`, e publica o `TFM_GCODE.exe` como artefato do workflow.
