# Compila, copia para %LOCALAPPDATA%\Programs\lancador, poe no Run do HKCU e reinicia.
# A copia existe para o cargo build nao esbarrar no exe em uso.
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
cargo build --release
if ($LASTEXITCODE) { exit $LASTEXITCODE }

$destino = "$env:LOCALAPPDATA\Programs\lancador"
$exe = "$destino\lancador.exe"
New-Item -ItemType Directory -Force $destino | Out-Null
if (Test-Path $exe) {
    & $exe --sair
    Get-Process lancador -ErrorAction SilentlyContinue | Wait-Process -Timeout 5 -ErrorAction SilentlyContinue
}
Copy-Item target\release\lancador.exe $exe -Force
Set-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name lancador -Value "`"$exe`""
# Pelo explorer, para nao ficar filho deste terminal.
explorer.exe $exe
