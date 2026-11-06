# Gera o instalador do app Rust: compila o release, atualiza dist\TFM_GCODE.exe e
# roda o Inno Setup sobre scripts\instalador.iss. Resultado:
# dist\TFM_GCODE_Instalador_<versao>.exe
$ErrorActionPreference = 'Stop'
$raiz = Split-Path -Parent $PSScriptRoot
$dist = Join-Path $raiz 'dist'

$iscc = @(
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw 'Inno Setup 6 nao encontrado. Instale de https://jrsoftware.org/isdl.php' }

$versao = (Select-String -Path (Join-Path $raiz 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value

Write-Host '== cargo build --release -p tfm-app =='
Push-Location $raiz
$old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
cargo build --release -p tfm-app
$code = $LASTEXITCODE
$ErrorActionPreference = $old
Pop-Location
if ($code -ne 0) { throw "cargo falhou ($code)" }

New-Item -ItemType Directory -Force $dist | Out-Null
$exe = Join-Path $dist 'TFM_GCODE.exe'
Copy-Item (Join-Path $raiz 'target\release\TFM_GCODE.exe') $exe -Force

Write-Host "== Inno Setup, versao $versao =="
& $iscc "/DVersao=$versao" "/DExeOrigem=$exe" "/DSaida=$dist" (Join-Path $PSScriptRoot 'instalador.iss')
if ($LASTEXITCODE -ne 0) { throw "ISCC falhou ($LASTEXITCODE)" }

$saida = Join-Path $dist "TFM_GCODE_Instalador_$versao.exe"
"Instalador: $saida ($([math]::Round((Get-Item $saida).Length / 1MB, 1)) MB)"
