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
//! └──────────────────────────────────────────────────────────────────────────┘
//! ```

use crate::params::{CompModeChoice, ScaleChoice, SosaTuneParams, EDITOR_HEIGHT, EDITOR_WIDTH};
use crate::shared::PitchDisplay;
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
    pub const MARGIN: f32 = 16.0;
    pub const BIG_KNOB: f32 = 30.0;
    pub const SMALL_KNOB: f32 = 20.0;
}

/// Angolo in alto a sinistra del pannello del compressore (per le prove).
#[cfg(test)]
pub(crate) const COMPRESSOR_ORIGIN: (f32, f32) = (layout::MARGIN, layout::COMP_TOP);

/// Registro diagnostico: scrive in
/// `~/Library/Logs/DiagnosticReports/SosaTune-diagnostica.log` cosa succede
/// quando si usano i comandi (mouse, manopole, valori, contatori del wrapper),
/// una riga al secondo e solo quando qualcosa cambia. Serve a capire, su un
/// computer reale, dove si perde un comando. Non tocca mai l'audio.
mod diagnostics {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    pub static DRAG_STARTS: AtomicU64 = AtomicU64::new(0);
    pub static DRAG_SETS: AtomicU64 = AtomicU64::new(0);
    pub static CLICK_SETS: AtomicU64 = AtomicU64::new(0);
    const MAX_LINES: usize = 3000;

    pub struct Diagnostics {
        enabled: bool,
        opened: Option<Instant>,
        last_check: Option<Instant>,
        last_line: String,
        lines: usize,
        moves: u64,
        presses: u64,
        releases: u64,
        frames: u64,
    }

    impl Diagnostics {
        pub fn new(enabled: bool) -> Self {
            Self {
                enabled,
                opened: None,
                last_check: None,
                last_line: String::new(),
                lines: 0,
                moves: 0,
                presses: 0,
                releases: 0,
                frames: 0,
            }
        }

        fn write(&mut self, text: &str) {
            if self.lines >= MAX_LINES {
                return;
            }
            let Some(home) = std::env::var_os("HOME") else { return };
            let path = std::path::Path::new(&home).join("Library/Logs/DiagnosticReports/SosaTune-diagnostica.log");
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let seconds = self.opened.map_or(0.0, |t| t.elapsed().as_secs_f32());
                let _ = writeln!(file, "[{seconds:7.1} s] {text}");
                self.lines += 1;
            }
        }

        pub fn observe(&mut self, ctx: &egui::Context, params: &SosaTuneParams) {
            if !self.enabled {
                return;
            }
            if self.opened.is_none() {
                self.opened = Some(Instant::now());
                let unix = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                self.write(&format!(
                    "=== SosaTune {} - finestra aperta (unix {unix}) ===",
                    env!("CARGO_PKG_VERSION")
                ));
            }
            self.frames += 1;
            ctx.input(|i| {
                for event in &i.raw.events {
                    match event {
                        egui::Event::PointerMoved(_) => self.moves += 1,
                        egui::Event::PointerButton { pressed: true, .. } => self.presses += 1,
                        egui::Event::PointerButton { pressed: false, .. } => self.releases += 1,
                        _ => {}
                    }
                }
            });
            if self.last_check.is_some_and(|t| t.elapsed().as_secs_f32() < 1.0) {
                return;
            }
            self.last_check = Some(Instant::now());
            let screen = ctx.screen_rect();
            let line = format!(
                "finestra {:.0}x{:.0} ppp={:.2} | mouse: mossi={} premuti={} rilasciati={} sopra={} trascina={} | manopole: inizio={} valori={} clic={} | parametri: velocita={:.3} soglia={:.3} comp={} | {}",
                screen.width(),
                screen.height(),
                ctx.pixels_per_point(),
                self.moves,
                self.presses,
                self.releases,
                ctx.is_pointer_over_area(),
                ctx.dragged_id().is_some(),
                DRAG_STARTS.load(Ordering::Relaxed),
                DRAG_SETS.load(Ordering::Relaxed),
                CLICK_SETS.load(Ordering::Relaxed),
                params.retune_speed.unmodulated_normalized_value(),
                params.comp.threshold.unmodulated_normalized_value(),
                params.comp.enabled.value(),
                nih_plug::sosatune_diag::summary(),
            );
            if line != self.last_line {
                self.write(&format!("{line} | fotogrammi={}", self.frames));
                self.last_line = line;
            }
        }
    }
}

/// Stato che appartiene solo alla grafica (non viene salvato).
struct GuiState {
    /// Registro diagnostico (acceso solo nella finestra vera, non nelle prove).
    diagnostics: diagnostics::Diagnostics,
    /// Indicatore della compressione: sale subito, scende con calma.
    meter_db: f32,
    /// Indicatori IN/OUT (dB) e controllo che il DAW mandi audio.
    level_in_db: f32,
    level_out_db: f32,
    last_blocks: usize,
    last_audio_time: f64,
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
        new_state(true),
        |ctx, _state| setup(ctx),
        move |ctx, setter, state| draw(ctx, &params, &display, setter, state),
    )
}

fn new_state(diagnostics_enabled: bool) -> GuiState {
    GuiState {
        diagnostics: diagnostics::Diagnostics::new(diagnostics_enabled),
        meter_db: 0.0,
        level_in_db: -90.0,
        level_out_db: -90.0,
        last_blocks: 0,
        last_audio_time: f64::NEG_INFINITY,
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
    state.diagnostics.observe(ctx, params);
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
            vertical_gradient(
                &painter,
                Rect::from_min_size(origin, Vec2::new(EDITOR_WIDTH as f32, EDITOR_HEIGHT as f32)),
                Color32::from_rgb(0x13, 0x14, 0x19),
                Color32::from_rgb(0x09, 0x0a, 0x0d),
            );

            header(ui, &painter, params, setter, at(0.0, 0.0));
            signal_status(ui, &painter, display, state, at(318.0, 0.0));
            tuning_ring(ui, &painter, display, at(layout::RING_X, layout::ROW_TOP));
            knobs(ui, &painter, params, setter, at(layout::KNOBS_X, layout::ROW_TOP));
            selectors(ui, &painter, params, setter, display, at(layout::SELECT_X, layout::ROW_TOP));
            keyboard(ui, &painter, params, setter, display, at(layout::MARGIN, layout::KEYS_TOP));
            compressor(ui, &painter, params, setter, display, state, at(layout::MARGIN, layout::COMP_TOP));
        });
}

/// Per l'anteprima (vedi `preview.rs`): la stessa interfaccia, senza finestra.
pub(crate) struct Headless {
    state: GuiState,
}

impl Headless {
    pub(crate) fn new(ctx: &egui::Context) -> Self {
        setup(ctx);
        Self { state: new_state(false) }
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

/// Indicatori IN/OUT nell'intestazione e stato dell'audio: mostrano a colpo
/// d'occhio se il DAW sta davvero facendo passare la voce da SosaTune.
fn signal_status(ui: &Ui, painter: &Painter, display: &PitchDisplay, state: &mut GuiState, origin: Pos2) {
    let (input_peak, output_peak, blocks, bypassed) = display.block_status();
    let now = ui.input(|i| i.time);
    if blocks != state.last_blocks {
        state.last_blocks = blocks;
        state.last_audio_time = now;
    }
    let receiving = now - state.last_audio_time < 0.5;
    let to_db = |x: f32| if x > 1e-5 { 20.0 * x.log10() } else { -90.0 };
    let (target_in, target_out) = if receiving { (to_db(input_peak), to_db(output_peak)) } else { (-90.0, -90.0) };
    // Salgono subito, scendono con calma (come i misuratori veri).
    let follow = |level: &mut f32, target: f32| {
        *level = if target > *level { target } else { *level + (target - *level) * 0.12 };
    };
    follow(&mut state.level_in_db, target_in);
    follow(&mut state.level_out_db, target_out);

    let y = origin.y + layout::HEADER_H / 2.0;
    let bar_left = origin.x + 34.0;
    let bar_w = 110.0;
    for (row, (label, db)) in [("IN", state.level_in_db), ("OUT", state.level_out_db)].into_iter().enumerate() {
        let row_y = y - 8.0 + 16.0 * row as f32;
        engraved(painter, Pos2::new(origin.x + 26.0, row_y), Align2::RIGHT_CENTER, label, bold(8.0), LEGEND_DIM);
        let track = Rect::from_min_size(Pos2::new(bar_left, row_y - 3.5), Vec2::new(bar_w, 7.0));
        painter.rect_filled(track, CornerRadius::same(3), DISPLAY_BG);
        painter.rect_stroke(track, CornerRadius::same(3), Stroke::new(1.0_f32, Color32::from_black_alpha(200)), StrokeKind::Inside);
        let fraction = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
        if fraction > 0.0 {
            let lit = Rect::from_min_size(track.min, Vec2::new(bar_w * fraction, track.height())).shrink(1.0);
            let color = if db > -1.0 { METER_RED } else if db > -12.0 { AMBER } else { MINT };
            painter.rect_filled(lit, CornerRadius::same(2), color);
        }
    }
    let (text, color) = if bypassed && receiving {
        ("BYPASS dal DAW", AMBER)
    } else if !receiving {
        ("nessun audio dal DAW", AMBER)
    } else if input_peak < 1e-4 {
        ("silenzio in ingresso", LEGEND_DIM)
    } else {
        ("audio OK", MINT)
    };
    let status = Rect::from_min_size(Pos2::new(bar_left + bar_w + 10.0, y - 10.0), Vec2::new(150.0, 20.0));
    ui.interact(status.union(Rect::from_min_size(origin, Vec2::new(bar_w + 40.0, layout::HEADER_H))), Id::new("stato-audio"), Sense::hover())
        .on_hover_text(
            "IN: la voce che arriva dal DAW. OUT: la voce dopo SosaTune.\n\
             \"nessun audio dal DAW\": il programma non sta facendo passare l'audio da SosaTune \
             (riproduzione ferma, plugin in uno slot non attivo o traccia sbagliata).\n\
             \"BYPASS dal DAW\": il DAW ha messo il plugin in bypass, la voce passa senza correzione.",
        );
    painter.circle_filled(Pos2::new(status.left() + 5.0, y), 3.5, color);
    painter.text(Pos2::new(status.left() + 14.0, y), Align2::LEFT_CENTER, text, regular(10.0), color);
}

/// Un pannello: piastra in grafite sfumata, con ombra, filo di luce in alto
/// e titolo inciso (lo stesso materiale del compressore).
fn panel(painter: &Painter, rect: Rect, title: &str) {
    painter.rect_filled(rect.translate(Vec2::new(0.0, 4.0)), CornerRadius::same(12), Color32::from_black_alpha(110));
    painter.rect_filled(rect, CornerRadius::same(12), PLATE_BOTTOM);
    vertical_gradient(painter, rect.shrink(4.0), PLATE_TOP, PLATE_BOTTOM);
    painter.rect_stroke(rect, CornerRadius::same(12), Stroke::new(1.0_f32, PLATE_EDGE), StrokeKind::Inside);
    painter.line_segment(
        [Pos2::new(rect.left() + 14.0, rect.top() + 1.5), Pos2::new(rect.right() - 14.0, rect.top() + 1.5)],
        Stroke::new(1.0_f32, Color32::from_white_alpha(12)),
    );
    if !title.is_empty() {
        engraved(painter, rect.left_top() + Vec2::new(18.0, 19.0), Align2::LEFT_CENTER, title, bold(10.5), LEGEND);
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
    diagnostics::CLICK_SETS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
    vertical_gradient(painter, bar, Color32::from_rgb(0x1d, 0x1f, 0x25), Color32::from_rgb(0x11, 0x12, 0x16));
    painter.line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0_f32, Color32::from_black_alpha(200)));
    painter.line_segment(
        [bar.left_bottom() + Vec2::new(0.0, 1.0), bar.right_bottom() + Vec2::new(0.0, 1.0)],
        Stroke::new(1.0_f32, Color32::from_white_alpha(10)),
    );

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
        let width = 22.0 + name.len() as f32 * 7.6;
        let chip = Rect::from_min_max(Pos2::new(right - width, y - 12.0), Pos2::new(right, y + 12.0));
        right -= width + 8.0;

        let active = (params.retune_speed.value() - retune).abs() < 0.5
            && (params.humanize.value() - humanize).abs() < 0.01
            && (params.flex_tune.value() - flex).abs() < 0.01
            && (params.vibrato.value() - vibrato).abs() < 0.01
            && (params.amount.value() - amount).abs() < 0.01;
        let response = ui.interact(chip, Id::new(("stile", *name)), Sense::click()).on_hover_text(*help);
        hardware_key(painter, chip, name, active, response.hovered(), 10.5);

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
        diagnostics::DRAG_STARTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
        diagnostics::DRAG_SETS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
    // Stesso modello di manopola del compressore, nelle due misure.
    let size = if radius > 25.0 { 24.0 } else { 18.0 };
    hardware_knob(ui, painter, setter, param, center, label, ("", ""), help, bipolar, false, size);
}

fn knobs(ui: &Ui, painter: &Painter, params: &SosaTuneParams, setter: &ParamSetter, origin: Pos2) {
    let rect = Rect::from_min_size(origin, Vec2::new(layout::KNOBS_W, layout::ROW_H));
    panel(painter, rect, "CORREZIONE");
    let column = |i: usize| origin.x + 46.0 + i as f32 * 91.0;
    let big_y = origin.y + 96.0;
    let small_y = origin.y + 205.0;
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
    engraved(painter, rect.left_top() - Vec2::new(0.0, 10.0), Align2::LEFT_CENTER, title, bold(9.0), LEGEND);

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
    painter.rect_filled(rect.translate(Vec2::new(0.0, 1.0)), CornerRadius::same(7), Color32::from_white_alpha(10));
    painter.rect_filled(rect, CornerRadius::same(7), DISPLAY_BG);
    painter.rect_stroke(
        rect,
        CornerRadius::same(7),
        Stroke::new(1.0_f32, if hovered { GOLD.gamma_multiply(0.6) } else { Color32::from_black_alpha(220) }),
        StrokeKind::Inside,
    );
    for (button, r) in [(left.shrink(4.0), &r_left), (right.shrink(4.0), &r_right)] {
        let (top, bottom) = if r.hovered() {
            (Color32::from_rgb(0x3a, 0x3d, 0x46), Color32::from_rgb(0x26, 0x28, 0x2f))
        } else {
            (Color32::from_rgb(0x2c, 0x2f, 0x37), Color32::from_rgb(0x1c, 0x1e, 0x23))
        };
        vertical_gradient(painter, button, top, bottom);
        painter.rect_stroke(button, CornerRadius::same(4), Stroke::new(1.0_f32, Color32::from_black_alpha(170)), StrokeKind::Inside);
    }
    // Se il nome e' lungo, il carattere si rimpicciolisce per stare tra le frecce.
    let available = (middle.width() - 8.0).max(10.0);
    let wide = painter.layout_no_wrap(names[current].to_owned(), regular(14.0), TEXT).size().x;
    let size = if wide > available { (14.0 * available / wide).max(10.0) } else { 14.0 };
    painter.text(middle.center(), Align2::CENTER_CENTER, names[current], regular(size), AMBER);

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
// --- Compressore: pannello "hardware" nella palette di SosaTune ----------------
//
// Un apparecchio da studio immaginario, disegnato per SosaTune: pannello in
// grafite, strumento ad ago retroilluminato, pulsante di accensione luminoso,
// tasti retroilluminati per la modalita', manopole godronate con l'arco dorato
// del valore e i numeri in piccoli display incassati.

/// Posizioni dei comandi del compressore, relative all'angolo del pannello
/// (usate dal disegno e dalle prove dell'interfaccia).
pub(crate) mod comp_layout {
    /// Centro del pulsante di accensione.
    pub const POWER: (f32, f32) = (266.0, 46.0);
    /// Centro del tasto di ogni modalita' (riga 0, 1, 2).
    pub const fn mode(row: usize) -> (f32, f32) {
        (266.0, 96.0 + 19.0 * row as f32)
    }
    /// Centro di ogni manopola (0 = soglia ... 6 = uscita).
    pub const fn knob(i: usize) -> (f32, f32) {
        (378.0 + 78.0 * i as f32, 62.0)
    }
    pub const KNOB_RADIUS: f32 = 22.0;
}

const PLATE_TOP: Color32 = Color32::from_rgb(0x25, 0x27, 0x2e);
const PLATE_BOTTOM: Color32 = Color32::from_rgb(0x16, 0x17, 0x1c);
const PLATE_EDGE: Color32 = Color32::from_rgb(0x33, 0x36, 0x40);
const LEGEND: Color32 = Color32::from_rgb(0xc9, 0xcc, 0xd6);
const LEGEND_DIM: Color32 = Color32::from_rgb(0x6c, 0x71, 0x7e);
const METER_LIGHT: Color32 = Color32::from_rgb(0xfb, 0xed, 0xc9);
const METER_EDGE: Color32 = Color32::from_rgb(0xd9, 0xc2, 0x8c);
const METER_INK: Color32 = Color32::from_rgb(0x2a, 0x22, 0x18);
const METER_RED: Color32 = Color32::from_rgb(0xc2, 0x3b, 0x26);
const AMBER: Color32 = Color32::from_rgb(0xff, 0xb4, 0x3c);
const DISPLAY_BG: Color32 = Color32::from_rgb(0x0b, 0x0c, 0x0f);

/// Testo inciso: un filo d'ombra sotto e il testo sopra.
fn engraved(painter: &Painter, pos: Pos2, align: Align2, text: &str, font: FontId, color: Color32) {
    painter.text(pos + Vec2::new(0.0, 1.0), align, text, font.clone(), Color32::from_black_alpha(120));
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

/// Un disco con sfumatura dal punto `light` (in genere in alto a sinistra,
/// da dove arriva la luce) verso il bordo.
fn shaded_disc(painter: &Painter, center: Pos2, radius: f32, light: Vec2, inner: Color32, outer: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(center + light, inner);
    let segments = 64;
    for i in 0..=segments {
        let a = i as f32 / segments as f32 * 2.0 * PI;
        mesh.colored_vertex(center + Vec2::new(a.cos(), a.sin()) * radius, outer);
    }
    for i in 0..segments {
        mesh.add_triangle(0, i + 1, i + 2);
    }
    painter.add(Shape::mesh(mesh));
}

/// Una vite a croce incassata.
fn screw(painter: &Painter, center: Pos2) {
    painter.circle_filled(center, 4.5, Color32::from_black_alpha(140));
    shaded_disc(painter, center, 3.8, Vec2::new(-1.2, -1.2), Color32::from_rgb(0x8a, 0x8d, 0x96), Color32::from_rgb(0x3a, 0x3d, 0x45));
    let s = Stroke::new(1.0_f32, Color32::from_rgb(0x1e, 0x20, 0x25));
    painter.line_segment([center - Vec2::new(2.2, 0.0), center + Vec2::new(2.2, 0.0)], s);
    painter.line_segment([center - Vec2::new(0.0, 2.2), center + Vec2::new(0.0, 2.2)], s);
}

/// Display incassato con il valore.
fn value_display(painter: &Painter, center: Pos2, text: &str, color: Color32) {
    let rect = Rect::from_center_size(center, Vec2::new(66.0, 18.0));
    painter.rect_filled(rect.translate(Vec2::new(0.0, 1.0)), CornerRadius::same(5), Color32::from_white_alpha(10));
    painter.rect_filled(rect, CornerRadius::same(5), DISPLAY_BG);
    painter.rect_stroke(rect, CornerRadius::same(5), Stroke::new(1.0_f32, Color32::from_black_alpha(200)), StrokeKind::Inside);
    painter.text(center, Align2::CENTER_CENTER, text, regular(10.5), color);
}

/// Manopola godronata con scala, arco del valore e display.
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
    bipolar: bool,
    auto: bool,
    radius: f32,
) {
    let hit = Rect::from_center_size(center, Vec2::splat(2.0 * radius + 18.0));
    let response = drag_param(ui, setter, param, hit, label, help);
    let active = response.hovered() || response.dragged();

    let start = 0.75 * PI;
    let sweep = 1.5 * PI;
    let value = param.modulated_normalized_value();
    let angle = start + value * sweep;

    // Scala: tacche fini, piu' lunghe agli estremi e al centro.
    for i in 0..=20 {
        let a = start + sweep * i as f32 / 20.0;
        let major = i % 5 == 0;
        painter.line_segment(
            [polar(center, radius + 6.0, a), polar(center, radius + if major { 10.0 } else { 8.0 }, a)],
            Stroke::new(if major { 1.3_f32 } else { 0.8_f32 }, if major { LEGEND_DIM } else { TRACK }),
        );
    }
    // Arco dorato del valore (dal centro per i comandi +/-).
    let from = if bipolar { 1.5 * PI } else { start };
    let gold = if auto { GOLD.gamma_multiply(0.35) } else { GOLD };
    if (angle - from).abs() > 0.01 {
        arc(painter, center, radius + 4.0, from.min(angle), from.max(angle), Stroke::new(2.0_f32, gold));
    }
    if !ends.0.is_empty() {
        engraved(painter, polar(center, radius + 13.0, start) + Vec2::new(-1.0, 6.0), Align2::CENTER_CENTER, ends.0, regular(7.5), LEGEND_DIM);
        engraved(painter, polar(center, radius + 13.0, start + sweep) + Vec2::new(1.0, 6.0), Align2::CENTER_CENTER, ends.1, regular(7.5), LEGEND_DIM);
    }

    // Ombra portata, gonna godronata, cappuccio lucido, indice dorato.
    painter.circle_filled(center + Vec2::new(0.0, 3.0), radius + 1.5, Color32::from_black_alpha(130));
    shaded_disc(painter, center, radius, Vec2::new(-6.0, -8.0), Color32::from_rgb(0x4a, 0x4d, 0x57), Color32::from_rgb(0x16, 0x17, 0x1b));
    for i in 0..56 {
        let a = i as f32 / 56.0 * 2.0 * PI + angle;
        let light = (a - 1.25 * PI).cos() * 0.5 + 0.5; // piu' chiara in alto a sinistra
        let c = Color32::from_white_alpha((10.0 + 30.0 * light) as u8);
        painter.line_segment([polar(center, radius - 3.5, a), polar(center, radius - 0.5, a)], Stroke::new(1.0_f32, c));
    }
    painter.circle_stroke(center, radius, Stroke::new(1.0_f32, Color32::from_black_alpha(180)));
    let cap = radius * 0.64;
    shaded_disc(painter, center, cap, Vec2::new(-3.5, -5.0), Color32::from_rgb(0x5c, 0x60, 0x6b), Color32::from_rgb(0x24, 0x26, 0x2c));
    painter.circle_stroke(center, cap, Stroke::new(1.0_f32, Color32::from_black_alpha(150)));
    arc(painter, center, cap - 1.5, 1.1 * PI, 1.45 * PI, Stroke::new(1.5_f32, Color32::from_white_alpha(45)));
    let pointer = if active { Color32::from_rgb(0xff, 0xd3, 0x7a) } else { GOLD };
    painter.line_segment([polar(center, radius - 5.0, angle), polar(center, cap * 0.35, angle)], Stroke::new(2.4_f32, pointer));
    painter.circle_filled(polar(center, radius - 5.0, angle), 1.2, pointer);

    engraved(painter, center - Vec2::new(0.0, radius + 21.0), Align2::CENTER_CENTER, label, bold(9.5), if active { TEXT } else { LEGEND });
    let (text, color) = if auto {
        ("AUTO".to_owned(), LEGEND_DIM)
    } else {
        (param.normalized_value_to_string(value, true), if active { GOLD } else { AMBER.gamma_multiply(0.92) })
    };
    value_display(painter, center + Vec2::new(0.0, radius + 26.0), &text, color);
}

/// Tasto con retroilluminazione dorata quando e' scelto.
fn hardware_key(painter: &Painter, key: Rect, name: &str, selected: bool, hovered: bool, size: f32) {
    painter.rect_filled(key.translate(Vec2::new(0.0, 1.5)), CornerRadius::same(5), Color32::from_black_alpha(130));
    if selected {
        painter.rect_filled(key.expand(3.0), CornerRadius::same(7), Color32::from_rgba_unmultiplied(255, 175, 60, 22));
        vertical_gradient(painter, key.shrink(1.0), Color32::from_rgb(0xff, 0xc9, 0x62), Color32::from_rgb(0xd9, 0x92, 0x22));
        painter.rect_stroke(key, CornerRadius::same(5), Stroke::new(1.0_f32, Color32::from_rgb(0xff, 0xd8, 0x8a)), StrokeKind::Inside);
    } else {
        let (top, bottom) = if hovered {
            (Color32::from_rgb(0x3a, 0x3d, 0x46), Color32::from_rgb(0x26, 0x28, 0x2f))
        } else {
            (Color32::from_rgb(0x30, 0x33, 0x3b), Color32::from_rgb(0x1f, 0x21, 0x27))
        };
        vertical_gradient(painter, key.shrink(1.0), top, bottom);
        painter.rect_stroke(key, CornerRadius::same(5), Stroke::new(1.0_f32, Color32::from_black_alpha(170)), StrokeKind::Inside);
        painter.line_segment(
            [key.left_top() + Vec2::new(4.0, 1.5), key.right_top() + Vec2::new(-4.0, 1.5)],
            Stroke::new(1.0_f32, Color32::from_white_alpha(16)),
        );
    }
    painter.text(
        key.center(),
        Align2::CENTER_CENTER,
        name,
        bold(size),
        if selected { Color32::from_rgb(0x2a, 0x1a, 0x05) } else if hovered { TEXT } else { LEGEND },
    );
}

/// Strumento ad ago della riduzione di guadagno (da 0 a 20 dB).
fn gain_reduction_meter(painter: &Painter, rect: Rect, db: f32, lit: bool) {
    // Cornice metallica e vetro.
    let bezel = rect.expand(5.0);
    painter.rect_filled(bezel.translate(Vec2::new(0.0, 2.0)), CornerRadius::same(9), Color32::from_black_alpha(140));
    painter.rect_filled(bezel, CornerRadius::same(9), Color32::from_rgb(0x2c, 0x2e, 0x35));
    painter.rect_stroke(bezel, CornerRadius::same(9), Stroke::new(1.0_f32, Color32::from_rgb(0x4a, 0x4d, 0x57)), StrokeKind::Inside);
    painter.rect_filled(rect.expand(1.0), CornerRadius::same(5), Color32::BLACK);

    // Quadrante retroilluminato: piu' luminoso in basso al centro, dove sta la lampadina.
    let clipped = painter.with_clip_rect(rect);
    let (light, edge) = if lit { (METER_LIGHT, METER_EDGE) } else { (METER_LIGHT.gamma_multiply(0.45), METER_EDGE.gamma_multiply(0.4)) };
    clipped.rect_filled(rect, CornerRadius::same(4), edge);
    shaded_disc(&clipped, Pos2::new(rect.center().x, rect.bottom() + 10.0), rect.width() * 0.75, Vec2::ZERO, light, edge);

    let pivot = Pos2::new(rect.center().x, rect.bottom() + 30.0);
    let scale_radius = rect.height() - 4.0;
    let left = -0.31 * PI; // 20 dB
    let right = 0.31 * PI; // 0 dB
    let angle_of = |db: f32| {
        let p = (db.clamp(0.0, 20.0) / 20.0).powf(0.62);
        right + (left - right) * p
    };
    let dial = |a: f32, r: f32| pivot + Vec2::new(a.sin() * r, -a.cos() * r);
    let arc_r = scale_radius - 26.0;
    // Arco della scala, piu' spesso e rosso oltre 10 dB.
    let steps = 60;
    for i in 0..steps {
        let d0 = 20.0 * i as f32 / steps as f32;
        let d1 = 20.0 * (i + 1) as f32 / steps as f32;
        let red = d0 >= 10.0;
        clipped.line_segment(
            [dial(angle_of(d0), arc_r + if red { 1.5 } else { 0.0 }), dial(angle_of(d1), arc_r + if red { 1.5 } else { 0.0 })],
            Stroke::new(if red { 3.5_f32 } else { 1.2_f32 }, if red { METER_RED } else { METER_INK }),
        );
    }
    // Tacche secondarie e principali con i numeri.
    for i in 0..=20 {
        let a = angle_of(i as f32);
        clipped.line_segment([dial(a, arc_r), dial(a, arc_r + 4.0)], Stroke::new(0.8_f32, METER_INK.gamma_multiply(0.8)));
    }
    for (value, label) in [(0.0, "0"), (1.0, "1"), (2.0, "2"), (3.0, "3"), (5.0, "5"), (7.0, "7"), (10.0, "10"), (15.0, "15"), (20.0, "20")] {
        let a = angle_of(value);
        let color = if value >= 10.0 { METER_RED } else { METER_INK };
        clipped.line_segment([dial(a, arc_r), dial(a, arc_r + 8.0)], Stroke::new(1.4_f32, color));
        clipped.text(dial(a, arc_r + 16.0), Align2::CENTER_CENTER, label, bold(9.0), color);
    }
    clipped.text(Pos2::new(rect.center().x, rect.bottom() - 25.0), Align2::CENTER_CENTER, "dB COMPRESSIONE", bold(7.0), METER_INK.gamma_multiply(0.7));

    // Ago con ombra, poi il coperchio del perno.
    let a = angle_of(db);
    clipped.line_segment([dial(a, 30.0) + Vec2::new(2.0, 2.0), dial(a, scale_radius - 2.0) + Vec2::new(2.0, 2.0)], Stroke::new(1.8_f32, Color32::from_black_alpha(40)));
    clipped.line_segment([dial(a, 30.0), dial(a, scale_radius - 2.0)], Stroke::new(1.5_f32, Color32::from_rgb(0x1e, 0x17, 0x10)));
    shaded_disc(&clipped, Pos2::new(rect.center().x, rect.bottom() + 6.0), 13.0, Vec2::new(-3.0, -6.0), Color32::from_rgb(0x4a, 0x4d, 0x57), Color32::from_rgb(0x15, 0x16, 0x1a));

    // Riflesso del vetro, in diagonale.
    let mut glare = egui::Mesh::default();
    glare.colored_vertex(rect.left_top(), Color32::from_white_alpha(38));
    glare.colored_vertex(Pos2::new(rect.left() + rect.width() * 0.55, rect.top()), Color32::from_white_alpha(0));
    glare.colored_vertex(Pos2::new(rect.left(), rect.top() + rect.height() * 0.7), Color32::from_white_alpha(0));
    glare.add_triangle(0, 1, 2);
    clipped.add(Shape::mesh(glare));
    clipped.rect_stroke(rect, CornerRadius::same(4), Stroke::new(2.0_f32, Color32::from_black_alpha(60)), StrokeKind::Inside);
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
    let at = |p: (f32, f32)| rect.min + Vec2::new(p.0, p.1);
    let x = |dx: f32| rect.left() + dx;
    let y = |dy: f32| rect.top() + dy;

    // Pannello: ombra, grafite sfumata, filo di luce in alto, viti.
    painter.rect_filled(rect.translate(Vec2::new(0.0, 4.0)), CornerRadius::same(12), Color32::from_black_alpha(110));
    painter.rect_filled(rect, CornerRadius::same(12), PLATE_BOTTOM);
    vertical_gradient(painter, rect.shrink(4.0), PLATE_TOP, PLATE_BOTTOM);
    painter.rect_stroke(rect, CornerRadius::same(12), Stroke::new(1.0_f32, PLATE_EDGE), StrokeKind::Inside);
    painter.line_segment(
        [Pos2::new(rect.left() + 14.0, rect.top() + 1.5), Pos2::new(rect.right() - 14.0, rect.top() + 1.5)],
        Stroke::new(1.0_f32, Color32::from_white_alpha(12)),
    );
    for corner in [
        Pos2::new(x(11.0), y(11.0)),
        Pos2::new(rect.right() - 11.0, y(11.0)),
        Pos2::new(x(11.0), rect.bottom() - 11.0),
        Pos2::new(rect.right() - 11.0, rect.bottom() - 11.0),
    ] {
        screw(painter, corner);
    }

    // Strumento ad ago: si muove con calma, come quelli veri (~300 ms).
    let reading = if on { display.gain_reduction() } else { 0.0 };
    state.meter_db += (reading - state.meter_db) * 0.3;
    let meter = Rect::from_min_max(Pos2::new(x(30.0), y(16.0)), Pos2::new(x(200.0), y(116.0)));
    ui.interact(meter, Id::new("compressore-indicatore"), Sense::hover())
        .on_hover_text(format!(
            "Di quanti dB il compressore sta abbassando la voce in questo momento: {:.1} dB.",
            state.meter_db
        ));
    gain_reduction_meter(painter, meter, state.meter_db, on);
    engraved(painter, Pos2::new(meter.center().x, y(136.0)), Align2::CENTER_CENTER, "COMPRESSORE A VALVOLA", bold(9.5), GOLD);

    // Separatori incisi.
    for sx in [218.0, 316.0] {
        painter.line_segment([Pos2::new(x(sx), y(18.0)), Pos2::new(x(sx), y(132.0))], Stroke::new(1.0_f32, Color32::from_black_alpha(150)));
        painter.line_segment([Pos2::new(x(sx + 1.0), y(18.0)), Pos2::new(x(sx + 1.0), y(132.0))], Stroke::new(1.0_f32, Color32::from_white_alpha(12)));
    }

    // Pulsante di accensione luminoso.
    let power = at(comp_layout::POWER);
    let response = ui
        .interact(Rect::from_center_size(power, Vec2::splat(46.0)), Id::new("compressore-acceso"), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(
            "Accende il compressore ottico a valvola, dopo la correzione: \
             tiene la voce a un volume costante e le dà calore e presenza.",
        );
    if response.clicked() {
        set_param(setter, &comp.enabled, !on);
    }
    if on {
        for (r, a) in [(28.0, 14u8), (24.0, 26), (21.0, 44)] {
            painter.circle_filled(power, r, Color32::from_rgba_unmultiplied(255, 175, 60, a));
        }
    }
    painter.circle_filled(power + Vec2::new(0.0, 2.5), 19.0, Color32::from_black_alpha(140));
    shaded_disc(painter, power, 19.0, Vec2::new(-4.0, -6.0), Color32::from_rgb(0x55, 0x58, 0x62), Color32::from_rgb(0x1a, 0x1b, 0x20));
    let (lens_in, lens_out) = if on {
        (Color32::from_rgb(0xff, 0xe2, 0xa0), AMBER)
    } else if response.hovered() {
        (Color32::from_rgb(0x5a, 0x4a, 0x30), Color32::from_rgb(0x2e, 0x26, 0x1a))
    } else {
        (Color32::from_rgb(0x46, 0x3a, 0x28), Color32::from_rgb(0x22, 0x1c, 0x14))
    };
    shaded_disc(painter, power, 13.0, Vec2::new(-2.0, -3.0), lens_in, lens_out);
    painter.circle_stroke(power, 13.0, Stroke::new(1.0_f32, Color32::from_black_alpha(160)));
    // Simbolo di accensione.
    let symbol = if on { Color32::from_rgb(0x5a, 0x34, 0x06) } else { LEGEND_DIM };
    arc(painter, power, 6.0, -0.32 * PI, 1.32 * PI, Stroke::new(1.8_f32, symbol));
    painter.line_segment([power - Vec2::new(0.0, 8.5), power - Vec2::new(0.0, 2.5)], Stroke::new(1.8_f32, symbol));
    engraved(painter, power + Vec2::new(0.0, 30.0), Align2::CENTER_CENTER, if on { "ACCESO" } else { "SPENTO" }, bold(8.5), if on { GOLD } else { LEGEND_DIM });

    // Modalita': tre tasti retroilluminati.
    let current = comp.mode.value();
    for (row, (choice, name)) in [
        (CompModeChoice::Fixed, "OTTICA"),
        (CompModeChoice::Manual, "MANUALE"),
        (CompModeChoice::FixedManual, "OTT. + MAN."),
    ]
    .into_iter()
    .enumerate()
    {
        let key = Rect::from_center_size(at(comp_layout::mode(row)), Vec2::new(82.0, 16.0));
        let r = ui
            .interact(key, Id::new(("compressore-modalita", row)), Sense::click())
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
        hardware_key(painter, key, name, choice == current, r.hovered(), 8.0);
    }

    // Manopole.
    let auto = current == CompModeChoice::Fixed;
    let knob_at = |i: usize| at(comp_layout::knob(i));
    hardware_knob(ui, painter, setter, &comp.threshold, knob_at(0), "SOGLIA", ("-50", "0"),
        "Il livello oltre il quale la voce viene compressa. Più in basso = più compressione.", false, false, comp_layout::KNOB_RADIUS);
    hardware_knob(ui, painter, setter, &comp.ratio, knob_at(1), "RAPPORTO", ("2", "10"),
        "Quanto comprime oltre la soglia: 2:1 delicato, 4:1 voce pop, 10:1 molto controllato.", false, false, comp_layout::KNOB_RADIUS);
    hardware_knob(ui, painter, setter, &comp.attack, knob_at(2), "ATTACCO", ("0.5", "300"),
        "Quanto in fretta reagisce (modalità Manuale e Ott. + Man.). Valori bassi domano i picchi, \
         valori alti lasciano passare l'attacco delle consonanti. In modalità Ottica è automatico.", false, auto, comp_layout::KNOB_RADIUS);
    hardware_knob(ui, painter, setter, &comp.release, knob_at(3), "RILASCIO", (".05", "10s"),
        "Quanto in fretta torna a riposo (modalità Manuale e Ott. + Man.). In modalità Ottica è automatico.", false, auto, comp_layout::KNOB_RADIUS);
    hardware_knob(ui, painter, setter, &comp.tube, knob_at(4), "VALVOLA", ("0", "100"),
        "Il calore della valvola e dei trasformatori: fino a ~30% è un colore discreto, \
         oltre diventa saturazione evidente. Senza aliasing grazie al sovracampionamento 4×.", false, false, comp_layout::KNOB_RADIUS);
    hardware_knob(ui, painter, setter, &comp.mix, knob_at(5), "MIX", ("0", "100"),
        "Compressione parallela: miscela tra la voce originale e quella compressa.", false, false, comp_layout::KNOB_RADIUS);
    hardware_knob(ui, painter, setter, &comp.output, knob_at(6), "USCITA", ("-20", "+20"),
        "Volume in uscita, per recuperare quello tolto dalla compressione.", true, false, comp_layout::KNOB_RADIUS);
}

