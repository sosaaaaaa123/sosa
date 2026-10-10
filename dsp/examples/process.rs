//! Corregge l'intonazione di un file WAV, senza bisogno di un DAW.
//! Utile per provare il motore sulle tue registrazioni.
//!
//! Uso:
//!   cargo run --release -p sosatune_dsp --example process -- voce.wav voce_corretta.wav
//!
//! Opzioni (tutte facoltative):
//!   --tonalita C        tonica: C, C#, Db, D, ... oppure Do, Do#, Reb, Re, ...
//!   --scala maggiore    cromatica | maggiore | minore | minore-armonica |
//!                       pentatonica-maggiore | pentatonica-minore | blues
//!   --velocita 20       velocita' di correzione in ms (0 = effetto robotico)
//!   --quantita 100      quantita' di correzione in percentuale
//!   --trasponi 0        trasposizione in semitoni
//!   --voce alto         soprano | alto | bassa
//!   --humanize 0        0-100: note lunghe corrette piu' dolcemente
//!   --flex 0            0-100: Flex-Tune, corregge solo vicino alla nota
//!   --vibrato 0         da -100 (toglie) a +100 (aggiunge) il vibrato naturale
//!   --formanti 0        spostamento del timbro in semitoni (da -12 a +12)
//!   --sensibilita 30    0-100: piu' alta = riconosce anche voci con molta aria
//!   --pitch file.csv    salva anche l'andamento dell'intonazione (per grafici)

use sosatune_dsp::scale::{scale_notes, ScaleKind, VoiceType, NOTE_NAMES};
use sosatune_dsp::{wav, CorrectionSettings, PitchCorrector, MAX_CHANNELS};
use std::io::Write;
use std::process::exit;
use std::time::Instant;

fn parse_key(text: &str) -> Option<u8> {
    let t = text.trim().to_lowercase();
    let (base, rest) = if let Some(r) = t.strip_prefix("sol") {
        (7, r)
    } else if let Some(r) = t.strip_prefix("do") {
        (0, r)
    } else if let Some(r) = t.strip_prefix("re") {
        (2, r)
    } else if let Some(r) = t.strip_prefix("mi") {
        (4, r)
    } else if let Some(r) = t.strip_prefix("fa") {
        (5, r)
    } else if let Some(r) = t.strip_prefix("la") {
        (9, r)
    } else if let Some(r) = t.strip_prefix("si") {
        (11, r)
    } else {
        let mut chars = t.chars();
        let base = match chars.next()? {
            'c' => 0,
            'd' => 2,
            'e' => 4,
            'f' => 5,
            'g' => 7,
            'a' => 9,
            'b' => 11,
            _ => return None,
        };
        (base, chars.as_str())
    };
    let accidental: i32 = match rest {
        "" => 0,
        "#" | "s" | "diesis" => 1,
        "b" | "bemolle" => -1,
        _ => return None,
    };
    Some((base + accidental).rem_euclid(12) as u8)
}

fn parse_scale(text: &str) -> Option<ScaleKind> {
    Some(match text.trim().to_lowercase().as_str() {
        "cromatica" | "chromatic" => ScaleKind::Chromatic,
        "maggiore" | "major" => ScaleKind::Major,
        "minore" | "minor" => ScaleKind::NaturalMinor,
        "minore-armonica" | "harmonic-minor" => ScaleKind::HarmonicMinor,
        "pentatonica-maggiore" | "major-pentatonic" => ScaleKind::MajorPentatonic,
        "pentatonica-minore" | "minor-pentatonic" => ScaleKind::MinorPentatonic,
        "blues" => ScaleKind::Blues,
        _ => return None,
    })
}

fn parse_voice(text: &str) -> Option<VoiceType> {
    Some(match text.trim().to_lowercase().as_str() {
        "soprano" => VoiceType::Soprano,
        "alto" | "tenore" | "alto-tenore" => VoiceType::AltoTenor,
        "bassa" | "basso" | "baritono" => VoiceType::LowMale,
        _ => return None,
    })
}

fn fail(message: &str) -> ! {
    eprintln!("Errore: {message}");
    eprintln!(
        "Uso: process <ingresso.wav> <uscita.wav> [--tonalita C] [--scala maggiore] [--velocita 20] \
         [--quantita 100] [--trasponi 0] [--voce alto] [--humanize 0] [--flex 0] [--vibrato 0] \
         [--formanti 0] [--sensibilita 30] [--pitch file.csv]"
    );
    exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut positional = Vec::new();
    let mut key = 0u8;
    let mut scale = ScaleKind::Chromatic;
    let mut settings = CorrectionSettings::default();
    let mut voice = VoiceType::AltoTenor;
    let mut pitch_csv: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if let Some(name) = arg.strip_prefix("--") {
            let value = args.get(i + 1).unwrap_or_else(|| fail(&format!("manca il valore di --{name}")));
            let number = || value.parse::<f32>().unwrap_or_else(|_| fail(&format!("--{name}: '{value}' non e' un numero")));
            match name {
                "tonalita" | "key" => key = parse_key(value).unwrap_or_else(|| fail("tonalita' non riconosciuta")),
                "scala" | "scale" => scale = parse_scale(value).unwrap_or_else(|| fail("scala non riconosciuta")),
                "velocita" | "retune" => settings.retune_ms = number().clamp(0.0, 1000.0),
                "quantita" | "amount" => settings.amount = (number() / 100.0).clamp(0.0, 1.0),
                "trasponi" | "transpose" => settings.transpose = number().clamp(-24.0, 24.0),
                "voce" | "voice" => voice = parse_voice(value).unwrap_or_else(|| fail("tipo di voce non riconosciuto")),
                "humanize" => settings.humanize = (number() / 100.0).clamp(0.0, 1.0),
                "flex" | "flex-tune" => settings.flex_tune = (number() / 100.0).clamp(0.0, 1.0),
                "vibrato" => settings.vibrato = (number() / 100.0).clamp(-1.0, 1.0),
                "formanti" | "formant" => settings.formant = number().clamp(-12.0, 12.0),
                "sensibilita" | "tracking" => settings.tracking = (number() / 100.0).clamp(0.0, 1.0),
                "pitch" => pitch_csv = Some(value.clone()),
                _ => fail(&format!("opzione sconosciuta --{name}")),
            }
            i += 2;
        } else {
            positional.push(arg.clone());
            i += 1;
        }
    }
    if positional.len() != 2 {
        fail("servono un file di ingresso e uno di uscita");
    }
    settings.notes = scale_notes(key, scale);

    let audio = wav::read(&positional[0]).unwrap_or_else(|e| fail(&format!("lettura di {}: {e}", positional[0])));
    let channels = audio.channels.len().min(MAX_CHANNELS);
    let frames = audio.channels[0].len();
    let sr = audio.sample_rate as f32;

    let mut corrector = PitchCorrector::new(sr, channels);
    corrector.set_voice(voice);
    let latency = corrector.latency();
    println!(
        "{} — {:.1} s, {} Hz, {} canali | tonalita' {} {:?} | velocita' {} ms | latenza {:.1} ms",
        positional[0],
        frames as f32 / sr,
        audio.sample_rate,
        channels,
        NOTE_NAMES[key as usize],
        scale,
        settings.retune_ms,
        latency as f32 / sr * 1000.0
    );

    // Elaboriamo anche `latency` campioni di silenzio in coda, poi togliamo il
    // ritardo dall'inizio: cosi' il file corretto e' allineato all'originale.
    let mut output = vec![Vec::with_capacity(frames); channels];
    let mut pitch_rows = Vec::new();
    let started = Instant::now();
    for n in 0..frames + latency {
        let mut frame = [0.0f32; MAX_CHANNELS];
        for ch in 0..channels {
            frame[ch] = audio.channels[ch].get(n).copied().unwrap_or(0.0);
        }
        if let Some(report) = corrector.process_frame(&mut frame[..channels], &settings) {
            pitch_rows.push((n as f32 / sr, report));
        }
        if n >= latency {
            for ch in 0..channels {
                output[ch].push(frame[ch]);
            }
        }
    }
    let elapsed = started.elapsed().as_secs_f32();

    wav::write(&positional[1], audio.sample_rate, &output)
        .unwrap_or_else(|e| fail(&format!("scrittura di {}: {e}", positional[1])));

    let voiced = pitch_rows.iter().filter(|(_, r)| r.voiced).count();
    println!(
        "Fatto: {} scritto. Elaborazione {:.0}x piu' veloce del tempo reale. Nota rilevata nel {:.0}% del tempo.",
        positional[1],
        frames as f32 / sr / elapsed.max(1e-6),
        100.0 * voiced as f32 / pitch_rows.len().max(1) as f32
    );

    if let Some(path) = pitch_csv {
        let mut file = std::fs::File::create(&path).unwrap_or_else(|e| fail(&format!("{path}: {e}")));
        writeln!(file, "tempo_s,nota_cantata,nota_uscita,nota_bersaglio").unwrap();
        for (t, r) in &pitch_rows {
            // L'analisi descrive l'audio di qualche millisecondo prima.
            writeln!(file, "{:.4},{},{},{}", t, r.input_note, r.output_note, r.target_note).unwrap();
        }
        println!("Andamento dell'intonazione salvato in {path}");
    }
}
