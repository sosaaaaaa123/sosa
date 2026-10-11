//! I preset di fabbrica ("Factory"): impostazioni pronte per gli stili piu'
//! comuni. Un preset cambia solo il carattere della correzione (velocita',
//! Humanize, Flex Tune, vibrato, quantita', stile), mai tonalita', scala o
//! tipo di voce, che dipendono dalla canzone e dal cantante.

use crate::params::{SosaTuneParams, StyleChoice};

#[derive(Clone, Copy, Debug)]
pub struct Preset {
    pub name: &'static str,
    pub help: &'static str,
    pub retune_ms: f32,
    pub humanize: f32,
    pub flex_tune: f32,
    pub vibrato: f32,
    pub amount: f32,
    pub style: StyleChoice,
}

pub const BANK: &str = "Factory";

pub const FACTORY: [Preset; 9] = [
    Preset {
        name: "Default",
        help: "Correzione pulita e precisa, adatta a quasi tutte le voci.",
        retune_ms: 20.0,
        humanize: 0.0,
        flex_tune: 0.0,
        vibrato: 0.0,
        amount: 1.0,
        style: StyleChoice::Modern,
    },
    Preset {
        name: "Natural Vocal",
        help: "Si sente intonato ma non corretto: le note lunghe restano libere.",
        retune_ms: 80.0,
        humanize: 0.35,
        flex_tune: 0.0,
        vibrato: 0.0,
        amount: 1.0,
        style: StyleChoice::Modern,
    },
    Preset {
        name: "Pop Polish",
        help: "Intonazione precisa e brillante, come nelle produzioni pop.",
        retune_ms: 15.0,
        humanize: 0.25,
        flex_tune: 0.0,
        vibrato: 0.0,
        amount: 1.0,
        style: StyleChoice::Modern,
    },
    Preset {
        name: "Hard Tune",
        help: "L'effetto estremo: la voce salta da una nota all'altra.",
        retune_ms: 0.0,
        humanize: 0.0,
        flex_tune: 0.0,
        vibrato: 0.0,
        amount: 1.0,
        style: StyleChoice::Modern,
    },
    Preset {
        name: "Trap Lead",
        help: "Quasi robotico ma con passaggi appena morbidi.",
        retune_ms: 5.0,
        humanize: 0.0,
        flex_tune: 0.0,
        vibrato: 0.0,
        amount: 1.0,
        style: StyleChoice::Modern,
    },
    Preset {
        name: "R&B Smooth",
        help: "Correzione morbida che lascia scivolate e un po' di vibrato.",
        retune_ms: 40.0,
        humanize: 0.3,
        flex_tune: 0.2,
        vibrato: 0.2,
        amount: 1.0,
        style: StyleChoice::Modern,
    },
    Preset {
        name: "Soft Touch",
        help: "Sistema solo le stonature e lascia l'espressione.",
        retune_ms: 150.0,
        humanize: 0.6,
        flex_tune: 0.6,
        vibrato: 0.0,
        amount: 0.8,
        style: StyleChoice::Modern,
    },
    Preset {
        name: "Classic Robot",
        help: "Il suono degli autotune di una volta, a velocità zero.",
        retune_ms: 0.0,
        humanize: 0.0,
        flex_tune: 0.0,
        vibrato: 0.0,
        amount: 1.0,
        style: StyleChoice::Classic,
    },
    Preset {
        name: "Retro Glide",
        help: "Stile classico: ogni nota scivola sulla nota giusta.",
        retune_ms: 60.0,
        humanize: 0.1,
        flex_tune: 0.0,
        vibrato: 0.0,
        amount: 1.0,
        style: StyleChoice::Classic,
    },
];

impl Preset {
    /// I parametri attuali sono quelli del preset?
    pub fn matches(&self, params: &SosaTuneParams) -> bool {
        (params.retune_speed.value() - self.retune_ms).abs() < 0.5
            && (params.humanize.value() - self.humanize).abs() < 0.005
            && (params.flex_tune.value() - self.flex_tune).abs() < 0.005
            && (params.vibrato.value() - self.vibrato).abs() < 0.005
            && (params.amount.value() - self.amount).abs() < 0.005
            && params.style.value() == self.style
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_values_in_range() {
        for (i, a) in FACTORY.iter().enumerate() {
            assert!((0.0..=400.0).contains(&a.retune_ms), "{}", a.name);
            assert!((0.0..=1.0).contains(&a.humanize) && (0.0..=1.0).contains(&a.flex_tune), "{}", a.name);
            assert!((-1.0..=1.0).contains(&a.vibrato) && (0.0..=1.0).contains(&a.amount), "{}", a.name);
            for b in &FACTORY[i + 1..] {
                assert_ne!(a.name, b.name);
            }
        }
        // I valori iniziali del plugin sono il preset "Default".
        assert!(FACTORY[0].matches(&SosaTuneParams::default()));
    }
}
