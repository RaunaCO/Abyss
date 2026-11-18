; Inno Setup 6 script for Abyss (per-user install, no administrator rights).
; Build the portable package first (scripts\package.ps1), then:
;   iscc /DVersion=1.5.0 scripts\abyss.iss
; Output: dist\Abyss-<version>-win-x64-setup.exe (unsigned).
;
; Uninstalling removes the program files and shortcuts. It then asks whether to
; delete the settings, session, ratings, history and caches in %APPDATA%\abyss and
; %LOCALAPPDATA%\abyss too; No (the default) keeps them for a later install. A
; silent uninstall always keeps them. The legacy tide configuration is never touched.
; The AppId stays stable so installed copies upgrade through the same entry.
; UsePreviousAppDir=no selects the new Abyss program folder on upgrade.
;
; Preview build, installed next to the normal one without touching it:
;   iscc /DVersion=1.5.0 /DPreview /DSourceDir=<package folder> scripts\abyss.iss
; It has its own AppId, folder and shortcuts ("Abyss Preview") and an empty
; `data` folder next to Abyss.exe, so it runs in portable mode: its settings and
; caches stay in that folder and never reach %APPDATA%\abyss.

#ifndef Version
  #define Version "1.5.0"
#endif
#define Package "Abyss-" + Version + "-win-x64"
#ifndef SourceDir
  #define SourceDir "..\dist\" + Package
#endif
#ifdef Preview
  #define AppTitle "Abyss Preview"
#else
  #define AppTitle "Abyss"
#endif

[Setup]
#ifdef Preview
AppId={{9B6C1E52-7D4A-4F0B-A3E8-1C2D5F6A7B81}
#else
AppId={{6E0D2F4B-3C1A-4E7B-9C55-7A1D2B3E4F60}
#endif
AppName={#AppTitle}
AppVersion={#Version}
AppPublisher=Abyss project
AppComments=Unofficial TIDAL desktop client. Not affiliated with TIDAL.
UsePreviousAppDir=no
DefaultDirName={localappdata}\Programs\{#AppTitle}
DefaultGroupName={#AppTitle}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\dist
#ifdef Preview
OutputBaseFilename={#Package}-preview-setup
#else
OutputBaseFilename={#Package}-setup
#endif
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\Abyss.exe
Compression=lzma2
SolidCompression=yes
LicenseFile=..\LICENSE
InfoBeforeFile=..\docs\LEGAL.md

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: recursesubdirs ignoreversion

#ifdef Preview
[Dirs]
; Portable mode (see docs/CONFIGURATION.md): data lives here, not in the profile.
Name: "{app}\data"
#endif

[Icons]
Name: "{autoprograms}\{#AppTitle}"; Filename: "{app}\Abyss.exe"
Name: "{autodesktop}\{#AppTitle}"; Filename: "{app}\Abyss.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Run]
Filename: "{app}\Abyss.exe"; Description: "Launch {#AppTitle}"; Flags: nowait postinstall skipifsilent

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if (CurUninstallStep <> usPostUninstall) or UninstallSilent then
    exit;
  if MsgBox('Also delete your {#AppTitle} data: settings, TIDAL session, ratings, listening history and caches?' + #13#10#13#10 +
            'This cannot be undone. Choose No to keep them for a later install.',
            mbConfirmation, MB_YESNO or MB_DEFBUTTON2) <> IDYES then
    exit;
#ifdef Preview
  DelTree(ExpandConstant('{app}\data'), True, True, True);
  RemoveDir(ExpandConstant('{app}'));
#else
  DelTree(ExpandConstant('{userappdata}\abyss'), True, True, True);
  DelTree(ExpandConstant('{localappdata}\abyss'), True, True, True);
#endif
end;
