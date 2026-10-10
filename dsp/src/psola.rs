//! Pitch shifting con TD-PSOLA (Time-Domain Pitch-Synchronous Overlap-Add).
//!
//! Perche' non basta "velocizzare" il suono come un nastro? Perche' cambierebbe
//! anche il timbro: la voce diventerebbe "da scoiattolo" o "da orco". Il timbro
//! della voce dipende dalle **formanti** (le risonanze di gola e bocca), che
//! devono restare ferme mentre cambia solo la nota.
//!
//! PSOLA fa cosi':
//! 1. taglia la voce in "grani" lunghi due periodi, uno per ogni vibrazione
//!    delle corde vocali, smussati ai bordi con una finestra di Hann;
//! 2. li rimette in fila, sovrapponendoli, a una distanza diversa:
//!    piu' vicini = nota piu' acuta, piu' lontani = nota piu' grave.
//!
//! Ogni grano contiene un singolo "colpo" delle corde vocali con la sua
//! risonanza intatta: per questo le formanti restano al loro posto.
//!
//! ```text
//!  ingresso:   /\    /\    /\    /\        (un grano ogni periodo P)
//!  uscita:     /\  /\  /\  /\  /\  /\      (un grano ogni P / rapporto)
//! ```
//!
//! Lavora in tempo reale con una latenza fissa: per prendere un grano centrato
//! in un punto dobbiamo aver gia' ricevuto il periodo successivo.
//!
//! Spostando i grani il volume puo' cambiare un po' (si sovrappongono di
//! piu' o di meno): un piccolo "compensatore di livello" confronta il volume
//! in ingresso e in uscita e corregge la differenza, lentamente.

use crate::MAX_CHANNELS;
use std::f32::consts::PI;

/// Cosa serve sapere per prendere un grano in un certo punto della voce.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrainPitch {
    /// Periodo della voce in quel punto (campioni).
    pub period: f32,
    /// Di quanto moltiplicare la frequenza (1.0 = invariata, 2.0 = un'ottava sopra).
    pub ratio: f32,
    /// `true` se li' c'e' una nota cantata (i grani vengono allineati agli
    /// impulsi delle corde vocali), `false` per consonanti e rumori.
    pub voiced: bool,
    /// Spostamento delle formanti (il "timbro", la lunghezza della gola):
    /// 1.0 = invariato, >1 voce piu' "piccola", <1 voce piu' "grande".
    pub formant: f32,
}

pub struct PsolaShifter {
    channels: usize,
    /// Buffer circolari: gli ultimi campioni ricevuti, per canale.
    input: Vec<Vec<f32>>,
    /// Buffer circolari dove si sommano i grani in uscita, per canale.
    output: Vec<Vec<f32>>,
    input_mask: usize,
    output_mask: usize,

    /// Il periodo piu' lungo gestito adesso / al massimo (campioni).
    max_period: f32,
    capacity_max_period: f32,
    /// Ritardo tra ingresso e uscita (campioni).
    latency: usize,

    /// Indice assoluto del prossimo campione.
    position: u64,
    /// Dove (in campioni di uscita) cade il centro del prossimo grano.
    next_center: f64,
    /// Centro (in campioni di ingresso) dell'ultimo grano preso dall'ingresso.
    analysis_mark: f64,
    started: bool,
    /// Il grano precedente era su una nota cantata?
    last_voiced: bool,
    /// Memoria di lavoro per l'allineamento dei grani (allocata una volta sola).
    align_region: Vec<f32>,
    align_reference: Vec<f32>,

    /// Compensazione del livello: energia media in ingresso e in uscita.
    level_in: f32,
    level_out: f32,
    level_gain: f32,
    level_coeff: f32,
    gain_coeff: f32,
}

impl PsolaShifter {
    /// Periodo minimo accettato: grani piu' corti non avrebbero senso.
    const MIN_PERIOD: f32 = 16.0;
    /// Spostamento massimo (campioni) quando si allinea un grano all'impulso.
    const MAX_ALIGN_REACH: usize = 128;
    /// Punti usati per confrontare le forme d'onda.
    const MAX_ALIGN_POINTS: usize = 256;

    /// Alloca tutta la memoria necessaria per periodi fino a `capacity_max_period`.
    pub fn new(channels: usize, capacity_max_period: usize, sample_rate: f32) -> Self {
        let channels = channels.clamp(1, MAX_CHANNELS);
        let cap = capacity_max_period.max(Self::MIN_PERIOD as usize) as f32;
        let max_latency = Self::latency_for(cap);
        // Spazio per la latenza piu' qualche periodo di storia (serve ad allineare i grani).
        let input_len = (max_latency + 6 * cap as usize + 8).next_power_of_two();
        let output_len = (2 * cap as usize + 8).next_power_of_two();

        let mut shifter = Self {
            channels,
            input: vec![vec![0.0; input_len]; channels],
            output: vec![vec![0.0; output_len]; channels],
            input_mask: input_len - 1,
            output_mask: output_len - 1,
            max_period: cap,
            capacity_max_period: cap,
            latency: max_latency,
            position: 0,
            next_center: 0.0,
            analysis_mark: 0.0,
            started: false,
            last_voiced: false,
            align_region: vec![0.0; cap as usize + 2 * Self::MAX_ALIGN_REACH + 4],
            align_reference: vec![0.0; Self::MAX_ALIGN_POINTS],
            level_in: 0.0,
            level_out: 0.0,
            level_gain: 1.0,
            // Medie su ~150 ms e guadagno che si muove altrettanto piano: deve
            // correggere solo le differenze stabili, non inseguire ogni attacco.
            level_coeff: 1.0 - (-1.0 / (0.15 * sample_rate)).exp(),
            gain_coeff: 1.0 - (-1.0 / (0.15 * sample_rate)).exp(),
        };
        shifter.set_max_period(cap);
        shifter
    }

    /// La latenza necessaria per un certo periodo massimo: un grano e' lungo
    /// 2 periodi, piu' 3/4 di periodo di margine per poterlo allineare.
    fn latency_for(max_period: f32) -> usize {
        (2.75 * max_period).ceil() as usize + 3
    }

    /// Imposta il periodo massimo (in base al tipo di voce). Cambia la
    /// latenza e azzera lo stato. Non alloca memoria.
    pub fn set_max_period(&mut self, max_period: f32) {
        self.max_period = max_period.clamp(Self::MIN_PERIOD, self.capacity_max_period);
        self.latency = Self::latency_for(self.max_period);
        self.reset();
    }

    pub fn latency(&self) -> usize {
        self.latency
    }

    pub fn max_period(&self) -> f32 {
        self.max_period
    }

    /// Svuota i buffer (per esempio quando il DAW ferma la riproduzione).
    pub fn reset(&mut self) {
        for buffer in self.input.iter_mut().chain(self.output.iter_mut()) {
            buffer.fill(0.0);
        }
        self.position = 0;
        // Il primo grano parte quando ci sono abbastanza dati in ingresso.
        self.next_center = (self.latency as f64) + self.max_period as f64;
        self.analysis_mark = 0.0;
        self.started = false;
        self.last_voiced = false;
        self.level_in = 0.0;
        self.level_out = 0.0;
        self.level_gain = 1.0;
    }

    /// Elabora un campione per canale.
    ///
    /// `pitch_at(t)` deve dire periodo, rapporto e presenza di una nota nel
    /// punto `t` dell'ingresso (vedi [`GrainPitch`]).
    #[inline]
    pub fn process<F>(&mut self, input: &[f32], output: &mut [f32], mut pitch_at: F)
    where
        F: FnMut(f64) -> GrainPitch,
    {
        let now = self.position;
        let in_index = now as usize & self.input_mask;
        for ch in 0..self.channels {
            self.input[ch][in_index] = input.get(ch).copied().unwrap_or(0.0);
        }

        // Nuovi grani da generare in questo istante?
        let latency = self.latency as f64;
        // I grani sono programmati per centro: il centro di ogni grano in uscita
        // cade esattamente ogni `periodo / rapporto` campioni, cioe' alla nuova
        // intonazione. Un grano lungo 2·P deve iniziare P campioni prima del
        // suo centro, quindi lo generiamo con un anticipo pari al periodo massimo.
        while now as f64 >= self.next_center - self.max_period as f64 {
            let center = self.next_center;

            // Il centro del grano in uscita corrisponde, nell'ingresso, al punto
            // (centro - latenza). Chiediamo il pitch li'.
            let target = center - latency;
            let pitch = pitch_at(target);
            let period = pitch.period.clamp(Self::MIN_PERIOD, self.max_period) as f64;
            let ratio = pitch.ratio.clamp(0.25, 4.0) as f64;

            // Limiti di sicurezza: i dati del grano devono essere nel buffer.
            let latest_mark = now as f64 - period - 2.0;
            let earliest_mark = now as f64 - self.input_mask as f64 + period + 2.0;

            // Il prossimo grano dall'ingresso deve stare a un numero intero di
            // periodi dal precedente: cosi' grani vicini sono "in fase" tra loro.
            let previous = self.analysis_mark;
            let steps = if self.started {
                ((target - previous) / period).round().max(0.0)
            } else {
                0.0
            };
            self.analysis_mark = if self.started { previous + steps * period } else { target };
            self.started = true;

            // Nelle note cantate centriamo i grani sugli impulsi delle corde
            // vocali. All'inizio di una nota cerchiamo l'impulso; poi lo
            // seguiamo periodo dopo periodo, perche' quando la voce scivola
            // veloce tra le note il periodo stimato e' un po' in ritardo, e
            // grani non centrati farebbero "trapelare" l'intonazione originale.
            if pitch.voiced {
                if !self.last_voiced {
                    self.analysis_mark = self.find_pulse(target, period, latest_mark);
                } else if steps >= 1.0 {
                    self.analysis_mark = self.align_mark(previous, self.analysis_mark, period, latest_mark);
                }
            }
            self.last_voiced = pitch.voiced;
            if self.analysis_mark > latest_mark || self.analysis_mark < earliest_mark {
                self.analysis_mark = target.clamp(earliest_mark, latest_mark);
            }
            // Le formanti si spostano leggendo il grano piu' compresso o piu'
            // dilatato nel tempo; serve che quei campioni siano disponibili.
            let room_after = (now as f64 - 2.0 - self.analysis_mark) / period;
            let room_before = (self.analysis_mark - (now as f64 - self.input_mask as f64 + 2.0)) / period;
            let formant = (pitch.formant as f64).clamp(0.5, 2.0).min(room_after).min(room_before).max(0.25);
            self.add_grain(center - period, self.analysis_mark, period, ratio, formant);

            self.next_center = center + period / ratio;
        }

        let out_index = now as usize & self.output_mask;
        // Livello dell'ingresso "di allora" (ritardato della latenza) e dell'uscita.
        // Si comincia a misurare solo quando l'uscita e' "a regime" (dopo i
        // primissimi grani), altrimenti l'avvio falserebbe il confronto.
        let warmed_up = now >= (self.latency + 2 * self.max_period as usize) as u64;
        if warmed_up {
            let delayed_index = (now as usize).wrapping_sub(self.latency) & self.input_mask;
            let mut energy_in = 0.0;
            let mut energy_out = 0.0;
            for ch in 0..self.channels {
                let x = self.input[ch][delayed_index];
                let y = self.output[ch][out_index];
                energy_in += x * x;
                energy_out += y * y;
            }
            self.level_in += self.level_coeff * (energy_in - self.level_in);
            self.level_out += self.level_coeff * (energy_out - self.level_out);
            // Evita numeri piccolissimi ("denormali") che rallentano alcune CPU.
            if self.level_in < 1e-20 {
                self.level_in = 0.0;
            }
            if self.level_out < 1e-20 {
                self.level_out = 0.0;
            }
            // Il guadagno si adatta solo quando c'e' davvero del suono (sopra
            // circa -60 dB) sia in ingresso sia in uscita; nel silenzio resta fermo.
            const ADAPT_FLOOR: f32 = 1e-6;
            if self.level_in > ADAPT_FLOOR && self.level_out > ADAPT_FLOOR {
                let wanted = (self.level_in / self.level_out).sqrt().clamp(0.6, 1.7);
                self.level_gain += self.gain_coeff * (wanted - self.level_gain);
            }
        }

        for ch in 0..self.channels {
            let sample = &mut self.output[ch][out_index];
            if let Some(out) = output.get_mut(ch) {
                *out = *sample * self.level_gain;
            }
            *sample = 0.0;
        }
        self.position += 1;
    }

    /// Campione (mono) dell'ingresso in una posizione con decimali.
    #[inline]
    fn mono_at(&self, position: f64) -> f32 {
        let index = position.floor();
        let frac = (position - index) as f32;
        let i0 = index as u64 as usize & self.input_mask;
        let i1 = (i0 + 1) & self.input_mask;
        let mut sum = 0.0;
        for ch in 0..self.channels {
            let input = &self.input[ch];
            sum += input[i0] + (input[i1] - input[i0]) * frac;
        }
        sum
    }

    /// Trova l'impulso delle corde vocali nel periodo centrato su `center`:
    /// e' il punto dove si concentra l'energia (subito dopo la chiusura delle
    /// corde vocali la voce "suona" piu' forte, poi si smorza).
    fn find_pulse(&self, center: f64, period: f64, latest: f64) -> f64 {
        let half = (period * 0.125).max(2.0) as i64;
        let first = (center - period * 0.5).ceil() as i64;
        let last = ((center + period * 0.5).floor()).min(latest.floor()) as i64;
        if last <= first {
            return center;
        }
        let energy_at = |i: i64| {
            let x = self.mono_at(i as f64);
            x * x
        };
        // Somma mobile dell'energia in una finestra di ±P/8.
        let mut sum: f32 = (first - half..=first + half).map(energy_at).sum();
        let mut best = (first, sum);
        for c in first + 1..=last {
            sum += energy_at(c + half) - energy_at(c - half - 1);
            if sum > best.1 {
                best = (c, sum);
            }
        }
        best.0 as f64
    }

    /// Cerca, vicino a `predicted` (entro 1/8 di periodo), il punto in cui la
    /// forma d'onda somiglia di piu' a quella attorno al grano precedente:
    /// cosi' ogni grano resta centrato sul "suo" impulso anche quando la voce
    /// cambia nota velocemente.
    fn align_mark(&mut self, previous: f64, predicted: f64, period: f64, latest: f64) -> f64 {
        let reach = ((period * 0.125) as usize).clamp(2, Self::MAX_ALIGN_REACH);
        let half = (period * 0.5) as usize;
        let step = (2 * half).div_ceil(Self::MAX_ALIGN_POINTS).max(1);
        let points = (2 * half) / step;
        if points < 8 {
            return predicted;
        }

        // 1) Il pezzo di segnale attorno al grano precedente (riferimento)...
        let mut reference_energy = 0.0f32;
        for i in 0..points {
            let x = self.mono_at(previous - half as f64 + (i * step) as f64);
            self.align_reference[i] = x;
            reference_energy += x * x;
        }
        if reference_energy <= 1e-12 {
            return predicted;
        }
        // 2) ...e tutta la zona in cui cercare, letta una volta sola.
        let region_start = predicted - (reach + half) as f64;
        let region_len = 2 * reach + 2 * half + 1;
        for j in 0..region_len {
            self.align_region[j] = self.mono_at(region_start + j as f64);
        }

        // 3) Somiglianza (correlazione normalizzata) per ogni spostamento.
        let mut scores = [f32::NEG_INFINITY; 2 * Self::MAX_ALIGN_REACH + 2];
        let mut best: Option<usize> = None;
        for slot in 0..=2 * reach {
            let offset = slot as f64 - reach as f64;
            if predicted + offset > latest {
                break;
            }
            let segment = &self.align_region[slot..];
            let mut dot = 0.0f32;
            let mut energy = 0.0f32;
            for (i, r) in self.align_reference[..points].iter().enumerate() {
                let x = segment[i * step];
                dot += r * x;
                energy += x * x;
            }
            scores[slot] = dot / (reference_energy * energy).sqrt().max(1e-12);
            if best.is_none_or(|b| scores[slot] > scores[b]) {
                best = Some(slot);
            }
        }
        let best = match best {
            Some(b) => b,
            None => return predicted,
        };
        // Se la previsione era gia' giusta (al campione) non tocchiamo nulla:
        // nelle note ferme la ricostruzione resta perfetta.
        if best == reach {
            return predicted;
        }
        // Interpolazione parabolica per una posizione con decimali.
        let mut fine = 0.0;
        if best > 0 && scores[best + 1].is_finite() {
            let (s0, s1, s2) = (scores[best - 1], scores[best], scores[best + 1]);
            let denom = s0 - 2.0 * s1 + s2;
            if denom.abs() > 1e-9 {
                fine = (0.5 * (s0 - s2) / denom).clamp(-0.5, 0.5) as f64;
            }
        }
        predicted + best as f64 - reach as f64 + fine
    }

    /// Prende dall'ingresso un grano lungo 2·periodo centrato su `center`, lo
    /// moltiplica per una finestra di Hann e lo somma all'uscita a partire da
    /// `start` (che puo' avere decimali: lo gestiamo con l'interpolazione).
    ///
    /// Con `formant` diverso da 1 il grano viene letto compresso (o dilatato)
    /// nel tempo: le risonanze della voce si spostano, la nota no (perche' la
    /// distanza tra i grani in uscita non cambia).
    fn add_grain(&mut self, start: f64, center: f64, period: f64, ratio: f64, formant: f64) {
        let first = start.ceil();
        let fraction = first - start; // in [0, 1)
        let length = 2.0 * period;
        let count = (length - fraction).ceil().max(0.0) as usize;

        // Con grani piu' distanziati (nota piu' grave) restano dei "vuoti" tra
        // un grano e l'altro e il volume cala: prima correzione approssimativa,
        // il resto lo fa il compensatore di livello.
        let gain = if ratio < 1.0 { (1.0 / ratio).sqrt() as f32 } else { 1.0 };
        let window_step = PI / period as f32;
        let first = first as u64;

        for j in 0..count {
            let t = j as f32 + fraction as f32;
            let window = 0.5 - 0.5 * (t * window_step).cos();
            let weight = window * gain;

            let source = center + (j as f64 + fraction - period) * formant;
            let index = source.floor();
            let frac = (source - index) as f32;
            let i0 = index as u64 as usize & self.input_mask;
            let i1 = (i0 + 1) & self.input_mask;
            let o = (first as usize + j) & self.output_mask;

            for ch in 0..self.channels {
                let input = &self.input[ch];
                let sample = input[i0] + (input[i1] - input[i0]) * frac;
                self.output[ch][o] += sample * weight;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;
    use crate::yin::YinDetector;

    fn run(shifter: &mut PsolaShifter, input: &[f32], period: f32, ratio: f32) -> Vec<f32> {
        input
            .iter()
            .map(|&x| {
                let mut out = [0.0];
                shifter.process(&[x], &mut out, |_| GrainPitch { period, ratio, voiced: true, formant: 1.0 });
                out[0]
            })
            .collect()
    }

    /// Con rapporto 1 l'uscita deve essere l'ingresso ritardato (della latenza,
    /// a meno dello spostamento necessario a centrare i grani sugli impulsi).
    #[test]
    fn unity_ratio_reconstructs_input() {
        let sr = 48000.0;
        for &hz in &[150.0f32, 220.0, 431.0] {
            let input = synth::steady_vowel(hz, sr, 24000);
            let mut shifter = PsolaShifter::new(1, 800, sr);
            let period = sr / hz;
            let out = run(&mut shifter, &input, period, 1.0);
            let lat = shifter.latency() as i64;

            let mut best_snr = f64::NEG_INFINITY;
            for lag in lat - period as i64..=lat + period as i64 {
                let (mut err, mut energy) = (0.0f64, 0.0f64);
                for i in 12000..input.len() {
                    let reference = input[(i as i64 - lag) as usize];
                    let e = out[i] - reference;
                    err += (e * e) as f64;
                    energy += (reference * reference) as f64;
                }
                best_snr = best_snr.max(10.0 * (energy / err).log10());
            }
            assert!(best_snr > 35.0, "{hz} Hz: ricostruzione imprecisa, SNR {best_snr:.1} dB");
        }
    }

    /// Con rapporto 2^(1/12) la nota deve salire esattamente di un semitono.
    #[test]
    fn shifts_by_requested_ratio() {
        let sr = 48000.0;
        for &(hz, semitones) in &[
            (220.0f32, 1.0f32),
            (180.0, -2.0),
            (330.0, 0.5),
            (150.0, 7.0),
            (110.0, 12.0),
            (440.0, -12.0),
        ] {
            let input = synth::steady_vowel(hz, sr, 24000);
            let mut shifter = PsolaShifter::new(1, 800, sr);
            let ratio = 2f32.powf(semitones / 12.0);
            let out = run(&mut shifter, &input, sr / hz, ratio);

            let mut yin = YinDetector::new(sr, 60.0);
            let len = yin.frame_len();
            let est = yin.detect(&out[out.len() - len..]);
            assert!(est.voiced);
            let got = sr / est.period;
            let expected = hz * ratio;
            let cents = 1200.0 * (got / expected).log2();
            assert!(cents.abs() < 5.0, "{hz} Hz +{semitones} st → {got} Hz (atteso {expected})");

            // Il volume deve restare simile.
            let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
            let gain_db = 20.0 * (rms(&out[12000..]) / rms(&input[12000..])).log10();
            assert!(gain_db.abs() < 1.0, "volume cambiato di {gain_db:.1} dB");
        }
    }

    #[test]
    fn stereo_channels_stay_independent() {
        let sr = 48000.0;
        let left = synth::steady_vowel(200.0, sr, 12000);
        let mut shifter = PsolaShifter::new(2, 800, sr);
        let mut right_peak = 0.0f32;
        for &x in &left {
            let mut out = [0.0; 2];
            shifter.process(&[x, 0.0], &mut out, |_| GrainPitch { period: sr / 200.0, ratio: 1.1, voiced: true, formant: 1.0 });
            right_peak = right_peak.max(out[1].abs());
        }
        assert_eq!(right_peak, 0.0);
    }
}
