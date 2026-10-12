# Sosa Tune

**Correzione dell'intonazione in tempo reale per la voce**, stile Auto-Tune, scritta in Rust con [nih-plug](https://github.com/robbert-vdh/nih-plug). Funziona come plugin **VST3**, **CLAP** e **AU** (Logic Pro, GarageBand) su macOS, e VST3/CLAP su Windows. Per installarlo su un altro computer ci sono gli installer pronti: `SosaTune-Installer-macOS.pkg` per Mac e `SosaTune-Installer-Windows.exe` per Windows, in cui scegli tu quali formati installare.

![L'interfaccia di Sosa Tune](docs/interfaccia.png)

![Esempio: melodia stonata e corretta](docs/grafico_demo.png)

*Una melodia cantata stonata (grigio) corretta in Do maggiore: in oro lo stile Robotico (velocità 0 ms, l'effetto "alla T-Pain"), in verde acqua lo stile Naturale (velocità 80 ms con Humanize), che porta la voce sulla nota ma conserva il vibrato.*

## Cosa fa

- Riconosce in tempo reale la nota che stai cantando (algoritmo **YIN**).
- La porta sulla nota giusta della **tonalità e scala** scelte: cromatica, maggiore, minore naturale e armonica, pentatoniche, blues, oppure una **scala personalizzata** cliccando sulla tastiera.
- **Velocità di correzione** regolabile: da robotica (0 ms) a naturale (100-400 ms).
- **Humanize**: sulle note lunghe la correzione si ammorbidisce, così le note tenute non suonano "congelate".
- **Flex-Tune**: corregge solo quando la voce è già vicina alla nota; scivolate e note volutamente "sporche" restano tue.
- **Vibrato naturale**: lo riduce o lo aggiunge, anche sopra la correzione robotica.
- **Formanti**: cambia il timbro (la "lunghezza della gola") senza cambiare la nota.
- **Sensibilità** del rilevamento, per voci pulite o con molta aria.
- **Preset pronti** (Factory): Default, Natural Vocal, Pop Polish, Hard Tune, Trap Lead, R&B Smooth, Soft Touch, Classic Robot, Retro Glide.
- **Quantità** di correzione e **trasposizione** fino a ±12 semitoni.
- Cambia la nota **senza cambiare il timbro** della voce (in HQ: **phase vocoder** a picchi bloccati con finestra adattata alla voce e formanti preservate dal "true envelope": niente "effetto scoiattolo", niente click).
- Due **modalità**: **Low Latency** a latenza zero per registrare con il plugin acceso, **HQ** per il mix.
- Tre impostazioni di **voce** (soprano, alto/tenore, voce bassa): decidono la nota più grave riconosciuta.
- Interfaccia con la nota cantata, l'indicatore dello scarto in centesimi, manopole, tastiera della scala e **AutoKey** (la tonalità si imposta da sola ascoltando la voce).
- Mono e stereo, qualsiasi frequenza di campionamento, latenza comunicata al DAW (che la compensa da solo).

| Modalità | Latenza | Quando usarla |
|---|---|---|
| **Low Latency** | **0 ms** | Mentre registri: nessun ritardo in cuffia. Qualità da monitoraggio (le formanti seguono la nota, il comando Formant non agisce) |
| **HQ** | 64 ms (compensata dal DAW) | Per il mix: il suono più pulito, timbro preservato, intonazione più precisa |

| Voce | Nota più grave riconosciuta |
|---|---|
| Soprano / voce acuta | 150 Hz (circa Re3) |
| Alto / Tenore | 75 Hz (circa Re2) |
| Voce bassa | 60 Hz (circa Si1) |

---

## Installazione sul Mac, passo per passo

Non serve esperienza: basta copiare i comandi nel **Terminale** (lo trovi in Applicazioni → Utility → Terminale). Dopo ogni comando premi Invio.

### 1. Strumenti di sviluppo di Apple

```bash
xcode-select --install
```

Si apre una finestra: scegli **Installa** e aspetta la fine (se dice che sono già installati, va bene così).

### 2. Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Quando chiede, premi **Invio** per l'installazione standard. Poi **chiudi e riapri il Terminale**, e controlla che funzioni:

```bash
cargo --version
```

Deve rispondere con un numero di versione (per esempio `cargo 1.9x.0`).

### 3. Il progetto

Estrai lo zip del progetto, per esempio nella cartella **Documenti**, poi entra nella cartella:

```bash
cd ~/Documents/sosatune
```

### 4. Compila il plugin

```bash
cargo xtask bundle sosatune --release
```

La prima volta ci mette **5-10 minuti**, perché scarica e compila tutte le librerie. Le volte successive basta meno di un minuto. Alla fine trovi i plugin nella cartella `target/bundled/`.

### 5. Installa il plugin

```bash
mkdir -p ~/Library/Audio/Plug-Ins/VST3 ~/Library/Audio/Plug-Ins/CLAP
cp -R target/bundled/SosaTune.vst3 ~/Library/Audio/Plug-Ins/VST3/
cp -R target/bundled/SosaTune.clap ~/Library/Audio/Plug-Ins/CLAP/
```

### 6. Aprilo nel DAW

Apri il tuo DAW, fai ripetere la scansione dei plugin se non compare da solo, e inserisci **Sosa Tune** come effetto sulla traccia della voce. Lo trovi tra gli effetti di "Vladislav".

- **Ableton Live, FL Studio, Cubase, Studio One, Reaper**: usa il VST3.
- **Bitwig, Reaper**: puoi usare anche il CLAP.
- **Logic Pro e GarageBand**: usa l'AU (`SosaTune.component`, nel pacchetto `.pkg` scegli "Audio Unit"). È creato con [clap-wrapper](https://github.com/free-audio/clap-wrapper) e supera `auval`; le manopole funzionano anche su una traccia vuota o a trasporto fermo.

---

## Come si usa

1. Imposta la **Tonalità** e la **Scala** della canzone (per esempio La minore → Tonalità A, Scala Minore naturale). Se non le conosci, lascia **Cromatica**: ogni nota viene portata al semitono più vicino.
2. Scegli il tipo di **Voce**.
3. Parti da un **preset** e poi rifinisci, per esempio:
   - **Natural Vocal**: velocità 80 ms e Humanize, si sente intonato ma non "corretto";
   - **Pop Polish**: velocità 15 ms, intonazione precisa e brillante;
   - **Hard Tune**: velocità 0 ms, l'effetto Auto-Tune estremo;
   - **Soft Touch**: sistema solo le stonature e lascia l'espressione (Flex-Tune alto).
4. **Mentre registri** metti **Mode → Low Latency** (nessun ritardo in cuffia); **per il mix** torna su **HQ**.
5. I controlli, uno per uno:

| Controllo | Cosa fa | Da dove partire |
|---|---|---|
| Velocità | Quanto in fretta la voce va sulla nota. 0 ms = robotico, 20-80 ms = musicale, 100-400 ms = naturale | 20-80 ms |
| Quantità | Quanta correzione. Sotto il 100% resta un po' dell'intonazione originale | 100% |
| Humanize | Note lunghe corrette più dolcemente, note brevi come prima. Proporzionale: al 25% le note tenute rallentano di ~12 ms, al 50% di 50 ms, al 100% di 200 ms | 20-50% |
| Flex-Tune | Corregge piena vicino alla nota e meno quando la voce è lontana. Proporzionale: al 25% le note lontane sono corrette al 75%, al 50% a metà, al 100% restano libere | 0% (pop), 25-50% (espressivo) |
| Vibrato | Negativo lo riduce, positivo lo aggiunge (anche con il robotico) | 0% |
| Formanti | Il timbro: + voce più piccola e chiara, − più grande e scura. La nota non cambia (solo in HQ) | 0 st |
| Sensibilità | Alzala per voci con molta aria o sussurrate, abbassala se senti salti strani | 30% |
| Trasponi | Sposta tutta la voce di semitoni, mantenendo il timbro | 0 st |
| Bypass | Il pulsante di bypass del DAW: la voce passa senza correzione ma con lo stesso ritardo, cosi' resta a tempo | spento |

6. Sulla **tastiera** clicca un tasto per escludere o includere una nota (la scala diventa "Personalizzata").

### L'interfaccia

- **Vocal Range** e **Key & Scale** (in alto): menu a tendina per tipo di voce, tonalità e scala. **AutoKey Listen**: accendilo e fai suonare la voce, la tonalità si imposta da sola.
- **Nota al centro**: la nota che stai cantando; l'indicatore dice di quanti **centesimi** sei calante (−) o crescente (+).
- **Manopole** Retune Speed, Humanize, Flex Tune e Detune: trascinale in su o in giù. Tieni premuto **Maiusc** per i ritocchi fini, fai **doppio clic** per tornare al valore iniziale. Passandoci sopra col mouse compare la spiegazione.
- **Mode**: Low Latency (0 ms, per registrare) o HQ (per il mix). **Modern / Classic**: note già intonate oppure ogni nota che scivola sulla nota giusta.
- **Edit Notes** (in basso): clic su una nota per includerla o escluderla dalla scala.
- **⚙**: Amount, Transpose, Vibrato, Formant, Sensitivity e la latenza attuale. **↶ ↷**: annulla e ripeti.

---

## Provare il motore senza DAW

Il motore audio (cartella `dsp/`) si può usare anche da solo, su file WAV.

**Demo** con un cantante sintetico stonato (crea `demo_1_originale.wav`, `demo_2_naturale.wav`, `demo_3_robotico.wav`, `demo_4_robotico_vibrato.wav`, `demo_5_formanti.wav`):

```bash
cargo run --release -p sosatune_dsp --example demo
```

**Correggere una tua registrazione** (WAV mono o stereo):

```bash
cargo run --release -p sosatune_dsp --example process -- voce.wav voce_corretta.wav --tonalita La --scala minore --velocita 0
```

Opzioni: `--tonalita` (C, C#, Db... oppure Do, Do#, Reb...), `--scala` (cromatica, maggiore, minore, minore-armonica, pentatonica-maggiore, pentatonica-minore, blues), `--velocita` in ms, `--quantita` in %, `--trasponi` in semitoni, `--voce` (soprano, alto, bassa), `--humanize` e `--flex` in %, `--vibrato` da -100 a 100, `--formanti` in semitoni, `--sensibilita` in %.

**Test automatici** (controllano rilevamento, correzione, qualità e stabilità):

```bash
cargo test -p sosatune_dsp --release
```

---

## Come funziona

```text
voce ──► YIN ──────────► scala ───────────► correttore ─────► phase vocoder ──► voce corretta
        "che nota è?"    "nota giusta       "di quanto e       (STFT → picchi spostati
        (rilevamento)    più vicina"        con che velocità"   → IFFT) sposta la nota
                         (quantizzazione)   rapporto = bersaglio / nota cantata
```

- **YIN** (`dsp/src/yin.rs`): confronta il segnale con se stesso spostato nel tempo; quando la differenza è minima, lo spostamento è il periodo dell'onda, cioè la nota. Calcolato con la FFT (`dsp/src/fft.rs`) per essere veloce, più un controllo contro gli errori d'ottava.
- **Scale** (`dsp/src/scale.rs`): sceglie la nota ammessa più vicina, con un po' di isteresi per non "tremolare" quando sei a metà tra due note.
- **Correttore** (`dsp/src/corrector.rs`): decide di quanto spostare la nota e con che velocità (un filtro che arriva al bersaglio in "velocità" millisecondi). Qui vivono anche Humanize (la velocità rallenta man mano che la nota dura), Flex-Tune (il peso della correzione cala con la distanza dalla nota) e il vibrato naturale (isolato con un filtro passa-banda centrato su 5,5 Hz, che non lo sfasa e permette quindi anche di toglierlo).
- **Phase vocoder** (modalità HQ, `dsp/src/vocoder.rs`): ogni 5 ms prende una "fetta" di voce, la smussa con una finestra di Hann lunga **6 periodi della voce** (da 32 ms per le voci acute a 64 ms per i bassi: le armoniche fitte delle voci maschili restano separate) e la trasforma in frequenze (FFT). Trova le armoniche (i picchi), misura la loro frequenza esatta dalla rotazione della fase, e sposta ogni armonica con le righe vicine alla frequenza giusta (frequenza × rapporto, anche di frazioni di riga). La fase di ogni armonica continua a girare alla velocità della frequenza nuova e le righe vicine la seguono ("identity phase locking", Laroche e Dolson): niente suono metallico. Il timbro (inviluppo spettrale) è misurato con il **true envelope**: un cepstrum lisciato e ripetuto finché passa sulle cime delle armoniche, prolungato in piano sotto la fondamentale; viene rimesso al suo posto, o spostato con il comando Formant. Poi FFT inversa e somma sovrapposta (overlap-add). Consonanti, "s", respiri e attacchi passano identici (niente ronzio metallico sulle sibilanti).
- **Latenza zero** (modalità Low Latency, `dsp/src/zerolat.rs`): la voce viene letta da una "testina" che scorre sul segnale appena arrivato un po' più veloce (nota più acuta) o più lenta (più grave). Quando arriva al presente o resta troppo indietro salta di un ciclo esatto della voce, nel punto in cui la forma d'onda combacia, con una dissolvenza: il ritardo resta di pochi millesimi e il DAW riceve latenza zero. Le consonanti passano identiche.

### Qualità misurata

I test (`cargo test -p sosatune_dsp --release`, 38 in tutto) controllano anche la qualità del suono:

- correzione robotica di una melodia: in media **0,3 centesimi** dalla nota giusta; su voci reali circa **2 centesimi**;
- le **formanti** restano entro ~1 dB nelle correzioni tipiche (fino a un paio di semitoni);
- l'**armonicità** della voce (quanto è "pulita", senza ruvidità) in uscita è pari a quella in ingresso, sia su voci sintetiche sia reali;
- vibrato naturale: rimesso con la profondità originale, oppure ridotto del 60-99%;
- Humanize e Flex-Tune proporzionali alla manopola; Low Latency a latenza zero, HQ con latenza fissa; nessun clic ai cambi di nota in entrambe le modalità.

Limiti noti: le trasposizioni estreme (un'ottava) e le voci molto acute (sopra ~600 Hz) cambiano un po' il timbro, come in ogni pitch shifter di questo tipo.

Il motore non alloca memoria e non usa lock durante l'elaborazione: è sicuro per il tempo reale. La correzione usa qualche percento di un core.

### Compatibilità controllata

A ogni modifica il plugin viene messo alla prova automaticamente su un Mac, con gli stessi strumenti che usano i produttori di plugin professionali (il risultato completo è in `rapporto.txt`, nel ramo `build`):

- **Validatore ufficiale Steinberg VST3**: 537 prove, 0 errori (canali mono, stereo, mono → stereo, frequenze da 22 a 384 kHz, bypass, salvataggio e ripristino);
- **pluginval** al livello più severo (10): apertura, parametri, automazione, salvataggio, chiamate da più thread, blocchi audio di ogni dimensione;
- **clap-validator**: 34 prove, 0 errori (formato CLAP);
- **prove dell'interfaccia** (`cargo test -p sosatune --release`): clic e trascinamenti simulati su tutti i comandi dell'interfaccia e bypass del DAW;
- **auval** di Apple per la versione AU.

Configurazioni supportate: stereo → stereo, mono → mono, mono → stereo (traccia vocale mono in Pro Tools/PatchWork, Logic, Cubase...).

## Struttura del progetto

```text
sosatune/
├── dsp/                 il motore audio (nessuna libreria esterna)
│   ├── src/             yin, fft, scale, corrector, vocoder (HQ), zerolat (Low Latency), synth, wav
│   ├── examples/        demo.rs e process.rs (provare su file WAV)
│   └── tests/           test di robustezza
├── plugin/              il plugin VST3/CLAP
│   └── src/             lib.rs (collegamento al DAW), params.rs (parametri),
│                        editor.rs (interfaccia), shared.rs (dati per il grafico)
├── xtask/               lo strumento che crea i file .vst3 e .clap
├── vendor/nih-plug/     nih-plug con le correzioni di compatibilità (vedi nih-plug-sosatune.patch)
└── .github/workflows/   compilazione automatica per Mac su GitHub
```

## Se qualcosa va storto

- **Errore durante la compilazione**: copia tutto il testo dell'errore e mandamelo, lo sistemiamo.
- **`cargo: command not found`**: chiudi e riapri il Terminale dopo aver installato Rust.
- **Il DAW non vede il plugin**: controlla che i file siano nelle cartelle `~/Library/Audio/Plug-Ins/VST3` e `CLAP` e fai ripetere la scansione dei plugin.
- **FL Studio lo segna come "error" nel Plugin Manager**: se succede anche ad altri plugin (FabFilter, Soundtoys...), è la verifica di FL che non funziona su quel Mac. In *Options → Manage plugins* togli la spunta a **Verify plugins**, attiva **Rescan plugins with errors** e premi **Find installed plugins**: Sosa Tune comparirà nell'elenco degli effetti del Mixer.
- **macOS blocca il plugin** (succede solo se lo copi da un altro computer): `xattr -dr com.apple.quarantine ~/Library/Audio/Plug-Ins/VST3/SosaTune.vst3`
- **Pro Tools con Blue Cat PatchWork dà errore**: installa SosaTune.vst3 in `/Library/Audio/Plug-Ins/VST3` (la cartella di sistema), poi in PatchWork apri il menu dei plugin e scegli *Refresh/Rescan*. Se l'errore resta, fai una foto del messaggio: dice esattamente cosa non va.
- **Compilarlo senza installare nulla**: se carichi il progetto su GitHub, il file `.github/workflows/build-macos.yml` lo compila da solo; lo zip pronto lo trovi nella scheda *Actions*.

## Prossimi passi

- **Fase 2: editor grafico stile Melodyne.** Il plugin registra la traccia mentre il DAW la riproduce, mostra le note come blocchi su una griglia di pianoforte, e ti permette di trascinarle, correggerle una per una e disegnare l'intonazione. Il motore è già pronto per questo: cambia solo da dove arriva la nota bersaglio.
- Note bersaglio via **MIDI** (suonare la melodia su una tastiera e far seguire la voce).

## Licenza

GPL-3.0-or-later (richiesta dal formato VST3 di nih-plug). La copia di nih-plug in `vendor/` è di Robbert van der Helm, licenza ISC.

I caratteri dell'interfaccia sono **Inter** (© The Inter Project Authors) e **Poppins** (© The Poppins Project Authors), con licenza SIL Open Font License 1.1: i testi completi sono in `plugin/assets/Inter-OFL.txt` e `plugin/assets/Poppins-OFL.txt`.
