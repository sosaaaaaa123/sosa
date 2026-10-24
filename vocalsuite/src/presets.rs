//! I preset di fabbrica. Un preset cambia il carattere della correzione e
//! gli effetti, mai tonalita', scala o tipo di voce (dipendono dalla canzone
//! e dal cantante).

use crate::params::{ChorusModeChoice, DelaySync, Stabilizer, StyleChoice, VocalSuiteParams};

#[derive(Clone, Copy, Debug)]
pub struct Preset {
    pub name: &'static str,
    /// La riga sotto il nome (il "carattere" del preset).
    pub tag: &'static str,
    pub retune_ms: f32,
    pub humanize: f32,
    pub flex_tune: f32,
    pub style: StyleChoice,
    pub stabilizer: Stabilizer,
    pub chorus: f32,
    pub formant: f32,
    pub reverb: f32,
    pub delay: f32,
    pub chorus_mode: ChorusModeChoice,
    pub chorus_blend: f32,
    pub delay_mode: i32,
    pub delay_sync: DelaySync,
    pub delay_intensity: f32,
    pub reverb_decay: f32,
}

const BASE: Preset = Preset {
    name: "Default",
    tag: "Celestial Clean",
    retune_ms: 20.0,
    humanize: 0.2,
    flex_tune: 0.0,
    style: StyleChoice::Modern,
    stabilizer: Stabilizer::Off,
    chorus: 25.0,
    formant: 50.0,
    reverb: 30.0,
    delay: 18.0,
    chorus_mode: ChorusModeChoice::I,
    chorus_blend: 0.4,
    delay_mode: 4,
    delay_sync: DelaySync::Sixteenth,
    delay_intensity: 0.3,
    reverb_decay: 2.2,
};

pub const FACTORY: [Preset; 10] = [
    BASE,
    Preset { name: "Natural Vocal", tag: "Transparent", retune_ms: 80.0, humanize: 0.35, chorus: 0.0, reverb: 20.0, delay: 0.0, ..BASE },
    Preset {
        name: "Pop Polish",
        tag: "Bright & Wide",
        retune_ms: 15.0,
        humanize: 0.25,
        chorus: 30.0,
        chorus_mode: ChorusModeChoice::II,
        reverb: 35.0,
        delay: 20.0,
        ..BASE
    },
    Preset { name: "Hard Tune", tag: "Robotic Lead", retune_ms: 0.0, humanize: 0.0, chorus: 15.0, reverb: 25.0, delay: 25.0, ..BASE },
    Preset {
        name: "Trap Lead",
        tag: "Dark Space",
        retune_ms: 5.0,
        humanize: 0.0,
        chorus: 20.0,
        chorus_blend: 0.7,
        formant: 46.0,
        reverb: 40.0,
        reverb_decay: 3.0,
        delay: 30.0,
        delay_mode: 2,
        delay_sync: DelaySync::Sixteenth,
        delay_intensity: 0.4,
        ..BASE
    },
    Preset {
        name: "R&B Smooth",
        tag: "Silk",
        retune_ms: 40.0,
        humanize: 0.3,
        flex_tune: 0.2,
        stabilizer: Stabilizer::Light,
        chorus: 35.0,
        reverb: 45.0,
        delay: 20.0,
        ..BASE
    },
    Preset { name: "Soft Touch", tag: "Natural Air", retune_ms: 150.0, humanize: 0.6, flex_tune: 0.6, chorus: 0.0, reverb: 25.0, delay: 0.0, ..BASE },
    Preset {
        name: "Juno Dream",
        tag: "Analog Chorus",
        retune_ms: 25.0,
        chorus: 60.0,
        chorus_mode: ChorusModeChoice::II,
        chorus_blend: 0.0,
        reverb: 40.0,
        delay: 15.0,
        ..BASE
    },
    Preset {
        name: "Space Echo Dub",
        tag: "Tape Echo",
        retune_ms: 30.0,
        chorus: 10.0,
        reverb: 20.0,
        delay: 55.0,
        delay_mode: 9,
        delay_sync: DelaySync::Eighth,
        delay_intensity: 0.6,
        ..BASE
    },
    Preset {
        name: "Celestial Choir",
        tag: "Huge Space",
        retune_ms: 20.0,
        humanize: 0.3,
        stabilizer: Stabilizer::Medium,
        chorus: 45.0,
        chorus_blend: 0.6,
        reverb: 70.0,
        reverb_decay: 5.5,
        delay: 30.0,
        delay_mode: 11,
        ..BASE
    },
];

impl Preset {
    /// I parametri attuali sono quelli del preset?
    pub fn matches(&self, p: &VocalSuiteParams) -> bool {
        let near = |a: f32, b: f32, tol: f32| (a - b).abs() <= tol;
        near(p.retune_speed.value(), self.retune_ms, 0.5)
            && near(p.humanize.value(), self.humanize, 0.005)
            && near(p.flex_tune.value(), self.flex_tune, 0.005)
            && p.style.value() == self.style
            && p.stabilizer.value() == self.stabilizer
            && near(p.chorus.value(), self.chorus, 0.05)
            && near(p.formant.value(), self.formant, 0.05)
            && near(p.reverb.value(), self.reverb, 0.05)
            && near(p.delay.value(), self.delay, 0.05)
            && p.chorus_mode.value() == self.chorus_mode
            && near(p.chorus_blend.value(), self.chorus_blend, 0.005)
            && p.delay_mode.value() == self.delay_mode
            && p.delay_sync.value() == self.delay_sync
            && near(p.delay_intensity.value(), self.delay_intensity, 0.005)
            && near(p.reverb_decay.value(), self.reverb_decay, 0.01)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_values_in_range() {
        for (i, a) in FACTORY.iter().enumerate() {
            assert!((0.0..=400.0).contains(&a.retune_ms), "{}", a.name);
            for v in [a.chorus, a.formant, a.reverb, a.delay] {
                assert!((0.0..=100.0).contains(&v), "{}", a.name);
            }
            assert!((1..=12).contains(&a.delay_mode));
            for b in &FACTORY[i + 1..] {
                assert_ne!(a.name, b.name);
            }
        }
        let p = VocalSuiteParams::with_defaults(&std::collections::HashMap::new());
        assert!(FACTORY[0].matches(&p), "i valori iniziali sono il preset Default");
    }
}
