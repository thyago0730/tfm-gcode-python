"""Gera docs/latest.json para atualização automática.

Uso:
  python scripts/make_latest_json.py --version 1.1 --asset-url https://github.com/<owner>/<repo>/releases/download/v1.1/TFM_GCODE_1.1.exe --installer-path scripts/dist/TFM_GCODE_1.1.exe
"""
from __future__ import annotations
import argparse
import hashlib
from pathlib import Path
import json


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open('rb') as f:
        for chunk in iter(lambda: f.read(8192), b''):
            h.update(chunk)
    return h.hexdigest()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--version', required=True, help='Versão remota ex.: 1.1')
    ap.add_argument('--asset-url', required=True, help='URL direta do instalador em GitHub Releases')
    ap.add_argument('--installer-path', required=True, help='Caminho local do instalador para calcular sha256')
    args = ap.parse_args()

    project_root = Path(__file__).resolve().parents[1]
    docs_dir = project_root / 'docs'
    docs_dir.mkdir(parents=True, exist_ok=True)
    latest_path = docs_dir / 'latest.json'

    installer = Path(args.installer_path)
    if not installer.exists():
        raise FileNotFoundError(f'Installer não encontrado: {installer}')

    digest = sha256_file(installer)
    data = {
        'version': str(args.version),
        'url': str(args.asset_url),
        'sha256': digest,
    }
    latest_path.write_text(json.dumps(data, indent=2), encoding='utf-8')
    print(f'[release] Escrito {latest_path} com sha256 {digest}')


if __name__ == '__main__':
    main()

