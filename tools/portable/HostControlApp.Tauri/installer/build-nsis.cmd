@echo off
setlocal
dotnet run --project "%~dp0..\..\HostControlPackaging\HostControlPackaging.csproj" -- build-nsis %*
set "EXIT_CODE=%ERRORLEVEL%"
endlocal
exit /b %EXIT_CODE%
