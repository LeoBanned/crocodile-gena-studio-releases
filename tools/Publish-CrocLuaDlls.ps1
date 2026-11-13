
[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [string]$DllFolder = (Join-Path ([Environment]::GetFolderPath('Desktop')) 'ffedw'),
    [switch]$Preview
)

$ErrorActionPreference = 'Stop'
$project = Split-Path $PSScriptRoot -Parent
$feedUrl = 'https://raw.githubusercontent.com/LeoBanned/crocodile-gena-studio-releases/croclua-dll-feed/latest.json'
$names = @('OpenSteamTool.dll', 'dwmapi.dll', 'xinput1_4.dll')
$source = (Resolve-Path -LiteralPath $DllFolder).ProviderPath

function Get-Sha256([string]$Path) {
    $hasher = [Security.Cryptography.SHA256]::Create()
    $stream = [IO.File]::OpenRead($Path)
    try { ([BitConverter]::ToString($hasher.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() }
    finally { $stream.Dispose(); $hasher.Dispose() }
}

$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$temp = Join-Path $tempRoot ('cgs-dll-publish-' + [guid]::NewGuid().ToString('N'))
$utf8 = New-Object Text.UTF8Encoding($false)
$staging = Join-Path $temp 'dlls'
New-Item -ItemType Directory -Path $staging -Force | Out-Null
try {
    $hashes = [ordered]@{}
    foreach ($name in $names) {
        $path = Join-Path $source $name
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Файл не найден: $path" }
        $staged = Join-Path $staging $name
        Copy-Item -LiteralPath $path -Destination $staged
        $length = (Get-Item -LiteralPath $staged).Length
        if ($length -lt 64 -or $length -gt 16MB) { throw "Недопустимый размер DLL: $name" }
        $bytes = [IO.File]::ReadAllBytes($staged)
        $offset = [BitConverter]::ToUInt32($bytes, 60)
        if ($bytes[0] -ne 77 -or $bytes[1] -ne 90 -or $offset -gt ($bytes.Length - 24) -or
            [BitConverter]::ToUInt32($bytes, $offset) -ne 17744 -or ($bytes[$offset + 23] -band 32) -eq 0) {
            throw "Файл не является Windows DLL: $name"
        }
        $hashes[$name] = Get-Sha256 $staged
    }

    $freshUrl = "$feedUrl" + '?check=' + [DateTime]::UtcNow.Ticks
    $current = Invoke-RestMethod -Uri $freshUrl -UseBasicParsing -TimeoutSec 15
    if ($current -is [string]) { $current = $current | ConvertFrom-Json }
    if ($current.revision -lt 1 -or $current.revision -ge [uint32]::MaxValue) { throw 'Недопустимый номер опубликованного комплекта.' }
    $revision = [uint32]($current.revision + 1)
    $archiveName = "dll-$revision.zip"
    if ($Preview) { Write-Host "Будет собрана ревизия $revision."; return }

    $archivePath = Join-Path $temp $archiveName
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [IO.Compression.ZipFile]::CreateFromDirectory($staging, $archivePath)
    $manifest = [ordered]@{
        revision = $revision
        sha256 = Get-Sha256 $archivePath
        files = $hashes
    }
    $output = Join-Path $project "build/croclua-dll-upload/revision-$revision"
    New-Item -ItemType Directory -Path $output -Force | Out-Null
    Copy-Item -LiteralPath $archivePath -Destination (Join-Path $output $archiveName) -Force
    [IO.File]::WriteAllText((Join-Path $output 'latest.json'), ($manifest | ConvertTo-Json -Depth 3) + "`n", $utf8)
    Write-Host "Готово: $output"
    Write-Host "Загрузите $archiveName и latest.json одним коммитом в ветку croclua-dll-feed."

} finally {
    $resolved = (Resolve-Path -LiteralPath $temp).ProviderPath
    if ([IO.Path]::GetDirectoryName($resolved) -eq $tempRoot.TrimEnd('\', '/') -and
        [IO.Path]::GetFileName($resolved) -like 'cgs-dll-publish-*') {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
