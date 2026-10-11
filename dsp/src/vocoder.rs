//! Pitch shifting con un **phase vocoder** "a picchi bloccati" e formanti
//! preservate: il motore della modalita' HQ.
//!
//! Il phase vocoder lavora nel dominio delle frequenze:
//!
//! 1. ogni 5 ms prende gli ultimi ~43 ms di voce, li smussa con una finestra
//!    di Hann e ne calcola lo spettro (FFT): un insieme di "righe", una per
//!    ogni armonica della voce;
//! 2. trova i **picchi** dello spettro (le armoniche) e la frequenza esatta
//!    di ognuno, misurando di quanto e' girata la sua fase dall'ultima volta;
//! 3. sposta ogni picco, insieme alle righe vicine che gli appartengono
//!    (la sua "zona"), alla frequenza nuova: frequenza x rapporto;
//! 4. la fase di ogni picco spostato continua a girare alla velocita' giusta
//!    per la frequenza nuova, e le righe della sua zona la seguono rigidamente
//!    ("identity phase locking", Laroche e Dolson 1999): cosi' ogni armonica
//!    resta un'onda pulita, senza l'effetto "metallico" o "in una stanza"
//!    dei phase vocoder semplici;
//! 5. il **timbro** (l'inviluppo spettrale, cioe' le formanti) viene misurato
//!    con il cepstrum e rimesso al suo posto dopo lo spostamento: la voce
//!    cambia nota ma non diventa "da scoiattolo" o "da orco";
//! 6. torna nel tempo (FFT inversa) e somma i pezzi sovrapposti.
//!
//! Dove non c'e' una nota (consonanti, respiri) e sugli attacchi bruschi lo
//! spettro passa **identico**: la ricostruzione e' perfetta, niente sbavature
//! sulle "t" e sulle "s".
//!
//! La latenza e' la lunghezza della finestra (2048 campioni a 44,1/48 kHz,
//! circa 43 ms), compensata dal DAW. (Finestra: la potenza di 2 piu' vicina
//! sopra i 40 ms.)

use crate::fft::{Complex, Fft};
use crate::MAX_CHANNELS;
use std::f32::consts::PI;

const TAU: f32 = 2.0 * PI;

/// Cosa serve sapere per spostare l'intonazione in un certo punto della voce.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrainPitch {
    /// Periodo della voce in quel punto (campioni): serve a separare le
    /// formanti dalle armoniche.
    pub period: f32,
    /// Di quanto moltiplicare la frequenza (1.0 = invariata, 2.0 = un'ottava sopra).
    pub ratio: f32,
    /// `true` se li' c'e' una nota cantata, `false` per consonanti, respiri e
    /// rumori (lo spettro passa identico).
    pub voiced: bool,
    /// Spostamento delle formanti (il "timbro", la lunghezza della gola):
    /// 1.0 = invariato, >1 voce piu' "piccola", <1 voce piu' "grande".
    pub formant: f32,
}

/// Riporta un angolo in (-π, π].
#[inline]
fn wrap(x: f32) -> f32 {
    x - TAU * (x / TAU).round()
}

/// Stato spettrale di un canale.
struct ChannelState {
    /// Ultimi `n` campioni ricevuti (buffer circolare).
    input: Vec<f32>,
    /// Uscita in costruzione (buffer circolare di 2n): ogni frame vi somma il suo pezzo.
    accum: Vec<f32>,
    /// Fase di ogni riga nel frame precedente (per la frequenza esatta dei picchi).
    prev_phase: Vec<f32>,
    /// Rotazione di fase applicata a ogni riga (quella del picco a cui
    /// appartiene) nel frame precedente e in quello attuale.
    prev_rotation: Vec<f32>,
    rotation: Vec<f32>,
    /// `false` finche' non c'e' un frame precedente valido.
    has_prev: bool,
    /// Energia media recente (per riconoscere gli attacchi).
    energy: f32,
}

pub struct PhaseVocoder {
    channels: usize,
    n: usize,
    half: usize,
    hop: usize,
    fft: Fft,
    window: Vec<f32>,
    /// 1 / somma dei quadrati delle finestre sovrapposte (ricostruzione esatta).
    norm: f32,
    position: u64,
    countdown: usize,
    states: Vec<ChannelState>,

    // Memoria di lavoro (allocata una volta sola).
    spectrum: Vec<Complex>,
    out_spectrum: Vec<Complex>,
    cepstrum: Vec<Complex>,
    magnitude: Vec<f32>,
    phase: Vec<f32>,
    log_env: Vec<f32>,
    target_env: Vec<f32>,
    peaks: Vec<usize>,
    frame_out: Vec<f32>,
    /// Spostamento frazionario delle righe (vedi `process_frame`).
    fractional: bool,
}

impl PhaseVocoder {
    /// Durata della finestra di analisi (s): circa 43 ms, abbastanza per
    /// separare le armoniche anche di una voce bassa.
    const WINDOW_S: f32 = 0.040;
    /// Frame sovrapposti: uno ogni n/8 campioni (circa 5 ms).
    const OVERLAP: usize = 8;

    pub fn new(channels: usize, sample_rate: f32) -> Self {
        Self::with_window(channels, sample_rate, Self::WINDOW_S)
    }

    /// Come [`new`](Self::new) con una finestra di `seconds` (arrotondata
    /// alla potenza di 2 superiore): piu' corta = meno latenza.
    pub fn with_window(channels: usize, sample_rate: f32, seconds: f32) -> Self {
        let channels = channels.clamp(1, MAX_CHANNELS);
        let n = ((sample_rate * seconds).round() as usize).next_power_of_two().max(256);
        let half = n / 2;
        let hop = n / Self::OVERLAP;
        // Hann "periodica": sovrapposta ogni n/8 da' una somma costante.
        let window: Vec<f32> = (0..n).map(|i| 0.5 - 0.5 * (TAU * i as f32 / n as f32).cos()).collect();
        let sum: f32 = (0..Self::OVERLAP).map(|j| window[j * hop].powi(2)).sum::<f32>()
            + (0..Self::OVERLAP).map(|j| window[(j * hop + hop / 2) % n].powi(2)).sum::<f32>();
        let norm = 2.0 / sum;
        let states = (0..channels)
            .map(|_| ChannelState {
                input: vec![0.0; n],
                accum: vec![0.0; 2 * n],
                prev_phase: vec![0.0; half + 1],
                prev_rotation: vec![0.0; half + 1],
                rotation: vec![0.0; half + 1],
                has_prev: false,
                energy: 0.0,
            })
            .collect();
        Self {
            channels,
            n,
            half,
            hop,
            fft: Fft::new(n),
            window,
            norm,
            position: 0,
            countdown: hop,
            states,
            spectrum: vec![Complex::default(); n],
            out_spectrum: vec![Complex::default(); n],
            cepstrum: vec![Complex::default(); n],
            magnitude: vec![0.0; half + 1],
            phase: vec![0.0; half + 1],
            log_env: vec![0.0; half + 1],
            target_env: vec![0.0; half + 1],
            peaks: Vec::with_capacity(half),
            frame_out: vec![0.0; n],
            fractional: true,
        }
    }

    /// Ritardo tra ingresso e uscita (campioni): la lunghezza della finestra.
    pub fn latency(&self) -> usize {
        self.n
    }

    pub fn window_len(&self) -> usize {
        self.n
    }

    pub fn reset(&mut self) {
        self.position = 0;
        self.countdown = self.hop;
        for s in &mut self.states {
            s.input.fill(0.0);
            s.accum.fill(0.0);
            s.prev_phase.fill(0.0);
            s.prev_rotation.fill(0.0);
            s.rotation.fill(0.0);
            s.has_prev = false;
            s.energy = 0.0;
        }
    }

    /// Elabora un campione per canale. `pitch_at(t)` dice rapporto, periodo
    /// e presenza di una nota nel punto `t` dell'ingresso (il centro del frame).
    #[inline]
    pub fn process<F>(&mut self, input: &[f32], output: &mut [f32], mut pitch_at: F)
    where
        F: FnMut(f64) -> GrainPitch,
    {
        let n = self.n;
        let now = self.position;
        for ch in 0..self.channels {
            self.states[ch].input[now as usize & (n - 1)] = input.get(ch).copied().unwrap_or(0.0);
        }

        self.countdown -= 1;
        if self.countdown == 0 {
            self.countdown = self.hop;
            // Il frame copre gli ultimi n campioni: il suo centro e' qui.
            let center = now as f64 - (n as f64 - 1.0) / 2.0;
            let pitch = pitch_at(center);
            for ch in 0..self.channels {
                self.process_frame(ch, now, pitch);
            }
        }

        let out_index = now as usize & (2 * n - 1);
        for ch in 0..self.channels {
            let s = &mut self.states[ch];
            if let Some(o) = output.get_mut(ch) {
                *o = s.accum[out_index];
            }
            s.accum[out_index] = 0.0;
        }
        self.position += 1;
    }

    fn process_frame(&mut self, ch: usize, now: u64, pitch: GrainPitch) {
        let n = self.n;
        let half = self.half;
        let hop = self.hop as f32;
        let start = now as usize + 1; // indice (circolare) del campione piu' vecchio
        let state = &mut self.states[ch];

        // 1) Finestra + "fase zero" (il centro del frame va all'indice 0).
        let mut new_energy = 0.0f32;
        for i in 0..n {
            let x = state.input[(start + i) & (n - 1)];
            if i >= n - self.hop {
                new_energy += x * x;
            }
            self.spectrum[(i + half) & (n - 1)] = Complex::new(x * self.window[i], 0.0);
        }
        new_energy /= self.hop as f32;
        // Attacco brusco: l'ultimo pezzo e' molto piu' forte della media recente.
        let onset = new_energy > 8.0 * state.energy + 1e-7;
        state.energy = 0.7 * state.energy + 0.3 * new_energy;

        self.fft.process(&mut self.spectrum, false);
        for k in 0..=half {
            let c = self.spectrum[k];
            self.magnitude[k] = (c.re * c.re + c.im * c.im).sqrt();
            self.phase[k] = c.im.atan2(c.re);
        }

        let ratio = pitch.ratio.clamp(0.25, 4.0);
        let formant = pitch.formant.clamp(0.5, 2.0);
        let shifting = pitch.voiced && ((ratio - 1.0).abs() > 1e-5 || (formant - 1.0).abs() > 1e-5);

        if !pitch.voiced || onset {
            // Niente nota (o attacco): lo spettro passa identico e le
            // rotazioni ripartono da zero.
            state.rotation.fill(0.0);
        }

        let passthrough = !pitch.voiced || (!shifting && state.prev_rotation.iter().all(|&r| r == 0.0)) || onset;
        if passthrough {
            self.out_spectrum.copy_from_slice(&self.spectrum);
            state.rotation.fill(0.0);
        } else {
            // 2) Inviluppo spettrale (formanti) con il cepstrum.
            let need_env = (ratio - 1.0).abs() > 1e-5 || (formant - 1.0).abs() > 1e-5;
            if need_env {
                for k in 0..n {
                    let m = if k <= half { self.magnitude[k] } else { self.magnitude[n - k] };
                    self.cepstrum[k] = Complex::new((m + 1e-9).ln(), 0.0);
                }
                self.fft.process(&mut self.cepstrum, true);
                // Teniamo solo le "quefrenze" piu' basse della meta' del
                // periodo: l'andamento lento (formanti), non le armoniche.
                let cut = (0.5 * pitch.period).clamp(8.0, (n / 8) as f32) as usize;
                for q in 0..n {
                    let qq = q.min(n - q);
                    let w = if qq < cut {
                        1.0
                    } else if qq < cut + 4 {
                        0.5 + 0.5 * (PI * (qq - cut) as f32 / 4.0).cos()
                    } else {
                        0.0
                    };
                    let c = self.cepstrum[q];
                    self.cepstrum[q] = Complex::new(c.re * w / n as f32, 0.0);
                }
                self.fft.process(&mut self.cepstrum, false);
                for k in 0..=half {
                    self.log_env[k] = self.cepstrum[k].re;
                }
                // Inviluppo voluto: lo stesso, oppure spostato (Formant).
                for k in 0..=half {
                    self.target_env[k] = if (formant - 1.0).abs() > 1e-5 {
                        let src = k as f32 / formant;
                        let i = src.floor() as usize;
                        if i >= half {
                            self.log_env[half]
                        } else {
                            let f = src - i as f32;
                            self.log_env[i] * (1.0 - f) + self.log_env[i + 1] * f
                        }
                    } else {
                        self.log_env[k]
                    };
                }
            }

            // 3) Picchi.
            let max_mag = self.magnitude.iter().cloned().fold(0.0f32, f32::max);
            let floor = max_mag * 3e-4;
            self.peaks.clear();
            for k in 2..half - 1 {
                let m = self.magnitude[k];
                if m > floor
                    && m > self.magnitude[k - 1]
                    && m >= self.magnitude[k + 1]
                    && m > self.magnitude[k - 2]
                    && m >= self.magnitude[k + 2]
                {
                    self.peaks.push(k);
                }
            }

            for c in self.out_spectrum.iter_mut() {
                *c = Complex::default();
            }
            if self.peaks.is_empty() {
                self.out_spectrum.copy_from_slice(&self.spectrum);
                state.rotation.fill(0.0);
            }
            let count = self.peaks.len();
            let mut lo = 0usize;
            for p in 0..count {
                let kp = self.peaks[p];
                // Fine della zona: la riga piu' debole tra questo picco e il prossimo.
                let hi = if p + 1 < count {
                    let next = self.peaks[p + 1];
                    let mut best = kp;
                    for k in kp..=next {
                        if self.magnitude[k] < self.magnitude[best] {
                            best = k;
                        }
                    }
                    best.max(kp)
                } else {
                    half
                };

                // Frequenza esatta del picco (rad/campione) dalla rotazione della fase.
                let bin_omega = TAU * kp as f32 / n as f32;
                let omega = if state.has_prev {
                    let dev = wrap(self.phase[kp] - state.prev_phase[kp] - bin_omega * hop) / hop;
                    if dev.abs() <= TAU * 2.0 / n as f32 {
                        bin_omega + dev
                    } else {
                        bin_omega
                    }
                } else {
                    bin_omega
                };
                let true_bin = omega * n as f32 / TAU;
                // Spostamento in righe (anche frazionario): ogni riga della zona
                // viene distribuita tra le due righe di arrivo piu' vicine, cosi'
                // il picco cade esattamente sulla frequenza nuova.
                let shift = true_bin * (ratio - 1.0);
                let shift_int = shift.floor();
                let frac = if self.fractional { shift - shift_int } else { 0.0 };
                let shift_int = if self.fractional { shift_int as isize } else { shift.round() as isize };
                // La fase dell'armonica spostata gira alla velocita' della
                // frequenza nuova: la rotazione accumula la differenza.
                let rot = wrap(state.prev_rotation[kp] + hop * omega * (ratio - 1.0));

                for k in lo..=hi {
                    state.rotation[k] = rot;
                    let ph = self.phase[k] + rot;
                    let (c, s) = (ph.cos(), ph.sin());
                    for (offset, weight) in [(0isize, 1.0 - frac), (1, frac)] {
                        if weight <= 0.0 {
                            continue;
                        }
                        let dest = k as isize + shift_int + offset;
                        if dest < 0 || dest as usize > half {
                            continue;
                        }
                        let dest = dest as usize;
                        let mut mag = self.magnitude[k] * weight;
                        if need_env {
                            let gain = (self.target_env[dest] - self.log_env[k]).clamp(-2.8, 2.8);
                            mag *= gain.exp();
                        }
                        self.out_spectrum[dest] = self.out_spectrum[dest] + Complex::new(mag * c, mag * s);
                    }
                }
                lo = hi + 1;
                if lo > half {
                    break;
                }
            }
            // Riga 0 e ultima: reali.
            self.out_spectrum[0] = Complex::new(self.out_spectrum[0].re, 0.0);
            self.out_spectrum[half] = Complex::new(self.out_spectrum[half].re, 0.0);
            for k in 1..half {
                self.out_spectrum[n - k] = self.out_spectrum[k].conj();
            }
        }

        state.prev_phase.copy_from_slice(&self.phase);
        state.prev_rotation.copy_from_slice(&state.rotation);
        state.has_prev = true;

        // 4) Ritorno nel tempo, finestra di sintesi, somma sovrapposta.
        self.fft.process(&mut self.out_spectrum, true);
        let scale = self.norm / n as f32;
        for i in 0..n {
            self.frame_out[i] = self.out_spectrum[(i + half) & (n - 1)].re * scale * self.window[i];
        }
        // Il frame copre l'ingresso [now - n + 1, now]: ogni campione esce
        // n campioni dopo, cioe' dal prossimo istante (now + 1) in poi.
        let base = now as usize + 1;
        for i in 0..n {
            state.accum[(base + i) & (2 * n - 1)] += self.frame_out[i];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: &[f32], sr: f32, pitch: GrainPitch) -> (Vec<f32>, usize) {
        let mut pv = PhaseVocoder::new(1, sr);
        let latency = pv.latency();
        let mut out = Vec::with_capacity(input.len());
        for &x in input {
            let mut o = [0.0];
            pv.process(&[x], &mut o, |_| pitch);
            out.push(o[0]);
        }
        (out, latency)
    }

    #[test]
    fn unvoiced_passes_through_exactly() {
        let sr = 48000.0;
        let input: Vec<f32> = (0..20000).map(|i| ((i as f32 * 0.013).sin() + (i as f32 * 0.31).cos() * 0.3) * 0.5).collect();
        let (out, latency) = run(&input, sr, GrainPitch { period: 200.0, ratio: 1.0, voiced: false, formant: 1.0 });
        for i in 8000..input.len() {
            assert!((out[i] - input[i - latency]).abs() < 1e-3, "campione {i}: {} vs {}", out[i], input[i - latency]);
        }
    }

    #[test]
    fn shifts_a_harmonic_tone_up_a_semitone() {
        let sr = 48000.0;
        let f0 = 220.0;
        let input: Vec<f32> = (0..48000)
            .map(|i| {
                let t = i as f32 / sr;
                (1..8).map(|h| (TAU * f0 * h as f32 * t).sin() / h as f32).sum::<f32>() * 0.3
            })
            .collect();
        let ratio = 2f32.powf(1.0 / 12.0);
        let (out, _) = run(&input, sr, GrainPitch { period: sr / f0, ratio, voiced: true, formant: 1.0 });
        // Frequenza fondamentale in uscita con l'autocorrelazione.
        let seg = &out[20000..40000];
        let target = f0 * ratio;
        let lag_for = |f: f32| sr / f;
        let best = (150..260)
            .map(|lag| {
                let s: f32 = seg[..18000].iter().zip(&seg[lag..lag + 18000]).map(|(a, b)| a * b).sum();
                (lag, s)
            })
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap()
            .0;
        assert!((best as f32 - lag_for(target)).abs() <= 1.0, "periodo {best} invece di {}", lag_for(target));
        // Livello simile all'ingresso.
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        let ratio_db = 20.0 * (rms(seg) / rms(&input[20000..40000])).log10();
        assert!(ratio_db.abs() < 1.5, "livello {ratio_db:.1} dB");
    }
}
