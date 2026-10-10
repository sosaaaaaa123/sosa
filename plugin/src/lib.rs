//! # SosaTune — il plugin
//!
//! Questo file collega il motore audio (`sosatune_dsp`) al DAW tramite nih-plug:
//! dichiara nome, formati (VST3 e CLAP), canali, parametri e interfaccia, e
//! nella funzione `process` passa l'audio al correttore campione per campione.

use sosatune_dsp::{CorrectionSettings, PitchCorrector, MAX_CHANNELS};
use nih_plug::prelude::*;
use std::sync::Arc;

mod editor;
mod params;
mod shared;

use params::{SosaTuneParams, VoiceChoice};
use shared::PitchDisplay;

pub struct SosaTune {
    params: Arc<SosaTuneParams>,
    /// Il motore audio. Viene creato in `initialize`, quando conosciamo la
    /// frequenza di campionamento e il numero di canali.
    corrector: Option<PitchCorrector>,
    /// Dati per il grafico dell'interfaccia.
    display: Arc<PitchDisplay>,
    /// Il tipo di voce attualmente impostato nel motore.
    current_voice: VoiceChoice,
    sample_rate: f32,
}

impl Default for SosaTune {
    fn default() -> Self {
        Self {
            params: Arc::new(SosaTuneParams::default()),
            corrector: None,
            display: Arc::new(PitchDisplay::default()),
            current_voice: VoiceChoice::AltoTenor,
            sample_rate: 48000.0,
        }
    }
}

impl SosaTune {
    fn publish_timing(&self) {
        if let Some(corrector) = &self.corrector {
            let latency_ms = corrector.latency() as f32 / self.sample_rate * 1000.0;
            self.display.set_timing(latency_ms, corrector.analysis_interval());
        }
    }
}

impl Plugin for SosaTune {
    const NAME: &'static str = "SosaTune";
    const VENDOR: &'static str = "Vladislav";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    // Funziona su tracce stereo e mono.
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
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

        // Qui si puo' allocare memoria: il motore prepara tutti i suoi buffer.
        self.sample_rate = buffer_config.sample_rate;
        let mut corrector = PitchCorrector::new(self.sample_rate, channels);
        self.current_voice = self.params.voice.value();
        corrector.set_voice(self.current_voice.to_dsp());

        // Diciamo al DAW di quanto ritardiamo l'audio, cosi' lo compensa.
        context.set_latency_samples(corrector.latency() as u32);
        self.corrector = Some(corrector);
        self.publish_timing();
        true
    }

    fn reset(&mut self) {
        if let Some(corrector) = &mut self.corrector {
            corrector.reset();
        }
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let Some(corrector) = self.corrector.as_mut() else {
            return ProcessStatus::Normal;
        };

        // Il tipo di voce cambia la latenza: va comunicata di nuovo al DAW.
        let voice = self.params.voice.value();
        if voice != self.current_voice {
            self.current_voice = voice;
            corrector.set_voice(voice.to_dsp());
            let latency = corrector.latency() as u32;
            nih_plug::util::permit_alloc(|| context.set_latency_samples(latency));
            let latency_ms = latency as f32 / self.sample_rate * 1000.0;
            self.display.set_timing(latency_ms, corrector.analysis_interval());
        }

        let settings = CorrectionSettings {
            notes: self.params.active_notes(),
            retune_ms: self.params.retune_speed.value(),
            amount: self.params.amount.value(),
            transpose: self.params.transpose.value() as f32,
            humanize: self.params.humanize.value(),
            flex_tune: self.params.flex_tune.value(),
            vibrato: self.params.vibrato.value(),
            formant: self.params.formant.value(),
            tracking: self.params.tracking.value(),
        };
        // Aggiorniamo il grafico solo se la finestra e' aperta.
        let gui_open = self.params.editor_state.is_open();

        let channels = buffer.as_slice();
        let num_channels = channels.len().min(MAX_CHANNELS);
        let num_samples = channels.first().map_or(0, |c| c.len());

        for i in 0..num_samples {
            let mut frame = [0.0f32; MAX_CHANNELS];
            for (ch, sample) in frame.iter_mut().enumerate().take(num_channels) {
                *sample = channels[ch][i];
            }

            let report = corrector.process_frame(&mut frame[..num_channels], &settings);

            for (ch, sample) in frame.iter().enumerate().take(num_channels) {
                channels[ch][i] = *sample;
            }
            if let (Some(report), true) = (report, gui_open) {
                self.display.push(&report);
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for SosaTune {
    const CLAP_ID: &'static str = "com.vladislav.sosatune";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Correzione dell'intonazione in tempo reale per la voce");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::PitchCorrection,
        ClapFeature::Mono,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for SosaTune {
    // Identificativo unico di 16 byte: non cambiarlo, o i DAW non riconosceranno
    // piu' i progetti salvati con questo plugin.
    const VST3_CLASS_ID: [u8; 16] = *b"SosaTuneVladPtch";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::PitchShift];
}

nih_export_clap!(SosaTune);
nih_export_vst3!(SosaTune);
