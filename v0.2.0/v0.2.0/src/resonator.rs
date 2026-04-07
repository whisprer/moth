//! Resonant body — the acoustic enclosure that colours the sound.
//!
//! The resonant body is the wooden box, the brass bell, the hollow tube —
//! the part of the instrument that amplifies and colours the vibrator's
//! output through its own resonant modes. In a guitar, it's the body.
//! In a clarinet, it's the bore extension. In a bell, it's the shell.
//!
//! # Architecture
//!
//! [`ResonantBody`] uses modal synthesis: a bank of parallel bandpass
//! filters (SVFs), each representing one resonant mode of the body.
//! The vibrator's output excites all modes simultaneously; each mode
//! rings at its own frequency, Q, and amplitude. The sum of all modes
//! IS the body's sound.
//!
//! ```text
//!            ┌─ [mode 0: SVF bandpass] ─ × gain × position_amp ─┐
//!            ├─ [mode 1: SVF bandpass] ─ × gain × position_amp ─┤
//! input ──→  ├─ [mode 2: SVF bandpass] ─ × gain × position_amp ─┼──→ Σ ──→ output
//!            ├─ ...                                              │
//!            └─ [mode N: SVF bandpass] ─ × gain × position_amp ─┘
//! ```
//!
//! # Geometry & Morphing
//!
//! The `geometry` parameter controls mode frequency spacing:
//! - `0.0` = compressed modes (tube-like, air column)
//! - `0.25` = harmonic modes (ideal string resonance)
//! - `0.5` = slightly inharmonic (wooden box, guitar body)
//! - `1.0` = widely spread (bell, metallic shell)
//!
//! All intermediate values are valid and musically meaningful.
//! Morphing between body shapes (e.g. guitar → violin → bell) is
//! achieved by sweeping `geometry`, `brightness`, and `damping`
//! continuously — every intermediate state is stable and sweet.
//!
//! # Envelope-Responsive Openness
//!
//! The body tracks the energy of the incoming signal via an envelope
//! follower. When the player plays hard, the body *opens up* —
//! higher modes become more audible, the Q increases slightly, the
//! sound blooms. When the player is gentle, the body settles into
//! warmth — only the lowest modes ring, the sound is soft and round.
//!
//! This is Moth's character expressed through physics: the instrument
//! meets you where you are. It does not resist you or punish you for
//! exploring. It meets you where you are.
//!
//! # DNA Integration
//!
//! [`ResonatorDna`](crate::instrument_dna::ResonatorDna) provides:
//! - `modal_drift` — subtle per-mode frequency offset, unique per instance
//! - `stereo_offset` — position in the internal stereo field
//! - `modulation_rate_hz` — speed of internal micro-movement (body "breathing")

use crate::dsp_core::{DcBlocker, soft_saturate};
use crate::instrument_dna::ResonatorDna;

// ─── Constants ──────────────────────────────────────────────────────────────

/// Number of resonant modes in the body model.
///
/// 24 modes give rich body resonance. Memory cost: ~300 bytes per body.
/// Enough for the characteristic low modes of a guitar body plus the
/// reverberant high-frequency tail.
const NUM_MODES: usize = 24;

const PI: f32 = core::f32::consts::PI;

// ─── Fast trig ──────────────────────────────────────────────────────────────

/// Fast cosine approximation for x in `[0, π]`.
///
/// Bhaskara I formula, accurate to ~1.5%. Used for the position-based
/// mode amplitude calculation (once per block, not per sample).
#[inline]
fn fast_cos(x: f32) -> f32 {
    // Handle full range [0, 2π] by symmetry
    let x = x % (2.0 * PI);
    let x = if x < 0.0 { x + 2.0 * PI } else { x };

    if x <= PI * 0.5 {
        // [0, π/2]: Bhaskara directly
        let x2 = x * x;
        (PI * PI - 4.0 * x2) / (PI * PI + x2)
    } else if x <= PI {
        // [π/2, π]: cos(x) = -cos(π - x)
        let xp = PI - x;
        let x2 = xp * xp;
        -(PI * PI - 4.0 * x2) / (PI * PI + x2)
    } else if x <= 1.5 * PI {
        // [π, 3π/2]: cos(x) = -cos(x - π)
        let xp = x - PI;
        let x2 = xp * xp;
        -(PI * PI - 4.0 * x2) / (PI * PI + x2)
    } else {
        // [3π/2, 2π]: cos(x) = cos(2π - x)
        let xp = 2.0 * PI - x;
        let x2 = xp * xp;
        (PI * PI - 4.0 * x2) / (PI * PI + x2)
    }
}

// ─── Chamberlin SVF ─────────────────────────────────────────────────────────

/// Chamberlin state variable filter — one resonant mode.
///
/// Provides bandpass output at the mode's frequency and Q.
/// The Chamberlin form allows real-time parameter changes without
/// transients — essential for the body's time-varying behaviour.
#[derive(Clone, Copy)]
struct ModeSvf {
    bp: f32, // bandpass state
    lp: f32, // lowpass state
}

impl ModeSvf {
    const fn new() -> Self {
        Self { bp: 0.0, lp: 0.0 }
    }

    /// Process one sample, returning the bandpass output.
    ///
    /// `f` = frequency coefficient = `2π × freq / sample_rate`, clamped < 1.0.
    /// `damp` = damping = `1 / Q`.
    #[inline]
    fn process(&mut self, input: f32, f: f32, damp: f32) -> f32 {
        // Chamberlin SVF update
        self.lp += f * self.bp;
        let hp = input - self.lp - damp * self.bp;
        self.bp += f * hp;
        self.bp
    }

    fn reset(&mut self) {
        self.bp = 0.0;
        self.lp = 0.0;
    }
}

// ─── Mode definition ────────────────────────────────────────────────────────

/// A single resonant mode with its derived parameters.
#[derive(Clone, Copy)]
struct Mode {
    svf: ModeSvf,
    /// Frequency ratio relative to the body's base frequency.
    freq_ratio: f32,
    /// Amplitude weight for this mode.
    gain: f32,
    /// Resonance Q factor.
    q: f32,
}

impl Mode {
    const fn new() -> Self {
        Self {
            svf: ModeSvf::new(),
            freq_ratio: 1.0,
            gain: 1.0,
            q: 50.0,
        }
    }
}

// ─── Body shape presets ─────────────────────────────────────────────────────

/// High-level body shape parameters — morphable bookmarks.
///
/// Like [`ExciterModel`](crate::exciter::ExciterModel) presets, these are
/// points in a continuous parameter space. Morph between any two using
/// [`lerp`](BodyShape::lerp).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyShape {
    /// Mode frequency spacing pattern.
    /// `0.0` = tube/air column, `0.25` = harmonic, `0.5` = wooden box, `1.0` = bell.
    pub geometry: f32,
    /// High-frequency mode emphasis. `0.0` = dark/muted, `1.0` = bright/ringy.
    pub brightness: f32,
    /// How quickly body resonances decay. `0.0` = short, `1.0` = long ring.
    pub damping: f32,
    /// Overall body size — scales all mode frequencies.
    /// `0.0` = tiny (high resonances), `1.0` = huge (low resonances).
    pub size: f32,
}

impl BodyShape {
    // ── Named presets ──

    /// Small acoustic guitar body — warm, woody, moderate resonance.
    pub const GUITAR_SMALL: Self = Self {
        geometry: 0.38,
        brightness: 0.45,
        damping: 0.35,
        size: 0.40,
    };

    /// Large acoustic guitar body — deeper, rounder, more bass.
    pub const GUITAR_LARGE: Self = Self {
        geometry: 0.38,
        brightness: 0.40,
        damping: 0.40,
        size: 0.55,
    };

    /// Violin body — bright, focused, quick response.
    pub const VIOLIN: Self = Self {
        geometry: 0.32,
        brightness: 0.60,
        damping: 0.30,
        size: 0.30,
    };

    /// Cello body — rich, warm, sustained.
    pub const CELLO: Self = Self {
        geometry: 0.33,
        brightness: 0.42,
        damping: 0.38,
        size: 0.65,
    };

    /// Hollow wooden box — dark, boxy, percussive.
    pub const WOODEN_BOX: Self = Self {
        geometry: 0.45,
        brightness: 0.30,
        damping: 0.50,
        size: 0.50,
    };

    /// Hollow tube / air column — breathy, harmonic, flute-like.
    pub const HOLLOW_TUBE: Self = Self {
        geometry: 0.08,
        brightness: 0.35,
        damping: 0.25,
        size: 0.55,
    };

    /// Metal plate — bright, inharmonic, gong-like.
    pub const METAL_PLATE: Self = Self {
        geometry: 0.78,
        brightness: 0.75,
        damping: 0.70,
        size: 0.35,
    };

    /// Bell / shell — very inharmonic, shimmering, sustained.
    pub const BELL: Self = Self {
        geometry: 0.92,
        brightness: 0.65,
        damping: 0.85,
        size: 0.28,
    };

    /// Linearly interpolate between two body shapes.
    ///
    /// `t = 0.0` → `self`, `t = 1.0` → `other`.
    /// Every intermediate state is musically valid and stable.
    #[inline]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let l = |a: f32, b: f32| a + (b - a) * t;
        Self {
            geometry: l(self.geometry, other.geometry),
            brightness: l(self.brightness, other.brightness),
            damping: l(self.damping, other.damping),
            size: l(self.size, other.size),
        }
    }
}

impl Default for BodyShape {
    fn default() -> Self {
        Self::GUITAR_SMALL
    }
}

// ─── The resonant body ─────────────────────────────────────────────────────

/// Modal resonant body processor.
///
/// Feed it the vibrator output; it produces the body-coloured sound.
///
/// # Example
///
/// ```
/// use moth::resonator::ResonantBody;
/// use moth::resonator::BodyShape;
/// use moth::instrument_dna::InstrumentDna;
///
/// let dna = InstrumentDna::from_seed(0xDEADBEEF, 48000.0);
/// let mut body = ResonantBody::new(&dna.resonator, 48000.0);
///
/// body.apply_shape(&BodyShape::GUITAR_SMALL);
/// body.set_position(0.3);
///
/// let input = [0.0f32; 128]; // from WaveguideString
/// let mut output = [0.0f32; 128];
/// body.process(&input, &mut output);
/// ```
pub struct ResonantBody {
    sample_rate: f32,
    dna: ResonatorDna,

    // ── Mode bank ──
    modes: [Mode; NUM_MODES],

    // ── Body parameters ──
    geometry: f32,
    brightness: f32,
    damping: f32,
    /// Base body resonant frequency in Hz. Derived from `size`.
    body_freq: f32,

    // ── Internal modulation ──
    /// LFO phase for internal body "breathing". Range [0, 1).
    lfo_phase: f32,
    /// LFO rate — per-sample phase increment, from DNA.
    lfo_rate: f32,

    // ── Envelope-responsive openness ──
    /// Smoothed input energy (envelope follower).
    input_envelope: f32,
    /// Current body openness. `0.0` = settled/warm, `1.0` = fully open/bright.
    /// Tracks input energy: play hard → body opens; play soft → body settles.
    openness: f32,

    // ── Excitation position ──
    /// Where on the body the vibration enters. Controls comb-filtered
    /// mode amplitude pattern.
    position: f32,
    /// Precomputed cos(position × π) for the Chebyshev recurrence.
    cos_pos: f32,

    // ── Output ──
    dc_blocker: DcBlocker,
}

impl ResonantBody {
    /// Create a new resonant body.
    pub fn new(dna: &ResonatorDna, sample_rate: f32) -> Self {
        let mut body = Self {
            sample_rate,
            dna: *dna,
            modes: [Mode::new(); NUM_MODES],
            geometry: 0.38,
            brightness: 0.45,
            damping: 0.35,
            body_freq: 200.0,
            lfo_phase: 0.0,
            lfo_rate: dna.modulation_rate_hz,
            input_envelope: 0.0,
            openness: 0.0,
            position: 0.3,
            cos_pos: fast_cos(0.3 * PI),
            dc_blocker: DcBlocker::new(sample_rate),
        };
        body.recompute_modes();
        body
    }

    /// Apply a body shape preset (or a morphed intermediate shape).
    pub fn apply_shape(&mut self, shape: &BodyShape) {
        self.geometry = shape.geometry.clamp(0.0, 1.0);
        self.brightness = shape.brightness.clamp(0.0, 1.0);
        self.damping = shape.damping.clamp(0.0, 1.0);

        // Size → base body frequency.
        // size=0.0 (tiny) → ~800 Hz, size=1.0 (huge) → ~60 Hz.
        // Quadratic mapping for perceptually linear size control.
        let s = shape.size.clamp(0.0, 1.0);
        self.body_freq = 800.0 * (1.0 - s * s) + 60.0 * s * s;

        self.recompute_modes();
    }

    /// Set the excitation position on the body.
    ///
    /// `0.0` / `1.0` = edge (all modes), `0.5` = centre (odd modes only).
    pub fn set_position(&mut self, position: f32) {
        self.position = position.clamp(0.0, 1.0);
        // Clamp away from extremes (Elements' formula)
        let clamped = 0.5 - 0.98 * (self.position - 0.5).abs();
        self.cos_pos = fast_cos(clamped * PI);
    }

    /// Recompute mode parameters from geometry/brightness/damping.
    ///
    /// Called when body shape changes. Not per-sample — only when
    /// `apply_shape()` or individual setters are called.
    fn recompute_modes(&mut self) {
        let stiffness_base = self.stiffness_from_geometry();

        // Brightness rolloff: how quickly higher modes lose gain.
        // brightness=0 → steep rolloff (dark), brightness=1 → flat (bright).
        // But never fully flat — warmth is always present.
        let rolloff = 0.15 + (1.0 - self.brightness) * 0.85;

        // Base Q from damping parameter.
        // damping=0 → Q=15 (very damped), damping=1 → Q=400 (long ring).
        let base_q = 15.0 + self.damping * self.damping * 385.0;

        let mut harmonic = 1.0f32;
        let mut stretch = 1.0f32;
        let mut stiffness = stiffness_base;

        for i in 0..NUM_MODES {
            // ── Frequency ratio ──
            let freq_ratio = harmonic * stretch;

            // DNA modal drift: each instance has unique mode positions.
            // Higher modes drift more (like imperfections in real bodies).
            let drift_scale = 1.0 + (i as f32) * 0.3;
            let drift = 1.0 + (self.dna.modal_drift - 1.0) * drift_scale;

            self.modes[i].freq_ratio = freq_ratio * drift;

            // ── Gain ──
            // Hyperbolic rolloff: warm bias on low modes.
            let base_gain = 1.0 / (1.0 + (i as f32) * rolloff);

            // Warmth emphasis: lowest 3 modes are always slightly boosted.
            // This ensures no instance of Moth is ever thin or cold.
            let warmth = if i == 0 {
                1.3
            } else if i < 3 {
                1.15
            } else {
                1.0
            };

            self.modes[i].gain = base_gain * warmth;

            // ── Q factor ──
            // Higher modes have lower Q (broader, more damped) — natural
            // for real bodies where high-frequency modes decay faster.
            let mode_q = base_q / (1.0 + (i as f32) * 0.12);
            self.modes[i].q = mode_q.max(2.0); // floor prevents instability

            // ── Advance for next mode ──
            stretch += stiffness;
            if stiffness < 0.0 {
                stiffness *= 0.93; // prevent negative freq folding
            } else {
                stiffness *= 0.98; // gradual taper
            }
            harmonic += 1.0;
        }
    }

    /// Map geometry [0, 1] to stiffness (mode frequency spreading rate).
    fn stiffness_from_geometry(&self) -> f32 {
        // 0.0  → -0.01 (compressed, tube-like: modes bunch together)
        // 0.25 → 0.0   (harmonic: modes at integer ratios)
        // 0.5  → 0.01  (wooden box: slight spread)
        // 1.0  → 0.03  (bell: wide spread, very inharmonic)
        (self.geometry - 0.25) * 0.04
    }

    /// Process one audio block through the resonant body.
    ///
    /// `input` = vibrator output. `output` = body-coloured sound.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        debug_assert_eq!(input.len(), output.len());
        let len = input.len();
        if len == 0 {
            return;
        }

        let cos_w = self.cos_pos;
        let two_cos_w = 2.0 * cos_w;
        let body_freq = self.body_freq;
        let inv_sr = 1.0 / self.sample_rate;

        for i in 0..len {
            let inp = input[i];

            // ── Envelope follower ──
            // Fast attack, slow release — the body responds to energy.
            let energy = inp * inp;
            let env_coeff = if energy > self.input_envelope {
                0.05 // fast attack: body opens quickly
            } else {
                0.0005 // slow release: body settles gently
            };
            self.input_envelope += env_coeff * (energy - self.input_envelope);

            // ── Openness ──
            // Maps input energy to [0, 1]. Gentle scaling so moderate
            // playing gives noticeable openness.
            let target = (self.input_envelope * 50.0).clamp(0.0, 1.0);
            self.openness += 0.005 * (target - self.openness);

            // ── Internal LFO (body "breathing") ──
            self.lfo_phase += self.lfo_rate;
            if self.lfo_phase >= 1.0 {
                self.lfo_phase -= 1.0;
            }
            // Triangle LFO: smooth, no harmonics, organic movement.
            let lfo = if self.lfo_phase < 0.5 {
                self.lfo_phase * 4.0 - 1.0
            } else {
                3.0 - self.lfo_phase * 4.0
            };

            // ── Process all modes ──
            let mut sum = 0.0f32;

            // Chebyshev recurrence for position-based mode amplitude:
            // cos(n × w) where w = clamped_position × π
            let mut cn_prev2 = 1.0f32; // cos(0) = 1
            let mut cn_prev1 = cos_w;  // cos(w)

            for m in 0..NUM_MODES {
                let mode = &mut self.modes[m];

                // ── Position amplitude (comb filtering) ──
                let pos_amp = if m == 0 {
                    1.0
                } else if m == 1 {
                    cos_w
                } else {
                    let cn = two_cos_w * cn_prev1 - cn_prev2;
                    cn_prev2 = cn_prev1;
                    cn_prev1 = cn;
                    cn
                };

                // ── Mode frequency with LFO micro-modulation ──
                let freq = body_freq * mode.freq_ratio;
                // LFO wobbles each mode's frequency very slightly.
                // DNA stereo_offset determines the wobble depth.
                let wobble = 1.0 + lfo * 0.0008 * self.dna.stereo_offset
                    * (1.0 + m as f32 * 0.1);
                let freq = freq * wobble;

                // ── SVF coefficient ──
                // Small-angle approximation: sin(πf/sr) ≈ πf/sr
                // Valid for body modes (typically < 5kHz at 48kHz).
                let f_coeff = (2.0 * PI * freq * inv_sr).clamp(0.001, 0.95);

                // ── Q with openness modulation ──
                // Playing harder → body opens → Q increases slightly
                // (modes ring longer, more harmonics audible).
                // But capped to prevent runaway resonance.
                let q_mod = 1.0 + self.openness * 0.4;
                let effective_q = (mode.q * q_mod).clamp(2.0, 500.0);
                let damp = 1.0 / effective_q;

                // ── Process SVF bandpass ──
                let bp = mode.svf.process(inp, f_coeff, damp);

                // ── Gain with openness-responsive brightness ──
                // Higher modes get louder when the body is open.
                // This creates the natural "body blooms when you dig in" effect.
                let open_boost = if m > 3 {
                    1.0 + self.openness * 0.6 * (1.0 - m as f32 / NUM_MODES as f32)
                } else {
                    1.0
                };

                sum += bp * mode.gain * pos_amp.abs() * open_boost;
            }

            // ── Output ──
            // Soft saturation prevents the resonant peaks from clipping.
            // This is the body "leaning toward resolution" — it gives
            // rather than resists under extreme input.
            let saturated = soft_saturate(sum * 0.15);
            output[i] = self.dc_blocker.process(saturated);
        }
    }

    /// Reset all state — silence the body.
    pub fn reset(&mut self) {
        for mode in self.modes.iter_mut() {
            mode.svf.reset();
        }
        self.input_envelope = 0.0;
        self.openness = 0.0;
        self.lfo_phase = 0.0;
        self.dc_blocker.reset();
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instrument_dna::InstrumentDna;

    const SR: f32 = 48_000.0;
    const BLOCK: usize = 256;

    fn make_body(seed: u32) -> ResonantBody {
        let dna = InstrumentDna::from_seed(seed, SR);
        ResonantBody::new(&dna.resonator, SR)
    }

    fn rms(buf: &[f32]) -> f32 {
        let sum: f32 = buf.iter().map(|&s| s * s).sum();
        (sum / buf.len() as f32).sqrt()
    }

    fn peak(buf: &[f32]) -> f32 {
        buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max)
    }

    // ── Basic operation ──

    #[test]
    fn silence_in_silence_out() {
        let mut body = make_body(42);
        body.apply_shape(&BodyShape::GUITAR_SMALL);
        let inp = [0.0f32; BLOCK];
        let mut out = [0.0f32; BLOCK];
        body.process(&inp, &mut out);
        assert!(peak(&out) < 1e-6, "No input should mean no output");
    }

    #[test]
    fn impulse_produces_ringing() {
        let mut body = make_body(42);
        body.apply_shape(&BodyShape::GUITAR_SMALL);

        // Impulse input
        let mut inp = [0.0f32; BLOCK];
        inp[0] = 1.0;
        let mut out = [0.0f32; BLOCK];
        body.process(&inp, &mut out);

        assert!(rms(&out) > 0.0001, "Impulse should excite body modes");

        // Process more blocks — body should still ring
        let silent = [0.0f32; BLOCK];
        let mut out2 = [0.0f32; BLOCK];
        body.process(&silent, &mut out2);

        assert!(
            rms(&out2) > 0.00001,
            "Body should ring after impulse, got RMS {}",
            rms(&out2)
        );
    }

    #[test]
    fn higher_damping_longer_ring() {
        let mut body_lo = make_body(42);
        body_lo.apply_shape(&BodyShape {
            damping: 0.1,
            ..BodyShape::GUITAR_SMALL
        });

        let mut body_hi = make_body(42);
        body_hi.apply_shape(&BodyShape {
            damping: 0.9,
            ..BodyShape::GUITAR_SMALL
        });

        let mut inp = [0.0f32; BLOCK];
        inp[0] = 1.0;
        let silent = [0.0f32; BLOCK];

        let mut out_lo = [0.0f32; BLOCK];
        let mut out_hi = [0.0f32; BLOCK];

        body_lo.process(&inp, &mut out_lo);
        body_hi.process(&inp, &mut out_hi);

        for _ in 0..30 {
            body_lo.process(&silent, &mut out_lo);
            body_hi.process(&silent, &mut out_hi);
        }

        assert!(
            rms(&out_hi) > rms(&out_lo) * 1.5,
            "Higher damping should ring longer: lo={:.6}, hi={:.6}",
            rms(&out_lo),
            rms(&out_hi)
        );
    }

    // ── Geometry ──

    #[test]
    fn geometry_changes_character() {
        let mut inp = [0.0f32; BLOCK];
        inp[0] = 1.0;
        let silent = [0.0f32; BLOCK];

        // Tube-like (harmonic)
        let mut body_tube = make_body(42);
        body_tube.apply_shape(&BodyShape::HOLLOW_TUBE);
        let mut out_tube = [0.0f32; BLOCK];
        body_tube.process(&inp, &mut out_tube);
        for _ in 0..5 {
            body_tube.process(&silent, &mut out_tube);
        }

        // Bell-like (inharmonic)
        let mut body_bell = make_body(42);
        body_bell.apply_shape(&BodyShape::BELL);
        let mut out_bell = [0.0f32; BLOCK];
        body_bell.process(&inp, &mut out_bell);
        for _ in 0..5 {
            body_bell.process(&silent, &mut out_bell);
        }

        // Outputs should differ
        let diff: f32 = out_tube
            .iter()
            .zip(out_bell.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(
            diff > 0.001,
            "Different geometry should produce different character: diff={diff}"
        );
    }

    // ── Morphing ──

    #[test]
    fn morph_guitar_to_bell_all_valid() {
        let mut inp = [0.0f32; BLOCK];
        inp[0] = 1.0;

        for step in 0..=10 {
            let t = step as f32 / 10.0;
            let shape = BodyShape::GUITAR_SMALL.lerp(BodyShape::BELL, t);

            let mut body = make_body(42);
            body.apply_shape(&shape);

            let mut out = [0.0f32; BLOCK];
            body.process(&inp, &mut out);

            let energy = rms(&out);
            assert!(
                energy > 0.00001,
                "Morph step {step}/10 should produce signal, got RMS {energy}"
            );
            assert!(
                peak(&out) < 5.0,
                "Morph step {step}/10 should not explode, got peak {}",
                peak(&out)
            );
        }
    }

    // ── DNA differentiation ──

    #[test]
    fn different_dna_different_ring() {
        let mut inp = [0.0f32; BLOCK];
        inp[0] = 1.0;
        let silent = [0.0f32; BLOCK];

        let mut body_a = make_body(0xAAAA);
        let mut body_b = make_body(0xBBBB);
        body_a.apply_shape(&BodyShape::GUITAR_SMALL);
        body_b.apply_shape(&BodyShape::GUITAR_SMALL);

        let mut out_a = [0.0f32; BLOCK];
        let mut out_b = [0.0f32; BLOCK];

        body_a.process(&inp, &mut out_a);
        body_b.process(&inp, &mut out_b);

        for _ in 0..5 {
            body_a.process(&silent, &mut out_a);
            body_b.process(&silent, &mut out_b);
        }

        let diff: f32 = out_a
            .iter()
            .zip(out_b.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(
            diff > 0.0001,
            "Different DNA should produce different ring: diff={diff}"
        );
    }

    #[test]
    fn same_dna_same_ring() {
        let mut inp = [0.0f32; BLOCK];
        inp[0] = 1.0;

        let mut body_a = make_body(42);
        let mut body_b = make_body(42);
        body_a.apply_shape(&BodyShape::GUITAR_SMALL);
        body_b.apply_shape(&BodyShape::GUITAR_SMALL);

        let mut out_a = [0.0f32; BLOCK];
        let mut out_b = [0.0f32; BLOCK];

        body_a.process(&inp, &mut out_a);
        body_b.process(&inp, &mut out_b);

        for (i, (&a, &b)) in out_a.iter().zip(out_b.iter()).enumerate() {
            assert_eq!(a, b, "Sample {i} differs: {a} vs {b}");
        }
    }

    // ── Openness / envelope response ──

    #[test]
    fn loud_input_opens_body() {
        let mut body = make_body(42);
        body.apply_shape(&BodyShape::GUITAR_SMALL);

        // Process several blocks of loud signal
        let loud = [0.5f32; BLOCK];
        let mut out = [0.0f32; BLOCK];
        for _ in 0..10 {
            body.process(&loud, &mut out);
        }

        assert!(
            body.openness > 0.1,
            "Sustained loud input should open the body, got openness {}",
            body.openness
        );
    }

    #[test]
    fn quiet_input_settles_body() {
        let mut body = make_body(42);
        body.apply_shape(&BodyShape::GUITAR_SMALL);

        // First, open it up
        let loud = [0.5f32; BLOCK];
        let mut out = [0.0f32; BLOCK];
        for _ in 0..10 {
            body.process(&loud, &mut out);
        }
        let open_level = body.openness;

        // Then, go quiet
        let quiet = [0.001f32; BLOCK];
        for _ in 0..200 {
            body.process(&quiet, &mut out);
        }

        assert!(
            body.openness < open_level * 0.5,
            "Quiet input should settle the body: was {open_level:.3}, now {:.3}",
            body.openness
        );
    }

    // ── Position ──

    #[test]
    fn position_changes_timbre() {
        let mut inp = [0.0f32; BLOCK];
        inp[0] = 1.0;
        let silent = [0.0f32; BLOCK];

        let mut body_edge = make_body(42);
        body_edge.apply_shape(&BodyShape::GUITAR_SMALL);
        body_edge.set_position(0.1);

        let mut body_mid = make_body(42);
        body_mid.apply_shape(&BodyShape::GUITAR_SMALL);
        body_mid.set_position(0.5);

        let mut out_edge = [0.0f32; BLOCK];
        let mut out_mid = [0.0f32; BLOCK];

        body_edge.process(&inp, &mut out_edge);
        body_mid.process(&inp, &mut out_mid);

        for _ in 0..5 {
            body_edge.process(&silent, &mut out_edge);
            body_mid.process(&silent, &mut out_mid);
        }

        let hf_edge: f32 = out_edge.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
        let hf_mid: f32 = out_mid.windows(2).map(|w| (w[1] - w[0]).abs()).sum();

        assert!(
            hf_edge > hf_mid,
            "Edge position should be brighter: edge={hf_edge:.4}, mid={hf_mid:.4}"
        );
    }

    // ── Safety ──

    #[test]
    fn output_stays_bounded() {
        let mut body = make_body(42);
        body.apply_shape(&BodyShape::BELL);

        let loud = [1.0f32; BLOCK];
        let mut out = [0.0f32; BLOCK];

        for _ in 0..50 {
            body.process(&loud, &mut out);
        }

        let p = peak(&out);
        assert!(
            p < 5.0,
            "Sustained loud input should not explode, got peak {p}"
        );
    }

    #[test]
    fn reset_silences() {
        let mut body = make_body(42);
        body.apply_shape(&BodyShape::GUITAR_SMALL);

        let mut inp = [0.0f32; BLOCK];
        inp[0] = 1.0;
        let mut out = [0.0f32; BLOCK];
        body.process(&inp, &mut out);

        body.reset();

        let silent = [0.0f32; BLOCK];
        body.process(&silent, &mut out);
        assert!(peak(&out) < 1e-6, "After reset, body should be silent");
    }

    // ── Warmth ──

    #[test]
    fn low_modes_always_boosted() {
        let body = make_body(42);
        // The first 3 modes should have higher gain than mode 4+
        assert!(
            body.modes[0].gain > body.modes[4].gain,
            "Mode 0 should be louder than mode 4"
        );
        assert!(
            body.modes[1].gain > body.modes[5].gain,
            "Mode 1 should be louder than mode 5"
        );
    }

    // ── Fast cos accuracy ──

    #[test]
    fn fast_cos_reasonable_accuracy() {
        // Test at known values
        let cases = [
            (0.0, 1.0),
            (PI * 0.5, 0.0),
            (PI, -1.0),
            (PI * 1.5, 0.0),
        ];
        for (x, expected) in cases {
            let got = fast_cos(x);
            assert!(
                (got - expected).abs() < 0.02,
                "fast_cos({x:.3}) = {got:.4}, expected {expected:.4}"
            );
        }
    }
}
