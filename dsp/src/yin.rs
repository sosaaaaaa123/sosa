//! Rilevamento dell'intonazione con l'algoritmo YIN
//! (de Cheveigné & Kawahara, 2002), accelerato con la FFT.
//!
//! Idea di base: un suono intonato e' periodico, cioe' si ripete uguale ogni
//! T campioni. Se confronto il segnale con se stesso spostato di τ campioni,
//! la differenza e' minima proprio quando τ = T. YIN cerca quel minimo.
//!
//! Passi:
//! 1. funzione differenza  d(τ) = Σ (x[j] - x[j+τ])²
//! 2. normalizzazione cumulativa d'(τ) = d(τ) / media(d(1..τ))  → evita di
//!    scegliere per sbaglio τ = 0 e rende la soglia indipendente dal volume
//! 3. primo τ sotto la soglia (0.15) → il periodo
//! 4. interpolazione parabolica → precisione sotto il campione
//!
//! Il punto 1 si puo' riscrivere come  d(τ) = E(0) + E(τ) - 2·r(τ), dove E sono
//! energie (calcolate con somme cumulative) e r(τ) e' una correlazione, che la
//! FFT calcola in un colpo solo per tutti i τ.

use crate::fft::{Complex, Fft};
use crate::scale::HIGHEST_HZ;

/// Il risultato di un'analisi.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PitchEstimate {
    /// Periodo dell'onda in campioni (con decimali). Frequenza = sample_rate / periodo.
    pub period: f32,
    /// Quanto e' "pulito" il pitch: 1 = perfettamente periodico, 0 = rumore.
    pub clarity: f32,
    /// `true` se c'e' una nota cantata (suono intonato), `false` per silenzio,
    /// respiri, consonanti come "s" o "f".
    pub voiced: bool,
}

impl PitchEstimate {
    pub const UNVOICED: PitchEstimate = PitchEstimate { period: 0.0, clarity: 0.0, voiced: false };
}

pub struct YinDetector {
    sample_rate: f32,
    /// Lunghezza della finestra di integrazione (campioni).
    window: usize,
    /// Periodo minimo e massimo cercati (campioni).
    tau_min: usize,
    tau_max: usize,
    /// Il periodo massimo per cui sono stati dimensionati i buffer.
    capacity_tau_max: usize,
    /// Soglia di YIN: piu' bassa = piu' severo nel dire "c'e' una nota".
    threshold: f32,
    /// Sotto questo volume (RMS) consideriamo il segnale silenzio.
    silence_rms: f32,

    fft: Fft,
    spectrum: Vec<Complex>,
    product: Vec<Complex>,
    cmndf: Vec<f32>,
    energy: Vec<f64>,
}

impl YinDetector {
    /// `lowest_hz_capacity`: la nota piu' grave che potra' mai essere richiesta
    /// con [`set_lowest_hz`](Self::set_lowest_hz). Tutta la memoria viene
    /// allocata qui.
    pub fn new(sample_rate: f32, lowest_hz_capacity: f32) -> Self {
        let capacity_tau_max = (sample_rate / lowest_hz_capacity).ceil() as usize;
        // La finestra e' lunga quanto il periodo massimo; il frame analizzato e'
        // finestra + periodo massimo. La FFT deve contenerlo senza "avvolgersi".
        let fft_len = (2 * capacity_tau_max).next_power_of_two();

        let mut detector = Self {
            sample_rate,
            window: capacity_tau_max,
            tau_min: 2,
            tau_max: capacity_tau_max,
            capacity_tau_max,
            threshold: 0.15,
            silence_rms: 10f32.powf(-50.0 / 20.0), // -50 dBFS
            fft: Fft::new(fft_len),
            spectrum: vec![Complex::ZERO; fft_len],
            product: vec![Complex::ZERO; fft_len],
            cmndf: vec![1.0; capacity_tau_max + 2],
            energy: vec![0.0; 2 * capacity_tau_max + 1],
        };
        detector.set_lowest_hz(lowest_hz_capacity);
        detector
    }

    /// Cambia la nota piu' grave riconosciuta (senza allocare memoria).
    pub fn set_lowest_hz(&mut self, lowest_hz: f32) {
        self.set_range(lowest_hz, lowest_hz);
    }

    /// Come [`set_lowest_hz`](Self::set_lowest_hz), ma con la finestra di
    /// confronto lunga un periodo di `window_hz` (>= `lowest_hz`): si
    /// riconoscono note fino a `lowest_hz` con frame piu' corti, cioe'
    /// un'analisi piu' pronta nei passaggi veloci.
    pub fn set_range(&mut self, lowest_hz: f32, window_hz: f32) {
        let tau_max = (self.sample_rate / lowest_hz).ceil() as usize;
        self.tau_max = tau_max.clamp(4, self.capacity_tau_max);
        let window = (self.sample_rate / window_hz.max(lowest_hz)).ceil() as usize;
        self.window = window.clamp(4, self.tau_max);
        self.tau_min = ((self.sample_rate / HIGHEST_HZ).floor() as usize).max(2);
    }

    /// Quanto e' "esigente" il rilevatore: soglia della YIN tra 0.05 (molto
    /// severo, riconosce solo voci pulitissime) e 0.4 (molto tollerante, anche
    /// voci con molta aria, ma con piu' rischio di errori).
    pub fn set_threshold(&mut self, threshold: f32) {
        self.threshold = threshold.clamp(0.05, 0.4);
    }

    /// Il periodo piu' lungo riconosciuto, in campioni.
    pub fn max_period(&self) -> usize {
        self.tau_max
    }

    /// Il periodo piu' lungo che i buffer possono gestire.
    pub fn capacity_max_period(&self) -> usize {
        self.capacity_tau_max
    }

    /// Quanti campioni servono a [`detect`](Self::detect).
    pub fn frame_len(&self) -> usize {
        self.window + self.tau_max
    }

    /// Il numero massimo di campioni che `frame_len` potra' mai restituire.
    pub fn max_frame_len(&self) -> usize {
        2 * self.capacity_tau_max
    }

    /// La lunghezza della finestra di integrazione (campioni).
    pub fn window_len(&self) -> usize {
        self.window
    }

    /// Analizza gli ultimi `frame_len()` campioni e restituisce il pitch.
    /// Non alloca memoria.
    pub fn detect(&mut self, frame: &[f32]) -> PitchEstimate {
        let w = self.window;
        let tau_max = self.tau_max;
        let len = w + tau_max;
        assert!(frame.len() >= len, "frame troppo corto per il rilevatore");
        let frame = &frame[frame.len() - len..];

        // --- Silenzio? -------------------------------------------------------
        self.energy[0] = 0.0;
        for (i, &x) in frame.iter().enumerate() {
            self.energy[i + 1] = self.energy[i] + (x as f64) * (x as f64);
        }
        let rms = (self.energy[len] / len as f64).sqrt() as f32;
        if rms < self.silence_rms {
            return PitchEstimate::UNVOICED;
        }

        // --- Correlazione r(τ) = Σ_{j<w} x[j]·x[j+τ] con la FFT ---------------
        // Trucco: una sola FFT complessa per due segnali reali.
        //   parte reale      a = primi w campioni (poi zeri)
        //   parte immaginaria b = tutto il frame
        let n = self.fft.len();
        for (j, z) in self.spectrum.iter_mut().enumerate() {
            let a = if j < w { frame[j] } else { 0.0 };
            let b = if j < len { frame[j] } else { 0.0 };
            *z = Complex::new(a, b);
        }
        self.fft.process(&mut self.spectrum, false);

        // Separa gli spettri A e B, poi calcola conj(A)·B (= spettro della correlazione).
        for k in 0..n {
            let zk = self.spectrum[k];
            let zn = self.spectrum[(n - k) % n].conj();
            let a = (zk + zn).scale(0.5);
            let diff = zk - zn;
            let b = Complex::new(diff.im * 0.5, -diff.re * 0.5); // diff / (2i)
            self.product[k] = a.conj() * b;
        }
        self.fft.process(&mut self.product, true);
        let inv_n = 1.0 / n as f32;

        // --- Funzione differenza normalizzata (CMNDF) --------------------------
        let e0 = self.energy[w];
        let mut running_sum = 0.0f64;
        self.cmndf[0] = 1.0;
        for tau in 1..=tau_max {
            let e_tau = self.energy[tau + w] - self.energy[tau];
            let corr = (self.product[tau].re * inv_n) as f64;
            let d = (e0 + e_tau - 2.0 * corr).max(0.0);
            running_sum += d;
            self.cmndf[tau] = if running_sum > 1e-12 {
                (d * tau as f64 / running_sum) as f32
            } else {
                1.0
            };
        }

        // --- Scelta del periodo -------------------------------------------------
        // Il minimo assoluto ci dice se c'e' una nota (se e' sopra la soglia,
        // il suono non e' abbastanza periodico: consonante, respiro, rumore).
        let tau_min = self.tau_min.max(2);
        let global_min = self.cmndf[tau_min..tau_max]
            .iter()
            .fold(f32::INFINITY, |m, &v| m.min(v));
        if global_min >= self.threshold {
            return PitchEstimate::UNVOICED;
        }

        // Poi prendiamo il PRIMO avvallamento "quasi profondo quanto il minimo".
        // La YIN originale prende il primo sotto la soglia fissa, ma con voci
        // che hanno una seconda armonica molto forte (formante vicina al
        // doppio della nota) a meta' periodo c'e' un avvallamento ingannevole
        // e la nota verrebbe letta un'ottava sopra. Confrontarlo col minimo
        // vero evita l'errore.
        let accept = global_min + (self.threshold - global_min) * 0.3;
        let mut tau = tau_min;
        while tau < tau_max && self.cmndf[tau] >= accept {
            tau += 1;
        }
        while tau + 1 < tau_max && self.cmndf[tau + 1] < self.cmndf[tau] {
            tau += 1;
        }
        let tau = tau.min(tau_max - 1);

        // --- Interpolazione parabolica per la precisione sub-campione ----------
        let s0 = self.cmndf[tau - 1];
        let s1 = self.cmndf[tau];
        let s2 = self.cmndf[tau + 1];
        let denom = s0 - 2.0 * s1 + s2;
        let shift = if denom.abs() > 1e-9 {
            (0.5 * (s0 - s2) / denom).clamp(-0.5, 0.5)
        } else {
            0.0
        };

        PitchEstimate {
            period: tau as f32 + shift,
            clarity: (1.0 - s1).clamp(0.0, 1.0),
            voiced: true,
        }
    }
}

impl YinDetector {
    /// Dopo [`detect`](Self::detect): c'e' un buon avvallamento anche vicino
    /// a `period` (entro ±6%)? Se si', restituisce quella stima. Serve al
    /// correttore per scartare gli errori d'ottava verso il basso: con una
    /// voce rauca i cicli si alternano (uno forte, uno debole) e il doppio del
    /// periodo puo' sembrare "piu' periodico" del periodo vero, ma anche il
    /// periodo vero resta un avvallamento netto sotto la soglia.
    pub fn dip_near(&self, period: f32) -> Option<PitchEstimate> {
        let tau_min = self.tau_min.max(2);
        let lo = ((period * 0.94).floor() as usize).max(tau_min);
        let hi = ((period * 1.06).ceil() as usize).min(self.tau_max.saturating_sub(1));
        if lo + 2 > hi {
            return None;
        }
        let tau = (lo..=hi).min_by(|&a, &b| self.cmndf[a].total_cmp(&self.cmndf[b]))?;
        // Deve essere un vero minimo locale, non il bordo della ricerca.
        if tau <= lo || tau >= hi || self.cmndf[tau] >= self.threshold {
            return None;
        }
        let (s0, s1, s2) = (self.cmndf[tau - 1], self.cmndf[tau], self.cmndf[tau + 1]);
        let denom = s0 - 2.0 * s1 + s2;
        let shift = if denom.abs() > 1e-9 { (0.5 * (s0 - s2) / denom).clamp(-0.5, 0.5) } else { 0.0 };
        Some(PitchEstimate { period: tau as f32 + shift, clarity: (1.0 - s1).clamp(0.0, 1.0), voiced: true })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;

    fn detect_hz(signal: &[f32], sr: f32) -> Option<f32> {
        let mut yin = YinDetector::new(sr, 60.0);
        let len = yin.frame_len();
        let est = yin.detect(&signal[signal.len() - len..]);
        est.voiced.then(|| sr / est.period)
    }

    fn cents(a: f32, b: f32) -> f32 {
        1200.0 * (a / b).log2()
    }

    #[test]
    fn detects_sines_and_voice() {
        let sr = 48000.0;
        for &hz in &[82.4, 110.0, 196.0, 261.63, 440.0, 659.3, 987.8] {
            let sine: Vec<f32> = (0..4096)
                .map(|i| 0.5 * (2.0 * std::f32::consts::PI * hz * i as f32 / sr).sin())
                .collect();
            let got = detect_hz(&sine, sr).expect("sinusoide non riconosciuta");
            assert!(cents(got, hz).abs() < 3.0, "sinusoide {hz} Hz → {got} Hz");

        }

        // La vocale sintetica su tutta l'estensione vocale, a quarti di tono:
        // nessun errore d'ottava ammesso.
        for quarter in 0..=180 {
            let note = 38.0 + quarter as f32 * 0.25;
            let hz = crate::scale::note_to_hz(note);
            let voice = synth::steady_vowel(hz, sr, 4096);
            let got = detect_hz(&voice, sr).expect("voce non riconosciuta");
            assert!(cents(got, hz).abs() < 3.0, "voce {hz:.1} Hz → {got:.1} Hz");
        }
    }

    #[test]
    fn silence_and_noise_are_unvoiced() {
        let sr = 44100.0;
        assert_eq!(detect_hz(&vec![0.0; 4096], sr), None);
        let noise = synth::noise(4096, 0.3, 1234);
        assert_eq!(detect_hz(&noise, sr), None);
    }

    #[test]
    fn lowest_hz_is_respected() {
        let sr = 48000.0;
        let mut yin = YinDetector::new(sr, 60.0);
        yin.set_lowest_hz(150.0);
        assert_eq!(yin.max_period(), 320);
        assert_eq!(yin.frame_len(), 640);
        let voice = synth::steady_vowel(300.0, sr, 2048);
        let est = yin.detect(&voice);
        assert!(est.voiced);
        assert!(cents(sr / est.period, 300.0).abs() < 3.0);
    }
}
