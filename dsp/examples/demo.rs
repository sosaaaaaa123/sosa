//! Demo: un cantante sintetico canta una melodia stonata, e SosaTune la corregge.
//!
//!   cargo run --release -p sosatune_dsp --example demo
//!
//! Crea questi file nella cartella corrente:
//!   demo_1_originale.wav          la melodia stonata
//!   demo_2_naturale.wav           stile "Naturale" (velocita' 80 ms con Humanize)
//!   demo_3_robotico.wav           stile "Robotico" (velocita' 0 ms, effetto "Auto-Tune")
//!   demo_4_robotico_vibrato.wav   robotico ma con il vibrato naturale rimesso (+100%)
//!   demo_5_formanti.wav           naturale con le formanti alzate di 4 semitoni
//! e demo_intonazione.csv con l'andamento delle note (per farne un grafico).

use sosatune_dsp::scale::{scale_notes, ScaleKind};
use sosatune_dsp::synth::{sing_melody, SungNote};
use sosatune_dsp::{wav, CorrectionSettings, PitchCorrector};
use std::io::Write;

fn correct(input: &[f32], sr: f32, settings: CorrectionSettings) -> (Vec<f32>, Vec<f32>) {
    let settings = CorrectionSettings {
        notes: scale_notes(0, ScaleKind::Major), // Do maggiore
        ..settings
    };
    let mut corrector = PitchCorrector::new(sr, 1);
    let latency = corrector.latency();
    let mut out = Vec::with_capacity(input.len());
    let mut notes = Vec::new();
    for n in 0..input.len() + latency {
        let mut frame = [input.get(n).copied().unwrap_or(0.0)];
        if let Some(r) = corrector.process_frame(&mut frame, &settings) {
            notes.push(r.output_note);
        }
        if n >= latency {
            out.push(frame[0]);
        }
    }
    (out, notes)
}

fn main() {
    let sr = 44100.0;
    // "Fra Martino" in Do maggiore, cantato con stonature tra 25 e 45 centesimi.
    let n = |note: f32, cents: f32, seconds: f32| SungNote { note, detune_cents: cents, seconds };
    let rest = |seconds: f32| SungNote { note: -1.0, detune_cents: 0.0, seconds };
    let melody = [
        n(60.0, 30.0, 0.45), n(62.0, -40.0, 0.45), n(64.0, 35.0, 0.45), n(60.0, -25.0, 0.45),
        n(60.0, 40.0, 0.45), n(62.0, -35.0, 0.45), n(64.0, 30.0, 0.45), n(60.0, -30.0, 0.45),
        rest(0.25),
        n(64.0, -30.0, 0.45), n(65.0, 40.0, 0.45), n(67.0, -45.0, 0.9),
        rest(0.25),
        n(64.0, 35.0, 0.45), n(65.0, -30.0, 0.45), n(67.0, 40.0, 0.9),
        rest(0.4),
    ];
    let (original, sung) = sing_melody(&melody, sr, 25.0);
    let natural_style = CorrectionSettings {
        retune_ms: 80.0,
        humanize: 0.35,
        ..Default::default()
    };
    let robotic_style = CorrectionSettings { retune_ms: 0.0, ..Default::default() };
    let (natural, natural_notes) = correct(&original, sr, natural_style);
    let (robotic, robotic_notes) = correct(&original, sr, robotic_style);
    let (robotic_vibrato, _) = correct(&original, sr, CorrectionSettings { vibrato: 1.0, ..robotic_style });
    let (formants, _) = correct(&original, sr, CorrectionSettings { formant: 4.0, ..natural_style });

    wav::write("demo_1_originale.wav", sr as u32, std::slice::from_ref(&original)).unwrap();
    wav::write("demo_2_naturale.wav", sr as u32, &[natural]).unwrap();
    wav::write("demo_3_robotico.wav", sr as u32, &[robotic]).unwrap();
    wav::write("demo_4_robotico_vibrato.wav", sr as u32, &[robotic_vibrato]).unwrap();
    wav::write("demo_5_formanti.wav", sr as u32, &[formants]).unwrap();

    // Andamento delle note: cantata (esatta, dal sintetizzatore) e corrette.
    let hop = (sr * 0.005).round() as usize;
    let mut csv = std::fs::File::create("demo_intonazione.csv").unwrap();
    writeln!(csv, "tempo_s,cantata,naturale,robotica").unwrap();
    for (i, (a, b)) in natural_notes.iter().zip(&robotic_notes).enumerate() {
        let t = (i + 1) * hop;
        // L'analisi i-esima descrive l'audio di circa mezza finestra prima.
        let sung_note = sung.get(t.saturating_sub(hop * 2)).copied().unwrap_or(f32::NAN);
        writeln!(csv, "{:.4},{},{},{}", t as f32 / sr, sung_note, a, b).unwrap();
    }

    println!("Creati 5 file demo_*.wav e demo_intonazione.csv");
}
