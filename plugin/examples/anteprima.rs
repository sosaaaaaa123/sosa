//! Salva un'immagine dell'interfaccia di SosaTune, senza aprire nessun DAW:
//!
//! ```bash
//! cargo run --release -p sosatune --example anteprima -- anteprima.png
//! ```

use std::fs::File;
use std::io::BufWriter;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "anteprima.png".to_owned());
    let image = sosatune::preview::render(2.0);

    let file = File::create(&path).expect("impossibile creare il file");
    let mut encoder = png::Encoder::new(BufWriter::new(file), image.width as u32, image.height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("errore nell'intestazione PNG");
    writer.write_image_data(&image.rgba).expect("errore nella scrittura PNG");
    println!("Anteprima salvata in {path} ({} x {} pixel)", image.width, image.height);
}
