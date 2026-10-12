//! # Il motore audio di Sosa Tune
//!
//! Questa libreria contiene tutta l'elaborazione del suono, separata dal plugin.
//! Il percorso del segnale, campione per campione, e' questo:
//!
//! ```text
//!  voce in ingresso
//!        │
//!        ├──► [yin]        "che nota sta cantando?"   → periodo dell'onda (es. 109 campioni = 440 Hz)
//!        │        │
//!        │        ▼
//!        │    [scale]      "qual e' la nota giusta della scala piu' vicina?"
//!        │        │
//!        │        ▼
//!        │    [corrector]  "di quanto devo spostarla, e con che velocita'?" → rapporto di pitch
//!        │        │
//!        ▼        ▼
//!     [vocoder] HQ: phase vocoder, sposta l'intonazione mantenendo il timbro (le formanti)
//!     [zerolat] Low Latency: testina a latenza zero per registrare
//!        │
//!        ▼
//!  voce corretta in uscita
//! ```
//!
//! Il punto di ingresso e' [`PitchCorrector`]: gli dai un campione alla volta e lui
//! restituisce il campione corretto.

pub mod chroma;
pub mod corrector;
pub mod fft;
pub mod scale;
pub mod synth;
pub mod vocoder;
pub mod wav;
pub mod yin;
pub mod zerolat;

pub use corrector::{CorrectionSettings, PitchCorrector, PitchReport};
pub use scale::{ScaleKind, VoiceType};

/// Numero massimo di canali gestiti (mono o stereo).
pub const MAX_CHANNELS: usize = 2;
