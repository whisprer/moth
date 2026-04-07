//! Voice — the complete Moth instrument voice.
//!
//! Chains all five signal chain sections with a hierarchical mixer.
//! Each level of the chain has its own blend/balance control, inspired
//! by the multi-level structural approach in Bernardes et al.'s
//! hierarchical mixing framework.
//!
//! ```text
//! PlayGesture ──→ ExciterProcessor ──┬──→ WaveguideString ──→ ResonantBody
//!                                    │         ↓                    ↓
//!                              exciter_bleed  vibrator_body_mix   body_output
//!                                    │              ↓                ↓
//!                                    └──────→ [Level 1 Mix] ←───────┘
//!                                                   ↓
//!                                            NonLinProcessor
//!                                                   ↓
//!                                            SpatialProcessor
//!                                                   ↓
//!                                                output
//! ```
//!
//! # Hierarchical Mix Levels
//!
//! - **Level 0**: Exciter bleed — raw exciter signal mixed into the output
//!   for transient click/attack presence (like Elements' strike bleed).
//! - **Level 1**: Vibrator/body balance — how much of the raw waveguide
//!   vs the body-coloured sound reaches the non-lin stage.
//! - **Level 2**: Non-lin wet/dry — controlled by `SaturationCharacter`.
//! - **Level 3**: Spatial wet/dry — controlled by `SpatialCharacter`.

use crate::exciter::ExciterModel;
use crate::exciter_dsp::ExciterProcessor;
use crate::gesture::PlayGesture;
use crate::instrument_dna::InstrumentDna;
use crate::nonlin::{NonLinProcessor, SaturationCharacter};
use crate::resonator::{BodyShape, ResonantBody};
use crate::spatial::{SpatialCharacter, SpatialProcessor};
use crate::vibrator::WaveguideString;

/// Maximum audio block size. Temporary buffers are allocated at this size.
const MAX_BLOCK: usize = 256;

/// Complete Moth voice — one note, all sections chained.
///
/// For polyphony, instantiate multiple `MothVoice`s (one per voice),
/// each with its own DNA variant via
/// [`InstrumentDna::voice_variant`](crate::instrument_dna::InstrumentDna::voice_variant).
pub struct MothVoice {
    // ── Signal chain ──
    exciter: ExciterProcessor,
    vibrator: WaveguideString,
    body: ResonantBody,
    nonlin: NonLinProcessor,
    spatial: SpatialProcessor,

    // ── Hierarchical mix ──
    /// Level 0: raw exciter transient mixed into output.
    /// `0.0` = no bleed, `0.1` = subtle click, `0.3` = prominent attack.
    exciter_bleed: f32,
    /// Level 1: vibrator/body balance.
    /// `0.0` = pure waveguide (raw string), `1.0` = pure body-filtered.
    body_mix: f32,

    // ── State ──
    sample_rate: f32,
}

impl MothVoice {
    /// Create a new Moth voice from a complete instrument DNA.
    ///
    /// Each voice carries DNA from every section of the instrument.
    /// For polyphonic use, create variants with
    /// [`InstrumentDna::voice_variant`].
    pub fn new(dna: &InstrumentDna, sample_rate: f32) -> Self {
        Self {
            exciter: ExciterProcessor::new(&dna.exciter, sample_rate),
            vibrator: WaveguideString::new(&dna.vibrator, sample_rate),
            body: ResonantBody::new(&dna.resonator, sample_rate),
            nonlin: NonLinProcessor::new(&dna.non_lin, sample_rate),
            spatial: SpatialProcessor::new(&dna.spatial, sample_rate),
            exciter_bleed: 0.05,
            body_mix: 0.85,
            sample_rate,
        }
    }

    // ── Configuration ──

    /// Set the vibrator frequency (pitch).
    pub fn set_frequency(&mut self, hz: f32) {
        self.vibrator.set_frequency(hz);
    }

    /// Set vibrator damping.
    pub fn set_damping(&mut self, damping: f32) {
        self.vibrator.set_damping(damping);
    }

    /// Set vibrator brightness.
    pub fn set_brightness(&mut self, brightness: f32) {
        self.vibrator.set_brightness(brightness);
    }

    /// Set vibrator dispersion (inharmonicity).
    pub fn set_dispersion(&mut self, dispersion: f32) {
        self.vibrator.set_dispersion(dispersion);
    }

    /// Apply a body shape.
    pub fn set_body(&mut self, shape: &BodyShape) {
        self.body.apply_shape(shape);
    }

    /// Set the body excitation position.
    pub fn set_position(&mut self, position: f32) {
        self.vibrator.set_position(position);
        self.body.set_position(position);
    }

    /// Apply a saturation character.
    pub fn set_nonlin(&mut self, ch: &SaturationCharacter) {
        self.nonlin.apply_character(ch);
    }

    /// Apply a spatial character.
    pub fn set_spatial(&mut self, ch: &SpatialCharacter) {
        self.spatial.apply_character(ch);
    }

    /// Set the exciter bleed level (raw transient in output).
    pub fn set_exciter_bleed(&mut self, bleed: f32) {
        self.exciter_bleed = bleed.clamp(0.0, 0.5);
    }

    /// Set the vibrator/body balance.
    pub fn set_body_mix(&mut self, mix: f32) {
        self.body_mix = mix.clamp(0.0, 1.0);
    }

    // ── Processing ──

    /// Process one audio block through the complete signal chain.
    ///
    /// `model` — the current exciter model (may be morphing).
    /// `gesture` — the current play gesture (from MIDI normaliser).
    /// `output` — filled with the final audio. Length must be ≤ MAX_BLOCK.
    pub fn process(
        &mut self,
        model: &ExciterModel,
        gesture: &PlayGesture,
        output: &mut [f32],
    ) {
        let len = output.len().min(MAX_BLOCK);

        // Temporary buffers on the stack
        let mut exciter_buf = [0.0f32; MAX_BLOCK];
        let mut vibrator_buf = [0.0f32; MAX_BLOCK];
        let mut body_buf = [0.0f32; MAX_BLOCK];
        let mut nonlin_buf = [0.0f32; MAX_BLOCK];

        let exc = &mut exciter_buf[..len];
        let vib = &mut vibrator_buf[..len];
        let bod = &mut body_buf[..len];
        let nl = &mut nonlin_buf[..len];
        let out = &mut output[..len];

        // ── Stage 1: Exciter ──
        self.exciter.process(model, gesture, exc);

        // ── Stage 2: Vibrator ──
        self.vibrator.process(exc, vib);

        // ── Stage 3: Resonant body ──
        self.body.process(vib, bod);

        // ── Level 1 Mix: vibrator/body balance ──
        let body_mix = self.body_mix;
        let vib_mix = 1.0 - body_mix;
        for i in 0..len {
            bod[i] = vib[i] * vib_mix + bod[i] * body_mix;
        }

        // ── Level 0: exciter bleed ──
        let bleed = self.exciter_bleed;
        if bleed > 0.001 {
            for i in 0..len {
                bod[i] += exc[i] * bleed;
            }
        }

        // ── Stage 4: Non-linearities ──
        self.nonlin.process(bod, nl);

        // ── Stage 5: Spatial ──
        self.spatial.process(nl, out);
    }

    /// Reset all state — silence everything.
    pub fn reset(&mut self) {
        self.exciter.reset();
        self.vibrator.reset();
        self.body.reset();
        self.nonlin.reset();
        self.spatial.reset();
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;
    const BLOCK: usize = 128;

    fn make_voice(seed: u32) -> MothVoice {
        let dna = InstrumentDna::from_seed(seed, SR);
        MothVoice::new(&dna, SR)
    }

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|&s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    fn peak(buf: &[f32]) -> f32 {
        buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max)
    }

    #[test]
    fn silent_gesture_silent_output() {
        let mut voice = make_voice(42);
        voice.set_frequency(440.0);
        voice.set_body(&BodyShape::GUITAR_SMALL);
        voice.set_nonlin(&SaturationCharacter::TAPE_GENTLE);
        voice.set_spatial(&SpatialCharacter::SMALL_ROOM);

        let mut out = [0.0f32; BLOCK];
        voice.process(&ExciterModel::PLUCK, &PlayGesture::SILENT, &mut out);

        assert!(peak(&out) < 0.001, "Silent gesture should be near-silent");
    }

    #[test]
    fn pluck_produces_pitched_sound() {
        let mut voice = make_voice(42);
        voice.set_frequency(440.0);
        voice.set_damping(0.7);
        voice.set_brightness(0.5);
        voice.set_body(&BodyShape::GUITAR_SMALL);
        voice.set_nonlin(&SaturationCharacter::TAPE_GENTLE);
        voice.set_spatial(&SpatialCharacter::SMALL_ROOM);

        let gesture = PlayGesture {
            position: 0.3,
            force: 0.8,
            speed: 0.0,
            continuity: true,
        };

        let mut out = [0.0f32; BLOCK];
        voice.process(&ExciterModel::PLUCK, &gesture, &mut out);

        assert!(rms(&out) > 0.001, "Pluck should produce audible output");

        // Should sustain into next block
        let silent_gesture = PlayGesture {
            continuity: false,
            ..PlayGesture::SILENT
        };
        let mut out2 = [0.0f32; BLOCK];
        voice.process(&ExciterModel::PLUCK, &silent_gesture, &mut out2);
        assert!(rms(&out2) > 0.0001, "String should still ring");
    }

    #[test]
    fn bow_sustains() {
        let mut voice = make_voice(42);
        voice.set_frequency(220.0);
        voice.set_damping(0.8);
        voice.set_body(&BodyShape::VIOLIN);
        voice.set_nonlin(&SaturationCharacter::TUBE_CLEAN);
        voice.set_spatial(&SpatialCharacter::MEDIUM_ROOM);

        let gesture = PlayGesture {
            position: 0.5,
            force: 0.7,
            speed: 0.5,
            continuity: true,
        };

        let mut out = [0.0f32; BLOCK];
        for _ in 0..20 {
            voice.process(&ExciterModel::BOW, &gesture, &mut out);
        }

        assert!(rms(&out) > 0.0001, "Bow should sustain over many blocks");
    }

    #[test]
    fn different_dna_different_sound() {
        let gesture = PlayGesture {
            position: 0.3,
            force: 0.7,
            speed: 0.0,
            continuity: true,
        };

        let mut voice_a = make_voice(0xAAAA);
        let mut voice_b = make_voice(0xBBBB);

        // Same settings
        for v in [&mut voice_a, &mut voice_b] {
            v.set_frequency(440.0);
            v.set_body(&BodyShape::GUITAR_SMALL);
            v.set_nonlin(&SaturationCharacter::CONSOLE);
            v.set_spatial(&SpatialCharacter::SMALL_ROOM);
        }

        let mut out_a = [0.0f32; BLOCK];
        let mut out_b = [0.0f32; BLOCK];
        voice_a.process(&ExciterModel::PLUCK, &gesture, &mut out_a);
        voice_b.process(&ExciterModel::PLUCK, &gesture, &mut out_b);

        let diff: f32 = out_a.iter().zip(out_b.iter())
            .map(|(a, b)| (a - b).abs()).sum();
        assert!(diff > 0.001, "Different DNA should sound different");
    }

    #[test]
    fn output_stays_bounded() {
        let mut voice = make_voice(42);
        voice.set_frequency(440.0);
        voice.set_body(&BodyShape::BELL);
        voice.set_nonlin(&SaturationCharacter::TAPE_HOT);
        voice.set_spatial(&SpatialCharacter::CATHEDRAL);

        let gesture = PlayGesture {
            position: 0.5,
            force: 1.0,
            speed: 1.0,
            continuity: true,
        };

        let mut out = [0.0f32; BLOCK];
        for _ in 0..50 {
            voice.process(&ExciterModel::BOW, &gesture, &mut out);
        }

        assert!(peak(&out) < 10.0, "Should not explode: peak={}", peak(&out));
    }

    #[test]
    fn reset_silences_everything() {
        let mut voice = make_voice(42);
        voice.set_frequency(440.0);
        voice.set_body(&BodyShape::GUITAR_SMALL);

        let gesture = PlayGesture {
            force: 0.8, continuity: true, ..PlayGesture::SILENT
        };
        let mut out = [0.0f32; BLOCK];
        voice.process(&ExciterModel::PLUCK, &gesture, &mut out);

        voice.reset();

        voice.process(&ExciterModel::PLUCK, &PlayGesture::SILENT, &mut out);
        assert!(peak(&out) < 0.01, "After reset, should be near-silent");
    }
}
