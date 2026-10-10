//! # SosaTune — il plugin
//!
//! Questo file collega il motore audio (`sosatune_dsp`) al DAW tramite nih-plug:
//! dichiara nome, formati (VST3 e CLAP), canali, parametri e interfaccia, e
//! nella funzione `process` passa l'audio al correttore campione per campione.

use sosatune_dsp::{CorrectionSettings, PitchCorrector, TubeCompressor, MAX_CHANNELS};
use nih_plug::prelude::*;
use std::sync::Arc;

mod denormals;
mod editor;
mod gui;
mod params;
#[doc(hidden)]
pub mod preview;
mod shared;

use params::{SosaTuneParams, VoiceChoice};
use shared::PitchDisplay;

pub struct SosaTune {
    params: Arc<SosaTuneParams>,
    /// Il motore audio. Viene creato in `initialize`, quando conosciamo la
    /// frequenza di campionamento e il numero di canali.
    corrector: Option<PitchCorrector>,
    /// Il compressore ottico a valvola, dopo la correzione.
    compressor: Option<TubeCompressor>,
    /// Dati per il grafico dell'interfaccia.
    display: Arc<PitchDisplay>,
    /// Il tipo di voce attualmente impostato nel motore.
    current_voice: VoiceChoice,
    sample_rate: f32,
    /// Quanti canali elabora il motore (quelli in ingresso: 1 o 2).
    input_channels: usize,
}

impl Default for SosaTune {
    fn default() -> Self {
        Self {
            params: Arc::new(SosaTuneParams::default()),
            corrector: None,
            compressor: None,
            display: Arc::new(PitchDisplay::default()),
            current_voice: VoiceChoice::AltoTenor,
            sample_rate: 48000.0,
            input_channels: 2,
        }
    }
}

impl SosaTune {
    /// Ritardo totale (correzione + compressore) in campioni. Quello del
    /// compressore e' fisso, anche quando e' spento: il DAW non deve
    /// ricalcolare la compensazione ogni volta che lo accendi.
    fn total_latency(corrector: &PitchCorrector) -> u32 {
        (corrector.latency() + TubeCompressor::fixed_latency()) as u32
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
        corrector.set_voice(self.current_voice.to_dsp());

        let mut compressor = TubeCompressor::new(self.sample_rate, channels);
        compressor.snap_to(&self.params.comp.settings(self.params.bypass.value()));

        // Diciamo al DAW di quanto ritardiamo l'audio, cosi' lo compensa.
        context.set_latency_samples(Self::total_latency(&corrector));
        self.corrector = Some(corrector);
        self.compressor = Some(compressor);
        self.publish_timing();
        true
    }

    fn reset(&mut self) {
        if let Some(corrector) = &mut self.corrector {
            corrector.reset();
        }
        if let Some(compressor) = &mut self.compressor {
            compressor.reset();
        }
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let (Some(corrector), Some(compressor)) = (self.corrector.as_mut(), self.compressor.as_mut()) else {
            return ProcessStatus::Normal;
        };

        // Il tipo di voce cambia la latenza: va comunicata di nuovo al DAW.
        let voice = self.params.voice.value();
        if voice != self.current_voice {
            self.current_voice = voice;
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
        // In bypass: nessuna correzione, nota e timbro invariati (ma stesso ritardo).
        let settings = if bypass {
            CorrectionSettings { amount: 0.0, transpose: 0.0, vibrato: 0.0, formant: 0.0, ..settings }
        } else {
            settings
        };
        let comp_settings = self.params.comp.settings(bypass);
        let mut max_gain_reduction = 0.0f32;
        let mut input_peak = 0.0f32;
        let mut output_peak = 0.0f32;
        // Aggiorniamo il grafico solo se la finestra e' aperta.
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
            compressor.process_frame(&mut frame[..num_inputs], &comp_settings);
            max_gain_reduction = max_gain_reduction.max(compressor.gain_reduction_db());

            for (ch, output) in channels.iter_mut().enumerate() {
                // Le uscite in piu' (mono -> stereo) ricevono il primo canale.
                output[i] = if ch < num_inputs { frame[ch] } else { frame[0] };
                output_peak = output_peak.max(output[i].abs());
            }
            if let (Some(report), true) = (report, gui_open) {
                self.display.push(&report);
            }
        }
        if gui_open {
            self.display.set_gain_reduction(max_gain_reduction);
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
