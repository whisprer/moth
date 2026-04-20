use nih_plug::prelude::*;
use nih_plug_vizia::ViziaState;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use moth::exciter::ExciterModel;
use moth::gesture::PlayGesture;
use moth::instrument_dna::InstrumentDna;
use moth::nonlin::SaturationCharacter;
use moth::resonator::BodyShape;
use moth::spatial::SpatialCharacter;
use moth::voice::MothVoice;

mod editor;

// ─── Plugin ─────────────────────────────────────────────────────────────────

struct MothPlugin {
    params: Arc<MothParams>,
    voice: Option<MothVoice>,
    /// Current MIDI note (if any).
    current_note: Option<u8>,
    /// Current gesture state.
    gesture: PlayGesture,
    /// Fingerprint of our previous output — detects feedback when the host
    /// feeds our own output back as input (no external routing configured).
    prev_out_sample: f32,
}

impl Default for MothPlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(MothParams::default()),
            voice: None,
            current_note: None,
            gesture: PlayGesture::SILENT,
            prev_out_sample: 0.0,
        }
    }
}

// ─── Parameters ─────────────────────────────────────────────────────────────

#[derive(Params)]
struct MothParams {
    /// Persisted editor window state (size, position).
    #[persist = "editor-state"]
    editor_state: Arc<ViziaState>,

    /// Persisted DNA seed — unique per plugin instance.
    /// 0 = not yet born (will be seeded on first init).
    /// Once seeded, the DAW persists this with the project forever.
    #[persist = "dna-seed"]
    dna_seed: Arc<AtomicU32>,

    // ── Exciter ──
    #[id = "exciter_morph"]
    exciter_morph: FloatParam,

    #[id = "spectral_tilt"]
    spectral_tilt: FloatParam,

    #[id = "stochasticity"]
    stochasticity: FloatParam,

    // ── Vibrator ──
    #[id = "vib_damping"]
    vib_damping: FloatParam,

    #[id = "vib_brightness"]
    vib_brightness: FloatParam,

    #[id = "vib_dispersion"]
    vib_dispersion: FloatParam,

    #[id = "position"]
    position: FloatParam,

    // ── Body ──
    #[id = "body_geometry"]
    body_geometry: FloatParam,

    #[id = "body_brightness"]
    body_brightness: FloatParam,

    #[id = "body_damping"]
    body_damping: FloatParam,

    #[id = "body_size"]
    body_size: FloatParam,

    // ── Non-lin ──
    #[id = "nl_drive"]
    nl_drive: FloatParam,

    #[id = "nl_tape"]
    nl_tape: FloatParam,

    #[id = "nl_tube"]
    nl_tube: FloatParam,

    #[id = "nl_warmth"]
    nl_warmth: FloatParam,

    #[id = "nl_tone"]
    nl_tone: FloatParam,

    // ── Spatial ──
    #[id = "room_size"]
    room_size: FloatParam,

    #[id = "room_mix"]
    room_mix: FloatParam,

    // ── Mixer ──
    #[id = "exciter_bleed"]
    exciter_bleed: FloatParam,

    #[id = "body_mix"]
    body_mix: FloatParam,

    // ── Master ──
    #[id = "master_gain"]
    master_gain: FloatParam,

    // ── External input ──
    #[id = "ext_mix"]
    ext_mix: FloatParam,
}

impl Default for MothParams {
    fn default() -> Self {
        Self {
            editor_state: editor::default_state(),
            dna_seed: Arc::new(AtomicU32::new(0)), // 0 = not yet born

            // ── Exciter ──
            exciter_morph: FloatParam::new(
                "Exciter",
                0.0, // pluck
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit(" morph")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            spectral_tilt: FloatParam::new("Tilt", 0.3, FloatRange::Linear { min: 0.0, max: 1.0 }),

            stochasticity: FloatParam::new(
                "Stochastic",
                0.05,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            ),

            // ── Vibrator ──
            vib_damping: FloatParam::new("Damping", 0.7, FloatRange::Linear { min: 0.0, max: 1.0 }),

            vib_brightness: FloatParam::new(
                "Brightness",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            ),

            vib_dispersion: FloatParam::new(
                "Dispersion",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            ),

            position: FloatParam::new("Position", 0.3, FloatRange::Linear { min: 0.0, max: 1.0 }),

            // ── Body ──
            body_geometry: FloatParam::new(
                "Geometry",
                0.38,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            ),

            body_brightness: FloatParam::new(
                "Body Bright",
                0.45,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            ),

            body_damping: FloatParam::new(
                "Body Damp",
                0.35,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            ),

            body_size: FloatParam::new("Body Size", 0.4, FloatRange::Linear { min: 0.0, max: 1.0 }),

            // ── Non-lin ──
            nl_drive: FloatParam::new("Drive", 1.5, FloatRange::Linear { min: 0.5, max: 4.0 }),

            nl_tape: FloatParam::new("Tape", 0.55, FloatRange::Linear { min: 0.0, max: 1.0 }),

            nl_tube: FloatParam::new("Tube", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 }),

            nl_warmth: FloatParam::new("Warmth", 0.4, FloatRange::Linear { min: 0.0, max: 1.0 }),

            nl_tone: FloatParam::new("Tone", 0.45, FloatRange::Linear { min: 0.0, max: 1.0 }),

            // ── Spatial ──
            room_size: FloatParam::new("Room", 0.3, FloatRange::Linear { min: 0.0, max: 1.0 }),

            room_mix: FloatParam::new("Reverb Mix", 0.2, FloatRange::Linear { min: 0.0, max: 1.0 }),

            // ── Mixer ──
            exciter_bleed: FloatParam::new(
                "Bleed",
                0.05,
                FloatRange::Linear { min: 0.0, max: 0.5 },
            ),

            body_mix: FloatParam::new("Body Mix", 0.85, FloatRange::Linear { min: 0.0, max: 1.0 }),

            // ── Master ──
            master_gain: FloatParam::new(
                "Master",
                util::db_to_gain(-6.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-36.0),
                    max: util::db_to_gain(6.0),
                    factor: FloatRange::gain_skew_factor(-36.0, 6.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(50.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(2))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),

            // ── External input ──
            ext_mix: FloatParam::new(
                "Ext Mix",
                0.0,
                FloatRange::Linear { min: 0.0, max: 0.99 },
            )
            .with_unit(" mix")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
        }
    }
}

// ─── Exciter morph helper ───────────────────────────────────────────────────

/// Map the exciter morph knob (0-1) to a blended ExciterModel.
///
/// 0.0 = Pluck, 0.2 = Pick, 0.4 = Bow, 0.6 = Breath, 0.8 = Ebow, 1.0 = Rain
fn morph_exciter(value: f32, tilt: f32, stochasticity: f32) -> ExciterModel {
    let presets = [
        ExciterModel::PLUCK,
        ExciterModel::PICK,
        ExciterModel::BOW,
        ExciterModel::BREATH,
        ExciterModel::EBOW,
        ExciterModel::RAIN,
    ];

    let scaled = value * (presets.len() - 1) as f32;
    let idx = (scaled as usize).min(presets.len() - 2);
    let frac = scaled - idx as f32;

    let mut model = presets[idx].lerp(presets[idx + 1], frac);
    model.spectral_tilt = tilt;
    model.stochasticity = stochasticity;
    model
}

/// Convert MIDI note number to frequency in Hz.
fn midi_note_to_hz(note: u8) -> f32 {
    440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0)
}

// ─── Plugin implementation ──────────────────────────────────────────────────

impl Plugin for MothPlugin {
    const NAME: &'static str = "Moth";
    const VENDOR: &'static str = "RYO Modular";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2), // external audio input
        main_output_channels: NonZeroU32::new(2), // stereo output
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone(), self.params.editor_state.clone())
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        let sample_rate = buffer_config.sample_rate;

        // ── DNA seed: birth of a unique Moth ──
        // First time this instance loads: generate a seed from entropy.
        // The DAW persists it with the project — same seed every reload.
        // On hardware, this would come from the MCU unique device ID.
        let mut seed = self.params.dna_seed.load(Ordering::Relaxed);
        if seed == 0 {
            // Not yet born — generate from system entropy.
            // Mix timestamp, process ID, and a pointer for uniqueness.
            let time_bits = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u32;
            let ptr_bits = (&*self as *const _ as usize) as u32;
            let pid_bits = std::process::id();

            // Combine entropy sources with xorshift mixing
            seed = time_bits;
            seed ^= pid_bits.wrapping_mul(2654435761); // Knuth multiplicative hash
            seed ^= ptr_bits.wrapping_mul(2246822519);
            seed ^= seed >> 16;
            seed = seed.wrapping_mul(0x45D9F3B);
            seed ^= seed >> 16;

            // Ensure nonzero (0 is our sentinel for "unborn")
            if seed == 0 {
                seed = 0x6D6F_7468;
            }

            self.params.dna_seed.store(seed, Ordering::Relaxed);
        }

        let dna = InstrumentDna::from_seed(seed, sample_rate);
        self.voice = Some(MothVoice::new(&dna, sample_rate));
        true
    }

    fn reset(&mut self) {
        if let Some(voice) = &mut self.voice {
            voice.reset();
        }
        self.current_note = None;
        self.gesture = PlayGesture::SILENT;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let voice = match &mut self.voice {
            Some(v) => v,
            None => return ProcessStatus::Normal,
        };

        // ── Handle MIDI events ──
        while let Some(event) = context.next_event() {
            match event {
                NoteEvent::NoteOn { note, velocity, .. } => {
                    self.current_note = Some(note);
                    voice.set_frequency(midi_note_to_hz(note));
                    self.gesture = PlayGesture {
                        position: self.params.position.value(),
                        force: velocity,
                        speed: 0.5,
                        continuity: true,
                    };
                }
                NoteEvent::NoteOff { note, .. } => {
                    if self.current_note == Some(note) {
                        self.current_note = None;
                        self.gesture.continuity = false;
                        self.gesture.force = 0.0;
                    }
                }
                NoteEvent::PolyPressure { pressure, note, .. } => {
                    if self.current_note == Some(note) {
                        self.gesture.force = pressure;
                    }
                }
                _ => {}
            }
        }

        // ── Read parameters and update voice ──
        let exciter_model = morph_exciter(
            self.params.exciter_morph.value(),
            self.params.spectral_tilt.value(),
            self.params.stochasticity.value(),
        );

        voice.set_damping(self.params.vib_damping.value());
        voice.set_brightness(self.params.vib_brightness.value());
        voice.set_dispersion(self.params.vib_dispersion.value());
        voice.set_position(self.params.position.value());

        voice.set_body(&BodyShape {
            geometry: self.params.body_geometry.value(),
            brightness: self.params.body_brightness.value(),
            damping: self.params.body_damping.value(),
            size: self.params.body_size.value(),
        });

        voice.set_nonlin(&SaturationCharacter {
            drive: self.params.nl_drive.value(),
            tape: self.params.nl_tape.value(),
            tube: self.params.nl_tube.value(),
            warmth: self.params.nl_warmth.value(),
            tone: self.params.nl_tone.value(),
        });

        voice.set_spatial(&SpatialCharacter {
            size: self.params.room_size.value(),
            brightness: 0.45, // fixed for now, could be another param
            mix: self.params.room_mix.value(),
        });

        voice.set_exciter_bleed(self.params.exciter_bleed.value());
        voice.set_body_mix(self.params.body_mix.value());

        self.gesture.position = self.params.position.value();

        // ── Process audio in chunks ──
        let num_samples = buffer.samples();
        let master_gain = self.params.master_gain.value();
        let ext_mix = self.params.ext_mix.value();

        let mut ext_buf = [0.0f32; 4096];
        let total = num_samples.min(4096);

        // ── Capture external input BEFORE we overwrite the buffer ──
        // When real audio is routed in (e.g. Reaper), the buffer has
        // genuine input. When nothing is routed (e.g. Ableton instrument
        // track), the buffer has garbage or our previous output.
        //
        // Either way: sanitize aggressively. Clamp to [-2, 2], kill
        // NaN/inf/denormal. If the input is garbage, it gets clamped
        // to near-zero and ext_mix just attenuates the exciter (harmless).
        if ext_mix > 0.001 && total > 0 {
            let channels = buffer.as_slice();
            if !channels.is_empty() {
                // Feedback detection: if first sample matches previous output,
                // the host recycled our buffer. Skip capture entirely.
                let first = channels[0][0];
                let is_feedback = first == self.prev_out_sample
                    && self.prev_out_sample != 0.0;

                if !is_feedback {
                    for i in 0..total {
                        let mut sum = 0.0f32;
                        for ch in channels.iter() {
                            let s = ch[i];
                            // Kill NaN, inf, denormals, and clamp to audio range
                            if s.is_finite() {
                                sum += s.clamp(-2.0, 2.0);
                            }
                        }
                        ext_buf[i] = sum / channels.len().max(1) as f32;
                    }
                }
            }
        }

        // ── Process Moth voice ──
        let mut mono_buf = [0.0f32; 4096];
        let mut voice_corrupted = false;

        let mut offset = 0;
        while offset < total {
            let chunk = (total - offset).min(256);
            let ext_slice = if ext_mix > 0.001 {
                Some(&ext_buf[offset..offset + chunk] as &[f32])
            } else {
                None
            };
            voice.process_with_external(
                &exciter_model,
                &self.gesture,
                ext_slice,
                ext_mix,
                &mut mono_buf[offset..offset + chunk],
            );
            offset += chunk;

            // Check for NaN/inf BEFORE flushing (so the guard can detect it)
            for s in mono_buf[offset - chunk..offset].iter() {
                if !s.is_finite() {
                    voice_corrupted = true;
                    break;
                }
            }

            // Flush denormals and NaN from output
            for s in mono_buf[offset - chunk..offset].iter_mut() {
                if !s.is_finite() || s.abs() < 1.0e-15 {
                    *s = 0.0;
                }
            }
        }

        // Nuclear guard: if voice produced NaN, reset it so it recovers
        if voice_corrupted {
            voice.reset();
            for s in mono_buf[..total].iter_mut() { *s = 0.0; }
        }

        // ── Write output + store fingerprint for next call ──
        let first_out = if total > 0 { mono_buf[0] * master_gain } else { 0.0 };
        self.prev_out_sample = first_out;

        for (i, mut frame) in buffer.iter_samples().enumerate() {
            let sample = if i < total {
                mono_buf[i] * master_gain
            } else {
                0.0
            };
            for channel_sample in frame.iter_mut() {
                *channel_sample = sample;
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for MothPlugin {
    const CLAP_ID: &'static str = "com.ryomodular.moth";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Physical modelling synthesiser — each instance alive and unrepeatable");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Synthesizer,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for MothPlugin {
    const VST3_CLASS_ID: [u8; 16] = *b"RYOMothSynthV001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

nih_export_clap!(MothPlugin);
nih_export_vst3!(MothPlugin);
