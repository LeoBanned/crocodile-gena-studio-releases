@echo off
:: HV-Launcher By SOULX3X
set "VERSION=5.0"

:: Custom EXE Support
set "CUSTOM_EXE_SUPPORT="

:: Flag to track if DSE was turned off by this script
set "DSE_WAS_OFF=0"

:: Error logging setup (log file created only on error)
set "LOG_FILE=%~dp0HV-Launcher-Error.log"

:: Enable ANSI escape codes
for /F "tokens=1,2 delims=#" %%a in ('"prompt #$H#$E# & echo on & for %%b in (1) do rem"') do (
  set "DEL=%%a"
  set "ESC=%%b"
)

:: ANSI color codes
set "RESET=%ESC%[0m"
set "BOLD=%ESC%[1m"
set "CYAN=%ESC%[96m"
set "GREEN=%ESC%[92m"
set "YELLOW=%ESC%[93m"
set "RED=%ESC%[91m"
set "MAGENTA=%ESC%[95m"
set "BLUE=%ESC%[94m"
set "WHITE=%ESC%[97m"

:: Display welcome message
echo.
echo %BOLD%%CYAN%================================================%RESET%
echo %BOLD%%WHITE%        HV-Launcher By SOULX3X%RESET%
echo %BOLD%%WHITE%           Version: %VERSION%%RESET%
echo %BOLD%%CYAN%================================================%RESET%
echo.

:: Check for administrator privileges

fltmc >nul 2>&1
if errorlevel 1 (
    schtasks /query /tn "HVLauncherAdmin" >nul 2>&1
    if not errorlevel 1 (
        if not exist "%temp%\hv_admin_lock" (
            echo 1 > "%temp%\hv_admin_lock"
            schtasks /run /tn "HVLauncherAdmin" >nul 2>&1
            exit /b
        )
        del "%temp%\hv_admin_lock" >nul 2>&1
    )

    echo.
    echo This script requires administrator privileges.
    echo.
    echo A UAC prompt will appear. Please click "Yes".
    echo.

    powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Start-Process -FilePath \"%~f0\" -ArgumentList '__MINIMIZED__' -Verb RunAs -WindowStyle Minimized"
    exit /b
)

if exist "%temp%\hv_admin_lock" del "%temp%\hv_admin_lock" >nul 2>&1

if /i not "%~1"=="__MINIMIZED__" (
    start "" /min "%~f0" __MINIMIZED__
    exit /b
)

:: Change to script directory
cd /d "%~dp0"

:: Detect Windows version
echo %BOLD%%BLUE%[SYSTEM INFO]%RESET% %YELLOW%Detecting Windows version...%RESET%

powershell -NoProfile -Command "$v=(Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'); if($v.CurrentBuild -ge 22000){$n='Windows 11'}else{$n='Windows 10'}; $d=$v.DisplayVersion; if(!$d){$d=$v.ReleaseId}; if($d){Write-Output \"$n $d\"}else{Write-Output \"$n\"}" > "%temp%\winver.tmp"
set /p WIN_VERSION=<"%temp%\winver.tmp"
del /q "%temp%\winver.tmp" 2>nul

echo %GREEN%Detected: %WIN_VERSION%%RESET%
echo.

:: Check Core Isolation (Memory Integrity)
echo.
echo %BOLD%%BLUE%[STEP 1/9]%RESET% %YELLOW%Checking Core Isolation (Memory Integrity)...%RESET%

set "CI="

for /f "tokens=3" %%A in ('reg query "HKLM\SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity" /v Enabled 2^>nul ^| find /i "Enabled"') do set CI=%%A
set CI=%CI: =%

echo %CYAN%Detected CI value:%RESET% "%CI%"

if "%CI%"=="" goto KVC_CHECK
if /i "%CI%"=="1" goto CORE_ON
if /i "%CI%"=="0x1" goto CORE_ON
goto KVC_CHECK

:CORE_ON
echo.
echo %BOLD%%RED%================================================%RESET%
echo %BOLD%%RED%  ERROR: Core Isolation (Memory Integrity) is ON%RESET%
echo %BOLD%%RED%================================================%RESET%
echo.
echo %YELLOW%Please %BOLD%DISABLE%RESET%%YELLOW% Core Isolation (Memory Integrity)%RESET%
echo from Windows Security - Device Security.
echo.
echo %CYAN%After disabling it, %BOLD%RESTART%RESET%%CYAN% your PC%RESET%
echo and run this script again.
echo.
call :LOG_ERROR "Core Isolation (Memory Integrity) is ON"
pause
goto CLEANUP_EXIT

:KVC_CHECK
:: Ensure kvc.exe exists (download if missing)
echo.
echo %BOLD%%BLUE%[STEP 2/9]%RESET% %YELLOW%Checking for kvc.exe...%RESET%
if exist "kvc.exe" goto KVC_READY

echo.
echo %MAGENTA%kvc.exe not found, installing...%RESET%
set "URL=https://github.com/wesmar/kvc/releases/download/latest/kvc.7z"
set "INSTALL_DIR=KVC Installing"
set "ARCHIVE=%INSTALL_DIR%\kvc.7z"

mkdir "%INSTALL_DIR%" >nul 2>&1

set RETRIES=0

:DOWNLOAD_RETRY
echo.
echo %CYAN%Attempting to download kvc.exe...%RESET%
powershell -NoProfile -Command "Invoke-WebRequest '%URL%' -OutFile '%ARCHIVE%'" >nul 2>&1

if exist "%ARCHIVE%" goto EXTRACT_KVC

set /a RETRIES+=1

if %RETRIES% GEQ 3 (
    call :LOG_ERROR "Failed to download kvc.7z after 3 attempts"
    echo.
    echo %RED%ERROR: Failed to download kvc after 3 attempts.%RESET%
    pause
    goto CLEANUP_EXIT
)

echo.
echo %YELLOW%Download failed. Retrying...%RESET%
timeout /t 2 /nobreak >nul
goto DOWNLOAD_RETRY

:EXTRACT_KVC
echo.
echo %CYAN%Extracting archive...%RESET%
if not exist "7zr.exe" (
    powershell -NoProfile -Command "Invoke-WebRequest 'https://www.7-zip.org/a/7zr.exe' -OutFile '7zr.exe'" >nul 2>&1
)
if not exist "7zr.exe" (
    call :LOG_ERROR "Failed to download 7zr.exe"
    echo.
    echo %RED%ERROR: Failed to download 7zr.exe.%RESET%
    rd /s /q "%INSTALL_DIR%" >nul 2>&1
    pause
    goto CLEANUP_EXIT
)
if exist "7zr.exe" 7zr.exe x "%ARCHIVE%" -o"%INSTALL_DIR%" -p"github.com" -y >nul 2>&1

set WAIT_COUNT=0

:WAIT_FOR_KVC
if exist "%INSTALL_DIR%\kvc.exe" goto MOVE_KVC

set /a WAIT_COUNT+=1

if %WAIT_COUNT% GEQ 30 (
    call :LOG_ERROR "Extraction of kvc.7z failed (timeout after 30s)"
    echo.
    echo %RED%ERROR: Extraction failed.%RESET%
    rd /s /q "%INSTALL_DIR%" >nul 2>&1
    if exist "7zr.exe" del /f /q "7zr.exe" >nul 2>&1
    pause
    goto CLEANUP_EXIT
)

timeout /t 1 /nobreak >nul
goto WAIT_FOR_KVC

:MOVE_KVC
move "%INSTALL_DIR%\kvc.exe" "kvc.exe" >nul
if errorlevel 1 (
    call :LOG_ERROR "Failed to move kvc.exe to script directory"
    echo.
    echo %RED%ERROR: Failed to move kvc.exe.%RESET%
    rd /s /q "%INSTALL_DIR%" >nul 2>&1
    if exist "7zr.exe" del /f /q "7zr.exe" >nul 2>&1
    pause
    goto CLEANUP_EXIT
)
rd /s /q "%INSTALL_DIR%" >nul 2>&1
if exist "7zr.exe" del /f /q "7zr.exe" >nul 2>&1

echo.
echo %GREEN%kvc.exe ready.%RESET%
echo.

:KVC_READY

:: Check MSI Afterburner
:CHECK_MSI
echo.
echo %BOLD%%BLUE%[STEP 3/9]%RESET% %YELLOW%Checking MSI Afterburner status...%RESET%
set "MSI_RUNNING=0"
set "MSI_PATH="

tasklist /FI "IMAGENAME eq MSIAfterburner.exe" | find /I "MSIAfterburner.exe" >nul

if %errorlevel%==0 (
    echo %GREEN%MSI Afterburner is running. Closing it...%RESET%
    set "MSI_RUNNING=1"

    for /f "tokens=2 delims==" %%A in (
     'wmic process where "name='MSIAfterburner.exe'" get ExecutablePath /value 2^>nul'
        ) do for /f "delims=" %%B in ("%%A") do set "MSI_PATH=%%B"

    taskkill /IM MSIAfterburner.exe /F >nul 2>&1
) else (
    echo %GREEN%MSI Afterburner is not running.%RESET%
)

:: Check RTSS and related processes
:CHECK_RTSS
echo.
echo %BOLD%%BLUE%[STEP 4/9]%RESET% %YELLOW%Checking RTSS related processes...%RESET%
set "RTSS_RUNNING=0"

tasklist /FI "IMAGENAME eq RTSS.exe" | find /I "RTSS.exe" >nul
if %errorlevel%==0 set "RTSS_RUNNING=1"

tasklist /FI "IMAGENAME eq EncoderServer.exe" | find /I "EncoderServer.exe" >nul
if %errorlevel%==0 set "RTSS_RUNNING=1"

tasklist /FI "IMAGENAME eq RTSSHooksLoader64.exe" | find /I "RTSSHooksLoader64.exe" >nul
if %errorlevel%==0 set "RTSS_RUNNING=1"

if "%RTSS_RUNNING%"=="1" (
    echo %GREEN%RTSS is running. Closing it...%RESET%
    taskkill /IM RTSS.exe /F >nul 2>&1
    taskkill /IM EncoderServer.exe /F >nul 2>&1
    taskkill /IM RTSSHooksLoader64.exe /F >nul 2>&1
) else (
    echo %GREEN%RTSS is not running.%RESET%
)

:: Check Vanguard
:CHECK_VANGUARD
echo.
echo %BOLD%%BLUE%[STEP 5/9]%RESET% %YELLOW%Checking Vanguard status...%RESET%
set "VANGUARD_RUNNING=0"
set "VANGUARD_PATH="

tasklist /FI "IMAGENAME eq vgtray.exe" | find /I "vgtray.exe" >nul

if %errorlevel%==0 (
    echo %GREEN%Vanguard is running. Closing it...%RESET%
    set "VANGUARD_RUNNING=1"
    taskkill /IM vgtray.exe /F >nul 2>&1
) else (
    echo %GREEN%Vanguard is not running.%RESET%
)

timeout /t 0 /nobreak >nul

:: Disable DSE
echo.
echo %BOLD%%BLUE%[STEP 6/9]%RESET% %YELLOW%Disabling DSE...%RESET%
kvc.exe dse off --safe
if errorlevel 1 (
    echo %YELLOW%Safe method failed, trying standard method...%RESET%
    kvc.exe dse off
    if errorlevel 1 (
        call :LOG_ERROR "Failed to disable DSE (both safe and standard methods failed)"
        echo %RED%ERROR: Failed to disable DSE.%RESET%
        pause
        goto CLEANUP_EXIT
    ) else (
        set "STANDARD_DSE=1"
        set "DSE_WAS_OFF=1"
        echo DSE Disabled Successfully
    )
) else (
    set "DSE_WAS_OFF=1"
    echo DSE Disabled Successfully
)
:: Find loader executable
echo.
echo %BOLD%%BLUE%[STEP 7/9]%RESET% %YELLOW%Finding loader executable...%RESET%
set "loader="

:: Try to find loader in order of preference
if exist "steamclient_loader_x64.exe" set "loader=steamclient_loader_x64.exe"
if exist "HypervisorLauncher.exe" set "loader=HypervisorLauncher.exe"
if exist "launcher.exe" set "loader=launcher.exe"
if exist "hypervisor-launcher.exe" set "loader=hypervisor-launcher.exe"
if exist "HV-StartGame.exe" set "loader=HV-StartGame.exe"
if exist "%CUSTOM_EXE_SUPPORT%" set "loader=%CUSTOM_EXE_SUPPORT%"


:: If no preferred loader found, search for the largest executable
if not defined loader (
    for /f "delims=" %%A in ('dir /b /A:-D /O:-S *.exe 2^>nul') do (
        if /i not "%%A"=="kvc.exe" if /i not "%%A"=="UbisoftConnectInstaller.exe" if /i not "%%A"=="UnityCrashHandler64.exe" if /i not "%%A"=="7zr.exe" (
            set "loader=%%A"
            goto FOUND_LOADER
        )
    )
)

:FOUND_LOADER
if not defined loader (
    call :LOG_ERROR "No loader executable found in %CD%"
    echo.
    echo %RED%ERROR: No executable found!%RESET%
    pause
    goto CLEANUP_EXIT
)

echo %GREEN%Found: %RESET%%loader%
start "" "%loader%"

set WAIT_LOADER=0
:WAIT_LOADER_LOOP
tasklist /FO CSV /FI "IMAGENAME eq %loader%" 2>nul | find /I "%loader%" >nul
if %errorlevel%==0 goto LOADER_STARTED

set /a WAIT_LOADER+=1
if %WAIT_LOADER% GEQ 5 (
    echo %YELLOW%WARNING: %loader% did not start in time. Proceeding anyway...%RESET%
    goto LOADER_STARTED
)
timeout /t 3 /nobreak >nul
goto WAIT_LOADER_LOOP

:LOADER_STARTED
timeout /t 3 /nobreak >nul

echo.
echo %BOLD%%GREEN%Loader started successfully. Running cleanup...%RESET%
goto CLEANUP_EXIT

:: Cleanup routine - always runs on exit
:CLEANUP_EXIT
:: Step 8/9: Turn DSE back on if we turned it off
if "%DSE_WAS_OFF%"=="1" (
    echo.
    echo %BOLD%%BLUE%[STEP 8/9]%RESET% %YELLOW%Enabling DSE...%RESET%
    if "%STANDARD_DSE%"=="1" (
        kvc.exe dse on
        if errorlevel 1 (
            call :LOG_ERROR "Failed to enable DSE (standard method failed)"
            echo %RED%ERROR: Failed to enable DSE.%RESET%
        ) else (
            echo DSE Enabled Successfully
        )
    ) else (
        kvc.exe dse on --safe
        if errorlevel 1 (
            echo %YELLOW%Safe method failed, trying standard method...%RESET%
            kvc.exe dse on
            if errorlevel 1 (
                call :LOG_ERROR "Failed to enable DSE (both safe and standard methods failed)"
                echo %RED%ERROR: Failed to enable DSE.%RESET%
            ) else (
                echo DSE Enabled Successfully
            )
        ) else (
            echo DSE Enabled Successfully
        )
    )
)
goto RESTORE_MSI

:: Restore MSI Afterburner
:RESTORE_MSI
if "%MSI_RUNNING%"=="1" (
    echo.
    echo %CYAN%Restoring MSI Afterburner...%RESET%
    if defined MSI_PATH (
        start "" "%MSI_PATH%"
        echo %GREEN%MSI Afterburner restored.%RESET%
    ) else (
        if exist "C:\Program Files (x86)\MSI Afterburner\MSIAfterburner.exe" (
            start "" "C:\Program Files (x86)\MSI Afterburner\MSIAfterburner.exe"
            echo %GREEN%MSI Afterburner restored.%RESET%
        ) else (
            if exist "C:\Program Files\MSI Afterburner\MSIAfterburner.exe" (
                start "" "C:\Program Files\MSI Afterburner\MSIAfterburner.exe"
                echo %GREEN%MSI Afterburner restored.%RESET%
            ) else (
                echo %YELLOW%WARNING: MSI Afterburner path could not be detected. Please launch it manually.%RESET%
            )
        )
    )
)
goto REMOVE_EXCLUSIONS

:: Remove Windows Defender exclusions
:REMOVE_EXCLUSIONS
echo.
echo %BOLD%%BLUE%[STEP 9/9]%RESET% %YELLOW%Removing Windows Defender exclusions for kvc.exe...%RESET%
set "KVC_PATH=%~dp0kvc.exe"
powershell -NoProfile -ExecutionPolicy Bypass -Command "[string[]]$excl = (Get-MpPreference).ExclusionPath; if ($excl -contains $env:KVC_PATH) { Remove-MpPreference -ExclusionPath $env:KVC_PATH }" >nul 2>&1
powershell -NoProfile -ExecutionPolicy Bypass -Command "Remove-MpPreference -ExclusionProcess 'kvc.exe' -ErrorAction SilentlyContinue" >nul 2>&1
echo %GREEN%Defender exclusions removed.%RESET%

exit

:: Error logging subroutine - creates log file only on first error
:LOG_ERROR
if not exist "%LOG_FILE%" (
    echo ============================================> "%LOG_FILE%"
    echo HV-Launcher Error Log>> "%LOG_FILE%"
    echo Version: %VERSION%>> "%LOG_FILE%"
    echo Date: %DATE% %TIME%>> "%LOG_FILE%"
    echo ============================================>> "%LOG_FILE%"
)
echo [%DATE% %TIME%] [Version: %VERSION%] ERROR: %~1>> "%LOG_FILE%"