#ifndef AppVersion
#define AppVersion "0.2.4"
#endif
#ifndef AppSource
  #define AppSource "..\..\target\release"
#endif

[Setup]
AppId={{0993BAD8-6D4D-508A-905B-AC909F1563DE}
AppName=Quiet.md MyNote
AppVersion={#AppVersion}
AppPublisher=yqhuang2912
AppPublisherURL=https://github.com/yqhuang2912/quiet-md
DefaultDirName={localappdata}\Programs\MyNote
DisableDirPage=no
DisableProgramGroupPage=yes
DisableWelcomePage=yes
DisableReadyPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.19041
OutputDir=..\..\dist\windows
OutputBaseFilename=quiet-md-{#AppVersion}-windows-x64-setup
SetupIconFile=..\..\assets\branding\app-icon.ico
UninstallDisplayIcon={app}\quiet_md.exe
WizardStyle=modern light
WizardSmallImageFile=wizard-logo.bmp
WizardSmallImageBackColor=$FFFFFF
WizardImageStretch=no
WizardSizePercent=105,105
Compression=lzma2
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
ChangesAssociations=yes
SetupLogging=yes
#ifdef Signed
SignTool=quietmd
SignedUninstaller=yes
SignedUninstallerDir={#AppSource}
#endif

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Messages]
SelectDirLabel3=Choose an install folder. Your notes are stored separately.
SelectTasksLabel2=Make Quiet.md easier to reach.
FinishedLabel=Quiet.md is ready. A quiet place for your notes.

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "{#AppSource}\quiet_md.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#AppSource}\vcruntime140.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"
Source: "..\..\THIRD_PARTY.md"; DestDir: "{app}"

[Icons]
Name: "{userprograms}\Quiet.md MyNote"; Filename: "{app}\quiet_md.exe"; WorkingDir: "{app}"
Name: "{userdesktop}\Quiet.md MyNote"; Filename: "{app}\quiet_md.exe"; WorkingDir: "{app}"; Tasks: desktopicon

; "Open with" for Markdown files (not the default app): they are edited in place.
[Registry]
Root: HKA; Subkey: "Software\Classes\MyNote.Markdown"; ValueType: string; ValueName: ""; ValueData: "Markdown"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\MyNote.Markdown\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\quiet_md.exe,0"
Root: HKA; Subkey: "Software\Classes\MyNote.Markdown\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\quiet_md.exe"" ""%1"""
Root: HKA; Subkey: "Software\Classes\.md\OpenWithProgids"; ValueType: string; ValueName: "MyNote.Markdown"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.markdown\OpenWithProgids"; ValueType: string; ValueName: "MyNote.Markdown"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\quiet_md.exe\SupportedTypes"; ValueType: string; ValueName: ".md"; ValueData: ""; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\quiet_md.exe\SupportedTypes"; ValueType: string; ValueName: ".markdown"; ValueData: ""

[Run]
Filename: "{app}\quiet_md.exe"; Description: "Launch Quiet.md"; Flags: nowait postinstall skipifsilent
; The in-app updater installs silently with /UPDATE=1 and expects the app back afterwards.
Filename: "{app}\quiet_md.exe"; Flags: nowait; Check: IsUpdate

[Code]
var
  LibraryPage: TInputDirWizardPage;
  AskLibrary: Boolean;

function LibraryConfig: String;
begin
  Result := ExpandConstant('{localappdata}\io.github.yqhuang2912.mynote\library.json');
end;

function IsUpdate: Boolean;
begin
  Result := ExpandConstant('{param:UPDATE|0}') = '1';
end;

procedure InitializeWizard;
begin
  WizardForm.Caption := 'Quiet.md';
  AskLibrary := not FileExists(LibraryConfig) and not WizardSilent and not IsUpdate;
  LibraryPage := CreateInputDirPage(wpSelectDir,
    'Choose document library / 选择文档库位置',
    'Where should your notes and attachments be saved? / 笔记和附件保存在哪里？',
    'This folder stores your notes, attachments and NAS sync settings. It is separate from the application folder.'#13#10 +
    'Choose an empty folder or an existing MyNote library. Future updates keep this location.'#13#10#13#10 +
    '请选择空文件夹或已有的文档库，后续升级会保留此位置。',
    False, 'Quiet.md');
  LibraryPage.Add('Document library / 文档库文件夹:');
  LibraryPage.Values[0] := ExpandConstant('{userdocs}\Quiet.md');
end;

function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := (PageID = LibraryPage.ID) and not AskLibrary;
end;

function HasContents(const Folder: String): Boolean;
var
  Entry: TFindRec;
begin
  Result := False;
  if FindFirst(AddBackslash(Folder) + '*', Entry) then begin
    try
      repeat
        if (Entry.Name <> '.') and (Entry.Name <> '..') then begin
          Result := True;
          Break;
        end;
      until not FindNext(Entry);
    finally
      FindClose(Entry);
    end;
  end;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
var
  Folder: String;
begin
  Result := True;
  if (CurPageID <> LibraryPage.ID) or not AskLibrary then Exit;
  Folder := RemoveBackslashUnlessRoot(Trim(LibraryPage.Values[0]));
  if ExtractFileDrive(Folder) <> '' then Folder := ExpandFileName(Folder);
  if (ExtractFileDrive(Folder) = '') or (Length(Folder) <= Length(ExtractFileDrive(Folder)) + 1) then begin
    MsgBox('Choose a folder, not a drive root. / 请选择文件夹，不能使用磁盘根目录。', mbError, MB_OK);
    Result := False;
  end else if (CompareText(AddBackslash(Folder), AddBackslash(WizardDirValue)) = 0) or
    (Pos(Lowercase(AddBackslash(WizardDirValue)), Lowercase(AddBackslash(Folder))) = 1) then begin
    MsgBox('Save documents separately from the application folder. / 文档库不能放在程序安装文件夹内。', mbError, MB_OK);
    Result := False;
  end else if FileExists(Folder) or (DirExists(Folder) and HasContents(Folder) and
    not (FileExists(AddBackslash(Folder) + 'format.json') and DirExists(AddBackslash(Folder) + 'docs'))) then begin
    MsgBox('Choose an empty folder or an existing MyNote library. / 请选择空文件夹或已有的文档库。', mbError, MB_OK);
    Result := False;
  end;
  if Result then LibraryPage.Values[0] := Folder;
end;

function JsonPath(Value: String): String;
begin
  StringChangeEx(Value, '\', '\\', True);
  StringChangeEx(Value, '"', '\"', True);
  Result := Value;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Config, Temp, Contents: String;
begin
  if (CurStep <> ssPostInstall) or not AskLibrary or FileExists(LibraryConfig) then Exit;
  Config := LibraryConfig;
  if not ForceDirectories(ExtractFileDir(Config)) then RaiseException('Could not save the library location.');
  repeat
    Temp := Config + '.install-' + IntToStr(Random(2147483647)) + '.tmp';
  until not FileExists(Temp);
  Contents := '{"root":"' + JsonPath(LibraryPage.Values[0]) + '","confirmed":true,"create":true}';
  if not SaveStringToFile(Temp, Utf8Encode(Contents), False) then RaiseException('Could not save the library location.');
  if not RenameFile(Temp, Config) then begin
    DeleteFile(Temp);
    if not FileExists(Config) then RaiseException('Could not save the library location.');
  end;
end;
