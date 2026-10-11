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
    /// Riduzione di guadagno del compressore (dB, la piu' alta dell'ultimo blocco).
    gain_reduction_db: AtomicF32,
    /// Picco in ingresso e in uscita dell'ultimo blocco audio (0..1).
    input_peak: AtomicF32,
    output_peak: AtomicF32,
    /// Quanti blocchi audio ha elaborato il plugin: se non cresce, il DAW non
    /// sta mandando audio a SosaTune.
    blocks: AtomicUsize,
    /// Il DAW ha messo il plugin in bypass.
    bypassed: AtomicBool,
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
            gain_reduction_db: AtomicF32::new(0.0),
            input_peak: AtomicF32::new(0.0),
            output_peak: AtomicF32::new(0.0),
            blocks: AtomicUsize::new(0),
            bypassed: AtomicBool::new(false),
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

    pub fn set_gain_reduction(&self, db: f32) {
        self.gain_reduction_db.store(db, Ordering::Relaxed);
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

    pub fn gain_reduction(&self) -> f32 {
        self.gain_reduction_db.load(Ordering::Relaxed)
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
