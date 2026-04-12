//! Normalised performance gesture — the universal input to the exciter section.
//!
//! Every input protocol (MIDI 1.0, MPE, MIDI 2.0, OSC, CV/Gate) normalises
//! to [`PlayGesture`]. The exciter doesn't know or care which protocol
//! generated the gesture — it reads these four values and that's it.
//!
//! This separation means adding a new input protocol is purely an input-side
//! concern: implement a normaliser that produces `PlayGesture`, done.

/// Normalised performance gesture.
///
/// Four dimensions of expression, protocol-agnostic. Each dimension maps
/// to a physically meaningful aspect of how an instrument is played.
///
/// `PlayGesture` is the *player's intent*. It modulates on top of the
/// `ExciterModel`'s *character* — two independent expression layers
/// operating simultaneously.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayGesture {
    /// Where on the vibrator the exciter makes contact.
    ///
    /// `0.0` = bridge/mouthpiece end (brighter, thinner).
    /// `1.0` = nut/bell end (warmer, rounder).
    /// `0.5` = centre (neutral).
    ///
    /// Source mapping:
    /// - MPE: per-note slide (CC74 / X axis)
    /// - MIDI 1.0: CC74 (brightness)
    /// - MIDI 2.0: per-note CC74 (32-bit)
    /// - OSC: `/gesture/position`
    /// - CV: dedicated position CV input
    pub position: f32,

    /// How hard the exciter is applied.
    ///
    /// `0.0` = silence / no contact.
    /// `1.0` = maximum force.
    ///
    /// For impulse exciters (pluck, hit): initial strike intensity.
    /// For continuous exciters (bow, breath): sustained pressure/force.
    ///
    /// Source mapping:
    /// - MPE: pressure (Z / channel aftertouch per note)
    /// - MIDI 1.0: velocity on attack → poly/channel aftertouch while held
    /// - MIDI 2.0: per-note velocity (32-bit) → per-note pressure
    /// - OSC: `/gesture/force`
    /// - CV: gate level or pressure CV
    pub force: f32,

    /// Rate of exciter movement.
    ///
    /// `0.0` = stationary.
    /// `1.0` = maximum speed.
    ///
    /// Physical meaning varies by exciter type:
    /// - Bow: bow velocity (critical — determines friction energy)
    /// - Breath: airflow rate
    /// - Pluck: largely irrelevant (force dominates)
    ///
    /// Each coupling mode in the `ExciterModel` interprets `speed`
    /// according to its own physics.
    ///
    /// Source mapping:
    /// - MPE: slide delta rate (X velocity)
    /// - MIDI 1.0: CC2 (breath controller)
    /// - MIDI 2.0: per-note CC2 (32-bit)
    /// - OSC: `/gesture/speed`
    /// - CV: dedicated speed/breath CV
    pub speed: f32,

    /// Whether the note is currently held (gate open).
    ///
    /// `true` = gate held. `false` = gate released.
    ///
    /// This is *player intent*, distinct from `ExciterModel.energy_continuity`
    /// which is *model character*. A held note (`continuity: true`) with
    /// `energy_continuity: 0.0` gives an impulse that rings out while the
    /// gate is open. A released note (`continuity: false`) with
    /// `energy_continuity: 1.0` stops the bow/breath.
    pub continuity: bool,
}

impl PlayGesture {
    /// A silent, neutral gesture — no note playing, all values at rest.
    pub const SILENT: Self = Self {
        position: 0.5,
        force: 0.0,
        speed: 0.0,
        continuity: false,
    };

    /// Clamp all continuous fields to their valid `[0.0, 1.0]` range.
    ///
    /// Call this after constructing a gesture from external input to
    /// guarantee downstream code never sees out-of-range values.
    #[inline]
    pub fn clamped(self) -> Self {
        Self {
            position: self.position.clamp(0.0, 1.0),
            force: self.force.clamp(0.0, 1.0),
            speed: self.speed.clamp(0.0, 1.0),
            continuity: self.continuity,
        }
    }

    /// Linearly interpolate between two gestures.
    ///
    /// `t = 0.0` → `self`, `t = 1.0` → `other`.
    /// Useful for smoothing gesture transitions to avoid zipper noise.
    /// `continuity` snaps to `other` when `t >= 0.5`.
    #[inline]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            position: self.position + (other.position - self.position) * t,
            force: self.force + (other.force - self.force) * t,
            speed: self.speed + (other.speed - self.speed) * t,
            continuity: if t >= 0.5 {
                other.continuity
            } else {
                self.continuity
            },
        }
    }
}

impl Default for PlayGesture {
    fn default() -> Self {
        Self::SILENT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_is_neutral() {
        let g = PlayGesture::SILENT;
        assert_eq!(g.position, 0.5);
        assert_eq!(g.force, 0.0);
        assert_eq!(g.speed, 0.0);
        assert!(!g.continuity);
    }

    #[test]
    fn default_is_silent() {
        assert_eq!(PlayGesture::default(), PlayGesture::SILENT);
    }

    #[test]
    fn clamp_enforces_range() {
        let g = PlayGesture {
            position: -0.5,
            force: 1.5,
            speed: 2.0,
            continuity: true,
        }
        .clamped();

        assert_eq!(g.position, 0.0);
        assert_eq!(g.force, 1.0);
        assert_eq!(g.speed, 1.0);
        assert!(g.continuity);
    }

    #[test]
    fn lerp_endpoints() {
        let a = PlayGesture {
            position: 0.0,
            force: 0.0,
            speed: 0.0,
            continuity: false,
        };
        let b = PlayGesture {
            position: 1.0,
            force: 1.0,
            speed: 1.0,
            continuity: true,
        };

        let at_a = a.lerp(b, 0.0);
        assert_eq!(at_a.position, 0.0);
        assert!(!at_a.continuity);

        let at_b = a.lerp(b, 1.0);
        assert_eq!(at_b.position, 1.0);
        assert!(at_b.continuity);

        let mid = a.lerp(b, 0.5);
        assert!((mid.position - 0.5).abs() < 1e-6);
        assert!((mid.force - 0.5).abs() < 1e-6);
        // At t=0.5, continuity snaps to `other`
        assert!(mid.continuity);
    }

    #[test]
    fn copy_and_clone() {
        let g = PlayGesture::SILENT;
        let _copy = g;
        let _clone = g.clone();
        assert_eq!(_copy, _clone);
    }
}
