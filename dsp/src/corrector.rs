//! Il correttore: unisce rilevamento (YIN), scelta della nota (scale) e
//! spostamento dell'intonazione (phase vocoder). E' il "cervello" del plugin.

use crate::formant::FormantFilter;
use crate::scale::{hz_to_note, NoteSnapper, VoiceType};
use crate::vocoder::{GrainPitch, PhaseVocoder};
use crate::yin::YinDetector;
use crate::zerolat::DelayShifter;
use crate::MAX_CHANNELS;

/// Le impostazioni scelte dall'utente (vengono dai parametri del plugin).
#[derive(Clone, Copy, Debug)]
pub struct CorrectionSettings {
    /// Le 12 note ammesse (indice 0 = Do).
    pub notes: [bool; 12],
    /// Velocita' di correzione in millisecondi: 0 = istantanea (effetto
    /// robotico "alla T-Pain"), 50-150 = naturale, 400 = molto morbida.
    pub retune_ms: f32,
    /// Quanta correzione applicare: 0.0 = nessuna, 1.0 = completa.
    pub amount: f32,
    /// Trasposizione aggiuntiva in semitoni (+12 = un'ottava sopra).
    pub transpose: f32,
    /// Humanize (0..1): sulle note lunghe la correzione diventa piu' morbida,
    /// cosi' le note tenute non suonano "congelate", mentre quelle brevi
    /// restano corrette con la velocita' scelta.
    pub humanize: f32,
    /// Flex-Tune (0..1): corregge solo quando la voce e' gia' vicina alla
    /// nota; le note volutamente "sporche" (scivolate, blue note) passano.
    pub flex_tune: f32,
    /// Vibrato naturale (-1..1): negativo lo riduce, positivo lo aggiunge
    /// (anche con correzione robotica, che altrimenti lo cancellerebbe).
    pub vibrato: f32,
    /// Correzione delle formanti (il tasto Formant di Auto-Tune): `true` =
    /// il timbro resta quello della voce anche spostando la nota (niente
    /// "effetto scoiattolo" trasponendo); `false` = le formanti si spostano
    /// insieme alla nota, come in un pitch shifter semplice.
    pub preserve_formants: bool,
    /// Spostamento delle formanti in semitoni (la manopola Throat: gola piu'
    /// corta = positivo, voce piu' "piccola"; piu' lunga = negativo, voce
    /// piu' "grande"), senza cambiare la nota. Agisce solo con
    /// `preserve_formants`.
    pub formant: f32,
    /// Sensibilita' del rilevamento (0..1): piu' alta = riconosce anche voci
    /// con molta aria, ma con piu' rischio di errori.
    pub tracking: f32,
    /// Accordatura di riferimento: la frequenza del La4 (440 Hz di solito).
    /// Tutte le note della scala si spostano insieme a lei.
    pub reference_hz: f32,
    /// Stile "classico": ogni nota parte dall'intonazione cantata e scivola
    /// sulla nota giusta con la velocita' scelta, e il bersaglio cambia
    /// appena la voce e' piu' vicina a un'altra nota (il carattere degli
    /// autotune di una volta). `false` = stile moderno: note che partono gia'
    /// intonate e nessun rimbalzo tra due note.
    pub classic: bool,
}

impl Default for CorrectionSettings {
    fn default() -> Self {
        Self {
            notes: [true; 12],
            retune_ms: 20.0,
            amount: 1.0,
            transpose: 0.0,
            humanize: 0.0,
            flex_tune: 0.0,
            vibrato: 0.0,
            preserve_formants: true,
            formant: 0.0,
            tracking: 0.3,
            reference_hz: 440.0,
            classic: false,
        }
    }
}

/// Curva "a S" da 0 (x <= a) a 1 (x >= b).
fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Peso della correzione con Flex-Tune, in base a quanto la voce e' lontana
/// dalla nota (`distance` in semitoni, da 0 a 0.5).
fn flex_weight(distance: f32, flex: f32) -> f32 {
    let flex = flex.clamp(0.0, 1.0);
    if flex <= 0.0 {
        return 1.0;
    }
    // Vicino alla nota la correzione resta piena; lontano dalla nota cala in
    // proporzione alla manopola: al 25% le note lontane sono corrette al 75%,
    // al 50% a meta', al 100% non vengono toccate. (Prima bastava un 25% per
    // lasciare quasi tutto com'era.) La zona "vicina" si restringe da 30 a
    // 15 centesimi alzando la manopola.
    let outer = 0.45 - 0.2 * flex;
    let local = 1.0 - smoothstep(outer * 0.5, outer, distance);
    1.0 - flex * (1.0 - local)
}

/// Filtro passa-banda del secondo ordine centrato sulla velocita' tipica del
/// vibrato (5,5 oscillazioni al secondo): isola il vibrato dalla curva
/// dell'intonazione. Al centro della banda non sposta la fase, quindi il
/// vibrato stimato e' "a tempo" con quello vero e lo si puo' anche togliere.
#[derive(Clone, Copy, Debug, Default)]
struct VibratoFilter {
    b0: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl VibratoFilter {
    const CENTER_HZ: f32 = 5.5;
    const Q: f32 = 0.8;

    fn new(sample_rate: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * Self::CENTER_HZ / sample_rate;
        let alpha = w0.sin() / (2.0 * Self::Q);
        let a0 = 1.0 + alpha;
        Self {
            b0: alpha / a0,
            b2: -alpha / a0,
            a1: -2.0 * w0.cos() / a0,
            a2: (1.0 - alpha) / a0,
            ..Default::default()
        }
    }

    /// Riparte come se l'ingresso fosse sempre stato `value` (uscita zero,
    /// nessun "colpo" iniziale).
    fn reset_to(&mut self, value: f32) {
        self.x1 = value;
        self.x2 = value;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Cosa e' successo nell'ultima analisi: serve all'interfaccia grafica.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PitchReport {
    /// C'e' una nota cantata?
    pub voiced: bool,
    /// Nota cantata (MIDI con decimali), NaN se non c'e'.
    pub input_note: f32,
    /// Nota in uscita dopo la correzione, NaN se non c'e'.
    pub output_note: f32,
    /// Nota bersaglio della scala, NaN se non c'e'.
    pub target_note: f32,
}

impl PitchReport {
    const SILENT: PitchReport = PitchReport {
        voiced: false,
        input_note: f32::NAN,
        output_note: f32::NAN,
        target_note: f32::NAN,
    };
}

/// Un'analisi passata: in che punto del segnale, che periodo, che correzione.
#[derive(Clone, Copy, Debug, Default)]
struct Analysis {
    center: f64,
    period: f32,
    ratio: f32,
    voiced: bool,
    valid: bool,
    /// `true` se e' un "buco" tenuto (nessuna nota letta, si ripete l'ultima).
    held: bool,
    /// Quanto la correzione deve "inchiodare" la voce alla nota anche tra
    /// un'analisi e l'altra (0 = segue la voce, 1 = robotico): vedi
    /// [`periodicity`].
    lock: f32,
}

const HISTORY: usize = 64;

/// Fino a quanti campioni prima della prima analisi cantata (e dopo l'ultima)
/// la voce viene gia' trattata come nota (vedi [`lookup`]).
const VOICED_EDGE_MS: f64 = 7.0;

pub struct PitchCorrector {
    sample_rate: f32,
    channels: usize,
    voice: VoiceType,

    detector: YinDetector,
    /// HQ: phase vocoder con finestra che segue la voce (vedi [`crate::vocoder`]).
    vocoder: PhaseVocoder,
    /// Low Latency: testina a latenza zero (vedi [`crate::zerolat`]).
    /// Entrambi pronti: cambiare modalita' durante la riproduzione non
    /// alloca memoria.
    zero: DelayShifter,
    /// Low Latency: formanti riportate al loro posto dopo la testina.
    formants: FormantFilter,
    /// Di quanto spostare le formanti dopo la testina (cambia con dolcezza).
    formant_warp: f32,

    /// Ultimi campioni (mono) per il rilevatore.
    mono: Vec<f32>,
    mono_mask: usize,
    frame: Vec<f32>,

    /// Ogni quanti campioni rifare l'analisi (circa 5 ms).
    hop: usize,
    /// Modalita' HQ (motore phase vocoder).
    hq: bool,
    /// Analisi consecutive per confermare un salto / per "tenere" una nota
    /// (corrispondono sempre a ~15 ms, qualunque sia `hop`).
    leap_confirmations: u32,
    min_run_to_hold: u32,
    countdown: usize,
    position: u64,

    /// Le analisi recenti: lo shifter lavora "in ritardo" (latenza) e deve
    /// usare il pitch giusto per il pezzo di audio che sta elaborando.
    history: [Analysis; HISTORY],
    history_head: usize,

    snapper: NoteSnapper,
    /// Correzione attuale in semitoni (smussata secondo la velocita').
    correction: f32,
    /// Primo stadio del filtro della correzione (vedi `approach`).
    correction_mid: f32,
    was_voiced: bool,
    /// Per brevi "buchi" nel rilevamento in mezzo a una nota manteniamo la
    /// correzione precedente invece di toglierla di colpo.
    hold_left: usize,
    hold_samples: usize,
    /// Quante analisi cantate di fila (senza buchi tenuti in mezzo).
    voiced_run: u32,
    last_period: f32,
    last_ratio: f32,
    unvoiced_period: f32,
    /// Ultima nota rilevata e quante analisi consecutive hanno visto un salto.
    last_note: f32,
    leap_count: u32,
    /// La nota bersaglio attuale e da quanto tempo (secondi) la si sta cantando.
    current_target: Option<i32>,
    note_age: f32,
    /// Da quanto (s) la voce sta piu' vicina a un'altra nota che a quella
    /// bersaglio: serve a distinguere vibrato e oscillazioni da un vero cambio.
    away_time: f32,
    /// L'analisi precedente ha trovato una nota cantata? (Se no, la prossima
    /// nota si sceglie da capo.)
    prev_analysis_voiced: bool,
    /// Vibrato della voce in ingresso e di quella gia' corretta.
    vibrato_in: VibratoFilter,
    vibrato_out: VibratoFilter,
}

impl PitchCorrector {
    /// Salto (in semitoni) oltre il quale un cambio di nota e' "sospetto".
    const LEAP_SEMITONES: f32 = 7.0;
    /// Per quanto tempo (s) un salto deve durare per essere considerato vero,
    /// e quanto a lungo (s) si deve cantare per "tenere" la nota durante un
    /// buco del rilevamento.
    const LEAP_CONFIRM_S: f32 = 0.015;
    const MIN_RUN_TO_HOLD_S: f32 = 0.015;

    /// Crea il correttore. Tutta la memoria viene allocata qui, mai durante
    /// l'elaborazione audio.
    pub fn new(sample_rate: f32, channels: usize) -> Self {
        let channels = channels.clamp(1, MAX_CHANNELS);
        let detector = YinDetector::new(sample_rate, VoiceType::LOWEST_SUPPORTED_HZ);
        let vocoder = PhaseVocoder::new(channels, sample_rate);
        let zero = DelayShifter::new(channels, sample_rate, detector.capacity_max_period());
        // Il buffer serve anche alla misura "ciclo per ciclo" dei frame, che
        // guarda fino a ~3 periodi indietro rispetto all'ultimo campione.
        let mono_len = (detector.max_frame_len()
            + 4 * detector.capacity_max_period()
            + 1)
        .next_power_of_two();
        let frame = vec![0.0; detector.max_frame_len()];
        let hop = ((sample_rate * 0.005).round() as usize).max(32);

        let mut corrector = Self {
            sample_rate,
            channels,
            voice: VoiceType::AltoTenor,
            detector,
            vocoder,
            zero,
            formants: FormantFilter::new(sample_rate, channels),
            formant_warp: 1.0,
            mono: vec![0.0; mono_len],
            mono_mask: mono_len - 1,
            frame,
            hop,
            hq: false,
            leap_confirmations: 3,
            min_run_to_hold: 3,
            countdown: hop,
            position: 0,
            history: [Analysis::default(); HISTORY],
            history_head: 0,
            snapper: NoteSnapper::default(),
            correction: 0.0,
            correction_mid: 0.0,
            was_voiced: false,
            hold_left: 0,
            hold_samples: (sample_rate * 0.03) as usize,
            voiced_run: 0,
            last_period: 0.0,
            last_ratio: 1.0,
            unvoiced_period: 0.0,
            last_note: 0.0,
            leap_count: 0,
            current_target: None,
            note_age: 0.0,
            away_time: 0.0,
            prev_analysis_voiced: false,
            vibrato_in: VibratoFilter::new(sample_rate / hop as f32),
            vibrato_out: VibratoFilter::new(sample_rate / hop as f32),
        };
        corrector.set_quality(false);
        corrector.set_voice(VoiceType::AltoTenor);
        corrector
    }

    /// Modalita' di qualita':
    /// - `false` = **Low Latency**: latenza zero (vedi [`crate::zerolat`]),
    ///   per registrare con il plugin acceso senza ritardi;
    /// - `true` = **HQ**: phase vocoder con finestra che segue la voce (lunga
    ///   per le voci maschili, corta per quelle femminili), solo le armoniche
    ///   vengono spostate, timbro preservato. Latenza 64 ms, compensata dal DAW.
    ///
    /// Cambia la latenza. Non alloca memoria.
    pub fn set_quality(&mut self, hq: bool) {
        self.hq = hq;
        let per_second = self.sample_rate / self.hop as f32;
        let analyses = |seconds: f32| ((seconds * per_second).round() as u32).max(1);
        self.leap_confirmations = analyses(Self::LEAP_CONFIRM_S);
        self.min_run_to_hold = analyses(Self::MIN_RUN_TO_HOLD_S);
        self.reset();
    }

    pub fn is_hq(&self) -> bool {
        self.hq
    }

    /// Cambia il tipo di voce (la nota piu' grave riconosciuta).
    pub fn set_voice(&mut self, voice: VoiceType) {
        self.voice = voice;
        self.detector.set_range(voice.lowest_hz(), voice.window_hz());
        let max_period = self.detector.max_period() as f32;
        // Per le parti senza nota (consonanti, respiri) usiamo grani di ~5 ms.
        self.unvoiced_period = (self.sample_rate * 0.005).min(max_period);
        self.reset();
    }

    pub fn voice(&self) -> VoiceType {
        self.voice
    }

    /// Ritardo introdotto dal plugin, in campioni. Il DAW lo compensa
    /// automaticamente (Plugin Delay Compensation).
    pub fn latency(&self) -> usize {
        if self.hq {
            self.vocoder.latency()
        } else {
            self.zero.latency()
        }
    }

    /// Intervallo di tempo tra due analisi, in secondi (per il grafico).
    pub fn analysis_interval(&self) -> f32 {
        self.hop as f32 / self.sample_rate
    }

    /// Azzera tutto lo stato (buffer, correzione in corso).
    pub fn reset(&mut self) {
        self.vocoder.reset();
        self.zero.reset();
        self.formants.reset();
        self.formant_warp = 1.0;
        self.mono.fill(0.0);
        self.countdown = self.hop;
        self.position = 0;
        self.history = [Analysis::default(); HISTORY];
        self.history_head = 0;
        self.snapper.reset();
        self.correction = 0.0;
        self.correction_mid = 0.0;
        self.was_voiced = false;
        self.hold_left = 0;
        self.voiced_run = 0;
        self.last_period = self.unvoiced_period;
        self.last_ratio = 1.0;
        self.last_note = 0.0;
        self.leap_count = 0;
        self.current_target = None;
        self.note_age = 0.0;
        self.away_time = 0.0;
        self.prev_analysis_voiced = false;
        self.vibrato_in.reset_to(0.0);
        self.vibrato_out.reset_to(0.0);
    }

    /// Elabora un istante: `frame` contiene un campione per canale e viene
    /// sovrascritto con il risultato. Restituisce `Some` quando e' stata fatta
    /// una nuova analisi (circa ogni 5 ms), utile per aggiornare il grafico.
    #[inline]
    pub fn process_frame(&mut self, frame: &mut [f32], settings: &CorrectionSettings) -> Option<PitchReport> {
        let channels = frame.len().min(self.channels);

        // 1) Mix mono per il rilevatore.
        let mono = frame[..channels].iter().sum::<f32>() / channels.max(1) as f32;
        self.mono[self.position as usize & self.mono_mask] = mono;

        // 2) Ogni `hop` campioni: nuova analisi.
        let mut report = None;
        self.countdown -= 1;
        if self.countdown == 0 {
            self.countdown = self.hop;
            if self.position + 1 >= self.detector.frame_len() as u64 {
                report = Some(self.analyze(settings));
            }
        }

        // 3) Pitch shifting con il pitch corretto per il punto che si sta elaborando.
        let mut input = [0.0f32; MAX_CHANNELS];
        let mut output = [0.0f32; MAX_CHANNELS];
        input[..channels].copy_from_slice(&frame[..channels]);
        let history = &self.history;
        let mono = &self.mono;
        let mono_mask = self.mono_mask;
        let newest = self.position;
        let edge = VOICED_EDGE_MS * 0.001 * self.sample_rate as f64;
        // Throat: le formanti si spostano solo con la correzione delle formanti
        // attiva. Senza, seguono la nota (formant = rapporto di pitch, vedi sotto).
        let preserve = settings.preserve_formants;
        let formant = if preserve { 2f32.powf(settings.formant.clamp(-12.0, 12.0) / 12.0) } else { 1.0 };
        let with_formant = |g: GrainPitch| GrainPitch { formant: if preserve { formant } else { g.ratio }, ..g };
        let unvoiced = GrainPitch { period: self.unvoiced_period, ratio: 1.0, voiced: false, formant };
        // Centro dell'analisi piu' recente (per la latenza zero).
        let latest = &self.history[(self.history_head + HISTORY - 1) % HISTORY];
        let latest_center = if latest.valid { latest.center } else { f64::NEG_INFINITY };
        let latest_period = latest.period;
        // `t`: dove leggere le analisi; `here`: dove misurare la voce ciclo per ciclo.
        let pitch_between = |t: f64, here: f64| {
            let Some((mut g, lock)) = lookup(history, t, edge) else {
                return unvoiced;
            };
            // Attacchi bruschi (consonanti esplosive come "t", "p", "k" dopo un
            // attimo di silenzio): il frame li conterrebbe e,
            // spostato, li sbaverebbe o li anticiperebbe (un
            // "gracchio" sull'attacco della parola). Quel frame passa intatto.
            if g.voiced && is_transient(mono, mono_mask, newest, here, g.period) {
                return GrainPitch { period: g.period, ratio: 1.0, voiced: false, formant };
            }
            if g.voiced {
                // Quanto e' "nota" il suono proprio dove cade questo frame?
                // Le analisi durano ~20 ms e vengono tenute per 30 ms nei buchi:
                // una consonante ("s", "t", "ch"), un respiro o la raucedine a
                // fine parola possono quindi finire dentro un tratto "cantato".
                // Spostati come se fossero voce, quei rumori diventano un ronzio
                // metallico che gracchia. Qui si misura ciclo per ciclo: dove la
                // periodicita' manca il frame passa intatto, nel mezzo la
                // correzione si attenua con dolcezza.
                let Some((local, score)) = periodicity(mono, mono_mask, newest, here, g.period) else {
                    return with_formant(g);
                };
                let voicing = smoothstep(NOISE_SCORE, VOICE_SCORE, score);
                if voicing <= 0.0 {
                    return GrainPitch { period: g.period, ratio: 1.0, voiced: false, formant };
                }
                g.ratio = g.ratio.powf(voicing);
                // Misura "ciclo per ciclo": l'intonazione esatta proprio dove
                // cade questo frame. Le analisi (ogni 5 ms, su ~20 ms di audio)
                // seguono bene le note tenute, ma nei passaggi veloci sono una
                // media: senza questa misura la voce uscirebbe "storta" tra una
                // nota e l'altra.
                if lock > 0.0 && score >= 0.8 {
                    // Frequenza voluta = quella dell'analisi x rapporto; quella
                    // vera qui e' sr/local: il rapporto si corregge di conseguenza.
                    g.ratio *= (local / g.period).powf(lock * voicing);
                    g.period = local;
                }
            }
            with_formant(g)
        };
        if self.hq {
            self.vocoder.process(&input[..channels], &mut output[..channels], |t| pitch_between(t, t));
        } else {
            // Latenza zero: l'audio non aspetta l'analisi del punto esatto.
            // Si usa l'ultima analisi disponibile, e la misura ciclo per ciclo
            // sui cicli appena arrivati la aggiorna (la voce intanto puo'
            // essersi mossa, per esempio con il vibrato).
            let here = newest as f64 - 1.4 * latest_period as f64 - 3.0;
            self.zero.process(&input[..channels], &mut output[..channels], |_| {
                let mut g = pitch_between(latest_center, here);
                if g.voiced {
                    // Tendenza: di quanto si e' mossa la voce nell'ultimo ciclo,
                    // proiettata fino a ora (sulla voce di prova: intonazione in
                    // uscita mediana 3,0 centesimi invece di 4,8).
                    let p = g.period;
                    if let (Some((a, sa)), Some((b, sb))) = (
                        periodicity(mono, mono_mask, newest, here - p as f64, p),
                        periodicity(mono, mono_mask, newest, here, p),
                    ) {
                        if sa >= 0.85 && sb >= 0.85 {
                            let slope = 12.0 * (a / b).log2() / p; // semitoni per campione
                            let ahead = (newest as f64 - here) as f32;
                            let drift = (slope * ahead).clamp(-0.5, 0.5);
                            g.ratio *= 2f32.powf(-drift / 12.0);
                        }
                    }
                }
                g
            });
            // Formanti (vedi `formant.rs`): la testina le ha spostate insieme
            // alla nota (del rapporto di pitch). Il filtro le misura sulla voce
            // gia' spostata e le riporta dove erano (diviso il rapporto),
            // oppure dove le vuole Throat. Con la correzione delle formanti
            // spenta il filtro e' esattamente neutro, e lo e' anche quando la
            // nota non si sposta: il suono della Low Latency resta quello.
            let target = if preserve { formant / self.zero.current_ratio() as f32 } else { 1.0 };
            // Il timbro e' misurato sugli ultimi ~24 ms. Verso il filtro
            // neutro (warp = 1) si va subito: per esempio quando finisce una
            // nota e arriva una consonante, che la testina non sposta. Verso
            // una correzione piu' forte si va piano (~25 ms), il tempo perche'
            // la misura descriva la voce nuova: all'attacco di una nota il
            // filtro non applica il timbro di un attimo prima.
            // Se il bersaglio sta dall'altra parte del neutro (la nota passa da
            // sopra a sotto l'intonazione cantata) prima si torna al neutro.
            let crossing = (target - 1.0) * (self.formant_warp - 1.0) < 0.0;
            let goal = if crossing { 1.0 } else { target };
            let toward_neutral = (goal - 1.0).abs() < (self.formant_warp - 1.0).abs();
            let time = if toward_neutral { 0.002 } else { 0.025 };
            self.formant_warp += (goal - self.formant_warp) * (1.0 / (time * self.sample_rate)).min(1.0);
            let warp = self.formant_warp;
            let out_mono = output[..channels].iter().sum::<f32>() / channels as f32;
            self.formants.push(out_mono, warp);
            for out in output.iter_mut().take(channels).enumerate() {
                let (ch, y) = out;
                let e = self.formants.whiten(ch, *y);
                *y = self.formants.color(ch, e, *y);
            }
        }
        frame[..channels].copy_from_slice(&output[..channels]);

        self.position += 1;
        report
    }

    fn analyze(&mut self, settings: &CorrectionSettings) -> PitchReport {
        // Copia gli ultimi `len` campioni dal buffer circolare.
        let len = self.detector.frame_len();
        let start = self.position + 1 - len as u64;
        for (i, x) in self.frame[..len].iter_mut().enumerate() {
            *x = self.mono[(start as usize + i) & self.mono_mask];
        }
        self.detector.set_threshold(0.08 + 0.22 * settings.tracking.clamp(0.0, 1.0));
        let mut estimate = self.detector.detect(&self.frame[..len]);

        // Errore d'ottava verso il basso: a meta' di una nota, con la voce un
        // po' rauca, il rilevatore puo' leggere il doppio (o il triplo) del
        // periodo. Se la nota appena letta e' un'ottava (o un'ottava e una
        // quinta) sotto quella di prima e il periodo "giusto" e' ancora un
        // buon avvallamento, teniamo quello: altrimenti i grani prenderebbero
        // due cicli alla volta e la voce "gracchierebbe" (un ringhio sotto la nota).
        if estimate.voiced && self.was_voiced {
            let drop = self.last_note - hz_to_note(self.sample_rate / estimate.period.max(1.0));
            for (k, semitones) in [(2.0f32, 12.0f32), (3.0, 19.02)] {
                if (drop - semitones).abs() < 1.0 {
                    if let Some(better) = self.detector.dip_near(estimate.period / k) {
                        estimate = better;
                    }
                    break;
                }
            }
        }

        let window = self.detector.window_len() as f64;
        let report;
        let (center, period, ratio, voiced);
        let mut lock = 0.0f32;

        // Un salto enorme di intonazione in mezzo a una nota e' quasi sempre un
        // errore del rilevatore (tipicamente un'ottava sopra, quando la voce si
        // spegne). Lo accettiamo solo se dura per alcune analisi di fila.
        // Con un'accordatura diversa da 440 Hz tutte le note si spostano
        // insieme: misuriamo la voce rispetto alla griglia spostata.
        let tuning = 12.0 * (settings.reference_hz.clamp(400.0, 480.0) / 440.0).log2();
        let detected_note = hz_to_note(self.sample_rate / estimate.period.max(1.0)) - tuning;
        let suspicious_leap = estimate.voiced
            && self.was_voiced
            && (detected_note - self.last_note).abs() > Self::LEAP_SEMITONES
            && self.leap_count < self.leap_confirmations;
        if suspicious_leap {
            self.leap_count += 1;
        } else {
            self.leap_count = 0;
        }

        if estimate.voiced && !suspicious_leap {
            let note = detected_note;
            let hop_seconds = self.hop as f32 / self.sample_rate;
            self.last_note = note;

            // Quale nota? Due regole contro i difetti tipici della correzione:
            //  - dopo una pausa, anche brevissima (una consonante come la "t"),
            //    la nota si sceglie da capo: si riparte da quella cantata ora,
            //    non da quella di prima della pausa;
            //  - durante la nota, si passa a quella vicina solo se la voce ci
            //    resta per almeno 25 ms (o salta chiaramente lontano): cosi'
            //    vibrato e piccole oscillazioni non fanno "rimbalzare" la
            //    correzione tra due note (effetto chorus, voce che gratta).
            // Nello stile robotico (velocita' 0) la nota cambia subito, come
            // negli autotune classici: i "gradini" sono l'effetto voluto.
            let fresh = !self.prev_analysis_voiced;
            // Nei primi 30 ms di una nota le letture sono ancora disturbate
            // dall'attacco: la nota si sceglie di nuovo a ogni analisi (la
            // piu' vicina, senza isteresi) e la correzione resta "da inizio
            // nota". Cosi' una prima lettura storta non resta incollata.
            let young_runs = ((0.03 / hop_seconds).ceil() as u32).max(1);
            let young = !fresh && self.voiced_run < young_runs;
            let robotic = settings.retune_ms < 1.0;
            let classic = settings.classic;
            let proposal = if classic || young {
                self.snapper.propose_plain(note, &settings.notes)
            } else {
                self.snapper.propose(note, &settings.notes)
            };
            let current = self.snapper.current();
            // Se la voce sta a meta' strada tra due note della scala (per
            // esempio un Do# in Sol minore, tra Do e Re) basta un niente per
            // farla sembrare piu' vicina all'una o all'altra: cambiando nota
            // ogni volta la correzione salterebbe avanti e indietro di un tono
            // (voce che gracchia). Piu' la scelta e' incerta, piu' a lungo la
            // voce deve restare dalla parte nuova prima di cambiare.
            let away_needed = match (current, proposal) {
                _ if robotic || classic => 0.0,
                (Some(c), Some(p)) if p != c => {
                    let margin = (note - c as f32).abs() - (note - p as f32).abs();
                    let decisive = ((margin - NoteSnapper::hysteresis(c, p)) / 0.6).clamp(0.0, 1.0);
                    0.09 + (0.025 - 0.09) * decisive
                }
                _ => 0.025,
            };
            let target = match (current, proposal) {
                (Some(c), Some(p)) if p != c && !fresh && !young => {
                    self.away_time += hop_seconds;
                    if self.away_time >= away_needed || (note - c as f32).abs() >= 1.5 {
                        self.away_time = 0.0;
                        self.snapper.set(Some(p));
                        Some(p)
                    } else {
                        Some(c)
                    }
                }
                _ => {
                    self.away_time = 0.0;
                    self.snapper.set(proposal);
                    proposal
                }
            };
            self.prev_analysis_voiced = true;

            // Da quanto si sta cantando questa nota? (serve a Humanize e al vibrato)
            let reference = target.map_or(note, |t| t as f32);
            // Inizio di una nota dopo una pausa (o una consonante, anche breve).
            // Nello stile classico anche i primi 30 ms ripartono dalla voce.
            let starting = !self.was_voiced || fresh || (young && classic);
            if target != self.current_target || !self.was_voiced {
                self.current_target = target;
                self.note_age = 0.0;
                self.vibrato_in.reset_to(note - reference);
                self.vibrato_out.reset_to(note - reference);
            } else {
                self.note_age += hop_seconds;
            }

            let weight = target.map_or(0.0, |t| {
                settings.amount.clamp(0.0, 1.0) * flex_weight((t as f32 - note).abs(), settings.flex_tune)
            });
            let desired = target.map_or(0.0, |t| (t as f32 - note) * weight);
            if starting && classic {
                // Stile classico: la nota parte com'e' stata cantata e
                // scivola sulla nota giusta con la velocita' scelta.
                self.correction = if settings.retune_ms < 1.0 { desired } else { 0.0 };
                self.correction_mid = self.correction;
            } else if young && settings.retune_ms >= 1.0 {
                // Primi 30 ms della nota: la nota si sceglie di nuovo a ogni
                // analisi (le prime letture sono disturbate dall'attacco) e la
                // correzione la raggiunge in ~8 ms. Prima ci saltava sopra di
                // colpo: se nell'attacco la nota scelta cambiava (voce che
                // sale "da sotto"), l'uscita faceva un gradino anche di due
                // semitoni in 5 ms, un piccolo "blip" sulla nota.
                let keep_natural = ((settings.retune_ms - 60.0) / 240.0).clamp(0.0, 1.0);
                self.approach(desired * (1.0 - keep_natural), 8.0);
            } else if starting {
                // Una nota che parte dopo una pausa parte gia' intonata: la
                // velocita' di correzione vale per i passaggi da una nota
                // all'altra. Se la voce partisse stonata e poi "scivolasse"
                // sulla nota, si sentirebbe la correzione arrivare in ritardo.
                // Solo con velocita' molto lente (stile naturale) si lascia un
                // po' dell'attacco originale.
                let keep_natural = ((settings.retune_ms - 60.0) / 240.0).clamp(0.0, 1.0);
                self.correction = desired * (1.0 - keep_natural);
                self.correction_mid = self.correction;
            }

            // Humanize: sulle note tenute (oltre ~0,15 s) la velocita' rallenta,
            // in proporzione al quadrato della manopola: al 25% appena (+12 ms),
            // al 50% +50 ms, al 100% +200 ms. (Prima arrivava a +400 ms in modo
            // lineare: anche un 20% toglieva quasi tutta la correzione.)
            let sustain = smoothstep(0.15, 0.6, self.note_age);
            let humanize = settings.humanize.clamp(0.0, 1.0);
            let retune_ms = settings.retune_ms + humanize * humanize * 200.0 * sustain;
            // Correzione veloce = voce "inchiodata" alla nota anche nei passaggi
            // rapidi (misura ciclo per ciclo); correzione lenta = la voce resta libera.
            // Humanize allenta l'aggancio solo un po' (il centro della nota resta preciso).
            let lock_ms = settings.retune_ms + 0.5 * (retune_ms - settings.retune_ms);
            lock = weight * (1.0 - lock_ms / 80.0).clamp(0.0, 1.0);
            if retune_ms < 1.0 {
                self.correction = desired;
                self.correction_mid = desired;
            } else {
                self.approach(desired, retune_ms);
            }

            // Vibrato naturale: lo isoliamo con il passa-banda e lo aggiungiamo
            // (o togliamo), entrando dolcemente dopo l'attacco della nota.
            let vibrato_in = self.vibrato_in.process(note - reference);
            let vibrato_out = self.vibrato_out.process(note + self.correction - reference);
            let amount = settings.vibrato.clamp(-1.0, 1.0);
            let vibrato_shift = if amount >= 0.0 { amount * vibrato_in } else { amount * vibrato_out }
                * smoothstep(0.08, 0.3, self.note_age);

            let shift = self.correction + vibrato_shift + settings.transpose;
            ratio = 2f32.powf(shift / 12.0);
            period = estimate.period;
            // Il periodo trovato descrive il tratto [inizio, inizio + finestra + periodo].
            center = start as f64 + (window + period as f64) * 0.5;

            voiced = true;
            self.was_voiced = true;
            self.hold_left = self.hold_samples;
            self.voiced_run = self.voiced_run.saturating_add(1);
            self.last_period = period;
            report = PitchReport {
                voiced: true,
                input_note: note,
                output_note: note + shift,
                target_note: target.map_or(f32::NAN, |t| t as f32 + settings.transpose),
            };
        } else if self.was_voiced && (self.hold_left > 0 || suspicious_leap) && self.voiced_run >= self.min_run_to_hold {
            // (Una nota letta per una o due analisi soltanto e' quasi sempre
            // un "lampo" del rilevatore su una consonante: non la si tiene,
            // altrimenti il suo periodo, magari sbagliato di un'ottava,
            // resterebbe per 30 ms.)
            // Buco momentaneo (o salto sospetto): manteniamo l'ultima correzione.
            self.hold_left = self.hold_left.saturating_sub(self.hop);
            self.prev_analysis_voiced = false;
            period = self.last_period;
            ratio = self.last_ratio;
            voiced = true;
            center = start as f64 + len as f64 * 0.5;
            report = PitchReport::SILENT;
        } else {
            self.was_voiced = false;
            self.voiced_run = 0;
            self.snapper.reset();
            self.prev_analysis_voiced = false;
            self.current_target = None;
            self.correction = 0.0;
            self.correction_mid = 0.0;
            period = self.unvoiced_period;
            ratio = 1.0;
            voiced = false;
            center = start as f64 + len as f64 * 0.5;
            report = PitchReport::SILENT;
        }

        self.last_ratio = ratio;
        let held = voiced && report.input_note.is_nan();
        self.history[self.history_head] = Analysis { center, period, ratio, voiced, valid: true, held, lock };
        self.history_head = (self.history_head + 1) % HISTORY;
        if voiced && !held {
            self.drop_isolated_blip();
        }
        report
    }
}

impl PitchCorrector {
    /// Avvicina la correzione a `target` in modo che arrivi al ~95% in `ms`
    /// millisecondi. Sono due filtri "a un polo" in fila: il passaggio da una
    /// nota all'altra parte morbido e arriva morbido (una curva a S). Con un
    /// filtro solo la correzione partiva di scatto (meta' del salto nei primi
    /// 5 ms): un angolo nell'intonazione, che si sente come una piccola
    /// risonanza all'inizio di ogni cambio di nota.
    fn approach(&mut self, target: f32, ms: f32) {
        let time_constant = ms * 0.001 / 4.74 * self.sample_rate;
        let alpha = 1.0 - (-(self.hop as f32) / time_constant).exp();
        self.correction_mid += alpha * (target - self.correction_mid);
        self.correction += alpha * (self.correction_mid - self.correction);
    }

    /// "Lampi" del rilevatore: a cavallo di una consonante o all'attacco di
    /// una parola capita che una o due analisi leggano la nota un'ottava (o
    /// piu') sopra o sotto, isolate tra due pause. Con quel periodo sbagliato
    /// i grani prenderebbero mezzo ciclo o due cicli alla volta: un "gracchio"
    /// di pochi millisecondi proprio sull'attacco. Lo shifter lavora in
    /// ritardo (la latenza), quindi quando arriva un'analisi buona possiamo
    /// ancora correggere quelle appena passate: un tratto cantato di al massimo
    /// 10 ms, lontano piu' di 7 semitoni dalla nota che segue e preceduto da
    /// una pausa (o dalla stessa nota che segue), diventa "non nota" e passa
    /// intatto.
    fn drop_isolated_blip(&mut self) {
        const JUMP: f32 = 1.498; // 7 semitoni
        let max_gap = 0.04 * self.sample_rate as f64;
        let index = |back: usize| (self.history_head + 2 * HISTORY - 1 - back) % HISTORY;
        let new = self.history[index(0)];
        let far = |a: f32, b: f32| a > b * JUMP || b > a * JUMP;

        // L'ultima analisi cantata "vera" prima di questa, non troppo lontana.
        let mut back = 1;
        while back < HISTORY {
            let a = self.history[index(back)];
            if !a.valid || new.center - a.center > max_gap {
                return;
            }
            if a.voiced && !a.held {
                break;
            }
            back += 1;
        }
        if back >= HISTORY || !far(self.history[index(back)].period, new.period) {
            return;
        }
        // Il tratto a cui appartiene: analisi vere di periodo simile (con
        // eventuali buchi tenuti in mezzo), al massimo due.
        let blip_period = self.history[index(back)].period;
        let first = back;
        let mut count = 0;
        let mut end = back;
        while end < HISTORY {
            let a = self.history[index(end)];
            if !a.valid || !a.voiced {
                break;
            }
            if !a.held {
                if far(a.period, blip_period) {
                    // Prima del lampo c'era un'altra nota: va bene solo se e'
                    // la stessa che segue (A - lampo - A).
                    if far(a.period, new.period) {
                        return;
                    }
                    break;
                }
                count += 1;
                if count > 2 {
                    return;
                }
            }
            end += 1;
        }
        for back in first..end {
            let a = &mut self.history[index(back)];
            a.voiced = false;
            a.ratio = 1.0;
            a.lock = 0.0;
        }
        // Anche i buchi tenuti tra il lampo e questa analisi avevano il suo periodo.
        for back in 1..first {
            let a = &mut self.history[index(back)];
            if a.held {
                a.voiced = false;
                a.ratio = 1.0;
                a.lock = 0.0;
            }
        }
    }
}

/// Il pitch nel punto `t` dell'ingresso, interpolando tra le due analisi
/// piu' vicine (una prima e una dopo `t`).
fn lookup(history: &[Analysis; HISTORY], t: f64, edge: f64) -> Option<(GrainPitch, f32)> {
    let mut before: Option<&Analysis> = None;
    let mut after: Option<&Analysis> = None;
    for a in history.iter().filter(|a| a.valid) {
        if a.center <= t {
            if before.is_none_or(|b| a.center > b.center) {
                before = Some(a);
            }
        } else if after.is_none_or(|b| a.center < b.center) {
            after = Some(a);
        }
    }
    let as_pitch = |a: &Analysis| {
        (GrainPitch { period: a.period, ratio: a.ratio, voiced: a.voiced, formant: 1.0 }, if a.voiced { a.lock } else { 0.0 })
    };
    match (before, after) {
        (Some(b), Some(a)) if b.voiced && a.voiced => {
            let w = ((t - b.center) / (a.center - b.center).max(1e-9)) as f32;
            Some((
                GrainPitch {
                    period: b.period + (a.period - b.period) * w,
                    ratio: b.ratio + (a.ratio - b.ratio) * w,
                    voiced: true,
                    formant: 1.0,
                },
                b.lock + (a.lock - b.lock) * w,
            ))
        }
        // Al confine tra voce e consonante/pausa: l'analisi guarda ~20 ms di
        // audio e il suo "centro" cade a meta', quindi la voce inizia (e
        // finisce) qualche millisecondo prima (dopo) del centro della prima
        // (ultima) analisi cantata. Quei millisecondi vanno gia' corretti come
        // voce, altrimenti l'attacco della nota esce non corretto e "gratta".
        (Some(_), Some(a)) if a.voiced && a.center - t <= edge => Some(as_pitch(a)),
        (Some(b), Some(_)) if b.voiced && t - b.center <= edge => Some(as_pitch(b)),
        (Some(b), Some(a)) => Some(as_pitch(if t - b.center <= a.center - t { b } else { a })),
        (Some(only), None) | (None, Some(only)) => Some(as_pitch(only)),
        (None, None) => None,
    }
}

/// `true` se attorno al punto `t` c'e' un attacco (o una chiusura) brusca:
/// l'energia nel periodo dopo `t` e' oltre 8 volte (9 dB) quella del periodo
/// prima, o viceversa. Non alloca memoria.
fn is_transient(buf: &[f32], mask: usize, newest: u64, t: f64, period: f32) -> bool {
    let p = period.round().max(16.0) as i64;
    let c = t.round() as i64;
    if c - p < 0 || c + p > newest as i64 || (newest as i64 - (c - p)) as usize >= buf.len() {
        return false;
    }
    let energy = |from: i64| -> f32 { (from..from + p).map(|i| buf[i as usize & mask].powi(2)).sum() };
    let before = energy(c - p) + 1e-9;
    let after = energy(c) + 1e-9;
    after > 8.0 * before || before > 8.0 * after
}

/// Sotto questa somiglianza tra un ciclo e il successivo il suono e' rumore
/// (consonante, respiro): il frame non si sposta.
const NOISE_SCORE: f32 = 0.4;
/// Sopra questa e' voce piena: correzione intera.
const VOICE_SCORE: f32 = 0.7;

/// Il periodo esatto della voce attorno al punto `t` del segnale, partendo
/// da una stima `p0` (dalle analisi), e quanto il suono li' e' periodico:
/// confronta un tratto di ~1,5 periodi con lo stesso tratto spostato di un
/// periodo, centrati su `t`, e cerca lo spostamento che li fa coincidere
/// meglio (entro ±3 semitoni da `p0`). Restituisce (periodo, somiglianza da
/// -1 a 1); se il picco cade sul bordo della ricerca il periodo resta `p0`.
/// `None` se mancano i campioni. Non alloca memoria.
fn periodicity(buf: &[f32], mask: usize, newest: u64, t: f64, p0: f32) -> Option<(f32, f32)> {
    if !(16.0..=4096.0).contains(&p0) {
        return None;
    }
    let len = ((1.5 * p0) as i64).max(32);
    let lo = (p0 * 0.84).floor() as i64;
    let hi = (p0 * 1.19).ceil() as i64;
    let center = t.round() as i64;
    let first = center - hi / 2 - len / 2 - 2;
    let last = center + hi / 2 + len / 2 + 2;
    if first < 0 || last > newest as i64 || (newest as i64 - first) as usize >= buf.len() {
        return None;
    }
    let at = |i: i64| buf[i as usize & mask];
    let score = |tau: i64, step: i64| -> f32 {
        let a0 = center - tau / 2 - len / 2;
        let b0 = a0 + tau;
        let (mut ab, mut aa, mut bb) = (0.0f32, 0.0f32, 0.0f32);
        let mut i = 0;
        while i < len {
            let a = at(a0 + i);
            let b = at(b0 + i);
            ab += a * b;
            aa += a * a;
            bb += b * b;
            i += step;
        }
        if aa <= 1e-12 || bb <= 1e-12 {
            return -1.0;
        }
        ab / (aa * bb).sqrt()
    };
    // Ricerca grossolana (un passo su due per i periodi lunghi), poi fine.
    let coarse = if p0 > 64.0 { 2 } else { 1 };
    let (mut best, mut best_score) = (lo, f32::MIN);
    let mut tau = lo;
    while tau <= hi {
        let s = score(tau, coarse);
        if s > best_score {
            best_score = s;
            best = tau;
        }
        tau += coarse;
    }
    let (mut fine, mut fine_score) = (best, f32::MIN);
    for tau in (best - 2).max(lo)..=(best + 2).min(hi) {
        let s = score(tau, 1);
        if s > fine_score {
            fine_score = s;
            fine = tau;
        }
    }
    // Serve un picco vero, non il bordo dell'intervallo di ricerca.
    if fine <= lo || fine >= hi {
        return Some((p0, fine_score));
    }
    let (minus, plus) = (score(fine - 1, 1), score(fine + 1, 1));
    let den = minus - 2.0 * fine_score + plus;
    let offset = if den < 0.0 { (0.5 * (minus - plus) / den).clamp(-0.5, 0.5) } else { 0.0 };
    Some((fine as f32 + offset, fine_score))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scale::{note_to_hz, scale_notes, ScaleKind};
    use crate::synth;
    use crate::yin::YinDetector;

    fn process_mono(corrector: &mut PitchCorrector, input: &[f32], settings: &CorrectionSettings) -> Vec<f32> {
        input
            .iter()
            .map(|&x| {
                let mut f = [x];
                corrector.process_frame(&mut f, settings);
                f[0]
            })
            .collect()
    }

    fn final_note(signal: &[f32], sr: f32) -> f32 {
        let mut yin = YinDetector::new(sr, 60.0);
        let len = yin.frame_len();
        let est = yin.detect(&signal[signal.len() - len..]);
        assert!(est.voiced, "nessuna nota in uscita");
        hz_to_note(sr / est.period)
    }

    #[test]
    fn corrects_sharp_note_to_scale() {
        let sr = 48000.0;
        let settings = CorrectionSettings {
            notes: scale_notes(0, ScaleKind::Major),
            retune_ms: 0.0,
            ..Default::default()
        };
        // La crescente di 38 centesimi → deve diventare un La perfetto (69).
        for &(sung, expected) in &[(69.38f32, 69.0f32), (64.7, 65.0), (57.6, 57.0), (70.3, 71.0)] {
            let input = synth::steady_vowel(note_to_hz(sung), sr, 24000);
            let mut corrector = PitchCorrector::new(sr, 1);
            let out = process_mono(&mut corrector, &input, &settings);
            let got = final_note(&out, sr);
            assert!(
                (got - expected).abs() < 0.05,
                "cantata {sung} → uscita {got:.3} (attesa {expected})"
            );
        }
    }

    #[test]
    fn amount_zero_leaves_pitch_alone() {
        let sr = 44100.0;
        let settings = CorrectionSettings { amount: 0.0, ..Default::default() };
        let input = synth::steady_vowel(note_to_hz(62.3), sr, 22050);
        let mut corrector = PitchCorrector::new(sr, 1);
        let out = process_mono(&mut corrector, &input, &settings);
        assert!((final_note(&out, sr) - 62.3).abs() < 0.05);
    }

    #[test]
    fn transpose_moves_by_semitones() {
        let sr = 48000.0;
        let settings = CorrectionSettings { transpose: 5.0, retune_ms: 0.0, ..Default::default() };
        let input = synth::steady_vowel(note_to_hz(57.0), sr, 24000);
        let mut corrector = PitchCorrector::new(sr, 1);
        let out = process_mono(&mut corrector, &input, &settings);
        assert!((final_note(&out, sr) - 62.0).abs() < 0.05);
    }

    #[test]
    fn slow_retune_reaches_target_gradually() {
        let sr = 48000.0;
        let settings = CorrectionSettings { retune_ms: 300.0, ..Default::default() };
        let input = synth::steady_vowel(note_to_hz(60.4), sr, 48000);
        let mut corrector = PitchCorrector::new(sr, 1);
        let mut reports = Vec::new();
        for &x in &input {
            let mut f = [x];
            if let Some(r) = corrector.process_frame(&mut f, &settings) {
                if r.voiced {
                    reports.push(r.output_note);
                }
            }
        }
        // All'inizio quasi nessuna correzione, alla fine quasi completa.
        assert!((reports[1] - 60.4).abs() < 0.1, "inizio: {}", reports[1]);
        assert!((reports.last().unwrap() - 60.0).abs() < 0.02, "fine: {}", reports.last().unwrap());
    }

    #[test]
    fn silence_stays_silent() {
        let mut corrector = PitchCorrector::new(48000.0, 2);
        let settings = CorrectionSettings::default();
        for _ in 0..48000 {
            let mut f = [0.0, 0.0];
            corrector.process_frame(&mut f, &settings);
            assert_eq!(f, [0.0, 0.0]);
        }
    }

    #[test]
    fn latency_low_is_zero_and_hq_is_fixed() {
        let mut c = PitchCorrector::new(48000.0, 1);
        for voice in [VoiceType::Soprano, VoiceType::AltoTenor, VoiceType::LowMale] {
            c.set_voice(voice);
            c.set_quality(false);
            assert_eq!(c.latency(), 0, "Low Latency: nessun ritardo");
            c.set_quality(true);
            // HQ: 64 ms (la finestra piu' lunga del phase vocoder), uguale per tutte le voci.
            assert_eq!(c.latency(), 3072);
        }
    }


    /// Una voce che scivola velocemente (3 oscillazioni al secondo di ±2,5
    /// semitoni) con correzione istantanea deve uscire "a scalini" sulle note
    /// della scala, senza che il movimento originale trapeli.
    #[test]
    fn fast_glides_become_steps() {
        let sr = 48000.0;
        let len = (sr * 2.0) as usize;
        let freqs: Vec<f32> = (0..len)
            .map(|i| {
                let t = i as f32 / sr;
                note_to_hz(49.0 + 2.5 * (2.0 * std::f32::consts::PI * 3.0 * t).sin())
            })
            .collect();
        let input = synth::sing_curve(&freqs, &vec![1.0; len], sr);
        let settings = CorrectionSettings {
            notes: scale_notes(0, ScaleKind::Major),
            retune_ms: 0.0,
            ..Default::default()
        };
        // Glissati velocissimi (47 semitoni al secondo): si misura in HQ,
        // dove ogni pezzo di voce usa l'analisi del punto esatto.
        let mut corrector = PitchCorrector::new(sr, 1);
        corrector.set_quality(true);
        let out = process_mono(&mut corrector, &input, &settings);

        // Misuriamo l'intonazione in uscita su finestre corte.
        let mut yin = YinDetector::new(sr, 80.0);
        let frame = yin.frame_len();
        let (mut total, mut on_note) = (0, 0);
        for start in (sr as usize / 2..len - frame).step_by(frame / 2) {
            let est = yin.detect(&out[start..start + frame]);
            if est.voiced {
                let note = hz_to_note(sr / est.period);
                total += 1;
                if (note - note.round()).abs() < 0.12 && settings.notes[crate::scale::pitch_class(note.round() as i32)] {
                    on_note += 1;
                }
            }
        }
        let share = on_note as f32 / total as f32;
        // (Con le note riconosciute fino a 75 Hz, per le voci maschili basse,
        // su questi glissati estremi si scende da ~88% a ~83%.)
        assert!(share > 0.8, "solo il {:.0}% delle finestre e' su una nota della scala", share * 100.0);
    }

    /// Un vero salto d'ottava deve essere seguito (dopo pochi millisecondi).
    #[test]
    fn real_octave_leaps_are_followed() {
        let sr = 48000.0;
        let melody = [
            synth::SungNote { note: 57.0, detune_cents: 20.0, seconds: 0.4 },
            synth::SungNote { note: 69.0, detune_cents: -20.0, seconds: 0.4 },
        ];
        let (input, _) = synth::sing_melody(&melody, sr, 0.0);
        let settings = CorrectionSettings { retune_ms: 0.0, ..Default::default() };
        let mut corrector = PitchCorrector::new(sr, 1);
        let mut last = None;
        for &x in &input {
            let mut f = [x];
            if let Some(r) = corrector.process_frame(&mut f, &settings) {
                if r.voiced {
                    last = Some(r);
                }
            }
        }
        assert!((last.unwrap().output_note - 69.0).abs() < 0.02);
    }

    /// Una melodia vera, stonata, con vibrato: ogni nota corretta deve finire
    /// sulla nota giusta della scala.
    #[test]
    fn corrects_a_whole_melody() {
        let sr = 44100.0;
        let melody = [
            synth::SungNote { note: 60.0, detune_cents: 35.0, seconds: 0.5 },
            synth::SungNote { note: 62.0, detune_cents: -40.0, seconds: 0.5 },
            synth::SungNote { note: 64.0, detune_cents: 30.0, seconds: 0.5 },
            synth::SungNote { note: 67.0, detune_cents: -35.0, seconds: 0.7 },
        ];
        let (input, _) = synth::sing_melody(&melody, sr, 0.0);
        let settings = CorrectionSettings {
            notes: scale_notes(0, ScaleKind::Major),
            retune_ms: 0.0,
            ..Default::default()
        };
        let mut corrector = PitchCorrector::new(sr, 1);
        let mut outputs = Vec::new();
        for &x in &input {
            let mut f = [x];
            if let Some(r) = corrector.process_frame(&mut f, &settings) {
                outputs.push(r);
            }
        }
        // Misuriamo a meta' di ogni nota (lontano dagli scivolamenti).
        let hop = corrector.analysis_interval();
        let mut t = 0.0;
        for n in &melody {
            let middle = ((t + n.seconds * 0.6) / hop) as usize;
            let r = outputs[middle];
            assert!(r.voiced);
            assert!((r.output_note - n.note).abs() < 0.02, "nota {} → {}", n.note, r.output_note);
            t += n.seconds;
        }
    }
}
