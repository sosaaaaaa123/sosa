//! Dati condivisi tra il thread audio e l'interfaccia grafica.
//!
//! Il thread audio non deve mai aspettare (niente lock, niente allocazioni),
//! quindi usiamo solo variabili "atomiche": il thread audio scrive, la
//! grafica legge quando vuole. Qualche lettura a meta' aggiornamento e'
//! innocua: al massimo un punto del grafico e' leggermente sbagliato per un
//! fotogramma.

use sosatune_dsp::chroma::key_from_chroma;
use sosatune_dsp::PitchReport;
use nih_plug::prelude::AtomicF32;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Quante analisi teniamo per il grafico (~5 secondi a una analisi ogni 5 ms).
pub const HISTORY_LEN: usize = 1024;

/// Un punto del grafico. `NAN` = nessuna nota in quel momento.
#[derive(Clone, Copy, Debug)]
pub struct PitchPoint {
    pub input: f32,
    pub output: f32,
    pub target: f32,
}

pub struct PitchDisplay {
    input: Vec<AtomicF32>,
    output: Vec<AtomicF32>,
    target: Vec<AtomicF32>,
    /// Numero totale di punti scritti finora.
    written: AtomicUsize,
    latency_ms: AtomicF32,
    interval_s: AtomicF32,
    /// Picco in ingresso e in uscita dell'ultimo blocco audio (0..1).
    input_peak: AtomicF32,
    output_peak: AtomicF32,
    /// Quanti blocchi audio ha elaborato il plugin: se non cresce, il DAW non
    /// sta mandando audio a SosaTune.
    blocks: AtomicUsize,
    /// Il DAW ha messo il plugin in bypass.
    bypassed: AtomicBool,
    /// Quanto tempo (s) e' stata cantata ciascuna delle 12 note: serve a
    /// riconoscere la tonalita' della canzone ("AutoKey Listen").
    note_seconds: [AtomicF32; 12],
    /// Quanto tempo (s) e' stata cantata ciascuna nota MIDI da `HIST_LOW` in
    /// su (con l'ottava): serve a riconoscere il tipo di voce.
    note_histogram: [AtomicF32; HIST_LEN],
    /// Cromagramma (energia delle 12 note in tutto lo spettro, anche con
    /// piu' strumenti insieme) dell'ingresso e del sidechain, e secondi di
    /// musica ascoltati: AutoKey anche su accordi e sulla base.
    chroma_main: [AtomicF32; 12],
    chroma_side: [AtomicF32; 12],
    music_main: AtomicF32,
    music_side: AtomicF32,
    /// L'interfaccia chiede al thread audio di ricominciare il cromagramma.
    chroma_reset: AtomicBool,
}

/// Prima nota MIDI dell'istogramma (Do1) e numero di note (fino a Mi6).
pub const HIST_LOW: i32 = 24;
pub const HIST_LEN: usize = 65;

/// Da dove arriva la tonalita' riconosciuta da AutoKey.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySource {
    /// La voce (una nota alla volta).
    Voice,
    /// L'audio della traccia, anche polifonico (accordi, una base).
    Polyphonic,
    /// L'ingresso sidechain (per esempio la base della canzone).
    Sidechain,
}

impl Default for PitchDisplay {
    fn default() -> Self {
        let make = || (0..HISTORY_LEN).map(|_| AtomicF32::new(f32::NAN)).collect();
        Self {
            input: make(),
            output: make(),
            target: make(),
            written: AtomicUsize::new(0),
            latency_ms: AtomicF32::new(0.0),
            interval_s: AtomicF32::new(0.005),
            input_peak: AtomicF32::new(0.0),
            output_peak: AtomicF32::new(0.0),
            blocks: AtomicUsize::new(0),
            bypassed: AtomicBool::new(false),
            note_seconds: std::array::from_fn(|_| AtomicF32::new(0.0)),
            note_histogram: std::array::from_fn(|_| AtomicF32::new(0.0)),
            chroma_main: std::array::from_fn(|_| AtomicF32::new(0.0)),
            chroma_side: std::array::from_fn(|_| AtomicF32::new(0.0)),
            music_main: AtomicF32::new(0.0),
            music_side: AtomicF32::new(0.0),
            chroma_reset: AtomicBool::new(false),
        }
    }
}

impl PitchDisplay {
    /// Chiamata dal thread audio dopo ogni analisi.
    pub fn push(&self, report: &PitchReport) {
        let index = self.written.load(Ordering::Relaxed) % HISTORY_LEN;
        self.input[index].store(report.input_note, Ordering::Relaxed);
        self.output[index].store(report.output_note, Ordering::Relaxed);
        self.target[index].store(report.target_note, Ordering::Relaxed);
        self.written.fetch_add(1, Ordering::Release);
    }

    pub fn set_timing(&self, latency_ms: f32, interval_s: f32) {
        self.latency_ms.store(latency_ms, Ordering::Relaxed);
        self.interval_s.store(interval_s, Ordering::Relaxed);
    }

    /// Chiamata dal thread audio a ogni analisi cantata: conta quanto tempo
    /// si sta su ciascuna nota. Le note cantate "in mezzo" contano meno.
    pub fn add_note(&self, note: f32, seconds: f32) {
        if !note.is_finite() {
            return;
        }
        let nearest = note.round();
        let off = (note - nearest) / 0.25;
        let weight = (-off * off).exp();
        let pc = (nearest as i32).rem_euclid(12) as usize;
        let slot = &self.note_seconds[pc];
        slot.store(slot.load(Ordering::Relaxed) + seconds * weight, Ordering::Relaxed);
        let bin = nearest as i32 - HIST_LOW;
        if (0..HIST_LEN as i32).contains(&bin) {
            let slot = &self.note_histogram[bin as usize];
            slot.store(slot.load(Ordering::Relaxed) + seconds, Ordering::Relaxed);
        }
    }

    /// Ricomincia a contare da zero (quando si accende "AutoKey Listen"):
    /// note cantate e cromagrammi.
    pub fn reset_notes(&self) {
        for slot in &self.note_seconds {
            slot.store(0.0, Ordering::Relaxed);
        }
        for slot in self.chroma_main.iter().chain(&self.chroma_side) {
            slot.store(0.0, Ordering::Relaxed);
        }
        self.music_main.store(0.0, Ordering::Relaxed);
        self.music_side.store(0.0, Ordering::Relaxed);
        self.chroma_reset.store(true, Ordering::Release);
    }

    /// Il thread audio: va ricominciato il cromagramma? (Una volta sola.)
    pub fn take_chroma_reset(&self) -> bool {
        self.chroma_reset.swap(false, Ordering::AcqRel)
    }

    /// Il thread audio pubblica i cromagrammi (ingresso e sidechain).
    pub fn set_chroma(&self, main: &[f32; 12], main_seconds: f32, side: &[f32; 12], side_seconds: f32) {
        for i in 0..12 {
            self.chroma_main[i].store(main[i], Ordering::Relaxed);
            self.chroma_side[i].store(side[i], Ordering::Relaxed);
        }
        self.music_main.store(main_seconds, Ordering::Relaxed);
        self.music_side.store(side_seconds, Ordering::Relaxed);
    }

    /// Ricomincia l'istogramma delle note (riconoscimento del tipo di voce).
    pub fn reset_histogram(&self) {
        for slot in &self.note_histogram {
            slot.store(0.0, Ordering::Relaxed);
        }
    }

    /// Secondi cantati per ciascuna nota MIDI (da `HIST_LOW`).
    pub fn note_histogram(&self) -> [f32; HIST_LEN] {
        std::array::from_fn(|i| self.note_histogram[i].load(Ordering::Relaxed))
    }

    /// La tonalita' secondo tutte le fonti disponibili (vedi [`choose_key`]).
    pub fn detect_key_all(&self) -> Option<(usize, bool, f32, KeySource)> {
        let load = |a: &[AtomicF32; 12]| -> [f32; 12] { std::array::from_fn(|i| a[i].load(Ordering::Relaxed)) };
        choose_key(
            &self.note_profile(),
            &load(&self.chroma_main),
            self.music_main.load(Ordering::Relaxed),
            &load(&self.chroma_side),
            self.music_side.load(Ordering::Relaxed),
        )
    }

    /// Secondi cantati su ciascuna nota (Do, Do#, ... Si).
    pub fn note_profile(&self) -> [f32; 12] {
        std::array::from_fn(|i| self.note_seconds[i].load(Ordering::Relaxed))
    }

    /// Chiamata dal thread audio alla fine di ogni blocco.
    pub fn set_block(&self, input_peak: f32, output_peak: f32, bypassed: bool) {
        self.input_peak.store(input_peak, Ordering::Relaxed);
        self.output_peak.store(output_peak, Ordering::Relaxed);
        self.bypassed.store(bypassed, Ordering::Relaxed);
        self.blocks.fetch_add(1, Ordering::Release);
    }

    /// (picco in ingresso, picco in uscita, blocchi elaborati, bypass).
    pub fn block_status(&self) -> (f32, f32, usize, bool) {
        (
            self.input_peak.load(Ordering::Relaxed),
            self.output_peak.load(Ordering::Relaxed),
            self.blocks.load(Ordering::Acquire),
            self.bypassed.load(Ordering::Relaxed),
        )
    }

    pub fn latency_ms(&self) -> f32 {
        self.latency_ms.load(Ordering::Relaxed)
    }

    /// Secondi tra un punto e il successivo.
    pub fn interval_s(&self) -> f32 {
        self.interval_s.load(Ordering::Relaxed)
    }

    /// L'ultimo punto scritto, se c'e'.
    pub fn latest(&self) -> Option<PitchPoint> {
        let written = self.written.load(Ordering::Acquire);
        if written == 0 {
            return None;
        }
        let index = (written - 1) % HISTORY_LEN;
        Some(PitchPoint {
            input: self.input[index].load(Ordering::Relaxed),
            output: self.output[index].load(Ordering::Relaxed),
            target: self.target[index].load(Ordering::Relaxed),
        })
    }
}

/// Riconosce la tonalita' dal tempo cantato su ciascuna nota.
///
/// Una voce spesso canta poco la tonica (in Sol minore magari quasi solo Re,
/// Mib e Do), quindi non basta guardare la nota piu' cantata: si sceglie la
/// scala (maggiore o minore naturale) che contiene piu' tempo cantato, e tra
/// scale con le stesse note (relative, es. Sol minore / Sib maggiore) quella
/// la cui tonica e quinta sono piu' presenti.
/// Restituisce (tonica 0..11, minore?, affidabilita' 0..1), oppure `None` se
/// c'e' troppo poco cantato (meno di 3 secondi).
pub fn detect_key(profile: &[f32; 12]) -> Option<(usize, bool, f32)> {
    const MAJOR: [usize; 7] = [0, 2, 4, 5, 7, 9, 11];
    const MINOR: [usize; 7] = [0, 2, 3, 5, 7, 8, 10];
    let total: f32 = profile.iter().sum();
    if total < 3.0 {
        return None;
    }
    let share = |pc: usize| profile[pc % 12] / total;
    let mut best = (0usize, false, f32::MIN);
    let mut second = f32::MIN;
    for tonic in 0..12 {
        for (minor, scale) in [(false, &MAJOR), (true, &MINOR)] {
            let fit: f32 = scale.iter().map(|&i| share(tonic + i)).sum();
            let score = fit + 0.15 * (share(tonic) + 0.5 * share(tonic + 7));
            if score > best.2 {
                second = best.2;
                best = (tonic, minor, score);
            } else if score > second {
                second = score;
            }
        }
    }
    // Affidabile se la migliore stacca bene la seconda.
    let confidence = ((best.2 - second) / 0.04).clamp(0.0, 1.0);
    Some((best.0, best.1, confidence))
}

/// Sceglie la tonalita' tra le fonti di AutoKey:
/// 1. il **sidechain** (la base della canzone), se ha suonato per almeno 4 s:
///    e' la fonte piu' sicura;
/// 2. l'**audio della traccia letto in modo polifonico**, quando non e' una
///    voce sola (meno di meta' del tempo con una nota singola pulita): un
///    pianoforte, una chitarra, la base messa sulla traccia;
/// 3. altrimenti la **voce**, nota per nota ([`detect_key`]).
pub fn choose_key(
    voice: &[f32; 12],
    main: &[f32; 12],
    main_seconds: f32,
    side: &[f32; 12],
    side_seconds: f32,
) -> Option<(usize, bool, f32, KeySource)> {
    const MIN_MUSIC_S: f32 = 4.0;
    if side_seconds >= MIN_MUSIC_S {
        if let Some((tonic, minor, confidence)) = key_from_chroma(side) {
            return Some((tonic, minor, confidence, KeySource::Sidechain));
        }
    }
    let sung: f32 = voice.iter().sum();
    if main_seconds >= MIN_MUSIC_S && sung < 0.5 * main_seconds {
        if let Some((tonic, minor, confidence)) = key_from_chroma(main) {
            return Some((tonic, minor, confidence, KeySource::Polyphonic));
        }
    }
    detect_key(voice).map(|(tonic, minor, confidence)| (tonic, minor, confidence, KeySource::Voice))
}

/// Il tipo di voce dalle note cantate (istogramma per nota MIDI da
/// `HIST_LOW`): 0 = Soprano, 1 = Alto/Tenor, 2 = Low Male. Guarda le note
/// piu' gravi cantate davvero (il 10% piu' basso del tempo), cosi' una nota
/// sbagliata isolata non conta. `None` se si e' cantato meno di 4 secondi.
pub fn detect_voice_range(histogram: &[f32; HIST_LEN]) -> Option<usize> {
    let total: f32 = histogram.iter().sum();
    if total < 4.0 {
        return None;
    }
    let mut acc = 0.0;
    let mut low_note = HIST_LOW;
    for (i, &t) in histogram.iter().enumerate() {
        acc += t;
        if acc >= 0.1 * total {
            low_note = HIST_LOW + i as i32;
            break;
        }
    }
    // Soprano riconosce da 150 Hz (Re3), Alto/Tenor da 75 Hz (Re2).
    Some(if low_note >= 52 {
        0
    } else if low_note >= 41 {
        1
    } else {
        2
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_best_key_source() {
        // Voce in Sol minore (vedi sotto) e base in Do maggiore sul sidechain:
        // vince la base.
        let voice = [11.0, 7.0, 42.0, 15.0, 3.0, 8.0, 0.0, 1.0, 0.0, 3.0, 5.0, 4.0];
        let c_major = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
        let none = [0.0; 12];
        let k = choose_key(&voice, &none, 0.0, &c_major, 10.0).unwrap();
        assert_eq!((k.0, k.1, k.3), (0, false, KeySource::Sidechain));
        // Senza sidechain e con la voce che canta quasi sempre: la voce.
        let k = choose_key(&voice, &c_major, 80.0, &none, 0.0).unwrap();
        assert_eq!((k.0, k.1, k.3), (7, true, KeySource::Voice));
        // Traccia polifonica (poche note singole pulite): il cromagramma.
        let little = [0.2; 12];
        let k = choose_key(&little, &c_major, 20.0, &none, 0.0).unwrap();
        assert_eq!((k.0, k.1, k.3), (0, false, KeySource::Polyphonic));
    }

    #[test]
    fn recognises_the_voice_range() {
        let sing = |from: i32, to: i32| {
            let mut h = [0.0; HIST_LEN];
            for n in from..=to {
                h[(n - HIST_LOW) as usize] = 1.0;
            }
            h
        };
        assert_eq!(detect_voice_range(&sing(57, 72)), Some(0), "voce femminile La3-Do5");
        assert_eq!(detect_voice_range(&sing(45, 60)), Some(1), "tenore/baritono La2-Do4");
        assert_eq!(detect_voice_range(&sing(36, 50)), Some(2), "basso Do2-Re3");
        assert!(detect_voice_range(&[0.0; HIST_LEN]).is_none());
    }

    #[test]
    fn detects_the_key_of_a_real_vocal() {
        // Tempo cantato per nota misurato su una voce vera di un brano in Sol
        // minore: la voce sta quasi sempre su Re, Mib e Do, la tonica Sol
        // quasi mai.
        let profile = [11.0, 7.0, 42.0, 15.0, 3.0, 8.0, 0.0, 1.0, 0.0, 3.0, 5.0, 4.0];
        let (tonic, minor, _) = detect_key(&profile).expect("abbastanza cantato");
        assert_eq!((tonic, minor), (7, true), "Sol minore");
        // Una scala maggiore cantata tutta (Do maggiore, tonica piu' presente).
        let c_major = [20.0, 0.0, 10.0, 0.0, 12.0, 8.0, 0.0, 15.0, 0.0, 8.0, 0.0, 6.0];
        assert_eq!(detect_key(&c_major).map(|k| (k.0, k.1)), Some((0, false)), "Do maggiore");
        assert!(detect_key(&[0.1; 12]).is_none(), "troppo poco cantato");
    }

    #[test]
    fn note_profile_counts_sung_time() {
        let display = PitchDisplay::default();
        for _ in 0..200 {
            display.add_note(62.02, 0.005); // Re, intonato
            display.add_note(66.5, 0.005); // a meta' tra Fa# e Sol: conta poco
        }
        let p = display.note_profile();
        assert!((p[2] - 1.0).abs() < 0.05, "Re: {}", p[2]);
        assert!(p[6] + p[7] < 0.1, "note in mezzo: {} {}", p[6], p[7]);
    }
}
