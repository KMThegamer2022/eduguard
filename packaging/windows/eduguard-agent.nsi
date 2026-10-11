; EduGuard Agent — Windows installer (NSIS, x64)
; Build: makensis packaging/windows/eduguard-agent.nsi
; Unattended enrollment:
;   eduguard-agent-<ver>-setup.exe /S /SERVER=http://server:8443 /TOKEN=eg-...

!ifndef PRODUCT_VERSION
  !define PRODUCT_VERSION "0.1.0"
!endif
!ifndef EG_BIN
  !define EG_BIN "eduguard-agent.exe"
!endif

!define PRODUCT_NAME "EduGuard Agent"
!define PRODUCT_ID "EduGuardAgent"

Name "${PRODUCT_NAME} ${PRODUCT_VERSION}"
OutFile "eduguard-agent-${PRODUCT_VERSION}-setup.exe"
InstallDir "$PROGRAMFILES64\EduGuard\Agent"
InstallDirRegKey HKLM "Software\${PRODUCT_ID}" "InstallDir"
RequestExecutionLevel admin
Unicode true

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

Var ServerUrl
Var Token

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/SERVER=" $ServerUrl
  ClearErrors
  ${GetOptions} $R0 "/TOKEN=" $Token
FunctionEnd

Section "EduGuard Agent" SEC_MAIN
  SetOutPath "$INSTDIR"
  File "${EG_BIN}"

  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_ID}" "DisplayName" "${PRODUCT_NAME}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_ID}" "DisplayVersion" "${PRODUCT_VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_ID}" "Publisher" "EduGuard"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_ID}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKLM "Software\${PRODUCT_ID}" "InstallDir" "$INSTDIR"

  ; The agent must run inside the interactive user session (tray icon +
  ; overlay), so start it per-logon via the HKLM Run key.
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Run" "EduGuardAgent" '"$INSTDIR\${EG_BIN}" run'

  ; Optional unattended enrollment (/SERVER= + /TOKEN=).
  ${If} $ServerUrl != ""
    ${If} $Token != ""
      DetailPrint "Enrolling with $ServerUrl ..."
      nsExec::ExecToLog '"$INSTDIR\${EG_BIN}" enroll --server "$ServerUrl" --token "$Token"'
    ${EndIf}
  ${EndIf}

  ; Start for the current user now.
  Exec '"$INSTDIR\${EG_BIN}" run'
SectionEnd

Function .onInstSuccess
  ${If} $ServerUrl == ""
    MessageBox MB_OK|MB_ICONINFORMATION \
      "${PRODUCT_NAME} installed (starts at each user logon; tray icon in the notification area).$\r$\n$\r$\n\
      To enroll this device:$\r$\n\
      1. On the server:  eduguard-server new-enrollment-token --label classroom$\r$\n\
      2. Here:           eduguard-agent enroll --server http://SERVER:8443 --token eg-..."
  ${Else}
    MessageBox MB_OK|MB_ICONINFORMATION "${PRODUCT_NAME} installed and enrolled with $ServerUrl."
  ${EndIf}
FunctionEnd

Section "Uninstall"
  nsExec::ExecToLog 'taskkill /F /IM ${EG_BIN}'
  DeleteRegValue HKLM "Software\Microsoft\Windows\CurrentVersion\Run" "EduGuardAgent"
  Delete "$INSTDIR\${EG_BIN}"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_ID}"
  DeleteRegKey HKLM "Software\${PRODUCT_ID}"
  ; Identity/config in C:\ProgramData\EduGuard are kept.
SectionEnd
