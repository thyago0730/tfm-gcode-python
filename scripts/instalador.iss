; Instalador do TFM G-Code Generator (Rust + egui), compilado com o Inno Setup 6.
; Uso: scripts\gerar_instalador.ps1 (passa a versão e o caminho do exe).
;
; Instala por usuário, sem pedir administrador. O app grava config, procedimentos
; e logs ao lado do .exe, então a pasta precisa ser gravável: Program Files não serve.
; Os dados do usuário nunca são copiados nem apagados: atualizar mantém tudo.

#ifndef Versao
  #define Versao "2.0.0"
#endif
#ifndef ExeOrigem
  #define ExeOrigem "..\dist\TFM_GCODE.exe"
#endif
#ifndef Saida
  #define Saida "..\dist"
#endif

[Setup]
AppId={{7B3C5E0A-4F1D-4C2B-9A66-0A1B2C3D4E5F}
AppName=TFM G-Code Generator
AppVersion={#Versao}
AppPublisher=TFM Usinagem & Manutenção Industrial
DefaultDirName={localappdata}\Programs\TFM G-Code
DefaultGroupName=TFM G-Code
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#Saida}
OutputBaseFilename=TFM_GCODE_Instalador_{#Versao}
SetupIconFile=..\assets\icone.ico
UninstallDisplayIcon={app}\TFM_GCODE.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[Tasks]
Name: "desktopicon"; Description: "Criar um atalho na área de trabalho"; GroupDescription: "Atalhos:"

[Files]
Source: "{#ExeOrigem}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\icone.ico"; DestDir: "{app}\assets"; Flags: ignoreversion

[Icons]
Name: "{group}\TFM G-Code Generator"; Filename: "{app}\TFM_GCODE.exe"
Name: "{group}\Desinstalar"; Filename: "{uninstallexe}"
Name: "{userdesktop}\TFM G-Code Generator"; Filename: "{app}\TFM_GCODE.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\TFM_GCODE.exe"; Description: "Abrir o TFM G-Code Generator"; Flags: nowait postinstall skipifsilent
