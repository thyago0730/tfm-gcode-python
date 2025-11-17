@echo off
REM Build sem splash para evitar sobreposição e artefatos de cor
python -m PyInstaller ^
  TFM_GCODE_nosplash.spec
pause
