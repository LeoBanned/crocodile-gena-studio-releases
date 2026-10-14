@echo off
setlocal
echo Crocodile Gena Studio - Windows x64 portable build
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0tools\build-windows.ps1" -Target portable
set "CGS_BUILD_EXIT=%ERRORLEVEL%"
echo.
pause
exit /b %CGS_BUILD_EXIT%
