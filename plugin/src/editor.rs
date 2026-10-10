//! L'interfaccia grafica, disegnata con egui (una libreria "immediate mode":
//! a ogni fotogramma si ridisegna tutto da capo, il che rende il codice
//! semplice da leggere: cio' che vedi e' esattamente cio' che e' scritto qui).
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────────┐
//! │ INTONA                                               latenza 30.7 ms │
//! │ ┌───────┐  Stile [Naturale] [Pop moderno] [Robotico] [Leggero]       │
//! │ │  La4  │  Tonalità [C ▾]  Velocità ──●── 20 ms  Humanize  ●──── 0%  │
//! │ │+12 ct │  Scala [Magg.▾]  Quantità ────● 100%   Flex-Tune ●──── 0%  │
//! │ └───────┘  Voce [Alto ▾]   Trasponi ──●── 0 st   Vibrato   ──●── 0%  │
//! │                            Formanti ──●── 0 st   Sensibilità ●── 30% │
//! │ [ tastiera: le note attive della scala, cliccabili ]                 │
//! │ [ grafico: voce originale (grigio) e corretta (arancione) ]          │
//! └──────────────────────────────────────────────────────────────────────┘
//! ```

use crate::params::{IntonaParams, ScaleChoice};
use crate::shared::{PitchDisplay, PitchPoint};
use intona_dsp::scale::{octave, pitch_class, NOTE_NAMES_IT};
use nih_plug::prelude::*;
use nih_plug_egui::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke,
    StrokeKind, Ui, Vec2,
};
use nih_plug_egui::{create_egui_editor, widgets::ParamSlider};
use std::sync::Arc;

// --- Colori -------------------------------------------------------------------
const BACKGROUND: Color32 = Color32::from_rgb(0x14, 0x16, 0x1b);
const PANEL: Color32 = Color32::from_rgb(0x1d, 0x20, 0x27);
const PANEL_BORDER: Color32 = Color32::from_rgb(0x2b, 0x2f, 0x38);
const TEXT: Color32 = Color32::from_rgb(0xe6, 0xe7, 0xeb);
const TEXT_DIM: Color32 = Color32::from_rgb(0x8b, 0x90, 0x9b);
const ACCENT: Color32 = Color32::from_rgb(0xff, 0x8a, 0x3d);
const INPUT_LINE: Color32 = Color32::from_rgb(0x9a, 0xa0, 0xab);
const IN_TUNE: Color32 = Color32::from_rgb(0x5f, 0xd0, 0x8a);

/// Quanti secondi mostra il grafico.
const GRAPH_SECONDS: f32 = 4.0;

/// Stato che appartiene solo alla grafica (non viene salvato).
struct GuiState {
    /// Copia degli ultimi punti del grafico (riusata a ogni fotogramma).
    points: Vec<PitchPoint>,
    /// Nota al centro del grafico (segue la voce con dolcezza).
    graph_center: f32,
}

pub fn create(params: Arc<IntonaParams>, display: Arc<PitchDisplay>) -> Option<Box<dyn Editor>> {
    let state = GuiState {
        points: Vec::with_capacity(crate::shared::HISTORY_LEN),
        graph_center: 60.0,
    };
    create_egui_editor(
        params.editor_state.clone(),
        state,
        |ctx, _state| {
            // Sempre tema scuro, anche se il Mac e' in modalita' chiara.
            ctx.set_theme(egui::ThemePreference::Dark);
            ctx.set_visuals(egui::Visuals::dark());
            ctx.style_mut(|style| {
                style.spacing.item_spacing = Vec2::new(8.0, 8.0);
                style.spacing.interact_size.y = 24.0;
            });
        },
        move |ctx, setter, state| {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(16.0))
                .show(ctx, |ui| {
                    header(ui, &display);
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        note_readout(ui, &display);
                        ui.add_space(12.0);
                        controls(ui, &params, setter);
                    });
                    ui.add_space(10.0);
                    keyboard(ui, &params, setter, &display);
                    ui.add_space(10.0);
                    pitch_graph(ui, &params, &display, state);
                });
        },
    )
}

// --- Intestazione ---------------------------------------------------------------

fn header(ui: &mut Ui, display: &PitchDisplay) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("INTONA").size(22.0).strong().color(ACCENT));
        ui.label(RichText::new("correzione dell'intonazione").size(14.0).color(TEXT_DIM));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(format!("latenza {:.1} ms", display.latency_ms()))
                    .size(12.0)
                    .color(TEXT_DIM),
            )
            .on_hover_text(
                "Il ritardo introdotto dal plugin. Il DAW lo compensa da solo; \
                 una voce piu' acuta (impostazione Voce) permette un ritardo minore.",
            );
        });
    });
}

// --- Nota cantata -------------------------------------------------------------

fn note_label(note: i32) -> String {
    format!("{}{}", NOTE_NAMES_IT[pitch_class(note)], octave(note))
}

fn note_readout(ui: &mut Ui, display: &PitchDisplay) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(150.0, 112.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect(rect, CornerRadius::same(10), PANEL, Stroke::new(1.0_f32, PANEL_BORDER), StrokeKind::Inside);

    let point = display.latest().filter(|p| !p.input.is_nan());
    match point {
        Some(p) => {
            let nearest = p.input.round() as i32;
            let cents = ((p.input - nearest as f32) * 100.0).round() as i32;
            painter.text(
                rect.center() - Vec2::new(0.0, 14.0),
                Align2::CENTER_CENTER,
                note_label(nearest),
                FontId::proportional(38.0),
                TEXT,
            );
            let color = if cents.abs() <= 10 { IN_TUNE } else { ACCENT };
            painter.text(
                rect.center() + Vec2::new(0.0, 22.0),
                Align2::CENTER_CENTER,
                format!("{cents:+} cent"),
                FontId::proportional(15.0),
                color,
            );
            if !p.target.is_nan() {
                painter.text(
                    rect.center() + Vec2::new(0.0, 42.0),
                    Align2::CENTER_CENTER,
                    format!("→ {}", note_label(p.target.round() as i32)),
                    FontId::proportional(12.0),
                    TEXT_DIM,
                );
            }
        }
        None => {
            painter.text(rect.center(), Align2::CENTER_CENTER, "—", FontId::proportional(38.0), TEXT_DIM);
        }
    }
}

// --- Controlli ----------------------------------------------------------------

/// Un menu a tendina per un parametro a scelta multipla.
fn enum_combo<T: Enum + PartialEq + 'static>(
    ui: &mut Ui,
    id: &str,
    param: &EnumParam<T>,
    setter: &ParamSetter,
) {
    let current = param.value().to_index();
    let names = T::variants();
    egui::ComboBox::from_id_salt(id)
        .width(150.0)
        .selected_text(names[current])
        .show_ui(ui, |ui| {
            for (index, name) in names.iter().enumerate() {
                if ui.selectable_label(index == current, *name).clicked() && index != current {
                    setter.begin_set_parameter(param);
                    setter.set_parameter(param, T::from_index(index));
                    setter.end_set_parameter(param);
                }
            }
        });
}

/// Imposta un parametro dall'interfaccia (avvisando il DAW, cosi' finisce
/// nell'automazione e nell'annulla/ripeti).
fn set_float(setter: &ParamSetter, param: &FloatParam, value: f32) {
    setter.begin_set_parameter(param);
    setter.set_parameter(param, value);
    setter.end_set_parameter(param);
}

/// Stili pronti: impostano insieme i controlli principali.
fn style_presets(ui: &mut Ui, params: &IntonaParams, setter: &ParamSetter) {
    // (nome, spiegazione, velocita' ms, humanize, flex-tune, vibrato, quantita')
    const PRESETS: [(&str, &str, f32, f32, f32, f32, f32); 4] = [
        ("Naturale", "Correzione trasparente: si sente intonato ma non corretto.", 80.0, 0.35, 0.0, 0.0, 1.0),
        ("Pop moderno", "Intonazione precisa e brillante, come nelle produzioni pop.", 20.0, 0.25, 0.0, 0.0, 1.0),
        ("Robotico", "L'effetto Auto-Tune estremo, la voce salta da nota a nota.", 0.0, 0.0, 0.0, 0.0, 1.0),
        ("Leggero", "Sistema solo le stonature, lascia l'espressione.", 150.0, 0.6, 0.6, 0.0, 0.8),
    ];
    ui.label(RichText::new("Stile").color(TEXT_DIM));
    for (name, help, retune, humanize, flex, vibrato, amount) in PRESETS {
        if ui.button(name).on_hover_text(help).clicked() {
            set_float(setter, &params.retune_speed, retune);
            set_float(setter, &params.humanize, humanize);
            set_float(setter, &params.flex_tune, flex);
            set_float(setter, &params.vibrato, vibrato);
            set_float(setter, &params.amount, amount);
        }
    }
}

fn controls(ui: &mut Ui, params: &IntonaParams, setter: &ParamSetter) {
    const SLIDER: f32 = 130.0;
    let label = |ui: &mut Ui, text: &str, help: &str| {
        ui.label(RichText::new(text).color(TEXT_DIM)).on_hover_text(help);
    };
    ui.vertical(|ui| {
        ui.horizontal(|ui| style_presets(ui, params, setter));
        ui.add_space(4.0);
        egui::Grid::new("controlli")
            .num_columns(6)
            .spacing([10.0, 10.0])
            .show(ui, |ui| {
                label(ui, "Tonalità", "La nota di base della scala della canzone.");
                enum_combo(ui, "key", &params.key, setter);
                label(
                    ui,
                    "Velocità",
                    "Quanto in fretta la voce viene portata sulla nota giusta.\n\
                     0 ms = effetto robotico (\"Auto-Tune\" estremo)\n\
                     20-80 ms = correzione evidente ma musicale\n\
                     100-400 ms = correzione naturale, conserva il vibrato",
                );
                ui.add(ParamSlider::for_param(&params.retune_speed, setter).with_width(SLIDER));
                label(
                    ui,
                    "Humanize",
                    "Sulle note lunghe la correzione diventa più morbida, così le note \
                     tenute non suonano \"congelate\"; quelle brevi restano corrette.",
                );
                ui.add(ParamSlider::for_param(&params.humanize, setter).with_width(SLIDER));
                ui.end_row();

                label(ui, "Scala", "Le note ammesse. Puoi anche cliccare sulla tastiera qui sotto.");
                enum_combo(ui, "scale", &params.scale, setter);
                label(ui, "Quantità", "Quanta correzione applicare: 100% = completa.");
                ui.add(ParamSlider::for_param(&params.amount, setter).with_width(SLIDER));
                label(
                    ui,
                    "Flex-Tune",
                    "Corregge solo quando la voce è già vicina alla nota: scivolate e \
                     note volutamente \"sporche\" restano come le hai cantate.",
                );
                ui.add(ParamSlider::for_param(&params.flex_tune, setter).with_width(SLIDER));
                ui.end_row();

                label(
                    ui,
                    "Voce",
                    "Il tipo di voce da correggere. Le voci acute permettono una latenza minore.",
                );
                enum_combo(ui, "voice", &params.voice, setter);
                label(ui, "Trasponi", "Sposta la voce di un certo numero di semitoni.");
                ui.add(ParamSlider::for_param(&params.transpose, setter).with_width(SLIDER));
                label(
                    ui,
                    "Vibrato",
                    "Vibrato naturale: valori negativi lo riducono, positivi lo \
                     aggiungono (anche con la correzione robotica, che lo cancellerebbe).",
                );
                ui.add(ParamSlider::for_param(&params.vibrato, setter).with_width(SLIDER));
                ui.end_row();

                ui.label("");
                ui.label("");
                label(
                    ui,
                    "Formanti",
                    "Il timbro (la \"lunghezza della gola\"), senza cambiare la nota.\n\
                     Positivo = voce più piccola e chiara, negativo = più grande e scura.",
                );
                ui.add(ParamSlider::for_param(&params.formant, setter).with_width(SLIDER));
                label(
                    ui,
                    "Sensibilità",
                    "Quanto il plugin è disposto a riconoscere una nota. Alzala per voci \
                     con molta aria o sussurrate; abbassala se senti salti strani.",
                );
                ui.add(ParamSlider::for_param(&params.tracking, setter).with_width(SLIDER));
                ui.end_row();
            });
    });
}

// --- Tastiera -----------------------------------------------------------------

/// Note dei tasti bianchi e posizione dei tasti neri (in "tasti bianchi").
const WHITE_KEYS: [usize; 7] = [0, 2, 4, 5, 7, 9, 11];
const BLACK_KEYS: [(usize, f32); 5] = [(1, 1.0), (3, 2.0), (6, 4.0), (8, 5.0), (10, 6.0)];

fn keyboard(ui: &mut Ui, params: &IntonaParams, setter: &ParamSetter, display: &PitchDisplay) {
    ui.label(
        RichText::new("Note della scala — clicca un tasto per includerlo o escluderlo")
            .size(12.0)
            .color(TEXT_DIM),
    );
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 70.0), Sense::click());
    let painter = ui.painter_at(rect);

    let notes = params.active_notes();
    let sung = display
        .latest()
        .filter(|p| !p.input.is_nan())
        .map(|p| pitch_class(p.input.round() as i32));

    let white_w = rect.width() / 7.0;
    let black_w = white_w * 0.6;
    let black_h = rect.height() * 0.6;
    let white_rect = |i: usize| {
        Rect::from_min_size(
            Pos2::new(rect.left() + i as f32 * white_w, rect.top()),
            Vec2::new(white_w - 2.0, rect.height()),
        )
    };
    let black_rect = |pos: f32| {
        Rect::from_min_size(
            Pos2::new(rect.left() + pos * white_w - black_w / 2.0 - 1.0, rect.top()),
            Vec2::new(black_w, black_h),
        )
    };

    for (i, &pc) in WHITE_KEYS.iter().enumerate() {
        let r = white_rect(i);
        let fill = if notes[pc] { Color32::from_rgb(0xe9, 0xea, 0xee) } else { Color32::from_rgb(0x4a, 0x4d, 0x55) };
        painter.rect_filled(r, CornerRadius::same(4), fill);
        if notes[pc] {
            let bar = Rect::from_min_max(Pos2::new(r.left(), r.bottom() - 5.0), r.right_bottom());
            painter.rect_filled(bar, CornerRadius::same(2), ACCENT);
        }
        let text_color = if notes[pc] { Color32::from_rgb(0x30, 0x33, 0x3a) } else { TEXT_DIM };
        painter.text(
            Pos2::new(r.center().x, r.bottom() - 16.0),
            Align2::CENTER_CENTER,
            NOTE_NAMES_IT[pc],
            FontId::proportional(12.0),
            text_color,
        );
        if sung == Some(pc) {
            painter.circle_filled(Pos2::new(r.center().x, r.bottom() - 32.0), 5.0, ACCENT);
        }
    }
    for &(pc, pos) in &BLACK_KEYS {
        let r = black_rect(pos);
        let fill = if notes[pc] { Color32::from_rgb(0x26, 0x29, 0x31) } else { Color32::from_rgb(0x3a, 0x3d, 0x45) };
        painter.rect(r, CornerRadius::same(3), fill, Stroke::new(1.0_f32, BACKGROUND), StrokeKind::Outside);
        if notes[pc] {
            let bar = Rect::from_min_max(Pos2::new(r.left() + 3.0, r.bottom() - 5.0), Pos2::new(r.right() - 3.0, r.bottom() - 2.0));
            painter.rect_filled(bar, CornerRadius::same(1), ACCENT);
        }
        if sung == Some(pc) {
            painter.circle_filled(Pos2::new(r.center().x, r.bottom() - 16.0), 4.0, ACCENT);
        }
    }

    // Clic: quale tasto? (prima i neri, che stanno sopra ai bianchi)
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let clicked = BLACK_KEYS
                .iter()
                .find(|(_, p)| black_rect(*p).contains(pos))
                .map(|(pc, _)| *pc)
                .or_else(|| (0..7).find(|&i| white_rect(i).contains(pos)).map(|i| WHITE_KEYS[i]));
            if let Some(pc) = clicked {
                toggle_note(params, setter, pc, &notes);
            }
        }
    }
}

/// Attiva/disattiva una nota. Se era scelta una scala predefinita, la copia
/// prima nella scala personalizzata, cosi' si parte dalle note gia' visibili.
fn toggle_note(params: &IntonaParams, setter: &ParamSetter, pc: usize, notes: &[bool; 12]) {
    let set_bool = |param: &BoolParam, value: bool| {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, value);
        setter.end_set_parameter(param);
    };
    if params.scale.value() != ScaleChoice::Custom {
        for (i, &on) in notes.iter().enumerate() {
            set_bool(params.custom.note(i), on);
        }
        setter.begin_set_parameter(&params.scale);
        setter.set_parameter(&params.scale, ScaleChoice::Custom);
        setter.end_set_parameter(&params.scale);
    }
    set_bool(params.custom.note(pc), !notes[pc]);
}

// --- Grafico dell'intonazione --------------------------------------------------

fn pitch_graph(ui: &mut Ui, params: &IntonaParams, display: &PitchDisplay, state: &mut GuiState) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Intonazione").size(12.0).color(TEXT_DIM));
        ui.label(RichText::new("voce originale").size(12.0).color(INPUT_LINE));
        ui.label(RichText::new("voce corretta").size(12.0).strong().color(ACCENT));
    });

    let size = Vec2::new(ui.available_width(), ui.available_height().max(140.0));
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect(rect, CornerRadius::same(10), PANEL, Stroke::new(1.0_f32, PANEL_BORDER), StrokeKind::Inside);

    let interval = display.interval_s().max(1e-4);
    let count = (GRAPH_SECONDS / interval) as usize;
    display.snapshot(count, &mut state.points);

    // Centro verticale: la mediana delle note recenti, inseguita lentamente.
    let mut recent: Vec<f32> = state.points.iter().map(|p| p.input).filter(|n| !n.is_nan()).collect();
    if !recent.is_empty() {
        recent.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = recent[recent.len() / 2];
        state.graph_center += (median - state.graph_center) * 0.08;
    }
    let half_range = 7.0; // semitoni sopra e sotto il centro
    let low = state.graph_center - half_range;
    let high = state.graph_center + half_range;
    let plot = rect.shrink2(Vec2::new(44.0, 10.0));
    let y_of = |note: f32| plot.bottom() - (note - low) / (high - low) * plot.height();
    // Il punto piu' recente sta sul bordo destro, i piu' vecchi scorrono a sinistra.
    let x_of = |i: usize| {
        let age = count.saturating_sub(1).saturating_sub(i);
        plot.right() - age as f32 / count.max(1) as f32 * plot.width()
    };

    // Righe orizzontali: una per semitono, piu' evidenti per le note della scala.
    let notes = params.active_notes();
    for note in low.ceil() as i32..=high.floor() as i32 {
        let y = y_of(note as f32);
        let in_scale = notes[pitch_class(note)];
        let color = if in_scale { Color32::from_rgb(0x3a, 0x3f, 0x4a) } else { Color32::from_rgb(0x25, 0x28, 0x30) };
        painter.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0_f32, color));
        if in_scale {
            painter.text(
                Pos2::new(rect.left() + 8.0, y),
                Align2::LEFT_CENTER,
                note_label(note),
                FontId::proportional(10.0),
                TEXT_DIM,
            );
        }
    }

    // Le curve: ogni tratto continuo di note diventa una linea.
    let offset = count.saturating_sub(state.points.len());
    let draw_curve = |value: fn(&PitchPoint) -> f32, color: Color32, width: f32| {
        let mut segment: Vec<Pos2> = Vec::new();
        let flush = |segment: &mut Vec<Pos2>| {
            if segment.len() >= 2 {
                painter.add(Shape::line(std::mem::take(segment), Stroke::new(width, color)));
            }
            segment.clear();
        };
        for (i, point) in state.points.iter().enumerate() {
            let note = value(point);
            if note.is_nan() || note < low - 1.0 || note > high + 1.0 {
                flush(&mut segment);
                continue;
            }
            segment.push(Pos2::new(x_of(i + offset), y_of(note).clamp(plot.top(), plot.bottom())));
        }
        flush(&mut segment);
    };
    draw_curve(|p| p.input, INPUT_LINE, 1.5);
    draw_curve(|p| p.output, ACCENT, 2.5);

    if state.points.iter().all(|p| p.input.is_nan()) {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Avvia la riproduzione o canta: qui vedrai la tua intonazione",
            FontId::proportional(13.0),
            TEXT_DIM,
        );
    }
}
