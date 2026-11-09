#ifndef AppVersion
  #define AppVersion "0.2.0"
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
function IsUpdate: Boolean;
begin
  Result := ExpandConstant('{param:UPDATE|0}') = '1';
end;

procedure InitializeWizard;
begin
  WizardForm.Caption := 'Quiet.md';
end;
