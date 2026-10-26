; Installer per Windows di SosaTune (Inno Setup).
; Compilato automaticamente su GitHub: vedi .github/workflows/build-windows.yml.

#ifndef AppVersion
  #define AppVersion "1.0.0"
#endif

[Setup]
AppId={{6B1C2F4E-7A3D-4E59-9C1B-50A7D3E8F1A2}
AppName=SosaTune
AppVersion={#AppVersion}
AppPublisher=Vladislav
DefaultDirName={commoncf64}\SosaTune
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
UninstallDisplayName=SosaTune
CloseApplications=yes

[Languages]
Name: "it"; MessagesFile: "compiler:Languages\Italian.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

; Si scelgono i formati da installare: tutti gia' spuntati, si toglie la
; spunta a quelli che non servono. (Logic Pro esiste solo su Mac: la versione
; Audio Unit e' nell'installer per Mac.)
[Types]
Name: "custom"; Description: "Scegli i formati"; Flags: iscustom

[Components]
Name: "vst3"; Description: "VST3  (FL Studio, Ableton Live, Cubase, Studio One, Reaper, PatchWork...)"; Types: custom
Name: "clap"; Description: "CLAP  (Bitwig, Reaper...)"; Types: custom

[Files]
Source: "..\..\target\bundled\SosaTune.vst3\*"; DestDir: "{commoncf64}\VST3\SosaTune.vst3"; Components: vst3; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\..\target\bundled\SosaTune.clap"; DestDir: "{commoncf64}\CLAP"; Components: clap; Flags: ignoreversion

[UninstallDelete]
Type: filesandordirs; Name: "{commoncf64}\VST3\SosaTune.vst3"
Type: files; Name: "{commoncf64}\CLAP\SosaTune.clap"

[Messages]
it.WelcomeLabel2=SosaTune, correzione dell'intonazione della voce in tempo reale.%n%nNella prossima schermata scegli quali formati installare (sono tutti spuntati: togli la spunta a quelli che non ti servono):%n- VST3 per FL Studio, Ableton Live, Cubase, Studio One, Reaper%n- CLAP per Bitwig e Reaper%n%nLa versione per Logic Pro (Audio Unit) e' nell'installer per Mac, perche' Logic esiste solo su Mac.%n%nChiudi il tuo DAW prima di continuare.
it.SelectComponentsLabel2=Scegli i formati di SosaTune da installare:
it.FinishedLabel=Installazione completata. Apri il DAW e fai ripetere la ricerca dei plugin: SosaTune compare tra gli effetti, sotto Vladislav. Per il mix scegli HQ, per cantare in diretta Low Latency.
en.WelcomeLabel2=SosaTune, real-time vocal pitch correction.%n%nOn the next page choose which formats to install (all are checked; uncheck the ones you don't need):%n- VST3 for FL Studio, Ableton Live, Cubase, Studio One, Reaper%n- CLAP for Bitwig and Reaper%n%nThe Logic Pro version (Audio Unit) is in the Mac installer.%n%nClose your DAW before continuing.
