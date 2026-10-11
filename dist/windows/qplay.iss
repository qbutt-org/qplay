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
Filename: "{app}\qplay.exe"; Description: "{cm:LaunchProgram,qplay}"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent; Check: not IsUpdate
Filename: "{app}\qplay.exe"; WorkingDir: "{app}"; Flags: nowait runascurrentuser; Check: IsUpdate

[CustomMessages]
english.PortableDirectory=This folder contains a portable qplay profile. Choose a different installation folder.
russian.PortableDirectory=Эта папка содержит переносной профиль qplay. Выберите другую папку для установки.
english.UpdateBusy=qplay is still closing. Try the update again after it exits.
russian.UpdateBusy=qplay ещё закрывается. Повторите обновление после выхода из программы.
english.UpdateInvalid=Could not verify this qplay update. Run the installer again.
russian.UpdateInvalid=Не удалось проверить обновление qplay. Запустите установщик ещё раз.

[Code]
function OpenProcess(Access: LongWord; Inherit: LongWord; ProcessID: LongWord): THandle;
    external 'OpenProcess@kernel32.dll stdcall';
function QueryFullProcessImageName(Process: THandle; Flags: LongWord; Name: String; var Size: LongWord): Boolean;
    external 'QueryFullProcessImageNameW@kernel32.dll stdcall';
function WaitForSingleObject(Handle: THandle; Milliseconds: LongWord): LongWord;
    external 'WaitForSingleObject@kernel32.dll stdcall';
function CloseHandle(Handle: THandle): Boolean;
    external 'CloseHandle@kernel32.dll stdcall';

function IsUpdate: Boolean;
begin
    Result := ExpandConstant('{param:QPLAYUPDATE|}') <> '';
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
    Process: THandle;
    Size: LongWord;
    ProcessPath: String;
begin
    Result := '';
    if FileExists(ExpandConstant('{app}\qplay.portable')) then
    begin
        Result := CustomMessage('PortableDirectory');
        Exit;
    end;
    if not IsUpdate then
        Exit;
    Result := CustomMessage('UpdateInvalid');
    Process := OpenProcess($100000 or $1000, 0, StrToIntDef(ExpandConstant('{param:QPLAYUPDATE|}'), 0));
    if Process = 0 then
    begin
        { The launcher may have already completed its normal shutdown. }
        Result := '';
        Exit;
    end;
    try
        Size := 32768;
        SetLength(ProcessPath, Size);
        if not QueryFullProcessImageName(Process, 0, ProcessPath, Size) then
            Exit;
        SetLength(ProcessPath, Size);
        if CompareText(ProcessPath, ExpandConstant('{app}\qplay.exe')) <> 0 then
            Exit;
        if WaitForSingleObject(Process, 120000) = 0 then
            Result := ''
        else
            Result := CustomMessage('UpdateBusy');
    finally
        CloseHandle(Process);
    end;
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
