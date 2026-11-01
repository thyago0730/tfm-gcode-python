# Compila o TFM G-Code Generator em release, atualiza dist\TFM_GCODE.exe e abre
# o programa. O exe em dist\ usa o config\config.json, os procedimentos e os
# logs da raiz do repositório (a pasta pai), igual ao app instalado.
#
# Uso: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\run_app.ps1
#      (ou dois cliques em scripts\run_app.cmd)
# Opções: -SemAbrir   só compila e atualiza dist, sem abrir o programa
#         -SemBuild   só abre o exe que já está em dist
param(
    [switch]$SemAbrir,
    [switch]$SemBuild
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$dist = Join-Path $root 'dist'
$exe = Join-Path $dist 'TFM_GCODE.exe'
Set-Location $root

if (-not $SemBuild) {
    Write-Host '== cargo build --release -p tfm-app =='
    # O cargo escreve o progresso em stderr; com 'Stop' o PowerShell 5.1
    # trataria isso como erro quando a saída é redirecionada.
    $eap = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    cargo build --release -p tfm-app
    $ErrorActionPreference = $eap
    if ($LASTEXITCODE -ne 0) {
        Write-Host 'Build falhou; nada foi copiado nem aberto.'
        exit 1
    }

    # Uma instância aberta a partir de dist\ mantém o exe em uso: pede o
    # fechamento normal (salva as preferências) e espera até 10 s.
    $abertos = Get-Process TFM_GCODE -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -eq $exe }
    foreach ($p in $abertos) {
        Write-Host ("Fechando a instância aberta (PID {0})..." -f $p.Id)
        $p.CloseMainWindow() | Out-Null
        if (-not $p.WaitForExit(10000)) {
            Write-Host 'A instância não fechou sozinha; encerrando.'
            Stop-Process -Id $p.Id -Force
        }
    }

    New-Item -ItemType Directory -Force $dist | Out-Null
    Copy-Item (Join-Path $root 'target\release\TFM_GCODE.exe') $exe -Force
    $commit = (git rev-parse --short HEAD 2>$null)
    Write-Host ("dist\TFM_GCODE.exe atualizado (build de {0})." -f $commit)
}

if (-not $SemAbrir) {
    if (-not (Test-Path $exe)) {
        Write-Host "Não existe $exe; rode sem -SemBuild."
        exit 1
    }
    Start-Process -FilePath $exe -WorkingDirectory $dist
    Write-Host 'TFM G-Code Generator aberto.'
}
