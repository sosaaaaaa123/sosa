//! Il correttore: unisce rilevamento (YIN), scelta della nota (scale) e
//! spostamento dell'intonazione (PSOLA). E' il "cervello" del plugin.

use crate::psola::{GrainPitch, PsolaShifter};
use crate::scale::{hz_to_note, NoteSnapper, VoiceType};
use crate::yin::YinDetector;
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
    /// Spostamento delle formanti in semitoni: positivo = voce piu' "piccola",
    /// negativo = voce piu' "grande", senza cambiare la nota.
    pub formant: f32,
    /// Sensibilita' del rilevamento (0..1): piu' alta = riconosce anche voci
    /// con molta aria, ma con piu' rischio di errori.
    pub tracking: f32,
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
            formant: 0.0,
            tracking: 0.3,
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
    if flex <= 0.0 {
        return 1.0;
    }
    // Distanza oltre la quale la nota resta com'e': 48 centesimi con Flex-Tune
    // al 25%, 37 al 50%, 25 al 75%, 12 al 100%.
    let outer = 0.5 - 0.38 * flex.min(1.0).powf(1.5);
    1.0 - smoothstep(outer * 0.65, outer, distance)
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
}

const HISTORY: usize = 64;

pub struct PitchCorrector {
    sample_rate: f32,
    channels: usize,
    voice: VoiceType,

    detector: YinDetector,
    shifter: PsolaShifter,

    /// Ultimi campioni (mono) per il rilevatore.
    mono: Vec<f32>,
    mono_mask: usize,
    frame: Vec<f32>,

    /// Ogni quanti campioni rifare l'analisi (circa 5 ms).
    hop: usize,
    countdown: usize,
    position: u64,

    /// Le analisi recenti: lo shifter lavora "in ritardo" (latenza) e deve
    /// usare il pitch giusto per il pezzo di audio che sta elaborando.
    history: [Analysis; HISTORY],
    history_head: usize,

    snapper: NoteSnapper,
    /// Correzione attuale in semitoni (smussata secondo la velocita').
    correction: f32,
    was_voiced: bool,
    /// Per brevi "buchi" nel rilevamento in mezzo a una nota manteniamo la
    /// correzione precedente invece di toglierla di colpo.
    hold_left: usize,
    hold_samples: usize,
    last_period: f32,
    last_ratio: f32,
    unvoiced_period: f32,
    /// Ultima nota rilevata e quante analisi consecutive hanno visto un salto.
    last_note: f32,
    leap_count: u32,
    /// La nota bersaglio attuale e da quanto tempo (secondi) la si sta cantando.
    current_target: Option<i32>,
    note_age: f32,
    /// Vibrato della voce in ingresso e di quella gia' corretta.
    vibrato_in: VibratoFilter,
    vibrato_out: VibratoFilter,
}

impl PitchCorrector {
    /// Salto (in semitoni) oltre il quale un cambio di nota e' "sospetto".
    const LEAP_SEMITONES: f32 = 7.0;
    /// Per quante analisi consecutive (circa 5 ms l'una) un salto deve durare
    /// per essere considerato vero.
    const LEAP_CONFIRMATIONS: u32 = 3;

    /// Crea il correttore. Tutta la memoria viene allocata qui, mai durante
    /// l'elaborazione audio.
    pub fn new(sample_rate: f32, channels: usize) -> Self {
        let channels = channels.clamp(1, MAX_CHANNELS);
        let detector = YinDetector::new(sample_rate, VoiceType::LOWEST_SUPPORTED_HZ);
        let shifter = PsolaShifter::new(channels, detector.capacity_max_period(), sample_rate);
        let mono_len = (detector.max_frame_len() + 1).next_power_of_two();
        let frame = vec![0.0; detector.max_frame_len()];
        let hop = ((sample_rate * 0.005).round() as usize).max(32);

        let mut corrector = Self {
            sample_rate,
            channels,
            voice: VoiceType::AltoTenor,
            detector,
            shifter,
            mono: vec![0.0; mono_len],
            mono_mask: mono_len - 1,
            frame,
            hop,
            countdown: hop,
            position: 0,
            history: [Analysis::default(); HISTORY],
            history_head: 0,
            snapper: NoteSnapper::default(),
            correction: 0.0,
            was_voiced: false,
            hold_left: 0,
            hold_samples: (sample_rate * 0.03) as usize,
            last_period: 0.0,
            last_ratio: 1.0,
            unvoiced_period: 0.0,
            last_note: 0.0,
            leap_count: 0,
            current_target: None,
            note_age: 0.0,
            vibrato_in: VibratoFilter::new(sample_rate / hop as f32),
            vibrato_out: VibratoFilter::new(sample_rate / hop as f32),
        };
        corrector.set_voice(VoiceType::AltoTenor);
        corrector
    }

    /// Cambia il tipo di voce. Cambia anche la latenza (vedi [`latency`](Self::latency)).
    pub fn set_voice(&mut self, voice: VoiceType) {
        self.voice = voice;
        self.detector.set_lowest_hz(voice.lowest_hz());
        let max_period = self.detector.max_period() as f32;
        self.shifter.set_max_period(max_period);
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
        self.shifter.latency()
    }

    /// Intervallo di tempo tra due analisi, in secondi (per il grafico).
    pub fn analysis_interval(&self) -> f32 {
        self.hop as f32 / self.sample_rate
    }

    /// Azzera tutto lo stato (buffer, correzione in corso).
    pub fn reset(&mut self) {
        self.shifter.reset();
        self.mono.fill(0.0);
        self.countdown = self.hop;
        self.position = 0;
        self.history = [Analysis::default(); HISTORY];
        self.history_head = 0;
        self.snapper.reset();
        self.correction = 0.0;
        self.was_voiced = false;
        self.hold_left = 0;
        self.last_period = self.unvoiced_period;
        self.last_ratio = 1.0;
        self.last_note = 0.0;
        self.leap_count = 0;
        self.current_target = None;
        self.note_age = 0.0;
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
        let formant = 2f32.powf(settings.formant.clamp(-12.0, 12.0) / 12.0);
        let unvoiced = GrainPitch { period: self.unvoiced_period, ratio: 1.0, voiced: false, formant };
        self.shifter.process(&input[..channels], &mut output[..channels], |t| {
            lookup(history, t).map_or(unvoiced, |g| GrainPitch { formant, ..g })
        });
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
        let estimate = self.detector.detect(&self.frame[..len]);

        let window = self.detector.window_len() as f64;
        let report;
        let (center, period, ratio, voiced);

        // Un salto enorme di intonazione in mezzo a una nota e' quasi sempre un
        // errore del rilevatore (tipicamente un'ottava sopra, quando la voce si
        // spegne). Lo accettiamo solo se dura per alcune analisi di fila.
        let detected_note = hz_to_note(self.sample_rate / estimate.period.max(1.0));
        let suspicious_leap = estimate.voiced
            && self.was_voiced
            && (detected_note - self.last_note).abs() > Self::LEAP_SEMITONES
            && self.leap_count < Self::LEAP_CONFIRMATIONS;
        if suspicious_leap {
            self.leap_count += 1;
        } else {
            self.leap_count = 0;
        }

        if estimate.voiced && !suspicious_leap {
            let note = detected_note;
            self.last_note = note;
            let target = self.snapper.update(note, &settings.notes);

            // Da quanto si sta cantando questa nota? (serve a Humanize e al vibrato)
            let hop_seconds = self.hop as f32 / self.sample_rate;
            let reference = target.map_or(note, |t| t as f32);
            if target != self.current_target || !self.was_voiced {
                self.current_target = target;
                self.note_age = 0.0;
                self.vibrato_in.reset_to(note - reference);
                self.vibrato_out.reset_to(note - reference);
                if !self.was_voiced && settings.retune_ms >= 1.0 {
                    // Inizio di una nuova nota: si parte dall'intonazione naturale.
                    self.correction = 0.0;
                }
            } else {
                self.note_age += hop_seconds;
            }

            let desired = match target {
                Some(t) => {
                    let offset = t as f32 - note;
                    offset * settings.amount.clamp(0.0, 1.0) * flex_weight(offset.abs(), settings.flex_tune)
                }
                None => 0.0,
            };

            // Humanize: sulle note tenute (oltre ~0,15 s) la velocita' rallenta.
            let sustain = smoothstep(0.15, 0.6, self.note_age);
            let retune_ms = settings.retune_ms + settings.humanize.clamp(0.0, 1.0) * 400.0 * sustain;
            if retune_ms < 1.0 {
                self.correction = desired;
            } else {
                // Filtro "a un polo": si avvicina al bersaglio esponenzialmente,
                // arrivando a ~95% in `retune_ms` millisecondi.
                let time_constant = retune_ms * 0.001 / 3.0 * self.sample_rate;
                let alpha = 1.0 - (-(self.hop as f32) / time_constant).exp();
                self.correction += alpha * (desired - self.correction);
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
            self.last_period = period;
            report = PitchReport {
                voiced: true,
                input_note: note,
                output_note: note + shift,
                target_note: target.map_or(f32::NAN, |t| t as f32 + settings.transpose),
            };
        } else if self.was_voiced && (self.hold_left > 0 || suspicious_leap) {
            // Buco momentaneo (o salto sospetto): manteniamo l'ultima correzione.
            self.hold_left = self.hold_left.saturating_sub(self.hop);
            period = self.last_period;
            ratio = self.last_ratio;
            voiced = true;
            center = start as f64 + len as f64 * 0.5;
            report = PitchReport::SILENT;
        } else {
            self.was_voiced = false;
            self.snapper.reset();
            self.current_target = None;
            self.correction = 0.0;
            period = self.unvoiced_period;
            ratio = 1.0;
            voiced = false;
            center = start as f64 + len as f64 * 0.5;
            report = PitchReport::SILENT;
        }

        self.last_ratio = ratio;
        self.history[self.history_head] = Analysis { center, period, ratio, voiced, valid: true };
        self.history_head = (self.history_head + 1) % HISTORY;
        report
    }
}

/// Il pitch nel punto `t` dell'ingresso, interpolando tra le due analisi
/// piu' vicine (una prima e una dopo `t`).
fn lookup(history: &[Analysis; HISTORY], t: f64) -> Option<GrainPitch> {
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
    let as_pitch = |a: &Analysis| GrainPitch { period: a.period, ratio: a.ratio, voiced: a.voiced, formant: 1.0 };
    match (before, after) {
        (Some(b), Some(a)) if b.voiced && a.voiced => {
            let w = ((t - b.center) / (a.center - b.center).max(1e-9)) as f32;
            Some(GrainPitch {
                period: b.period + (a.period - b.period) * w,
                ratio: b.ratio + (a.ratio - b.ratio) * w,
                voiced: true,
                formant: 1.0,
            })
        }
        (Some(b), Some(a)) => Some(as_pitch(if t - b.center <= a.center - t { b } else { a })),
        (Some(only), None) | (None, Some(only)) => Some(as_pitch(only)),
        (None, None) => None,
    }
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
    fn latency_depends_on_voice_type() {
        let mut c = PitchCorrector::new(48000.0, 1);
        let alto = c.latency();
        c.set_voice(VoiceType::Soprano);
        let soprano = c.latency();
        c.set_voice(VoiceType::LowMale);
        let low = c.latency();
        assert!(soprano < alto && alto < low, "{soprano} {alto} {low}");
        // Anche nel caso peggiore restiamo sotto i 50 ms.
        assert!(low as f32 / 48000.0 < 0.05);
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
        let mut corrector = PitchCorrector::new(sr, 1);
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
        assert!(share > 0.85, "solo il {:.0}% delle finestre e' su una nota della scala", share * 100.0);
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
