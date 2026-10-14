; Inno Setup script for the PakPos Windows installer.
; Built in CI with: iscc /DAppVersion=x.y.z /DSourceDir=<dir> packaging\windows\pakpos.iss

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\target\release"
#endif

[Setup]
AppId={{6E2B5F0C-6B1D-4C59-9B57-5A1E0D1C7A11}
AppName=PakPos
AppVersion={#AppVersion}
AppPublisher=PakPos contributors
AppPublisherURL=https://github.com/sansob/PakPos
DefaultDirName={autopf}\PakPos
DefaultGroupName=PakPos
DisableProgramGroupPage=yes
OutputBaseFilename=PakPos-{#AppVersion}-windows-x64-setup
OutputDir=..\..\dist
SetupIconFile=..\..\assets\icon.ico
UninstallDisplayIcon={app}\pakpos.exe
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequiredOverridesAllowed=dialog
ChangesEnvironment=yes
LicenseFile=..\..\LICENSE-MIT

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "addtopath"; Description: "Add pakpos-cli to PATH"; Flags: unchecked

[Files]
Source: "{#SourceDir}\pakpos.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\pakpos-cli.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\PakPos"; Filename: "{app}\pakpos.exe"
Name: "{autodesktop}\PakPos"; Filename: "{app}\pakpos.exe"; Tasks: desktopicon

[Registry]
Root: HKA; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Tasks: addtopath; Check: NeedsAddPath(ExpandConstant('{app}'))

[Run]
Filename: "{app}\pakpos.exe"; Description: "{cm:LaunchProgram,PakPos}"; Flags: nowait postinstall skipifsilent

[Code]
function NeedsAddPath(Param: string): boolean;
var
  OrigPath: string;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', OrigPath) then
  begin
    Result := True;
    exit;
  end;
  Result := Pos(';' + Uppercase(Param) + ';', ';' + Uppercase(OrigPath) + ';') = 0;
end;
