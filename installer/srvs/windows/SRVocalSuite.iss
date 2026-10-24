; Installer per Windows di SR Vocal Suite (Inno Setup).
; Compilato automaticamente su GitHub: vedi .github/workflows/build-windows.yml.

#ifndef AppVersion
  #define AppVersion "1.0.0"
#endif

[Setup]
AppId={{9E4D7A21-3C5B-4F86-A1D2-7B8C9E0F1A35}
AppName=SR Vocal Suite
AppVersion={#AppVersion}
AppPublisher=Vladislav
DefaultDirName={commoncf64}\SR Vocal Suite
DisableDirPage=yes
DisableProgramGroupPage=yes
OutputDir=..\..\..
OutputBaseFilename=SRVocalSuite-Installer-Windows
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
WizardStyle=modern
UninstallDisplayName=SR Vocal Suite
CloseApplications=yes

[Languages]
Name: "it"; MessagesFile: "compiler:Languages\Italian.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

; Si scelgono i formati da installare: VST3 gia' selezionato, CLAP no.
[Types]
Name: "custom"; Description: "Scegli i formati"; Flags: iscustom

[Components]
Name: "vst3"; Description: "SR Vocal Suite VST3 (FL Studio, Ableton Live, Cubase, Studio One, Reaper, PatchWork...)"; Types: custom
Name: "clap"; Description: "SR Vocal Suite CLAP (Bitwig, Reaper...)"

[Files]
Source: "..\..\..\target\bundled\SRVocalSuite.vst3\*"; DestDir: "{commoncf64}\VST3\SRVocalSuite.vst3"; Components: vst3; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\..\..\target\bundled\SRVocalSuite.clap"; DestDir: "{commoncf64}\CLAP"; Components: clap; Flags: ignoreversion

[UninstallDelete]
Type: filesandordirs; Name: "{commoncf64}\VST3\SRVocalSuite.vst3"

[Messages]
it.WelcomeLabel2=SR Vocal Suite, correzione dell'intonazione della voce con chorus, tape echo, riverbero e formanti.%n%nNella prossima schermata scegli quali formati installare:%nVST3 (C:\Programmi\File comuni\VST3) per la maggior parte dei DAW%nCLAP (C:\Programmi\File comuni\CLAP) per Bitwig e Reaper%n%nChiudi il tuo DAW prima di continuare.
it.FinishedLabel=Installazione completata. Apri il DAW e fai ripetere la ricerca dei plugin: SR Vocal Suite compare tra gli effetti VST3 (e CLAP). Per cantare in diretta accendi Monitor (latenza zero); per il mix lascialo spento (HQ).
