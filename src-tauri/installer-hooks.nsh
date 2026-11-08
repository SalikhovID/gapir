; The speech library ships as DLLs (CPU tiers and Vulkan load at runtime). Its build
; script puts them next to gapir.exe, so the installer takes them from there.
!macro NSIS_HOOK_POSTINSTALL
  !searchreplace GAPIR_BIN_DIR "${MAINBINARYSRCPATH}" "${MAINBINARYNAME}.exe" ""
  SetOutPath $INSTDIR
  File "${GAPIR_BIN_DIR}*.dll"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\*.dll"
  RMDir "$INSTDIR"
!macroend
