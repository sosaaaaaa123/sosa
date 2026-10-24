//! Mattoni comuni degli effetti: linea di ritardo con lettura frazionaria,
//! filtri, valori che scorrono dolcemente.

use std::f32::consts::{PI, TAU};

/// Linea di ritardo circolare con lettura a ritardo frazionario
/// (interpolazione cubica di Hermite: niente "zipper" quando il ritardo si
/// muove, come nella modulazione del nastro o del chorus).
pub struct DelayLine {
    buf: Vec<f32>,
    mask: usize,
    pos: usize,
}

impl DelayLine {
    /// Una linea che puo' ritardare almeno `max_samples` campioni.
    pub fn new(max_samples: usize) -> Self {
        let len = (max_samples + 4).next_power_of_two();
        Self { buf: vec![0.0; len], mask: len - 1, pos: 0 }
    }

    pub fn clear(&mut self) {
        self.buf.fill(0.0);
        self.pos = 0;
    }

    /// Il ritardo massimo che si puo' leggere.
    pub fn max_delay(&self) -> f32 {
        (self.buf.len() - 4) as f32
    }

    #[inline]
    pub fn write(&mut self, x: f32) {
        self.pos = (self.pos + 1) & self.mask;
        self.buf[self.pos] = x;
    }

    /// Il campione scritto `delay` campioni fa (0 = l'ultimo scritto).
    #[inline]
    pub fn read(&self, delay: f32) -> f32 {
        let delay = delay.clamp(1.0, self.max_delay());
        let whole = delay.floor();
        let frac = delay - whole;
        let i = self.pos.wrapping_sub(whole as usize);
        let at = |k: usize| self.buf[k & self.mask];
        // Punti: x0 = piu' recente (ritardo whole-1) ... x3 (ritardo whole+2).
        let xm1 = at(i.wrapping_add(1));
        let x0 = at(i);
        let x1 = at(i.wrapping_sub(1));
        let x2 = at(i.wrapping_sub(2));
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * frac + c2) * frac + c1) * frac + x0
    }

    /// Lettura a ritardo intero (piu' veloce, per i ritardi fissi).
    #[inline]
    pub fn read_int(&self, delay: usize) -> f32 {
        self.buf[self.pos.wrapping_sub(delay) & self.mask]
    }
}

/// Filtro a un polo (passa-basso), con la sua controparte passa-alto.
#[derive(Clone, Copy, Default)]
pub struct OnePole {
    a: f32,
    z: f32,
}

impl OnePole {
    pub fn new(cutoff_hz: f32, sr: f32) -> Self {
        let mut f = Self::default();
        f.set(cutoff_hz, sr);
        f
    }

    pub fn set(&mut self, cutoff_hz: f32, sr: f32) {
        let c = (cutoff_hz / sr).clamp(1e-6, 0.49);
        self.a = 1.0 - (-TAU * c).exp();
    }

    #[inline]
    pub fn lowpass(&mut self, x: f32) -> f32 {
        self.z += self.a * (x - self.z);
        self.z
    }

    #[inline]
    pub fn highpass(&mut self, x: f32) -> f32 {
        x - self.lowpass(x)
    }

    pub fn reset(&mut self) {
        self.z = 0.0;
    }
}

/// Biquad (formule di R. Bristow-Johnson), forma diretta trasposta II.
#[derive(Clone, Copy)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        Self { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, z1: 0.0, z2: 0.0 }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum BiquadKind {
    Lowpass,
    Highpass,
    Peak(f32),
    LowShelf(f32),
    HighShelf(f32),
}

impl Biquad {
    pub fn new(kind: BiquadKind, freq: f32, q: f32, sr: f32) -> Self {
        let mut b = Self::default();
        b.set(kind, freq, q, sr);
        b
    }

    pub fn set(&mut self, kind: BiquadKind, freq: f32, q: f32, sr: f32) {
        let freq = freq.clamp(5.0, sr * 0.45);
        let w = TAU * freq / sr;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q.max(0.05));
        let (b0, b1, b2, a0, a1, a2) = match kind {
            BiquadKind::Lowpass => ((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha),
            BiquadKind::Highpass => ((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha),
            BiquadKind::Peak(db) => {
                let a = 10f32.powf(db / 40.0);
                (1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a)
            }
            BiquadKind::LowShelf(db) => {
                let a = 10f32.powf(db / 40.0);
                let sq = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * c + sq),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * c),
                    a * ((a + 1.0) - (a - 1.0) * c - sq),
                    (a + 1.0) + (a - 1.0) * c + sq,
                    -2.0 * ((a - 1.0) + (a + 1.0) * c),
                    (a + 1.0) + (a - 1.0) * c - sq,
                )
            }
            BiquadKind::HighShelf(db) => {
                let a = 10f32.powf(db / 40.0);
                let sq = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * c + sq),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
                    a * ((a + 1.0) + (a - 1.0) * c - sq),
                    (a + 1.0) - (a - 1.0) * c + sq,
                    2.0 * ((a - 1.0) - (a + 1.0) * c),
                    (a + 1.0) - (a - 1.0) * c - sq,
                )
            }
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// Un valore che va verso il bersaglio con un filtro a un polo (tempo in s).
#[derive(Clone, Copy)]
pub struct Smoothed {
    value: f32,
    coef: f32,
}

impl Smoothed {
    pub fn new(value: f32, time_s: f32, sr: f32) -> Self {
        let coef = 1.0 - (-1.0 / (time_s.max(1e-4) * sr)).exp();
        Self { value, coef }
    }

    #[inline]
    pub fn next(&mut self, target: f32) -> f32 {
        self.value += self.coef * (target - self.value);
        self.value
    }
}

/// Oscillatore lento (LFO) con fase in 0..1.
#[derive(Clone, Copy)]
pub struct Lfo {
    phase: f32,
}

impl Lfo {
    pub fn new(phase: f32) -> Self {
        Self { phase: phase.rem_euclid(1.0) }
    }

    #[inline]
    pub fn advance(&mut self, hz: f32, sr: f32) -> f32 {
        let p = self.phase;
        self.phase = (self.phase + hz / sr).fract();
        p
    }

    pub fn sine(p: f32) -> f32 {
        (TAU * p).sin()
    }

    /// Triangolo -1..1.
    pub fn triangle(p: f32) -> f32 {
        4.0 * (p - (p + 0.5).floor()).abs() - 1.0
    }
}

/// Generatore pseudo-casuale semplice (xorshift), per il "vagare" del nastro.
#[derive(Clone, Copy)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    /// Valore in -1..1.
    #[inline]
    pub fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    }
}

/// Saturazione morbida (tangente iperbolica razionale: costa poco ed e'
/// liscia, con un leggero sbilanciamento per le armoniche pari del nastro).
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

/// Filtro passa-tutto del primo ordine (dispersione della molla, diffusione).
#[derive(Clone, Copy, Default)]
pub struct Allpass1 {
    a: f32,
    z: f32,
}

impl Allpass1 {
    pub fn new(a: f32) -> Self {
        Self { a, z: 0.0 }
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = -self.a * x + self.z;
        self.z = x + self.a * y;
        y
    }

    pub fn reset(&mut self) {
        self.z = 0.0;
    }
}

/// Coefficiente per un passa-tutto del primo ordine che ha il ritardo di
/// gruppo massimo intorno a `freq`.
pub fn allpass_coef(freq: f32, sr: f32) -> f32 {
    let t = (PI * freq / sr).tan();
    (1.0 - t) / (1.0 + t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_line_reads_back_exactly_at_integer_delays() {
        let mut d = DelayLine::new(100);
        for i in 0..50 {
            d.write(i as f32);
        }
        assert_eq!(d.read(1.0), 48.0);
        assert_eq!(d.read(10.0), 39.0);
        assert!((d.read(10.5) - 38.5).abs() < 1e-4, "{}", d.read(10.5));
        assert_eq!(d.read_int(0), 49.0);
    }

    #[test]
    fn biquad_lowpass_passes_dc() {
        let mut f = Biquad::new(BiquadKind::Lowpass, 1000.0, 0.707, 48000.0);
        let mut y = 0.0;
        for _ in 0..10000 {
            y = f.process(1.0);
        }
        assert!((y - 1.0).abs() < 1e-3);
        let mut s = Biquad::new(BiquadKind::LowShelf(6.0), 200.0, 0.707, 48000.0);
        for _ in 0..20000 {
            y = s.process(1.0);
        }
        assert!((20.0 * y.log10() - 6.0).abs() < 0.1, "{y}");
    }

    #[test]
    fn soft_clip_is_bounded_and_transparent_when_small() {
        assert!((soft_clip(0.01) - 0.01).abs() < 1e-5);
        assert!(soft_clip(100.0).abs() <= 1.0 + 1e-6);
    }
}
