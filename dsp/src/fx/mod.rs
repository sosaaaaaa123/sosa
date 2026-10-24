//! # Effetti di SR Vocal Suite
//!
//! Dopo la correzione dell'intonazione la voce passa per tre effetti:
//!
//! ```text
//!  voce corretta ──► [chorus] ──┬────────────────────────────────► + ──► uscita
//!                  Juno BBD +   ├──► [space_echo] (mandata) ──────► +
//!                  multi-voce   │          │ (meta' dell'eco)
//!                  stile FL     └──────────┴──► [reverb] (mandata) ► +
//! ```
//!
//! - [`chorus`]: il chorus a BBD del Juno-60 (modi I, II e I+II) mescolato
//!   a un chorus digitale a piu' voci nello stile del Chorus di FL Studio;
//! - [`space_echo`]: il tape echo a tre testine stile Space Echo RE-201
//!   (selettore a 12 posizioni, nastro con wow e flutter, saturazione,
//!   rigenerazione, toni sulle ripetizioni e riverbero a molla);
//! - [`reverb`]: un riverbero pulito (rete di ritardi con retroazione, FDN)
//!   per lo spazio.
//!
//! Tutti gli effetti sono "senza fruscio": nessun rumore aggiunto.
//! Nessuno alloca memoria o usa lock durante l'elaborazione.

pub mod chorus;
pub mod reverb;
pub mod space_echo;
mod util;

pub use chorus::{Chorus, ChorusMode, ChorusSettings};
pub use reverb::{Reverb, ReverbSettings};
pub use space_echo::{SpaceEcho, SpaceEchoSettings};

/// Da 0..100 (manopola) a guadagno lineare di una mandata: -36 dB appena
/// aperta, 0 dB al massimo, con un attacco morbido sotto il 3%.
pub fn send_gain(knob: f32) -> f32 {
    let v = (knob / 100.0).clamp(0.0, 1.0);
    if v <= 0.0 {
        return 0.0;
    }
    let db = -36.0 + 36.0 * v.sqrt();
    let fade = (v / 0.03).min(1.0);
    10f32.powf(db / 20.0) * fade
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_curve() {
        assert_eq!(send_gain(0.0), 0.0);
        assert!((send_gain(100.0) - 1.0).abs() < 1e-6);
        let g27 = 20.0 * send_gain(27.0).log10();
        assert!((-19.0..-15.0).contains(&g27), "{g27}");
        let mut last = 0.0;
        for k in 0..=100 {
            let g = send_gain(k as f32);
            assert!(g >= last);
            last = g;
        }
    }
}
