//! # Il motore audio di SosaTune
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
//!     [psola]   sposta l'intonazione mantenendo il timbro (le formanti) della voce
//!        │
//!        ▼
//!  voce corretta in uscita
//! ```
//!
//! Il punto di ingresso e' [`PitchCorrector`]: gli dai un campione alla volta e lui
//! restituisce il campione corretto.
//!
//! Dopo la correzione c'e' [`TubeCompressor`], un compressore ottico a valvola
//! (vedi `compressor.rs`) con saturazione sovracampionata.

pub mod compressor;
pub mod corrector;
pub mod fft;
pub mod oversampling;
pub mod psola;
pub mod scale;
pub mod synth;
pub mod wav;
pub mod yin;

pub use compressor::{CompMode, CompressorSettings, TubeCompressor};
pub use corrector::{CorrectionSettings, PitchCorrector, PitchReport};
pub use scale::{ScaleKind, VoiceType};

/// Numero massimo di canali gestiti (mono o stereo).
pub const MAX_CHANNELS: usize = 2;
