//! Un piccolo "cantante sintetico" per i test e per la demo.
//!
//! Genera una vocale "a" con la stessa struttura di una voce vera:
//! una sorgente glottale (gli impulsi delle corde vocali, modello di Rosenberg)
//! filtrata da tre risonanze (le formanti di gola e bocca).
//! Non suona come un cantante vero, ma e' perfetto per verificare che
//! rilevamento e correzione funzionino, perche' sappiamo esattamente che nota
//! sta "cantando" in ogni istante.

use crate::scale::note_to_hz;
use std::f32::consts::PI;

/// Formanti della vocale "a": (frequenza Hz, larghezza di banda Hz, guadagno).
const VOWEL_A: [(f32, f32, f32); 3] = [(730.0, 80.0, 1.0), (1090.0, 90.0, 0.5), (2440.0, 120.0, 0.25)];

/// Filtro risonante a due poli: simula una formante.
#[derive(Clone, Copy, Default)]
struct Resonator {
    b1: f32,
    b2: f32,
    a0: f32,
    y1: f32,
    y2: f32,
}

impl Resonator {
    fn new(freq: f32, bandwidth: f32, gain: f32, sr: f32) -> Self {
        let r = (-PI * bandwidth / sr).exp();
        let theta = 2.0 * PI * freq / sr;
        Self { b1: 2.0 * r * theta.cos(), b2: -r * r, a0: (1.0 - r) * gain, y1: 0.0, y2: 0.0 }
    }

    fn tick(&mut self, x: f32) -> f32 {
        let y = self.a0 * x + self.b1 * self.y1 + self.b2 * self.y2;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Forma d'onda di un impulso glottale (Rosenberg), fase in [0, 1).
fn glottal_pulse(phase: f32) -> f32 {
    const OPEN: f32 = 0.4;
    const CLOSE: f32 = 0.16;
    if phase < OPEN {
        0.5 * (1.0 - (PI * phase / OPEN).cos())
    } else if phase < OPEN + CLOSE {
        (0.5 * PI * (phase - OPEN) / CLOSE).cos()
    } else {
        0.0
    }
}

/// Genera la voce seguendo una curva di frequenza (Hz per campione) e
/// un inviluppo di volume. Restituisce il segnale normalizzato a picco 0.5.
pub fn sing_curve(freq_hz: &[f32], envelope: &[f32], sr: f32) -> Vec<f32> {
    let mut out = sing_curve_detailed(freq_hz, envelope, sr, 0.0, 1);
    let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    if peak > 0.0 {
        for x in &mut out {
            *x *= 0.5 / peak;
        }
    }
    out
}

/// Come [`sing_curve`], ma con un po' di "aria" (il rumore del respiro che
/// passa tra le corde vocali, `breath` da 0 a 1) e senza normalizzare il
/// volume: due voci generate con le stesse impostazioni sono confrontabili.
pub fn sing_curve_detailed(freq_hz: &[f32], envelope: &[f32], sr: f32, breath: f32, seed: u32) -> Vec<f32> {
    let mut formants: Vec<Resonator> = VOWEL_A
        .iter()
        .map(|&(f, bw, g)| Resonator::new(f, bw, g, sr))
        .collect();
    let air = noise(freq_hz.len(), 1.0, seed);
    let mut phase = 0.0f32;
    let mut previous = 0.0f32;
    let mut out = Vec::with_capacity(freq_hz.len());

    for ((&f, &env), &n) in freq_hz.iter().zip(envelope).zip(&air) {
        phase += f / sr;
        if phase >= 1.0 {
            phase -= 1.0;
        }
        let flow = glottal_pulse(phase);
        let pulse = flow * env;
        // Derivata = effetto della radiazione dalle labbra.
        let mut source = pulse - previous;
        previous = pulse;
        // Il respiro e' piu' forte quando le corde vocali sono aperte.
        source += n * breath * 0.02 * (0.3 + 0.7 * flow) * env;
        let y: f32 = formants.iter_mut().map(|r| r.tick(source)).sum();
        out.push(y);
    }
    out
}

/// Una vocale tenuta a frequenza fissa.
pub fn steady_vowel(hz: f32, sr: f32, len: usize) -> Vec<f32> {
    sing_curve(&vec![hz; len], &vec![1.0; len], sr)
}

/// Rumore bianco deterministico (sempre uguale a parita' di `seed`).
pub fn noise(len: usize, amplitude: f32, seed: u32) -> Vec<f32> {
    let mut state = seed.max(1);
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state as f32 / u32::MAX as f32 * 2.0 - 1.0) * amplitude
        })
        .collect()
}

/// Una nota cantata: nota MIDI, quanto e' stonata (centesimi), durata (s).
#[derive(Clone, Copy, Debug)]
pub struct SungNote {
    pub note: f32,
    pub detune_cents: f32,
    pub seconds: f32,
}

/// Una melodia cantata in modo realistico: legato tra le note con un piccolo
/// "scivolamento" (glide), vibrato, e una leggera deriva di intonazione.
/// Le note con `note < 0` sono pause.
/// Restituisce (audio, nota "cantata" in ogni campione, NaN nelle pause).
pub fn sing_melody(notes: &[SungNote], sr: f32, vibrato_cents: f32) -> (Vec<f32>, Vec<f32>) {
    let glide = (0.06 * sr) as usize;
    let fade = (0.03 * sr) as usize;
    let mut sung = Vec::new();
    let mut envelope = Vec::new();
    let mut previous_note: Option<f32> = None;
    let mut time = 0usize;

    for (index, n) in notes.iter().enumerate() {
        let len = (n.seconds * sr) as usize;
        let starts_phrase = previous_note.is_none();
        let ends_phrase = notes.get(index + 1).is_none_or(|next| next.note < 0.0);
        if n.note < 0.0 {
            sung.extend(std::iter::repeat_n(f32::NAN, len));
            envelope.extend(std::iter::repeat_n(0.0, len));
            previous_note = None;
            time += len;
            continue;
        }
        let target = n.note + n.detune_cents / 100.0;
        let from = previous_note.unwrap_or(target - 0.3); // attacco leggermente "da sotto"
        for i in 0..len {
            let t = (time + i) as f32 / sr;
            let g = (i as f32 / glide as f32).min(1.0);
            let glide_shape = 0.5 - 0.5 * (PI * g).cos();
            // Il vibrato cresce dopo l'attacco, come nei cantanti veri.
            let vib_amount = ((i as f32 / sr - 0.15) / 0.3).clamp(0.0, 1.0);
            let vibrato = vibrato_cents / 100.0 * vib_amount * (2.0 * PI * 5.5 * t).sin();
            let drift = 0.08 * (2.0 * PI * 0.7 * t).sin();
            sung.push(from + (target - from) * glide_shape + vibrato + drift);

            let attack = if starts_phrase { (i as f32 / fade as f32).min(1.0) } else { 1.0 };
            let release = if ends_phrase { ((len - i) as f32 / fade as f32).min(1.0) } else { 1.0 };
            envelope.push(attack.min(release));
        }
        previous_note = Some(target);
        time += len;
    }

    let freqs: Vec<f32> = sung
        .iter()
        .map(|&n| if n.is_nan() { 100.0 } else { note_to_hz(n) })
        .collect();
    (sing_curve(&freqs, &envelope, sr), sung)
}
