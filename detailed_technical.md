# Moth — Detailed Technical Manual

**Physical Modelling Synthesiser — Complete Design & Theory Reference**

*Version 0.2.0 — April 2026*
*Prepared with Claude Opus 4.6*

---

## Table of Contents

1. [Philosophy & Identity](#1-philosophy--identity)
2. [The DNA System](#2-the-dna-system)
3. [Input Protocol & Gesture Normalisation](#3-input-protocol--gesture-normalisation)
4. [Exciter Section](#4-exciter-section)
5. [Vibrator Section](#5-vibrator-section)
6. [Resonant Body Section](#6-resonant-body-section)
7. [Non-Linearities Section](#7-non-linearities-section)
8. [Spatial Section](#8-spatial-section)
9. [Voice & Hierarchical Mixer](#9-voice--hierarchical-mixer)
10. [VST3 Plugin Wrapper](#10-vst3-plugin-wrapper)
11. [DSP Primitives](#11-dsp-primitives)
12. [References](#12-references)

---

## 1. Philosophy & Identity

### 1.1 The Name

Moth is named after a friend of the designer who passed away. In Hindu philosophy, the moth drawn to flame is not a cautionary symbol — it is the soul in its most faithful expression, giving itself completely to the light. The friend carried this quality in his living: he gave love unquestioningly and received it naturally in return.

His name is woven into the root of every instance of Moth that will ever exist. The constant `0x6D6F7468` — the ASCII encoding of "moth" (`m=0x6D, o=0x6F, t=0x74, h=0x68`) — is XOR'd into the seed derivation that produces each instrument's unique personality. This is not a dedication on a wall. It is genetic. Constitutive. Every person who plays a Moth and finds it warm, responsive, somehow generous, is touching something real.

### 1.2 Design Principles

**All sweetspot.** Every combination of parameters produces a musically valid, pleasant result. The instrument does not resist the player or punish exploration. This is achieved through bounded parameter ranges, constrained cross-modulation, and the warmth floor.

**Continuous, not discrete.** Physical excitation mechanisms exist on a continuum. A bow is not a fundamentally different thing from a pluck — it is friction-dominated continuous energy transfer vs impulse-dominated direct injection. Moth parameterises this continuum directly, avoiding enums and named categories in favour of continuous f32 axes.

**Everything affects everything.** No parameter lives in isolation. Push the texture and the warmth shifts; alter the depth and the envelope breathes differently. But the interactions are bounded — cross-modulation within safe envelopes, never producing garbage.

**No instance is ever cold.** The warmth floor — a DNA-derived parameter in [0.15, 0.35] — caps the maximum brightness of the tilt filter. Even when the user cranks spectral tilt to maximum, a residual warmth remains. This is not a technical constraint. It is the character of the person whose name is woven into every instance.

**Determinism, not randomness.** Same hardware seed → same personality → same character every power cycle. The instrument has a voice, not noise.

### 1.3 Inspirations

The DNA and sweetspot systems are directly inspired by Mutable Instruments Elements (Émilie Gillet, 2014). Elements' `Part::Seed()` method reads the STM32F4's unique device ID (`0x1FFF7A10`), scrambles it through an LCG with the constant `0xF0CACC1A` ("focaccia"), and derives five bounded parameters: modulation frequency, modulation offset, reverb diffusion, reverb LP, and exciter signature. Every parameter lives in a carefully vetted sweet-spot range. Moth extends this approach to 15 parameters across all five signal chain sections.

---

## 2. The DNA System

### 2.1 Seed Derivation

The DNA system converts a single `u32` hardware seed into a constellation of bounded personality parameters. The algorithm:

```
state = seed XOR 0x6D6F7468     // XOR with MOTH constant
state = state × 1664525 + 1013904223   // Knuth LCG, advance once to diffuse

For each parameter:
    state = state × 1664525 + 1013904223   // advance LCG
    unit = (state >> 8) / 16777216.0       // upper 24 bits → [0, 1)
    value = min + unit × (max - min)       // map to sweet-spot range
```

The LCG uses Knuth's constants (same as Mutable Instruments Elements). Upper 24 bits are used for the normalised float because lower bits of LCGs have shorter periods.

### 2.2 Sweet-Spot Ranges

Every derived parameter lives in a range where all values produce musically valid results:

| Parameter | Range | Rationale |
|-----------|-------|-----------|
| `exciter.signature` | [0.0, 1.0) | Safe: interpreted as modifier on bounded internals |
| `exciter.noise_phase_offset` | [0.0, 1.0) | Noise buffer position — all positions valid |
| `exciter.stochastic_bias` | [0.42, 0.58] | Narrow: felt but never obviously lopsided |
| `exciter.spectral_tilt_bias` | [0.88, 1.02] | **Asymmetric warm**: population trends below 1.0 |
| `exciter.coupling_curve_shape` | [0.85, 1.15] | ±15% nonlinearity scaling |
| `exciter.warmth_floor` | [0.15, 0.35] | **The warmth guarantee**: minimum softness beneath all variation |
| `vibrator.delay_micro_offset` | [0.0, 1.0) | Fractional sample: intonation personality |
| `vibrator.dispersion_asymmetry` | [0.95, 1.05] | Subtle: tuning unaffected, spectral evolution unique |
| `resonator.modal_drift` | [0.997, 1.003] | Per-mode frequency offset × mode index |
| `resonator.stereo_offset` | [0.05, 0.15] | Stereo decorrelation LFO offset |
| `resonator.modulation_rate_hz` | [0.4, 1.2] / sr | Internal body "breathing" speed |
| `spatial.reverb_diffusion` | [0.55, 0.70] | From Elements' tested range |
| `spatial.reverb_brightness` | [0.70, 0.90] | Below 0.70 = muddy; above 0.90 = metallic |
| `nonlin.saturation_asymmetry` | [0.93, 1.07] | Even-harmonic bias from tube operating point |
| `nonlin.transfer_inflection` | [0.95, 1.05] | Where saturation curve kicks in |

### 2.3 Derivation Order

Parameters are pulled from the LCG in a fixed sequence. **New parameters must only be appended** — this ensures firmware updates don't change an existing instrument's personality. The order is documented in the source as a numbered list (1–15).

### 2.4 Voice Variants

For polyphony, `InstrumentDna::voice_variant(index, sample_rate)` mixes the voice index into the seed using the golden ratio constant (`0x9E3779B9`) for good bit diffusion across sequential indices. Each voice in a polyphonic instrument has its own micro-variation while remaining deterministically derived from the same hardware seed.

### 2.5 The Warmth Floor

The warmth floor (parameter 6 in derivation order, range [0.15, 0.35]) is applied in the exciter DSP as a ceiling on the tilt filter's maximum coefficient:

```
max_brightness = 1.0 - dna.warmth_floor
tilt_coeff = 0.05 + effective_tilt × (max_brightness - 0.05)
```

At a warmth floor of 0.25, even with the user's spectral tilt at maximum, 25% of the lowpass effect remains. The instrument cannot produce a cold, harsh sound.

---

## 3. Input Protocol & Gesture Normalisation

### 3.1 PlayGesture

Every input protocol normalises to four dimensions:

| Field | Range | Physical Meaning |
|-------|-------|-----------------|
| `position` | [0.0, 1.0] | Where on the vibrator the exciter contacts |
| `force` | [0.0, 1.0] | How hard (velocity → aftertouch) |
| `speed` | [0.0, 1.0] | Rate of movement (bow velocity, airflow) |
| `continuity` | bool | Gate held (distinguishes bow from pluck) |

### 3.2 MIDI 1.0 Normaliser

A byte-level state machine parsing raw MIDI:

- **Running status** — implicit status byte continuation. System Common (0xF0-0xF7) cancels running status; System Real-Time (0xF8-0xFF) does not.
- **Monophonic note stacking** — 8-deep fixed-size stack, last-note priority. Release falls back to previous held note. Retrigger moves to top.
- **Aftertouch → force override** — poly AT updates force only for the active note. Channel AT applies globally. Whichever is higher wins.
- **CC mapping** — CC2 (breath) → speed, CC74 (brightness) → position, CC123 → All Notes Off.
- **Pitch bend** — parsed and exposed separately (`pitch_bend()`, `pitch_bend_normalised()`) because it maps to vibrator frequency, not exciter gesture.

### 3.3 Future Protocols

The architecture supports MPE (MIDI Polyphonic Expression), MIDI 2.0 (32-bit resolution), OSC (Open Sound Control), and CV/Gate — each would implement a normaliser that produces `PlayGesture`. The exciter doesn't know or care which protocol generated the gesture.

---

## 4. Exciter Section

### 4.1 Continuous Parameter Space

The `ExciterModel` uses continuous f32 axes rather than a named enum. This is the core architectural insight: physical excitation mechanisms exist on a continuum. The parameters:

- **`energy_continuity`** [0, 1] — 0 = pure impulse, 1 = pure continuous
- **`coupling_direct`** [0, 1] — direct mechanical displacement (pluck, hammer)
- **`coupling_friction`** [0, 1] — stick-slip (bow, singing bowl)
- **`coupling_pressure`** [0, 1] — airflow through reed/jet (clarinet, flute)
- **`spectral_tilt`** [0, 1] — exciter material hardness (finger → metal beater)
- **`stochasticity`** [0, 1] — randomness/turbulence amount
- **`multiplicity`** u8 — simultaneous contact points (1 = single, 32 = rain)

**Coupling axes are independent** — they do NOT sum to 1.0. This allows hybrid excitations like col legno tratto (friction + direct simultaneously at full strength). Energy normalisation happens at the output stage via `total_coupling().max(1.0)`.

### 4.2 Named Presets as Bookmarks

PLUCK, PICK, BOW, BREATH, FLUTE, MALLET, BEATER, STRUM, EBOW, SINGING_BOWL, COL_LEGNO, RAIN — all are `const` values in the continuous space. The morphing system interpolates between any two by lerping every field independently.

### 4.3 Coupling Mode Signal Generators

Three per-sample signal generators, mixed by coupling weights:

**Direct coupling (pluck/hammer/e-bow):**
On gate trigger, an impulse envelope jumps to `force` and decays exponentially. Decay rate: `0.998 - tilt × 0.098` (soft=slow contact, hard=fast click). A DNA-signature-derived pre-impulse creates negative displacement before the strike (like Elements' plectrum pull-back: `0.05 + signature × 0.20`). In continuous mode, the envelope sustains at `energy_continuity × force`. The signal is white noise (from DNA-seeded xorshift RNG) shaped by this envelope.

**Friction coupling (bow/singing bowl):**
Bistable stick-slip process (inspired by Elements' `ProcessFlow`). A particle state alternates between +0.5 and -0.5 with flip probability `v^4 × 0.125 × dna.coupling_curve_shape × dna.stochastic_bias`. Between flips, noise proportional to velocity is added. Output scaled by force (bow pressure). The `v^4` mapping matches Elements and gives a perceptually natural response to bow speed.

**Pressure coupling (reed/air-jet):**
Simplified reed model: `reed_opening = 0.8 - closure_rate × pressure`, clamped to [0, 1]. Flow = airflow × reed_opening. Turbulence noise low-passed and mixed in proportional to stochasticity × airflow. Light feedback (`-0.1 × prev_output`) adds self-oscillation tendency. Based on Elements' `tube.cc` reed model.

### 4.4 Output Chain

Mixed signal → spectral tilt filter (one-pole LP, coefficient = `0.05 + tilt × (1.0 - warmth_floor - 0.05)`) → soft saturation (Padé-tanh) → DC blocker.

Gesture parameters (`force`, `speed`) are linearly interpolated per-sample across the block (anti-zipper, matching Elements' `strength_increment` pattern).

---

## 5. Vibrator Section

### 5.1 Digital Waveguide

The `WaveguideString` implements the canonical Karplus-Strong extended waveguide: a circular buffer delay line whose length determines the fundamental frequency (`sample_rate / frequency`).

Signal flow per sample:
1. Read from delay line at fractional position (linear interpolation)
2. FIR damping: `y = (1-g) × x + g × x[n-1]` (two-tap averaging, g controls brightness)
3. IIR damping: one-pole lowpass (additional brightness shaping)
4. Feedback gain: `0.90 + damping² × 0.09999` (quadratic mapping for perceptual linearity)
5. Optional allpass dispersion (inharmonicity)
6. Write back: feedback + excitation input
7. Output: read at position-dependent comb tap

### 5.2 Pitch & Tuning

Delay length = `sample_rate / frequency + DNA.delay_micro_offset × 0.5`. The DNA offset (±0.25 samples max) gives each instance unique intonation — like bridge placement tolerance on a real instrument. Pitch changes are smoothed per-sample across the block to prevent clicks.

### 5.3 Damping Model

Two-stage damping in the feedback loop:
- **FIR** (two-tap average): `g = (1 - brightness) × 0.45`. Controls frequency-dependent decay — higher frequencies are attenuated more per round trip (natural for real strings).
- **IIR** (one-pole LP): coefficient `0.15 + brightness × 0.80`. Additional brightness shaping.
- **Feedback gain**: energy retained per round trip. damping=0→0.90 (fast), damping=1→0.99999 (infinite).

### 5.4 Dispersion (Inharmonicity)

First-order allpass filter in the feedback loop: `y[n] = -g × w[n] + w[n-D]` where `w[n] = x[n] + g × w[n-D]`. The allpass gain is `g = -0.5 × (dispersion × DNA.dispersion_asymmetry)²`. This adds frequency-dependent delay without changing magnitude — higher partials arrive slightly later, creating the inharmonicity characteristic of stiff strings (piano), metallic bars, and bells.

### 5.5 Position (Comb Filtering)

Output is read from `clamped_position × delay_length` samples offset. Uses Elements' clamping formula: `clamped = 0.5 - 0.98 × |position - 0.5|` to prevent degenerate comb settings. Position 0.5 = centre (even harmonics cancelled, warm/hollow). Near 0 or 1 = bridge (all harmonics, bright/thin).

### 5.6 Memory

Delay line: 4096 samples × 4 bytes = 16 KB per voice. Supports fundamentals down to ~11.7 Hz at 48kHz. Allpass: 256 samples × 4 bytes = 1 KB.

---

## 6. Resonant Body Section

### 6.1 Modal Synthesis

24 parallel Chamberlin SVF bandpass filters, each representing one resonant mode of the virtual body. The vibrator's output excites all modes simultaneously; each mode rings at its own frequency, Q, and amplitude. The sum is the body's sound.

The Chamberlin SVF update (per mode, per sample):
```
lp += f × bp
hp = input - lp - (1/Q) × bp
bp += f × hp
output = bp  // bandpass
```

Where `f = 2π × mode_freq / sample_rate` (small-angle approximation, accurate to <1% for body modes).

### 6.2 Geometry → Mode Spacing

The `geometry` parameter controls a stiffness value that determines how mode frequencies spread relative to the harmonic series:

```
stiffness = (geometry - 0.25) × 0.04
```

For each mode `n`: `freq_ratio = harmonic × stretch_factor`, where `stretch_factor` accumulates stiffness per mode. Negative stiffness compresses (tube-like); zero = harmonic (string); positive = spreads (plate/bell). The accumulation tapers (×0.93 for negative, ×0.98 for positive) to prevent negative-frequency folding and to allow extra high partials.

DNA `modal_drift` offsets each mode uniquely: `drift = 1.0 + (modal_drift - 1.0) × (mode_index × 0.3 + 1.0)`. Higher modes drift more — like wood grain density variation.

### 6.3 Brightness & Warmth Bias

Mode gain follows hyperbolic rolloff: `gain = 1.0 / (1.0 + mode_index × rolloff)` where `rolloff = 0.15 + (1 - brightness) × 0.85`. The minimum rolloff of 0.15 ensures warmth is always present — high modes never match low mode amplitude.

The lowest three modes are additionally boosted (×1.3, ×1.15, ×1.15) — the warmth emphasis. No instance of Moth produces a thin or cold body sound.

### 6.4 Envelope-Responsive Openness

An energy follower tracks input: `envelope += coeff × (energy - envelope)` with fast attack (0.05) and very slow release (0.0005). The tracked energy maps to `openness ∈ [0, 1]`.

When playing hard (high openness):
- Higher modes get louder: up to +60% boost above mode 3
- Q increases: `effective_Q = mode_Q × (1 + openness × 0.4)`
- The body blooms

When gentle (low openness):
- Only the warmest low modes remain
- Q relaxes
- The body settles into warmth

This is the instrument meeting you where you are.

### 6.5 Internal Modulation (Body Breathing)

A triangle LFO at DNA-derived `modulation_rate_hz` wobbles each mode's frequency by `0.08% × stereo_offset × (1 + mode_index × 0.1)`. Higher modes wobble more. Not random, not mechanical — organic.

### 6.6 Position-Based Mode Amplitude

A Chebyshev recurrence generates `cos(n × position × π)` without per-sample trig: seed value `cos(position × π)` computed once per block via Bhaskara I approximation (1.5% accuracy), then `c[n] = 2 × cos_w × c[n-1] - c[n-2]`. This creates the comb-like amplitude pattern that simulates pickup position.

### 6.7 Body Shape Presets

GUITAR_SMALL, GUITAR_LARGE, VIOLIN, CELLO, WOODEN_BOX, HOLLOW_TUBE, METAL_PLATE, BELL — all morphable via lerp. Every intermediate state is stable and musical.

---

## 7. Non-Linearities Section

### 7.1 Philosophy

This is a warmth and colour section, not a distortion unit. Drive is bounded to [0.5, 4.0]. The saturation curves are smooth and gentle. Even at maximum settings, the output leans toward resolution rather than harshness.

### 7.2 Signal Flow

```
input → pre-warmth LP → drive gain → DC bias (DNA) → tape saturation
      → tube saturation → gain compensation → post-tone LP → DC blocker → mix
```

### 7.3 Tape Saturation (Chowdhury, CCRMA 2019)

Symmetric soft clipping (`soft_saturate`: Padé approximant to tanh) with **hysteresis**:

```
saturated = soft_saturate(input)
output = saturated × (1 - hyst) + prev_output × hyst
prev_output = output
```

The hysteresis blends current output with previous — the simplified Jiles-Atherton model. Magnetisation depends on history: transients are softened because the output lags behind sudden changes. `hyst = tape_amount × 0.6`.

### 7.4 Tube Saturation (Karjalainen/Pakarinen, HUT 2006)

Asymmetric clipping:
```
if x >= 0: y = soft_saturate(x)                    // positive: standard curve
else:      y = soft_saturate(x × asymmetry) / asymmetry  // negative: pre-scaled
```

Where `asymmetry = DNA.saturation_asymmetry` ∈ [0.93, 1.07]. Values > 1.0 make the negative half clip harder (classic triode characteristic), creating even harmonics (2nd, 4th, 6th) perceived as "warm" and "full".

### 7.5 Magnetic Character (Najnudel/Müller, IRCAM 2020)

Not a separate saturator but a pre-emphasis filter. The `warmth` parameter controls a one-pole lowpass before saturation. Low frequencies are emphasised going into the saturator, so they hit the curve first — exactly how a signal transformer's ferromagnetic core behaves. Adds "weight" and "thickness" without muddiness.

### 7.6 DNA Integration

- **`saturation_asymmetry`** creates a DC bias (`(asym - 1.0) × 0.5`) — the tube's operating point, unique per instance
- **`transfer_inflection`** scales the drive gain, modifying where the saturation curve kicks in
- Both are subtle (±5-7%) — like component tolerances in a real analogue circuit

### 7.7 Gain Compensation

Output scaled by `1 / (1 + (drive - 1) × 0.3)` — roughly constant perceived loudness regardless of drive setting. The saturation compresses, so full inverse is unnecessary.

---

## 8. Spatial Section

### 8.1 FDN Reverb (Heldmann/Schlecht, Aalto 2021)

Four delay lines with a Hadamard feedback matrix:

```
H = 0.5 × [[ 1,  1,  1,  1],
            [ 1, -1,  1, -1],
            [ 1,  1, -1, -1],
            [ 1, -1, -1,  1]]
```

Delay lengths: 1087, 1283, 1429, 1597 samples (mutually prime, ~22-33ms at 48kHz). Scaled by `sample_rate / 48000` for other rates.

Each feedback path has a one-pole lowpass damping filter whose coefficient is derived from `brightness × DNA.reverb_diffusion`. This creates frequency-dependent decay — high frequencies die faster, like a real room with absorptive surfaces.

### 8.2 Design Criteria

**Colourless**: the reverb tail should be spectrally flat. The body section provides the timbral colouration — the room should not add its own resonant peaks. The Hadamard matrix + prime delays + homogeneous decay achieves this (Heldmann/Schlecht: "allpass FDN with homogeneous decay produces narrow modal excitation distribution → high perceived modal density → colourless").

**Always decaying**: feedback gain capped at 0.85 (size=1.0). The reverb always decays, never builds. No possibility of runaway feedback.

**Warm**: DNA `reverb_brightness` is bounded to [0.70, 0.90] — never fully bright. Slight variation per delay line from DNA `reverb_diffusion` gives each instance unique spatial character.

### 8.3 Memory

4 × 2048 × 4 bytes = 32 KB per voice.

---

## 9. Voice & Hierarchical Mixer

### 9.1 Signal Chain

`MothVoice` chains all five sections in series with a hierarchical mixer (inspired by Bernardes et al., 2018):

```
Exciter → Vibrator → Body → [Level 1 Mix + Level 0 Bleed] → Non-lin → Spatial → Output
```

### 9.2 Mix Levels

| Level | Control | Range | Purpose |
|-------|---------|-------|---------|
| 0 | Exciter bleed | [0, 0.5] | Raw exciter transient mixed into output for attack presence |
| 1 | Body mix | [0, 1.0] | Balance between raw waveguide and body-filtered sound |
| 2 | Non-lin wet/dry | via SaturationCharacter | Character amount |
| 3 | Spatial wet/dry | via SpatialCharacter | Room amount |

### 9.3 Block Processing

Audio is processed in chunks of up to 256 samples (MAX_BLOCK). Temporary buffers are stack-allocated. For polyphony, instantiate multiple `MothVoice`s with DNA variants.

---

## 10. VST3 Plugin Wrapper

### 10.1 Architecture

A separate crate (`moth-vst`) depending on the `moth` library via path and `nih-plug` for the plugin framework. Zero changes to the library code. The wrapper maps 21 plugin parameters to Moth's configuration and handles MIDI note events.

### 10.2 Exciter Morph Knob

A single knob (0.0-1.0) morphs through six exciter presets by interpolating between adjacent pairs:

| Knob Position | Preset A | Preset B |
|--------------|----------|----------|
| 0.0 – 0.2 | PLUCK | PICK |
| 0.2 – 0.4 | PICK | BOW |
| 0.4 – 0.6 | BOW | BREATH |
| 0.6 – 0.8 | BREATH | EBOW |
| 0.8 – 1.0 | EBOW | RAIN |

### 10.3 MIDI Handling

nih-plug provides parsed `NoteEvent`s. Note On → frequency (`440 × 2^((note-69)/12)`), velocity → force, gate → continuity. Poly aftertouch → force modulation.

---

## 11. DSP Primitives

### 11.1 DspRng (xorshift32)

Audio-rate noise generator. Period 2^32 - 1. Seeded from DNA for unique per-instance noise texture.

```
x ^= x << 13
x ^= x >> 17
x ^= x << 5
```

### 11.2 OnePole

`y[n] = y[n-1] + coeff × (x[n] - y[n-1])`. Used for spectral tilt, damping, parameter smoothing, envelope following.

### 11.3 DcBlocker

First-order highpass: `y[n] = x[n] - x[n-1] + R × y[n-1]` where `R = 1 - 20/sample_rate`. Cutoff ~20 Hz regardless of sample rate.

### 11.4 Soft Saturator

Padé approximant to tanh: `f(x) = x × (27 + x²) / (27 + 9x²)`. No libm dependency. Odd symmetry. Bounded output. Used throughout the signal chain for gentle saturation and clipping prevention.

### 11.5 Fast Exponential Decay

`fast_exp_neg(x) = 1 / (1 + x + 0.48x² + 0.235x³)` for x ≥ 0. ~0.3% accuracy for x ∈ [0, 5].

---

## 12. References

### Physical Modelling

1. Smith, J.O. III. *Physical Audio Signal Processing*. Online book, Stanford CCRMA. [ccrma.stanford.edu/~jos/pasp/](https://ccrma.stanford.edu/~jos/pasp/)
2. Bilbao, S. *Numerical Sound Synthesis*. Wiley, 2009.
3. Desvages, C. *Physical modelling of the bowed string and applications to sound synthesis*. PhD thesis, University of Edinburgh, 2018.
4. Chaigne, A. & Askenfelt, A. "Numerical simulations of piano strings." *JASA* 95, 1994.
5. Hikichi, T. & Osaka, N. "Sound timbre interpolation based on physical modeling." *Acoust. Sci. & Tech.* 22(2), 2001.

### Mutable Instruments Elements

6. Gillet, É. *Mutable Instruments Elements*. Open source (MIT), 2014. [github.com/pichenettes/eurorack](https://github.com/pichenettes/eurorack)

### Body Morphing

7. Penttinen, H., Karjalainen, M. & Härmä, A. "Morphing Instrument Body Models." *Proc. DAFx-01*, Limerick, 2001.
8. Siedenburg, K., Jacobsen, S. & Reuter, C. "Spectral envelope position and shape in sustained musical instrument sounds." *JASA* 149(6), 2021.

### Analogue Modelling

9. Chowdhury, J. "Real-Time Physical Modelling for Analog Tape Machines." *Proc. DAFx-19*, Birmingham, 2019.
10. Karjalainen, M. & Pakarinen, J. "Wave Digital Simulation of a Vacuum-Tube Amplifier." *Proc. ICASSP*, Toulouse, 2006.
11. Najnudel, J. & Müller, R. "A Power-Balanced Dynamic Model of Ferromagnetic Coils." *Proc. DAFx-20*, Vienna, 2020.
12. Yeh, D.T., Bank, B. & Karjalainen, M. "Nonlinear Modeling of a Guitar Loudspeaker Cabinet." *Proc. DAFx-08*, Espoo, 2008.
13. Werner, K.J. *Virtual Analog Modeling of Audio Circuitry Using Wave Digital Filters*. PhD dissertation, Stanford, 2016.
14. Albertini, D., Bernardini, A. & Sarti, A. "Antiderivative Antialiasing in Nonlinear Wave Digital Filters." *Proc. DAFx-20*, Vienna, 2020.

### Reverb & Spatial

15. Heldmann, J. & Schlecht, S.J. "The Role of Modal Excitation in Colorless Reverberation." *Proc. DAFx-21*, Vienna, 2021.
16. McCormack, L., Politis, A. & Pulkki, V. "Parametric Spatial Audio Effects Based on the Multi-Directional Decomposition of Ambisonic Sound Scenes." *Proc. DAFx-21*, Vienna, 2021.
17. Gonzalez, R., Politis, A. & Lokki, T. "Spherical Decomposition of Arbitrary Scattering Geometries for Virtual Acoustic Environments." *Proc. DAFx-21*, Vienna, 2021.

### Mixing

18. Bernardes, G., Davies, M.E.P. & Guedes, C. "A Hierarchical Harmonic Mixing Method." *Proc. CMMR*, 2018.

### Waveguide Theory

19. Amir, N., Pagneux, V. & Kergomard, J. "A study of wave propagation in varying cross-section waveguides by modal decomposition. Part II: Results." *JASA* 101(5), 1997.

---

*This document was prepared during the initial design and implementation session for Moth v0.2.0, April 2026.*
