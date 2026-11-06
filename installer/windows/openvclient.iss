; OpenVClient Windows installer (Inno Setup 6.3+).
; Built by scripts\bundle-windows.ps1:  iscc /DAppVersion=0.1.0 /DSourceExe=...\openvclient.exe /DArch=x64 openvclient.iss
;
; Wizard: Welcome -> License -> Install for me / all users -> Folder ->
; Start menu -> Extra tasks (desktop shortcut) -> Ready -> Installing ->
; Finish (launch OpenVClient). Uninstall from Settings > Apps, Start menu or
; Control Panel. User data (connections, keychain entries, logs) is kept on
; uninstall unless the user ticks the box offered by the uninstaller.

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\..\target\release\openvclient.exe"
#endif
#ifndef Arch
  #define Arch "x64"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif

#define AppName "OpenVClient"
#define AppPublisher "OpenVClient contributors"
#define AppURL "https://github.com/Mrhudson69/openvclient"
#define AppExeName "OpenVClient.exe"

[Setup]
; Stable id: upgrades replace the previous install in place. Do not change.
AppId={{6B0F8C3E-4E57-4B7D-9A57-0E5C2D6F7A11}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppURL}
AppSupportURL={#AppURL}/issues
AppUpdatesURL={#AppURL}/releases
AppCopyright=© OpenVClient contributors
VersionInfoVersion={#AppVersion}
VersionInfoProductName={#AppName}
VersionInfoDescription={#AppName} Setup

; Per-user by default (no admin prompt); the wizard offers "all users".
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=auto
DisableWelcomePage=no
AllowNoIcons=yes
LicenseFile=..\..\LICENSE-MIT
UninstallDisplayIcon={app}\{#AppExeName}
UninstallDisplayName={#AppName}
CloseApplications=yes
RestartApplications=no

#if Arch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif
MinVersion=10.0.17763

WizardStyle=modern
WizardSizePercent=110
SetupIconFile=..\..\assets\icon\openvclient.ico
WizardImageFile=..\..\assets\installer\wizard-large.bmp
WizardSmallImageFile=..\..\assets\installer\wizard-small.bmp
Compression=lzma2/max
SolidCompression=yes
OutputDir={#OutputDir}
OutputBaseFilename=OpenVClient-{#AppVersion}-windows-{#Arch}-setup

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Messages]
WelcomeLabel2=This will install [name/ver] on your computer.%n%nOpenVClient manages databases, object storage and servers from one window: PostgreSQL, MySQL, SQL Server, MongoDB, Redis, S3, Azure Blob, SSH and more.%n%nIt is recommended that you close other applications before continuing.

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "{#AppExeName}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Comment: "Databases and infrastructure"
Name: "{group}\{cm:UninstallProgram,{#AppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExeName}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[Code]
// Offer to remove saved connections and logs on uninstall (default: keep).
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: string;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    DataDir := ExpandConstant('{userappdata}\OpenVClient');
    if DirExists(DataDir) then
      if MsgBox('Also remove your saved OpenVClient connections, history and logs?' + #13#10 + #13#10 +
                DataDir + #13#10 + #13#10 +
                'Passwords stay in Windows Credential Manager under "dev.openvclient.client" until you delete them there.',
                mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
        DelTree(DataDir, True, True, True);
  end;
end;
