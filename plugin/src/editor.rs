//! L'interfaccia grafica di SosaTune, disegnata con egui.
//!
//! egui e' una libreria "immediate mode": a ogni fotogramma si ridisegna tutto
//! da capo. Qui disegniamo ogni elemento a mano (manopole, indicatore, selettori)
//! in posizioni fisse, cosi' l'aspetto e' sempre identico e facile da modificare:
//! tutte le misure sono nelle costanti `LAYOUT` piu' sotto.
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────────────┐
//! │ SOSATUNE  correzione vocale        [NATURALE] [POP] [ROBOTICO] [LEGGERO] │
//! ├────────────────┬─────────────────────────────────┬───────────────────────┤
//! │  INTONAZIONE   │  CORREZIONE                     │  TONALITÀ E SCALA      │
//! │    ╭──────╮    │   (◔)    (◔)    (◔)    (◔)      │  ◀  C (Do)          ▶ │
//! │   │  La4  │    │  velocità humanize flex vibrato │  ◀  Maggiore        ▶ │
//! │   │ +12 ct│    │   (·)    (·)    (·)    (·)      │  ◀  Alto / Tenore   ▶ │
//! │    ╰──────╯    │  quantità trasponi formanti sens│  latenza 30.7 ms       │
//! ├────────────────┴─────────────────────────────────┴───────────────────────┤
//! │  tastiera: note della scala (clic per includere/escludere)               │
//! ├──────────────────────────────────────────────────────────────────────────┤
//! │  grafico: voce originale (grigio) e corretta (oro)                       │
//! └──────────────────────────────────────────────────────────────────────────┘
//! ```

use crate::params::{ScaleChoice, SosaTuneParams, EDITOR_HEIGHT, EDITOR_WIDTH};
use crate::shared::{PitchDisplay, PitchPoint};
use nih_plug::prelude::*;
use crate::gui::create_egui_editor;
use crate::gui::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Id, Painter, Pos2, Rect,
    Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};
use sosatune_dsp::scale::{octave, pitch_class, NOTE_NAMES_IT};
use std::f32::consts::PI;
use std::sync::Arc;

// --- Colori (l'identita' visiva di SosaTune: grafite e oro) --------------------
const BACKGROUND: Color32 = Color32::from_rgb(0x0e, 0x0f, 0x13);
const HEADER: Color32 = Color32::from_rgb(0x13, 0x14, 0x19);
const PANEL: Color32 = Color32::from_rgb(0x17, 0x19, 0x1f);
const PANEL_HI: Color32 = Color32::from_rgb(0x20, 0x23, 0x2b);
const BORDER: Color32 = Color32::from_rgb(0x2a, 0x2e, 0x38);
const TRACK: Color32 = Color32::from_rgb(0x2c, 0x30, 0x3a);
const KNOB_BODY: Color32 = Color32::from_rgb(0x25, 0x28, 0x31);
const KNOB_RIM: Color32 = Color32::from_rgb(0x3a, 0x3e, 0x4a);
const TEXT: Color32 = Color32::from_rgb(0xec, 0xec, 0xf1);
const TEXT_DIM: Color32 = Color32::from_rgb(0x8a, 0x8f, 0x9c);
const TEXT_FAINT: Color32 = Color32::from_rgb(0x5b, 0x60, 0x6c);
const GOLD: Color32 = Color32::from_rgb(0xf4, 0xb9, 0x42);
const MINT: Color32 = Color32::from_rgb(0x4f, 0xe0, 0xb6);
const INPUT_LINE: Color32 = Color32::from_rgb(0x8e, 0x94, 0xa2);

/// Quanti secondi mostra il grafico.
const GRAPH_SECONDS: f32 = 4.0;

/// Tutte le posizioni (in pixel, dall'angolo in alto a sinistra della finestra).
mod layout {
    pub const HEADER_H: f32 = 56.0;
    pub const ROW_TOP: f32 = 72.0;
    pub const ROW_H: f32 = 260.0;
    pub const RING_X: f32 = 16.0;
    pub const RING_W: f32 = 260.0;
    pub const KNOBS_X: f32 = 288.0;
    pub const KNOBS_W: f32 = 364.0;
    pub const SELECT_X: f32 = 664.0;
    pub const SELECT_W: f32 = 260.0;
    pub const KEYS_TOP: f32 = 344.0;
    pub const KEYS_H: f32 = 70.0;
    pub const GRAPH_TOP: f32 = 426.0;
    pub const MARGIN: f32 = 16.0;
    pub const BIG_KNOB: f32 = 30.0;
    pub const SMALL_KNOB: f32 = 20.0;
}

/// Stato che appartiene solo alla grafica (non viene salvato).
struct GuiState {
    /// Copia degli ultimi punti del grafico (riusata a ogni fotogramma).
    points: Vec<PitchPoint>,
    /// Nota al centro del grafico (segue la voce con dolcezza).
    graph_center: f32,
}

/// Avvolge l'interfaccia in una "rete di sicurezza": se la finestra non si
/// riesce ad aprire (per esempio perche' il Mac non concede la grafica OpenGL),
/// il plugin continua a funzionare senza interfaccia invece di far chiudere
/// di colpo l'intero DAW.
struct SafeEditor(Box<dyn Editor>);

impl Editor for SafeEditor {
    fn spawn(
        &self,
        parent: ParentWindowHandle,
        context: Arc<dyn GuiContext>,
    ) -> Box<dyn std::any::Any + Send> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.0.spawn(parent, context))) {
            Ok(handle) => handle,
            Err(_) => {
                nih_error!("SosaTune: impossibile aprire l'interfaccia grafica, il plugin continua senza");
                Box::new(())
            }
        }
    }

    fn size(&self) -> (u32, u32) {
        self.0.size()
    }

    fn set_scale_factor(&self, factor: f32) -> bool {
        self.0.set_scale_factor(factor)
    }

    fn param_value_changed(&self, id: &str, normalized_value: f32) {
        self.0.param_value_changed(id, normalized_value)
    }

    fn param_modulation_changed(&self, id: &str, modulation_offset: f32) {
        self.0.param_modulation_changed(id, modulation_offset)
    }

    fn param_values_changed(&self) {
        self.0.param_values_changed()
    }
}

pub fn create(params: Arc<SosaTuneParams>, display: Arc<PitchDisplay>) -> Option<Box<dyn Editor>> {
    let editor = create_inner(params, display)?;
    Some(Box::new(SafeEditor(editor)))
}

fn create_inner(params: Arc<SosaTuneParams>, display: Arc<PitchDisplay>) -> Option<Box<dyn Editor>> {
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
            load_fonts(ctx);
        },
        move |ctx, setter, state| {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(BACKGROUND))
                .show(ctx, |ui| {
                    let origin = ui.max_rect().min;
                    // Riserviamo tutta la finestra: dentro disegniamo a mano.
                    ui.allocate_exact_size(
                        Vec2::new(EDITOR_WIDTH as f32, EDITOR_HEIGHT as f32),
                        Sense::hover(),
                    );
                    let at = |x: f32, y: f32| origin + Vec2::new(x, y);
                    let painter = ui.painter().clone();

                    header(ui, &painter, &params, setter, at(0.0, 0.0));
                    tuning_ring(ui, &painter, &display, at(layout::RING_X, layout::ROW_TOP));
                    knobs(ui, &painter, &params, setter, at(layout::KNOBS_X, layout::ROW_TOP));
                    selectors(ui, &painter, &params, setter, &display, at(layout::SELECT_X, layout::ROW_TOP));
                    keyboard(ui, &painter, &params, setter, &display, at(layout::MARGIN, layout::KEYS_TOP));
                    pitch_graph(&painter, &params, &display, state, at(layout::MARGIN, layout::GRAPH_TOP));
                });
        },
    )
}

// --- Caratteri ------------------------------------------------------------------

/// Il carattere di SosaTune e' "Sora" (licenza libera SIL OFL, vedi
/// `assets/Sora-OFL.txt`), incluso direttamente nel plugin. I caratteri
/// predefiniti di egui restano come riserva per i simboli che Sora non ha.
fn load_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "sora".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!("../assets/Sora-Regular.ttf"))),
    );
    fonts.font_data.insert(
        "sora-bold".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!("../assets/Sora-Bold.ttf"))),
    );
    let fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
        family.insert(0, "sora".to_owned());
    }
    let mut bold = vec!["sora-bold".to_owned()];
    bold.extend(fallbacks);
    fonts.families.insert(FontFamily::Name("bold".into()), bold);
    ctx.set_fonts(fonts);
}

fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

// --- Piccoli aiuti per disegnare -------------------------------------------------

fn panel(painter: &Painter, rect: Rect, title: &str) {
    painter.rect(rect, CornerRadius::same(12), PANEL, Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
    if !title.is_empty() {
        painter.text(
            rect.left_top() + Vec2::new(16.0, 18.0),
            Align2::LEFT_CENTER,
            title,
            bold(11.0),
            TEXT_DIM,
        );
    }
}

/// Un punto sulla circonferenza; gli angoli sono in radianti, 0 = destra,
/// crescenti in senso orario (sullo schermo la y va verso il basso).
fn polar(center: Pos2, radius: f32, angle: f32) -> Pos2 {
    center + Vec2::new(angle.cos(), angle.sin()) * radius
}

/// Un arco di cerchio da `from` a `to` (radianti).
fn arc(painter: &Painter, center: Pos2, radius: f32, from: f32, to: f32, stroke: Stroke) {
    let (a, b) = if from <= to { (from, to) } else { (to, from) };
    let steps = (((b - a) * radius / 4.0).ceil() as usize).clamp(2, 128);
    let points: Vec<Pos2> = (0..=steps)
        .map(|i| polar(center, radius, a + (b - a) * i as f32 / steps as f32))
        .collect();
    painter.add(Shape::line(points, stroke));
}

fn note_label(note: i32) -> String {
    format!("{}{}", NOTE_NAMES_IT[pitch_class(note)], octave(note))
}

/// Imposta un parametro dall'interfaccia (avvisando il DAW, cosi' finisce
/// nell'automazione e nell'annulla/ripeti).
fn set_param<P: Param>(setter: &ParamSetter, param: &P, value: P::Plain) {
    setter.begin_set_parameter(param);
    setter.set_parameter(param, value);
    setter.end_set_parameter(param);
}

// --- Intestazione: marchio e stili -------------------------------------------------

/// Stili pronti: (nome, spiegazione, velocita' ms, humanize, flex-tune, vibrato, quantita').
const PRESETS: [(&str, &str, f32, f32, f32, f32, f32); 4] = [
    ("NATURALE", "Correzione trasparente: si sente intonato ma non corretto.", 80.0, 0.35, 0.0, 0.0, 1.0),
    ("POP", "Intonazione precisa e brillante, come nelle produzioni pop.", 20.0, 0.25, 0.0, 0.0, 1.0),
    ("ROBOTICO", "L'effetto estremo: la voce salta da una nota all'altra.", 0.0, 0.0, 0.0, 0.0, 1.0),
    ("LEGGERO", "Sistema solo le stonature e lascia l'espressione.", 150.0, 0.6, 0.6, 0.0, 0.8),
];

fn header(ui: &Ui, painter: &Painter, params: &SosaTuneParams, setter: &ParamSetter, origin: Pos2) {
    let bar = Rect::from_min_size(origin, Vec2::new(EDITOR_WIDTH as f32, layout::HEADER_H));
    painter.rect_filled(bar, CornerRadius::ZERO, HEADER);
    painter.line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0_f32, BORDER));

    // Marchio: "SOSA" bianco + "TUNE" oro.
    let y = bar.center().y;
    let sosa = painter.text(Pos2::new(origin.x + 24.0, y), Align2::LEFT_CENTER, "SOSA", bold(26.0), TEXT);
    let tune = painter.text(Pos2::new(sosa.right(), y), Align2::LEFT_CENTER, "TUNE", bold(26.0), GOLD);
    painter.text(
        Pos2::new(tune.right() + 14.0, y + 3.0),
        Align2::LEFT_CENTER,
        "correzione vocale",
        regular(12.0),
        TEXT_FAINT,
    );

    // Pulsanti degli stili, allineati a destra.
    let mut right = bar.right() - 20.0;
    for (name, help, retune, humanize, flex, vibrato, amount) in PRESETS.iter().rev() {
        let width = 18.0 + name.len() as f32 * 7.6;
        let chip = Rect::from_min_max(Pos2::new(right - width, y - 13.0), Pos2::new(right, y + 13.0));
        right -= width + 8.0;

        let active = (params.retune_speed.value() - retune).abs() < 0.5
            && (params.humanize.value() - humanize).abs() < 0.01
            && (params.flex_tune.value() - flex).abs() < 0.01
            && (params.vibrato.value() - vibrato).abs() < 0.01
            && (params.amount.value() - amount).abs() < 0.01;
        let response = ui.interact(chip, Id::new(("stile", *name)), Sense::click()).on_hover_text(*help);
        let (fill, text, border) = if active {
            (GOLD, BACKGROUND, GOLD)
        } else if response.hovered() {
            (PANEL_HI, TEXT, GOLD)
        } else {
            (PANEL, TEXT_DIM, BORDER)
        };
        painter.rect(chip, CornerRadius::same(13), fill, Stroke::new(1.0_f32, border), StrokeKind::Inside);
        painter.text(chip.center(), Align2::CENTER_CENTER, *name, bold(11.0), text);

        if response.clicked() {
            set_param(setter, &params.retune_speed, *retune);
            set_param(setter, &params.humanize, *humanize);
            set_param(setter, &params.flex_tune, *flex);
            set_param(setter, &params.vibrato, *vibrato);
            set_param(setter, &params.amount, *amount);
        }
    }
}

// --- Indicatore circolare dell'intonazione -----------------------------------------

fn tuning_ring(ui: &Ui, painter: &Painter, display: &PitchDisplay, origin: Pos2) {
    let rect = Rect::from_min_size(origin, Vec2::new(layout::RING_W, layout::ROW_H));
    panel(painter, rect, "INTONAZIONE");
    ui.interact(rect, Id::new("anello"), Sense::hover()).on_hover_text(
        "La nota che stai cantando e di quanti centesimi sei calante (−) o crescente (+). \
         Verde = entro ±10 centesimi.",
    );

    let center = rect.center() + Vec2::new(0.0, 12.0);
    let radius = 86.0;
    // L'arco va da -50 centesimi (in basso a sinistra) a +50 (in basso a destra).
    let start = 0.75 * PI;
    let sweep = 1.5 * PI;
    let angle_of = |cents: f32| start + (cents.clamp(-50.0, 50.0) + 50.0) / 100.0 * sweep;

    arc(painter, center, radius, start, start + sweep, Stroke::new(10.0_f32, TRACK));
    for (cents, label) in [(-50.0, "-50"), (-25.0, ""), (0.0, "0"), (25.0, ""), (50.0, "+50")] {
        let a = angle_of(cents);
        painter.line_segment(
            [polar(center, radius + 8.0, a), polar(center, radius + 14.0, a)],
            Stroke::new(1.5_f32, TEXT_FAINT),
        );
        if !label.is_empty() {
            painter.text(polar(center, radius + 24.0, a), Align2::CENTER_CENTER, label, regular(10.0), TEXT_FAINT);
        }
    }
    // La zona "intonata" (±10 centesimi), appena accennata.
    arc(painter, center, radius, angle_of(-10.0), angle_of(10.0), Stroke::new(10.0_f32, MINT.gamma_multiply(0.18)));

    match display.latest().filter(|p| !p.input.is_nan()) {
        Some(p) => {
            let nearest = p.input.round() as i32;
            let cents = (p.input - nearest as f32) * 100.0;
            let color = if cents.abs() <= 10.0 { MINT } else { GOLD };
            arc(painter, center, radius, angle_of(0.0), angle_of(cents), Stroke::new(10.0_f32, color));
            painter.circle_filled(polar(center, radius, angle_of(cents)), 8.0, color);
            painter.circle_filled(polar(center, radius, angle_of(cents)), 3.5, BACKGROUND);

            painter.text(center - Vec2::new(0.0, 10.0), Align2::CENTER_CENTER, note_label(nearest), bold(40.0), TEXT);
            painter.text(
                center + Vec2::new(0.0, 26.0),
                Align2::CENTER_CENTER,
                format!("{:+.0} cent", cents),
                regular(14.0),
                color,
            );
            if !p.target.is_nan() {
                painter.text(
                    center + Vec2::new(0.0, 48.0),
                    Align2::CENTER_CENTER,
                    format!("→ {}", note_label(p.target.round() as i32)),
                    regular(12.0),
                    TEXT_DIM,
                );
            }
        }
        None => {
            painter.text(center - Vec2::new(0.0, 10.0), Align2::CENTER_CENTER, "—", regular(42.0), TEXT_FAINT);
            painter.text(center + Vec2::new(0.0, 26.0), Align2::CENTER_CENTER, "in ascolto", regular(13.0), TEXT_FAINT);
        }
    }
}

// --- Manopole --------------------------------------------------------------------

/// Una manopola: si trascina in su/giu' (con Maiusc per i ritocchi fini),
/// doppio clic per tornare al valore iniziale.
#[allow(clippy::too_many_arguments)]
fn knob<P: Param>(
    ui: &Ui,
    painter: &Painter,
    setter: &ParamSetter,
    param: &P,
    center: Pos2,
    radius: f32,
    label: &str,
    help: &str,
    bipolar: bool,
) {
    let hit = Rect::from_center_size(center, Vec2::splat(radius * 2.0 + 20.0));
    let response = ui
        .interact(hit, Id::new(("manopola", label)), Sense::click_and_drag())
        .on_hover_cursor(CursorIcon::ResizeVertical)
        .on_hover_text(help);

    // Trascinamento: teniamo il valore di partenza e lo spostamento accumulato,
    // cosi' anche i parametri "a scatti" si muovono in modo fluido.
    let memory_id = response.id.with("trascinamento");
    if response.drag_started() {
        setter.begin_set_parameter(param);
        let start = (param.unmodulated_normalized_value(), 0.0f32);
        ui.memory_mut(|m| m.data.insert_temp(memory_id, start));
    }
    if response.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let speed = if fine { 0.0015 } else { 0.006 };
        let (start, moved) = ui
            .memory_mut(|m| m.data.get_temp::<(f32, f32)>(memory_id))
            .unwrap_or((param.unmodulated_normalized_value(), 0.0));
        let moved = moved - response.drag_delta().y * speed;
        setter.set_parameter_normalized(param, (start + moved).clamp(0.0, 1.0));
        ui.memory_mut(|m| m.data.insert_temp(memory_id, (start, moved)));
    }
    if response.drag_stopped() {
        setter.end_set_parameter(param);
        ui.memory_mut(|m| m.data.remove::<(f32, f32)>(memory_id));
    }
    if response.double_clicked() {
        setter.begin_set_parameter(param);
        setter.set_parameter_normalized(param, param.default_normalized_value());
        setter.end_set_parameter(param);
    }

    // Disegno: anello di sfondo, arco del valore, corpo e lancetta.
    let value = param.modulated_normalized_value();
    let start = 0.75 * PI;
    let sweep = 1.5 * PI;
    let angle = start + value * sweep;
    let ring = radius + 7.0;
    let width = if radius > 25.0 { 4.0_f32 } else { 3.0_f32 };
    arc(painter, center, ring, start, start + sweep, Stroke::new(width, TRACK));
    let from = if bipolar { 1.5 * PI } else { start };
    if (angle - from).abs() > 0.01 {
        arc(painter, center, ring, from, angle, Stroke::new(width, GOLD));
    }
    let body = if response.hovered() || response.dragged() { KNOB_RIM } else { KNOB_BODY };
    painter.circle_filled(center, radius, body);
    painter.circle_stroke(center, radius, Stroke::new(1.0_f32, KNOB_RIM));
    painter.line_segment(
        [polar(center, radius * 0.3, angle), polar(center, radius - 5.0, angle)],
        Stroke::new(2.5_f32, TEXT),
    );

    let label_size = if radius > 25.0 { 11.0 } else { 10.0 };
    painter.text(
        center - Vec2::new(0.0, ring + 12.0),
        Align2::CENTER_CENTER,
        label,
        regular(label_size),
        TEXT_DIM,
    );
    painter.text(
        center + Vec2::new(0.0, ring + 12.0),
        Align2::CENTER_CENTER,
        param.normalized_value_to_string(value, true),
        regular(label_size + 2.0),
        TEXT,
    );
}

fn knobs(ui: &Ui, painter: &Painter, params: &SosaTuneParams, setter: &ParamSetter, origin: Pos2) {
    let rect = Rect::from_min_size(origin, Vec2::new(layout::KNOBS_W, layout::ROW_H));
    panel(painter, rect, "CORREZIONE");
    let column = |i: usize| origin.x + 46.0 + i as f32 * 91.0;
    let big_y = origin.y + 102.0;
    let small_y = origin.y + 210.0;
    let big = layout::BIG_KNOB;
    let small = layout::SMALL_KNOB;

    knob(ui, painter, setter, &params.retune_speed, Pos2::new(column(0), big_y), big, "VELOCITÀ",
        "Quanto in fretta la voce viene portata sulla nota giusta.\n\
         0 ms = effetto robotico, 20-80 ms = musicale, 100-400 ms = naturale.", false);
    knob(ui, painter, setter, &params.humanize, Pos2::new(column(1), big_y), big, "HUMANIZE",
        "Sulle note lunghe la correzione diventa più morbida, così le note tenute \
         non suonano \"congelate\"; quelle brevi restano corrette.", false);
    knob(ui, painter, setter, &params.flex_tune, Pos2::new(column(2), big_y), big, "FLEX-TUNE",
        "Corregge solo quando la voce è già vicina alla nota: scivolate e note \
         volutamente \"sporche\" restano come le hai cantate.", false);
    knob(ui, painter, setter, &params.vibrato, Pos2::new(column(3), big_y), big, "VIBRATO",
        "Vibrato naturale: a sinistra lo riduce, a destra lo aggiunge \
         (anche con la correzione robotica, che lo cancellerebbe).", true);

    knob(ui, painter, setter, &params.amount, Pos2::new(column(0), small_y), small, "QUANTITÀ",
        "Quanta correzione applicare: 100% = completa.", false);
    knob(ui, painter, setter, &params.transpose, Pos2::new(column(1), small_y), small, "TRASPONI",
        "Sposta tutta la voce di un certo numero di semitoni, mantenendo il timbro.", true);
    knob(ui, painter, setter, &params.formant, Pos2::new(column(2), small_y), small, "FORMANTI",
        "Il timbro (la \"lunghezza della gola\"), senza cambiare la nota.\n\
         A destra voce più piccola e chiara, a sinistra più grande e scura.", true);
    knob(ui, painter, setter, &params.tracking, Pos2::new(column(3), small_y), small, "SENSIBILITÀ",
        "Quanto il plugin è disposto a riconoscere una nota. Alzala per voci con \
         molta aria o sussurrate; abbassala se senti salti strani.", false);
}

// --- Selettori di tonalita', scala e voce ---------------------------------------------

/// Un selettore a frecce: clic sulle frecce per scorrere, clic al centro per
/// passare alla scelta successiva.
fn selector<T: Enum + PartialEq + 'static>(
    ui: &Ui,
    painter: &Painter,
    setter: &ParamSetter,
    param: &EnumParam<T>,
    rect: Rect,
    title: &str,
    help: &str,
) {
    painter.text(rect.left_top() - Vec2::new(0.0, 10.0), Align2::LEFT_CENTER, title, regular(10.0), TEXT_FAINT);

    let names = T::variants();
    let current = param.value().to_index();
    let count = names.len();
    let left = Rect::from_min_max(rect.min, Pos2::new(rect.left() + 32.0, rect.bottom()));
    let right = Rect::from_min_max(Pos2::new(rect.right() - 32.0, rect.top()), rect.max);
    let middle = Rect::from_min_max(Pos2::new(left.right(), rect.top()), Pos2::new(right.left(), rect.bottom()));

    let r_left = ui.interact(left, Id::new((title, "indietro")), Sense::click());
    let r_right = ui.interact(right, Id::new((title, "avanti")), Sense::click());
    let r_middle = ui.interact(middle, Id::new((title, "centro")), Sense::click()).on_hover_text(help);

    let hovered = r_left.hovered() || r_right.hovered() || r_middle.hovered();
    painter.rect(
        rect,
        CornerRadius::same(8),
        PANEL_HI,
        Stroke::new(1.0_f32, if hovered { GOLD } else { BORDER }),
        StrokeKind::Inside,
    );
    painter.text(rect.center(), Align2::CENTER_CENTER, names[current], regular(14.0), TEXT);

    let triangle = |center: Pos2, pointing_right: bool, active: bool| {
        let d = if pointing_right { 1.0 } else { -1.0 };
        let points = vec![
            center + Vec2::new(5.0 * d, 0.0),
            center + Vec2::new(-4.0 * d, -6.0),
            center + Vec2::new(-4.0 * d, 6.0),
        ];
        painter.add(Shape::convex_polygon(points, if active { GOLD } else { TEXT_DIM }, Stroke::NONE));
    };
    triangle(left.center(), false, r_left.hovered());
    triangle(right.center(), true, r_right.hovered());

    let new_index = if r_left.clicked() {
        Some((current + count - 1) % count)
    } else if r_right.clicked() || r_middle.clicked() {
        Some((current + 1) % count)
    } else {
        None
    };
    if let Some(index) = new_index {
        set_param(setter, param, T::from_index(index));
    }
}

fn selectors(ui: &Ui, painter: &Painter, params: &SosaTuneParams, setter: &ParamSetter, display: &PitchDisplay, origin: Pos2) {
    let rect = Rect::from_min_size(origin, Vec2::new(layout::SELECT_W, layout::ROW_H));
    panel(painter, rect, "");
    let row = |y: f32| Rect::from_min_size(origin + Vec2::new(16.0, y), Vec2::new(layout::SELECT_W - 32.0, 34.0));

    selector(ui, painter, setter, &params.key, row(30.0), "TONALITÀ", "La nota di base della scala della canzone.");
    selector(ui, painter, setter, &params.scale, row(94.0), "SCALA",
        "Le note ammesse. Puoi anche cliccare sulla tastiera qui sotto per crearne una tua.");
    selector(ui, painter, setter, &params.voice, row(158.0), "VOCE",
        "Il tipo di voce da correggere. Le voci acute permettono una latenza minore.");

    painter.text(
        origin + Vec2::new(16.0, 226.0),
        Align2::LEFT_CENTER,
        format!("Latenza {:.1} ms", display.latency_ms()),
        regular(11.0),
        TEXT_DIM,
    );
    painter.text(
        origin + Vec2::new(16.0, 242.0),
        Align2::LEFT_CENTER,
        "compensata automaticamente dal DAW",
        regular(10.0),
        TEXT_FAINT,
    );
}

// --- Tastiera ------------------------------------------------------------------

/// Note dei tasti bianchi e posizione dei tasti neri (in "tasti bianchi").
const WHITE_KEYS: [usize; 7] = [0, 2, 4, 5, 7, 9, 11];
const BLACK_KEYS: [(usize, f32); 5] = [(1, 1.0), (3, 2.0), (6, 4.0), (8, 5.0), (10, 6.0)];

fn keyboard(ui: &Ui, painter: &Painter, params: &SosaTuneParams, setter: &ParamSetter, display: &PitchDisplay, origin: Pos2) {
    let rect = Rect::from_min_size(origin, Vec2::new(EDITOR_WIDTH as f32 - 2.0 * layout::MARGIN, layout::KEYS_H));
    let response = ui
        .interact(rect, Id::new("tastiera"), Sense::click())
        .on_hover_text("Le note della scala: clicca un tasto per includerlo o escluderlo.");

    let notes = params.active_notes();
    let sung = display
        .latest()
        .filter(|p| !p.input.is_nan())
        .map(|p| pitch_class(p.input.round() as i32));

    let white_w = rect.width() / 7.0;
    let black_w = white_w * 0.55;
    let black_h = rect.height() * 0.6;
    let white_rect = |i: usize| {
        Rect::from_min_size(
            Pos2::new(rect.left() + i as f32 * white_w + 1.0, rect.top()),
            Vec2::new(white_w - 2.0, rect.height()),
        )
    };
    let black_rect = |pos: f32| {
        Rect::from_min_size(
            Pos2::new(rect.left() + pos * white_w - black_w / 2.0, rect.top()),
            Vec2::new(black_w, black_h),
        )
    };

    for (i, &pc) in WHITE_KEYS.iter().enumerate() {
        let r = white_rect(i);
        let fill = if notes[pc] { Color32::from_rgb(0xdc, 0xde, 0xe4) } else { Color32::from_rgb(0x33, 0x36, 0x3f) };
        painter.rect_filled(r, CornerRadius::same(6), fill);
        if notes[pc] {
            let bar = Rect::from_min_max(Pos2::new(r.left() + 6.0, r.bottom() - 6.0), Pos2::new(r.right() - 6.0, r.bottom() - 3.0));
            painter.rect_filled(bar, CornerRadius::same(2), GOLD);
        }
        let text_color = if notes[pc] { Color32::from_rgb(0x2a, 0x2d, 0x35) } else { TEXT_FAINT };
        painter.text(
            Pos2::new(r.center().x, r.bottom() - 18.0),
            Align2::CENTER_CENTER,
            NOTE_NAMES_IT[pc],
            regular(12.0),
            text_color,
        );
        if sung == Some(pc) {
            painter.circle_filled(Pos2::new(r.center().x, r.bottom() - 36.0), 5.0, GOLD);
        }
    }
    for &(pc, pos) in &BLACK_KEYS {
        let r = black_rect(pos);
        let fill = if notes[pc] { Color32::from_rgb(0x1c, 0x1e, 0x25) } else { Color32::from_rgb(0x3f, 0x43, 0x4d) };
        painter.rect(r, CornerRadius::same(4), fill, Stroke::new(2.0_f32, BACKGROUND), StrokeKind::Outside);
        if notes[pc] {
            let bar = Rect::from_min_max(Pos2::new(r.left() + 5.0, r.bottom() - 6.0), Pos2::new(r.right() - 5.0, r.bottom() - 3.0));
            painter.rect_filled(bar, CornerRadius::same(1), GOLD);
        }
        if sung == Some(pc) {
            painter.circle_filled(Pos2::new(r.center().x, r.bottom() - 16.0), 4.0, GOLD);
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
fn toggle_note(params: &SosaTuneParams, setter: &ParamSetter, pc: usize, notes: &[bool; 12]) {
    if params.scale.value() != ScaleChoice::Custom {
        for (i, &on) in notes.iter().enumerate() {
            set_param(setter, params.custom.note(i), on);
        }
        set_param(setter, &params.scale, ScaleChoice::Custom);
    }
    set_param(setter, params.custom.note(pc), !notes[pc]);
}

// --- Grafico dell'intonazione --------------------------------------------------

fn pitch_graph(painter: &Painter, params: &SosaTuneParams, display: &PitchDisplay, state: &mut GuiState, origin: Pos2) {
    let size = Vec2::new(
        EDITOR_WIDTH as f32 - 2.0 * layout::MARGIN,
        EDITOR_HEIGHT as f32 - layout::GRAPH_TOP - layout::MARGIN,
    );
    let rect = Rect::from_min_size(origin, size);
    panel(painter, rect, "GRAFICO");
    // Legenda.
    let legend_y = rect.top() + 18.0;
    painter.line_segment(
        [Pos2::new(rect.left() + 90.0, legend_y), Pos2::new(rect.left() + 108.0, legend_y)],
        Stroke::new(2.0_f32, INPUT_LINE),
    );
    painter.text(Pos2::new(rect.left() + 114.0, legend_y), Align2::LEFT_CENTER, "voce originale", regular(11.0), TEXT_DIM);
    painter.line_segment(
        [Pos2::new(rect.left() + 210.0, legend_y), Pos2::new(rect.left() + 228.0, legend_y)],
        Stroke::new(3.0_f32, GOLD),
    );
    painter.text(Pos2::new(rect.left() + 234.0, legend_y), Align2::LEFT_CENTER, "SosaTune", regular(11.0), TEXT_DIM);

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
    let half_range = 6.0; // semitoni sopra e sotto il centro
    let low = state.graph_center - half_range;
    let high = state.graph_center + half_range;
    let plot = Rect::from_min_max(rect.min + Vec2::new(52.0, 34.0), rect.max - Vec2::new(16.0, 12.0));
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
        let color = if in_scale { Color32::from_rgb(0x30, 0x34, 0x3f) } else { Color32::from_rgb(0x1f, 0x22, 0x29) };
        painter.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0_f32, color));
        if in_scale {
            painter.text(Pos2::new(rect.left() + 14.0, y), Align2::LEFT_CENTER, note_label(note), regular(10.0), TEXT_FAINT);
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
    draw_curve(|p| p.output, GOLD, 2.5);

    if state.points.iter().all(|p| p.input.is_nan()) {
        painter.text(
            plot.center(),
            Align2::CENTER_CENTER,
            "Avvia la riproduzione o canta: qui vedrai la tua intonazione",
            regular(13.0),
            TEXT_FAINT,
        );
    }
}
