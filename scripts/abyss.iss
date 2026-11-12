; Inno Setup 6 script for Abyss (per-user install, no administrator rights).
; Build the portable package first (scripts\package.ps1), then:
;   iscc /DVersion=1.1.0 scripts\abyss.iss
; Output: dist\Abyss-<version>-win-x64-setup.exe (unsigned).
;
; Uninstalling removes the program files and shortcuts only. Settings, the
; session, ratings and caches in %APPDATA%\abyss and %LOCALAPPDATA%\abyss are
; left in place, together with the original legacy tide configuration.
; The AppId stays stable so installed copies upgrade through the same entry.
; UsePreviousAppDir=no selects the new Abyss program folder on upgrade.

#ifndef Version
  #define Version "1.1.0"
#endif
#define Package "Abyss-" + Version + "-win-x64"

[Setup]
AppId={{6E0D2F4B-3C1A-4E7B-9C55-7A1D2B3E4F60}
AppName=Abyss
AppVersion={#Version}
AppPublisher=Abyss project
AppComments=Unofficial TIDAL desktop client. Not affiliated with TIDAL.
UsePreviousAppDir=no
DefaultDirName={localappdata}\Programs\Abyss
DefaultGroupName=Abyss
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\dist
OutputBaseFilename={#Package}-setup
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\Abyss.exe
Compression=lzma2
SolidCompression=yes
LicenseFile=..\LICENSE
InfoBeforeFile=..\docs\LEGAL.md

[Files]
Source: "..\dist\{#Package}\*"; DestDir: "{app}"; Flags: recursesubdirs ignoreversion

[Icons]
Name: "{autoprograms}\Abyss"; Filename: "{app}\Abyss.exe"
Name: "{autodesktop}\Abyss"; Filename: "{app}\Abyss.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Run]
Filename: "{app}\Abyss.exe"; Description: "Launch Abyss"; Flags: nowait postinstall skipifsilent
