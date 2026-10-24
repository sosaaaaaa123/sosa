//! Dove sta ogni cosa, in pixel dell'immagine di riferimento (1448 x 1086).
//! Le stesse misure sono in `tools/make_skin.py` (le parti tolte dalla
//! pelle vengono ridisegnate qui, esattamente nello stesso punto).

/// Larghezza e altezza dell'immagine di riferimento.
pub const REF_W: f32 = 1448.0;
pub const REF_H: f32 = 1086.0;

pub type R = (f32, f32, f32, f32);

// --- Intestazione -------------------------------------------------------------
pub const VOICE_FIELD: R = (393.0, 22.0, 592.0, 92.0);
pub const VOICE_VALUE: (f32, f32) = (449.0, 67.0);
pub const KEY_FIELD: R = (600.0, 22.0, 792.0, 92.0);
pub const KEY_VALUE: (f32, f32) = (656.0, 67.0);
pub const PRESET_BOX: R = (806.0, 22.0, 1116.0, 92.0);
pub const PRESET_PREV: R = (812.0, 30.0, 862.0, 84.0);
pub const PRESET_NEXT: R = (1062.0, 30.0, 1112.0, 84.0);
/// Riga 1: nome del preset e accordatura (centro del gruppo), riga 2: carattere.
pub const PRESET_LINE1: (f32, f32) = (972.0, 42.0);
pub const PRESET_LINE2: (f32, f32) = (958.5, 64.0);
pub const UNDO: (f32, f32) = (1187.0, 55.0);
pub const REDO: (f32, f32) = (1240.0, 55.0);
pub const MENU: (f32, f32) = (1337.0, 55.0);
pub const SETTINGS: (f32, f32) = (1398.0, 55.0);

// --- Sfera della tonalita' ---------------------------------------------------------
pub const ORB: (f32, f32, f32) = (705.5, 304.5, 191.0);
pub const ORB_LETTER: (f32, f32) = (708.0, 297.5);
/// Altezza delle maiuscole della lettera grande.
pub const ORB_LETTER_CAP: f32 = 93.0;
pub const ORB_SCALE: (f32, f32) = (706.0, 366.5);

/// Sfere piccole: (centro x, centro y, raggio, raggio del pallino).
pub const RETUNE_ORB: (f32, f32, f32, f32) = (1000.5, 229.5, 58.0, 53.2);
pub const HUMANIZE_ORB: (f32, f32, f32, f32) = (1143.5, 358.5, 62.0, 56.6);

// --- Grafico dell'intonazione --------------------------------------------------------
pub const GRAPH: R = (128.0, 483.0, 1414.0, 597.0);
pub const GRAPH_ZERO_Y: f32 = 540.0;
/// Pixel per centesimo (+50 a y = 501, -50 a y = 578).
pub const GRAPH_PX_PER_CENT: f32 = 0.77;
pub const NOTE_LABEL: (f32, f32) = (62.5, 543.0);

// --- Barra dei comandi ------------------------------------------------------------------
pub const INPUT_METER: R = (122.0, 640.0, 332.0, 662.0);
pub const OUTPUT_METER: R = (1252.0, 640.0, 1402.0, 662.0);
pub const INPUT_SEGMENTS: usize = 31;
pub const OUTPUT_SEGMENTS: usize = 22;
pub const BYPASS: R = (531.0, 630.0, 664.0, 676.0);
pub const BYPASS_ICON: (f32, f32) = (556.5, 653.0);
pub const BYPASS_TEXT: (f32, f32) = (593.0, 653.0);
pub const MONITOR: R = (688.0, 629.0, 830.0, 676.0);
pub const MONITOR_ICON: (f32, f32) = (724.0, 653.0);
pub const MONITOR_TEXT: (f32, f32) = (761.0, 653.0);
pub const STABILIZER: R = (857.0, 631.0, 1060.0, 676.0);

// --- Manopole -----------------------------------------------------------------------------
/// (centro x, centro y, raggio dell'arco, raggio del pallino, colore dell'arco RGB).
pub struct KnobGeo {
    pub cx: f32,
    pub cy: f32,
    pub arc_r: f32,
    pub dot_r: f32,
    pub color: (u8, u8, u8),
}

pub const KNOBS: [KnobGeo; 4] = [
    KnobGeo { cx: 192.5, cy: 872.5, arc_r: 103.0, dot_r: 82.0, color: (0xd6, 0xe9, 0xff) },
    KnobGeo { cx: 552.5, cy: 872.5, arc_r: 108.5, dot_r: 82.0, color: (0x7c, 0xcc, 0xff) },
    KnobGeo { cx: 907.5, cy: 874.5, arc_r: 104.0, dot_r: 82.0, color: (0x6c, 0xb4, 0xff) },
    KnobGeo { cx: 1265.5, cy: 873.5, arc_r: 102.5, dot_r: 82.0, color: (0x5c, 0xf0, 0xe6) },
];
/// Centro verticale dei numeri delle manopole.
pub const KNOB_NUMBER_Y: f32 = 876.5;
/// Angolo (gradi dall'alto, in senso orario) per 0 e per 100.
pub const KNOB_ANGLE_MIN: f32 = -150.0;
pub const KNOB_ANGLE_MAX: f32 = 150.0;

pub fn center(r: R) -> (f32, f32) {
    (0.5 * (r.0 + r.2), 0.5 * (r.1 + r.3))
}
