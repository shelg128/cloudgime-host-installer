@echo off
setlocal

set "PCNUMBER=%~1"
set "TRANSPORTMODE=%~2"
set "TURNARG="
set "PRESETARG="

if "%PCNUMBER%"=="" (
    set /p PCNUMBER=PC keberapa: 
)

if "%PCNUMBER%"=="" (
    echo PC number wajib diisi.
    exit /b 1
)

if /I "%TRANSPORTMODE%"=="turn" set "TURNARG=-UseTurnFallback"
if /I "%TRANSPORTMODE%"=="relay" set "TURNARG=-UseTurnFallback"
if /I "%TRANSPORTMODE%"=="direct-safe" set "PRESETARG=-TransportPreset direct-safe"
if /I "%TRANSPORTMODE%"=="direct-strong" set "PRESETARG=-TransportPreset direct-strong"

powershell -ExecutionPolicy Bypass -File "%~dp0make_portable_mon.ps1" -PcNumber %PCNUMBER% %PRESETARG% %TURNARG%
set "EXITCODE=%ERRORLEVEL%"

if not "%EXITCODE%"=="0" (
    echo.
    echo Gagal membuat bundle.
    exit /b %EXITCODE%
)

echo.
echo Tekan tombol apa saja untuk tutup.
pause >nul
