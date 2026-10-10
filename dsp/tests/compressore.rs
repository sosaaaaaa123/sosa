//! Test del compressore ottico a valvola: curva, tempi, comportamento
//! "ottico", armoniche della valvola, assenza di aliasing, trasparenza e
//! stabilita'.

use sosatune_dsp::{CompMode, CompressorSettings, TubeCompressor};
use std::f32::consts::PI;

const SR: f32 = 48000.0;

fn sine(freq: f32, amp: f32, seconds: f32) -> Vec<f32> {
    let n = (SR * seconds) as usize;
    // Fase calcolata in doppia precisione: con la precisione semplice una
    // sinusoide lunga accumula rumore di fase che sembrerebbe un difetto.
    let w = 2.0 * std::f64::consts::PI * freq as f64 / SR as f64;
    (0..n).map(|i| amp * ((w * i as f64).sin() as f32)).collect()
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

/// Elabora (mono) e restituisce uscita e riduzione di guadagno campione per campione.
fn run(input: &[f32], settings: &CompressorSettings) -> (Vec<f32>, Vec<f32>) {
    let mut comp = TubeCompressor::new(SR, 1);
    comp.snap_to(settings);
    let mut out = Vec::with_capacity(input.len());
    let mut gr = Vec::with_capacity(input.len());
    for &x in input {
        let mut f = [x];
        comp.process_frame(&mut f, settings);
        out.push(f[0]);
        gr.push(comp.gain_reduction_db());
    }
    (out, gr)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len() as f64).sqrt() as f32
}

/// Ampiezza della componente a `freq` (Goertzel su un numero intero di periodi).
fn amplitude_at(x: &[f32], freq: f32) -> f32 {
    let w = 2.0 * std::f64::consts::PI * freq as f64 / SR as f64;
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, &v) in x.iter().enumerate() {
        re += v as f64 * (w * i as f64).cos();
        im -= v as f64 * (w * i as f64).sin();
    }
    (2.0 * (re * re + im * im).sqrt() / x.len() as f64) as f32
}

fn base() -> CompressorSettings {
    CompressorSettings { enabled: true, tube: 0.0, ..CompressorSettings::default() }
}

#[test]
fn off_is_an_exact_delay() {
    let input = sine(440.0, 0.7, 0.5);
    let settings = CompressorSettings { enabled: false, ..base() };
    let (out, _) = run(&input, &settings);
    let l = TubeCompressor::fixed_latency();
    for i in l..input.len() {
        assert_eq!(out[i], input[i - l]);
    }
}

#[test]
fn on_without_compression_is_transparent() {
    // Sotto la soglia e senza valvola: identico all'originale (ritardato).
    let settings = CompressorSettings { threshold_db: 0.0, ..base() };
    let l = TubeCompressor::fixed_latency();
    for &freq in &[100.0f32, 1000.0, 10000.0, 18000.0] {
        let input = sine(freq, 0.25, 0.5);
        let (out, _) = run(&input, &settings);
        let mut err = 0.0f32;
        for i in 8000..input.len() {
            err = err.max((out[i] - input[i - l]).abs());
        }
        // Meno di -90 dB rispetto al segnale.
        assert!(db(err / 0.25) < -90.0, "{freq} Hz: errore {} dB", db(err / 0.25));
    }
}

#[test]
fn static_curve_matches_threshold_and_ratio() {
    for &(threshold, ratio) in &[(-20.0f32, 2.0f32), (-30.0, 4.0), (-24.0, 10.0)] {
        for &level in &[-36.0f32, -12.0, -6.0] {
            let amp = 10f32.powf(level / 20.0);
            let input = sine(1000.0, amp, 1.0);
            let settings = CompressorSettings {
                threshold_db: threshold,
                ratio,
                mode: CompMode::Manual,
                attack_ms: 1.0,
                release_ms: 800.0,
                ..base()
            };
            let (out, _) = run(&input, &settings);
            let tail = 30000..48000;
            let measured = db(rms(&input[tail.clone()])) - db(rms(&out[tail]));
            let expected = TubeCompressor::static_gain_reduction(level, threshold, ratio);
            assert!(
                (measured - expected).abs() < 0.8,
                "soglia {threshold} rapporto {ratio} livello {level}: {measured:.2} dB invece di {expected:.2}"
            );
        }
    }
}

/// Tempo (ms) dal campione `from` finche' `gr` attraversa `level`.
fn time_to_cross(gr: &[f32], from: usize, level: f32, rising: bool) -> f32 {
    let i = (from..gr.len())
        .find(|&i| if rising { gr[i] >= level } else { gr[i] <= level })
        .expect("la soglia non viene mai raggiunta");
    (i - from) as f32 / SR * 1000.0
}

#[test]
fn manual_attack_and_release_times() {
    for &(attack, release) in &[(5.0f32, 100.0f32), (30.0, 600.0)] {
        let mut input = sine(1000.0, 0.01, 0.5);
        input.extend(sine(1000.0, 0.5, 2.0));
        input.extend(sine(1000.0, 0.01, 3.0));
        let settings = CompressorSettings {
            threshold_db: -30.0,
            ratio: 10.0,
            mode: CompMode::Manual,
            attack_ms: attack,
            release_ms: release,
            ..base()
        };
        let (_, gr) = run(&input, &settings);
        let step_up = (0.5 * SR) as usize;
        let step_down = (2.5 * SR) as usize;
        let full = gr[step_down - 1];
        let t_attack = time_to_cross(&gr, step_up, full * (1.0 - (-1.0f32).exp()), true);
        let t_release = time_to_cross(&gr, step_down, full * (-1.0f32).exp(), false);
        assert!((t_attack / attack - 1.0).abs() < 0.5, "attacco {t_attack:.1} ms invece di {attack}");
        assert!((t_release / release - 1.0).abs() < 0.2, "rilascio {t_release:.1} ms invece di {release}");
    }
}

#[test]
fn optical_release_depends_on_the_program() {
    // Stessa compressione, ma per 0,15 s oppure per 4 s: dopo la frase lunga
    // la cella ottica deve rilasciare piu' lentamente.
    let settings = CompressorSettings {
        threshold_db: -26.0,
        ratio: 4.0,
        mode: CompMode::Fixed,
        ..base()
    };
    let recovery = |burst: f32| {
        let mut input = sine(1000.0, 0.5, burst);
        let end = input.len();
        input.extend(sine(1000.0, 0.001, 8.0));
        let (_, gr) = run(&input, &settings);
        let start_gr = gr[end - 1];
        let fast_phase = gr[end + (0.12 * SR) as usize] / start_gr;
        let to_1db = time_to_cross(&gr, end, 1.0, false);
        (start_gr, fast_phase, to_1db)
    };
    let (gr_short, fast_short, short) = recovery(0.15);
    let (gr_long, fast_long, long) = recovery(4.0);
    assert!(gr_short > 8.0 && gr_long > 12.0, "compressione {gr_short:.1} / {gr_long:.1} dB");
    // Prima fase veloce: in 120 ms se ne va almeno un terzo della compressione.
    assert!(fast_short < 0.67 && fast_long < 0.9, "fase veloce {fast_short:.2} / {fast_long:.2}");
    // Seconda fase: dopo la frase lunga il recupero e' molto piu' lento.
    assert!(long > 1.8 * short, "recupero {short:.0} ms dopo frase breve, {long:.0} ms dopo frase lunga");
    assert!(short < 1500.0 && long < 8000.0, "recupero {short:.0} / {long:.0} ms");
}

#[test]
fn optical_attack_is_faster_on_strong_peaks() {
    let settings = CompressorSettings { threshold_db: -30.0, ratio: 4.0, mode: CompMode::Fixed, ..base() };
    let attack_time = |amp: f32| {
        let mut input = sine(1000.0, 0.003, 0.3);
        let start = input.len();
        input.extend(sine(1000.0, amp, 0.5));
        let (_, gr) = run(&input, &settings);
        let peak = gr[start..].iter().cloned().fold(0.0f32, f32::max);
        time_to_cross(&gr, start, 0.5 * peak, true)
    };
    let soft = attack_time(0.06);
    let hard = attack_time(0.9);
    assert!(hard < soft, "picco forte {hard:.1} ms, picco debole {soft:.1} ms");
    assert!(hard < 10.0, "attacco sui picchi forti {hard:.1} ms");
}

#[test]
fn tube_adds_warm_even_harmonics() {
    let threshold_off = CompressorSettings { threshold_db: 0.0, ..base() };
    let input = sine(1000.0, 0.5, 2.0);
    let segment = 48000..96000;

    let (clean, _) = run(&input, &CompressorSettings { tube: 0.0, ..threshold_off });
    let f0 = amplitude_at(&clean[segment.clone()], 1000.0);
    for h in 2..=5 {
        let a = amplitude_at(&clean[segment.clone()], 1000.0 * h as f32);
        assert!(db(a / f0) < -100.0, "senza valvola armonica {h}: {} dB", db(a / f0));
    }

    let mut previous_h2 = -200.0;
    // (valvola, seconda armonica minima): discreta al 30%, evidente al massimo.
    for &(tube, min_h2) in &[(0.3f32, -55.0f32), (0.6, -35.0), (1.0, -20.0)] {
        let (warm, _) = run(&input, &CompressorSettings { tube, ..threshold_off });
        let f0 = amplitude_at(&warm[segment.clone()], 1000.0);
        let h2 = db(amplitude_at(&warm[segment.clone()], 2000.0) / f0);
        let h3 = db(amplitude_at(&warm[segment.clone()], 3000.0) / f0);
        assert!(h2 > previous_h2, "la seconda armonica deve crescere con la valvola");
        assert!(h2 > min_h2, "valvola {tube}: seconda armonica {h2:.1} dB");
        assert!(h2 > h3 - 3.0, "valvola {tube}: H2 {h2:.1} dB, H3 {h3:.1} dB");
        // Il volume resta simile (la curva ha pendenza 1).
        assert!((db(f0 / 0.5)).abs() < 2.5, "valvola {tube}: volume {} dB", db(f0 / 0.5));
        previous_h2 = h2;
    }
}

#[test]
fn no_audible_aliasing_even_at_full_saturation() {
    // Una nota acuta (7 kHz) con la valvola al massimo: le armoniche oltre i
    // 24 kHz non devono ripiegarsi nella banda udibile.
    let settings = CompressorSettings { threshold_db: 0.0, tube: 1.0, ..base() };
    let input = sine(7000.0, 0.7, 2.0);
    let (out, _) = run(&input, &settings);
    let seg = &out[48000..96000];
    let f0 = amplitude_at(seg, 7000.0);
    let mut worst = -300.0f32;
    for h in 4..=13u32 {
        let f = (h * 7000) % 48000;
        let folded = if f > 24000 { 48000 - f } else { f };
        if folded % 7000 == 0 || folded > 20000 || folded < 20 {
            continue;
        }
        worst = worst.max(db(amplitude_at(seg, folded as f32) / f0));
    }
    assert!(worst < -90.0, "aliasing a {worst:.1} dB");
}

#[test]
fn parallel_mix_and_output_gain() {
    let l = TubeCompressor::fixed_latency();
    let input = sine(1000.0, 0.5, 1.0);
    let dry = CompressorSettings { threshold_db: -40.0, ratio: 10.0, mix: 0.0, ..base() };
    let (out, _) = run(&input, &dry);
    for i in l..input.len() {
        assert_eq!(out[i], input[i - l]);
    }
    let louder = CompressorSettings { threshold_db: 0.0, output_db: 6.0, ..base() };
    let (out, _) = run(&input, &louder);
    let gain = db(rms(&out[24000..48000]) / rms(&input[24000..48000]));
    assert!((gain - 6.0).abs() < 0.05, "uscita {gain:.2} dB");
}

#[test]
fn switching_on_and_off_has_no_clicks() {
    let input = sine(200.0, 0.5, 1.0);
    let mut comp = TubeCompressor::new(SR, 1);
    let mut on = CompressorSettings { threshold_db: -30.0, ratio: 8.0, tube: 1.0, ..base() };
    on.enabled = false;
    comp.snap_to(&on);
    let mut out = Vec::new();
    for (i, &x) in input.iter().enumerate() {
        on.enabled = (i / 12000) % 2 == 1;
        let mut f = [x];
        comp.process_frame(&mut f, &on);
        out.push(f[0]);
    }
    // Il salto massimo tra due campioni resta quello di una sinusoide a 200 Hz.
    let max_step = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    let sine_step = 0.5 * 2.0 * PI * 200.0 / SR;
    assert!(max_step < 1.3 * sine_step, "salto {max_step} (sinusoide {sine_step})");
}

#[test]
fn stays_stable_with_extreme_input_and_settings() {
    let mut rng = 0x1234_5678u32;
    let mut next = || {
        rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
        rng as f32 / u32::MAX as f32
    };
    for &sr in &[22050.0f32, 44100.0, 96000.0, 192000.0] {
        let mut comp = TubeCompressor::new(sr, 2);
        for block in 0..200 {
            let settings = CompressorSettings {
                enabled: next() > 0.2,
                threshold_db: -60.0 + 60.0 * next(),
                ratio: 1.0 + 19.0 * next(),
                attack_ms: 0.1 + 300.0 * next(),
                release_ms: 10.0 + 10000.0 * next(),
                mode: [CompMode::Fixed, CompMode::Manual, CompMode::FixedManual][block % 3],
                tube: next(),
                mix: next(),
                output_db: -20.0 + 40.0 * next(),
            };
            for i in 0..256 {
                let x = match block % 4 {
                    0 => 0.0,
                    1 => (next() * 2.0 - 1.0) * 100.0,
                    2 => 1e-30 * (i as f32).sin(),
                    _ => next() * 2.0 - 1.0,
                };
                let mut f = [x, -x];
                comp.process_frame(&mut f, &settings);
                assert!(f[0].is_finite() && f[1].is_finite(), "valore non valido a {sr} Hz");
                assert!(f[0].abs() < 1e4);
            }
        }
    }
}

#[test]
fn stereo_link_uses_the_same_gain_on_both_sides() {
    let settings = CompressorSettings { threshold_db: -30.0, ratio: 4.0, ..base() };
    let mut comp = TubeCompressor::new(SR, 2);
    comp.snap_to(&settings);
    let left = sine(1000.0, 0.5, 1.0);
    let mut out_l = Vec::new();
    let mut out_r = Vec::new();
    for &x in &left {
        // Destra molto piu' bassa: deve essere compressa quanto la sinistra.
        let mut f = [x, 0.01 * x];
        comp.process_frame(&mut f, &settings);
        out_l.push(f[0]);
        out_r.push(f[1]);
    }
    let ratio_out = rms(&out_r[24000..]) / rms(&out_l[24000..]);
    assert!((db(ratio_out) - db(0.01)).abs() < 0.1);
}

/// Tutte e tre le modalita' arrivano alla stessa compressione su una nota
/// tenuta (la curva statica), e l'indicatore dice il vero.
#[test]
fn every_mode_reaches_the_curve_and_the_meter_is_honest() {
    let level = -6.0f32;
    let input = sine(1000.0, 10f32.powf(level / 20.0), 2.0);
    for mode in [CompMode::Fixed, CompMode::Manual, CompMode::FixedManual] {
        let settings = CompressorSettings {
            threshold_db: -24.0,
            ratio: 4.0,
            mode,
            attack_ms: 5.0,
            release_ms: 500.0,
            ..base()
        };
        let (out, gr) = run(&input, &settings);
        let tail = 72000..96000;
        let measured = db(rms(&input[tail.clone()])) - db(rms(&out[tail.clone()]));
        let expected = TubeCompressor::static_gain_reduction(level, -24.0, 4.0);
        assert!((measured - expected).abs() < 1.0, "{mode:?}: {measured:.2} dB invece di {expected:.2}");
        let meter = gr[tail].iter().sum::<f32>() / 24000.0;
        assert!((meter - measured).abs() < 0.5, "{mode:?}: indicatore {meter:.2} dB, reale {measured:.2} dB");
    }
}

/// "Ottica + manuale" comprime almeno quanto ciascuna delle due da sola:
/// attacco del piu' veloce, rilascio del piu' lento.
#[test]
fn fixed_manual_combines_both_cells() {
    // Una frase forte di 0,6 s, poi silenzio.
    let mut input = sine(500.0, 0.7, 0.6);
    input.extend(std::iter::repeat(0.0).take((SR * 2.0) as usize));
    let make = |mode| CompressorSettings {
        threshold_db: -30.0,
        ratio: 6.0,
        mode,
        attack_ms: 40.0,
        release_ms: 120.0,
        ..base()
    };
    let (_, fixed) = run(&input, &make(CompMode::Fixed));
    let (_, manual) = run(&input, &make(CompMode::Manual));
    let (_, both) = run(&input, &make(CompMode::FixedManual));
    for i in (0..input.len()).step_by(240) {
        let floor = fixed[i].max(manual[i]);
        assert!(both[i] >= floor - 0.05, "campione {i}: {:.2} dB < {:.2} dB", both[i], floor);
    }
    // E durante la frase comprime davvero.
    let during = (0.5 * SR) as usize;
    assert!(both[during] > 10.0, "compressione {:.1} dB", both[during]);
}

/// Funziona a tutte le frequenze di campionamento dei DAW, mono e stereo,
/// con la stessa curva.
#[test]
fn works_at_every_sample_rate_and_channel_count() {
    for &sr in &[44100.0f32, 48000.0, 88200.0, 96000.0, 192000.0] {
        for channels in [1usize, 2] {
            let mut comp = TubeCompressor::new(sr, channels);
            let settings = CompressorSettings {
                threshold_db: -20.0,
                ratio: 4.0,
                mode: CompMode::Manual,
                attack_ms: 2.0,
                release_ms: 300.0,
                ..base()
            };
            comp.snap_to(&settings);
            let n = (sr * 1.0) as usize;
            let w = 2.0 * std::f64::consts::PI * 1000.0 / sr as f64;
            let (mut sum_in, mut sum_out) = (0.0f64, 0.0f64);
            for i in 0..n {
                let x = 0.5 * (w * i as f64).sin() as f32;
                let mut frame = [x, x];
                comp.process_frame(&mut frame[..channels], &settings);
                if i > n / 2 {
                    sum_in += (x as f64).powi(2);
                    sum_out += (frame[0] as f64).powi(2);
                    if channels == 2 {
                        assert!((frame[0] - frame[1]).abs() < 1e-6, "{sr} Hz: canali diversi");
                    }
                }
            }
            let measured = 10.0 * (sum_in / sum_out).log10() as f32;
            let expected = TubeCompressor::static_gain_reduction(db(0.5), -20.0, 4.0);
            assert!((measured - expected).abs() < 1.0, "{sr} Hz, {channels} canali: {measured:.2} invece di {expected:.2}");
        }
    }
}

/// Su una voce bassa (90 Hz) il guadagno resta fermo durante la nota: niente
/// "tremolio" a ogni ciclo, quindi niente distorsione aggiunta dal compressore.
#[test]
fn gain_does_not_ripple_on_low_voices() {
    let input = sine(90.0, 0.5, 1.5);
    for mode in [CompMode::Fixed, CompMode::Manual, CompMode::FixedManual] {
        let settings = CompressorSettings {
            threshold_db: -30.0,
            ratio: 6.0,
            mode,
            attack_ms: 0.5,
            release_ms: 50.0,
            ..base()
        };
        let (out, gr) = run(&input, &settings);
        let tail = &gr[48000..72000];
        let max = tail.iter().cloned().fold(f32::MIN, f32::max);
        let min = tail.iter().cloned().fold(f32::MAX, f32::min);
        assert!(max - min < 0.1, "{mode:?}: il guadagno oscilla di {:.2} dB", max - min);
        // Distorsione: la terza armonica (dovuta al tremolio) resta trascurabile.
        let seg = &out[48000..64000]; // esattamente 30 periodi
        let f0 = amplitude_at(seg, 90.0);
        let h2 = amplitude_at(seg, 180.0);
        let h3 = amplitude_at(seg, 270.0);
        assert!(db(h2.max(h3) / f0) < -60.0, "{mode:?}: armoniche a {:.1} dB", db(h2.max(h3) / f0));
    }
}
