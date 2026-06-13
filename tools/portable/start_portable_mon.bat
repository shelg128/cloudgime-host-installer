@echo off
setlocal

set "PCNUMBER=%~1"

if "%PCNUMBER%"=="" (
    set /p PCNUMBER=PC keberapa: 
)

if "%PCNUMBER%"=="" (
    echo PC number wajib diisi.
    exit /b 1
)

powershell -ExecutionPolicy Bypass -File "%~dp0start_portable_mon.ps1" -PcNumber %PCNUMBER%
set "EXITCODE=%ERRORLEVEL%"

if not "%EXITCODE%"=="0" (
    echo.
    echo Gagal menjalankan bundle.
    exit /b %EXITCODE%
)

echo.
echo Tekan tombol apa saja untuk tutup.
pause >nul
