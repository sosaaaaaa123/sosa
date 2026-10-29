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
- **Formanti**: cambia il timbro (la "lunghezza della gola") senza cambiare la nota; con la voce più grande non perde volume e le "s" e le "z" non spiccano. In HQ, pulito anche spostando molto (niente grana, niente fruscio sugli acuti, niente "battiti" del timbro quando la nota si perde per un attimo).
- **Sensibilità** del rilevamento, per voci pulite o con molta aria.
- **Preset pronti** (Factory): Default, Natural Vocal, Pop Polish, Hard Tune, Trap Lead, R&B Smooth, Soft Touch, Classic Robot, Retro Glide.
- **Quantità** di correzione e **trasposizione** fino a ±12 semitoni.
- Cambia la nota **senza cambiare il timbro** della voce (in HQ: **phase vocoder** a picchi bloccati con finestra adattata alla voce e formanti preservate dal "true envelope": niente "effetto scoiattolo", niente click).
- Due **modalità**: **Low Latency** a latenza zero per registrare con il plugin acceso, **HQ** per il mix.
- Tre impostazioni di **voce** (soprano, alto/tenore, voce bassa): decidono la nota più grave riconosciuta. Con **Auto** il plugin ascolta qualche secondo di voce e sceglie da solo.
- Interfaccia con la nota cantata, l'indicatore dello scarto in centesimi, manopole, tastiera della scala e **AutoKey** (la tonalità si imposta da sola ascoltando la voce, gli **accordi** sulla traccia o la **base mandata al sidechain**).
- **Confronto A/B** con **Copy**, numeri che si possono **scrivere a mano**, finestra **ridimensionabile** (dal 60% al 200%), **Save as Default** per far partire ogni nuova istanza dalle tue impostazioni.
- Mono e stereo, qualsiasi frequenza di campionamento, latenza comunicata al DAW (che la compensa da solo).

| Modalità | Latenza | Quando usarla |
|---|---|---|
| **Low Latency** | **0 ms** | Mentre registri: nessun ritardo in cuffia. Stessa correzione e stesso timbro di HQ (formanti al loro posto, comando Formant), calcolati solo sul passato |
| **HQ** | 64 ms (compensata dal DAW) | Per il mix: il suono più pulito, timbro preservato, intonazione più precisa |

In bypass la latenza resta quella della modalità (come chiedono le regole VST3 e come fanno i plugin professionali): accendere e spegnere il plugin durante l'ascolto non sposta la traccia e non crea buchi; in bypass esce la voce originale identica, a tempo. Per registrare senza ritardo usa Low Latency, che è a 0 ms anche con il plugin acceso.

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
2. Scegli il tipo di **Voce** (oppure **Auto** e canta qualche secondo).
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
| Formanti | Il timbro: + voce più piccola e chiara, − più grande e scura. La nota non cambia (in HQ e in Low Latency) | 0 st |
| Sensibilità | Alzala per voci con molta aria o sussurrate, abbassala se senti salti strani | 30% |
| Trasponi | Sposta tutta la voce di semitoni, mantenendo il timbro | 0 st |
| Bypass | Il pulsante di bypass del DAW: esce la voce originale, identica, con lo stesso ritardo, così resta a tempo | spento |

6. Sulla **tastiera** clicca una nota della scala per escluderla (tonalità e scala restano quelle, accanto compare quante note hai tolto, per esempio "G Minor -1"); clicca una nota fuori dalla scala per aggiungerla (allora la scala diventa "Personalizzata"). Scegliere un'altra tonalità o scala azzera le esclusioni. Ogni scelta fatta a mano spegne AutoKey, così non la sovrascrive.

### L'interfaccia

- **Vocal Range** e **Key & Scale** (in alto): menu a tendina per tipo di voce, tonalità e scala. In Vocal Range, **Auto (ascolta)**: canta qualche secondo e il tipo di voce si sceglie da solo.
- **AutoKey Listen**: accendilo e fai suonare la traccia, la tonalità si imposta da sola in Key & Scale. Ascolta la voce, ma anche gli **accordi** (pianoforte, chitarra, una base sulla traccia). Per seguire **la base della canzone**, mandala all'ingresso **sidechain** del plugin ("Sidechain (base per AutoKey)"): il plugin la legge da solo e, quando arriva, AutoKey usa quella (dopo circa 4 secondi di musica). Sotto compare per qualche secondo la tonalità trovata e da dove (voce, accordi o base). La relativa (per esempio Si♭ maggiore invece di Sol minore) ha le stesse note: il plugin non cambia avanti e indietro tra le due. AutoKey lavora con la finestra del plugin aperta.

  Per mandare la base al sidechain: **Logic** → menu "Side Chain" in alto nella finestra del plugin; **Ableton Live** → sezione Sidechain del plugin, "Audio From"; **FL Studio** → dal canale mixer della base, clic destro sulla freccia verso il canale della voce → "Sidechain to this track", poi ingranaggio del plugin → Processing: assegna quel canale all'ingresso sidechain; **Cubase / Studio One** → attiva il sidechain del plugin e crea una mandata dalla base; **Reaper** → mandata dalla base ai canali 3/4 della traccia della voce.
- **A / B / Copy** (in alto a sinistra): due impostazioni complete da confrontare. Clic su A o B per passare dall'una all'altra; **Copy** copia quella attiva sull'altra. Le due impostazioni vengono salvate con il progetto.
- **Nota al centro**: la nota che stai cantando; l'indicatore dice di quanti **centesimi** sei calante (−) o crescente (+).
- **Manopole** Detune, Retune Speed, Humanize, Flex Tune, Vibrato e Formant: trascinale in su o in giù. Tieni premuto **Maiusc** per i ritocchi fini, fai **doppio clic** per tornare al valore iniziale. **Clic sul numero** per scrivere il valore con la tastiera (Invio conferma, Esc annulla). Passandoci sopra col mouse compare la spiegazione.
- **Low Latency / HQ** e **Modern / Classic** (in alto a destra, nel riquadro del preset): 0 ms per registrare o HQ per il mix; note già intonate oppure ogni nota che scivola sulla nota giusta.
- **Edit Notes** (in basso): clic su una nota della scala per escluderla o riammetterla (la nota esclusa ha l'anello spento e una barra; tonalità e scala non cambiano), clic su una nota fuori dalla scala per aggiungerla (scala personalizzata).
- **☰** (preset): la lista dei preset, **Save as Default** (le nuove istanze partono dalle impostazioni attuali) e **Reset Default**.
- **⚙**: Amount, Transpose e Sensitivity (anche scrivendo il numero), misura della finestra (75-200%), Save/Reset Default e la latenza attuale. **↶ ↷**: annulla e ripeti. **?**: guida rapida.
- **Angolo in basso a destra**: trascinalo per ingrandire o rimpicciolire la finestra (se il DAW lo permette).

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
- **Scale** (`dsp/src/scale.rs`): sceglie la nota ammessa più vicina, con un po' di isteresi per non "tremolare" quando sei a metà tra due note. Dentro una frase si cambia nota quando la voce resta un attimo dalla parte nuova (25-90 ms, così il vibrato non fa rimbalzare la correzione); con Retune Speed basso (effetto autotune, fino a 10 ms, poi crescendo fino a 40 ms) l'attesa sparisce come a Retune 0 quando la voce va sulla nota nuova: negli abbellimenti e nei passaggi veloci ogni nota viene presa (note sotto i 90 ms mancate con Retune 10: 8-12% invece di 11-20%). Resta solo tra due note a un tono con la voce in bilico a metà (la correzione non salta avanti e indietro).
- **Correttore** (`dsp/src/corrector.rs`): decide di quanto spostare la nota e con che velocità (due filtri in fila che arrivano al bersaglio in "velocità" millisecondi con una curva a S: il passaggio tra due note parte e arriva morbido, senza scatti). Qui vivono anche Humanize (la velocità rallenta man mano che la nota dura), Flex-Tune (il peso della correzione cala con la distanza dalla nota) e il vibrato naturale (isolato con un filtro passa-banda centrato su 5,5 Hz, che non lo sfasa e permette quindi anche di toglierlo).
- **Phase vocoder** (modalità HQ, `dsp/src/vocoder.rs`): ogni 5 ms prende una "fetta" di voce, la smussa con una finestra di Hann lunga **6 periodi della voce** (da 32 ms per le voci acute a 64 ms per i bassi: le armoniche fitte delle voci maschili restano separate) e la trasforma in frequenze (FFT). Trova le armoniche (i picchi), misura la loro frequenza esatta dalla rotazione della fase, e sposta ogni armonica con le righe vicine alla frequenza giusta (frequenza × rapporto, anche di frazioni di riga); quando la nota scende le armoniche si avvicinano, e ogni armonica scrive solo fino a metà strada dalle vicine (le code non si sommano tra loro: meno fruscio tra le armoniche). La fase di ogni armonica continua a girare alla velocità della frequenza nuova e le righe vicine la seguono ("identity phase locking", Laroche e Dolson); in più le armoniche fino a 16 kHz restano in fase tra loro (l'armonica h gira di h volte la fondamentale), così la forma d'onda di ogni ciclo della voce si conserva, anche negli acuti: niente suono metallico o "phasey" (con il limite a 5 kHz gli impulsi della voce si sparpagliavano sopra i 5 kHz, una risonanza sui medio-alti). Le armoniche vengono cercate fino a 100 dB sotto la più forte, così anche quelle deboli in alto finiscono al posto giusto. Il timbro (inviluppo spettrale) è misurato con il **true envelope**: un cepstrum lisciato e ripetuto finché passa sulle cime delle armoniche, prolungato in piano sotto la fondamentale; viene rimesso al suo posto, o spostato con il comando Formant. Per Formant si usa un inviluppo lisciato nel tempo (20 ms sulle vocali, fino a 100 ms sopra i 4 kHz: la misura su una voce vera trema di ~1 dB ogni 5 ms e, spostata, rendeva la voce granulosa), lo spostamento sfuma sopra i 7 kHz (l'aria e le "s" restano come sono: niente fruscio quando la voce diventa più piccola, niente voce spenta quando diventa più grande) e le spinte oltre ~20 dB si ammorbidiscono (riempire una valle tra due formanti alzava anche il rumore che c'era dentro); con le formanti in basso il calo di volume del corpo della voce (fino a 3 kHz) si recupera, misurato sulle vocali come i LUFS, mentre gli acuti, le "s" e le "z" restano come prima. Quando la nota si perde per un attimo in mezzo al canto (frame che passano senza correzione) il timbro resta spostato, solo come filtro, pieno per 16 ms e poi a sfumare entro 64 ms; non se il frame ha molti più acuti della nota di prima (una "s", una "f" che comincia): quelli passano intatti come prima. Poi FFT inversa e somma sovrapposta (overlap-add) tenendo di ogni fetta solo i 16 ms centrali: nei passaggi tra le note non si sommano pezzi spostati in modo diverso (che darebbero una risonanza sui medio-alti). Consonanti, "s", respiri e attacchi passano identici (niente ronzio metallico sulle sibilanti).
- **Latenza zero** (modalità Low Latency, `dsp/src/zerolat.rs`): la voce viene letta da una "testina" che scorre sul segnale appena arrivato un po' più veloce (nota più acuta) o più lenta (più grave). Quando arriva al presente o resta troppo indietro salta di un ciclo esatto della voce, nel punto in cui la forma d'onda combacia, con una dissolvenza: il ritardo resta di pochi millesimi e il DAW riceve latenza zero. Le consonanti passano identiche. Dopo la testina il **timbro di HQ** (`dsp/src/timbre.rs`): ogni ~5 ms, sulla voce appena arrivata, si misura l'inviluppo con lo stesso true envelope di HQ e si calcola lo stesso guadagno (formanti rimesse al loro posto, comando Formant con le sue regole); quel guadagno diventa un filtro a fase minima di 8 ms (nessun ritardo), che passa dal vecchio al nuovo con una dissolvenza. Limitato a ±12 dB e sfumato sopra i 7 kHz; nota ferma e Formant a 0: la voce passa identica (anche con il vibrato: la misura ciclo per ciclo e la sua "tendenza" contano solo quanto la correzione inchioda la voce alla nota, piena con Retune 0 e nulla con Quantità 0). La scelta della nota è la stessa di HQ (stesse regole, stessa velocità, Humanize, vibrato), ma misurata sugli **ultimi cicli appena arrivati** (3 periodi, insieme a un quarto della stima di YIN) invece che al centro dell'analisi (~20 ms prima): la correzione reagisce dopo pochi cicli di voce. (Su 1,5 periodi soltanto, su una voce ruvida la nota ballava di qualche decina di centesimi e con Retune basso la correzione saltava avanti e indietro tra due note vicine: 40-83 salti rapidi al minuto contro i 13-22 di HQ, un tremolio sporco invece degli scalini netti dell'effetto autotune; ora 9-22.) Subito dopo l'attacco, se YIN perde la nota per un attimo ma gli ultimi cicli sono chiaramente la stessa nota, la nota continua. All'**attacco** di una nota non si aspetta che la finestra dell'analisi sia piena: appena tre cicli chiari (con il volume che sale davvero) la nota viene corretta, come in HQ, invece di uscire stonata per i primi ~20 ms. Rispetto a HQ manca solo l'anticipo (HQ vede avanti): sulle voci di prova l'intonazione di Low Latency segue quella di HQ con 5-15 ms di ritardo (prima 20-25 ms), e quando la voce sta esattamente a metà tra due note le due modalità possono scegliere note diverse.

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
- **prove dell'interfaccia** (`cargo test -p sosatune --release`): clic e trascinamenti simulati su tutti i comandi dell'interfaccia e bypass del DAW, numeri scritti a mano, A/B e Copy, Save as Default, Vocal Range Auto, ridimensionamento della finestra;
- **auval** di Apple per la versione AU.

Configurazioni supportate: stereo → stereo, mono → mono, mono → stereo (traccia vocale mono in Pro Tools/PatchWork, Logic, Cubase...), ognuna con un ingresso **sidechain** stereo facoltativo (per AutoKey).

## Struttura del progetto

```text
sosatune/
├── dsp/                 il motore audio (nessuna libreria esterna)
│   ├── src/             yin, fft, scale, corrector, vocoder (HQ), zerolat (Low Latency),
│   │                    chroma (tonalità dagli accordi), synth, wav
│   ├── examples/        demo.rs e process.rs (provare su file WAV)
│   └── tests/           test di robustezza
├── plugin/              il plugin VST3/CLAP
│   └── src/             lib.rs (collegamento al DAW), params.rs (parametri),
│                        editor.rs (interfaccia), shared.rs (dati per il grafico,
│                        AutoKey, tipo di voce), defaults.rs (Save as Default)
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
