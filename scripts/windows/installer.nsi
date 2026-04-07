!include "MUI2.nsh"

!ifndef VERSION
!define VERSION "dev"
!endif

!ifndef OUTFILE
!define OUTFILE "imranview-setup.exe"
!endif

!ifndef BIN
!define BIN "target\\release\\imranview.exe"
!endif

Name "ImranView ${VERSION}"
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\\ImranView"
InstallDirRegKey HKLM "Software\\ImranView" "InstallPath"
RequestExecutionLevel admin

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "Install"
  SetOutPath "$INSTDIR"
  File "${BIN}"

  WriteRegStr HKLM "Software\\ImranView" "InstallPath" "$INSTDIR"

  WriteUninstaller "$INSTDIR\\Uninstall.exe"

  CreateDirectory "$SMPROGRAMS\\ImranView"
  CreateShortCut "$SMPROGRAMS\\ImranView\\ImranView.lnk" "$INSTDIR\\imranview.exe"
  CreateShortCut "$SMPROGRAMS\\ImranView\\Uninstall ImranView.lnk" "$INSTDIR\\Uninstall.exe"
  CreateShortCut "$DESKTOP\\ImranView.lnk" "$INSTDIR\\imranview.exe"

  ; Register Open With + file associations for common image formats.
  WriteRegStr HKCR "Applications\\imranview.exe\\shell\\open\\command" "" "$\"$INSTDIR\\imranview.exe$\" $\"%1$\""
  WriteRegStr HKCR "Applications\\imranview.exe\\SupportedTypes" ".avif" ""
  WriteRegStr HKCR "Applications\\imranview.exe\\SupportedTypes" ".bmp" ""
  WriteRegStr HKCR "Applications\\imranview.exe\\SupportedTypes" ".gif" ""
  WriteRegStr HKCR "Applications\\imranview.exe\\SupportedTypes" ".heic" ""
  WriteRegStr HKCR "Applications\\imranview.exe\\SupportedTypes" ".heif" ""
  WriteRegStr HKCR "Applications\\imranview.exe\\SupportedTypes" ".hdr" ""
  WriteRegStr HKCR "Applications\\imranview.exe\\SupportedTypes" ".ico" ""
  WriteRegStr HKCR "Applications\\imranview.exe\\SupportedTypes" ".jpeg" ""
  WriteRegStr HKCR "Applications\\imranview.exe\\SupportedTypes" ".jpg" ""
  WriteRegStr HKCR "Applications\\imranview.exe\\SupportedTypes" ".pbm" ""
  WriteRegStr HKCR "Applications\\imranview.exe\\SupportedTypes" ".pgm" ""
  WriteRegStr HKCR "Applications\\imranview.exe\\SupportedTypes" ".png" ""
  WriteRegStr HKCR "Applications\\imranview.exe\\SupportedTypes" ".pnm" ""
  WriteRegStr HKCR "Applications\\imranview.exe\\SupportedTypes" ".ppm" ""
  WriteRegStr HKCR "Applications\\imranview.exe\\SupportedTypes" ".qoi" ""
  WriteRegStr HKCR "Applications\\imranview.exe\\SupportedTypes" ".tif" ""
  WriteRegStr HKCR "Applications\\imranview.exe\\SupportedTypes" ".tiff" ""
  WriteRegStr HKCR "Applications\\imranview.exe\\SupportedTypes" ".webp" ""

  WriteRegStr HKCR "ImranView.Image" "" "ImranView Image"
  WriteRegStr HKCR "ImranView.Image\\DefaultIcon" "" "$INSTDIR\\imranview.exe,0"
  WriteRegStr HKCR "ImranView.Image\\shell\\open\\command" "" "$\"$INSTDIR\\imranview.exe$\" $\"%1$\""

  WriteRegStr HKCR ".avif\\OpenWithProgids" "ImranView.Image" ""
  WriteRegStr HKCR ".bmp\\OpenWithProgids" "ImranView.Image" ""
  WriteRegStr HKCR ".gif\\OpenWithProgids" "ImranView.Image" ""
  WriteRegStr HKCR ".heic\\OpenWithProgids" "ImranView.Image" ""
  WriteRegStr HKCR ".heif\\OpenWithProgids" "ImranView.Image" ""
  WriteRegStr HKCR ".hdr\\OpenWithProgids" "ImranView.Image" ""
  WriteRegStr HKCR ".ico\\OpenWithProgids" "ImranView.Image" ""
  WriteRegStr HKCR ".jpeg\\OpenWithProgids" "ImranView.Image" ""
  WriteRegStr HKCR ".jpg\\OpenWithProgids" "ImranView.Image" ""
  WriteRegStr HKCR ".pbm\\OpenWithProgids" "ImranView.Image" ""
  WriteRegStr HKCR ".pgm\\OpenWithProgids" "ImranView.Image" ""
  WriteRegStr HKCR ".png\\OpenWithProgids" "ImranView.Image" ""
  WriteRegStr HKCR ".pnm\\OpenWithProgids" "ImranView.Image" ""
  WriteRegStr HKCR ".ppm\\OpenWithProgids" "ImranView.Image" ""
  WriteRegStr HKCR ".qoi\\OpenWithProgids" "ImranView.Image" ""
  WriteRegStr HKCR ".tif\\OpenWithProgids" "ImranView.Image" ""
  WriteRegStr HKCR ".tiff\\OpenWithProgids" "ImranView.Image" ""
  WriteRegStr HKCR ".webp\\OpenWithProgids" "ImranView.Image" ""

  WriteRegStr HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ImranView" "DisplayName" "ImranView"
  WriteRegStr HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ImranView" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ImranView" "Publisher" "stonecharioteer"
  WriteRegStr HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ImranView" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ImranView" "UninstallString" "$\"$INSTDIR\\Uninstall.exe$\""
SectionEnd

Section "Uninstall"
  Delete "$DESKTOP\\ImranView.lnk"
  Delete "$SMPROGRAMS\\ImranView\\ImranView.lnk"
  Delete "$SMPROGRAMS\\ImranView\\Uninstall ImranView.lnk"
  RMDir "$SMPROGRAMS\\ImranView"

  Delete "$INSTDIR\\imranview.exe"
  Delete "$INSTDIR\\Uninstall.exe"
  RMDir "$INSTDIR"

  DeleteRegValue HKCR ".avif\\OpenWithProgids" "ImranView.Image"
  DeleteRegValue HKCR ".bmp\\OpenWithProgids" "ImranView.Image"
  DeleteRegValue HKCR ".gif\\OpenWithProgids" "ImranView.Image"
  DeleteRegValue HKCR ".heic\\OpenWithProgids" "ImranView.Image"
  DeleteRegValue HKCR ".heif\\OpenWithProgids" "ImranView.Image"
  DeleteRegValue HKCR ".hdr\\OpenWithProgids" "ImranView.Image"
  DeleteRegValue HKCR ".ico\\OpenWithProgids" "ImranView.Image"
  DeleteRegValue HKCR ".jpeg\\OpenWithProgids" "ImranView.Image"
  DeleteRegValue HKCR ".jpg\\OpenWithProgids" "ImranView.Image"
  DeleteRegValue HKCR ".pbm\\OpenWithProgids" "ImranView.Image"
  DeleteRegValue HKCR ".pgm\\OpenWithProgids" "ImranView.Image"
  DeleteRegValue HKCR ".png\\OpenWithProgids" "ImranView.Image"
  DeleteRegValue HKCR ".pnm\\OpenWithProgids" "ImranView.Image"
  DeleteRegValue HKCR ".ppm\\OpenWithProgids" "ImranView.Image"
  DeleteRegValue HKCR ".qoi\\OpenWithProgids" "ImranView.Image"
  DeleteRegValue HKCR ".tif\\OpenWithProgids" "ImranView.Image"
  DeleteRegValue HKCR ".tiff\\OpenWithProgids" "ImranView.Image"
  DeleteRegValue HKCR ".webp\\OpenWithProgids" "ImranView.Image"

  DeleteRegKey HKCR "Applications\\imranview.exe"
  DeleteRegKey HKCR "ImranView.Image"

  DeleteRegKey HKLM "Software\\ImranView"
  DeleteRegKey HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ImranView"
SectionEnd
