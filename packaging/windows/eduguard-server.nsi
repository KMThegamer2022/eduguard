; EduGuard Server — Windows installer (NSIS, x64)
; Build: makensis packaging/windows/eduguard-server.nsi
;   (expects eduguard-server.exe next to this .nsi, or set EG_BIN)

!ifndef PRODUCT_VERSION
  !define PRODUCT_VERSION "0.1.0"
!endif
!ifndef EG_BIN
  !define EG_BIN "eduguard-server.exe"
!endif

!define PRODUCT_NAME "EduGuard Server"
!define PRODUCT_ID "EduGuardServer"
!define SERVICE_NAME "EduGuard Server"

Name "${PRODUCT_NAME} ${PRODUCT_VERSION}"
OutFile "eduguard-server-${PRODUCT_VERSION}-setup.exe"
InstallDir "$PROGRAMFILES64\EduGuard\Server"
InstallDirRegKey HKLM "Software\${PRODUCT_ID}" "InstallDir"
RequestExecutionLevel admin
Unicode true

!include "MUI2.nsh"
!include "FileFunc.nsh"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Var AdminPassword

Section "EduGuard Server" SEC_MAIN
  SetOutPath "$INSTDIR"
  File "${EG_BIN}"
  File /oname=start-server.cmd "start-server.cmd"

  ; Uninstall entry
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_ID}" "DisplayName" "${PRODUCT_NAME}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_ID}" "DisplayVersion" "${PRODUCT_VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_ID}" "Publisher" "EduGuard"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_ID}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKLM "Software\${PRODUCT_ID}" "InstallDir" "$INSTDIR"

  ; First run: create C:\ProgramData\EduGuard\server\server.toml and capture
  ; the generated admin password for the finish screen.
  DetailPrint "Generating console admin credentials..."
  nsExec::ExecToStack '"$INSTDIR\${EG_BIN}" reset-admin-password'
  Pop $0    ; exit code
  Pop $1    ; stdout (password)
  StrCpy $AdminPassword $1
  DetailPrint "Admin password: $AdminPassword"

  ; Start now and at boot (runs as SYSTEM — no interactive logon needed),
  ; plus allow inbound console/agent traffic on the default port.
  nsExec::ExecToLog 'schtasks /Create /F /TN "${SERVICE_NAME}" /SC ONSTART /RU SYSTEM /TR "$\"$INSTDIR\start-server.cmd$\""'
  nsExec::ExecToLog 'schtasks /Run /TN "${SERVICE_NAME}"'
  nsExec::ExecToLog 'netsh advfirewall firewall add rule name="${SERVICE_NAME}" dir=in action=allow protocol=TCP localport=8443'

  CreateDirectory "$SMPROGRAMS\EduGuard"
  CreateShortCut "$SMPROGRAMS\EduGuard\EduGuard Console.lnk" "http://localhost:8443/"
SectionEnd

Function .onInstSuccess
  MessageBox MB_OK|MB_ICONINFORMATION \
    "${PRODUCT_NAME} installed.$\r$\n$\r$\n\
    Console:      http://localhost:8443/$\r$\n\
    Username:     admin$\r$\n\
    Password:     $AdminPassword$\r$\n\
    (also stored in C:\ProgramData\EduGuard\server\server.toml,$\r$\n\
    rotate with:  eduguard-server reset-admin-password)$\r$\n$\r$\n\
    Create an enrollment token:  eduguard-server new-enrollment-token --label classroom"
FunctionEnd

Section "Uninstall"
  nsExec::ExecToLog 'schtasks /End /TN "${SERVICE_NAME}"'
  nsExec::ExecToLog 'schtasks /Delete /F /TN "${SERVICE_NAME}"'
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="${SERVICE_NAME}"'
  Delete "$INSTDIR\eduguard-server.exe"
  Delete "$INSTDIR\start-server.cmd"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\EduGuard\EduGuard Console.lnk"
  RMDir "$SMPROGRAMS\EduGuard"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_ID}"
  DeleteRegKey HKLM "Software\${PRODUCT_ID}"
  ; Data/config in C:\ProgramData\EduGuard\server are intentionally kept.
SectionEnd
