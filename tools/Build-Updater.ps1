param([string]$SourceRoot = (Split-Path $PSScriptRoot -Parent))
$ErrorActionPreference='Stop'
$taskPreviousFlags=$env:RUSTFLAGS
try {
    $env:RUSTFLAGS=($taskPreviousFlags+' -C target-feature=+crt-static').Trim()
    & cargo build --manifest-path (Join-Path $SourceRoot 'tools/updater-helper/Cargo.toml') --release --locked --target-dir (Join-Path $SourceRoot 'src-tauri/target')
    if($LASTEXITCODE -ne 0){throw 'Native updater build failed'}
} finally { $env:RUSTFLAGS=$taskPreviousFlags }
$taskOutput=Join-Path $SourceRoot 'src-tauri/resources/updater'
New-Item -ItemType Directory -Force -Path $taskOutput | Out-Null
Copy-Item -LiteralPath (Join-Path $SourceRoot 'src-tauri/target/release/cgs-updater.exe') -Destination (Join-Path $taskOutput 'CgsUpdater.exe') -Force
