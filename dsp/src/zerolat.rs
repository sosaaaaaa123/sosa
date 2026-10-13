//! Pitch shifting **a latenza zero** per la modalita' Low Latency (per
//! registrare con il plugin acceso senza sentire ritardi).
//!
//! La voce viene letta da una "testina" che scorre sul segnale appena
//! arrivato a velocita' diversa: piu' veloce = nota piu' acuta, piu' lenta =
//! piu' grave (come un nastro, ma solo di pochi millesimi). La testina non
//! puo' superare il presente ne' restare troppo indietro: quando serve salta
//! di un ciclo esatto della voce (un periodo) avanti o indietro, scegliendo
//! il punto in cui la forma d'onda combacia, con una dissolvenza breve. Il
//! ciclo nuovo e' quasi identico a quello vecchio, quindi il salto non si
//! sente.
//!
//! Il ritardo resta tra 3 campioni e un periodo della voce (in media 2-5 ms),
//! e il plugin dichiara al DAW latenza zero. La testina sposta le formanti
//! insieme alla nota: dopo, il filtro di `timbre.rs` le riporta al loro
//! posto (come fa HQ) e applica il comando Formant.

use crate::vocoder::GrainPitch;
use crate::MAX_CHANNELS;

pub struct DelayShifter {
    channels: usize,
    sample_rate: f32,
    buffers: Vec<Vec<f32>>,
    mask: usize,
    /// Indice (assoluto) dell'ultimo campione scritto.
    newest: u64,
    started: bool,
    /// Posizione (assoluta, frazionaria) della testina principale.
    read: f64,
    /// Seconda testina, durante una dissolvenza.
    fade_from: f64,
    fade_left: usize,
    fade_len: usize,
    countdown: usize,
    hop: usize,
    pitch: GrainPitch,
    /// Rapporto di velocita' della testina, cambiato con dolcezza campione per campione.
    ratio: f64,
    smoothing: f64,
}

impl DelayShifter {
    /// Ritardo minimo della testina (campioni): serve all'interpolazione.
    const MIN_DELAY: f64 = 3.0;
    /// Ogni quanti campioni chiedere di nuovo l'intonazione (~0,7 ms).
    const HOP_S: f32 = 0.0007;

    pub fn new(channels: usize, sample_rate: f32, max_period: usize) -> Self {
        let channels = channels.clamp(1, MAX_CHANNELS);
        let len = (8 * max_period + 4096).next_power_of_two();
        Self {
            channels,
            sample_rate,
            buffers: vec![vec![0.0; len]; channels],
            mask: len - 1,
            newest: 0,
            started: false,
            read: 0.0,
            fade_from: 0.0,
            fade_left: 0,
            fade_len: 1,
            countdown: 1,
            hop: ((sample_rate * Self::HOP_S) as usize).max(8),
            pitch: GrainPitch { period: 0.0, ratio: 1.0, voiced: false, formant: 1.0 },
            ratio: 1.0,
            // (Raccordo provato a 1,5 ms: intonazione meno pronta, nessun vantaggio.)
            smoothing: 1.0,
        }
    }

    /// Latenza dichiarata al DAW.
    pub fn latency(&self) -> usize {
        0
    }

    pub fn reset(&mut self) {
        for b in &mut self.buffers {
            b.fill(0.0);
        }
        self.newest = 0;
        self.started = false;
        self.read = 0.0;
        self.fade_left = 0;
        self.countdown = 1;
        self.pitch = GrainPitch { period: 0.0, ratio: 1.0, voiced: false, formant: 1.0 };
        self.ratio = 1.0;
    }

    /// Rapporto di velocita' attuale della testina (= di quanto sposta la
    /// nota, e con lei le formanti).
    pub fn current_ratio(&self) -> f64 {
        self.ratio
    }

    /// Ritardo attuale della testina (campioni).
    pub fn delay(&self) -> f64 {
        self.newest as f64 - self.read
    }

    #[inline]
    fn sample(buffer: &[f32], mask: usize, pos: f64) -> f32 {
        // Interpolazione di Hermite a 4 punti.
        let i = pos.floor();
        let t = (pos - i) as f32;
        let i = i as i64;
        let at = |k: i64| buffer[(k as usize) & mask];
        let (y0, y1, y2, y3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * t + c2) * t + c1) * t + y1
    }

    /// Correlazione normalizzata tra i `w` campioni che finiscono in `a` e in `b`.
    fn similarity(&self, a: i64, b: i64, w: usize) -> f32 {
        let buf = &self.buffers[0];
        let (mut ab, mut aa, mut bb) = (0.0f32, 0.0f32, 0.0f32);
        for k in 0..w as i64 {
            let x = buf[((a - k) as usize) & self.mask];
            let y = buf[((b - k) as usize) & self.mask];
            ab += x * y;
            aa += x * x;
            bb += y * y;
        }
        ab / (aa * bb).sqrt().max(1e-12)
    }

    /// Il salto (in campioni) vicino a un periodo dove la forma d'onda combacia meglio.
    fn best_jump(&self, period: f32, forward: bool, limit: f64) -> f64 {
        let w = (period as usize).clamp(16, (0.012 * self.sample_rate) as usize);
        let here = self.read.floor() as i64;
        let lo = (period * 0.85).round() as i64;
        let hi = ((period * 1.15).round() as i64).min(limit.floor() as i64);
        if hi < lo {
            return period.round().min(limit as f32).max(1.0) as f64;
        }
        let mut best = (period.round() as i64).clamp(lo, hi);
        let mut best_score = f32::MIN;
        let mut j = lo;
        while j <= hi {
            let other = if forward { here + j } else { here - j };
            let score = self.similarity(here, other, w);
            if score > best_score {
                best_score = score;
                best = j;
            }
            j += 1;
        }
        best as f64
    }

    #[inline]
    pub fn process<F>(&mut self, input: &[f32], output: &mut [f32], mut pitch_at: F)
    where
        F: FnMut(f64) -> GrainPitch,
    {
        if self.started {
            self.newest += 1;
        } else {
            self.started = true;
            self.read = self.newest as f64 - Self::MIN_DELAY;
        }
        let index = self.newest as usize & self.mask;
        for ch in 0..self.channels {
            self.buffers[ch][index] = input.get(ch).copied().unwrap_or(0.0);
        }

        self.countdown -= 1;
        if self.countdown == 0 {
            self.countdown = self.hop;
            self.pitch = pitch_at(self.newest as f64);
        }
        let voiced = self.pitch.voiced && self.pitch.period > 1.0;
        let target = if voiced { self.pitch.ratio.clamp(0.5, 2.0) as f64 } else { 1.0 };
        // Niente gradini ogni volta che arriva un rapporto nuovo (~1,5 ms di raccordo).
        self.ratio += (target - self.ratio) * self.smoothing;
        let ratio = self.ratio;
        let period = if voiced { self.pitch.period } else { 0.008 * self.sample_rate };
        let newest = self.newest as f64;

        if self.fade_left == 0 {
            let delay = newest - self.read;
            let fade = if voiced {
                (period as usize).clamp(48, (0.010 * self.sample_rate) as usize)
            } else {
                (0.004 * self.sample_rate) as usize
            };
            let shrink = fade as f64 * (ratio - 1.0).max(0.0);
            if delay - shrink < Self::MIN_DELAY + 1.0 && ratio > 1.0 {
                // La testina sta raggiungendo il presente: torna indietro di un ciclo.
                let jump = self.best_jump(period, false, period as f64 * 1.15);
                self.start_fade(self.read - jump, fade);
            } else if voiced && delay > Self::MIN_DELAY + 1.5 * period as f64 + shrink {
                // Troppo indietro (voce piu' grave, o ritardo accumulato): avanti di un ciclo.
                let limit = delay - Self::MIN_DELAY - shrink - 1.0;
                let jump = self.best_jump(period, true, limit);
                self.start_fade(self.read + jump, fade);
            } else if !voiced && delay > Self::MIN_DELAY + 0.003 * self.sample_rate as f64 {
                // Nelle consonanti e nei silenzi si torna al ritardo minimo.
                self.start_fade(newest - Self::MIN_DELAY, fade);
            }
        }

        // Le testine non possono superare il presente.
        let limit = newest - 2.0;
        self.read = self.read.min(limit);
        let gain = if self.fade_left > 0 {
            self.fade_from = self.fade_from.min(limit);
            let g = 1.0 - self.fade_left as f32 / self.fade_len as f32;
            // Dissolvenza a potenza costante "morbida" (i cicli sono simili).
            0.5 - 0.5 * (std::f32::consts::PI * g).cos()
        } else {
            1.0
        };
        for ch in 0..self.channels {
            let buf = &self.buffers[ch];
            let main = Self::sample(buf, self.mask, self.read);
            let value = if self.fade_left > 0 {
                let old = Self::sample(buf, self.mask, self.fade_from);
                old * (1.0 - gain) + main * gain
            } else {
                main
            };
            if let Some(o) = output.get_mut(ch) {
                *o = value;
            }
        }
        self.read += ratio;
        if self.fade_left > 0 {
            self.fade_from += ratio;
            self.fade_left -= 1;
        }
    }

    fn start_fade(&mut self, new_read: f64, len: usize) {
        self.fade_from = self.read;
        self.read = new_read;
        self.fade_len = len.max(1);
        self.fade_left = self.fade_len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn tone(sr: f32, f0: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / sr;
                (1..6).map(|h| (TAU * f0 * h as f32 * t).sin() / h as f32).sum::<f32>() * 0.3
            })
            .collect()
    }

    fn period_of(x: &[f32], lo: usize, hi: usize) -> usize {
        (lo..hi)
            .max_by(|&a, &b| {
                let s = |lag: usize| x[..x.len() - hi].iter().zip(&x[lag..]).map(|(a, b)| a * b).sum::<f32>();
                s(a).partial_cmp(&s(b)).unwrap()
            })
            .unwrap()
    }

    #[test]
    fn shifts_pitch_with_tiny_delay() {
        let sr = 48000.0;
        let f0 = 150.0;
        let input = tone(sr, f0, 48000);
        for ratio in [1.06f32, 0.94] {
            let mut z = DelayShifter::new(1, sr, 800);
            let pitch = GrainPitch { period: sr / f0, ratio, voiced: true, formant: 1.0 };
            let mut out = Vec::new();
            let mut max_delay = 0.0f64;
            for &x in &input {
                let mut o = [0.0];
                z.process(&[x], &mut o, |_| pitch);
                out.push(o[0]);
                max_delay = max_delay.max(z.delay());
            }
            let p = period_of(&out[20000..40000], 250, 380);
            let expected = sr / (f0 * ratio);
            assert!((p as f32 - expected).abs() <= 2.0, "ratio {ratio}: periodo {p} invece di {expected}");
            // Il ritardo resta sotto un periodo e mezzo (+ dissolvenza).
            assert!(max_delay < 2.0 * (sr / f0) as f64, "ritardo massimo {max_delay}");
        }
    }

    #[test]
    fn unvoiced_passes_with_three_samples_of_delay() {
        let sr = 48000.0;
        let input: Vec<f32> = (0..5000).map(|i| (i as f32 * 0.37).sin()).collect();
        let mut z = DelayShifter::new(1, sr, 800);
        let mut out = Vec::new();
        for &x in &input {
            let mut o = [0.0];
            z.process(&[x], &mut o, |_| GrainPitch { period: 0.0, ratio: 1.0, voiced: false, formant: 1.0 });
            out.push(o[0]);
        }
        for i in 100..input.len() {
            assert!((out[i] - input[i - 3]).abs() < 1e-3, "campione {i}");
        }
    }
}
