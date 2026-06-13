@echo off
setlocal
dotnet run --project "%~dp0..\..\HostControlPackaging\HostControlPackaging.csproj" -- prepare-nsis-payload %*
endlocal
