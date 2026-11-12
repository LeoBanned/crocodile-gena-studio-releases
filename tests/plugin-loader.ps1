$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskBackend = Join-Path $taskRoot 'src-tauri/src/assets/crocodile-manifest/backend'
$taskResources = Join-Path $taskBackend 'loader'
$taskInstaller = Join-Path $taskBackend 'install-loader.ps1'
$taskTemp = Join-Path ([IO.Path]::GetTempPath()) ('cgs-plugin-loader-test-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($taskTemp) | Out-Null
[IO.File]::WriteAllBytes((Join-Path $taskTemp 'steam.exe'), [byte[]]@())
function Assert-Test($Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
function Hash-Test([string]$Path) { return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash }

[IO.File]::WriteAllText((Join-Path $taskTemp 'opensteamtool.toml'), "# custom`n[general]`nlog = true`n[lua]`npaths = [`n  'config/stplug-in', # old folder`n]`nother = 'keep'`n[cloud]`nenabled = true`nlibrary = 'cgs_cloud_redirect.dll'`n")
[IO.File]::WriteAllText((Join-Path $taskTemp 'OpenSteamTool.dll'), 'old loader')
[IO.Directory]::CreateDirectory((Join-Path $taskTemp 'config/stplug-in')) | Out-Null
[IO.File]::WriteAllText((Join-Path $taskTemp 'config/stplug-in/123.lua'), 'legacy Lua')
& $taskInstaller -SteamPath $taskTemp -ResourcePath $taskResources
foreach ($taskName in @('OpenSteamTool.dll', 'dwmapi.dll', 'xinput1_4.dll')) {
    Assert-Test ((Hash-Test (Join-Path $taskTemp $taskName)) -eq (Hash-Test (Join-Path $taskResources $taskName))) "Wrong DLL: $taskName"
    if (Test-Path -LiteralPath (Join-Path $taskRoot "CrocLua/$taskName")) {
        Assert-Test ((Hash-Test (Join-Path $taskResources $taskName)) -eq (Hash-Test (Join-Path $taskRoot "CrocLua/$taskName"))) "DLL does not match supplied CrocLua source: $taskName"
    }
}
$taskConfig = [IO.File]::ReadAllText((Join-Path $taskTemp 'opensteamtool.toml'))
Assert-Test ($taskConfig.Contains('config/lua') -and $taskConfig.Contains("other = 'keep'") -and $taskConfig.Contains('# custom')) 'Config settings lost'
Assert-Test ($taskConfig.Contains("[cloud]`nenabled = true`nlibrary = 'cgs_cloud_redirect.dll'")) 'Plugin installation lost local cloud settings'
Assert-Test (Test-Path -LiteralPath (Join-Path $taskTemp 'config/lua') -PathType Container) 'Lua folder missing'
$taskBackups = @(Get-ChildItem -LiteralPath (Join-Path $taskTemp 'config/cgs-luatools-backups') -Directory)
Assert-Test ($taskBackups.Count -eq 1) 'Missing backup'
Assert-Test ([IO.File]::ReadAllText((Join-Path $taskBackups[0].FullName 'OpenSteamTool.dll')) -eq 'old loader') 'Original DLL lost'
[IO.File]::Move((Join-Path $taskTemp 'config/lua/123.lua'), (Join-Path $taskTemp 'config/lua/123.lua.disabled'))
$taskDllTimes = @{}
foreach ($taskName in @('OpenSteamTool.dll', 'dwmapi.dll', 'xinput1_4.dll')) {
    $taskDllTimes[$taskName] = [IO.File]::GetLastWriteTimeUtc((Join-Path $taskTemp $taskName))
}
& $taskInstaller -SteamPath $taskTemp -ResourcePath $taskResources
foreach ($taskName in @('OpenSteamTool.dll', 'dwmapi.dll', 'xinput1_4.dll')) {
    Assert-Test ([IO.File]::GetLastWriteTimeUtc((Join-Path $taskTemp $taskName)) -eq $taskDllTimes[$taskName]) "Current plugin DLL rewritten: $taskName"
}
Assert-Test (@(Get-ChildItem -LiteralPath (Join-Path $taskTemp 'config/cgs-luatools-backups') -Directory).Count -eq 1) 'Repeated install created backup'
Assert-Test (-not [IO.File]::Exists((Join-Path $taskTemp 'config/lua/123.lua'))) 'Disabled Lua resurrected'
[IO.File]::Delete((Join-Path $taskTemp 'config/lua/123.lua.disabled'))
& $taskInstaller -SteamPath $taskTemp -ResourcePath $taskResources
Assert-Test (-not [IO.File]::Exists((Join-Path $taskTemp 'config/lua/123.lua'))) 'Deleted Lua resurrected'

# A locked second DLL must roll back the first replacement and preserve all originals.
[IO.File]::WriteAllText((Join-Path $taskTemp 'OpenSteamTool.dll'), 'before rollback')
[IO.File]::WriteAllText((Join-Path $taskTemp 'dwmapi.dll'), 'locked DLL')
$taskLock = [IO.File]::Open((Join-Path $taskTemp 'dwmapi.dll'), [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
$taskFailed = $false
try { & $taskInstaller -SteamPath $taskTemp -ResourcePath $taskResources }
catch { $taskFailed = $true }
finally { $taskLock.Dispose() }
Assert-Test $taskFailed 'Locked DLL unexpectedly replaced'
Assert-Test ([IO.File]::ReadAllText((Join-Path $taskTemp 'OpenSteamTool.dll')) -eq 'before rollback') 'Rollback failed'
Assert-Test ([IO.File]::ReadAllText((Join-Path $taskTemp 'dwmapi.dll')) -eq 'locked DLL') 'Locked original lost'

# Refuse unsupported configuration instead of silently destroying it.
$taskInvalid = '[lua]' + "`npaths = 123`n"
[IO.File]::WriteAllText((Join-Path $taskTemp 'opensteamtool.toml'), $taskInvalid)
$taskFailed = $false
try { & $taskInstaller -SteamPath $taskTemp -ResourcePath $taskResources }
catch { $taskFailed = $true }
Assert-Test $taskFailed 'Invalid paths accepted'
Assert-Test ([IO.File]::ReadAllText((Join-Path $taskTemp 'opensteamtool.toml')) -eq $taskInvalid) 'Invalid config overwritten'
$taskInline = "lua = { paths = [`"config/lua`"], other = true }`n[general]`nlog = true`n"
[IO.File]::WriteAllText((Join-Path $taskTemp 'opensteamtool.toml'), $taskInline)
& $taskInstaller -SteamPath $taskTemp -ResourcePath $taskResources
Assert-Test ([IO.File]::ReadAllText((Join-Path $taskTemp 'opensteamtool.toml')) -eq $taskInline) 'Configured inline table changed'
$taskQuoted = '["lua"]' + "`n" + '"paths" = ["config/stplug-in"]' + "`n"
[IO.File]::WriteAllText((Join-Path $taskTemp 'opensteamtool.toml'), $taskQuoted)
& $taskInstaller -SteamPath $taskTemp -ResourcePath $taskResources
$taskQuotedResult = [IO.File]::ReadAllText((Join-Path $taskTemp 'opensteamtool.toml'))
Assert-Test ($taskQuotedResult.Contains('config/lua') -and -not $taskQuotedResult.Contains('[lua]')) 'Quoted Lua table duplicated'
$taskFresh = Join-Path $taskTemp 'fresh'
[IO.Directory]::CreateDirectory($taskFresh) | Out-Null
[IO.File]::WriteAllBytes((Join-Path $taskFresh 'steam.exe'), [byte[]]@())
& $taskInstaller -SteamPath $taskFresh -ResourcePath $taskResources
Assert-Test ([IO.File]::ReadAllText((Join-Path $taskFresh 'opensteamtool.toml')).Contains('config/lua')) 'Fresh config incorrect'
# Existing version 2 is detected even without a preference created by the application.
$taskAlternate = Join-Path $taskTemp 'alternate'
[IO.Directory]::CreateDirectory($taskAlternate) | Out-Null
[IO.File]::WriteAllBytes((Join-Path $taskAlternate 'steam.exe'), [byte[]]@())
foreach ($taskName in @('OpenSteamTool.dll','dwmapi.dll','xinput1_4.dll')) {
    [IO.File]::Copy((Join-Path $taskResources ('compatibility\' + $taskName)), (Join-Path $taskAlternate $taskName))
}
& $taskInstaller -SteamPath $taskAlternate -ResourcePath $taskResources
$taskPinPath = Join-Path $taskAlternate 'config/cgs-luatools-loader.json'
$taskPin = [IO.File]::ReadAllText($taskPinPath) | ConvertFrom-Json
Assert-Test ($taskPin.variant -ceq 'compatibility') 'Existing alternative version was not pinned'
Assert-Test ($taskPin.schema -eq 2) 'Old compatibility layout was not migrated'
Assert-Test (-not [IO.File]::Exists((Join-Path $taskAlternate 'dwmapi.dll'))) 'Old compatibility proxy was left in the Steam root'
$taskTargets = [ordered]@{
    'OpenSteamTool.dll' = 'OpenSteamTool.dll'
    'dwmapi.dll' = 'bin/dwmapi.dll'
    'xinput1_4.dll' = 'xinput1_4.dll'
}
foreach ($taskName in $taskTargets.Keys) {
    Assert-Test ((Hash-Test (Join-Path $taskAlternate $taskTargets[$taskName])) -eq (Hash-Test (Join-Path $taskResources $taskName))) "Wrong compatibility DLL: $taskName"
}

# Simulate an older pinned installation with one changed DLL and one missing DLL.
$taskOldMain = Join-Path $taskAlternate 'OpenSteamTool.dll'
[IO.File]::WriteAllText($taskOldMain, 'older pinned release')
$taskPin.hashes.'OpenSteamTool.dll' = (Hash-Test $taskOldMain).ToLowerInvariant()
[IO.File]::WriteAllText($taskPinPath, ($taskPin | ConvertTo-Json -Depth 4), (New-Object Text.UTF8Encoding($false)))
[IO.File]::Delete((Join-Path $taskAlternate 'xinput1_4.dll'))
$taskProxy = Join-Path $taskAlternate 'bin/dwmapi.dll'
$taskProxyTime = [IO.File]::GetLastWriteTimeUtc($taskProxy)
& $taskInstaller -SteamPath $taskAlternate -ResourcePath $taskResources
Assert-Test ((Hash-Test $taskOldMain) -eq (Hash-Test (Join-Path $taskResources 'OpenSteamTool.dll'))) 'Old pinned DLL was not upgraded'
Assert-Test ((Hash-Test (Join-Path $taskAlternate 'xinput1_4.dll')) -eq (Hash-Test (Join-Path $taskResources 'xinput1_4.dll'))) 'Missing DLL was not restored'
Assert-Test ([IO.File]::GetLastWriteTimeUtc($taskProxy) -eq $taskProxyTime) 'Matching DLL was rewritten'

[IO.File]::WriteAllText($taskPinPath, '{bad json')
$taskFailed = $false
try { & $taskInstaller -SteamPath $taskAlternate -ResourcePath $taskResources } catch { $taskFailed = $true }
Assert-Test $taskFailed 'Corrupt selection silently reset to version one'
Write-Host "PASS: offline DLL installation, both layouts, automatic upgrades, repair, config preservation, backups, idempotence, rollback and corrupt selection. Temporary test folder: $taskTemp"
