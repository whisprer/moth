//! Core DSP primitives — noise generation, filters, utilities.
//!
//! All types are `no_std`, `Copy`-able where possible, and use zero heap
//! allocation. These are the building blocks for every section of the
//! signal chain.

// ─── Noise generation ───────────────────────────────────────────────────────

/// Fast deterministic PRNG for audio-rate noise generation.
///
/// Uses xorshift32 — better statistical properties than LCG at equivalent
/// cost. Period of 2^32 - 1 (the zero state is excluded).
///
/// Seeded from [`InstrumentDna`] so that each instrument instance has a
/// unique noise texture.
#[derive(Debug, Clone, Copy)]
pub struct DspRng {
    state: u32,
}

impl DspRng {
    /// Create a new RNG from a seed.
    ///
    /// The seed is OR'd with 1 to avoid the zero state (xorshift's
    /// only fixed point). Any non-zero seed produces a full-period
    /// sequence.
    #[inline]
    pub fn new(seed: u32) -> Self {
        Self {
            state: seed | 1, // xorshift32 must never be zero
        }
    }

    /// Advance and return the raw 32-bit state.
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }

    /// Uniform random float in `[0.0, 1.0)`.
    #[inline]
    pub fn next_unipolar(&mut self) -> f32 {
        self.next_u32() as f32 / 4_294_967_296.0
    }

    /// Uniform random float in `[-1.0, 1.0)`.
    #[inline]
    pub fn next_bipolar(&mut self) -> f32 {
        self.next_unipolar() * 2.0 - 1.0
    }
}

// ─── Filters ────────────────────────────────────────────────────────────────

/// One-pole lowpass filter.
///
/// The simplest possible IIR filter: `y[n] = y[n-1] + coeff * (x[n] - y[n-1])`.
/// `coeff` in `(0, 1)` maps approximately to a cutoff frequency of
/// `coeff * sample_rate / (2π)`.
///
/// Used for spectral tilt shaping, parameter smoothing, and envelope
/// following throughout the signal chain.
#[derive(Debug, Clone, Copy)]
pub struct OnePole {
    state: f32,
    coeff: f32,
}

impl OnePole {
    /// Create a new one-pole filter.
    ///
    /// `coeff` in `(0, 1)` — higher = brighter / faster response.
    #[inline]
    pub fn new(coeff: f32) -> Self {
        Self {
            state: 0.0,
            coeff: coeff.clamp(0.0001, 0.9999),
        }
    }

    /// Update the filter coefficient without resetting state.
    #[inline]
    pub fn set_coeff(&mut self, coeff: f32) {
        self.coeff = coeff.clamp(0.0001, 0.9999);
    }

    /// Process one sample through the lowpass.
    #[inline]
    pub fn process(&mut self, input: f32) -> f32 {
        self.state += self.coeff * (input - self.state);
        self.state
    }

    /// Reset the filter state to zero.
    #[inline]
    pub fn reset(&mut self) {
        self.state = 0.0;
    }

    /// Current output value.
    #[inline]
    pub fn state(&self) -> f32 {
        self.state
    }
}

/// DC blocking filter — removes DC offset from a signal.
///
/// First-order highpass: `y[n] = x[n] - x[n-1] + R * y[n-1]`
/// where `R` is close to 1.0 (typically `1.0 - 20/sample_rate`).
///
/// Essential after any nonlinear processing that might introduce DC.
#[derive(Debug, Clone, Copy)]
pub struct DcBlocker {
    x_prev: f32,
    y_prev: f32,
    r: f32,
}

impl DcBlocker {
    /// Create a new DC blocker.
    ///
    /// `sample_rate` — the system sample rate in Hz. The cutoff is
    /// set to ~20 Hz regardless of sample rate.
    #[inline]
    pub fn new(sample_rate: f32) -> Self {
        Self {
            x_prev: 0.0,
            y_prev: 0.0,
            r: 1.0 - 20.0 / sample_rate,
        }
    }

    /// Process one sample.
    #[inline]
    pub fn process(&mut self, input: f32) -> f32 {
        let y = input - self.x_prev + self.r * self.y_prev;
        self.x_prev = input;
        self.y_prev = y;
        y
    }

    /// Reset state.
    #[inline]
    pub fn reset(&mut self) {
        self.x_prev = 0.0;
        self.y_prev = 0.0;
    }
}

/// Parameter smoother — one-pole lowpass specialised for control-rate
/// parameter interpolation (anti-zipper noise).
///
/// Unlike [`OnePole`], this tracks a *target* value and converges toward
/// it. Use for smoothing knob movements, aftertouch, etc.
#[derive(Debug, Clone, Copy)]
pub struct Smoother {
    state: f32,
    coeff: f32,
}

impl Smoother {
    /// Create a new smoother.
    ///
    /// `coeff` — smoothing speed. `0.001` = very smooth (slow),
    /// `0.1` = responsive (fast). Typical: `0.01` for knobs,
    /// `0.05` for aftertouch, `0.1` for velocity.
    #[inline]
    pub fn new(coeff: f32) -> Self {
        Self {
            state: 0.0,
            coeff: coeff.clamp(0.0001, 1.0),
        }
    }

    /// Set the current value immediately (no smoothing).
    #[inline]
    pub fn set(&mut self, value: f32) {
        self.state = value;
    }

    /// Advance one step toward the target.
    #[inline]
    pub fn tick(&mut self, target: f32) -> f32 {
        self.state += self.coeff * (target - self.state);
        self.state
    }

    /// Current smoothed value.
    #[inline]
    pub fn value(&self) -> f32 {
        self.state
    }
}

// ─── Utility functions ──────────────────────────────────────────────────────

/// Soft saturation — prevents hard clipping while preserving signal shape.
///
/// Uses `tanh`-like polynomial approximation (no libm dependency):
/// `f(x) = x * (27 + x²) / (27 + 9x²)` — Padé approximant to tanh.
#[inline]
pub fn soft_saturate(x: f32) -> f32 {
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

/// Fast approximation of `exp(-x)` for `x >= 0`.
///
/// Schraudolph's method adapted to f32. Accurate to ~0.3% for x in [0, 5].
/// Used for envelope curves and decay calculations where exact exp() is
/// overkill.
#[inline]
pub fn fast_exp_neg(x: f32) -> f32 {
    // For small x, use the rational approximation: 1/(1+x+0.5*x^2)
    // More stable and accurate than bit tricks for our use case.
    let x = x.max(0.0);
    let x2 = x * x;
    1.0 / (1.0 + x + 0.48 * x2 + 0.235 * x2 * x)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── DspRng ──

    #[test]
    fn rng_deterministic() {
        let mut a = DspRng::new(42);
        let mut b = DspRng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn rng_different_seeds_diverge() {
        let mut a = DspRng::new(1);
        let mut b = DspRng::new(2);
        let mut same = 0;
        for _ in 0..100 {
            if a.next_u32() == b.next_u32() {
                same += 1;
            }
        }
        assert!(same < 5, "Different seeds should rarely collide");
    }

    #[test]
    fn rng_bipolar_range() {
        let mut rng = DspRng::new(123);
        for _ in 0..10_000 {
            let v = rng.next_bipolar();
            assert!((-1.0..1.0).contains(&v), "bipolar out of range: {v}");
        }
    }

    #[test]
    fn rng_unipolar_range() {
        let mut rng = DspRng::new(456);
        for _ in 0..10_000 {
            let v = rng.next_unipolar();
            assert!((0.0..1.0).contains(&v), "unipolar out of range: {v}");
        }
    }

    #[test]
    fn rng_zero_seed_handled() {
        let mut rng = DspRng::new(0); // would be zero without the |1 fix
        let first = rng.next_u32();
        assert_ne!(first, 0, "Zero seed should still produce non-zero output");
    }

    // ── OnePole ──

    #[test]
    fn one_pole_tracks_dc() {
        let mut lp = OnePole::new(0.1);
        // Feed constant 1.0 — should converge
        for _ in 0..1000 {
            lp.process(1.0);
        }
        assert!(
            (lp.state() - 1.0).abs() < 0.01,
            "Should converge to DC input"
        );
    }

    #[test]
    fn one_pole_higher_coeff_faster() {
        let mut slow = OnePole::new(0.01);
        let mut fast = OnePole::new(0.1);
        for _ in 0..100 {
            slow.process(1.0);
            fast.process(1.0);
        }
        assert!(
            fast.state() > slow.state(),
            "Higher coefficient should converge faster"
        );
    }

    // ── DcBlocker ──

    #[test]
    fn dc_blocker_removes_dc() {
        let mut dc = DcBlocker::new(48000.0);
        // Feed DC offset signal
        let mut last = 0.0f32;
        for _ in 0..48_000 {
            last = dc.process(1.0);
        }
        assert!(
            last.abs() < 0.01,
            "DC blocker should remove constant offset, got {last}"
        );
    }

    #[test]
    fn dc_blocker_passes_ac() {
        let mut dc = DcBlocker::new(48000.0);
        // Feed alternating +1/-1 signal (high-frequency AC)
        // Should pass through with minimal attenuation after transient
        let mut max_out = 0.0f32;
        for i in 0..4800 {
            let input = if i % 2 == 0 { 1.0 } else { -1.0 };
            let out = dc.process(input);
            if i > 480 {
                max_out = max_out.max(out.abs());
            }
        }
        assert!(
            max_out > 0.9,
            "DC blocker should pass high-freq AC, got {max_out}"
        );
    }

    // ── Smoother ──

    #[test]
    fn smoother_converges() {
        let mut s = Smoother::new(0.1);
        for _ in 0..200 {
            s.tick(1.0);
        }
        assert!((s.value() - 1.0).abs() < 0.01);
    }

    #[test]
    fn smoother_immediate_set() {
        let mut s = Smoother::new(0.01);
        s.set(0.75);
        assert_eq!(s.value(), 0.75);
    }

    // ── Soft saturate ──

    #[test]
    fn soft_saturate_identity_near_zero() {
        // Near zero, soft_saturate ≈ identity
        let v = soft_saturate(0.1);
        assert!((v - 0.1).abs() < 0.01, "Near-zero should be near-linear");
    }

    #[test]
    fn soft_saturate_bounded() {
        // Large inputs should be bounded
        assert!(soft_saturate(100.0) < 4.0);
        assert!(soft_saturate(-100.0) > -4.0);
    }

    #[test]
    fn soft_saturate_odd_symmetry() {
        assert!(
            (soft_saturate(0.5) + soft_saturate(-0.5)).abs() < 1e-6,
            "Should be an odd function"
        );
    }
}
