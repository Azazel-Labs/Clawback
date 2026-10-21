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
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#PayloadDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\Clawback"; Filename: "{app}\clawback.exe"; Parameters: "--gui"
Name: "{autodesktop}\Clawback"; Filename: "{app}\clawback.exe"; Parameters: "--gui"; Tasks: desktopicon

[Run]
Filename: "{app}\clawback.exe"; Parameters: "--gui"; Description: "{cm:LaunchProgram,Clawback}"; Flags: nowait postinstall skipifsilent runasoriginaluser
