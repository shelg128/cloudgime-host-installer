!include "MUI2.nsh"
!include "LogicLib.nsh"

!ifndef PRODUCT_NAME
!define PRODUCT_NAME "Cloudgime Host"
!endif
!ifndef PRODUCT_VERSION
!define PRODUCT_VERSION "0.1.0"
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

  ExecWait '"$PLUGINSDIR\app\cloudgime-host-bootstrap.exe" install --install-root "$INSTDIR" --release-root "$PLUGINSDIR\app" --bundle-source-root "$PLUGINSDIR\bundle"' $0
  ${If} $0 != 0
    MessageBox MB_ICONSTOP "Cloudgime Host setup failed.$\r$\nExit code: $0"
    Abort
  ${EndIf}
SectionEnd
