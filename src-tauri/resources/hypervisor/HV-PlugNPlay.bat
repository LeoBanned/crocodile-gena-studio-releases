@echo off
setlocal EnableExtensions EnableDelayedExpansion
REM ============================================================
REM HV-PlugNPlay - Bypass Script V2.3.1
REM by Verix
REM ============================================================

set "DEBUG_KEEP_OPEN=0"

set "SCRIPT_DIR=%~dp0"
set "SCRIPT_PATH=%~f0"

REM Use pushd so UNC/SMB paths are mapped to a temporary drive letter.
pushd "%SCRIPT_DIR%" >nul 2>&1
if errorlevel 1 (
    echo [ERROR] Unable to access script directory:
    echo         "%SCRIPT_DIR%"
    pause
    exit /b 1
)

set "SCRIPT_DIR=%CD%"
if not "%SCRIPT_DIR:~-1%"=="\" set "SCRIPT_DIR=%SCRIPT_DIR%\"
set "SCRIPT_PATH=%SCRIPT_DIR%%~nx0"
set "FAIL_REASON="
set "ELEVATE_WINDOW_STYLE=Normal"
if "%DEBUG_KEEP_OPEN%"=="0" set "ELEVATE_WINDOW_STYLE=Hidden"
set "LOG_FILE=%SystemRoot%\Temp\HV-PlugNPlay-debug.log"

if "%DEBUG_KEEP_OPEN%"=="1" (
    cls
    echo ============================================================
    echo          Hypervisor Plug-and-Play Bypass Script
    echo ============================================================
    echo.
)

if /i not "%~1"=="__MINIMIZED__" (
    >"%LOG_FILE%" echo [%date% %time%] START args=%* script="%SCRIPT_PATH%" dir="%SCRIPT_DIR%"
) else (
    >>"%LOG_FILE%" echo [%date% %time%] CHILD START args=%* script="%SCRIPT_PATH%" dir="%SCRIPT_DIR%"
)

call :ensure_admin
if errorlevel 1 (
    >>"%LOG_FILE%" echo [%date% %time%] ensure_admin requested relaunch or failed.
    goto END
)

>>"%LOG_FILE%" echo [%date% %time%] running with admin token.

if /i not "%~1"=="__MINIMIZED__" (
    if "%DEBUG_KEEP_OPEN%"=="1" (
        >>"%LOG_FILE%" echo [%date% %time%] DEBUG_KEEP_OPEN active; running in current elevated window.
    ) else (
        >>"%LOG_FILE%" echo [%date% %time%] DEBUG_KEEP_OPEN disabled; relaunching hidden child process.
        powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Start-Process -FilePath $env:SCRIPT_PATH -WorkingDirectory $env:SCRIPT_DIR -ArgumentList '__MINIMIZED__' -WindowStyle Hidden" >nul 2>&1
        goto END
    )
)

set "MSI_RUNNING=0"
set "MSI_PATH="
set "DRV_FOLDER=DenuvOwO"
set "DRV_EXE=%DRV_FOLDER%\drvloader.exe"
set "WAIT_SECONDS=5"

call :ensure_drvloader
if errorlevel 1 (
    >>"%LOG_FILE%" echo [%date% %time%] ensure_drvloader failed.
    set "FAIL_REASON=drvloader preparation failed."
    goto END
)

>>"%LOG_FILE%" echo [%date% %time%] ensure_drvloader ok.

call :check_core_isolation
if errorlevel 1 (
    >>"%LOG_FILE%" echo [%date% %time%] check_core_isolation failed.
    set "FAIL_REASON=Core Isolation is enabled."
    goto END
)

>>"%LOG_FILE%" echo [%date% %time%] check_core_isolation ok.

call :handle_msi

echo [OK] Core Isolation is disabled. Continuing...
echo.

echo [INFO] Stopping Vanguard anti-cheat to prevent BSOD...
net stop vgc >nul 2>&1
net stop vgk >nul 2>&1
taskkill /IM vgtray.exe /F >nul 2>&1
echo [OK] Vanguard stopped (or was not running).
echo.

cls
echo ============================================================
echo [1/3] Driver Signature Enforcement: BYPASS
echo ============================================================
echo.
call :run_drvloader bypass
if errorlevel 1 (
    >>"%LOG_FILE%" echo [%date% %time%] drvloader bypass failed.
    set "FAIL_REASON=drvloader bypass command failed."
    goto END
)

>>"%LOG_FILE%" echo [%date% %time%] drvloader bypass ok.
cls

call :find_launcher_exe
if errorlevel 1 (
    >>"%LOG_FILE%" echo [%date% %time%] find_launcher_exe failed.
    set "FAIL_REASON=Game executable not found under script directory."
    goto END
)

>>"%LOG_FILE%" echo [%date% %time%] find_launcher_exe ok. launcher="%launcher_exe%"

cls
echo ============================================================
echo [2/3] Launching Game
echo ============================================================
echo.
echo [INFO] Launch target:
echo        "%launcher_exe%"
echo [INFO] Working directory:
echo        "%GAME_DIR%"
echo.
start "" /d "%GAME_DIR%" "%launcher_exe%"

echo [OK] Game launch command sent.
echo.

call :wait_for_game_process
if errorlevel 1 (
    >>"%LOG_FILE%" echo [%date% %time%] wait_for_game_process failed.
    set "FAIL_REASON=Delay/wait step failed."
    goto END
)

>>"%LOG_FILE%" echo [%date% %time%] wait_for_game_process ok.

echo [OK] %WAIT_SECONDS%-second launch delay completed.
echo.

echo ============================================================
echo [3/3] Driver Signature Enforcement: RESTORE
echo ============================================================
echo.
call :run_drvloader restore
if errorlevel 1 (
    >>"%LOG_FILE%" echo [%date% %time%] drvloader restore failed.
    set "FAIL_REASON=drvloader restore command failed."
    goto END
)

>>"%LOG_FILE%" echo [%date% %time%] drvloader restore ok.

call :restart_msi_if_needed

goto END

:ensure_admin
powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "$p = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent()); if ($p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { exit 0 } else { exit 1 }" >nul 2>&1
if errorlevel 1 (
    if "%DEBUG_KEEP_OPEN%"=="0" (
        powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Start-Process -FilePath $env:SCRIPT_PATH -WorkingDirectory $env:SCRIPT_DIR -ArgumentList '__MINIMIZED__' -Verb RunAs -WindowStyle Hidden" >nul 2>&1
        exit /b 1
    )
    cls
    echo ============================================================
    echo                 ADMINISTRATOR PRIVILEGES REQUIRED
    echo ============================================================
    echo.
    echo [INFO] This script requires administrator rights.
    echo [INFO] A UAC prompt will appear. Click "Yes" to continue.
    echo.
    powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Start-Process -FilePath $env:SCRIPT_PATH -WorkingDirectory $env:SCRIPT_DIR -Verb RunAs -WindowStyle %ELEVATE_WINDOW_STYLE%" >nul 2>&1
    if errorlevel 1 (
        echo [ERROR] Failed to trigger UAC elevation.
        echo [INFO] Try running this script manually with "Run as administrator".
        pause
    )
    exit /b 1
)
exit /b 0


:ensure_drvloader
if exist "%DRV_EXE%" (
    echo [OK] drvloader.exe found.
    exit /b 0
)

echo [INFO] drvloader.exe not found. Downloading...

set "DRV_URL=https://github.com/wesmar/KernelResearchKit/releases/download/bypass-code-integrity/KernelResearchKit.7z"
set "DRV_ARCHIVE=%DRV_FOLDER%\KernelResearchKit.7z"
set "DRV_RETRIES=0"

mkdir "%DRV_FOLDER%" >nul 2>&1

:DRV_DOWNLOAD_RETRY
powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Invoke-WebRequest '%DRV_URL%' -OutFile '%DRV_ARCHIVE%'" >nul 2>&1
if exist "%DRV_ARCHIVE%" goto DRV_EXTRACT

set /a DRV_RETRIES+=1
if %DRV_RETRIES% GEQ 3 (
    echo [ERROR] Failed to download drvloader after 3 attempts.
    pause
    exit /b 1
)

echo [WARN] Download failed. Retrying...
timeout /t 2 /nobreak >nul
goto DRV_DOWNLOAD_RETRY

:DRV_EXTRACT
echo [INFO] Extracting archive...

if not exist "%DRV_FOLDER%\7zr.exe" (
    powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Invoke-WebRequest 'https://www.7-zip.org/a/7zr.exe' -OutFile '%DRV_FOLDER%\7zr.exe'" >nul 2>&1
)

if exist "%DRV_FOLDER%\7zr.exe" "%DRV_FOLDER%\7zr.exe" x "%DRV_ARCHIVE%" -o"%DRV_FOLDER%" -p"github.com" -y >nul 2>&1

set "DRV_WAIT_COUNT=0"

:DRV_WAIT_FOR_EXE
if exist "%DRV_EXE%" goto DRV_DONE

set /a DRV_WAIT_COUNT+=1
if %DRV_WAIT_COUNT% GEQ 30 (
    echo [ERROR] Extraction failed.
    if exist "%DRV_ARCHIVE%" del /f /q "%DRV_ARCHIVE%" >nul 2>&1
    if exist "%DRV_FOLDER%\7zr.exe" del /f /q "%DRV_FOLDER%\7zr.exe" >nul 2>&1
    pause
    exit /b 1
)

timeout /t 1 /nobreak >nul
goto DRV_WAIT_FOR_EXE

:DRV_DONE
if exist "%DRV_ARCHIVE%" del /f /q "%DRV_ARCHIVE%" >nul 2>&1
if exist "%DRV_FOLDER%\7zr.exe" del /f /q "%DRV_FOLDER%\7zr.exe" >nul 2>&1
for /f "delims=" %%F in ('dir /b /a-d "%DRV_FOLDER%\*" 2^>nul') do (
    if /i not "%%F"=="drvloader.exe" del /f /q "%DRV_FOLDER%\%%F" >nul 2>&1
)

if exist "%DRV_EXE%" (
    echo [OK] drvloader.exe ready.
    exit /b 0
)

echo [ERROR] drvloader.exe could not be prepared.
exit /b 1


:check_core_isolation
echo [INFO] Checking Core Isolation ^(Memory Integrity^)...

set "CI="
for /f "tokens=3" %%A in ('reg query "HKLM\SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity" /v Enabled 2^>nul ^| find /i "Enabled"') do set "CI=%%A"
set "CI=%CI: =%"

echo [INFO] Detected CI value: "%CI%"

if "%CI%"=="" (
    echo [WARN] Core Isolation key not found. Assuming disabled.
    exit /b 0
)

if /i "%CI%"=="1" goto CORE_ON
if /i "%CI%"=="0x1" goto CORE_ON

exit /b 0

:CORE_ON
cls
color 0C
echo.
echo ================================================
echo   ERROR: Core Isolation (Memory Integrity) is ON
echo ===============================================
echo.
echo [ACTION] Please DISABLE Core Isolation ^(Memory Integrity^)
echo          from Windows Security - Device Security.
echo.
echo [ACTION] After disabling it, RESTART your PC
echo          and run this script again.
echo.
pause
color 07
exit /b 1


:handle_msi
echo [INFO] Checking MSI Afterburner status...

tasklist /FI "IMAGENAME eq MSIAfterburner.exe" | find /I "MSIAfterburner.exe" >nul
if errorlevel 1 (
    echo [OK] MSI Afterburner is not running.
    echo.
    exit /b 0
)

echo [INFO] MSI Afterburner is running. Closing it...
set "MSI_RUNNING=1"

for /f "usebackq delims=" %%A in (`powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "(Get-Process -Name 'MSIAfterburner' -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty Path)"`) do (
    if not defined MSI_PATH set "MSI_PATH=%%A"
)

if not defined MSI_PATH (
    for /f "delims=" %%A in ('where MSIAfterburner.exe 2^>nul') do (
        if not defined MSI_PATH set "MSI_PATH=%%A"
    )
)

taskkill /IM MSIAfterburner.exe /F >nul 2>&1
echo.
exit /b 0


:find_launcher_exe
set "launcher_exe="
set "GAME_DIR="

for /f "usebackq delims=" %%F in (`powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "$root=$env:SCRIPT_DIR; $names=@('HV-StartGame.exe','hypervisor-launcher.exe','steamclient_loader_x64.exe','launcher.exe','HypervisorLauncher.exe'); $hit=Get-ChildItem -LiteralPath $root -Recurse -File -Filter '*.exe' -ErrorAction SilentlyContinue | Where-Object { $names -contains $_.Name } | Select-Object -First 1 -ExpandProperty FullName; if($hit){ Write-Output $hit }"`) do (
    if not defined launcher_exe set "launcher_exe=%%F"
)

if defined launcher_exe (
    for %%I in ("!launcher_exe!") do set "GAME_DIR=%%~dpI"
    >>"%LOG_FILE%" echo [%date% %time%] named launcher found: "!launcher_exe!"
)

if not defined launcher_exe (
    echo [WARN] No named launcher found. Searching for largest .exe...
    set "LARGEST_EXE="
    set "LARGEST_SIZE=0"
    for /f "usebackq tokens=1,2 delims=|" %%A in (`powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "$root=$env:SCRIPT_DIR; $exe=Get-ChildItem -LiteralPath $root -Recurse -File -Filter '*.exe' -ErrorAction SilentlyContinue | Where-Object { $_.BaseName -notmatch '(?i)(trial|_plus|drvloader)' } | Sort-Object Length -Descending | Select-Object -First 1; if($exe){ Write-Output ($exe.FullName + '|' + $exe.Length) }"`) do (
        if not defined LARGEST_EXE (
            set "LARGEST_EXE=%%A"
            set "LARGEST_SIZE=%%B"
        )
    )

    if defined LARGEST_EXE (
        echo [INFO] Falling back to largest exe ^(!LARGEST_SIZE! bytes^):
        echo        !LARGEST_EXE!
        set "launcher_exe=!LARGEST_EXE!"
        for %%I in ("!launcher_exe!") do set "GAME_DIR=%%~dpI"
        >>"%LOG_FILE%" echo [%date% %time%] fallback launcher selected: "!launcher_exe!" size=!LARGEST_SIZE!
    ) else (
        >>"%LOG_FILE%" echo [%date% %time%] no launcher candidates found under "!SCRIPT_DIR!"
        cls
        echo ============================================================
        echo [ERROR] Game executable not found.
        echo ============================================================
        echo.
        echo Searched recursively under: !SCRIPT_DIR!
        echo Candidate names:
        echo   HV-StartGame.exe
        echo   hypervisor-launcher.exe
        echo   steamclient_loader_x64.exe
        echo   launcher.exe
        echo   HypervisorLauncher.exe
        echo   ^(also tried: largest .exe fallback^)
        exit /b 1
    )
)

exit /b 0

:wait_for_game_process
echo [INFO] Waiting %WAIT_SECONDS% seconds before restoring DSE...
timeout /t %WAIT_SECONDS% /nobreak >nul
exit /b 0

:restart_msi_if_needed
:: ============================================
:: RESTART MSI AFTERBURNER IF IT WAS RUNNING
:: ============================================
if not "%MSI_RUNNING%"=="1" (
    echo [INFO] MSI Afterburner was not running before. Skipping restart.
    exit /b 0
)

echo [INFO] Restarting MSI Afterburner...
if defined MSI_PATH (
    start "" "%MSI_PATH%"
) else (
    if exist "C:\Program Files (x86)\MSI Afterburner\MSIAfterburner.exe" (
        start "" "C:\Program Files (x86)\MSI Afterburner\MSIAfterburner.exe"
    ) else (
        echo [WARN] MSI Afterburner path could not be detected. Please launch it manually.
    )
)
exit /b 0

:run_drvloader
set "DRV_ACTION=%~1"
if "%DEBUG_KEEP_OPEN%"=="0" goto run_drvloader_hidden
"%DRV_EXE%" %DRV_ACTION%
exit /b %errorlevel%

:run_drvloader_hidden
powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "$psi = New-Object System.Diagnostics.ProcessStartInfo; $psi.FileName = $env:DRV_EXE; $psi.Arguments = $env:DRV_ACTION; $psi.UseShellExecute = $false; $psi.CreateNoWindow = $true; $psi.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden; $p = [System.Diagnostics.Process]::Start($psi); $p.WaitForExit(); exit $p.ExitCode" >nul 2>&1
exit /b %errorlevel%

:END
if defined FAIL_REASON (
    >>"%LOG_FILE%" echo [%date% %time%] FAILURE: %FAIL_REASON%
    echo.
    echo [ERROR] %FAIL_REASON%
    echo [INFO] Script directory: "%SCRIPT_DIR%"
    pause
)
if not defined FAIL_REASON (
    >>"%LOG_FILE%" echo [%date% %time%] END without explicit failure reason.
)
popd >nul 2>&1
endlocal
exit /b
