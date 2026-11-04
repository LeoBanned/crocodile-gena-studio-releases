param([string]$SourceRoot = (Split-Path $PSScriptRoot -Parent))
$ErrorActionPreference = 'Stop'
# Run from an x64 MSVC developer environment; CMake/.NET are not required.
$taskSource = Join-Path $SourceRoot 'third_party/cloudredirect'
$taskBuild = Join-Path $SourceRoot 'build/local-cloud'
$taskOutput = Join-Path $SourceRoot 'src-tauri/resources/local-cloud'
New-Item -ItemType Directory -Force -Path $taskBuild,$taskOutput | Out-Null
$taskCmake = [IO.File]::ReadAllText((Join-Path $taskSource 'CMakeLists.txt'))
$taskFiles = @()
foreach ($taskSection in @('COMMON_SOURCES','PROVIDER_SOURCES','PLATFORM_SOURCES')) {
    $taskMatch = [regex]::Match($taskCmake, '(?s)set\(' + $taskSection + '\s+(.*?)\)')
    if (-not $taskMatch.Success) { throw "Missing source list $taskSection" }
    $taskFiles += [regex]::Matches($taskMatch.Groups[1].Value, 'src/[\w/]+\.(?:cpp|c)\b') | ForEach-Object { Join-Path $taskSource $_.Value }
}
$taskArgs = @('/nologo','/LD','/O2','/MP4','/MT','/EHsc','/std:c++20','/utf-8','/DWIN32','/D_WINDOWS','/DMINIZ_NO_STDIO',('/FI"'+$taskSource+'/src/common/cgs_version.h"'),('/I"'+$taskSource+'/src/common"'),('/I"'+$taskSource+'/src/providers"'),('/I"'+$taskSource+'/src/platform/win"'))
$taskArgs += $taskFiles | ForEach-Object { '"' + $_ + '"' }
$taskArgs += @('/link',('/OUT:"'+$taskOutput+'/cgs_cloud_redirect.dll"'),('/IMPLIB:"'+$taskBuild+'/cgs_cloud_redirect.lib"'),'Shlwapi.lib','Advapi32.lib','ws2_32.lib','winhttp.lib','Crypt32.lib','Shell32.lib','Ole32.lib','Psapi.lib','User32.lib')
$taskRsp = Join-Path $taskBuild 'compile.rsp'
[IO.File]::WriteAllText($taskRsp,($taskArgs -join ' '),[Text.UTF8Encoding]::new($false))
Push-Location $taskBuild
try { & cl.exe ('@'+$taskRsp); if ($LASTEXITCODE -ne 0) { throw 'Local cloud native build failed' } } finally { Pop-Location }
Copy-Item -LiteralPath (Join-Path $taskSource 'LICENSE') -Destination (Join-Path $taskOutput 'LICENSE.txt') -Force
