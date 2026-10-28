!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "StrFunc.nsh"
!include "nsDialogs.nsh"
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
!ifndef HOST_CONTROL_EXE
!define HOST_CONTROL_EXE "cloudgime-host-control.exe"
!endif
!define SINGLE_INSTALLER_WIZARD_TOKEN_PAGE
Name "${PRODUCT_NAME}"
OutFile "${OUTPUT_DIR}\${INSTALLER_FILE}"
VIProductVersion "${PRODUCT_VERSION_QUAD}"
VIAddVersionKey "ProductName" "${PRODUCT_NAME}"
VIAddVersionKey "ProductVersion" "${PRODUCT_VERSION}"
VIAddVersionKey "FileDescription" "${PRODUCT_NAME} Universal Node candidate"
VIAddVersionKey "FileVersion" "${PRODUCT_VERSION}"
VIAddVersionKey "LegalCopyright" "CloudGime"
InstallDir "$PROGRAMFILES64\Cloudgime Host"
RequestExecutionLevel admin
ShowInstDetails hide
!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
Page custom SetupTokenPageCreate SetupTokenPageLeave
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_TEXT_REBOOT "Instalasi CloudGime Host selesai. Komputer harus direstart agar sistem rental CloudGime dan penguncian layar otomatis aktif."
!define MUI_FINISHPAGE_TEXT_REBOOTNOW "Restart Komputer Sekarang (Disarankan)"
!define MUI_FINISHPAGE_TEXT_REBOOTLATER "Saya akan restart nanti secara manual"
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_LANGUAGE "English"


Var SetupTokenInput
Var SetupTokenValue
Var SetupTokenFile
Var HostControlExecutable
Var InstallerDiagnosticStage
Var InstallerDiagnosticCode

; Silent installation must leave an operator-readable, token-free receipt.
; SetShellVarContext all makes $APPDATA resolve to C:\ProgramData.
Function WriteInstallerDiagnostic
  SetShellVarContext all
  CreateDirectory "$APPDATA\CloudGime\installer-diagnostics"
  FileOpen $R8 "$APPDATA\CloudGime\installer-diagnostics\universal-installer-stage.log" a
  ${If} $R8 != ""
    FileWrite $R8 "$InstallerDiagnosticStage|$InstallerDiagnosticCode$\r$\n"
    FileClose $R8
  ${EndIf}
FunctionEnd

Function .onInit
  ; A setup credential is never accepted from argv. CI/remote automation must
  ; provide it only through the short-lived inherited environment, then this
  ; installer passes it to Bootstrap and clears it before the child returns.
  ${GetParameters} $R8
  ${StrStr} $R9 $R8 "/SETUP_TOKEN="
  ${If} $R9 != ""
    SetErrorLevel 87
    Quit
  ${EndIf}
  ; Silent automation supplies a protected *file path*, never a secret argv.
  ; The reference survives UAC elevation; Bootstrap reads and deletes the file
  ; before launching host-installer. The file must be under the protected
  ; ProgramData CloudGime installer-secrets root or Bootstrap fails closed.
  ${GetOptions} "$CMDLINE" "/SETUP_TOKEN_FILE=" $SetupTokenFile
  ${If} ${Silent}
    ${If} $SetupTokenFile == ""
      SetErrorLevel 87
      Quit
    ${EndIf}
  ${EndIf}
FunctionEnd

Function SetupTokenPageCreate
  ${If} ${Silent}
    Abort
  ${EndIf}
  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}
  ${NSD_CreateLabel} 0 0 100% 25u "Masukkan Setup Token / Lisensi CloudGime yang didapatkan dari Web Panel untuk PC ini:$\r$\nLisensi akan divalidasi dan dihubungkan secara otomatis."
  Pop $0
  ${NSD_CreateText} 0 30u 100% 14u ""
  Pop $SetupTokenInput
  ${NSD_CreateLabel} 0 48u 100% 20u "Contoh: stk_pc03_xxxxxxxxxxxx (klik kanan -> Tempel / Ctrl+V)"
  Pop $0
  ${NSD_SetFocus} $SetupTokenInput
  nsDialogs::Show
FunctionEnd

Function SetupTokenPageLeave
  ${If} ${Silent}
    Return
  ${EndIf}
  ${NSD_GetText} $SetupTokenInput $SetupTokenValue
  ${If} $SetupTokenValue == ""
    MessageBox MB_ICONEXCLAMATION "A CloudGime setup token or license is required."
    Abort
  ${EndIf}
  StrLen $0 $SetupTokenValue
  ${If} $0 < 12
    MessageBox MB_ICONEXCLAMATION "The setup token appears incomplete. Check it and try again."
    Abort
  ${EndIf}
  ; Wizard setup already runs in the elevated installer process. Keep the
  ; secret only in that process environment until the immediate Bootstrap
  ; child reads it; Bootstrap clears it before launching host-installer.
  StrCpy $R4 $SetupTokenValue
  System::Call 'Kernel32::SetEnvironmentVariable(t "CLOUDGIME_SETUP_TOKEN", t r4)i.r0'
  IntCmp $0 0 setup_token_environment_failed setup_token_environment_ready setup_token_environment_ready
setup_token_environment_failed:
  MessageBox MB_ICONSTOP "CloudGime could not securely prepare the setup credential."
  Abort
setup_token_environment_ready:
  StrCpy $R4 ""
  StrCpy $SetupTokenValue ""
FunctionEnd

Function RunInlineActivation
  ; The only command-line value is the protected one-time file path, never the
  ; token. Bootstrap validates its ProgramData root ACL, consumes/deletes the
  ; file, then writes the token only to host-installer stdin. The atomic issuer
  ; owns the license claim, credentials and replay receipt; do not use Host
  ; Control's legacy component-only /setup-tokens/claim route here.
  IfFileExists "$INSTDIR\app\cloudgime-host-bootstrap.exe" activation_bootstrap_present activation_missing
activation_bootstrap_present:
  StrCpy $HostControlExecutable "$INSTDIR\app\${HOST_CONTROL_EXE}"
  ; Silent automation supplies the protected file reference. The interactive
  ; wizard keeps the token only in this elevated process environment.
  StrCmp $SetupTokenFile "" activation_from_environment activation_from_file
activation_from_file:
  ExecWait '"$INSTDIR\app\cloudgime-host-bootstrap.exe" provision-installer-license-file "$INSTDIR\data" "$SetupTokenFile" --silent' $0
  Goto activation_invoked
activation_from_environment:
  ExecWait '"$INSTDIR\app\cloudgime-host-bootstrap.exe" provision-installer-license-env "$INSTDIR\data" --silent' $0
activation_invoked:
  StrCpy $InstallerDiagnosticStage "ACTIVATION"
  StrCpy $InstallerDiagnosticCode $0
  Call WriteInstallerDiagnostic
  System::Call 'Kernel32::SetEnvironmentVariable(t "CLOUDGIME_SETUP_TOKEN", t "")i.r1'
  StrCpy $R4 ""
  StrCpy $SetupTokenValue ""
  StrCpy $SetupTokenFile ""
  ; IntCmp branches are: less, equal, greater. Only exit code 0 succeeds.
  IntCmp $0 0 activation_failed activation_ok activation_failed
  activation_ok:
    Return
  activation_missing:
    IfSilent +2
    MessageBox MB_ICONSTOP "CloudGime activation component is missing from this installer bundle. Installation cannot continue."
    Abort
  activation_failed:
    System::Call 'Kernel32::SetEnvironmentVariable(t "CLOUDGIME_SETUP_TOKEN", t "")i.r1'
    StrCpy $R4 ""
    StrCpy $SetupTokenValue ""
    StrCpy $SetupTokenFile ""
    IfSilent +2
    MessageBox MB_ICONSTOP "The setup token could not be activated. Check the token and network connection, then run Setup again."
    Abort
FunctionEnd

Section "Install"
  SetRegView 64
  SetShellVarContext all
  InitPluginsDir
  ; Keep a durable copy of the activation component under the selected node
  ; directory. It owns the DPAPI activation state after the setup token claim.
  SetOutPath "$INSTDIR\app"
  File /r "${PAYLOAD_APP}\*"
  ; Bootstrap still receives a temporary release root so it can validate and
  ; stage the signed Universal bundle before it changes node services.
  SetOutPath "$PLUGINSDIR\app"
  File /r "${PAYLOAD_APP}\*"
  SetOutPath "$PLUGINSDIR\bundle"
  File /r "${PAYLOAD_BUNDLE}\*"
  ; Package contract was checked before NSIS and is checked again before Host changes.
  ExecWait '"$PLUGINSDIR\app\cloudgime-host-bootstrap.exe" validate-universal-bundle --silent --bundle-root "$PLUGINSDIR\bundle"' $0
  StrCpy $InstallerDiagnosticStage "BUNDLE_PREFLIGHT"
  StrCpy $InstallerDiagnosticCode $0
  Call WriteInstallerDiagnostic
  ${If} $0 != 0
    SetErrorLevel 1
    IfSilent +2
    MessageBox MB_ICONSTOP "Universal package integrity validation failed."
    Abort
  ${EndIf}
  ; Validate the immutable bundle before touching local machine state. For an
  ; explicit /FRESH install, archive stale identity first; preflight then sees
  ; the clean state. Ordinary installs retain strict identity validation.
  ${GetParameters} $R0
  ${StrStr} $R1 $R0 "/FRESH"
  StrCmp $R1 "" preflight_install
  ; Reset both the shared ProgramData root and the writable node-data root so
  ; /FRESH archives stale identity regardless of where a previous run stored it.
  ExecWait '"$PLUGINSDIR\app\cloudgime-host-bootstrap.exe" reset-universal-identity' $0
  ${If} $0 == 0
    ExecWait '"$PLUGINSDIR\app\cloudgime-host-bootstrap.exe" reset-universal-identity --state-root "$INSTDIR\data"' $0
  ${EndIf}
  StrCpy $InstallerDiagnosticStage "FRESH_IDENTITY_RESET"
  StrCpy $InstallerDiagnosticCode $0
  Call WriteInstallerDiagnostic
  ${If} $0 != 0
    SetErrorLevel 1
    IfSilent +2
    MessageBox MB_ICONSTOP "CloudGime tidak dapat mengarsipkan identitas node lama untuk fresh install. Kode: $0"
    Abort
  ${EndIf}
preflight_install:
  ; Validate against the exact writable node root used by both the preceding
  ; /FRESH identity archive and the subsequent Bootstrap install. Do not let
  ; preflight silently fall back to a legacy ProgramData root.
  ExecWait '"$PLUGINSDIR\app\cloudgime-host-bootstrap.exe" validate-universal-install --silent --bundle-root "$PLUGINSDIR\bundle" --data-root "$INSTDIR\data"' $0
  StrCpy $InstallerDiagnosticStage "INSTALL_PREFLIGHT"
  StrCpy $InstallerDiagnosticCode $0
  Call WriteInstallerDiagnostic
  ${If} $0 != 0
    SetErrorLevel 1
    IfSilent +2
    MessageBox MB_ICONSTOP "Universal package preflight failed; installed components were not changed."
    Abort
  ${EndIf}  ; Install/upgrade components first, but keep the broker deferred. The inline
  ; claim below remains the only path that activates a node for rental.
  ; Install or upgrade mandatory Sentinel in deferred mode. The broker stays
  ; stopped until Host installation/finalization succeeds and readiness is checked.
  IfFileExists "$PLUGINSDIR\bundle\sentinel\SentinelLauncherSetup.exe" 0 sentinel_missing
  ExecWait '"$PLUGINSDIR\bundle\sentinel\SentinelLauncherSetup.exe" /S /DEFERBROKERSTART' $1
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
  ; Keep the immutable signed bundle source separate from writable node data.
  ExecWait '"$PLUGINSDIR\app\cloudgime-host-bootstrap.exe" install --silent --require-universal --install-root "$INSTDIR" --data-root "$INSTDIR\data" --release-root "$PLUGINSDIR\app" --bundle-source-root "$PLUGINSDIR\bundle" --overwrite-data --reset-local-identity' $0
  Goto bootstrap_done
upgrade_install:
  ; Keep the immutable signed bundle source separate from writable node data.
  ExecWait '"$PLUGINSDIR\app\cloudgime-host-bootstrap.exe" install --silent --require-universal --install-root "$INSTDIR" --data-root "$INSTDIR\data" --release-root "$PLUGINSDIR\app" --bundle-source-root "$PLUGINSDIR\bundle"' $0
bootstrap_done:
  StrCpy $InstallerDiagnosticStage "BOOTSTRAP_INSTALL"
  StrCpy $InstallerDiagnosticCode $0
  Call WriteInstallerDiagnostic
  ${If} $0 != 0
    SetErrorLevel 1
    IfSilent +2
    MessageBox MB_ICONSTOP "Cloudgime Host setup failed.$\r$\nExit code: $0"
    Abort
  ${EndIf}
  ; Components are now present under $INSTDIR. Claim the token using this exact
  ; node's installed Host Control so its protected activation state is durable.
  Call RunInlineActivation

  ; A valid claim is not the acceptance condition. Start the deferred Universal
  ; Node runtime only after credentials exist, then wait for rentalReady:true.
  ; Arm the locally generated first-boot plan only after the universal data
  ; root exists. The finalizer below executes this plan but never creates one.
  ExecWait '"$INSTDIR\app\cloudgime-host-bootstrap.exe" prepare-installer-first-boot "$INSTDIR\data" --silent' $0
  StrCpy $InstallerDiagnosticStage "FIRST_BOOT_PREPARE"
  StrCpy $InstallerDiagnosticCode $0
  Call WriteInstallerDiagnostic
  ${If} $0 != 0
    IfSilent +2
      MessageBox MB_ICONSTOP "CloudGime tidak dapat menyiapkan first boot otomatis. Kode: $0"
    Abort
  ${EndIf}

  ExecWait '"$INSTDIR\app\cloudgime-host-bootstrap.exe" finalize-universal-node --silent --data-root "$INSTDIR\data"' $0
  StrCpy $InstallerDiagnosticStage "UNIVERSAL_FINALIZE"
  StrCpy $InstallerDiagnosticCode $0
  Call WriteInstallerDiagnostic
  ${If} $0 != 0
    SetErrorLevel 1
    IfSilent +2
    MessageBox MB_ICONSTOP "CloudGime services could not be finalized. Installation is not ready for rental.$\r$\nExit code: $0"
    Abort
  ${EndIf}
  ${GetOptions} "$CMDLINE" "/READINESS_TIMEOUT=" $1
  ${If} $1 == ""
    StrCpy $1 "180"
  ${EndIf}
  System::Call 'Kernel32::SetEnvironmentVariable(t "CLOUDGIME_INSTALLER_READINESS_TIMEOUT_SECONDS", t r1)i.r2'
  ; Use the same durable node data root for the readiness credential lookup.
  ExecWait '"$HostControlExecutable" --bundle-root "$INSTDIR\data" --headless-wait-rental-ready --result-json "$APPDATA\CloudGime\installer-diagnostics\cloudgime-readiness-result.json"' $0
  StrCpy $InstallerDiagnosticStage "RENTAL_READINESS"
  StrCpy $InstallerDiagnosticCode $0
  Call WriteInstallerDiagnostic
  System::Call 'Kernel32::SetEnvironmentVariable(t "CLOUDGIME_INSTALLER_READINESS_TIMEOUT_SECONDS", t "")i.r2'
  ${If} $0 != 0
    SetErrorLevel 1
    IfSilent +2
    MessageBox MB_ICONSTOP "CloudGime components were installed, but the control plane did not confirm rentalReady: true. Installation is not complete.$\r$\nCheck network/Sentinel readiness and run Setup again."
    Abort
  ${EndIf}
  SetRebootFlag true
SectionEnd