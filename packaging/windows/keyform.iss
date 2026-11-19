; Inno Setup script for the Keyform Windows installer.
; iscc /DVersion=X.Y.Z /DBinary=path\to\keyform.exe /DOutDir=dist keyform.iss
#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef Binary
  #define Binary "..\..\target\release\keyform.exe"
#endif
#ifndef OutDir
  #define OutDir "..\..\dist"
#endif

[Setup]
AppId={{6A0F4E57-4B1D-4C0B-9C5E-1E0E2D7F4B11}
AppName=Keyform
AppVersion={#Version}
AppPublisher=Keyform contributors
DefaultDirName={autopf}\Keyform
DefaultGroupName=Keyform
OutputDir={#OutDir}
OutputBaseFilename=Keyform-{#Version}-setup
SetupIconFile=..\..\assets\icons\keyform.ico
UninstallDisplayIcon={app}\keyform.exe
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesAssociations=yes
PrivilegesRequiredOverridesAllowed=dialog
LicenseFile=..\..\LICENSE-MIT

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "ja"; MessagesFile: "compiler:Languages\Japanese.isl"

[Tasks]
Name: "assoc"; Description: "Open .cmo3, .can3 and .moc3 files with Keyform"; GroupDescription: "File associations:"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#Binary}"; DestDir: "{app}"; DestName: "keyform.exe"; Flags: ignoreversion
Source: "..\..\assets\icons\keyform.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Keyform"; Filename: "{app}\keyform.exe"; IconFilename: "{app}\keyform.ico"
Name: "{autodesktop}\Keyform"; Filename: "{app}\keyform.exe"; IconFilename: "{app}\keyform.ico"; Tasks: desktopicon

[Registry]
Root: HKA; Subkey: "Software\Classes\Keyform.Project"; ValueType: string; ValueData: "2D mesh model project"; Flags: uninsdeletekey; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\Keyform.Project\DefaultIcon"; ValueType: string; ValueData: "{app}\keyform.ico"; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\Keyform.Project\shell\open\command"; ValueType: string; ValueData: """{app}\keyform.exe"" ""%1"""; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\.cmo3\OpenWithProgids"; ValueType: string; ValueName: "Keyform.Project"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\.can3\OpenWithProgids"; ValueType: string; ValueName: "Keyform.Project"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\.moc3\OpenWithProgids"; ValueType: string; ValueName: "Keyform.Project"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\Applications\keyform.exe\SupportedTypes"; ValueType: string; ValueName: ".cmo3"; ValueData: ""; Flags: uninsdeletekey; Tasks: assoc

[Run]
Filename: "{app}\keyform.exe"; Description: "{cm:LaunchProgram,Keyform}"; Flags: nowait postinstall skipifsilent
