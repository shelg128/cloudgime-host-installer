!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "StrFunc.nsh"
${Using:StrFunc} StrStr

!ifndef PRODUCT_NAME
!define PRODUCT_NAME "Cloudgime Host"
!endif
!ifndef PRODUCT_VERSION
!define PRODUCT_VERSION "1.1.2"
!endif
!ifndef PRODUCT_VERSION_QUAD
!define PRODUCT_VERSION_QUAD "1.1.2.0"
!endif
!ifndef OUTPUT_DIR
!define OUTPUT_DIR "."
!endif
!ifndef PAYLOAD_APP
!define PAYLOAD_APP "..\payload\app"
!endif
!ifndef PAYLOAD_BUNDLE
!define PAYLOAD_BUNDLE "..\payload\bundle"
!endif

Name "${PRODUCT_NAME}"
OutFile "${OUTPUT_DIR}\CloudgimeHostSetup.exe"
VIProductVersion "${PRODUCT_VERSION_QUAD}"
VIAddVersionKey "ProductName" "${PRODUCT_NAME}"
VIAddVersionKey "ProductVersion" "${PRODUCT_VERSION}"
VIAddVersionKey "FileDescription" "${PRODUCT_NAME} offline installer"
VIAddVersionKey "FileVersion" "${PRODUCT_VERSION}"
InstallDir "$PROGRAMFILES64\Cloudgime Host"
RequestExecutionLevel admin
ShowInstDetails hide
ShowUninstDetails hide

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_LANGUAGE "English"

Section "Install"
  SetRegView 64
  SetShellVarContext all
  InitPluginsDir
  SetOutPath "$PLUGINSDIR\app"
  File /r "${PAYLOAD_APP}\*"
  SetOutPath "$PLUGINSDIR\bundle"
  File /r "${PAYLOAD_BUNDLE}\*"

  ${GetParameters} $R0
  ${StrStr} $R1 $R0 "/FRESH"
  StrCmp $R1 "" fresh_install_normal
  ExecWait '"$PLUGINSDIR\app\cloudgime-host-bootstrap.exe" install --install-root "$INSTDIR" --release-root "$PLUGINSDIR\app" --bundle-source-root "$PLUGINSDIR\bundle" --overwrite-data --reset-local-identity' $0
  Goto fresh_install_done
fresh_install_normal:
  ExecWait '"$PLUGINSDIR\app\cloudgime-host-bootstrap.exe" install --install-root "$INSTDIR" --release-root "$PLUGINSDIR\app" --bundle-source-root "$PLUGINSDIR\bundle"' $0
fresh_install_done:
  ${If} $0 != 0
    MessageBox MB_ICONSTOP "Cloudgime Host setup failed.$\r$\nExit code: $0"
    Abort
  ${EndIf}

  ; Sentinel Launcher is a required native billing/access-gate dependency.
  ; Preserve an existing deployment because it owns local billing state.
  IfFileExists "$PROGRAMFILES64\Sentinel Launcher\Sentinel Launcher.exe" sentinel_ready
  IfFileExists "$PROGRAMFILES\Sentinel Launcher\Sentinel Launcher.exe" sentinel_ready
  IfFileExists "$PLUGINSDIR\bundle\sentinel\SentinelLauncherSetup.exe" 0 sentinel_missing_payload
  ExecWait '"$PLUGINSDIR\bundle\sentinel\SentinelLauncherSetup.exe" /S' $1
  ${If} $1 != 0
    MessageBox MB_ICONSTOP "Sentinel Launcher prerequisite failed.$\r$\nExit code: $1"
    Abort
  ${EndIf}
  Goto sentinel_ready
sentinel_missing_payload:
  MessageBox MB_ICONSTOP "Sentinel Launcher prerequisite is missing from this Cloudgime Host installer."
  Abort
sentinel_ready:
SectionEnd
