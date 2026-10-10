//! # Anteprima dell'interfaccia, senza DAW e senza scheda grafica
//!
//! Disegna l'interfaccia vera di SosaTune (lo stesso codice di `editor.rs`)
//! in un'immagine: egui produce triangoli colorati e una "texture" con i
//! caratteri, e qui li coloriamo pixel per pixel con la CPU. Serve per vedere
//! com'e' fatto il plugin senza aprirlo in un DAW (vedi
//! `examples/anteprima.rs`, che salva il risultato in PNG).
//!
//! Per rendere l'anteprima realistica, prima del disegno una voce sintetica
//! leggermente stonata passa nel correttore vero: il grafico e l'indicatore
//! mostrano quindi dati reali.

use crate::editor::Headless;
use crate::gui::egui;
use crate::params::{SosaTuneParams, EDITOR_HEIGHT, EDITOR_WIDTH};
use crate::shared::PitchDisplay;
use nih_plug::prelude::*;
use sosatune_dsp::synth::{sing_melody, SungNote};
use sosatune_dsp::{CorrectionSettings, PitchCorrector, TubeCompressor};
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
    // Alcuni fotogrammi: i caratteri vengono caricati dal secondo, e il grafico
    // si centra sulla voce con dolcezza.
    for frame in 0..90 {
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

/// Fa cantare al correttore (e al compressore) la melodia di prova, con le
/// impostazioni del plugin, e mette il risultato nel grafico e nell'indicatore.
fn sing_into(params: &SosaTuneParams, display: &PitchDisplay) {
    let sr = 44100.0;
    let n = |note: f32, cents: f32, seconds: f32| SungNote { note, detune_cents: cents, seconds };
    let rest = |seconds: f32| SungNote { note: -1.0, detune_cents: 0.0, seconds };
    let melody = [
        n(60.0, 30.0, 0.45), n(62.0, -40.0, 0.45), n(64.0, 35.0, 0.45), n(60.0, -25.0, 0.45),
        n(60.0, 40.0, 0.45), n(62.0, -35.0, 0.45), n(64.0, 30.0, 0.45), n(60.0, -30.0, 0.45),
        rest(0.25),
        n(64.0, -30.0, 0.45), n(65.0, 40.0, 0.45), n(67.0, -22.0, 0.9),
    ];
    let (audio, _) = sing_melody(&melody, sr, 25.0);

    let mut corrector = PitchCorrector::new(sr, 1);
    corrector.set_voice(params.voice.value().to_dsp());
    let settings = CorrectionSettings {
        notes: params.active_notes(),
        retune_ms: params.retune_speed.value(),
        amount: params.amount.value(),
        humanize: params.humanize.value(),
        flex_tune: params.flex_tune.value(),
        tracking: params.tracking.value(),
        ..Default::default()
    };
    let comp_settings = params.comp.settings(false);
    let mut compressor = TubeCompressor::new(sr, 1);
    compressor.snap_to(&comp_settings);
    let latency_ms = (corrector.latency() + TubeCompressor::fixed_latency()) as f32 / sr * 1000.0;
    display.set_timing(latency_ms, corrector.analysis_interval());

    // Ci fermiamo a meta' dell'ultima nota, cosi' l'indicatore mostra una nota.
    let stop = (5.3 * sr) as usize;
    for &sample in audio.iter().take(stop) {
        let mut frame = [sample];
        if let Some(report) = corrector.process_frame(&mut frame, &settings) {
            display.push(&report);
        }
        compressor.process_frame(&mut frame, &comp_settings);
    }
    display.set_gain_reduction(compressor.gain_reduction_db());
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
    //! Prove dell'interfaccia del compressore: clic e trascinamenti "finti"
    //! sull'interfaccia vera, controllando quali parametri cambiano.

    use super::*;
    use crate::editor::{comp_layout, COMPRESSOR_ORIGIN};
    use crate::params::CompModeChoice;
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
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        }
    }

    fn click(pos: egui::Pos2) -> Vec<Vec<egui::Event>> {
        vec![vec![egui::Event::PointerMoved(pos)], vec![press(pos, true)], vec![press(pos, false)]]
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

    fn comp_point(dx: f32, dy: f32) -> egui::Pos2 {
        egui::pos2(COMPRESSOR_ORIGIN.0 + dx, COMPRESSOR_ORIGIN.1 + dy)
    }

    #[test]
    fn power_switch_turns_the_compressor_on_and_off() {
        let params = SosaTuneParams::default();
        assert!(!params.comp.enabled.value(), "deve partire spento");
        let changes = run(&params, click(comp_point(comp_layout::POWER.0, comp_layout::POWER.1)));
        let enabled = params.comp.enabled.as_ptr();
        assert!(
            changes.iter().any(|&(p, v)| p == enabled && v == 1.0),
            "la levetta non accende il compressore: {changes:?}"
        );

        let params = SosaTuneParams::preview();
        let changes = run(&params, click(comp_point(comp_layout::POWER.0, comp_layout::POWER.1)));
        assert!(changes.iter().any(|&(p, v)| p == params.comp.enabled.as_ptr() && v == 0.0));
    }

    #[test]
    fn mode_buttons_select_each_mode() {
        let params = SosaTuneParams::preview();
        let mode = params.comp.mode.as_ptr();
        // Le tre righe: Ottica (gia' scelta), Manuale, Ottica + manuale.
        for (row, expected) in [(1usize, CompModeChoice::Manual), (2, CompModeChoice::FixedManual)] {
            let changes = run(&params, click(comp_point(comp_layout::mode(row).0, comp_layout::mode(row).1)));
            let normalized = params.comp.mode.preview_normalized(expected);
            assert!(
                changes.iter().any(|&(p, v)| p == mode && (v - normalized).abs() < 1e-6),
                "riga {row}: {changes:?}"
            );
        }
        // Cliccare la modalita' gia' scelta non cambia nulla.
        let changes = run(&params, click(comp_point(comp_layout::mode(0).0, comp_layout::mode(0).1)));
        assert!(changes.iter().all(|&(p, _)| p != mode));
    }

    #[test]
    fn every_knob_follows_the_mouse() {
        let params = SosaTuneParams::preview();
        let comp = &params.comp;
        let knobs = [
            comp.threshold.as_ptr(),
            comp.ratio.as_ptr(),
            comp.attack.as_ptr(),
            comp.release.as_ptr(),
            comp.tube.as_ptr(),
            comp.mix.as_ptr(),
            comp.output.as_ptr(),
        ];
        let starts = [
            comp.threshold.unmodulated_normalized_value(),
            comp.ratio.unmodulated_normalized_value(),
            comp.attack.unmodulated_normalized_value(),
            comp.release.unmodulated_normalized_value(),
            comp.tube.unmodulated_normalized_value(),
            comp.mix.unmodulated_normalized_value(),
            comp.output.unmodulated_normalized_value(),
        ];
        for (i, (&ptr, &start)) in knobs.iter().zip(starts.iter()).enumerate() {
            let changes = run(&params, drag_up(comp_point(comp_layout::knob(i).0, comp_layout::knob(i).1), 40.0));
            let last = changes.iter().filter(|(p, _)| *p == ptr).map(|&(_, v)| v).last();
            let last = last.unwrap_or_else(|| panic!("la manopola {i} non risponde: {changes:?}"));
            // In su = valore piu' alto (o al massimo, se c'era gia').
            assert!(last > start || start >= 1.0, "manopola {i}: {start} -> {last}");
            // Nessun altro parametro deve cambiare.
            assert!(changes.iter().all(|(p, _)| *p == ptr), "manopola {i}: cambiano altri parametri");
        }
    }

    #[test]
    fn plugin_bypass_also_bypasses_the_compressor() {
        let params = SosaTuneParams::preview();
        assert!(params.comp.settings(false).enabled);
        assert!(!params.comp.settings(true).enabled);
        let defaults = SosaTuneParams::default();
        assert!(!defaults.comp.settings(false).enabled, "spento per i progetti esistenti");
    }

    #[test]
    fn preview_renders() {
        let image = render(1.0);
        assert_eq!(image.width, EDITOR_WIDTH as usize);
        assert_eq!(image.height, EDITOR_HEIGHT as usize);
        // Non e' un'immagine vuota: ci sono pixel chiari (testi, quadrante).
        let bright = image.rgba.chunks(4).filter(|p| p[0] > 200 && p[1] > 180).count();
        assert!(bright > 1000, "anteprima quasi vuota ({bright} pixel chiari)");
    }
}
