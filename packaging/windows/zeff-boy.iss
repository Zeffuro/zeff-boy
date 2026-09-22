#ifndef MyAppVersion
  #error MyAppVersion must be supplied
#endif
#ifndef MySourceDir
  #error MySourceDir must be supplied
#endif
#ifndef MyOutputDir
  #error MyOutputDir must be supplied
#endif

#define MyAppName "Zeff Boy"
#define MyAppExeName "zeff-boy.exe"

[Setup]
AppId={{C2417DE7-B9ED-4BE0-AB8B-74873C3B0C49}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher=Zeffuro
AppPublisherURL=https://github.com/Zeffuro/zeff-boy
AppSupportURL=https://github.com/Zeffuro/zeff-boy/issues
DefaultDirName={localappdata}\Programs\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#MyOutputDir}
OutputBaseFilename=zeff-boy-v{#MyAppVersion}-x86_64-pc-windows-msvc-setup
SetupIconFile={#SourcePath}\..\..\assets\icon.ico
UninstallDisplayName={#MyAppName}
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2
SolidCompression=yes

[Files]
Source: "{#MySourceDir}\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#MySourceDir}\LICENSE-MIT"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "{#MySourceDir}\LICENSE-APACHE"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "{#MySourceDir}\THIRD_PARTY_NOTICES.md"; DestDir: "{app}\licenses"; Flags: ignoreversion

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\{#MyAppExeName}"; ValueType: string; ValueData: "{app}\{#MyAppExeName}"; Flags: uninsdeletekey

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; IconFilename: "{app}\{#MyAppExeName}"
