@echo off
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0Publish-CrocLuaDlls.ps1" %*
set "CGS_PUBLISH_EXIT=%errorlevel%"
pause
exit /b %CGS_PUBLISH_EXIT%