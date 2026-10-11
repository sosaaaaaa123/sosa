//! Dati condivisi tra il thread audio e l'interfaccia grafica.
//!
//! Il thread audio non deve mai aspettare (niente lock, niente allocazioni),
//! quindi usiamo solo variabili "atomiche": il thread audio scrive, la
//! grafica legge quando vuole. Qualche lettura a meta' aggiornamento e'
//! innocua: al massimo un punto del grafico e' leggermente sbagliato per un
//! fotogramma.

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
    }

    /// Ricomincia a contare da zero (quando si accende "AutoKey Listen").
    pub fn reset_notes(&self) {
        for slot in &self.note_seconds {
            slot.store(0.0, Ordering::Relaxed);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

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
