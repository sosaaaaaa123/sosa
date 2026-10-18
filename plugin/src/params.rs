//! I parametri del plugin: tutto cio' che l'utente puo' regolare e che il
//! DAW salva nel progetto e puo' automatizzare.

use sosatune_dsp::scale::{scale_notes, ScaleKind, VoiceType};
use sosatune_dsp::CorrectionSettings;
use nih_plug::prelude::*;
use crate::gui::EguiState;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::Arc;

/// Dimensione iniziale della finestra del plugin (pixel, al 100%).
/// La grafica e' disegnata sulle misure dell'immagine di riferimento
/// (1448 pixel di larghezza); sotto il pannello resta lo stesso bordo dei
/// lati (prima era il doppio). La finestra si puo' ingrandire o
/// rimpicciolire mantenendo queste proporzioni.
pub const EDITOR_WIDTH: u32 = 960;
pub const EDITOR_HEIGHT: u32 = 694;

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

/// Il tipo di voce: decide la nota piu' grave riconosciuta.
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

/// Modalita' di elaborazione: latenza zero (per registrare) oppure qualita' massima (per il mix).
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityChoice {
    #[id = "low-latency"]
    #[name = "Low Latency"]
    LowLatency,
    #[id = "hq"]
    #[name = "HQ"]
    Hq,
}

/// Stile della correzione.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleChoice {
    /// Note che partono gia' intonate, nessun rimbalzo tra due note.
    #[id = "modern"]
    #[name = "Modern"]
    Modern,
    /// Ogni nota scivola sulla nota giusta, il bersaglio cambia subito.
    #[id = "classic"]
    #[name = "Classic"]
    Classic,
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

/// Le note escluse a mano con Edit Notes, sopra la scala scelta: la voce non
/// ci va, ma tonalita' e scala restano quelle (G Minor resta G Minor, senza
/// diventare "Custom"). Si azzerano quando si sceglie un'altra tonalita' o
/// un'altra scala.
#[derive(Params)]
pub struct ExcludedNotes {
    #[id = "skip-c"]
    pub c: BoolParam,
    #[id = "skip-cs"]
    pub c_sharp: BoolParam,
    #[id = "skip-d"]
    pub d: BoolParam,
    #[id = "skip-ds"]
    pub d_sharp: BoolParam,
    #[id = "skip-e"]
    pub e: BoolParam,
    #[id = "skip-f"]
    pub f: BoolParam,
    #[id = "skip-fs"]
    pub f_sharp: BoolParam,
    #[id = "skip-g"]
    pub g: BoolParam,
    #[id = "skip-gs"]
    pub g_sharp: BoolParam,
    #[id = "skip-a"]
    pub a: BoolParam,
    #[id = "skip-as"]
    pub a_sharp: BoolParam,
    #[id = "skip-b"]
    pub b: BoolParam,
}

impl Default for ExcludedNotes {
    fn default() -> Self {
        Self::with_defaults(&HashMap::new())
    }
}

impl ExcludedNotes {
    fn with_defaults(saved: &HashMap<String, f32>) -> Self {
        let note = |name: &str, id: &str| {
            BoolParam::new(format!("Escludi {name}"), saved.get(&format!("skip-{id}")).is_some_and(|&v| v >= 0.5))
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
    /// Dimensione della finestra, salvata insieme al progetto. (La chiave
    /// cambia quando cambiano le proporzioni della finestra: i progetti vecchi
    /// non devono riaprirla con la misura precedente.)
    #[persist = "editor-state-5"]
    pub editor_state: Arc<EguiState>,

    /// Confronto A/B: quale delle due impostazioni e' attiva (0 = A, 1 = B)...
    #[persist = "ab-slot"]
    pub ab_slot: Arc<AtomicUsize>,
    /// ...e i valori (normalizzati) dell'altra, per id del parametro.
    #[persist = "ab-other"]
    pub ab_other: Arc<RwLock<Vec<(String, f32)>>>,

    /// L'ultimo preset caricato (indice in `presets::FACTORY`).
    #[persist = "preset"]
    pub preset: Arc<AtomicUsize>,

    /// "AutoKey Listen": la tonalita' viene riconosciuta dalla voce e
    /// impostata da sola.
    #[persist = "autokey"]
    pub autokey: Arc<AtomicBool>,

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

    /// Accordatura: frequenza del La4 ("Detune").
    #[id = "detune"]
    pub detune: FloatParam,

    /// Low Latency / HQ.
    #[id = "quality"]
    pub quality: EnumParam<QualityChoice>,

    /// Modern / Classic.
    #[id = "style"]
    pub style: EnumParam<StyleChoice>,

    #[nested(group = "Scala personalizzata")]
    pub custom: CustomNotes,

    #[nested(group = "Note escluse")]
    pub excluded: ExcludedNotes,
}

impl Default for SosaTuneParams {
    fn default() -> Self {
        Self::with_defaults(&crate::defaults::load())
    }
}

impl SosaTuneParams {
    /// I parametri, con i valori iniziali presi da `saved` ("Save As
    /// Default": id del parametro -> valore) dove ci sono.
    pub(crate) fn with_defaults(saved: &HashMap<String, f32>) -> Self {
        let d = |id: &str, fallback: f32| saved.get(id).copied().filter(|v| v.is_finite()).unwrap_or(fallback);
        fn pick<T: Enum>(saved: &HashMap<String, f32>, id: &str, fallback: T) -> T {
            match saved.get(id) {
                Some(&v) if v.is_finite() && v >= 0.0 && (v as usize) < T::variants().len() => T::from_index(v as usize),
                _ => fallback,
            }
        }
        Self {
            editor_state: EguiState::from_size(EDITOR_WIDTH, EDITOR_HEIGHT),
            key: EnumParam::new("Tonalità", pick(saved, "key", Key::C)),
            scale: EnumParam::new("Scala", pick(saved, "scale", ScaleChoice::Chromatic)),
            voice: EnumParam::new("Voce", pick(saved, "voice", VoiceChoice::AltoTenor)),
            retune_speed: retune_param(d("retune", 20.0).clamp(0.0, 400.0)),
            amount: FloatParam::new("Quantità", d("amount", 1.0).clamp(0.0, 1.0), FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            transpose: IntParam::new(
                "Trasposizione",
                d("transpose", 0.0).round().clamp(-12.0, 12.0) as i32,
                IntRange::Linear { min: -12, max: 12 },
            )
            .with_unit(" st"),
            bypass: BoolParam::new("Bypass", false).make_bypass(),
            humanize: percent_param("Humanize", d("humanize", 0.0).clamp(0.0, 1.0), 0.0),
            flex_tune: percent_param("Flex-Tune", d("flex-tune", 0.0).clamp(0.0, 1.0), 0.0),
            vibrato: percent_param("Vibrato", d("vibrato", 0.0).clamp(-1.0, 1.0), -1.0),
            formant: FloatParam::new("Formanti", d("formant", 0.0).clamp(-12.0, 12.0), FloatRange::Linear { min: -12.0, max: 12.0 })
                .with_unit(" st")
                .with_step_size(0.1)
                .with_value_to_string(formatters::v2s_f32_rounded(1)),
            tracking: percent_param("Sensibilità", d("tracking", 0.3).clamp(0.0, 1.0), 0.0),
            detune: FloatParam::new("Detune", d("detune", 440.0).clamp(410.0, 470.0), FloatRange::Linear { min: 410.0, max: 470.0 })
                .with_unit(" Hz")
                .with_step_size(0.1)
                .with_value_to_string(formatters::v2s_f32_rounded(1)),
            // HQ (phase vocoder) e' il suono migliore: e' la scelta iniziale.
            quality: EnumParam::new("Mode", pick(saved, "quality", QualityChoice::Hq)),
            style: EnumParam::new("Style", pick(saved, "style", StyleChoice::Modern)),
            custom: CustomNotes::with_defaults(saved),
            excluded: ExcludedNotes::with_defaults(saved),
            preset: Arc::new(AtomicUsize::new(0)),
            autokey: Arc::new(AtomicBool::new(false)),
            ab_slot: Arc::new(AtomicUsize::new(0)),
            ab_other: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

/// Retune Speed: 0 ms = effetto robotico, fino a 400 ms.
fn retune_param(default: f32) -> FloatParam {
    FloatParam::new(
        "Velocità",
        default,
        FloatRange::Skewed {
            min: 0.0,
            max: 400.0,
            // Piu' risoluzione sui valori bassi, dove si sente di piu' la differenza.
            factor: FloatRange::skew_factor(-1.5),
        },
    )
    .with_unit(" ms")
    .with_step_size(1.0)
    .with_value_to_string(formatters::v2s_f32_rounded(0))
}

/// Un parametro in percentuale, da `min` (0 o -1) a 1.
fn percent_param(name: &str, default: f32, min: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min, max: 1.0 })
        .with_unit("%")
        .with_value_to_string(formatters::v2s_f32_percentage(0))
        .with_string_to_value(formatters::s2v_f32_percentage())
}

impl SosaTuneParams {
    /// Valori per l'anteprima (vedi `preview.rs`): quelli dell'immagine di
    /// riferimento (La maggiore, Retune Speed 15, Humanize 24, Flex Tune 28).
    pub(crate) fn preview() -> Self {
        let p = Self::default();
        Self {
            key: EnumParam::new("Tonalità", Key::A),
            scale: EnumParam::new("Scala", ScaleChoice::Major),
            retune_speed: retune_param(15.0),
            humanize: percent_param("Humanize", 0.24, 0.0),
            flex_tune: percent_param("Flex-Tune", 0.28, 0.0),
            autokey: Arc::new(AtomicBool::new(true)),
            quality: EnumParam::new("Mode", QualityChoice::LowLatency),
            ..p
        }
    }

    /// Le impostazioni per il motore audio.
    pub fn correction_settings(&self) -> CorrectionSettings {
        CorrectionSettings {
            notes: self.active_notes(),
            retune_ms: self.retune_speed.value(),
            amount: self.amount.value(),
            transpose: self.transpose.value() as f32,
            humanize: self.humanize.value(),
            flex_tune: self.flex_tune.value(),
            vibrato: self.vibrato.value(),
            formant: self.formant.value(),
            tracking: self.tracking.value(),
            reference_hz: self.detune.value(),
            classic: self.style.value() == StyleChoice::Classic,
        }
    }

    /// Le note della tonalita' e della scala scelte (prima delle esclusioni).
    pub fn scale_notes(&self) -> [bool; 12] {
        match self.scale.value().kind() {
            Some(kind) => scale_notes(self.key.value().semitone(), kind),
            None => self.custom.mask(),
        }
    }

    /// Le note ammesse in questo momento: quelle della scala, tolte quelle
    /// escluse con Edit Notes. Se fossero escluse tutte, valgono quelle
    /// della scala (una scala vuota non correggerebbe niente).
    pub fn active_notes(&self) -> [bool; 12] {
        let base = self.scale_notes();
        let excluded = self.excluded.mask();
        let notes: [bool; 12] = std::array::from_fn(|pc| base[pc] && !excluded[pc]);
        if notes.iter().any(|&on| on) {
            notes
        } else {
            base
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ogni comando dell'interfaccia (cioe' ogni parametro) deve arrivare al
    /// motore audio: cambiandolo, cambiano le impostazioni della correzione.
    #[test]
    fn every_control_reaches_the_engine() {
        let base = SosaTuneParams::default().correction_settings();
        let with = |p: SosaTuneParams| p.correction_settings();

        let s = with(SosaTuneParams { retune_speed: retune_param(15.0), ..Default::default() });
        assert_eq!(s.retune_ms, 15.0);
        let s = with(SosaTuneParams { humanize: percent_param("Humanize", 0.24, 0.0), ..Default::default() });
        assert!((s.humanize - 0.24).abs() < 1e-6);
        let s = with(SosaTuneParams { flex_tune: percent_param("Flex-Tune", 0.28, 0.0), ..Default::default() });
        assert!((s.flex_tune - 0.28).abs() < 1e-6);
        let s = with(SosaTuneParams {
            detune: FloatParam::new("Detune", 432.0, FloatRange::Linear { min: 410.0, max: 470.0 }),
            ..Default::default()
        });
        assert_eq!(s.reference_hz, 432.0);
        let s = with(SosaTuneParams { style: EnumParam::new("Style", StyleChoice::Classic), ..Default::default() });
        assert!(s.classic && !base.classic);
        let s = with(SosaTuneParams { amount: FloatParam::new("Quantità", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 }), ..Default::default() });
        assert_eq!(s.amount, 0.5);
        let s = with(SosaTuneParams { transpose: IntParam::new("Trasposizione", 3, IntRange::Linear { min: -12, max: 12 }), ..Default::default() });
        assert_eq!(s.transpose, 3.0);
        let s = with(SosaTuneParams { vibrato: percent_param("Vibrato", -0.5, -1.0), ..Default::default() });
        assert_eq!(s.vibrato, -0.5);
        let s = with(SosaTuneParams {
            formant: FloatParam::new("Formanti", 2.0, FloatRange::Linear { min: -12.0, max: 12.0 }),
            ..Default::default()
        });
        assert_eq!(s.formant, 2.0);
        let s = with(SosaTuneParams { tracking: percent_param("Sensibilità", 0.8, 0.0), ..Default::default() });
        assert!((s.tracking - 0.8).abs() < 1e-6);

        // Key & Scale: Sol minore = Sol La Sib Do Re Mib Fa.
        let s = with(SosaTuneParams {
            key: EnumParam::new("Tonalità", Key::G),
            scale: EnumParam::new("Scala", ScaleChoice::NaturalMinor),
            ..Default::default()
        });
        let expected: [bool; 12] = std::array::from_fn(|pc| [7, 9, 10, 0, 2, 3, 5].contains(&pc));
        assert_eq!(s.notes, expected);
        // Scala personalizzata: le note di Edit Notes.
        let mut custom = CustomNotes::default();
        custom.c_sharp = BoolParam::new("Nota Do#", false);
        let s = with(SosaTuneParams { scale: EnumParam::new("Scala", ScaleChoice::Custom), custom, ..Default::default() });
        assert!(!s.notes[1] && s.notes[0]);

        // "Save As Default": i valori salvati diventano quelli iniziali.
        let saved: HashMap<String, f32> = [
            ("retune", 35.0),
            ("key", 7.0),
            ("scale", 2.0),
            ("vibrato", -0.25),
            ("note-cs", 0.0),
            ("quality", 0.0),
            ("formant", -3.5),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
        let p = SosaTuneParams::with_defaults(&saved);
        assert_eq!(p.retune_speed.value(), 35.0);
        assert_eq!(p.retune_speed.default_plain_value(), 35.0, "il doppio clic torna al valore salvato");
        assert_eq!(p.key.value(), Key::G);
        assert_eq!(p.scale.value(), ScaleChoice::from_index(2));
        assert_eq!(p.vibrato.value(), -0.25);
        assert!(!p.custom.c_sharp.value() && p.custom.c.value());
        assert_eq!(p.quality.value(), QualityChoice::LowLatency);
        assert_eq!(p.formant.value(), -3.5);
        // Valori fuori misura o di un'altra versione: ignorati senza danni.
        let junk: HashMap<String, f32> =
            [("key", 99.0), ("retune", f32::NAN), ("sconosciuto", 1.0)].into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
        let p = SosaTuneParams::with_defaults(&junk);
        assert_eq!(p.key.value(), Key::C);
        assert_eq!(p.retune_speed.value(), 20.0);

        // Vocal Range e Mode passano al motore nel plugin (set_voice / set_quality).
        assert_eq!(VoiceChoice::Low.to_dsp(), VoiceType::LowMale);
        assert_eq!(VoiceChoice::Soprano.to_dsp(), VoiceType::Soprano);
    }
}
