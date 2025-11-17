"""Hook do pytest para limpeza automática de artefatos de análise.

Ao finalizar a sessão de testes, remove conteúdos de `tests/analysis/` e
arquivos temporários comuns no projeto.
"""
from pathlib import Path


def pytest_sessionfinish(session, exitstatus):
    try:
        project_root = Path(__file__).resolve().parents[1]
        # Importa dinamicamente o script de limpeza
        import sys
        sys.path.append(str(project_root / 'scripts'))
        from cleanup_test_artifacts import cleanup
        cleanup(project_root)
        print("[pytest cleanup] Artefatos de análise limpos.")
    except Exception as e:
        print(f"[pytest cleanup] Falha ao executar limpeza: {e}")

