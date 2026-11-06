; OpenVClient Windows installer (Inno Setup 6.6 or newer).
; Built by scripts\bundle-windows.ps1, for example:
;   iscc /DAppVersion=0.1.0 /DSourceExe=...\openvclient.exe /DArch=x64 openvclient.iss
;
; Wizard: Install for me / all users -> Welcome -> License -> Folder ->
; Start menu -> Extra tasks (desktop shortcut) -> Ready -> Installing ->
; Finish (launch OpenVClient). Light or dark to match Windows.
; Upgrades install over the previous version in place (same AppId).
; Uninstall from Settings > Apps, the Start menu or Control Panel. User data
; (connections, history, logs) is kept unless the user agrees to remove it.

#if Ver < EncodeVer(6, 6, 0)
  #error Inno Setup 6.6 or newer is required (winget install JRSoftware.InnoSetup)
#endif

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
; Windows version resources take numbers only: 0.2.0-rc.1 -> 0.2.0.
#define AppVersionNumeric Copy(AppVersion, 1, Pos("-", AppVersion + "-") - 1)
#ifndef SourceExe
  #define SourceExe "..\..\target\release\openvclient.exe"
#endif
#ifndef Arch
  #define Arch "x64"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif
; bundle-windows.ps1 passes a combined MIT + Apache-2.0 text for the License page.
#ifndef LicenseText
  #define LicenseText "..\..\LICENSE-MIT"
#endif

#define AppName "OpenVClient"
#define AppPublisher "OpenVClient contributors"
#define AppURL "https://github.com/openvclient/openvclient"
#define AppExeName "OpenVClient.exe"
#define Art "..\..\assets\installer\"

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
AppComments=Databases and infrastructure in one window
AppCopyright=© OpenVClient contributors · MIT OR Apache-2.0
VersionInfoVersion={#AppVersionNumeric}
VersionInfoProductVersion={#AppVersionNumeric}
VersionInfoProductTextVersion={#AppVersion}
VersionInfoProductName={#AppName}
VersionInfoCompany={#AppPublisher}
VersionInfoDescription={#AppName} Setup

; Per-user by default (no admin prompt); the first dialog offers "all users".
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
; Folder and Start menu pages are shown on a first install and skipped on upgrades.
DisableDirPage=auto
DisableProgramGroupPage=auto
DisableWelcomePage=no
AllowNoIcons=yes
LicenseFile={#LicenseText}
UninstallDisplayIcon={app}\{#AppExeName},0
UninstallDisplayName={#AppName}
; Close a running OpenVClient through Restart Manager before replacing files.
; "force" is needed because a plain RmShutdown is cancelled by Windows and the
; user is then told to close the app by hand. It is safe here: the app answers
; WM_QUERYENDSESSION and closes itself normally (crates/openvclient-app/src/
; winsession.rs), saving its workspace, so nothing is killed and no work is lost.
CloseApplications=force
RestartApplications=no

#if Arch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif
; Windows 10 1809 or newer.
MinVersion=10.0.17763

; Modern look that follows the Windows light/dark setting.
WizardStyle=modern dynamic
SetupIconFile=..\..\assets\icon\openvclient.ico
; One image per DPI step (100/125/150/200/250 %); Setup picks the best match.
; The side panel is dark slate and the header icon is transparent, so the same
; files work in light and dark mode.
WizardImageFile={#Art}wizard-large-202x386.png,{#Art}wizard-large-269x515.png,{#Art}wizard-large-336x643.png,{#Art}wizard-large-430x824.png,{#Art}wizard-large-534x1022.png
WizardSmallImageFile={#Art}wizard-small-58.png,{#Art}wizard-small-77.png,{#Art}wizard-small-97.png,{#Art}wizard-small-124.png,{#Art}wizard-small-159.png
Compression=lzma2/max
SolidCompression=yes
OutputDir={#OutputDir}
OutputBaseFilename=OpenVClient-{#AppVersion}-windows-{#Arch}-setup

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Messages]
WelcomeLabel2=This will install [name/ver] on your computer.%n%nOpenVClient manages databases, object storage and servers from one window: PostgreSQL, MySQL, SQL Server, MongoDB, Redis, S3, Azure Blob, SSH and more.%n%nClose OpenVClient if it is running before you continue.
FinishedLabel=Setup has finished installing [name] on your computer. You can start it from the Start menu.

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
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Comment: "Databases and infrastructure"; Tasks: desktopicon

[Run]
; postinstall entries run as the user who started Setup, not elevated.
Filename: "{app}\{#AppExeName}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[Code]
// Offer to remove saved connections, history and logs on uninstall.
// The default (and the answer in silent uninstalls) is to keep them.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: string;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    // Covers the workspace (OpenVClient\OpenVClient\config) and the window state (OpenVClient\data).
    DataDir := ExpandConstant('{userappdata}\OpenVClient');
    if DirExists(DataDir) then
      if SuppressibleMsgBox('Also remove your saved OpenVClient connections, history and logs?' + #13#10 + #13#10 +
                DataDir + #13#10 + #13#10 +
                'Choose No to keep them for a later reinstall. Saved passwords stay in Windows Credential Manager' + #13#10 +
                '(entries ending in "dev.openvclient.client") until you remove them there.',
                mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDNO) = IDYES then
        DelTree(DataDir, True, True, True);
  end;
end;
