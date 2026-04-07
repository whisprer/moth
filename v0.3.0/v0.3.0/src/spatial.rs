//! Spatial section — reverb, delay, and stereo width.
//!
//! The final colouration stage: the virtual room that Moth's sound
//! inhabits. A feedback delay network (FDN) provides the reverb tail,
//! designed for colourless operation (Heldmann/Schlecht, Aalto 2021) —
//! the *body* colours the sound, not the room.
//!
//! # FDN Reverb
//!
//! Four delay lines with a Hadamard feedback matrix. Delay lengths are
//! mutually prime (~22–33ms at 48kHz) for maximal echo density. One-pole
//! lowpass filters in each feedback path provide frequency-dependent
//! decay — high frequencies die faster, like a real room with absorption.
//!
//! The reverb is intentionally *warm*. The feedback damping never goes
//! fully bright. The DNA spatial parameters (`reverb_diffusion`,
//! `reverb_brightness`) season the character per-instance.

use crate::dsp_core::{DcBlocker, OnePole};
use crate::instrument_dna::SpatialDna;

// ─── Constants ──────────────────────────────────────────────────────────────

/// Number of delay lines in the FDN.
const FDN_SIZE: usize = 4;

/// Delay buffer size per line (samples). Supports delays up to ~42ms at 48kHz.
const FDN_DELAY_BUF: usize = 2048;

/// Prime delay lengths in samples (~22–33ms at 48kHz).
/// Mutually prime for maximal echo density and minimal periodicity.
const DELAY_LENGTHS: [usize; FDN_SIZE] = [1087, 1283, 1429, 1597];

// ─── FDN Delay Line ─────────────────────────────────────────────────────────

struct FdnDelay {
    buffer: [f32; FDN_DELAY_BUF],
    write_pos: usize,
    length: usize,
}

impl FdnDelay {
    fn new(length: usize) -> Self {
        Self {
            buffer: [0.0; FDN_DELAY_BUF],
            write_pos: 0,
            length: length.min(FDN_DELAY_BUF - 1),
        }
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let read_pos = (self.write_pos + FDN_DELAY_BUF - self.length) % FDN_DELAY_BUF;
        let output = self.buffer[read_pos];
        self.buffer[self.write_pos] = input;
        self.write_pos = (self.write_pos + 1) % FDN_DELAY_BUF;
        output
    }

    fn clear(&mut self) {
        for s in self.buffer.iter_mut() {
            *s = 0.0;
        }
    }
}

// ─── Spatial character ──────────────────────────────────────────────────────

/// Spatial processing parameters — morphable presets for the room.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpatialCharacter {
    /// Room size — scales feedback gain (decay time).
    /// `0.0` = tiny/dry, `0.5` = medium room, `1.0` = large hall.
    pub size: f32,
    /// High-frequency absorption in the reverb tail.
    /// `0.0` = dark (heavy absorption), `1.0` = bright (minimal absorption).
    pub brightness: f32,
    /// Reverb wet/dry mix. `0.0` = fully dry, `1.0` = fully wet.
    pub mix: f32,
}

impl SpatialCharacter {
    pub const DRY: Self = Self {
        size: 0.0,
        brightness: 0.5,
        mix: 0.0,
    };
    pub const SMALL_ROOM: Self = Self {
        size: 0.3,
        brightness: 0.45,
        mix: 0.20,
    };
    pub const MEDIUM_ROOM: Self = Self {
        size: 0.5,
        brightness: 0.50,
        mix: 0.25,
    };
    pub const LARGE_HALL: Self = Self {
        size: 0.8,
        brightness: 0.40,
        mix: 0.30,
    };
    pub const CATHEDRAL: Self = Self {
        size: 0.95,
        brightness: 0.35,
        mix: 0.35,
    };

    #[inline]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let l = |a: f32, b: f32| a + (b - a) * t;
        Self {
            size: l(self.size, other.size),
            brightness: l(self.brightness, other.brightness),
            mix: l(self.mix, other.mix),
        }
    }
}

impl Default for SpatialCharacter {
    fn default() -> Self {
        Self::SMALL_ROOM
    }
}

// ─── The spatial processor ──────────────────────────────────────────────────

/// FDN reverb + spatial processing.
pub struct SpatialProcessor {
    dna: SpatialDna,

    // FDN delay lines
    delays: [FdnDelay; FDN_SIZE],
    // Damping filters per delay line (frequency-dependent decay)
    damping: [OnePole; FDN_SIZE],

    // Feedback gain (controls decay time)
    feedback_gain: f32,

    // Mix
    wet_level: f32,
    dry_level: f32,

    // Output
    dc_blocker: DcBlocker,
}

impl SpatialProcessor {
    pub fn new(dna: &SpatialDna, sample_rate: f32) -> Self {
        // Scale delay lengths for sample rate (base lengths are for 48kHz)
        let sr_ratio = sample_rate / 48_000.0;

        let delays = [
            FdnDelay::new((DELAY_LENGTHS[0] as f32 * sr_ratio) as usize),
            FdnDelay::new((DELAY_LENGTHS[1] as f32 * sr_ratio) as usize),
            FdnDelay::new((DELAY_LENGTHS[2] as f32 * sr_ratio) as usize),
            FdnDelay::new((DELAY_LENGTHS[3] as f32 * sr_ratio) as usize),
        ];

        // DNA diffusion affects the damping filter settings.
        // DNA brightness affects the initial damping coefficients.
        let damp_coeff = 0.3 + dna.reverb_brightness * 0.5;
        let damping = [
            OnePole::new(damp_coeff * dna.reverb_diffusion),
            OnePole::new(damp_coeff * 1.02), // slight variation per line
            OnePole::new(damp_coeff * 0.98),
            OnePole::new(damp_coeff * dna.reverb_diffusion * 1.01),
        ];

        Self {
            dna: *dna,
            delays,
            damping,
            feedback_gain: 0.5,
            wet_level: 0.2,
            dry_level: 0.8,
            dc_blocker: DcBlocker::new(sample_rate),
        }
    }

    /// Apply a spatial character preset.
    pub fn apply_character(&mut self, ch: &SpatialCharacter) {
        // Size → feedback gain.
        // size=0 → 0.2 (very short tail), size=1 → 0.85 (long tail).
        // Never reaches 1.0 — the reverb always decays, never builds.
        let s = ch.size.clamp(0.0, 1.0);
        self.feedback_gain = 0.2 + s * 0.65;

        // Brightness → damping filter coefficient.
        let base_damp = 0.15 + ch.brightness.clamp(0.0, 1.0) * 0.65;
        for (i, d) in self.damping.iter_mut().enumerate() {
            // DNA variation per line
            let dna_var = match i {
                0 => self.dna.reverb_diffusion,
                1 => 1.02,
                2 => 0.98,
                _ => self.dna.reverb_diffusion * 1.01,
            };
            d.set_coeff((base_damp * dna_var).clamp(0.05, 0.85));
        }

        // Mix
        self.wet_level = ch.mix.clamp(0.0, 1.0);
        self.dry_level = 1.0 - self.wet_level * 0.5; // dry doesn't fully duck
    }

    /// Process one audio block through the spatial section.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        debug_assert_eq!(input.len(), output.len());

        let fb = self.feedback_gain;

        for i in 0..input.len() {
            let dry = input[i];

            // Read from all four delay lines
            let d0 = self.delays[0].process(0.0); // read only, write later
            let d1 = self.delays[1].process(0.0);
            let d2 = self.delays[2].process(0.0);
            let d3 = self.delays[3].process(0.0);

            // Hadamard mixing matrix (4×4, normalised by 0.5)
            // H = 0.5 * [[1, 1, 1, 1],
            //             [1,-1, 1,-1],
            //             [1, 1,-1,-1],
            //             [1,-1,-1, 1]]
            let m0 = 0.5 * (d0 + d1 + d2 + d3);
            let m1 = 0.5 * (d0 - d1 + d2 - d3);
            let m2 = 0.5 * (d0 + d1 - d2 - d3);
            let m3 = 0.5 * (d0 - d1 - d2 + d3);

            // Apply damping (frequency-dependent decay) and feedback gain
            let f0 = self.damping[0].process(m0) * fb + dry * 0.25;
            let f1 = self.damping[1].process(m1) * fb + dry * 0.25;
            let f2 = self.damping[2].process(m2) * fb + dry * 0.25;
            let f3 = self.damping[3].process(m3) * fb + dry * 0.25;

            // Write back to delay lines (overwrite the zero we wrote above)
            // We need to rewind the write position by 1 since process() advanced it
            self.delays[0].buffer[(self.delays[0].write_pos + FDN_DELAY_BUF - 1) % FDN_DELAY_BUF] =
                f0;
            self.delays[1].buffer[(self.delays[1].write_pos + FDN_DELAY_BUF - 1) % FDN_DELAY_BUF] =
                f1;
            self.delays[2].buffer[(self.delays[2].write_pos + FDN_DELAY_BUF - 1) % FDN_DELAY_BUF] =
                f2;
            self.delays[3].buffer[(self.delays[3].write_pos + FDN_DELAY_BUF - 1) % FDN_DELAY_BUF] =
                f3;

            // Sum the delay outputs for the wet signal
            let wet = (d0 + d1 + d2 + d3) * 0.25;

            // Mix and output
            let mixed = dry * self.dry_level + wet * self.wet_level;
            output[i] = self.dc_blocker.process(mixed);
        }
    }

    pub fn reset(&mut self) {
        for d in self.delays.iter_mut() {
            d.clear();
        }
        for d in self.damping.iter_mut() {
            d.reset();
        }
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

    fn make_spatial(seed: u32) -> SpatialProcessor {
        let dna = InstrumentDna::from_seed(seed, SR);
        SpatialProcessor::new(&dna.spatial, SR)
    }

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|&s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    fn peak(buf: &[f32]) -> f32 {
        buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max)
    }

    #[test]
    fn silence_in_silence_out() {
        let mut sp = make_spatial(42);
        sp.apply_character(&SpatialCharacter::MEDIUM_ROOM);
        let inp = [0.0f32; BLOCK];
        let mut out = [0.0f32; BLOCK];
        sp.process(&inp, &mut out);
        assert!(peak(&out) < 1e-6);
    }

    #[test]
    fn impulse_produces_reverb_tail() {
        let mut sp = make_spatial(42);
        sp.apply_character(&SpatialCharacter::LARGE_HALL);

        let mut inp = [0.0f32; BLOCK];
        inp[0] = 1.0;
        let mut out = [0.0f32; BLOCK];
        sp.process(&inp, &mut out);

        // Process more blocks — reverb tail should persist
        let silent = [0.0f32; BLOCK];
        let mut tail = [0.0f32; BLOCK];
        for _ in 0..10 {
            sp.process(&silent, &mut tail);
        }
        assert!(rms(&tail) > 0.0001, "Reverb tail should persist");
    }

    #[test]
    fn larger_room_longer_tail() {
        let mut sp_small = make_spatial(42);
        sp_small.apply_character(&SpatialCharacter::SMALL_ROOM);
        let mut sp_large = make_spatial(42);
        sp_large.apply_character(&SpatialCharacter::LARGE_HALL);

        let mut inp = [0.0f32; BLOCK];
        inp[0] = 1.0;
        let silent = [0.0f32; BLOCK];
        let mut out_s = [0.0f32; BLOCK];
        let mut out_l = [0.0f32; BLOCK];

        sp_small.process(&inp, &mut out_s);
        sp_large.process(&inp, &mut out_l);

        for _ in 0..30 {
            sp_small.process(&silent, &mut out_s);
            sp_large.process(&silent, &mut out_l);
        }

        assert!(
            rms(&out_l) > rms(&out_s),
            "Larger room should have longer tail"
        );
    }

    #[test]
    fn dry_mode_passes_through() {
        let mut sp = make_spatial(42);
        sp.apply_character(&SpatialCharacter::DRY);

        let inp: Vec<f32> = (0..BLOCK)
            .map(|i| if i % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        let mut out = vec![0.0f32; BLOCK];
        for _ in 0..3 {
            sp.process(&inp, &mut out);
        }

        assert!(rms(&out) > 0.3, "Dry mode should pass signal through");
    }

    #[test]
    fn output_bounded() {
        let mut sp = make_spatial(42);
        sp.apply_character(&SpatialCharacter::CATHEDRAL);

        let loud = [1.0f32; BLOCK];
        let mut out = [0.0f32; BLOCK];
        for _ in 0..50 {
            sp.process(&loud, &mut out);
        }
        assert!(peak(&out) < 10.0, "Should not explode: peak={}", peak(&out));
    }

    #[test]
    fn different_dna_different_reverb() {
        let mut sp_a = make_spatial(0xAAAA);
        let mut sp_b = make_spatial(0xBBBB);
        sp_a.apply_character(&SpatialCharacter::MEDIUM_ROOM);
        sp_b.apply_character(&SpatialCharacter::MEDIUM_ROOM);

        let mut inp = [0.0f32; BLOCK];
        inp[0] = 1.0;
        let silent = [0.0f32; BLOCK];
        let mut out_a = [0.0f32; BLOCK];
        let mut out_b = [0.0f32; BLOCK];

        sp_a.process(&inp, &mut out_a);
        sp_b.process(&inp, &mut out_b);
        for _ in 0..5 {
            sp_a.process(&silent, &mut out_a);
            sp_b.process(&silent, &mut out_b);
        }

        let diff: f32 = out_a
            .iter()
            .zip(out_b.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(diff > 0.001, "Different DNA should give different reverb");
    }
}
