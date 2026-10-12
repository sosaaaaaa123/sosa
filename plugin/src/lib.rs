//! # SosaTune — il plugin
//!
//! Questo file collega il motore audio (`sosatune_dsp`) al DAW tramite nih-plug:
//! dichiara nome, formati (VST3 e CLAP), canali, parametri e interfaccia, e
//! nella funzione `process` passa l'audio al correttore campione per campione.

use sosatune_dsp::chroma::ChromaAnalyzer;
use sosatune_dsp::{CorrectionSettings, PitchCorrector, MAX_CHANNELS};
use nih_plug::prelude::*;
use std::sync::Arc;

mod defaults;
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
    /// AutoKey polifonico: cromagramma dell'ingresso e del sidechain.
    chroma_main: Option<ChromaAnalyzer>,
    chroma_side: Option<ChromaAnalyzer>,
    /// Latenza comunicata al DAW (quella del motore: in HQ resta uguale
    /// anche in bypass, cosi' accendere e spegnere non sposta la traccia).
    reported_latency: u32,
    /// Dissolvenza tra voce corretta (0) e voce originale (1), per entrare e
    /// uscire dal bypass senza clic.
    bypass_mix: f32,
    /// La voce originale ritardata quanto il motore (per il bypass): canali
    /// uno dopo l'altro, `dry_capacity` campioni ciascuno.
    dry_delay: Vec<f32>,
    dry_capacity: usize,
    dry_pos: usize,
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
            chroma_main: None,
            chroma_side: None,
            reported_latency: 0,
            bypass_mix: 0.0,
            dry_delay: Vec::new(),
            dry_capacity: 0,
            dry_pos: 0,
        }
    }
}

impl SosaTune {
    /// Ritardo totale in campioni: quello del motore (0 in Low Latency).
    ///
    /// In bypass resta lo stesso, come chiedono le regole VST3 e come fanno
    /// i plugin professionali: se cambiasse, a ogni clic su bypass il DAW
    /// dovrebbe riallineare la traccia (un salto o un buco nell'audio, e in
    /// alcuni DAW la voce resterebbe fuori tempo fino allo stop). Cosi'
    /// invece il confronto acceso/spento e' immediato e sempre a tempo; per
    /// registrare senza ritardo c'e' Low Latency, che e' a 0 ms.
    fn total_latency(corrector: &PitchCorrector) -> u32 {
        corrector.latency() as u32
    }

    fn publish_timing(&self) {
        if let Some(corrector) = &self.corrector {
            let latency_ms = self.reported_latency as f32 / self.sample_rate * 1000.0;
            self.display.set_timing(latency_ms, corrector.analysis_interval());
        }
    }
}

/// Nome dell'ingresso sidechain nel DAW.
const SIDECHAIN_NAMES: PortNames = PortNames {
    layout: None,
    main_input: None,
    main_output: None,
    aux_inputs: &["Sidechain (base per AutoKey)"],
    aux_outputs: &[],
};

impl Plugin for SosaTune {
    const NAME: &'static str = "Sosa Tune";
    const VENDOR: &'static str = "Vladislav";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    // Funziona su tracce stereo e mono, e anche mono -> stereo (la voce mono
    // corretta viene copiata su entrambi i lati): e' la configurazione che
    // molti DAW e contenitori di plugin (Pro Tools, Logic, Blue Cat PatchWork)
    // usano per gli effetti su una traccia vocale mono.
    // In piu' c'e' un ingresso sidechain (facoltativo): collegandoci la base
    // della canzone, AutoKey riconosce la tonalita' dagli strumenti.
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
            main_output_channels: NonZeroU32::new(1),
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

        self.chroma_main = Some(ChromaAnalyzer::new(self.sample_rate));
        self.chroma_side = Some(ChromaAnalyzer::new(self.sample_rate));

        // Diciamo al DAW di quanto ritardiamo l'audio, cosi' lo compensa.
        self.bypass_mix = if self.params.bypass.value() { 1.0 } else { 0.0 };
        // Linea di ritardo per la voce originale: basta per la latenza piu'
        // lunga del motore (HQ con voce grave, sotto i 100 ms).
        self.dry_capacity = (self.sample_rate * 0.25).ceil() as usize + 1;
        self.dry_delay = vec![0.0; self.dry_capacity * MAX_CHANNELS];
        self.dry_pos = 0;
        self.reported_latency = Self::total_latency(&corrector);
        context.set_latency_samples(self.reported_latency);
        self.corrector = Some(corrector);
        self.publish_timing();
        true
    }

    fn reset(&mut self) {
        if let Some(corrector) = &mut self.corrector {
            corrector.reset();
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
        let Some(corrector) = self.corrector.as_mut() else {
            return ProcessStatus::Normal;
        };

        // Tipo di voce e modalita' cambiano il motore e la latenza: va
        // comunicata di nuovo al DAW.
        let voice = self.params.voice.value();
        let quality = self.params.quality.value();
        let bypass = self.params.bypass.value();
        if voice != self.current_voice || quality != self.current_quality {
            self.current_voice = voice;
            self.current_quality = quality;
            corrector.set_quality(quality == QualityChoice::Hq);
            corrector.set_voice(voice.to_dsp());
        }
        let latency = Self::total_latency(corrector);
        if latency != self.reported_latency {
            self.reported_latency = latency;
            nih_plug::util::permit_alloc(|| context.set_latency_samples(latency));
            let latency_ms = latency as f32 / self.sample_rate * 1000.0;
            self.display.set_timing(latency_ms, corrector.analysis_interval());
        }

        // Numeri piccolissimi ("denormali") rallentano molto alcune CPU: per
        // tutta la durata di questo blocco li trattiamo come zero.
        let _ftz = denormals::FlushToZero::new();

        let settings = self.params.correction_settings();
        // In bypass il motore continua a girare senza correggere (cosi' e'
        // gia' pronto quando si riaccende) ma in uscita va la voce originale,
        // identica, ritardata quanto il motore (stessa latenza, sempre a tempo).
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

        // AutoKey in ascolto: cromagramma dell'ingresso (polifonico) e del sidechain.
        let listening = self.params.autokey.load(std::sync::atomic::Ordering::Relaxed);
        if self.display.take_chroma_reset() {
            for analyzer in [&mut self.chroma_main, &mut self.chroma_side].into_iter().flatten() {
                analyzer.reset();
            }
        }
        // La base della canzone sul sidechain, se il DAW ce la manda: AutoKey
        // la legge da sola (e' la fonte piu' sicura per la tonalita').
        let side = aux.inputs.first_mut().map(|b| b.as_slice_immutable());
        let side_channels = side.as_ref().map_or(0, |s| s.len());
        // Alcuni DAW, senza nessun sidechain scelto, ci mandano una copia
        // dell'ingresso: non e' una base, va ignorata (altrimenti AutoKey
        // leggerebbe la voce come se fosse la base).
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
            // La voce originale, ritardata della latenza attuale.
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

            // Entrata e uscita dal bypass: 10 ms di dissolvenza.
            if self.bypass_mix != bypass_target {
                self.bypass_mix = if bypass_target > self.bypass_mix {
                    (self.bypass_mix + fade_step).min(1.0)
                } else {
                    (self.bypass_mix - fade_step).max(0.0)
                };
            }
            let mix = self.bypass_mix;
            for (ch, output) in channels.iter_mut().enumerate() {
                // Le uscite in piu' (mono -> stereo) ricevono il primo canale.
                let c = if ch < num_inputs { ch } else { 0 };
                output[i] = frame[c] * (1.0 - mix) + dry[c] * mix;
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
