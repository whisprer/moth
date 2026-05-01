//! Moth — Shadow Hills edition (Step 1: faceplate background + positioned sliders)
//!
//! This first version just gets the faceplate showing as the background
//! with all parameter sliders positioned over the correct slots.
//! Animated knobs and VU needle come in step 2.

use nih_plug::prelude::*;
use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::vizia::vg;
use nih_plug_vizia::widgets::*;
use nih_plug_vizia::{create_vizia_editor, ViziaState, ViziaTheming};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use crate::MothParams;

// ─── Embedded faceplate ─────────────────────────────────────────────────────

const FACEPLATE_PNG: &[u8] = include_bytes!("../assets/faceplate.png");

const FW: u32 = 1344;
const FH: u32 = 797;
const EDITOR_WIDTH: u32 = FW;
const EDITOR_HEIGHT: u32 = FH;

// ─── CSS ────────────────────────────────────────────────────────────────────
//
// The faceplate provides all the visual chrome — labels, panels, decorative
// knobs, the moth illustration. We just need to position interactive sliders
// over the slots painted into the image.

const STYLE: &str = r#"
* { font-size: 11; color: #c4a55a; }

.moth-root {
    background-color: #112216;
    width: 1s; height: 1s;
    child-space: 0px;
}

.faceplate-bg {
    width: 1s; height: 1s;
    child-space: 0px;
}

/* Slider widget — just a thin coloured fill bar that sits over the painted slot */
.slot {
    width: 130px;
    height: 14px;
    background-color: #0e1711;
    border-radius: 2px;
    position-type: self-directed;
}

.slot-wide {
    width: 140px;
    height: 14px;
    background-color: #0e1711;
    border-radius: 2px;
    position-type: self-directed;
}

param-slider {
    background-color: rgba(0, 0, 0, 0);
    color: #c4a55a;
    font-size: 10;
}
param-slider .fill {
    background-color: #c4a55a;
    border-radius: 2px;
}
param-slider.sl-purple { color: #9b7ad8; }
param-slider.sl-purple .fill { background-color: #9b7ad8; }
param-slider.sl-teal { color: #5dcaa5; }
param-slider.sl-teal .fill { background-color: #5dcaa5; }
"#;

// ─── Faceplate background view ──────────────────────────────────────────────

struct FaceplateBg {
    image_id: std::cell::Cell<Option<vg::ImageId>>,
}

impl FaceplateBg {
    pub fn new(cx: &mut Context) -> Handle<'_, Self> {
        Self { image_id: std::cell::Cell::new(None) }.build(cx, |_| {})
    }
}

impl View for FaceplateBg {
    fn element(&self) -> Option<&'static str> { Some("faceplate-bg") }

    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let b = cx.bounds();
        let (x, y, w, h) = (b.x, b.y, b.w, b.h);

        // Lazy-load the faceplate image on first draw
        let id = if let Some(id) = self.image_id.get() {
            id
        } else {
            use std::io::Cursor;
            let img = match image::ImageReader::new(Cursor::new(FACEPLATE_PNG))
                .with_guessed_format()
                .ok()
                .and_then(|r| r.decode().ok())
            {
                Some(i) => i,
                None => {
                    let mut p = vg::Path::new(); p.rect(x, y, w, h);
                    canvas.fill_path(&p, &vg::Paint::color(vg::Color::rgbf(0.8, 0.0, 0.0)));
                    return;
                }
            };
            let rgba = img.to_rgba8();
            let (iw, ih) = rgba.dimensions();
            // Build the pixel buffer — femtovg's RGBA8 = rgb::RGBA<u8>
            let raw = rgba.into_raw();
            let pixels: Vec<vg::rgb::RGBA8> = raw.chunks_exact(4)
                .map(|c| vg::rgb::RGBA::new(c[0], c[1], c[2], c[3]))
                .collect();
            let imgref = vg::imgref::Img::new(pixels, iw as usize, ih as usize);

            match canvas.create_image(
                vg::ImageSource::from(imgref.as_ref()),
                vg::ImageFlags::empty(),
            ) {
                Ok(id) => { self.image_id.set(Some(id)); id },
                Err(_) => {
                    let mut p = vg::Path::new(); p.rect(x, y, w, h);
                    canvas.fill_path(&p, &vg::Paint::color(vg::Color::rgbf(0.9, 0.8, 0.0)));
                    return;
                }
            }
        };

        // Draw the image
        let mut path = vg::Path::new();
        path.rect(x, y, w, h);

        // femtovg 0.7: Paint::image(id, cx, cy, width, height, angle, alpha)
        // The image extends from (cx, cy) to (cx+width, cy+height) in path-coord space.
        let paint = vg::Paint::image(id, x, y, w, h, 0.0, 1.0);
        canvas.fill_path(&path, &paint);
    }
}

// ─── Data model ─────────────────────────────────────────────────────────────

#[derive(Lens)]
struct Data { params: Arc<MothParams> }
impl Model for Data {}

pub(crate) fn default_state() -> Arc<ViziaState> {
    ViziaState::new(|| (EDITOR_WIDTH, EDITOR_HEIGHT))
}

// ─── Slot positions (faceplate-pixel space) ─────────────────────────────────
//
// Each tuple: (x, y, width). The slot height is always 14px and centred on y.

// ═══════════════════════════════════════════════════════════════════════════
//  PHOSPHOR DISPLAYS — live signal chain visualisations
// ═══════════════════════════════════════════════════════════════════════════
//
// Display window coordinates (faceplate pixels), inset 6px inside the bezels:
//   Exciter:    (52, 148, 120, 92)   from detected (46, 142, 132x104)
//   Vibrator:   (230, 146, 121, 95)
//   Body:       (417, 151, 107, 109) — round display
//   Character:  (741, 149, 120, 91)
//
// Each display reads its parameter values and draws traces in amber/purple/teal.

// Phosphor colours (bright glowing CRT)
const PH_AMBER: vg::Color    = vg::Color::rgbf(0.910, 0.753, 0.416);
const PH_AMBER_DIM: vg::Color = vg::Color::rgbf(0.831, 0.659, 0.333);
const PH_PURPLE: vg::Color   = vg::Color::rgbf(0.708, 0.578, 0.947);
const PH_TEAL: vg::Color     = vg::Color::rgbf(0.465, 0.892, 0.747);
const PH_GRID: vg::Color     = vg::Color::rgbf(0.075, 0.140, 0.090);

fn col_a(c: vg::Color, a: f32) -> vg::Color { vg::Color::rgbaf(c.r, c.g, c.b, a) }
fn pstroke(c: vg::Color, w: f32) -> vg::Paint {
    let mut p = vg::Paint::color(c); p.set_line_width(w); p
}
/// Glow line — thick dim underneath, thin bright on top.
fn glow_line(canvas: &mut Canvas, path: &vg::Path, color: vg::Color, width: f32) {
    canvas.stroke_path(path, &pstroke(col_a(color, 0.3), width + 3.0));
    canvas.stroke_path(path, &pstroke(color, width));
}

// ─── Exciter phosphor display ───────────────────────────────────────────────

struct ExciterDisplay { params: Arc<MothParams> }
impl ExciterDisplay {
    pub fn new(cx: &mut Context, params: Arc<MothParams>) -> Handle<'_, Self> {
        Self { params }.build(cx, |_| {})
    }
}
impl View for ExciterDisplay {
    fn element(&self) -> Option<&'static str> { Some("exciter-display") }
    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let b = cx.bounds();
        let (x, y, w, h) = (b.x, b.y, b.w, b.h);

        // Faint graticule grid — barely visible
        for i in 1..4 {
            let gy = y + h * i as f32 / 4.0;
            let mut gl = vg::Path::new();
            gl.move_to(x + 4.0, gy); gl.line_to(x + w - 4.0, gy);
            canvas.stroke_path(&gl, &pstroke(PH_GRID, 0.5));
        }

        let morph = self.params.exciter_morph.value();
        let tilt = self.params.spectral_tilt.value();
        let stoch = self.params.stochasticity.value();

        // Coupling values from the 6 morph presets
        let cont = [0.0_f32, 0.0, 1.0, 1.0, 1.0, 0.3];
        let direct = [1.0_f32, 1.0, 0.0, 0.0, 1.0, 1.0];
        let friction = [0.0_f32, 0.0, 1.0, 0.0, 0.0, 0.0];
        let pressure = [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0];
        let sc = morph * 5.0;
        let idx = (sc as usize).min(4);
        let frac = sc - idx as f32;
        let lerp = |a: f32, b: f32| a + (b - a) * frac;
        let ec = lerp(cont[idx], cont[idx + 1]);
        let fd = lerp(direct[idx], direct[idx + 1]);
        let ff = lerp(friction[idx], friction[idx + 1]);
        let fp = lerp(pressure[idx], pressure[idx + 1]);

        let pad = 8.0;
        let ew = w - pad * 2.0;
        let mid_y = y + h * 0.5;
        let amp_scale = h * 0.35 * (0.5 + tilt * 0.5);
        let steps = (ew as usize).max(2);

        let env_at = |t: f32| -> f32 {
            if ec < 0.3 {
                let att = (-t * 20.0 * (1.0 - ec * 2.0)).exp();
                let dec = (-t * 8.0 * (1.0 - tilt)).exp();
                att * dec * fd + ff * 0.3 * (t * 40.0).sin() * (-t * 3.0).exp()
                    + fp * 0.2 * (1.0 - t) * (1.0 - t * 3.0).max(0.0)
            } else if ec < 0.7 {
                let sus = 0.3 + ec * 0.7;
                let att = 1.0 - (-t * 30.0).exp();
                let shape = att * sus;
                shape * ff * 0.8 * (1.0 + 0.15 * (t * 60.0).sin())
                    + shape * fp * 0.6 * (1.0 + 0.1 * (t * 25.0).sin())
                    + fd * 0.3 * (-t * 15.0).exp()
            } else {
                let sus = 0.5 + ec * 0.4;
                let att = 1.0 - (-t * 15.0).exp();
                let shape = att * sus;
                shape * fp * 0.9 * (1.0 + 0.08 * (t * 20.0).sin())
                    + shape * ff * 0.4 * (1.0 + 0.2 * (t * 50.0).sin())
                    + fd * 0.1 * (-t * 20.0).exp()
            }
        };

        // Purple stochasticity band
        if stoch > 0.01 {
            let noise_w = stoch * amp_scale * 0.6;
            let mut top_pts = Vec::with_capacity(steps + 1);
            let mut bot_pts = Vec::with_capacity(steps + 1);
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let env = env_at(t);
                let base = mid_y - env * amp_scale;
                let band = noise_w * env.abs().max(0.05);
                top_pts.push((x + pad + i as f32 * ew / steps as f32, base - band));
                bot_pts.push((x + pad + i as f32 * ew / steps as f32, base + band));
            }
            let mut band = vg::Path::new();
            band.move_to(top_pts[0].0, top_pts[0].1);
            for &(px, py) in &top_pts[1..] { band.line_to(px, py); }
            for &(px, py) in bot_pts.iter().rev() { band.line_to(px, py); }
            band.close();
            canvas.fill_path(&band, &vg::Paint::color(col_a(PH_PURPLE, 0.18 + stoch * 0.2)));
        }

        // Amber main envelope with phosphor glow
        let mut env_path = vg::Path::new();
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let py = mid_y - env_at(t) * amp_scale;
            let px = x + pad + i as f32 * ew / steps as f32;
            if i == 0 { env_path.move_to(px, py); } else { env_path.line_to(px, py); }
        }
        glow_line(canvas, &env_path, PH_AMBER, 1.5);

        // Faint mirror below
        let mut mir = vg::Path::new();
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let env = if ec < 0.3 {
                (-t * 20.0 * (1.0 - ec * 2.0)).exp() * (-t * 8.0 * (1.0 - tilt)).exp() * fd
            } else { (1.0 - (-t * 30.0).exp()) * (0.3 + ec * 0.5) };
            let py = mid_y + env * amp_scale;
            let px = x + pad + i as f32 * ew / steps as f32;
            if i == 0 { mir.move_to(px, py); } else { mir.line_to(px, py); }
        }
        canvas.stroke_path(&mir, &pstroke(col_a(PH_AMBER_DIM, 0.35), 1.0));

        // Teal coupling-mode tick bars at the bottom (d / f / p)
        let bar_y = y + h - 9.0;
        for (i, &strength) in [fd, ff, fp].iter().enumerate() {
            let bx = x + pad + i as f32 * 14.0;
            let mut bar = vg::Path::new();
            bar.rounded_rect(bx, bar_y, 10.0, 4.0, 1.5);
            let c = if strength > 0.01 {
                col_a(PH_TEAL, 0.5 + strength * 0.5)
            } else {
                col_a(PH_GRID, 0.6)
            };
            canvas.fill_path(&bar, &vg::Paint::color(c));
        }
    }
}

// ─── Vibrator phosphor display ──────────────────────────────────────────────

struct VibratorDisplay { params: Arc<MothParams> }
impl VibratorDisplay {
    pub fn new(cx: &mut Context, params: Arc<MothParams>) -> Handle<'_, Self> {
        Self { params }.build(cx, |_| {})
    }
}
impl View for VibratorDisplay {
    fn element(&self) -> Option<&'static str> { Some("vibrator-display") }
    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let b = cx.bounds();
        let (x, y, w, h) = (b.x, b.y, b.w, b.h);

        for i in 1..4 {
            let gy = y + h * i as f32 / 4.0;
            let mut gl = vg::Path::new();
            gl.move_to(x + 4.0, gy); gl.line_to(x + w - 4.0, gy);
            canvas.stroke_path(&gl, &pstroke(PH_GRID, 0.5));
        }

        let damp = self.params.vib_damping.value();
        let bright = self.params.vib_brightness.value();
        let disp = self.params.vib_dispersion.value();
        let pos = self.params.position.value();

        let pad = 6.0;
        let ew = w - pad * 2.0;
        let mid_y = y + h * 0.5;
        let partials = 8;
        let steps = (ew as usize).max(2);

        // Teal: position tap marker (dashed vertical)
        let clamped_pos = 0.5 - 0.98 * (pos - 0.5_f32).abs();
        let pos_x = x + pad + clamped_pos * ew;
        let mut pos_dash = vg::Path::new();
        for d in 0..6 {
            let t0 = y + 4.0 + d as f32 * (h - 8.0) / 6.0;
            pos_dash.move_to(pos_x, t0);
            pos_dash.line_to(pos_x, t0 + (h - 8.0) / 12.0);
        }
        canvas.stroke_path(&pos_dash, &pstroke(col_a(PH_TEAL, 0.55), 1.0));

        // Purple: brightness rolloff envelope
        let rolloff_rate = (1.0 - bright) * 0.45;
        let mut rolloff = vg::Path::new();
        for p in 0..=partials {
            let pf = (p + 1) as f32;
            let atten = (-rolloff_rate * pf * 1.2).exp();
            let rx = x + pad + (pf / (partials as f32 + 1.0)) * ew;
            let ry = mid_y - atten * (h * 0.40);
            if p == 0 { rolloff.move_to(rx, ry); } else { rolloff.line_to(rx, ry); }
        }
        canvas.stroke_path(&rolloff, &pstroke(col_a(PH_PURPLE, 0.55), 1.2));

        // Amber: partials with glow
        for p in 1..=partials {
            let pf = p as f32;
            let amp = 0.85_f32.powf(pf - 1.0) * (0.3 + damp * 0.7);
            let freq = pf * (1.0 + disp * disp * 0.02 * pf * pf);
            let alpha = (amp * 0.9).max(0.08);
            let c = if p <= 3 { PH_AMBER } else { PH_AMBER_DIM };
            let lw = if p <= 3 { 1.3 } else { 0.6 };

            let mut path = vg::Path::new();
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let env = (-t * (3.0 + pf * 0.5) * (1.1 - damp)).exp();
                let val = (t * std::f32::consts::PI * 2.0 * freq * 2.0).sin()
                    * amp * env * (h * 0.32);
                let px = x + pad + i as f32 * ew / steps as f32;
                if i == 0 { path.move_to(px, mid_y - val); }
                else { path.line_to(px, mid_y - val); }
            }
            if p <= 3 {
                canvas.stroke_path(&path, &pstroke(col_a(c, alpha * 0.3), lw + 2.0));
            }
            canvas.stroke_path(&path, &pstroke(col_a(c, alpha), lw));
        }
    }
}

// ─── Body phosphor display ──────────────────────────────────────────────────

struct BodyDisplay { params: Arc<MothParams> }
impl BodyDisplay {
    pub fn new(cx: &mut Context, params: Arc<MothParams>) -> Handle<'_, Self> {
        Self { params }.build(cx, |_| {})
    }
}
impl View for BodyDisplay {
    fn element(&self) -> Option<&'static str> { Some("body-display") }
    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let b = cx.bounds();
        let (x, y, w, h) = (b.x, b.y, b.w, b.h);

        let geom = self.params.body_geometry.value();
        let size = self.params.body_size.value();
        let bright = self.params.body_brightness.value();
        let damp = self.params.body_damping.value();

        let cx2 = x + w * 0.5;
        let cy = y + h * 0.42;
        let sc = 0.4 + size * 0.5;
        let stiff = (geom - 0.25) * 4.0;

        // Body outline
        let mut body_path = vg::Path::new();
        if stiff < -0.5 {
            body_path.ellipse(cx2, cy, 14.0 * sc + 6.0, 30.0 * sc + 8.0);
        } else if stiff < 0.3 {
            let bw = 22.0 * sc + 8.0;
            let bh = 28.0 * sc + 6.0;
            let waist = 0.55 + stiff * 0.3;
            body_path.move_to(cx2, cy - bh);
            body_path.bezier_to(cx2 + bw * 0.7, cy - bh, cx2 + bw, cy - bh * 0.5,
                cx2 + bw * 0.85, cy - bh * 0.15);
            body_path.bezier_to(cx2 + bw * waist, cy, cx2 + bw * waist, cy,
                cx2 + bw * 0.9, cy + bh * 0.2);
            body_path.bezier_to(cx2 + bw * 1.1, cy + bh * 0.6, cx2 + bw * 0.8, cy + bh,
                cx2, cy + bh);
            body_path.bezier_to(cx2 - bw * 0.8, cy + bh, cx2 - bw * 1.1, cy + bh * 0.6,
                cx2 - bw * 0.9, cy + bh * 0.2);
            body_path.bezier_to(cx2 - bw * waist, cy, cx2 - bw * waist, cy,
                cx2 - bw * 0.85, cy - bh * 0.15);
            body_path.bezier_to(cx2 - bw, cy - bh * 0.5, cx2 - bw * 0.7, cy - bh,
                cx2, cy - bh);
            body_path.close();
        } else if stiff < 0.7 {
            let bw = 24.0 * sc + 6.0;
            let bh = 16.0 * sc + 6.0;
            let r = bw.min(bh) * (1.0 - (0.2 + (stiff - 0.3) * 1.5)).max(0.05);
            body_path.rounded_rect(cx2 - bw, cy - bh, bw * 2.0, bh * 2.0, r);
        } else {
            let br = 20.0 * sc + 6.0;
            let flat = (stiff - 0.7) * 3.0;
            body_path.ellipse(cx2, cy, br * (1.0 - flat * 0.3), br * (1.0 + flat * 0.5));
        }
        canvas.fill_path(&body_path, &vg::Paint::color(col_a(PH_AMBER, 0.06)));
        glow_line(canvas, &body_path, PH_AMBER, 1.3);

        // Sound hole for guitar range
        if stiff > -0.3 && stiff < 0.5 {
            let mut hole = vg::Path::new();
            hole.ellipse(cx2, cy + 5.0 * sc, 4.0 * sc, 1.5 * sc);
            canvas.stroke_path(&hole, &pstroke(col_a(PH_AMBER_DIM, 0.5), 0.5));
        }

        // Mode spectrum bars at bottom
        let modes = 12;
        let mode_y = y + h - 6.0;
        let mode_w = w - 12.0;
        let stiffness = (geom - 0.25) * 0.04;
        let rolloff = 0.15 + (1.0 - bright) * 0.85;
        let base_q = 8.0 + damp * 40.0;

        for i in 0..modes {
            let mf = (i + 1) as f32;
            let mut stretch = 1.0_f32;
            let mut acc = stiffness;
            for _ in 0..i {
                acc *= if stiffness < 0.0 { 0.93 } else { 0.98 };
                stretch += acc;
            }
            let fpos = (mf * stretch) / 24.0;
            if fpos > 1.0 { continue; }

            let gain = 1.0 / (1.0 + i as f32 * rolloff);
            let boosted = if i == 0 { gain * 1.3 } else if i < 3 { gain * 1.15 } else { gain };
            let mx = x + 6.0 + fpos * mode_w;
            let bh_bar = boosted * 14.0;

            // Teal Q width
            let mode_q = base_q / (1.0 + i as f32 * 0.2);
            let qw = (3.0 / mode_q).max(0.5).min(3.0);
            if qw > 1.0 {
                let mut qb = vg::Path::new();
                qb.rounded_rect(mx - qw, mode_y - bh_bar, qw * 2.0, bh_bar, 1.0);
                canvas.fill_path(&qb, &vg::Paint::color(col_a(PH_TEAL, 0.15)));
            }

            let c = if i < 3 { PH_AMBER } else { PH_AMBER_DIM };
            let lw = if i < 3 { 1.6 } else { 0.8 };
            let mut bar = vg::Path::new();
            bar.move_to(mx, mode_y);
            bar.line_to(mx, mode_y - bh_bar);
            canvas.stroke_path(&bar, &pstroke(c, lw));
        }
    }
}

// ─── Character (nonlin) phosphor display ────────────────────────────────────

struct NonlinDisplay { params: Arc<MothParams> }
impl NonlinDisplay {
    pub fn new(cx: &mut Context, params: Arc<MothParams>) -> Handle<'_, Self> {
        Self { params }.build(cx, |_| {})
    }
}
impl View for NonlinDisplay {
    fn element(&self) -> Option<&'static str> { Some("nonlin-display") }
    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let b = cx.bounds();
        let (x, y, w, h) = (b.x, b.y, b.w, b.h);

        let drive = self.params.nl_drive.value();
        let tape = self.params.nl_tape.value();
        let tube = self.params.nl_tube.value();
        let warmth = self.params.nl_warmth.value();
        let tone = self.params.nl_tone.value();
        let tt = if tape + tube > 0.001 { tape / (tape + tube) } else { 0.5 };
        let asym = 1.0 + (1.0 - tt) * 0.07;

        let pad = 8.0;
        let gw = w - pad * 2.0;
        let gh = h - pad * 2.0;
        let gc_x = x + pad;
        let gc_y = y + pad;

        // Cross axes
        let mut axes = vg::Path::new();
        axes.move_to(gc_x, gc_y + gh * 0.5);
        axes.line_to(gc_x + gw, gc_y + gh * 0.5);
        axes.move_to(gc_x + gw * 0.5, gc_y);
        axes.line_to(gc_x + gw * 0.5, gc_y + gh);
        canvas.stroke_path(&axes, &pstroke(PH_GRID, 0.5));

        let soft_sat = |v: f32| -> f32 { v * (27.0 + v * v) / (27.0 + 9.0 * v * v) };
        let steps = (gw as usize).max(2);

        // Purple warmth pre-emphasis
        if warmth > 0.01 {
            let ww = gw * 0.32 * warmth;
            let wh = gh * 0.10 * warmth;
            let mut wp = vg::Path::new();
            wp.move_to(gc_x, gc_y + gh * 0.5);
            wp.bezier_to(gc_x + ww * 0.3, gc_y + gh * 0.5 - wh,
                gc_x + ww * 0.7, gc_y + gh * 0.5 - wh * 0.8,
                gc_x + ww, gc_y + gh * 0.5);
            wp.close();
            canvas.fill_path(&wp, &vg::Paint::color(col_a(PH_PURPLE, 0.18)));
        }

        // Teal tone post-filter rolloff
        if tone < 0.99 {
            let rs = (1.0 - tone) * 0.20;
            let fs = gc_x + gw * (0.6 + tone * 0.35);
            let mut tf = vg::Path::new();
            tf.move_to(fs, gc_y);
            tf.line_to(gc_x + gw, gc_y);
            tf.line_to(gc_x + gw, gc_y + gh * rs);
            tf.bezier_to(gc_x + gw - (gw + gc_x - fs) * 0.5, gc_y + gh * rs * 0.3,
                fs + (gw + gc_x - fs) * 0.2, gc_y, fs, gc_y);
            tf.close();
            canvas.fill_path(&tf, &vg::Paint::color(col_a(PH_TEAL, 0.10)));
        }

        // Main transfer curve with glow
        let mut path = vg::Path::new();
        let mut prev = 0.0_f32;
        for i in 0..=steps {
            let inp = (i as f32 / steps as f32) * 2.0 - 1.0;
            let driven = inp * drive;
            let out = if tt > 0.5 {
                let hyst = (tt - 0.5) * 0.6;
                let s = soft_sat(driven);
                let v = s * (1.0 - hyst * 0.3) + prev * hyst * 0.3;
                prev = v; v
            } else if driven >= 0.0 {
                soft_sat(driven)
            } else {
                soft_sat(driven * asym) / asym
            };
            let px = gc_x + i as f32 * gw / steps as f32;
            let py = gc_y + gh * 0.5 - out.clamp(-1.0, 1.0) * gh * 0.5 * 0.9;
            if i == 0 { path.move_to(px, py); } else { path.line_to(px, py); }
        }
        glow_line(canvas, &path, PH_AMBER, 1.6);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  VU NEEDLE — analog meter overlay
// ═══════════════════════════════════════════════════════════════════════════

// Pivot point in faceplate-pixel space — bottom-centre of cream face
const VU_PIVOT_X: f32 = 1180.0;
const VU_PIVOT_Y: f32 = 470.0;
const VU_NEEDLE_R: f32 = 110.0;

const VU_BOX_X: i32 = 1050;
const VU_BOX_Y: i32 = 290;
const VU_BOX_W: i32 = 240;
const VU_BOX_H: i32 = 240;

struct VuNeedle { meters: Arc<crate::AudioMeters> }
impl VuNeedle {
    pub fn new(cx: &mut Context, meters: Arc<crate::AudioMeters>) -> Handle<'_, Self> {
        Self { meters }.build(cx, |_| {})
    }
}
impl View for VuNeedle {
    fn element(&self) -> Option<&'static str> { Some("vu-needle") }

    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let b = cx.bounds();

        let pivot_x = b.x + (VU_PIVOT_X - VU_BOX_X as f32);
        let pivot_y = b.y + (VU_PIVOT_Y - VU_BOX_Y as f32);

        // Read smoothed peak from the audio thread (atomic, lock-free)
        let peak_bits = self.meters.peak_level.load(Ordering::Relaxed);
        let peak = f32::from_bits(peak_bits).max(1.0e-10);
        let peak_db = 20.0 * peak.log10();

        // Map [-36, +6] dB to [-1.0, +1.0]
        // Map dB to needle position.
        // Moth's peak output is typically around -10 to -6 dB at loudest.
        // Scale range tuned so that:
        //   -40 dB = idle (hard left)
        //   -20 dB = mid-left (quiet sustain)
        //   -10 dB = needle near 0 mark
        //    -3 dB = pegged right (red zone)
        let normalized = ((peak_db + 40.0) / 37.0).clamp(0.0, 1.0);
        let angle_deg = -50.0 + normalized * 100.0;
        let angle_rad = angle_deg * std::f32::consts::PI / 180.0;

        let tip_x = pivot_x + angle_rad.sin() * VU_NEEDLE_R;
        let tip_y = pivot_y - angle_rad.cos() * VU_NEEDLE_R;

        let mut needle = vg::Path::new();
        needle.move_to(pivot_x, pivot_y);
        needle.line_to(tip_x, tip_y);
        let mut needle_paint = vg::Paint::color(vg::Color::rgbaf(0.10, 0.08, 0.06, 0.95));
        needle_paint.set_line_width(1.6);
        canvas.stroke_path(&needle, &needle_paint);

        let mut cap = vg::Path::new();
        cap.circle(pivot_x, pivot_y, 5.0);
        canvas.fill_path(&cap, &vg::Paint::color(vg::Color::rgbf(0.18, 0.14, 0.10)));
        let mut cap_hi = vg::Path::new();
        cap_hi.circle(pivot_x - 1.2, pivot_y - 1.2, 1.8);
        canvas.fill_path(&cap_hi, &vg::Paint::color(vg::Color::rgbf(0.55, 0.45, 0.28)));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  COUPLING MODE LEDS — three amber dots showing active exciter modes
// ═══════════════════════════════════════════════════════════════════════════
//
// Three small LED dots that brighten when each coupling mode is active:
//   D — direct coupling (pluck/strike/ebow)
//   F — friction coupling (bow)
//   P — pressure coupling (breath)
// Their amber glow intensity reflects the current morph position.

const LED_BOX_X: i32 = 130;
const LED_BOX_Y: i32 = 730;
const LED_BOX_W: i32 = 80;
const LED_BOX_H: i32 = 16;

struct CouplingLeds { params: Arc<MothParams> }
impl CouplingLeds {
    pub fn new(cx: &mut Context, params: Arc<MothParams>) -> Handle<'_, Self> {
        Self { params }.build(cx, |_| {})
    }
}
impl View for CouplingLeds {
    fn element(&self) -> Option<&'static str> { Some("coupling-leds") }
    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let b = cx.bounds();
        let (x, y, w, h) = (b.x, b.y, b.w, b.h);

        // Read morph and derive coupling weights (same logic as ExciterDisplay)
        let morph = self.params.exciter_morph.value();
        let direct = [1.0_f32, 1.0, 0.0, 0.0, 1.0, 1.0];
        let friction = [0.0_f32, 0.0, 1.0, 0.0, 0.0, 0.0];
        let pressure = [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0];
        let sc = morph * 5.0;
        let idx = (sc as usize).min(4);
        let frac = sc - idx as f32;
        let lerp = |a: f32, b: f32| a + (b - a) * frac;
        let fd = lerp(direct[idx], direct[idx + 1]);
        let ff = lerp(friction[idx], friction[idx + 1]);
        let fp = lerp(pressure[idx], pressure[idx + 1]);

        let cy = y + h * 0.5;
        let radius = 4.0;
        let spacing = w / 4.0;

        // Three LEDs: D F P
        for (i, &strength) in [fd, ff, fp].iter().enumerate() {
            let cx2 = x + spacing * (i as f32 + 1.0);

            // Off LED: dim base colour
            let mut led = vg::Path::new();
            led.circle(cx2, cy, radius);
            canvas.fill_path(&led, &vg::Paint::color(vg::Color::rgbf(0.18, 0.14, 0.08)));

            // Glow when active
            if strength > 0.01 {
                // Outer glow
                let mut glow = vg::Path::new();
                glow.circle(cx2, cy, radius + 3.0);
                canvas.fill_path(&glow, &vg::Paint::color(
                    vg::Color::rgbaf(0.910, 0.753, 0.416, strength * 0.4)
                ));
                // Bright core
                let mut core = vg::Path::new();
                core.circle(cx2, cy, radius * 0.85);
                canvas.fill_path(&core, &vg::Paint::color(
                    vg::Color::rgbaf(0.910, 0.753, 0.416, 0.6 + strength * 0.4)
                ));
                // Hot spot
                let mut hot = vg::Path::new();
                hot.circle(cx2 - 1.0, cy - 1.0, radius * 0.35);
                canvas.fill_path(&hot, &vg::Paint::color(
                    vg::Color::rgbaf(1.0, 0.92, 0.65, strength * 0.8)
                ));
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  KNOB INDICATORS — rotating pointer line on painted knobs
// ═══════════════════════════════════════════════════════════════════════════
//
// The faceplate has 4 painted knobs (Morph, Tilt, Size, Drive). They're
// static images; we draw a thin cream/brass indicator line on top that
// rotates with the parameter value. Range: -135° (left, value=0) to
// +135° (right, value=1), 270° total sweep — standard chicken-head range.

struct KnobIndicator {
    params: Arc<MothParams>,
    which: KnobParam,
    radius: f32,
}

#[derive(Clone, Copy)]
enum KnobParam { Morph, Tilt, Size, Drive }

impl KnobIndicator {
    pub fn new(cx: &mut Context, params: Arc<MothParams>, which: KnobParam, radius: f32)
        -> Handle<'_, Self>
    {
        Self { params, which, radius }.build(cx, |_| {})
    }
}
impl View for KnobIndicator {
    fn element(&self) -> Option<&'static str> { Some("knob-indicator") }
    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let b = cx.bounds();
        let cx_pos = b.x + b.w * 0.5;
        let cy_pos = b.y + b.h * 0.5;

        // Read normalized parameter value (0..1)
        let val = match self.which {
            KnobParam::Morph => self.params.exciter_morph.value(),
            KnobParam::Tilt  => self.params.spectral_tilt.value(),
            KnobParam::Size  => self.params.body_size.value(),
            KnobParam::Drive => {
                // nl_drive is 0.5..4.0, normalize to 0..1
                let v = self.params.nl_drive.value();
                ((v - 0.5) / 3.5).clamp(0.0, 1.0)
            }
        };

        // 270° sweep, starting at -135° (7 o'clock)
        let angle_deg = -135.0 + val * 270.0;
        let angle_rad = angle_deg * std::f32::consts::PI / 180.0;

        // Indicator line from inner edge to outer edge of knob
        let inner = self.radius * 0.30;
        let outer = self.radius * 0.85;
        let sin = angle_rad.sin();
        let cos = -angle_rad.cos();  // negative because Y increases downward

        let x0 = cx_pos + sin * inner;
        let y0 = cy_pos + cos * inner;
        let x1 = cx_pos + sin * outer;
        let y1 = cy_pos + cos * outer;

        let mut line = vg::Path::new();
        line.move_to(x0, y0);
        line.line_to(x1, y1);
        let mut paint = vg::Paint::color(vg::Color::rgbf(0.910, 0.863, 0.784));
        paint.set_line_width(2.0);
        paint.set_line_cap(vg::LineCap::Round);
        canvas.stroke_path(&line, &paint);
    }
}

// ─── Editor creation ────────────────────────────────────────────────────────

pub(crate) fn create(
    params: Arc<MothParams>,
    meters: Arc<crate::AudioMeters>,
    editor_state: Arc<ViziaState>,
) -> Option<Box<dyn Editor>> {
    create_vizia_editor(editor_state, ViziaTheming::Custom, move |cx, _| {
        cx.add_stylesheet(STYLE).expect("moth: stylesheet");
        Data { params: params.clone() }.build(cx);

        // Periodic redraw timer — keeps the VU needle live since it reads
        // from an atomic that vizia can't observe. 30 fps is plenty for VU.
        let timer = cx.add_timer(
            std::time::Duration::from_millis(33),
            None,
            |cx, action| {
                if matches!(action, TimerAction::Tick(_)) {
                    cx.needs_redraw();
                }
            },
        );
        cx.start_timer(timer);

        // Pre-clone for the display Views (each takes ownership of its own Arc)
        let p_exc = params.clone();
        let p_vib = params.clone();
        let p_bod = params.clone();
        let p_nl  = params.clone();
        let m_vu  = meters.clone();
        let p_led = params.clone();
        let p_kn1 = params.clone();
        let p_kn2 = params.clone();
        let p_kn3 = params.clone();
        let p_kn4 = params.clone();

        // Root: stack the faceplate background, then absolutely-positioned sliders on top
        ZStack::new(cx, move |cx| {
            // Layer 1: the faceplate image (covers entire window)
            FaceplateBg::new(cx).class("faceplate-bg");

            // Layer 2: phosphor displays in the four dark windows.
            place_view(cx,  52, 148, 120,  92, move |cx| { ExciterDisplay::new(cx, p_exc.clone()); });
            place_view(cx, 230, 146, 121,  95, move |cx| { VibratorDisplay::new(cx, p_vib.clone()); });
            place_view(cx, 417, 151, 107, 109, move |cx| { BodyDisplay::new(cx, p_bod.clone()); });
            place_view(cx, 741, 149, 120,  91, move |cx| { NonlinDisplay::new(cx, p_nl.clone()); });

            // Layer 2b: VU meter needle overlay on the OUT meter.
            place_view(cx, VU_BOX_X, VU_BOX_Y, VU_BOX_W, VU_BOX_H, move |cx| { VuNeedle::new(cx, m_vu.clone()); });

            // Layer 2c: Coupling mode LEDs (D F P) under the EXCITER column.
            place_view(cx, LED_BOX_X, LED_BOX_Y, LED_BOX_W, LED_BOX_H, move |cx| { CouplingLeds::new(cx, p_led.clone()); });

            // Layer 2d: Animated indicator lines on the four painted knobs.
            // The Morph and Tilt knobs in the faceplate are painted with
            // a slight 3D parallax (viewed off-axis) so their visual centres
            // are offset from their geometric centres — we shift the boxes
            // ~12px right to land the indicator on the perceived centre.
            // Morph: nudged up-left to align with painted knob centre
            place_view(cx,  70, 460, 90, 90, move |cx| { KnobIndicator::new(cx, p_kn1.clone(), KnobParam::Morph, 36.0); });
            // Tilt: nudged down-left to align with painted knob centre
            place_view(cx,  70, 620, 90, 90, move |cx| { KnobIndicator::new(cx, p_kn2.clone(), KnobParam::Tilt, 36.0); });
            // Size knob: centre (470, 555), painted knob ~95px diameter
            place_view(cx, 420, 505, 100, 100, move |cx| { KnobIndicator::new(cx, p_kn3.clone(), KnobParam::Size, 42.0); });
            // Drive knob: centre (635, 335), painted knob ~70px diameter (perfect already)
            place_view(cx, 590, 290, 90,  90,  move |cx| { KnobIndicator::new(cx, p_kn4.clone(), KnobParam::Drive, 45.0); });

            // Layer 3: each slider positioned absolutely over its painted slot.
            // Vizia uses left/top with position-type self-directed for absolute.

            // EXCITER column (x=107)
            place_slider(cx, "amber",  107, 295, 130, |p| &p.exciter_morph);
            place_slider(cx, "purple", 107, 343, 130, |p| &p.spectral_tilt);
            place_slider(cx, "purple", 107, 391, 130, |p| &p.stochasticity);

            // VIBRATOR column (x=290)
            place_slider(cx, "amber",  290, 293, 130, |p| &p.vib_damping);
            place_slider(cx, "purple", 290, 341, 130, |p| &p.vib_brightness);
            place_slider(cx, "teal",   290, 391, 130, |p| &p.vib_dispersion);
            place_slider(cx, "teal",   290, 439, 130, |p| &p.position);

            // BODY column (x=470)
            place_slider(cx, "amber",  470, 321, 130, |p| &p.body_geometry);
            place_slider(cx, "purple", 470, 370, 130, |p| &p.body_brightness);
            place_slider(cx, "teal",   470, 417, 130, |p| &p.body_damping);
            place_slider(cx, "teal",   470, 467, 130, |p| &p.body_size);

            // MIX column (x=635)
            // MIX column (x=635) — slot positions tuned for painted slot alignment
            place_slider(cx, "teal",   635, 165, 140, |p| &p.exciter_bleed);
            place_slider(cx, "amber",  635, 215, 140, |p| &p.body_mix);

            // CHARACTER column (x=800)
            place_slider(cx, "amber",  800, 294, 130, |p| &p.nl_drive);
            place_slider(cx, "amber",  800, 343, 130, |p| &p.nl_tape);
            place_slider(cx, "amber",  800, 391, 130, |p| &p.nl_tube);
            place_slider(cx, "purple", 800, 439, 130, |p| &p.nl_warmth);
            place_slider(cx, "teal",   800, 487, 130, |p| &p.nl_tone);

            // SPACE column (x=970)
            place_slider(cx, "amber",  970, 302, 130, |p| &p.room_size);
            place_slider(cx, "teal",   970, 350, 130, |p| &p.room_mix);

            // (Master gain has no slot — only the VU meter, which we'll add next step.
            //  For now, accessible via DAW generic param panel.)

        }).class("moth-root");

        ResizeHandle::new(cx);
    })
}

/// Place a slider absolutely positioned at the given faceplate-pixel coordinates.
/// `cx_px`, `cy_px`: centre of the slot in the original faceplate pixels.
/// `w_px`: slot width.
/// Place an arbitrary view absolutely positioned at faceplate-pixel coords.
/// `x_px`, `y_px`: top-left corner. `w_px`, `h_px`: size.
fn place_view<F>(cx: &mut Context, x_px: i32, y_px: i32, w_px: i32, h_px: i32, content: F)
where
    F: FnOnce(&mut Context) + 'static,
{
    HStack::new(cx, content)
        .position_type(PositionType::SelfDirected)
        .left(Pixels(x_px as f32))
        .top(Pixels(y_px as f32))
        .width(Pixels(w_px as f32))
        .height(Pixels(h_px as f32));
}

fn place_slider<P, FMap>(
    cx: &mut Context,
    color: &'static str,
    cx_px: i32,
    cy_px: i32,
    w_px: i32,
    params_to_param: FMap,
)
where
    P: Param + 'static,
    FMap: 'static + Fn(&Arc<MothParams>) -> &P + Copy,
{
    let h_px = 14;
    let left = cx_px - w_px / 2;
    let top = cy_px - h_px / 2;
    let cls = match color {
        "purple" => "sl-purple",
        "teal"   => "sl-teal",
        _        => "sl-amber",
    };

    HStack::new(cx, move |cx| {
        ParamSlider::new(cx, Data::params, params_to_param)
            .set_style(ParamSliderStyle::FromLeft)
            .class(cls)
            .width(Pixels(w_px as f32))
            .height(Pixels(h_px as f32));
    })
    .position_type(PositionType::SelfDirected)
    .left(Pixels(left as f32))
    .top(Pixels(top as f32))
    .width(Pixels(w_px as f32))
    .height(Pixels(h_px as f32));
}
