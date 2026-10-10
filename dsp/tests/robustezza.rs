//! Test di robustezza: il motore non deve mai bloccarsi, produrre valori
//! non validi (NaN/infinito) o picchi di volume, qualunque cosa riceva.

use intona_dsp::scale::{note_to_hz, scale_notes, ScaleKind, VoiceType};
use intona_dsp::synth::{noise, sing_melody, SungNote};
use intona_dsp::{CorrectionSettings, PitchCorrector};

fn mixed_signal(sr: f32) -> Vec<f32> {
    let melody = [
        SungNote { note: 57.0, detune_cents: 20.0, seconds: 0.4 },
        SungNote { note: 64.0, detune_cents: -30.0, seconds: 0.3 },
        SungNote { note: -1.0, detune_cents: 0.0, seconds: 0.1 },
        SungNote { note: 45.0, detune_cents: 40.0, seconds: 0.4 },
        SungNote { note: 76.0, detune_cents: -45.0, seconds: 0.3 },
    ];
    let (mut signal, _) = sing_melody(&melody, sr, 30.0);
    // Aggiungiamo rumore ("consonanti"), un impulso secco e un tratto di silenzio.
    let burst = noise((sr * 0.1) as usize, 0.4, 7);
    signal.extend(burst);
    signal.push(1.0);
    signal.extend(std::iter::repeat_n(0.0, (sr * 0.2) as usize));
    let square: Vec<f32> = (0..(sr * 0.3) as usize)
        .map(|i| if (i as f32 * 220.0 / sr).fract() < 0.5 { 0.9 } else { -0.9 })
        .collect();
    signal.extend(square);
    signal
}

#[test]
fn every_sample_rate_and_voice_type_is_stable() {
    for &sr in &[22050.0f32, 44100.0, 48000.0, 88200.0, 96000.0, 192000.0] {
        let input = mixed_signal(sr);
        for &voice in &[VoiceType::Soprano, VoiceType::AltoTenor, VoiceType::LowMale] {
            for &channels in &[1usize, 2] {
                let mut corrector = PitchCorrector::new(sr, channels);
                corrector.set_voice(voice);
                let settings = CorrectionSettings {
                    notes: scale_notes(2, ScaleKind::MinorPentatonic),
                    retune_ms: 0.0,
                    amount: 1.0,
                    transpose: 7.0,
                    humanize: 0.5,
                    flex_tune: 0.3,
                    vibrato: 0.8,
                    formant: -3.0,
                    tracking: 0.8,
                };
                let mut peak = 0.0f32;
                for &x in &input {
                    let mut frame = [x, -x];
                    corrector.process_frame(&mut frame[..channels], &settings);
                    for y in &frame[..channels] {
                        assert!(y.is_finite(), "valore non valido a {sr} Hz");
                        peak = peak.max(y.abs());
                    }
                }
                assert!(peak < 3.0, "picco eccessivo {peak} a {sr} Hz, {voice:?}");
            }
        }
    }
}

#[test]
fn changing_settings_while_running_is_safe() {
    let sr = 48000.0;
    let input = mixed_signal(sr);
    let mut corrector = PitchCorrector::new(sr, 2);
    for (i, &x) in input.iter().enumerate() {
        // Cambia scala, velocita' e trasposizione ogni 10 ms, come un'automazione.
        let k = i / 480;
        let settings = CorrectionSettings {
            notes: scale_notes((k % 12) as u8, if k % 2 == 0 { ScaleKind::Major } else { ScaleKind::Blues }),
            retune_ms: (k % 5) as f32 * 50.0,
            amount: (k % 3) as f32 / 2.0,
            transpose: (k % 25) as f32 - 12.0,
            humanize: (k % 4) as f32 / 3.0,
            flex_tune: (k % 6) as f32 / 5.0,
            vibrato: (k % 7) as f32 / 3.0 - 1.0,
            formant: (k % 13) as f32 - 6.0,
            tracking: (k % 5) as f32 / 4.0,
        };
        if i % 20000 == 0 {
            corrector.set_voice(match k % 3 {
                0 => VoiceType::Soprano,
                1 => VoiceType::AltoTenor,
                _ => VoiceType::LowMale,
            });
        }
        let mut frame = [x, x];
        corrector.process_frame(&mut frame, &settings);
        assert!(frame.iter().all(|y| y.is_finite()));
    }
}

#[test]
fn no_scale_notes_means_no_correction() {
    let sr = 44100.0;
    let input = intona_dsp::synth::steady_vowel(note_to_hz(61.4), sr, 22050);
    let settings = CorrectionSettings { notes: [false; 12], retune_ms: 0.0, ..Default::default() };
    let mut corrector = PitchCorrector::new(sr, 1);
    let mut last = None;
    for &x in &input {
        let mut f = [x];
        if let Some(r) = corrector.process_frame(&mut f, &settings) {
            if r.voiced {
                last = Some(r);
            }
        }
    }
    let r = last.expect("nota non rilevata");
    assert!((r.output_note - r.input_note).abs() < 1e-4);
    assert!(r.target_note.is_nan());
}
