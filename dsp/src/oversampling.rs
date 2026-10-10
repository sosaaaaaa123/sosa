//! # Sovracampionamento 2x con filtri "half-band"
//!
//! Una saturazione (valvola, trasformatore) crea armoniche nuove. Quelle che
//! superano la meta' della frequenza di campionamento "si ripiegano" e
//! diventano note stonate che non c'erano (aliasing): e' il suono ruvido e
//! digitale delle saturazioni economiche. Per evitarlo si lavora a una
//! frequenza piu' alta (4 volte, con due stadi da 2x) e si filtra prima di
//! tornare alla frequenza originale.
//!
//! I filtri sono FIR a fase lineare "half-band" (meta' dei coefficienti sono
//! zero, quindi costano poco) progettati con una finestra di Kaiser per circa
//! 110 dB di reiezione. La fase lineare significa che il ritardo e' fisso e
//! uguale per tutte le frequenze: il DAW lo compensa e il suono non cambia.

/// Progetta un filtro half-band di `taps` coefficienti (`taps = 4q + 3`).
/// Banda passante fino a ~0.21 della frequenza a cui lavora il filtro
/// (0.42 di quella originale), banda bloccata da ~0.29 (0.58).
pub fn design_halfband(taps: usize, kaiser_beta: f64) -> Vec<f64> {
    assert!(taps % 4 == 3, "un filtro half-band deve avere 4q+3 coefficienti");
    let center = (taps - 1) / 2;
    let i0_beta = bessel_i0(kaiser_beta);
    let mut h = vec![0.0f64; taps];
    let mut side_sum = 0.0;
    for (n, tap) in h.iter_mut().enumerate() {
        let k = n as i64 - center as i64;
        if k == 0 {
            continue;
        }
        if k % 2 == 0 {
            // Gli zeri tipici dei filtri half-band: esattamente zero.
            continue;
        }
        let t = k as f64 / 2.0;
        let sinc = (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t);
        let r = 2.0 * n as f64 / (taps - 1) as f64 - 1.0;
        let window = bessel_i0(kaiser_beta * (1.0 - r * r).max(0.0).sqrt()) / i0_beta;
        *tap = 0.5 * sinc * window;
        side_sum += *tap;
    }
    // Guadagno esattamente 1 in continua: centro 0.5, il resto somma 0.5.
    for (n, tap) in h.iter_mut().enumerate() {
        if n != center {
            *tap *= 0.5 / side_sum;
        }
    }
    h[center] = 0.5;
    h
}

/// Funzione di Bessel modificata I0 (serve alla finestra di Kaiser).
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let half = x / 2.0;
    for k in 1..60 {
        term *= (half / k as f64) * (half / k as f64);
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

/// Buffer circolare "doppio": ogni campione e' scritto due volte, cosi' gli
/// ultimi `len` campioni sono sempre una fetta contigua (niente modulo nel
/// ciclo dei coefficienti).
struct History {
    data: Vec<f32>,
    len: usize,
    pos: usize,
}

impl History {
    fn new(len: usize) -> Self {
        Self { data: vec![0.0; 2 * len], len, pos: 0 }
    }

    /// Inserisce un campione; `recent()[0]` e' il piu' nuovo.
    #[inline]
    fn push(&mut self, x: f32) {
        self.pos = if self.pos == 0 { self.len - 1 } else { self.pos - 1 };
        self.data[self.pos] = x;
        self.data[self.pos + self.len] = x;
    }

    #[inline]
    fn recent(&self) -> &[f32] {
        &self.data[self.pos..self.pos + self.len]
    }

    fn clear(&mut self) {
        self.data.fill(0.0);
        self.pos = 0;
    }
}

/// Raddoppia la frequenza di campionamento: un campione in, due fuori.
pub struct Upsampler2 {
    /// I coefficienti di posto pari, gia' moltiplicati per 2.
    even_taps: Vec<f32>,
    history: History,
    /// Posizione del coefficiente centrale nel ramo dispari.
    middle: usize,
}

impl Upsampler2 {
    pub fn new(design: &[f64]) -> Self {
        let center = (design.len() - 1) / 2;
        let even_taps: Vec<f32> = design.iter().step_by(2).map(|&h| (2.0 * h) as f32).collect();
        let len = even_taps.len();
        Self { even_taps, history: History::new(len), middle: (center - 1) / 2 }
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> [f32; 2] {
        self.history.push(x);
        let recent = self.history.recent();
        let even = dot(&self.even_taps, recent);
        // Nel ramo dispari c'e' solo il coefficiente centrale (0.5 · 2 = 1).
        let odd = recent[self.middle];
        [even, odd]
    }

    pub fn reset(&mut self) {
        self.history.clear();
    }
}

/// Dimezza la frequenza di campionamento: due campioni in, uno fuori.
pub struct Downsampler2 {
    even_taps: Vec<f32>,
    even_history: History,
    odd_history: History,
    /// Quanti campioni dispari indietro si trova il coefficiente centrale.
    middle: usize,
}

impl Downsampler2 {
    pub fn new(design: &[f64]) -> Self {
        let center = (design.len() - 1) / 2;
        let even_taps: Vec<f32> = design.iter().step_by(2).map(|&h| h as f32).collect();
        let len = even_taps.len();
        Self {
            even_taps,
            even_history: History::new(len),
            odd_history: History::new(len),
            middle: (center + 1) / 2,
        }
    }

    /// `pair` = (campione pari, campione dispari) alla frequenza doppia.
    #[inline]
    pub fn process(&mut self, pair: [f32; 2]) -> f32 {
        self.even_history.push(pair[0]);
        self.odd_history.push(pair[1]);
        dot(&self.even_taps, self.even_history.recent())
            + 0.5 * self.odd_history.recent()[self.middle]
    }

    pub fn reset(&mut self) {
        self.even_history.clear();
        self.odd_history.clear();
    }
}

#[inline]
fn dot(a: &[f32], b: &[f32]) -> f32 {
    // Quattro accumulatori: il compilatore li trasforma in istruzioni SIMD.
    let n = a.len().min(b.len());
    let (a, b) = (&a[..n], &b[..n]);
    let mut acc = [0.0f32; 4];
    let chunks = n / 4;
    for i in 0..chunks {
        for j in 0..4 {
            acc[j] += a[4 * i + j] * b[4 * i + j];
        }
    }
    let mut sum = (acc[0] + acc[1]) + (acc[2] + acc[3]);
    for i in 4 * chunks..n {
        sum += a[i] * b[i];
    }
    sum
}

/// Ritardo di un passaggio su e giu' (in campioni alla frequenza di ingresso).
pub fn round_trip_delay(taps: usize) -> usize {
    (taps - 1) / 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_is_a_pure_delay_for_audio() {
        let design = design_halfband(91, 11.2);
        let mut up = Upsampler2::new(&design);
        let mut down = Downsampler2::new(&design);
        let delay = round_trip_delay(91);
        let sr = 48000.0f32;
        for &freq in &[100.0f32, 1000.0, 8000.0, 18000.0] {
            up.reset();
            down.reset();
            let n = 4000;
            let w = 2.0 * std::f64::consts::PI * freq as f64 / sr as f64;
            let input: Vec<f32> = (0..n).map(|i| (w * i as f64).sin() as f32).collect();
            let output: Vec<f32> = input.iter().map(|&x| down.process(up.process(x))).collect();
            let mut err: f32 = 0.0;
            for i in 1000..n {
                err = err.max((output[i] - input[i - delay]).abs());
            }
            assert!(err < 2e-5, "{freq} Hz: errore {err}");
        }
    }
}
