//! Salva un'immagine dell'interfaccia di SosaTune, senza aprire nessun DAW:
//!
//! ```bash
//! cargo run --release -p sosatune --example anteprima -- anteprima.png
//! ```
//!
//! Salva anche `anteprima-impostazioni.png`, `anteprima-preset.png` e
//! `anteprima-guida.png` (pannelli e menu aperti).

use std::fs::File;
use std::io::BufWriter;

use sosatune::preview::{render_view, PreviewImage, View};

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "anteprima.png".to_owned());
    save(&path, &render_view(2.0, View::Main));
    // Accanto: pannello impostazioni, lista dei preset e guida rapida.
    let stem = path.trim_end_matches(".png");
    for (view, suffix) in [(View::Settings, "impostazioni"), (View::Presets, "preset"), (View::Help, "guida")] {
        save(&format!("{stem}-{suffix}.png"), &render_view(2.0, view));
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
