; Standalone release installer, or the isolated helper built by steam/build-depot.ps1.
#ifndef PayloadDir
  #error PayloadDir is required
#endif
#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef TargetArch
  #error TargetArch is required
#endif

#ifdef SteamTurbo
  #define ProductName "Clawback Steam Turbo"
  #define ServiceName "ClawbackSteamTurbo"
#else
  #define ProductName "Clawback"
  #define ServiceName "ClawbackTurbo"
#endif

[Setup]
#ifdef SteamTurbo
AppId={{1938AA72-4F67-4CD6-8795-B67A3A177523}
#else
AppId={{C86807C6-CFAA-4AC5-B171-9292DB2B1C5C}
#endif
AppName={#ProductName}
AppVersion={#AppVersion}
AppPublisher=Azazel Labs
AppPublisherURL=https://github.com/Azazel-Labs/Clawback
AppSupportURL=https://github.com/Azazel-Labs/Clawback/issues
DefaultDirName={autopf}\{#ProductName}
DisableDirPage=yes
UsePreviousAppDir=no
DefaultGroupName=Clawback
DisableProgramGroupPage=yes
PrivilegesRequired=admin
#if TargetArch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#elif TargetArch == "x64"
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#else
  #error Unsupported TargetArch
#endif
UninstallDisplayIcon={app}\clawback.exe
SetupIconFile=..\..\assets\icons\clawback.ico
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

#ifndef SteamTurbo
[Tasks]
Name: "turbo"; Description: "Enable Turbo without repeated administrator prompts (this Windows user only)"; GroupDescription: "Allow reading file names and sizes in protected folders:"; Flags: unchecked
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
#endif

[Files]
Source: "{#PayloadDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

#ifndef SteamTurbo
[Icons]
Name: "{group}\Clawback"; Filename: "{app}\clawback.exe"; Parameters: "--gui"
Name: "{autodesktop}\Clawback"; Filename: "{app}\clawback.exe"; Parameters: "--gui"; Tasks: desktopicon

[Run]
Filename: "{app}\clawback.exe"; Parameters: "--gui"; Description: "{cm:LaunchProgram,Clawback}"; Flags: nowait postinstall skipifsilent runasoriginaluser
#endif

[Code]
function RunServiceCommand(Params: String): Boolean;
var
  Code: Integer;
begin
  Result := Exec(ExpandConstant('{app}\clawback.exe'), Params, ExpandConstant('{app}'), SW_HIDE, ewWaitUntilTerminated, Code);
  if Result then Result := Code = 0;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  { A privileged service must never run from a portable/custom writable folder. }
  if CompareText(ExpandConstant('{app}'), ExpandConstant('{autopf}\{#ProductName}')) <> 0 then
    Result := '{#ProductName} must be installed in Program Files\{#ProductName}.'
  else if RegKeyExists(HKLM, 'SYSTEM\CurrentControlSet\Services\{#ServiceName}') and FileExists(ExpandConstant('{app}\clawback.exe')) then
    if not RunServiceCommand('--turbo-service-stop') then
      Result := 'Could not stop the Clawback Turbo service. Close Clawback and retry.';
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Params, UserSid: String;
begin
  if CurStep = ssPostInstall then begin
#ifdef SteamTurbo
    begin
#else
    if WizardIsTaskSelected('turbo') then begin
#endif
      Params := '--turbo-service-install';
      { Optional explicit account SID for managed/headless installations. }
      UserSid := ExpandConstant('{param:TurboUser|}');
      if UserSid <> '' then Params := Params + ' "' + UserSid + '"';
    end
#ifndef SteamTurbo
    else Params := '--turbo-service-remove'
#endif
    ;
    if not RunServiceCommand(Params) then
      RaiseException('Could not configure the Clawback Turbo service. The application is installed, but Turbo service setup failed.');
  end;
end;

function InitializeUninstall(): Boolean;
begin
  Result := RunServiceCommand('--turbo-service-remove');
  if not Result then
    SuppressibleMsgBox('Could not remove the Clawback Turbo service. Restart Windows and retry uninstalling.', mbError, MB_OK, IDOK);
end;
