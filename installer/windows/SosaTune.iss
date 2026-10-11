; Installer per Windows di Sosa Tune (Inno Setup).
; Compilato automaticamente su GitHub: vedi .github/workflows/build-windows.yml.

#ifndef AppVersion
  #define AppVersion "1.0.0"
#endif

[Setup]
AppId={{6B1C2F4E-7A3D-4E59-9C1B-50A7D3E8F1A2}
AppName=Sosa Tune
AppVersion={#AppVersion}
AppPublisher=Vladislav
DefaultDirName={commoncf64}\Sosa Tune
DisableDirPage=yes
DisableProgramGroupPage=yes
OutputDir=..\..
OutputBaseFilename=SosaTune-Installer-Windows
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
WizardStyle=modern
UninstallDisplayName=Sosa Tune
CloseApplications=yes

[Languages]
Name: "it"; MessagesFile: "compiler:Languages\Italian.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[Types]
Name: "full"; Description: "VST3 e CLAP"
Name: "custom"; Description: "Personalizzata"; Flags: iscustom

[Components]
Name: "vst3"; Description: "Sosa Tune VST3 (FL Studio, Ableton Live, Cubase, Studio One, Reaper, PatchWork...)"; Types: full custom
Name: "clap"; Description: "Sosa Tune CLAP (Bitwig, Reaper...)"; Types: full custom

[Files]
Source: "..\..\target\bundled\SosaTune.vst3\*"; DestDir: "{commoncf64}\VST3\SosaTune.vst3"; Components: vst3; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\..\target\bundled\SosaTune.clap"; DestDir: "{commoncf64}\CLAP"; Components: clap; Flags: ignoreversion

[UninstallDelete]
Type: filesandordirs; Name: "{commoncf64}\VST3\SosaTune.vst3"

[Messages]
it.WelcomeLabel2=Verra' installato Sosa Tune, correzione dell'intonazione della voce in tempo reale.%n%nVST3 in C:\Programmi\File comuni\VST3%nCLAP in C:\Programmi\File comuni\CLAP%n%nChiudi il tuo DAW prima di continuare.
it.FinishedLabel=Installazione completata. Apri il DAW e fai ripetere la ricerca dei plugin: Sosa Tune compare tra gli effetti VST3 (e CLAP). Per il mix scegli HQ, per cantare in diretta Low Latency.
