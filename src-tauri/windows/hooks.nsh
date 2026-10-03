!include LogicLib.nsh

; Upgrade behaviour for the NSIS installer.
;
; 0.3 and anything older has no host DLL. The app is closed and the files are
; replaced; Explorer stays up.
;
; 0.4 and newer load trailist_host.dll into Explorer. That file stays locked
; until the shell process is gone, so Explorer is restarted once before the
; new DLL is copied. The same restart runs on uninstall, or the file delete
; fails and the old DLL stays mapped.

!macro TrayList.StopApp
  ExecWait '"$SYSDIR\taskkill.exe" /F /IM ${MAINBINARYNAME}.exe' $0
  ExecWait '"$SYSDIR\taskkill.exe" /F /IM TrayList.exe' $0
  ExecWait '"$SYSDIR\taskkill.exe" /F /IM trailist.exe' $0
  Sleep 400
!macroend

; taskkill returns as soon as the signal is sent. Wait until the shell is
; actually back, and start it if Windows did not.
!macro TrayList.RestartExplorer
  DetailPrint "Restarting Explorer so trailist_host.dll can be replaced."
  ExecWait '"$SYSDIR\taskkill.exe" /F /IM explorer.exe' $0
  StrCpy $R6 0
  ${Do}
    Sleep 400
    IntOp $R6 $R6 + 1
    nsExec::ExecToStack 'cmd /c tasklist /NH /FI "IMAGENAME eq explorer.exe"'
    Pop $0
    Pop $1
    ${StrLoc} $R7 $1 "explorer.exe" ">"
    ${If} $R7 != ""
      ${ExitDo}
    ${EndIf}
    ${If} $R6 >= 20
      Exec '"$WINDIR\explorer.exe"'
      ${ExitDo}
    ${EndIf}
  ${Loop}
!macroend

; $R5 = 1 when the installed copy is 0.4 or newer, or the host DLL is already
; sitting in the install directory (the registry version is gone after an
; uninstall-first upgrade, the file is not).
!macro TrayList.HostNeedsShellRestart
  StrCpy $R5 0
  ReadRegStr $R0 SHCTX "${UNINSTKEY}" "DisplayVersion"
  ${If} $R0 != ""
    nsis_tauri_utils::SemverCompare $R0 "0.4.0"
    Pop $R1
    ${If} $R1 >= 0
      StrCpy $R5 1
      DetailPrint "TrayList $R0 is installed. The host DLL is replaced after Explorer restarts."
    ${Else}
      DetailPrint "TrayList $R0 is installed. Replacing it without restarting Explorer."
    ${EndIf}
  ${EndIf}
  ${If} ${FileExists} "$INSTDIR\trailist_host.dll"
    StrCpy $R5 1
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro TrayList.StopApp
  !insertmacro TrayList.HostNeedsShellRestart
  ${If} $R5 = 1
    !insertmacro TrayList.RestartExplorer
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro TrayList.StopApp
  ${If} ${FileExists} "$INSTDIR\trailist_host.dll"
    !insertmacro TrayList.RestartExplorer
  ${EndIf}
!macroend
