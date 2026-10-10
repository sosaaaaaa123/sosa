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
//! │  COMPRESSORE A VALVOLA [ON] soglia rapporto attacco rilascio valvola ... ▮│
//! ├──────────────────────────────────────────────────────────────────────────┤
//! │  grafico: voce originale (grigio) e corretta (oro)                       │
//! └──────────────────────────────────────────────────────────────────────────┘
//! ```

use crate::params::{CompModeChoice, ScaleChoice, SosaTuneParams, EDITOR_HEIGHT, EDITOR_WIDTH};
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
    pub const COMP_TOP: f32 = 426.0;
    pub const COMP_H: f32 = 150.0;
    pub const GRAPH_TOP: f32 = 588.0;
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
    /// Indicatore della compressione: sale subito, scende con calma.
    meter_db: f32,
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
    create_egui_editor(
        params.editor_state.clone(),
        new_state(),
        |ctx, _state| setup(ctx),
        move |ctx, setter, state| draw(ctx, &params, &display, setter, state),
    )
}

fn new_state() -> GuiState {
    GuiState {
        points: Vec::with_capacity(crate::shared::HISTORY_LEN),
        graph_center: 60.0,
        meter_db: 0.0,
    }
}

/// Impostazioni fatte una volta sola, all'apertura della finestra.
fn setup(ctx: &egui::Context) {
    // Sempre tema scuro, anche se il Mac e' in modalita' chiara.
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_visuals(egui::Visuals::dark());
    load_fonts(ctx);
}

/// Disegna un fotogramma dell'interfaccia.
fn draw(
    ctx: &egui::Context,
    params: &SosaTuneParams,
    display: &PitchDisplay,
    setter: &ParamSetter,
    state: &mut GuiState,
) {
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

            header(ui, &painter, params, setter, at(0.0, 0.0));
            tuning_ring(ui, &painter, display, at(layout::RING_X, layout::ROW_TOP));
            knobs(ui, &painter, params, setter, at(layout::KNOBS_X, layout::ROW_TOP));
            selectors(ui, &painter, params, setter, display, at(layout::SELECT_X, layout::ROW_TOP));
            keyboard(ui, &painter, params, setter, display, at(layout::MARGIN, layout::KEYS_TOP));
            compressor(ui, &painter, params, setter, display, state, at(layout::MARGIN, layout::COMP_TOP));
            pitch_graph(&painter, params, display, state, at(layout::MARGIN, layout::GRAPH_TOP));
        });
}

/// Per l'anteprima (vedi `preview.rs`): la stessa interfaccia, senza finestra.
pub(crate) struct Headless {
    state: GuiState,
}

impl Headless {
    pub(crate) fn new(ctx: &egui::Context) -> Self {
        setup(ctx);
        Self { state: new_state() }
    }

    pub(crate) fn draw(
        &mut self,
        ctx: &egui::Context,
        params: &SosaTuneParams,
        display: &PitchDisplay,
        setter: &ParamSetter,
    ) {
        draw(ctx, params, display, setter, &mut self.state);
    }
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
                    format!("verso {}", note_label(p.target.round() as i32)),
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
/// Rende `hit` una manopola: trascina in su/giu' (Maiusc = ritocchi fini),
/// doppio clic = valore iniziale.
fn drag_param<P: Param>(
    ui: &Ui,
    setter: &ParamSetter,
    param: &P,
    hit: Rect,
    label: &str,
    help: &str,
) -> egui::Response {
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

    response
}

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
    let response = drag_param(ui, setter, param, hit, label, help);

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
    // Se il nome e' lungo, il carattere si rimpicciolisce per stare tra le frecce.
    let available = (middle.width() - 8.0).max(10.0);
    let wide = painter.layout_no_wrap(names[current].to_owned(), regular(14.0), TEXT).size().x;
    let size = if wide > available { (14.0 * available / wide).max(10.0) } else { 14.0 };
    painter.text(middle.center(), Align2::CENTER_CENTER, names[current], regular(size), TEXT);

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

// --- Compressore ottico a valvola ----------------------------------------------

#[allow(clippy::too_many_arguments)]
// --- Compressore: pannello da rack analogico -----------------------------------
//
// Un apparecchio da studio immaginario, disegnato per SosaTune: pannello in
// metallo scuro con le scritte incise, strumento ad ago per la riduzione di
// guadagno, interruttore a levetta con spia, manopole a gonna con la scala
// incisa attorno.

const PLATE_TOP: Color32 = Color32::from_rgb(0x34, 0x32, 0x2f);
const PLATE_BOTTOM: Color32 = Color32::from_rgb(0x22, 0x21, 0x1f);
const PLATE_EDGE: Color32 = Color32::from_rgb(0x48, 0x45, 0x40);
const EAR: Color32 = Color32::from_rgb(0x1a, 0x19, 0x18);
const ENGRAVING: Color32 = Color32::from_rgb(0xd8, 0xd0, 0xbc);
const ENGRAVING_DIM: Color32 = Color32::from_rgb(0x8f, 0x89, 0x7c);
const METER_FACE: Color32 = Color32::from_rgb(0xf1, 0xe4, 0xc2);
const METER_INK: Color32 = Color32::from_rgb(0x2b, 0x24, 0x1a);
const METER_RED: Color32 = Color32::from_rgb(0xb8, 0x3a, 0x2a);
const LAMP_ON: Color32 = Color32::from_rgb(0xff, 0xb3, 0x40);
const LAMP_OFF: Color32 = Color32::from_rgb(0x3a, 0x2c, 0x1e);

/// Testo inciso nel metallo: un'ombra scura sotto e il testo chiaro sopra.
fn engraved(painter: &Painter, pos: Pos2, align: Align2, text: &str, font: FontId, color: Color32) {
    painter.text(pos + Vec2::new(0.0, 1.0), align, text, font.clone(), Color32::from_black_alpha(160));
    painter.text(pos, align, text, font, color);
}

/// Un rettangolo con sfumatura verticale.
fn vertical_gradient(painter: &Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

/// Vite del pannello, con la fessura inclinata.
fn screw(painter: &Painter, center: Pos2, slot_angle: f32) {
    painter.circle_filled(center, 5.0, Color32::from_rgb(0x6d, 0x69, 0x62));
    painter.circle_stroke(center, 5.0, Stroke::new(1.0_f32, Color32::from_rgb(0x12, 0x11, 0x10)));
    painter.circle_filled(center - Vec2::new(1.2, 1.2), 2.5, Color32::from_white_alpha(28));
    painter.line_segment(
        [polar(center, 3.6, slot_angle), polar(center, 3.6, slot_angle + PI)],
        Stroke::new(1.4_f32, Color32::from_rgb(0x22, 0x20, 0x1d)),
    );
}

/// Manopola a gonna con la scala incisa attorno.
#[allow(clippy::too_many_arguments)]
fn hardware_knob<P: Param>(
    ui: &Ui,
    painter: &Painter,
    setter: &ParamSetter,
    param: &P,
    center: Pos2,
    label: &str,
    ends: (&str, &str),
    help: &str,
    readout: Option<String>,
) {
    let radius = 21.0;
    let hit = Rect::from_center_size(center, Vec2::splat(2.0 * radius + 16.0));
    let response = drag_param(ui, setter, param, hit, label, help);
    let active = response.hovered() || response.dragged();

    let start = 0.75 * PI;
    let sweep = 1.5 * PI;
    // Scala incisa: 11 tacche, piu' lunghe agli estremi e al centro.
    for i in 0..=10 {
        let a = start + sweep * i as f32 / 10.0;
        let long = i == 0 || i == 5 || i == 10;
        let inner = radius + 4.0;
        let outer = radius + if long { 9.0 } else { 7.0 };
        painter.line_segment(
            [polar(center, inner, a), polar(center, outer, a)],
            Stroke::new(if long { 1.4_f32 } else { 1.0_f32 }, ENGRAVING_DIM),
        );
    }
    engraved(painter, polar(center, radius + 15.0, start) + Vec2::new(-2.0, 4.0), Align2::RIGHT_CENTER, ends.0, regular(8.0), ENGRAVING_DIM);
    engraved(painter, polar(center, radius + 15.0, start + sweep) + Vec2::new(2.0, 4.0), Align2::LEFT_CENTER, ends.1, regular(8.0), ENGRAVING_DIM);

    // Ombra, gonna, cappuccio con riflesso, indice.
    painter.circle_filled(center + Vec2::new(0.0, 2.5), radius + 1.0, Color32::from_black_alpha(110));
    painter.circle_filled(center, radius, Color32::from_rgb(0x15, 0x14, 0x13));
    painter.circle_stroke(center, radius, Stroke::new(1.0_f32, Color32::from_rgb(0x4a, 0x47, 0x42)));
    let cap = radius * 0.66;
    painter.circle_filled(center, cap, Color32::from_rgb(0x2a, 0x28, 0x26));
    painter.circle_stroke(center, cap, Stroke::new(1.0_f32, Color32::from_rgb(0x55, 0x52, 0x4c)));
    arc(painter, center, cap - 2.0, 1.05 * PI, 1.55 * PI, Stroke::new(2.0_f32, Color32::from_white_alpha(30)));
    let value = param.modulated_normalized_value();
    let angle = start + value * sweep;
    painter.line_segment(
        [polar(center, cap + 1.0, angle), polar(center, radius - 2.0, angle)],
        Stroke::new(2.6_f32, if active { GOLD } else { ENGRAVING }),
    );
    painter.circle_filled(polar(center, cap * 0.55, angle), 1.6, if active { GOLD } else { ENGRAVING_DIM });

    engraved(painter, center - Vec2::new(0.0, radius + 22.0), Align2::CENTER_CENTER, label, bold(9.0), ENGRAVING);
    let text = readout.unwrap_or_else(|| param.normalized_value_to_string(value, true));
    engraved(
        painter,
        center + Vec2::new(0.0, radius + 24.0),
        Align2::CENTER_CENTER,
        &text,
        regular(if active { 11.0 } else { 10.0 }),
        if active { GOLD } else { ENGRAVING },
    );
}

/// Strumento ad ago della riduzione di guadagno (da 0 a 20 dB).
fn gain_reduction_meter(painter: &Painter, rect: Rect, db: f32, lit: bool) {
    // Cornice e vetro.
    painter.rect_filled(rect.expand(3.0), CornerRadius::same(5), Color32::from_rgb(0x0b, 0x0b, 0x0a));
    painter.rect_stroke(rect.expand(3.0), CornerRadius::same(5), Stroke::new(1.0_f32, PLATE_EDGE), StrokeKind::Outside);
    let face = if lit { METER_FACE } else { METER_FACE.gamma_multiply(0.55) };
    vertical_gradient(painter, rect, face, face.gamma_multiply(0.86));

    // L'ago ruota attorno a un perno sotto il quadrante.
    let pivot = Pos2::new(rect.center().x, rect.bottom() + 34.0);
    let scale_radius = rect.height() + 2.0;
    let left = -0.30 * PI; // 20 dB
    let right = 0.30 * PI; // 0 dB
    // Scala compressa verso i valori alti, come sugli strumenti veri.
    let angle_of = |db: f32| {
        let p = (db.clamp(0.0, 20.0) / 20.0).powf(0.62);
        right + (left - right) * p
    };
    let dial = |a: f32, r: f32| pivot + Vec2::new(a.sin() * r, -a.cos() * r);
    let clipped = painter.with_clip_rect(rect);
    // Arco della scala (rosso oltre 10 dB).
    let steps = 40;
    for i in 0..steps {
        let d0 = 20.0 * i as f32 / steps as f32;
        let d1 = 20.0 * (i + 1) as f32 / steps as f32;
        let color = if d0 >= 10.0 { METER_RED } else { METER_INK };
        clipped.line_segment(
            [dial(angle_of(d0), scale_radius - 30.0), dial(angle_of(d1), scale_radius - 30.0)],
            Stroke::new(if d0 >= 10.0 { 2.2_f32 } else { 1.2_f32 }, color),
        );
    }
    for (value, label) in [(0.0, "0"), (1.0, "1"), (2.0, "2"), (3.0, "3"), (5.0, "5"), (7.0, "7"), (10.0, "10"), (15.0, "15"), (20.0, "20")] {
        let a = angle_of(value);
        let color = if value >= 10.0 { METER_RED } else { METER_INK };
        clipped.line_segment([dial(a, scale_radius - 30.0), dial(a, scale_radius - 23.0)], Stroke::new(1.3_f32, color));
        clipped.text(dial(a, scale_radius - 16.0), Align2::CENTER_CENTER, label, regular(8.5), color);
    }
    clipped.text(
        Pos2::new(rect.center().x, rect.bottom() - 26.0),
        Align2::CENTER_CENTER,
        "RIDUZIONE dB",
        bold(8.0),
        METER_INK.gamma_multiply(0.85),
    );
    // Ago e perno.
    let a = angle_of(db);
    clipped.line_segment(
        [dial(a, 26.0) + Vec2::new(1.5, 1.5), dial(a, scale_radius - 8.0) + Vec2::new(1.5, 1.5)],
        Stroke::new(1.6_f32, Color32::from_black_alpha(45)),
    );
    clipped.line_segment([dial(a, 26.0), dial(a, scale_radius - 8.0)], Stroke::new(1.4_f32, Color32::from_rgb(0x1a, 0x16, 0x12)));
    let hub = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 12.0), rect.right_bottom());
    clipped.rect_filled(hub, CornerRadius::ZERO, Color32::from_rgb(0x1c, 0x1a, 0x17));
    // Riflesso del vetro.
    let glare = Rect::from_min_max(rect.left_top(), Pos2::new(rect.right(), rect.top() + rect.height() * 0.35));
    vertical_gradient(&clipped, glare, Color32::from_white_alpha(26), Color32::from_white_alpha(0));
    if lit {
        // Retroilluminazione calda.
        painter.rect_stroke(rect, CornerRadius::same(2), Stroke::new(2.0_f32, Color32::from_rgba_unmultiplied(255, 170, 60, 30)), StrokeKind::Inside);
    }
}

fn compressor(
    ui: &Ui,
    painter: &Painter,
    params: &SosaTuneParams,
    setter: &ParamSetter,
    display: &PitchDisplay,
    state: &mut GuiState,
    origin: Pos2,
) {
    let comp = &params.comp;
    let on = comp.enabled.value();
    let rect = Rect::from_min_size(origin, Vec2::new(EDITOR_WIDTH as f32 - 2.0 * layout::MARGIN, layout::COMP_H));
    let x = |dx: f32| rect.left() + dx;
    let y = |dy: f32| rect.top() + dy;

    // Pannello: ombra, metallo sfumato, spazzolatura, bordo, "orecchie" del rack.
    painter.rect_filled(rect.translate(Vec2::new(0.0, 3.0)), CornerRadius::same(6), Color32::from_black_alpha(120));
    painter.rect_filled(rect, CornerRadius::same(6), PLATE_BOTTOM);
    vertical_gradient(painter, rect.shrink(2.0), PLATE_TOP, PLATE_BOTTOM);
    let mut line_y = rect.top() + 4.0;
    let mut i = 0;
    while line_y < rect.bottom() - 3.0 {
        let alpha = if i % 3 == 0 { 9 } else { 5 };
        painter.line_segment(
            [Pos2::new(rect.left() + 20.0, line_y), Pos2::new(rect.right() - 20.0, line_y)],
            Stroke::new(1.0_f32, Color32::from_white_alpha(alpha)),
        );
        line_y += 2.0;
        i += 1;
    }
    painter.rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0_f32, PLATE_EDGE), StrokeKind::Inside);
    for ear in [
        Rect::from_min_max(rect.min, Pos2::new(x(18.0), rect.bottom())),
        Rect::from_min_max(Pos2::new(rect.right() - 18.0, rect.top()), rect.max),
    ] {
        painter.rect_filled(ear.shrink(1.0), CornerRadius::same(5), EAR);
        screw(painter, Pos2::new(ear.center().x, rect.top() + 18.0), 0.6);
        screw(painter, Pos2::new(ear.center().x, rect.bottom() - 18.0), 2.1);
    }

    // Strumento ad ago. Si muove con calma, come quelli veri (~300 ms).
    let reading = if on { display.gain_reduction() } else { 0.0 };
    state.meter_db += (reading - state.meter_db) * 0.3;
    let meter = Rect::from_min_max(Pos2::new(x(34.0), y(14.0)), Pos2::new(x(190.0), y(118.0)));
    ui.interact(meter, Id::new("compressore-indicatore"), Sense::hover())
        .on_hover_text(format!(
            "Di quanti dB il compressore sta abbassando la voce in questo momento: {:.1} dB.",
            state.meter_db
        ));
    gain_reduction_meter(painter, meter, state.meter_db, on);
    engraved(painter, Pos2::new(meter.center().x, y(136.0)), Align2::CENTER_CENTER, "COMPRESSORE A VALVOLA", bold(9.0), ENGRAVING);

    // Separatori incisi.
    for sx in [204.0, 314.0] {
        painter.line_segment([Pos2::new(x(sx), y(14.0)), Pos2::new(x(sx), y(136.0))], Stroke::new(1.0_f32, Color32::from_black_alpha(140)));
        painter.line_segment([Pos2::new(x(sx + 1.0), y(14.0)), Pos2::new(x(sx + 1.0), y(136.0))], Stroke::new(1.0_f32, Color32::from_white_alpha(14)));
    }

    // Interruttore a levetta con spia.
    let toggle = Pos2::new(x(236.0), y(48.0));
    let lamp = Pos2::new(x(282.0), y(48.0));
    let switch_hit = Rect::from_center_size(Pos2::new(x(258.0), y(48.0)), Vec2::new(84.0, 64.0));
    let response = ui
        .interact(switch_hit, Id::new("compressore-acceso"), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(
            "Accende il compressore ottico a valvola, dopo la correzione: \
             tiene la voce a un volume costante e le dà calore e presenza.",
        );
    if response.clicked() {
        set_param(setter, &comp.enabled, !on);
    }
    engraved(painter, toggle - Vec2::new(0.0, 30.0), Align2::CENTER_CENTER, "ACCESO", bold(8.0), if on { ENGRAVING } else { ENGRAVING_DIM });
    engraved(painter, toggle + Vec2::new(0.0, 30.0), Align2::CENTER_CENTER, "SPENTO", bold(8.0), if on { ENGRAVING_DIM } else { ENGRAVING });
    painter.circle_filled(toggle, 10.0, Color32::from_rgb(0x8a, 0x86, 0x7e));
    painter.circle_stroke(toggle, 10.0, Stroke::new(1.0_f32, Color32::from_rgb(0x2a, 0x28, 0x25)));
    painter.circle_filled(toggle, 6.0, Color32::from_rgb(0x3a, 0x38, 0x34));
    let tip = toggle + Vec2::new(0.0, if on { -18.0 } else { 18.0 });
    painter.line_segment([toggle + Vec2::new(1.5, 2.0), tip + Vec2::new(1.5, 2.0)], Stroke::new(6.0_f32, Color32::from_black_alpha(90)));
    painter.line_segment([toggle, tip], Stroke::new(5.0_f32, Color32::from_rgb(0xc9, 0xc4, 0xb8)));
    painter.circle_filled(tip, 4.0, if response.hovered() { GOLD } else { Color32::from_rgb(0xe4, 0xdf, 0xd3) });
    // Spia: con un alone quando e' accesa.
    if on {
        for (r, a) in [(16.0, 18u8), (12.0, 35), (9.0, 60)] {
            painter.circle_filled(lamp, r, Color32::from_rgba_unmultiplied(255, 170, 60, a));
        }
    }
    painter.circle_filled(lamp, 7.0, if on { LAMP_ON } else { LAMP_OFF });
    painter.circle_stroke(lamp, 7.5, Stroke::new(1.5_f32, Color32::from_rgb(0x6d, 0x69, 0x62)));
    painter.circle_filled(lamp - Vec2::new(2.0, 2.0), 2.2, Color32::from_white_alpha(if on { 160 } else { 40 }));

    // Modalita': tre posizioni con il loro led.
    engraved(painter, Pos2::new(x(258.0), y(92.0)), Align2::CENTER_CENTER, "MODALITÀ", bold(8.0), ENGRAVING);
    let current = comp.mode.value();
    for (row, (choice, name)) in [
        (CompModeChoice::Fixed, "OTTICA"),
        (CompModeChoice::Manual, "MANUALE"),
        (CompModeChoice::FixedManual, "OTT.+MAN."),
    ]
    .into_iter()
    .enumerate()
    {
        let row_y = y(108.0 + 14.0 * row as f32);
        let hit = Rect::from_min_max(Pos2::new(x(216.0), row_y - 7.0), Pos2::new(x(304.0), row_y + 7.0));
        let r = ui
            .interact(hit, Id::new(("compressore-modalita", row)), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(
                "Ottica: attacco e rilascio automatici, che seguono la voce come una vera \
                 cella ottica (rilascio più lento dopo le frasi lunghe).\n\
                 Manuale: attacco e rilascio esattamente come le manopole.\n\
                 Ottica + manuale: entrambi insieme.",
            );
        if r.clicked() && choice != current {
            set_param(setter, &comp.mode, choice);
        }
        let selected = choice == current;
        let led = Pos2::new(x(224.0), row_y);
        if selected {
            painter.circle_filled(led, 6.0, Color32::from_rgba_unmultiplied(255, 170, 60, 40));
        }
        painter.circle_filled(led, 3.5, if selected { LAMP_ON } else { LAMP_OFF });
        engraved(
            painter,
            Pos2::new(x(234.0), row_y),
            Align2::LEFT_CENTER,
            name,
            regular(8.5),
            if selected || r.hovered() { ENGRAVING } else { ENGRAVING_DIM },
        );
    }

    // Manopole.
    let column = |i: usize| x(362.0 + 80.0 * i as f32);
    let knob_y = y(72.0);
    let auto = current == CompModeChoice::Fixed;
    let auto_text = || if auto { Some("AUTO".to_owned()) } else { None };
    hardware_knob(ui, painter, setter, &comp.threshold, Pos2::new(column(0), knob_y), "SOGLIA", ("-50", "0"),
        "Il livello oltre il quale la voce viene compressa. Più in basso = più compressione.", None);
    hardware_knob(ui, painter, setter, &comp.ratio, Pos2::new(column(1), knob_y), "RAPPORTO", ("2", "10"),
        "Quanto comprime oltre la soglia: 2:1 delicato, 4:1 voce pop, 10:1 molto controllato.", None);
    hardware_knob(ui, painter, setter, &comp.attack, Pos2::new(column(2), knob_y), "ATTACCO", ("0.5", "300"),
        "Quanto in fretta reagisce (modalità Manuale e Ott.+Man.). Valori bassi domano i picchi, \
         valori alti lasciano passare l'attacco delle consonanti. In modalità Ottica è automatico.", auto_text());
    hardware_knob(ui, painter, setter, &comp.release, Pos2::new(column(3), knob_y), "RILASCIO", (".05", "10s"),
        "Quanto in fretta torna a riposo (modalità Manuale e Ott.+Man.). In modalità Ottica è automatico.", auto_text());
    hardware_knob(ui, painter, setter, &comp.tube, Pos2::new(column(4), knob_y), "VALVOLA", ("0", "100"),
        "Il calore della valvola e dei trasformatori: fino a ~30% è un colore discreto, \
         oltre diventa saturazione evidente. Senza aliasing grazie al sovracampionamento 4×.", None);
    hardware_knob(ui, painter, setter, &comp.mix, Pos2::new(column(5), knob_y), "MIX", ("0", "100"),
        "Compressione parallela: miscela tra la voce originale e quella compressa.", None);
    hardware_knob(ui, painter, setter, &comp.output, Pos2::new(column(6), knob_y), "USCITA", ("-20", "+20"),
        "Volume in uscita, per recuperare quello tolto dalla compressione.", None);
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
    // Le etichette non devono sovrapporsi: se le righe sono fitte (scala
    // cromatica) scriviamo solo le note naturali, distanziate di almeno 13 px.
    let notes = params.active_notes();
    let row_spacing = plot.height() / (high - low);
    let mut last_label_y = f32::INFINITY;
    for note in low.ceil() as i32..=high.floor() as i32 {
        let y = y_of(note as f32);
        let in_scale = notes[pitch_class(note)];
        let color = if in_scale { Color32::from_rgb(0x30, 0x34, 0x3f) } else { Color32::from_rgb(0x1f, 0x22, 0x29) };
        painter.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0_f32, color));
        let natural = matches!(pitch_class(note), 0 | 2 | 4 | 5 | 7 | 9 | 11);
        let readable = row_spacing >= 13.0 || natural;
        if in_scale && readable && last_label_y - y >= 13.0 {
            painter.text(Pos2::new(rect.left() + 14.0, y), Align2::LEFT_CENTER, note_label(note), regular(10.0), TEXT_FAINT);
            last_label_y = y;
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
