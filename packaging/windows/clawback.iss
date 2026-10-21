; Compiled by build-installer.ps1 from the same payload as the portable ZIP.
#ifndef PayloadDir
  #error PayloadDir is required
#endif
#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef TargetArch
  #error TargetArch is required
#endif

[Setup]
AppId={{C86807C6-CFAA-4AC5-B171-9292DB2B1C5C}
AppName=Clawback
AppVersion={#AppVersion}
AppPublisher=Azazel Labs
AppPublisherURL=https://github.com/Azazel-Labs/Clawback
AppSupportURL=https://github.com/Azazel-Labs/Clawback/issues
DefaultDirName={autopf}\Clawback
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

[Tasks]
Name: "turbo"; Description: "Enable Turbo without repeated administrator prompts (this Windows user only)"; GroupDescription: "Allow reading file names and sizes in protected folders:"; Flags: unchecked
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#PayloadDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\Clawback"; Filename: "{app}\clawback.exe"; Parameters: "--gui"
Name: "{autodesktop}\Clawback"; Filename: "{app}\clawback.exe"; Parameters: "--gui"; Tasks: desktopicon

[Run]
Filename: "{app}\clawback.exe"; Parameters: "--gui"; Description: "{cm:LaunchProgram,Clawback}"; Flags: nowait postinstall skipifsilent runasoriginaluser

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
  if CompareText(ExpandConstant('{app}'), ExpandConstant('{autopf}\Clawback')) <> 0 then
    Result := 'Clawback must be installed in Program Files\Clawback. Use the portable ZIP for other locations.'
  else if RegKeyExists(HKLM, 'SYSTEM\CurrentControlSet\Services\ClawbackTurbo') and FileExists(ExpandConstant('{app}\clawback.exe')) then
    if not RunServiceCommand('--turbo-service-stop') then
      Result := 'Could not stop the Clawback Turbo service. Close Clawback and retry.';
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Params, UserSid: String;
begin
  if CurStep = ssPostInstall then begin
    if WizardIsTaskSelected('turbo') then begin
      Params := '--turbo-service-install';
      { Optional explicit account SID for managed/headless installations. }
      UserSid := ExpandConstant('{param:TurboUser|}');
      if UserSid <> '' then Params := Params + ' "' + UserSid + '"';
    end else Params := '--turbo-service-remove';
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
