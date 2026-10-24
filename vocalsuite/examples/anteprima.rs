//! Salva un'immagine dell'interfaccia di SR Vocal Suite, senza aprire nessun DAW:
//!
//! ```bash
//! cargo run --release -p srvocalsuite --example anteprima -- anteprima.png
//! ```
//!
//! Salva anche `anteprima-impostazioni.png`, `anteprima-preset.png`,
//! `anteprima-guida.png` e `anteprima-tonalita.png`.

use std::fs::File;
use std::io::BufWriter;

use srvocalsuite::preview::{render_view, PreviewImage, View};

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "anteprima.png".to_owned());
    let scale: f32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(2.0);
    save(&path, &render_view(scale, View::Main));
    let stem = path.trim_end_matches(".png");
    for (view, suffix) in [(View::Settings, "impostazioni"), (View::Presets, "preset"), (View::Help, "guida"), (View::KeyMenu, "tonalita")] {
        save(&format!("{stem}-{suffix}.png"), &render_view(scale, view));
    }
}

fn save(path: &str, image: &PreviewImage) {
    let file = File::create(path).expect("impossibile creare il file");
    let mut encoder = png::Encoder::new(BufWriter::new(file), image.width as u32, image.height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("errore nell'intestazione PNG");
    writer.write_image_data(&image.rgba).expect("errore nella scrittura PNG");
    println!("Anteprima salvata in {path} ({} x {} pixel)", image.width, image.height);
}
