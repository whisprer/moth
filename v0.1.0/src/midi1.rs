//! MIDI 1.0 byte-level parser and [`PlayGesture`] normaliser.
//!
//! Processes raw MIDI bytes one at a time via [`Midi1Normaliser::feed_byte`],
//! maintaining internal state (note tracking, CC values, aftertouch, pitch bend).
//! The current normalised gesture is always available via [`Midi1Normaliser::gesture`].
//!
//! # Supported messages
//!
//! - Note On / Note Off (with velocity-as-note-off)
//! - Poly Aftertouch
//! - Channel Aftertouch
//! - Control Change: CC2 (breath), CC74 (brightness/position)
//! - Pitch Bend
//!
//! # Channel filtering
//!
//! The normaliser can listen to a specific MIDI channel (0–15) or all
//! channels (omni mode). Set via [`Midi1Normaliser::new`].
//!
//! # Monophonic
//!
//! This normaliser is monophonic — it tracks the most recently played note.
//! Last-note priority with note stacking (releasing a held note falls back
//! to the previous held note, up to 8 deep). Polyphonic handling comes
//! with the MPE normaliser.
//!
//! # No dependencies
//!
//! Parses raw bytes directly — no MIDI library dependency. `no_std` compatible.

use crate::gesture::PlayGesture;

// ─── MIDI constants ─────────────────────────────────────────────────────────

const STATUS_NOTE_OFF: u8 = 0x80;
const STATUS_NOTE_ON: u8 = 0x90;
const STATUS_POLY_AT: u8 = 0xA0;
const STATUS_CC: u8 = 0xB0;
const STATUS_CHAN_AT: u8 = 0xD0;
const STATUS_PITCH_BEND: u8 = 0xE0;

const CC_BREATH: u8 = 2;
const CC_BRIGHTNESS: u8 = 74;

/// Maximum depth of the note stack for last-note-priority monophonic handling.
const NOTE_STACK_SIZE: usize = 8;

// ─── Parsed MIDI events ─────────────────────────────────────────────────────

/// A parsed MIDI 1.0 channel voice message.
///
/// Returned by [`Midi1Normaliser::feed_byte`] when a complete message has
/// been assembled. Useful for logging/debugging — the normaliser has already
/// updated its internal state by the time this is returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiEvent {
    NoteOn { channel: u8, note: u8, velocity: u8 },
    NoteOff { channel: u8, note: u8, velocity: u8 },
    PolyAftertouch { channel: u8, note: u8, pressure: u8 },
    ChannelAftertouch { channel: u8, pressure: u8 },
    ControlChange { channel: u8, controller: u8, value: u8 },
    PitchBend { channel: u8, value: i16 },
}

// ─── Parser state machine ───────────────────────────────────────────────────

/// Current state of the MIDI byte parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParserState {
    /// Waiting for a status byte or using running status.
    Idle,
    /// Have status, waiting for first data byte.
    WaitData1 { status: u8 },
    /// Have status + first data byte, waiting for second.
    WaitData2 { status: u8, data1: u8 },
}

// ─── Note stack ─────────────────────────────────────────────────────────────

/// Fixed-size note stack for last-note-priority monophonic voice allocation.
///
/// When a new note is pressed, it's pushed on top. When a note is released,
/// it's removed from wherever it sits in the stack, and the top of the
/// stack becomes the active note. This gives natural monophonic legato
/// behaviour.
#[derive(Debug, Clone)]
struct NoteStack {
    notes: [u8; NOTE_STACK_SIZE],
    velocities: [u8; NOTE_STACK_SIZE],
    len: usize,
}

impl NoteStack {
    const fn new() -> Self {
        Self {
            notes: [0; NOTE_STACK_SIZE],
            velocities: [0; NOTE_STACK_SIZE],
            len: 0,
        }
    }

    /// Push a note onto the stack. If the note is already in the stack,
    /// move it to the top (retrigger). If the stack is full, evict the
    /// oldest note.
    fn push(&mut self, note: u8, velocity: u8) {
        // Remove if already present (retrigger)
        self.remove(note);

        // If full, shift everything down (evict oldest)
        if self.len >= NOTE_STACK_SIZE {
            for i in 0..(NOTE_STACK_SIZE - 1) {
                self.notes[i] = self.notes[i + 1];
                self.velocities[i] = self.velocities[i + 1];
            }
            self.len = NOTE_STACK_SIZE - 1;
        }

        self.notes[self.len] = note;
        self.velocities[self.len] = velocity;
        self.len += 1;
    }

    /// Remove a specific note from the stack (note-off).
    /// Returns true if the note was found and removed.
    fn remove(&mut self, note: u8) -> bool {
        if let Some(idx) = self.notes[..self.len].iter().position(|&n| n == note) {
            // Shift everything above down
            for i in idx..(self.len - 1) {
                self.notes[i] = self.notes[i + 1];
                self.velocities[i] = self.velocities[i + 1];
            }
            self.len -= 1;
            true
        } else {
            false
        }
    }

    /// Returns the top (most recent) note and its velocity, if any.
    fn top(&self) -> Option<(u8, u8)> {
        if self.len > 0 {
            Some((self.notes[self.len - 1], self.velocities[self.len - 1]))
        } else {
            None
        }
    }

    /// Returns true if the stack is empty (no notes held).
    fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Clear all notes.
    fn clear(&mut self) {
        self.len = 0;
    }
}

// ─── The normaliser ─────────────────────────────────────────────────────────

/// MIDI 1.0 to [`PlayGesture`] normaliser.
///
/// Feed it raw MIDI bytes. Poll [`gesture()`](Midi1Normaliser::gesture)
/// for the current normalised state at any time.
///
/// # Example
///
/// ```
/// use pulse_physics::midi1::Midi1Normaliser;
///
/// let mut norm = Midi1Normaliser::new(None); // omni mode
///
/// // Note On: channel 0, note 60, velocity 100
/// norm.feed_byte(0x90);
/// norm.feed_byte(60);
/// let event = norm.feed_byte(100);
///
/// let g = norm.gesture();
/// assert!(g.continuity);
/// assert!(g.force > 0.5);
/// ```
#[derive(Debug, Clone)]
pub struct Midi1Normaliser {
    /// Channel filter: `None` = omni (accept all), `Some(ch)` = specific.
    channel_filter: Option<u8>,

    /// Byte parser state.
    parser: ParserState,

    /// Running status for MIDI running status support.
    running_status: Option<u8>,

    /// Note stack for monophonic last-note-priority.
    note_stack: NoteStack,

    /// Current force level (from velocity or aftertouch).
    force: f32,

    /// Current position (from CC74).
    position: f32,

    /// Current speed (from CC2 breath controller).
    speed: f32,

    /// Current pitch bend value, raw signed.
    pitch_bend_raw: i16,

    /// Most recent poly aftertouch value for the active note.
    poly_aftertouch: f32,

    /// Most recent channel aftertouch value.
    channel_aftertouch: f32,
}

impl Midi1Normaliser {
    /// Create a new normaliser.
    ///
    /// # Arguments
    ///
    /// * `channel_filter` — `None` for omni mode (accept all channels),
    ///   `Some(0..=15)` to listen to a specific channel.
    pub fn new(channel_filter: Option<u8>) -> Self {
        Self {
            channel_filter,
            parser: ParserState::Idle,
            running_status: None,
            note_stack: NoteStack::new(),
            force: 0.0,
            position: 0.5,
            speed: 0.0,
            pitch_bend_raw: 0,
            poly_aftertouch: 0.0,
            channel_aftertouch: 0.0,
        }
    }

    /// Feed a single raw MIDI byte into the parser.
    ///
    /// Returns `Some(MidiEvent)` when a complete message has been parsed
    /// and the internal state updated. Returns `None` when more bytes are
    /// needed to complete the current message.
    ///
    /// System messages (0xF0–0xFF) are silently ignored.
    pub fn feed_byte(&mut self, byte: u8) -> Option<MidiEvent> {
        // System messages: ignore entirely, don't disturb parser state
        if byte >= 0xF0 {
            // System Real-Time (0xF8–0xFF) can appear mid-message
            // without disrupting the current parse. System Common
            // (0xF0–0xF7) cancel any running status.
            if byte < 0xF8 {
                self.running_status = None;
                self.parser = ParserState::Idle;
            }
            return None;
        }

        // Status byte?
        if byte & 0x80 != 0 {
            let msg_type = byte & 0xF0;
            let channel = byte & 0x0F;

            // Channel filter
            if let Some(ch) = self.channel_filter {
                if channel != ch {
                    self.parser = ParserState::Idle;
                    self.running_status = None;
                    return None;
                }
            }

            self.running_status = Some(byte);

            // Channel Aftertouch and Program Change are 1-data-byte messages
            if msg_type == STATUS_CHAN_AT || msg_type == 0xC0 {
                self.parser = ParserState::WaitData1 { status: byte };
            } else {
                // All other channel voice messages are 2-data-byte
                self.parser = ParserState::WaitData1 { status: byte };
            }
            return None;
        }

        // Data byte (0x00–0x7F)
        match self.parser {
            ParserState::Idle => {
                // Data byte with no pending status — try running status
                if let Some(status) = self.running_status {
                    let msg_type = status & 0xF0;
                    if msg_type == STATUS_CHAN_AT || msg_type == 0xC0 {
                        // 1-data-byte message via running status
                        return self.complete_message(status, byte, 0);
                    } else {
                        // 2-data-byte message — this is data1
                        self.parser = ParserState::WaitData2 { status, data1: byte };
                        return None;
                    }
                }
                // No running status — stray data byte, ignore
                None
            }

            ParserState::WaitData1 { status } => {
                let msg_type = status & 0xF0;
                if msg_type == STATUS_CHAN_AT || msg_type == 0xC0 {
                    // 1-data-byte message complete
                    self.parser = ParserState::Idle;
                    self.complete_message(status, byte, 0)
                } else {
                    // 2-data-byte message — need one more
                    self.parser = ParserState::WaitData2 { status, data1: byte };
                    None
                }
            }

            ParserState::WaitData2 { status, data1 } => {
                // 2-data-byte message complete
                self.parser = ParserState::Idle;
                self.complete_message(status, data1, byte)
            }
        }
    }

    /// Process a completed MIDI message and update internal state.
    fn complete_message(&mut self, status: u8, data1: u8, data2: u8) -> Option<MidiEvent> {
        let msg_type = status & 0xF0;
        let channel = status & 0x0F;

        match msg_type {
            STATUS_NOTE_ON => {
                if data2 == 0 {
                    // Velocity 0 = Note Off (common MIDI convention)
                    self.handle_note_off(data1);
                    Some(MidiEvent::NoteOff {
                        channel,
                        note: data1,
                        velocity: 0,
                    })
                } else {
                    self.handle_note_on(data1, data2);
                    Some(MidiEvent::NoteOn {
                        channel,
                        note: data1,
                        velocity: data2,
                    })
                }
            }

            STATUS_NOTE_OFF => {
                self.handle_note_off(data1);
                Some(MidiEvent::NoteOff {
                    channel,
                    note: data1,
                    velocity: data2,
                })
            }

            STATUS_POLY_AT => {
                // Only update if this is the currently active note
                if let Some((active_note, _)) = self.note_stack.top() {
                    if data1 == active_note {
                        self.poly_aftertouch = data2 as f32 / 127.0;
                        self.update_force();
                    }
                }
                Some(MidiEvent::PolyAftertouch {
                    channel,
                    note: data1,
                    pressure: data2,
                })
            }

            STATUS_CC => {
                match data1 {
                    CC_BREATH => {
                        self.speed = data2 as f32 / 127.0;
                    }
                    CC_BRIGHTNESS => {
                        self.position = data2 as f32 / 127.0;
                    }
                    123 => {
                        // All Notes Off
                        self.note_stack.clear();
                        self.force = 0.0;
                        self.poly_aftertouch = 0.0;
                        self.channel_aftertouch = 0.0;
                    }
                    _ => {} // Other CCs: ignored for now
                }
                Some(MidiEvent::ControlChange {
                    channel,
                    controller: data1,
                    value: data2,
                })
            }

            STATUS_CHAN_AT => {
                self.channel_aftertouch = data1 as f32 / 127.0;
                self.update_force();
                Some(MidiEvent::ChannelAftertouch {
                    channel,
                    pressure: data1,
                })
            }

            STATUS_PITCH_BEND => {
                // Pitch bend: 14-bit value, data1=LSB, data2=MSB
                // Centre = 8192, range 0–16383
                let raw = ((data2 as i16) << 7) | (data1 as i16);
                self.pitch_bend_raw = raw - 8192;
                Some(MidiEvent::PitchBend {
                    channel,
                    value: self.pitch_bend_raw,
                })
            }

            _ => None, // Program Change, etc. — ignored
        }
    }

    /// Handle a Note On event.
    fn handle_note_on(&mut self, note: u8, velocity: u8) {
        self.note_stack.push(note, velocity);
        self.force = velocity as f32 / 127.0;
        self.poly_aftertouch = 0.0;
    }

    /// Handle a Note Off event.
    fn handle_note_off(&mut self, note: u8) {
        self.note_stack.remove(note);

        if let Some((_prev_note, prev_vel)) = self.note_stack.top() {
            // Fall back to previous held note
            self.force = prev_vel as f32 / 127.0;
            self.poly_aftertouch = 0.0;
        } else {
            // No notes held
            self.force = 0.0;
            self.poly_aftertouch = 0.0;
            self.channel_aftertouch = 0.0;
        }
    }

    /// Recalculate force from velocity + aftertouch.
    /// Aftertouch overrides velocity when present (non-zero).
    fn update_force(&mut self) {
        let at = self.poly_aftertouch.max(self.channel_aftertouch);
        if at > 0.0 {
            self.force = at;
        }
    }

    /// The current normalised gesture.
    ///
    /// Always valid — returns [`PlayGesture::SILENT`] when no notes are held.
    /// Call this at your audio rate or control rate to get the current state.
    #[inline]
    pub fn gesture(&self) -> PlayGesture {
        PlayGesture {
            position: self.position,
            force: self.force,
            speed: self.speed,
            continuity: !self.note_stack.is_empty(),
        }
    }

    /// The currently active MIDI note number, if any.
    #[inline]
    pub fn active_note(&self) -> Option<u8> {
        self.note_stack.top().map(|(note, _)| note)
    }

    /// The raw pitch bend value (-8192 to +8191).
    ///
    /// Not part of `PlayGesture` because pitch bend maps to vibrator
    /// frequency, not exciter gesture. Exposed here for the vibrator
    /// section to consume.
    #[inline]
    pub fn pitch_bend(&self) -> i16 {
        self.pitch_bend_raw
    }

    /// Pitch bend as a normalised float in `[-1.0, +1.0]`.
    #[inline]
    pub fn pitch_bend_normalised(&self) -> f32 {
        self.pitch_bend_raw as f32 / 8192.0
    }

    /// Reset all state — panic button.
    pub fn reset(&mut self) {
        self.parser = ParserState::Idle;
        self.running_status = None;
        self.note_stack.clear();
        self.force = 0.0;
        self.position = 0.5;
        self.speed = 0.0;
        self.pitch_bend_raw = 0;
        self.poly_aftertouch = 0.0;
        self.channel_aftertouch = 0.0;
    }

    /// Feed a slice of raw MIDI bytes, returning events for each complete message.
    ///
    /// This is a convenience wrapper around [`feed_byte`](Midi1Normaliser::feed_byte)
    /// for processing MIDI buffers.
    ///
    /// The callback `f` is called for each complete message. To collect events,
    /// use a closure that pushes to a vec (in std) or a fixed-size buffer (no_std).
    pub fn feed_bytes(&mut self, bytes: &[u8], mut f: impl FnMut(MidiEvent)) {
        for &byte in bytes {
            if let Some(event) = self.feed_byte(byte) {
                f(event);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: feed a complete 3-byte message.
    fn feed_msg(norm: &mut Midi1Normaliser, b0: u8, b1: u8, b2: u8) -> Option<MidiEvent> {
        norm.feed_byte(b0);
        norm.feed_byte(b1);
        norm.feed_byte(b2)
    }

    #[test]
    fn initial_state_is_silent() {
        let norm = Midi1Normaliser::new(None);
        assert_eq!(norm.gesture(), PlayGesture::SILENT);
        assert_eq!(norm.active_note(), None);
        assert_eq!(norm.pitch_bend(), 0);
    }

    #[test]
    fn note_on_sets_gate_and_force() {
        let mut norm = Midi1Normaliser::new(None);

        // Note On: ch0, note 60, velocity 100
        let event = feed_msg(&mut norm, 0x90, 60, 100);
        assert_eq!(
            event,
            Some(MidiEvent::NoteOn {
                channel: 0,
                note: 60,
                velocity: 100
            })
        );

        let g = norm.gesture();
        assert!(g.continuity, "Gate should be open after Note On");
        assert!((g.force - 100.0 / 127.0).abs() < 1e-4);
        assert_eq!(norm.active_note(), Some(60));
    }

    #[test]
    fn note_off_clears_gate() {
        let mut norm = Midi1Normaliser::new(None);
        feed_msg(&mut norm, 0x90, 60, 100); // Note On

        // Note Off: ch0, note 60
        let event = feed_msg(&mut norm, 0x80, 60, 0);
        assert_eq!(
            event,
            Some(MidiEvent::NoteOff {
                channel: 0,
                note: 60,
                velocity: 0
            })
        );

        let g = norm.gesture();
        assert!(!g.continuity, "Gate should be closed after Note Off");
        assert_eq!(g.force, 0.0);
        assert_eq!(norm.active_note(), None);
    }

    #[test]
    fn velocity_zero_is_note_off() {
        let mut norm = Midi1Normaliser::new(None);
        feed_msg(&mut norm, 0x90, 60, 100); // Note On

        // Note On with velocity 0 = Note Off
        let event = feed_msg(&mut norm, 0x90, 60, 0);
        assert_eq!(
            event,
            Some(MidiEvent::NoteOff {
                channel: 0,
                note: 60,
                velocity: 0
            })
        );

        assert!(!norm.gesture().continuity);
    }

    #[test]
    fn note_stacking_last_note_priority() {
        let mut norm = Midi1Normaliser::new(None);

        feed_msg(&mut norm, 0x90, 60, 100); // C4 on
        assert_eq!(norm.active_note(), Some(60));

        feed_msg(&mut norm, 0x90, 64, 80); // E4 on
        assert_eq!(norm.active_note(), Some(64));
        assert!((norm.gesture().force - 80.0 / 127.0).abs() < 1e-4);

        // Release E4 — should fall back to C4
        feed_msg(&mut norm, 0x80, 64, 0);
        assert_eq!(norm.active_note(), Some(60));
        assert!((norm.gesture().force - 100.0 / 127.0).abs() < 1e-4);

        // Release C4 — nothing held
        feed_msg(&mut norm, 0x80, 60, 0);
        assert_eq!(norm.active_note(), None);
        assert!(!norm.gesture().continuity);
    }

    #[test]
    fn note_retrigger_moves_to_top() {
        let mut norm = Midi1Normaliser::new(None);

        feed_msg(&mut norm, 0x90, 60, 100); // C4
        feed_msg(&mut norm, 0x90, 64, 80); // E4
        feed_msg(&mut norm, 0x90, 60, 110); // C4 retrigger

        assert_eq!(norm.active_note(), Some(60));
        assert!((norm.gesture().force - 110.0 / 127.0).abs() < 1e-4);

        // Release C4 — fall back to E4
        feed_msg(&mut norm, 0x80, 60, 0);
        assert_eq!(norm.active_note(), Some(64));
    }

    #[test]
    fn cc74_sets_position() {
        let mut norm = Midi1Normaliser::new(None);

        // CC74 = 64 (centre)
        let event = feed_msg(&mut norm, 0xB0, CC_BRIGHTNESS, 64);
        assert_eq!(
            event,
            Some(MidiEvent::ControlChange {
                channel: 0,
                controller: CC_BRIGHTNESS,
                value: 64
            })
        );

        assert!((norm.gesture().position - 64.0 / 127.0).abs() < 1e-4);
    }

    #[test]
    fn cc2_sets_speed() {
        let mut norm = Midi1Normaliser::new(None);

        feed_msg(&mut norm, 0xB0, CC_BREATH, 100);
        assert!((norm.gesture().speed - 100.0 / 127.0).abs() < 1e-4);
    }

    #[test]
    fn poly_aftertouch_updates_force() {
        let mut norm = Midi1Normaliser::new(None);

        feed_msg(&mut norm, 0x90, 60, 100); // Note On
        let initial_force = norm.gesture().force;

        // Poly Aftertouch on the active note
        feed_msg(&mut norm, 0xA0, 60, 120);
        let at_force = norm.gesture().force;

        assert!((at_force - 120.0 / 127.0).abs() < 1e-4);
        assert!(at_force > initial_force, "Aftertouch should override velocity");
    }

    #[test]
    fn poly_aftertouch_ignores_inactive_notes() {
        let mut norm = Midi1Normaliser::new(None);

        feed_msg(&mut norm, 0x90, 60, 100); // Note On, note 60
        let force_before = norm.gesture().force;

        // Poly Aftertouch on a DIFFERENT note
        feed_msg(&mut norm, 0xA0, 61, 127);
        let force_after = norm.gesture().force;

        assert_eq!(force_before, force_after, "AT on inactive note should not change force");
    }

    #[test]
    fn channel_aftertouch_updates_force() {
        let mut norm = Midi1Normaliser::new(None);

        feed_msg(&mut norm, 0x90, 60, 80); // Note On

        // Channel Aftertouch (1 data byte)
        norm.feed_byte(0xD0);
        let event = norm.feed_byte(110);
        assert_eq!(
            event,
            Some(MidiEvent::ChannelAftertouch {
                channel: 0,
                pressure: 110
            })
        );

        assert!((norm.gesture().force - 110.0 / 127.0).abs() < 1e-4);
    }

    #[test]
    fn pitch_bend_parsing() {
        let mut norm = Midi1Normaliser::new(None);

        // Pitch bend: centre (MSB=64, LSB=0 → raw = 8192 → centred = 0)
        feed_msg(&mut norm, 0xE0, 0, 64);
        assert_eq!(norm.pitch_bend(), 0);
        assert!((norm.pitch_bend_normalised()).abs() < 1e-4);

        // Pitch bend: max up (MSB=127, LSB=127 → raw = 16383 → centred = 8191)
        feed_msg(&mut norm, 0xE0, 127, 127);
        assert_eq!(norm.pitch_bend(), 8191);
        assert!((norm.pitch_bend_normalised() - 8191.0 / 8192.0).abs() < 1e-3);

        // Pitch bend: max down (MSB=0, LSB=0 → raw = 0 → centred = -8192)
        feed_msg(&mut norm, 0xE0, 0, 0);
        assert_eq!(norm.pitch_bend(), -8192);
        assert!((norm.pitch_bend_normalised() - (-1.0)).abs() < 1e-4);
    }

    #[test]
    fn channel_filter_rejects_other_channels() {
        let mut norm = Midi1Normaliser::new(Some(0)); // Listen to ch0 only

        // Note On on channel 1 — should be ignored
        let event = feed_msg(&mut norm, 0x91, 60, 100);
        assert_eq!(event, None);
        assert_eq!(norm.active_note(), None);

        // Note On on channel 0 — should be accepted
        let event = feed_msg(&mut norm, 0x90, 60, 100);
        assert!(event.is_some());
        assert_eq!(norm.active_note(), Some(60));
    }

    #[test]
    fn omni_mode_accepts_all_channels() {
        let mut norm = Midi1Normaliser::new(None);

        feed_msg(&mut norm, 0x90, 60, 100); // ch0
        assert_eq!(norm.active_note(), Some(60));

        feed_msg(&mut norm, 0x95, 64, 100); // ch5
        assert_eq!(norm.active_note(), Some(64));
    }

    #[test]
    fn running_status() {
        let mut norm = Midi1Normaliser::new(None);

        // First Note On: status + data
        feed_msg(&mut norm, 0x90, 60, 100);
        assert_eq!(norm.active_note(), Some(60));

        // Second Note On via running status: just data bytes
        norm.feed_byte(64);
        let event = norm.feed_byte(80);
        assert_eq!(
            event,
            Some(MidiEvent::NoteOn {
                channel: 0,
                note: 64,
                velocity: 80
            })
        );
        assert_eq!(norm.active_note(), Some(64));
    }

    #[test]
    fn running_status_channel_aftertouch() {
        let mut norm = Midi1Normaliser::new(None);
        feed_msg(&mut norm, 0x90, 60, 100); // Note On

        // Channel Aftertouch status byte + data
        norm.feed_byte(0xD0);
        norm.feed_byte(100);

        // Running status: next byte should be another channel AT
        let event = norm.feed_byte(80);
        assert_eq!(
            event,
            Some(MidiEvent::ChannelAftertouch {
                channel: 0,
                pressure: 80
            })
        );
    }

    #[test]
    fn system_realtime_does_not_disrupt_parse() {
        let mut norm = Midi1Normaliser::new(None);

        // Start a Note On, inject a system real-time (0xF8 = timing clock)
        // mid-message — it should be transparent.
        norm.feed_byte(0x90);
        norm.feed_byte(60);
        norm.feed_byte(0xF8); // Timing clock — should be ignored
        let event = norm.feed_byte(100);

        assert_eq!(
            event,
            Some(MidiEvent::NoteOn {
                channel: 0,
                note: 60,
                velocity: 100
            })
        );
    }

    #[test]
    fn system_common_cancels_running_status() {
        let mut norm = Midi1Normaliser::new(None);

        feed_msg(&mut norm, 0x90, 60, 100); // Note On, establishes running status

        // System Common message (0xF2 = Song Position Pointer) cancels running status
        norm.feed_byte(0xF2);

        // Now send data bytes — should NOT be interpreted as Note On via running status
        norm.feed_byte(64);
        let event = norm.feed_byte(80);
        assert_eq!(event, None, "Running status should be cancelled by System Common");
    }

    #[test]
    fn all_notes_off_clears_everything() {
        let mut norm = Midi1Normaliser::new(None);

        feed_msg(&mut norm, 0x90, 60, 100);
        feed_msg(&mut norm, 0x90, 64, 100);
        assert!(norm.gesture().continuity);

        // CC 123 = All Notes Off
        feed_msg(&mut norm, 0xB0, 123, 0);
        assert!(!norm.gesture().continuity);
        assert_eq!(norm.active_note(), None);
    }

    #[test]
    fn reset_clears_all_state() {
        let mut norm = Midi1Normaliser::new(None);

        feed_msg(&mut norm, 0x90, 60, 100);
        feed_msg(&mut norm, 0xB0, CC_BRIGHTNESS, 100);
        feed_msg(&mut norm, 0xB0, CC_BREATH, 100);
        feed_msg(&mut norm, 0xE0, 0, 127);

        norm.reset();

        assert_eq!(norm.gesture(), PlayGesture::SILENT);
        assert_eq!(norm.active_note(), None);
        assert_eq!(norm.pitch_bend(), 0);
    }

    #[test]
    fn feed_bytes_batch() {
        let mut norm = Midi1Normaliser::new(None);
        let mut events = std::vec::Vec::new();

        // Two Note On messages in a single buffer
        let bytes = [0x90, 60, 100, 0x90, 64, 80];
        norm.feed_bytes(&bytes, |e| events.push(e));

        assert_eq!(events.len(), 2);
        assert_eq!(norm.active_note(), Some(64));
    }

    #[test]
    fn max_velocity_maps_to_one() {
        let mut norm = Midi1Normaliser::new(None);
        feed_msg(&mut norm, 0x90, 60, 127);
        assert_eq!(norm.gesture().force, 1.0);
    }

    #[test]
    fn min_velocity_maps_near_zero() {
        let mut norm = Midi1Normaliser::new(None);
        feed_msg(&mut norm, 0x90, 60, 1);
        assert!((norm.gesture().force - 1.0 / 127.0).abs() < 1e-6);
    }

    /// Stress test: note stack overflow doesn't panic.
    #[test]
    fn note_stack_overflow() {
        let mut norm = Midi1Normaliser::new(None);

        // Press more notes than the stack can hold
        for note in 0..20u8 {
            feed_msg(&mut norm, 0x90, 60 + note, 100);
        }

        // Should still be functional — most recent note is active
        assert_eq!(norm.active_note(), Some(79)); // 60 + 19
        assert!(norm.gesture().continuity);

        // Release all — should end up empty
        for note in 0..20u8 {
            feed_msg(&mut norm, 0x80, 60 + note, 0);
        }
        assert!(!norm.gesture().continuity);
    }
}
