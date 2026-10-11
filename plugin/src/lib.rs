//! # SosaTune — il plugin
//!
//! Questo file collega il motore audio (`sosatune_dsp`) al DAW tramite nih-plug:
//! dichiara nome, formati (VST3 e CLAP), canali, parametri e interfaccia, e
//! nella funzione `process` passa l'audio al correttore campione per campione.

use sosatune_dsp::{CorrectionSettings, PitchCorrector, MAX_CHANNELS};
use nih_plug::prelude::*;
use std::sync::Arc;

mod denormals;
mod editor;
mod gui;
mod params;
mod presets;
#[doc(hidden)]
pub mod preview;
mod shared;

use params::{QualityChoice, SosaTuneParams, VoiceChoice};
use shared::PitchDisplay;

pub struct SosaTune {
    params: Arc<SosaTuneParams>,
    /// Il motore audio. Viene creato in `initialize`, quando conosciamo la
    /// frequenza di campionamento e il numero di canali.
    corrector: Option<PitchCorrector>,
    /// Dati per il grafico dell'interfaccia.
    display: Arc<PitchDisplay>,
    /// Il tipo di voce e la modalita' (Low Latency / HQ) impostati nel motore.
    current_voice: VoiceChoice,
    current_quality: QualityChoice,
    sample_rate: f32,
    /// Quanti canali elabora il motore (quelli in ingresso: 1 o 2).
    input_channels: usize,
}

impl Default for SosaTune {
    fn default() -> Self {
        Self {
            params: Arc::new(SosaTuneParams::default()),
            corrector: None,
            display: Arc::new(PitchDisplay::default()),
            current_voice: VoiceChoice::AltoTenor,
            current_quality: QualityChoice::LowLatency,
            sample_rate: 48000.0,
            input_channels: 2,
        }
    }
}

impl SosaTune {
    /// Ritardo totale in campioni.
    fn total_latency(corrector: &PitchCorrector) -> u32 {
        corrector.latency() as u32
    }

    fn publish_timing(&self) {
        if let Some(corrector) = &self.corrector {
            let latency_ms = Self::total_latency(corrector) as f32 / self.sample_rate * 1000.0;
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

    // Funziona su tracce stereo e mono, e anche mono -> stereo (la voce mono
    // corretta viene copiata su entrambi i lati): e' la configurazione che
    // molti DAW e contenitori di plugin (Pro Tools, Logic, Blue Cat PatchWork)
    // usano per gli effetti su una traccia vocale mono.
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
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(2),
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
        self.input_channels = channels;
        let mut corrector = PitchCorrector::new(self.sample_rate, channels);
        self.current_voice = self.params.voice.value();
        self.current_quality = self.params.quality.value();
        corrector.set_quality(self.current_quality == QualityChoice::Hq);
        corrector.set_voice(self.current_voice.to_dsp());

        // Diciamo al DAW di quanto ritardiamo l'audio, cosi' lo compensa.
        context.set_latency_samples(Self::total_latency(&corrector));
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

        // Tipo di voce e modalita' cambiano la latenza: va comunicata di nuovo al DAW.
        let voice = self.params.voice.value();
        let quality = self.params.quality.value();
        if voice != self.current_voice || quality != self.current_quality {
            self.current_voice = voice;
            self.current_quality = quality;
            corrector.set_quality(quality == QualityChoice::Hq);
            corrector.set_voice(voice.to_dsp());
            let latency = Self::total_latency(corrector);
            nih_plug::util::permit_alloc(|| context.set_latency_samples(latency));
            let latency_ms = latency as f32 / self.sample_rate * 1000.0;
            self.display.set_timing(latency_ms, corrector.analysis_interval());
        }

        // Numeri piccolissimi ("denormali") rallentano molto alcune CPU: per
        // tutta la durata di questo blocco li trattiamo come zero.
        let _ftz = denormals::FlushToZero::new();

        let bypass = self.params.bypass.value();
        let settings = self.params.correction_settings();
        // In bypass: nessuna correzione, nota e timbro invariati (ma stesso ritardo).
        let settings = if bypass {
            CorrectionSettings { amount: 0.0, transpose: 0.0, vibrato: 0.0, formant: 0.0, ..settings }
        } else {
            settings
        };
        let mut input_peak = 0.0f32;
        let analysis_interval = corrector.analysis_interval();
        let mut output_peak = 0.0f32;
        // Aggiorniamo l'indicatore solo se la finestra e' aperta.
        let gui_open = self.params.editor_state.is_open();

        let channels = buffer.as_slice();
        let num_outputs = channels.len();
        // Canali da elaborare: quelli in ingresso (mai piu' di quelli presenti).
        let num_inputs = self.input_channels.min(num_outputs).min(MAX_CHANNELS);
        let num_samples = channels.first().map_or(0, |c| c.len());
        if num_inputs == 0 {
            return ProcessStatus::Normal;
        }

        for i in 0..num_samples {
            let mut frame = [0.0f32; MAX_CHANNELS];
            for (ch, sample) in frame.iter_mut().enumerate().take(num_inputs) {
                *sample = channels[ch][i];
            }

            for &x in &frame[..num_inputs] {
                input_peak = input_peak.max(x.abs());
            }
            let report = corrector.process_frame(&mut frame[..num_inputs], &settings);

            for (ch, output) in channels.iter_mut().enumerate() {
                // Le uscite in piu' (mono -> stereo) ricevono il primo canale.
                output[i] = if ch < num_inputs { frame[ch] } else { frame[0] };
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
        // Per gli indicatori IN/OUT: dimostrano che l'audio passa davvero dal plugin.
        self.display.set_block(input_peak, output_peak, bypass);

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
