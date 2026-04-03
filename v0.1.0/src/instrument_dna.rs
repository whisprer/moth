//! Deterministic per-instrument personality derived from a unique hardware seed.
//!
//! # Design Philosophy
//!
//! Inspired by Mutable Instruments Elements' `Part::Seed()` system (Émilie Gillet).
//! A single `u32` — typically the MCU's silicon-fused unique device ID, read once
//! at boot — deterministically produces a set of bounded parameters that give each
//! physical instrument instance its own timbral character.
//!
//! **The core guarantee:** every derived value lives within a musically-vetted
//! sweet-spot range. The instrument cannot sound *bad*, only *different*. Two units
//! with identical knob positions will produce subtly distinct timbres — like two
//! acoustic guitars built from the same plans but different pieces of wood.
//!
//! # Derivation
//!
//! Uses the same LCG constants as Elements (Knuth's multiplicative congruential):
//! `state = state * 1664525 + 1013904223`, seeded by XOR with `0xf0cacc1a`.
//! Each parameter is pulled in a **fixed order** — adding new parameters at the
//! end preserves backward compatibility (existing instruments keep their personality
//! when firmware is updated with new DNA traits).
//!
//! # Ranges
//!
//! Each derived parameter documents its sweet-spot range and the musical rationale
//! for those bounds. Ranges were chosen so that *every possible value within them
//! produces a musically valid result* when combined with any other derived values.
//! This is the "all settings are sweet spots" principle.

/// Deterministic PRNG for seed derivation.
///
/// Uses Knuth's LCG (same constants as Mutable Instruments Elements).
/// Not cryptographically secure — that's not the point. The point is
/// determinism: same seed → same sequence → same instrument personality,
/// every boot, forever.
#[derive(Debug, Clone)]
struct DnaRng {
    state: u32,
}

impl DnaRng {
    /// Create a new RNG from a hardware seed.
    ///
    /// The seed is XOR'd with `0xf0cacc1a` (Émilie's "focaccia" constant)
    /// and then advanced once through the LCG to diffuse the initial bits.
    /// This ensures that even sequential serial numbers (common in production
    /// runs) produce well-separated initial states.
    fn new(seed: u32) -> Self {
        let state = seed ^ 0xf0ca_cc1a;
        let state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        Self { state }
    }

    /// Advance the LCG and return a normalised `f32` in `[0.0, 1.0)`.
    ///
    /// Uses the upper 24 bits of the state for better distribution
    /// (lower bits of LCGs have shorter periods).
    fn next_unit(&mut self) -> f32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.state >> 8) as f32 / 16_777_216.0
    }

    /// Advance the LCG and return an `f32` mapped to `[min, max)`.
    fn next_in_range(&mut self, min: f32, max: f32) -> f32 {
        min + self.next_unit() * (max - min)
    }

    /// Advance the LCG and return the raw `u32` state.
    ///
    /// Useful for parameters that need integer entropy (e.g. wavetable offsets,
    /// bit patterns for noise shaping).
    fn next_raw(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
}

// ─── Derived parameter groups ───────────────────────────────────────────────

/// Exciter-section DNA: how the instrument is *played*.
///
/// These parameters shape the character of energy injection — pluck snap,
/// bow grain, breath turbulence texture — without changing the *type* of
/// excitation. Two instruments with identical `ExciterModel` settings but
/// different DNA will feel subtly different under the fingers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExciterDna {
    /// General exciter personality. Affects coupling curve shapes,
    /// pluck displacement ratios, friction curve asymmetry.
    ///
    /// Range: `[0.0, 1.0)` — full unit range is safe here because each
    /// exciter model interprets this as a *modifier* on already-bounded
    /// internal parameters (see Elements' `ProcessPlectrum`: signature
    /// scales an additive term within `0.05 + sig * 0.2`, so even at
    /// sig=1.0 the total multiplier is only 0.25).
    pub signature: f32,

    /// Phase offset into noise/turbulence generation.
    ///
    /// Range: `[0.0, 1.0)` — maps to a position in the noise buffer.
    /// Different offsets produce different micro-texture in breath,
    /// granular, and stochastic exciter modes. Like Elements'
    /// `signature_ * 8192.0f` offset into `smp_noise_sample`.
    pub noise_phase_offset: f32,

    /// Asymmetry bias in stochastic processes.
    ///
    /// Range: `[0.42, 0.58]` — a slight lean in probability distributions.
    /// At 0.5 = perfectly symmetric randomness. The narrow range ensures
    /// the bias is *felt* (instruments have character) but never *heard*
    /// as obviously lopsided. Affects particle density distribution,
    /// granular trigger probability skew, flow direction flip bias.
    pub stochastic_bias: f32,

    /// Spectral tilt modifier for the DNA layer.
    ///
    /// Range: `[0.92, 1.08]` — a subtle multiplier on the user-controlled
    /// `spectral_tilt` parameter. Makes one instrument's "hard pick" setting
    /// marginally brighter or darker than another's. Narrow range because
    /// tilt is already a primary user control — DNA should season it, not
    /// override it.
    pub spectral_tilt_bias: f32,

    /// Coupling curve inflection modifier.
    ///
    /// Range: `[0.85, 1.15]` — scales the nonlinearity of friction and
    /// pressure coupling functions. Higher values = sharper stick-slip
    /// transition, more abrupt reed-opening. Changes the *feel* of
    /// continuous excitation without changing its fundamental character.
    pub coupling_curve_shape: f32,
}

/// Vibrator-section DNA: what *vibrates*.
///
/// Subtle per-instance variation in waveguide behaviour — the equivalent
/// of wood grain density variation in a real instrument's string/tube.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VibratorDna {
    /// Fractional sample offset in delay line interpolation.
    ///
    /// Range: `[0.0, 1.0)` — a fixed micro-offset added to waveguide
    /// delay lengths. Produces per-instance inharmonicity variation,
    /// like the bridge placement tolerance on a real instrument.
    pub delay_micro_offset: f32,

    /// Dispersion asymmetry — how unevenly stiffness affects high vs low
    /// partials.
    ///
    /// Range: `[0.95, 1.05]` — multiplier on the stretch factor accumulation
    /// rate. Subtle enough that tuning isn't affected, but the spectral
    /// evolution of each note is unique per unit.
    pub dispersion_asymmetry: f32,
}

/// Resonant body DNA: the acoustic enclosure that colours the sound.
///
/// Per-instance variation in modal synthesis — like the wood grain,
/// glue joint tightness, and air volume of a real instrument body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResonatorDna {
    /// Modal frequency micro-drift.
    ///
    /// Range: `[0.997, 1.003]` — a per-mode multiplier on partial
    /// frequencies. At 1.0 = mathematically perfect modes. The narrow
    /// range produces the subtle "alive" quality of real resonant bodies
    /// where modes aren't quite at their theoretical positions.
    pub modal_drift: f32,

    /// Stereo imaging personality — asymmetry in the pickup/mic simulation.
    ///
    /// Range: `[0.05, 0.15]` — offset for the stereo decorrelation LFO,
    /// directly analogous to Elements' `resonator_modulation_offset`.
    /// Determines where in the virtual body the "stereo microphones" sit.
    pub stereo_offset: f32,

    /// Internal modulation rate — speed of micro-movement within the body.
    ///
    /// Range: `[0.4, 1.2] / sample_rate` — LFO rate for position modulation,
    /// directly analogous to Elements' `resonator_modulation_frequency`.
    /// Faster = more shimmery stereo motion. Slower = more stable image.
    /// All values in this range sound good; it's a character difference,
    /// not a quality difference.
    pub modulation_rate_hz: f32,
}

/// Spatial-section DNA: room, reverb, spatial processing.
///
/// Per-instance variation in the virtual acoustic space surrounding
/// the instrument.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpatialDna {
    /// Reverb diffusion density.
    ///
    /// Range: `[0.55, 0.70]` — directly from Elements' vetted range.
    /// Lower = more discrete echoes. Higher = smoother wash. Both
    /// extremes within this range sound musical.
    pub reverb_diffusion: f32,

    /// Reverb high-frequency damping.
    ///
    /// Range: `[0.70, 0.90]` — how bright/dark the reverb tail is.
    /// From Elements' tested range. Below 0.70 gets muddy; above 0.90
    /// gets metallic. This range is always pleasant.
    pub reverb_brightness: f32,
}

/// Non-linearities section DNA: saturation, colouration, character.
///
/// Per-instance variation in the distortion/saturation/compression
/// character — like component tolerances in analogue circuits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NonLinDna {
    /// Saturation curve asymmetry.
    ///
    /// Range: `[0.93, 1.07]` — ratio between positive and negative
    /// clipping thresholds. At 1.0 = symmetric. Real valve amps and
    /// tape machines have slight asymmetry that produces even-harmonic
    /// warmth. This range is subtle enough to add character without
    /// introducing audible DC offset.
    pub saturation_asymmetry: f32,

    /// Transfer function inflection point shift.
    ///
    /// Range: `[0.95, 1.05]` — where the saturation curve transitions
    /// from linear to compressed. Affects whether the distortion
    /// "blooms" early or late. Like component tolerance in a valve.
    pub transfer_inflection: f32,
}

// ─── The main DNA struct ────────────────────────────────────────────────────

/// Complete per-instrument personality, deterministically derived from a
/// single hardware seed.
///
/// Created once at boot. Immutable thereafter. The instrument's *nature*.
///
/// # Derivation Order
///
/// Parameters are derived in a fixed sequence. **New parameters must only
/// be appended to the end of each section's derivation, and new sections
/// appended after existing ones.** This ensures firmware updates don't
/// change an existing instrument's personality — only extend it.
///
/// Current derivation order:
/// 1. `exciter.signature`
/// 2. `exciter.noise_phase_offset`
/// 3. `exciter.stochastic_bias`
/// 4. `exciter.spectral_tilt_bias`
/// 5. `exciter.coupling_curve_shape`
/// 6. `vibrator.delay_micro_offset`
/// 7. `vibrator.dispersion_asymmetry`
/// 8. `resonator.modal_drift`
/// 9. `resonator.stereo_offset`
/// 10. `resonator.modulation_rate_hz`
/// 11. `spatial.reverb_diffusion`
/// 12. `spatial.reverb_brightness`
/// 13. `non_lin.saturation_asymmetry`
/// 14. `non_lin.transfer_inflection`
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InstrumentDna {
    /// The original hardware seed. Stored for diagnostics/identification only.
    seed: u32,

    /// Exciter-section personality.
    pub exciter: ExciterDna,

    /// Vibrator-section personality.
    pub vibrator: VibratorDna,

    /// Resonant body personality.
    pub resonator: ResonatorDna,

    /// Spatial/reverb personality.
    pub spatial: SpatialDna,

    /// Non-linearity personality.
    pub non_lin: NonLinDna,
}

impl InstrumentDna {
    /// Derive a complete instrument personality from a hardware seed.
    ///
    /// # Arguments
    ///
    /// * `seed` — A `u32` unique to this physical instrument instance.
    ///   On embedded targets this comes from the MCU's unique device ID
    ///   (e.g. STM32 `UID` at `0x1FFF_7A10`, ESP32 `efuse MAC`).
    ///   For desktop/plugin prototyping, use any unique value.
    ///
    /// * `sample_rate` — The system sample rate in Hz. Required because
    ///   some derived parameters (e.g. modulation rates) are stored as
    ///   per-sample increments.
    ///
    /// # Determinism
    ///
    /// Same `seed` + same `sample_rate` → identical `InstrumentDna`, always.
    /// This function is pure — no global state, no system calls, no randomness
    /// beyond what the seed provides.
    pub fn from_seed(seed: u32, sample_rate: f32) -> Self {
        let mut rng = DnaRng::new(seed);

        // ── Exciter section ──
        // Derivation order: signature, noise_phase, stochastic_bias,
        //                   spectral_tilt_bias, coupling_curve_shape

        let exciter = ExciterDna {
            signature: rng.next_unit(),
            noise_phase_offset: rng.next_unit(),
            stochastic_bias: rng.next_in_range(0.42, 0.58),
            spectral_tilt_bias: rng.next_in_range(0.92, 1.08),
            coupling_curve_shape: rng.next_in_range(0.85, 1.15),
        };

        // ── Vibrator section ──
        let vibrator = VibratorDna {
            delay_micro_offset: rng.next_unit(),
            dispersion_asymmetry: rng.next_in_range(0.95, 1.05),
        };

        // ── Resonator section ──
        let resonator = ResonatorDna {
            modal_drift: rng.next_in_range(0.997, 1.003),
            stereo_offset: rng.next_in_range(0.05, 0.15),
            modulation_rate_hz: rng.next_in_range(0.4, 1.2) / sample_rate,
        };

        // ── Spatial section ──
        let spatial = SpatialDna {
            reverb_diffusion: rng.next_in_range(0.55, 0.70),
            reverb_brightness: rng.next_in_range(0.70, 0.90),
        };

        // ── Non-linearities section ──
        let non_lin = NonLinDna {
            saturation_asymmetry: rng.next_in_range(0.93, 1.07),
            transfer_inflection: rng.next_in_range(0.95, 1.05),
        };

        Self {
            seed,
            exciter,
            vibrator,
            resonator,
            spatial,
            non_lin,
        }
    }

    /// Returns the original hardware seed for identification/diagnostics.
    #[inline]
    pub fn seed(&self) -> u32 {
        self.seed
    }

    /// Derive a secondary DNA from this instrument's seed combined with
    /// an additional differentiator.
    ///
    /// Useful for polyphonic instruments where each voice needs its own
    /// micro-variation, or for instruments with multiple vibrating elements
    /// (e.g. a multi-string model where each string gets its own sub-DNA).
    ///
    /// The `voice_index` is mixed into the seed so that voice 0's DNA
    /// differs from voice 1's, but both are deterministic from the
    /// original hardware seed.
    pub fn voice_variant(&self, voice_index: u32, sample_rate: f32) -> Self {
        // Mix voice index into the seed using a different scramble path
        // than the primary derivation. The golden ratio constant provides
        // good bit diffusion for sequential indices.
        let variant_seed = self.seed
            .wrapping_add(voice_index.wrapping_mul(0x9E37_79B9));
        Self::from_seed(variant_seed, sample_rate)
    }
}

// ─── Display ────────────────────────────────────────────────────────────────

impl core::fmt::Display for InstrumentDna {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "InstrumentDna(seed=0x{:08X}) {{\n\
             \x20 exciter:   sig={:.3} noise={:.3} bias={:.3} tilt={:.3} curve={:.3}\n\
             \x20 vibrator:  delay={:.4} dispersion={:.4}\n\
             \x20 resonator: drift={:.5} stereo={:.3} mod_rate={:.6}\n\
             \x20 spatial:   diffusion={:.3} brightness={:.3}\n\
             \x20 non_lin:   sat_asym={:.3} inflection={:.3}\n\
             }}",
            self.seed,
            self.exciter.signature,
            self.exciter.noise_phase_offset,
            self.exciter.stochastic_bias,
            self.exciter.spectral_tilt_bias,
            self.exciter.coupling_curve_shape,
            self.vibrator.delay_micro_offset,
            self.vibrator.dispersion_asymmetry,
            self.resonator.modal_drift,
            self.resonator.stereo_offset,
            self.resonator.modulation_rate_hz,
            self.spatial.reverb_diffusion,
            self.spatial.reverb_brightness,
            self.non_lin.saturation_asymmetry,
            self.non_lin.transfer_inflection,
        )
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SAMPLE_RATE: f32 = 48_000.0;

    /// Core invariant: same seed → identical DNA, always.
    #[test]
    fn determinism() {
        let a = InstrumentDna::from_seed(0xDEAD_BEEF, TEST_SAMPLE_RATE);
        let b = InstrumentDna::from_seed(0xDEAD_BEEF, TEST_SAMPLE_RATE);
        assert_eq!(a, b, "Same seed must produce identical DNA");
    }

    /// Different seeds must produce different DNA.
    #[test]
    fn differentiation() {
        let a = InstrumentDna::from_seed(0x0000_0001, TEST_SAMPLE_RATE);
        let b = InstrumentDna::from_seed(0x0000_0002, TEST_SAMPLE_RATE);
        assert_ne!(a.exciter.signature, b.exciter.signature,
            "Adjacent seeds must produce different signatures");
        assert_ne!(a.resonator.stereo_offset, b.resonator.stereo_offset,
            "Adjacent seeds must produce different stereo offsets");
    }

    /// Sequential serial numbers (production run scenario) must produce
    /// well-separated DNA values, not clustered ones.
    #[test]
    fn sequential_seeds_spread() {
        let dnas: Vec<InstrumentDna> = (0..100)
            .map(|i| InstrumentDna::from_seed(i, TEST_SAMPLE_RATE))
            .collect();

        // Check that signatures span a reasonable portion of [0, 1)
        let sigs: Vec<f32> = dnas.iter().map(|d| d.exciter.signature).collect();
        let min = sigs.iter().cloned().reduce(f32::min).unwrap();
        let max = sigs.iter().cloned().reduce(f32::max).unwrap();
        let spread = max - min;

        assert!(spread > 0.5,
            "100 sequential seeds should spread signatures across >50% of range, got {spread:.3}");
    }

    /// Every derived parameter must land within its documented sweet-spot range.
    #[test]
    fn all_values_in_sweet_spot_ranges() {
        // Test with a variety of seeds including edge cases
        let seeds = [
            0x0000_0000, 0x0000_0001, 0xFFFF_FFFF, 0xFFFF_FFFE,
            0x8000_0000, 0x7FFF_FFFF, 0xDEAD_BEEF, 0xCAFE_BABE,
            0x1234_5678, 0xF0CA_CC1A, // the focaccia constant itself
        ];

        for seed in seeds {
            let dna = InstrumentDna::from_seed(seed, TEST_SAMPLE_RATE);

            // Exciter ranges
            assert!((0.0..1.0).contains(&dna.exciter.signature),
                "seed 0x{seed:08X}: signature {:.4} out of [0,1)", dna.exciter.signature);
            assert!((0.0..1.0).contains(&dna.exciter.noise_phase_offset),
                "seed 0x{seed:08X}: noise_phase {:.4} out of [0,1)", dna.exciter.noise_phase_offset);
            assert!((0.42..=0.58).contains(&dna.exciter.stochastic_bias),
                "seed 0x{seed:08X}: stochastic_bias {:.4} out of [0.42,0.58]", dna.exciter.stochastic_bias);
            assert!((0.92..=1.08).contains(&dna.exciter.spectral_tilt_bias),
                "seed 0x{seed:08X}: spectral_tilt_bias {:.4} out of [0.92,1.08]", dna.exciter.spectral_tilt_bias);
            assert!((0.85..=1.15).contains(&dna.exciter.coupling_curve_shape),
                "seed 0x{seed:08X}: coupling_curve {:.4} out of [0.85,1.15]", dna.exciter.coupling_curve_shape);

            // Vibrator ranges
            assert!((0.0..1.0).contains(&dna.vibrator.delay_micro_offset),
                "seed 0x{seed:08X}: delay_micro {:.4} out of [0,1)", dna.vibrator.delay_micro_offset);
            assert!((0.95..=1.05).contains(&dna.vibrator.dispersion_asymmetry),
                "seed 0x{seed:08X}: dispersion_asym {:.5} out of [0.95,1.05]", dna.vibrator.dispersion_asymmetry);

            // Resonator ranges
            assert!((0.997..=1.003).contains(&dna.resonator.modal_drift),
                "seed 0x{seed:08X}: modal_drift {:.6} out of [0.997,1.003]", dna.resonator.modal_drift);
            assert!((0.05..=0.15).contains(&dna.resonator.stereo_offset),
                "seed 0x{seed:08X}: stereo_offset {:.4} out of [0.05,0.15]", dna.resonator.stereo_offset);
            // modulation_rate_hz is divided by sample_rate, so check the
            // pre-division value by multiplying back up
            let mod_hz = dna.resonator.modulation_rate_hz * TEST_SAMPLE_RATE;
            assert!((0.4..=1.2).contains(&mod_hz),
                "seed 0x{seed:08X}: mod_rate_hz {mod_hz:.4} out of [0.4,1.2]");

            // Spatial ranges
            assert!((0.55..=0.70).contains(&dna.spatial.reverb_diffusion),
                "seed 0x{seed:08X}: reverb_diff {:.4} out of [0.55,0.70]", dna.spatial.reverb_diffusion);
            assert!((0.70..=0.90).contains(&dna.spatial.reverb_brightness),
                "seed 0x{seed:08X}: reverb_bright {:.4} out of [0.70,0.90]", dna.spatial.reverb_brightness);

            // Non-lin ranges
            assert!((0.93..=1.07).contains(&dna.non_lin.saturation_asymmetry),
                "seed 0x{seed:08X}: sat_asym {:.4} out of [0.93,1.07]", dna.non_lin.saturation_asymmetry);
            assert!((0.95..=1.05).contains(&dna.non_lin.transfer_inflection),
                "seed 0x{seed:08X}: inflection {:.4} out of [0.95,1.05]", dna.non_lin.transfer_inflection);
        }
    }

    /// Voice variants must differ from the primary DNA and from each other.
    #[test]
    fn voice_variants_differ() {
        let primary = InstrumentDna::from_seed(0xBEEF_CAFE, TEST_SAMPLE_RATE);
        let v0 = primary.voice_variant(0, TEST_SAMPLE_RATE);
        let v1 = primary.voice_variant(1, TEST_SAMPLE_RATE);
        let v2 = primary.voice_variant(2, TEST_SAMPLE_RATE);

        // All four should be different
        assert_ne!(primary.exciter.signature, v0.exciter.signature);
        assert_ne!(v0.exciter.signature, v1.exciter.signature);
        assert_ne!(v1.exciter.signature, v2.exciter.signature);

        // But all must still be in sweet-spot ranges
        for dna in [&primary, &v0, &v1, &v2] {
            assert!((0.0..1.0).contains(&dna.exciter.signature));
            assert!((0.55..=0.70).contains(&dna.spatial.reverb_diffusion));
        }
    }

    /// Voice variant determinism — same index → same variant, always.
    #[test]
    fn voice_variant_determinism() {
        let primary = InstrumentDna::from_seed(0x1337_C0DE, TEST_SAMPLE_RATE);
        let a = primary.voice_variant(7, TEST_SAMPLE_RATE);
        let b = primary.voice_variant(7, TEST_SAMPLE_RATE);
        assert_eq!(a, b, "Same voice index must produce identical variant DNA");
    }

    /// The seed 0 (all zeros) must not produce degenerate (all-zero or
    /// all-identical) output.
    #[test]
    fn zero_seed_not_degenerate() {
        let dna = InstrumentDna::from_seed(0, TEST_SAMPLE_RATE);
        // signature and noise_phase should be different from each other
        // (they would be identical only if the LCG isn't advancing)
        assert_ne!(dna.exciter.signature, dna.exciter.noise_phase_offset,
            "Zero seed produced identical sequential values — LCG not advancing");
        // Values should not be zero
        assert!(dna.exciter.signature > 0.001,
            "Zero seed produced near-zero signature");
    }

    /// Different sample rates produce different modulation_rate_hz
    /// (since it's stored as per-sample increment) but identical
    /// non-rate-dependent parameters.
    #[test]
    fn sample_rate_affects_only_rate_params() {
        let at_44k = InstrumentDna::from_seed(42, 44_100.0);
        let at_96k = InstrumentDna::from_seed(42, 96_000.0);

        // Rate-independent params must be identical
        assert_eq!(at_44k.exciter.signature, at_96k.exciter.signature);
        assert_eq!(at_44k.spatial.reverb_diffusion, at_96k.spatial.reverb_diffusion);

        // Rate-dependent params must differ
        assert_ne!(at_44k.resonator.modulation_rate_hz, at_96k.resonator.modulation_rate_hz);

        // But the underlying Hz value should be the same
        let hz_44k = at_44k.resonator.modulation_rate_hz * 44_100.0;
        let hz_96k = at_96k.resonator.modulation_rate_hz * 96_000.0;
        assert!((hz_44k - hz_96k).abs() < 0.001,
            "Same seed should produce same Hz rate regardless of sample rate");
    }

    /// Statistical sanity: over many seeds, derived values should be roughly
    /// uniformly distributed within their ranges (not clustered).
    #[test]
    fn distribution_uniformity() {
        let n = 10_000;
        let sigs: Vec<f32> = (0..n)
            .map(|i| InstrumentDna::from_seed(i, TEST_SAMPLE_RATE).exciter.signature)
            .collect();

        // Divide [0,1) into 10 bins and count
        let mut bins = [0u32; 10];
        for s in &sigs {
            let bin = (s * 10.0).min(9.0) as usize;
            bins[bin] += 1;
        }

        // Each bin should have roughly n/10 = 1000 entries.
        // Allow ±40% tolerance (600-1400) for LCG distribution.
        let expected = n as f32 / 10.0;
        for (i, &count) in bins.iter().enumerate() {
            let ratio = count as f32 / expected;
            assert!(
                (0.6..=1.4).contains(&ratio),
                "Bin {i} has {count} entries (expected ~{expected:.0}), ratio {ratio:.2} — distribution too uneven"
            );
        }
    }

    /// Verify the Display impl doesn't panic and produces readable output.
    #[test]
    fn display_format() {
        let dna = InstrumentDna::from_seed(0xDEAD_BEEF, TEST_SAMPLE_RATE);
        let s = format!("{dna}");
        assert!(s.contains("0xDEADBEEF"), "Display should show hex seed");
        assert!(s.contains("exciter:"), "Display should label sections");
        assert!(s.contains("spatial:"), "Display should include spatial section");
    }

    /// No-std compatibility check: ensure nothing in the public API
    /// requires std. (This is a compile-time check more than a runtime one,
    /// but we verify the struct is Copy + Clone + no heap allocation.)
    #[test]
    fn no_alloc_construction() {
        // This test passes if it compiles — InstrumentDna::from_seed
        // uses only stack allocation and arithmetic.
        let dna = InstrumentDna::from_seed(1, 48_000.0);
        let _copy = dna; // Copy
        let _clone = dna.clone(); // Clone
        assert_eq!(_copy, _clone);
    }
}
