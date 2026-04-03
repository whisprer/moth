# Moth — Architecture Overview

**A physical modelling synthesiser. Maths-first, pure Rust, no_std. Each instance alive and unrepeatable.**

*Version 0.2.0 — April 2026*

---

## Identity

Moth is named after a person, not a creature. His name is woven into the root of every instance — the constant `0x6D6F7468` ("moth" in ASCII) is XOR'd into the seed derivation that generates each instrument's unique personality. This is not a dedication. It is genetic.

No instance of Moth is ever cold.

---

## Signal Chain

```
MIDI → Midi1Normaliser → PlayGesture
                              ↓
ExciterModel ──→ ExciterProcessor ──┬──→ WaveguideString ──→ ResonantBody
                     ↑              │         ↑                    ↑
                 ExciterDna     bleed     VibratorDna          ResonatorDna
                                    ↓         ↓                    ↓
                              [Hierarchical Mix: vibrator/body balance]
                                              ↓
                                    NonLinProcessor ← NonLinDna
                                              ↓
                                    SpatialProcessor ← SpatialDna
                                              ↓
                                           output

              ← All DNA from InstrumentDna(seed ^ MOTH) →
```

---

## Modules (12 source files, ~7500 lines)

| File | Lines | Purpose |
|------|-------|---------|
| `instrument_dna.rs` | 656 | Per-instance personality from hardware seed. 15 derived parameters across 5 sections. Warmth floor. |
| `gesture.rs` | 199 | `PlayGesture` — normalised 4D input (position, force, speed, continuity). |
| `exciter.rs` | 503 | `ExciterModel` — continuous parameter space (no enums). 12 named presets. Morphable via lerp. |
| `exciter_dsp.rs` | 1063 | Three coupling mode signal generators: direct (pluck/hammer), friction (bow), pressure (breath). |
| `vibrator.rs` | 890 | `WaveguideString` — delay line + FIR/IIR damping + allpass dispersion + position comb. |
| `resonator.rs` | 963 | `ResonantBody` — 24-mode modal synthesis (SVF bandpass bank). Envelope-responsive openness. |
| `nonlin.rs` | 731 | Tape saturation (hysteresis), tube saturation (asymmetric), magnetic warmth (pre-emphasis). |
| `spatial.rs` | 369 | FDN reverb (4 delay, Hadamard matrix, prime lengths). Colourless design. |
| `voice.rs` | 370 | `MothVoice` — chains all sections with hierarchical mixer (4 blend levels). |
| `midi1.rs` | 912 | MIDI 1.0 byte parser + `PlayGesture` normaliser. Note stacking, running status, aftertouch. |
| `dsp_core.rs` | 382 | Primitives: xorshift RNG, one-pole filter, DC blocker, smoother, soft saturator. |
| `lib.rs` | 39 | Crate root. `#![no_std]`. |

**Plugin wrapper** (`moth-vst/`, separate crate): 482 lines. nih-plug VST3/CLAP bridge. 21 parameters. Zero changes to library code.

---

## DNA System

Each physical Moth derives a unique personality from a hardware seed (MCU unique device ID):

```
seed ^ 0x6D6F7468 ("moth") → LCG (Knuth's constants) → 15 parameters
```

**Derivation order** (append-only for firmware compatibility):

1–6: Exciter — signature, noise_phase_offset, stochastic_bias, spectral_tilt_bias [0.88,1.02], coupling_curve_shape, **warmth_floor** [0.15,0.35]
7–8: Vibrator — delay_micro_offset, dispersion_asymmetry
9–11: Resonator — modal_drift, stereo_offset, modulation_rate_hz
12–13: Spatial — reverb_diffusion, reverb_brightness
14–15: Non-lin — saturation_asymmetry, transfer_inflection

**Key constraints:**
- Spectral tilt bias range [0.88, 1.02] — asymmetric, population trends warm
- Warmth floor [0.15, 0.35] — caps maximum tilt filter brightness
- All ranges are "all-sweetspot" — every possible value sounds good

---

## Key Design Principles

1. **Continuous, not discrete.** ExciterModel uses f32 axes, not enums. The morphing system interpolates between any two states by lerping every field.

2. **Independent coupling axes.** Direct, friction, and pressure coupling are independent [0,1] — not sum-to-1. Hybrid excitations at full strength simultaneously.

3. **DNA is constitutive, not decorative.** The MOTH constant, warmth floor, and per-instance variation are structural. They cannot be turned off.

4. **Envelope-responsive body.** The resonant body opens when you play hard (higher modes bloom, Q increases) and settles when you're gentle. The instrument meets you where you are.

5. **Warmth floor.** No instance is ever cold. The tilt filter's maximum brightness is `1.0 - warmth_floor`. Even at maximum user brightness, warmth remains.

6. **`no_std` from day one.** Zero heap allocation, zero dependencies. Runs on bare metal ARM Cortex-M. Plugin wrapper adds std only at the boundary.

---

## Dependencies

**Library:** None. Pure Rust, no_std, zero crates.
**Plugin:** `nih-plug` (VST3/CLAP framework), `moth` (path dependency).

---

## Build

```bash
# Library tests
cd v0.2.0 && cargo test

# VST3 plugin
cd moth-vst && cargo xtask bundle moth-vst --release
# Output: target/bundled/Moth.vst3
```

---

## Memory Budget (per voice, 48kHz)

| Component | Bytes |
|-----------|-------|
| Exciter processor | ~200 |
| Waveguide delay line (4096 × f32) | 16,384 |
| Allpass dispersion (256 × f32) | 1,024 |
| Resonator (24 modes × SVF) | ~400 |
| Non-lin processor | ~100 |
| Spatial FDN (4 × 2048 × f32) | 32,768 |
| **Total per voice** | **~51 KB** |

4-voice polyphony: ~204 KB. Feasible on embedded targets with ≥256 KB RAM.

---

## Mathematical References (used for understanding only, not as runtime dependencies)

- Julius O. Smith III — *Physical Audio Signal Processing* (Stanford CCRMA)
- Stefan Bilbao — *Numerical Sound Synthesis* (Wiley)
- Charlotte Desvages — PhD thesis, bowed string modelling (Edinburgh 2018)
- Mutable Instruments Elements — Émilie Gillet (open source, MIT)
- Jatin Chowdhury — *Real-Time Physical Modelling for Analog Tape Machines* (CCRMA 2019)
- Heldmann & Schlecht — *The Role of Modal Excitation in Colorless Reverberation* (Aalto 2021)
- Penttinen, Karjalainen & Härmä — *Morphing Instrument Body Models* (HUT 2001)
- Hikichi & Osaka — *Sound Timbre Interpolation Based on Physical Modeling* (NTT 2001)
- Karjalainen & Pakarinen — *Wave Digital Simulation of a Vacuum-Tube Amplifier* (HUT 2006)
- Najnudel & Müller — *A Power-Balanced Dynamic Model of Ferromagnetic Coils* (IRCAM 2020)
