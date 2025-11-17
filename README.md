# TFM G-Code Generator

Ferramenta desktop Windows para geração, visualização e análise de G-code (Tkinter + Matplotlib), com atualização automática via GitHub Pages.

## Publicar no GitHub

- Crie um repositório (ex.: `tfm-gcode-python`) no GitHub.
- Inicialize o git local e faça o primeiro push:
  - `git init`
  - `git add .`
  - `git commit -m "Initial publish"`
  - `git branch -M main`
  - `git remote add origin https://github.com/<owner>/tfm-gcode-python.git`
  - `git push -u origin main`

Alternativa com GitHub CLI:
- `gh repo create <owner>/tfm-gcode-python --public --source . --remote origin --push`

## Atualização automática via Pages

- Habilite GitHub Pages para servir a pasta `docs/`.
- Publique o instalador em GitHub Releases e gere `docs/latest.json` com `scripts/make_latest_json.py`.
- Configure `config/update.feed_url` para `https://<owner>.github.io/<repo>/latest.json`.

## Build do instalador

- Use PyInstaller com os `.spec` do diretório `scripts/` ou `TFM_GCODE.spec`.
- O installer gerado deve ser publicado como asset no Release correspondente.

Para mais detalhes, veja `docs/README.md`.
