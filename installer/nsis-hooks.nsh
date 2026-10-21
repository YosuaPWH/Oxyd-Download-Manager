; Oxyd Download Manager - NSIS hooks for the Tauri installer.
; Registers oxyd-bridge.exe as the native messaging host for Chrome and Edge (HKCU)
; and removes the registration on uninstall. Keep the extension IDs in sync with
; EXTENSION_IDS in crates/oxyd-protocol/src/lib.rs.

!define OXYD_HOST "com.oxyd.bridge"
!define OXYD_HOST_DIR "$LOCALAPPDATA\Oxyd\native-host"
!define OXYD_CHROME_KEY "Software\Google\Chrome\NativeMessagingHosts\${OXYD_HOST}"
!define OXYD_EDGE_KEY "Software\Microsoft\Edge\NativeMessagingHosts\${OXYD_HOST}"

!macro NSIS_HOOK_POSTINSTALL
  ; JSON needs every backslash of the bridge path doubled.
  StrCpy $R0 "$INSTDIR\oxyd-bridge.exe"
  StrCpy $R1 ""
  StrCpy $R2 0
  oxyd_escape_loop:
    StrCpy $R3 $R0 1 $R2
    StrCmp $R3 "" oxyd_escape_done
    StrCmp $R3 "\" 0 +3
      StrCpy $R1 "$R1\\"
      Goto oxyd_escape_next
    StrCpy $R1 "$R1$R3"
  oxyd_escape_next:
    IntOp $R2 $R2 + 1
    Goto oxyd_escape_loop
  oxyd_escape_done:

  CreateDirectory "${OXYD_HOST_DIR}"
  FileOpen $R4 "${OXYD_HOST_DIR}\${OXYD_HOST}.json" w
  FileWrite $R4 "{$\r$\n"
  FileWrite $R4 "  $\"name$\": $\"${OXYD_HOST}$\",$\r$\n"
  FileWrite $R4 "  $\"description$\": $\"Oxyd native messaging bridge$\",$\r$\n"
  FileWrite $R4 "  $\"path$\": $\"$R1$\",$\r$\n"
  FileWrite $R4 "  $\"type$\": $\"stdio$\",$\r$\n"
  FileWrite $R4 "  $\"allowed_origins$\": [$\r$\n"
  FileWrite $R4 "    $\"chrome-extension://gnihamodjbioaennpopmldmjdecbmfpl/$\"$\r$\n"
  FileWrite $R4 "  ]$\r$\n"
  FileWrite $R4 "}$\r$\n"
  FileClose $R4

  WriteRegStr HKCU "${OXYD_CHROME_KEY}" "" "${OXYD_HOST_DIR}\${OXYD_HOST}.json"
  WriteRegStr HKCU "${OXYD_EDGE_KEY}" "" "${OXYD_HOST_DIR}\${OXYD_HOST}.json"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegKey HKCU "${OXYD_CHROME_KEY}"
  DeleteRegKey HKCU "${OXYD_EDGE_KEY}"
  Delete "${OXYD_HOST_DIR}\${OXYD_HOST}.json"
  RMDir "${OXYD_HOST_DIR}"
  ; Autostart entry written by tauri-plugin-autostart.
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Oxyd Download Manager"
!macroend
