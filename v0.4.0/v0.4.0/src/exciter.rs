//! Continuous exciter parameter space — NO enums, NO named categories.
//!
//! Physical excitation mechanisms exist on a continuum. A bow is not a
//! fundamentally different *thing* from a pluck — it's friction-dominated
//! continuous energy transfer vs impulse-dominated direct injection.
//!
//! This module parameterises that continuum directly via [`ExciterModel`].
//! Named presets ([`ExciterModel::PLUCK`], [`ExciterModel::BOW`], etc.)
//! are convenience bookmarks in the continuous space, not ontological
//! categories. The morphing system interpolates between any two states
//! by lerping every field independently.
//!
//! # Coupling Axes
//!
//! The three coupling modes (`coupling_direct`, `coupling_friction`,
//! `coupling_pressure`) are **independent** `[0.0, 1.0]` axes — they
//! do NOT sum to 1.0. This allows hybrid excitations that layer
//! mechanisms at full strength simultaneously (e.g. col legno tratto:
//! high friction + significant direct contact). Total energy normalisation
//! happens at the output stage, not in the model definition.

/// The continuous exciter parameter space.
///
/// Every field is a `f32` in `[0.0, 1.0]` (except `multiplicity: u8`).
/// Any combination of values is valid — there are no illegal states.
/// The morphing system can lerp between any two `ExciterModel` instances.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExciterModel {
    /// Temporal energy profile.
    ///
    /// `0.0` = pure impulse — all energy delivered at the contact instant,
    /// then the exciter disengages. The vibrator rings freely.
    ///
    /// `1.0` = pure continuous — energy is sustained for the duration of
    /// the gesture. The exciter remains coupled to the vibrator.
    ///
    /// Intermediate values produce hybrid behaviours: a struck bell that's
    /// then lightly held (`0.2`), or a bowed string with percussive attack (`0.8`).
    pub energy_continuity: f32,

    /// Direct mechanical energy injection — strength of displacement coupling.
    ///
    /// Models: pluck, hammer, mallet — direct physical displacement of the
    /// vibrator. Implemented as a shaped noise burst injected into the
    /// waveguide's initial conditions (extended Karplus-Strong).
    ///
    /// `0.0` = no direct injection. `1.0` = full strength.
    /// **Independent** of friction and pressure axes.
    pub coupling_direct: f32,

    /// Friction (stick-slip) energy coupling strength.
    ///
    /// Models: bow, singing bowl rim, wine glass edge, friction drum.
    /// Nonlinear friction function of relative velocity — when exciter
    /// speed relative to vibrator speed → 0, stick; when force exceeds
    /// static friction threshold, slip. Produces sawtooth-like energy
    /// injection.
    ///
    /// `0.0` = no friction coupling. `1.0` = full strength.
    /// **Independent** of direct and pressure axes.
    pub coupling_friction: f32,

    /// Pressure (airflow) energy coupling strength.
    ///
    /// Models: clarinet reed, flute air-jet, brass embouchure, organ pipe.
    /// Nonlinear reed-opening function × mouth pressure → flow equation.
    /// Self-oscillates above threshold.
    ///
    /// `0.0` = no pressure coupling. `1.0` = full strength.
    /// **Independent** of direct and friction axes.
    pub coupling_pressure: f32,

    /// Material hardness / spectral tilt of the exciter.
    ///
    /// Controls the bandwidth of the excitation signal — how much
    /// high-frequency energy the exciter injects.
    ///
    /// `0.00` = soft, warm — fingertip pluck, felt mallet, cotton bow.
    /// `0.25` = springy, flexible — horsehair bow, soft piano hammer, brush.
    /// `0.50` = medium — tongued breath, soft plectrum, wooden mallet.
    /// `0.75` = firm — hard guitar pick, wooden drumstick.
    /// `1.00` = hard, bright — metal beater, harpsichord quill, wire brush.
    ///
    /// This parameter shapes:
    /// - Impulse bandwidth for direct injection
    /// - Friction curve sharpness for stick-slip coupling
    /// - Reed-opening attack profile for pressure coupling
    ///
    /// **DNA interaction:** the instrument's `ExciterDna.spectral_tilt_bias`
    /// is a subtle multiplier on this value, so two instruments at the same
    /// `spectral_tilt` setting will differ slightly in brightness.
    pub spectral_tilt: f32,

    /// Amount of randomness / turbulence in the excitation signal.
    ///
    /// `0.0` = perfectly deterministic excitation — clean, precise.
    /// `0.5` = moderate turbulence — breathy, organic.
    /// `1.0` = fully stochastic — granular, rain-on-surface.
    ///
    /// Stochasticity affects multiple noise injection points simultaneously,
    /// with the coupling mode determining *which* injection topology dominates:
    /// - Direct coupling: timing jitter + spectral noise in the impulse
    /// - Friction coupling: bow pressure fluctuation + position wander
    /// - Pressure coupling: turbulence noise mixed into airflow
    ///
    /// **DNA interaction:** the instrument's `ExciterDna.stochastic_bias`
    /// and `noise_phase_offset` shape the *character* of the randomness.
    /// This parameter controls only *how much*.
    pub stochasticity: f32,

    /// Number of simultaneous excitation contact points.
    ///
    /// `1` = single contact point (normal play — one pluck, one bow).
    /// `2–6` = strum / chord spread — multiple sequential impulses with
    ///   position spread across the vibrator.
    /// `8–64` = granular cloud / rain / hail — many simultaneous micro-events.
    ///
    /// For multiplicity > 1, the individual events are spread in time
    /// (determined by `energy_continuity` — impulse = rapid spread,
    /// continuous = sustained cloud) and position (determined by
    /// `PlayGesture.position` as centre, with spread width proportional
    /// to `stochasticity`).
    pub multiplicity: u8,
}

impl ExciterModel {
    // ── Named presets ────────────────────────────────────────────────
    //
    // These are bookmarks in the continuous space, not an enum.
    // Every value between any two presets is musically valid.

    /// Plucked string — finger pluck, clean and warm.
    pub const PLUCK: Self = Self {
        energy_continuity: 0.0,
        coupling_direct: 1.0,
        coupling_friction: 0.0,
        coupling_pressure: 0.0,
        spectral_tilt: 0.15,
        stochasticity: 0.05,
        multiplicity: 1,
    };

    /// Plucked string — hard pick, bright and snappy.
    pub const PICK: Self = Self {
        energy_continuity: 0.0,
        coupling_direct: 1.0,
        coupling_friction: 0.0,
        coupling_pressure: 0.0,
        spectral_tilt: 0.80,
        stochasticity: 0.03,
        multiplicity: 1,
    };

    /// Bowed string — classical bow, sustained friction.
    pub const BOW: Self = Self {
        energy_continuity: 1.0,
        coupling_direct: 0.0,
        coupling_friction: 1.0,
        coupling_pressure: 0.0,
        spectral_tilt: 0.30,
        stochasticity: 0.01,
        multiplicity: 1,
    };

    /// Blown tube — clarinet/oboe reed, sustained pressure.
    pub const BREATH: Self = Self {
        energy_continuity: 1.0,
        coupling_direct: 0.0,
        coupling_friction: 0.0,
        coupling_pressure: 1.0,
        spectral_tilt: 0.40,
        stochasticity: 0.02,
        multiplicity: 1,
    };

    /// Breathy flute — air-jet, more turbulent than reed.
    pub const FLUTE: Self = Self {
        energy_continuity: 1.0,
        coupling_direct: 0.0,
        coupling_friction: 0.0,
        coupling_pressure: 0.85,
        spectral_tilt: 0.25,
        stochasticity: 0.15,
        multiplicity: 1,
    };

    /// Hammer / mallet — percussive strike, felt-wrapped.
    pub const MALLET: Self = Self {
        energy_continuity: 0.0,
        coupling_direct: 1.0,
        coupling_friction: 0.0,
        coupling_pressure: 0.0,
        spectral_tilt: 0.10,
        stochasticity: 0.0,
        multiplicity: 1,
    };

    /// Hard beater — metallic, bright percussive strike.
    pub const BEATER: Self = Self {
        energy_continuity: 0.0,
        coupling_direct: 1.0,
        coupling_friction: 0.0,
        coupling_pressure: 0.0,
        spectral_tilt: 1.0,
        stochasticity: 0.0,
        multiplicity: 1,
    };

    /// Guitar strum — 6 strings, slight timing spread.
    pub const STRUM: Self = Self {
        energy_continuity: 0.0,
        coupling_direct: 1.0,
        coupling_friction: 0.0,
        coupling_pressure: 0.0,
        spectral_tilt: 0.50,
        stochasticity: 0.10,
        multiplicity: 6,
    };

    /// E-bow — continuous direct electromagnetic excitation.
    /// No friction, no pressure — pure sustained displacement.
    pub const EBOW: Self = Self {
        energy_continuity: 1.0,
        coupling_direct: 1.0,
        coupling_friction: 0.0,
        coupling_pressure: 0.0,
        spectral_tilt: 0.20,
        stochasticity: 0.0,
        multiplicity: 1,
    };

    /// Singing bowl — rim friction with some pressure coupling.
    /// Hybrid: continuous friction + secondary pressure resonance.
    pub const SINGING_BOWL: Self = Self {
        energy_continuity: 1.0,
        coupling_direct: 0.0,
        coupling_friction: 0.6,
        coupling_pressure: 0.4,
        spectral_tilt: 0.20,
        stochasticity: 0.0,
        multiplicity: 1,
    };

    /// Col legno tratto — bowing with the wood of the bow.
    /// Hybrid: friction + direct contact simultaneously.
    pub const COL_LEGNO: Self = Self {
        energy_continuity: 0.9,
        coupling_direct: 0.5,
        coupling_friction: 0.8,
        coupling_pressure: 0.0,
        spectral_tilt: 0.70,
        stochasticity: 0.05,
        multiplicity: 1,
    };

    /// Granular rain — many small stochastic impulses.
    pub const RAIN: Self = Self {
        energy_continuity: 0.3,
        coupling_direct: 1.0,
        coupling_friction: 0.0,
        coupling_pressure: 0.0,
        spectral_tilt: 0.60,
        stochasticity: 0.95,
        multiplicity: 32,
    };

    // ── Morphing ─────────────────────────────────────────────────────

    /// Linearly interpolate between two exciter models.
    ///
    /// `t = 0.0` → `self`, `t = 1.0` → `other`.
    ///
    /// This is the core of the morphing system — the instrument can
    /// continuously evolve from a bowed string to a blown tube to a
    /// granular cloud by sweeping `t` through time.
    ///
    /// `multiplicity` is interpolated as a float and rounded, so a morph
    /// from `PLUCK` (mult=1) to `RAIN` (mult=32) smoothly increases the
    /// number of simultaneous events.
    #[inline]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let lerp_f = |a: f32, b: f32| -> f32 { a + (b - a) * t };

        let mult_f =
            self.multiplicity as f32 + (other.multiplicity as f32 - self.multiplicity as f32) * t;

        Self {
            energy_continuity: lerp_f(self.energy_continuity, other.energy_continuity),
            coupling_direct: lerp_f(self.coupling_direct, other.coupling_direct),
            coupling_friction: lerp_f(self.coupling_friction, other.coupling_friction),
            coupling_pressure: lerp_f(self.coupling_pressure, other.coupling_pressure),
            spectral_tilt: lerp_f(self.spectral_tilt, other.spectral_tilt),
            stochasticity: lerp_f(self.stochasticity, other.stochasticity),
            multiplicity: ((mult_f + 0.5) as u8).max(1),
        }
    }

    /// Returns the total coupling energy (sum of all three axes).
    ///
    /// Useful for output-stage normalisation — scale the exciter output
    /// by `1.0 / total_coupling().max(1.0)` to prevent energy spikes
    /// when multiple coupling modes are at full strength.
    #[inline]
    pub fn total_coupling(&self) -> f32 {
        self.coupling_direct + self.coupling_friction + self.coupling_pressure
    }

    /// Clamp all continuous fields to `[0.0, 1.0]`, multiplicity to `[1, 255]`.
    #[inline]
    pub fn clamped(self) -> Self {
        Self {
            energy_continuity: self.energy_continuity.clamp(0.0, 1.0),
            coupling_direct: self.coupling_direct.clamp(0.0, 1.0),
            coupling_friction: self.coupling_friction.clamp(0.0, 1.0),
            coupling_pressure: self.coupling_pressure.clamp(0.0, 1.0),
            spectral_tilt: self.spectral_tilt.clamp(0.0, 1.0),
            stochasticity: self.stochasticity.clamp(0.0, 1.0),
            multiplicity: self.multiplicity.max(1),
        }
    }
}

impl Default for ExciterModel {
    /// Default is a gentle finger pluck — the most universal starting point.
    fn default() -> Self {
        Self::PLUCK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All presets must have valid field ranges.
    #[test]
    fn all_presets_in_range() {
        let presets = [
            ("PLUCK", ExciterModel::PLUCK),
            ("PICK", ExciterModel::PICK),
            ("BOW", ExciterModel::BOW),
            ("BREATH", ExciterModel::BREATH),
            ("FLUTE", ExciterModel::FLUTE),
            ("MALLET", ExciterModel::MALLET),
            ("BEATER", ExciterModel::BEATER),
            ("STRUM", ExciterModel::STRUM),
            ("EBOW", ExciterModel::EBOW),
            ("SINGING_BOWL", ExciterModel::SINGING_BOWL),
            ("COL_LEGNO", ExciterModel::COL_LEGNO),
            ("RAIN", ExciterModel::RAIN),
        ];

        for (name, preset) in &presets {
            assert!(
                (0.0..=1.0).contains(&preset.energy_continuity),
                "{name}: energy_continuity out of range"
            );
            assert!(
                (0.0..=1.0).contains(&preset.coupling_direct),
                "{name}: coupling_direct out of range"
            );
            assert!(
                (0.0..=1.0).contains(&preset.coupling_friction),
                "{name}: coupling_friction out of range"
            );
            assert!(
                (0.0..=1.0).contains(&preset.coupling_pressure),
                "{name}: coupling_pressure out of range"
            );
            assert!(
                (0.0..=1.0).contains(&preset.spectral_tilt),
                "{name}: spectral_tilt out of range"
            );
            assert!(
                (0.0..=1.0).contains(&preset.stochasticity),
                "{name}: stochasticity out of range"
            );
            assert!(
                preset.multiplicity >= 1,
                "{name}: multiplicity must be >= 1"
            );
        }
    }

    /// Impulse presets should have energy_continuity == 0.
    #[test]
    fn impulse_presets_are_impulsive() {
        assert_eq!(ExciterModel::PLUCK.energy_continuity, 0.0);
        assert_eq!(ExciterModel::PICK.energy_continuity, 0.0);
        assert_eq!(ExciterModel::MALLET.energy_continuity, 0.0);
        assert_eq!(ExciterModel::BEATER.energy_continuity, 0.0);
        assert_eq!(ExciterModel::STRUM.energy_continuity, 0.0);
    }

    /// Continuous presets should have energy_continuity == 1.0.
    #[test]
    fn continuous_presets_are_continuous() {
        assert_eq!(ExciterModel::BOW.energy_continuity, 1.0);
        assert_eq!(ExciterModel::BREATH.energy_continuity, 1.0);
        assert_eq!(ExciterModel::EBOW.energy_continuity, 1.0);
        assert_eq!(ExciterModel::SINGING_BOWL.energy_continuity, 1.0);
    }

    /// Morphing at t=0 returns self, at t=1 returns other.
    #[test]
    fn lerp_endpoints() {
        let a = ExciterModel::PLUCK;
        let b = ExciterModel::BOW;

        let at_a = a.lerp(b, 0.0);
        assert_eq!(at_a.energy_continuity, a.energy_continuity);
        assert_eq!(at_a.coupling_direct, a.coupling_direct);
        assert_eq!(at_a.multiplicity, a.multiplicity);

        let at_b = a.lerp(b, 1.0);
        assert_eq!(at_b.energy_continuity, b.energy_continuity);
        assert_eq!(at_b.coupling_friction, b.coupling_friction);
        assert_eq!(at_b.multiplicity, b.multiplicity);
    }

    /// Morphing midpoint should be the average.
    #[test]
    fn lerp_midpoint() {
        let a = ExciterModel::PLUCK; // continuity=0, direct=1, friction=0
        let b = ExciterModel::BOW; // continuity=1, direct=0, friction=1

        let mid = a.lerp(b, 0.5);
        assert!((mid.energy_continuity - 0.5).abs() < 1e-6);
        assert!((mid.coupling_direct - 0.5).abs() < 1e-6);
        assert!((mid.coupling_friction - 0.5).abs() < 1e-6);
    }

    /// Multiplicity morphing: PLUCK(1) → RAIN(32) should increase smoothly.
    #[test]
    fn lerp_multiplicity() {
        let a = ExciterModel::PLUCK; // mult=1
        let b = ExciterModel::RAIN; // mult=32

        let at_quarter = a.lerp(b, 0.25);
        let at_half = a.lerp(b, 0.5);
        let at_three_quarter = a.lerp(b, 0.75);

        assert!(at_quarter.multiplicity >= 1);
        assert!(at_half.multiplicity > at_quarter.multiplicity);
        assert!(at_three_quarter.multiplicity > at_half.multiplicity);
        assert_eq!(a.lerp(b, 1.0).multiplicity, 32);
    }

    /// t is clamped — out-of-range values don't produce garbage.
    #[test]
    fn lerp_clamps_t() {
        let a = ExciterModel::PLUCK;
        let b = ExciterModel::BOW;

        let under = a.lerp(b, -1.0);
        assert_eq!(under.energy_continuity, a.energy_continuity);

        let over = a.lerp(b, 2.0);
        assert_eq!(over.energy_continuity, b.energy_continuity);
    }

    /// Total coupling is the sum of all three axes.
    #[test]
    fn total_coupling_sum() {
        let model = ExciterModel::SINGING_BOWL;
        let expected = model.coupling_direct + model.coupling_friction + model.coupling_pressure;
        assert!((model.total_coupling() - expected).abs() < 1e-6);
    }

    /// Coupling axes are independent — hybrid presets prove it.
    #[test]
    fn coupling_axes_independent() {
        // Col legno has both direct AND friction coupling simultaneously
        let cl = ExciterModel::COL_LEGNO;
        assert!(cl.coupling_direct > 0.0);
        assert!(cl.coupling_friction > 0.0);
        assert!(
            cl.total_coupling() > 1.0,
            "Hybrid coupling should exceed 1.0"
        );

        // Singing bowl has friction AND pressure
        let sb = ExciterModel::SINGING_BOWL;
        assert!(sb.coupling_friction > 0.0);
        assert!(sb.coupling_pressure > 0.0);
    }

    /// Default is PLUCK.
    #[test]
    fn default_is_pluck() {
        assert_eq!(ExciterModel::default(), ExciterModel::PLUCK);
    }

    /// Spectral tilt differentiates soft from hard exciters.
    #[test]
    fn spectral_tilt_ordering() {
        assert!(ExciterModel::MALLET.spectral_tilt < ExciterModel::PICK.spectral_tilt);
        assert!(ExciterModel::PICK.spectral_tilt < ExciterModel::BEATER.spectral_tilt);
        assert!(ExciterModel::PLUCK.spectral_tilt < ExciterModel::PICK.spectral_tilt);
    }

    #[test]
    fn copy_and_clone() {
        let m = ExciterModel::BOW;
        let _copy = m;
        let _clone = m.clone();
        assert_eq!(_copy, _clone);
    }
}
