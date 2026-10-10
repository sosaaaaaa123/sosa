//! I parametri del plugin: tutto cio' che l'utente puo' regolare e che il
//! DAW salva nel progetto e puo' automatizzare.

use sosatune_dsp::scale::{scale_notes, ScaleKind, VoiceType};
use sosatune_dsp::{CompMode, CompressorSettings};
use nih_plug::prelude::*;
use crate::gui::EguiState;
use std::sync::Arc;

/// Dimensione iniziale della finestra del plugin (pixel).
pub const EDITOR_WIDTH: u32 = 940;
pub const EDITOR_HEIGHT: u32 = 750;

/// La tonalita' (la nota "di casa" della scala).
///
/// Gli `id` non vanno mai cambiati: sono quelli salvati nei progetti del DAW.
/// I `name` invece sono solo testo visualizzato e si possono modificare.
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
    /// 0 = Do, 1 = Do#, ... 11 = Si.
    pub fn semitone(self) -> u8 {
        self.to_index() as u8
    }
}

/// Il tipo di scala. "Personalizzata" usa le note scelte sulla tastiera.
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

/// Il tipo di voce: decide la nota piu' grave riconosciuta e la latenza.
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

/// Le modalita' del compressore (come sull'hardware ottico a valvole).
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompModeChoice {
    #[id = "fixed"]
    #[name = "Ottica (automatica)"]
    Fixed,
    #[id = "manual"]
    #[name = "Manuale"]
    Manual,
    #[id = "fixed-manual"]
    #[name = "Ottica + manuale"]
    FixedManual,
}

impl CompModeChoice {
    pub fn to_dsp(self) -> CompMode {
        match self {
            CompModeChoice::Fixed => CompMode::Fixed,
            CompModeChoice::Manual => CompMode::Manual,
            CompModeChoice::FixedManual => CompMode::FixedManual,
        }
    }
}

/// Il compressore ottico a valvola, dopo la correzione dell'intonazione.
#[derive(Params)]
pub struct CompressorParams {
    #[id = "comp-on"]
    pub enabled: BoolParam,
    #[id = "comp-threshold"]
    pub threshold: FloatParam,
    #[id = "comp-ratio"]
    pub ratio: FloatParam,
    #[id = "comp-attack"]
    pub attack: FloatParam,
    #[id = "comp-release"]
    pub release: FloatParam,
    #[id = "comp-mode"]
    pub mode: EnumParam<CompModeChoice>,
    #[id = "comp-tube"]
    pub tube: FloatParam,
    #[id = "comp-mix"]
    pub mix: FloatParam,
    #[id = "comp-output"]
    pub output: FloatParam,
}

impl Default for CompressorParams {
    fn default() -> Self {
        // Parte spento: i progetti esistenti non cambiano suono.
        Self::new(false)
    }
}

impl CompressorParams {
    fn new(enabled: bool) -> Self {
        let db = |name: &str, default: f32, min: f32, max: f32| {
            FloatParam::new(name, default, FloatRange::Linear { min, max })
                .with_unit(" dB")
                .with_step_size(0.1)
                .with_value_to_string(formatters::v2s_f32_rounded(1))
        };
        let ms = |name: &str, default: f32, min: f32, max: f32| {
            FloatParam::new(name, default, FloatRange::Skewed { min, max, factor: FloatRange::skew_factor(-2.0) })
                .with_unit(" ms")
                .with_value_to_string(formatters::v2s_f32_rounded(1))
        };
        Self {
            enabled: BoolParam::new("Compressore", enabled),
            threshold: db("Soglia", -18.0, -50.0, 0.0),
            ratio: FloatParam::new(
                "Rapporto",
                3.0,
                FloatRange::Skewed { min: 1.5, max: 20.0, factor: FloatRange::skew_factor(-1.5) },
            )
            .with_unit(":1")
            .with_step_size(0.1)
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
            attack: ms("Attacco", 10.0, 0.5, 300.0),
            release: ms("Rilascio", 300.0, 50.0, 10000.0),
            mode: EnumParam::new("Modalità", CompModeChoice::Fixed),
            tube: percent_param("Valvola", 0.3, 0.0),
            mix: percent_param("Mix", 1.0, 0.0),
            output: db("Uscita", 0.0, -20.0, 20.0),
        }
    }
}

impl CompressorParams {
    pub fn settings(&self, bypass: bool) -> CompressorSettings {
        CompressorSettings {
            enabled: self.enabled.value() && !bypass,
            threshold_db: self.threshold.value(),
            ratio: self.ratio.value(),
            attack_ms: self.attack.value(),
            release_ms: self.release.value(),
            mode: self.mode.value().to_dsp(),
            tube: self.tube.value(),
            mix: self.mix.value(),
            output_db: self.output.value(),
        }
    }
}

/// Le 12 note della scala personalizzata (attiva/disattiva).
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
        let note = |name: &str| BoolParam::new(format!("Nota {name}"), true);
        Self {
            c: note("Do"),
            c_sharp: note("Do#"),
            d: note("Re"),
            d_sharp: note("Re#"),
            e: note("Mi"),
            f: note("Fa"),
            f_sharp: note("Fa#"),
            g: note("Sol"),
            g_sharp: note("Sol#"),
            a: note("La"),
            a_sharp: note("La#"),
            b: note("Si"),
        }
    }
}

impl CustomNotes {
    /// Il parametro della nota `pitch_class` (0 = Do ... 11 = Si).
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
pub struct SosaTuneParams {
    /// Dimensione della finestra, salvata insieme al progetto. (La chiave e'
    /// cambiata quando la finestra e' diventata piu' alta: i progetti vecchi
    /// non devono riaprirla con la misura precedente.)
    #[persist = "editor-state-2"]
    pub editor_state: Arc<EguiState>,

    #[id = "key"]
    pub key: EnumParam<Key>,

    #[id = "scale"]
    pub scale: EnumParam<ScaleChoice>,

    #[id = "voice"]
    pub voice: EnumParam<VoiceChoice>,

    /// Velocita' di correzione ("Retune Speed"): 0 ms = effetto robotico.
    #[id = "retune"]
    pub retune_speed: FloatParam,

    /// Quantita' di correzione: 0% = nessuna, 100% = completa.
    #[id = "amount"]
    pub amount: FloatParam,

    /// Trasposizione in semitoni.
    #[id = "transpose"]
    pub transpose: IntParam,

    /// Bypass: il DAW lo usa per il suo pulsante di esclusione dell'effetto.
    /// La voce passa senza correzione ma con lo stesso ritardo, cosi' resta
    /// allineata al resto della canzone.
    #[id = "bypass"]
    pub bypass: BoolParam,

    /// Humanize: note lunghe corrette piu' dolcemente.
    #[id = "humanize"]
    pub humanize: FloatParam,

    /// Flex-Tune: corregge solo quando la voce e' vicina alla nota.
    #[id = "flex-tune"]
    pub flex_tune: FloatParam,

    /// Vibrato naturale: da -100% (lo toglie) a +100% (lo aggiunge).
    #[id = "vibrato"]
    pub vibrato: FloatParam,

    /// Spostamento delle formanti (timbro) in semitoni.
    #[id = "formant"]
    pub formant: FloatParam,

    /// Sensibilita' del rilevamento della nota.
    #[id = "tracking"]
    pub tracking: FloatParam,

    #[nested(group = "Scala personalizzata")]
    pub custom: CustomNotes,

    #[nested(group = "Compressore")]
    pub comp: CompressorParams,
}

impl Default for SosaTuneParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(EDITOR_WIDTH, EDITOR_HEIGHT),
            key: EnumParam::new("Tonalità", Key::C),
            scale: EnumParam::new("Scala", ScaleChoice::Chromatic),
            voice: EnumParam::new("Voce", VoiceChoice::AltoTenor),
            retune_speed: FloatParam::new(
                "Velocità",
                20.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 400.0,
                    // Piu' risoluzione sui valori bassi, dove si sente di piu' la differenza.
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_unit(" ms")
            .with_step_size(1.0)
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            amount: FloatParam::new("Quantità", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            transpose: IntParam::new("Trasposizione", 0, IntRange::Linear { min: -12, max: 12 })
                .with_unit(" st"),
            bypass: BoolParam::new("Bypass", false).make_bypass(),
            humanize: percent_param("Humanize", 0.0, 0.0),
            flex_tune: percent_param("Flex-Tune", 0.0, 0.0),
            vibrato: percent_param("Vibrato", 0.0, -1.0),
            formant: FloatParam::new("Formanti", 0.0, FloatRange::Linear { min: -12.0, max: 12.0 })
                .with_unit(" st")
                .with_step_size(0.1)
                .with_value_to_string(formatters::v2s_f32_rounded(1)),
            tracking: percent_param("Sensibilità", 0.3, 0.0),
            custom: CustomNotes::default(),
            comp: CompressorParams::default(),
        }
    }
}

/// Un parametro in percentuale, da `min` (0 o -1) a 1.
fn percent_param(name: &str, default: f32, min: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min, max: 1.0 })
        .with_unit("%")
        .with_value_to_string(formatters::v2s_f32_percentage(0))
        .with_string_to_value(formatters::s2v_f32_percentage())
}

impl SosaTuneParams {
    /// Valori per l'anteprima (vedi `preview.rs`): come quelli iniziali, ma
    /// con il compressore acceso per mostrarlo al lavoro.
    pub(crate) fn preview() -> Self {
        Self { comp: CompressorParams::new(true), ..Self::default() }
    }

    /// Le note ammesse in questo momento (dipendono da tonalita' e scala).
    pub fn active_notes(&self) -> [bool; 12] {
        match self.scale.value().kind() {
            Some(kind) => scale_notes(self.key.value().semitone(), kind),
            None => self.custom.mask(),
        }
    }
}
