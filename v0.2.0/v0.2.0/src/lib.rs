//! # Moth
//!
//! A physical modelling synthesiser — maths-first, pure Rust.
//!
//! Signal chain: Exciter → Vibrator → Resonant Body → Non-Lins → Spatial
//!
//! Every instance of Moth is alive and unrepeatable. At first boot, a
//! hardware seed is woven through a constellation of modifiers — subtle
//! biases in tone, in warmth, in how parameters lean into one another.
//! That process happens once. What emerges is the specific character of
//! *your* Moth: not a variation on a theme, but a distinct voice.
//!
//! Every parameter affects every other. Nothing lives in isolation. Push
//! the texture and the warmth shifts; alter the depth and the envelope
//! breathes differently. But the surface gives none of this away — what
//! you find there is intuitive, responsive, kind. Every combination you
//! discover is a sweetspot.
//!
//! No instance of Moth is ever cold.
//!
//! This crate is `no_std` — all DSP and parameter logic runs on bare metal
//! with zero heap allocation.

#![no_std]

#[cfg(test)]
extern crate std;

pub mod instrument_dna;
pub mod gesture;
pub mod exciter;
pub mod exciter_dsp;
pub mod vibrator;
pub mod resonator;
pub mod nonlin;
pub mod spatial;
pub mod voice;
pub mod midi1;
pub mod dsp_core;
