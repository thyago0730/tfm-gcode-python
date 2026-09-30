Testes automatizados

Execute a partir da raiz do repositório:

```
pip install pytest
python -m pytest -q
```

Os testes importam `src/app/gcode_generator.py` e `src/app/safe_eval.py`, que não dependem de Tkinter,
então rodam em qualquer máquina (inclusive no CI em `.github/workflows/tests.yml`).
`tests/test_core.py` contém as verificações principais (estrutura do programa, números finitos,
bloqueio de expressões inseguras nas fórmulas).
