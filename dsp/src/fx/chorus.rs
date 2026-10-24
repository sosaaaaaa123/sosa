//! # Chorus: Juno-60 (BBD analogico) + chorus digitale stile FL Studio
//!
//! Due chorus in parallelo, mescolati con `blend`:
//!
//! **Juno** — il chorus a BBD del Roland Juno-60: due linee di ritardo
//! analogiche (MN3009) mosse da un LFO triangolare, in controfase tra
//! sinistra e destra (da una voce mono nasce lo stereo). Valori della
//! macchina:
//!
//! | Modo  | LFO      | Ritardo         |
//! |-------|----------|-----------------|
//! | I     | 0,513 Hz | 1,66 - 5,35 ms  |
//! | II    | 0,863 Hz | 1,66 - 5,35 ms  |
//! | I+II  | 9,75 Hz  | 3,3 - 3,7 ms    |
//!
//! Il segnale nei BBD passa per i filtri anti-aliasing (morbidi, ~8-9 kHz)
//! e una leggera saturazione: il chorus del Juno e' caldo e un po' scuro.
//! Il fruscio dei BBD non e' modellato (effetto pulito).
//!
//! **FL** — il chorus digitale a piu' voci nello stile del Chorus di FL
//! Studio: tre voci per lato con ritardi diversi (7-20 ms), ciascuna mossa
//! da un LFO sinusoidale con la sua velocita' (piu' un LFO comune lento),
//! aperte nello stereo. Brillante e largo, senza filtri.

use super::util::{Biquad, BiquadKind, DelayLine, Lfo, Smoothed, soft_clip};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChorusMode {
    I,
    II,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChorusSettings {
    /// Quantita' di chorus 0..1 (1 = wet allo stesso livello del dry, come il Juno).
    pub mix: f32,
    pub mode: ChorusMode,
    /// 0 = solo Juno, 1 = solo FL.
    pub blend: f32,
    /// Moltiplicatore della velocita' degli LFO (1 = valori originali).
    pub rate: f32,
}

impl Default for ChorusSettings {
    fn default() -> Self {
        Self { mix: 0.35, mode: ChorusMode::I, blend: 0.4, rate: 1.0 }
    }
}

impl ChorusMode {
    /// (frequenza LFO, ritardo minimo, ritardo massimo) in Hz e secondi.
    fn juno(self) -> (f32, f32, f32) {
        match self {
            ChorusMode::I => (0.513, 0.00166, 0.00535),
            ChorusMode::II => (0.863, 0.00166, 0.00535),
            ChorusMode::Both => (9.75, 0.0033, 0.0037),
        }
    }
}

const FL_VOICES: usize = 3;
const FL_BASE_MS: [f32; FL_VOICES] = [7.3, 11.6, 16.1];
const FL_DEPTH_MS: [f32; FL_VOICES] = [2.4, 2.9, 3.4];
const FL_RATE: [f32; FL_VOICES] = [0.31, 0.47, 0.73];

pub struct Chorus {
    sr: f32,
    bbd: [DelayLine; 2],
    bbd_pre: [Biquad; 2],
    bbd_post: [[Biquad; 2]; 2],
    juno_lfo: Lfo,
    /// Le tre grandezze del modo, che scorrono quando si cambia modo.
    juno_rate: Smoothed,
    juno_min: Smoothed,
    juno_max: Smoothed,
    tri_smooth: Smoothed,
    fl: [DelayLine; 2],
    fl_lfo: [[Lfo; FL_VOICES]; 2],
    fl_slow: Lfo,
    mix: Smoothed,
    blend: Smoothed,
}

impl Chorus {
    pub fn new(sr: f32) -> Self {
        let max = (0.03 * sr) as usize + 8;
        let (r, lo, hi) = ChorusMode::I.juno();
        let lp = |f: f32, q: f32| Biquad::new(BiquadKind::Lowpass, f, q, sr);
        Self {
            sr,
            bbd: [DelayLine::new(max), DelayLine::new(max)],
            bbd_pre: [lp(9000.0, 0.7), lp(9000.0, 0.7)],
            bbd_post: [[lp(8200.0, 0.54), lp(10500.0, 1.3)], [lp(8200.0, 0.54), lp(10500.0, 1.3)]],
            juno_lfo: Lfo::new(0.0),
            juno_rate: Smoothed::new(r, 0.05, sr),
            juno_min: Smoothed::new(lo * sr, 0.05, sr),
            juno_max: Smoothed::new(hi * sr, 0.05, sr),
            tri_smooth: Smoothed::new(0.0, 0.004, sr),
            fl: [DelayLine::new(max), DelayLine::new(max)],
            fl_lfo: [
                [Lfo::new(0.0), Lfo::new(0.33), Lfo::new(0.66)],
                [Lfo::new(0.5), Lfo::new(0.83), Lfo::new(0.16)],
            ],
            fl_slow: Lfo::new(0.25),
            mix: Smoothed::new(0.0, 0.02, sr),
            blend: Smoothed::new(0.4, 0.02, sr),
        }
    }

    pub fn reset(&mut self) {
        for d in self.bbd.iter_mut().chain(self.fl.iter_mut()) {
            d.clear();
        }
        for f in self.bbd_pre.iter_mut().chain(self.bbd_post.iter_mut().flatten()) {
            f.reset();
        }
    }

    /// Elabora un campione (L, R) e restituisce (L, R) con il chorus.
    #[inline]
    pub fn process(&mut self, l: f32, r: f32, s: &ChorusSettings) -> (f32, f32) {
        let mix = self.mix.next(s.mix.clamp(0.0, 1.0));
        let blend = self.blend.next(s.blend.clamp(0.0, 1.0));
        let mono = 0.5 * (l + r);
        let rate_k = s.rate.clamp(0.1, 4.0);

        // --- Juno: un BBD per lato, LFO triangolare in controfase.
        let (rate, lo, hi) = s.mode.juno();
        let rate = self.juno_rate.next(rate * rate_k);
        let lo = self.juno_min.next(lo * self.sr);
        let hi = self.juno_max.next(hi * self.sr);
        let p = self.juno_lfo.advance(rate, self.sr);
        // Il triangolo del Juno ha gli spigoli un po' arrotondati.
        let tri = self.tri_smooth.next(Lfo::triangle(p));
        let center = 0.5 * (lo + hi);
        let depth = 0.5 * (hi - lo);
        let mut juno = [0.0f32; 2];
        for (k, out) in juno.iter_mut().enumerate() {
            let x = self.bbd_pre[k].process(mono);
            // Nel BBD entra il segnale gia' un po' saturato (compander).
            self.bbd[k].write(soft_clip(x * 1.2) / 1.2);
            let lfo = if k == 0 { tri } else { -tri };
            let y = self.bbd[k].read(center + depth * lfo);
            let y = self.bbd_post[k][0].process(y);
            *out = self.bbd_post[k][1].process(y);
        }

        // --- FL: tre voci per lato.
        let slow = Lfo::sine(self.fl_slow.advance(0.11 * rate_k, self.sr));
        let mut fl = [0.0f32; 2];
        let inputs = [l, r];
        for k in 0..2 {
            self.fl[k].write(inputs[k]);
            let mut acc = 0.0;
            for v in 0..FL_VOICES {
                let ph = self.fl_lfo[k][v].advance(FL_RATE[v] * rate_k, self.sr);
                let m = 0.8 * Lfo::sine(ph) + 0.2 * slow;
                let d = (FL_BASE_MS[v] + FL_DEPTH_MS[v] * m) * 0.001 * self.sr;
                // Le voci si aprono: la prima a sinistra, la terza a destra.
                let pan = [1.0, 0.7, 0.4][if k == 0 { v } else { FL_VOICES - 1 - v }];
                acc += self.fl[k].read(d) * pan;
            }
            fl[k] = acc / 2.1;
        }

        let wet_l = juno[0] * (1.0 - blend) + fl[0] * blend;
        let wet_r = juno[1] * (1.0 - blend) + fl[1] * blend;
        // Il Juno somma dry e wet allo stesso livello: per non alzare il
        // volume si compensa (fino a -3 dB al massimo).
        let comp = 1.0 / (1.0 + 0.41 * mix);
        ((l + wet_l * mix) * comp, (r + wet_r * mix) * comp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_mix_is_transparent() {
        let sr = 48000.0;
        let mut c = Chorus::new(sr);
        let s = ChorusSettings { mix: 0.0, ..Default::default() };
        // Lascia scendere il mix iniziale.
        for i in 0..4800 {
            c.process(0.0, 0.0, &s);
            let _ = i;
        }
        for i in 0..2000 {
            let x = (i as f32 * 0.03).sin() * 0.5;
            let (l, r) = c.process(x, x, &s);
            assert!((l - x).abs() < 1e-4 && (r - x).abs() < 1e-4);
        }
    }

    #[test]
    fn makes_stereo_from_mono_and_stays_bounded() {
        let sr = 44100.0;
        for mode in [ChorusMode::I, ChorusMode::II, ChorusMode::Both] {
            for blend in [0.0, 0.5, 1.0] {
                let mut c = Chorus::new(sr);
                let s = ChorusSettings { mix: 1.0, mode, blend, rate: 1.0 };
                let mut diff = 0.0f32;
                let mut peak = 0.0f32;
                for i in 0..(sr as usize * 2) {
                    let x = (i as f32 * 440.0 * std::f32::consts::TAU / sr).sin() * 0.7;
                    let (l, r) = c.process(x, x, &s);
                    assert!(l.is_finite() && r.is_finite());
                    diff += (l - r).abs();
                    peak = peak.max(l.abs()).max(r.abs());
                }
                assert!(diff / (sr * 2.0) > 0.01, "{mode:?} {blend}: niente stereo");
                assert!(peak < 1.2, "{mode:?} {blend}: picco {peak}");
            }
        }
    }
}
