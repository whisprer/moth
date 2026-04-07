//! Vibrator section — what vibrates.
//!
//! The vibrator receives the excitation signal from the exciter and produces
//! pitched audio. It is the string, the tube, the membrane — the part of the
//! instrument that sustains vibration at a specific frequency.
//!
//! # Architecture
//!
//! [`WaveguideString`] implements a digital waveguide string model:
//!
//! ```text
//!                    ┌──────────────────────────────────┐
//!  excitation ──→ [+] ──→ delay_line ──→ damping_lp ──→ ×feedback ──┐
//!                  ↑                                                 │
//!                  └────── dispersion_ap ←───────────────────────────┘
//!                              │
//!                         read at position
//!                              │
//!                              ↓
//!                           output
//! ```
//!
//! The delay line length determines the fundamental frequency. The damping
//! filter in the feedback loop controls how quickly the sound decays and
//! how the spectrum evolves over time — higher frequencies decay faster,
//! producing the natural brightness-loss of a real vibrating string.
//!
//! # Position
//!
//! The output is read from a comb-filtered tap point along the delay line,
//! controlled by `position` (from [`PlayGesture`](crate::gesture::PlayGesture)).
//! Position 0.5 = middle of the string (removes even harmonics — warm, hollow).
//! Position near 0 or 1 = near the bridge (all harmonics present — bright, thin).
//!
//! # DNA Integration
//!
//! [`VibratorDna`](crate::instrument_dna::VibratorDna) provides:
//! - `delay_micro_offset` — fractional-sample offset, making each instance's
//!   intonation subtly unique (like bridge placement tolerance on a real instrument)
//! - `dispersion_asymmetry` — how unevenly stiffness affects partials, giving
//!   each instance its own spectral evolution character

use crate::dsp_core::{DcBlocker, OnePole};
use crate::instrument_dna::VibratorDna;

// ─── Constants ──────────────────────────────────────────────────────────────

/// Maximum delay line length in samples.
///
/// At 48 kHz, this supports fundamentals down to ~11.7 Hz (below MIDI note 0).
/// Memory cost: 4096 × 4 bytes = 16 KB per voice.
const DELAY_LINE_SIZE: usize = 4096;

/// Maximum allpass delay for dispersion.
///
/// Enough for significant inharmonicity effects (piano-like stiffness).
const ALLPASS_SIZE: usize = 256;

// ─── Delay line ─────────────────────────────────────────────────────────────

/// Circular buffer delay line with fractional-sample interpolation.
///
/// The workhorse of waveguide synthesis. Write one sample per tick,
/// read at any fractional offset with linear interpolation.
struct DelayLine {
    buffer: [f32; DELAY_LINE_SIZE],
    write_pos: usize,
}

impl DelayLine {
    const fn new() -> Self {
        Self {
            buffer: [0.0; DELAY_LINE_SIZE],
            write_pos: 0,
        }
    }

    /// Write a sample at the current position and advance.
    #[inline]
    fn write(&mut self, sample: f32) {
        self.buffer[self.write_pos] = sample;
        self.write_pos = (self.write_pos + 1) % DELAY_LINE_SIZE;
    }

    /// Read at a fractional delay offset from the write head.
    ///
    /// `delay` is in samples (fractional). A delay of 0.0 reads the most
    /// recently written sample. Linear interpolation between adjacent
    /// samples for sub-sample accuracy (critical for pitch accuracy at
    /// high frequencies).
    #[inline]
    fn read(&self, delay: f32) -> f32 {
        let delay = delay.clamp(0.0, (DELAY_LINE_SIZE - 2) as f32);
        let delay_int = delay as usize;
        let delay_frac = delay - delay_int as f32;

        // Read positions (backwards from write head)
        let pos_a = (self.write_pos + DELAY_LINE_SIZE - delay_int - 1) % DELAY_LINE_SIZE;
        let pos_b = (self.write_pos + DELAY_LINE_SIZE - delay_int - 2) % DELAY_LINE_SIZE;

        let a = self.buffer[pos_a];
        let b = self.buffer[pos_b];

        // Linear interpolation
        a + (b - a) * delay_frac
    }

    /// Clear the delay line to silence.
    fn clear(&mut self) {
        for s in self.buffer.iter_mut() {
            *s = 0.0;
        }
    }
}

// ─── Allpass filter ─────────────────────────────────────────────────────────

/// First-order allpass filter for waveguide dispersion.
///
/// Adds frequency-dependent delay without changing the magnitude spectrum.
/// In the waveguide feedback loop, this makes higher partials arrive slightly
/// later than lower ones — the inharmonicity characteristic of stiff strings
/// (piano), metallic bars, and bells.
///
/// Transfer function: `H(z) = (g + z^-D) / (1 + g * z^-D)`
struct Allpass {
    buffer: [f32; ALLPASS_SIZE],
    write_pos: usize,
    /// Allpass coefficient. Range `(-1, 1)`.
    /// Negative values spread partials downward (piano-like).
    /// Positive values spread upward (metallic).
    gain: f32,
    /// Delay length for the allpass (integer samples).
    delay: usize,
}

impl Allpass {
    const fn new() -> Self {
        Self {
            buffer: [0.0; ALLPASS_SIZE],
            write_pos: 0,
            gain: 0.0,
            delay: 1,
        }
    }

    /// Set the allpass gain coefficient.
    #[inline]
    fn set_gain(&mut self, gain: f32) {
        self.gain = gain.clamp(-0.99, 0.99);
    }

    /// Set the allpass delay length.
    #[inline]
    fn set_delay(&mut self, delay: usize) {
        self.delay = delay.clamp(1, ALLPASS_SIZE - 1);
    }

    /// Process one sample.
    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let read_pos =
            (self.write_pos + ALLPASS_SIZE - self.delay) % ALLPASS_SIZE;
        let delayed = self.buffer[read_pos];

        // Standard allpass using the "w" trick:
        //   w[n] = x[n] + g * w[n-D]
        //   y[n] = -g * w[n] + w[n-D]
        // We store w in the buffer so only one delay line is needed.
        let w = input + self.gain * delayed;
        let output = -self.gain * w + delayed;

        self.buffer[self.write_pos] = w;
        self.write_pos = (self.write_pos + 1) % ALLPASS_SIZE;

        output
    }

    /// Clear state.
    fn clear(&mut self) {
        for s in self.buffer.iter_mut() {
            *s = 0.0;
        }
    }
}

/// Two-tap averaging FIR filter for string damping.
///
/// `y[n] = (1-g) * x[n] + g * x[n-1]`
///
/// Simpler and more stable than IIR in the waveguide feedback loop.
/// `g` controls brightness: `g=0` = no filtering (bright), `g=0.5` =
/// maximum damping (Karplus-Strong averaging), values between shape
/// the brightness decay.
#[derive(Debug, Clone, Copy)]
struct FirDamping {
    prev: f32,
    g: f32,
}

impl FirDamping {
    const fn new() -> Self {
        Self { prev: 0.0, g: 0.25 }
    }

    #[inline]
    fn set_g(&mut self, g: f32) {
        self.g = g.clamp(0.0, 0.5);
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let output = (1.0 - self.g) * input + self.g * self.prev;
        self.prev = input;
        output
    }

    fn reset(&mut self) {
        self.prev = 0.0;
    }
}

// ─── The waveguide string ───────────────────────────────────────────────────

/// Waveguide string model — the core vibrator.
///
/// Converts excitation signals into pitched, decaying vibrations.
/// The delay line length determines the fundamental frequency; the
/// damping filter shapes the decay and spectral evolution; the allpass
/// adds inharmonicity (dispersion); and the comb-filtered output tap
/// simulates pickup/excitation position.
///
/// # Usage
///
/// ```
/// use moth::vibrator::WaveguideString;
/// use moth::instrument_dna::InstrumentDna;
///
/// let dna = InstrumentDna::from_seed(0xDEADBEEF, 48000.0);
/// let mut string = WaveguideString::new(&dna.vibrator, 48000.0);
///
/// string.set_frequency(440.0);
/// string.set_damping(0.7);
/// string.set_brightness(0.6);
/// string.set_position(0.3);
///
/// let excitation = [0.0f32; 128]; // from ExciterProcessor
/// let mut output = [0.0f32; 128];
/// string.process(&excitation, &mut output);
/// ```
pub struct WaveguideString {
    sample_rate: f32,
    dna: VibratorDna,

    // ── Delay line ──
    delay: DelayLine,

    // ── Current pitch ──
    /// Delay length in fractional samples. `sample_rate / frequency`.
    delay_length: f32,
    /// Target delay length — we interpolate toward this to avoid clicks
    /// on pitch changes.
    delay_target: f32,

    // ── Damping ──
    /// FIR averaging filter in the feedback loop — frequency-dependent decay.
    fir_damping: FirDamping,
    /// IIR lowpass in the feedback loop — additional brightness control.
    iir_damping: OnePole,
    /// Feedback gain — energy retained per round trip. Controls decay time.
    feedback_gain: f32,

    // ── Dispersion ──
    /// Allpass filter for inharmonicity.
    dispersion: Allpass,
    /// Current dispersion amount (0.0 = harmonic, 1.0 = very inharmonic).
    dispersion_amount: f32,

    // ── Position ──
    /// Comb delay for position-dependent spectral shaping.
    /// Clamped to avoid degenerate cases (0 or full delay).
    comb_position: f32,

    // ── Output ──
    dc_blocker: DcBlocker,
}

impl WaveguideString {
    /// Create a new waveguide string.
    ///
    /// Initialised to 440 Hz, moderate damping, no dispersion.
    pub fn new(dna: &VibratorDna, sample_rate: f32) -> Self {
        let initial_delay = sample_rate / 440.0;

        Self {
            sample_rate,
            dna: *dna,
            delay: DelayLine::new(),
            delay_length: initial_delay,
            delay_target: initial_delay,
            fir_damping: FirDamping::new(),
            iir_damping: OnePole::new(0.5),
            feedback_gain: 0.998,
            dispersion: Allpass::new(),
            dispersion_amount: 0.0,
            comb_position: 0.5,
            dc_blocker: DcBlocker::new(sample_rate),
        }
    }

    /// Set the fundamental frequency in Hz.
    ///
    /// The delay line length is `sample_rate / frequency`, plus the
    /// per-instance DNA micro-offset. The pitch change is smoothed
    /// across the next process block to avoid clicks.
    pub fn set_frequency(&mut self, hz: f32) {
        let hz = hz.clamp(8.0, self.sample_rate * 0.45);
        let base_delay = self.sample_rate / hz;

        // DNA micro-offset: each instance is very slightly detuned,
        // like bridge placement tolerance on a real instrument.
        let offset = self.dna.delay_micro_offset * 0.5; // ±0.25 samples max
        self.delay_target = (base_delay + offset).clamp(2.0, (DELAY_LINE_SIZE - 4) as f32);
    }

    /// Set the damping amount.
    ///
    /// `0.0` = very short decay (pluck dies immediately).
    /// `0.5` = moderate sustain (~1-2 seconds).
    /// `1.0` = near-infinite sustain (never decays).
    pub fn set_damping(&mut self, damping: f32) {
        let d = damping.clamp(0.0, 1.0);

        // Feedback gain: how much energy survives each round trip.
        // Map quadratically for perceptually linear control:
        // d=0 → 0.90 (fast decay), d=0.5 → 0.995 (moderate), d=1.0 → 0.99999 (infinite)
        self.feedback_gain = if d >= 0.999 {
            1.0
        } else {
            let mapped = d * d; // quadratic for better feel in the lower range
            0.90 + mapped * 0.09999
        };
    }

    /// Set the brightness.
    ///
    /// Controls how quickly high frequencies decay relative to low frequencies.
    ///
    /// `0.0` = dark, muted — high frequencies die almost immediately.
    /// `0.5` = natural — balanced decay across the spectrum.
    /// `1.0` = bright, metallic — all frequencies sustain equally.
    pub fn set_brightness(&mut self, brightness: f32) {
        let b = brightness.clamp(0.0, 1.0);

        // FIR damping: g=0 → no averaging (bright), g=0.5 → max averaging (dark)
        let fir_g = (1.0 - b) * 0.45;
        self.fir_damping.set_g(fir_g);

        // IIR damping: coefficient controls the lowpass cutoff in the feedback
        let iir_coeff = 0.15 + b * 0.80;
        self.iir_damping.set_coeff(iir_coeff);
    }

    /// Set the excitation/pickup position along the string.
    ///
    /// `0.0` and `1.0` = near the bridge/nut (bright, all harmonics).
    /// `0.5` = middle of the string (warm, even harmonics removed).
    ///
    /// Uses the Elements clamping formula to prevent degenerate comb
    /// settings: `clamped = 0.5 - 0.98 * |position - 0.5|`.
    pub fn set_position(&mut self, position: f32) {
        let p = position.clamp(0.0, 1.0);
        // Clamp to [0.01, 0.5] to avoid zero comb delay or full-period comb
        self.comb_position = 0.5 - 0.98 * (p - 0.5).abs();
    }

    /// Set the dispersion (inharmonicity) amount.
    ///
    /// `0.0` = perfectly harmonic partials (ideal string).
    /// `0.5` = moderate inharmonicity (acoustic piano).
    /// `1.0` = extreme inharmonicity (metallic bar, bell-like).
    ///
    /// DNA `dispersion_asymmetry` modifies this per-instance — each Moth
    /// has slightly different inharmonicity character.
    pub fn set_dispersion(&mut self, dispersion: f32) {
        let d = dispersion.clamp(0.0, 1.0);
        self.dispersion_amount = d;

        // Allpass gain coefficient: controls the severity of the frequency
        // spreading. DNA asymmetry makes it subtly different per-instance.
        let dna_scaled = d * self.dna.dispersion_asymmetry;

        // Map to allpass gain: negative values = spreading partials upward
        // (the physical direction for stiff strings)
        let ap_gain = -0.5 * dna_scaled * dna_scaled;
        self.dispersion.set_gain(ap_gain);

        // Allpass delay: a fraction of the main delay, proportional to dispersion
        let ap_delay = if self.delay_target > 8.0 {
            ((self.delay_target * d * 0.1) as usize).clamp(1, ALLPASS_SIZE - 1)
        } else {
            1
        };
        self.dispersion.set_delay(ap_delay);
    }

    /// Process one audio block.
    ///
    /// The excitation signal (from [`ExciterProcessor`](crate::exciter_dsp::ExciterProcessor))
    /// is injected into the delay line. The output is the vibrating string's
    /// audio — pitched, decaying, shaped by damping and position.
    ///
    /// `excitation` and `output` must be the same length.
    pub fn process(&mut self, excitation: &[f32], output: &mut [f32]) {
        debug_assert_eq!(excitation.len(), output.len());
        let len = excitation.len();
        if len == 0 {
            return;
        }

        // Smooth pitch changes across the block to prevent clicks
        let delay_inc = (self.delay_target - self.delay_length) / len as f32;

        // Comb delay for position-based spectral shaping
        // (recomputed per-sample as delay_length changes with pitch smoothing)

        for i in 0..len {
            // Advance pitch smoothing
            self.delay_length += delay_inc;
            let current_delay = self.delay_length.clamp(2.0, (DELAY_LINE_SIZE - 4) as f32);

            // ── Read from delay line ──
            let delayed = self.delay.read(current_delay);

            // ── Feedback processing ──
            // FIR damping (frequency-dependent decay)
            let fir_out = self.fir_damping.process(delayed);

            // IIR damping (additional brightness shaping)
            let iir_out = self.iir_damping.process(fir_out);

            // Feedback gain (overall energy loss per round trip)
            let feedback = iir_out * self.feedback_gain;

            // ── Dispersion ──
            let dispersed = if self.dispersion_amount > 0.001 {
                self.dispersion.process(feedback)
            } else {
                feedback
            };

            // ── Write back: feedback + new excitation ──
            let input = dispersed + excitation[i];
            self.delay.write(input);

            // ── Output: read at position-dependent comb tap ──
            let comb_delay = self.comb_position * current_delay;
            let raw_output = self.delay.read(comb_delay);

            // DC block the output
            output[i] = self.dc_blocker.process(raw_output);
        }

        // Snap delay to target at end of block
        self.delay_length = self.delay_target;
    }

    /// Reset all state — silence the string.
    pub fn reset(&mut self) {
        self.delay.clear();
        self.fir_damping.reset();
        self.iir_damping.reset();
        self.dispersion.clear();
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

    fn make_string(seed: u32) -> WaveguideString {
        let dna = InstrumentDna::from_seed(seed, SR);
        WaveguideString::new(&dna.vibrator, SR)
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
        let mut ws = make_string(42);
        ws.set_frequency(440.0);
        let exc = [0.0f32; BLOCK];
        let mut out = [0.0f32; BLOCK];
        ws.process(&exc, &mut out);
        assert!(peak(&out) < 1e-6, "No excitation should mean no output");
    }

    #[test]
    fn impulse_produces_decaying_pitched_signal() {
        let mut ws = make_string(42);
        ws.set_frequency(440.0);
        ws.set_damping(0.7);
        ws.set_brightness(0.5);
        ws.set_position(0.3);

        // Single impulse excitation
        let mut exc = [0.0f32; BLOCK];
        exc[0] = 1.0;

        let mut out = [0.0f32; BLOCK];
        ws.process(&exc, &mut out);

        // Should have output energy
        let energy = rms(&out);
        assert!(energy > 0.001, "Impulse should excite the string, got RMS {energy}");

        // Process more blocks — energy should persist (it's ringing)
        let exc_silent = [0.0f32; BLOCK];
        let mut out2 = [0.0f32; BLOCK];
        ws.process(&exc_silent, &mut out2);

        let energy2 = rms(&out2);
        assert!(
            energy2 > 0.0001,
            "String should still be ringing after one block, got RMS {energy2}"
        );
    }

    #[test]
    fn pitch_determines_period() {
        let mut ws = make_string(42);
        ws.set_frequency(480.0); // clean divisor of 48000 → period = 100 samples
        ws.set_damping(0.9);
        ws.set_brightness(1.0); // maximum brightness for clearest periodicity

        // Impulse
        let mut exc = [0.0f32; 512];
        exc[0] = 1.0;

        let mut out = [0.0f32; 512];
        ws.process(&exc, &mut out);

        // Check for periodicity: the signal should repeat roughly every
        // 100 samples (48000/480). Look for a peak near sample 100.
        let period = (SR / 480.0) as usize; // 100

        // Find the first significant peak after the initial transient
        let search_start = period - 5;
        let search_end = (period + 5).min(out.len());
        let peak_region: f32 = out[search_start..search_end]
            .iter()
            .map(|s| s.abs())
            .fold(0.0f32, f32::max);

        assert!(
            peak_region > 0.01,
            "Should see a reflection peak near sample {period}, got {peak_region}"
        );
    }

    #[test]
    fn higher_damping_longer_sustain() {
        let freq = 440.0;

        // Low damping
        let mut ws_lo = make_string(42);
        ws_lo.set_frequency(freq);
        ws_lo.set_damping(0.2);
        ws_lo.set_brightness(0.5);

        // High damping
        let mut ws_hi = make_string(42);
        ws_hi.set_frequency(freq);
        ws_hi.set_damping(0.9);
        ws_hi.set_brightness(0.5);

        // Same impulse
        let mut exc = [0.0f32; BLOCK];
        exc[0] = 1.0;
        let exc_silent = [0.0f32; BLOCK];

        let mut out_lo = [0.0f32; BLOCK];
        let mut out_hi = [0.0f32; BLOCK];

        // Process initial block
        ws_lo.process(&exc, &mut out_lo);
        ws_hi.process(&exc, &mut out_hi);

        // Process several more silent blocks
        for _ in 0..20 {
            ws_lo.process(&exc_silent, &mut out_lo);
            ws_hi.process(&exc_silent, &mut out_hi);
        }

        let energy_lo = rms(&out_lo);
        let energy_hi = rms(&out_hi);

        assert!(
            energy_hi > energy_lo * 2.0,
            "Higher damping should sustain longer: lo={energy_lo:.6}, hi={energy_hi:.6}"
        );
    }

    #[test]
    fn brightness_affects_spectral_content() {
        let freq = 200.0;

        // Dark
        let mut ws_dark = make_string(42);
        ws_dark.set_frequency(freq);
        ws_dark.set_damping(0.8);
        ws_dark.set_brightness(0.0);

        // Bright
        let mut ws_bright = make_string(42);
        ws_bright.set_frequency(freq);
        ws_bright.set_damping(0.8);
        ws_bright.set_brightness(1.0);

        let mut exc = [0.0f32; BLOCK];
        exc[0] = 1.0;
        let exc_silent = [0.0f32; BLOCK];

        let mut out_dark = [0.0f32; BLOCK];
        let mut out_bright = [0.0f32; BLOCK];

        ws_dark.process(&exc, &mut out_dark);
        ws_bright.process(&exc, &mut out_bright);

        // Let it ring for several cycles
        for _ in 0..10 {
            ws_dark.process(&exc_silent, &mut out_dark);
            ws_bright.process(&exc_silent, &mut out_bright);
        }

        // Measure HF content (sum of absolute sample-to-sample differences)
        let hf_dark: f32 = out_dark.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
        let hf_bright: f32 = out_bright
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .sum();

        assert!(
            hf_bright > hf_dark,
            "Bright setting should have more HF: dark={hf_dark:.4}, bright={hf_bright:.4}"
        );
    }

    #[test]
    fn position_changes_timbre() {
        let freq = 200.0;
        let mut exc = [0.0f32; BLOCK];
        exc[0] = 1.0;
        let exc_silent = [0.0f32; BLOCK];

        // Position near bridge
        let mut ws_bridge = make_string(42);
        ws_bridge.set_frequency(freq);
        ws_bridge.set_damping(0.8);
        ws_bridge.set_brightness(0.8);
        ws_bridge.set_position(0.1);

        // Position at centre
        let mut ws_mid = make_string(42);
        ws_mid.set_frequency(freq);
        ws_mid.set_damping(0.8);
        ws_mid.set_brightness(0.8);
        ws_mid.set_position(0.5);

        let mut out_bridge = [0.0f32; BLOCK];
        let mut out_mid = [0.0f32; BLOCK];

        ws_bridge.process(&exc, &mut out_bridge);
        ws_mid.process(&exc, &mut out_mid);

        for _ in 0..5 {
            ws_bridge.process(&exc_silent, &mut out_bridge);
            ws_mid.process(&exc_silent, &mut out_mid);
        }

        // Bridge position should have more HF (more harmonics present)
        let hf_bridge: f32 = out_bridge
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .sum();
        let hf_mid: f32 = out_mid.windows(2).map(|w| (w[1] - w[0]).abs()).sum();

        assert!(
            hf_bridge > hf_mid,
            "Bridge position should be brighter: bridge={hf_bridge:.4}, mid={hf_mid:.4}"
        );
    }

    // ── DNA differentiation ──

    #[test]
    fn different_dna_different_ring() {
        let mut exc = [0.0f32; BLOCK];
        exc[0] = 1.0;
        let exc_silent = [0.0f32; BLOCK];

        let mut ws_a = make_string(0xAAAA);
        let mut ws_b = make_string(0xBBBB);

        ws_a.set_frequency(440.0);
        ws_a.set_damping(0.8);
        ws_b.set_frequency(440.0);
        ws_b.set_damping(0.8);

        let mut out_a = [0.0f32; BLOCK];
        let mut out_b = [0.0f32; BLOCK];

        ws_a.process(&exc, &mut out_a);
        ws_b.process(&exc, &mut out_b);

        for _ in 0..5 {
            ws_a.process(&exc_silent, &mut out_a);
            ws_b.process(&exc_silent, &mut out_b);
        }

        let diff: f32 = out_a
            .iter()
            .zip(out_b.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();

        assert!(
            diff > 0.01,
            "Different DNA should produce different ringing: diff={diff}"
        );
    }

    #[test]
    fn same_dna_same_ring() {
        let mut exc = [0.0f32; BLOCK];
        exc[0] = 1.0;

        let mut ws_a = make_string(42);
        let mut ws_b = make_string(42);

        ws_a.set_frequency(440.0);
        ws_a.set_damping(0.8);
        ws_b.set_frequency(440.0);
        ws_b.set_damping(0.8);

        let mut out_a = [0.0f32; BLOCK];
        let mut out_b = [0.0f32; BLOCK];

        ws_a.process(&exc, &mut out_a);
        ws_b.process(&exc, &mut out_b);

        for (i, (&a, &b)) in out_a.iter().zip(out_b.iter()).enumerate() {
            assert_eq!(a, b, "Sample {i} differs: {a} vs {b}");
        }
    }

    // ── Dispersion ──

    #[test]
    fn dispersion_changes_spectral_evolution() {
        let mut exc = [0.0f32; BLOCK];
        exc[0] = 1.0;
        let exc_silent = [0.0f32; BLOCK];

        // No dispersion
        let mut ws_clean = make_string(42);
        ws_clean.set_frequency(200.0);
        ws_clean.set_damping(0.85);
        ws_clean.set_dispersion(0.0);

        // High dispersion
        let mut ws_disp = make_string(42);
        ws_disp.set_frequency(200.0);
        ws_disp.set_damping(0.85);
        ws_disp.set_dispersion(0.8);

        let mut out_clean = [0.0f32; BLOCK];
        let mut out_disp = [0.0f32; BLOCK];

        ws_clean.process(&exc, &mut out_clean);
        ws_disp.process(&exc, &mut out_disp);

        for _ in 0..5 {
            ws_clean.process(&exc_silent, &mut out_clean);
            ws_disp.process(&exc_silent, &mut out_disp);
        }

        // Outputs should differ (dispersion changes the sound)
        let diff: f32 = out_clean
            .iter()
            .zip(out_disp.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();

        assert!(
            diff > 0.01,
            "Dispersion should change the sound: diff={diff}"
        );
    }

    // ── Safety ──

    #[test]
    fn output_stays_bounded() {
        let mut ws = make_string(42);
        ws.set_frequency(440.0);
        ws.set_damping(1.0); // maximum sustain
        ws.set_brightness(1.0); // maximum brightness

        // Strong excitation over many blocks
        let exc = [0.5f32; BLOCK];
        let mut out = [0.0f32; BLOCK];

        for _ in 0..100 {
            ws.process(&exc, &mut out);
        }

        let p = peak(&out);
        assert!(
            p < 50.0,
            "Output should not explode with sustained excitation, got peak {p}"
        );
    }

    #[test]
    fn reset_silences() {
        let mut ws = make_string(42);
        ws.set_frequency(440.0);

        let mut exc = [0.0f32; BLOCK];
        exc[0] = 1.0;
        let mut out = [0.0f32; BLOCK];
        ws.process(&exc, &mut out);

        ws.reset();

        let silent = [0.0f32; BLOCK];
        ws.process(&silent, &mut out);
        assert!(
            peak(&out) < 1e-6,
            "After reset, string should be silent"
        );
    }

    #[test]
    fn frequency_change_smooth() {
        let mut ws = make_string(42);
        ws.set_frequency(440.0);
        ws.set_damping(0.95);

        // Excite
        let mut exc = [0.0f32; BLOCK];
        exc[0] = 1.0;
        let mut out = [0.0f32; BLOCK];
        ws.process(&exc, &mut out);

        // Change frequency — should not click
        ws.set_frequency(880.0);
        let silent = [0.0f32; BLOCK];
        ws.process(&silent, &mut out);

        // Check for clicks: no single sample should be dramatically
        // larger than its neighbours
        let max_diff: f32 = out
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);

        assert!(
            max_diff < 1.0,
            "Frequency change should be smooth, got max diff {max_diff}"
        );
    }
}
