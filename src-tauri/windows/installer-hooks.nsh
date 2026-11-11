; Hooks for the Tauri NSIS template (tauri-cli 2.11.4). See docs/WINDOWS_UPDATE.md.
;
; The template includes this file near its top, before its own !defines
; (PRODUCTNAME, VERSION, UNINSTKEY ...) and variables ($PassiveMode,
; $UpdateMode ...). Code here may only use NSIS built-ins and the headers the
; template includes first (MUI2, FileFunc, x64/LogicLib, WordFunc). Macros
; are expanded where the template inserts them, so they can use $INSTDIR.
;
; Rule: when a release removes or renames a bundled resource, check whether
; NSIS_HOOK_POSTINSTALL must delete the old path. Update mode overwrites in
; place and never runs the previous version's uninstaller.

; ---------------------------------------------------------------------------
; Upgrade in place instead of the uninstall-first wizard (approved under the
; Architecture Approval Gate by Hogan, 2026-11-11).
;
; App releases up to 0.7.5 start the downloaded installer with no arguments,
; and users double-click it. The template then shows its "already installed"
; page, whose default runs the old uninstaller first. When this installer has
; no arguments at all and an older copy of the same product is registered,
; restart it with the template's own update switches (what the app passes
; from now on, and what tauri-plugin-updater passes):
;   /P       passive: progress bar only, closes when done
;   /UPDATE  overwrite in place, never run the old uninstaller
;   /R       relaunch the app afterwards
; Anything that cannot be read or compared keeps the normal wizard, as does
; the same version (repair/uninstall page) and a downgrade.
;
; MUI2 inserts .onGUIInit with the first MUI_LANGUAGE, after the template's
; .onInit has run SetContext (SHCTX = HKCU for this currentUser installer),
; and calls MUI_CUSTOMFUNCTION_GUIINIT before the first page is shown. Silent
; installs (/S) never reach .onGUIInit.
; ---------------------------------------------------------------------------
!define MUI_CUSTOMFUNCTION_GUIINIT VibeUsageUpgradeInPlace

Function VibeUsageUpgradeInPlace
  Push $0
  Push $1
  Push $2

  ; Any argument (/P, /UPDATE, /NS, /D= ...) means the caller chose a mode.
  ${GetParameters} $0
  StrCmp $0 "" 0 vibe_upgrade_wizard

  ; $(^Name) is the compiled product name: the external-test build shares
  ; this file as "Vibe Usage Test", a separate installation.
  ReadRegStr $0 SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\$(^Name)" "UninstallString"
  StrCmp $0 "" vibe_upgrade_wizard
  ReadRegStr $0 SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\$(^Name)" "DisplayVersion"
  StrCmp $0 "" vibe_upgrade_wizard

  ; This installer's own version: the template's VIProductVersion (X.Y.Z.0),
  ; which NSIS also writes as the file version.
  ClearErrors
  ${GetFileVersion} "$EXEPATH" $1
  IfErrors vibe_upgrade_wizard
  StrCmp $1 "" vibe_upgrade_wizard

  ; 1 = this installer is newer than the registered copy.
  ${VersionCompare} $1 $0 $2
  StrCmp $2 "1" 0 vibe_upgrade_wizard

  ClearErrors
  Exec '"$EXEPATH" /P /UPDATE /R'
  IfErrors vibe_upgrade_wizard
  Quit

  vibe_upgrade_wizard:
  Pop $2
  Pop $1
  Pop $0
FunctionEnd

; Releases up to 0.7.2 bundled a vendored CLI under $INSTDIR\cli. An update
; overwrites in place, so remove that obsolete payload here, only when both of
; its entry files show it is ours.
!macro NSIS_HOOK_POSTINSTALL
  ${If} ${FileExists} "$INSTDIR\cli\bin\vibe-usage.js"
  ${AndIf} ${FileExists} "$INSTDIR\cli\package.json"
    RMDir /r "$INSTDIR\cli"
  ${EndIf}
!macroend

; Retry only the known bundled runtime after normal Tauri uninstall. Never
; kill processes by filename or recursively delete user configuration/data.
!macro NSIS_HOOK_POSTUNINSTALL
  Push $0
  StrCpy $0 30
  vibe_node_delete_retry:
    IfFileExists "$INSTDIR\node\node.exe" 0 vibe_node_delete_done
    ClearErrors
    Delete "$INSTDIR\node\node.exe"
    IfErrors 0 vibe_node_delete_done
    Sleep 100
    IntOp $0 $0 - 1
    IntCmp $0 0 vibe_node_delete_deferred vibe_node_delete_deferred vibe_node_delete_retry
  vibe_node_delete_deferred:
    ; Windows can still hold an executable briefly (e.g. an AV scan). Record
    ; reboot-required cleanup instead of silently abandoning the runtime.
    Delete /REBOOTOK "$INSTDIR\node\node.exe"
    SetRebootFlag true
    SetErrorLevel 3010
  vibe_node_delete_done:
    RMDir /REBOOTOK "$INSTDIR\node"
    ; Only remove empty packaged directories left by a transient cwd lock.
    RMDir "$INSTDIR\cli\src\quotas\providers"
    RMDir "$INSTDIR\cli\src\quotas"
    RMDir "$INSTDIR\cli\src\parsers"
    RMDir "$INSTDIR\cli\src"
    RMDir "$INSTDIR\cli\bin"
    RMDir "$INSTDIR\cli"
    RMDir /REBOOTOK "$INSTDIR"
  Pop $0
!macroend
