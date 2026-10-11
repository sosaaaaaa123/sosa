//! Test delle funzioni "professionali": Humanize, Flex-Tune, vibrato
//! naturale, formanti e sensibilita' del rilevamento.

use sosatune_dsp::fft::{Complex, Fft};
use sosatune_dsp::scale::{hz_to_note, note_to_hz};
use sosatune_dsp::synth::{sing_curve, sing_curve_detailed};
use sosatune_dsp::yin::YinDetector;
use sosatune_dsp::{CorrectionSettings, PitchCorrector, PitchReport};
use std::f32::consts::PI;

const SR: f32 = 48000.0;

/// Una voce che segue la curva `note(t)` (nota MIDI in funzione del tempo).
fn voice(seconds: f32, note: impl Fn(f32) -> f32) -> Vec<f32> {
    let n = (SR * seconds) as usize;
    let freqs: Vec<f32> = (0..n).map(|i| note_to_hz(note(i as f32 / SR))).collect();
    sing_curve(&freqs, &vec![1.0; n], SR)
}

/// Elabora e restituisce (uscita audio, (tempo, rapporto) per ogni analisi).
fn run(input: &[f32], settings: &CorrectionSettings) -> (Vec<f32>, Vec<(f32, PitchReport)>) {
    let mut corrector = PitchCorrector::new(SR, 1);
    let mut out = Vec::with_capacity(input.len());
    let mut reports = Vec::new();
    for (i, &x) in input.iter().enumerate() {
        let mut f = [x];
        if let Some(r) = corrector.process_frame(&mut f, settings) {
            reports.push((i as f32 / SR, r));
        }
        out.push(f[0]);
    }
    (out, reports)
}

fn outputs_between(reports: &[(f32, PitchReport)], from: f32, to: f32) -> Vec<f32> {
    reports
        .iter()
        .filter(|(t, r)| *t >= from && *t <= to && r.voiced)
        .map(|(_, r)| r.output_note)
        .collect()
}

fn std_dev(values: &[f32]) -> f32 {
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    (values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / values.len() as f32).sqrt()
}

fn mean(values: &[f32]) -> f32 {
    values.iter().sum::<f32>() / values.len() as f32
}

#[test]
fn humanize_relaxes_only_long_notes() {
    // Una nota tenuta che "ondeggia" lentamente di ±30 centesimi.
    let input = voice(2.5, |t| 60.0 + 0.3 * (2.0 * PI * 0.6 * t).sin());
    let strict = CorrectionSettings { retune_ms: 0.0, ..Default::default() };
    let human = CorrectionSettings { humanize: 1.0, ..strict };

    let (_, r_strict) = run(&input, &strict);
    let (_, r_human) = run(&input, &human);

    // Senza Humanize la nota resta inchiodata sul Do.
    let tail_strict = outputs_between(&r_strict, 1.0, 2.4);
    assert!(std_dev(&tail_strict) < 0.01, "senza humanize: {}", std_dev(&tail_strict));
    // Con Humanize, sulla parte tenuta passa un po' del movimento naturale...
    let tail_human = outputs_between(&r_human, 1.0, 2.4);
    let movement = std_dev(&tail_human);
    assert!(movement > 0.03 && movement < 0.2, "movimento con humanize: {movement}");
    // ...ma l'inizio della nota e' corretto come senza Humanize.
    let start_human = outputs_between(&r_human, 0.05, 0.12);
    assert!(start_human.iter().all(|n| (n - 60.0).abs() < 0.02), "{start_human:?}");
}

#[test]
fn flex_tune_leaves_expressive_notes_alone() {
    let settings = |flex: f32| CorrectionSettings { retune_ms: 0.0, flex_tune: flex, ..Default::default() };
    let final_output = |sung: f32, flex: f32| {
        let input = voice(1.0, |_| sung);
        let (_, reports) = run(&input, &settings(flex));
        mean(&outputs_between(&reports, 0.5, 0.95))
    };
    // 40 centesimi crescente: senza Flex-Tune corretta, con Flex-Tune lasciata stare.
    assert!((final_output(60.4, 0.0) - 60.0).abs() < 0.02);
    assert!((final_output(60.4, 1.0) - 60.4).abs() < 0.03);
    // 4 centesimi crescente: corretta anche con Flex-Tune al massimo.
    assert!((final_output(60.04, 1.0) - 60.0).abs() < 0.02);
}

#[test]
fn natural_vibrato_can_be_added_or_removed() {
    // Nota un po' crescente (+10 centesimi) con vibrato di ±35 centesimi a 5,5 Hz.
    // (Un vibrato che supera la meta' tra due note farebbe saltare la
    // correzione robotica da una nota all'altra, come in ogni Auto-Tune.)
    let input = voice(2.0, |t| 60.1 + 0.35 * (2.0 * PI * 5.5 * t).sin());
    let input_depth = 0.35 / 2f32.sqrt(); // deviazione standard di una sinusoide

    let base = CorrectionSettings { retune_ms: 0.0, ..Default::default() };
    let (_, flat) = run(&input, &base);
    let flat_depth = std_dev(&outputs_between(&flat, 0.6, 1.9));
    assert!(flat_depth < 0.03, "correzione robotica: il vibrato sparisce ({flat_depth})");

    let (_, restored) = run(&input, &CorrectionSettings { vibrato: 1.0, ..base });
    let restored_out = outputs_between(&restored, 0.6, 1.9);
    let restored_depth = std_dev(&restored_out);
    println!("vibrato ripristinato: {restored_depth:.3} (ingresso {input_depth:.3})");
    assert!(
        (restored_depth / input_depth - 1.0).abs() < 0.3,
        "vibrato +100%: profondita' {restored_depth} invece di {input_depth}"
    );
    assert!((mean(&restored_out) - 60.0).abs() < 0.05, "il centro resta sulla nota giusta");

    // Con correzione lenta il vibrato passa; con vibrato -100% sparisce quasi
    // del tutto, anche per vibrati piu' lenti o piu' veloci del solito.
    for rate in [4.5f32, 5.5, 7.0] {
        let input = voice(2.0, |t| 60.1 + 0.35 * (2.0 * PI * rate * t).sin());
        let slow = CorrectionSettings { retune_ms: 400.0, ..Default::default() };
        let (_, natural) = run(&input, &slow);
        let (_, removed) = run(&input, &CorrectionSettings { vibrato: -1.0, ..slow });
        let natural_depth = std_dev(&outputs_between(&natural, 0.8, 1.9));
        let removed_depth = std_dev(&outputs_between(&removed, 0.8, 1.9));
        println!("vibrato {rate} Hz: {natural_depth:.3} -> {removed_depth:.3}");
        assert!(natural_depth > 0.2, "{natural_depth}");
        assert!(removed_depth < natural_depth * 0.4, "{rate} Hz: {removed_depth} vs {natural_depth}");
    }
}

/// Baricentro dello spettro (Hz) tra 100 e 5000 Hz: sale se le formanti salgono.
fn spectral_centroid(signal: &[f32]) -> f32 {
    let n = 8192;
    let fft = Fft::new(n);
    let start = signal.len() / 2 - n / 2;
    let mut data: Vec<Complex> = (0..n)
        .map(|i| {
            let w = 0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos();
            Complex::new(signal[start + i] * w, 0.0)
        })
        .collect();
    fft.process(&mut data, false);
    let (mut weighted, mut total) = (0.0, 0.0);
    for (k, c) in data.iter().enumerate().take(n / 2) {
        let f = k as f32 * SR / n as f32;
        if (100.0..5000.0).contains(&f) {
            let p = c.re * c.re + c.im * c.im;
            weighted += f * p;
            total += p;
        }
    }
    weighted / total
}

#[test]
fn formant_shift_changes_timbre_not_pitch() {
    let input = voice(1.5, |_| 57.0);
    let plain = CorrectionSettings { amount: 0.0, ..Default::default() };
    let (out_plain, _) = run(&input, &plain);
    let (out_up, _) = run(&input, &CorrectionSettings { formant: 4.0, ..plain });
    let (out_down, _) = run(&input, &CorrectionSettings { formant: -4.0, ..plain });

    let c_plain = spectral_centroid(&out_plain);
    let up = spectral_centroid(&out_up) / c_plain;
    let down = spectral_centroid(&out_down) / c_plain;
    assert!(up > 1.12 && up < 1.45, "formanti +4: baricentro x{up}");
    assert!(down < 0.9 && down > 0.68, "formanti -4: baricentro x{down}");

    // La nota non cambia.
    for out in [&out_up, &out_down] {
        let mut yin = YinDetector::new(SR, 60.0);
        let len = yin.frame_len();
        let est = yin.detect(&out[out.len() / 2..out.len() / 2 + len]);
        assert!(est.voiced);
        assert!((hz_to_note(SR / est.period) - 57.0).abs() < 0.05);
    }
}

#[test]
fn tracking_recognises_breathy_voices() {
    let n = (SR * 1.5) as usize;
    let freqs = vec![note_to_hz(55.0); n];
    let breathy = sing_curve_detailed(&freqs, &vec![1.0; n], SR, 2.5, 3);
    let voiced_share = |tracking: f32| {
        let settings = CorrectionSettings { tracking, ..Default::default() };
        let (_, reports) = run(&breathy, &settings);
        reports.iter().filter(|(_, r)| r.voiced).count() as f32 / reports.len() as f32
    };
    let strict = voiced_share(0.0);
    let relaxed = voiced_share(1.0);
    println!("voce con molta aria: riconosciuta {:.0}% (severo) / {:.0}% (tollerante)", strict * 100.0, relaxed * 100.0);
    assert!(relaxed > strict + 0.3, "{strict} -> {relaxed}");
    assert!(relaxed > 0.7);
}

/// Dopo una micro-pausa (una consonante come la "t" di "te") la nuova nota
/// si aggancia subito: non si riparte dalla nota cantata prima della pausa.
#[test]
fn note_after_a_short_gap_starts_on_the_new_note() {
    use sosatune_dsp::synth::steady_vowel;
    use sosatune_dsp::{CorrectionSettings, PitchCorrector};
    let sr = 48000.0;
    let mut audio = steady_vowel(220.0, sr, (0.3 * sr) as usize); // La3 (57)
    audio.extend(std::iter::repeat(0.0).take((0.012 * sr) as usize)); // "t"
    audio.extend(steady_vowel(233.08, sr, (0.3 * sr) as usize)); // La#3 (58), un semitono sopra
    let mut corrector = PitchCorrector::new(sr, 1);
    let settings = CorrectionSettings::default();
    let mut targets = Vec::new();
    let mut gap_seen = false;
    for &x in &audio {
        let mut f = [x];
        if let Some(r) = corrector.process_frame(&mut f, &settings) {
            if !r.voiced {
                gap_seen = true;
            } else if gap_seen {
                targets.push(r.target_note);
            }
        }
    }
    assert!(gap_seen, "la pausa deve essere riconosciuta");
    let first_on_new_note = targets.iter().position(|&t| (t - 58.0).abs() < 0.01);
    assert_eq!(first_on_new_note, Some(0), "bersagli dopo la pausa: {:?}", &targets[..targets.len().min(6)]);
}

/// Voce rauca: un ciclo forte e uno debole, alternati (diplofonia, tipica
/// della voce "graffiata" a fine frase). Il doppio del periodo sembra allora
/// piu' periodico del periodo vero e il rilevatore tendeva a leggere la nota
/// un'ottava sotto: i grani prendevano due cicli alla volta e l'uscita
/// "gracchiava". La nota (iniziata pulita) deve restare quella cantata.
#[test]
fn rough_voice_is_not_read_an_octave_below() {
    let hz = note_to_hz(60.3);
    let clean = voice(1.2, |_| 60.3);
    let rough: Vec<f32> = clean
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            let cycle = (i as f32 / SR * hz).floor() as i64;
            if i as f32 / SR < 0.4 || cycle % 2 == 0 { x } else { x * 0.6 }
        })
        .collect();
    let settings = CorrectionSettings { retune_ms: 10.0, ..CorrectionSettings::default() };
    let (_, reports) = run(&rough, &settings);
    let read: Vec<f32> = reports
        .iter()
        .filter(|(t, r)| *t > 0.1 && r.voiced)
        .map(|(_, r)| r.input_note)
        .collect();
    assert!(read.len() > 150, "nota riconosciuta troppo poco: {}", read.len());
    let wrong = read.iter().filter(|&&n| (n - 60.3).abs() > 1.0).count();
    assert_eq!(wrong, 0, "{wrong} analisi su {} lette lontano dalla nota cantata", read.len());
}

#[test]
fn voice_between_two_scale_notes_does_not_warble() {
    // Sol minore: Do (60) e Re (62) sono nella scala, Do# no. Una voce che
    // ondeggia intorno al Do# (vibrato largo, voce incerta) non deve far saltare la correzione
    // avanti e indietro di un tono (voce che gracchia).
    use sosatune_dsp::scale::{scale_notes, ScaleKind};
    let settings = CorrectionSettings { notes: scale_notes(7, ScaleKind::NaturalMinor), ..CorrectionSettings::default() };
    let input = voice(2.0, |t| 61.0 + 0.3 * (2.0 * PI * 6.0 * t).sin());
    let (_, reports) = run(&input, &settings);
    let targets: Vec<f32> = reports.iter().filter(|(t, r)| *t > 0.1 && r.voiced).map(|(_, r)| r.target_note).collect();
    let changes = targets.windows(2).filter(|w| (w[0] - w[1]).abs() > 0.5).count();
    assert!(changes <= 1, "la correzione salta tra le note {changes} volte");

    // Un vero passaggio da Do a Re deve invece arrivare subito.
    let input = voice(1.0, |t| if t < 0.5 { 60.0 } else { 62.0 });
    let (_, reports) = run(&input, &settings);
    let arrived = reports.iter().find(|(t, r)| *t > 0.5 && r.voiced && (r.target_note - 62.0).abs() < 0.01).map(|(t, _)| *t);
    let arrived = arrived.expect("il Re deve essere raggiunto");
    assert!(arrived < 0.5 + 0.07, "il Re arriva dopo {:.0} ms", (arrived - 0.5) * 1000.0);
}
