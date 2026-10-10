//! Dati condivisi tra il thread audio e l'interfaccia grafica.
//!
//! Il thread audio non deve mai aspettare (niente lock, niente allocazioni),
//! quindi usiamo solo variabili "atomiche": il thread audio scrive, la
//! grafica legge quando vuole. Qualche lettura a meta' aggiornamento e'
//! innocua: al massimo un punto del grafico e' leggermente sbagliato per un
//! fotogramma.

use intona_dsp::PitchReport;
use nih_plug::prelude::AtomicF32;
use std::sync::atomic::{AtomicUsize, Ordering};

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

    pub fn latency_ms(&self) -> f32 {
        self.latency_ms.load(Ordering::Relaxed)
    }

    /// Secondi tra un punto e il successivo.
    pub fn interval_s(&self) -> f32 {
        self.interval_s.load(Ordering::Relaxed)
    }

    /// Copia in `out` gli ultimi `count` punti, dal piu' vecchio al piu' nuovo.
    pub fn snapshot(&self, count: usize, out: &mut Vec<PitchPoint>) {
        out.clear();
        let written = self.written.load(Ordering::Acquire);
        let count = count.min(HISTORY_LEN - 1).min(written);
        for k in (1..=count).rev() {
            let index = (written - k) % HISTORY_LEN;
            out.push(PitchPoint {
                input: self.input[index].load(Ordering::Relaxed),
                output: self.output[index].load(Ordering::Relaxed),
                target: self.target[index].load(Ordering::Relaxed),
            });
        }
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
