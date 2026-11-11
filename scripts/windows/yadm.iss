; Inno Setup script for the YADM Windows installer. Packs dist\YADM (made by
; scripts\bundle-windows.ps1, which also compiles this) into
; dist\YADM-<version>-windows-x64-setup.exe.
;
;   ISCC.exe /DAppVersion=0.1.0 scripts\windows\yadm.iss

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
; Keep AppId fixed forever: Windows uses it to find an existing install to upgrade.
AppId={{6F2C3B9E-8A41-4D7E-9C5B-2E1A7D4F8B30}
AppName=YADM
AppVersion={#AppVersion}
AppVerName=YADM {#AppVersion}
AppPublisher=YADM
AppPublisherURL=https://github.com/amovah/yadm
AppSupportURL=https://github.com/amovah/yadm/issues
AppUpdatesURL=https://github.com/amovah/yadm/releases
DefaultDirName={autopf}\YADM
DefaultGroupName=YADM
DisableProgramGroupPage=yes
; Install for the current user by default (no admin prompt); the first page
; offers "all users" for an install under Program Files.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupIconFile=..\..\assets\icon.ico
UninstallDisplayIcon={app}\YADM.exe
UninstallDisplayName=YADM
OutputDir=..\..\dist
OutputBaseFilename=YADM-{#AppVersion}-windows-x64-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\..\dist\YADM\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\YADM"; Filename: "{app}\YADM.exe"
Name: "{autodesktop}\YADM"; Filename: "{app}\YADM.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\YADM.exe"; Description: "{cm:LaunchProgram,YADM}"; Flags: nowait postinstall skipifsilent

