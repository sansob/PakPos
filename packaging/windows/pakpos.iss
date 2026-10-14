; Inno Setup script for the PakPos Windows installer.
; Built in CI with: iscc /DAppVersion=x.y.z /DSourceDir=<dir> packaging\windows\pakpos.iss

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Arch
  #define Arch "x64"
#endif
#define SysEnvKey "SYSTEM\CurrentControlSet\Control\Session Manager\Environment"
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
OutputBaseFilename=PakPos-{#AppVersion}-windows-{#Arch}-setup
OutputDir=..\..\dist
SetupIconFile=..\..\assets\icon.ico
UninstallDisplayIcon={app}\pakpos.exe
Compression=lzma2
SolidCompression=yes
#if Arch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif
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
; Per-user install: HKCU\Environment. All-users (admin) install: the system
; environment key; HKLM has no "Environment" key of its own.
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Tasks: addtopath; Check: (not IsAdminInstallMode) and NeedsAddPath(ExpandConstant('{app}'))
Root: HKLM; Subkey: "{#SysEnvKey}"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Tasks: addtopath; Check: IsAdminInstallMode and NeedsAddPath(ExpandConstant('{app}'))

[Run]
Filename: "{app}\pakpos.exe"; Description: "{cm:LaunchProgram,PakPos}"; Flags: nowait postinstall skipifsilent

[Code]
procedure PathKey(var Root: Integer; var Key: string);
begin
  if IsAdminInstallMode then
  begin
    Root := HKEY_LOCAL_MACHINE;
    Key := '{#SysEnvKey}';
  end
  else
  begin
    Root := HKEY_CURRENT_USER;
    Key := 'Environment';
  end;
end;

function NeedsAddPath(Param: string): boolean;
var
  Root: Integer;
  Key, OrigPath: string;
begin
  PathKey(Root, Key);
  if not RegQueryStringValue(Root, Key, 'Path', OrigPath) then
  begin
    Result := True;
    exit;
  end;
  Result := Pos(';' + Uppercase(Param) + ';', ';' + Uppercase(OrigPath) + ';') = 0;
end;

{ Removes the install folder from PATH again on uninstall. }
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Root, P: Integer;
  Key, Path, Dir: string;
begin
  if CurUninstallStep <> usPostUninstall then
    exit;
  PathKey(Root, Key);
  if not RegQueryStringValue(Root, Key, 'Path', Path) then
    exit;
  Dir := ExpandConstant('{app}');
  Path := ';' + Path + ';';
  P := Pos(';' + Uppercase(Dir) + ';', Uppercase(Path));
  if P = 0 then
    exit;
  Delete(Path, P, Length(Dir) + 1);
  Path := Copy(Path, 2, Length(Path) - 2);
  RegWriteExpandStringValue(Root, Key, 'Path', Path);
end;
