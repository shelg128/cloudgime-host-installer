@echo off
setlocal
dotnet run --project "%~dp0..\HostControlPackaging\HostControlPackaging.csproj" -- validate-end-to-end %*
endlocal
