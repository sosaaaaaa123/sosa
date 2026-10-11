//! Note, scale e conversioni tra frequenza e numero di nota.
//!
//! Usiamo i numeri di nota MIDI: 60 = Do centrale (C4), 69 = La 440 Hz (A4).
//! Ogni +1 e' un semitono. I numeri possono avere la virgola: 69.3 significa
//! "un La crescente di 30 centesimi".
//!
//! Una scala e' rappresentata come 12 interruttori, uno per ogni nota
//! dell'ottava (Do, Do#, Re, ... Si): `true` = la nota e' ammessa.

/// Nomi delle note in notazione internazionale (indice 0 = C).
pub const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Nomi delle note in notazione italiana (indice 0 = Do).
pub const NOTE_NAMES_IT: [&str; 12] = [
    "Do", "Do#", "Re", "Re#", "Mi", "Fa", "Fa#", "Sol", "Sol#", "La", "La#", "Si",
];

/// Converte una frequenza in Hz in numero di nota MIDI (con decimali).
#[inline]
pub fn hz_to_note(hz: f32) -> f32 {
    69.0 + 12.0 * (hz / 440.0).log2()
}

/// Converte un numero di nota MIDI in frequenza (Hz).
#[inline]
pub fn note_to_hz(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

/// La "classe" della nota: 0 = Do, 1 = Do#, ... 11 = Si (indipendente dall'ottava).
#[inline]
pub fn pitch_class(note: i32) -> usize {
    note.rem_euclid(12) as usize
}

/// L'ottava in notazione scientifica (60 → 4, cioe' C4).
#[inline]
pub fn octave(note: i32) -> i32 {
    note.div_euclid(12) - 1
}

/// I tipi di scala predefiniti.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleKind {
    Chromatic,
    Major,
    NaturalMinor,
    HarmonicMinor,
    MajorPentatonic,
    MinorPentatonic,
    Blues,
}

impl ScaleKind {
    /// Gli intervalli della scala, in semitoni a partire dalla tonica.
    pub fn intervals(self) -> &'static [u8] {
        match self {
            ScaleKind::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            ScaleKind::Major => &[0, 2, 4, 5, 7, 9, 11],
            ScaleKind::NaturalMinor => &[0, 2, 3, 5, 7, 8, 10],
            ScaleKind::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
            ScaleKind::MajorPentatonic => &[0, 2, 4, 7, 9],
            ScaleKind::MinorPentatonic => &[0, 3, 5, 7, 10],
            ScaleKind::Blues => &[0, 3, 5, 6, 7, 10],
        }
    }
}

/// Le note ammesse per una tonalita' (`key`: 0 = Do ... 11 = Si) e una scala.
pub fn scale_notes(key: u8, kind: ScaleKind) -> [bool; 12] {
    let mut notes = [false; 12];
    for &interval in kind.intervals() {
        notes[(key as usize + interval as usize) % 12] = true;
    }
    notes
}

/// La nota ammessa piu' vicina a `note`, oppure `None` se nessuna nota e' attiva.
pub fn nearest_note(note: f32, notes: &[bool; 12]) -> Option<i32> {
    if !notes.iter().any(|&on| on) {
        return None;
    }
    let base = note.round() as i32;
    let mut best: Option<(i32, f32)> = None;
    // Basta cercare entro mezza ottava sopra e sotto.
    for offset in -6..=6 {
        let candidate = base + offset;
        if notes[pitch_class(candidate)] {
            let distance = (note - candidate as f32).abs();
            if best.is_none_or(|(_, d)| distance < d) {
                best = Some((candidate, distance));
            }
        }
    }
    best.map(|(n, _)| n)
}

/// Sceglie la nota di arrivo con un po' di "isteresi": quando il cantante e'
/// proprio a meta' tra due note, senza isteresi la correzione salterebbe
/// avanti e indietro tra le due, producendo un suono tremolante.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoteSnapper {
    current: Option<i32>,
}

impl NoteSnapper {
    /// Quanto (in semitoni) la nuova nota deve essere "piu' vicina" della
    /// precedente prima di cambiare bersaglio.
    pub const HYSTERESIS: f32 = 0.15;

    /// Isteresi tra due note precise. Tra note a un semitono basta poco; tra
    /// note distanti un tono o piu' (per esempio Do e Re in Sol minore, con il
    /// Do# fuori scala in mezzo) una voce che sta "in mezzo" e' ambigua: si
    /// cambia nota solo quando la voce supera chiaramente la meta' strada (di
    /// almeno 1/8 di semitono), cosi' la correzione non salta avanti e indietro.
    pub fn hysteresis(current: i32, nearest: i32) -> f32 {
        if (current - nearest).abs() >= 2 {
            0.25
        } else {
            Self::HYSTERESIS
        }
    }

    pub fn reset(&mut self) {
        self.current = None;
    }

    /// La nota attuale (senza aggiornarla).
    pub fn current(&self) -> Option<i32> {
        self.current
    }

    /// Quale nota sceglierebbe [`update`](Self::update) per `note`, senza
    /// cambiare nulla. `None` se la scala e' vuota.
    pub fn propose(&self, note: f32, notes: &[bool; 12]) -> Option<i32> {
        let nearest = nearest_note(note, notes)?;
        let keep_current = match self.current {
            Some(current) if current != nearest && notes[pitch_class(current)] => {
                (note - current as f32).abs() < (note - nearest as f32).abs() + Self::hysteresis(current, nearest)
            }
            _ => false,
        };
        Some(if keep_current { self.current.unwrap_or(nearest) } else { nearest })
    }

    /// Imposta la nota (dopo aver deciso che il cambio e' vero).
    pub fn set(&mut self, note: Option<i32>) {
        self.current = note;
    }

    pub fn update(&mut self, note: f32, notes: &[bool; 12]) -> Option<i32> {
        let nearest = match nearest_note(note, notes) {
            Some(n) => n,
            None => {
                self.current = None;
                return None;
            }
        };
        let keep_current = match self.current {
            Some(current) if current != nearest && notes[pitch_class(current)] => {
                (note - current as f32).abs() < (note - nearest as f32).abs() + Self::hysteresis(current, nearest)
            }
            _ => false,
        };
        if !keep_current {
            self.current = Some(nearest);
        }
        self.current
    }
}

/// Il tipo di voce: decide la nota piu' grave che il rilevatore deve
/// riconoscere. Una voce acuta permette una latenza piu' bassa, perche' i
/// periodi dell'onda sono piu' corti.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceType {
    Soprano,
    AltoTenor,
    LowMale,
}

impl VoiceType {
    /// Frequenza piu' bassa riconosciuta (Hz).
    pub fn lowest_hz(self) -> f32 {
        match self {
            VoiceType::Soprano => 150.0,
            VoiceType::AltoTenor => 90.0,
            VoiceType::LowMale => 60.0,
        }
    }

    /// La frequenza piu' bassa tra tutti i tipi di voce: serve per
    /// dimensionare i buffer una volta sola.
    pub const LOWEST_SUPPORTED_HZ: f32 = 60.0;
}

/// Frequenza piu' alta riconosciuta (Hz), uguale per tutti i tipi di voce.
pub const HIGHEST_HZ: f32 = 1300.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions() {
        assert!((hz_to_note(440.0) - 69.0).abs() < 1e-5);
        assert!((note_to_hz(60.0) - 261.6256).abs() < 1e-2);
        assert_eq!(pitch_class(-1), 11);
        assert_eq!(octave(60), 4);
    }

    #[test]
    fn c_major_snapping() {
        let c_major = scale_notes(0, ScaleKind::Major);
        assert_eq!(nearest_note(69.3, &c_major), Some(69)); // La crescente → La
        assert_eq!(nearest_note(70.2, &c_major), Some(71)); // La# non c'e' → Si piu' vicino
        assert_eq!(nearest_note(65.6, &c_major), Some(65)); // Fa# non c'e' → Fa
        assert_eq!(nearest_note(60.0, &[false; 12]), None);
    }

    #[test]
    fn a_minor_equals_c_major_notes() {
        assert_eq!(scale_notes(9, ScaleKind::NaturalMinor), scale_notes(0, ScaleKind::Major));
    }

    #[test]
    fn hysteresis_avoids_flicker() {
        let chromatic = scale_notes(0, ScaleKind::Chromatic);
        let mut snapper = NoteSnapper::default();
        assert_eq!(snapper.update(69.45, &chromatic), Some(69));
        // Appena oltre la meta': resta sul La grazie all'isteresi...
        assert_eq!(snapper.update(69.55, &chromatic), Some(69));
        // ...ma se il cantante va decisamente verso il La#, cambia.
        assert_eq!(snapper.update(69.75, &chromatic), Some(70));
    }
}
