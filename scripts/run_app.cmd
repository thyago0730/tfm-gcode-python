@echo off
rem Dois cliques aqui: compila em release, atualiza dist\TFM_GCODE.exe e abre o programa.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0run_app.ps1" %*
if errorlevel 1 pause
