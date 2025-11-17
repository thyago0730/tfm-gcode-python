Documentação do projeto.

Pastas principais:
- src: código-fonte (app, visualizers, simulators, menus)
- scripts: utilitários de execução e automação
- assets: ícones e imagens estáticas
- config: arquivos de configuração
- data: dados gerados/consumidos (ex.: procedures)
- tests: testes automatizados
## Organização atualizada do projeto

- `src/app/` — aplicação principal e UI.
- `config/` — configurações padrão (`config.json`).
- `tests/` — testes automatizados e fixtures:
  - `tests/fixtures/` — arquivos de referência (entradas/saídas esperadas).
  - `tests/analysis/` — artefatos voláteis gerados em análises (limpos automaticamente).
- `scripts/` — utilitários:
  - `scripts/cleanup_test_artifacts.py` — remove artefatos de análise após execução.

### Limpeza automática de artefatos de análise

Para manter o repositório limpo, artefatos gerados em análises/testes devem ser colocados em `tests/analysis/` e removidos ao finalizar.

Execute a limpeza com:

```
python scripts/cleanup_test_artifacts.py
```

Arquivos temporários comuns (`*.tmp`, `*.temp`, `*.analysis.json`, `*.analysis.csv`) também são removidos.

## Publicação e atualização via GitHub

- Habilite GitHub Pages apontando para a pasta `docs/` do repositório.
- Publique o instalador em GitHub Releases (asset do release).
- O app checa `latest.json` hospedado em Pages e compara com `APP_VERSION`.

### Estrutura de `latest.json`

Coloque em `docs/latest.json` (será servido por GitHub Pages):

```
{
  "version": "1.1",
  "url": "https://github.com/<owner>/<repo>/releases/download/v1.1/TFM_GCODE_1.1.exe",
  "sha256": "<HASH_SHA256_OPCIONAL>"
}
```

### Passo a passo de release

- Gere o instalador (`.exe`) com seu processo atual.
- Faça upload como asset do release (ex.: tag `v1.1`).
- Calcule o SHA-256 localmente e atualize o `latest.json`:

```
PowerShell:
Get-FileHash "scripts/dist/TFM_GCODE_1.1.exe" -Algorithm SHA256 | Select-Object -ExpandProperty Hash
```

- Atualize o campo `url` para o link direto do asset:
  `https://github.com/<owner>/<repo>/releases/download/<tag>/<asset>`.
- Commit do `docs/latest.json` na branch publicada do Pages.

### Configuração no app

- O app possui `APP_VERSION` e um bloco `update` em `config/config.json`:
  - `enabled`: ativa/desativa o verificador.
  - `check_on_start`: checa ao iniciar.
  - `feed_url`: deixe vazio em dev (usa `docs/latest.json` local). Em produção, defina para o Pages: `https://<owner>.github.io/<repo>/latest.json`.
