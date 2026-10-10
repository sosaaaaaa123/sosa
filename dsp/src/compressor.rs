//! # Compressore ottico a valvola
//!
//! Modellato sul comportamento dei compressori ottici a valvole da studio
//! (la famiglia del celebre CL 1B): morbidi, musicali, con un rilascio che
//! "respira" seguendo la voce, e un colore caldo dato dalla valvola e dai
//! trasformatori.
//!
//! ```text
//!  ingresso ──┬──────────────────────────────────────────── ritardo ──┐ (secco)
//!             │                                                       │
//!             ├─► rilevatore ─► curva (soglia, rapporto,  ─► cella    │
//!             │   (stereo        ginocchio morbido)          ottica   │
//!             │    collegato)                                  │      │
//!             ▼                                                ▼      │
//!          guadagno ◄──────────────────────────────── riduzione dB    │
//!             │                                                       │
//!             ▼                                                       │
//!   ×4 ─► trasformatore ─► valvola ─► ÷4 ─► via la continua ─► uscita ─► mix ─► fuori
//! ```
//!
//! ## La cella ottica
//!
//! In un compressore ottico il segnale accende un piccolo pannello luminoso
//! e una fotoresistenza ne "legge" la luce. La fotoresistenza ha memoria:
//!
//! - **attacco**: piu' forte e' il picco, piu' in fretta reagisce;
//! - **rilascio in due fasi**: prima veloce (circa meta' della compressione in
//!   ~60 ms), poi lento (da mezzo secondo a qualche secondo);
//! - **memoria**: piu' a lungo e piu' forte si comprime, piu' lenta diventa la
//!   seconda fase. Sulle note lunghe il compressore e' fermo e trasparente,
//!   sulle frasi veloci recupera subito.
//!
//! Come sull'hardware ci sono tre modalita': `Fixed` (solo la cella ottica,
//! attacco e rilascio automatici), `Manual` (tempi esatti dalle manopole) e
//! `FixedManual` (entrambi insieme: vince chi comprime di piu').
//!
//! ## Valvola e trasformatore
//!
//! - La valvola e' una curva asimmetrica: aggiunge soprattutto armoniche
//!   **pari** (seconda, quarta), che suonano calde e "piene" invece che ruvide.
//! - Il trasformatore satura un po' di piu' le frequenze basse, come quelli
//!   veri: corpo e spessore sulla voce.
//! - Tutto questo gira a frequenza quadrupla (vedi [`crate::oversampling`]),
//!   quindi niente aliasing anche con la saturazione al massimo.
//!
//! Tutta la memoria viene allocata in [`TubeCompressor::new`]: l'elaborazione
//! non alloca e non usa lock.

use crate::oversampling::{design_halfband, round_trip_delay, Downsampler2, Upsampler2};
use crate::MAX_CHANNELS;

/// Coefficienti del primo stadio (frequenza originale <-> doppia).
const STAGE1_TAPS: usize = 91;
/// Coefficienti del secondo stadio (doppia <-> quadrupla).
const STAGE2_TAPS: usize = 27;
const KAISER_BETA: f64 = 11.2;

/// Larghezza del ginocchio morbido (dB): le celle ottiche entrano in
/// compressione molto gradualmente.
const KNEE_DB: f32 = 8.0;

/// Il rilevatore ricorda il picco degli ultimi `HOLD_SEGMENTS` segmenti da
/// `HOLD_SEGMENT_MS` (cioe' 9-12 ms). L'onda raddrizzata ha un picco ogni
/// mezzo periodo, quindi basta per tutte le note sopra ~56 Hz: su una nota
/// tenuta il livello e' fermo e il guadagno non "trema" a ogni ciclo (sarebbe
/// distorsione sulle voci basse). Il rilascio parte al massimo 12 ms dopo.
const HOLD_SEGMENTS: usize = 4;
const HOLD_SEGMENT_MS: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompMode {
    /// Solo cella ottica: tempi automatici, dipendenti dal programma.
    Fixed,
    /// Attacco e rilascio esattamente come impostati.
    Manual,
    /// Cella ottica e tempi manuali insieme.
    FixedManual,
}

#[derive(Clone, Copy, Debug)]
pub struct CompressorSettings {
    pub enabled: bool,
    /// Soglia in dBFS.
    pub threshold_db: f32,
    /// Rapporto di compressione (2 = 2:1).
    pub ratio: f32,
    /// Attacco in millisecondi (modalita' manuali).
    pub attack_ms: f32,
    /// Rilascio in millisecondi (modalita' manuali).
    pub release_ms: f32,
    pub mode: CompMode,
    /// Quantita' di saturazione della valvola, 0..1.
    pub tube: f32,
    /// Miscela tra suono originale (0) e compresso (1): compressione parallela.
    pub mix: f32,
    /// Guadagno d'uscita in dB.
    pub output_db: f32,
}

impl Default for CompressorSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold_db: -18.0,
            ratio: 3.0,
            attack_ms: 10.0,
            release_ms: 300.0,
            mode: CompMode::Fixed,
            tube: 0.3,
            mix: 1.0,
            output_db: 0.0,
        }
    }
}

/// Lo stato per un canale.
struct Channel {
    up1: Upsampler2,
    up2: Upsampler2,
    down2: Downsampler2,
    down1: Downsampler2,
    /// Un campione di ritardo a frequenza doppia (allinea il ritardo totale a
    /// un numero intero di campioni).
    align: f32,
    /// Filtro passa-basso del trasformatore (a frequenza quadrupla).
    transformer_lp: f32,
    /// Filtro che toglie la continua creata dalla curva asimmetrica.
    dc_x: f32,
    dc_y: f32,
    /// Filtro passa-alto del rilevatore (le basse non devono far "pompare").
    detector_x: f32,
    detector_y: f32,
    /// Il segnale originale, ritardato come quello elaborato.
    dry: Vec<f32>,
    /// Il segnale compresso (prima della valvola), ritardato allo stesso modo.
    clean: Vec<f32>,
}

pub struct TubeCompressor {
    sample_rate: f32,
    channels: usize,
    channel: Vec<Channel>,
    latency: usize,
    dry_pos: usize,

    // Rilevatore con tenuta del picco.
    hold_segments: [f32; HOLD_SEGMENTS],
    hold_index: usize,
    hold_current: f32,
    hold_count: usize,
    hold_segment_len: usize,

    // Coefficienti dei filtri fissi.
    dc_coeff: f32,
    detector_coeff: f32,
    transformer_coeff: f32,

    // Cella ottica.
    /// Riduzione di guadagno in uscita dalla cella (dB, positiva).
    gain_reduction: f32,
    /// Ramo manuale (dB).
    manual_state: f32,
    /// Fase veloce e fase lenta della fotoresistenza (dB).
    opto_fast: f32,
    opto_slow: f32,
    /// Memoria della fotoresistenza, 0..1.
    opto_memory: f32,

    /// Accensione/spegnimento senza clic (0 = spento, 1 = acceso).
    enable_ramp: f32,
    enable_step: f32,

    /// Coefficienti calcolati una volta sola (o solo quando cambia la manopola).
    opto_fast_release: f32,
    memory_charge: f32,
    memory_discharge: f32,
    cached_attack_ms: f32,
    cached_attack: f32,
    cached_release_ms: f32,
    cached_release: f32,

    /// Parametri "lisciati" per non creare scalini quando si gira una manopola.
    smooth_output: f32,
    smooth_mix: f32,
    smooth_tube: f32,
    smooth_coeff: f32,
}

impl TubeCompressor {
    pub fn new(sample_rate: f32, channels: usize) -> Self {
        let channels = channels.clamp(1, MAX_CHANNELS);
        let stage1 = design_halfband(STAGE1_TAPS, KAISER_BETA);
        let stage2 = design_halfband(STAGE2_TAPS, KAISER_BETA);
        let latency = Self::fixed_latency();
        let channel = (0..channels)
            .map(|_| Channel {
                up1: Upsampler2::new(&stage1),
                up2: Upsampler2::new(&stage2),
                down2: Downsampler2::new(&stage2),
                down1: Downsampler2::new(&stage1),
                align: 0.0,
                transformer_lp: 0.0,
                dc_x: 0.0,
                dc_y: 0.0,
                detector_x: 0.0,
                detector_y: 0.0,
                dry: vec![0.0; latency + 1],
                clean: vec![0.0; latency + 1],
            })
            .collect();

        let one_pole = |hz: f32, rate: f32| (-2.0 * std::f32::consts::PI * hz / rate).exp();
        let mut compressor = Self {
            sample_rate,
            channels,
            channel,
            latency,
            dry_pos: 0,
            hold_segments: [0.0; HOLD_SEGMENTS],
            hold_index: 0,
            hold_current: 0.0,
            hold_count: 0,
            hold_segment_len: ((HOLD_SEGMENT_MS / 1000.0 * sample_rate).round() as usize).max(1),
            dc_coeff: one_pole(5.0, sample_rate),
            detector_coeff: one_pole(60.0, sample_rate),
            transformer_coeff: one_pole(150.0, 4.0 * sample_rate),
            gain_reduction: 0.0,
            manual_state: 0.0,
            opto_fast: 0.0,
            opto_slow: 0.0,
            opto_memory: 0.0,
            opto_fast_release: time_coeff(60.0, sample_rate),
            memory_charge: time_coeff(2000.0, sample_rate),
            memory_discharge: time_coeff(6000.0, sample_rate),
            cached_attack_ms: -1.0,
            cached_attack: 0.0,
            cached_release_ms: -1.0,
            cached_release: 0.0,
            enable_ramp: 0.0,
            enable_step: 1.0 / (0.01 * sample_rate).max(1.0),
            smooth_output: 1.0,
            smooth_mix: 1.0,
            smooth_tube: 0.0,
            smooth_coeff: 1.0 - (-1.0 / (0.02 * sample_rate)).exp(),
        };
        compressor.reset();
        compressor
    }

    /// Ritardo del compressore in campioni: sempre lo stesso, acceso o spento,
    /// a qualunque frequenza di campionamento.
    pub const fn fixed_latency() -> usize {
        // Stadio 1: (91-1)/2 = 45. Stadio 2: (27-1)/2 = 13 campioni a
        // frequenza doppia, piu' 1 di allineamento = 7 alla frequenza originale.
        (STAGE1_TAPS - 1) / 2 + ((STAGE2_TAPS - 1) / 2 + 1) / 2
    }

    pub fn latency(&self) -> usize {
        self.latency
    }

    /// Riduzione di guadagno attuale in dB (per l'indicatore).
    pub fn gain_reduction_db(&self) -> f32 {
        self.gain_reduction * self.enable_ramp
    }

    pub fn reset(&mut self) {
        for ch in &mut self.channel {
            ch.up1.reset();
            ch.up2.reset();
            ch.down2.reset();
            ch.down1.reset();
            ch.align = 0.0;
            ch.transformer_lp = 0.0;
            ch.dc_x = 0.0;
            ch.dc_y = 0.0;
            ch.detector_x = 0.0;
            ch.detector_y = 0.0;
            ch.dry.fill(0.0);
            ch.clean.fill(0.0);
        }
        self.dry_pos = 0;
        self.hold_segments = [0.0; HOLD_SEGMENTS];
        self.hold_index = 0;
        self.hold_current = 0.0;
        self.hold_count = 0;
        self.gain_reduction = 0.0;
        self.manual_state = 0.0;
        self.opto_fast = 0.0;
        self.opto_slow = 0.0;
        self.opto_memory = 0.0;
    }

    /// Prepara lo stato "acceso/spento" e i parametri lisciati, cosi' il primo
    /// blocco dopo il caricamento di un progetto non parte con una dissolvenza.
    pub fn snap_to(&mut self, settings: &CompressorSettings) {
        self.enable_ramp = if settings.enabled { 1.0 } else { 0.0 };
        self.smooth_output = db_to_gain(settings.output_db);
        self.smooth_mix = settings.mix.clamp(0.0, 1.0);
        self.smooth_tube = settings.tube.clamp(0.0, 1.0);
    }

    /// La curva del compressore: quanti dB togliere per un certo livello.
    #[inline]
    pub fn static_gain_reduction(level_db: f32, threshold_db: f32, ratio: f32) -> f32 {
        let slope = 1.0 - 1.0 / ratio.max(1.0);
        let over = level_db - threshold_db;
        if 2.0 * over <= -KNEE_DB {
            0.0
        } else if 2.0 * over < KNEE_DB {
            let x = over + KNEE_DB / 2.0;
            slope * x * x / (2.0 * KNEE_DB)
        } else {
            slope * over
        }
    }

    /// Elabora un campione per canale, sul posto.
    #[inline]
    pub fn process_frame(&mut self, frame: &mut [f32], settings: &CompressorSettings) {
        let channels = frame.len().min(self.channels);
        let frame = &mut frame[..channels];

        // 1) Rilevatore: picco dei canali (stereo collegato), senza le basse.
        let mut peak = 0.0f32;
        for (ch, &x) in self.channel.iter_mut().zip(frame.iter()) {
            ch.detector_y = self.detector_coeff * (ch.detector_y + x - ch.detector_x);
            ch.detector_x = x;
            peak = peak.max(ch.detector_y.abs());
        }
        // Tenuta del picco: il massimo del segmento in corso e dei precedenti.
        self.hold_current = self.hold_current.max(peak);
        self.hold_count += 1;
        let mut held = self.hold_current;
        for &segment in &self.hold_segments {
            held = held.max(segment);
        }
        if self.hold_count >= self.hold_segment_len {
            self.hold_segments[self.hold_index] = self.hold_current;
            self.hold_index = (self.hold_index + 1) % HOLD_SEGMENTS;
            self.hold_current = 0.0;
            self.hold_count = 0;
        }
        let level_db = 20.0 * (held + 1e-9).log10();
        let target =
            Self::static_gain_reduction(level_db, settings.threshold_db, settings.ratio.clamp(1.0, 20.0));

        // 2) Cella ottica / tempi manuali.
        self.update_ballistics(target, settings);
        let gain = db_to_gain(-self.gain_reduction);

        // 3) Parametri lisciati e accensione senza clic.
        let k = self.smooth_coeff;
        self.smooth_output += k * (db_to_gain(settings.output_db) - self.smooth_output);
        self.smooth_mix += k * (settings.mix.clamp(0.0, 1.0) - self.smooth_mix);
        self.smooth_tube += k * (settings.tube.clamp(0.0, 1.0) - self.smooth_tube);
        if settings.enabled {
            self.enable_ramp = (self.enable_ramp + self.enable_step).min(1.0);
        } else {
            self.enable_ramp = (self.enable_ramp - self.enable_step).max(0.0);
        }

        // Curva della valvola: pendenza 1 attorno allo zero, quindi a
        // saturazione bassa il volume non cambia.
        let tube = self.smooth_tube;
        // Pendenza quadratica: fino a ~30% e' il colore discreto della valvola
        // (circa 0,5% di distorsione sui picchi), oltre diventa saturazione
        // evidente.
        let drive = 3.2 * tube * tube;
        let bias = 0.18;
        let tube_on = drive > 1e-3;
        let (t0, norm) = if tube_on {
            let t0 = (drive * bias).tanh();
            (t0, 1.0 / (drive * (1.0 - t0 * t0)))
        } else {
            (0.0, 1.0)
        };
        let transformer = 0.6 * tube;
        let tlp = self.transformer_coeff;

        let dry_pos = self.dry_pos;
        let delayed_pos = (dry_pos + 1) % self.channel[0].dry.len();
        for (ch, sample) in self.channel.iter_mut().zip(frame.iter_mut()) {
            let input = *sample;
            ch.dry[dry_pos] = input;
            let dry = ch.dry[delayed_pos];

            // 4) Guadagno della cella, poi la valvola a frequenza quadrupla.
            let compressed = input * gain;
            ch.clean[dry_pos] = compressed;
            let clean = ch.clean[delayed_pos];
            let mut double = [0.0f32; 2];
            for (slot, &u) in double.iter_mut().zip(ch.up1.process(compressed).iter()) {
                let mut quad = ch.up2.process(u);
                for w in &mut quad {
                    // Trasformatore: le basse saturano un po' di piu'.
                    ch.transformer_lp += (1.0 - tlp) * (*w - ch.transformer_lp);
                    let mut v = *w;
                    if tube_on {
                        let low = ch.transformer_lp;
                        let low_sat = (1.8 * low).tanh() / 1.8;
                        v += transformer * (low_sat - low);
                    }
                    if tube_on {
                        v = ((drive * (v + bias)).tanh() - t0) * norm;
                    }
                    *w = v;
                }
                *slot = ch.down2.process(quad);
            }
            // Un campione di allineamento a frequenza doppia.
            let aligned = [ch.align, double[0]];
            ch.align = double[1];
            let wet_raw = ch.down1.process(aligned);

            // 5) Via la continua che la curva asimmetrica crea. Il filtro agisce
            //    solo sulla parte aggiunta dalla valvola: il segnale compresso
            //    passa intatto, senza spostamenti di fase sulle basse.
            let added = wet_raw - clean;
            ch.dc_y = self.dc_coeff * (ch.dc_y + added - ch.dc_x);
            ch.dc_x = added;
            let wet = (clean + ch.dc_y) * self.smooth_output;

            // 6) Miscela parallela e accensione.
            let processed = dry + self.smooth_mix * (wet - dry);
            *sample = dry + self.enable_ramp * (processed - dry);
        }
        self.dry_pos = delayed_pos;
    }

    #[inline]
    fn update_ballistics(&mut self, target: f32, settings: &CompressorSettings) {
        let sr = self.sample_rate;

        // Ramo manuale: attacco e rilascio esatti (nel dominio dei dB).
        let manual = if matches!(settings.mode, CompMode::Manual | CompMode::FixedManual) {
            let attack_ms = settings.attack_ms.clamp(0.1, 1000.0);
            if attack_ms != self.cached_attack_ms {
                self.cached_attack_ms = attack_ms;
                self.cached_attack = time_coeff(attack_ms, sr);
            }
            let release_ms = settings.release_ms.clamp(10.0, 20000.0);
            if release_ms != self.cached_release_ms {
                self.cached_release_ms = release_ms;
                self.cached_release = time_coeff(release_ms, sr);
            }
            let c = if target > self.manual_state { self.cached_attack } else { self.cached_release };
            self.manual_state += c * (target - self.manual_state);
            self.manual_state
        } else {
            self.manual_state = 0.0;
            0.0
        };

        // Ramo ottico.
        let opto = if matches!(settings.mode, CompMode::Fixed | CompMode::FixedManual) {
            // La fase lenta si "carica" solo in parte: e' lei che, nel rilascio,
            // trattiene la compressione e la lascia andare piano.
            let hold = 0.3 + 0.5 * self.opto_memory;
            let slow_target = target * hold;
            let attacking = target > self.opto_fast || slow_target > self.opto_slow;
            // Attacco: ~10 ms, piu' veloce sui picchi forti.
            let attack = if attacking {
                let over = (target - self.opto_fast).max(0.0);
                time_coeff(10.0 / (1.0 + over / 6.0), sr)
            } else {
                0.0
            };
            if target > self.opto_fast {
                self.opto_fast += attack * (target - self.opto_fast);
            } else {
                self.opto_fast += self.opto_fast_release * (target - self.opto_fast);
            }
            if slow_target > self.opto_slow {
                self.opto_slow += attack * (slow_target - self.opto_slow);
            } else {
                // Il rilascio lento si allunga con la memoria (da 0,4 a 2,4 s).
                // Approssimazione al primo ordine di 1 - e^(-x): x e' piccolissimo.
                let slow_release_ms = 400.0 + 2000.0 * self.opto_memory;
                let c = 1000.0 / (slow_release_ms * sr);
                self.opto_slow += c * (slow_target - self.opto_slow);
            }
            // Memoria: si carica in ~2 s di compressione, si scarica in ~6 s.
            let opto = self.opto_fast.max(self.opto_slow);
            let charge = (opto / 12.0).min(1.0);
            let c = if charge > self.opto_memory { self.memory_charge } else { self.memory_discharge };
            self.opto_memory += c * (charge - self.opto_memory);
            opto
        } else {
            self.opto_fast = 0.0;
            self.opto_slow = 0.0;
            self.opto_memory = 0.0;
            0.0
        };

        self.gain_reduction = manual.max(opto);
    }
}

/// Coefficiente di un filtro a un polo con costante di tempo `ms`.
#[inline]
fn time_coeff(ms: f32, sample_rate: f32) -> f32 {
    1.0 - (-1000.0 / (ms.max(0.05) * sample_rate)).exp()
}

#[inline]
fn db_to_gain(db: f32) -> f32 {
    (db * (std::f32::consts::LN_10 / 20.0)).exp()
}

/// Ritardo di andata e ritorno del sovracampionamento, per i test.
pub fn oversampling_latency() -> usize {
    round_trip_delay(STAGE1_TAPS) + (round_trip_delay(STAGE2_TAPS) + 1) / 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latency_is_integer_and_consistent() {
        assert_eq!(TubeCompressor::fixed_latency(), oversampling_latency());
        assert_eq!(TubeCompressor::fixed_latency(), 52);
    }

    #[test]
    fn soft_knee_curve() {
        let gr = |l| TubeCompressor::static_gain_reduction(l, -20.0, 4.0);
        assert_eq!(gr(-30.0), 0.0);
        assert!(gr(-20.0) > 0.0 && gr(-20.0) < 1.0);
        assert!((gr(0.0) - 15.0).abs() < 1e-4);
    }
}
