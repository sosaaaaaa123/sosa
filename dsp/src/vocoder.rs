//! Pitch shifting con un **phase vocoder** "a picchi bloccati" e formanti
//! preservate: il motore della modalita' HQ.
//!
//! Il phase vocoder lavora nel dominio delle frequenze:
//!
//! 1. ogni 5 ms prende un pezzo di voce lungo circa 6 periodi (32-64 ms:
//!    piu' lungo per le voci maschili, che hanno armoniche fitte, piu' corto
//!    per quelle femminili), lo smussa con una finestra di Hann e ne calcola
//!    lo spettro (FFT): un insieme di "righe", una per ogni armonica;
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
//!    con il "true envelope" (cepstrum ripetuto che segue le cime delle
//!    armoniche) e rimesso al suo posto dopo lo spostamento: la voce cambia
//!    nota ma non diventa "da scoiattolo" o "da orco";
//! 6. torna nel tempo (FFT inversa) e somma i pezzi sovrapposti.
//!
//! Dove non c'e' una nota (consonanti, respiri) e sugli attacchi bruschi lo
//! spettro passa **identico**: la ricostruzione e' perfetta, niente sbavature
//! sulle "t" e sulle "s".
//!
//! La latenza e' la finestra piu' lunga (64 ms), compensata dal DAW.

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
    /// Ultimi campioni ricevuti (buffer circolare di `n_fft`).
    input: Vec<f32>,
    /// Uscita in costruzione (buffer circolare di 2·n_fft): ogni frame vi somma il suo pezzo.
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
    /// Dimensione della FFT (potenza di 2) e meta' (+1 = righe utili).
    n_fft: usize,
    half: usize,
    hop: usize,
    /// Finestra di analisi piu' corta e piu' lunga (campioni, pari).
    min_window: usize,
    max_window: usize,
    /// Finestra attuale: segue la voce (circa 6 periodi), con dolcezza.
    window_len: f32,
    /// Periodi della voce dentro una finestra.
    periods_per_window: f32,
    fft: Fft,
    /// FFT piccola per l'inviluppo spettrale (calcolato su righe raggruppate a 4 a 4).
    env_fft: Fft,
    n_env: usize,
    position: u64,
    countdown: usize,
    states: Vec<ChannelState>,
    /// Somma delle finestre (analisi x sintesi) caduta su ogni campione in
    /// uscita: dividendo per lei la ricostruzione e' esatta qualunque sia la
    /// lunghezza delle finestre.
    norm: Vec<f32>,

    // Memoria di lavoro (allocata una volta sola).
    window: Vec<f32>,
    spectrum: Vec<Complex>,
    out_spectrum: Vec<Complex>,
    env_buf: Vec<Complex>,
    env_log: Vec<f32>,
    env_work: Vec<f32>,
    magnitude: Vec<f32>,
    phase: Vec<f32>,
    log_env: Vec<f32>,
    target_env: Vec<f32>,
    peaks: Vec<usize>,
    peak_bin: Vec<f32>,
    frame_out: Vec<f32>,
}

impl PhaseVocoder {
    /// Finestra piu' lunga (s): 64 ms, abbastanza per separare bene le
    /// armoniche anche di un basso (80 Hz). E' anche la latenza.
    const MAX_WINDOW_S: f32 = 0.064;
    /// Finestra piu' corta (s): per le voci acute, che hanno armoniche
    /// lontane tra loro e cambiano in fretta.
    const MIN_WINDOW_S: f32 = 0.032;
    /// Un frame ogni ~5 ms.
    const HOP_S: f32 = 0.0053;
    /// Quanti periodi della voce stanno in una finestra.
    const PERIODS_PER_WINDOW: f32 = 6.0;
    /// Iterazioni del "true envelope" (inviluppo che passa sopra le armoniche).
    const ENVELOPE_ITERATIONS: usize = 4;
    /// Ad ogni iterazione l'inviluppo sale del doppio di quanto le armoniche
    /// gli passano sopra: arriva sulle cime in meno passaggi (con 4 iterazioni
    /// e' preciso come con 8 senza accelerazione).
    const ENVELOPE_ACCEL: f32 = 2.0;

    pub fn new(channels: usize, sample_rate: f32) -> Self {
        Self::with_windows(channels, sample_rate, Self::MIN_WINDOW_S, Self::MAX_WINDOW_S)
    }

    /// Come [`new`](Self::new) con limiti diversi per la finestra (s).
    pub fn with_windows(channels: usize, sample_rate: f32, min_s: f32, max_s: f32) -> Self {
        let channels = channels.clamp(1, MAX_CHANNELS);
        let even = |s: f32| (((sample_rate * s) / 2.0).round() as usize * 2).max(64);
        let max_window = even(max_s);
        let min_window = even(min_s).min(max_window);
        let n_fft = max_window.next_power_of_two();
        let half = n_fft / 2;
        let hop = ((sample_rate * Self::HOP_S).round() as usize).clamp(16, min_window / 4);
        let n_env = (n_fft / 4).max(64);
        let states = (0..channels)
            .map(|_| ChannelState {
                input: vec![0.0; n_fft],
                accum: vec![0.0; 2 * n_fft],
                prev_phase: vec![0.0; half + 1],
                prev_rotation: vec![0.0; half + 1],
                rotation: vec![0.0; half + 1],
                has_prev: false,
                energy: 0.0,
            })
            .collect();
        Self {
            channels,
            n_fft,
            half,
            hop,
            min_window,
            max_window,
            window_len: max_window as f32,
            periods_per_window: Self::PERIODS_PER_WINDOW,
            fft: Fft::new(n_fft),
            env_fft: Fft::new(n_env),
            n_env,
            position: 0,
            countdown: hop,
            states,
            norm: vec![0.0; 2 * n_fft],
            window: vec![0.0; max_window],
            spectrum: vec![Complex::default(); n_fft],
            out_spectrum: vec![Complex::default(); n_fft],
            env_buf: vec![Complex::default(); n_env],
            env_log: vec![0.0; n_env / 2 + 1],
            env_work: vec![0.0; n_env / 2 + 1],
            magnitude: vec![0.0; half + 1],
            phase: vec![0.0; half + 1],
            log_env: vec![0.0; half + 1],
            target_env: vec![0.0; half + 1],
            peaks: Vec::with_capacity(half),
            peak_bin: Vec::with_capacity(half),
            frame_out: vec![0.0; max_window],
        }
    }

    /// Ritardo tra ingresso e uscita (campioni): la finestra piu' lunga.
    pub fn latency(&self) -> usize {
        self.max_window
    }

    /// Lunghezza attuale della finestra di analisi (campioni).
    pub fn window_len(&self) -> usize {
        self.window_len.round() as usize
    }

    pub fn reset(&mut self) {
        self.position = 0;
        self.countdown = self.hop;
        self.window_len = self.max_window as f32;
        self.norm.fill(0.0);
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
        let n = self.n_fft;
        let now = self.position;
        for ch in 0..self.channels {
            self.states[ch].input[now as usize & (n - 1)] = input.get(ch).copied().unwrap_or(0.0);
        }

        self.countdown -= 1;
        if self.countdown == 0 {
            self.countdown = self.hop;
            // Centro del frame: a meta' della finestra piu' lunga, dietro
            // l'ultimo campione arrivato.
            let center = now as i64 - (self.max_window / 2) as i64;
            if center >= 0 {
                let pitch = pitch_at(center as f64);
                self.prepare_window(pitch);
                let len = self.window_len();
                let mut same = false;
                for ch in 0..self.channels {
                    if ch == 1 {
                        // Voce mono su traccia stereo (i due canali identici):
                        // si calcola una volta sola e si copia.
                        same = (0..len).all(|i| {
                            let idx = (center as usize + n - len / 2 + i) & (n - 1);
                            self.states[0].input[idx] == self.states[1].input[idx]
                        });
                    }
                    if ch == 1 && same {
                        let (first, rest) = self.states.split_at_mut(1);
                        let (a, b) = (&first[0], &mut rest[0]);
                        b.prev_phase.copy_from_slice(&a.prev_phase);
                        b.prev_rotation.copy_from_slice(&a.prev_rotation);
                        b.rotation.copy_from_slice(&a.rotation);
                        b.has_prev = a.has_prev;
                        b.energy = a.energy;
                        self.add_frame(1, center, len);
                    } else {
                        self.process_frame(ch, center, len, pitch);
                        self.add_frame(ch, center, len);
                    }
                }
                // Somma delle finestre (una volta per tutti i canali).
                let mask = 2 * n - 1;
                let base = center as usize + self.max_window - len / 2;
                for i in 0..len {
                    self.norm[(base + i) & mask] += self.window[i] * self.window[i];
                }
            }
        }

        let index = now as usize & (2 * n - 1);
        let norm = self.norm[index];
        let scale = if norm > 1e-3 { 1.0 / norm } else { 0.0 };
        for ch in 0..self.channels {
            let s = &mut self.states[ch];
            if let Some(o) = output.get_mut(ch) {
                *o = s.accum[index] * scale;
            }
            s.accum[index] = 0.0;
        }
        self.norm[index] = 0.0;
        self.position += 1;
    }

    /// Sceglie la lunghezza della finestra per questo frame e la prepara.
    fn prepare_window(&mut self, pitch: GrainPitch) {
        if pitch.voiced && pitch.period > 1.0 {
            let target = (pitch.period * self.periods_per_window).clamp(self.min_window as f32, self.max_window as f32);
            // Cambia al massimo del 15% per frame: niente salti.
            let step = 0.15 * self.window_len;
            self.window_len += (target - self.window_len).clamp(-step, step);
        }
        let len = self.window_len();
        let len = (len / 2 * 2).clamp(self.min_window, self.max_window);
        self.window_len = len as f32;
        // Hann simmetrica di lunghezza `len`.
        for i in 0..len {
            let x = PI * (i as f32 + 0.5) / len as f32;
            let s = x.sin();
            self.window[i] = s * s;
        }
    }

    /// Somma il frame appena calcolato (in `frame_out`) nell'uscita del canale.
    fn add_frame(&mut self, ch: usize, center: i64, len: usize) {
        let mask = 2 * self.n_fft - 1;
        // Il campione dell'ingresso `t` esce all'istante `t + latenza`.
        let base = center as usize + self.max_window - len / 2;
        let state = &mut self.states[ch];
        for i in 0..len {
            state.accum[(base + i) & mask] += self.frame_out[i];
        }
    }

    /// Inviluppo spettrale (le formanti) con il "true envelope": il cepstrum
    /// lisciato viene ripetuto alzandolo dove le armoniche gli passano
    /// sopra, cosi' segue le cime delle armoniche invece di stare a meta'
    /// tra armoniche e valli (importante per le voci maschili, che hanno
    /// armoniche fitte). Calcolato su righe raggruppate a 4 a 4: l'inviluppo
    /// e' una curva lenta e cosi' costa un sedicesimo.
    fn spectral_envelope(&mut self, period: f32) {
        let n_env = self.n_env;
        let eh = n_env / 2;
        let group = self.n_fft / n_env;
        for q in 0..=eh {
            let lo = (q * group).saturating_sub(group / 2);
            let hi = (q * group + group / 2).min(self.half);
            let mut sum = 0.0f32;
            for k in lo..=hi {
                sum += self.magnitude[k];
            }
            self.env_log[q] = (sum / (hi - lo + 1) as f32 + 1e-9).ln();
        }
        // Sotto la prima armonica la voce non ha niente: lo spettro crolla
        // verso lo zero e l'inviluppo, lisciato, lo seguirebbe gia' sopra la
        // fondamentale. Spostando le formanti (o la nota) quel crollo finirebbe
        // sulle prime armoniche, che sono le piu' forti: voce svuotata o
        // rimbombante, soprattutto nelle voci maschili. Li' sotto si prosegue
        // allora piatto, al livello della prima armonica.
        let first = eh as f32 * 2.0 / period.max(2.0);
        let a = ((0.7 * first).floor() as usize).min(eh);
        let b = ((1.3 * first).ceil() as usize).clamp(a, eh);
        let mut peak = a;
        for q in a..=b {
            if self.env_log[q] > self.env_log[peak] {
                peak = q;
            }
        }
        let level = self.env_log[peak];
        for v in &mut self.env_log[..peak] {
            *v = level;
        }
        // Le quefrenze sono in campioni: teniamo quelle sotto meta' periodo.
        let cut = (0.5 * period).clamp(6.0, (n_env / 4) as f32) as usize;
        self.env_work.copy_from_slice(&self.env_log);
        for iteration in 0..Self::ENVELOPE_ITERATIONS {
            for k in 0..n_env {
                let v = if k <= eh { self.env_work[k] } else { self.env_work[n_env - k] };
                self.env_buf[k] = Complex::new(v, 0.0);
            }
            self.env_fft.process(&mut self.env_buf, true);
            for q in 0..n_env {
                let qq = q.min(n_env - q);
                let w = if qq < cut {
                    1.0
                } else if qq < cut + 3 {
                    0.5 + 0.5 * (PI * (qq - cut) as f32 / 3.0).cos()
                } else {
                    0.0
                };
                let c = self.env_buf[q];
                self.env_buf[q] = Complex::new(c.re * w / n_env as f32, 0.0);
            }
            self.env_fft.process(&mut self.env_buf, false);
            let last = iteration + 1 == Self::ENVELOPE_ITERATIONS;
            for q in 0..=eh {
                let smooth = self.env_buf[q].re;
                self.env_work[q] = if last {
                    smooth
                } else {
                    smooth + Self::ENVELOPE_ACCEL * (self.env_log[q] - smooth).max(0.0)
                };
            }
        }
        // Di nuovo su tutte le righe (interpolazione lineare).
        for k in 0..=self.half {
            let pos = k as f32 / group as f32;
            let i = (pos.floor() as usize).min(eh - 1);
            let f = (pos - i as f32).min(1.0);
            self.log_env[k] = self.env_work[i] * (1.0 - f) + self.env_work[i + 1] * f;
        }
    }

    fn process_frame(&mut self, ch: usize, center: i64, len: usize, pitch: GrainPitch) {
        let n = self.n_fft;
        let half = self.half;
        let hop = self.hop as f32;
        let state = &mut self.states[ch];

        // 1) Finestra + "fase zero" (il centro del frame va all'indice 0).
        for c in self.spectrum.iter_mut() {
            *c = Complex::default();
        }
        let first = center as usize + n - len / 2;
        let mut new_energy = 0.0f32;
        for i in 0..len {
            let x = state.input[(first + i) & (n - 1)];
            if i >= len - self.hop {
                new_energy += x * x;
            }
            self.spectrum[(i + n - len / 2) & (n - 1)] = Complex::new(x * self.window[i], 0.0);
        }
        new_energy /= self.hop as f32;
        // Attacco brusco: l'ultimo pezzo della finestra e' molto piu' forte
        // della media recente.
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
        let state = &mut self.states[ch];
        if !pitch.voiced || onset {
            // Niente nota (o attacco): lo spettro passa identico e le
            // rotazioni ripartono da zero.
            state.rotation.fill(0.0);
        }
        let passthrough = !pitch.voiced || (!shifting && state.prev_rotation.iter().all(|&r| r == 0.0)) || onset;

        if passthrough {
            self.out_spectrum.copy_from_slice(&self.spectrum);
            self.states[ch].rotation.fill(0.0);
        } else {
            // 2) Inviluppo spettrale (formanti).
            let need_env = (ratio - 1.0).abs() > 1e-5 || (formant - 1.0).abs() > 1e-5;
            if need_env {
                self.spectral_envelope(pitch.period);
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

            // 3) Picchi: massimi locali su mezzo lobo della finestra (con
            // finestre corte e FFT lunga ogni armonica occupa piu' righe).
            let state = &mut self.states[ch];
            let reach = ((2 * n) / len).max(2);
            let max_mag = self.magnitude.iter().cloned().fold(0.0f32, f32::max);
            let floor = max_mag * 3e-4;
            self.peaks.clear();
            self.peak_bin.clear();
            let mut k = reach;
            while k + reach <= half {
                let m = self.magnitude[k];
                if m > floor && (k - reach..=k + reach).all(|j| j == k || self.magnitude[j] < m || (j > k && self.magnitude[j] <= m)) {
                    // Frequenza esatta (in righe) dalla rotazione della fase.
                    let bin_omega = TAU * k as f32 / n as f32;
                    let omega = if state.has_prev {
                        let dev = wrap(self.phase[k] - state.prev_phase[k] - bin_omega * hop) / hop;
                        if dev.abs() <= TAU * reach as f32 / n as f32 {
                            bin_omega + dev
                        } else {
                            bin_omega
                        }
                    } else {
                        bin_omega
                    };
                    self.peaks.push(k);
                    self.peak_bin.push(omega * n as f32 / TAU);
                    k += reach;
                    continue;
                }
                k += 1;
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
                    best
                } else {
                    half
                };
                let true_bin = self.peak_bin[p];
                {
                    // Spostamento in righe (anche frazionario): ogni riga della
                    // zona viene distribuita tra le due righe di arrivo piu'
                    // vicine, cosi' il picco cade esattamente sulla frequenza nuova.
                    let shift = true_bin * (ratio - 1.0);
                    let shift_floor = shift.floor();
                    let frac = shift - shift_floor;
                    let shift_int = shift_floor as isize;
                    // La fase dell'armonica spostata gira alla velocita' della
                    // frequenza nuova: la rotazione accumula la differenza.
                    let omega = TAU * true_bin / n as f32;
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

        let state = &mut self.states[ch];
        state.prev_phase.copy_from_slice(&self.phase);
        state.prev_rotation.copy_from_slice(&state.rotation);
        state.has_prev = true;

        // 5) Ritorno nel tempo e finestra di sintesi.
        self.fft.process(&mut self.out_spectrum, true);
        let scale = 1.0 / n as f32;
        for i in 0..len {
            self.frame_out[i] = self.out_spectrum[(i + n - len / 2) & (n - 1)].re * scale * self.window[i];
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
