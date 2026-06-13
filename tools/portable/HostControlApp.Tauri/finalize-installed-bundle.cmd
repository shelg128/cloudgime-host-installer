@echo off
setlocal
dotnet run --project "%~dp0..\HostControlBootstrap\HostControlBootstrap.csproj" -- repair-state %*
endlocal
