//! L'interfaccia grafica di SR Vocal Suite, disegnata con egui.
//!
//! La grafica e' l'immagine di riferimento, 1:1: la "pelle"
//! (`assets/skin.png`, creata da `tools/make_skin.py`) e' l'immagine senza le
//! parti che cambiano, e qui sopra si disegnano, nello stesso punto e con lo
//! stesso stile, i valori dei menu, la tonalita' nella sfera, l'arco e il
//! pallino delle manopole con il numero, i pallini di Retune Speed e
//! Humanize, la curva dell'intonazione, i meter e i pulsanti Bypass/Monitor.
//! Tutte le misure sono in pixel dell'immagine (vedi `layout.rs`) e vengono
//! scalate alla finestra con [`Geo`].
//!
//! Comandi:
//! - manopole e sfere: trascina in su/giu' (Maiusc = fine), doppio clic =
//!   valore iniziale, clic sul numero per scriverlo;
//! - Vocal Range, Key & Scale (anche clic sulla sfera), preset (‹ ›, clic sul
//!   nome = lista, clic su "440 Hz" = accordatura), ↶ ↷, ☰ (preset, Save as
//!   Default, guida), ⚙ (tutte le impostazioni di correzione ed effetti);
//! - Bypass, Monitor (Low Latency per registrare), Note Stabilizer.

use crate::gui::create_egui_editor;
use crate::gui::egui::{
    self, epaint::Mesh, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Id, Order, Painter, Pos2,
    Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};
use crate::layout::{self as lay, R};
use crate::params::{
    ChorusModeChoice, DelaySync, Key, QualityChoice, ScaleChoice, Stabilizer, StyleChoice, VocalSuiteParams,
    VoiceChoice, EDITOR_HEIGHT, EDITOR_WIDTH,
};
use crate::presets::FACTORY;
use crate::shared::{KeySource, PitchDisplay, HISTORY_LEN};
use nih_plug::prelude::*;
use sosatune_dsp::chroma::relative_key;
use sosatune_dsp::fx::space_echo::mode_heads;
use std::f32::consts::TAU;
use std::sync::atomic::Ordering;
use std::sync::Arc;

// --- Geometria ---------------------------------------------------------------------

thread_local! {
    static UI_SCALE: std::cell::Cell<f32> = const { std::cell::Cell::new(EDITOR_WIDTH as f32 / lay::REF_W) };
}

fn scale() -> f32 {
    UI_SCALE.with(|s| s.get())
}

fn set_scale(window: Vec2) {
    let s = (window.x / lay::REF_W).min(window.y / lay::REF_H).max(0.05);
    UI_SCALE.with(|c| c.set(s));
}

#[derive(Clone, Copy)]
struct Geo {
    origin: Pos2,
}

impl Geo {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        self.origin + Vec2::new(x * scale(), y * scale())
    }
    fn pt(&self, xy: (f32, f32)) -> Pos2 {
        self.p(xy.0, xy.1)
    }
    fn d(&self, v: f32) -> f32 {
        v * scale()
    }
    fn r(&self, r: R) -> Rect {
        Rect::from_min_max(self.p(r.0, r.1), self.p(r.2, r.3))
    }
}

// --- Colori (campionati dall'immagine) ---------------------------------------------

const TEXT: Color32 = Color32::from_rgb(0xec, 0xf1, 0xf8);
const TEXT_DIM: Color32 = Color32::from_rgb(0xa8, 0xb4, 0xc6);
const TEXT_FAINT: Color32 = Color32::from_rgb(0x6c, 0x7a, 0x8e);
const CYAN: Color32 = Color32::from_rgb(0x5c, 0xd4, 0xff);
const BLUE: Color32 = Color32::from_rgb(0x3a, 0x9c, 0xff);
const MENU_BG: Color32 = Color32::from_rgb(0x07, 0x10, 0x1b);
const PANEL_EDGE: Color32 = Color32::from_rgb(0x2a, 0x48, 0x6c);

/// Colore che si "somma" alla luce sotto (bagliori).
fn light(c: Color32, k: f32) -> Color32 {
    let k = k.clamp(0.0, 1.0);
    Color32::from_rgba_premultiplied((c.r() as f32 * k) as u8, (c.g() as f32 * k) as u8, (c.b() as f32 * k) as u8, 0)
}

fn rgb(c: (u8, u8, u8)) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()), mix(a.a(), b.a()))
}

fn alpha(c: Color32, a: f32) -> Color32 {
    let a = a.clamp(0.0, 1.0);
    Color32::from_rgba_premultiplied(
        (c.r() as f32 * a) as u8,
        (c.g() as f32 * a) as u8,
        (c.b() as f32 * a) as u8,
        (c.a() as f32 * a) as u8,
    )
}

// --- Caratteri -------------------------------------------------------------------------

fn load_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let add = |fonts: &mut egui::FontDefinitions, name: &str, data: &'static [u8]| {
        fonts.font_data.insert(name.to_owned(), Arc::new(egui::FontData::from_static(data)));
    };
    add(&mut fonts, "inter", include_bytes!("../assets/Inter-Regular.ttf"));
    add(&mut fonts, "inter-medium", include_bytes!("../assets/Inter-Medium.ttf"));
    add(&mut fonts, "inter-semibold", include_bytes!("../assets/Inter-SemiBold.ttf"));
    add(&mut fonts, "inter-light", include_bytes!("../assets/InterDisplay-ExtraLight.ttf"));
    // La tonalita' nella sfera: Poppins (SIL OFL, `assets/Poppins-OFL.txt`).
    add(&mut fonts, "poppins-light", include_bytes!("../assets/Poppins-Light.ttf"));
    add(&mut fonts, "poppins-medium", include_bytes!("../assets/Poppins-Medium.ttf"));
    let fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
        family.insert(0, "inter".to_owned());
    }
    for (family, font) in [
        ("medium", "inter-medium"),
        ("semibold", "inter-semibold"),
        ("light", "inter-light"),
        ("display-light", "poppins-light"),
        ("display-medium", "poppins-medium"),
    ] {
        let mut list = vec![font.to_owned()];
        list.extend(fallbacks.iter().cloned());
        fonts.families.insert(FontFamily::Name(family.into()), list);
    }
    ctx.set_fonts(fonts);
}

#[derive(Clone, Copy)]
enum Weight {
    #[allow(dead_code)]
    Light,
    Regular,
    Medium,
    SemiBold,
    /// Poppins Light (lettera nella sfera).
    DisplayLight,
    /// Poppins Medium ("MAJOR").
    DisplayMedium,
}

fn font(size: f32, weight: Weight) -> FontId {
    let size = size * scale();
    match weight {
        Weight::Regular => FontId::proportional(size),
        Weight::Medium => FontId::new(size, FontFamily::Name("medium".into())),
        Weight::SemiBold => FontId::new(size, FontFamily::Name("semibold".into())),
        Weight::Light => FontId::new(size, FontFamily::Name("light".into())),
        Weight::DisplayLight => FontId::new(size, FontFamily::Name("display-light".into())),
        Weight::DisplayMedium => FontId::new(size, FontFamily::Name("display-medium".into())),
    }
}

fn text_tracked(painter: &Painter, pos: Pos2, align: Align2, text: impl ToString, font: FontId, color: Color32, tracking: f32) -> Rect {
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

fn txt(painter: &Painter, pos: Pos2, align: Align2, text: impl ToString, font: FontId, color: Color32) -> Rect {
    text_tracked(painter, pos, align, text, font, color, -0.01)
}

fn text_width(painter: &Painter, text: &str, font: FontId, tracking: f32) -> f32 {
    let mut job = egui::text::LayoutJob::default();
    let size = font.size;
    job.append(text, 0.0, egui::TextFormat { font_id: font, extra_letter_spacing: tracking * size, ..Default::default() });
    painter.layout_job(job).size().x
}

/// Testo con le lettere distanziate (come "MAJOR"), centrato su `center`.
fn spaced_centered(painter: &Painter, center: Pos2, text: &str, font: FontId, color: Color32, spacing: f32) {
    let galleys: Vec<_> = text.chars().map(|c| painter.layout_no_wrap(c.to_string(), font.clone(), color)).collect();
    let natural: f32 = galleys.iter().map(|g| g.size().x).sum();
    let gap = spacing * font.size;
    let total = natural + gap * (galleys.len().saturating_sub(1)) as f32;
    let mut x = center.x - total / 2.0;
    for galley in galleys {
        let size = galley.size();
        painter.galley(Pos2::new(x, center.y - size.y / 2.0), galley, color);
        x += size.x + gap;
    }
}

// --- Primitive di disegno ------------------------------------------------------------------

fn radial(painter: &Painter, center: Pos2, r0: f32, r1: f32, c0: Color32, c1: Color32) {
    let segments = ((r1 * 0.9) as usize).clamp(24, 128);
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

/// Un alone luminoso rotondo.
fn glow_dot(painter: &Painter, center: Pos2, r: f32, color: Color32, k: f32) {
    radial(painter, center, 0.0, r, light(color, k), Color32::TRANSPARENT);
}

/// Punto sulla circonferenza: angolo in gradi dall'alto, in senso orario.
fn polar(center: Pos2, radius: f32, deg: f32) -> Pos2 {
    let a = deg.to_radians();
    center + Vec2::new(a.sin(), -a.cos()) * radius
}

/// Un arco (striscia) dall'angolo `from` a `to`, con colore e spessore.
fn arc_band(painter: &Painter, center: Pos2, radius: f32, from: f32, to: f32, width: f32, inner: Color32, outer: Color32) {
    if to <= from {
        return;
    }
    let steps = (((to - from) / 2.0).ceil() as usize).max(2);
    let mut mesh = Mesh::default();
    for i in 0..=steps {
        let a = from + (to - from) * i as f32 / steps as f32;
        mesh.colored_vertex(polar(center, radius - width / 2.0, a), inner);
        mesh.colored_vertex(polar(center, radius, a), outer);
        mesh.colored_vertex(polar(center, radius + width / 2.0, a), inner);
    }
    for i in 0..steps as u32 {
        let k = i * 3;
        for (a, b) in [(0, 1), (1, 2)] {
            mesh.add_triangle(k + a, k + b, k + 3 + a);
            mesh.add_triangle(k + b, k + 3 + b, k + 3 + a);
        }
    }
    painter.add(Shape::mesh(mesh));
}

fn rounded_gradient(painter: &Painter, rect: Rect, radius: f32, top: Color32, bottom: Color32) {
    // Contorno arrotondato, colorato dall'alto in basso.
    let mut pts = Vec::new();
    let r = radius.min(rect.height() / 2.0).min(rect.width() / 2.0);
    let corners = [
        (rect.right_top() + Vec2::new(-r, r), -90.0f32),
        (rect.right_bottom() + Vec2::new(-r, -r), 0.0),
        (rect.left_bottom() + Vec2::new(r, -r), 90.0),
        (rect.left_top() + Vec2::new(r, r), 180.0),
    ];
    for (c, start) in corners {
        for i in 0..=8 {
            let a = (start + 90.0 * i as f32 / 8.0).to_radians();
            pts.push(c + Vec2::new(a.cos(), a.sin()) * r);
        }
    }
    let mut mesh = Mesh::default();
    let center = rect.center();
    let color_at = |p: Pos2| lerp_color(top, bottom, (p.y - rect.top()) / rect.height().max(1.0));
    mesh.colored_vertex(center, color_at(center));
    for p in &pts {
        mesh.colored_vertex(*p, color_at(*p));
    }
    let n = pts.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    painter.add(Shape::mesh(mesh));
}

// --- Parametri dall'interfaccia --------------------------------------------------------------

fn set_param<P: Param>(setter: &ParamSetter, param: &P, value: P::Plain) {
    setter.begin_set_parameter(param);
    setter.set_parameter(param, value);
    setter.end_set_parameter(param);
}

/// Trascinamento verticale (Maiusc = fine), doppio clic = valore iniziale.
fn drag_param<P: Param>(ui: &Ui, setter: &ParamSetter, param: &P, hit: Rect, id: &str, help: &str) -> egui::Response {
    let response = ui
        .interact(hit, Id::new(("manopola", id)), Sense::click_and_drag())
        .on_hover_cursor(CursorIcon::ResizeVertical)
        .on_hover_text(help);
    let memory_id = response.id.with("trascinamento");
    if response.drag_started() {
        setter.begin_set_parameter(param);
        let start = (param.unmodulated_normalized_value(), 0.0f32);
        ui.memory_mut(|m| m.data.insert_temp(memory_id, start));
    }
    if response.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let speed = (if fine { 0.0012 } else { 0.005 }) / scale().max(0.2) * 0.7;
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
    // Rotellina del mouse.
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.1 {
            let step = scroll * 0.0015;
            setter.begin_set_parameter(param);
            setter.set_parameter_normalized(param, (param.unmodulated_normalized_value() + step).clamp(0.0, 1.0));
            setter.end_set_parameter(param);
        }
    }
    response
}

/// Cursore orizzontale (pannello impostazioni).
fn drag_param_h<P: Param>(ui: &Ui, setter: &ParamSetter, param: &P, track: Rect, hit: Rect, id: &str, help: &str) {
    let response = ui
        .interact(hit, Id::new(("cursore", id)), Sense::click_and_drag())
        .on_hover_cursor(CursorIcon::ResizeHorizontal)
        .on_hover_text(help);
    let to_norm = |x: f32| ((x - track.left()) / track.width().max(1.0)).clamp(0.0, 1.0);
    if response.drag_started() {
        setter.begin_set_parameter(param);
    }
    if response.dragged() || (response.clicked() && !response.double_clicked()) {
        if let Some(p) = response.interact_pointer_pos() {
            if response.clicked() {
                setter.begin_set_parameter(param);
            }
            setter.set_parameter_normalized(param, to_norm(p.x));
            if response.clicked() {
                setter.end_set_parameter(param);
            }
        }
    }
    if response.drag_stopped() {
        setter.end_set_parameter(param);
    }
    if response.double_clicked() {
        set_param_normalized(setter, param, param.default_normalized_value());
    }
}

fn set_param_normalized<P: Param>(setter: &ParamSetter, param: &P, value: f32) {
    setter.begin_set_parameter(param);
    setter.set_parameter_normalized(param, value);
    setter.end_set_parameter(param);
}

fn apply_preset(params: &VocalSuiteParams, setter: &ParamSetter, index: usize) {
    let index = index % FACTORY.len();
    let p = &FACTORY[index];
    set_param(setter, &params.retune_speed, p.retune_ms);
    set_param(setter, &params.humanize, p.humanize);
    set_param(setter, &params.flex_tune, p.flex_tune);
    set_param(setter, &params.style, p.style);
    set_param(setter, &params.stabilizer, p.stabilizer);
    set_param(setter, &params.chorus, p.chorus);
    set_param(setter, &params.formant, p.formant);
    set_param(setter, &params.reverb, p.reverb);
    set_param(setter, &params.delay, p.delay);
    set_param(setter, &params.chorus_mode, p.chorus_mode);
    set_param(setter, &params.chorus_blend, p.chorus_blend);
    set_param(setter, &params.delay_mode, p.delay_mode);
    set_param(setter, &params.delay_sync, p.delay_sync);
    set_param(setter, &params.delay_intensity, p.delay_intensity);
    set_param(setter, &params.reverb_decay, p.reverb_decay);
    params.preset.store(index, Ordering::Relaxed);
}

const KEY_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

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

fn key_scale_label(params: &VocalSuiteParams) -> String {
    match params.scale.value() {
        ScaleChoice::Chromatic => "Chromatic".to_owned(),
        ScaleChoice::Custom => "Custom".to_owned(),
        scale => format!("{} {}", KEY_NAMES[params.key.value().semitone() as usize], scale_label(scale)),
    }
}

/// Nome di una nota MIDI con l'ottava (60 = C4).
fn note_with_octave(midi: f32) -> String {
    let n = midi.round() as i32;
    format!("{}{}", KEY_NAMES[n.rem_euclid(12) as usize], n.div_euclid(12) - 1)
}

fn manual_key_choice(params: &VocalSuiteParams, state: &mut GuiState, now: f64) {
    if params.autokey.swap(false, Ordering::Relaxed) {
        state.toast = Some(("AutoKey spento: tonalità scelta a mano".to_owned(), now));
    }
}

// --- Annulla / ripeti ---------------------------------------------------------------------------

#[derive(Default)]
struct History {
    ptrs: Vec<ParamPtr>,
    committed: Vec<f32>,
    undo: Vec<Vec<f32>>,
    redo: Vec<Vec<f32>>,
    last_touch: f64,
    quiet_until: f64,
}

impl History {
    const LIMIT: usize = 100;

    fn snapshot(&self) -> Vec<f32> {
        // SAFETY: i puntatori vengono dai parametri del plugin, vivi quanto l'interfaccia.
        self.ptrs.iter().map(|p| unsafe { p.unmodulated_normalized_value() }).collect()
    }

    fn track(&mut self, ctx: &egui::Context, params: &VocalSuiteParams) {
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
            return;
        }
        if by_user {
            self.undo.push(std::mem::replace(&mut self.committed, current));
            if self.undo.len() > Self::LIMIT {
                self.undo.remove(0);
            }
            self.redo.clear();
        } else {
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

// --- Stato della grafica ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Menu {
    Voice,
    KeyScale,
    Preset,
    Main,
    Stabilizer,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Overlay {
    Settings,
    Help,
}

struct GuiState {
    skin: Option<egui::TextureHandle>,
    menu: Option<(Menu, Rect)>,
    overlay: Option<Overlay>,
    history: History,
    /// Livelli dei meter (0..1, con caduta lenta).
    meter_in: f32,
    meter_out: f32,
    /// Livello della voce per far "respirare" la sfera.
    level: f32,
    /// Ultima nota mostrata nel grafico (nota MIDI del bersaglio).
    shown_note: Option<f32>,
    note_light: f32,
    autokey_checked: f64,
    last_blocks: usize,
    editing: Option<Editing>,
    auto_voice: Option<(f64, VoiceChoice)>,
    auto_voice_checked: f64,
    reset_histogram: bool,
    /// AutoKey appena acceso: si ricomincia ad ascoltare da zero.
    reset_notes: bool,
    toast: Option<(String, f64)>,
}

struct Editing {
    id: &'static str,
    text: String,
    focus: bool,
}

fn new_state() -> GuiState {
    GuiState {
        skin: None,
        menu: None,
        overlay: None,
        history: History::default(),
        meter_in: 0.0,
        meter_out: 0.0,
        level: 0.0,
        shown_note: None,
        note_light: 0.0,
        autokey_checked: 0.0,
        last_blocks: 0,
        editing: None,
        auto_voice: None,
        auto_voice_checked: 0.0,
        reset_histogram: false,
        reset_notes: false,
        toast: None,
    }
}

/// La pelle dell'interfaccia (immagine PNG inclusa nel plugin).
fn load_skin(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let bytes: &[u8] = include_bytes!("../assets/skin.png");
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let image = match info.color_type {
        png::ColorType::Rgb => egui::ColorImage::from_rgb([w, h], &buf[..w * h * 3]),
        png::ColorType::Rgba => egui::ColorImage::from_rgba_unmultiplied([w, h], &buf[..w * h * 4]),
        _ => return None,
    };
    let options = egui::TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..egui::TextureOptions::LINEAR };
    Some(ctx.load_texture("sr-vocal-suite-skin", image, options))
}

// --- Finestra ------------------------------------------------------------------------------------------

/// Se la finestra non si apre (per esempio niente OpenGL) il plugin continua
/// a funzionare senza interfaccia invece di far chiudere il DAW.
struct SafeEditor(Box<dyn Editor>);

impl Editor for SafeEditor {
    fn spawn(&self, parent: ParentWindowHandle, context: Arc<dyn GuiContext>) -> Box<dyn std::any::Any + Send> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.0.spawn(parent, context))) {
            Ok(handle) => handle,
            Err(_) => {
                nih_error!("SR Vocal Suite: impossibile aprire l'interfaccia grafica, il plugin continua senza");
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

pub fn create(params: Arc<VocalSuiteParams>, display: Arc<PitchDisplay>) -> Option<Box<dyn Editor>> {
    let editor = create_egui_editor(
        params.editor_state.clone(),
        new_state(),
        |ctx, state| setup(ctx, state),
        move |ctx, setter, state| draw(ctx, &params, &display, setter, state),
    )?;
    Some(Box::new(SafeEditor(editor)))
}

fn setup(ctx: &egui::Context, state: &mut GuiState) {
    ctx.set_theme(egui::ThemePreference::Dark);
    let mut visuals = egui::Visuals::dark();
    visuals.popup_shadow = egui::epaint::Shadow::NONE;
    ctx.set_visuals(visuals);
    load_fonts(ctx);
    state.skin = load_skin(ctx);
}

/// Per l'anteprima e le prove: la stessa interfaccia, senza finestra.
pub(crate) struct Headless {
    state: GuiState,
}

impl Headless {
    pub(crate) fn new(ctx: &egui::Context) -> Self {
        let mut state = new_state();
        setup(ctx, &mut state);
        Self { state }
    }

    pub(crate) fn draw(&mut self, ctx: &egui::Context, params: &VocalSuiteParams, display: &PitchDisplay, setter: &ParamSetter) {
        draw(ctx, params, display, setter, &mut self.state);
    }
}

fn draw(ctx: &egui::Context, params: &VocalSuiteParams, display: &PitchDisplay, setter: &ParamSetter, state: &mut GuiState) {
    let screen = ctx.screen_rect();
    set_scale(screen.size());
    state.history.track(ctx, params);
    let now = ctx.input(|i| i.time);
    autokey(params, display, setter, state, now);
    auto_voice(params, display, setter, state, now);
    if state.skin.is_none() {
        state.skin = load_skin(ctx);
    }

    egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(0x05, 0x0b, 0x14))).show(ctx, |ui| {
        ui.allocate_exact_size(screen.size(), Sense::hover());
        // L'immagine resta nelle sue proporzioni, centrata.
        let size = Vec2::new(lay::REF_W, lay::REF_H) * scale();
        let origin = screen.min + (screen.size() - size) / 2.0;
        let g = Geo { origin };
        let painter = ui.painter().clone();
        if let Some(skin) = &state.skin {
            painter.image(skin.id(), Rect::from_min_size(origin, size), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        header(ui, &painter, g, params, setter, state, now);
        key_orb(ui, &painter, g, params, display, state, now);
        small_orbs(ui, &painter, g, params, setter);
        pitch_graph(&painter, g, params, display, state);
        control_bar(ui, &painter, g, params, display, setter, state, now);
        knobs(ui, &painter, g, params, setter, state);
        toast(&painter, g, state, now);
        resize_grip(ui, &painter, screen, params);
    });

    let size = Vec2::new(lay::REF_W, lay::REF_H) * scale();
    let g = Geo { origin: screen.min + (screen.size() - size) / 2.0 };
    menus(ctx, g, params, setter, state, now);
    overlays(ctx, g, params, display, setter, state, now);
    // Movimento continuo (sfera, grafico, meter): ridisegno a ogni battito.
    ctx.request_repaint();
}

// --- Intestazione ---------------------------------------------------------------------------------------------

fn hover_frame(painter: &Painter, rect: Rect, hovered: bool, g: Geo) {
    if hovered {
        painter.rect_filled(rect, CornerRadius::same(g.d(12.0) as u8), Color32::from_white_alpha(6));
    }
}

#[allow(clippy::too_many_arguments)]
fn header(ui: &Ui, painter: &Painter, g: Geo, params: &VocalSuiteParams, setter: &ParamSetter, state: &mut GuiState, now: f64) {
    // Vocal Range.
    let voice_rect = g.r(lay::VOICE_FIELD);
    let r = ui.interact(voice_rect, Id::new("vocal-range"), Sense::click()).on_hover_text("Tipo di voce: decide la nota più grave riconosciuta. Auto la sceglie ascoltando.");
    hover_frame(painter, voice_rect, r.hovered(), g);
    let voice_text = if state.auto_voice.is_some() { "Auto…".to_owned() } else { voice_label(params.voice.value()).to_owned() };
    text_tracked(painter, g.pt(lay::VOICE_VALUE), Align2::LEFT_CENTER, voice_text, font(17.0, Weight::Regular), TEXT, -0.03);
    if r.clicked() {
        state.menu = Some((Menu::Voice, voice_rect));
    }

    // Key & Scale.
    let key_rect = g.r(lay::KEY_FIELD);
    let r = ui.interact(key_rect, Id::new("key-scale"), Sense::click()).on_hover_text("Tonalità e scala: la voce va solo sulle note della scala.");
    hover_frame(painter, key_rect, r.hovered(), g);
    let label = key_scale_label(params);
    let size = if label.len() > 10 { 15.0 } else { 17.0 };
    text_tracked(painter, g.pt(lay::KEY_VALUE), Align2::LEFT_CENTER, label, font(size, Weight::Regular), TEXT, -0.03);
    if r.clicked() {
        state.menu = Some((Menu::KeyScale, key_rect));
    }

    // Preset: ‹ nome* accordatura ›, sotto il carattere.
    let current = params.preset.load(Ordering::Relaxed) % FACTORY.len();
    let preset = &FACTORY[current];
    let modified = !preset.matches(params);
    for (rect, step, id) in [(lay::PRESET_PREV, FACTORY.len() - 1, "preset-prev"), (lay::PRESET_NEXT, 1, "preset-next")] {
        let rect = g.r(rect);
        let r = ui.interact(rect, Id::new(id), Sense::click()).on_hover_text("Preset precedente / successivo");
        hover_frame(painter, rect, r.hovered(), g);
        if r.clicked() {
            apply_preset(params, setter, current + step);
        }
    }
    let name = format!("{}{}", preset.name, if modified { "*" } else { "" });
    let tuning = format!("{:.0} Hz", params.detune.value());
    let tuning = if (params.detune.value() - params.detune.value().round()).abs() > 0.05 { format!("{:.1} Hz", params.detune.value()) } else { tuning };
    let name_font = font(16.5, Weight::Regular);
    let tune_font = font(13.0, Weight::Regular);
    let gap = g.d(13.0);
    let name_w = text_width(painter, &name, name_font.clone(), -0.01);
    let tune_w = text_width(painter, &tuning, tune_font.clone(), -0.01);
    let line1 = g.pt(lay::PRESET_LINE1);
    let left = line1.x - (name_w + gap + tune_w) / 2.0;
    let name_rect = txt(painter, Pos2::new(left, line1.y), Align2::LEFT_CENTER, &name, name_font, TEXT);
    let tune_pos = Pos2::new(left + name_w + gap, line1.y + g.d(1.0));
    let tune_rect = Rect::from_min_size(tune_pos - Vec2::new(g.d(4.0), g.d(11.0)), Vec2::new(tune_w + g.d(8.0), g.d(22.0)));
    let tag_pos = g.pt(lay::PRESET_LINE2);
    let tag_rect = txt(painter, tag_pos, Align2::CENTER_CENTER, preset.tag, font(13.0, Weight::Regular), Color32::from_rgb(0x9a, 0xa8, 0xbc));
    // Accordatura: clic per scriverla (o trascinare).
    value_entry(
        ui,
        painter,
        setter,
        &params.detune,
        "detune",
        tune_rect,
        (tune_pos, Align2::LEFT_CENTER),
        tuning,
        tune_font,
        Color32::from_rgb(0xa2, 0xae, 0xc0),
        |v| v,
        state,
    );
    let list_rect = name_rect.union(tag_rect).expand(g.d(6.0));
    let r = ui.interact(list_rect, Id::new("preset-list"), Sense::click()).on_hover_text("Lista dei preset");
    if r.clicked() {
        state.menu = Some((Menu::Preset, g.r(lay::PRESET_BOX)));
    }

    // ↶ ↷ ☰ ⚙
    for (pos, id, help) in [
        (lay::UNDO, "undo", "Annulla"),
        (lay::REDO, "redo", "Ripeti"),
        (lay::MENU, "menu", "Preset, Save as Default, guida"),
        (lay::SETTINGS, "settings", "Impostazioni: correzione, chorus, delay, riverbero, finestra"),
    ] {
        let rect = Rect::from_center_size(g.pt(pos), Vec2::splat(g.d(48.0)));
        let r = ui.interact(rect, Id::new(id), Sense::click()).on_hover_text(help);
        if r.hovered() {
            glow_dot(painter, rect.center(), g.d(30.0), CYAN, 0.18);
        }
        if r.clicked() {
            match id {
                "undo" => state.history.undo(setter, now),
                "redo" => state.history.redo(setter, now),
                "menu" => state.menu = Some((Menu::Main, rect)),
                _ => state.overlay = if state.overlay == Some(Overlay::Settings) { None } else { Some(Overlay::Settings) },
            }
        }
    }
}

// --- Sfera della tonalita' ---------------------------------------------------------------------------------

fn key_orb(ui: &Ui, painter: &Painter, g: Geo, params: &VocalSuiteParams, display: &PitchDisplay, state: &mut GuiState, now: f64) {
    let (cx, cy, r) = lay::ORB;
    let center = g.p(cx, cy);
    // Respira con la voce.
    let (_, out_peak, blocks, _) = display.block_status();
    let target = if blocks != state.last_blocks { out_peak.min(1.0) } else { 0.0 };
    state.level += (target - state.level) * if target > state.level { 0.35 } else { 0.06 };
    let pulse = state.level.sqrt();
    if pulse > 0.01 {
        glow_dot(painter, center, g.d(r * 0.95), BLUE, 0.10 * pulse);
    }

    let tonic = params.key.value().semitone() as usize;
    let letter = KEY_NAMES[tonic].chars().next().unwrap_or('C');
    let sharp = KEY_NAMES[tonic].len() > 1;
    let scale_word = match params.scale.value() {
        ScaleChoice::Chromatic => "CHROMATIC",
        ScaleChoice::Major => "MAJOR",
        ScaleChoice::NaturalMinor => "MINOR",
        ScaleChoice::HarmonicMinor => "HARM. MINOR",
        ScaleChoice::MajorPentatonic => "MAJ. PENTA",
        ScaleChoice::MinorPentatonic => "MIN. PENTA",
        ScaleChoice::Blues => "BLUES",
        ScaleChoice::Custom => "CUSTOM",
    };
    // Lettera grande: Inter sottile, ingrossata appena (come nell'immagine), con alone.
    // Poppins: altezza delle maiuscole = 0,70 em.
    let size = lay::ORB_LETTER_CAP / 0.70;
    let f = font(size, Weight::DisplayLight);
    let galley = painter.layout_no_wrap(letter.to_string(), f.clone(), TEXT);
    let mark = sharp.then(|| painter.layout_no_wrap("#".to_owned(), font(size * 0.42, Weight::Regular), TEXT));
    let total_w = galley.size().x + mark.as_ref().map_or(0.0, |m| m.size().x * 0.95);
    let lc = g.pt(lay::ORB_LETTER);
    let top_left = Pos2::new(lc.x - total_w / 2.0, lc.y - galley.size().y / 2.0);
    glow_dot(painter, lc, g.d(95.0), Color32::from_rgb(0x50, 0x9c, 0xff), 0.22 + 0.15 * pulse);
    let thick = g.d(0.9);
    for (dx, dy) in [(-thick, 0.0), (thick, 0.0), (0.0, -thick * 0.6), (0.0, thick * 0.6), (0.0, 0.0)] {
        painter.galley(top_left + Vec2::new(dx, dy), galley.clone(), TEXT);
    }
    if let Some(m) = mark {
        let pos = Pos2::new(top_left.x + galley.size().x + g.d(2.0), top_left.y + galley.size().y * 0.14);
        painter.galley(pos, m, TEXT);
    }
    spaced_centered(painter, g.pt(lay::ORB_SCALE), scale_word, font(17.5, Weight::DisplayMedium), Color32::from_rgb(0xe8, 0xee, 0xf8), 0.36);

    // Clic sulla sfera: Key & Scale.
    let hit = Rect::from_center_size(center, Vec2::splat(g.d(r * 1.3)));
    let response = ui.interact(hit, Id::new("sfera"), Sense::click()).on_hover_text("Tonalità e scala (clic per cambiarle)");
    if response.clicked() {
        state.menu = Some((Menu::KeyScale, g.r(lay::KEY_FIELD)));
    }
    let _ = now;
}

// --- Retune Speed e Humanize ----------------------------------------------------------------------------------

fn knob_angle(norm: f32) -> f32 {
    lay::KNOB_ANGLE_MIN + (lay::KNOB_ANGLE_MAX - lay::KNOB_ANGLE_MIN) * norm.clamp(0.0, 1.0)
}

/// Il pallino bianco delle manopole e delle sfere.
fn white_dot(painter: &Painter, center: Pos2, r: f32, tint: Color32) {
    radial(painter, center, r * 0.6, r * 2.6, light(tint, 0.55), Color32::TRANSPARENT);
    radial(painter, center + Vec2::new(0.0, r * 0.25), r * 0.8, r * 1.35, Color32::from_black_alpha(70), Color32::TRANSPARENT);
    painter.circle_filled(center, r, Color32::from_rgb(0xf2, 0xf6, 0xfc));
    painter.circle_filled(center - Vec2::new(r * 0.25, r * 0.3), r * 0.45, Color32::from_rgba_premultiplied(255, 255, 255, 120));
}

fn small_orbs(ui: &Ui, painter: &Painter, g: Geo, params: &VocalSuiteParams, setter: &ParamSetter) {
    let orbs = [
        (lay::RETUNE_ORB, "retune", "Retune Speed: quanto in fretta la voce va sulla nota. 0 = robotico, 10-30 = pop, 50-150 = naturale (ms)."),
        (lay::HUMANIZE_ORB, "humanize", "Humanize: le note lunghe vengono corrette più dolcemente, così restano naturali."),
    ];
    for ((cx, cy, r, dot_r), id, help) in orbs {
        let center = g.p(cx, cy);
        let hit = Rect::from_center_size(center, Vec2::splat(g.d(r * 2.1)));
        let (response, norm, value) = if id == "retune" {
            let resp = drag_param(ui, setter, &params.retune_speed, hit, id, help);
            (resp, params.retune_speed.unmodulated_normalized_value(), format!("{:.0} ms", params.retune_speed.value()))
        } else {
            let resp = drag_param(ui, setter, &params.humanize, hit, id, help);
            (resp, params.humanize.unmodulated_normalized_value(), format!("{:.0}%", params.humanize.value() * 100.0))
        };
        white_dot(painter, polar(center, g.d(dot_r), knob_angle(norm)), g.d(9.5), CYAN);
        if response.hovered() || response.dragged() {
            txt(painter, center + Vec2::new(0.0, g.d(r * 0.62)), Align2::CENTER_CENTER, value, font(14.0, Weight::Medium), CYAN);
        }
    }
}

// --- Grafico dell'intonazione --------------------------------------------------------------------------------------

fn pitch_graph(base: &Painter, g: Geo, params: &VocalSuiteParams, display: &PitchDisplay, state: &mut GuiState) {
    let area = g.r(lay::GRAPH);
    let painter = base.with_clip_rect(area);
    let zero_y = g.p(0.0, lay::GRAPH_ZERO_Y).y;
    let px_per_cent = g.d(lay::GRAPH_PX_PER_CENT);
    let points = display.history();
    let written = display.written();
    let interval = display.interval_s().max(1e-3);
    // Finestra di ~3,5 s, il punto piu' recente a destra.
    let visible = ((3.5 / interval) as usize).clamp(64, HISTORY_LEN);
    let shown = &points[points.len().saturating_sub(visible)..];
    let offset = visible - shown.len();
    let x_at = |i: usize| area.left() + area.width() * (i + offset) as f32 / (visible - 1) as f32;
    // Indice assoluto del punto i (per i pallini che scorrono con il tempo).
    let first_abs = written - shown.len();

    painter.line_segment(
        [Pos2::new(area.left(), zero_y), Pos2::new(area.right(), zero_y)],
        Stroke::new(g.d(1.6), Color32::from_rgba_premultiplied(190, 215, 255, 150)),
    );

    // Curva: scarto in centesimi della voce dalla nota bersaglio.
    let mut segments: Vec<Vec<(Pos2, f32, usize)>> = Vec::new();
    let mut current: Vec<(Pos2, f32, usize)> = Vec::new();
    for (i, p) in shown.iter().enumerate() {
        if p.input.is_finite() && p.target.is_finite() {
            let cents = ((p.input - p.target) * 100.0).clamp(-62.0, 62.0);
            current.push((Pos2::new(x_at(i), zero_y - cents * px_per_cent), cents, first_abs + i));
        } else if !current.is_empty() {
            segments.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }
    // Media mobile su 5 punti (~25 ms): la curva resta fedele ma senza
    // tremolii da un'analisi all'altra.
    for seg in &mut segments {
        let raw: Vec<f32> = seg.iter().map(|s| s.1).collect();
        for (k, item) in seg.iter_mut().enumerate() {
            let lo = k.saturating_sub(2);
            let hi = (k + 3).min(raw.len());
            let c = raw[lo..hi].iter().sum::<f32>() / (hi - lo) as f32;
            item.1 = c;
            item.0.y = zero_y - c * px_per_cent;
        }
    }
    let glow = Color32::from_rgb(0x3c, 0x9a, 0xff);
    let every = ((0.22 / interval) as usize).max(4);
    for seg in &segments {
        if seg.len() < 2 {
            continue;
        }
        // Riempimento tra curva e zero.
        let mut mesh = Mesh::default();
        for (k, (p, cents, _)) in seg.iter().enumerate() {
            let a = (cents.abs() / 50.0).clamp(0.15, 1.0);
            mesh.colored_vertex(*p, light(glow, 0.55 * a));
            mesh.colored_vertex(Pos2::new(p.x, zero_y), light(glow, 0.05));
            if k > 0 {
                let i = (k as u32) * 2;
                mesh.add_triangle(i - 2, i - 1, i);
                mesh.add_triangle(i - 1, i + 1, i);
            }
        }
        painter.add(Shape::mesh(mesh));
        // Puntini "a matrice" intorno alla curva.
        let mut last_x = f32::MIN;
        for (p, cents, _) in seg.iter() {
            if p.x - last_x < g.d(6.0) {
                continue;
            }
            last_x = p.x;
            let spread = (cents.abs() * px_per_cent).max(g.d(8.0)) + g.d(14.0);
            let mut y = -spread;
            while y <= spread {
                let k = 1.0 - (y.abs() / spread);
                let pos = Pos2::new(p.x, zero_y + y);
                painter.rect_filled(Rect::from_center_size(pos, Vec2::splat(g.d(1.6))), CornerRadius::ZERO, light(Color32::from_rgb(0x8c, 0xc4, 0xff), 0.25 * k));
                y += g.d(5.0);
            }
        }
        // La curva: bagliore largo, poi il filo luminoso.
        let line: Vec<Pos2> = seg.iter().map(|(p, _, _)| *p).collect();
        painter.add(Shape::line(line.clone(), Stroke::new(g.d(9.0), light(glow, 0.22))));
        painter.add(Shape::line(line.clone(), Stroke::new(g.d(4.0), light(glow, 0.45))));
        painter.add(Shape::line(line, Stroke::new(g.d(1.8), Color32::from_rgb(0xd8, 0xec, 0xff))));
        // Pallini luminosi a intervalli regolari, che scorrono con il tempo.
        for (p, _, abs) in seg.iter() {
            if abs % every == 0 {
                glow_dot(&painter, *p, g.d(14.0), Color32::from_rgb(0x9c, 0xd0, 0xff), 0.7);
                painter.circle_filled(*p, g.d(3.4), Color32::from_rgb(0xf4, 0xf8, 0xff));
            }
        }
    }

    // Nome della nota (a sinistra): la nota bersaglio piu' recente.
    if let Some(last) = shown.iter().rev().take(8).find(|p| p.target.is_finite()) {
        state.shown_note = Some(last.target);
        state.note_light = 1.0;
    } else {
        state.note_light = (state.note_light - 0.01).max(0.35);
    }
    let label = state.shown_note.map(note_with_octave).unwrap_or_else(|| note_with_octave(60.0 + params.key.value().semitone() as f32));
    let color = lerp_color(TEXT_DIM, TEXT, state.note_light);
    txt(base, g.pt(lay::NOTE_LABEL), Align2::CENTER_CENTER, label, font(14.5, Weight::Regular), color);
}

// --- Barra dei comandi ---------------------------------------------------------------------------------------------

fn meter(painter: &Painter, g: Geo, area: R, segments: usize, level: f32) {
    let rect = g.r(area);
    let pitch = rect.width() / segments as f32;
    let bar_w = pitch * 0.6;
    let lit = (level.clamp(0.0, 1.0) * segments as f32).round() as usize;
    let cyan = Color32::from_rgb(0x7c, 0xd6, 0xfa);
    let white = Color32::from_rgb(0xf2, 0xfa, 0xff);
    let blue = Color32::from_rgb(0x60, 0x98, 0xd3);
    let off = Color32::from_rgb(0x2a, 0x3a, 0x4a);
    for i in 0..segments {
        let x = rect.left() + pitch * i as f32;
        let r = Rect::from_min_max(Pos2::new(x, rect.top()), Pos2::new(x + bar_w, rect.bottom()));
        let color = if i < lit {
            let t = i as f32 / lit.max(1) as f32;
            // Dal ciano al bianco verso il livello, poi blu sulle ultime.
            if t < 0.55 {
                lerp_color(cyan, white, t / 0.55)
            } else {
                lerp_color(white, blue, (t - 0.55) / 0.45 * 0.8)
            }
        } else {
            off
        };
        painter.rect_filled(r, CornerRadius::same(g.d(1.0) as u8), color);
        if i < lit {
            painter.rect_filled(r.expand(g.d(2.0)), CornerRadius::same(g.d(3.0) as u8), light(cyan, 0.08));
        }
    }
}

fn level_to_meter(peak: f32) -> f32 {
    let db = 20.0 * peak.max(1e-6).log10();
    ((db + 48.0) / 48.0).clamp(0.0, 1.0)
}

/// Pulsante a pillola (Bypass, Monitor).
fn pill(painter: &Painter, g: Geo, rect: Rect, on: bool, hovered: bool) {
    let radius = rect.height() / 2.0;
    if on {
        for k in 1..=4 {
            let e = g.d(2.5 * k as f32);
            painter.rect_filled(rect.expand(e), CornerRadius::same((radius + e) as u8), light(Color32::from_rgb(0x2c, 0x8c, 0xff), 0.07));
        }
        rounded_gradient(painter, rect, radius, Color32::from_rgb(0x0c, 0x26, 0x48), Color32::from_rgb(0x06, 0x16, 0x2c));
        painter.rect_stroke(rect, CornerRadius::same(radius as u8), Stroke::new(g.d(2.0), Color32::from_rgb(0x5a, 0xb4, 0xff)), StrokeKind::Inside);
        painter.rect_stroke(rect.shrink(g.d(2.0)), CornerRadius::same((radius - g.d(2.0)).max(0.0) as u8), Stroke::new(g.d(1.0), light(Color32::from_rgb(0x8c, 0xd0, 0xff), 0.35)), StrokeKind::Inside);
    } else {
        rounded_gradient(painter, rect, radius, Color32::from_rgb(0x0d, 0x17, 0x22), Color32::from_rgb(0x09, 0x11, 0x1a));
        let edge = if hovered { Color32::from_rgb(0x3c, 0x52, 0x6c) } else { Color32::from_rgb(0x1e, 0x2b, 0x3a) };
        painter.rect_stroke(rect, CornerRadius::same(radius as u8), Stroke::new(g.d(1.2), edge), StrokeKind::Inside);
    }
}

fn headphones_icon(painter: &Painter, g: Geo, c: Pos2, color: Color32) {
    let r = g.d(10.5);
    let mut pts = Vec::new();
    for i in 0..=24 {
        let a = 180.0f32 + 180.0 * i as f32 / 24.0;
        let a = a.to_radians();
        pts.push(c + Vec2::new(a.cos() * r, a.sin() * r * 1.05 + g.d(2.0)));
    }
    painter.add(Shape::line(pts, Stroke::new(g.d(2.0), color)));
    for side in [-1.0f32, 1.0] {
        let cup = Rect::from_center_size(c + Vec2::new(side * (r - g.d(1.0)), g.d(6.0)), Vec2::new(g.d(5.5), g.d(10.0)));
        painter.rect_filled(cup, CornerRadius::same(g.d(2.5) as u8), color);
    }
}

#[allow(clippy::too_many_arguments)]
fn control_bar(ui: &Ui, painter: &Painter, g: Geo, params: &VocalSuiteParams, display: &PitchDisplay, setter: &ParamSetter, state: &mut GuiState, now: f64) {
    // Meter.
    let (in_peak, out_peak, blocks, _) = display.block_status();
    let fresh = blocks != state.last_blocks;
    state.last_blocks = blocks;
    let (tin, tout) = if fresh { (level_to_meter(in_peak), level_to_meter(out_peak)) } else { (0.0, 0.0) };
    state.meter_in = if tin > state.meter_in { tin } else { (state.meter_in - 0.012).max(tin) };
    state.meter_out = if tout > state.meter_out { tout } else { (state.meter_out - 0.012).max(tout) };
    meter(painter, g, lay::INPUT_METER, lay::INPUT_SEGMENTS, state.meter_in);
    meter(painter, g, lay::OUTPUT_METER, lay::OUTPUT_SEGMENTS, state.meter_out);

    // Bypass.
    let bypass = params.bypass.value();
    let rect = g.r(lay::BYPASS);
    let r = ui.interact(rect, Id::new("bypass"), Sense::click()).on_hover_text("Bypass: esce la voce originale, identica e a tempo.");
    pill(painter, g, rect, bypass, r.hovered());
    let icon = g.pt(lay::BYPASS_ICON);
    let ink = if bypass { TEXT } else { Color32::from_rgb(0xdc, 0xe4, 0xee) };
    painter.circle_stroke(icon, g.d(9.0), Stroke::new(g.d(1.8), ink));
    if bypass {
        painter.circle_filled(icon, g.d(4.0), CYAN);
    }
    txt(painter, g.pt(lay::BYPASS_TEXT), Align2::LEFT_CENTER, "Bypass", font(14.5, Weight::Regular), ink);
    if r.clicked() {
        set_param(setter, &params.bypass, !bypass);
    }

    // Monitor = Low Latency.
    let monitor = params.quality.value() == QualityChoice::LowLatency;
    let rect = g.r(lay::MONITOR);
    let r = ui
        .interact(rect, Id::new("monitor"), Sense::click())
        .on_hover_text("Monitor: latenza zero per registrare in cuffia (Low Latency). Spento = HQ, la qualità piena per il mix.");
    pill(painter, g, rect, monitor, r.hovered());
    let ink = if monitor { TEXT } else { Color32::from_rgb(0xdc, 0xe4, 0xee) };
    headphones_icon(painter, g, g.pt(lay::MONITOR_ICON), ink);
    txt(painter, g.pt(lay::MONITOR_TEXT), Align2::LEFT_CENTER, "Monitor", font(14.5, Weight::Regular), ink);
    if r.clicked() {
        let next = if monitor { QualityChoice::Hq } else { QualityChoice::LowLatency };
        set_param(setter, &params.quality, next);
        state.toast = Some((if monitor { "HQ: qualità piena per il mix".to_owned() } else { "Monitor: latenza zero per registrare".to_owned() }, now));
    }

    // Note Stabilizer: menu.
    let rect = g.r(lay::STABILIZER);
    let level = params.stabilizer.value();
    let r = ui
        .interact(rect, Id::new("stabilizer"), Sense::click())
        .on_hover_text(format!("Note Stabilizer: {} — tiene ferme le note lunghe (toglie vibrato e oscillazioni).", stabilizer_label(level)));
    if level != Stabilizer::Off {
        let radius = rect.height() / 2.0;
        painter.rect_stroke(rect, CornerRadius::same(radius as u8), Stroke::new(g.d(1.4), Color32::from_rgb(0x3c, 0x8c, 0xd8)), StrokeKind::Inside);
        glow_dot(painter, g.p(889.0, 653.0), g.d(22.0), CYAN, 0.25 + 0.1 * level.to_index() as f32);
    } else if r.hovered() {
        let radius = rect.height() / 2.0;
        painter.rect_stroke(rect, CornerRadius::same(radius as u8), Stroke::new(g.d(1.2), Color32::from_rgb(0x3c, 0x52, 0x6c)), StrokeKind::Inside);
    }
    if r.clicked() {
        state.menu = Some((Menu::Stabilizer, rect));
    }
}

fn stabilizer_label(s: Stabilizer) -> &'static str {
    match s {
        Stabilizer::Off => "Off",
        Stabilizer::Light => "Light",
        Stabilizer::Medium => "Medium",
        Stabilizer::Strong => "Strong",
    }
}

// --- Manopole -----------------------------------------------------------------------------------------------

fn knobs(ui: &Ui, painter: &Painter, g: Geo, params: &VocalSuiteParams, setter: &ParamSetter, state: &mut GuiState) {
    let list: [(&FloatParam, &'static str, &str); 4] = [
        (&params.chorus, "chorus", "CHORUS · WIDTH: chorus analogico del Juno-60 mescolato al chorus di FL Studio (modo e miscela nel pannello ⚙)."),
        (&params.formant, "formant", "FORMANT · CHARACTER: il timbro senza cambiare la nota. 50 = naturale, meno = voce più grande e scura, più = più piccola e chiara."),
        (&params.reverb, "reverb", "REVERB · SPACE: quanto riverbero (durata, pre-delay e colore nel pannello ⚙)."),
        (&params.delay, "delay", "DELAY · TIME: quanta eco a nastro stile Space Echo (modo 1-12, tempo e Intensity nel pannello ⚙)."),
    ];
    for ((param, id, help), geo) in list.into_iter().zip(lay::KNOBS.iter()) {
        let center = g.p(geo.cx, geo.cy);
        let value = param.value();
        let norm = param.unmodulated_normalized_value();
        let angle = knob_angle(norm);
        let color = rgb(geo.color);
        // Arco del valore: alone e filo luminoso da 0 al valore.
        arc_band(painter, center, g.d(geo.arc_r), lay::KNOB_ANGLE_MIN, angle, g.d(16.0), Color32::TRANSPARENT, light(color, 0.32));
        arc_band(painter, center, g.d(geo.arc_r), lay::KNOB_ANGLE_MIN, angle, g.d(5.5), Color32::TRANSPARENT, light(color, 0.9));
        arc_band(painter, center, g.d(geo.arc_r), lay::KNOB_ANGLE_MIN, angle, g.d(2.6), light(color, 0.4), Color32::from_rgb(0xf4, 0xfa, 0xff));
        // Il pallino.
        white_dot(painter, polar(center, g.d(geo.dot_r), angle), g.d(10.5), color);
        // Interazione e numero.
        let hit = Rect::from_center_size(center, Vec2::splat(g.d(200.0)));
        drag_param(ui, setter, param, hit, id, help);
        let number = Rect::from_center_size(g.p(geo.cx, lay::KNOB_NUMBER_Y), Vec2::new(g.d(70.0), g.d(36.0)));
        value_entry(
            ui,
            painter,
            setter,
            param,
            id,
            number,
            (number.center(), Align2::CENTER_CENTER),
            format!("{:.0}", value),
            font(29.0, Weight::Regular),
            TEXT,
            |v| v.clamp(0.0, 100.0),
            state,
        );
    }
}

// --- Numeri scritti a mano, messaggi, finestra ------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn value_entry<P: Param>(
    ui: &Ui,
    painter: &Painter,
    setter: &ParamSetter,
    param: &P,
    id: &'static str,
    rect: Rect,
    text_pos: (Pos2, Align2),
    shown: String,
    font_id: FontId,
    color: Color32,
    to_plain: impl Fn(f32) -> P::Plain,
    state: &mut GuiState,
) {
    let editing = state.editing.as_ref().is_some_and(|e| e.id == id);
    if !editing {
        txt(painter, text_pos.0, text_pos.1, &shown, font_id, color);
        let response = ui
            .interact(rect, Id::new(("numero", id)), Sense::click())
            .on_hover_cursor(CursorIcon::Text)
            .on_hover_text("Clic per scrivere il valore");
        if response.clicked() {
            state.editing = Some(Editing { id, text: String::new(), focus: true });
        }
        return;
    }
    let ctx = ui.ctx().clone();
    let Some(edit) = state.editing.as_mut() else { return };
    painter.rect_filled(rect, CornerRadius::same(6), Color32::from_rgba_premultiplied(4, 10, 20, 220));
    painter.rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.4_f32, CYAN), StrokeKind::Inside);
    let response = egui::Area::new(Id::new(("scrivi", id)))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .show(&ctx, |ui| {
            ui.add_sized(
                rect.size(),
                egui::TextEdit::singleline(&mut edit.text)
                    .font(font_id.clone())
                    .hint_text(egui::RichText::new(shown.clone()).font(font_id.clone()).color(TEXT_FAINT))
                    .horizontal_align(egui::Align::Center)
                    .vertical_align(egui::Align::Center)
                    .text_color(TEXT)
                    .frame(false)
                    .margin(egui::Margin::ZERO),
            )
        })
        .inner;
    if edit.focus {
        response.request_focus();
        edit.focus = false;
        return;
    }
    let (enter, escape) = ctx.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
    if escape {
        state.editing = None;
    } else if enter || response.lost_focus() {
        if let Some(number) = parse_number(&edit.text) {
            setter.begin_set_parameter(param);
            setter.set_parameter(param, to_plain(number));
            setter.end_set_parameter(param);
        }
        state.editing = None;
    }
}

fn parse_number(text: &str) -> Option<f32> {
    let cleaned: String = text
        .trim()
        .replace(',', ".")
        .chars()
        .filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
        .collect();
    cleaned.parse::<f32>().ok().filter(|v| v.is_finite())
}

fn toast(painter: &Painter, g: Geo, state: &GuiState, now: f64) {
    if let Some((text, when)) = &state.toast {
        let age = (now - when) as f32;
        if age < 2.5 {
            let a = (2.5 - age).clamp(0.0, 1.0);
            let pos = g.p(724.0, 112.0);
            let galley_w = text_width(painter, text, font(16.0, Weight::Regular), -0.01);
            let bg = Rect::from_center_size(pos, Vec2::new(galley_w + g.d(30.0), g.d(32.0)));
            painter.rect_filled(bg, CornerRadius::same(g.d(16.0) as u8), Color32::from_rgba_premultiplied(4, 12, 24, (200.0 * a) as u8));
            txt(painter, pos, Align2::CENTER_CENTER, text, font(16.0, Weight::Regular), alpha(CYAN, a));
        }
    }
}

const MIN_ZOOM: f32 = 0.6;
const MAX_ZOOM: f32 = 2.0;

fn request_zoom(params: &VocalSuiteParams, zoom: f32) {
    let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    let width = (EDITOR_WIDTH as f32 * zoom).round() as u32;
    let height = (EDITOR_HEIGHT as f32 * zoom).round() as u32;
    if params.editor_state.size() != (width, height) {
        params.editor_state.set_requested_size((width, height));
    }
}

fn resize_grip(ui: &Ui, painter: &Painter, screen: Rect, params: &VocalSuiteParams) {
    let size = 18.0;
    let rect = Rect::from_min_max(screen.max - Vec2::splat(size), screen.max);
    let response = ui
        .interact(rect, Id::new("ridimensiona"), Sense::drag())
        .on_hover_cursor(CursorIcon::ResizeNwSe)
        .on_hover_text("Trascina per ingrandire o rimpicciolire la finestra");
    if response.hovered() || response.dragged() {
        for k in [4.0, 9.0, 14.0] {
            painter.line_segment([Pos2::new(screen.max.x - k, screen.max.y - 2.0), Pos2::new(screen.max.x - 2.0, screen.max.y - k)], Stroke::new(1.2_f32, TEXT_DIM));
        }
    }
    if response.dragged() {
        if let Some(pointer) = response.interact_pointer_pos() {
            let width = (pointer.x - screen.min.x + 4.0).max(1.0);
            request_zoom(params, width / EDITOR_WIDTH as f32);
        }
    }
}

fn save_as_default(params: &VocalSuiteParams, state: &mut GuiState, now: f64) {
    let values: std::collections::HashMap<String, f32> = params
        .param_map()
        .into_iter()
        .filter(|(id, _, _)| id != "bypass")
        // SAFETY: i puntatori vengono dai parametri del plugin.
        .map(|(id, ptr, _)| (id, unsafe { ptr.unmodulated_plain_value() }))
        .collect();
    let text = match crate::defaults::save(&values) {
        Ok(()) => "Salvato come predefinito: le nuove istanze partono così".to_owned(),
        Err(e) => format!("Non riesco a salvare il predefinito ({e})"),
    };
    state.toast = Some((text, now));
}

fn reset_default(state: &mut GuiState, now: f64) {
    let text = match crate::defaults::clear() {
        Ok(()) => "Predefinito di fabbrica (dalle nuove istanze)".to_owned(),
        Err(e) => format!("Non riesco a cancellare il predefinito ({e})"),
    };
    state.toast = Some((text, now));
}

fn auto_voice(params: &VocalSuiteParams, display: &PitchDisplay, setter: &ParamSetter, state: &mut GuiState, now: f64) {
    if state.reset_histogram {
        display.reset_histogram();
        state.reset_histogram = false;
    }
    let Some((started, previous)) = state.auto_voice else { return };
    if now - state.auto_voice_checked < 1.0 {
        return;
    }
    state.auto_voice_checked = now;
    if let Some(range) = crate::shared::detect_voice_range(&display.note_histogram()) {
        let voice = [VoiceChoice::Soprano, VoiceChoice::AltoTenor, VoiceChoice::Low][range];
        set_param(setter, &params.voice, voice);
        state.auto_voice = None;
        state.toast = Some((format!("Vocal Range: {}", voice_label(voice)), now));
    } else if now - started > 40.0 {
        set_param(setter, &params.voice, previous);
        state.auto_voice = None;
        state.toast = Some(("Vocal Range: nessuna voce sentita".to_owned(), now));
    }
}

fn autokey(params: &VocalSuiteParams, display: &PitchDisplay, setter: &ParamSetter, state: &mut GuiState, now: f64) {
    if state.reset_notes {
        display.reset_notes();
        state.reset_notes = false;
    }
    if !params.autokey.load(Ordering::Relaxed) || now - state.autokey_checked < 1.0 {
        return;
    }
    state.autokey_checked = now;
    let Some((tonic, minor, confidence, source)) = display.detect_key_all() else { return };
    if confidence < 0.5 {
        return;
    }
    let scale = if minor { ScaleChoice::NaturalMinor } else { ScaleChoice::Major };
    let key = Key::from_index(tonic % 12);
    let current = (params.key.value().semitone() as usize, params.scale.value());
    let current_minor = match current.1 {
        ScaleChoice::Major => Some(false),
        ScaleChoice::NaturalMinor => Some(true),
        _ => None,
    };
    if current_minor.is_some_and(|m| relative_key(current.0, m) == (tonic, minor)) {
        return;
    }
    if params.key.value() != key || params.scale.value() != scale {
        set_param(setter, &params.key, key);
        set_param(setter, &params.scale, scale);
        let from = match source {
            KeySource::Voice => "dalla voce",
            KeySource::Polyphonic => "dagli accordi",
            KeySource::Sidechain => "dalla base in sidechain",
        };
        state.toast = Some((format!("AutoKey: {} {} {from}", KEY_NAMES[tonic], scale_label(scale)), now));
    }
}

// --- Menu a tendina ---------------------------------------------------------------------------------------------

fn menu_item(ui: &Ui, painter: &Painter, rect: Rect, id: Id, text: &str, selected: bool, size: f32) -> bool {
    let response = ui.interact(rect, id, Sense::click());
    if selected {
        painter.rect_filled(rect, CornerRadius::same(6), Color32::from_rgb(0x0b, 0x3a, 0x62));
        painter.rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0_f32, Color32::from_rgb(0x4c, 0xc8, 0xf0)), StrokeKind::Inside);
    } else if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(6), Color32::from_white_alpha(10));
    }
    painter.text(rect.left_center() + Vec2::new(10.0, 0.0), Align2::LEFT_CENTER, text, font(size, Weight::Regular), TEXT);
    response.clicked()
}

fn menu_frame(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect.translate(Vec2::new(0.0, 6.0)), CornerRadius::same(12), Color32::from_black_alpha(120));
    painter.rect(rect, CornerRadius::same(12), MENU_BG, Stroke::new(1.2_f32, Color32::from_rgb(0x24, 0x3a, 0x56)), StrokeKind::Inside);
}

fn menus(ctx: &egui::Context, g: Geo, params: &VocalSuiteParams, setter: &ParamSetter, state: &mut GuiState, now: f64) {
    let Some((menu, trigger)) = state.menu else { return };
    let row = 50.0;
    let (pos, size) = match menu {
        Menu::Voice => (g.p(lay::VOICE_FIELD.0, lay::VOICE_FIELD.3 + 6.0), Vec2::new(g.d(250.0), g.d(28.0 + row * 4.0))),
        Menu::KeyScale => (g.p(lay::KEY_FIELD.0 - 10.0, lay::KEY_FIELD.3 + 6.0), Vec2::new(g.d(430.0), g.d(560.0))),
        Menu::Preset => (g.p(lay::PRESET_BOX.0, lay::PRESET_BOX.3 + 6.0), Vec2::new(g.d(lay::PRESET_BOX.2 - lay::PRESET_BOX.0), g.d(28.0 + 46.0 * (FACTORY.len() + 2) as f32 + 8.0))),
        Menu::Main => (g.p(1110.0, 86.0), Vec2::new(g.d(300.0), g.d(28.0 + 46.0 * (FACTORY.len() + 3) as f32 + 16.0))),
        Menu::Stabilizer => (g.p(lay::STABILIZER.0, lay::STABILIZER.1 - 6.0 - (28.0 + row * 4.0)), Vec2::new(g.d(lay::STABILIZER.2 - lay::STABILIZER.0), g.d(28.0 + row * 4.0))),
    };
    let popup = Rect::from_min_size(pos, size);
    let mut close = false;
    egui::Area::new(Id::new("menu-tendina")).order(Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        ui.allocate_exact_size(size, Sense::hover());
        let painter = ui.painter().clone();
        menu_frame(&painter, popup);
        let item = |i: usize, step: f32, h: f32| Rect::from_min_size(pos + Vec2::new(g.d(12.0), g.d(14.0 + step * i as f32)), Vec2::new(size.x - g.d(24.0), g.d(h)));
        match menu {
            Menu::Voice => {
                let auto = state.auto_voice.is_some();
                for (i, voice) in [VoiceChoice::Soprano, VoiceChoice::AltoTenor, VoiceChoice::Low].into_iter().enumerate() {
                    if menu_item(ui, &painter, item(i, row, 44.0), Id::new(("voce", i)), voice_label(voice), !auto && params.voice.value() == voice, 20.0) {
                        set_param(setter, &params.voice, voice);
                        state.auto_voice = None;
                        close = true;
                    }
                }
                if menu_item(ui, &painter, item(3, row, 44.0), Id::new(("voce", 3)), "Auto (ascolta)", auto, 20.0) {
                    state.auto_voice = Some((now, params.voice.value()));
                    state.auto_voice_checked = now;
                    state.reset_histogram = true;
                    set_param(setter, &params.voice, VoiceChoice::Low);
                    state.toast = Some(("Vocal Range: canta qualche secondo…".to_owned(), now));
                    close = true;
                }
            }
            Menu::KeyScale => {
                painter.text(pos + Vec2::new(g.d(20.0), g.d(26.0)), Align2::LEFT_CENTER, "Key", font(17.0, Weight::Regular), TEXT_DIM);
                let current_key = params.key.value().semitone() as usize;
                for i in 0..12 {
                    let (col, r) = (i % 6, i / 6);
                    let rect = Rect::from_min_size(pos + Vec2::new(g.d(14.0 + 67.0 * col as f32), g.d(44.0 + 54.0 * r as f32)), Vec2::new(g.d(62.0), g.d(48.0)));
                    let response = ui.interact(rect, Id::new(("tonalita", i)), Sense::click());
                    if i == current_key {
                        painter.rect_filled(rect, CornerRadius::same(6), Color32::from_rgb(0x0b, 0x3a, 0x62));
                        painter.rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0_f32, Color32::from_rgb(0x4c, 0xc8, 0xf0)), StrokeKind::Inside);
                    } else if response.hovered() {
                        painter.rect_filled(rect, CornerRadius::same(6), Color32::from_white_alpha(10));
                    }
                    txt(&painter, rect.center(), Align2::CENTER_CENTER, KEY_NAMES[i], font(20.0, Weight::Medium), TEXT);
                    if response.clicked() {
                        set_param(setter, &params.key, Key::from_index(i));
                        if matches!(params.scale.value(), ScaleChoice::Chromatic | ScaleChoice::Custom) {
                            set_param(setter, &params.scale, ScaleChoice::Major);
                        }
                        manual_key_choice(params, state, now);
                    }
                }
                painter.text(pos + Vec2::new(g.d(20.0), g.d(176.0)), Align2::LEFT_CENTER, "Scale", font(17.0, Weight::Regular), TEXT_DIM);
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
                    let (col, r) = (i % 2, i / 2);
                    let rect = Rect::from_min_size(pos + Vec2::new(g.d(14.0 + 203.0 * col as f32), g.d(194.0 + 50.0 * r as f32)), Vec2::new(g.d(198.0), g.d(44.0)));
                    if menu_item(ui, &painter, rect, Id::new(("scala", i)), scale_label(scale), params.scale.value() == scale, 18.0) {
                        set_param(setter, &params.scale, scale);
                        manual_key_choice(params, state, now);
                    }
                }
                // Note della scala: clic = includi/escludi (diventa Custom).
                painter.text(pos + Vec2::new(g.d(20.0), g.d(412.0)), Align2::LEFT_CENTER, "Notes", font(17.0, Weight::Regular), TEXT_DIM);
                let notes = params.active_notes();
                for pc in 0..12 {
                    let rect = Rect::from_min_size(pos + Vec2::new(g.d(14.0 + 33.6 * pc as f32), g.d(430.0)), Vec2::new(g.d(30.0), g.d(40.0)));
                    let response = ui.interact(rect, Id::new(("nota", pc)), Sense::click()).on_hover_text("Clic per includere o escludere la nota (scala Custom)");
                    let on = notes[pc];
                    painter.rect_filled(rect, CornerRadius::same(5), if on { Color32::from_rgb(0x0b, 0x3a, 0x62) } else { Color32::from_rgb(0x0a, 0x12, 0x1c) });
                    painter.rect_stroke(rect, CornerRadius::same(5), Stroke::new(1.0_f32, if on { Color32::from_rgb(0x4c, 0xc8, 0xf0) } else { Color32::from_rgb(0x22, 0x32, 0x46) }), StrokeKind::Inside);
                    txt(&painter, rect.center(), Align2::CENTER_CENTER, KEY_NAMES[pc], font(13.0, Weight::Medium), if on { TEXT } else { TEXT_FAINT });
                    if response.clicked() {
                        if params.scale.value() != ScaleChoice::Custom {
                            for (i, &n) in notes.iter().enumerate() {
                                set_param(setter, params.custom.note(i), n);
                            }
                            set_param(setter, &params.scale, ScaleChoice::Custom);
                        }
                        let count = notes.iter().filter(|&&n| n).count();
                        if !(on && count <= 1) {
                            set_param(setter, params.custom.note(pc), !on);
                        }
                        manual_key_choice(params, state, now);
                    }
                }
                // AutoKey.
                let auto = params.autokey.load(Ordering::Relaxed);
                let rect = Rect::from_min_size(pos + Vec2::new(g.d(14.0), g.d(490.0)), Vec2::new(size.x - g.d(28.0), g.d(46.0)));
                if menu_item(ui, &painter, rect, Id::new("autokey"), if auto { "AutoKey Listen: ON" } else { "AutoKey Listen (riconosce la tonalità)" }, auto, 18.0) {
                    params.autokey.store(!auto, Ordering::Relaxed);
                    if !auto {
                        state.reset_notes = true;
                        state.toast = Some(("AutoKey: fai suonare la traccia…".to_owned(), now));
                    }
                }
            }
            Menu::Preset | Menu::Main => {
                let current = params.preset.load(Ordering::Relaxed) % FACTORY.len();
                let mut y = 0usize;
                for (i, preset) in FACTORY.iter().enumerate() {
                    let rect = item(y, 46.0, 42.0);
                    let selected = i == current && preset.matches(params);
                    let label = format!("{}  ·  {}", preset.name, preset.tag);
                    if menu_item(ui, &painter, rect, Id::new(("preset", i)), &label, selected, 17.0) {
                        apply_preset(params, setter, i);
                        close = true;
                    }
                    y += 1;
                }
                let line_y = pos.y + g.d(14.0 + 46.0 * y as f32 - 3.0);
                painter.line_segment([Pos2::new(pos.x + g.d(24.0), line_y), Pos2::new(pos.x + size.x - g.d(24.0), line_y)], Stroke::new(1.0_f32, Color32::from_rgb(0x22, 0x34, 0x4a)));
                if menu_item(ui, &painter, item(y, 46.0, 42.0), Id::new("salva-predefinito"), "Save as Default", false, 17.0) {
                    save_as_default(params, state, now);
                    close = true;
                }
                y += 1;
                if menu_item(ui, &painter, item(y, 46.0, 42.0), Id::new("azzera-predefinito"), "Reset Default", false, 17.0) {
                    reset_default(state, now);
                    close = true;
                }
                y += 1;
                if menu == Menu::Main && menu_item(ui, &painter, item(y, 46.0, 42.0), Id::new("guida"), "Guida rapida", false, 17.0) {
                    state.overlay = Some(Overlay::Help);
                    close = true;
                }
            }
            Menu::Stabilizer => {
                for (i, level) in [Stabilizer::Off, Stabilizer::Light, Stabilizer::Medium, Stabilizer::Strong].into_iter().enumerate() {
                    if menu_item(ui, &painter, item(i, row, 44.0), Id::new(("stab", i)), stabilizer_label(level), params.stabilizer.value() == level, 18.0) {
                        set_param(setter, &params.stabilizer, level);
                        close = true;
                    }
                }
            }
        }
    });
    let clicked_outside = ctx.input(|i| i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| !popup.contains(p) && !trigger.contains(p)));
    if close || clicked_outside {
        state.menu = None;
    }
}

// --- Pannelli: impostazioni e guida ---------------------------------------------------------------------------------

const PANEL: R = (60.0, 104.0, 1388.0, 1046.0);

fn overlays(ctx: &egui::Context, g: Geo, params: &VocalSuiteParams, display: &PitchDisplay, setter: &ParamSetter, state: &mut GuiState, now: f64) {
    let Some(overlay) = state.overlay else { return };
    let rect = g.r(PANEL);
    let mut close = false;
    egui::Area::new(Id::new("pannello")).order(Order::Middle).fixed_pos(rect.min).show(ctx, |ui| {
        ui.allocate_exact_size(rect.size(), Sense::hover());
        let painter = ui.painter().clone();
        let _ = ui.interact(rect, Id::new("pannello-sfondo"), Sense::click());
        painter.rect_filled(rect.translate(Vec2::new(0.0, 8.0)), CornerRadius::same(18), Color32::from_black_alpha(140));
        rounded_gradient(&painter, rect, g.d(24.0), Color32::from_rgba_premultiplied(0x0a, 0x16, 0x26, 250), Color32::from_rgba_premultiplied(0x04, 0x0b, 0x15, 250));
        painter.rect_stroke(rect, CornerRadius::same(g.d(24.0) as u8), Stroke::new(1.4_f32, PANEL_EDGE), StrokeKind::Inside);
        let x_center = g.p(PANEL.2 - 40.0, PANEL.1 + 40.0);
        let x_hit = Rect::from_center_size(x_center, Vec2::splat(g.d(50.0)));
        let x_response = ui.interact(x_hit, Id::new("chiudi"), Sense::click()).on_hover_text("Chiudi");
        let stroke = Stroke::new(g.d(2.4), if x_response.hovered() { TEXT } else { TEXT_DIM });
        let k = g.d(10.0);
        painter.line_segment([x_center + Vec2::new(-k, -k), x_center + Vec2::new(k, k)], stroke);
        painter.line_segment([x_center + Vec2::new(-k, k), x_center + Vec2::new(k, -k)], stroke);
        if x_response.clicked() {
            close = true;
        }
        match overlay {
            Overlay::Settings => settings_page(ui, &painter, g, params, display, setter, state, now),
            Overlay::Help => help_page(&painter, g),
        }
    });
    if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) && state.editing.is_none() {
        state.overlay = None;
    }
}

/// Un cursore orizzontale con etichetta e valore (clic sul valore = scriverlo).
#[allow(clippy::too_many_arguments)]
fn slider<P: Param>(
    ui: &Ui,
    painter: &Painter,
    g: Geo,
    setter: &ParamSetter,
    param: &P,
    id: &'static str,
    label: &str,
    x: f32,
    y: f32,
    w: f32,
    help: &str,
    to_plain: impl Fn(f32) -> P::Plain,
    state: &mut GuiState,
) {
    txt(painter, g.p(x, y), Align2::LEFT_CENTER, label, font(16.5, Weight::Regular), Color32::from_rgb(0xc8, 0xd2, 0xe0));
    let shown = param.to_string();
    let number = Rect::from_min_max(g.p(x + w - 120.0, y - 13.0), g.p(x + w, y + 13.0));
    value_entry(ui, painter, setter, param, id, number, (g.p(x + w, y), Align2::RIGHT_CENTER), shown, font(16.0, Weight::Medium), CYAN, to_plain, state);
    let track = Rect::from_min_max(g.p(x, y + 20.0), g.p(x + w, y + 25.0));
    let norm = param.unmodulated_normalized_value();
    painter.rect_filled(track, CornerRadius::same(g.d(2.5) as u8), Color32::from_rgb(0x16, 0x22, 0x32));
    let fill = Rect::from_min_max(track.min, Pos2::new(track.left() + track.width() * norm, track.bottom()));
    painter.rect_filled(fill, CornerRadius::same(g.d(2.5) as u8), Color32::from_rgb(0x3a, 0x9c, 0xff));
    let knob = Pos2::new(fill.right(), track.center().y);
    glow_dot(painter, knob, g.d(14.0), CYAN, 0.3);
    painter.circle_filled(knob, g.d(7.0), Color32::from_rgb(0xf2, 0xf6, 0xfc));
    let hit = Rect::from_min_max(g.p(x - 8.0, y + 10.0), g.p(x + w + 8.0, y + 36.0));
    drag_param_h(ui, setter, param, track, hit, id, help);
}

/// Una fila di pulsanti di scelta (restituisce l'indice cliccato).
fn choices(ui: &Ui, painter: &Painter, g: Geo, id: &str, labels: &[&str], selected: usize, x: f32, y: f32, w: f32, h: f32, gap: f32) -> Option<usize> {
    let mut clicked = None;
    for (i, label) in labels.iter().enumerate() {
        let x0 = x + i as f32 * (w + gap);
        let rect = g.r((x0, y, x0 + w, y + h));
        let response = ui.interact(rect, Id::new((id, i)), Sense::click());
        let on = i == selected;
        pill(painter, g, rect, on, response.hovered());
        txt(painter, rect.center(), Align2::CENTER_CENTER, *label, font(15.0, Weight::Medium), if on { TEXT } else { TEXT_DIM });
        if response.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

#[allow(clippy::too_many_arguments)]
fn settings_page(ui: &Ui, painter: &Painter, g: Geo, params: &VocalSuiteParams, display: &PitchDisplay, setter: &ParamSetter, state: &mut GuiState, now: f64) {
    let (x0, y0) = (PANEL.0 + 50.0, PANEL.1 + 46.0);
    txt(painter, g.p(x0, y0), Align2::LEFT_CENTER, "Impostazioni", font(28.0, Weight::Medium), TEXT);
    let col = [PANEL.0 + 50.0, PANEL.0 + 490.0, PANEL.0 + 930.0];
    let w = 360.0;
    let head = |painter: &Painter, x: f32, y: f32, t: &str| {
        txt(painter, g.p(x, y), Align2::LEFT_CENTER, t, font(15.0, Weight::SemiBold), CYAN);
        painter.line_segment([g.p(x, y + 16.0), g.p(x + w, y + 16.0)], Stroke::new(g.d(1.0), Color32::from_rgb(0x1e, 0x30, 0x46)));
    };
    let step = 62.0;

    // --- Colonna 1: correzione.
    let (x, mut y) = (col[0], y0 + 64.0);
    head(painter, x, y, "CORREZIONE (SOSATUNE)");
    y += 46.0;
    slider(ui, painter, g, setter, &params.amount, "amount", "Amount", x, y, w, "Quanta correzione: 100% = completa.", |v| v / 100.0, state);
    y += step;
    slider(ui, painter, g, setter, &params.flex_tune, "flex-tune", "Flex-Tune", x, y, w, "Corregge piena vicino alla nota, meno quando la voce è lontana.", |v| v / 100.0, state);
    y += step;
    slider(ui, painter, g, setter, &params.vibrato, "vibrato", "Vibrato", x, y, w, "Negativo lo toglie, positivo lo aggiunge.", |v| v / 100.0, state);
    y += step;
    slider(ui, painter, g, setter, &params.transpose, "transpose", "Transpose", x, y, w, "Sposta tutta la voce di semitoni, mantenendo il timbro.", |v| v.round() as i32, state);
    y += step;
    slider(ui, painter, g, setter, &params.tracking, "tracking", "Sensitivity", x, y, w, "Alzala per voci con molta aria, abbassala se senti salti strani.", |v| v / 100.0, state);
    y += step;
    slider(ui, painter, g, setter, &params.detune, "detune-s", "Tuning (A4)", x, y, w, "Accordatura del La4: 440 Hz, o quella della base.", |v| v, state);
    y += step - 6.0;
    txt(painter, g.p(x, y), Align2::LEFT_CENTER, "Style", font(16.5, Weight::Regular), Color32::from_rgb(0xc8, 0xd2, 0xe0));
    let sel = params.style.value().to_index();
    if let Some(i) = choices(ui, painter, g, "style", &["Modern", "Classic"], sel, x + 120.0, y - 18.0, 115.0, 36.0, 10.0) {
        set_param(setter, &params.style, StyleChoice::from_index(i));
    }
    y += 52.0;
    txt(painter, g.p(x, y), Align2::LEFT_CENTER, "Engine", font(16.5, Weight::Regular), Color32::from_rgb(0xc8, 0xd2, 0xe0));
    let sel = if params.quality.value() == QualityChoice::Hq { 1 } else { 0 };
    if let Some(i) = choices(ui, painter, g, "quality", &["Low Latency", "HQ"], sel, x + 120.0, y - 18.0, 115.0, 36.0, 10.0) {
        set_param(setter, &params.quality, if i == 0 { QualityChoice::LowLatency } else { QualityChoice::Hq });
    }
    y += 40.0;
    txt(
        painter,
        g.p(x, y),
        Align2::LEFT_CENTER,
        format!("Latenza attuale: {:.1} ms (compensata dal DAW)", display.latency_ms()),
        font(14.0, Weight::Regular),
        TEXT_FAINT,
    );

    // --- Colonna 2: chorus e riverbero.
    let (x, mut y) = (col[1], y0 + 64.0);
    head(painter, x, y, "CHORUS (JUNO-60 + FL STUDIO)");
    y += 40.0;
    txt(painter, g.p(x, y), Align2::LEFT_CENTER, "Mode", font(16.5, Weight::Regular), Color32::from_rgb(0xc8, 0xd2, 0xe0));
    let sel = params.chorus_mode.value().to_index();
    if let Some(i) = choices(ui, painter, g, "chorus-mode", &["I", "II", "I + II"], sel, x + 100.0, y - 18.0, 78.0, 36.0, 8.0) {
        set_param(setter, &params.chorus_mode, ChorusModeChoice::from_index(i));
    }
    y += 46.0;
    slider(ui, painter, g, setter, &params.chorus_blend, "chorus-blend", "Juno  ↔  FL", x, y, w, "0% = solo il BBD del Juno-60, 100% = solo il chorus a più voci di FL Studio.", |v| v / 100.0, state);
    y += step;
    slider(ui, painter, g, setter, &params.chorus_rate, "chorus-rate", "Rate", x, y, w, "Velocità degli LFO (1x = valori originali del Juno).", |v| v, state);
    y += step + 10.0;
    head(painter, x, y, "REVERB");
    y += 46.0;
    slider(ui, painter, g, setter, &params.reverb_decay, "reverb-decay", "Decay", x, y, w, "Durata della coda (RT60).", |v| v, state);
    y += step;
    slider(ui, painter, g, setter, &params.reverb_predelay, "reverb-predelay", "Pre-delay", x, y, w, "Ritardo prima della coda: stacca la voce dal riverbero.", |v| v, state);
    y += step;
    slider(ui, painter, g, setter, &params.reverb_damping, "reverb-damping", "Damping", x, y, w, "Quanto si scuriscono gli acuti nella coda.", |v| v / 100.0, state);
    y += step;
    slider(ui, painter, g, setter, &params.reverb_lowcut, "reverb-lowcut", "Low Cut", x, y, w, "Taglia i bassi del riverbero (voce più pulita).", |v| v, state);

    // --- Colonna 3: tape echo.
    let (x, mut y) = (col[2], y0 + 64.0);
    head(painter, x, y, "DELAY (SPACE ECHO RE-201)");
    y += 34.0;
    let mode = params.delay_mode.value().clamp(1, 12);
    for i in 0..12 {
        let (c, r) = (i % 6, i / 6);
        let x0 = x + c as f32 * 61.0;
        let y0 = y + r as f32 * 44.0;
        let rect = g.r((x0, y0, x0 + 54.0, y0 + 36.0));
        let response = ui.interact(rect, Id::new(("echo-mode", i)), Sense::click());
        let on = mode == i as i32 + 1;
        let (heads, spring) = mode_heads(i as u8 + 1);
        let tip: String = {
            let hs: Vec<String> = (0..3).filter(|&k| heads[k]).map(|k| (k + 1).to_string()).collect();
            let mut s = if hs.is_empty() { "Solo molla".to_owned() } else { format!("Testine {}", hs.join("+")) };
            if spring && !hs.is_empty() {
                s.push_str(" + molla");
            }
            s
        };
        let response = response.on_hover_text(format!("Mode {}: {tip}", i + 1));
        pill(painter, g, rect, on, response.hovered());
        txt(painter, rect.center(), Align2::CENTER_CENTER, (i + 1).to_string(), font(15.0, Weight::Medium), if on { TEXT } else { TEXT_DIM });
        if response.clicked() {
            set_param(setter, &params.delay_mode, i as i32 + 1);
        }
    }
    y += 98.0;
    let (heads, spring) = mode_heads(mode as u8);
    let hs: Vec<String> = (0..3).filter(|&k| heads[k]).map(|k| (k + 1).to_string()).collect();
    let desc = if hs.is_empty() { "Mode 12: solo riverbero a molla".to_owned() } else { format!("Mode {mode}: testine {}{}", hs.join(" + "), if spring { " + molla" } else { "" }) };
    txt(painter, g.p(x, y), Align2::LEFT_CENTER, desc, font(14.5, Weight::Regular), TEXT_DIM);
    y += 34.0;
    txt(painter, g.p(x, y), Align2::LEFT_CENTER, "Sync", font(16.5, Weight::Regular), Color32::from_rgb(0xc8, 0xd2, 0xe0));
    let syncs = ["Free", "1/32", "1/16T", "1/16", "1/8T", "1/8", "1/8D", "1/4"];
    let sel = params.delay_sync.value().to_index();
    if let Some(i) = choices(ui, painter, g, "delay-sync-a", &syncs[..4], if sel < 4 { sel } else { 99 }, x + 70.0, y - 17.0, 66.0, 32.0, 6.0) {
        set_param(setter, &params.delay_sync, DelaySync::from_index(i));
    }
    if let Some(i) = choices(ui, painter, g, "delay-sync-b", &syncs[4..], if sel >= 4 { sel - 4 } else { 99 }, x + 70.0, y + 21.0, 66.0, 32.0, 6.0) {
        set_param(setter, &params.delay_sync, DelaySync::from_index(i + 4));
    }
    y += 76.0;
    if params.delay_sync.value() == DelaySync::Free {
        slider(ui, painter, g, setter, &params.delay_time, "delay-time", "Repeat Rate (head 1)", x, y, w, "Tempo della testina 1 (le testine 2 e 3: doppio e triplo).", |v| v, state);
    } else {
        txt(painter, g.p(x, y), Align2::LEFT_CENTER, "Repeat Rate: a tempo del DAW (testine 1x, 2x, 3x)", font(15.0, Weight::Regular), TEXT_DIM);
    }
    y += step;
    slider(ui, painter, g, setter, &params.delay_intensity, "delay-intensity", "Intensity", x, y, w, "Rigenerazione: oltre ~85% auto-oscillazione come la macchina.", |v| v / 100.0, state);
    y += step;
    slider(ui, painter, g, setter, &params.delay_bass, "delay-bass", "Bass", x, y, w, "Bassi delle ripetizioni (anche nel giro, come sulla macchina).", |v| v / 12.0, state);
    y += step;
    slider(ui, painter, g, setter, &params.delay_treble, "delay-treble", "Treble", x, y, w, "Acuti delle ripetizioni.", |v| v / 12.0, state);
    y += step;
    slider(ui, painter, g, setter, &params.delay_wow, "delay-wow", "Wow & Flutter", x, y, w, "Movimento del nastro: 0 = nastro nuovo e perfetto.", |v| v / 100.0, state);
    y += step;
    slider(ui, painter, g, setter, &params.delay_spring, "delay-spring", "Spring Reverb", x, y, w, "Livello della molla (modi 5-12).", |v| v / 100.0, state);

    // --- In basso: uscita, finestra, predefinito.
    let yb = PANEL.3 - 64.0;
    slider(ui, painter, g, setter, &params.output_gain, "output", "Output", col[0], yb - 14.0, w, "Volume in uscita.", |v| v, state);
    txt(painter, g.p(col[1], yb), Align2::LEFT_CENTER, "Finestra", font(16.5, Weight::Regular), Color32::from_rgb(0xc8, 0xd2, 0xe0));
    let zooms = [0.75f32, 1.0, 1.25, 1.5];
    let current = params.editor_state.size().0 as f32 / EDITOR_WIDTH as f32;
    let sel = zooms.iter().position(|z| (current - z).abs() < 0.02).unwrap_or(99);
    if let Some(i) = choices(ui, painter, g, "zoom", &["75%", "100%", "125%", "150%"], sel, col[1] + 90.0, yb - 18.0, 66.0, 36.0, 6.0) {
        request_zoom(params, zooms[i]);
    }
    for (i, (label, id)) in [("Save as Default", "s-save"), ("Reset Default", "s-reset")].into_iter().enumerate() {
        let x0 = col[2] + i as f32 * 186.0;
        let rect = g.r((x0, yb - 18.0, x0 + 174.0, yb + 18.0));
        let response = ui.interact(rect, Id::new(id), Sense::click());
        pill(painter, g, rect, false, response.hovered());
        txt(painter, rect.center(), Align2::CENTER_CENTER, label, font(15.0, Weight::Regular), TEXT);
        if response.clicked() {
            if i == 0 {
                save_as_default(params, state, now);
            } else {
                reset_default(state, now);
            }
        }
    }
}

fn help_page(painter: &Painter, g: Geo) {
    let x = PANEL.0 + 50.0;
    txt(painter, g.p(x, PANEL.1 + 46.0), Align2::LEFT_CENTER, "Guida rapida", font(28.0, Weight::Medium), TEXT);
    let lines = [
        ("Vocal Range", "tipo di voce (la nota più grave riconosciuta); Auto la sceglie ascoltando."),
        ("Key & Scale / sfera", "tonalità e scala; nel menu anche le singole note e AutoKey Listen."),
        ("Preset  ‹ ›", "i preset di fabbrica; clic sul nome = lista; clic su 440 Hz = accordatura."),
        ("Retune Speed", "0 = robotico, 10-30 = pop, 50-150 = naturale (ms). Trascina la sfera."),
        ("Humanize", "le note lunghe vengono corrette più dolcemente."),
        ("Grafico", "lo scarto della voce dalla nota giusta in centesimi; a sinistra la nota."),
        ("Bypass", "esce la voce originale, identica e a tempo."),
        ("Monitor", "latenza zero per registrare (Low Latency); spento = HQ per il mix."),
        ("Note Stabilizer", "tiene ferme le note lunghe: Light, Medium, Strong."),
        ("Chorus · Width", "Juno-60 (BBD, modi I / II / I+II) mescolato al chorus di FL Studio."),
        ("Formant · Character", "il timbro: 50 = naturale, sotto più grande e scuro, sopra più chiaro."),
        ("Reverb · Space", "quanto riverbero (durata, pre-delay, colore nel pannello ⚙)."),
        ("Delay · Time", "quanta eco a nastro Space Echo RE-201 (modo 1-12, tempo, Intensity in ⚙)."),
        ("Frecce, ☰, ingranaggio", "annulla e ripeti / preset, Save as Default, guida / tutte le impostazioni."),
    ];
    for (i, (name, text)) in lines.iter().enumerate() {
        let y = PANEL.1 + 110.0 + i as f32 * 52.0;
        txt(painter, g.p(x, y), Align2::LEFT_CENTER, *name, font(18.0, Weight::Medium), CYAN);
        txt(painter, g.p(x + 300.0, y), Align2::LEFT_CENTER, *text, font(18.0, Weight::Regular), TEXT);
    }
    txt(
        painter,
        g.p(x, PANEL.3 - 40.0),
        Align2::LEFT_CENTER,
        "Manopole e sfere: trascina (Maiusc = fine) o usa la rotellina, doppio clic = predefinito, clic sul numero per scriverlo.",
        font(16.0, Weight::Regular),
        TEXT_DIM,
    );
}

/// Dove si trovano i comandi (in punti della finestra iniziale), per le prove.
#[allow(dead_code)]
pub(crate) mod positions {
    use super::*;

    fn at(x: f32, y: f32) -> Pos2 {
        Pos2::new(x, y) * (EDITOR_WIDTH as f32 / lay::REF_W)
    }
    pub fn knob(i: usize) -> Pos2 {
        let k = &lay::KNOBS[i];
        at(k.cx, k.cy - 40.0)
    }
    pub fn knob_number(i: usize) -> Pos2 {
        at(lay::KNOBS[i].cx, lay::KNOB_NUMBER_Y)
    }
    pub fn retune() -> Pos2 {
        at(lay::RETUNE_ORB.0, lay::RETUNE_ORB.1)
    }
    pub fn humanize() -> Pos2 {
        at(lay::HUMANIZE_ORB.0, lay::HUMANIZE_ORB.1)
    }
    pub fn bypass() -> Pos2 {
        let c = lay::center(lay::BYPASS);
        at(c.0, c.1)
    }
    pub fn monitor() -> Pos2 {
        let c = lay::center(lay::MONITOR);
        at(c.0, c.1)
    }
    pub fn stabilizer() -> Pos2 {
        let c = lay::center(lay::STABILIZER);
        at(c.0, c.1)
    }
    pub fn voice_field() -> Pos2 {
        let c = lay::center(lay::VOICE_FIELD);
        at(c.0, c.1)
    }
    pub fn key_field() -> Pos2 {
        let c = lay::center(lay::KEY_FIELD);
        at(c.0, c.1)
    }
    pub fn preset_next() -> Pos2 {
        let c = lay::center(lay::PRESET_NEXT);
        at(c.0, c.1)
    }
    pub fn preset_prev() -> Pos2 {
        let c = lay::center(lay::PRESET_PREV);
        at(c.0, c.1)
    }
    pub fn settings() -> Pos2 {
        at(lay::SETTINGS.0, lay::SETTINGS.1)
    }
    pub fn menu() -> Pos2 {
        at(lay::MENU.0, lay::MENU.1)
    }
    pub fn undo() -> Pos2 {
        at(lay::UNDO.0, lay::UNDO.1)
    }
    pub fn orb() -> Pos2 {
        at(lay::ORB.0, lay::ORB.1)
    }
    /// Una voce del menu Note Stabilizer (0 = Off ... 3 = Strong).
    pub fn stabilizer_item(i: usize) -> Pos2 {
        let top = lay::STABILIZER.1 - 6.0 - (28.0 + 50.0 * 4.0);
        at(lay::STABILIZER.0 + 100.0, top + 14.0 + 50.0 * i as f32 + 22.0)
    }
    /// Una tonalita' nel menu Key & Scale (0 = C ... 11 = B).
    pub fn key_item(i: usize) -> Pos2 {
        let (col, r) = (i % 6, i / 6);
        at(lay::KEY_FIELD.0 - 10.0 + 14.0 + 67.0 * col as f32 + 31.0, lay::KEY_FIELD.3 + 6.0 + 44.0 + 54.0 * r as f32 + 24.0)
    }
    /// Un modo del delay nel pannello impostazioni (0 = modo 1).
    pub fn echo_mode(i: usize) -> Pos2 {
        let (c, r) = (i % 6, i / 6);
        let x = PANEL.0 + 930.0 + c as f32 * 61.0 + 27.0;
        let y = PANEL.1 + 46.0 + 64.0 + 34.0 + r as f32 * 44.0 + 18.0;
        at(x, y)
    }
}
