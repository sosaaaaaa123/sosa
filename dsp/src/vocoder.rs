//! Pitch shifting con un **phase vocoder** "a picchi bloccati" e formanti
//! preservate: il motore della modalita' HQ.
//!
//! Il phase vocoder lavora nel dominio delle frequenze:
//!
//! 1. ogni 5 ms prende un pezzo di voce lungo circa 6 periodi (32-64 ms:
//!    piu' lungo per le voci maschili, che hanno armoniche fitte, piu' corto
//!    per quelle femminili), lo smussa con una finestra di Hann e ne calcola
//!    lo spettro (FFT): un insieme di "righe", una per ogni armonica;
//! 2. trova i **picchi** dello spettro (le armoniche) e la frequenza esatta
//!    di ognuno, misurando di quanto e' girata la sua fase dall'ultima volta;
//! 3. sposta ogni picco, insieme alle righe vicine che gli appartengono
//!    (la sua "zona"), alla frequenza nuova: frequenza x rapporto;
//! 4. la fase di ogni picco spostato continua a girare alla velocita' giusta
//!    per la frequenza nuova, e le righe della sua zona la seguono rigidamente
//!    ("identity phase locking", Laroche e Dolson 1999): cosi' ogni armonica
//!    resta un'onda pulita, senza l'effetto "metallico" o "in una stanza"
//!    dei phase vocoder semplici. In piu' le armoniche (fino a 16 kHz)
//!    restano in fase tra loro: l'armonica h gira di h volte la fondamentale,
//!    e la forma d'onda di ogni ciclo della voce si conserva;
//! 5. il **timbro** (l'inviluppo spettrale, cioe' le formanti) viene misurato
//!    con il "true envelope" (cepstrum ripetuto che segue le cime delle
//!    armoniche) e rimesso al suo posto dopo lo spostamento: la voce cambia
//!    nota ma non diventa "da scoiattolo" o "da orco". Con il comando Formant le
//!    formanti si spostano apposta (vedi `formant_shift`);
//! 6. torna nel tempo (FFT inversa) e somma i pezzi sovrapposti, tenendo
//!    di ogni pezzo solo i 16 ms centrali (finestra di sintesi corta): nei
//!    passaggi tra le note non si mescolano pezzi spostati in modo diverso.
//!
//! Dove non c'e' una nota (consonanti, respiri) e sugli attacchi bruschi lo
//! spettro passa **identico**: la ricostruzione e' perfetta, niente sbavature
//! sulle "t" e sulle "s".
//!
//! La latenza e' la finestra piu' lunga (64 ms), compensata dal DAW.

use crate::fft::{Complex, Fft};
use crate::MAX_CHANNELS;
use std::f32::consts::{LN_10, PI};

const TAU: f32 = 2.0 * PI;

/// Cosa serve sapere per spostare l'intonazione in un certo punto della voce.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrainPitch {
    /// Periodo della voce in quel punto (campioni): serve a separare le
    /// formanti dalle armoniche.
    pub period: f32,
    /// Di quanto moltiplicare la frequenza (1.0 = invariata, 2.0 = un'ottava sopra).
    pub ratio: f32,
    /// `true` se li' c'e' una nota cantata, `false` per consonanti, respiri e
    /// rumori (lo spettro passa identico).
    pub voiced: bool,
    /// Spostamento delle formanti (il "timbro", la lunghezza della gola):
    /// 1.0 = invariato, >1 voce piu' "piccola", <1 voce piu' "grande".
    pub formant: f32,
}

/// Riporta un angolo in (-π, π].
#[inline]
fn wrap(x: f32) -> f32 {
    x - TAU * (x / TAU).round()
}

/// Stato spettrale di un canale.
struct ChannelState {
    /// Ultimi campioni ricevuti (buffer circolare di `n_fft`).
    input: Vec<f32>,
    /// Uscita in costruzione (buffer circolare di 2·n_fft): ogni frame vi somma il suo pezzo.
    accum: Vec<f32>,
    /// Fase di ogni riga nel frame precedente (per la frequenza esatta dei picchi).
    prev_phase: Vec<f32>,
    /// Rotazione di fase applicata a ogni riga (quella del picco a cui
    /// appartiene) nel frame precedente e in quello attuale.
    prev_rotation: Vec<f32>,
    rotation: Vec<f32>,
    /// `false` finche' non c'e' un frame precedente valido.
    has_prev: bool,
    /// Energia media recente (per riconoscere gli attacchi).
    energy: f32,
    /// Rotazione accumulata della fondamentale: l'armonica `h` gira di
    /// `h` volte questo angolo (vedi `HARMONIC_LOCK_HZ`).
    theta: f32,
    /// Inviluppo (logaritmico) lisciato nel tempo, usato solo per spostare
    /// le formanti (comando Formant), e da quanti frame non lo si aggiorna.
    formant_env: Vec<f32>,
    formant_age: usize,
    /// Brillantezza (vedi `spectral_tilt`) delle ultime note, lisciata.
    formant_tilt: f32,
    /// Compenso di volume del comando Formant (logaritmico), lisciato.
    formant_comp: f32,
}

pub struct PhaseVocoder {
    channels: usize,
    /// Dimensione della FFT (potenza di 2) e meta' (+1 = righe utili).
    n_fft: usize,
    half: usize,
    hop: usize,
    /// Finestra di analisi piu' corta e piu' lunga (campioni, pari).
    min_window: usize,
    max_window: usize,
    /// Finestra attuale: segue la voce (circa 6 periodi), con dolcezza.
    window_len: f32,
    /// Periodi della voce dentro una finestra.
    periods_per_window: f32,
    fft: Fft,
    /// FFT piccola per l'inviluppo spettrale (calcolato su righe raggruppate a 4 a 4).
    env_fft: Fft,
    n_env: usize,
    position: u64,
    countdown: usize,
    states: Vec<ChannelState>,
    /// Righe FFT sotto `HARMONIC_LOCK_HZ`.
    harmonic_lock_bins: f32,
    /// Lunghezza della finestra di sintesi (campioni, pari).
    synth_len: usize,
    /// Somma delle finestre (analisi x sintesi) caduta su ogni campione in
    /// uscita: dividendo per lei la ricostruzione e' esatta qualunque sia la
    /// lunghezza delle finestre.
    norm: Vec<f32>,

    // Memoria di lavoro (allocata una volta sola).
    window: Vec<f32>,
    synth_window: Vec<f32>,
    spectrum: Vec<Complex>,
    out_spectrum: Vec<Complex>,
    env_buf: Vec<Complex>,
    env_log: Vec<f32>,
    env_work: Vec<f32>,
    magnitude: Vec<f32>,
    phase: Vec<f32>,
    log_env: Vec<f32>,
    /// Formant: guadagno (logaritmico) per ogni riga in uscita.
    formant_gain: Vec<f32>,
    /// Low Latency: guadagno del timbro per la testina (vedi `timbre_gain`).
    timbre: Vec<f32>,
    /// Formant: quanto l'inviluppo lisciato segue quello nuovo a ogni frame
    /// (per ogni riga) e quanto pesa lo spostamento (1 fino a 7 kHz, poi 0).
    formant_follow: Vec<f32>,
    formant_weight: Vec<f32>,
    /// Formant: dopo quanti frame senza nota l'inviluppo lisciato riparte da capo.
    formant_hold: usize,
    /// Formant: righe (da, a) su cui si misura il livello medio dell'inviluppo.
    formant_level_bins: (usize, usize),
    /// Formant nei frame senza nota: fino a quanti frame dall'ultima nota e'
    /// pieno e dopo quanti e' sparito (vedi `FORMANT_BRIDGE_S`).
    formant_bridge: (f32, f32),
    /// Righe 200 Hz, 4 kHz e 16 kHz (per `spectral_tilt`).
    tilt_bins: (usize, usize, usize),
    /// Formant: dove agisce il compenso di volume (il corpo della voce: 1
    /// fino a 3 kHz, 0 da 4,5 kHz), peso di ogni riga nel volume percepito
    /// (curva K della misura LUFS, solo nel corpo) e righe su cui si misura.
    body_weight: Vec<f32>,
    loudness_weight: Vec<f32>,
    loudness_bins: (usize, usize),
    /// Formant: quanto il compenso di volume segue quello nuovo a ogni frame.
    formant_comp_follow: f32,
    peaks: Vec<usize>,
    peak_bin: Vec<f32>,
    frame_out: Vec<f32>,
}

impl PhaseVocoder {
    /// Finestra piu' lunga (s): 64 ms, abbastanza per separare bene le
    /// armoniche anche di un basso (80 Hz). E' anche la latenza.
    const MAX_WINDOW_S: f32 = 0.064;
    /// Finestra piu' corta (s): per le voci acute, che hanno armoniche
    /// lontane tra loro e cambiano in fretta.
    const MIN_WINDOW_S: f32 = 0.032;
    /// Un frame ogni ~5 ms.
    const HOP_S: f32 = 0.0053;
    /// Quanti periodi della voce stanno in una finestra.
    const PERIODS_PER_WINDOW: f32 = 6.0;
    /// Finestra di sintesi: 16 ms al centro della finestra di analisi. Ogni
    /// pezzo in uscita contiene cosi' solo la voce vicina al suo centro,
    /// spostata con il rapporto giusto per quel punto. Con la finestra lunga
    /// (fino a 64 ms) nei passaggi tra le note si sommavano pezzi spostati con
    /// rapporti diversi: tante copie appena stonate tra loro, una risonanza
    /// metallica sui medio-alti (misurata: armoniche del basso fino a 1,8 dB
    /// fuori posto nei passaggi, ora 0,8).
    const SYNTH_WINDOW_S: f32 = 0.016;
    /// Sotto questa frequenza le armoniche restano "in fase" tra loro (vedi
    /// `process_frame`): la forma d'onda della voce si conserva come in un
    /// pitch shifter nel tempo. Prima il limite era 5 kHz e sopra ogni
    /// armonica girava per conto suo: gli impulsi della voce (la "chiusura"
    /// delle corde vocali, che da' presenza e aria) si sparpagliavano nella
    /// parte alta, una risonanza "phasey" sui medio-alti e sugli acuti.
    /// Misurato come fattore di cresta della banda 5-8 kHz (impulsi netti =
    /// cresta alta): voce maschile 14,8 dB in ingresso, 10,4 in uscita, ora
    /// 14,9; voci femminili vere da ~0,4 dB sotto l'ingresso a pari. Nessun
    /// ronzio alla velocita' dei frame (misurato sull'inviluppo delle bande
    /// alte attorno a 189 Hz) e intonazione invariata.
    const HARMONIC_LOCK_HZ: f32 = 16000.0;
    /// Picchi considerati fino a 100 dB sotto il piu' forte. (Con 70 dB le
    /// armoniche deboli in alto restavano fuori e venivano spostate tutte
    /// insieme all'ultima armonica trovata, fuori posto: un fruscio
    /// metallico sopra i 5 kHz.)
    const PEAK_FLOOR: f32 = 1e-5;
    /// Iterazioni del "true envelope" (inviluppo che passa sopra le armoniche).
    const ENVELOPE_ITERATIONS: usize = 4;
    /// Ad ogni iterazione l'inviluppo sale del doppio di quanto le armoniche
    /// gli passano sopra: arriva sulle cime in meno passaggi (con 4 iterazioni
    /// e' preciso come con 8 senza accelerazione).
    const ENVELOPE_ACCEL: f32 = 2.0;
    /// Comando Formant (spostamento delle formanti): lisciatura nel tempo
    /// dell'inviluppo da spostare. La misura del timbro su una voce vera
    /// cambia di circa 1 dB da un frame all'altro (5 ms) anche su una nota
    /// ferma: e' rumore di misura, non la bocca che si muove. Spostata cosi'
    /// com'era, ogni frequenza riceveva il tremolio di un'altra: voce
    /// "granulosa", soprattutto spostando di molti semitoni (tremolio misurato
    /// sulle bande da ~0,52 a ~0,23 dB tra un frame e l'altro). Sotto 1,5 kHz
    /// (prima e seconda formante, quelle delle vocali) si segue in fretta
    /// per non impastare le parole; sopra 4 kHz (la "gola", che cambia poco)
    /// si liscia di piu'.
    const FORMANT_SMOOTH_LOW_S: f32 = 0.020;
    const FORMANT_SMOOTH_HIGH_S: f32 = 0.100;
    const FORMANT_SMOOTH_FROM_HZ: f32 = 1500.0;
    const FORMANT_SMOOTH_TO_HZ: f32 = 4000.0;
    /// Formant: sopra 7 kHz lo spostamento sfuma fino a sparire a 14 kHz. La'
    /// non ci sono formanti ma l'aria e le "s": spostandole in alto
    /// diventavano un fruscio forte (+10-15 dB sopra i 16 kHz), in basso
    /// la voce perdeva brillantezza.
    const FORMANT_FADE_HZ: f32 = 7000.0;
    /// Formant: le spinte verso l'alto si ammorbidiscono (al massimo ~20 dB,
    /// 10 dB diventano 9). Riempire una valle tra due formanti alza anche il
    /// rumore che c'era dentro: spostando molto, quei punti diventavano ruvidi.
    const FORMANT_MAX_BOOST: f32 = 20.0 / 8.685_89;
    /// Formant: in un buco senza nota piu' corto di cosi' l'inviluppo lisciato
    /// resta valido (la voce riconosciuta a tratti non lo fa ripartire ogni volta).
    const FORMANT_HOLD_S: f32 = 0.050;
    /// Formant nei frame che passano senza nota: consonanti, attacchi e i
    /// punti in cui la nota si perde per un attimo in mezzo al canto (sulla
    /// voce di prova ~100 frame isolati con la voce forte). Prima li' il timbro
    /// tornava quello originale per 10-15 ms e poi di nuovo spostato: con
    /// Formant lontano da 0 un "battito" del timbro. Ora quei frame ricevono
    /// lo stesso spostamento (solo il timbro: la nota passa intatta come
    /// prima), pieno per 16 ms e poi a sfumare fino a sparire a 64 ms: le
    /// consonanti lunghe, i respiri e le pause restano come sono.
    const FORMANT_BRIDGE_S: f32 = 0.016;
    const FORMANT_RELEASE_S: f32 = 0.064;
    /// ...ma solo se il frame e' ancora la stessa voce: se ha molti piu'
    /// acuti della nota di prima (una "s", una "f", un "ch" che comincia:
    /// sulla voce di prova +9 dB gia' dal primo frame, contro ~+1 dB dei buchi
    /// in mezzo alla nota) resta intatto come prima. Pieno fino a +3 dB, zero da +8 dB.
    const FORMANT_BRIGHTER_DB: (f32, f32) = (3.0, 8.0);
    /// Formant: la voce non perde volume. Con le formanti in basso (voce piu'
    /// grande) il corpo della voce si abbassava (misurato su voci vere: -2/-3
    /// LU a -6 semitoni) e le "s" e le "z" spiccavano di piu' (+2 dB rispetto
    /// alla voce). Ora quel calo si recupera, solo nel corpo della voce (fino
    /// a 3 kHz, sfumando a 4,5 kHz): gli acuti, le "s" e le "z" restano come
    /// prima. Il compenso si misura sulle vocali (non sui frame molto piu'
    /// brillanti, una "z" o il respiro), e' lisciato su 60 ms e non abbassa
    /// mai la voce: con le formanti in alto (che alzavano un po' il volume)
    /// il suono resta esattamente quello di prima. (Un compenso in entrambe
    /// le direzioni e su tutte le frequenze rendeva le "s" e le "z" piu'
    /// evidenti: con Formant in alto la voce scendeva sotto le "s", con
    /// Formant in basso le "z" e l'aria salivano di ~1,3 dB.)
    const FORMANT_LOUDNESS_S: f32 = 0.060;

    /// Il corpo della voce, dove agisce il compenso di volume di Formant.
    fn body(hz: f32) -> f32 {
        let x = ((hz - 3000.0) / 1500.0).clamp(0.0, 1.0);
        0.5 + 0.5 * (PI * x).cos()
    }

    pub fn new(channels: usize, sample_rate: f32) -> Self {
        Self::with_windows(channels, sample_rate, Self::MIN_WINDOW_S, Self::MAX_WINDOW_S)
    }

    /// Come [`new`](Self::new) con limiti diversi per la finestra (s).
    pub fn with_windows(channels: usize, sample_rate: f32, min_s: f32, max_s: f32) -> Self {
        let channels = channels.clamp(1, MAX_CHANNELS);
        let even = |s: f32| (((sample_rate * s) / 2.0).round() as usize * 2).max(64);
        let max_window = even(max_s);
        let min_window = even(min_s).min(max_window);
        let n_fft = max_window.next_power_of_two();
        let half = n_fft / 2;
        let hop = ((sample_rate * Self::HOP_S).round() as usize).clamp(16, min_window / 4);
        let n_env = (n_fft / 4).max(64);
        let states = (0..channels)
            .map(|_| ChannelState {
                input: vec![0.0; n_fft],
                accum: vec![0.0; 2 * n_fft],
                prev_phase: vec![0.0; half + 1],
                prev_rotation: vec![0.0; half + 1],
                rotation: vec![0.0; half + 1],
                has_prev: false,
                energy: 0.0,
                theta: 0.0,
                formant_env: vec![0.0; half + 1],
                formant_age: usize::MAX,
                formant_tilt: 0.0,
                formant_comp: 0.0,
            })
            .collect();
        let follow = |seconds: f32| 1.0 - (-(hop as f32) / (seconds * sample_rate)).exp();
        let bin_hz = sample_rate / n_fft as f32;
        let formant_follow = (0..=half)
            .map(|k| {
                let x = ((k as f32 * bin_hz - Self::FORMANT_SMOOTH_FROM_HZ)
                    / (Self::FORMANT_SMOOTH_TO_HZ - Self::FORMANT_SMOOTH_FROM_HZ))
                    .clamp(0.0, 1.0);
                let x = x * x * (3.0 - 2.0 * x);
                follow(Self::FORMANT_SMOOTH_LOW_S + (Self::FORMANT_SMOOTH_HIGH_S - Self::FORMANT_SMOOTH_LOW_S) * x)
            })
            .collect();
        let formant_weight = (0..=half)
            .map(|k| {
                let octaves = ((k as f32 * bin_hz).max(1.0) / Self::FORMANT_FADE_HZ).log2().clamp(0.0, 1.0);
                0.5 + 0.5 * (PI * octaves).cos()
            })
            .collect();
        Self {
            channels,
            n_fft,
            half,
            hop,
            min_window,
            max_window,
            window_len: max_window as f32,
            periods_per_window: Self::PERIODS_PER_WINDOW,
            harmonic_lock_bins: Self::HARMONIC_LOCK_HZ / sample_rate * n_fft as f32,
            synth_len: even(Self::SYNTH_WINDOW_S).max(3 * hop).min(min_window),
            fft: Fft::new(n_fft),
            env_fft: Fft::new(n_env),
            n_env,
            position: 0,
            countdown: hop,
            states,
            norm: vec![0.0; 2 * n_fft],
            window: vec![0.0; max_window],
            synth_window: vec![0.0; max_window],
            spectrum: vec![Complex::default(); n_fft],
            out_spectrum: vec![Complex::default(); n_fft],
            env_buf: vec![Complex::default(); n_env],
            env_log: vec![0.0; n_env / 2 + 1],
            env_work: vec![0.0; n_env / 2 + 1],
            magnitude: vec![0.0; half + 1],
            phase: vec![0.0; half + 1],
            log_env: vec![0.0; half + 1],
            formant_gain: vec![0.0; half + 1],
            timbre: vec![0.0; half + 1],
            formant_follow,
            formant_weight,
            formant_hold: ((Self::FORMANT_HOLD_S * sample_rate) as usize / hop).max(1),
            formant_level_bins: (
                ((200.0 / bin_hz) as usize).clamp(1, half - 1),
                ((4000.0 / bin_hz) as usize).clamp(2, half),
            ),
            formant_bridge: (
                Self::FORMANT_BRIDGE_S * sample_rate / hop as f32,
                Self::FORMANT_RELEASE_S * sample_rate / hop as f32,
            ),
            tilt_bins: (
                ((200.0 / bin_hz) as usize).clamp(1, half - 2),
                ((4000.0 / bin_hz) as usize).clamp(2, half - 1),
                ((16000.0 / bin_hz) as usize).clamp(3, half),
            ),
            body_weight: (0..=half).map(|k| Self::body(k as f32 * bin_hz)).collect(),
            loudness_weight: (0..=half)
                .map(|k| {
                    // Curva K (ITU-R BS.1770): +4 dB sopra ~1,5 kHz, taglio sotto ~40 Hz.
                    let f = k as f32 * bin_hz;
                    let x = (f / 1500.0) * (f / 1500.0);
                    let f4 = (f / 38.0).powi(4);
                    (1.0 + 1.512 * x) / (1.0 + x) * f4 / (1.0 + f4) * Self::body(f)
                })
                .collect(),
            loudness_bins: (((60.0 / bin_hz) as usize).clamp(1, half), ((4500.0 / bin_hz) as usize).clamp(1, half)),
            formant_comp_follow: 1.0 - (-(hop as f32) / (Self::FORMANT_LOUDNESS_S * sample_rate)).exp(),
            peaks: Vec::with_capacity(half),
            peak_bin: Vec::with_capacity(half),
            frame_out: vec![0.0; max_window],
        }
    }

    /// Ritardo tra ingresso e uscita (campioni): la finestra piu' lunga.
    pub fn latency(&self) -> usize {
        self.max_window
    }

    /// Dimensione della FFT.
    pub fn fft_len(&self) -> usize {
        self.n_fft
    }

    /// Lunghezza attuale della finestra di analisi (campioni).
    pub fn window_len(&self) -> usize {
        self.window_len.round() as usize
    }

    pub fn reset(&mut self) {
        self.position = 0;
        self.countdown = self.hop;
        self.window_len = self.max_window as f32;
        self.norm.fill(0.0);
        for s in &mut self.states {
            s.input.fill(0.0);
            s.accum.fill(0.0);
            s.prev_phase.fill(0.0);
            s.prev_rotation.fill(0.0);
            s.rotation.fill(0.0);
            s.has_prev = false;
            s.energy = 0.0;
            s.theta = 0.0;
            s.formant_age = usize::MAX;
        }
    }

    /// Elabora un campione per canale. `pitch_at(t)` dice rapporto, periodo
    /// e presenza di una nota nel punto `t` dell'ingresso (il centro del frame).
    #[inline]
    pub fn process<F>(&mut self, input: &[f32], output: &mut [f32], mut pitch_at: F)
    where
        F: FnMut(f64) -> GrainPitch,
    {
        let n = self.n_fft;
        let now = self.position;
        for ch in 0..self.channels {
            self.states[ch].input[now as usize & (n - 1)] = input.get(ch).copied().unwrap_or(0.0);
        }

        self.countdown -= 1;
        if self.countdown == 0 {
            self.countdown = self.hop;
            // Centro del frame: a meta' della finestra piu' lunga, dietro
            // l'ultimo campione arrivato.
            let center = now as i64 - (self.max_window / 2) as i64;
            if center >= 0 {
                let pitch = pitch_at(center as f64);
                self.prepare_window(pitch);
                let len = self.window_len();
                let mut same = false;
                for ch in 0..self.channels {
                    if ch == 1 {
                        // Voce mono su traccia stereo (i due canali identici):
                        // si calcola una volta sola e si copia.
                        same = (0..len).all(|i| {
                            let idx = (center as usize + n - len / 2 + i) & (n - 1);
                            self.states[0].input[idx] == self.states[1].input[idx]
                        });
                    }
                    if ch == 1 && same {
                        let (first, rest) = self.states.split_at_mut(1);
                        let (a, b) = (&first[0], &mut rest[0]);
                        b.prev_phase.copy_from_slice(&a.prev_phase);
                        b.prev_rotation.copy_from_slice(&a.prev_rotation);
                        b.rotation.copy_from_slice(&a.rotation);
                        b.has_prev = a.has_prev;
                        b.energy = a.energy;
                        b.formant_env.copy_from_slice(&a.formant_env);
                        b.formant_age = a.formant_age;
                        b.formant_tilt = a.formant_tilt;
                        b.formant_comp = a.formant_comp;
                        self.add_frame(1, center, len);
                    } else {
                        self.process_frame(ch, center, len, pitch);
                        self.add_frame(ch, center, len);
                    }
                }
                // Somma delle finestre (una volta per tutti i canali).
                let mask = 2 * n - 1;
                let base = center as usize + self.max_window - len / 2;
                for i in 0..len {
                    self.norm[(base + i) & mask] += self.window[i] * self.synth_window[i];
                }
            }
        }

        let index = now as usize & (2 * n - 1);
        let norm = self.norm[index];
        let scale = if norm > 1e-3 { 1.0 / norm } else { 0.0 };
        for ch in 0..self.channels {
            let s = &mut self.states[ch];
            if let Some(o) = output.get_mut(ch) {
                *o = s.accum[index] * scale;
            }
            s.accum[index] = 0.0;
        }
        self.norm[index] = 0.0;
        self.position += 1;
    }

    /// Sceglie la lunghezza della finestra per questo frame e la prepara.
    fn prepare_window(&mut self, pitch: GrainPitch) {
        if pitch.voiced && pitch.period > 1.0 {
            let target = (pitch.period * self.periods_per_window).clamp(self.min_window as f32, self.max_window as f32);
            // Cambia al massimo del 15% per frame: niente salti.
            let step = 0.15 * self.window_len;
            self.window_len += (target - self.window_len).clamp(-step, step);
        }
        let len = self.window_len();
        let len = (len / 2 * 2).clamp(self.min_window, self.max_window);
        self.window_len = len as f32;
        // Hann simmetrica di lunghezza `len`.
        for i in 0..len {
            let x = PI * (i as f32 + 0.5) / len as f32;
            let s = x.sin();
            self.window[i] = s * s;
        }
        // Finestra di sintesi: Hann di `synth_len` campioni al centro.
        let short = self.synth_len.min(len);
        let offset = (len - short) / 2;
        for i in 0..len {
            self.synth_window[i] = if i < offset || i >= offset + short {
                0.0
            } else {
                let x = PI * ((i - offset) as f32 + 0.5) / short as f32;
                let s = x.sin();
                s * s
            };
        }
    }

    /// Somma il frame appena calcolato (in `frame_out`) nell'uscita del canale.
    fn add_frame(&mut self, ch: usize, center: i64, len: usize) {
        let mask = 2 * self.n_fft - 1;
        // Il campione dell'ingresso `t` esce all'istante `t + latenza`.
        let base = center as usize + self.max_window - len / 2;
        let state = &mut self.states[ch];
        for i in 0..len {
            state.accum[(base + i) & mask] += self.frame_out[i];
        }
    }

    /// Inviluppo spettrale (le formanti) con il "true envelope": il cepstrum
    /// lisciato viene ripetuto alzandolo dove le armoniche gli passano
    /// sopra, cosi' segue le cime delle armoniche invece di stare a meta'
    /// tra armoniche e valli (importante per le voci maschili, che hanno
    /// armoniche fitte). Calcolato su righe raggruppate a 4 a 4: l'inviluppo
    /// e' una curva lenta e cosi' costa un sedicesimo.
    fn spectral_envelope(&mut self, period: f32) {
        let n_env = self.n_env;
        let eh = n_env / 2;
        let group = self.n_fft / n_env;
        for q in 0..=eh {
            let lo = (q * group).saturating_sub(group / 2);
            let hi = (q * group + group / 2).min(self.half);
            let mut sum = 0.0f32;
            for k in lo..=hi {
                sum += self.magnitude[k];
            }
            self.env_log[q] = (sum / (hi - lo + 1) as f32 + 1e-9).ln();
        }
        // Sotto la prima armonica la voce non ha niente: lo spettro crolla
        // verso lo zero e l'inviluppo, lisciato, lo seguirebbe gia' sopra la
        // fondamentale. Spostando le formanti (o la nota) quel crollo finirebbe
        // sulle prime armoniche, che sono le piu' forti: voce svuotata o
        // rimbombante, soprattutto nelle voci maschili. Li' sotto si prosegue
        // allora piatto, al livello della prima armonica.
        let first = eh as f32 * 2.0 / period.max(2.0);
        let a = ((0.7 * first).floor() as usize).min(eh);
        let b = ((1.3 * first).ceil() as usize).clamp(a, eh);
        let mut peak = a;
        for q in a..=b {
            if self.env_log[q] > self.env_log[peak] {
                peak = q;
            }
        }
        let level = self.env_log[peak];
        for v in &mut self.env_log[..peak] {
            *v = level;
        }
        // Le quefrenze sono in campioni: teniamo quelle sotto meta' periodo.
        let cut = (0.5 * period).clamp(6.0, (n_env / 4) as f32) as usize;
        self.env_work.copy_from_slice(&self.env_log);
        for iteration in 0..Self::ENVELOPE_ITERATIONS {
            for k in 0..n_env {
                let v = if k <= eh { self.env_work[k] } else { self.env_work[n_env - k] };
                self.env_buf[k] = Complex::new(v, 0.0);
            }
            self.env_fft.process(&mut self.env_buf, true);
            for q in 0..n_env {
                let qq = q.min(n_env - q);
                let w = if qq < cut {
                    1.0
                } else if qq < cut + 3 {
                    0.5 + 0.5 * (PI * (qq - cut) as f32 / 3.0).cos()
                } else {
                    0.0
                };
                let c = self.env_buf[q];
                self.env_buf[q] = Complex::new(c.re * w / n_env as f32, 0.0);
            }
            self.env_fft.process(&mut self.env_buf, false);
            let last = iteration + 1 == Self::ENVELOPE_ITERATIONS;
            for q in 0..=eh {
                let smooth = self.env_buf[q].re;
                self.env_work[q] = if last {
                    smooth
                } else {
                    smooth + Self::ENVELOPE_ACCEL * (self.env_log[q] - smooth).max(0.0)
                };
            }
        }
        // Di nuovo su tutte le righe (interpolazione lineare).
        for k in 0..=self.half {
            let pos = k as f32 / group as f32;
            let i = (pos.floor() as usize).min(eh - 1);
            let f = (pos - i as f32).min(1.0);
            self.log_env[k] = self.env_work[i] * (1.0 - f) + self.env_work[i + 1] * f;
        }
    }

    /// Formant: guadagno per ogni riga in uscita che sposta le formanti
    /// (l'inviluppo letto `formant` volte piu' in basso: >1 voce piu' piccola,
    /// <1 piu' grande). Usa l'inviluppo lisciato nel tempo, sfuma sopra 7 kHz
    /// e ammorbidisce le spinte troppo forti (vedi le costanti `FORMANT_*`).
    fn formant_shift(&mut self, ch: usize, formant: f32) {
        let half = self.half;
        // Si liscia la forma dell'inviluppo, non il volume: il livello medio
        // (che sale e scende a ogni sillaba, uguale per tutte le frequenze) si
        // toglie prima. Altrimenti, con lisciature diverse tra bassi e acuti,
        // negli attacchi e nelle code delle note il guadagno sbaglierebbe.
        let (lo, hi) = (self.formant_level_bins.0, self.formant_level_bins.1);
        let level = self.log_env[lo..hi].iter().sum::<f32>() / (hi - lo) as f32;
        let tilt = self.spectral_tilt();
        let state = &mut self.states[ch];
        let fresh = state.formant_age > self.formant_hold;
        if fresh {
            for k in 0..=half {
                state.formant_env[k] = self.log_env[k] - level;
            }
            state.formant_tilt = tilt;
        } else {
            for k in 0..=half {
                state.formant_env[k] += self.formant_follow[k] * (self.log_env[k] - level - state.formant_env[k]);
            }
            state.formant_tilt += self.formant_follow[0] * (tilt - state.formant_tilt);
        }
        state.formant_age = 0;
        self.formant_gain_from_env(ch, formant, 1.0, Some(fresh));
    }

    /// Formant: guadagno per ogni riga dall'inviluppo lisciato del canale
    /// (`amount` = quanto dello spostamento, da 0 a 1). `update`: il frame ha
    /// una nota e aggiorna il compenso di volume (`Some(true)`: da capo);
    /// `None` usa quello delle note appena passate.
    fn formant_gain_from_env(&mut self, ch: usize, formant: f32, amount: f32, update: Option<bool>) {
        let half = self.half;
        let env = &self.states[ch].formant_env;
        for k in 0..=half {
            let src = k as f32 / formant;
            let i = src.floor() as usize;
            let moved = if i >= half {
                env[half]
            } else {
                let f = src - i as f32;
                env[i] * (1.0 - f) + env[i + 1] * f
            };
            let mut gain = (moved - env[k]) * self.formant_weight[k];
            if gain > 0.0 {
                gain = Self::FORMANT_MAX_BOOST * (gain / Self::FORMANT_MAX_BOOST).tanh();
            }
            self.formant_gain[k] = gain;
        }
        // Volume (`FORMANT_LOUDNESS_S`): quanto il guadagno alzerebbe il
        // volume percepito del corpo di questo frame (non sui frame molto piu'
        // brillanti della voce: una "z", il respiro).
        let brighter = (self.spectral_tilt() - self.states[ch].formant_tilt) * 10.0 / LN_10;
        if let Some(fresh) = update.filter(|_| brighter < Self::FORMANT_BRIGHTER_DB.0) {
            let (lo, hi) = self.loudness_bins;
            let (mut before, mut after) = (0.0f64, 0.0f64);
            for k in lo..=hi {
                let power = self.magnitude[k] * self.magnitude[k] * self.loudness_weight[k];
                before += power as f64;
                after += (power * (2.0 * self.formant_gain[k]).exp()) as f64;
            }
            let raw = if before > 1e-20 { (0.5 * (after / before).ln()) as f32 } else { 0.0 };
            let state = &mut self.states[ch];
            if fresh {
                state.formant_comp = raw;
            } else {
                state.formant_comp += self.formant_comp_follow * (raw - state.formant_comp);
            }
        }
        // Solo per recuperare un calo, e solo nel corpo della voce.
        let comp = self.states[ch].formant_comp.min(0.0);
        for (g, w) in self.formant_gain.iter_mut().zip(&self.body_weight) {
            *g = (*g - comp * w) * amount;
        }
    }

    /// Low Latency: lo stesso timbro di HQ per la testina a latenza zero
    /// (vedi `timbre.rs`). Su `segment` (la voce appena arrivata, prima dello
    /// spostamento) misura l'inviluppo come in HQ e restituisce il guadagno
    /// (logaritmico, per riga) da dare alla voce spostata di `ratio` dalla
    /// testina: le formanti tornano dove erano (la testina le sposta con la
    /// nota) e si spostano come vuole il comando Formant (`formant`), con le
    /// stesse regole di HQ (inviluppo lisciato, aria intatta, spinte morbide,
    /// volume). Usa il canale 0: un'istanza dedicata a questo.
    pub fn timbre_gain(&mut self, segment: &[f32], period: f32, ratio: f32, formant: f32) -> &[f32] {
        let half = self.half;
        let len = ((period.max(1.0) * self.periods_per_window) as usize)
            .clamp(self.min_window, self.max_window)
            .min(segment.len())
            / 2
            * 2;
        let start = segment.len() - len;
        for c in self.spectrum.iter_mut() {
            *c = Complex::default();
        }
        for i in 0..len {
            let x = PI * (i as f32 + 0.5) / len as f32;
            let w = x.sin() * x.sin();
            self.spectrum[i] = Complex::new(segment[start + i] * w, 0.0);
        }
        self.fft.process(&mut self.spectrum, false);
        for k in 0..=half {
            let c = self.spectrum[k];
            self.magnitude[k] = (c.re * c.re + c.im * c.im).sqrt();
        }
        self.spectral_envelope(period);
        // La testina ha portato la riga `k / ratio` alla riga `k`: si rimette
        // l'inviluppo della riga `k` (come il "keep" di HQ).
        let ratio = ratio.clamp(0.25, 4.0);
        for k in 0..=half {
            let src = k as f32 / ratio;
            let i = src.floor() as usize;
            let moved = if i >= half {
                self.log_env[half]
            } else {
                let f = src - i as f32;
                self.log_env[i] * (1.0 - f) + self.log_env[i + 1] * f
            };
            self.timbre[k] = self.log_env[k] - moved;
        }
        let state = &mut self.states[0];
        state.formant_age = state.formant_age.saturating_add(1);
        if (formant - 1.0).abs() > 1e-5 {
            self.formant_shift(0, formant.clamp(0.5, 2.0));
            for k in 0..=half {
                self.timbre[k] += self.formant_gain[k];
            }
        }
        for g in self.timbre.iter_mut() {
            *g = g.clamp(-2.8, 2.8);
        }
        &self.timbre
    }

    /// Quanto e' brillante il frame: energia tra 4 e 16 kHz rispetto a
    /// quella tra 200 Hz e 4 kHz (logaritmo naturale del rapporto).
    fn spectral_tilt(&self) -> f32 {
        let (lo, mid, hi) = self.tilt_bins;
        let low: f32 = self.magnitude[lo..mid].iter().map(|m| m * m).sum();
        let high: f32 = self.magnitude[mid..hi].iter().map(|m| m * m).sum();
        ((high + 1e-12) / (low + 1e-12)).ln()
    }

    /// Formant in un frame che passa senza nota (vedi `FORMANT_BRIDGE_S`):
    /// quanto dello spostamento applicare, dai frame passati dall'ultima nota.
    fn formant_bridge(&self, age: usize) -> f32 {
        let (full, gone) = self.formant_bridge;
        let t = age as f32;
        if t <= full {
            1.0
        } else if t >= gone {
            0.0
        } else {
            0.5 + 0.5 * (PI * (t - full) / (gone - full)).cos()
        }
    }

    fn process_frame(&mut self, ch: usize, center: i64, len: usize, pitch: GrainPitch) {
        let n = self.n_fft;
        let half = self.half;
        let hop = self.hop as f32;
        let state = &mut self.states[ch];

        // 1) Finestra + "fase zero" (il centro del frame va all'indice 0).
        for c in self.spectrum.iter_mut() {
            *c = Complex::default();
        }
        let first = center as usize + n - len / 2;
        let mut new_energy = 0.0f32;
        for i in 0..len {
            let x = state.input[(first + i) & (n - 1)];
            if i >= len - self.hop {
                new_energy += x * x;
            }
            self.spectrum[(i + n - len / 2) & (n - 1)] = Complex::new(x * self.window[i], 0.0);
        }
        new_energy /= self.hop as f32;
        // Attacco brusco: l'ultimo pezzo della finestra e' molto piu' forte
        // della media recente.
        let onset = new_energy > 8.0 * state.energy + 1e-7;
        state.energy = 0.7 * state.energy + 0.3 * new_energy;

        self.fft.process(&mut self.spectrum, false);
        for k in 0..=half {
            let c = self.spectrum[k];
            self.magnitude[k] = (c.re * c.re + c.im * c.im).sqrt();
            self.phase[k] = c.im.atan2(c.re);
        }

        let ratio = pitch.ratio.clamp(0.25, 4.0);
        let formant = pitch.formant.clamp(0.5, 2.0);
        {
            let state = &mut self.states[ch];
            state.formant_age = state.formant_age.saturating_add(1);
        }
        let shifting = pitch.voiced && ((ratio - 1.0).abs() > 1e-5 || (formant - 1.0).abs() > 1e-5);
        let state = &mut self.states[ch];
        if !pitch.voiced || onset {
            // Niente nota (o attacco): lo spettro passa identico e le
            // rotazioni ripartono da zero.
            state.rotation.fill(0.0);
            state.theta = 0.0;
        }
        let passthrough = !pitch.voiced || (!shifting && state.prev_rotation.iter().all(|&r| r == 0.0)) || onset;

        if passthrough {
            self.out_spectrum.copy_from_slice(&self.spectrum);
            self.states[ch].rotation.fill(0.0);
            self.states[ch].theta = 0.0;
            // Formant: subito dopo una nota il timbro resta spostato (la nota
            // e le fasi no: e' solo un filtro), poi sfuma (`FORMANT_BRIDGE_S`).
            let mut amount = self.formant_bridge(self.states[ch].formant_age);
            if (formant - 1.0).abs() > 1e-5 && amount > 0.0 {
                // Solo se e' ancora la voce di prima (la nota persa per un
                // attimo), non una "s" o una "f" che comincia: quelle hanno
                // molti piu' acuti della vocale e restano intatte.
                let brighter = (self.spectral_tilt() - self.states[ch].formant_tilt) * 10.0 / LN_10;
                let x = ((Self::FORMANT_BRIGHTER_DB.1 - brighter)
                    / (Self::FORMANT_BRIGHTER_DB.1 - Self::FORMANT_BRIGHTER_DB.0))
                    .clamp(0.0, 1.0);
                amount *= x * x * (3.0 - 2.0 * x);
                if amount > 0.0 {
                    self.formant_gain_from_env(ch, formant, amount, None);
                    for k in 0..=half {
                        let g = self.formant_gain[k].clamp(-2.8, 2.8).exp();
                        let c = self.out_spectrum[k];
                        self.out_spectrum[k] = Complex::new(c.re * g, c.im * g);
                        if k > 0 && k < half {
                            let c = self.out_spectrum[n - k];
                            self.out_spectrum[n - k] = Complex::new(c.re * g, c.im * g);
                        }
                    }
                }
            }
        } else {
            // 2) Inviluppo spettrale (formanti).
            let need_env = (ratio - 1.0).abs() > 1e-5 || (formant - 1.0).abs() > 1e-5;
            let shift_formants = (formant - 1.0).abs() > 1e-5;
            if need_env {
                self.spectral_envelope(pitch.period);
                if shift_formants {
                    self.formant_shift(ch, formant);
                }
            }

            // 3) Picchi: massimi locali su mezzo lobo della finestra (con
            // finestre corte e FFT lunga ogni armonica occupa piu' righe).
            let state = &mut self.states[ch];
            let reach = ((2 * n) / len).max(2);
            let max_mag = self.magnitude.iter().cloned().fold(0.0f32, f32::max);
            let floor = max_mag * Self::PEAK_FLOOR;
            self.peaks.clear();
            self.peak_bin.clear();
            let mut k = reach;
            while k + reach <= half {
                let m = self.magnitude[k];
                if m > floor && (k - reach..=k + reach).all(|j| j == k || self.magnitude[j] < m || (j > k && self.magnitude[j] <= m)) {
                    // Frequenza esatta (in righe) dalla rotazione della fase.
                    let bin_omega = TAU * k as f32 / n as f32;
                    let omega = if state.has_prev {
                        let dev = wrap(self.phase[k] - state.prev_phase[k] - bin_omega * hop) / hop;
                        if dev.abs() <= TAU * reach as f32 / n as f32 {
                            bin_omega + dev
                        } else {
                            bin_omega
                        }
                    } else {
                        bin_omega
                    };
                    self.peaks.push(k);
                    self.peak_bin.push(omega * n as f32 / TAU);
                    k += reach;
                    continue;
                }
                k += 1;
            }

            for c in self.out_spectrum.iter_mut() {
                *c = Complex::default();
            }
            if self.peaks.is_empty() {
                self.out_spectrum.copy_from_slice(&self.spectrum);
                state.rotation.fill(0.0);
                state.theta = 0.0;
            }
            let count = self.peaks.len();
            // Armoniche "in fase" tra loro: la fondamentale accumula la sua
            // rotazione (theta) e l'armonica h gira di h x theta. Cosi' la
            // forma d'onda di ogni ciclo della voce (l'impulso delle corde
            // vocali) resta la stessa a nota spostata. Se ogni armonica
            // accumulasse la sua rotazione per conto suo, i piccoli errori di
            // misura le sfaserebbero piano piano tra loro: la voce perde la
            // forma e diventa "phasey", metallica (misurato sulle voci
            // maschili: somiglianza della forma d'onda con la voce ideale da
            // 0,95 a 0,99).
            let f0_bins = n as f32 / pitch.period.max(1.0);
            state.theta = wrap(state.theta + hop * TAU / pitch.period.max(1.0) * (ratio - 1.0));
            let theta = state.theta;
            let mut lo = 0usize;
            for p in 0..count {
                let kp = self.peaks[p];
                // Fine della zona: la riga piu' debole tra questo picco e il prossimo.
                let hi = if p + 1 < count {
                    let next = self.peaks[p + 1];
                    let mut best = kp;
                    for k in kp..=next {
                        if self.magnitude[k] < self.magnitude[best] {
                            best = k;
                        }
                    }
                    best
                } else {
                    half
                };
                let true_bin = self.peak_bin[p];
                // Nota spostata in basso: le armoniche si avvicinano tra loro e
                // le code di ogni zona finirebbero sopra la zona vicina, sommate
                // con un'altra fase (un battimento, un fruscio tra le
                // armoniche). Ogni zona scrive solo fino a meta' strada dalle
                // armoniche vicine, nelle posizioni nuove (misurato: voce
                // trasposta di -4 semitoni meno ruvida, 12,9% -> 11,5% dei frame,
                // e il rumore tra le armoniche sotto 1 kHz -1,2 dB). Spostando in
                // alto le zone si allontanano e non serve.
                let (cell_lo, cell_hi) = if ratio < 1.0 {
                    let moved = |q: usize| self.peak_bin[q] * ratio;
                    (
                        if p > 0 { 0.5 * (moved(p - 1) + moved(p)) } else { f32::MIN },
                        if p + 1 < count { 0.5 * (moved(p) + moved(p + 1)) } else { f32::MAX },
                    )
                } else {
                    (f32::MIN, f32::MAX)
                };
                {
                    // Spostamento in righe (anche frazionario): ogni riga della
                    // zona viene distribuita tra le due righe di arrivo piu'
                    // vicine, cosi' il picco cade esattamente sulla frequenza nuova.
                    let shift = true_bin * (ratio - 1.0);
                    let shift_floor = shift.floor();
                    let frac = shift - shift_floor;
                    let shift_int = shift_floor as isize;
                    // La fase dell'armonica spostata gira alla velocita' della
                    // frequenza nuova: la rotazione accumula la differenza.
                    let omega = TAU * true_bin / n as f32;
                    let h = (true_bin / f0_bins).round();
                    let harmonic = h >= 1.0
                        && true_bin <= self.harmonic_lock_bins
                        && (true_bin - h * f0_bins).abs() < 0.35 * f0_bins;
                    let rot = if harmonic {
                        wrap(h * theta)
                    } else {
                        wrap(state.prev_rotation[kp] + hop * omega * (ratio - 1.0))
                    };
                    for k in lo..=hi {
                        state.rotation[k] = rot;
                        let ph = self.phase[k] + rot;
                        let (c, s) = (ph.cos(), ph.sin());
                        for (offset, weight) in [(0isize, 1.0 - frac), (1, frac)] {
                            if weight <= 0.0 {
                                continue;
                            }
                            let dest = k as isize + shift_int + offset;
                            if (dest as f32) < cell_lo || (dest as f32) > cell_hi {
                                continue;
                            }
                            if dest < 0 || dest as usize > half {
                                continue;
                            }
                            let dest = dest as usize;
                            let mut mag = self.magnitude[k] * weight;
                            if need_env {
                                // Timbro rimesso al suo posto dopo lo spostamento
                                // della nota (+ comando Formant, se c'e').
                                let keep = self.log_env[dest] - self.log_env[k];
                                let gain = if shift_formants { self.formant_gain[dest] + keep } else { keep };
                                mag *= gain.clamp(-2.8, 2.8).exp();
                            }
                            self.out_spectrum[dest] = self.out_spectrum[dest] + Complex::new(mag * c, mag * s);
                        }
                    }
                }
                lo = hi + 1;
                if lo > half {
                    break;
                }
            }
            // Riga 0 e ultima: reali.
            self.out_spectrum[0] = Complex::new(self.out_spectrum[0].re, 0.0);
            self.out_spectrum[half] = Complex::new(self.out_spectrum[half].re, 0.0);
            for k in 1..half {
                self.out_spectrum[n - k] = self.out_spectrum[k].conj();
            }
        }

        let state = &mut self.states[ch];
        state.prev_phase.copy_from_slice(&self.phase);
        state.prev_rotation.copy_from_slice(&state.rotation);
        state.has_prev = true;

        // 5) Ritorno nel tempo e finestra di sintesi.
        self.fft.process(&mut self.out_spectrum, true);
        let scale = 1.0 / n as f32;
        for i in 0..len {
            self.frame_out[i] = self.out_spectrum[(i + n - len / 2) & (n - 1)].re * scale * self.synth_window[i];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: &[f32], sr: f32, pitch: GrainPitch) -> (Vec<f32>, usize) {
        let mut pv = PhaseVocoder::new(1, sr);
        let latency = pv.latency();
        let mut out = Vec::with_capacity(input.len());
        for &x in input {
            let mut o = [0.0];
            pv.process(&[x], &mut o, |_| pitch);
            out.push(o[0]);
        }
        (out, latency)
    }

    /// Livello (dB) di alcune bande di frequenza, frame per frame (FFT di
    /// 2048 campioni ogni 256).
    fn band_levels(x: &[f32], sr: f32, bands: &[(f32, f32)]) -> Vec<Vec<f32>> {
        let n = 2048;
        let fft = Fft::new(n);
        let mut buf = vec![Complex::default(); n];
        let mut out = vec![Vec::new(); bands.len()];
        let mut start = 0;
        while start + n <= x.len() {
            for i in 0..n {
                let w = (PI * (i as f32 + 0.5) / n as f32).sin().powi(2);
                buf[i] = Complex::new(x[start + i] * w, 0.0);
            }
            fft.process(&mut buf, false);
            for (b, &(lo, hi)) in bands.iter().enumerate() {
                let (first, last) = ((lo / sr * n as f32) as usize, (hi / sr * n as f32) as usize);
                let e: f32 = buf[first..last].iter().map(|c| c.re * c.re + c.im * c.im).sum();
                out[b].push(10.0 * (e + 1e-20).log10());
            }
            start += 256;
        }
        out
    }

    /// Banda (lo..hi Hz) di un segnale, con una FFT sola su tutto (filtro "a muro").
    fn band(x: &[f32], sr: f32, lo: f32, hi: f32) -> Vec<f32> {
        let n = x.len().next_power_of_two();
        let fft = Fft::new(n);
        let mut buf: Vec<Complex> = (0..n).map(|i| Complex::new(x.get(i).copied().unwrap_or(0.0), 0.0)).collect();
        fft.process(&mut buf, false);
        for k in 0..n {
            let hz = k.min(n - k) as f32 * sr / n as f32;
            if hz < lo || hz > hi {
                buf[k] = Complex::default();
            }
        }
        fft.process(&mut buf, true);
        buf[..x.len()].iter().map(|c| c.re / n as f32).collect()
    }

    /// Fattore di cresta mediano (picco / RMS, dB) su blocchi da 20 ms.
    fn crest_db(x: &[f32], sr: f32) -> f32 {
        let n = (0.02 * sr) as usize;
        let mut c: Vec<f32> = x
            .chunks_exact(n)
            .map(|b| {
                let peak = b.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                let rms = (b.iter().map(|v| v * v).sum::<f32>() / n as f32).sqrt();
                20.0 * (peak / (rms + 1e-12)).log10()
            })
            .collect();
        c.sort_by(|a, b| a.partial_cmp(b).unwrap());
        c[c.len() / 2]
    }

    #[test]
    fn high_harmonics_keep_the_pulses_of_the_voice() {
        // Voce maschile (110 Hz) con vibrato (5,5 Hz, +-30 cent) e il piccolo
        // tremolio d'intonazione di una voce vera, portata dritta su La# come
        // fa l'autotune. Gli impulsi delle corde vocali arrivano anche negli
        // acuti: sopra i 5 kHz devono restare netti come nell'originale. Con le
        // armoniche "in fase" solo fino a 5 kHz il vibrato le sparpagliava
        // (cresta 5-8 kHz 13,9 dB contro 16,8 dell'ingresso): una risonanza
        // "phasey" sui medio-alti e sugli acuti.
        let sr = 48000.0;
        let len = 120_000;
        let jitter = crate::synth::noise(len, 1.0, 7);
        let k = 1.0 - (-2.0 * PI * 40.0 / sr).exp();
        let mut lp = 0.0f32;
        let f0: Vec<f32> = jitter
            .iter()
            .enumerate()
            .map(|(i, &j)| {
                lp += k * (j - lp);
                let vibrato = 2f32.powf(30.0 / 1200.0 * (2.0 * PI * 5.5 * i as f32 / sr).sin());
                110.0 * vibrato * (1.0 + 0.01 * lp)
            })
            .collect();
        let input = crate::synth::sing_curve_detailed(&f0, &vec![1.0; len], sr, 0.0, 3);
        let mut pv = PhaseVocoder::new(1, sr);
        let latency = pv.latency();
        let mut out = Vec::with_capacity(len);
        for i in 0..len + latency {
            let x = input.get(i).copied().unwrap_or(0.0);
            let mut o = [0.0];
            pv.process(&[x], &mut o, |t| {
                let j = (t.max(0.0) as usize).min(len - 1);
                GrainPitch { period: sr / f0[j], ratio: 116.54 / f0[j], voiced: true, formant: 1.0 }
            });
            if i >= latency {
                out.push(o[0]);
            }
        }
        let skip = (0.2 * sr) as usize;
        let c_in = crest_db(&band(&input[skip..], sr, 5000.0, 8000.0), sr);
        let c_out = crest_db(&band(&out[skip..], sr, 5000.0, 8000.0), sr);
        assert!(c_out > c_in - 1.0, "impulsi sparpagliati negli acuti: cresta 5-8 kHz {c_out:.1} dB, ingresso {c_in:.1} dB");
    }

    #[test]
    fn formant_moves_the_timbre_keeps_the_air_and_does_not_tremble() {
        // Una vocale "a" con il respiro (rumore che scende verso gli acuti, come
        // l'aria di una voce vera) e un tremolo di volume (5 Hz, +-6 dB).
        let sr = 48000.0;
        let len = 96000;
        let vowel = crate::synth::steady_vowel(150.0, sr, len);
        let mut breath = crate::synth::noise(len, 0.03, 7);
        let k = (-TAU * 3000.0 / sr).exp();
        for _ in 0..2 {
            let mut y = 0.0;
            for v in breath.iter_mut() {
                y = (1.0 - k) * *v + k * y;
                *v = y;
            }
        }
        let input: Vec<f32> = vowel
            .iter()
            .zip(&breath)
            .enumerate()
            .map(|(i, (v, b))| (v + b) * (1.0 + 0.5 * (TAU * 5.0 * i as f32 / sr).sin()))
            .collect();
        let bands = [(400.0, 800.0), (800.0, 1600.0), (1600.0, 3200.0), (14000.0, 20000.0)];
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
        let db = 20.0 / std::f32::consts::LN_10;
        for semitones in [4.0f32, -4.0] {
            let formant = 2f32.powf(semitones / 12.0);
            let pitch = GrainPitch { period: sr / 150.0, ratio: 1.0, voiced: true, formant };
            let mut pv = PhaseVocoder::new(1, sr);
            let latency = pv.latency();
            let mut out = Vec::with_capacity(len);
            let mut gains: Vec<Vec<f32>> = Vec::new();
            for (i, &x) in input.iter().enumerate() {
                let mut o = [0.0];
                pv.process(&[x], &mut o, |_| pitch);
                out.push(o[0]);
                if i > 12000 && pv.countdown == pv.hop {
                    // Un frame appena calcolato: il guadagno del comando Formant.
                    gains.push(pv.formant_gain.clone());
                }
            }
            let dry = band_levels(&input[12000..len - latency], sr, &bands);
            let wet = band_levels(&out[12000 + latency..], sr, &bands);
            let change: Vec<f32> = (0..bands.len()).map(|b| mean(&wet[b]) - mean(&dry[b])).collect();
            // Il timbro si sposta davvero...
            if semitones > 0.0 {
                assert!(change[1] > 6.0, "{semitones:+}: formanti non spostate in alto {change:?}");
            } else {
                assert!(change[1] < -1.5 && change[2] < -1.5, "{semitones:+}: formanti non spostate in basso {change:?}");
            }
            // ...l'aria sopra i 14 kHz resta com'era...
            assert!(change[3].abs() < 0.5, "{semitones:+}: aria cambiata di {:.1} dB", change[3]);
            let bin = |hz: f32| (hz / sr * pv.n_fft as f32) as usize;
            for g in &gains {
                assert!(g[bin(14100.0)..].iter().all(|v| v.abs() < 1e-6));
                assert!(g.iter().all(|v| v * db <= 20.01 + 3.0), "spinta oltre 20 dB");
            }
            // ...il volume percepito non cambia...
            let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
            let level = db * (rms(&out[12000 + latency..]) / rms(&input[12000..len - latency])).ln();
            assert!(level.abs() < 0.5, "{semitones:+}: volume cambiato di {level:+.2} dB");
            // ...e il guadagno non trema da un frame all'altro (senza lisciatura
            // il respiro lo faceva saltare di 1-2 dB ogni 5 ms).
            let (lo, hi) = (bin(300.0), bin(7000.0));
            let steps: Vec<f32> = gains
                .windows(2)
                .flat_map(|w| (lo..hi).map(move |k| (w[1][k] - w[0][k]).abs() * db))
                .collect();
            let tremble = mean(&steps);
            assert!(tremble < 0.5, "{semitones:+}: il guadagno trema di {tremble:.2} dB a frame");
        }
    }

    #[test]
    fn formant_does_not_blink_when_the_note_drops_for_a_moment() {
        // Una vocale tenuta in cui la nota "si perde" per un frame ogni tanto
        // (succede in mezzo al canto) e poi una lunga consonante. Il timbro
        // spostato dal comando Formant deve restare fermo nei buchi brevi e sparire
        // in quelli lunghi.
        let sr = 48000.0;
        let len = 48000;
        let input = crate::synth::steady_vowel(150.0, sr, len);
        let formant = 2f32.powf(4.0 / 12.0);
        let voiced = GrainPitch { period: sr / 150.0, ratio: 1.0, voiced: true, formant };
        let gap = GrainPitch { voiced: false, ..voiced };
        // `every`: un frame senza nota ogni tanti; dal frame `until` in poi solo consonante.
        let render = |every: usize, until: usize| {
            let mut pv = PhaseVocoder::new(1, sr);
            let latency = pv.latency();
            let mut frame = 0usize;
            let mut out = Vec::with_capacity(len);
            for &x in &input {
                let mut o = [0.0];
                pv.process(&[x], &mut o, |_| {
                    frame += 1;
                    if frame >= until || (every > 0 && frame % every == 3) {
                        gap
                    } else {
                        voiced
                    }
                });
                out.push(o[0]);
            }
            (out, latency, pv.hop)
        };
        let (steady, latency, hop) = render(0, usize::MAX);
        let (blinking, ..) = render(7, usize::MAX);
        let energy = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum::<f32>();
        let span = 12000 + latency..len;
        let effect = energy(&steady[span.clone()], &input[span.start - latency..len - latency]);
        let blink = energy(&blinking[span.clone()], &steady[span.clone()]);
        assert!(blink < 0.01 * effect, "il timbro lampeggia: {:.4} dell'effetto", blink / effect);

        // Dopo 64 ms senza nota la voce passa di nuovo identica.
        let until = 24000 / hop;
        let (consonant, ..) = render(0, until);
        let from = until * hop + 4000 + 2 * latency;
        for i in from..len {
            assert!((consonant[i] - input[i - latency]).abs() < 1e-3, "campione {i} non intatto");
        }
        // Subito dopo la nota invece il timbro spostato c'e' ancora (il
        // secondo frame senza nota esce a until*hop + latenza/2 + hop).
        let near = until * hop + latency / 2 + hop - 100;
        let moved = energy(&consonant[near..near + 200], &input[near - latency..near + 200 - latency]);
        let kept = energy(&consonant[near..near + 200], &steady[near..near + 200]);
        assert!(kept < 0.05 * moved, "Formant si spegne di colpo dopo la nota");

        // Una "s" subito dopo la vocale invece passa intatta, anche nei primi ms.
        let switch = 24000;
        let hiss = crate::synth::noise(len, 0.2, 3);
        let sibilant: Vec<f32> = (0..len)
            .map(|i| if i < switch { input[i] } else { hiss[i] - 2.0 * hiss[i - 1] + hiss[i - 2] })
            .collect();
        let mut pv = PhaseVocoder::new(1, sr);
        let mut out = Vec::with_capacity(len);
        for &x in &sibilant {
            let mut o = [0.0];
            pv.process(&[x], &mut o, |t| if t >= switch as f64 { gap } else { voiced });
            out.push(o[0]);
        }
        for i in switch + 400..switch + 4000 {
            assert!((out[i + latency] - sibilant[i]).abs() < 1e-3, "la \"s\" cambia al campione {i}");
        }
    }

    #[test]
    fn formant_does_not_make_s_and_z_stand_out() {
        // Una "z": vocale con tanto fruscio acuto (4-12 kHz). Con Formant in
        // alto o in basso la voce non deve scendere sotto il fruscio, e il
        // fruscio non deve salire (con il compenso di volume su tutte le
        // frequenze e in entrambe le direzioni le "s" e le "z" spiccavano di piu').
        let sr = 48000.0;
        let len = 72000;
        let vowel = crate::synth::steady_vowel(150.0, sr, len);
        let mut hiss = crate::synth::noise(len, 0.25, 11);
        let mut prev = 0.0;
        for v in hiss.iter_mut() {
            // Derivata: rumore che sale verso gli acuti, come una "s".
            let x = *v;
            *v = x - prev;
            prev = x;
        }
        let input: Vec<f32> = vowel.iter().zip(&hiss).map(|(v, h)| v + h).collect();
        let bands = [(150.0, 3000.0), (4500.0, 12000.0)];
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
        for semitones in [4.0f32, -4.0] {
            let formant = 2f32.powf(semitones / 12.0);
            let (out, latency) = run(&input, sr, GrainPitch { period: sr / 150.0, ratio: 1.0, voiced: true, formant });
            let dry = band_levels(&input[12000..len - latency], sr, &bands);
            let wet = band_levels(&out[12000 + latency..], sr, &bands);
            let body = mean(&wet[0]) - mean(&dry[0]);
            let hiss = mean(&wet[1]) - mean(&dry[1]);
            // (Formant in basso legge l'inviluppo da piu' in alto: su un fruscio
            // che sale verso gli acuti 4,5-7 kHz salgono di ~1 dB da sempre.)
            assert!(hiss < 1.15, "{semitones:+}: fruscio acuto +{hiss:.2} dB");
            assert!(body > -0.5, "{semitones:+}: la voce perde {body:.2} dB");
            assert!(hiss - body < 1.5, "{semitones:+}: le s spiccano di {:.2} dB sulla voce", hiss - body);
        }
    }

    #[test]
    fn unvoiced_passes_through_exactly() {
        let sr = 48000.0;
        let input: Vec<f32> = (0..20000).map(|i| ((i as f32 * 0.013).sin() + (i as f32 * 0.31).cos() * 0.3) * 0.5).collect();
        let (out, latency) = run(&input, sr, GrainPitch { period: 200.0, ratio: 1.0, voiced: false, formant: 1.0 });
        for i in 8000..input.len() {
            assert!((out[i] - input[i - latency]).abs() < 1e-3, "campione {i}: {} vs {}", out[i], input[i - latency]);
        }
    }

    #[test]
    fn shifts_a_harmonic_tone_up_a_semitone() {
        let sr = 48000.0;
        let f0 = 220.0;
        let input: Vec<f32> = (0..48000)
            .map(|i| {
                let t = i as f32 / sr;
                (1..8).map(|h| (TAU * f0 * h as f32 * t).sin() / h as f32).sum::<f32>() * 0.3
            })
            .collect();
        let ratio = 2f32.powf(1.0 / 12.0);
        let (out, _) = run(&input, sr, GrainPitch { period: sr / f0, ratio, voiced: true, formant: 1.0 });
        // Frequenza fondamentale in uscita con l'autocorrelazione.
        let seg = &out[20000..40000];
        let target = f0 * ratio;
        let lag_for = |f: f32| sr / f;
        let best = (150..260)
            .map(|lag| {
                let s: f32 = seg[..18000].iter().zip(&seg[lag..lag + 18000]).map(|(a, b)| a * b).sum();
                (lag, s)
            })
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap()
            .0;
        assert!((best as f32 - lag_for(target)).abs() <= 1.0, "periodo {best} invece di {}", lag_for(target));
        // Livello simile all'ingresso.
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        let ratio_db = 20.0 * (rms(seg) / rms(&input[20000..40000])).log10();
        assert!(ratio_db.abs() < 1.5, "livello {ratio_db:.1} dB");
    }
}
