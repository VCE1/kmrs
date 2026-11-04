; Per-user KMServer installer; pass /DVERSION=x.y.z and /DOUTFILE=path.
; The executable and icon are expected next to this script.

Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!define MUI_ICON "kmrs.ico"
!define MUI_UNICON "kmrs.ico"

!ifndef VERSION
!define VERSION "0.0.0"
!endif
!ifndef OUTFILE
!define OUTFILE "KMServer-setup.exe"
!endif

Name "KMServer"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\KMServer"
InstallDirRegKey HKCU "Software\KMServer" "InstallDir"
RequestExecutionLevel user

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Var UpdatePid
Var ConfigDir
Var BackupDir
Var UpdateFailed

!macro RequireFile source
	ClearErrors
	File "${source}"
	${If} ${Errors}
		StrCpy $UpdateFailed 1
	${EndIf}
!macroend

Function .onInit
	${GetParameters} $0
	${GetOptions} $0 "/UPDATEPID=" $UpdatePid
	${GetOptions} $0 "/CONFIGDIR=" $ConfigDir
	${GetOptions} $0 "/INSTALLDIR=" $1
	${If} $1 != ""
		StrCpy $INSTDIR $1
	${EndIf}
	StrCpy $UpdateFailed 0
	${If} $UpdatePid != ""
		System::Call 'kernel32::OpenProcess(i 0x00100000, i 0, i $UpdatePid) p.r1'
		${If} $1 == 0
			MessageBox MB_OK|MB_ICONSTOP "The running KMServer process could not be found. No files were changed."
			Abort
		${EndIf}
		System::Call 'kernel32::WaitForSingleObject(i r1, i 60000) i.r2'
		System::Call 'kernel32::CloseHandle(i r1)'
		${If} $2 != 0
			MessageBox MB_OK|MB_ICONSTOP "KMServer did not stop in time. No files were changed."
			Abort
		${EndIf}
		StrCpy $BackupDir "$INSTDIR.update-backup"
		IfFileExists "$BackupDir\*.*" 0 +2
		RMDir /r "$BackupDir"
		ClearErrors
		Rename "$INSTDIR" "$BackupDir"
		${If} ${Errors}
			MessageBox MB_OK|MB_ICONSTOP "The current installation could not be preserved. KMServer was not changed."
			Abort
		${EndIf}
		CreateDirectory "$INSTDIR"
	${EndIf}
FunctionEnd

Function .onInstSuccess
	Call FinishUpdate
FunctionEnd

Function .onInstFailed
	StrCpy $UpdateFailed 1
	Call FinishUpdate
FunctionEnd

Function FinishUpdate
	${If} $UpdatePid == ""
		Return
	${EndIf}
	${If} $UpdateFailed != 0
		RMDir /r "$INSTDIR"
		ClearErrors
		Rename "$BackupDir" "$INSTDIR"
		${If} ${Errors}
			MessageBox MB_OK|MB_ICONSTOP "The update failed and the previous installation could not be restored. The backup is at $BackupDir."
			Return
		${EndIf}
		MessageBox MB_OK|MB_ICONSTOP "The update could not be installed. The previous installation was restored."
	${Else}
		RMDir /r "$BackupDir"
	${EndIf}
	Call LaunchServer
FunctionEnd

Function LaunchServer
	${If} $ConfigDir == ""
		Exec '"$INSTDIR\kmrs.exe"'
	${Else}
		Exec '"$INSTDIR\kmrs.exe" --config-dir "$ConfigDir"'
	${EndIf}
FunctionEnd

Section "Install"
	SetOutPath "$INSTDIR"
	!insertmacro RequireFile "kmrs.exe"
	!insertmacro RequireFile "kmrs.ico"
	!insertmacro RequireFile "pdfium.dll"
	SetOutPath "$INSTDIR\licenses"
	ClearErrors
	File /r "PDFium"
	${If} ${Errors}
		StrCpy $UpdateFailed 1
	${EndIf}
	WriteUninstaller "$INSTDIR\Uninstall.exe"
	${If} ${Errors}
		StrCpy $UpdateFailed 1
	${EndIf}
	WriteRegStr HKCU "Software\KMServer" "InstallDir" "$INSTDIR"

	CreateDirectory "$SMPROGRAMS\KMServer"
	CreateShortcut "$SMPROGRAMS\KMServer\KMServer.lnk" "$INSTDIR\kmrs.exe" "" "$INSTDIR\kmrs.ico"
	CreateShortcut "$SMPROGRAMS\KMServer\Uninstall.lnk" "$INSTDIR\Uninstall.exe"

	WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\KMServer" "DisplayName" "KMServer"
	WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\KMServer" "DisplayIcon" '"$INSTDIR\kmrs.exe",0'
	WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\KMServer" "UninstallString" '"$INSTDIR\Uninstall.exe"'
	WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\KMServer" "DisplayVersion" "${VERSION}"
	WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\KMServer" "NoModify" 1
	WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\KMServer" "NoRepair" 1
SectionEnd

Section "Uninstall"
	Delete "$INSTDIR\kmrs.exe"
	Delete "$INSTDIR\kmrs.ico"
	Delete "$INSTDIR\pdfium.dll"
	Delete "$INSTDIR\Uninstall.exe"
	RMDir /r "$INSTDIR\licenses"
	RMDir "$INSTDIR"
	Delete "$SMPROGRAMS\KMServer\KMServer.lnk"
	Delete "$SMPROGRAMS\KMServer\Uninstall.lnk"
	RMDir "$SMPROGRAMS\KMServer"
	DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\KMServer"
	DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "KMServer"
	; Compatibility with the old startup registration; remove in 0.27.0.
	DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "KMReader"
	DeleteRegKey HKCU "Software\KMServer"
SectionEnd
