//! Il timbro di HQ in **Low Latency** (latenza zero).
//!
//! La testina a latenza zero (`zerolat.rs`) sposta la voce come un nastro
//! piu' veloce o piu' lento: insieme alla nota si spostano anche le formanti
//! (le risonanze di gola e bocca), cosi' la Low Latency suonava come un altro
//! autotune (piu' chiaro correggendo in su, piu' scuro in giu', e il comando
//! Formant non agiva). Qui si usa lo stesso calcolo di HQ:
//!
//! 1. ogni ~5 ms, sulla voce appena arrivata (solo il passato: niente
//!    latenza) si misura l'inviluppo con il "true envelope" di HQ e si
//!    calcola il guadagno per ogni frequenza che rimette le formanti al loro
//!    posto e applica il comando Formant (`PhaseVocoder::timbre_gain`);
//! 2. quel guadagno diventa un filtro a **fase minima** (risposta tutta
//!    "in avanti", nessun ritardo: il cepstrum piegato), lungo 8 ms;
//! 3. il filtro si applica alla voce spostata dalla testina, passando dal
//!    filtro vecchio al nuovo con una dissolvenza di un passo.
//!
//! Quando la nota non si sposta e Formant e' a 0 il filtro e' esattamente
//! neutro (la voce passa identica, campione per campione): consonanti,
//! respiri e note gia' intonate restano come prima.
//!
//! Tutta la memoria viene allocata in [`TimbreFilter::new`]: si puo' usare
//! nel thread audio.

use crate::fft::{Complex, Fft};
use crate::vocoder::PhaseVocoder;
use crate::MAX_CHANNELS;

pub struct TimbreFilter {
    channels: usize,
    analyzer: PhaseVocoder,
    fft: Fft,
    n: usize,
    cepstrum: Vec<Complex>,
    /// Guadagno (logaritmico, per riga) lisciato nel tempo: un filtro nuovo
    /// ogni ~5 ms, ma simile al precedente (in HQ ci pensa la somma dei
    /// pezzi sovrapposti). Passare di colpo tra filtri molto diversi, anche
    /// con una dissolvenza, sommava fasi diverse: piccoli clic negli acuti.
    gain: Vec<f32>,
    gain_follow: f32,
    weight: Vec<f32>,
    /// Ultimi campioni della voce in ingresso (mono, prima della testina).
    input: Vec<f32>,
    input_pos: usize,
    segment: Vec<f32>,
    /// Filtro attuale e precedente (durante la dissolvenza); `None` = neutro.
    taps: [Vec<f32>; 2],
    /// Gli stessi filtri al contrario (per la convoluzione su memoria contigua).
    reversed: [Vec<f32>; 2],
    neutral: [bool; 2],
    current: usize,
    /// Ultimi campioni in uscita dalla testina, per canale (per la convoluzione).
    history: Vec<Vec<f32>>,
    history_pos: usize,
    hop: usize,
    countdown: usize,
    fade_left: usize,
    /// Rete di sicurezza: picco della voce prima e dopo il filtro (per
    /// canale) e quanto in fretta calano. Il filtro non puo' alzare i picchi
    /// piu' del doppio (su un'onda quadra o un colpo secco un filtro con
    /// spinte forti farebbe "scoppiare" il suono).
    level_in: Vec<f32>,
    level_out: Vec<f32>,
    release: f32,
}

impl TimbreFilter {
    /// Lunghezza del filtro: 8 ms (l'inviluppo e' una curva liscia, la sua
    /// risposta si spegne in pochi millisecondi).
    const TAPS_S: f32 = 0.008;
    /// Sotto questo guadagno (logaritmico) ovunque il filtro e' neutro.
    const NEUTRAL: f32 = 1e-4;
    /// Lisciatura del guadagno nel tempo (vedi `gain`).
    const SMOOTH_S: f32 = 0.012;
    /// Guadagno massimo del filtro (+-12 dB, logaritmo naturale). HQ arriva a
    /// +-24 dB riga per riga dentro un pezzo di 16 ms; un filtro nel tempo con
    /// una punta stretta di +24 dB "suona" da solo e moltiplica ogni rumore
    /// (sulla voce di prova: brevi scoppi di 10 ms, +12 dB, a fine nota).
    const MAX_GAIN: f32 = 12.0 / 8.685_89;
    /// Sopra 7 kHz il filtro sfuma fino a 12 kHz: li' c'e' l'aria, non le
    /// formanti, e le punte strette del timbro erano solo rumore.
    const FADE_HZ: (f32, f32) = (7000.0, 12000.0);

    pub fn new(sample_rate: f32, channels: usize) -> Self {
        let channels = channels.clamp(1, MAX_CHANNELS);
        let analyzer = PhaseVocoder::new(1, sample_rate);
        let n = analyzer.fft_len();
        let taps = ((sample_rate * Self::TAPS_S) as usize).max(64).min((n / 4).max(8));
        let input_len = n.next_power_of_two();
        let history_len = taps.next_power_of_two();
        let hop = hop_of(sample_rate);
        Self {
            channels,
            analyzer,
            fft: Fft::new(n),
            n,
            cepstrum: vec![Complex::default(); n],
            gain: vec![0.0; n / 2 + 1],
            weight: (0..=n / 2)
                .map(|k| {
                    let f = k as f32 * sample_rate / n as f32;
                    let x = ((f - Self::FADE_HZ.0) / (Self::FADE_HZ.1 - Self::FADE_HZ.0)).clamp(0.0, 1.0);
                    0.5 + 0.5 * (std::f32::consts::PI * x).cos()
                })
                .collect(),
            gain_follow: 1.0 - (-(hop_of(sample_rate) as f32) / (Self::SMOOTH_S * sample_rate)).exp(),
            input: vec![0.0; input_len],
            input_pos: 0,
            segment: vec![0.0; n / 4 * 3],
            taps: [vec![0.0; taps], vec![0.0; taps]],
            reversed: [vec![0.0; taps], vec![0.0; taps]],
            neutral: [true, true],
            current: 0,
            // Ogni campione scritto due volte (a `pos` e a `pos + len`): gli
            // ultimi campioni sono sempre uno dopo l'altro in memoria.
            history: vec![vec![0.0; 2 * history_len]; channels],
            history_pos: 0,
            hop,
            countdown: 1,
            fade_left: 0,
            level_in: vec![0.0; channels],
            level_out: vec![0.0; channels],
            release: (-1.0 / (0.05 * sample_rate)).exp(),
        }
    }

    pub fn reset(&mut self) {
        self.analyzer.reset();
        self.input.fill(0.0);
        self.input_pos = 0;
        self.neutral = [true, true];
        self.gain.fill(0.0);
        for h in &mut self.history {
            h.fill(0.0);
        }
        self.history_pos = 0;
        self.countdown = 1;
        self.fade_left = 0;
        self.level_in.fill(0.0);
        self.level_out.fill(0.0);
    }

    /// Un campione: `input` = voce in ingresso (mono, prima della testina),
    /// `output` = voce spostata dalla testina (tutti i canali, filtrata qui).
    /// `ratio` = di quanto la testina sposta la nota ora, `formant` = comando
    /// Formant (rapporto), `period` e `voiced` = l'analisi piu' recente.
    pub fn process(&mut self, input: f32, output: &mut [f32], ratio: f32, formant: f32, period: f32, voiced: bool) {
        let mask = self.input.len() - 1;
        self.input[self.input_pos] = input;
        self.input_pos = (self.input_pos + 1) & mask;
        let hlen = self.history[0].len() / 2;
        let hmask = hlen - 1;
        for ch in 0..self.channels {
            let v = output.get(ch).copied().unwrap_or(0.0);
            self.history[ch][self.history_pos] = v;
            self.history[ch][self.history_pos + hlen] = v;
        }

        self.countdown -= 1;
        if self.countdown == 0 {
            self.countdown = self.hop;
            let shift = voiced && period > 1.0 && ((ratio - 1.0).abs() > 1e-4 || (formant - 1.0).abs() > 1e-5);
            let next = 1 - self.current;
            self.design(next, shift, ratio, formant, period);
            if !(self.neutral[next] && self.neutral[self.current]) {
                self.current = next;
                self.fade_left = self.hop;
            }
        }

        let cur = self.current;
        let old = 1 - cur;
        if !(self.neutral[cur] && (self.fade_left == 0 || self.neutral[old])) {
            let g = if self.fade_left > 0 { 1.0 - self.fade_left as f32 / self.hop as f32 } else { 1.0 };
            for ch in 0..self.channels {
                let y_new = self.filter(cur, ch, hmask);
                let mut y = if self.fade_left > 0 {
                    let y_old = self.filter(old, ch, hmask);
                    y_old + (y_new - y_old) * g
                } else {
                    y_new
                };
                let x = self.history[ch][self.history_pos].abs();
                self.level_in[ch] = x.max(self.level_in[ch] * self.release);
                self.level_out[ch] = y.abs().max(self.level_out[ch] * self.release);
                let limit = 2.0 * self.level_in[ch];
                if self.level_out[ch] > limit {
                    y *= limit / self.level_out[ch];
                }
                if let Some(o) = output.get_mut(ch) {
                    *o = y;
                }
            }
        }
        if self.fade_left > 0 {
            self.fade_left -= 1;
        }
        self.history_pos = (self.history_pos + 1) & hmask;
    }

    #[inline]
    fn filter(&self, which: usize, ch: usize, hmask: usize) -> f32 {
        let h = &self.history[ch];
        if self.neutral[which] {
            return h[self.history_pos];
        }
        let rev = &self.reversed[which];
        let end = self.history_pos + hmask + 2;
        let window = &h[end - rev.len()..end];
        // Somma a blocchi di 8 (il compilatore la vettorizza).
        let mut acc = [0.0f32; 8];
        for (r, w) in rev.chunks_exact(8).zip(window.chunks_exact(8)) {
            for j in 0..8 {
                acc[j] += r[j] * w[j];
            }
        }
        let mut y: f32 = acc.iter().sum();
        let rest = rev.len() / 8 * 8;
        for j in rest..rev.len() {
            y += rev[j] * window[j];
        }
        y
    }

    /// Prepara il filtro `which` dal guadagno del timbro di HQ.
    fn design(&mut self, which: usize, shift: bool, ratio: f32, formant: f32, period: f32) {
        let n = self.n;
        let half = n / 2;
        if shift {
            let mask = self.input.len() - 1;
            let len = self.segment.len();
            for i in 0..len {
                self.segment[i] = self.input[(self.input_pos + mask + 1 - len + i) & mask];
            }
            let target = self.analyzer.timbre_gain(&self.segment, period, ratio, formant);
            for ((g, t), w) in self.gain.iter_mut().zip(target).zip(&self.weight) {
                let t = (t * w).clamp(-Self::MAX_GAIN, Self::MAX_GAIN);
                *g += self.gain_follow * (t - *g);
            }
        } else if !self.gain.iter().all(|g| g.abs() < Self::NEUTRAL) {
            // Senza spostamento il filtro torna neutro, con la stessa dolcezza.
            for g in self.gain.iter_mut() {
                *g -= self.gain_follow * *g;
                if g.abs() < 0.5 * Self::NEUTRAL {
                    *g = 0.0;
                }
            }
        }
        let gain = &self.gain;
        if gain.iter().all(|g| g.abs() < Self::NEUTRAL) {
            self.neutral[which] = true;
            return;
        }
        // Fase minima dal cepstrum: logaritmo del modulo -> cepstrum ->
        // piegato sulla parte "causale" -> esponenziale -> risposta.
        for k in 0..=half {
            self.cepstrum[k] = Complex::new(gain[k], 0.0);
        }
        for k in 1..half {
            self.cepstrum[n - k] = self.cepstrum[k];
        }
        self.fft.process(&mut self.cepstrum, true);
        let scale = 1.0 / n as f32;
        for q in 0..n {
            let fold = if q == 0 || q == half {
                1.0
            } else if q < half {
                2.0
            } else {
                0.0
            };
            self.cepstrum[q] = Complex::new(self.cepstrum[q].re * scale * fold, 0.0);
        }
        self.fft.process(&mut self.cepstrum, false);
        for c in self.cepstrum.iter_mut() {
            let m = c.re.exp();
            *c = Complex::new(m * c.im.cos(), m * c.im.sin());
        }
        self.fft.process(&mut self.cepstrum, true);
        let taps = &mut self.taps[which];
        let count = taps.len();
        let taper = count / 4;
        for (i, t) in taps.iter_mut().enumerate() {
            let w = if i + taper >= count {
                let x = (i + taper - count) as f32 / taper as f32;
                0.5 + 0.5 * (std::f32::consts::PI * x).cos()
            } else {
                1.0
            };
            *t = self.cepstrum[i].re * scale * w;
        }
        for (r, t) in self.reversed[which].iter_mut().zip(self.taps[which].iter().rev()) {
            *r = *t;
        }
        self.neutral[which] = false;
    }
}

fn hop_of(sample_rate: f32) -> usize {
    ((sample_rate * 0.0053).round() as usize).max(16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_when_nothing_moves() {
        let sr = 48000.0;
        let mut f = TimbreFilter::new(sr, 1);
        let x = crate::synth::steady_vowel(200.0, sr, 20000);
        for &v in &x {
            let mut o = [v];
            f.process(v, &mut o, 1.0, 1.0, sr / 200.0, true);
            assert_eq!(o[0], v);
        }
    }

    #[test]
    fn minimum_phase_filter_matches_the_gain() {
        // Formant +4 su una vocale ferma: il filtro deve dare (in media) il
        // guadagno chiesto, senza ritardo (energia della risposta all'inizio).
        let sr = 48000.0;
        let mut f = TimbreFilter::new(sr, 1);
        let x = crate::synth::steady_vowel(150.0, sr, 30000);
        let formant = 2f32.powf(4.0 / 12.0);
        for &v in &x {
            let mut o = [v];
            f.process(v, &mut o, 1.0, formant, sr / 150.0, true);
        }
        let taps = &f.taps[f.current];
        assert!(!f.neutral[f.current]);
        let energy: f32 = taps.iter().map(|t| t * t).sum();
        let early: f32 = taps[..taps.len() / 4].iter().map(|t| t * t).sum();
        assert!(early > 0.9 * energy, "risposta non concentrata all'inizio: {early} / {energy}");
        // Risposta in frequenza vs guadagno voluto, sotto 6 kHz.
        let n = f.n;
        let mut spec = vec![Complex::default(); n];
        for (i, &t) in taps.iter().enumerate() {
            spec[i] = Complex::new(t, 0.0);
        }
        f.fft.process(&mut spec, false);
        let seg: Vec<f32> = x[x.len() - f.segment.len()..].to_vec();
        let gain = f.analyzer.timbre_gain(&seg, sr / 150.0, 1.0, formant).to_vec();
        let mut err = 0.0;
        let mut count = 0;
        for k in (n * 200 / 48000)..(n * 6000 / 48000) {
            let got = (spec[k].re * spec[k].re + spec[k].im * spec[k].im).sqrt().max(1e-9).ln();
            err += (got - gain[k]).abs();
            count += 1;
        }
        let err_db = err / count as f32 * 8.686;
        assert!(err_db < 1.0, "filtro lontano dal guadagno di {err_db:.2} dB");
    }
}
