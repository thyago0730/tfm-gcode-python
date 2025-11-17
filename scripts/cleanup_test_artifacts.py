"""
Cleanup de artefatos de análise de testes.

Remove arquivos gerados em testes/analysis e outros padrões voláteis
após execução de testes ou análises locais.
"""
from pathlib import Path
import os
import shutil


def remove_dir_contents(dir_path: Path):
    if not dir_path.exists():
        return
    for entry in dir_path.iterdir():
        try:
            if entry.is_file():
                entry.unlink(missing_ok=True)
            elif entry.is_dir():
                shutil.rmtree(entry, ignore_errors=True)
        except Exception:
            # Mantém robustez; ignora falhas pontuais
            pass


def cleanup(project_root: Path | None = None):
    project_root = project_root or Path(__file__).resolve().parents[1]
    analysis_dir = project_root / 'tests' / 'analysis'
    remove_dir_contents(analysis_dir)

    # Limpeza adicional: arquivos temporários comuns
    patterns = ['*.tmp', '*.temp', '*.analysis.json', '*.analysis.csv']
    for pattern in patterns:
        for p in project_root.rglob(pattern):
            try:
                p.unlink(missing_ok=True)
            except Exception:
                pass

    # Opcional: limpar diretório vazio
    try:
        if analysis_dir.exists() and not any(analysis_dir.iterdir()):
            # mantém .gitkeep
            pass
    except Exception:
        pass


def main():
    cleanup()


if __name__ == '__main__':
    main()
