@echo off
setlocal
dotnet run --project "%~dp0..\HostControlPackaging\HostControlPackaging.csproj" -- publish-release %*
endlocal
