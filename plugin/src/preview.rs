//! # Anteprima dell'interfaccia, senza DAW e senza scheda grafica
//!
//! Disegna l'interfaccia vera di SosaTune (lo stesso codice di `editor.rs`)
//! in un'immagine: egui produce triangoli colorati e una "texture" con i
//! caratteri, e qui li coloriamo pixel per pixel con la CPU. Serve per vedere
//! com'e' fatto il plugin senza aprirlo in un DAW (vedi
//! `examples/anteprima.rs`, che salva il risultato in PNG).
//!
//! Per rendere l'anteprima realistica, prima del disegno una voce sintetica
//! leggermente stonata passa nel correttore vero: la nota nella sfera e
//! l'indicatore mostrano quindi dati reali.

use crate::editor::Headless;
use crate::gui::egui;
use crate::params::{SosaTuneParams, EDITOR_HEIGHT, EDITOR_WIDTH};
use crate::shared::PitchDisplay;
use nih_plug::prelude::*;
use sosatune_dsp::synth::{sing_melody, SungNote};
use sosatune_dsp::{CorrectionSettings, PitchCorrector};
use std::collections::HashMap;

/// Un'immagine RGBA (8 bit per canale, alfa sempre 255).
pub struct PreviewImage {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// Disegna l'interfaccia. `scale` = 2 per un'immagine nitida come sugli
/// schermi Retina.
pub fn render(scale: f32) -> PreviewImage {
    let params = SosaTuneParams::preview();
    let display = PitchDisplay::default();
    sing_into(&params, &display);

    let ctx = egui::Context::default();
    let mut gui = Headless::new(&ctx);
    let host = PreviewHost;
    let setter = ParamSetter::new(&host);

    let mut textures: HashMap<egui::TextureId, Texture> = HashMap::new();
    let size = egui::vec2(EDITOR_WIDTH as f32, EDITOR_HEIGHT as f32);
    let mut primitives = Vec::new();
    let mut pixels_per_point = scale;
    // Alcuni fotogrammi: i caratteri vengono caricati dal secondo, e
    // l'indicatore si posa sulla voce con dolcezza.
    let started = std::time::Instant::now();
    for frame in 0..40 {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(frame as f64 / 30.0),
            ..Default::default()
        };
        input.viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo { native_pixels_per_point: Some(scale), ..Default::default() },
        );
        // Il "DAW" finto manda audio: gli indicatori IN/OUT si muovono.
        display.set_block(0.42, 0.37, false);
        let output = ctx.run(input, |ctx| gui.draw(ctx, &params, &display, &setter));
        for (id, delta) in output.textures_delta.set {
            apply_texture(&mut textures, id, delta);
        }
        for id in output.textures_delta.free {
            textures.remove(&id);
        }
        pixels_per_point = output.pixels_per_point;
        primitives = ctx.tessellate(output.shapes, pixels_per_point);
    }

    eprintln!(
        "Interfaccia: {:.2} ms per fotogramma (disegno + tassellazione)",
        started.elapsed().as_secs_f64() * 1000.0 / 40.0
    );
    let width = (EDITOR_WIDTH as f32 * pixels_per_point).round() as usize;
    let height = (EDITOR_HEIGHT as f32 * pixels_per_point).round() as usize;
    let mut canvas = Canvas { width, height, pixels: vec![[0.0; 4]; width * height] };
    for primitive in &primitives {
        if let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive {
            if let Some(texture) = textures.get(&mesh.texture_id) {
                canvas.draw_mesh(mesh, texture, primitive.clip_rect, pixels_per_point);
            }
        }
    }

    let mut rgba = Vec::with_capacity(width * height * 4);
    for p in &canvas.pixels {
        for c in &p[..3] {
            rgba.push((c.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
        rgba.push(255);
    }
    PreviewImage { width, height, rgba }
}

/// Fa cantare al correttore la melodia di prova (in La maggiore), con le
/// impostazioni del plugin, e mette il risultato nell'indicatore.
fn sing_into(params: &SosaTuneParams, display: &PitchDisplay) {
    let sr = 44100.0;
    let n = |note: f32, cents: f32, seconds: f32| SungNote { note, detune_cents: cents, seconds };
    let melody = [
        n(64.0, 30.0, 0.45), n(66.0, -40.0, 0.45), n(68.0, 25.0, 0.45), n(64.0, -25.0, 0.45),
        n(66.0, 35.0, 0.45), n(69.0, -18.0, 1.2),
    ];
    let (audio, _) = sing_melody(&melody, sr, 25.0);

    let mut corrector = PitchCorrector::new(sr, 1);
    corrector.set_quality(params.quality.value() == crate::params::QualityChoice::Hq);
    corrector.set_voice(params.voice.value().to_dsp());
    let settings: CorrectionSettings = params.correction_settings();
    let latency_ms = corrector.latency() as f32 / sr * 1000.0;
    display.set_timing(latency_ms, corrector.analysis_interval());

    // Ci fermiamo a meta' dell'ultima nota, cosi' l'indicatore mostra una nota.
    let stop = (3.0 * sr) as usize;
    for &sample in audio.iter().take(stop) {
        let mut frame = [sample];
        if let Some(report) = corrector.process_frame(&mut frame, &settings) {
            display.push(&report);
            if report.voiced {
                display.add_note(report.input_note, corrector.analysis_interval());
            }
        }
    }
}

/// Un "host" finto: l'anteprima non cambia nessun parametro.
struct PreviewHost;

impl GuiContext for PreviewHost {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Vst3
    }

    fn request_resize(&self) -> bool {
        false
    }

    unsafe fn raw_begin_set_parameter(&self, _param: ParamPtr) {}

    unsafe fn raw_set_parameter_normalized(&self, _param: ParamPtr, _normalized: f32) {}

    unsafe fn raw_end_set_parameter(&self, _param: ParamPtr) {}

    fn get_state(&self) -> PluginState {
        unreachable!("l'anteprima non salva lo stato")
    }

    fn set_state(&self, _state: PluginState) {}
}

/// Una texture: colori premoltiplicati per l'alfa, da 0 a 1.
struct Texture {
    width: usize,
    height: usize,
    pixels: Vec<[f32; 4]>,
}

impl Texture {
    /// Lettura con interpolazione bilineare (come fa la scheda grafica).
    fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let x = (u * self.width as f32 - 0.5).clamp(0.0, (self.width - 1) as f32);
        let y = (v * self.height as f32 - 0.5).clamp(0.0, (self.height - 1) as f32);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.width - 1), (y0 + 1).min(self.height - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let at = |x: usize, y: usize| self.pixels[y * self.width + x];
        let (a, b, c, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
        let mut out = [0.0; 4];
        for i in 0..4 {
            let top = a[i] + fx * (b[i] - a[i]);
            let bottom = c[i] + fx * (d[i] - c[i]);
            out[i] = top + fy * (bottom - top);
        }
        out
    }
}

fn to_f32(c: egui::Color32) -> [f32; 4] {
    [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, c.a() as f32 / 255.0]
}

fn apply_texture(
    textures: &mut HashMap<egui::TextureId, Texture>,
    id: egui::TextureId,
    delta: egui::epaint::ImageDelta,
) {
    let [w, h] = delta.image.size();
    let pixels: Vec<[f32; 4]> = match &delta.image {
        egui::ImageData::Color(image) => image.pixels.iter().map(|&c| to_f32(c)).collect(),
        egui::ImageData::Font(image) => image.srgba_pixels(None).map(to_f32).collect(),
    };
    match delta.pos {
        None => {
            textures.insert(id, Texture { width: w, height: h, pixels });
        }
        Some([px, py]) => {
            if let Some(texture) = textures.get_mut(&id) {
                for y in 0..h {
                    for x in 0..w {
                        let (tx, ty) = (px + x, py + y);
                        if tx < texture.width && ty < texture.height {
                            texture.pixels[ty * texture.width + tx] = pixels[y * w + x];
                        }
                    }
                }
            }
        }
    }
}

struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<[f32; 4]>,
}

impl Canvas {
    fn draw_mesh(&mut self, mesh: &egui::epaint::Mesh, texture: &Texture, clip: egui::Rect, ppp: f32) {
        let clip_min_x = (clip.min.x * ppp).max(0.0);
        let clip_min_y = (clip.min.y * ppp).max(0.0);
        let clip_max_x = (clip.max.x * ppp).min(self.width as f32);
        let clip_max_y = (clip.max.y * ppp).min(self.height as f32);
        if clip_min_x >= clip_max_x || clip_min_y >= clip_max_y {
            return;
        }
        for tri in mesh.indices.chunks_exact(3) {
            let v = [
                &mesh.vertices[tri[0] as usize],
                &mesh.vertices[tri[1] as usize],
                &mesh.vertices[tri[2] as usize],
            ];
            let p = [v[0].pos * ppp, v[1].pos * ppp, v[2].pos * ppp];
            let area = edge(p[0], p[1], p[2]);
            if area.abs() < 1e-8 {
                continue;
            }
            let min_x = p.iter().map(|q| q.x).fold(f32::MAX, f32::min).max(clip_min_x).floor() as usize;
            let max_x = p.iter().map(|q| q.x).fold(f32::MIN, f32::max).min(clip_max_x).ceil() as usize;
            let min_y = p.iter().map(|q| q.y).fold(f32::MAX, f32::min).max(clip_min_y).floor() as usize;
            let max_y = p.iter().map(|q| q.y).fold(f32::MIN, f32::max).min(clip_max_y).ceil() as usize;
            let colors = [to_f32(v[0].color), to_f32(v[1].color), to_f32(v[2].color)];
            for y in min_y..max_y.min(self.height) {
                for x in min_x..max_x.min(self.width) {
                    let point = egui::pos2(x as f32 + 0.5, y as f32 + 0.5);
                    if point.x < clip_min_x || point.x >= clip_max_x || point.y < clip_min_y || point.y >= clip_max_y {
                        continue;
                    }
                    let w0 = edge(p[1], p[2], point) / area;
                    let w1 = edge(p[2], p[0], point) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                        continue;
                    }
                    let u = w0 * v[0].uv.x + w1 * v[1].uv.x + w2 * v[2].uv.x;
                    let t = w0 * v[0].uv.y + w1 * v[1].uv.y + w2 * v[2].uv.y;
                    let texel = texture.sample(u, t);
                    let mut src = [0.0f32; 4];
                    for i in 0..4 {
                        let c = w0 * colors[0][i] + w1 * colors[1][i] + w2 * colors[2][i];
                        src[i] = c * texel[i];
                    }
                    // Fusione con alfa premoltiplicato (come egui sulla scheda grafica).
                    let dst = &mut self.pixels[y * self.width + x];
                    for i in 0..4 {
                        dst[i] = src[i] + dst[i] * (1.0 - src[3]);
                    }
                }
            }
        }
    }
}

#[inline]
fn edge(a: egui::Pos2, b: egui::Pos2, c: egui::Pos2) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

#[cfg(test)]
mod tests {
    //! Prove dell'interfaccia: clic e trascinamenti "finti" sull'interfaccia
    //! vera, controllando quali parametri cambiano.

    use super::*;
    use crate::editor::layout;
    use crate::params::{Key, QualityChoice, ScaleChoice, StyleChoice, VoiceChoice};
    use crate::presets::FACTORY;
    use std::sync::atomic::Ordering;
    use std::sync::Mutex;

    /// Un host che registra ogni modifica di parametro chiesta dall'interfaccia.
    #[derive(Default)]
    struct RecordingHost {
        changes: Mutex<Vec<(ParamPtr, f32)>>,
    }

    impl GuiContext for RecordingHost {
        fn plugin_api(&self) -> PluginApi {
            PluginApi::Vst3
        }
        fn request_resize(&self) -> bool {
            false
        }
        unsafe fn raw_begin_set_parameter(&self, _param: ParamPtr) {}
        unsafe fn raw_set_parameter_normalized(&self, param: ParamPtr, normalized: f32) {
            self.changes.lock().unwrap().push((param, normalized));
        }
        unsafe fn raw_end_set_parameter(&self, _param: ParamPtr) {}
        fn get_state(&self) -> PluginState {
            unreachable!()
        }
        fn set_state(&self, _state: PluginState) {}
    }

    /// Fa girare l'interfaccia con una sequenza di eventi (uno per fotogramma).
    fn run(params: &SosaTuneParams, frames: Vec<Vec<egui::Event>>) -> Vec<(ParamPtr, f32)> {
        let display = PitchDisplay::default();
        let host = RecordingHost::default();
        let setter = ParamSetter::new(&host);
        let ctx = egui::Context::default();
        let mut gui = Headless::new(&ctx);
        let size = egui::vec2(EDITOR_WIDTH as f32, EDITOR_HEIGHT as f32);
        // Due fotogrammi a vuoto (caricamento dei caratteri), poi gli eventi.
        let mut all = vec![Vec::new(), Vec::new()];
        all.extend(frames);
        all.push(Vec::new());
        for (i, events) in all.into_iter().enumerate() {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(i as f64 / 60.0),
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| gui.draw(ctx, params, &display, &setter));
        }
        let changes = host.changes.lock().unwrap().clone();
        changes
    }

    fn press(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::default() }
    }

    fn click(pos: egui::Pos2) -> Vec<Vec<egui::Event>> {
        vec![vec![egui::Event::PointerMoved(pos)], vec![press(pos, true)], vec![press(pos, false)], vec![]]
    }

    /// Trascina in su di `pixels` punti.
    fn drag_up(pos: egui::Pos2, pixels: f32) -> Vec<Vec<egui::Event>> {
        let mut frames = vec![vec![egui::Event::PointerMoved(pos)], vec![press(pos, true)]];
        let steps = 10;
        for i in 1..=steps {
            frames.push(vec![egui::Event::PointerMoved(pos - egui::vec2(0.0, pixels * i as f32 / steps as f32))]);
        }
        frames.push(vec![press(pos - egui::vec2(0.0, pixels), false)]);
        frames
    }

    fn last_value(changes: &[(ParamPtr, f32)], ptr: ParamPtr) -> Option<f32> {
        changes.iter().filter(|(p, _)| *p == ptr).map(|&(_, v)| v).last()
    }

    #[test]
    fn every_knob_follows_the_mouse() {
        let params = SosaTuneParams::preview();
        let knobs = [
            (layout::retune(), params.retune_speed.as_ptr(), params.retune_speed.unmodulated_normalized_value()),
            (layout::humanize(), params.humanize.as_ptr(), params.humanize.unmodulated_normalized_value()),
            (layout::flex(), params.flex_tune.as_ptr(), params.flex_tune.unmodulated_normalized_value()),
            (layout::detune(), params.detune.as_ptr(), params.detune.unmodulated_normalized_value()),
        ];
        for (i, &(pos, ptr, start)) in knobs.iter().enumerate() {
            let changes = run(&params, drag_up(pos, 40.0));
            let last = last_value(&changes, ptr).unwrap_or_else(|| panic!("la manopola {i} non risponde: {changes:?}"));
            assert!(last > start, "manopola {i}: {start} -> {last}");
            assert!(changes.iter().all(|(p, _)| *p == ptr), "manopola {i}: cambiano altri parametri");
        }
    }

    #[test]
    fn mode_and_style_buttons_work() {
        let params = SosaTuneParams::preview();
        let changes = run(&params, click(layout::mode_hq()));
        let hq = params.quality.preview_normalized(QualityChoice::Hq);
        assert_eq!(last_value(&changes, params.quality.as_ptr()), Some(hq), "HQ: {changes:?}");
        // La modalita' gia' scelta non cambia nulla.
        let changes = run(&params, click(layout::mode_low()));
        assert!(changes.is_empty(), "{changes:?}");

        let changes = run(&params, click(layout::classic()));
        let classic = params.style.preview_normalized(StyleChoice::Classic);
        assert_eq!(last_value(&changes, params.style.as_ptr()), Some(classic), "Classic: {changes:?}");
        let changes = run(&params, click(layout::modern()));
        assert!(changes.is_empty(), "{changes:?}");
    }

    #[test]
    fn power_button_is_the_daw_bypass() {
        let params = SosaTuneParams::preview();
        let changes = run(&params, click(layout::power()));
        assert_eq!(last_value(&changes, params.bypass.as_ptr()), Some(1.0), "{changes:?}");
    }

    #[test]
    fn clicking_a_note_edits_the_scale() {
        let params = SosaTuneParams::preview();
        // La maggiore: il Do (naturale) non c'e'. Cliccandolo entra nella scala
        // personalizzata insieme alle note di La maggiore.
        let changes = run(&params, click(layout::note(0)));
        let custom = params.scale.preview_normalized(ScaleChoice::Custom);
        assert_eq!(last_value(&changes, params.scale.as_ptr()), Some(custom));
        assert_eq!(last_value(&changes, params.custom.note(0).as_ptr()), Some(1.0), "Do aggiunto");
        assert_eq!(last_value(&changes, params.custom.note(9).as_ptr()), Some(1.0), "La resta");
        assert_eq!(last_value(&changes, params.custom.note(10).as_ptr()), Some(0.0), "La# fuori");
    }

    #[test]
    fn preset_arrows_and_list_load_presets() {
        let params = SosaTuneParams::preview();
        let changes = run(&params, click(layout::preset_next()));
        let retune = params.retune_speed.preview_normalized(FACTORY[1].retune_ms);
        assert_eq!(last_value(&changes, params.retune_speed.as_ptr()), Some(retune), "{changes:?}");
        assert_eq!(params.preset.load(Ordering::Relaxed), 1);

        let params = SosaTuneParams::preview();
        let changes = run(&params, click(layout::preset_prev()));
        let last = FACTORY.len() - 1;
        let retune = params.retune_speed.preview_normalized(FACTORY[last].retune_ms);
        assert_eq!(last_value(&changes, params.retune_speed.as_ptr()), Some(retune));
        let style = params.style.preview_normalized(FACTORY[last].style);
        assert_eq!(last_value(&changes, params.style.as_ptr()), Some(style));

        let params = SosaTuneParams::preview();
        let mut frames = click(layout::preset_list());
        frames.extend(click(layout::preset_item(3)));
        let changes = run(&params, frames);
        let retune = params.retune_speed.preview_normalized(FACTORY[3].retune_ms);
        assert_eq!(last_value(&changes, params.retune_speed.as_ptr()), Some(retune), "{changes:?}");
    }

    #[test]
    fn dropdowns_set_voice_key_and_scale() {
        let params = SosaTuneParams::preview();
        let mut frames = click(layout::voice_field());
        frames.extend(click(layout::voice_item(0)));
        let changes = run(&params, frames);
        let soprano = params.voice.preview_normalized(VoiceChoice::Soprano);
        assert_eq!(last_value(&changes, params.voice.as_ptr()), Some(soprano), "{changes:?}");

        let params = SosaTuneParams::preview();
        let mut frames = click(layout::key_field());
        frames.extend(click(layout::key_item(7)));
        frames.extend(click(layout::scale_item(1)));
        let changes = run(&params, frames);
        assert_eq!(last_value(&changes, params.key.as_ptr()), Some(params.key.preview_normalized(Key::G)), "{changes:?}");
        let minor = params.scale.preview_normalized(ScaleChoice::NaturalMinor);
        assert_eq!(last_value(&changes, params.scale.as_ptr()), Some(minor), "{changes:?}");
    }

    #[test]
    fn settings_panel_knobs_work() {
        let params = SosaTuneParams::preview();
        let start = params.transpose.unmodulated_normalized_value();
        let mut frames = click(layout::settings());
        frames.extend(drag_up(layout::settings_knob(1), 40.0));
        let changes = run(&params, frames);
        let last = last_value(&changes, params.transpose.as_ptr()).expect("Transpose non risponde");
        assert!(last > start, "{start} -> {last}");
    }

    #[test]
    fn autokey_button_toggles_listening() {
        let params = SosaTuneParams::preview();
        params.autokey.store(false, Ordering::Relaxed);
        assert!(!params.autokey.load(Ordering::Relaxed));
        run(&params, click(layout::autokey()));
        assert!(params.autokey.load(Ordering::Relaxed));
        run(&params, click(layout::autokey()));
        assert!(!params.autokey.load(Ordering::Relaxed));
    }

    #[test]
    fn help_and_undo_buttons_do_not_break_anything() {
        let params = SosaTuneParams::preview();
        let mut frames = click(layout::help());
        frames.extend(click(layout::help()));
        frames.extend(click(layout::undo()));
        frames.extend(click(layout::redo()));
        let changes = run(&params, frames);
        // Nulla da annullare: nessun parametro cambia.
        assert!(changes.is_empty(), "{changes:?}");
    }

    #[test]
    fn preview_renders() {
        let image = render(1.0);
        assert_eq!(image.width, EDITOR_WIDTH as usize);
        assert_eq!(image.height, EDITOR_HEIGHT as usize);
        // Non e' un'immagine vuota: ci sono pixel chiari (testi, sfera).
        let bright = image.rgba.chunks(4).filter(|p| p[0] > 200 && p[1] > 200 && p[2] > 200).count();
        assert!(bright > 1000, "anteprima quasi vuota ({bright} pixel chiari)");
    }
}
