#if DecodeVer(Ver) != "6.7.3"
    #error Build with the pinned Inno Setup 6.7.3 compiler.
#endif

[Setup]
AppId={{84F36B20-44FD-4F38-9D36-EE19306361E2}
AppName=qplay
AppVersion={#AppVersion}
AppPublisher=qbutt-org
AppPublisherURL=https://github.com/qbutt-org/qplay
AppSupportURL=https://github.com/qbutt-org/qplay/issues
DefaultDirName={localappdata}\Programs\qplay
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.19041
DisableProgramGroupPage=yes
OutputBaseFilename=qplay-setup
SetupIconFile={#ProjectDir}\assets\qplay.ico
UninstallDisplayIcon={app}\qplay.exe
UninstallDisplayName=qplay
VersionInfoVersion={#AppVersion}.0
VersionInfoDescription=qplay Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
ChangesAssociations=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#BundleDir}\*"; DestDir: "{app}"; Excludes: "\qplay.portable,\userdata,\userdata\*"; Flags: ignoreversion recursesubdirs createallsubdirs

[InstallDelete]
Type: files; Name: "{app}\qplay.exe.config"
Type: files; Name: "{app}\engine\profile\fonts\fluent-system-icons.ttf"
Type: files; Name: "{app}\engine\profile\input.conf"
Type: files; Name: "{app}\engine\profile\menu.conf"
Type: files; Name: "{app}\engine\profile\mpv.conf"
Type: files; Name: "{app}\engine\profile\script-modules\framegen\catalog.lua"
Type: files; Name: "{app}\engine\profile\script-modules\framegen\engine.lua"
Type: files; Name: "{app}\engine\profile\script-modules\framegen\policy.lua"
Type: files; Name: "{app}\engine\profile\script-modules\framegen\session_quality.lua"
Type: files; Name: "{app}\engine\profile\script-modules\modernx\i18n.lua"
Type: files; Name: "{app}\engine\profile\script-modules\modernx\layout.lua"
Type: files; Name: "{app}\engine\profile\script-modules\modernx\shader_menu.lua"
Type: files; Name: "{app}\engine\profile\script-modules\modernx\subtitles.lua"
Type: files; Name: "{app}\engine\profile\script-modules\modernx\tooltip.lua"
Type: files; Name: "{app}\engine\profile\script-modules\shader_presets\auto_select.lua"
Type: files; Name: "{app}\engine\profile\script-modules\shader_presets\catalog.lua"
Type: files; Name: "{app}\engine\profile\script-modules\video_profile\classifier.lua"
Type: files; Name: "{app}\engine\profile\script-modules\video_profile\network_source.lua"
Type: files; Name: "{app}\engine\profile\script-modules\video_profile\stream_selection.lua"
Type: files; Name: "{app}\engine\profile\script-modules\video_profile\timing.lua"
Type: files; Name: "{app}\engine\profile\script-opts\framegen.conf"
Type: files; Name: "{app}\engine\profile\script-opts\modernx.conf"
Type: files; Name: "{app}\engine\profile\script-opts\shader_presets.conf"
Type: files; Name: "{app}\engine\profile\script-opts\video_profile.conf"
Type: files; Name: "{app}\engine\profile\scripts\console.lua"
Type: files; Name: "{app}\engine\profile\scripts\framegen.lua"
Type: files; Name: "{app}\engine\profile\scripts\modernx.lua"
Type: files; Name: "{app}\engine\profile\scripts\qplay_controls.lua"
Type: files; Name: "{app}\engine\profile\scripts\qplay_runtime.lua"
Type: files; Name: "{app}\engine\profile\scripts\select.lua"
Type: files; Name: "{app}\engine\profile\scripts\shader_presets.lua"
Type: files; Name: "{app}\engine\profile\scripts\video_profile.lua"
Type: files; Name: "{app}\engine\profile\tools\hardware\DxgiGpuInfo.cs"
Type: files; Name: "{app}\engine\profile\tools\hardware\shader-profile.ps1"

[Icons]
Name: "{autoprograms}\qplay"; Filename: "{app}\qplay.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\qplay"; Filename: "{app}\qplay.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Registry]
Root: HKA; Subkey: "Software\qbutt-org\qplay"; ValueType: string; ValueName: "InstallDir"; ValueData: "{app}"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\qbutt-org\qplay"; ValueType: string; ValueName: "Version"; ValueData: "{#AppVersion}"
Root: HKA; Subkey: "Software\qbutt-org\qplay"; ValueType: dword; ValueName: "ProtocolVersion"; ValueData: "1"
Root: HKA; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\qplay.exe"; ValueType: string; ValueName: ""; ValueData: "{app}\qplay.exe"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\qplay.Video"; ValueType: string; ValueName: ""; ValueData: "qplay media"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\qplay.Video\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\qplay.exe"",0"
Root: HKA; Subkey: "Software\Classes\qplay.Video\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\qplay.exe"" -- ""%1"""
Root: HKA; Subkey: "Software\Classes\Applications\qplay.exe\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\qplay.exe"" -- ""%1"""; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\qplay.exe"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "qplay"
Root: HKA; Subkey: "Software\qbutt-org\qplay\Capabilities"; ValueType: string; ValueName: "ApplicationName"; ValueData: "qplay"
Root: HKA; Subkey: "Software\qbutt-org\qplay\Capabilities"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "qplay media player"
Root: HKA; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "qplay"; ValueData: "Software\qbutt-org\qplay\Capabilities"; Flags: uninsdeletevalue

[Run]
Filename: "{app}\qplay.exe"; Description: "{cm:LaunchProgram,qplay}"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent

[CustomMessages]
english.PortableDirectory=This folder contains a portable qplay profile. Choose a different installation folder.
russian.PortableDirectory=Эта папка содержит переносной профиль qplay. Выберите другую папку для установки.

[Code]
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
    Result := '';
    if FileExists(ExpandConstant('{app}\qplay.portable')) then
        Result := CustomMessage('PortableDirectory');
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
    Extensions: TArrayOfString;
    Index: Integer;
begin
    if CurStep <> ssPostInstall then
        Exit;
    Extensions := ['.mkv', '.mp4', '.webm', '.avi', '.mov', '.m4v', '.mp3', '.flac', '.ogg', '.wav'];
    for Index := 0 to GetArrayLength(Extensions) - 1 do
    begin
        RegWriteStringValue(HKCU64, 'Software\Classes\' + Extensions[Index] + '\OpenWithProgids', 'qplay.Video', '');
        RegWriteStringValue(HKCU64, 'Software\qbutt-org\qplay\Capabilities\FileAssociations', Extensions[Index], 'qplay.Video');
    end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
    Extensions: TArrayOfString;
    Index: Integer;
begin
    if CurUninstallStep <> usPostUninstall then
        Exit;
    Extensions := ['.mkv', '.mp4', '.webm', '.avi', '.mov', '.m4v', '.mp3', '.flac', '.ogg', '.wav'];
    for Index := 0 to GetArrayLength(Extensions) - 1 do
        RegDeleteValue(HKCU64, 'Software\Classes\' + Extensions[Index] + '\OpenWithProgids', 'qplay.Video');
end;
