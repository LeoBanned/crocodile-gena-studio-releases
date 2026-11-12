param(
    [Parameter(Mandatory=$true)][string]$SteamPath,
    [Parameter(Mandatory=$true)][string]$ResourcePath
)
$ErrorActionPreference = 'Stop'
$taskSteam = (Resolve-Path -LiteralPath $SteamPath).ProviderPath
$taskResources = (Resolve-Path -LiteralPath $ResourcePath).ProviderPath
if (-not (Test-Path -LiteralPath (Join-Path $taskSteam 'steam.exe') -PathType Leaf)) {
    throw 'steam.exe not found. Loader files were not changed.'
}

function Read-OptionalBytes([string]$Path) {
    $taskAncestor = Split-Path -Parent $Path
    while ($taskAncestor -and $taskAncestor -ne $taskSteam -and $taskAncestor.StartsWith($taskSteam, [StringComparison]::OrdinalIgnoreCase)) {
        if ((Test-Path -LiteralPath $taskAncestor) -and ((Get-Item -LiteralPath $taskAncestor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Loader path contains a link.' }
        $taskAncestor = Split-Path -Parent $taskAncestor
    }
    if (Test-Path -LiteralPath $Path) {
        if ((Get-Item -LiteralPath $Path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw "Refusing to replace a link: $Path"
        }
        return ,([IO.File]::ReadAllBytes($Path))
    }
    return $null
}
function Same-Bytes($Left, $Right) {
    if ($null -eq $Left -or $null -eq $Right) { return ($null -eq $Left -and $null -eq $Right) }
    if ($Left.Length -ne $Right.Length) { return $false }
    $taskHash = [Security.Cryptography.SHA256]::Create()
    try { return [Convert]::ToBase64String($taskHash.ComputeHash($Left)) -eq [Convert]::ToBase64String($taskHash.ComputeHash($Right)) }
    finally { $taskHash.Dispose() }
}
function Write-Atomic([string]$Path, [byte[]]$Bytes) {
    $taskTemporary = $Path + '.' + [guid]::NewGuid().ToString('N') + '.tmp'
    try {
        [IO.File]::WriteAllBytes($taskTemporary, $Bytes)
        if ([IO.File]::Exists($Path)) { [IO.File]::Replace($taskTemporary, $Path, [NullString]::Value) }
        else { [IO.File]::Move($taskTemporary, $Path) }
    } finally {
        if ([IO.File]::Exists($taskTemporary)) { [IO.File]::Delete($taskTemporary) }
    }
}

# Preserve unrelated settings; refuse unusual Lua syntax instead of resetting a custom config.
# The application's TOML parser supports broader custom configurations during plugin installation.
function Configure-Lua([string]$Content) {
    if ([string]::IsNullOrWhiteSpace($Content)) {
        return [IO.File]::ReadAllText((Join-Path $taskResources 'opensteamtool.toml'))
    }
    $taskSectionPattern = '(?ms)^[ \t]*\[[ \t]*(?:lua|"lua"|''lua'')[ \t]*\][ \t]*(?:#[^\r\n]*)?\r?\n(?<body>.*?)(?=^[ \t]*\[|\z)'
    $taskSections = [regex]::Matches($Content, $taskSectionPattern)
    if ($taskSections.Count -eq 0) {
        # The native TOML editor preserves inline tables. Accept an already configured
        # inline Lua table without rewriting or dropping its other fields.
        if ($Content -match '(?m)^[ \t]*lua[ \t]*=[ \t]*\{[^}\r\n]*\bpaths[ \t]*=[ \t]*\[[ \t]*["'']config/lua["''][ \t]*,?[ \t]*\]') {
            return $Content
        }
        if ($Content -match '(?m)^\s*(?:(?:lua|"lua"|''lua'')\s*=|\[\s*(?:lua|"lua"|''lua'')\s*\])') {
            throw 'Unsupported Lua configuration. Update the plugin from CrocLua; the existing config is preserved.'
        }
        return $Content.TrimEnd() + "`n`n[lua]`npaths = [`"config/lua`"]`n"
    }
    if ($taskSections.Count -ne 1) { throw 'Duplicate Lua sections. Config was not changed.' }
    $taskSection = $taskSections[0]
    $taskBody = $taskSection.Groups['body'].Value
    $taskPathsPattern = '(?ms)^[ \t]*(?:paths|"paths"|''paths'')[ \t]*=[ \t]*\[(?<values>(?:[^"''\[\]]|"(?:\\.|[^"\\])*"|''[^'']*'')*)\][ \t]*(?:#[^\r\n]*)?(?:\r?\n|\z)'
    $taskPaths = [regex]::Matches($taskBody, $taskPathsPattern)
    if ($taskPaths.Count -gt 1) { throw 'Duplicate Lua paths. Config was not changed.' }
    if ($taskPaths.Count -eq 0) {
        if ($taskBody -match '(?m)^\s*["'']?paths["'']?\s*=') {
            throw 'Unsupported Lua paths. Update from CrocLua; config was not changed.'
        }
        $taskNewBody = "paths = [`"config/lua`"]`n" + $taskBody
    } else {
        $taskPathMatch = $taskPaths[0]
        $taskValues = [regex]::Replace($taskPathMatch.Groups['values'].Value, '#[^\r\n]*', '')
        if ($taskValues -notmatch '^\s*(?:(?:"(?:\\.|[^"\\])*"|''[^'']*'')\s*,\s*)*(?:(?:"(?:\\.|[^"\\])*"|''[^'']*'')\s*,?\s*)?$') {
            throw 'Invalid Lua paths. Config was not changed.'
        }
        $taskNewBody = $taskBody.Remove($taskPathMatch.Index, $taskPathMatch.Length).Insert($taskPathMatch.Index, "paths = [`"config/lua`"]`n")
    }
    $taskBodyStart = $taskSection.Groups['body'].Index
    return $Content.Remove($taskBodyStart, $taskBody.Length).Insert($taskBodyStart, $taskNewBody)
}

# The application and plugin share an OS-held file lock and a pinned SHA-256 selection.
# The lock file stays on disk; closing its handle releases the lock even after a crash.
$taskConfigDir = Join-Path $taskSteam 'config'
if ((Test-Path -LiteralPath $taskConfigDir) -and ((Get-Item -LiteralPath $taskConfigDir -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw 'Steam config is a link. Loader files were not changed.'
}
[IO.Directory]::CreateDirectory($taskConfigDir) | Out-Null
$taskLockPath = Join-Path $taskConfigDir 'cgs-luatools-loader.lock'
[void](Read-OptionalBytes $taskLockPath)
try { $taskLoaderLock = [IO.File]::Open($taskLockPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
catch { throw 'Loader is busy in CrocLua or another plugin operation. Wait and try again.' }
try {
$taskSelectionPath = Join-Path $taskConfigDir 'cgs-luatools-loader.json'
if ((Test-Path -LiteralPath $taskSelectionPath) -and (Get-Item -LiteralPath $taskSelectionPath).Length -gt 16384) { throw 'Invalid loader selection. Choose the DLL version again in CrocLua.' }
$taskOldSelection = Read-OptionalBytes $taskSelectionPath
$taskUtf8 = New-Object Text.UTF8Encoding($false, $true)
function Hash-Bytes([byte[]]$Bytes) {
    $taskHasher = [Security.Cryptography.SHA256]::Create()
    try { return ([BitConverter]::ToString($taskHasher.ComputeHash($Bytes))).Replace('-','').ToLowerInvariant() }
    finally { $taskHasher.Dispose() }
}
function Validate-Selection($Selection) {
    if ($null -eq $Selection -or $Selection.schema -notin @(1,2) -or
        ($Selection.schema -isnot [int] -and $Selection.schema -isnot [long]) -or
        $Selection.variant -cnotin @('standard','compatibility') -or
        @($Selection.PSObject.Properties).Count -ne 3 -or @($Selection.hashes.PSObject.Properties).Count -ne 3) {
        throw 'Unsupported loader selection. Automatic replacement refused. Choose a version in CrocLua.'
    }
    $taskNames = @($Selection.hashes.PSObject.Properties.Name)
    $taskProxy = if ($Selection.schema -eq 1) { 'dwmapi.dll' }
        elseif ($Selection.variant -ceq 'compatibility') { 'bin/dwmapi.dll' }
        elseif ($taskNames -ccontains 'version.dll') { 'version.dll' } else { 'dwmapi.dll' }
    foreach ($taskName in @('OpenSteamTool.dll','xinput1_4.dll',$taskProxy)) {
        if ($taskNames -cnotcontains $taskName -or $Selection.hashes.$taskName -cnotmatch '^[0-9a-f]{64}$') {
            throw 'Invalid pinned DLL path or fingerprint. Automatic replacement refused.'
        }
    }
}
$taskSelection = $null
if ($null -ne $taskOldSelection) {
    try { $taskSelection = $taskUtf8.GetString($taskOldSelection) | ConvertFrom-Json }
    catch { throw 'Invalid loader selection. Automatic replacement refused. Choose a version in CrocLua.' }
    Validate-Selection $taskSelection
}
$taskPrevious = $taskSelection
$taskVariant = if ($null -ne $taskPrevious) { $taskPrevious.variant } else { 'standard' }
if ($null -eq $taskPrevious) {
    $taskLegacyMain = Read-OptionalBytes (Join-Path $taskResources 'compatibility/OpenSteamTool.dll')
    $taskLegacyProxy = Read-OptionalBytes (Join-Path $taskResources 'compatibility/dwmapi.dll')
    $taskInstalledProxy = Read-OptionalBytes (Join-Path $taskSteam 'bin/dwmapi.dll')
    if (($null -ne $taskLegacyMain -and (Same-Bytes (Read-OptionalBytes (Join-Path $taskSteam 'OpenSteamTool.dll')) $taskLegacyMain)) -or
        ($null -ne $taskInstalledProxy -and ((Same-Bytes $taskInstalledProxy $taskLegacyProxy) -or
            (Same-Bytes $taskInstalledProxy (Read-OptionalBytes (Join-Path $taskResources 'dwmapi.dll')))))) {
        $taskVariant = 'compatibility'
    }
}
$taskActiveResources = $taskResources
$taskOnline = Join-Path $taskConfigDir 'cgs-luatools-online'
$taskOnlineManifest = Read-OptionalBytes (Join-Path $taskOnline 'latest.json')
$taskOnlineHashes = $null
if ($null -ne $taskOnlineManifest) {
    $taskOnlineInfo = $taskUtf8.GetString($taskOnlineManifest) | ConvertFrom-Json
    if ($taskOnlineInfo.revision -gt [uint32]([IO.File]::ReadAllText((Join-Path $taskResources 'revision.txt')).Trim())) {
        if (@($taskOnlineInfo.files.PSObject.Properties).Count -ne 3) { throw 'Invalid cached DLL package.' }
        $taskOnlineHashes = $taskOnlineInfo.files
        $taskActiveResources = $taskOnline
    }
}
$taskDllBytes = [ordered]@{}
$taskHashes = [ordered]@{}
foreach ($taskName in @('OpenSteamTool.dll','dwmapi.dll','xinput1_4.dll')) {
    $taskTargetName = if ($taskVariant -eq 'compatibility' -and $taskName -eq 'dwmapi.dll') { 'bin/dwmapi.dll' } else { $taskName }
    $taskBytes = Read-OptionalBytes (Join-Path $taskActiveResources $taskName)
    if ($null -eq $taskBytes -or $taskBytes.Length -gt 16MB -or
        ($null -ne $taskOnlineHashes -and (Hash-Bytes $taskBytes) -cne $taskOnlineHashes.$taskName)) {
        throw "Invalid cached or bundled DLL: $taskName"
    }
    if ($taskBytes.Length -lt 1024 -or $taskBytes[0] -ne 77 -or $taskBytes[1] -ne 90) {
        throw "Invalid bundled DLL: $taskName"
    }
    $taskDllBytes[$taskTargetName] = $taskBytes
    $taskHashes[$taskTargetName] = Hash-Bytes $taskBytes
}
$taskSelection = [pscustomobject]@{schema=2;variant=$taskVariant;hashes=[pscustomobject]$taskHashes}
$taskSelectionBytes = $taskOldSelection
if ($null -eq $taskPrevious -or $taskPrevious.schema -ne $taskSelection.schema -or
    @($taskHashes.Keys | Where-Object { $taskPrevious.hashes.$_ -cne $taskHashes[$_] }).Count -gt 0) {
    $taskSelectionBytes = $taskUtf8.GetBytes(($taskSelection | ConvertTo-Json -Depth 4))
}

# Remove a previous proxy only if its installed bytes still match the old selection.
if ($null -ne $taskPrevious) {
    foreach ($taskProperty in $taskPrevious.hashes.PSObject.Properties) {
        if ($taskHashes.Contains($taskProperty.Name)) { continue }
        $taskInstalled = Read-OptionalBytes (Join-Path $taskSteam $taskProperty.Name)
        if ($null -eq $taskInstalled) { continue }
        if ((Hash-Bytes $taskInstalled) -cne $taskProperty.Value) {
            throw "Previous proxy was modified externally: $($taskProperty.Name)"
        }
        $taskDllBytes[$taskProperty.Name] = $null
    }
} elseif ($taskVariant -eq 'compatibility') {
    $taskLegacyRoot = Read-OptionalBytes (Join-Path $taskSteam 'dwmapi.dll')
    if ($null -ne $taskLegacyRoot -and (Same-Bytes $taskLegacyRoot $taskLegacyProxy)) {
        $taskDllBytes['dwmapi.dll'] = $null
    }
}

$taskConfigPath = Join-Path $taskSteam 'opensteamtool.toml'
$taskOldConfig = Read-OptionalBytes $taskConfigPath
$taskUtf8 = New-Object Text.UTF8Encoding($false, $true)
$taskConfigText = if ($null -ne $taskOldConfig) { $taskUtf8.GetString($taskOldConfig).TrimStart([char]0xfeff) } else { '' }
$taskConfigBytes = $taskUtf8.GetBytes((Configure-Lua $taskConfigText))
$taskChanges = @()
foreach ($taskName in (@($taskDllBytes.Keys) + @('opensteamtool.toml', 'config/cgs-luatools-loader.json'))) {
    $taskTarget = Join-Path $taskSteam $taskName
    $taskOriginal = Read-OptionalBytes $taskTarget
    $taskBytes = if ($taskName -eq 'opensteamtool.toml') { $taskConfigBytes }
        elseif ($taskName -eq 'config/cgs-luatools-loader.json') { $taskSelectionBytes }
        else { $taskDllBytes[$taskName] }
    if (-not (Same-Bytes $taskOriginal $taskBytes)) {
        [IO.Directory]::CreateDirectory((Split-Path -Parent $taskTarget)) | Out-Null
        $taskChanges += [pscustomobject]@{ Name=$taskName; Target=$taskTarget; Original=$taskOriginal; Bytes=$taskBytes }
    }
}
if ($taskChanges.Count -gt 0) {
    $taskBackup = Join-Path $taskSteam ('config\cgs-luatools-backups\' + [guid]::NewGuid().ToString())
    [IO.Directory]::CreateDirectory($taskBackup) | Out-Null
    foreach ($taskChange in $taskChanges) {
        if ($null -ne $taskChange.Original) {
            $taskBackupFile = Join-Path $taskBackup $taskChange.Name
            [IO.Directory]::CreateDirectory((Split-Path -Parent $taskBackupFile)) | Out-Null
            [IO.File]::WriteAllBytes($taskBackupFile, $taskChange.Original)
        }
    }
    $taskCommitted = @()
    try {
        foreach ($taskChange in $taskChanges) {
            if (-not (Same-Bytes (Read-OptionalBytes $taskChange.Target) $taskChange.Original)) {
                throw "File changed during installation: $($taskChange.Name)"
            }
            if ($null -eq $taskChange.Bytes) { [IO.File]::Delete($taskChange.Target) }
            else { Write-Atomic $taskChange.Target $taskChange.Bytes }
            $taskCommitted += $taskChange
        }
    } catch {
        $taskFailure = $_.Exception.Message
        $taskRollbackFailures = @()
        for ($taskIndex=$taskCommitted.Count-1; $taskIndex -ge 0; $taskIndex--) {
            $taskChange = $taskCommitted[$taskIndex]
            try {
                if (-not (Same-Bytes (Read-OptionalBytes $taskChange.Target) $taskChange.Bytes)) {
                    throw 'File changed by another process; rollback skipped.'
                }
                if ($null -eq $taskChange.Original) { [IO.File]::Delete($taskChange.Target) }
                else { Write-Atomic $taskChange.Target $taskChange.Original }
            } catch { $taskRollbackFailures += "$($taskChange.Name): $($_.Exception.Message)" }
        }
        throw "Loader update failed: $taskFailure. Close Steam and update from CrocLua. Backup: $taskBackup. Rollback errors: $($taskRollbackFailures -join '; ')"
    }
}
[IO.Directory]::CreateDirectory((Join-Path $taskSteam 'config\lua')) | Out-Null

# Share CrocLua's one-time migration ledger; never resurrect deleted/disabled Lua.
$taskLegacy = Join-Path $taskSteam 'config\stplug-in'
if ([IO.Directory]::Exists($taskLegacy)) {
    $taskMarker = Join-Path $taskSteam 'config\cgs-luatools-lua-migration.json'
    $taskMigrated = New-Object 'Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
    if ([IO.File]::Exists($taskMarker)) {
        foreach ($taskItem in @([IO.File]::ReadAllText($taskMarker) | ConvertFrom-Json)) {
            [void]$taskMigrated.Add([string]$taskItem)
        }
    }
    foreach ($taskSource in Get-ChildItem -LiteralPath $taskLegacy -File) {
        if ($taskSource.Name -notmatch '(?i)\.lua(?:\.disabled)?$') { continue }
        if ($taskSource.Attributes -band [IO.FileAttributes]::ReparsePoint) { continue }
        $taskKey = ($taskSource.Name -replace '(?i)\.disabled$', '').ToLowerInvariant()
        if ($taskMigrated.Contains($taskKey)) { continue }
        $taskEnabled = Join-Path $taskSteam ('config\lua\' + $taskKey)
        $taskDisabled = $taskEnabled + '.disabled'
        if (-not [IO.File]::Exists($taskEnabled) -and -not [IO.File]::Exists($taskDisabled)) {
            $taskDestination = Join-Path $taskSteam ('config\lua\' + $taskSource.Name)
            [IO.File]::Copy($taskSource.FullName, $taskDestination, $false)
        }
        [void]$taskMigrated.Add($taskKey)
    }
    $taskLedger = ConvertTo-Json -InputObject @($taskMigrated | Sort-Object) -Compress
    $taskLedgerBytes = $taskUtf8.GetBytes($taskLedger)
    if (-not (Same-Bytes (Read-OptionalBytes $taskMarker) $taskLedgerBytes)) {
        Write-Atomic $taskMarker $taskLedgerBytes
    }
}
} finally { $taskLoaderLock.Dispose() }
