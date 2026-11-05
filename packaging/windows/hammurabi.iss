; The Windows installer. build-installer.ps1 runs this; it needs /DAppVersion and /DStage.
; All builds share one AppId, so installing another build replaces the one there.

#define AppExe "hammurabi.exe"
#define AppLock "dev.menzo.hammurabi"

[Setup]
AppId={{7F2E4B1C-9A3D-4E86-B5C7-2D1F6A8E0C43}
AppName=Hammurabi
AppVersion={#AppVersion}
AppVerName=Hammurabi {#AppVersion}
AppPublisher=Can Ozmen
AppPublisherURL=https://hammurabi.menzo.dev
AppSupportURL=https://github.com/canmenzo/hammurabi/issues
AppUpdatesURL=https://github.com/canmenzo/hammurabi/releases/latest
VersionInfoVersion={#AppVersion}
; Per user, so it needs no administrator rights and lands in %LOCALAPPDATA%\Programs.
PrivilegesRequired=lowest
DefaultDirName={autopf}\Hammurabi
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
SetupIconFile=..\..\assets\hammurabi.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName=Hammurabi
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
LZMAUseSeparateProcess=yes
; PrepareToInstall stops the running copy, which keeps its window open against a plain close.
CloseApplications=no
RestartApplications=no

[Tasks]
Name: desktopicon; Description: "Create a desktop shortcut"

[InstallDelete]
; The CUDA build's DLLs would be left behind when switching to another build.
Type: files; Name: "{app}\*.dll"

[Files]
Source: "{#Stage}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{autoprograms}\Hammurabi"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\Hammurabi"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; Start at login is switched on inside the app, so only the uninstaller touches these.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "Hammurabi"; Flags: dontcreatekey uninsdeletevalue
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"; ValueType: none; ValueName: "Hammurabi"; Flags: dontcreatekey uninsdeletevalue

[Run]
Filename: "{app}\{#AppExe}"; Description: "Start Hammurabi"; Flags: nowait postinstall skipifsilent

[Code]
const
  StillRunning = 'Hammurabi is still running. Quit it from its tray icon, then try again.';
  TrayRows = 'Control Panel\NotifyIconSettings';

function StopRunningCopy(): Boolean;
var
  ResultCode, I: Integer;
  Exe: String;
begin
  Exe := ExpandConstant('{app}\{#AppExe}');
  if CheckForMutexes('{#AppLock}') and FileExists(Exe) then begin
    Exec(Exe, '--quit', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
    I := 0;
    while CheckForMutexes('{#AppLock}') and (I < 100) do begin
      Sleep(100);
      I := I + 1;
    end;
  end;
  Result := not CheckForMutexes('{#AppLock}');
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  if not StopRunningCopy() then
    Result := StillRunning;
end;

function InitializeUninstall(): Boolean;
begin
  Result := StopRunningCopy();
  if not Result then
    SuppressibleMsgBox(StillRunning, mbError, MB_OK, IDOK);
end;

// Windows keeps a row in Settings > Taskbar for every program that had a tray icon.
procedure RemoveTrayRow();
var
  Rows: TArrayOfString;
  I: Integer;
  Path: String;
begin
  if RegGetSubkeyNames(HKCU, TrayRows, Rows) then
    for I := 0 to GetArrayLength(Rows) - 1 do
      if RegQueryStringValue(HKCU, TrayRows + '\' + Rows[I], 'ExecutablePath', Path)
        and (CompareText(Path, ExpandConstant('{app}\{#AppExe}')) = 0) then
        RegDeleteKeyIncludingSubkeys(HKCU, TrayRows + '\' + Rows[I]);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep <> usPostUninstall then
    Exit;
  RemoveTrayRow();
  if SuppressibleMsgBox('Also delete your settings and downloaded models? Models can take several GB.',
    mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDNO) = IDYES then begin
    DelTree(ExpandConstant('{userappdata}\menzo\Hammurabi'), True, True, True);
    DelTree(ExpandConstant('{localappdata}\menzo\Hammurabi'), True, True, True);
    RemoveDir(ExpandConstant('{userappdata}\menzo'));
    RemoveDir(ExpandConstant('{localappdata}\menzo'));
  end;
end;
