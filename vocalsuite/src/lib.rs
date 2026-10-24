//! # SR Vocal Suite — il plugin
//!
//! Correzione dell'intonazione con il motore di SosaTune, poi gli effetti
//! della suite:
//!
//! ```text
//! voce ─► SosaTune (YIN + phase vocoder / Low Latency, formanti) ─► Chorus (Juno + FL)
//!                                                                     │
//!          ┌──────────────────────────────────────────────────────────┤
//!          │                     ├─► Delay (tape echo RE-201, mandata) ─┐
//!          │                     └─► Reverb (mandata, + meta' dell'eco) ┤
//!          └──────────────────────────────────────────────────────────►+─► Output
//! ```

use nih_plug::prelude::*;
use sosatune_dsp::chroma::ChromaAnalyzer;
use sosatune_dsp::fx::{send_gain, Chorus, Reverb, SpaceEcho};
use sosatune_dsp::{CorrectionSettings, PitchCorrector, MAX_CHANNELS};
use std::sync::Arc;

mod defaults;
mod denormals;
mod editor;
mod gui;
mod layout;
mod params;
mod presets;
#[doc(hidden)]
pub mod preview;
mod shared;

use params::{QualityChoice, VocalSuiteParams, VoiceChoice};
use shared::PitchDisplay;

pub struct VocalSuite {
    params: Arc<VocalSuiteParams>,
    corrector: Option<PitchCorrector>,
    chorus: Option<Chorus>,
    echo: Option<SpaceEcho>,
    reverb: Option<Reverb>,
    display: Arc<PitchDisplay>,
    current_voice: VoiceChoice,
    current_quality: QualityChoice,
    sample_rate: f32,
    input_channels: usize,
    chroma_main: Option<ChromaAnalyzer>,
    chroma_side: Option<ChromaAnalyzer>,
    reported_latency: u32,
    bypass_mix: f32,
    /// Mandate (lineari) del delay e del riverbero, che scorrono dolcemente.
    echo_send: f32,
    reverb_send: f32,
    dry_delay: Vec<f32>,
    dry_capacity: usize,
    dry_pos: usize,
}

impl Default for VocalSuite {
    fn default() -> Self {
        Self {
            params: Arc::new(VocalSuiteParams::default()),
            corrector: None,
            chorus: None,
            echo: None,
            reverb: None,
            display: Arc::new(PitchDisplay::default()),
            current_voice: VoiceChoice::AltoTenor,
            current_quality: QualityChoice::Hq,
            sample_rate: 48000.0,
            input_channels: 2,
            chroma_main: None,
            chroma_side: None,
            reported_latency: 0,
            bypass_mix: 0.0,
            echo_send: 0.0,
            reverb_send: 0.0,
            dry_delay: Vec::new(),
            dry_capacity: 0,
            dry_pos: 0,
        }
    }
}

const SIDECHAIN_NAMES: PortNames = PortNames {
    layout: None,
    main_input: None,
    main_output: None,
    aux_inputs: &["Sidechain (base per AutoKey)"],
    aux_outputs: &[],
};

impl Plugin for VocalSuite {
    const NAME: &'static str = "SR Vocal Suite";
    const VENDOR: &'static str = "Vladislav";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            aux_input_ports: &[new_nonzero_u32(2)],
            names: SIDECHAIN_NAMES,
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(2),
            aux_input_ports: &[new_nonzero_u32(2)],
            names: SIDECHAIN_NAMES,
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            aux_input_ports: &[new_nonzero_u32(2)],
            names: SIDECHAIN_NAMES,
            ..AudioIOLayout::const_default()
        },
    ];

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone(), self.display.clone())
    }

    fn initialize(
        &mut self,
        audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        context: &mut impl InitContext<Self>,
    ) -> bool {
        let channels = audio_io_layout
            .main_input_channels
            .map(|c| c.get() as usize)
            .unwrap_or(1)
            .min(MAX_CHANNELS);
        self.sample_rate = buffer_config.sample_rate;
        self.input_channels = channels;
        let mut corrector = PitchCorrector::new(self.sample_rate, channels);
        self.current_voice = self.params.voice.value();
        self.current_quality = self.params.quality.value();
        corrector.set_quality(self.current_quality == QualityChoice::Hq);
        corrector.set_voice(self.current_voice.to_dsp());
        self.chroma_main = Some(ChromaAnalyzer::new(self.sample_rate));
        self.chroma_side = Some(ChromaAnalyzer::new(self.sample_rate));
        self.chorus = Some(Chorus::new(self.sample_rate));
        self.echo = Some(SpaceEcho::new(self.sample_rate));
        self.reverb = Some(Reverb::new(self.sample_rate));

        self.bypass_mix = if self.params.bypass.value() { 1.0 } else { 0.0 };
        self.reported_latency = corrector.latency() as u32;
        self.dry_capacity = ((self.sample_rate * 0.25).ceil() as usize + 1).max(self.reported_latency as usize + 1);
        self.dry_delay = vec![0.0; self.dry_capacity * MAX_CHANNELS];
        self.dry_pos = 0;
        context.set_latency_samples(self.reported_latency);
        let latency_ms = self.reported_latency as f32 / self.sample_rate * 1000.0;
        self.display.set_timing(latency_ms, corrector.analysis_interval());
        self.corrector = Some(corrector);
        true
    }

    fn reset(&mut self) {
        if let Some(c) = &mut self.corrector {
            c.reset();
        }
        if let Some(c) = &mut self.chorus {
            c.reset();
        }
        if let Some(e) = &mut self.echo {
            e.reset();
        }
        if let Some(r) = &mut self.reverb {
            r.reset();
        }
        self.bypass_mix = if self.params.bypass.value() { 1.0 } else { 0.0 };
        self.dry_delay.fill(0.0);
        self.dry_pos = 0;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let (Some(corrector), Some(chorus), Some(echo), Some(reverb)) =
            (self.corrector.as_mut(), self.chorus.as_mut(), self.echo.as_mut(), self.reverb.as_mut())
        else {
            return ProcessStatus::Normal;
        };

        let voice = self.params.voice.value();
        let quality = self.params.quality.value();
        let bypass = self.params.bypass.value();
        if voice != self.current_voice || quality != self.current_quality {
            self.current_voice = voice;
            self.current_quality = quality;
            corrector.set_quality(quality == QualityChoice::Hq);
            corrector.set_voice(voice.to_dsp());
        }
        let latency = corrector.latency() as u32;
        if latency != self.reported_latency {
            self.reported_latency = latency;
            nih_plug::util::permit_alloc(|| context.set_latency_samples(latency));
            let latency_ms = latency as f32 / self.sample_rate * 1000.0;
            self.display.set_timing(latency_ms, corrector.analysis_interval());
        }

        let _ftz = denormals::FlushToZero::new();

        let settings = self.params.correction_settings();
        let settings = if bypass {
            CorrectionSettings { amount: 0.0, transpose: 0.0, vibrato: 0.0, formant: 0.0, ..settings }
        } else {
            settings
        };
        let chorus_settings = self.params.chorus_settings();
        let echo_settings = self.params.echo_settings(context.transport().tempo);
        let reverb_settings = self.params.reverb_settings();
        let echo_target = if bypass { 0.0 } else { send_gain(self.params.delay.value()) };
        let reverb_target = if bypass { 0.0 } else { send_gain(self.params.reverb.value()) };
        let send_coef = 1.0 - (-1.0 / (0.02 * self.sample_rate)).exp();

        let mut input_peak = 0.0f32;
        let mut output_peak = 0.0f32;
        let analysis_interval = corrector.analysis_interval();
        let gui_open = self.params.editor_state.is_open();

        let channels = buffer.as_slice();
        let num_outputs = channels.len();
        let num_inputs = self.input_channels.min(num_outputs).min(MAX_CHANNELS);
        let num_samples = channels.first().map_or(0, |c| c.len());
        if num_inputs == 0 {
            return ProcessStatus::Normal;
        }

        let listening = self.params.autokey.load(std::sync::atomic::Ordering::Relaxed);
        if self.display.take_chroma_reset() {
            for analyzer in [&mut self.chroma_main, &mut self.chroma_side].into_iter().flatten() {
                analyzer.reset();
            }
        }
        let side = aux.inputs.first_mut().map(|b| b.as_slice_immutable());
        let side_channels = side.as_ref().map_or(0, |s| s.len());
        let side = side.filter(|s| {
            side_channels > 0
                && listening
                && !(0..num_samples).all(|i| {
                    let main: f32 = (0..num_inputs).map(|ch| channels[ch][i]).sum::<f32>() / num_inputs as f32;
                    let x: f32 = s.iter().map(|c| c[i]).sum::<f32>() / side_channels as f32;
                    (main - x).abs() < 1e-6
                })
        });
        for analyzer in [&mut self.chroma_main, &mut self.chroma_side].into_iter().flatten() {
            analyzer.set_reference(settings.reference_hz);
        }

        let fade_step = 1.0 / (0.01 * self.sample_rate);
        let bypass_target = if bypass { 1.0 } else { 0.0 };
        for i in 0..num_samples {
            let mut frame = [0.0f32; MAX_CHANNELS];
            for (ch, sample) in frame.iter_mut().enumerate().take(num_inputs) {
                *sample = channels[ch][i];
            }
            let mut dry = [0.0f32; MAX_CHANNELS];
            let cap = self.dry_capacity;
            if cap > 0 {
                let delay = (self.reported_latency as usize).min(cap - 1);
                let read = (self.dry_pos + cap - delay) % cap;
                for ch in 0..num_inputs {
                    self.dry_delay[ch * cap + self.dry_pos] = frame[ch];
                    dry[ch] = self.dry_delay[ch * cap + read];
                }
                self.dry_pos = (self.dry_pos + 1) % cap;
            } else {
                dry = frame;
            }

            let mut mono = 0.0;
            for &x in &frame[..num_inputs] {
                input_peak = input_peak.max(x.abs());
                mono += x;
            }
            if listening {
                if let Some(a) = &mut self.chroma_main {
                    a.process(mono / num_inputs as f32);
                }
                if let (Some(a), Some(side)) = (&mut self.chroma_side, &side) {
                    let x: f32 = side.iter().map(|c| c[i]).sum::<f32>() / side_channels as f32;
                    a.process(x);
                }
            }
            let report = corrector.process_frame(&mut frame[..num_inputs], &settings);

            // --- Effetti ---------------------------------------------------
            let (l, r) = if num_inputs == 2 { (frame[0], frame[1]) } else { (frame[0], frame[0]) };
            let (cl, cr) = chorus.process(l, r, &chorus_settings);
            let voice_mono = 0.5 * (cl + cr);
            self.echo_send += send_coef * (echo_target - self.echo_send);
            self.reverb_send += send_coef * (reverb_target - self.reverb_send);
            let (el, er) = if self.echo_send > 1e-5 || !echo.is_idle() {
                echo.process(voice_mono * self.echo_send, &echo_settings)
            } else {
                (0.0, 0.0)
            };
            let (rl, rr) = if self.reverb_send > 1e-5 || !reverb.is_idle() {
                reverb.process((voice_mono + 0.5 * (el + er)) * self.reverb_send, &reverb_settings)
            } else {
                (0.0, 0.0)
            };
            let gain = nih_plug::util::db_to_gain_fast(self.params.output_gain.smoothed.next());
            let wet = [(cl + el + rl) * gain, (cr + er + rr) * gain];

            if self.bypass_mix != bypass_target {
                self.bypass_mix = if bypass_target > self.bypass_mix {
                    (self.bypass_mix + fade_step).min(1.0)
                } else {
                    (self.bypass_mix - fade_step).max(0.0)
                };
            }
            let mix = self.bypass_mix;
            for (ch, output) in channels.iter_mut().enumerate() {
                let processed = if num_outputs == 1 { 0.5 * (wet[0] + wet[1]) } else { wet[ch.min(1)] };
                let original = if ch < num_inputs { dry[ch] } else { dry[0] };
                output[i] = processed * (1.0 - mix) + original * mix;
                output_peak = output_peak.max(output[i].abs());
            }
            if let Some(report) = report {
                if report.voiced {
                    self.display.add_note(report.input_note, analysis_interval);
                }
                if gui_open {
                    self.display.push(&report);
                }
            }
        }
        if listening {
            if let (Some(m), Some(s)) = (&self.chroma_main, &self.chroma_side) {
                self.display.set_chroma(&m.chroma(), m.seconds(), &s.chroma(), s.seconds());
            }
        }
        self.display.set_block(input_peak, output_peak, bypass);
        // Le code di eco e riverbero continuano anche senza ingresso.
        ProcessStatus::Tail(u32::MAX)
    }
}

impl ClapPlugin for VocalSuite {
    const CLAP_ID: &'static str = "com.vladislav.srvocalsuite";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Correzione dell'intonazione con chorus, tape echo, riverbero e formanti");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::PitchCorrection,
        ClapFeature::Chorus,
        ClapFeature::Delay,
        ClapFeature::Reverb,
        ClapFeature::Mono,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for VocalSuite {
    // Identificativo unico (non cambiarlo: i progetti salvati lo cercano).
    const VST3_CLASS_ID: [u8; 16] = *b"SRVocalSuiteVlad";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::PitchShift, Vst3SubCategory::Delay, Vst3SubCategory::Reverb];
}

nih_export_clap!(VocalSuite);
nih_export_vst3!(VocalSuite);
