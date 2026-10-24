//! # Tape echo a tre testine (stile Roland Space Echo RE-201)
//!
//! Il modello segue lo schema della macchina:
//!
//! ```text
//!            ┌──────────── INTENSITY (rigenerazione) ◄──────────────────┐
//!            ▼                                                           │
//!  in ─► preamp ─► testina di registrazione ═══ nastro ═══►  T1  T2  T3 ─┤
//!        (sat.)    (saturazione + passa-alto)  (wow/flutter)   │   │   │  │
//!                                                     MODE SELECTOR (1-12)
//!                                                              │
//!                                     perdita sugli acuti, "head bump", BASS/TREBLE
//!                                                              │
//!                                      ECHO ──────────────────►+──► uscita (solo wet)
//!                                      molla (modi 5-12) ─────►┘
//! ```
//!
//! - Le tre testine di lettura sono a distanza 1 : 2 : 3 dalla testina di
//!   registrazione: con REPEAT RATE (velocita' del nastro) si sposta tutto
//!   insieme. Cambiando la velocita' il nastro accelera o rallenta come
//!   quello vero: la nota delle ripetizioni scivola (non salta).
//! - Il selettore MODE ha le 12 posizioni della macchina: 1-4 solo eco,
//!   5-11 eco con molla, 12 solo molla.
//! - Wow (lento, ~0,5 Hz), flutter (~6 Hz) e un leggero vagare casuale
//!   modulano la velocita' del nastro: tutte le testine si muovono insieme.
//! - Saturazione del nastro dentro il giro: con INTENSITY alta le
//!   ripetizioni si scuriscono e si "impastano"; oltre ~85% il giro va in
//!   auto-oscillazione come sulla macchina, ma resta limitato dal nastro.
//! - Nessun fruscio: il rumore del nastro non e' modellato.

use super::util::{Allpass1, Biquad, BiquadKind, DelayLine, Lfo, OnePole, Rng, Smoothed, soft_clip, allpass_coef};

/// Impostazioni del tape echo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpaceEchoSettings {
    /// Posizione del selettore (1..=12).
    pub mode: u8,
    /// Ritardo della testina 1 in millisecondi (le altre: 2x e 3x).
    pub head_ms: f32,
    /// Rigenerazione 0..1 (oltre ~0,85 auto-oscillazione).
    pub intensity: f32,
    /// Toni sulle ripetizioni, -1..1 (= -12..+12 dB circa).
    pub bass: f32,
    pub treble: f32,
    /// Livello della molla 0..1.
    pub spring: f32,
    /// Quantita' di wow & flutter 0..1 (0 = nastro nuovo, perfetto).
    pub wow: f32,
    /// Allargamento stereo delle testine 0..1 (0 = mono come la macchina).
    pub spread: f32,
}

impl Default for SpaceEchoSettings {
    fn default() -> Self {
        Self { mode: 4, head_ms: 125.0, intensity: 0.35, bass: 0.0, treble: 0.0, spring: 0.3, wow: 0.35, spread: 0.5 }
    }
}

/// Le testine attive e la molla per ciascuna posizione del selettore.
pub fn mode_heads(mode: u8) -> ([bool; 3], bool) {
    match mode.clamp(1, 12) {
        1 => ([true, false, false], false),
        2 => ([false, true, false], false),
        3 => ([false, false, true], false),
        4 => ([false, true, true], false),
        5 => ([true, false, false], true),
        6 => ([false, true, false], true),
        7 => ([false, false, true], true),
        8 => ([true, true, false], true),
        9 => ([false, true, true], true),
        10 => ([true, false, true], true),
        11 => ([true, true, true], true),
        _ => ([false, false, false], true),
    }
}

/// Distanza delle testine dalla testina di registrazione (in "testine 1").
const HEAD_RATIO: [f32; 3] = [1.0, 2.0, 3.0];
/// Ritardo massimo della testina 1 (ms).
pub const MAX_HEAD_MS: f32 = 1000.0;
pub const MIN_HEAD_MS: f32 = 20.0;

/// Riverbero a molla: due molle (linee con dispersione) in parallelo.
struct Spring {
    lines: [DelayLine; 2],
    disp: [[Allpass1; 10]; 2],
    damp: [OnePole; 2],
    lengths: [usize; 2],
    input_lp: OnePole,
    out_hp: [Biquad; 2],
    state: [f32; 2],
}

impl Spring {
    fn new(sr: f32) -> Self {
        let lengths = [(0.0337 * sr) as usize, (0.0411 * sr) as usize];
        let coef = [allpass_coef(1800.0, sr).clamp(0.3, 0.75), allpass_coef(2100.0, sr).clamp(0.3, 0.75)];
        Self {
            lines: [DelayLine::new(lengths[0] + 8), DelayLine::new(lengths[1] + 8)],
            disp: [[Allpass1::new(coef[0]); 10], [Allpass1::new(coef[1]); 10]],
            damp: [OnePole::new(4200.0, sr), OnePole::new(3800.0, sr)],
            lengths,
            input_lp: OnePole::new(5000.0, sr),
            out_hp: [
                Biquad::new(BiquadKind::Highpass, 220.0, 0.6, sr),
                Biquad::new(BiquadKind::Highpass, 220.0, 0.6, sr),
            ],
            state: [0.0; 2],
        }
    }

    fn reset(&mut self) {
        for l in &mut self.lines {
            l.clear();
        }
        for d in self.disp.iter_mut().flatten() {
            d.reset();
        }
        for d in &mut self.damp {
            d.reset();
        }
        self.input_lp.reset();
        for h in &mut self.out_hp {
            h.reset();
        }
        self.state = [0.0; 2];
    }

    /// Restituisce (sinistra, destra).
    #[inline]
    fn process(&mut self, x: f32) -> (f32, f32) {
        let x = self.input_lp.lowpass(x);
        let mut out = [0.0f32; 2];
        for k in 0..2 {
            let fed = self.lines[k].read_int(self.lengths[k]);
            // La dispersione (ritardo diverso per ogni frequenza) da' il
            // tipico "boing" della molla.
            let mut y = fed;
            for ap in &mut self.disp[k] {
                y = ap.process(y);
            }
            let y = self.damp[k].lowpass(y);
            self.state[k] = y;
            self.lines[k].write(x * 0.6 + y * 0.72);
            out[k] = self.out_hp[k].process(y);
        }
        (out[0] * 0.9 + out[1] * 0.3, out[1] * 0.9 + out[0] * 0.3)
    }
}

pub struct SpaceEcho {
    sr: f32,
    tape: DelayLine,
    /// Ritardo della testina 1 in campioni (scorre come il nastro).
    head: Smoothed,
    wow: Lfo,
    flutter: Lfo,
    drift_rng: Rng,
    drift: OnePole,
    drift_value: f32,
    drift_counter: u32,
    /// Filtri del giro (passa-alto della testina di registrazione, perdita
    /// sugli acuti del nastro, "head bump").
    rec_hp: OnePole,
    tape_lp: [Biquad; 2],
    bump: Biquad,
    bass: Biquad,
    treble: Biquad,
    tone: (f32, f32),
    feedback: f32,
    spring: Spring,
    /// Guadagni (testine e molla) che scorrono dolcemente al cambio di modo.
    head_gain: [Smoothed; 3],
    spring_gain: Smoothed,
    regen: Smoothed,
    /// Ultima volta che c'era segnale (per fermarsi quando tutto tace).
    quiet_samples: u32,
}

impl SpaceEcho {
    pub fn new(sr: f32) -> Self {
        let max = (MAX_HEAD_MS * 3.0 * 1.02 / 1000.0 * sr) as usize + 64;
        let s = SpaceEchoSettings::default();
        let mut e = Self {
            sr,
            tape: DelayLine::new(max),
            head: Smoothed::new(s.head_ms * 0.001 * sr, 0.12, sr),
            wow: Lfo::new(0.0),
            flutter: Lfo::new(0.37),
            drift_rng: Rng::new(0x5eed),
            drift: OnePole::new(0.7, sr),
            drift_value: 0.0,
            drift_counter: 0,
            rec_hp: OnePole::new(70.0, sr),
            tape_lp: [
                Biquad::new(BiquadKind::Lowpass, 5200.0, 0.6, sr),
                Biquad::new(BiquadKind::Lowpass, 7500.0, 0.55, sr),
            ],
            bump: Biquad::new(BiquadKind::Peak(2.5), 110.0, 0.9, sr),
            bass: Biquad::default(),
            treble: Biquad::default(),
            tone: (f32::NAN, f32::NAN),
            feedback: 0.0,
            spring: Spring::new(sr),
            head_gain: [Smoothed::new(0.0, 0.03, sr); 3],
            spring_gain: Smoothed::new(0.0, 0.03, sr),
            regen: Smoothed::new(0.0, 0.03, sr),
            quiet_samples: u32::MAX,
        };
        e.set_tone(0.0, 0.0);
        e
    }

    pub fn reset(&mut self) {
        self.tape.clear();
        self.feedback = 0.0;
        self.spring.reset();
        for f in &mut self.tape_lp {
            f.reset();
        }
        self.bump.reset();
        self.bass.reset();
        self.treble.reset();
        self.rec_hp.reset();
        self.quiet_samples = u32::MAX;
    }

    fn set_tone(&mut self, bass: f32, treble: f32) {
        if self.tone == (bass, treble) {
            return;
        }
        self.tone = (bass, treble);
        self.bass.set(BiquadKind::LowShelf(bass.clamp(-1.0, 1.0) * 12.0), 160.0, 0.7, self.sr);
        self.treble.set(BiquadKind::HighShelf(treble.clamp(-1.0, 1.0) * 12.0), 2800.0, 0.7, self.sr);
    }

    /// Tutto in silenzio da un po' (ingresso e ripetizioni): l'effetto si
    /// puo' saltare.
    pub fn is_idle(&self) -> bool {
        self.quiet_samples > (self.sr * 0.5) as u32 && self.feedback.abs() < 1e-6
    }

    /// Elabora un campione mono (la mandata). Restituisce l'eco (L, R).
    #[inline]
    pub fn process(&mut self, input: f32, s: &SpaceEchoSettings) -> (f32, f32) {
        if input.abs() > 1e-6 {
            self.quiet_samples = 0;
        } else {
            self.quiet_samples = self.quiet_samples.saturating_add(1);
        }
        self.set_tone(s.bass, s.treble);
        let (heads, spring_on) = mode_heads(s.mode);

        // Velocita' del nastro: la testina 1 scorre verso il nuovo ritardo.
        let target = s.head_ms.clamp(MIN_HEAD_MS, MAX_HEAD_MS) * 0.001 * self.sr;
        let base = self.head.next(target);

        // Wow, flutter e vagare casuale (variazione relativa di velocita').
        let wow_p = self.wow.advance(0.55, self.sr);
        let flutter_p = self.flutter.advance(6.3, self.sr);
        self.drift_counter += 1;
        if self.drift_counter >= (self.sr * 0.05) as u32 {
            self.drift_counter = 0;
            self.drift_value = self.drift_rng.next();
        }
        let drift = self.drift.lowpass(self.drift_value);
        let amount = s.wow.clamp(0.0, 1.0);
        let speed_mod = amount
            * (0.0032 * Lfo::sine(wow_p) + 0.0007 * Lfo::sine(flutter_p) + 0.0035 * drift);

        // Lettura delle tre testine.
        let mut taps = [0.0f32; 3];
        for (k, tap) in taps.iter_mut().enumerate() {
            let d = base * HEAD_RATIO[k] * (1.0 + speed_mod);
            *tap = self.tape.read(d);
        }
        // Guadagni (dolci) delle testine scelte.
        let active = heads.iter().filter(|&&h| h).count().max(1) as f32;
        let norm = 1.0 / active.sqrt();
        let mut mix = 0.0;
        let mut mix_l = 0.0;
        let mut mix_r = 0.0;
        const PAN: [f32; 3] = [-0.55, 0.55, 0.0];
        for k in 0..3 {
            let g = self.head_gain[k].next(if heads[k] { norm } else { 0.0 });
            let v = taps[k] * g;
            mix += v;
            let p = PAN[k] * s.spread.clamp(0.0, 1.0);
            mix_l += v * (1.0 - p).min(1.0);
            mix_r += v * (1.0 + p).min(1.0);
        }

        // Riproduzione: perdita sugli acuti, head bump, toni (anche nel giro,
        // come sulla macchina: le ripetizioni si scuriscono a ogni passaggio).
        let mut play = self.tape_lp[0].process(mix);
        play = self.bump.process(play);
        play = self.bass.process(play);
        play = self.treble.process(play);

        // Rigenerazione.
        let regen_target = if heads.iter().any(|&h| h) { s.intensity.clamp(0.0, 1.0) * 1.12 } else { 0.0 };
        let regen = self.regen.next(regen_target);
        self.feedback = play * regen;

        // Registrazione sul nastro: preamp + passa-alto + saturazione.
        let rec = self.rec_hp.highpass(input * 0.9 + self.feedback);
        let rec = soft_clip(rec * 1.15) / 1.15;
        self.tape.write(rec);
        if rec.abs() > 1e-6 {
            self.quiet_samples = 0;
        }

        // Uscita: eco (con lo stereo delle testine) e molla.
        // Lo stereo: differenza tra le testine a sinistra e a destra,
        // scurita come il resto del nastro.
        let side = self.tape_lp[1].process(mix_l - mix_r) * 0.5;
        let echo_l = play + side;
        let echo_r = play - side;
        let spring_g = self.spring_gain.next(if spring_on { s.spring.clamp(0.0, 1.0) } else { 0.0 });
        let (sl, sr) = if spring_on || spring_g > 1e-4 {
            let send = if spring_on { input + play * 0.5 } else { 0.0 };
            self.spring.process(send)
        } else {
            (0.0, 0.0)
        };
        (echo_l + sl * spring_g * 0.6, echo_r + sr * spring_g * 0.6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn impulse_response(s: &SpaceEchoSettings, seconds: f32, sr: f32) -> Vec<f32> {
        let mut e = SpaceEcho::new(sr);
        // Il nastro arriva alla velocita' giusta prima dell'impulso.
        for _ in 0..(sr as usize) {
            e.process(0.0, s);
        }
        (0..(seconds * sr) as usize)
            .map(|i| {
                let (l, r) = e.process(if i == 0 { 1.0 } else { 0.0 }, s);
                0.5 * (l + r)
            })
            .collect()
    }

    fn peak_near(y: &[f32], at: usize, win: usize) -> f32 {
        y[at.saturating_sub(win)..(at + win).min(y.len())].iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    #[test]
    fn heads_are_where_the_mode_says() {
        let sr = 48000.0;
        let head = 100.0;
        let at = |k: f32| (head * k * 0.001 * sr) as usize;
        for mode in 1..=4u8 {
            let s = SpaceEchoSettings { mode, head_ms: head, intensity: 0.0, wow: 0.0, spring: 0.0, ..Default::default() };
            let y = impulse_response(&s, 0.5, sr);
            let (heads, _) = mode_heads(mode);
            for k in 0..3 {
                let p = peak_near(&y, at(k as f32 + 1.0), 120);
                if heads[k] {
                    assert!(p > 0.05, "modo {mode}: testina {} assente ({p})", k + 1);
                } else {
                    assert!(p < 0.01, "modo {mode}: testina {} presente ({p})", k + 1);
                }
            }
        }
    }

    #[test]
    fn intensity_makes_repeats_that_decay_and_never_explode() {
        let sr = 44100.0;
        let s = SpaceEchoSettings { mode: 1, head_ms: 80.0, intensity: 0.6, wow: 0.3, ..Default::default() };
        let y = impulse_response(&s, 3.0, sr);
        let r1 = peak_near(&y, (0.08 * sr) as usize, 200);
        let r3 = peak_near(&y, (0.24 * sr) as usize, 200);
        assert!(r3 < r1 && r3 > 0.02 * r1, "ripetizioni {r1} {r3}");
        // Al massimo (auto-oscillazione) resta limitato dal nastro.
        let mut e = SpaceEcho::new(sr);
        let s = SpaceEchoSettings { mode: 11, intensity: 1.0, spring: 1.0, ..Default::default() };
        let mut peak = 0.0f32;
        for i in 0..(sr as usize * 20) {
            let x = if i < 2000 { ((i as f32) * 0.05).sin() * 0.8 } else { 0.0 };
            let (l, r) = e.process(x, &s);
            assert!(l.is_finite() && r.is_finite());
            peak = peak.max(l.abs()).max(r.abs());
        }
        assert!(peak < 4.0, "picco {peak}");
    }

    #[test]
    fn reverb_only_mode_has_no_echo() {
        let sr = 48000.0;
        let s = SpaceEchoSettings { mode: 12, head_ms: 150.0, intensity: 0.5, spring: 1.0, ..Default::default() };
        let y = impulse_response(&s, 1.0, sr);
        let energy: f32 = y.iter().map(|v| v * v).sum();
        assert!(energy > 1e-4, "la molla deve suonare");
        let s1 = SpaceEchoSettings { spring: 0.0, ..s };
        let y = impulse_response(&s1, 1.0, sr);
        let energy: f32 = y.iter().map(|v| v * v).sum();
        assert!(energy < 1e-8, "modo 12 senza molla deve tacere: {energy}");
    }
}
