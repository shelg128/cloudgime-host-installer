@echo off
setlocal
set "ROOT=%MOONLIGHT_BUNDLE_ROOT%"
if "%ROOT%"=="" set "ROOT=%~dp0..\.."
set "BOOTSTRAP=%ROOT%\cloudgime-host-bootstrap.exe"
if not exist "%BOOTSTRAP%" (
  echo Cloudgime bootstrap executable not found:
  echo %BOOTSTRAP%
  exit /b 1
)
"%BOOTSTRAP%" uninstall-gamepad
exit /b %ERRORLEVEL%
