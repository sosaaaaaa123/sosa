//! Riconoscimento della tonalita' anche da musica **polifonica** (accordi,
//! la base della canzone): per AutoKey.
//!
//! Il rilevatore della voce (YIN) segue una nota alla volta. Qui invece si
//! guarda tutto lo spettro: ogni ~40 ms si cercano i picchi (le note e le
//! loro armoniche, anche di piu' strumenti insieme), si porta ogni picco
//! sulla sua nota (Do, Do#, ... Si, senza ottava) e si sommano le energie:
//! il "cromagramma". Una base in La minore, per esempio, ha molto La, Do, Mi
//! e poco Do#, Fa#, Sol#.
//!
//! La tonalita' e' quella il cui profilo (Krumhansl e Kessler: quanto pesa
//! ogni grado della scala in un brano tonale) somiglia di piu' al
//! cromagramma.
//!
//! Tutta la memoria viene allocata in [`ChromaAnalyzer::new`]: si puo' usare
//! nel thread audio.

use crate::fft::{Complex, Fft};
use std::f32::consts::PI;

pub struct ChromaAnalyzer {
    sample_rate: f32,
    n: usize,
    hop: usize,
    ring: Vec<f32>,
    pos: usize,
    countdown: usize,
    window: Vec<f32>,
    fft: Fft,
    buf: Vec<Complex>,
    mag: Vec<f32>,
    lo_bin: usize,
    hi_bin: usize,
    chroma: [f32; 12],
    /// Secondi di musica (non di silenzio) gia' ascoltati.
    seconds: f32,
    reference_hz: f32,
}

impl ChromaAnalyzer {
    /// Note piu' basse e piu' alte considerate (Hz).
    const LOWEST_HZ: f32 = 60.0;
    const HIGHEST_HZ: f32 = 5000.0;

    pub fn new(sample_rate: f32) -> Self {
        // ~170 ms di finestra: abbastanza per separare due semitoni vicini
        // anche sulle note basse (a 100 Hz distano 6 Hz).
        let n = ((sample_rate * 0.17) as usize).next_power_of_two().max(1024);
        let hop = n / 4;
        let window = (0..n).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos()).collect();
        let bin = |hz: f32| ((hz * n as f32 / sample_rate) as usize).clamp(2, n / 2 - 2);
        Self {
            sample_rate,
            n,
            hop,
            ring: vec![0.0; n],
            pos: 0,
            countdown: hop,
            window,
            fft: Fft::new(n),
            buf: vec![Complex::default(); n],
            mag: vec![0.0; n / 2 + 1],
            lo_bin: bin(Self::LOWEST_HZ),
            hi_bin: bin(Self::HIGHEST_HZ),
            chroma: [0.0; 12],
            seconds: 0.0,
            reference_hz: 440.0,
        }
    }

    /// Accordatura (frequenza del La4).
    pub fn set_reference(&mut self, hz: f32) {
        self.reference_hz = hz.clamp(400.0, 480.0);
    }

    /// Ricomincia da zero.
    pub fn reset(&mut self) {
        self.chroma = [0.0; 12];
        self.seconds = 0.0;
        self.ring.fill(0.0);
        self.countdown = self.hop;
    }

    /// Energia accumulata per ciascuna delle 12 note (Do = 0).
    pub fn chroma(&self) -> [f32; 12] {
        self.chroma
    }

    /// Secondi di musica ascoltati (il silenzio non conta).
    pub fn seconds(&self) -> f32 {
        self.seconds
    }

    /// Un campione (mono).
    #[inline]
    pub fn process(&mut self, x: f32) {
        self.ring[self.pos] = x;
        self.pos = (self.pos + 1) % self.n;
        self.countdown -= 1;
        if self.countdown == 0 {
            self.countdown = self.hop;
            self.analyze();
        }
    }

    fn analyze(&mut self) {
        let n = self.n;
        let mut energy = 0.0f32;
        for i in 0..n {
            let x = self.ring[(self.pos + i) % n];
            energy += x * x;
            self.buf[i] = Complex::new(x * self.window[i], 0.0);
        }
        // Silenzio (sotto -60 dBFS di media): non dice niente sulla tonalita'.
        if energy / (n as f32) < 1e-6 {
            return;
        }
        self.fft.process(&mut self.buf, false);
        let mut max_mag = 0.0f32;
        for k in self.lo_bin - 1..=self.hi_bin + 1 {
            let c = self.buf[k];
            self.mag[k] = (c.re * c.re + c.im * c.im).sqrt();
            max_mag = max_mag.max(self.mag[k]);
        }
        if max_mag <= 0.0 {
            return;
        }
        let floor = max_mag * 0.01;
        let mut frame = [0.0f32; 12];
        for k in self.lo_bin..=self.hi_bin {
            let (a, b, c) = (self.mag[k - 1], self.mag[k], self.mag[k + 1]);
            if b <= floor || b <= a || b < c {
                continue;
            }
            // Frequenza esatta del picco (parabola sui logaritmi).
            let (la, lb, lc) = (a.max(1e-12).ln(), b.ln(), c.max(1e-12).ln());
            let den = la - 2.0 * lb + lc;
            let offset = if den.abs() > 1e-9 { (0.5 * (la - lc) / den).clamp(-0.5, 0.5) } else { 0.0 };
            let hz = (k as f32 + offset) * self.sample_rate / n as f32;
            let note = 69.0 + 12.0 * (hz / self.reference_hz).log2();
            let nearest = note.round();
            // I picchi a meta' tra due note (rumore, battimenti) contano poco.
            let off = (note - nearest) / 0.2;
            let tuning = (-off * off).exp();
            // Le armoniche alte aggiungono soprattutto quinte e terze "finte":
            // pesano meno delle note di base.
            let height = 1.0 / (1.0 + hz / 1200.0);
            let pc = (nearest as i32).rem_euclid(12) as usize;
            frame[pc] += b.sqrt() * tuning * height;
        }
        let total: f32 = frame.iter().sum();
        if total <= 0.0 {
            return;
        }
        // Ogni pezzo di musica conta uguale, forte o piano.
        for (acc, v) in self.chroma.iter_mut().zip(frame) {
            *acc += v / total;
        }
        self.seconds += self.hop as f32 / self.sample_rate;
    }
}

/// Profili di Krumhansl e Kessler: quanto "pesa" ogni grado della scala.
const MAJOR_PROFILE: [f32; 12] = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const MINOR_PROFILE: [f32; 12] = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];

fn correlation(a: &[f32; 12], b: &[f32; 12]) -> f32 {
    let ma = a.iter().sum::<f32>() / 12.0;
    let mb = b.iter().sum::<f32>() / 12.0;
    let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for i in 0..12 {
        let (x, y) = (a[i] - ma, b[i] - mb);
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    ab / (aa * bb).sqrt().max(1e-12)
}

/// La tonalita' relativa (stesse note): La minore per Do maggiore e viceversa.
pub fn relative_key(tonic: usize, minor: bool) -> (usize, bool) {
    if minor {
        ((tonic + 3) % 12, false)
    } else {
        ((tonic + 9) % 12, true)
    }
}

/// La tonalita' piu' probabile per un cromagramma: (tonica 0..11, minore?,
/// affidabilita' 0..1). `None` se il cromagramma e' vuoto o piatto.
///
/// L'affidabilita' guarda quanto la migliore stacca le altre tonalita',
/// esclusa la sua relativa: Do maggiore e La minore hanno le stesse note, e
/// per l'intonazione la scala e' la stessa (sbagliare tra le due non cambia
/// nessuna nota corretta).
pub fn key_from_chroma(chroma: &[f32; 12]) -> Option<(usize, bool, f32)> {
    let total: f32 = chroma.iter().sum();
    if total <= 0.0 {
        return None;
    }
    let mut scores = [[0.0f32; 2]; 12];
    let mut best = (0usize, false, f32::MIN);
    for (tonic, row) in scores.iter_mut().enumerate() {
        for (minor, profile) in [(false, &MAJOR_PROFILE), (true, &MINOR_PROFILE)] {
            let rotated: [f32; 12] = std::array::from_fn(|i| profile[(i + 12 - tonic) % 12]);
            let r = correlation(chroma, &rotated);
            row[minor as usize] = r;
            if r > best.2 {
                best = (tonic, minor, r);
            }
        }
    }
    if best.2 < 0.3 {
        return None;
    }
    let relative = relative_key(best.0, best.1);
    let mut second = f32::MIN;
    for (tonic, row) in scores.iter().enumerate() {
        for minor in [false, true] {
            if (tonic, minor) == (best.0, best.1) || (tonic, minor) == relative {
                continue;
            }
            second = second.max(row[minor as usize]);
        }
    }
    // Affidabile se somiglia bene e stacca le altre tonalita'.
    let confidence = (((best.2 - second) / 0.08).clamp(0.0, 1.0) * ((best.2 - 0.3) / 0.4).clamp(0.0, 1.0)).sqrt();
    Some((best.0, best.1, confidence))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scale::note_to_hz;

    /// Accordi suonati da un "pianoforte" sintetico (armoniche che calano).
    fn chords(sr: f32, chords: &[[f32; 3]], seconds_each: f32) -> Vec<f32> {
        let len = (sr * seconds_each) as usize;
        let mut out = Vec::new();
        for chord in chords {
            for i in 0..len {
                let t = i as f32 / sr;
                let env = (-t * 1.5).exp();
                let mut v = 0.0;
                for &note in chord {
                    // basso un'ottava sotto per la prima nota dell'accordo
                    for (k, &n) in [note - 12.0, note].iter().enumerate() {
                        if k == 0 && note != chord[0] {
                            continue;
                        }
                        let f = note_to_hz(n);
                        for h in 1..8 {
                            v += (2.0 * PI * f * h as f32 * t).sin() / (h * h) as f32;
                        }
                    }
                }
                out.push(0.1 * env * v);
            }
        }
        out
    }

    fn detect(audio: &[f32], sr: f32) -> (usize, bool) {
        let mut a = ChromaAnalyzer::new(sr);
        for &x in audio {
            a.process(x);
        }
        let k = key_from_chroma(&a.chroma()).expect("tonalita'");
        (k.0, k.1)
    }

    #[test]
    fn finds_major_and_minor_keys_from_chords() {
        let sr = 48000.0;
        // Do maggiore: C - F - G - C (I IV V I), poi Am - F - G - C.
        let c = [60.0, 64.0, 67.0];
        let f = [65.0, 69.0, 72.0];
        let g = [67.0, 71.0, 74.0];
        let am = [69.0, 72.0, 76.0];
        let audio = chords(sr, &[c, f, g, c, am, f, g, c], 1.0);
        assert_eq!(detect(&audio, sr), (0, false), "Do maggiore");
        // La minore: Am - Dm - E - Am (con il Sol# della minore armonica).
        let dm = [62.0, 65.0, 69.0];
        let e = [64.0, 68.0, 71.0];
        let audio = chords(sr, &[am, dm, e, am, am, dm, e, am], 1.0);
        assert_eq!(detect(&audio, sr), (9, true), "La minore");
        // Sol minore (Gm - Cm - D - Gm): tonica fuori dal "Do".
        let gm = [67.0, 70.0, 74.0];
        let cm = [60.0, 63.0, 67.0];
        let d = [62.0, 66.0, 69.0];
        let audio = chords(sr, &[gm, cm, d, gm, gm, cm, d, gm], 1.0);
        assert_eq!(detect(&audio, sr), (7, true), "Sol minore");
    }

    /// Una base "vera": accordi, basso 808, cassa, rullante e charleston
    /// (la batteria piu' forte degli accordi).
    fn beat(sr: f32, chords: &[[f32; 3]], seconds: f32) -> Vec<f32> {
        let n = (sr * seconds) as usize;
        let mut seed = 12345u32;
        let mut noise = move || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 9) as f32 / (1u32 << 23) as f32 - 0.5
        };
        let mut prev_noise = 0.0;
        (0..n)
            .map(|i| {
                let t = i as f32 / sr;
                let chord = chords[(t as usize) % chords.len()];
                let tc = t.fract();
                // Accordi (pad).
                let mut pad = 0.0;
                for &note in &chord {
                    let f = note_to_hz(note);
                    for h in 1..6 {
                        pad += (2.0 * PI * f * h as f32 * t).sin() / (h * h) as f32;
                    }
                }
                // 808 sulla fondamentale, due ottave sotto.
                let bass = (2.0 * PI * note_to_hz(chord[0] - 24.0) * t).sin() * (-tc * 2.0).exp();
                // Batteria a 120 bpm.
                let beat_t = (t * 2.0).fract() / 2.0;
                let beat_n = (t * 2.0) as usize;
                let kick_f = 45.0 + 75.0 * (-beat_t * 30.0).exp();
                let kick = (2.0 * PI * kick_f * beat_t).sin() * (-beat_t * 12.0).exp();
                let w = noise();
                let snare = if beat_n % 2 == 1 { w * (-beat_t * 25.0).exp() } else { 0.0 };
                let hat_t = (t * 4.0).fract() / 4.0;
                let hat = (w - prev_noise) * (-hat_t * 80.0).exp();
                prev_noise = w;
                0.08 * pad + 0.35 * bass + 0.6 * kick + 0.5 * snare + 0.3 * hat
            })
            .collect()
    }

    #[test]
    fn finds_the_key_of_a_beat_with_drums() {
        let sr = 44100.0;
        // Sol minore: Gm - Eb - Bb - F.
        let gm = [55.0, 58.0, 62.0];
        let eb = [51.0, 55.0, 58.0];
        let bb = [58.0, 62.0, 65.0];
        let f = [53.0, 57.0, 60.0];
        let audio = beat(sr, &[gm, eb, bb, f], 8.0);
        let mut a = ChromaAnalyzer::new(sr);
        for &x in &audio {
            a.process(x);
        }
        let (tonic, minor, confidence) = key_from_chroma(&a.chroma()).expect("tonalita'");
        // Sol minore o la sua relativa Si bemolle maggiore (stesse note).
        assert!((tonic, minor) == (7, true) || (tonic, minor) == (10, false), "{tonic} {minor}");
        assert!(confidence >= 0.5, "affidabilita' {confidence}: AutoKey non la imposterebbe");
        assert!(a.seconds() > 6.0);
    }

    #[test]
    fn relative_keys_have_the_same_notes() {
        assert_eq!(relative_key(0, false), (9, true));
        assert_eq!(relative_key(9, true), (0, false));
        assert_eq!(relative_key(7, true), (10, false));
    }

    #[test]
    fn silence_counts_nothing() {
        let mut a = ChromaAnalyzer::new(44100.0);
        for _ in 0..44100 {
            a.process(0.0);
        }
        assert_eq!(a.seconds(), 0.0);
        assert!(key_from_chroma(&a.chroma()).is_none());
    }
}
