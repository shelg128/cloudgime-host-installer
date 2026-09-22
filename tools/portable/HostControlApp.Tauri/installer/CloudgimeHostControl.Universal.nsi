!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "StrFunc.nsh"
${Using:StrFunc} StrStr
!ifndef PRODUCT_NAME
!define PRODUCT_NAME "Cloudgime Host"
!endif
!ifndef PRODUCT_VERSION
!define PRODUCT_VERSION "1.2.0"
!endif
!ifndef PRODUCT_VERSION_QUAD
!define PRODUCT_VERSION_QUAD "1.2.0.0"
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
!ifndef INSTALLER_FILE
!define INSTALLER_FILE "CloudgimeHostSetup.exe"
!endif
Name "${PRODUCT_NAME}"
OutFile "${OUTPUT_DIR}\${INSTALLER_FILE}"
VIProductVersion "${PRODUCT_VERSION_QUAD}"
VIAddVersionKey "ProductName" "${PRODUCT_NAME}"
VIAddVersionKey "ProductVersion" "${PRODUCT_VERSION}"
VIAddVersionKey "FileDescription" "${PRODUCT_NAME} Universal Node candidate"
VIAddVersionKey "FileVersion" "${PRODUCT_VERSION}"
InstallDir "$PROGRAMFILES64\Cloudgime Host"
RequestExecutionLevel admin
ShowInstDetails hide
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
  ; Package contract was checked before NSIS and is checked again before Host changes.
  ExecWait '"$PLUGINSDIR\app\cloudgime-host-bootstrap.exe" validate-universal-bundle --silent --bundle-root "$PLUGINSDIR\bundle"' $0
  ${If} $0 != 0
    SetErrorLevel 1
    IfSilent +2
    MessageBox MB_ICONSTOP "Universal package integrity validation failed."
    Abort
  ${EndIf}
  ; Identity/maintenance/fixture preflight precedes installation of either component.
  ${GetParameters} $R0
  ${StrStr} $R1 $R0 "/FRESH"
  StrCmp $R1 "" preflight_upgrade
  ExecWait '"$PLUGINSDIR\app\cloudgime-host-bootstrap.exe" validate-universal-install --silent --bundle-root "$PLUGINSDIR\bundle" --overwrite-data --reset-local-identity' $0
  Goto preflight_done
preflight_upgrade:
  ExecWait '"$PLUGINSDIR\app\cloudgime-host-bootstrap.exe" validate-universal-install --silent --bundle-root "$PLUGINSDIR\bundle"' $0
preflight_done:
  ${If} $0 != 0
    SetErrorLevel 1
    IfSilent +2
    MessageBox MB_ICONSTOP "Universal package preflight failed; installed components were not changed."
    Abort
  ${EndIf}
  ; Install mandatory Sentinel before Host. An existing Sentinel deployment is preserved.
  IfFileExists "$PROGRAMFILES64\Sentinel Launcher\Sentinel Launcher.exe" sentinel_present
  IfFileExists "$PLUGINSDIR\bundle\sentinel\SentinelLauncherSetup.exe" 0 sentinel_missing
  ExecWait '"$PLUGINSDIR\bundle\sentinel\SentinelLauncherSetup.exe" /S' $1
  ${If} $1 != 0
    SetErrorLevel 1
    IfSilent +2
    MessageBox MB_ICONSTOP "Sentinel installation failed.$\r$\nExit code: $1"
    Abort
  ${EndIf}
  Goto sentinel_present
sentinel_missing:
  SetErrorLevel 1
  IfSilent +2
  MessageBox MB_ICONSTOP "Universal package is invalid: Sentinel payload is missing."
  Abort
sentinel_present:
  ${GetParameters} $R0
  ${StrStr} $R1 $R0 "/FRESH"
  StrCmp $R1 "" upgrade_install
  ExecWait '"$PLUGINSDIR\app\cloudgime-host-bootstrap.exe" install --silent --require-universal --install-root "$INSTDIR" --release-root "$PLUGINSDIR\app" --bundle-source-root "$PLUGINSDIR\bundle" --overwrite-data --reset-local-identity' $0
  Goto bootstrap_done
upgrade_install:
  ExecWait '"$PLUGINSDIR\app\cloudgime-host-bootstrap.exe" install --silent --require-universal --install-root "$INSTDIR" --release-root "$PLUGINSDIR\app" --bundle-source-root "$PLUGINSDIR\bundle"' $0
bootstrap_done:
  ${If} $0 != 0
    SetErrorLevel 1
    IfSilent +2
    MessageBox MB_ICONSTOP "Cloudgime Host setup failed.$\r$\nExit code: $0"
    Abort
  ${EndIf}
SectionEnd
