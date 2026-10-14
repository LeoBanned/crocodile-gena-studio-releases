param([ValidateSet('portable', 'setup', 'all')][string]$Target = 'portable')
$ErrorActionPreference = 'Stop'
$taskRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location $taskRoot
try {
    if ($env:OS -ne 'Windows_NT') { throw 'This helper builds Windows x64. See BUILD.md.' }
    foreach ($taskCommand in @('node', 'npm.cmd', 'cargo', 'rustup')) {
        if (-not (Get-Command $taskCommand -ErrorAction SilentlyContinue)) { throw "Missing $taskCommand. Install prerequisites from BUILD.md and reopen PowerShell." }
    }
    & npm.cmd ci
    if ($LASTEXITCODE -ne 0) { throw 'npm ci failed. Check internet access and package-lock.json.' }
    & npm.cmd run doctor
    if ($LASTEXITCODE -ne 0) { throw 'Preflight failed. Fix the messages above.' }
    if ($Target -eq 'portable') { & npm.cmd run build:portable }
    elseif ($Target -eq 'setup') { & npm.cmd run build:setup }
    else { & npm.cmd run build -- --ci }
    if ($LASTEXITCODE -ne 0) { throw 'Build failed. See the compiler output above and BUILD.md.' }
    Write-Host ''
    Write-Host 'Ready: src-tauri\target\release\crocodile-gena-studio.exe' -ForegroundColor Green
    if ($Target -ne 'portable') { Write-Host 'Installers: src-tauri\target\release\bundle\' -ForegroundColor Green }
} catch {
    Write-Host $_.Exception.Message -ForegroundColor Red
    exit 1
} finally { Pop-Location }
