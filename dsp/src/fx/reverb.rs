//! # Riverbero (REVERB · SPACE)
//!
//! Un riverbero pulito per la voce: pre-delay, quattro diffusori in serie
//! (le prime riflessioni si "spalmano" subito, niente eco a pallini) e una
//! rete di 8 linee di ritardo con retroazione (FDN, matrice di Householder).
//! Ogni linea ha il suo smorzamento sugli acuti (le code si scuriscono come
//! in una sala vera) e si muove pochissimo nel tempo, cosi' la coda non
//! "suona metallica". Il tempo di decadimento e' quello impostato (RT60)
//! a tutte le frequenze basse e medie. Nessun rumore aggiunto.

use super::util::{Biquad, BiquadKind, DelayLine, Lfo, OnePole, Smoothed};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReverbSettings {
    /// Tempo di decadimento (RT60) in secondi.
    pub decay_s: f32,
    /// Pre-delay in millisecondi.
    pub predelay_ms: f32,
    /// Smorzamento degli acuti 0..1 (1 = coda scura).
    pub damping: f32,
    /// Taglio dei bassi sulla coda (Hz).
    pub low_cut_hz: f32,
    /// Ampiezza stereo 0..1.
    pub width: f32,
}

impl Default for ReverbSettings {
    fn default() -> Self {
        Self { decay_s: 2.2, predelay_ms: 24.0, damping: 0.45, low_cut_hz: 180.0, width: 1.0 }
    }
}

const N: usize = 8;
/// Lunghezze delle linee (ms), numeri primi tra loro.
const LINE_MS: [f32; N] = [29.7, 37.1, 41.1, 43.7, 53.9, 61.3, 67.9, 79.3];
const DIFF_MS: [f32; 4] = [4.77, 3.59, 12.73, 9.31];

struct Diffuser {
    line: DelayLine,
    len: usize,
    g: f32,
}

impl Diffuser {
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let d = self.line.read_int(self.len);
        let v = x + self.g * d;
        self.line.write(v);
        d - self.g * v
    }
}

pub struct Reverb {
    sr: f32,
    pre: DelayLine,
    diff: [Diffuser; 4],
    lines: [DelayLine; N],
    len: [f32; N],
    damp: [OnePole; N],
    lfo: [Lfo; N],
    state: [f32; N],
    low_cut: [Biquad; 2],
    in_lp: OnePole,
    settings: Option<ReverbSettings>,
    gains: [f32; N],
    predelay: Smoothed,
    quiet: u32,
}

impl Reverb {
    pub fn new(sr: f32) -> Self {
        let line = |ms: f32| DelayLine::new((ms * 0.001 * sr * 1.05) as usize + 16);
        let len: [f32; N] = std::array::from_fn(|i| LINE_MS[i] * 0.001 * sr);
        Self {
            sr,
            pre: DelayLine::new((0.2 * sr) as usize + 8),
            diff: std::array::from_fn(|i| Diffuser {
                line: line(DIFF_MS[i]),
                len: (DIFF_MS[i] * 0.001 * sr) as usize,
                g: if i < 2 { 0.72 } else { 0.62 },
            }),
            lines: std::array::from_fn(|i| line(LINE_MS[i])),
            len,
            damp: [OnePole::new(8000.0, sr); N],
            lfo: std::array::from_fn(|i| Lfo::new(i as f32 / N as f32)),
            state: [0.0; N],
            low_cut: [Biquad::default(); 2],
            in_lp: OnePole::new(12000.0, sr),
            settings: None,
            gains: [0.0; N],
            predelay: Smoothed::new(24.0 * 0.001 * sr, 0.05, sr),
            quiet: u32::MAX,
        }
    }

    pub fn reset(&mut self) {
        self.pre.clear();
        for d in &mut self.diff {
            d.line.clear();
        }
        for l in &mut self.lines {
            l.clear();
        }
        for d in &mut self.damp {
            d.reset();
        }
        for f in &mut self.low_cut {
            f.reset();
        }
        self.state = [0.0; N];
        self.quiet = u32::MAX;
    }

    fn configure(&mut self, s: &ReverbSettings) {
        if self.settings == Some(*s) {
            return;
        }
        self.settings = Some(*s);
        let rt = s.decay_s.clamp(0.2, 12.0);
        for i in 0..N {
            let seconds = self.len[i] / self.sr;
            self.gains[i] = 10f32.powf(-3.0 * seconds / rt);
            // Smorzamento: da 12 kHz (aperto) a 2,5 kHz (scuro).
            let cutoff = 12000.0 * (2500.0f32 / 12000.0).powf(s.damping.clamp(0.0, 1.0));
            self.damp[i].set(cutoff, self.sr);
        }
        for f in &mut self.low_cut {
            f.set(BiquadKind::Highpass, s.low_cut_hz.clamp(20.0, 1000.0), 0.6, self.sr);
        }
    }

    pub fn is_idle(&self) -> bool {
        let rt = self.settings.map_or(2.0, |s| s.decay_s);
        self.quiet as f32 > (rt * 1.5 + 0.3) * self.sr
    }

    /// Elabora la mandata mono e restituisce la coda (L, R).
    #[inline]
    pub fn process(&mut self, input: f32, s: &ReverbSettings) -> (f32, f32) {
        self.configure(s);
        if input.abs() > 1e-6 {
            self.quiet = 0;
        } else {
            self.quiet = self.quiet.saturating_add(1);
        }
        self.pre.write(self.in_lp.lowpass(input));
        let pd = self.predelay.next(s.predelay_ms.clamp(0.0, 190.0) * 0.001 * self.sr);
        let mut x = self.pre.read(pd.max(1.0));
        for d in &mut self.diff {
            x = d.process(x);
        }

        // Linee: lettura (con un movimento piccolissimo), smorzamento.
        let mut out = [0.0f32; N];
        for i in 0..N {
            let p = self.lfo[i].advance(0.13 + 0.031 * i as f32, self.sr);
            let wobble = Lfo::sine(p) * 0.0009 * self.sr;
            let y = self.lines[i].read(self.len[i] + wobble);
            out[i] = self.damp[i].lowpass(y) * self.gains[i];
        }
        // Householder: y = x - (2/N) * somma.
        let sum: f32 = out.iter().sum::<f32>() * (2.0 / N as f32);
        for i in 0..N {
            let fb = out[i] - sum;
            // L'ingresso entra con segni alterni (le linee partono scorrelate).
            let inj = if i % 2 == 0 { x } else { -x };
            self.lines[i].write(fb + inj * 0.35);
        }
        self.state = out;

        // Uscita stereo: due combinazioni ortogonali delle linee.
        let l = out[0] + out[2] - out[4] + out[6] + 0.5 * (out[1] - out[5]);
        let r = out[1] + out[3] - out[5] + out[7] + 0.5 * (out[2] - out[6]);
        let l = self.low_cut[0].process(l) * 0.42;
        let r = self.low_cut[1].process(r) * 0.42;
        let w = s.width.clamp(0.0, 1.0);
        let mid = 0.5 * (l + r);
        (mid + (l - mid) * w, mid + (r - mid) * w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decay_time(rt: f32, sr: f32) -> f32 {
        let mut rv = Reverb::new(sr);
        let s = ReverbSettings { decay_s: rt, predelay_ms: 0.0, damping: 0.0, low_cut_hz: 20.0, width: 1.0 };
        let n = (sr * (rt * 1.5 + 0.5)) as usize;
        let mut energy = Vec::with_capacity(n);
        for i in 0..n {
            // Rumore breve come eccitazione (deterministico).
            let x = if i < 480 { ((i as f32 * 12.9898).sin() * 43758.545).fract() - 0.5 } else { 0.0 };
            let (l, r) = rv.process(x, &s);
            energy.push(l * l + r * r);
        }
        // Integrazione all'indietro di Schroeder, poi tempo per -20 dB (x3).
        let mut acc = 0.0f64;
        let mut edc = vec![0.0f64; n];
        for i in (0..n).rev() {
            acc += energy[i] as f64;
            edc[i] = acc;
        }
        let db = |i: usize| 10.0 * (edc[i] / edc[0]).log10();
        let t5 = (0..n).find(|&i| db(i) < -5.0).unwrap();
        let t25 = (0..n).find(|&i| db(i) < -25.0).unwrap();
        (t25 - t5) as f32 / sr * 3.0
    }

    #[test]
    fn decay_matches_the_setting() {
        for rt in [1.0, 2.5] {
            let measured = decay_time(rt, 48000.0);
            assert!((measured / rt - 1.0).abs() < 0.25, "RT60 {rt}: misurato {measured}");
        }
    }

    #[test]
    fn stable_with_any_settings() {
        let sr = 44100.0;
        let mut rv = Reverb::new(sr);
        let s = ReverbSettings { decay_s: 12.0, predelay_ms: 190.0, damping: 0.0, low_cut_hz: 20.0, width: 1.0 };
        let mut peak = 0.0f32;
        for i in 0..(sr as usize * 10) {
            let x = if i < 4410 { (i as f32 * 0.07).sin() } else { 0.0 };
            let (l, r) = rv.process(x, &s);
            assert!(l.is_finite() && r.is_finite());
            peak = peak.max(l.abs()).max(r.abs());
        }
        assert!(peak < 2.0, "{peak}");
    }
}
