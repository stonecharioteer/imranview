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

  DeleteRegKey HKLM "Software\\ImranView"
  DeleteRegKey HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ImranView"
SectionEnd
