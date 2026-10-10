//! FFT (trasformata di Fourier veloce) radix-2, scritta da zero.
//!
//! La FFT trasforma un pezzo di segnale nel suo spettro di frequenze, e viceversa.
//! Qui la usiamo per un trucco di velocita': calcolare l'autocorrelazione
//! (quanto il segnale "somiglia a se stesso" spostato nel tempo) in O(N log N)
//! invece che in O(N²). Il rilevatore di pitch ne ha bisogno centinaia di volte
//! al secondo.

use std::f64::consts::PI;
use std::ops::{Add, Mul, Sub};

/// Un numero complesso: parte reale + parte immaginaria.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Complex {
    pub re: f32,
    pub im: f32,
}

impl Complex {
    pub const ZERO: Complex = Complex { re: 0.0, im: 0.0 };

    #[inline]
    pub fn new(re: f32, im: f32) -> Self {
        Self { re, im }
    }

    /// Il coniugato: stessa parte reale, parte immaginaria col segno opposto.
    #[inline]
    pub fn conj(self) -> Self {
        Self::new(self.re, -self.im)
    }

    #[inline]
    pub fn scale(self, k: f32) -> Self {
        Self::new(self.re * k, self.im * k)
    }
}

impl Add for Complex {
    type Output = Complex;
    #[inline]
    fn add(self, o: Complex) -> Complex {
        Complex::new(self.re + o.re, self.im + o.im)
    }
}

impl Sub for Complex {
    type Output = Complex;
    #[inline]
    fn sub(self, o: Complex) -> Complex {
        Complex::new(self.re - o.re, self.im - o.im)
    }
}

impl Mul for Complex {
    type Output = Complex;
    #[inline]
    fn mul(self, o: Complex) -> Complex {
        Complex::new(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
}

/// Una FFT di dimensione fissa `n` (potenza di 2). Le tabelle vengono preparate
/// una volta sola nel costruttore, cosi' `process` non alloca memoria: e'
/// fondamentale nel thread audio, dove un'allocazione puo' causare "click".
pub struct Fft {
    n: usize,
    /// Fattori di rotazione e^(-2πik/n) per k = 0..n/2.
    twiddles: Vec<Complex>,
    /// Tabella dell'ordine "bit-reversed" usato dall'algoritmo iterativo.
    bit_reverse: Vec<u32>,
}

impl Fft {
    pub fn new(n: usize) -> Self {
        assert!(n >= 2 && n.is_power_of_two(), "la dimensione della FFT deve essere una potenza di 2");

        let twiddles = (0..n / 2)
            .map(|k| {
                let angle = -2.0 * PI * k as f64 / n as f64;
                Complex::new(angle.cos() as f32, angle.sin() as f32)
            })
            .collect();

        let bits = n.trailing_zeros();
        let bit_reverse = (0..n as u32)
            .map(|i| i.reverse_bits() >> (32 - bits))
            .collect();

        Self { n, twiddles, bit_reverse }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// Trasforma `data` sul posto.
    ///
    /// - `inverse = false`: X[k] = Σ x[j]·e^(-2πijk/n)  (dal tempo alle frequenze)
    /// - `inverse = true`:  x[j] = Σ X[k]·e^(+2πijk/n)  (dalle frequenze al tempo)
    ///
    /// L'inversa NON divide per `n`: lo fa chi la usa, quando serve.
    pub fn process(&self, data: &mut [Complex], inverse: bool) {
        let n = self.n;
        assert_eq!(data.len(), n);

        // 1) Riordina i campioni in ordine bit-reversed.
        for i in 0..n {
            let j = self.bit_reverse[i] as usize;
            if j > i {
                data.swap(i, j);
            }
        }

        // 2) Combina "farfalle" di dimensione 2, 4, 8, ... fino a n.
        let mut size = 2;
        while size <= n {
            let half = size / 2;
            let step = n / size;
            for start in (0..n).step_by(size) {
                for k in 0..half {
                    let mut w = self.twiddles[k * step];
                    if inverse {
                        w = w.conj();
                    }
                    let a = data[start + k];
                    let b = data[start + k + half] * w;
                    data[start + k] = a + b;
                    data[start + k + half] = a - b;
                }
            }
            size *= 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Confronta la FFT con la definizione "lenta" della trasformata.
    #[test]
    fn fft_matches_naive_dft() {
        let n = 64;
        let fft = Fft::new(n);
        let input: Vec<Complex> = (0..n)
            .map(|i| Complex::new((i as f32 * 0.37).sin(), (i as f32 * 0.11).cos() * 0.5))
            .collect();

        let mut fast = input.clone();
        fft.process(&mut fast, false);

        for k in 0..n {
            let mut acc = (0.0f64, 0.0f64);
            for (j, x) in input.iter().enumerate() {
                let angle = -2.0 * PI * (j * k) as f64 / n as f64;
                acc.0 += x.re as f64 * angle.cos() - x.im as f64 * angle.sin();
                acc.1 += x.re as f64 * angle.sin() + x.im as f64 * angle.cos();
            }
            assert!((fast[k].re as f64 - acc.0).abs() < 1e-3, "bin {k}");
            assert!((fast[k].im as f64 - acc.1).abs() < 1e-3, "bin {k}");
        }

        // Andata e ritorno: inversa / n deve ridare il segnale originale.
        fft.process(&mut fast, true);
        for (a, b) in fast.iter().zip(&input) {
            assert!((a.re / n as f32 - b.re).abs() < 1e-5);
            assert!((a.im / n as f32 - b.im).abs() < 1e-5);
        }
    }
}
