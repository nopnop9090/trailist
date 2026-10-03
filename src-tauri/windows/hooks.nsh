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

; The running app owns a hidden window, class "TrayList.Instance", title
; "TrayList". Its executable name is not part of the contract: a debug build
; and an installed build do not share one. `TrayList.Shutdown` is a registered
; message (not `WM_USER`, which other windows would interpret). `wParam` is
; 0x54594C31, the same constant as `win/instance.rs`. The reply is 1 when the
; process has accepted and is leaving.
;
; A copy from before that window existed can only be the file this installer
; itself placed. That one is matched by full path, never by file name.
!macro TrayList.StopApp
  DetailPrint "Asking TrayList to exit."
  System::Call 'user32::RegisterWindowMessageW(w "TrayList.Shutdown") i .r1'
  ${If} $1 <> 0
    StrCpy $R6 0
    ${Do}
      System::Call 'user32::FindWindowW(w "TrayList.Instance", w "TrayList") p .r2'
      ${If} $2 = 0
        ${ExitDo}
      ${EndIf}
      IntOp $R6 $R6 + 1
      ${If} $R6 > 8
        DetailPrint "Stopped looking for further TrayList windows."
        ${ExitDo}
      ${EndIf}
      System::Call 'user32::GetWindowThreadProcessId(p r2, *i .r3) i .r0'
      ; PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | SYNCHRONIZE
      System::Call 'kernel32::OpenProcess(i 0x101001, i 0, i r3) p .r4'
      ${If} $4 <> 0
        System::Alloc 2048
        Pop $7
        System::Call 'kernel32::QueryFullProcessImageNameW(p r4, i 0, p r7, *i 1024) i .r8'
        ${If} $8 <> 0
          System::Call '*$7(&w1024 .r9)'
          DetailPrint "Closing $9"
        ${Else}
          DetailPrint "Closing TrayList process $3"
        ${EndIf}
        System::Free $7
        ; SMTO_ABORTIFHUNG, five seconds. $5 is the reply, $6 is whether it arrived.
        System::Call 'user32::SendMessageTimeoutW(p r2, i r1, i 0x54594C31, i 0, i 2, i 5000, *i .r5) i .r6'
        ${If} $6 <> 0
        ${AndIf} $5 = 1
          System::Call 'kernel32::WaitForSingleObject(p r4, i 8000) i .r8'
        ${Else}
          StrCpy $8 258
        ${EndIf}
        ${If} $8 <> 0
          DetailPrint "TrayList process $3 did not exit; ending it."
          System::Call 'kernel32::TerminateProcess(p r4, i 1) i .r0'
          System::Call 'kernel32::WaitForSingleObject(p r4, i 2000) i .r0'
        ${EndIf}
        System::Call 'kernel32::CloseHandle(p r4)'
      ${EndIf}
    ${Loop}
  ${Else}
    DetailPrint "The shutdown message could not be registered."
  ${EndIf}

  StrCpy $R0 "$INSTDIR\${MAINBINARYNAME}.exe"
  DetailPrint "Stopping $R0 if a copy from before the shutdown window is still running."
  System::Call 'kernel32::SetEnvironmentVariableW(w "TRAYLIST_STOP", w R0) i .r0'
  ExecWait '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -WindowStyle Hidden -Command "Get-CimInstance Win32_Process | Where-Object { $$_.ExecutablePath -eq $$env:TRAYLIST_STOP } | ForEach-Object { Stop-Process -Id $$_.ProcessId -Force }"' $0
  System::Call 'kernel32::SetEnvironmentVariableW(w "TRAYLIST_STOP", p 0) i .r0'
!macroend

; taskkill returns as soon as the signal is sent. Wait until Shell_TrayWnd
; exists again, and start the shell if Windows did not. FindWindow is used
; instead of a string search: the uninstall section cannot call ordinary
; NSIS functions, and this macro runs there too.
!macro TrayList.RestartExplorer
  DetailPrint "Restarting Explorer so trailist_host.dll can be replaced."
  ExecWait '"$SYSDIR\taskkill.exe" /F /IM explorer.exe' $0
  StrCpy $R6 0
  ${Do}
    Sleep 400
    IntOp $R6 $R6 + 1
    System::Call 'user32::FindWindowW(w "Shell_TrayWnd", p 0) p .r0'
    ${If} $0 <> 0
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
