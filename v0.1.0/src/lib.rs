//! PulsePhysics — physical modelling synthesiser, maths-first, pure Rust.
//!
//! Signal chain: Exciter → Vibrator → Resonant Body → Non-Lins → Spatial
//!
//! Every instance of this instrument has a unique timbral personality derived
//! deterministically from a hardware seed via [`InstrumentDna`].
//!
//! This crate is `no_std` — all DSP and parameter logic runs on bare metal
//! with zero heap allocation. Only test code uses `std`.

#![no_std]

#[cfg(test)]
extern crate std;

pub mod instrument_dna;
pub mod gesture;
pub mod exciter;
pub mod exciter_dsp;
pub mod midi1;
pub mod dsp_core;
