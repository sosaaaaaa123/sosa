//! Formanti in **Low Latency** (latenza zero), come il tasto Formant e la
//! manopola Throat di Auto-Tune.
//!
//! La testina a latenza zero (`zerolat.rs`) sposta la voce come un nastro
//! piu' veloce o piu' lento: insieme alla nota si spostano anche le formanti
//! (le risonanze di gola e bocca), e trasponendo molto la voce diventa uno
//! "scoiattolo" o un "orco". Questo filtro le rimette al loro posto:
//!
//! 1. sulla voce gia' spostata misura il timbro con la **predizione lineare**
//!    (LPC, sugli ultimi ~24 ms);
//! 2. un primo filtro toglie quel timbro ("sbianca" la voce);
//! 3. un secondo filtro rimette lo stesso timbro **stirato in frequenza**:
//!    diviso il rapporto di pitch (formanti di nuovo dov'erano), e
//!    moltiplicato per Throat (gola piu' corta o piu' lunga).
//!
//! Con fattore 1 (correzione delle formanti spenta, o nota non spostata e
//! Throat a 100) i due filtri si annullano esattamente: il suono resta
//! identico.
//!
//! Tutti i filtri guardano solo il passato: la latenza resta zero. Sono
//! filtri a traliccio ("lattice") con i coefficienti di riflessione, che
//! restano stabili anche cambiando i coefficienti campione per campione.
//!
//! Stirare il timbro di un fattore `r` vuol dire ricalcolare il filtro da
//! un'autocorrelazione "stirata": R'(k) = R(r k) ha lo spettro S(f / r),
//! cioe' le stesse risonanze a frequenze moltiplicate per `r`.
//!
//! Tutta la memoria viene allocata in [`FormantFilter::new`]: si puo' usare
//! nel thread audio.

use crate::MAX_CHANNELS;

pub struct FormantFilter {
    order: usize,
    hop: usize,
    countdown: usize,
    /// Ultimi campioni (mono, con pre-enfasi) per l'analisi.
    history: Vec<f32>,
    pos: usize,
    prev_input: f32,
    window: Vec<f32>,
    frame: Vec<f64>,
    /// Autocorrelazione fino al ritardo `max_lag` (in doppia precisione: lo
    /// spettro di una voce va da 0 a -60 dB e oltre).
    r: Vec<f64>,
    rw: Vec<f64>,
    lag_window: Vec<f64>,
    levinson_a: Vec<f64>,
    levinson_tmp: Vec<f64>,
    levinson_k: Vec<f64>,
    /// Coefficienti di riflessione: analisi (sbiancare) e sintesi (rimettere
    /// le formanti), da / verso, e quelli di questo campione.
    ka_from: Vec<f32>,
    ka_to: Vec<f32>,
    kb_from: Vec<f32>,
    kb_to: Vec<f32>,
    ka: Vec<f32>,
    kb: Vec<f32>,
    t: f32,
    /// Stati dei filtri a traliccio, per canale.
    analysis_state: Vec<Vec<f32>>,
    synthesis_state: Vec<Vec<f32>>,
    /// Volume: potenza media (~30 ms) della voce di riferimento e di quella
    /// filtrata, per canale. Spostando le formanti l'energia si sposta tra
    /// frequenze che la pendenza della voce pesa in modo diverso: senza
    /// questo il volume cambierebbe di qualche dB.
    reference_power: Vec<f32>,
    output_power: Vec<f32>,
    power_smoothing: f32,
    /// Rete di sicurezza: livello di picco della voce di riferimento e di
    /// quella con le formanti rimesse (per canale), e quanto in fretta calano.
    reference_level: Vec<f32>,
    output_level: Vec<f32>,
    level_release: f32,
}

impl FormantFilter {
    /// Spostamento massimo delle formanti (un'ottava in su o in giu').
    pub const MAX_WARP: f32 = 2.0;
    /// Pre-enfasi dell'analisi: il filtro descrive le risonanze (la gola),
    /// non la pendenza dello spettro della sorgente, che resta nel residuo.
    const PRE_EMPHASIS: f32 = 0.97;
    /// Rumore bianco aggiunto all'analisi (rispetto all'energia): -40 dB.
    /// Le risonanze stimate non diventano mai troppo strette (un filtro troppo
    /// "appuntito" squillerebbe), e le formanti vengono stimate bene lo stesso
    /// (misurato sulle vocali di prova: errore del timbro uguale o minore che
    /// con -60 dB).
    const NOISE_FLOOR: f64 = 1e-4;
    /// Larghezza (Hz) della finestra sulle autocorrelazioni: lo spettro
    /// stimato e' liscio, segue le formanti e non le singole armoniche. Piu'
    /// stretta, sulle voci gravi la stima cambia da un ciclo all'altro della
    /// voce e il filtro produce brevi soffi negli acuti (misurato su voci
    /// vere: da 60 "clic" a 1 con 300-400 Hz, formanti stimate quasi uguali).
    const SMOOTHING_HZ: f64 = 400.0;

    pub fn new(sample_rate: f32, channels: usize) -> Self {
        let order = ((sample_rate / 2000.0).round() as usize + 4).clamp(12, 32);
        let hop = ((sample_rate * 0.004) as usize).max(16);
        let len = ((sample_rate * 0.024) as usize).max(4 * order);
        // Finestra asimmetrica: sale lentamente e scende in fretta sugli
        // ultimi campioni, cosi' il filtro segue la voce con poco ritardo.
        let rise = len * 3 / 4;
        let fall = len - rise;
        let window = (0..len)
            .map(|i| {
                if i < rise {
                    let x = (std::f32::consts::FRAC_PI_2 * i as f32 / rise as f32).sin();
                    x * x
                } else {
                    (std::f32::consts::FRAC_PI_2 * (i - rise) as f32 / fall as f32).cos()
                }
            })
            .collect();
        let max_lag = (order as f32 * Self::MAX_WARP).ceil() as usize + 3;
        // Finestra sulle autocorrelazioni (gaussiana, ~100 Hz): lo spettro
        // stimato e' liscio, segue le formanti e non le singole armoniche.
        let bandwidth = Self::SMOOTHING_HZ;
        let lag_window = (0..=order)
            .map(|k| {
                let x = 2.0 * std::f64::consts::PI * bandwidth * k as f64 / sample_rate as f64;
                (-0.5 * x * x).exp()
            })
            .collect();
        let channels = channels.clamp(1, MAX_CHANNELS);
        Self {
            order,
            hop,
            countdown: hop,
            history: vec![0.0; len],
            pos: 0,
            prev_input: 0.0,
            window,
            frame: vec![0.0; len],
            r: vec![0.0; max_lag + 1],
            rw: vec![0.0; order + 1],
            lag_window,
            levinson_a: vec![0.0; order + 1],
            levinson_tmp: vec![0.0; order + 1],
            levinson_k: vec![0.0; order],
            ka_from: vec![0.0; order],
            ka_to: vec![0.0; order],
            kb_from: vec![0.0; order],
            kb_to: vec![0.0; order],
            ka: vec![0.0; order],
            kb: vec![0.0; order],
            t: 1.0,
            analysis_state: vec![vec![0.0; order]; channels],
            synthesis_state: vec![vec![0.0; order]; channels],
            reference_power: vec![0.0; channels],
            output_power: vec![0.0; channels],
            power_smoothing: (-1.0 / (0.03 * sample_rate)).exp(),
            reference_level: vec![0.0; channels],
            output_level: vec![0.0; channels],
            level_release: (-1.0 / (0.005 * sample_rate)).exp(),
        }
    }

    pub fn reset(&mut self) {
        self.history.fill(0.0);
        self.pos = 0;
        self.prev_input = 0.0;
        self.countdown = self.hop;
        for v in [&mut self.ka_from, &mut self.ka_to, &mut self.kb_from, &mut self.kb_to, &mut self.ka, &mut self.kb] {
            v.fill(0.0);
        }
        self.t = 1.0;
        for s in self.analysis_state.iter_mut().chain(self.synthesis_state.iter_mut()) {
            s.fill(0.0);
        }
        self.reference_power.fill(0.0);
        self.output_power.fill(0.0);
        self.reference_level.fill(0.0);
        self.output_level.fill(0.0);
    }

    /// Ordine del filtro (numero di coefficienti).
    pub fn order(&self) -> usize {
        self.order
    }

    /// Un campione nuovo (mono) per l'analisi. `warp` = di quanto stirare
    /// le formanti (1 = timbro invariato, 1,26 = 4 semitoni piu' in alto,
    /// cioe' una gola piu' corta). Va chiamata una volta per campione, prima
    /// di [`whiten`](Self::whiten) e [`color`](Self::color).
    #[inline]
    pub fn push(&mut self, mono: f32, warp: f32) {
        let x = if mono.is_finite() { mono } else { 0.0 };
        self.history[self.pos] = x - Self::PRE_EMPHASIS * self.prev_input;
        self.prev_input = x;
        self.pos = (self.pos + 1) % self.history.len();

        self.countdown -= 1;
        if self.countdown == 0 {
            self.countdown = self.hop;
            self.analyze(warp);
        }
        // Coefficienti di questo campione: passaggio graduale dai vecchi ai
        // nuovi lungo un hop (niente gradini, niente clic).
        if self.t < 1.0 {
            self.t = (self.t + 1.0 / self.hop as f32).min(1.0);
            let t = self.t;
            for i in 0..self.order {
                self.ka[i] = self.ka_from[i] + (self.ka_to[i] - self.ka_from[i]) * t;
                self.kb[i] = self.kb_from[i] + (self.kb_to[i] - self.kb_from[i]) * t;
            }
        }
    }

    fn analyze(&mut self, warp: f32) {
        let len = self.history.len();
        let mut energy = 0.0f64;
        for i in 0..len {
            let x = (self.history[(self.pos + i) % len] * self.window[i]) as f64;
            self.frame[i] = x;
            energy += x * x;
        }
        // Silenzio: si tengono i filtri di prima (si annullano a vicenda).
        if energy < 1e-10 * len as f64 {
            return;
        }
        let max_lag = self.r.len() - 1;
        for lag in 0..=max_lag {
            let mut acc = 0.0f64;
            for i in lag..len {
                acc += self.frame[i] * self.frame[i - lag];
            }
            self.r[lag] = acc;
        }
        self.r[0] *= 1.0 + Self::NOISE_FLOOR;

        // Il punto di partenza: i coefficienti di adesso.
        self.ka_from.copy_from_slice(&self.ka);
        self.kb_from.copy_from_slice(&self.kb);

        // Analisi: il timbro di adesso.
        for k in 0..=self.order {
            self.rw[k] = self.r[k] * self.lag_window[k];
        }
        levinson(&self.rw, &mut self.levinson_k, &mut self.levinson_a, &mut self.levinson_tmp);
        for (dst, &k) in self.ka_to.iter_mut().zip(&self.levinson_k) {
            *dst = k as f32;
        }

        // Sintesi: lo stesso timbro, oppure spostato di `warp`.
        let warp = warp.clamp(1.0 / Self::MAX_WARP, Self::MAX_WARP);
        if (warp - 1.0).abs() < 1e-4 {
            self.kb_to.copy_from_slice(&self.ka_to);
        } else {
            for k in 0..=self.order {
                self.rw[k] = interpolate(&self.r, k as f64 * warp as f64) * self.lag_window[k];
            }
            self.rw[0] = self.r[0] * self.lag_window[0];
            levinson(&self.rw, &mut self.levinson_k, &mut self.levinson_a, &mut self.levinson_tmp);
            for (dst, &k) in self.kb_to.iter_mut().zip(&self.levinson_k) {
                *dst = k as f32;
            }
        }
        self.t = 0.0;
    }

    /// Toglie il timbro misurato (filtro di analisi a traliccio, solo passato).
    #[inline]
    pub fn whiten(&mut self, ch: usize, x: f32) -> f32 {
        let state = &mut self.analysis_state[ch];
        // f: errore "in avanti"; state[i] = errore "all'indietro" di ordine i
        // al campione precedente.
        let mut f = x;
        let mut b_prev = x;
        for (i, &k) in self.ka.iter().enumerate() {
            let b_delayed = state[i];
            let f_next = f + k * b_delayed;
            let b_next = b_delayed + k * f;
            state[i] = b_prev;
            b_prev = b_next;
            f = f_next;
        }
        f
    }

    /// Rimette il timbro, stirato di `warp` (filtro di sintesi a traliccio,
    /// tutti poli).
    ///
    /// `reference` e' la stessa voce senza questo trattamento: serve per il
    /// volume e come rete di sicurezza. Un taglio
    /// netto dopo una nota tenuta lascia nel residuo un colpo che il filtro
    /// farebbe "squillare" sulle risonanze; il risultato non puo' quindi
    /// superare di oltre ~6 dB il livello della voce di riferimento, che in
    /// un caso normale non viene mai toccato.
    #[inline]
    pub fn color(&mut self, ch: usize, e: f32, reference: f32) -> f32 {
        let y = self.synthesize(ch, e);
        // Stesso volume della voce di riferimento.
        let a = self.power_smoothing;
        let rp = &mut self.reference_power[ch];
        *rp = *rp * a + reference * reference * (1.0 - a);
        let op = &mut self.output_power[ch];
        *op = *op * a + y * y * (1.0 - a);
        let y = if *op > 1e-20 { y * (*rp / *op).sqrt().clamp(0.25, 4.0) } else { y };
        let release = self.level_release;
        let ref_level = &mut self.reference_level[ch];
        *ref_level = reference.abs().max(*ref_level * release);
        let out_level = &mut self.output_level[ch];
        *out_level = y.abs().max(*out_level * release);
        let limit = 2.0 * *ref_level + 1e-6;
        let y = if *out_level > limit { y * (limit / *out_level) } else { y };
        y.clamp(-1.25 * limit, 1.25 * limit)
    }

    #[inline]
    fn synthesize(&mut self, ch: usize, e: f32) -> f32 {
        let state = &mut self.synthesis_state[ch];
        let p = self.order;
        let mut f = e;
        // Dall'ordine piu' alto al piu' basso.
        for i in (0..p).rev() {
            let k = self.kb[i];
            f -= k * state[i];
            // Errore all'indietro di ordine i+1 per il prossimo campione.
            if i + 1 < p {
                state[i + 1] = state[i] + k * f;
            }
        }
        state[0] = f;
        if f.is_finite() {
            f
        } else {
            state.fill(0.0);
            0.0
        }
    }
}

/// Autocorrelazione a un ritardo frazionario. Interpolazione lineare: una
/// cubica "rimbalza" tra i ritardi e l'autocorrelazione stirata smette di
/// descrivere uno spettro vero (formanti sbagliate, misurato: errore
/// dell'inviluppo quasi doppio).
fn interpolate(r: &[f64], lag: f64) -> f64 {
    let last = r.len() - 1;
    let i = lag.floor() as usize;
    if i >= last {
        return r[last];
    }
    let t = lag - i as f64;
    r[i] * (1.0 - t) + r[i + 1] * t
}

/// Levinson-Durbin: dai valori di autocorrelazione `r[0..=p]` ai coefficienti
/// di riflessione `k` (A(z) = 1 + a1 z^-1 + ...). Restituisce l'errore di
/// predizione. Coefficienti limitati a |k| < 0,995: filtri sempre stabili.
fn levinson(r: &[f64], k: &mut [f64], a: &mut [f64], tmp: &mut [f64]) -> f64 {
    let p = k.len();
    a.fill(0.0);
    a[0] = 1.0;
    k.fill(0.0);
    let mut err = r[0];
    if err <= 0.0 {
        return 1e-30;
    }
    for i in 1..=p {
        let mut acc = r[i];
        for j in 1..i {
            acc += a[j] * r[i - j];
        }
        let ki = (-acc / err).clamp(-0.995, 0.995);
        tmp[..i].copy_from_slice(&a[..i]);
        for j in 1..i {
            a[j] = tmp[j] + ki * tmp[i - j];
        }
        a[i] = ki;
        k[i - 1] = ki;
        err *= 1.0 - ki * ki;
        if err <= r[0] * 1e-12 {
            break;
        }
    }
    err.max(1e-30)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::fft::{Complex, Fft};
    use crate::synth::{steady_vowel, steady_vowel_formants};
    use std::f32::consts::PI;

    /// Il "timbro" di una vocale tenuta: l'energia relativa di ogni armonica
    /// fino a 5 kHz (alla fine del segnale).
    pub(crate) fn harmonic_profile(signal: &[f32], sr: f32, f0: f32) -> Vec<f32> {
        let n = 8192;
        let fft = Fft::new(n);
        let start = signal.len() - n - 1000;
        let mut data: Vec<Complex> = (0..n)
            .map(|i| Complex::new(signal[start + i] * (0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos()), 0.0))
            .collect();
        fft.process(&mut data, false);
        let power: Vec<f32> = data.iter().take(n / 2).map(|c| c.re * c.re + c.im * c.im).collect();
        let mut profile = Vec::new();
        let mut h = 1.0;
        while h * f0 < 5000.0 {
            let k = (h * f0 * n as f32 / sr).round() as usize;
            profile.push(power[k - 2..=k + 2].iter().cloned().fold(0.0, f32::max));
            h += 1.0;
        }
        let total: f32 = profile.iter().sum();
        profile.iter().map(|p| p / total).collect()
    }

    /// Differenza media (dB) tra due timbri.
    pub(crate) fn timbre_distance(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| (10.0 * (x.max(1e-12) / y.max(1e-12)).log10()).abs()).sum::<f32>() / a.len() as f32
    }

    fn run(input: &[f32], sr: f32, warp: f32) -> Vec<f32> {
        let mut f = FormantFilter::new(sr, 1);
        input
            .iter()
            .map(|&x| {
                f.push(x, warp);
                let e = f.whiten(0, x);
                f.color(0, e, x)
            })
            .collect()
    }

    #[test]
    fn without_warp_the_voice_comes_back_identical() {
        let sr = 48000.0;
        let input = steady_vowel(180.0, sr, 24000);
        let out = run(&input, sr, 1.0);
        let err = input.iter().zip(&out).skip(2000).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(err < 1e-3, "differenza massima {err}");
    }

    #[test]
    fn warp_moves_the_formants_towards_the_ideal() {
        let sr = 48000.0;
        for f0 in [110.0f32, 150.0, 220.0] {
            let input = steady_vowel(f0, sr, 30000);
            for semitones in [4.0f32, -4.0] {
                let warp = 2f32.powf(semitones / 12.0);
                let ideal = harmonic_profile(&steady_vowel_formants(f0, sr, 30000, warp), sr, f0);
                let before = timbre_distance(&harmonic_profile(&input, sr, f0), &ideal);
                let after = timbre_distance(&harmonic_profile(&run(&input, sr, warp), sr, f0), &ideal);
                println!("f0 {f0} Hz, formanti {semitones:+}: distanza dal timbro ideale {before:.1} -> {after:.1} dB");
                assert!(after < 0.85 * before, "f0 {f0}, {semitones:+}: {before:.1} -> {after:.1} dB");
            }
        }
    }

    #[test]
    fn silence_noise_and_loud_input_stay_finite() {
        let sr = 44100.0;
        let mut f = FormantFilter::new(sr, 2);
        let mut seed = 1u32;
        for i in 0..20000 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = (seed >> 9) as f32 / (1u32 << 23) as f32 - 0.5;
            let x = match i / 5000 {
                0 => 0.0,
                1 => noise,
                2 => (i as f32 * 0.3).sin() * 4.0,
                _ => 1e-12,
            };
            f.push(x, 1.6);
            for ch in 0..2 {
                let e = f.whiten(ch, x);
                let y = f.color(ch, e, x);
                assert!(y.is_finite() && y.abs() < 20.0, "campione {i}: {y}");
                // Dopo il taglio netto (a 15000) nessuno squillo che duri.
                if i > 15000 + 2000 {
                    assert!(y.abs() < 1e-3, "campione {i}: {y}");
                }
            }
        }
    }
}
