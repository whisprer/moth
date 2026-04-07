//! Exciter signal generation — the three coupling mode DSP engines.
//!
//! The [`ExciterProcessor`] takes an [`ExciterModel`](crate::exciter::ExciterModel)
//! (what kind of exciter), a [`PlayGesture`](crate::gesture::PlayGesture) (how
//! it's being played), and an [`ExciterDna`](crate::instrument_dna::ExciterDna)
//! (per-instance character) and produces an audio-rate excitation signal.
//!
//! # Coupling Mode Engines
//!
//! Three independent signal generators, mixed by coupling weights:
//!
//! - **Direct** — shaped noise burst for plucks, hammers, mallets.
//!   Impulse on gate rising edge, decaying envelope shaped by spectral tilt.
//!   Continuous mode sustains a driving signal (e-bow).
//!
//! - **Friction** — bistable stick-slip process for bowed strings, singing bowls.
//!   State alternates between stuck/slipping with probability controlled by
//!   gesture speed (bow velocity). Produces characteristic sawtooth-like energy.
//!
//! - **Pressure** — turbulent flow through a reed/jet for winds and brass.
//!   Nonlinear reed-opening function shapes the flow. Turbulence noise
//!   proportional to stochasticity and airflow rate.
//!
//! # Output
//!
//! The mixed signal is the excitation input for the vibrator section
//! (waveguide delay line). It's musically meaningful on its own —
//! audible as noise bursts, friction textures, and reed-like tones —
//! but becomes a full instrument voice only when coupled to a vibrator
//! and resonant body.
//!
//! # DNA Integration
//!
//! Per-instance character from [`ExciterDna`] is woven throughout:
//! - `signature` — offsets the direct impulse shape and initial burst character
//! - `noise_phase_offset` — seeds the noise RNG for unique texture
//! - `stochastic_bias` — shifts probability distributions in friction/pressure
//! - `spectral_tilt_bias` — subtle brightness modifier on the tilt filter
//! - `coupling_curve_shape` — scales nonlinearity in friction/pressure functions

use crate::dsp_core::{DcBlocker, DspRng, OnePole, soft_saturate};
use crate::exciter::ExciterModel;
use crate::gesture::PlayGesture;
use crate::instrument_dna::ExciterDna;

// ─── Per-coupling-mode state ────────────────────────────────────────────────

/// State for the direct coupling engine (pluck / hammer / e-bow).
#[derive(Debug, Clone, Copy)]
struct DirectState {
    /// Decaying impulse envelope. Set to `force` on gate rising edge,
    /// decays toward the sustain level.
    envelope: f32,

    /// Secondary envelope for the signature-derived pre-impulse.
    /// Like Elements' plectrum: a negative displacement before the
    /// main strike, whose magnitude depends on DNA.signature.
    pre_impulse_env: f32,
}

impl DirectState {
    const fn new() -> Self {
        Self {
            envelope: 0.0,
            pre_impulse_env: 0.0,
        }
    }
}

/// State for the friction coupling engine (bow / singing bowl).
#[derive(Debug, Clone, Copy)]
struct FrictionState {
    /// Bistable oscillator state — alternates between positive and
    /// negative values representing stick and slip phases.
    particle: f32,

    /// Smoothed output for the friction signal — prevents clicks
    /// at stick-slip transitions.
    smoothed: f32,
}

impl FrictionState {
    const fn new() -> Self {
        Self {
            particle: 0.5,
            smoothed: 0.0,
        }
    }
}

/// State for the pressure coupling engine (reed / air-jet).
#[derive(Debug, Clone, Copy)]
struct PressureState {
    /// Low-passed turbulence noise. Smoothed to avoid aliasing
    /// in the turbulent flow component.
    turbulence_lp: f32,

    /// Previous output for one-sample feedback in the reed model.
    prev_output: f32,
}

impl PressureState {
    const fn new() -> Self {
        Self {
            turbulence_lp: 0.0,
            prev_output: 0.0,
        }
    }
}

// ─── The processor ──────────────────────────────────────────────────────────

/// Audio-rate exciter signal processor.
///
/// Created once per voice at initialisation. Call [`process`](ExciterProcessor::process)
/// once per audio block with the current model, gesture, and output buffer.
///
/// # Example
///
/// ```
/// use moth::exciter_dsp::ExciterProcessor;
/// use moth::exciter::ExciterModel;
/// use moth::gesture::PlayGesture;
/// use moth::instrument_dna::InstrumentDna;
///
/// let dna = InstrumentDna::from_seed(0xDEAD_BEEF, 48000.0);
/// let mut proc = ExciterProcessor::new(&dna.exciter, 48000.0);
///
/// let model = ExciterModel::PLUCK;
/// let gesture = PlayGesture { position: 0.5, force: 0.8, speed: 0.0, continuity: true };
///
/// let mut buffer = [0.0f32; 128];
/// proc.process(&model, &gesture, &mut buffer);
///
/// // Buffer now contains the excitation signal
/// assert!(buffer.iter().any(|&s| s.abs() > 0.01));
/// ```
pub struct ExciterProcessor {
    sample_rate: f32,
    dna: ExciterDna,

    // ── Gate detection ──
    prev_gate: bool,

    // ── Per-coupling-mode state ──
    direct: DirectState,
    friction: FrictionState,
    pressure: PressureState,

    // ── Output chain ──
    tilt_filter: OnePole,
    dc_blocker: DcBlocker,

    // ── Noise generator ──
    rng: DspRng,

    // ── Smoothed gesture parameters (anti-zipper) ──
    smooth_force: f32,
    smooth_speed: f32,
}

impl ExciterProcessor {
    /// Create a new exciter processor.
    ///
    /// # Arguments
    ///
    /// * `dna` — per-instance exciter personality from [`InstrumentDna`].
    /// * `sample_rate` — system sample rate in Hz.
    pub fn new(dna: &ExciterDna, sample_rate: f32) -> Self {
        // Seed the noise RNG from DNA — noise_phase_offset gives each
        // instance a unique position in the noise sequence, and signature
        // provides additional differentiation.
        let rng_seed = (dna.noise_phase_offset * 4_294_967_296.0) as u32
            ^ (dna.signature * 2_147_483_648.0) as u32;

        Self {
            sample_rate,
            dna: *dna,
            prev_gate: false,
            direct: DirectState::new(),
            friction: FrictionState::new(),
            pressure: PressureState::new(),
            tilt_filter: OnePole::new(0.5),
            dc_blocker: DcBlocker::new(sample_rate),
            rng: DspRng::new(rng_seed),
            smooth_force: 0.0,
            smooth_speed: 0.0,
        }
    }

    /// Process one audio block.
    ///
    /// Fills `output` with the mixed excitation signal. The buffer length
    /// determines the block size — typically 16–256 samples.
    ///
    /// Gesture parameters (`force`, `speed`) are linearly interpolated
    /// across the block to prevent zipper noise. `position` and
    /// `continuity` are applied immediately (position is a slow-changing
    /// parameter; continuity is a boolean gate).
    pub fn process(
        &mut self,
        model: &ExciterModel,
        gesture: &PlayGesture,
        output: &mut [f32],
    ) {
        let len = output.len();
        if len == 0 {
            return;
        }

        // ── Gate edge detection ──
        let gate = gesture.continuity;
        let trigger = gate && !self.prev_gate;
        let _release = !gate && self.prev_gate;
        self.prev_gate = gate;

        // ── Handle triggers ──
        if trigger {
            self.on_trigger(model, gesture);
        }

        // ── Apply DNA to model parameters ──
        let effective_tilt = (model.spectral_tilt * self.dna.spectral_tilt_bias).clamp(0.0, 1.0);

        // Update tilt filter coefficient.
        // tilt=0 (soft) → coeff near 0.05 (dark).
        // tilt=1 (hard) → coeff capped by (1.0 - warmth_floor).
        //
        // The warmth floor ensures the tilt filter never goes fully
        // transparent. At warmth_floor=0.25, even maximum brightness
        // retains 25% lowpass effect. The instrument does not bite.
        let max_brightness = 1.0 - self.dna.warmth_floor; // e.g. 0.75 for floor=0.25
        let tilt_coeff = 0.05 + effective_tilt * (max_brightness - 0.05);
        self.tilt_filter.set_coeff(tilt_coeff);

        // ── Per-sample interpolation targets ──
        let force_target = gesture.force;
        let speed_target = gesture.speed;
        let force_inc = (force_target - self.smooth_force) / len as f32;
        let speed_inc = (speed_target - self.smooth_speed) / len as f32;

        // ── Coupling weights (pre-computed) ──
        let total_coupling = model.total_coupling().max(0.001);
        let norm = if total_coupling > 1.0 {
            1.0 / total_coupling
        } else {
            1.0
        };

        let w_direct = model.coupling_direct * norm;
        let w_friction = model.coupling_friction * norm;
        let w_pressure = model.coupling_pressure * norm;

        // ── Per-sample processing ──
        for sample in output.iter_mut() {
            self.smooth_force += force_inc;
            self.smooth_speed += speed_inc;

            let force = self.smooth_force.clamp(0.0, 1.0);
            let speed = self.smooth_speed.clamp(0.0, 1.0);

            // Generate each coupling mode
            let direct = self.process_direct(model, force, gate, effective_tilt);
            let friction = self.process_friction(model, force, speed, gate);
            let pressure = self.process_pressure(model, force, speed, gate);

            // Mix by coupling weights
            let mixed = direct * w_direct + friction * w_friction + pressure * w_pressure;

            // Apply spectral tilt filter
            let filtered = self.tilt_filter.process(mixed);

            // Soft saturation to prevent clipping
            let saturated = soft_saturate(filtered);

            // DC blocking
            let clean = self.dc_blocker.process(saturated);

            *sample = clean;
        }

        // ── Snap smoothed values to target to avoid drift ──
        self.smooth_force = force_target;
        self.smooth_speed = speed_target;
    }

    /// Handle gate rising edge — reset/trigger per-mode state.
    fn on_trigger(&mut self, _model: &ExciterModel, gesture: &PlayGesture) {
        let force = gesture.force;

        // Direct: initialise impulse envelope
        // DNA signature affects the pre-impulse magnitude (like Elements'
        // plectrum: negative displacement before the strike)
        let pre_impulse_amount = 0.05 + self.dna.signature * 0.20;
        self.direct.pre_impulse_env = -force * pre_impulse_amount;
        self.direct.envelope = force;

        // Friction: reset particle to initial position
        // DNA stochastic_bias determines which "side" we start on
        self.friction.particle = if self.dna.stochastic_bias > 0.5 {
            0.5
        } else {
            -0.5
        };

        // Pressure: reset state
        self.pressure.turbulence_lp = 0.0;
        self.pressure.prev_output = 0.0;
    }

    // ── Direct coupling engine ──────────────────────────────────────────

    /// Direct coupling: shaped noise burst / continuous driving signal.
    ///
    /// **Impulse mode** (`energy_continuity` near 0):
    /// On trigger, the envelope jumps to `force` and decays exponentially.
    /// Decay rate is controlled by spectral tilt — harder materials have
    /// shorter contact time (faster decay), softer materials sustain longer.
    ///
    /// **Continuous mode** (`energy_continuity` near 1):
    /// The envelope sustains at `energy_continuity * force` while the gate
    /// is held. The initial transient still occurs on top.
    ///
    /// The signal is white noise shaped by this envelope. The tilt filter
    /// (applied after mixing) further shapes the spectral content.
    #[inline]
    fn process_direct(
        &mut self,
        model: &ExciterModel,
        force: f32,
        gate: bool,
        effective_tilt: f32,
    ) -> f32 {
        // Sustain level while gate is held
        let sustain = if gate {
            model.energy_continuity * force
        } else {
            0.0
        };

        // Decay rate: soft (tilt=0) → 0.998 (slow, long contact)
        //             hard (tilt=1) → 0.90 (fast, short click)
        let decay = 0.998 - effective_tilt * 0.098;

        // Decay impulse envelope toward sustain level
        if self.direct.envelope > sustain {
            self.direct.envelope *= decay;
            if self.direct.envelope < sustain {
                self.direct.envelope = sustain;
            }
        } else {
            // Ramp up to sustain if below (e.g. after trigger with low
            // initial force, then force increases)
            self.direct.envelope += 0.01 * (sustain - self.direct.envelope);
        }

        // Pre-impulse envelope decays faster (it's the plectrum "pull-back")
        self.direct.pre_impulse_env *= 0.85;

        // Gate off: decay toward zero
        if !gate {
            self.direct.envelope *= 0.995;
            if self.direct.envelope.abs() < 1e-6 {
                self.direct.envelope = 0.0;
            }
        }

        // Noise source, scaled by envelope
        let noise = self.rng.next_bipolar();
        let total_env = self.direct.envelope + self.direct.pre_impulse_env;

        noise * total_env
    }

    // ── Friction coupling engine ────────────────────────────────────────

    /// Friction coupling: bistable stick-slip process.
    ///
    /// Inspired by Elements' `ProcessFlow`. The internal state alternates
    /// between two values (stuck / slipping) with a probability controlled
    /// by gesture speed (bow velocity). Between transitions, the output
    /// is the particle state plus a small amount of noise proportional to
    /// the speed.
    ///
    /// **Physical analogy:** when bow speed is low relative to string speed,
    /// the rosin creates static friction (stick). When the accumulated
    /// force exceeds the threshold, dynamic friction takes over (slip).
    /// The alternation produces the characteristic sawtooth-like energy
    /// injection of a bowed string.
    ///
    /// **DNA influence:**
    /// - `coupling_curve_shape` scales the flip threshold — higher values
    ///   make stick-slip transitions sharper.
    /// - `stochastic_bias` shifts the flip probability asymmetrically.
    #[inline]
    fn process_friction(
        &mut self,
        model: &ExciterModel,
        force: f32,
        speed: f32,
        gate: bool,
    ) -> f32 {
        if !gate {
            // Bow lifted — gentle decay to silence
            self.friction.smoothed *= 0.998;
            self.friction.particle *= 0.998;
            return self.friction.smoothed;
        }

        // Bow velocity (speed) controls the rate of stick-slip transitions.
        // Higher speed → more energy, faster transitions.
        let velocity = speed.max(0.001);
        let v4 = velocity * velocity * velocity * velocity; // v^4, from Elements

        // Flip threshold: how likely the state is to flip each sample.
        // Low velocity → very rare flips (near-static friction, quiet).
        // High velocity → frequent flips (strong bowing, loud).
        let base_threshold = 0.0001 + v4 * 0.125;

        // DNA modulation: coupling_curve_shape makes transitions sharper/softer
        let threshold = base_threshold * self.dna.coupling_curve_shape;

        // Stochastic bias shifts the flip probability slightly
        let biased_threshold = threshold * (self.dna.stochastic_bias * 2.0);

        // Model stochasticity adds additional randomness to the process
        let stoch_boost = 1.0 + model.stochasticity * 2.0;
        let final_threshold = (biased_threshold * stoch_boost).clamp(0.0, 0.5);

        // Roll the dice — flip?
        let noise = self.rng.next_unipolar();
        if noise < final_threshold {
            self.friction.particle = -self.friction.particle;
        }

        // Raw output: particle state + noise scaled by velocity
        let raw = self.friction.particle
            + (self.rng.next_bipolar() * 0.5 - self.friction.particle) * v4;

        // Smooth the output to soften transitions
        self.friction.smoothed += 0.2 * (raw - self.friction.smoothed);

        // Scale by bow pressure (force) — harder pressure = louder
        self.friction.smoothed * force
    }

    // ── Pressure coupling engine ────────────────────────────────────────

    /// Pressure coupling: turbulent flow through a nonlinear reed/jet.
    ///
    /// Models the exciter side of wind instrument physics:
    /// 1. Mouth pressure (`force`) drives airflow through a reed or jet.
    /// 2. The reed has a nonlinear opening function — it opens with moderate
    ///    pressure but closes again under extreme pressure (overblow).
    /// 3. Turbulence noise is mixed into the flow, proportional to
    ///    `stochasticity` and airflow rate.
    ///
    /// The output is a driving pressure signal that, when fed into a tube
    /// waveguide (vibrator section), produces self-oscillation above
    /// threshold — the characteristic onset of a wind instrument's tone.
    ///
    /// **DNA influence:**
    /// - `coupling_curve_shape` affects the reed stiffness curve.
    /// - `stochastic_bias` shifts the turbulence noise spectrum.
    #[inline]
    fn process_pressure(
        &mut self,
        model: &ExciterModel,
        force: f32,
        speed: f32,
        gate: bool,
    ) -> f32 {
        if !gate {
            // No breath — gentle decay
            self.pressure.turbulence_lp *= 0.995;
            self.pressure.prev_output *= 0.995;
            return self.pressure.prev_output;
        }

        // Mouth pressure and airflow
        let mouth_pressure = force;
        let airflow = speed.max(0.001);

        // ── Turbulence ──
        // Turbulent noise proportional to airflow and stochasticity.
        // Low-passed to avoid aliasing and produce a realistic breathy texture.
        let raw_noise = self.rng.next_bipolar();
        let turb_amount = model.stochasticity * airflow;

        // DNA bias shifts the noise distribution slightly
        let biased_noise = raw_noise + (self.dna.stochastic_bias - 0.5) * 0.1;

        // Smooth the turbulence — coefficient controls the turbulence spectrum
        // (higher = brighter turbulence)
        let turb_coeff = 0.2 + airflow * 0.3;
        self.pressure.turbulence_lp +=
            turb_coeff * (biased_noise * turb_amount - self.pressure.turbulence_lp);

        // ── Reed model ──
        // Nonlinear reed-opening function, inspired by Elements' tube.cc:
        //   reed = pressure * -0.2 + 0.8
        // The reed starts mostly open (0.8) and closes as pressure increases.
        // DNA coupling_curve_shape modifies the closure rate.
        let closure_rate = 0.2 * self.dna.coupling_curve_shape;
        let reed_opening = (0.8 - closure_rate * mouth_pressure).clamp(0.0, 1.0);

        // ── Flow through reed ──
        // Flow ∝ airflow × reed_opening
        // When pressure is too high, reed closes → overblow / squeal
        let flow = airflow * reed_opening;

        // ── Combine flow + turbulence ──
        let output = flow * mouth_pressure + self.pressure.turbulence_lp;

        // Light feedback from previous output adds a subtle self-oscillation
        // tendency, making the exciter "want" to produce pitched sound even
        // before the vibrator waveguide provides its own feedback.
        let with_feedback = output - 0.1 * self.pressure.prev_output;

        self.pressure.prev_output = with_feedback;

        with_feedback
    }

    /// Reset all state — panic/silence.
    pub fn reset(&mut self) {
        self.prev_gate = false;
        self.direct = DirectState::new();
        self.friction = FrictionState::new();
        self.pressure = PressureState::new();
        self.tilt_filter.reset();
        self.dc_blocker.reset();
        self.smooth_force = 0.0;
        self.smooth_speed = 0.0;
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instrument_dna::InstrumentDna;

    const SR: f32 = 48_000.0;
    const BLOCK: usize = 256;

    /// Helper: create a processor from a seed.
    fn make_proc(seed: u32) -> ExciterProcessor {
        let dna = InstrumentDna::from_seed(seed, SR);
        ExciterProcessor::new(&dna.exciter, SR)
    }

    /// Helper: RMS energy of a buffer.
    fn rms(buf: &[f32]) -> f32 {
        let sum: f32 = buf.iter().map(|&s| s * s).sum();
        (sum / buf.len() as f32).sqrt()
    }

    /// Helper: peak absolute value.
    fn peak(buf: &[f32]) -> f32 {
        buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max)
    }

    // ── Basic sanity ──

    #[test]
    fn silent_gesture_produces_silence() {
        let mut proc = make_proc(42);
        let mut buf = [0.0f32; BLOCK];
        proc.process(&ExciterModel::PLUCK, &PlayGesture::SILENT, &mut buf);
        assert!(
            peak(&buf) < 1e-6,
            "Silent gesture should produce silence, got peak {}",
            peak(&buf)
        );
    }

    #[test]
    fn pluck_produces_impulse() {
        let mut proc = make_proc(42);
        let mut buf = [0.0f32; BLOCK];

        let gesture = PlayGesture {
            position: 0.5,
            force: 0.8,
            speed: 0.0,
            continuity: true,
        };

        proc.process(&ExciterModel::PLUCK, &gesture, &mut buf);

        // Should have energy — the trigger fires on this first block
        let energy = rms(&buf);
        assert!(
            energy > 0.01,
            "Pluck should produce audible impulse, got RMS {energy}"
        );

        // Energy should be concentrated at the start (impulse-like)
        let first_quarter_rms = rms(&buf[..BLOCK / 4]);
        let last_quarter_rms = rms(&buf[3 * BLOCK / 4..]);
        assert!(
            first_quarter_rms > last_quarter_rms * 1.5,
            "Pluck impulse should be front-loaded: first={first_quarter_rms:.4}, last={last_quarter_rms:.4}"
        );
    }

    #[test]
    fn bow_produces_sustained_signal() {
        let mut proc = make_proc(42);
        let gesture = PlayGesture {
            position: 0.5,
            force: 0.7,
            speed: 0.5,
            continuity: true,
        };

        // Process several blocks to reach steady state
        let mut buf = [0.0f32; BLOCK];
        for _ in 0..10 {
            proc.process(&ExciterModel::BOW, &gesture, &mut buf);
        }

        // Should still have energy after many blocks (sustained excitation)
        let energy = rms(&buf);
        assert!(
            energy > 0.01,
            "Bow should sustain energy over time, got RMS {energy}"
        );
    }

    #[test]
    fn breath_produces_signal_with_flow() {
        let mut proc = make_proc(42);
        let gesture = PlayGesture {
            position: 0.5,
            force: 0.6,
            speed: 0.7,
            continuity: true,
        };

        let mut buf = [0.0f32; BLOCK];
        proc.process(&ExciterModel::BREATH, &gesture, &mut buf);

        let energy = rms(&buf);
        assert!(
            energy > 0.001,
            "Breath with airflow should produce signal, got RMS {energy}"
        );
    }

    #[test]
    fn no_speed_bow_is_quiet() {
        let mut proc = make_proc(42);
        let gesture = PlayGesture {
            position: 0.5,
            force: 0.8,
            speed: 0.0, // No bow movement
            continuity: true,
        };

        // Process several blocks
        let mut buf = [0.0f32; BLOCK];
        for _ in 0..5 {
            proc.process(&ExciterModel::BOW, &gesture, &mut buf);
        }

        let energy = rms(&buf);
        assert!(
            energy < 0.05,
            "Bow with zero speed should be very quiet, got RMS {energy}"
        );
    }

    // ── Force scaling ──

    #[test]
    fn higher_force_louder_pluck() {
        let mut proc_soft = make_proc(42);
        let mut proc_hard = make_proc(42);

        let soft = PlayGesture {
            force: 0.2,
            continuity: true,
            ..PlayGesture::SILENT
        };
        let hard = PlayGesture {
            force: 0.9,
            continuity: true,
            ..PlayGesture::SILENT
        };

        let mut buf_soft = [0.0f32; BLOCK];
        let mut buf_hard = [0.0f32; BLOCK];
        proc_soft.process(&ExciterModel::PLUCK, &soft, &mut buf_soft);
        proc_hard.process(&ExciterModel::PLUCK, &hard, &mut buf_hard);

        assert!(
            rms(&buf_hard) > rms(&buf_soft) * 1.5,
            "Harder pluck should be louder: soft={}, hard={}",
            rms(&buf_soft),
            rms(&buf_hard)
        );
    }

    // ── Gate behaviour ──

    #[test]
    fn gate_off_decays_to_silence() {
        let mut proc = make_proc(42);

        // Gate on — trigger
        let on = PlayGesture {
            force: 0.8,
            speed: 0.5,
            continuity: true,
            ..PlayGesture::SILENT
        };
        let mut buf = [0.0f32; BLOCK];
        proc.process(&ExciterModel::BOW, &on, &mut buf);

        // Gate off — should decay
        let off = PlayGesture {
            force: 0.0,
            speed: 0.0,
            continuity: false,
            ..PlayGesture::SILENT
        };
        for _ in 0..100 {
            proc.process(&ExciterModel::BOW, &off, &mut buf);
        }

        let energy = rms(&buf);
        assert!(
            energy < 0.001,
            "Should decay to silence after gate off, got RMS {energy}"
        );
    }

    // ── DNA differentiation ──

    #[test]
    fn different_dna_different_output() {
        let gesture = PlayGesture {
            position: 0.5,
            force: 0.7,
            speed: 0.5,
            continuity: true,
        };
        let model = ExciterModel::BOW;

        let mut proc_a = make_proc(0xAAAA);
        let mut proc_b = make_proc(0xBBBB);

        let mut buf_a = [0.0f32; BLOCK];
        let mut buf_b = [0.0f32; BLOCK];

        proc_a.process(&model, &gesture, &mut buf_a);
        proc_b.process(&model, &gesture, &mut buf_b);

        // Outputs should differ (different noise seeds, different DNA)
        let diff: f32 = buf_a
            .iter()
            .zip(buf_b.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();

        assert!(
            diff > 0.1,
            "Different DNA should produce different output, total diff = {diff}"
        );
    }

    #[test]
    fn same_dna_same_output() {
        let gesture = PlayGesture {
            position: 0.5,
            force: 0.7,
            speed: 0.5,
            continuity: true,
        };
        let model = ExciterModel::PLUCK;

        let mut proc_a = make_proc(42);
        let mut proc_b = make_proc(42);

        let mut buf_a = [0.0f32; BLOCK];
        let mut buf_b = [0.0f32; BLOCK];

        proc_a.process(&model, &gesture, &mut buf_a);
        proc_b.process(&model, &gesture, &mut buf_b);

        // Outputs should be identical (same seed, deterministic)
        for (i, (&a, &b)) in buf_a.iter().zip(buf_b.iter()).enumerate() {
            assert_eq!(a, b, "Sample {i} differs: {a} vs {b}");
        }
    }

    // ── Coupling mode mixing ──

    #[test]
    fn zero_coupling_produces_silence() {
        let mut proc = make_proc(42);

        let model = ExciterModel {
            coupling_direct: 0.0,
            coupling_friction: 0.0,
            coupling_pressure: 0.0,
            ..ExciterModel::PLUCK
        };

        let gesture = PlayGesture {
            force: 1.0,
            speed: 1.0,
            continuity: true,
            ..PlayGesture::SILENT
        };

        let mut buf = [0.0f32; BLOCK];
        proc.process(&model, &gesture, &mut buf);

        assert!(
            peak(&buf) < 1e-4,
            "Zero coupling should produce silence, got peak {}",
            peak(&buf)
        );
    }

    #[test]
    fn hybrid_coupling_mixes_modes() {
        let gesture = PlayGesture {
            position: 0.5,
            force: 0.7,
            speed: 0.5,
            continuity: true,
        };

        // Pure direct
        let mut proc_d = make_proc(42);
        let direct_only = ExciterModel {
            coupling_direct: 1.0,
            coupling_friction: 0.0,
            coupling_pressure: 0.0,
            energy_continuity: 0.5,
            ..ExciterModel::PLUCK
        };
        let mut buf_d = [0.0f32; BLOCK];
        proc_d.process(&direct_only, &gesture, &mut buf_d);

        // Pure friction
        let mut proc_f = make_proc(42);
        let friction_only = ExciterModel {
            coupling_direct: 0.0,
            coupling_friction: 1.0,
            coupling_pressure: 0.0,
            energy_continuity: 1.0,
            ..ExciterModel::BOW
        };
        let mut buf_f = [0.0f32; BLOCK];
        proc_f.process(&friction_only, &gesture, &mut buf_f);

        // Hybrid: both at 0.5
        let mut proc_h = make_proc(42);
        let hybrid = ExciterModel {
            coupling_direct: 0.5,
            coupling_friction: 0.5,
            coupling_pressure: 0.0,
            energy_continuity: 0.75,
            spectral_tilt: 0.5,
            stochasticity: 0.05,
            multiplicity: 1,
        };
        let mut buf_h = [0.0f32; BLOCK];
        proc_h.process(&hybrid, &gesture, &mut buf_h);

        // Hybrid should have energy (it's a mix of two active modes)
        assert!(
            rms(&buf_h) > 0.001,
            "Hybrid should produce signal: RMS {}",
            rms(&buf_h)
        );
    }

    // ── Spectral tilt ──

    #[test]
    fn soft_tilt_darker_than_hard() {
        let gesture = PlayGesture {
            force: 0.8,
            continuity: true,
            ..PlayGesture::SILENT
        };

        // Soft exciter (felt mallet)
        let mut proc_soft = make_proc(42);
        let soft_model = ExciterModel {
            spectral_tilt: 0.0,
            ..ExciterModel::MALLET
        };
        let mut buf_soft = [0.0f32; BLOCK];
        proc_soft.process(&soft_model, &gesture, &mut buf_soft);

        // Hard exciter (metal beater)
        let mut proc_hard = make_proc(42);
        let hard_model = ExciterModel {
            spectral_tilt: 1.0,
            ..ExciterModel::BEATER
        };
        let mut buf_hard = [0.0f32; BLOCK];
        proc_hard.process(&hard_model, &gesture, &mut buf_hard);

        // Measure "brightness" as high-frequency energy:
        // simple proxy = sum of absolute sample-to-sample differences
        let hf_soft: f32 = buf_soft
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .sum();
        let hf_hard: f32 = buf_hard
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .sum();

        assert!(
            hf_hard > hf_soft,
            "Hard tilt should have more HF energy: soft_hf={hf_soft:.3}, hard_hf={hf_hard:.3}"
        );
    }

    // ── Morphing ──

    #[test]
    fn morph_pluck_to_bow_produces_signal_throughout() {
        let gesture = PlayGesture {
            position: 0.5,
            force: 0.7,
            speed: 0.5,
            continuity: true,
        };

        // Morph through 11 steps from pluck to bow
        for step in 0..=10 {
            let t = step as f32 / 10.0;
            let model = ExciterModel::PLUCK.lerp(ExciterModel::BOW, t);

            let mut proc = make_proc(42);
            let mut buf = [0.0f32; BLOCK];
            proc.process(&model, &gesture, &mut buf);

            let energy = rms(&buf);
            assert!(
                energy > 0.001,
                "Morph step {step}/10 (t={t:.1}) should produce signal, got RMS {energy}"
            );
        }
    }

    // ── Output quality ──

    #[test]
    fn output_never_exceeds_bounds() {
        let gesture = PlayGesture {
            position: 0.5,
            force: 1.0,
            speed: 1.0,
            continuity: true,
        };

        // Test all presets at maximum gesture
        let presets = [
            ExciterModel::PLUCK,
            ExciterModel::PICK,
            ExciterModel::BOW,
            ExciterModel::BREATH,
            ExciterModel::MALLET,
            ExciterModel::BEATER,
            ExciterModel::EBOW,
            ExciterModel::SINGING_BOWL,
            ExciterModel::COL_LEGNO,
            ExciterModel::RAIN,
        ];

        for (i, preset) in presets.iter().enumerate() {
            let mut proc = make_proc(42);
            let mut buf = [0.0f32; BLOCK];
            proc.process(preset, &gesture, &mut buf);

            let p = peak(&buf);
            assert!(
                p < 3.0,
                "Preset {i} at max gesture exceeds safe level: peak {p}"
            );
        }
    }

    #[test]
    fn reset_clears_state() {
        let mut proc = make_proc(42);
        let gesture = PlayGesture {
            force: 0.8,
            speed: 0.5,
            continuity: true,
            ..PlayGesture::SILENT
        };

        // Process some audio
        let mut buf = [0.0f32; BLOCK];
        proc.process(&ExciterModel::BOW, &gesture, &mut buf);

        // Reset
        proc.reset();

        // Process with silent gesture — should be silent
        proc.process(&ExciterModel::BOW, &PlayGesture::SILENT, &mut buf);
        assert!(
            peak(&buf) < 0.01,
            "After reset + silent gesture, should be near-silent"
        );
    }

    // ── Stochasticity ──

    #[test]
    fn higher_stochasticity_more_variance() {
        let gesture = PlayGesture {
            force: 0.7,
            speed: 0.5,
            continuity: true,
            ..PlayGesture::SILENT
        };

        // Low stochasticity
        let mut proc_lo = make_proc(42);
        let lo_model = ExciterModel {
            stochasticity: 0.01,
            ..ExciterModel::BOW
        };
        let mut buf_lo = [0.0f32; BLOCK];
        for _ in 0..5 {
            proc_lo.process(&lo_model, &gesture, &mut buf_lo);
        }

        // High stochasticity
        let mut proc_hi = make_proc(42);
        let hi_model = ExciterModel {
            stochasticity: 0.9,
            ..ExciterModel::BOW
        };
        let mut buf_hi = [0.0f32; BLOCK];
        for _ in 0..5 {
            proc_hi.process(&hi_model, &gesture, &mut buf_hi);
        }

        // Measure variance (sample-to-sample variation)
        let var_lo: f32 = buf_lo.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum();
        let var_hi: f32 = buf_hi.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum();

        assert!(
            var_hi > var_lo,
            "Higher stochasticity should produce more variance: lo={var_lo:.4}, hi={var_hi:.4}"
        );
    }
}
