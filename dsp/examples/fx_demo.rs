//! Demo degli effetti di SR Vocal Suite su una voce sintetica:
//!
//! ```bash
//! cargo run --release -p sosatune_dsp --example fx_demo
//! ```
//!
//! Crea `fx_1_dry.wav`, `fx_2_chorus.wav`, `fx_3_echo.wav`, `fx_4_reverb.wav`
//! e `fx_5_tutto.wav` (correzione + chorus + delay + reverb), e stampa i
//! livelli.

use sosatune_dsp::fx::{send_gain, Chorus, ChorusSettings, Reverb, ReverbSettings, SpaceEcho, SpaceEchoSettings};
use sosatune_dsp::synth::{sing_melody, SungNote};
use sosatune_dsp::wav;

fn rms_db(x: &[f32]) -> f32 {
    let e: f32 = x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32;
    10.0 * e.max(1e-12).log10()
}

fn main() {
    let sr = 48000.0;
    let melody = [
        SungNote { note: 57.0, detune_cents: 25.0, seconds: 0.45 },
        SungNote { note: 59.0, detune_cents: -20.0, seconds: 0.3 },
        SungNote { note: 61.0, detune_cents: 15.0, seconds: 0.6 },
        SungNote { note: -1.0, detune_cents: 0.0, seconds: 0.3 },
        SungNote { note: 64.0, detune_cents: -30.0, seconds: 0.5 },
        SungNote { note: 62.0, detune_cents: 10.0, seconds: 0.8 },
    ];
    let (mut voice, _) = sing_melody(&melody, sr, 25.0);
    voice.extend(std::iter::repeat_n(0.0, (sr * 3.0) as usize));
    let render = |chorus_k: f32, echo_k: f32, reverb_k: f32| {
        let mut ch = Chorus::new(sr);
        let mut echo = SpaceEcho::new(sr);
        let mut rv = Reverb::new(sr);
        let cs = ChorusSettings { mix: chorus_k / 100.0, ..Default::default() };
        let es = SpaceEchoSettings::default();
        let rs = ReverbSettings::default();
        let (mut l, mut r) = (Vec::new(), Vec::new());
        for &x in &voice {
            let (cl, cr) = ch.process(x, x, &cs);
            let mono = 0.5 * (cl + cr);
            let (el, er) = echo.process(mono * send_gain(echo_k), &es);
            let (rl, rr) = rv.process((mono + 0.5 * (el + er)) * send_gain(reverb_k), &rs);
            l.push(cl + el + rl);
            r.push(cr + er + rr);
        }
        (l, r)
    };
    let outs = [
        ("fx_1_dry.wav", render(0.0, 0.0, 0.0)),
        ("fx_2_chorus.wav", render(35.0, 0.0, 0.0)),
        ("fx_3_echo.wav", render(0.0, 60.0, 0.0)),
        ("fx_4_reverb.wav", render(0.0, 0.0, 60.0)),
        ("fx_5_tutto.wav", render(35.0, 27.0, 48.0)),
    ];
    let dry_db = rms_db(&voice);
    for (name, (l, r)) in outs {
        let peak = l.iter().chain(&r).fold(0.0f32, |m, v| m.max(v.abs()));
        println!("{name}: RMS {:+.1} dB rispetto al dry, picco {:.2}", rms_db(&l) - dry_db, peak);
        wav::write(name, sr as u32, &[l, r]).expect("scrittura WAV");
    }
}
