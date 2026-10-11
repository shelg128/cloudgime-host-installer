!include "MUI2.nsh"
!include "LogicLib.nsh"
Name "CloudGime Managed Upgrade (Internal)"
OutFile "${OUTPUT_FILE}"
RequestExecutionLevel admin
ShowInstDetails show
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
Var OperationId
Section
 InitPluginsDir
 SetOutPath "$PLUGINSDIR\package"
 File /r "${PAYLOAD_ROOT}\*"
 SetOutPath "$PLUGINSDIR\engine"
 File /oname=cloudgime-host-bootstrap.exe "${BOOTSTRAP_FILE}"
 ; GUID is only transaction identity. Authorization remains Windows admin plus
 ; backend device-authenticated idle check and exact local STOPPED state.
 System::Call 'ole32::CoCreateGuid(g .r0) i .r1'
 ${If} $1 != 0
  SetErrorLevel 60
  Abort
 ${EndIf}
 StrCpy $OperationId $0 36 1
 SetShellVarContext all
 DetailPrint "Staging stopped-node upgrade: $OperationId"
 ExecWait '"$PLUGINSDIR\engine\cloudgime-host-bootstrap.exe" managed-upgrade-stage $OperationId "$PLUGINSDIR\package" ${PACKAGE_SHA256} --silent' $1
 ${If} $1 != 0
  SetErrorLevel 60
  Abort
 ${EndIf}
 DetailPrint "Preparing stopped-node upgrade: $OperationId"
 ExecWait '"$APPDATA\CloudGime\ManagedUpgradeRecovery\$OperationId\engine.exe" managed-upgrade prepare $OperationId "$APPDATA\CloudGime\ManagedUpgradeRecovery\$OperationId\payload" --silent' $1
 ${If} $1 != 0
  SetErrorLevel 61
  DetailPrint "Preflight rejected. Existing node was not upgraded."
  Abort
 ${EndIf}
 ExecWait '"$APPDATA\CloudGime\ManagedUpgradeRecovery\$OperationId\engine.exe" managed-upgrade apply $OperationId --silent' $1
 ${If} $1 != 0
  SetErrorLevel 62
  DetailPrint "Upgrade failed. Inspect protected recovery journal; do not resume the node."
  Abort
 ${EndIf}
 DetailPrint "Stopped-node payload upgraded. Sentinel remains stopped; no license activation or automatic resume."
 SetErrorLevel 0
SectionEnd
