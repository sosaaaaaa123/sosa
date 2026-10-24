//! I parametri di SR Vocal Suite: tutto cio' che si regola, che il DAW salva
//! nel progetto e che puo' automatizzare.
//!
//! La correzione dell'intonazione e' quella di SosaTune (stessi parametri e
//! stessi id); in piu' ci sono Note Stabilizer, Monitor e i quattro moduli
//! in basso (Chorus, Formant, Reverb, Delay) con le loro impostazioni.

use crate::gui::EguiState;
use nih_plug::prelude::*;
use sosatune_dsp::fx::{ChorusMode, ChorusSettings, ReverbSettings, SpaceEchoSettings};
use sosatune_dsp::scale::{scale_notes, ScaleKind, VoiceType};
use sosatune_dsp::CorrectionSettings;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::Arc;

/// Dimensione iniziale della finestra (punti, al 100%): le proporzioni
/// dell'immagine di riferimento (1448 x 1086).
pub const EDITOR_WIDTH: u32 = 1024;
pub const EDITOR_HEIGHT: u32 = 768;

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    #[id = "c"]
    #[name = "C (Do)"]
    C,
    #[id = "c-sharp"]
    #[name = "C# / Db (Do#)"]
    CSharp,
    #[id = "d"]
    #[name = "D (Re)"]
    D,
    #[id = "d-sharp"]
    #[name = "D# / Eb (Mib)"]
    DSharp,
    #[id = "e"]
    #[name = "E (Mi)"]
    E,
    #[id = "f"]
    #[name = "F (Fa)"]
    F,
    #[id = "f-sharp"]
    #[name = "F# / Gb (Fa#)"]
    FSharp,
    #[id = "g"]
    #[name = "G (Sol)"]
    G,
    #[id = "g-sharp"]
    #[name = "G# / Ab (Lab)"]
    GSharp,
    #[id = "a"]
    #[name = "A (La)"]
    A,
    #[id = "a-sharp"]
    #[name = "A# / Bb (Sib)"]
    ASharp,
    #[id = "b"]
    #[name = "B (Si)"]
    B,
}

impl Key {
    pub fn semitone(self) -> u8 {
        self.to_index() as u8
    }
}

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleChoice {
    #[id = "chromatic"]
    #[name = "Cromatica"]
    Chromatic,
    #[id = "major"]
    #[name = "Maggiore"]
    Major,
    #[id = "natural-minor"]
    #[name = "Minore naturale"]
    NaturalMinor,
    #[id = "harmonic-minor"]
    #[name = "Minore armonica"]
    HarmonicMinor,
    #[id = "major-pentatonic"]
    #[name = "Pentatonica maggiore"]
    MajorPentatonic,
    #[id = "minor-pentatonic"]
    #[name = "Pentatonica minore"]
    MinorPentatonic,
    #[id = "blues"]
    #[name = "Blues"]
    Blues,
    #[id = "custom"]
    #[name = "Personalizzata"]
    Custom,
}

impl ScaleChoice {
    pub fn kind(self) -> Option<ScaleKind> {
        match self {
            ScaleChoice::Chromatic => Some(ScaleKind::Chromatic),
            ScaleChoice::Major => Some(ScaleKind::Major),
            ScaleChoice::NaturalMinor => Some(ScaleKind::NaturalMinor),
            ScaleChoice::HarmonicMinor => Some(ScaleKind::HarmonicMinor),
            ScaleChoice::MajorPentatonic => Some(ScaleKind::MajorPentatonic),
            ScaleChoice::MinorPentatonic => Some(ScaleKind::MinorPentatonic),
            ScaleChoice::Blues => Some(ScaleKind::Blues),
            ScaleChoice::Custom => None,
        }
    }
}

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceChoice {
    #[id = "soprano"]
    #[name = "Soprano / voce acuta"]
    Soprano,
    #[id = "alto-tenor"]
    #[name = "Alto / Tenore"]
    AltoTenor,
    #[id = "low"]
    #[name = "Voce bassa (baritono, basso)"]
    Low,
}

impl VoiceChoice {
    pub fn to_dsp(self) -> VoiceType {
        match self {
            VoiceChoice::Soprano => VoiceType::Soprano,
            VoiceChoice::AltoTenor => VoiceType::AltoTenor,
            VoiceChoice::Low => VoiceType::LowMale,
        }
    }
}

/// Monitor acceso = Low Latency (0 ms, per registrare in cuffia);
/// spento = HQ (qualita' piena per il mix, latenza compensata dal DAW).
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityChoice {
    #[id = "low-latency"]
    #[name = "Monitor (Low Latency)"]
    LowLatency,
    #[id = "hq"]
    #[name = "HQ"]
    Hq,
}

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleChoice {
    #[id = "modern"]
    #[name = "Modern"]
    Modern,
    #[id = "classic"]
    #[name = "Classic"]
    Classic,
}

/// Note Stabilizer: tiene ferme le note lunghe togliendo il vibrato e le
/// oscillazioni lente dell'intonazione (sopra la correzione).
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stabilizer {
    #[id = "off"]
    #[name = "Off"]
    Off,
    #[id = "light"]
    #[name = "Light"]
    Light,
    #[id = "medium"]
    #[name = "Medium"]
    Medium,
    #[id = "strong"]
    #[name = "Strong"]
    Strong,
}

impl Stabilizer {
    /// Quanto vibrato togliere (si somma al comando Vibrato).
    pub fn vibrato_reduction(self) -> f32 {
        match self {
            Stabilizer::Off => 0.0,
            Stabilizer::Light => 0.35,
            Stabilizer::Medium => 0.65,
            Stabilizer::Strong => 0.95,
        }
    }
}

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChorusModeChoice {
    #[id = "i"]
    #[name = "I"]
    I,
    #[id = "ii"]
    #[name = "II"]
    II,
    #[id = "i-ii"]
    #[name = "I + II"]
    Both,
}

impl ChorusModeChoice {
    pub fn to_dsp(self) -> ChorusMode {
        match self {
            ChorusModeChoice::I => ChorusMode::I,
            ChorusModeChoice::II => ChorusMode::II,
            ChorusModeChoice::Both => ChorusMode::Both,
        }
    }
}

/// Tempo della testina 1 del tape echo: libero (ms) o agganciato al tempo
/// del DAW. Le testine 2 e 3 sono a 2x e 3x (con 1/16: 1/8 e 1/8 puntato).
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelaySync {
    #[id = "free"]
    #[name = "Free (ms)"]
    Free,
    #[id = "1-32"]
    #[name = "1/32"]
    ThirtySecond,
    #[id = "1-16t"]
    #[name = "1/16 T"]
    SixteenthTriplet,
    #[id = "1-16"]
    #[name = "1/16"]
    Sixteenth,
    #[id = "1-8t"]
    #[name = "1/8 T"]
    EighthTriplet,
    #[id = "1-8"]
    #[name = "1/8"]
    Eighth,
    #[id = "1-8d"]
    #[name = "1/8 D"]
    EighthDotted,
    #[id = "1-4"]
    #[name = "1/4"]
    Quarter,
}

impl DelaySync {
    /// Durata in battiti (semiminime), `None` = libero.
    pub fn beats(self) -> Option<f32> {
        match self {
            DelaySync::Free => None,
            DelaySync::ThirtySecond => Some(0.125),
            DelaySync::SixteenthTriplet => Some(1.0 / 6.0),
            DelaySync::Sixteenth => Some(0.25),
            DelaySync::EighthTriplet => Some(1.0 / 3.0),
            DelaySync::Eighth => Some(0.5),
            DelaySync::EighthDotted => Some(0.75),
            DelaySync::Quarter => Some(1.0),
        }
    }
}

/// Le 12 note della scala personalizzata.
#[derive(Params)]
pub struct CustomNotes {
    #[id = "note-c"]
    pub c: BoolParam,
    #[id = "note-cs"]
    pub c_sharp: BoolParam,
    #[id = "note-d"]
    pub d: BoolParam,
    #[id = "note-ds"]
    pub d_sharp: BoolParam,
    #[id = "note-e"]
    pub e: BoolParam,
    #[id = "note-f"]
    pub f: BoolParam,
    #[id = "note-fs"]
    pub f_sharp: BoolParam,
    #[id = "note-g"]
    pub g: BoolParam,
    #[id = "note-gs"]
    pub g_sharp: BoolParam,
    #[id = "note-a"]
    pub a: BoolParam,
    #[id = "note-as"]
    pub a_sharp: BoolParam,
    #[id = "note-b"]
    pub b: BoolParam,
}

impl Default for CustomNotes {
    fn default() -> Self {
        Self::with_defaults(&HashMap::new())
    }
}

impl CustomNotes {
    fn with_defaults(saved: &HashMap<String, f32>) -> Self {
        let note = |name: &str, id: &str| {
            BoolParam::new(format!("Nota {name}"), saved.get(&format!("note-{id}")).is_none_or(|&v| v >= 0.5))
        };
        Self {
            c: note("Do", "c"),
            c_sharp: note("Do#", "cs"),
            d: note("Re", "d"),
            d_sharp: note("Re#", "ds"),
            e: note("Mi", "e"),
            f: note("Fa", "f"),
            f_sharp: note("Fa#", "fs"),
            g: note("Sol", "g"),
            g_sharp: note("Sol#", "gs"),
            a: note("La", "a"),
            a_sharp: note("La#", "as"),
            b: note("Si", "b"),
        }
    }

    pub fn note(&self, pitch_class: usize) -> &BoolParam {
        match pitch_class % 12 {
            0 => &self.c,
            1 => &self.c_sharp,
            2 => &self.d,
            3 => &self.d_sharp,
            4 => &self.e,
            5 => &self.f,
            6 => &self.f_sharp,
            7 => &self.g,
            8 => &self.g_sharp,
            9 => &self.a,
            10 => &self.a_sharp,
            _ => &self.b,
        }
    }

    pub fn mask(&self) -> [bool; 12] {
        std::array::from_fn(|pc| self.note(pc).value())
    }
}

#[derive(Params)]
pub struct VocalSuiteParams {
    /// Dimensione della finestra, salvata con il progetto.
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,
    /// L'ultimo preset caricato (indice in `presets::FACTORY`).
    #[persist = "preset"]
    pub preset: Arc<AtomicUsize>,
    /// AutoKey: la tonalita' si imposta da sola ascoltando.
    #[persist = "autokey"]
    pub autokey: Arc<AtomicBool>,

    // --- Correzione (motore SosaTune) -------------------------------------
    #[id = "key"]
    pub key: EnumParam<Key>,
    #[id = "scale"]
    pub scale: EnumParam<ScaleChoice>,
    #[id = "voice"]
    pub voice: EnumParam<VoiceChoice>,
    #[id = "retune"]
    pub retune_speed: FloatParam,
    #[id = "humanize"]
    pub humanize: FloatParam,
    #[id = "amount"]
    pub amount: FloatParam,
    #[id = "transpose"]
    pub transpose: IntParam,
    #[id = "flex-tune"]
    pub flex_tune: FloatParam,
    #[id = "vibrato"]
    pub vibrato: FloatParam,
    #[id = "tracking"]
    pub tracking: FloatParam,
    /// Accordatura (La4), mostrata accanto al preset.
    #[id = "detune"]
    pub detune: FloatParam,
    #[id = "style"]
    pub style: EnumParam<StyleChoice>,
    /// Monitor: Low Latency (acceso) / HQ (spento).
    #[id = "quality"]
    pub quality: EnumParam<QualityChoice>,
    #[id = "stabilizer"]
    pub stabilizer: EnumParam<Stabilizer>,
    #[id = "bypass"]
    pub bypass: BoolParam,

    // --- Moduli in basso (0..100) ----------------------------------------
    #[id = "chorus"]
    pub chorus: FloatParam,
    /// FORMANT · CHARACTER: 50 = timbro naturale, 0 = piu' grande e scuro,
    /// 100 = piu' piccolo e chiaro (±6 semitoni).
    #[id = "formant-character"]
    pub formant: FloatParam,
    #[id = "reverb"]
    pub reverb: FloatParam,
    #[id = "delay"]
    pub delay: FloatParam,

    // --- Impostazioni (pannello ⚙) -----------------------------------------
    #[id = "chorus-mode"]
    pub chorus_mode: EnumParam<ChorusModeChoice>,
    /// 0% = solo Juno, 100% = solo FL.
    #[id = "chorus-blend"]
    pub chorus_blend: FloatParam,
    #[id = "chorus-rate"]
    pub chorus_rate: FloatParam,

    #[id = "delay-mode"]
    pub delay_mode: IntParam,
    #[id = "delay-sync"]
    pub delay_sync: EnumParam<DelaySync>,
    #[id = "delay-time"]
    pub delay_time: FloatParam,
    #[id = "delay-intensity"]
    pub delay_intensity: FloatParam,
    #[id = "delay-bass"]
    pub delay_bass: FloatParam,
    #[id = "delay-treble"]
    pub delay_treble: FloatParam,
    #[id = "delay-wow"]
    pub delay_wow: FloatParam,
    #[id = "delay-spring"]
    pub delay_spring: FloatParam,

    #[id = "reverb-decay"]
    pub reverb_decay: FloatParam,
    #[id = "reverb-predelay"]
    pub reverb_predelay: FloatParam,
    #[id = "reverb-damping"]
    pub reverb_damping: FloatParam,
    #[id = "reverb-lowcut"]
    pub reverb_lowcut: FloatParam,

    #[id = "output"]
    pub output_gain: FloatParam,

    #[nested(group = "Scala personalizzata")]
    pub custom: CustomNotes,
}

impl Default for VocalSuiteParams {
    fn default() -> Self {
        Self::with_defaults(&crate::defaults::load())
    }
}

/// Un parametro 0..100 (le manopole grandi e le sfere).
fn knob_param(name: &str, default: f32) -> FloatParam {
    FloatParam::new(name, default.clamp(0.0, 100.0), FloatRange::Linear { min: 0.0, max: 100.0 })
        .with_step_size(0.1)
        .with_value_to_string(formatters::v2s_f32_rounded(0))
}

fn percent_param(name: &str, default: f32, min: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min, max: 1.0 })
        .with_unit("%")
        .with_value_to_string(formatters::v2s_f32_percentage(0))
        .with_string_to_value(formatters::s2v_f32_percentage())
}

fn retune_param(default: f32) -> FloatParam {
    FloatParam::new(
        "Retune Speed",
        default,
        FloatRange::Skewed { min: 0.0, max: 400.0, factor: FloatRange::skew_factor(-1.5) },
    )
    .with_unit(" ms")
    .with_step_size(1.0)
    .with_value_to_string(formatters::v2s_f32_rounded(0))
}

fn tone_param(name: &str, default: f32) -> FloatParam {
    FloatParam::new(name, default.clamp(-1.0, 1.0), FloatRange::Linear { min: -1.0, max: 1.0 })
        .with_value_to_string(Arc::new(|v| format!("{:+.1} dB", v * 12.0)))
        .with_string_to_value(Arc::new(|s| {
            s.trim().trim_end_matches("dB").trim().replace(',', ".").parse::<f32>().ok().map(|db| (db / 12.0).clamp(-1.0, 1.0))
        }))
}

impl VocalSuiteParams {
    pub(crate) fn with_defaults(saved: &HashMap<String, f32>) -> Self {
        let d = |id: &str, fallback: f32| saved.get(id).copied().filter(|v| v.is_finite()).unwrap_or(fallback);
        fn pick<T: Enum>(saved: &HashMap<String, f32>, id: &str, fallback: T) -> T {
            match saved.get(id) {
                Some(&v) if v.is_finite() && v >= 0.0 && (v as usize) < T::variants().len() => T::from_index(v as usize),
                _ => fallback,
            }
        }
        let p = &crate::presets::FACTORY[0];
        Self {
            editor_state: EguiState::from_size(EDITOR_WIDTH, EDITOR_HEIGHT),
            preset: Arc::new(AtomicUsize::new(0)),
            autokey: Arc::new(AtomicBool::new(false)),

            key: EnumParam::new("Tonalità", pick(saved, "key", Key::C)),
            scale: EnumParam::new("Scala", pick(saved, "scale", ScaleChoice::Chromatic)),
            voice: EnumParam::new("Vocal Range", pick(saved, "voice", VoiceChoice::AltoTenor)),
            retune_speed: retune_param(d("retune", p.retune_ms).clamp(0.0, 400.0)),
            humanize: percent_param("Humanize", d("humanize", p.humanize).clamp(0.0, 1.0), 0.0),
            amount: FloatParam::new("Quantità", d("amount", 1.0).clamp(0.0, 1.0), FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            transpose: IntParam::new("Trasposizione", d("transpose", 0.0).round().clamp(-12.0, 12.0) as i32, IntRange::Linear { min: -12, max: 12 })
                .with_unit(" st"),
            flex_tune: percent_param("Flex-Tune", d("flex-tune", p.flex_tune).clamp(0.0, 1.0), 0.0),
            vibrato: percent_param("Vibrato", d("vibrato", 0.0).clamp(-1.0, 1.0), -1.0),
            tracking: percent_param("Sensibilità", d("tracking", 0.3).clamp(0.0, 1.0), 0.0),
            detune: FloatParam::new("Accordatura (La4)", d("detune", 440.0).clamp(410.0, 470.0), FloatRange::Linear { min: 410.0, max: 470.0 })
                .with_unit(" Hz")
                .with_step_size(0.1)
                .with_value_to_string(formatters::v2s_f32_rounded(1)),
            style: EnumParam::new("Style", pick(saved, "style", p.style)),
            quality: EnumParam::new("Monitor", pick(saved, "quality", QualityChoice::Hq)),
            stabilizer: EnumParam::new("Note Stabilizer", pick(saved, "stabilizer", p.stabilizer)),
            bypass: BoolParam::new("Bypass", false).make_bypass(),

            chorus: knob_param("Chorus Width", d("chorus", p.chorus)),
            formant: knob_param("Formant Character", d("formant-character", p.formant)),
            reverb: knob_param("Reverb Space", d("reverb", p.reverb)),
            delay: knob_param("Delay Time", d("delay", p.delay)),

            chorus_mode: EnumParam::new("Chorus Mode", pick(saved, "chorus-mode", p.chorus_mode)),
            chorus_blend: percent_param("Chorus Juno/FL", d("chorus-blend", p.chorus_blend).clamp(0.0, 1.0), 0.0),
            chorus_rate: FloatParam::new("Chorus Rate", d("chorus-rate", 1.0).clamp(0.25, 4.0), FloatRange::Skewed { min: 0.25, max: 4.0, factor: FloatRange::skew_factor(-1.0) })
                .with_unit("x")
                .with_value_to_string(formatters::v2s_f32_rounded(2)),

            delay_mode: IntParam::new("Echo Mode", d("delay-mode", p.delay_mode as f32).round().clamp(1.0, 12.0) as i32, IntRange::Linear { min: 1, max: 12 }),
            delay_sync: EnumParam::new("Echo Sync", pick(saved, "delay-sync", p.delay_sync)),
            delay_time: FloatParam::new("Repeat Rate", d("delay-time", 125.0).clamp(20.0, 1000.0), FloatRange::Skewed { min: 20.0, max: 1000.0, factor: FloatRange::skew_factor(-1.0) })
                .with_unit(" ms")
                .with_step_size(0.1)
                .with_value_to_string(formatters::v2s_f32_rounded(0)),
            delay_intensity: percent_param("Intensity", d("delay-intensity", p.delay_intensity).clamp(0.0, 1.0), 0.0),
            delay_bass: tone_param("Echo Bass", d("delay-bass", 0.0)),
            delay_treble: tone_param("Echo Treble", d("delay-treble", -0.15)),
            delay_wow: percent_param("Wow & Flutter", d("delay-wow", 0.35).clamp(0.0, 1.0), 0.0),
            delay_spring: percent_param("Spring Reverb", d("delay-spring", 0.3).clamp(0.0, 1.0), 0.0),

            reverb_decay: FloatParam::new("Reverb Decay", d("reverb-decay", p.reverb_decay).clamp(0.3, 10.0), FloatRange::Skewed { min: 0.3, max: 10.0, factor: FloatRange::skew_factor(-1.2) })
                .with_unit(" s")
                .with_step_size(0.01)
                .with_value_to_string(formatters::v2s_f32_rounded(2)),
            reverb_predelay: FloatParam::new("Pre-delay", d("reverb-predelay", 24.0).clamp(0.0, 150.0), FloatRange::Linear { min: 0.0, max: 150.0 })
                .with_unit(" ms")
                .with_step_size(0.5)
                .with_value_to_string(formatters::v2s_f32_rounded(0)),
            reverb_damping: percent_param("Damping", d("reverb-damping", 0.45).clamp(0.0, 1.0), 0.0),
            reverb_lowcut: FloatParam::new("Reverb Low Cut", d("reverb-lowcut", 180.0).clamp(20.0, 800.0), FloatRange::Skewed { min: 20.0, max: 800.0, factor: FloatRange::skew_factor(-1.0) })
                .with_unit(" Hz")
                .with_step_size(1.0)
                .with_value_to_string(formatters::v2s_f32_rounded(0)),

            output_gain: FloatParam::new("Output", d("output", 0.0).clamp(-24.0, 12.0), FloatRange::Linear { min: -24.0, max: 12.0 })
                .with_unit(" dB")
                .with_step_size(0.1)
                .with_smoother(SmoothingStyle::Linear(30.0))
                .with_value_to_string(formatters::v2s_f32_rounded(1)),

            custom: CustomNotes::with_defaults(saved),
        }
    }

    /// I valori dell'immagine di riferimento (per l'anteprima e il confronto 1:1).
    pub(crate) fn preview() -> Self {
        let p = Self::default();
        Self {
            key: EnumParam::new("Tonalità", Key::A),
            scale: EnumParam::new("Scala", ScaleChoice::Major),
            chorus: knob_param("Chorus Width", 35.0),
            formant: knob_param("Formant Character", 62.0),
            reverb: knob_param("Reverb Space", 48.0),
            delay: knob_param("Delay Time", 27.0),
            // (115 ms: il pallino di Retune Speed dove sta nell'immagine)
            retune_speed: retune_param(115.0),
            humanize: percent_param("Humanize", 0.64, 0.0),
            quality: EnumParam::new("Monitor", QualityChoice::LowLatency),
            ..p
        }
    }

    /// Spostamento delle formanti in semitoni (dalla manopola 0..100).
    pub fn formant_semitones(&self) -> f32 {
        (self.formant.value() - 50.0) / 50.0 * 6.0
    }

    pub fn correction_settings(&self) -> CorrectionSettings {
        let vibrato = (self.vibrato.value() - self.stabilizer.value().vibrato_reduction()).clamp(-1.0, 1.0);
        CorrectionSettings {
            notes: self.active_notes(),
            retune_ms: self.retune_speed.value(),
            amount: self.amount.value(),
            transpose: self.transpose.value() as f32,
            humanize: self.humanize.value(),
            flex_tune: self.flex_tune.value(),
            vibrato,
            formant: self.formant_semitones(),
            tracking: self.tracking.value(),
            reference_hz: self.detune.value(),
            classic: self.style.value() == StyleChoice::Classic,
        }
    }

    pub fn scale_notes(&self) -> [bool; 12] {
        match self.scale.value().kind() {
            Some(kind) => scale_notes(self.key.value().semitone(), kind),
            None => self.custom.mask(),
        }
    }

    /// Le note ammesse (una scala vuota non correggerebbe niente: in quel
    /// caso valgono tutte).
    pub fn active_notes(&self) -> [bool; 12] {
        let notes = self.scale_notes();
        if notes.iter().any(|&on| on) {
            notes
        } else {
            [true; 12]
        }
    }

    pub fn chorus_settings(&self) -> ChorusSettings {
        ChorusSettings {
            mix: self.chorus.value() / 100.0,
            mode: self.chorus_mode.value().to_dsp(),
            blend: self.chorus_blend.value(),
            rate: self.chorus_rate.value(),
        }
    }

    /// Le impostazioni del tape echo; `tempo` = battiti al minuto del DAW
    /// (se il DAW non lo dice: 120).
    pub fn echo_settings(&self, tempo: Option<f64>) -> SpaceEchoSettings {
        let head_ms = match self.delay_sync.value().beats() {
            None => self.delay_time.value(),
            Some(beats) => {
                let bpm = tempo.filter(|t| t.is_finite() && *t > 20.0).unwrap_or(120.0) as f32;
                (beats * 60_000.0 / bpm).clamp(sosatune_dsp::fx::space_echo::MIN_HEAD_MS, sosatune_dsp::fx::space_echo::MAX_HEAD_MS)
            }
        };
        SpaceEchoSettings {
            mode: self.delay_mode.value().clamp(1, 12) as u8,
            head_ms,
            intensity: self.delay_intensity.value(),
            bass: self.delay_bass.value(),
            treble: self.delay_treble.value(),
            spring: self.delay_spring.value(),
            wow: self.delay_wow.value(),
            spread: 0.5,
        }
    }

    pub fn reverb_settings(&self) -> ReverbSettings {
        ReverbSettings {
            decay_s: self.reverb_decay.value(),
            predelay_ms: self.reverb_predelay.value(),
            damping: self.reverb_damping.value(),
            low_cut_hz: self.reverb_lowcut.value(),
            width: 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ogni comando arriva al motore.
    #[test]
    fn every_control_reaches_the_engine() {
        let p = VocalSuiteParams::with_defaults(&HashMap::new());
        let base = p.correction_settings();
        assert_eq!(base.formant, 0.0, "Formant a 50 = timbro naturale");

        let p = VocalSuiteParams { formant: knob_param("F", 100.0), ..VocalSuiteParams::with_defaults(&HashMap::new()) };
        assert!((p.correction_settings().formant - 6.0).abs() < 1e-5);
        let p = VocalSuiteParams { stabilizer: EnumParam::new("S", Stabilizer::Strong), ..VocalSuiteParams::with_defaults(&HashMap::new()) };
        assert!(p.correction_settings().vibrato < -0.9);
        let p = VocalSuiteParams { chorus: knob_param("C", 40.0), ..VocalSuiteParams::with_defaults(&HashMap::new()) };
        assert!((p.chorus_settings().mix - 0.4).abs() < 1e-6);
        let p = VocalSuiteParams {
            delay_sync: EnumParam::new("S", DelaySync::Sixteenth),
            ..VocalSuiteParams::with_defaults(&HashMap::new())
        };
        assert!((p.echo_settings(Some(120.0)).head_ms - 125.0).abs() < 1e-3, "1/16 a 120 BPM = 125 ms");
        assert!((p.echo_settings(Some(60.0)).head_ms - 250.0).abs() < 1e-3);
        let p = VocalSuiteParams { delay_sync: EnumParam::new("S", DelaySync::Free), ..VocalSuiteParams::with_defaults(&HashMap::new()) };
        assert_eq!(p.echo_settings(None).head_ms, p.delay_time.value());
        let p = VocalSuiteParams {
            key: EnumParam::new("K", Key::G),
            scale: EnumParam::new("S", ScaleChoice::NaturalMinor),
            ..VocalSuiteParams::with_defaults(&HashMap::new())
        };
        let expected: [bool; 12] = std::array::from_fn(|pc| [7, 9, 10, 0, 2, 3, 5].contains(&pc));
        assert_eq!(p.correction_settings().notes, expected);
    }

    #[test]
    fn saved_defaults_are_used() {
        let saved: HashMap<String, f32> =
            [("retune", 35.0), ("reverb", 70.0), ("delay-mode", 9.0), ("stabilizer", 2.0), ("key", 99.0)]
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect();
        let p = VocalSuiteParams::with_defaults(&saved);
        assert_eq!(p.retune_speed.value(), 35.0);
        assert_eq!(p.reverb.value(), 70.0);
        assert_eq!(p.delay_mode.value(), 9);
        assert_eq!(p.stabilizer.value(), Stabilizer::Medium);
        assert_eq!(p.key.value(), Key::C, "valore fuori scala ignorato");
    }
}
