//! L'interfaccia grafica di SosaTune, disegnata con egui.
//!
//! La grafica riproduce l'immagine di riferimento del plugin (1448 x 1086
//! pixel): tutte le misure qui sotto sono in pixel di quell'immagine e
//! vengono scalate alla finestra con [`Geo`]. Cosi' ogni elemento sta
//! esattamente dove sta nell'immagine, ed e' facile ritoccarlo.
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────────────┐
//! │ SOSA TUNE   │ Vocal Range ⌄ │ Key & Scale ⌄ │ Default*   ←  →  ☰         │
//! │ AUTOTUNE    │ Alto/Tenor    │ A Major       │ Factory                     │
//! ├──────────────────────────────────────────────────────────────────────────┤
//! │ ● AutoKey Listen        ╭─────╮                                 (↶)      │
//! │  ~~~~~~~~~~~~~~~~~~~~~~ │  A  │ ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~  (↷)      │
//! │                         ╰─────╯                                 (?) (⚙)  │
//! │          [════════════════▮════════════════]                    (⏻)      │
//! │          -100     -50     0     +50     +100                             │
//! │                         Edit Notes                                       │
//! │   (C) (C#) (D) (D#) (E) (F) (F#) (G) (G#) (A) (A#) (B)                   │
//! ├──────────────────────────────────────────────────────────────────────────┤
//! │ Detune 440.0 Hz ◯ │ Retune ◔ 15 │ Humanize ◔ 24 │ Flex ◔ 28 │ Mode  ◖ Modern│
//! └──────────────────────────────────────────────────────────────────────────┘
//! ```

use crate::gui::create_egui_editor;
use crate::gui::egui::{
    self, epaint::Mesh, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Id, Order, Painter,
    Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};
use crate::params::{
    Key, QualityChoice, ScaleChoice, SosaTuneParams, StyleChoice, VoiceChoice, EDITOR_HEIGHT, EDITOR_WIDTH,
};
use crate::presets::{self, FACTORY};
use crate::shared::{detect_key, PitchDisplay};
use nih_plug::prelude::*;
use std::f32::consts::{PI, TAU};
use std::sync::atomic::Ordering;
use std::sync::Arc;

// --- Geometria: misure in pixel dell'immagine di riferimento ----------------------

/// Larghezza dell'immagine di riferimento.
const REF_WIDTH: f32 = 1448.0;
/// Da pixel dell'immagine a punti della finestra.
const SCALE: f32 = EDITOR_WIDTH as f32 / REF_WIDTH;

/// Converte le misure dell'immagine di riferimento in posizioni nella finestra.
#[derive(Clone, Copy)]
struct Geo {
    origin: Pos2,
}

impl Geo {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        self.origin + Vec2::new(x * SCALE, y * SCALE)
    }
    fn d(&self, v: f32) -> f32 {
        v * SCALE
    }
    fn r(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(self.p(x0, y0), self.p(x1, y1))
    }
}

// --- Colori (campionati dall'immagine di riferimento) ------------------------------

const OUTER_BG: Color32 = Color32::from_rgb(0x07, 0x17, 0x22);
const PANEL_TOP: Color32 = Color32::from_rgb(0x05, 0x0d, 0x17);
const PANEL_BG: Color32 = Color32::from_rgb(0x03, 0x0a, 0x12);
const PANEL_EDGE: Color32 = Color32::from_rgb(0x22, 0x37, 0x50);
const SEPARATOR: Color32 = Color32::from_rgb(0x1e, 0x29, 0x38);
const TEXT: Color32 = Color32::from_rgb(0xf2, 0xf5, 0xfa);
const TEXT_DIM: Color32 = Color32::from_rgb(0xa9, 0xb4, 0xc6);
const TEXT_FAINT: Color32 = Color32::from_rgb(0x6c, 0x7a, 0x8e);
const CYAN: Color32 = Color32::from_rgb(0x43, 0xe6, 0xff);
const CYAN_DEEP: Color32 = Color32::from_rgb(0x0b, 0x8f, 0xc4);
const BLUE: Color32 = Color32::from_rgb(0x3a, 0xa8, 0xf5);
const TEAL: Color32 = Color32::from_rgb(0x4f, 0xd2, 0xdc);
const BUTTON_BG: Color32 = Color32::from_rgb(0x0c, 0x11, 0x18);
const BUTTON_EDGE: Color32 = Color32::from_rgb(0x3e, 0x46, 0x56);
const BOX_BG: Color32 = Color32::from_rgb(0x03, 0x07, 0x10);
const BOX_EDGE: Color32 = Color32::from_rgb(0x16, 0x1f, 0x2e);
const MENU_BG: Color32 = Color32::from_rgb(0x07, 0x10, 0x1b);

/// Colore che si "somma" alla luce sotto (alfa zero con colori premoltiplicati):
/// serve per bagliori e riflessi.
fn light(c: Color32, k: f32) -> Color32 {
    let k = k.clamp(0.0, 1.0);
    Color32::from_rgba_premultiplied(
        (c.r() as f32 * k) as u8,
        (c.g() as f32 * k) as u8,
        (c.b() as f32 * k) as u8,
        0,
    )
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()), mix(a.a(), b.a()))
}

// --- Caratteri ------------------------------------------------------------------

/// Il carattere e' "Inter" (licenza libera SIL OFL, vedi `assets/Inter-OFL.txt`),
/// incluso nel plugin in quattro pesi.
fn load_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let add = |fonts: &mut egui::FontDefinitions, name: &str, data: &'static [u8]| {
        fonts.font_data.insert(name.to_owned(), Arc::new(egui::FontData::from_static(data)));
    };
    add(&mut fonts, "inter", include_bytes!("../assets/Inter-Regular.ttf"));
    add(&mut fonts, "inter-medium", include_bytes!("../assets/Inter-Medium.ttf"));
    add(&mut fonts, "inter-semibold", include_bytes!("../assets/Inter-SemiBold.ttf"));
    add(&mut fonts, "inter-light", include_bytes!("../assets/InterDisplay-ExtraLight.ttf"));
    // La grande nota nella sfera: Poppins Bold (SIL OFL, `assets/Poppins-OFL.txt`).
    add(&mut fonts, "poppins-bold", include_bytes!("../assets/Poppins-Bold.ttf"));
    let fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
        family.insert(0, "inter".to_owned());
    }
    for (family, font) in [
        ("medium", "inter-medium"),
        ("semibold", "inter-semibold"),
        ("light", "inter-light"),
        ("display", "poppins-bold"),
    ] {
        let mut list = vec![font.to_owned()];
        list.extend(fallbacks.iter().cloned());
        fonts.families.insert(FontFamily::Name(family.into()), list);
    }
    ctx.set_fonts(fonts);
}

#[derive(Clone, Copy)]
enum Weight {
    Light,
    Regular,
    Medium,
    SemiBold,
    /// Poppins Bold, per la nota grande nella sfera.
    Display,
}

/// Un carattere; `size` in pixel dell'immagine di riferimento.
fn font(size: f32, weight: Weight) -> FontId {
    let size = size * SCALE;
    match weight {
        Weight::Regular => FontId::proportional(size),
        Weight::Medium => FontId::new(size, FontFamily::Name("medium".into())),
        Weight::SemiBold => FontId::new(size, FontFamily::Name("semibold".into())),
        Weight::Light => FontId::new(size, FontFamily::Name("light".into())),
        Weight::Display => FontId::new(size, FontFamily::Name("display".into())),
    }
}

/// Testo con la spaziatura delle lettere dell'immagine di riferimento: il
/// carattere dell'immagine e' un po' piu' stretto di Inter, quindi le lettere
/// vengono avvicinate di `tracking` (in frazioni della dimensione).
fn text_tracked(
    painter: &Painter,
    pos: Pos2,
    align: Align2,
    text: impl ToString,
    font: FontId,
    color: Color32,
    tracking: f32,
) -> Rect {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &text.to_string(),
        0.0,
        egui::TextFormat { font_id: font.clone(), color, extra_letter_spacing: tracking * font.size, ..Default::default() },
    );
    let galley = painter.layout_job(job);
    let rect = align.anchor_size(pos, galley.size());
    painter.galley(rect.min, galley, color);
    rect
}

/// Testo dell'interfaccia (spaziatura standard dell'immagine).
fn txt(painter: &Painter, pos: Pos2, align: Align2, text: impl ToString, font: FontId, color: Color32) -> Rect {
    text_tracked(painter, pos, align, text, font, color, -0.04)
}

/// Testo con le lettere distanziate, che occupa esattamente la larghezza
/// `width` a partire da `left` (centrato in verticale su `y`).
fn spaced_text(painter: &Painter, left: Pos2, width: f32, text: &str, font: FontId, color: Color32) {
    let galleys: Vec<_> = text
        .chars()
        .map(|c| painter.layout_no_wrap(c.to_string(), font.clone(), color))
        .collect();
    let natural: f32 = galleys.iter().map(|g| g.size().x).sum();
    let gaps = (galleys.len().max(2) - 1) as f32;
    let spacing = ((width - natural) / gaps).max(0.0);
    let mut x = left.x;
    for galley in galleys {
        let size = galley.size();
        painter.galley(Pos2::new(x, left.y - size.y / 2.0), galley, color);
        x += size.x + spacing;
    }
}

/// Il nome di una nota, con il diesis piccolo in alto ("C#" -> C + ♯ in apice).
fn note_name(painter: &Painter, center: Pos2, pc: usize, size: f32, weight: Weight, color: Color32) -> Rect {
    const LETTERS: [&str; 12] = ["C", "C", "D", "D", "E", "F", "F", "G", "G", "A", "A", "B"];
    let sharp = matches!(pc % 12, 1 | 3 | 6 | 8 | 10);
    let letter = painter.layout_no_wrap(LETTERS[pc % 12].to_owned(), font(size, weight), color);
    if !sharp {
        let s = letter.size();
        let pos = center - s / 2.0;
        painter.galley(pos, letter, color);
        return Rect::from_min_size(pos, s);
    }
    let mark = painter.layout_no_wrap("#".to_owned(), font(size * 0.5, weight), color);
    let total = letter.size().x + mark.size().x * 0.9;
    let left = center.x - total / 2.0;
    let top = center.y - letter.size().y / 2.0;
    let letter_size = letter.size();
    painter.galley(Pos2::new(left, top), letter, color);
    painter.galley(Pos2::new(left + letter_size.x + mark.size().x * 0.05, top + letter_size.y * 0.08), mark, color);
    Rect::from_min_size(Pos2::new(left, top), Vec2::new(total, letter_size.y))
}

// --- Primitive di disegno ---------------------------------------------------------

/// Corona circolare sfumata da `r0` (colore `c0`) a `r1` (colore `c1`).
fn radial(painter: &Painter, center: Pos2, r0: f32, r1: f32, c0: Color32, c1: Color32) {
    let segments = ((r1 * 0.9) as usize).clamp(24, 160);
    let mut mesh = Mesh::default();
    for i in 0..=segments {
        let a = TAU * i as f32 / segments as f32;
        let dir = Vec2::new(a.cos(), a.sin());
        mesh.colored_vertex(center + dir * r0, c0);
        mesh.colored_vertex(center + dir * r1, c1);
    }
    for i in 0..segments as u32 {
        let k = i * 2;
        mesh.add_triangle(k, k + 1, k + 2);
        mesh.add_triangle(k + 1, k + 3, k + 2);
    }
    painter.add(Shape::mesh(mesh));
}

/// Un anello luminoso: piu' intenso sul raggio `r`, sfuma di `spread` dentro e fuori.
fn glow_ring(painter: &Painter, center: Pos2, r: f32, spread: f32, color: Color32, k: f32) {
    radial(painter, center, (r - spread).max(0.0), r, Color32::TRANSPARENT, light(color, k));
    radial(painter, center, r, r + spread, light(color, k), Color32::TRANSPARENT);
}

/// I punti del contorno di un rettangolo arrotondato (raggi per angolo:
/// alto-sinistra, alto-destra, basso-destra, basso-sinistra).
fn rounded_outline(rect: Rect, radii: [f32; 4]) -> Vec<Pos2> {
    let corners = [
        (Pos2::new(rect.left() + radii[0], rect.top() + radii[0]), radii[0], PI),
        (Pos2::new(rect.right() - radii[1], rect.top() + radii[1]), radii[1], 1.5 * PI),
        (Pos2::new(rect.right() - radii[2], rect.bottom() - radii[2]), radii[2], 0.0),
        (Pos2::new(rect.left() + radii[3], rect.bottom() - radii[3]), radii[3], 0.5 * PI),
    ];
    let mut points = Vec::with_capacity(4 * 13);
    for (center, radius, start) in corners {
        for i in 0..=12 {
            let a = start + 0.5 * PI * i as f32 / 12.0;
            points.push(center + Vec2::new(a.cos(), a.sin()) * radius);
        }
    }
    points
}

/// Rettangolo arrotondato riempito con una sfumatura verticale.
fn rounded_gradient(painter: &Painter, rect: Rect, radii: [f32; 4], top: Color32, bottom: Color32) {
    let outline = rounded_outline(rect, radii);
    let color_at = |y: f32| lerp_color(top, bottom, (y - rect.top()) / rect.height().max(1.0));
    let mut mesh = Mesh::default();
    let c = rect.center();
    mesh.colored_vertex(c, color_at(c.y));
    for p in &outline {
        mesh.colored_vertex(*p, color_at(p.y));
    }
    let n = outline.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    painter.add(Shape::mesh(mesh));
}

fn rounded_stroke(painter: &Painter, rect: Rect, radii: [f32; 4], stroke: Stroke) {
    let mut outline = rounded_outline(rect, radii);
    outline.push(outline[0]);
    painter.add(Shape::line(outline, stroke));
}

/// Bagliore intorno a un rettangolo arrotondato.
fn glow_rect(painter: &Painter, rect: Rect, radius: f32, color: Color32, spread: f32, k: f32) {
    let steps = 6;
    for i in 0..steps {
        let t = i as f32 / steps as f32;
        let grow = spread * t;
        let r = rect.expand(grow);
        let stroke = Stroke::new(spread / steps as f32 + 0.5, light(color, k * (1.0 - t).powi(2)));
        painter.rect_stroke(r, CornerRadius::same((radius + grow).min(255.0) as u8), stroke, StrokeKind::Middle);
    }
}

/// Una linea luminosa (nucleo piu' alone).
fn glow_line(painter: &Painter, points: &[Pos2], color: Color32, width: f32, k: f32) {
    for (w, a) in [(width * 7.0, 0.05), (width * 3.5, 0.12), (width * 1.6, 0.35), (width, 1.0)] {
        painter.add(Shape::line(points.to_vec(), Stroke::new(w, light(color, k * a))));
    }
}

fn polar(center: Pos2, radius: f32, angle: f32) -> Pos2 {
    center + Vec2::new(angle.cos(), angle.sin()) * radius
}

fn arc_points(center: Pos2, radius: f32, from: f32, to: f32) -> Vec<Pos2> {
    let steps = (((to - from).abs() * radius / 3.0).ceil() as usize).clamp(2, 256);
    (0..=steps).map(|i| polar(center, radius, from + (to - from) * i as f32 / steps as f32)).collect()
}

/// Generatore di numeri pseudo-casuali fisso: stelle e puntini sono sempre
/// negli stessi posti, fotogramma dopo fotogramma.
struct Lcg(u32);

impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
}

// --- Parametri dall'interfaccia ---------------------------------------------------

/// Imposta un parametro dall'interfaccia (avvisando il DAW, cosi' finisce
/// nell'automazione).
fn set_param<P: Param>(setter: &ParamSetter, param: &P, value: P::Plain) {
    diagnostics::CLICK_SETS.fetch_add(1, Ordering::Relaxed);
    setter.begin_set_parameter(param);
    setter.set_parameter(param, value);
    setter.end_set_parameter(param);
}

/// Rende `hit` una manopola: trascina in su/giu' (Maiusc = ritocchi fini),
/// doppio clic = valore iniziale.
fn drag_param<P: Param>(ui: &Ui, setter: &ParamSetter, param: &P, hit: Rect, id: &str, help: &str) -> egui::Response {
    let response = ui
        .interact(hit, Id::new(("manopola", id)), Sense::click_and_drag())
        .on_hover_cursor(CursorIcon::ResizeVertical)
        .on_hover_text(help);
    let memory_id = response.id.with("trascinamento");
    if response.drag_started() {
        diagnostics::DRAG_STARTS.fetch_add(1, Ordering::Relaxed);
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
        diagnostics::DRAG_SETS.fetch_add(1, Ordering::Relaxed);
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

fn toggle_note(params: &SosaTuneParams, setter: &ParamSetter, pc: usize, notes: &[bool; 12]) {
    if params.scale.value() != ScaleChoice::Custom {
        for (i, &on) in notes.iter().enumerate() {
            set_param(setter, params.custom.note(i), on);
        }
        set_param(setter, &params.scale, ScaleChoice::Custom);
    }
    set_param(setter, params.custom.note(pc), !notes[pc]);
}

fn apply_preset(params: &SosaTuneParams, setter: &ParamSetter, index: usize) {
    let index = index % FACTORY.len();
    let preset = &FACTORY[index];
    set_param(setter, &params.retune_speed, preset.retune_ms);
    set_param(setter, &params.humanize, preset.humanize);
    set_param(setter, &params.flex_tune, preset.flex_tune);
    set_param(setter, &params.vibrato, preset.vibrato);
    set_param(setter, &params.amount, preset.amount);
    set_param(setter, &params.style, preset.style);
    params.preset.store(index, Ordering::Relaxed);
}

const KEY_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

fn key_from_index(i: usize) -> Key {
    Key::from_index(i % 12)
}

fn scale_label(scale: ScaleChoice) -> &'static str {
    match scale {
        ScaleChoice::Chromatic => "Chromatic",
        ScaleChoice::Major => "Major",
        ScaleChoice::NaturalMinor => "Minor",
        ScaleChoice::HarmonicMinor => "Harmonic Minor",
        ScaleChoice::MajorPentatonic => "Major Penta",
        ScaleChoice::MinorPentatonic => "Minor Penta",
        ScaleChoice::Blues => "Blues",
        ScaleChoice::Custom => "Custom",
    }
}

fn voice_label(voice: VoiceChoice) -> &'static str {
    match voice {
        VoiceChoice::Soprano => "Soprano",
        VoiceChoice::AltoTenor => "Alto/Tenor",
        VoiceChoice::Low => "Low Male",
    }
}

fn key_scale_label(params: &SosaTuneParams) -> String {
    match params.scale.value() {
        ScaleChoice::Chromatic => "Chromatic".to_owned(),
        ScaleChoice::Custom => "Custom".to_owned(),
        scale => format!("{} {}", KEY_NAMES[params.key.value().semitone() as usize], scale_label(scale)),
    }
}

// --- Annulla / ripeti --------------------------------------------------------------

/// Cronologia delle modifiche fatte dall'interfaccia (pulsanti ↶ e ↷).
/// Ogni "fotografia" e' il valore normalizzato di tutti i parametri (tranne
/// il bypass), nello stesso ordine di `ptrs`.
#[derive(Default)]
struct History {
    ptrs: Vec<ParamPtr>,
    committed: Vec<f32>,
    undo: Vec<Vec<f32>>,
    redo: Vec<Vec<f32>>,
    /// Ultima volta che l'utente ha toccato la finestra (s).
    last_touch: f64,
    /// Fino a quando ignorare i cambiamenti (subito dopo un annulla/ripeti).
    quiet_until: f64,
}

impl History {
    const LIMIT: usize = 100;

    fn snapshot(&self) -> Vec<f32> {
        // SAFETY: i puntatori vengono dai parametri del plugin, che vivono
        // quanto l'interfaccia.
        self.ptrs.iter().map(|p| unsafe { p.unmodulated_normalized_value() }).collect()
    }

    /// Da chiamare a ogni fotogramma.
    fn track(&mut self, ctx: &egui::Context, params: &SosaTuneParams) {
        if self.ptrs.is_empty() {
            self.ptrs = params.param_map().into_iter().filter(|(id, _, _)| id != "bypass").map(|(_, p, _)| p).collect();
            self.committed = self.snapshot();
        }
        let (now, touching) = ctx.input(|i| (i.time, i.pointer.any_down() || i.pointer.any_pressed()));
        if touching {
            self.last_touch = now;
        }
        let dragging = ctx.input(|i| i.pointer.any_down());
        let current = self.snapshot();
        if current == self.committed {
            return;
        }
        let by_user = now - self.last_touch < 0.6 && now >= self.quiet_until;
        if dragging && by_user {
            // Un gesto in corso (una manopola trascinata): si registra alla fine.
            return;
        }
        if by_user {
            self.undo.push(std::mem::replace(&mut self.committed, current));
            if self.undo.len() > Self::LIMIT {
                self.undo.remove(0);
            }
            self.redo.clear();
        } else {
            // Cambiato dal DAW (automazione, preset del DAW): non si annulla.
            self.committed = current;
        }
    }

    fn apply(&mut self, setter: &ParamSetter, values: &[f32], now: f64) {
        let context = setter.raw_context;
        for (ptr, &value) in self.ptrs.iter().zip(values) {
            // SAFETY: vedi `snapshot`.
            let current = unsafe { ptr.unmodulated_normalized_value() };
            if (current - value).abs() > 1e-7 {
                unsafe {
                    context.raw_begin_set_parameter(*ptr);
                    context.raw_set_parameter_normalized(*ptr, value);
                    context.raw_end_set_parameter(*ptr);
                }
            }
        }
        self.committed = values.to_vec();
        self.quiet_until = now + 0.4;
    }

    fn undo(&mut self, setter: &ParamSetter, now: f64) {
        if let Some(previous) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.apply(setter, &previous, now);
        }
    }

    fn redo(&mut self, setter: &ParamSetter, now: f64) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.apply(setter, &next, now);
        }
    }
}

// --- Stato della grafica -------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Menu {
    Voice,
    KeyScale,
    Preset,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Overlay {
    Help,
    Settings,
}

/// Stato che appartiene solo alla grafica (non viene salvato).
struct GuiState {
    diagnostics: diagnostics::Diagnostics,
    menu: Option<(Menu, Rect)>,
    overlay: Option<Overlay>,
    history: History,
    /// Indicatore -100..+100 (centesimi), con un po' di inerzia.
    meter_cents: f32,
    meter_lit: f32,
    /// L'ultima nota mostrata nella sfera.
    shown_note: Option<usize>,
    note_light: f32,
    /// Livello della voce (0..1) per animare le onde.
    level: f32,
    /// AutoKey: ultimo controllo e ultima tonalita' trovata.
    autokey_checked: f64,
    autokey_found: Option<(String, f64)>,
    last_blocks: usize,
    last_audio_time: f64,
}

fn new_state(diagnostics_enabled: bool) -> GuiState {
    GuiState {
        diagnostics: diagnostics::Diagnostics::new(diagnostics_enabled),
        menu: None,
        overlay: None,
        history: History::default(),
        meter_cents: 0.0,
        meter_lit: 0.0,
        shown_note: None,
        note_light: 0.0,
        level: 0.0,
        autokey_checked: 0.0,
        autokey_found: None,
        last_blocks: 0,
        last_audio_time: f64::NEG_INFINITY,
    }
}

// --- Finestra -----------------------------------------------------------------------

/// Avvolge l'interfaccia in una "rete di sicurezza": se la finestra non si
/// riesce ad aprire (per esempio perche' il Mac non concede la grafica OpenGL),
/// il plugin continua a funzionare senza interfaccia invece di far chiudere
/// di colpo l'intero DAW.
struct SafeEditor(Box<dyn Editor>);

impl Editor for SafeEditor {
    fn spawn(&self, parent: ParentWindowHandle, context: Arc<dyn GuiContext>) -> Box<dyn std::any::Any + Send> {
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
    let editor = create_egui_editor(
        params.editor_state.clone(),
        new_state(true),
        |ctx, _state| setup(ctx),
        move |ctx, setter, state| draw(ctx, &params, &display, setter, state),
    )?;
    Some(Box::new(SafeEditor(editor)))
}

fn setup(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    let mut visuals = egui::Visuals::dark();
    visuals.popup_shadow = egui::epaint::Shadow::NONE;
    ctx.set_visuals(visuals);
    load_fonts(ctx);
}

/// Per l'anteprima e le prove (vedi `preview.rs`): la stessa interfaccia, senza finestra.
pub(crate) struct Headless {
    state: GuiState,
}

impl Headless {
    pub(crate) fn new(ctx: &egui::Context) -> Self {
        setup(ctx);
        Self { state: new_state(false) }
    }

    pub(crate) fn draw(&mut self, ctx: &egui::Context, params: &SosaTuneParams, display: &PitchDisplay, setter: &ParamSetter) {
        draw(ctx, params, display, setter, &mut self.state);
    }
}

/// Disegna un fotogramma dell'interfaccia.
fn draw(ctx: &egui::Context, params: &SosaTuneParams, display: &PitchDisplay, setter: &ParamSetter, state: &mut GuiState) {
    state.diagnostics.observe(ctx, params);
    state.history.track(ctx, params);
    let now = ctx.input(|i| i.time);
    autokey(params, display, setter, state, now);

    egui::CentralPanel::default().frame(egui::Frame::new().fill(OUTER_BG)).show(ctx, |ui| {
        let origin = ui.max_rect().min;
        ui.allocate_exact_size(Vec2::new(EDITOR_WIDTH as f32, EDITOR_HEIGHT as f32), Sense::hover());
        let g = Geo { origin };
        let painter = ui.painter().clone();

        background(&painter, g);
        header(ui, &painter, g, params, setter, state);
        stage(ui, &painter, g, params, display, setter, state, now);
        side_buttons(ui, &painter, g, params, setter, state, now);
        edit_notes(ui, &painter, g, params, display, setter, state);
        bottom_bar(ui, &painter, g, params, setter);
    });

    let g = Geo { origin: ctx.screen_rect().min };
    menus(ctx, g, params, setter, state);
    overlays(ctx, g, params, display, setter, state);
}

// --- Sfondo e pannello -----------------------------------------------------------------

const PANEL: (f32, f32, f32, f32) = (35.0, 72.0, 1413.0, 1010.0);

fn background(painter: &Painter, g: Geo) {
    let all = g.r(0.0, 0.0, REF_WIDTH, 1086.0);
    painter.rect_filled(all, CornerRadius::ZERO, OUTER_BG);
    // Leggera luce dall'alto al centro, come nell'immagine.
    radial(painter, g.p(724.0, 40.0), 0.0, g.d(700.0), light(Color32::from_rgb(0x0e, 0x2a, 0x3c), 0.25), Color32::TRANSPARENT);

    let panel = g.r(PANEL.0, PANEL.1, PANEL.2, PANEL.3);
    let r = g.d(34.0);
    // Ombra e bagliore del bordo.
    glow_rect(painter, panel, r, Color32::from_rgb(0x1d, 0x4a, 0x70), g.d(18.0), 0.25);
    rounded_gradient(painter, panel, [r; 4], PANEL_TOP, PANEL_BG);
    rounded_stroke(painter, panel, [r; 4], Stroke::new(g.d(2.0), PANEL_EDGE));
}

// --- Intestazione -----------------------------------------------------------------

fn chevron(painter: &Painter, g: Geo, x: f32, y: f32) {
    let stroke = Stroke::new(g.d(2.2), Color32::from_rgb(0xc4, 0xcc, 0xd8));
    painter.add(Shape::line(vec![g.p(x - 8.0, y - 4.5), g.p(x, y + 4.5), g.p(x + 8.0, y - 4.5)], stroke));
}

fn arrow(painter: &Painter, g: Geo, x: f32, y: f32, dir: f32, color: Color32) {
    let stroke = Stroke::new(g.d(2.4), color);
    painter.line_segment([g.p(x - 13.0, y), g.p(x + 13.0, y)], stroke);
    let tip = x + 13.0 * dir;
    painter.add(Shape::line(vec![g.p(tip - 10.0 * dir, y - 10.0), g.p(tip, y), g.p(tip - 10.0 * dir, y + 10.0)], stroke));
}

fn header(ui: &Ui, painter: &Painter, g: Geo, params: &SosaTuneParams, setter: &ParamSetter, state: &mut GuiState) {
    // Marchio.
    spaced_text(painter, g.p(80.0, 120.0), g.d(300.0), "SOSA TUNE", font(48.0, Weight::Light), TEXT);
    spaced_text(painter, g.p(85.0, 163.0), g.d(214.0), "AUTOTUNE", font(18.0, Weight::Medium), Color32::from_rgb(0xdc, 0xe3, 0xee));

    // Separatori.
    for x in [407.0, 672.0] {
        painter.line_segment([g.p(x, 92.0), g.p(x, 180.0)], Stroke::new(g.d(1.5), SEPARATOR));
    }
    painter.line_segment([g.p(PANEL.0 + 2.0, 188.0), g.p(PANEL.2 - 2.0, 188.0)], Stroke::new(g.d(1.5), Color32::from_rgb(0x15, 0x22, 0x31)));

    // Vocal Range e Key & Scale: menu a tendina.
    let fields = [
        (Menu::Voice, 441.0, 632.0, "Vocal Range", voice_label(params.voice.value()).to_owned(), g.r(415.0, 92.0, 664.0, 182.0),
            "Il tipo di voce: decide la nota più grave riconosciuta (e la latenza)."),
        (Menu::KeyScale, 706.0, 878.0, "Key & Scale", key_scale_label(params), g.r(680.0, 92.0, 905.0, 182.0),
            "Tonalità e scala: la voce viene portata sulle note di questa scala."),
    ];
    for (menu, x, chevron_x, label, value, hit, help) in fields {
        let response = ui.interact(hit, Id::new(("campo", label)), Sense::click()).on_hover_text(help);
        let open = state.menu.map(|(m, _)| m) == Some(menu);
        if response.hovered() || open {
            painter.rect_filled(hit.shrink(g.d(4.0)), CornerRadius::same(g.d(10.0) as u8), Color32::from_white_alpha(6));
        }
        txt(painter, g.p(x, 113.0), Align2::LEFT_CENTER, label, font(21.0, Weight::Regular), TEXT_DIM);
        txt(painter, g.p(x, 148.0), Align2::LEFT_CENTER, value, font(25.0, Weight::Medium), TEXT);
        chevron(painter, g, chevron_x, 134.0);
        if response.clicked() {
            state.menu = if open { None } else { Some((menu, hit)) };
        }
    }

    // Preset.
    let preset_box = g.r(930.0, 89.0, 1398.0, 183.0);
    painter.rect(
        preset_box,
        CornerRadius::same(g.d(14.0) as u8),
        Color32::from_rgb(0x04, 0x0b, 0x14),
        Stroke::new(g.d(1.5), Color32::from_rgb(0x1a, 0x25, 0x35)),
        StrokeKind::Inside,
    );
    let index = params.preset.load(Ordering::Relaxed) % FACTORY.len();
    let preset = &FACTORY[index];
    let modified = !preset.matches(params);
    let name = if modified { format!("{}*", preset.name) } else { preset.name.to_owned() };
    let name_hit = g.r(936.0, 94.0, 1180.0, 178.0);
    let name_response = ui
        .interact(name_hit, Id::new("preset-nome"), Sense::click())
        .on_hover_text(format!("{}\n(* = modificato. Clic per la lista dei preset.)", preset.help));
    txt(painter, g.p(948.0, 118.0), Align2::LEFT_CENTER, name, font(26.0, Weight::Medium), TEXT);
    txt(painter, g.p(948.0, 152.0), Align2::LEFT_CENTER, presets::BANK, font(24.5, Weight::Regular), Color32::from_rgb(0x9a, 0xab, 0xc4));

    let clicked = |id: &str, center: Pos2, help: &str| {
        let hit = Rect::from_center_size(center, Vec2::splat(g.d(56.0)));
        let response = ui.interact(hit, Id::new(id), Sense::click()).on_hover_text(help);
        if response.hovered() {
            painter.circle_filled(center, g.d(26.0), Color32::from_white_alpha(8));
        }
        response.clicked()
    };
    let icon = Color32::from_rgb(0xd2, 0xda, 0xe6);
    let prev = clicked("preset-prec", g.p(1216.0, 135.0), "Preset precedente");
    let next = clicked("preset-succ", g.p(1288.0, 135.0), "Preset successivo");
    let list = clicked("preset-lista", g.p(1356.0, 135.0), "Tutti i preset");
    arrow(painter, g, 1216.0, 135.0, -1.0, icon);
    arrow(painter, g, 1288.0, 135.0, 1.0, icon);
    for dy in [-10.0, 0.0, 10.0] {
        painter.line_segment([g.p(1343.0, 135.0 + dy), g.p(1369.0, 135.0 + dy)], Stroke::new(g.d(2.4), icon));
    }
    if prev {
        apply_preset(params, setter, (index + FACTORY.len() - 1) % FACTORY.len());
    }
    if next {
        apply_preset(params, setter, index + 1);
    }
    if list || name_response.clicked() {
        let open = state.menu.map(|(m, _)| m) == Some(Menu::Preset);
        state.menu = if open { None } else { Some((Menu::Preset, preset_box)) };
    }
}

// --- La scena centrale: sfera, onde, indicatore -------------------------------------

/// La sfera: centro e raggio, in pixel dell'immagine.
const SPHERE: (f32, f32, f32) = (720.0, 445.0, 227.0);
/// Centro dei cerchi sottili ("orbite") dietro la sfera.
const RINGS_CENTER: (f32, f32) = (720.0, 510.0);

/// Curva morbida che passa per i punti dati (x crescenti), Catmull-Rom.
fn spline(points: &[(f32, f32)], x: f32) -> f32 {
    let n = points.len();
    if x <= points[0].0 {
        return points[0].1;
    }
    if x >= points[n - 1].0 {
        return points[n - 1].1;
    }
    let i = points.windows(2).position(|w| x >= w[0].0 && x <= w[1].0).unwrap_or(0);
    let (x0, y0) = points[i];
    let (x1, y1) = points[i + 1];
    let slope = |a: usize, b: usize| (points[b].1 - points[a].1) / (points[b].0 - points[a].0);
    let m0 = if i == 0 { slope(0, 1) } else { slope(i - 1, i + 1) };
    let m1 = if i + 2 >= n { slope(i, i + 1) } else { slope(i, i + 2) };
    let h = x1 - x0;
    let t = (x - x0) / h;
    let (t2, t3) = (t * t, t * t * t);
    (2.0 * t3 - 3.0 * t2 + 1.0) * y0 + (t3 - 2.0 * t2 + t) * h * m0 + (-2.0 * t3 + 3.0 * t2) * y1 + (t3 - t2) * h * m1
}

/// Il "nastro" di onde, ricalcato dall'immagine: due bordi che si toccano in
/// due nodi (x = 322 e x = 1190). A sinistra del primo nodo e' un ventaglio
/// di linee che scende verso il bordo; tra i due nodi abbraccia la sfera
/// (il bordo alto fa due creste, ai lati della sfera); a destra si riapre.
const RIBBON_A: [(f32, f32); 25] = [
    (20.0, 509.0), (100.0, 512.0), (150.0, 510.0), (200.0, 500.0), (250.0, 484.0), (290.0, 468.0), (322.0, 450.0),
    (370.0, 426.0), (415.0, 417.0), (460.0, 431.0), (500.0, 455.0), (600.0, 505.0), (720.0, 522.0), (840.0, 508.0),
    (940.0, 470.0), (1000.0, 410.0), (1050.0, 376.0), (1095.0, 367.0), (1140.0, 382.0), (1190.0, 425.0),
    (1240.0, 397.0), (1300.0, 380.0), (1350.0, 376.0), (1400.0, 377.0), (1440.0, 378.0),
];
const RIBBON_B: [(f32, f32); 23] = [
    (20.0, 582.0), (100.0, 572.0), (150.0, 561.0), (200.0, 543.0), (250.0, 516.0), (290.0, 482.0), (322.0, 450.0),
    (380.0, 490.0), (430.0, 519.0), (480.0, 542.0), (560.0, 570.0), (720.0, 596.0), (880.0, 582.0), (960.0, 562.0),
    (1020.0, 541.0), (1080.0, 510.0), (1160.0, 452.0), (1190.0, 425.0), (1240.0, 468.0), (1290.0, 498.0),
    (1340.0, 514.0), (1400.0, 519.0), (1440.0, 520.0),
];
/// Il bordo alto della nuvola di puntini a sinistra (il bordo basso e' il nastro).
const DOTS_TOP: [(f32, f32); 8] =
    [(20.0, 362.0), (75.0, 340.0), (130.0, 334.0), (185.0, 340.0), (235.0, 357.0), (275.0, 390.0), (300.0, 420.0), (322.0, 450.0)];

/// Un punto di un filo del nastro: `t` = 0 sul bordo A, 1 sul bordo B.
fn ribbon_y(x: f32, t: f32, swell: f32, phase: f32) -> f32 {
    let a = spline(&RIBBON_A, x);
    let b = spline(&RIBBON_B, x);
    let mid = 0.5 * (a + b);
    // Respiro leggero delle onde (piu' ampio quando arriva la voce).
    let wobble = 3.0 * (x * 0.011 + phase).sin() * ((x - 20.0) / 200.0).clamp(0.0, 1.0);
    mid + (a - mid) * (1.0 - 2.0 * t) * swell + wobble
}

/// Quanto e' visibile il nastro in `x` (sfuma ai bordi del pannello).
fn ribbon_fade(x: f32) -> f32 {
    let left = ((x - 30.0) / 90.0).clamp(0.0, 1.0);
    let right = ((1412.0 - x) / 60.0).clamp(0.0, 1.0);
    left * right
}

#[allow(clippy::too_many_arguments)]
fn stage(
    ui: &Ui,
    painter: &Painter,
    g: Geo,
    params: &SosaTuneParams,
    display: &PitchDisplay,
    setter: &ParamSetter,
    state: &mut GuiState,
    now: f64,
) {
    let area = g.r(PANEL.0 + 2.0, 189.0, PANEL.2 - 2.0, 700.0);
    let clip = painter.with_clip_rect(area);

    // Voce in arrivo? (anima le onde)
    let (input_peak, _, blocks, _) = display.block_status();
    if blocks != state.last_blocks {
        state.last_blocks = blocks;
        state.last_audio_time = now;
    }
    let receiving = now - state.last_audio_time < 0.5;
    let target_level = if receiving { (input_peak * 3.0).clamp(0.0, 1.0) } else { 0.0 };
    state.level += (target_level - state.level) * 0.15;
    let phase = (now as f32) * 0.8;
    let swell = 1.0 + 0.12 * state.level;

    let center = g.p(SPHERE.0, SPHERE.1);
    let radius = g.d(SPHERE.2);

    // Orbite sottili.
    let rings_center = g.p(RINGS_CENTER.0, RINGS_CENTER.1);
    let ring_color = Color32::from_rgb(0x9c, 0xbc, 0xe8);
    for (r, k) in [(300.0f32, 0.05f32), (342.0, 0.10), (392.0, 0.36), (432.0, 0.07), (476.0, 0.55), (548.0, 0.12), (624.0, 0.05)] {
        let steps = 220;
        for s in 0..steps {
            let a0 = TAU * s as f32 / steps as f32;
            let a1 = TAU * (s + 1) as f32 / steps as f32;
            let p0 = polar(rings_center, g.d(r), a0);
            let p1 = polar(rings_center, g.d(r), a1);
            let y = (0.5 * (p0.y + p1.y) - g.origin.y) / SCALE;
            // Visibili soprattutto ai lati della sfera, sfumano in alto e in basso.
            let fade = (1.0 - ((y - 560.0) / 80.0).clamp(0.0, 1.0)) * (0.25 + 0.75 * ((y - 230.0) / 150.0).clamp(0.0, 1.0));
            if fade <= 0.0 {
                continue;
            }
            clip.line_segment([p0, p1], Stroke::new(g.d(1.4), light(ring_color, k * fade)));
            if k > 0.3 {
                clip.line_segment([p0, p1], Stroke::new(g.d(8.0), light(ring_color, 0.08 * fade)));
                clip.line_segment([p0, p1], Stroke::new(g.d(3.0), light(ring_color, 0.18 * fade)));
            }
        }
    }

    // Il nastro di onde (dietro la sfera).
    let strands = 24;
    let strand = |t: f32, from: f32, to: f32| -> Vec<(Pos2, f32)> {
        let mut points = Vec::new();
        let mut x = from;
        while x <= to {
            points.push((g.p(x, ribbon_y(x, t, swell, phase)), ribbon_fade(x)));
            x += 5.0;
        }
        points
    };
    let draw_strand = |p: &Painter, points: &[(Pos2, f32)], color: Color32, width: f32, k: f32, glow: bool| {
        for w in points.windows(2) {
            let fade = 0.5 * (w[0].1 + w[1].1);
            if fade <= 0.0 {
                continue;
            }
            if glow {
                p.line_segment([w[0].0, w[1].0], Stroke::new(width * 9.0, light(color, 0.06 * k * fade)));
                p.line_segment([w[0].0, w[1].0], Stroke::new(width * 4.0, light(color, 0.14 * k * fade)));
                p.line_segment([w[0].0, w[1].0], Stroke::new(width * 2.0, light(color, 0.35 * k * fade)));
            }
            p.line_segment([w[0].0, w[1].0], Stroke::new(width, light(color, k * fade)));
        }
    };
    let strand_color = Color32::from_rgb(0xb4, 0xd2, 0xff);
    let edge_color = Color32::from_rgb(0xe8, 0xf4, 0xff);
    // Un velo di luce tra i due bordi, poi i fili.
    let veil = |p: &Painter, from: f32, to: f32, k: f32, mask: &dyn Fn(Pos2) -> f32| {
        let mut mesh = Mesh::default();
        let mut x = from;
        let mut columns = 0u32;
        while x <= to {
            let pa = g.p(x, ribbon_y(x, 0.0, swell, phase));
            let pb = g.p(x, ribbon_y(x, 1.0, swell, phase));
            let f = ribbon_fade(x);
            mesh.colored_vertex(pa, light(strand_color, k * 0.9 * f * mask(pa)));
            mesh.colored_vertex(pb, light(strand_color, k * 0.3 * f * mask(pb)));
            columns += 1;
            x += 6.0;
        }
        for i in 0..columns.saturating_sub(1) {
            let j = i * 2;
            mesh.add_triangle(j, j + 1, j + 2);
            mesh.add_triangle(j + 1, j + 3, j + 2);
        }
        p.add(Shape::mesh(mesh));
    };
    veil(&clip, 20.0, 1440.0, 0.08, &|_| 1.0);
    // Bagliore largo lungo il bordo luminoso.
    {
        let bloom = strand(0.08, 20.0, 1440.0);
        for w in bloom.windows(2) {
            let f = 0.5 * (w[0].1 + w[1].1);
            clip.line_segment([w[0].0, w[1].0], Stroke::new(g.d(26.0), light(strand_color, 0.05 * f)));
        }
    }
    for i in 1..strands {
        let t = i as f32 / strands as f32;
        // Le linee vicine al bordo alto sono piu' luminose.
        let k = 0.30 + 0.55 * (1.0 - t).powi(3);
        draw_strand(&clip, &strand(t, 20.0, 1440.0), strand_color, g.d(1.4), k, false);
    }
    draw_strand(&clip, &strand(1.0, 20.0, 1440.0), strand_color, g.d(1.6), 0.6, true);
    draw_strand(&clip, &strand(0.0, 20.0, 1440.0), edge_color, g.d(3.0), 1.0, true);

    // Nuvola di puntini a sinistra, a righe che convergono nel primo nodo.
    let mut rng = Lcg(7);
    let rows = 13;
    for row in 0..rows {
        let f = (row + 1) as f32 / rows as f32;
        let mut x = 36.0 + (row % 2) as f32 * 4.5;
        while x < 318.0 {
            let base = ribbon_y(x, 0.0, swell, phase);
            let top = spline(&DOTS_TOP, x);
            let y = base + (top - base) * f + (rng.next() - 0.5) * 3.0;
            let fade = ((x - 30.0) / 50.0).clamp(0.0, 1.0) * (0.55 + 0.45 * (1.0 - (2.0 * f - 1.0).abs()));
            let size = g.d(0.9 + 1.6 * rng.next());
            if rng.next() < 0.9 {
                clip.circle_filled(g.p(x, y), size, light(Color32::from_rgb(0xe8, 0xf2, 0xff), (0.45 + 0.55 * rng.next()) * fade));
            }
            x += 8.6;
        }
    }

    // Puntini sparsi intorno alla sfera, in alto a sinistra e a destra.
    let mut rng = Lcg(99);
    for (from, to) in [(-3.0f32, -2.35f32), (-0.85, -0.15)] {
        let mut d: f32 = 240.0;
        while d < 345.0 {
            let mut a = from;
            while a < to {
                let mid = 0.5 * (from + to);
                let spread = 0.5 * (to - from);
                let density = (1.0 - ((a - mid) / spread).powi(2)) * (1.0 - ((d - 292.0) / 60.0).abs()).max(0.0);
                if rng.next() < density * 0.55 {
                    let p = polar(center, g.d(d + (rng.next() - 0.5) * 4.0), a);
                    clip.circle_filled(p, g.d(0.8 + 1.2 * rng.next()), light(Color32::WHITE, 0.25 + 0.6 * rng.next()));
                }
                a += 9.0 / d;
            }
            d += 9.0;
        }
    }

    // La sfera.
    sphere(&clip, g, center, radius);

    // Davanti alla parte bassa della sfera passa il nastro (sfuma verso l'alto,
    // cosi' sembra entrare nella sfera).
    let front_mask = |p: Pos2| -> f32 {
        let inside = ((radius + g.d(2.0) - (p - center).length()) / g.d(8.0)).clamp(0.0, 1.0);
        let low = ((p.y - (center.y - radius * 0.02)) / (radius * 0.22)).clamp(0.0, 1.0);
        inside * low
    };
    let in_sphere = |points: Vec<(Pos2, f32)>| -> Vec<(Pos2, f32)> {
        points.into_iter().map(|(p, f)| (p, f * front_mask(p))).collect()
    };
    veil(&clip, 480.0, 960.0, 0.08, &front_mask);
    for i in 1..strands {
        let t = i as f32 / strands as f32;
        let k = 0.30 + 0.55 * (1.0 - t).powi(3);
        draw_strand(&clip, &in_sphere(strand(t, 480.0, 960.0)), strand_color, g.d(1.4), k, false);
    }
    draw_strand(&clip, &in_sphere(strand(1.0, 480.0, 960.0)), strand_color, g.d(1.6), 0.6, true);
    draw_strand(&clip, &in_sphere(strand(0.0, 480.0, 960.0)), edge_color, g.d(3.0), 1.0, true);

    // La nota nella sfera.
    let latest = display.latest().filter(|p| receiving && p.input.is_finite());
    if let Some(p) = latest {
        let target = if p.target.is_finite() { p.target } else { p.input.round() };
        let pc = (target.round() as i32).rem_euclid(12) as usize;
        state.shown_note = Some(pc);
        state.note_light = 1.0;
        let cents = ((p.input - target) * 100.0).clamp(-100.0, 100.0);
        state.meter_cents += (cents - state.meter_cents) * 0.35;
        state.meter_lit += (1.0 - state.meter_lit) * 0.3;
    } else {
        state.note_light = (state.note_light - 0.02).max(0.35);
        state.meter_cents *= 0.9;
        state.meter_lit = (state.meter_lit - 0.03).max(0.55);
    }
    // Centro della lettera grande (come nell'immagine: 722, 421).
    let note_center = g.p(722.0, 421.0);
    let note_color = lerp_color(Color32::from_rgb(0x9a, 0xa8, 0xba), Color32::WHITE, state.note_light);
    match state.shown_note {
        Some(pc) => {
            // Bagliore del testo.
            radial(&clip, note_center, 0.0, g.d(110.0), light(Color32::from_rgb(0x8c, 0xb4, 0xe8), 0.16 * state.note_light), Color32::TRANSPARENT);
            big_note(&clip, note_center, pc, note_color);
        }
        None => {
            txt(&clip, note_center, Align2::CENTER_CENTER, "–", font(120.0, Weight::Light), TEXT_FAINT);
        }
    }
    let sphere_hit = Rect::from_center_size(center, Vec2::splat(radius * 1.6));
    let hover = match latest {
        Some(p) if p.target.is_finite() => format!(
            "Nota cantata: {} {:+.0} centesimi, corretta su {}",
            KEY_NAMES[(p.input.round() as i32).rem_euclid(12) as usize],
            (p.input - p.input.round()) * 100.0,
            KEY_NAMES[(p.target.round() as i32).rem_euclid(12) as usize]
        ),
        Some(p) => format!("Nota cantata: {}", KEY_NAMES[(p.input.round() as i32).rem_euclid(12) as usize]),
        None => "In ascolto: canta o fai partire la traccia".to_owned(),
    };
    ui.interact(sphere_hit, Id::new("sfera"), Sense::hover()).on_hover_text(hover);

    // AutoKey Listen.
    let listening = params.autokey.load(Ordering::Relaxed);
    let led = g.p(101.0, 253.0);
    let pulse = if listening { 0.8 + 0.2 * ((now as f32) * 4.0).sin() } else { 0.0 };
    if listening {
        glow_ring(painter, led, g.d(10.0), g.d(14.0), CYAN, 0.6 * pulse);
        radial(painter, led, 0.0, g.d(11.5), Color32::from_rgb(0xf0, 0xff, 0xff), Color32::from_rgb(0x52, 0xe4, 0xff));
    } else {
        radial(painter, led, 0.0, g.d(11.0), Color32::from_rgb(0x1a, 0x36, 0x46), Color32::from_rgb(0x10, 0x22, 0x30));
        painter.circle_stroke(led, g.d(11.0), Stroke::new(g.d(1.2), Color32::from_rgb(0x2c, 0x4c, 0x60)));
    }
    txt(painter, g.p(137.0, 255.0), Align2::LEFT_CENTER, "AutoKey Listen", font(25.0, Weight::Regular), Color32::from_rgb(0xe6, 0xec, 0xf4));
    let hit = g.r(80.0, 230.0, 335.0, 276.0);
    let help = match &state.autokey_found {
        Some((text, _)) => format!("Riconoscimento automatico della tonalità dalla voce.\nUltima trovata: {text}"),
        None => "Riconoscimento automatico della tonalità: accendilo e fai suonare la voce \
                 per qualche secondo, Key & Scale si imposta da solo."
            .to_owned(),
    };
    let response = ui.interact(hit, Id::new("autokey"), Sense::click()).on_hover_text(help);
    if response.clicked() {
        let on = !listening;
        params.autokey.store(on, Ordering::Relaxed);
        if on {
            display.reset_notes();
            state.autokey_checked = now;
            state.autokey_found = None;
        }
    }
    if let Some((found, when)) = &state.autokey_found {
        if now - when < 4.0 {
            txt(painter, g.p(137.0, 289.0), Align2::LEFT_CENTER, format!("→ {found}"), font(20.0, Weight::Regular), CYAN);
        }
    }

    tuning_meter(ui, painter, g, state);
    let _ = setter;
}

/// La nota grande nella sfera (Poppins Bold, alta come nell'immagine), con
/// il diesis piccolo in alto a destra.
fn big_note(painter: &Painter, center: Pos2, pc: usize, color: Color32) {
    const LETTERS: [&str; 12] = ["C", "C", "D", "D", "E", "F", "F", "G", "G", "A", "A", "B"];
    // Lettere maiuscole alte 112 pixel: Poppins ha le maiuscole alte 0,705
    // della dimensione e la linea di base a 1,05 dalla cima della riga.
    let size = 112.0 / 0.705;
    let font_id = font(size, Weight::Display);
    let px = size * SCALE;
    let sharp = matches!(pc % 12, 1 | 3 | 6 | 8 | 10);
    let letter = painter.layout_no_wrap(LETTERS[pc % 12].to_owned(), font_id, color);
    let mark = sharp.then(|| painter.layout_no_wrap("#".to_owned(), font(size * 0.45, Weight::Display), color));
    let total = letter.size().x + mark.as_ref().map_or(0.0, |m| m.size().x * 0.95);
    let left = center.x - total / 2.0;
    let top = center.y - (1.05 - 0.705 / 2.0) * px;
    let letter_width = letter.size().x;
    painter.galley(Pos2::new(left, top), letter, color);
    if let Some(mark) = mark {
        painter.galley(Pos2::new(left + letter_width, top + 0.28 * px), mark, color);
    }
}

fn sphere(painter: &Painter, g: Geo, center: Pos2, radius: f32) {
    // Alone esterno, piu' forte in alto.
    let rim_color = Color32::from_rgb(0xf2, 0xf8, 0xff);
    let glow_color = Color32::from_rgb(0x8c, 0xbe, 0xff);
    let segments = 200;
    let rim_k = |a: f32| -> f32 {
        // a: angolo sullo schermo (sin > 0 = in basso). Il bordo e' pieno
        // sopra e ai lati, sfuma nella parte bassa.
        let down = a.sin();
        let low = ((down - 0.35) / 0.5).clamp(0.0, 1.0);
        let side = a.cos().abs();
        (1.0 - 0.9 * low) * (0.82 + 0.18 * side)
    };
    glow_ring(painter, center, radius, g.d(30.0), glow_color, 0.65);
    glow_ring(painter, center, radius + g.d(8.0), g.d(90.0), Color32::from_rgb(0xb4, 0xd4, 0xff), 0.08);

    // Corpo: scuro al centro, piu' chiaro verso il bordo.
    radial(painter, center, 0.0, radius * 0.62, Color32::from_rgb(0x03, 0x08, 0x0f), Color32::from_rgb(0x05, 0x0c, 0x16));
    radial(painter, center, radius * 0.62, radius * 0.92, Color32::from_rgb(0x05, 0x0c, 0x16), Color32::from_rgb(0x0a, 0x17, 0x27));
    radial(painter, center, radius * 0.92, radius, Color32::from_rgb(0x0a, 0x17, 0x27), Color32::from_rgb(0x24, 0x40, 0x5e));

    // Stelle, piu' fitte e luminose verso il bordo.
    let mut rng = Lcg(2024);
    for _ in 0..760 {
        let a = rng.next() * TAU;
        let d = rng.next().powf(0.38);
        let p = center + Vec2::new(a.cos(), a.sin()) * radius * d * 0.975;
        let size = g.d(0.55 + 1.5 * rng.next().powf(3.0));
        let k = (0.12 + 0.75 * rng.next()) * (0.45 + 0.55 * d);
        painter.circle_filled(p, size, light(Color32::from_rgb(0xdc, 0xea, 0xff), k));
    }
    // Trama luminosa lungo il bordo interno, come un pianeta in controluce.
    for _ in 0..180 {
        let a = rng.next() * TAU;
        let d = 0.88 + 0.11 * rng.next();
        let k = rim_k(a) * (0.25 + 0.6 * rng.next()) * ((d - 0.86) / 0.13);
        let p = polar(center, radius * d, a);
        painter.circle_filled(p, g.d(0.7 + 0.9 * rng.next()), light(Color32::from_rgb(0xe8, 0xf2, 0xff), k));
    }

    // Bordo luminoso: corone sfumate (senza scalini), piu' intense dove
    // il bordo e' illuminato.
    let band = |r0: f32, r1: f32, c0: Color32, c1: Color32, k0: f32, k1: f32| {
        let mut mesh = Mesh::default();
        for i in 0..=segments {
            let a = TAU * i as f32 / segments as f32;
            let k = rim_k(a);
            mesh.colored_vertex(polar(center, r0, a), light(c0, k0 * k));
            mesh.colored_vertex(polar(center, r1, a), light(c1, k1 * k));
        }
        for i in 0..segments as u32 {
            let j = i * 2;
            mesh.add_triangle(j, j + 1, j + 2);
            mesh.add_triangle(j + 1, j + 3, j + 2);
        }
        painter.add(Shape::mesh(mesh));
    };
    let (inner, core0, core1, outer) = (radius - g.d(16.0), radius - g.d(3.5), radius + g.d(2.5), radius + g.d(12.0));
    band(inner, core0, glow_color, rim_color, 0.0, 0.55);
    band(core0, radius, rim_color, Color32::WHITE, 0.55, 1.0);
    band(radius, core1, Color32::WHITE, rim_color, 1.0, 0.6);
    band(core1, outer, rim_color, glow_color, 0.6, 0.0);
}

/// L'indicatore orizzontale: di quanti centesimi la voce cantata e' lontana
/// dalla nota giusta (-100 .. +100).
fn tuning_meter(ui: &Ui, painter: &Painter, g: Geo, state: &GuiState) {
    // Sotto l'indicatore: un "orizzonte" curvo e lucido (chiaro ai lati) e,
    // piu' in basso, la fascia arrotondata di "Edit Notes".
    let horizon_y = |x: f32| 625.0 + 95.0 * ((x - 723.0).abs() / 688.0).min(1.0).powf(4.5);
    let band_top: f32 = 684.0;
    let (left, right) = (PANEL.0 + 1.0, PANEL.2 - 1.0);
    {
        // Vetro: dall'orizzonte alla fascia, opaco (copre la base della sfera).
        let mut mesh = Mesh::default();
        let mut x = left;
        let mut columns = 0u32;
        while x <= right + 0.1 {
            let y = horizon_y(x);
            let edge = ((x - 723.0).abs() / 688.0).powf(3.0);
            let top_color = lerp_color(Color32::from_rgb(0x10, 0x24, 0x3a), Color32::from_rgb(0x1c, 0x34, 0x52), edge);
            mesh.colored_vertex(g.p(x, y), top_color);
            mesh.colored_vertex(g.p(x, band_top.max(y) + 40.0), Color32::from_rgb(0x08, 0x14, 0x22));
            columns += 1;
            x += 8.0;
        }
        for i in 0..columns - 1 {
            let k = i * 2;
            mesh.add_triangle(k, k + 1, k + 2);
            mesh.add_triangle(k + 1, k + 3, k + 2);
        }
        painter.add(Shape::mesh(mesh));
        // Luce sotto la barra, al centro.
        radial(painter, g.p(723.0, 640.0), 0.0, g.d(260.0), light(Color32::from_rgb(0x1c, 0x5c, 0x94), 0.35), Color32::TRANSPARENT);
    }
    // Il filo luminoso dell'orizzonte: piu' intenso verso i lati.
    {
        let mut x = left;
        let mut prev: Option<Pos2> = None;
        while x <= right + 0.1 {
            let p = g.p(x, horizon_y(x));
            if let Some(q) = prev {
                let edge = ((x - 723.0).abs() / 688.0).clamp(0.0, 1.0);
                let k = 0.25 + 0.75 * edge.powf(2.0);
                painter.line_segment([q, p], Stroke::new(g.d(10.0), light(Color32::from_rgb(0x5c, 0x9c, 0xe8), 0.10 * k)));
                painter.line_segment([q, p], Stroke::new(g.d(1.8), light(Color32::from_rgb(0xa8, 0xd0, 0xff), 0.75 * k)));
            }
            prev = Some(p);
            x += 6.0;
        }
    }
    // La fascia di Edit Notes.
    {
        let band = g.r(left, band_top, right, 860.0);
        let r = g.d(88.0);
        rounded_gradient(painter, band, [r, r, 0.0, 0.0], Color32::from_rgb(0x06, 0x15, 0x26), Color32::from_rgb(0x02, 0x0b, 0x16));
        let mut outline = rounded_outline(band, [r, r, 0.0, 0.0]);
        // Solo il bordo alto (con gli angoli).
        outline.retain(|p| p.y < band.top() + r + 1.0);
        painter.add(Shape::line(outline.clone(), Stroke::new(g.d(6.0), light(Color32::from_rgb(0x4c, 0x80, 0xc0), 0.10))));
        painter.add(Shape::line(outline, Stroke::new(g.d(1.6), light(Color32::from_rgb(0x8c, 0xb4, 0xe0), 0.55))));
    }

    // La barra.
    let bar = g.r(355.0, 588.0, 1092.0, 620.0);
    painter.rect_filled(bar, CornerRadius::same(g.d(5.0) as u8), Color32::from_rgb(0x06, 0x12, 0x1e));
    let x_of = |cents: f32| 722.5 + cents.clamp(-100.0, 100.0) * 3.875;
    let x = x_of(state.meter_cents).clamp(359.0, 1088.0);
    let lit = state.meter_lit;
    // Riempimento luminoso intorno all'indicatore.
    {
        let mut mesh = Mesh::default();
        let (top, bottom) = (bar.top() + g.d(2.0), bar.bottom() - g.d(2.0));
        let stops = [
            (-150.0f32, 0.0f32),
            (-70.0, 0.35),
            (-24.0, 0.75),
            (-8.0, 1.0),
            (8.0, 1.0),
            (24.0, 0.7),
            (60.0, 0.25),
            (120.0, 0.0),
        ];
        for (dx, k) in stops {
            let px = g.p((x + dx).clamp(357.0, 1090.0), 0.0).x;
            let c = light(lerp_color(CYAN_DEEP, CYAN, k), k * lit);
            mesh.colored_vertex(Pos2::new(px, top), c);
            mesh.colored_vertex(Pos2::new(px, bottom), c);
        }
        for i in 0..(stops.len() as u32 - 1) {
            let k = i * 2;
            mesh.add_triangle(k, k + 1, k + 2);
            mesh.add_triangle(k + 1, k + 3, k + 2);
        }
        painter.add(Shape::mesh(mesh));
    }
    let core = g.r(x - 5.0, 590.0, x + 5.0, 618.0);
    painter.rect_filled(core, CornerRadius::same(2), lerp_color(CYAN, Color32::WHITE, lit));
    glow_rect(painter, core, g.d(2.0), CYAN, g.d(8.0), 0.5 * lit);
    painter.rect_stroke(bar, CornerRadius::same(g.d(5.0) as u8), Stroke::new(g.d(1.6), Color32::from_rgb(0x2c, 0x8c, 0x9c)), StrokeKind::Inside);
    for end in [356.5, 1090.5] {
        painter.line_segment([g.p(end, 592.0), g.p(end, 616.0)], Stroke::new(g.d(2.4), Color32::from_rgb(0x4c, 0xe0, 0xf0)));
    }

    // Scala graduata.
    let tick = Color32::from_rgb(0x8c, 0x9c, 0xb0);
    let mut cents = -100;
    while cents <= 100 {
        let major = cents % 50 == 0;
        let x = x_of(cents as f32);
        let (y0, y1) = if major { (626.0, 643.0) } else { (630.0, 639.0) };
        painter.line_segment([g.p(x, y0), g.p(x, y1)], Stroke::new(g.d(if major { 1.6 } else { 1.2 }), tick));
        cents += 5;
    }
    for (c, label) in [(-100.0, "−100"), (-50.0, "−50"), (0.0, "0"), (50.0, "+50"), (100.0, "+100")] {
        txt(painter, g.p(x_of(c), 657.0), Align2::CENTER_CENTER, label, font(23.0, Weight::Regular), Color32::from_rgb(0xe2, 0xe8, 0xf0));
    }
    ui.interact(bar.expand(g.d(10.0)), Id::new("indicatore"), Sense::hover()).on_hover_text(
        "Di quanti centesimi la voce cantata è lontana dalla nota giusta: \
         a sinistra calante, a destra crescente.",
    );
}

// --- Pulsanti laterali ---------------------------------------------------------------

fn round_button(painter: &Painter, g: Geo, center: Pos2, hovered: bool) {
    let r = g.d(34.0);
    radial(painter, center, 0.0, r, Color32::from_rgb(0x12, 0x18, 0x20), BUTTON_BG);
    let edge = if hovered { Color32::from_rgb(0x6a, 0x76, 0x8a) } else { BUTTON_EDGE };
    painter.circle_stroke(center, r, Stroke::new(g.d(1.6), edge));
}

fn icon_undo(painter: &Painter, g: Geo, c: Pos2, mirror: bool) {
    let s = if mirror { -1.0 } else { 1.0 };
    let stroke = Stroke::new(g.d(2.4), Color32::from_rgb(0xe6, 0xec, 0xf4));
    // Freccia che torna indietro: tratto orizzontale in alto, curva, ritorno.
    let mut points = vec![c + Vec2::new(-11.0 * s, -7.0) * SCALE];
    for i in 0..=12 {
        let a = -PI / 2.0 + PI * i as f32 / 12.0;
        points.push(c + Vec2::new((2.0 + 9.0 * a.cos()) * s, 2.0 + 9.0 * a.sin()) * SCALE);
    }
    points.push(c + Vec2::new(-9.0 * s, 11.0) * SCALE);
    painter.add(Shape::line(points, stroke));
    painter.add(Shape::line(
        vec![c + Vec2::new(-5.0 * s, -13.0) * SCALE, c + Vec2::new(-11.0 * s, -7.0) * SCALE, c + Vec2::new(-5.0 * s, -1.0) * SCALE],
        stroke,
    ));
}

fn icon_gear(painter: &Painter, g: Geo, c: Pos2) {
    let stroke = Stroke::new(g.d(2.2), Color32::from_rgb(0xe6, 0xec, 0xf4));
    let mut points = Vec::new();
    let teeth = 8;
    for i in 0..=teeth * 8 {
        let a = TAU * i as f32 / (teeth * 8) as f32;
        let phase = (i % 8) as f32;
        let r = if (2.0..6.0).contains(&phase) { 15.0 } else { 11.5 };
        points.push(polar(c, g.d(r), a));
    }
    painter.add(Shape::line(points, stroke));
    painter.circle_stroke(c, g.d(5.5), stroke);
}

fn icon_power(painter: &Painter, g: Geo, c: Pos2, color: Color32) {
    let stroke = Stroke::new(g.d(2.8), color);
    painter.add(Shape::line(arc_points(c, g.d(14.0), -PI / 2.0 + 0.75, 1.5 * PI - 0.75), stroke));
    painter.line_segment([c + Vec2::new(0.0, -17.0) * SCALE, c + Vec2::new(0.0, -3.0) * SCALE], stroke);
}

fn side_buttons(ui: &Ui, painter: &Painter, g: Geo, params: &SosaTuneParams, setter: &ParamSetter, state: &mut GuiState, now: f64) {
    let buttons: [(&str, f32, &str); 4] = [
        ("annulla", 245.0, "Annulla l'ultima modifica"),
        ("ripeti", 328.0, "Ripeti la modifica annullata"),
        ("aiuto", 412.0, "Guida ai comandi"),
        ("impostazioni", 495.0, "Altre impostazioni: quantità, trasposizione, vibrato, formanti, sensibilità"),
    ];
    for (id, y, help) in buttons {
        let center = g.p(1338.0, y);
        let hit = Rect::from_center_size(center, Vec2::splat(g.d(70.0)));
        let response = ui.interact(hit, Id::new(id), Sense::click()).on_hover_text(help);
        round_button(painter, g, center, response.hovered());
        match id {
            "annulla" => icon_undo(painter, g, center, false),
            "ripeti" => icon_undo(painter, g, center, true),
            "aiuto" => {
                txt(painter, center + Vec2::new(0.0, g.d(1.0)), Align2::CENTER_CENTER, "?", font(34.0, Weight::Regular), TEXT);
            }
            _ => icon_gear(painter, g, center),
        }
        if response.clicked() {
            match id {
                "annulla" => state.history.undo(setter, now),
                "ripeti" => state.history.redo(setter, now),
                "aiuto" => state.overlay = if state.overlay == Some(Overlay::Help) { None } else { Some(Overlay::Help) },
                _ => state.overlay = if state.overlay == Some(Overlay::Settings) { None } else { Some(Overlay::Settings) },
            }
        }
    }

    // Accensione (bypass del DAW).
    let center = g.p(1338.0, 592.0);
    let on = !params.bypass.value();
    let hit = Rect::from_center_size(center, Vec2::splat(g.d(76.0)));
    let response = ui
        .interact(hit, Id::new("accensione"), Sense::click())
        .on_hover_text(if on { "Sosa Tune è acceso (clic per escluderlo)" } else { "Sosa Tune è escluso (clic per accenderlo)" });
    let r = g.d(36.0);
    if on {
        radial(painter, center, 0.0, r, Color32::from_rgb(0x02, 0x16, 0x2a), Color32::from_rgb(0x02, 0x3e, 0x6c));
        glow_ring(painter, center, r, g.d(14.0), Color32::from_rgb(0x36, 0xc8, 0xff), 0.75);
        painter.circle_stroke(center, r, Stroke::new(g.d(3.0), Color32::from_rgb(0x9c, 0xf2, 0xff)));
        icon_power(painter, g, center, Color32::from_rgb(0xe8, 0xfb, 0xff));
    } else {
        round_button(painter, g, center, response.hovered());
        icon_power(painter, g, center, TEXT_FAINT);
    }
    if response.clicked() {
        set_param(setter, &params.bypass, on);
    }
}

// --- Edit Notes -----------------------------------------------------------------------

fn note_center(g: Geo, pc: usize) -> Pos2 {
    g.p(175.0 + pc as f32 * 99.36, 776.0)
}

fn edit_notes(ui: &Ui, painter: &Painter, g: Geo, params: &SosaTuneParams, display: &PitchDisplay, setter: &ParamSetter, state: &GuiState) {
    txt(painter, g.p(723.0, 713.0), Align2::CENTER_CENTER, "Edit Notes", font(25.0, Weight::Regular), TEXT);
    let notes = params.active_notes();
    let current = state.shown_note.filter(|_| display.latest().is_some_and(|p| p.input.is_finite()) && state.note_light > 0.9);
    let r = g.d(38.0);
    for pc in 0..12 {
        let center = note_center(g, pc);
        let hit = Rect::from_center_size(center, Vec2::splat(r * 2.1));
        let response = ui
            .interact(hit, Id::new(("nota", pc)), Sense::click())
            .on_hover_text(if notes[pc] {
                "Nota della scala: clic per escluderla (la scala diventa personalizzata)"
            } else {
                "Nota esclusa: clic per includerla (la scala diventa personalizzata)"
            });
        let is_current = current == Some(pc);
        if is_current {
            radial(painter, center, 0.0, r, Color32::from_rgb(0x02, 0x1c, 0x26), Color32::from_rgb(0x00, 0x55, 0x6e));
            glow_ring(painter, center, r, g.d(12.0), TEAL, 0.6);
            painter.circle_stroke(center, r, Stroke::new(g.d(2.6), Color32::from_rgb(0x8c, 0xe6, 0xee)));
        } else if notes[pc] {
            radial(painter, center, 0.0, r, Color32::from_rgb(0x03, 0x10, 0x1e), Color32::from_rgb(0x02, 0x40, 0x6c));
            glow_ring(painter, center, r, g.d(11.0), BLUE, 0.5);
            painter.circle_stroke(center, r, Stroke::new(g.d(2.4), Color32::from_rgb(0x86, 0xd2, 0xfa)));
        } else {
            painter.circle_filled(center, r, Color32::from_rgb(0x04, 0x0b, 0x13));
            let edge = if response.hovered() { Color32::from_rgb(0x3c, 0x4c, 0x60) } else { Color32::from_rgb(0x1e, 0x29, 0x37) };
            painter.circle_stroke(center, r, Stroke::new(g.d(1.6), edge));
        }
        let color = if notes[pc] || is_current { TEXT } else { Color32::from_rgb(0xc8, 0xd0, 0xdc) };
        note_name(painter, center, pc, 26.0, Weight::Medium, color);
        if response.clicked() {
            toggle_note(params, setter, pc, &notes);
        }
    }
}

// --- Fascia inferiore --------------------------------------------------------------------

#[derive(Clone, Copy)]
enum ArcStyle {
    Cyan,
    Violet,
    White,
    None,
}

/// Una manopola come nell'immagine: corpo scuro, linea dell'indice, arco
/// colorato del valore.
fn knob_visual(painter: &Painter, g: Geo, center: Pos2, r: f32, value: f32, style: ArcStyle) {
    // Ombra.
    radial(painter, center + Vec2::new(0.0, g.d(4.0)), r * 0.9, r * 1.35, Color32::from_black_alpha(150), Color32::TRANSPARENT);
    radial(painter, center, 0.0, r, Color32::from_rgb(0x15, 0x1d, 0x28), Color32::from_rgb(0x07, 0x0c, 0x14));
    painter.circle_stroke(center, r, Stroke::new(g.d(1.4), Color32::from_rgb(0x2a, 0x34, 0x44)));
    // Riflesso in alto a sinistra.
    painter.add(Shape::line(arc_points(center, r - g.d(1.5), PI * 1.05, PI * 1.6), Stroke::new(g.d(1.4), light(Color32::WHITE, 0.25))));

    let start = 0.75 * PI;
    let sweep = 1.5 * PI;
    let angle = start + sweep * value.clamp(0.0, 1.0);
    let arc_r = r + g.d(3.5);
    match style {
        ArcStyle::None => {
            painter.circle_filled(polar(center, r * 0.72, angle), g.d(2.2), Color32::from_rgb(0xc8, 0xd2, 0xe0));
        }
        _ => {
            let (from, to) = match style {
                ArcStyle::Cyan => (Color32::from_rgb(0x0a, 0x6a, 0x9a), Color32::from_rgb(0x6c, 0xf0, 0xff)),
                ArcStyle::Violet => (Color32::from_rgb(0x4a, 0x2a, 0xc8), Color32::from_rgb(0xc4, 0xb0, 0xff)),
                _ => (Color32::from_rgb(0x9a, 0xa4, 0xb4), Color32::WHITE),
            };
            if value > 0.002 {
                let steps = 40;
                for i in 0..steps {
                    let t0 = i as f32 / steps as f32;
                    let t1 = (i + 1) as f32 / steps as f32;
                    let a0 = start + (angle - start) * t0;
                    let a1 = start + (angle - start) * t1;
                    let color = lerp_color(from, to, t1);
                    painter.line_segment([polar(center, arc_r, a0), polar(center, arc_r, a1)], Stroke::new(g.d(16.0), light(color, 0.10)));
                    painter.line_segment([polar(center, arc_r, a0), polar(center, arc_r, a1)], Stroke::new(g.d(8.0), light(color, 0.22)));
                    painter.line_segment([polar(center, arc_r, a0), polar(center, arc_r, a1)], Stroke::new(g.d(5.4), color));
                }
            }
            painter.line_segment([polar(center, r * 0.08, angle), polar(center, r * 0.86, angle)], Stroke::new(g.d(1.9), Color32::from_rgb(0xd0, 0xda, 0xe6)));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn knob_with_box<P: Param>(
    ui: &Ui,
    painter: &Painter,
    g: Geo,
    setter: &ParamSetter,
    param: &P,
    label: (&str, f32),
    knob: (f32, f32),
    value_box: (f32, f32),
    text: String,
    style: ArcStyle,
    help: &str,
) {
    txt(painter, g.p(label.1, 879.0), Align2::LEFT_CENTER, label.0, font(22.0, Weight::Regular), Color32::from_rgb(0xdc, 0xe2, 0xec));
    let rect = g.r(value_box.0, 909.0, value_box.1, 976.0);
    painter.rect(rect, CornerRadius::same(g.d(9.0) as u8), BOX_BG, Stroke::new(g.d(1.4), BOX_EDGE), StrokeKind::Inside);
    // Il numero sta al centro della parte del riquadro non coperta dalla manopola.
    let text_x = 0.5 * (knob.0 + 34.0 + value_box.1);
    text_tracked(painter, g.p(text_x + 2.0, 943.0), Align2::CENTER_CENTER, text, font(30.0, Weight::Medium), TEXT, -0.07);
    let center = g.p(knob.0, knob.1);
    knob_visual(painter, g, center, g.d(34.0), param.unmodulated_normalized_value(), style);
    let hit = g.r(knob.0 - 46.0, 860.0, value_box.1, 985.0);
    drag_param(ui, setter, param, hit, label.0, help);
}

fn bottom_bar(ui: &Ui, painter: &Painter, g: Geo, params: &SosaTuneParams, setter: &ParamSetter) {
    let bar = g.r(37.0, 843.0, 1411.0, 1008.0);
    let radii = [g.d(42.0), g.d(42.0), g.d(32.0), g.d(32.0)];
    rounded_gradient(painter, bar, radii, Color32::from_rgb(0x08, 0x10, 0x22), Color32::from_rgb(0x06, 0x09, 0x17));
    rounded_stroke(painter, bar, radii, Stroke::new(g.d(1.6), Color32::from_rgb(0x1a, 0x30, 0x4c)));
    // Riflesso sul bordo in alto.
    painter.line_segment([g.p(120.0, 844.5), g.p(1330.0, 844.5)], Stroke::new(g.d(1.2), light(Color32::from_rgb(0x5a, 0x8c, 0xd0), 0.35)));
    for x in [302.0, 512.0, 724.0, 926.0] {
        painter.line_segment([g.p(x, 862.0), g.p(x, 990.0)], Stroke::new(g.d(1.4), SEPARATOR));
    }

    // Detune.
    txt(painter, g.p(74.0, 899.0), Align2::LEFT_CENTER, "Detune", font(21.0, Weight::Regular), Color32::from_rgb(0xc8, 0xd0, 0xdc));
    txt(painter, g.p(74.0, 940.0), Align2::LEFT_CENTER, format!("{:.1} Hz", params.detune.value()), font(30.0, Weight::Medium), TEXT);
    knob_visual(painter, g, g.p(243.0, 928.0), g.d(32.0), params.detune.unmodulated_normalized_value(), ArcStyle::None);
    drag_param(
        ui,
        setter,
        &params.detune,
        g.r(60.0, 870.0, 290.0, 980.0),
        "detune",
        "Accordatura: la frequenza del La4. 440 Hz è lo standard; cambiala se la base \
         è accordata diversamente (per esempio 432 Hz). Doppio clic = 440 Hz.",
    );

    knob_with_box(
        ui,
        painter,
        g,
        setter,
        &params.retune_speed,
        ("Retune Speed", 330.0),
        (367.0, 943.0),
        (392.0, 484.0),
        format!("{:.0}", params.retune_speed.value()),
        ArcStyle::Cyan,
        "Quanto in fretta la voce viene portata sulla nota giusta (ms).\n\
         0 = effetto robotico, 10-30 = pop, 50-150 = naturale.",
    );
    knob_with_box(
        ui,
        painter,
        g,
        setter,
        &params.humanize,
        ("Humanize", 542.0),
        (573.0, 943.0),
        (598.0, 692.0),
        format!("{:.0}", params.humanize.value() * 100.0),
        ArcStyle::Violet,
        "Sulle note lunghe la correzione diventa più morbida, così le note tenute \
         non suonano \"congelate\"; quelle brevi restano corrette.",
    );
    knob_with_box(
        ui,
        painter,
        g,
        setter,
        &params.flex_tune,
        ("Flex Tune", 755.0),
        (787.0, 943.0),
        (812.0, 904.0),
        format!("{:.0}", params.flex_tune.value() * 100.0),
        ArcStyle::White,
        "Corregge solo quando la voce è già vicina alla nota: scivolate e note \
         volutamente \"sporche\" restano come le hai cantate.",
    );

    // Mode.
    txt(painter, g.p(958.0, 894.0), Align2::LEFT_CENTER, "Mode", font(22.0, Weight::Regular), Color32::from_rgb(0xdc, 0xe2, 0xec));
    let quality = params.quality.value();
    let modes = [
        (QualityChoice::LowLatency, "Low Latency", g.r(958.0, 918.0, 1102.0, 968.0), "Latenza minima (21 ms): per cantare in diretta con il plugin acceso. Stesso motore phase vocoder, finestra più corta."),
        (QualityChoice::Hq, "HQ", g.r(1112.0, 918.0, 1196.0, 968.0), "Qualità alta: finestra del phase vocoder più lunga, il suono più pulito e l'intonazione più precisa (latenza 43 ms, compensata dal DAW). Ideale per il mix."),
    ];
    for (choice, label, rect, help) in modes {
        let response = ui.interact(rect, Id::new(("modo", label)), Sense::click()).on_hover_text(help);
        let selected = quality == choice;
        let radius = CornerRadius::same(g.d(9.0) as u8);
        if selected {
            rounded_gradient(painter, rect, [g.d(9.0); 4], Color32::from_rgb(0x08, 0x2c, 0x50), Color32::from_rgb(0x04, 0x3a, 0x6a));
            glow_rect(painter, rect, g.d(9.0), Color32::from_rgb(0x4c, 0xd8, 0xff), g.d(10.0), 0.55);
            painter.rect_stroke(rect, radius, Stroke::new(g.d(2.2), Color32::from_rgb(0xae, 0xf5, 0xff)), StrokeKind::Inside);
        } else {
            let edge = if response.hovered() { Color32::from_rgb(0x34, 0x44, 0x58) } else { Color32::from_rgb(0x1d, 0x28, 0x38) };
            painter.rect(rect, radius, Color32::from_rgb(0x02, 0x09, 0x13), Stroke::new(g.d(1.5), edge), StrokeKind::Inside);
        }
        txt(painter, rect.center(), Align2::CENTER_CENTER, label, font(21.0, Weight::Regular), TEXT);
        if response.clicked() && !selected {
            set_param(setter, &params.quality, choice);
        }
    }

    // Modern / Classic.
    let style = params.style.value();
    for (choice, label, y, help) in [
        (StyleChoice::Modern, "Modern", 897.0, "Moderno: le note partono già intonate e la correzione non rimbalza tra due note."),
        (StyleChoice::Classic, "Classic", 953.0, "Classico: ogni nota parte come l'hai cantata e scivola sulla nota giusta; il bersaglio cambia subito (il carattere degli autotune di una volta)."),
    ] {
        let on = style == choice;
        let pill = g.r(1227.0, y - 17.0, 1289.0, y + 17.0);
        let hit = g.r(1220.0, y - 24.0, 1400.0, y + 24.0);
        let response = ui.interact(hit, Id::new(("stile", label)), Sense::click()).on_hover_text(help);
        let r = g.d(17.0);
        if on {
            rounded_gradient(painter, pill, [r; 4], Color32::from_rgb(0x1e, 0x72, 0xee), Color32::from_rgb(0x2c, 0x8c, 0xff));
            glow_rect(painter, pill, r, Color32::from_rgb(0x2c, 0x8c, 0xff), g.d(8.0), 0.35);
            let knob = g.p(1244.0, y);
            radial(painter, knob, 0.0, g.d(13.0), Color32::WHITE, Color32::from_rgb(0xd8, 0xe8, 0xff));
            painter.circle_filled(g.p(1275.0, y), g.d(6.0), Color32::from_rgb(0x0d, 0x98, 0xf5));
        } else {
            rounded_gradient(painter, pill, [r; 4], Color32::from_rgb(0x1c, 0x2c, 0x3e), Color32::from_rgb(0x14, 0x22, 0x32));
            let knob = g.p(1272.0, y);
            radial(painter, knob, 0.0, g.d(13.5), Color32::from_rgb(0x5a, 0x70, 0x8e), Color32::from_rgb(0x34, 0x48, 0x62));
            if response.hovered() {
                rounded_stroke(painter, pill, [r; 4], Stroke::new(g.d(1.2), Color32::from_rgb(0x4a, 0x5c, 0x74)));
            }
        }
        txt(painter, g.p(1308.0, y), Align2::LEFT_CENTER, label, font(23.0, Weight::Regular), TEXT);
        if response.clicked() && !on {
            set_param(setter, &params.style, choice);
        }
    }
}

// --- Menu a tendina ------------------------------------------------------------------

fn menu_item(ui: &Ui, painter: &Painter, rect: Rect, id: Id, text: &str, selected: bool, size: f32) -> bool {
    let response = ui.interact(rect, id, Sense::click());
    if selected {
        painter.rect_filled(rect, CornerRadius::same(6), Color32::from_rgb(0x0b, 0x3a, 0x62));
        painter.rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, Color32::from_rgb(0x4c, 0xc8, 0xf0)), StrokeKind::Inside);
    } else if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(6), Color32::from_white_alpha(10));
    }
    painter.text(rect.left_center() + Vec2::new(10.0, 0.0), Align2::LEFT_CENTER, text, font(size, Weight::Regular), TEXT);
    response.clicked()
}

fn menu_frame(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect.translate(Vec2::new(0.0, 6.0)), CornerRadius::same(12), Color32::from_black_alpha(120));
    painter.rect(rect, CornerRadius::same(12), MENU_BG, Stroke::new(1.2, Color32::from_rgb(0x24, 0x3a, 0x56)), StrokeKind::Inside);
}

fn menus(ctx: &egui::Context, g: Geo, params: &SosaTuneParams, setter: &ParamSetter, state: &mut GuiState) {
    let Some((menu, trigger)) = state.menu else { return };
    let (pos, size) = match menu {
        Menu::Voice => (g.p(420.0, 186.0), Vec2::new(g.d(250.0), g.d(170.0))),
        Menu::KeyScale => (g.p(684.0, 186.0), Vec2::new(g.d(430.0), g.d(420.0))),
        Menu::Preset => (g.p(930.0, 188.0), Vec2::new(g.d(468.0), g.d(30.0 + 52.0 * FACTORY.len() as f32))),
    };
    let popup = Rect::from_min_size(pos, size);
    let mut close = false;
    egui::Area::new(Id::new("menu-tendina"))
        .order(Order::Foreground)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            ui.allocate_exact_size(size, Sense::hover());
            let painter = ui.painter().clone();
            menu_frame(&painter, popup);
            match menu {
                Menu::Voice => {
                    for (i, voice) in [VoiceChoice::Soprano, VoiceChoice::AltoTenor, VoiceChoice::Low].into_iter().enumerate() {
                        let rect = Rect::from_min_size(pos + Vec2::new(g.d(12.0), g.d(14.0 + 50.0 * i as f32)), Vec2::new(size.x - g.d(24.0), g.d(44.0)));
                        if menu_item(ui, &painter, rect, Id::new(("voce", i)), voice_label(voice), params.voice.value() == voice, 23.0) {
                            set_param(setter, &params.voice, voice);
                            close = true;
                        }
                    }
                }
                Menu::KeyScale => {
                    painter.text(pos + Vec2::new(g.d(20.0), g.d(26.0)), Align2::LEFT_CENTER, "Key", font(19.0, Weight::Regular), TEXT_DIM);
                    let current_key = params.key.value().semitone() as usize;
                    for i in 0..12 {
                        let (col, row) = (i % 6, i / 6);
                        let rect = Rect::from_min_size(
                            pos + Vec2::new(g.d(14.0 + 67.0 * col as f32), g.d(44.0 + 54.0 * row as f32)),
                            Vec2::new(g.d(62.0), g.d(48.0)),
                        );
                        let response = ui.interact(rect, Id::new(("tonalita", i)), Sense::click());
                        let selected = i == current_key;
                        if selected {
                            painter.rect_filled(rect, CornerRadius::same(6), Color32::from_rgb(0x0b, 0x3a, 0x62));
                            painter.rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, Color32::from_rgb(0x4c, 0xc8, 0xf0)), StrokeKind::Inside);
                        } else if response.hovered() {
                            painter.rect_filled(rect, CornerRadius::same(6), Color32::from_white_alpha(10));
                        }
                        note_name(&painter, rect.center(), i, 23.0, Weight::Medium, TEXT);
                        if response.clicked() {
                            set_param(setter, &params.key, key_from_index(i));
                            if matches!(params.scale.value(), ScaleChoice::Chromatic | ScaleChoice::Custom) {
                                set_param(setter, &params.scale, ScaleChoice::Major);
                            }
                        }
                    }
                    painter.text(pos + Vec2::new(g.d(20.0), g.d(176.0)), Align2::LEFT_CENTER, "Scale", font(19.0, Weight::Regular), TEXT_DIM);
                    let scales = [
                        ScaleChoice::Major,
                        ScaleChoice::NaturalMinor,
                        ScaleChoice::HarmonicMinor,
                        ScaleChoice::MajorPentatonic,
                        ScaleChoice::MinorPentatonic,
                        ScaleChoice::Blues,
                        ScaleChoice::Chromatic,
                        ScaleChoice::Custom,
                    ];
                    for (i, scale) in scales.into_iter().enumerate() {
                        let (col, row) = (i % 2, i / 2);
                        let rect = Rect::from_min_size(
                            pos + Vec2::new(g.d(14.0 + 203.0 * col as f32), g.d(194.0 + 54.0 * row as f32)),
                            Vec2::new(g.d(198.0), g.d(48.0)),
                        );
                        if menu_item(ui, &painter, rect, Id::new(("scala", i)), scale_label(scale), params.scale.value() == scale, 21.0) {
                            set_param(setter, &params.scale, scale);
                            close = true;
                        }
                    }
                }
                Menu::Preset => {
                    let current = params.preset.load(Ordering::Relaxed) % FACTORY.len();
                    for (i, preset) in FACTORY.iter().enumerate() {
                        let rect = Rect::from_min_size(pos + Vec2::new(g.d(12.0), g.d(15.0 + 52.0 * i as f32)), Vec2::new(size.x - g.d(24.0), g.d(46.0)));
                        let selected = i == current && preset.matches(params);
                        if menu_item(ui, &painter, rect, Id::new(("preset", i)), preset.name, selected, 22.0) {
                            apply_preset(params, setter, i);
                            close = true;
                        }
                    }
                }
            }
        });
    // Clic fuori dal menu (e fuori dal campo che l'ha aperto): si chiude.
    let clicked_outside = ctx.input(|i| {
        i.pointer.any_click()
            && i.pointer.interact_pos().is_some_and(|p| !popup.contains(p) && !trigger.contains(p))
    });
    if close || clicked_outside {
        state.menu = None;
    }
}

// --- Guida e impostazioni ----------------------------------------------------------------

fn overlays(ctx: &egui::Context, g: Geo, params: &SosaTuneParams, display: &PitchDisplay, setter: &ParamSetter, state: &mut GuiState) {
    let Some(overlay) = state.overlay else { return };
    let rect = g.r(70.0, 205.0, 1280.0, 690.0);
    let mut close = false;
    egui::Area::new(Id::new("pannello"))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            ui.allocate_exact_size(rect.size(), Sense::hover());
            let painter = ui.painter().clone();
            // Blocca i clic verso i comandi sotto il pannello.
            let _ = ui.interact(rect, Id::new("pannello-sfondo"), Sense::click());
            painter.rect_filled(rect.translate(Vec2::new(0.0, 8.0)), CornerRadius::same(18), Color32::from_black_alpha(140));
            rounded_gradient(&painter, rect, [g.d(22.0); 4], Color32::from_rgb(0x0a, 0x16, 0x26), Color32::from_rgb(0x05, 0x0c, 0x16));
            rounded_stroke(&painter, rect, [g.d(22.0); 4], Stroke::new(1.4, Color32::from_rgb(0x2a, 0x48, 0x6c)));

            // Chiudi.
            let x_center = g.p(1240.0, 245.0);
            let x_hit = Rect::from_center_size(x_center, Vec2::splat(g.d(50.0)));
            let x_response = ui.interact(x_hit, Id::new("chiudi"), Sense::click()).on_hover_text("Chiudi");
            let stroke = Stroke::new(g.d(2.4), if x_response.hovered() { TEXT } else { TEXT_DIM });
            painter.line_segment([x_center + Vec2::splat(-10.0 * SCALE), x_center + Vec2::splat(10.0 * SCALE)], stroke);
            painter.line_segment([x_center + Vec2::new(-10.0, 10.0) * SCALE, x_center + Vec2::new(10.0, -10.0) * SCALE], stroke);
            if x_response.clicked() {
                close = true;
            }

            match overlay {
                Overlay::Help => help_page(&painter, g),
                Overlay::Settings => settings_page(ui, &painter, g, params, display, setter),
            }
        });
    if close {
        state.overlay = None;
    }
}

fn help_page(painter: &Painter, g: Geo) {
    painter.text(g.p(110.0, 250.0), Align2::LEFT_CENTER, "Guida rapida", font(30.0, Weight::Medium), TEXT);
    let lines = [
        ("Vocal Range", "il tipo di voce (decide la nota più grave riconosciuta)."),
        ("Key & Scale", "tonalità e scala: la voce va solo sulle note illuminate."),
        ("AutoKey Listen", "accendilo e fai suonare la voce: la tonalità si imposta da sola."),
        ("Edit Notes", "clic su una nota per includerla o escluderla dalla scala."),
        ("Indicatore", "di quanti centesimi la voce è calante (−) o crescente (+)."),
        ("Retune Speed", "0 = robotico, 10-30 = pop, 50-150 = naturale (ms)."),
        ("Humanize / Flex Tune", "note lunghe più libere / corregge solo vicino alla nota."),
        ("Detune", "accordatura del La4 (440 Hz standard)."),
        ("Mode", "Low Latency (21 ms) per cantare in diretta, HQ (43 ms) per il mix."),
        ("Modern / Classic", "note già intonate / ogni nota scivola sulla nota giusta."),
        ("↶ ↷  ⚙  ⏻", "annulla e ripeti, altre impostazioni, accensione."),
    ];
    for (i, (name, text)) in lines.iter().enumerate() {
        let y = 300.0 + i as f32 * 34.0;
        painter.text(g.p(110.0, y), Align2::LEFT_CENTER, *name, font(21.0, Weight::Medium), CYAN);
        painter.text(g.p(390.0, y), Align2::LEFT_CENTER, *text, font(21.0, Weight::Regular), TEXT);
    }
    txt(painter, 
        g.p(110.0, 668.0),
        Align2::LEFT_CENTER,
        "Manopole: trascina in su o in giù (Maiusc = ritocchi fini), doppio clic = valore iniziale.",
        font(19.0, Weight::Regular),
        TEXT_DIM,
    );
}

fn settings_page(ui: &Ui, painter: &Painter, g: Geo, params: &SosaTuneParams, display: &PitchDisplay, setter: &ParamSetter) {
    painter.text(g.p(110.0, 250.0), Align2::LEFT_CENTER, "Impostazioni", font(30.0, Weight::Medium), TEXT);
    let knobs: [(&str, String, f32, &str); 5] = [
        ("Amount", format!("{:.0}%", params.amount.value() * 100.0), params.amount.unmodulated_normalized_value(), "Quanta correzione applicare: 100% = completa."),
        ("Transpose", format!("{:+} st", params.transpose.value()), params.transpose.unmodulated_normalized_value(), "Sposta tutta la voce di un certo numero di semitoni."),
        ("Vibrato", format!("{:+.0}%", params.vibrato.value() * 100.0), params.vibrato.unmodulated_normalized_value(), "Vibrato naturale: a sinistra lo riduce, a destra lo aggiunge."),
        ("Formant", format!("{:+.1} st", params.formant.value()), params.formant.unmodulated_normalized_value(), "Il timbro della voce, senza cambiare la nota: a destra più piccola e chiara, a sinistra più grande e scura."),
        ("Sensitivity", format!("{:.0}%", params.tracking.value() * 100.0), params.tracking.unmodulated_normalized_value(), "Quanto il plugin è disposto a riconoscere una nota: alzala per voci con molta aria."),
    ];
    for (i, (label, text, value, help)) in knobs.iter().enumerate() {
        let x = 230.0 + i as f32 * 205.0;
        let center = g.p(x, 400.0);
        painter.text(g.p(x, 320.0), Align2::CENTER_CENTER, *label, font(22.0, Weight::Regular), Color32::from_rgb(0xdc, 0xe2, 0xec));
        let style = match i {
            0 => ArcStyle::Cyan,
            2 => ArcStyle::Violet,
            _ => ArcStyle::White,
        };
        knob_visual(painter, g, center, g.d(40.0), *value, style);
        txt(painter, g.p(x, 480.0), Align2::CENTER_CENTER, text, font(24.0, Weight::Medium), TEXT);
        let hit = g.r(x - 70.0, 300.0, x + 70.0, 500.0);
        match i {
            0 => drag_param(ui, setter, &params.amount, hit, "amount", help),
            1 => drag_param(ui, setter, &params.transpose, hit, "transpose", help),
            2 => drag_param(ui, setter, &params.vibrato, hit, "vibrato", help),
            3 => drag_param(ui, setter, &params.formant, hit, "formant", help),
            _ => drag_param(ui, setter, &params.tracking, hit, "tracking", help),
        };
    }
    txt(painter, 
        g.p(110.0, 570.0),
        Align2::LEFT_CENTER,
        format!(
            "Latenza: {:.1} ms ({})",
            display.latency_ms(),
            if params.quality.value() == QualityChoice::Hq { "HQ" } else { "Low Latency" }
        ),
        font(21.0, Weight::Regular),
        TEXT_DIM,
    );
    txt(painter, 
        g.p(110.0, 610.0),
        Align2::LEFT_CENTER,
        "Il DAW compensa la latenza da solo. Tutti i comandi sono automatizzabili dal DAW.",
        font(19.0, Weight::Regular),
        TEXT_FAINT,
    );
}

// --- AutoKey ----------------------------------------------------------------------------

/// "AutoKey Listen": una volta al secondo guarda quanto e' stata cantata ogni
/// nota e, se la tonalita' e' chiara, imposta Key & Scale.
fn autokey(params: &SosaTuneParams, display: &PitchDisplay, setter: &ParamSetter, state: &mut GuiState, now: f64) {
    if !params.autokey.load(Ordering::Relaxed) || now - state.autokey_checked < 1.0 {
        return;
    }
    state.autokey_checked = now;
    let Some((tonic, minor, confidence)) = detect_key(&display.note_profile()) else { return };
    if confidence < 0.5 {
        return;
    }
    let scale = if minor { ScaleChoice::NaturalMinor } else { ScaleChoice::Major };
    let key = key_from_index(tonic);
    if params.key.value() != key || params.scale.value() != scale {
        set_param(setter, &params.key, key);
        set_param(setter, &params.scale, scale);
        state.autokey_found = Some((format!("{} {}", KEY_NAMES[tonic], scale_label(scale)), now));
    }
}

// --- Diagnostica -------------------------------------------------------------------------

/// Registro diagnostico: scrive in
/// `~/Library/Logs/DiagnosticReports/SosaTune-diagnostica.log` cosa succede
/// quando si usano i comandi, una riga al secondo e solo quando qualcosa
/// cambia. Serve a capire, su un computer reale, dove si perde un comando.
mod diagnostics {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::AtomicU64;
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
        presses: u64,
        frames: u64,
    }

    impl Diagnostics {
        pub fn new(enabled: bool) -> Self {
            Self { enabled, opened: None, last_check: None, last_line: String::new(), lines: 0, presses: 0, frames: 0 }
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
                self.write(&format!("=== SosaTune {} - finestra aperta ===", env!("CARGO_PKG_VERSION")));
            }
            self.frames += 1;
            ctx.input(|i| {
                for event in &i.raw.events {
                    if let egui::Event::PointerButton { pressed: true, .. } = event {
                        self.presses += 1;
                    }
                }
            });
            if self.last_check.is_some_and(|t| t.elapsed().as_secs_f32() < 1.0) {
                return;
            }
            self.last_check = Some(Instant::now());
            let line = format!(
                "clic={} | manopole: inizio={} valori={} clic={} | retune={:.3} detune={:.1} | {}",
                self.presses,
                DRAG_STARTS.load(Ordering::Relaxed),
                DRAG_SETS.load(Ordering::Relaxed),
                CLICK_SETS.load(Ordering::Relaxed),
                params.retune_speed.unmodulated_normalized_value(),
                params.detune.value(),
                nih_plug::sosatune_diag::summary(),
            );
            if line != self.last_line {
                self.write(&format!("{line} | fotogrammi={}", self.frames));
                self.last_line = line;
            }
        }
    }
}

// --- Posizioni per le prove dell'interfaccia --------------------------------------------

/// Dove si trovano i comandi (in punti della finestra), per le prove in `preview.rs`.
#[cfg(test)]
pub(crate) mod layout {
    use super::*;

    fn at(x: f32, y: f32) -> Pos2 {
        Geo { origin: Pos2::ZERO }.p(x, y)
    }

    pub fn retune() -> Pos2 {
        at(367.0, 943.0)
    }
    pub fn humanize() -> Pos2 {
        at(573.0, 943.0)
    }
    pub fn flex() -> Pos2 {
        at(787.0, 943.0)
    }
    pub fn detune() -> Pos2 {
        at(243.0, 928.0)
    }
    pub fn mode_hq() -> Pos2 {
        at(1154.0, 943.0)
    }
    pub fn mode_low() -> Pos2 {
        at(1030.0, 943.0)
    }
    pub fn classic() -> Pos2 {
        at(1258.0, 953.0)
    }
    pub fn modern() -> Pos2 {
        at(1258.0, 897.0)
    }
    pub fn power() -> Pos2 {
        at(1338.0, 592.0)
    }
    pub fn undo() -> Pos2 {
        at(1338.0, 245.0)
    }
    pub fn redo() -> Pos2 {
        at(1338.0, 328.0)
    }
    pub fn settings() -> Pos2 {
        at(1338.0, 495.0)
    }
    pub fn help() -> Pos2 {
        at(1338.0, 412.0)
    }
    pub fn note(pc: usize) -> Pos2 {
        note_center(Geo { origin: Pos2::ZERO }, pc)
    }
    pub fn preset_next() -> Pos2 {
        at(1288.0, 135.0)
    }
    pub fn preset_prev() -> Pos2 {
        at(1216.0, 135.0)
    }
    pub fn preset_list() -> Pos2 {
        at(1356.0, 135.0)
    }
    pub fn voice_field() -> Pos2 {
        at(520.0, 140.0)
    }
    pub fn key_field() -> Pos2 {
        at(780.0, 140.0)
    }
    pub fn autokey() -> Pos2 {
        at(200.0, 253.0)
    }
    /// Una voce del menu Vocal Range (0 = Soprano, 1 = Alto/Tenor, 2 = Low Male).
    pub fn voice_item(i: usize) -> Pos2 {
        at(540.0, 186.0 + 14.0 + 50.0 * i as f32 + 22.0)
    }
    /// Una tonalita' (0 = C ... 11 = B) nel menu Key & Scale.
    pub fn key_item(i: usize) -> Pos2 {
        let (col, row) = (i % 6, i / 6);
        at(684.0 + 14.0 + 67.0 * col as f32 + 31.0, 186.0 + 44.0 + 54.0 * row as f32 + 24.0)
    }
    /// Una scala nel menu Key & Scale (0 = Major, 1 = Minor, ...).
    pub fn scale_item(i: usize) -> Pos2 {
        let (col, row) = (i % 2, i / 2);
        at(684.0 + 14.0 + 203.0 * col as f32 + 99.0, 186.0 + 194.0 + 54.0 * row as f32 + 24.0)
    }
    pub fn preset_item(i: usize) -> Pos2 {
        at(1164.0, 188.0 + 15.0 + 52.0 * i as f32 + 23.0)
    }
    /// Le manopole del pannello impostazioni (0 = Amount ... 4 = Sensitivity).
    pub fn settings_knob(i: usize) -> Pos2 {
        at(230.0 + i as f32 * 205.0, 400.0)
    }
}
