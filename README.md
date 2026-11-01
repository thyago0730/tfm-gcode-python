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

O executável fica em `target/release/TFM_GCODE.exe` (cerca de 18 MB, sem dependências externas). Em release ele abre só a janela, sem console; em debug (`cargo run -p tfm-app`) o console mostra o log, controlado por `RUST_LOG` (padrão `warn,tfm_app=info,tfm_core=info`).

## Testes

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

A suíte cobre o motor (unitários por módulo e integração em `crates/tfm-core/tests/`, incluindo as fixtures golden) e a interface (estado dos campos, preferências, cálculo em segundo plano, diálogos e ações). O teste `crates/tfm-app/tests/ui_parity.rs` confere, sem abrir janela, que os valores padrão da interface geram para a Espiral o mesmo G-code de `tests/golden/espiral_padrao.gcode`.

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

`src/app/` contém o aplicativo Python/Tkinter original (`TFM_GCODE.py`, `core/`, `ui/`). Ele não é mais o produto, mas é o **oráculo** do port: as fixtures golden saem dele e qualquer dúvida de comportamento se resolve lendo a função Python correspondente — os módulos Rust citam, no cabeçalho, o que portam. Ambiente: `.venv/` com `requirements.txt`.

## Diferenças em relação à versão Python

- **"Camadas Inteligentes" agora funciona.** No Python a caixa existia, mas `_get_current_params` não copiava `smart_layers`/`root_feed_scale`, então ela não tinha efeito. No Rust o valor chega ao gerador e a camada de raiz usa o "% Vel. Raiz". Com a caixa desmarcada (padrão) o G-code é idêntico ao do Python.
- **Tema escuro** usa tons mais claros da mesma paleta (`#3b82f6`, `#22c55e`, `#eab308`, `#ef4444`) por contraste sobre o fundo escuro; o tema claro usa exatamente as cores do design (`#2563eb`, `#16a34a`, `#ca8a04`, `#dc2626`).
- **Relatório PDF** e **atualizador automático** (`latest.json`) ainda não foram portados; o item de menu do relatório aparece desabilitado ("em breve").
- O `.tap` é gravado com fim de linha **CRLF** no Windows (como o Python gravava lá) e LF nos demais sistemas; o Mach3 aceita os dois.
- O 3D é um renderizador por software sobre o `Painter` do egui (linhas projetadas), não VTK.

## Integração contínua

`.github/workflows/rust.yml` roda `cargo test --workspace` e `cargo build -p tfm-app --release` no Windows a cada push e pull request em `main` e `rust-egui`, e publica o `TFM_GCODE.exe` como artefato do workflow.
