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
$taskSelection = $null
if ($null -ne $taskOldSelection) {
    try { $taskSelection = $taskUtf8.GetString($taskOldSelection) | ConvertFrom-Json }
    catch { throw 'Invalid loader selection. Automatic replacement refused. Choose a version in CrocLua.' }
    if ($null -eq $taskSelection -or $taskSelection.schema -ne 1 -or
        ($taskSelection.schema -isnot [int] -and $taskSelection.schema -isnot [long]) -or
        $taskSelection.variant -cnotin @('standard','compatibility') -or
        @($taskSelection.PSObject.Properties).Count -ne 3 -or @($taskSelection.hashes.PSObject.Properties).Count -ne 3) {
        throw 'Unsupported loader selection. Automatic replacement refused. Choose a version in CrocLua.'
    }
    foreach ($taskName in @('OpenSteamTool.dll','dwmapi.dll','xinput1_4.dll')) {
        if (@($taskSelection.hashes.PSObject.Properties | Where-Object { $_.Name -ceq $taskName }).Count -ne 1 -or $taskSelection.hashes.$taskName -cnotmatch '^[0-9a-f]{64}$') {
            throw 'Invalid pinned DLL fingerprint. Automatic replacement refused.'
        }
    }
}
$taskVariant = if ($null -ne $taskSelection) { $taskSelection.variant } else { 'standard' }
if ($null -eq $taskSelection) {
    $taskExistingMain = Read-OptionalBytes (Join-Path $taskSteam 'OpenSteamTool.dll')
    $taskAlternateMain = [IO.File]::ReadAllBytes((Join-Path $taskResources 'compatibility\OpenSteamTool.dll'))
    if (Same-Bytes $taskExistingMain $taskAlternateMain) { $taskVariant = 'compatibility' }
    elseif ($null -eq $taskExistingMain) {
        foreach ($taskName in @('dwmapi.dll','xinput1_4.dll')) {
            if (Same-Bytes (Read-OptionalBytes (Join-Path $taskSteam $taskName)) ([IO.File]::ReadAllBytes((Join-Path $taskResources ('compatibility\' + $taskName))))) { $taskVariant = 'compatibility' }
        }
    }
}
$taskVariantResources = if ($taskVariant -eq 'compatibility') { Join-Path $taskResources 'compatibility' } else { $taskResources }
$taskHashes = [ordered]@{}
$taskDllBytes = @{}
$taskCurrent = $true
foreach ($taskName in @('OpenSteamTool.dll','dwmapi.dll','xinput1_4.dll')) {
    $taskBytes = [IO.File]::ReadAllBytes((Join-Path $taskVariantResources $taskName))
    if ($taskBytes.Length -lt 1024 -or $taskBytes[0] -ne 77 -or $taskBytes[1] -ne 90) { throw "Invalid bundled DLL: $taskName" }
    $taskDllBytes[$taskName] = $taskBytes
    $taskHasher = [Security.Cryptography.SHA256]::Create()
    try { $taskHashes[$taskName] = ([BitConverter]::ToString($taskHasher.ComputeHash($taskBytes))).Replace('-','').ToLowerInvariant() }
    finally { $taskHasher.Dispose() }
    $taskInstalledBytes = Read-OptionalBytes (Join-Path $taskSteam $taskName)
    if ($null -ne $taskSelection -and $null -ne $taskInstalledBytes) {
        $taskHasher = [Security.Cryptography.SHA256]::Create()
        try { $taskInstalledHash = ([BitConverter]::ToString($taskHasher.ComputeHash($taskInstalledBytes))).Replace('-','').ToLowerInvariant() }
        finally { $taskHasher.Dispose() }
        $taskCurrent = $taskCurrent -and ($taskInstalledHash -ceq $taskSelection.hashes.$taskName)
    } else { $taskCurrent = $taskCurrent -and (Same-Bytes $taskInstalledBytes $taskBytes) }
}
if ($null -ne $taskSelection -and -not $taskCurrent) {
    foreach ($taskName in @('OpenSteamTool.dll','dwmapi.dll','xinput1_4.dll')) {
        if ($taskHashes[$taskName] -cne $taskSelection.hashes.$taskName) {
            throw 'Pinned DLLs need repair but differ from this plugin bundle. Automatic replacement refused. Choose a version explicitly in CrocLua.'
        }
    }
}
if ($null -eq $taskSelection) { $taskSelection = [ordered]@{ schema=1; variant=$taskVariant; hashes=$taskHashes } }
$taskSelectionBytes = if ($null -ne $taskOldSelection) { $taskOldSelection } else { $taskUtf8.GetBytes(($taskSelection | ConvertTo-Json -Depth 4)) }

$taskConfigPath = Join-Path $taskSteam 'opensteamtool.toml'
$taskOldConfig = Read-OptionalBytes $taskConfigPath
$taskUtf8 = New-Object Text.UTF8Encoding($false, $true)
$taskConfigText = if ($null -ne $taskOldConfig) { $taskUtf8.GetString($taskOldConfig).TrimStart([char]0xfeff) } else { '' }
$taskConfigBytes = $taskUtf8.GetBytes((Configure-Lua $taskConfigText))
$taskChanges = @()
foreach ($taskName in @('OpenSteamTool.dll', 'dwmapi.dll', 'xinput1_4.dll', 'opensteamtool.toml', 'config/cgs-luatools-loader.json')) {
    if ($taskCurrent -and $taskName -like '*.dll') { continue }
    $taskTarget = Join-Path $taskSteam $taskName
    $taskOriginal = Read-OptionalBytes $taskTarget
    $taskBytes = if ($taskName -eq 'opensteamtool.toml') { $taskConfigBytes }
        elseif ($taskName -eq 'config/cgs-luatools-loader.json') { $taskSelectionBytes }
        else { $taskDllBytes[$taskName] }
    if ($taskName -like '*.dll' -and ($taskBytes.Length -lt 1024 -or $taskBytes[0] -ne 77 -or $taskBytes[1] -ne 90)) {
        throw "Invalid bundled DLL: $taskName"
    }
    if (-not (Same-Bytes $taskOriginal $taskBytes)) {
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
            Write-Atomic $taskChange.Target $taskChange.Bytes
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
