//! Moth — vizia GUI editor with live signal chain visualizations.
//!
//! Each section of the signal chain has a custom-drawn visualization
//! that updates reactively as parameters change:
//!
//! - **Exciter**: energy envelope shape + coupling mode indicators
//! - **Vibrator**: decaying partial sinusoids showing damping & dispersion
//! - **Body**: morphing cross-section outline + modal frequency spectrum
//! - **Character**: saturation transfer curve (tape vs tube)
//! - **Spatial**: FDN delay line radial diagram

use nih_plug::prelude::*;
use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::vizia::vg;
use nih_plug_vizia::widgets::*;
use nih_plug_vizia::{assets, create_vizia_editor, ViziaState, ViziaTheming};
use std::sync::Arc;

use crate::MothParams;

// ─── Colours ────────────────────────────────────────────────────────────────

const COL_AMBER: vg::Color     = vg::Color::rgbf(0.831, 0.659, 0.333);
const COL_AMBER_HI: vg::Color  = vg::Color::rgbf(0.910, 0.753, 0.416);
const COL_AMBER_MUT: vg::Color = vg::Color::rgbf(0.541, 0.478, 0.396);
const COL_BG: vg::Color        = vg::Color::rgbf(0.078, 0.071, 0.063);
const COL_LINE: vg::Color      = vg::Color::rgbf(0.165, 0.153, 0.141);

// ─── Window ─────────────────────────────────────────────────────────────────

const EDITOR_WIDTH: u32 = 920;
const EDITOR_HEIGHT: u32 = 560;

// ─── CSS ────────────────────────────────────────────────────────────────────

const STYLE: &str = r#"
* {
    font-size: 13;
}

.moth-root {
    background-color: #1a1816;
    child-space: 0px;
    width: 1s;
    height: 1s;
}

.header {
    height: auto;
    width: 1s;
    child-top: 8px;
    child-bottom: 8px;
    child-left: 16px;
    child-right: 16px;
    col-between: 8px;
    background-color: #1e1c19;
    border-color: #2a2724;
    border-width: 0px 0px 1px 0px;
}

.title {
    color: #d4a855;
    font-size: 20;
    width: auto;
    height: auto;
}

.subtitle {
    color: #6b6560;
    font-size: 11;
    width: auto;
    height: auto;
    child-top: 1s;
    child-bottom: 0px;
}

.vendor {
    color: #6b6560;
    font-size: 11;
    width: auto;
    height: auto;
    child-left: 1s;
    child-top: 1s;
    child-bottom: 0px;
}

.signal-chain {
    width: 1s;
    height: 1s;
    child-space: 0px;
    child-top: 4px;
    child-bottom: 8px;
    child-left: 8px;
    child-right: 8px;
    col-between: 2px;
}

.section {
    width: 1s;
    height: 1s;
    child-space: 0px;
    child-left: 6px;
    child-right: 6px;
    child-top: 4px;
    row-between: 2px;
}

.section-narrow {
    width: 1s;
    max-width: 110px;
    height: 1s;
    child-space: 0px;
    child-left: 6px;
    child-right: 6px;
    child-top: 4px;
    row-between: 2px;
}

.section-header {
    color: #d4a855;
    font-size: 10;
    width: 1s;
    height: auto;
    child-left: 1s;
    child-right: 1s;
    child-bottom: 4px;
}

.section-header-muted {
    color: #8a7a65;
    font-size: 10;
    width: 1s;
    height: auto;
    child-left: 1s;
    child-right: 1s;
    child-bottom: 4px;
}

.section-divider {
    width: 1px;
    height: 1s;
    background-color: #2a2724;
}

.vis {
    width: 1s;
    height: 90px;
    border-radius: 6px;
    child-space: 0px;
}

.vis-tall {
    width: 1s;
    height: 110px;
    border-radius: 6px;
    child-space: 0px;
}

.param-row {
    width: 1s;
    height: auto;
    row-between: 1px;
    child-bottom: 3px;
}

.param-label {
    color: #8a8580;
    font-size: 10;
    width: 1s;
    height: auto;
    child-bottom: 1px;
}

.param-row .widget {
    width: 1s;
    height: 18px;
}

.footer {
    height: auto;
    width: 1s;
    child-left: 1s;
    child-right: 1s;
    child-top: 2px;
    child-bottom: 6px;
    border-color: #2a2724;
    border-width: 1px 0px 0px 0px;
    col-between: 4px;
}

.footer-text {
    color: #4a4640;
    font-size: 9;
    width: auto;
    height: auto;
}

param-slider {
    background-color: #252320;
    border-radius: 3px;
}
"#;

// ─── Colour helpers ─────────────────────────────────────────────────────────

fn col_a(base: vg::Color, alpha: f32) -> vg::Color {
    vg::Color::rgbaf(base.r, base.g, base.b, alpha)
}

fn fill(color: vg::Color) -> vg::Paint {
    vg::Paint::color(color)
}

fn stroke(color: vg::Color, width: f32) -> vg::Paint {
    let mut p = vg::Paint::color(color);
    p.set_line_width(width);
    p
}

// ═══════════════════════════════════════════════════════════════════════════
//  Custom visualisation views
// ═══════════════════════════════════════════════════════════════════════════

// ─── Exciter vis ────────────────────────────────────────────────────────────

struct ExciterVis {
    params: Arc<MothParams>,
}

impl ExciterVis {
    pub fn new(cx: &mut Context, params: Arc<MothParams>) -> Handle<Self> {
        Self { params }.build(cx, |_| {})
    }
}

impl View for ExciterVis {
    fn element(&self) -> Option<&'static str> {
        Some("exciter-vis")
    }

    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let bounds = cx.bounds();
        let x = bounds.x;
        let y = bounds.y;
        let w = bounds.w;
        let h = bounds.h;

        let mut bg = vg::Path::new();
        bg.rounded_rect(x, y, w, h, 6.0);
        canvas.fill_path(&bg, &fill(COL_BG));

        let morph = self.params.exciter_morph.value();
        let tilt = self.params.spectral_tilt.value();

        // Coupling values from the 6 presets used by morph_exciter
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

        let pad = 15.0;
        let ew = w - pad * 2.0;
        let mid_y = y + h * 0.45;
        let steps = (ew as usize).max(2);

        // Centre line
        let mut cl = vg::Path::new();
        cl.move_to(x + pad, mid_y);
        cl.line_to(x + pad + ew, mid_y);
        canvas.stroke_path(&cl, &stroke(COL_LINE, 0.5));

        // Main energy envelope
        let mut env_path = vg::Path::new();
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let env = if ec < 0.3 {
                let att = (-t * 20.0 * (1.0 - ec * 2.0)).exp();
                let dec = (-t * 8.0 * (1.0 - tilt)).exp();
                att * dec * fd
                    + ff * 0.3 * (t * 40.0).sin() * (-t * 3.0).exp()
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
            };
            let py = mid_y - env * (h * 0.35) * (0.5 + tilt * 0.5);
            let px = x + pad + i as f32;
            if i == 0 { env_path.move_to(px, py); } else { env_path.line_to(px, py); }
        }
        canvas.stroke_path(&env_path, &stroke(COL_AMBER, 1.5));

        // Mirror envelope (faint)
        let mut mir = vg::Path::new();
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let env = if ec < 0.3 {
                (-t * 20.0 * (1.0 - ec * 2.0)).exp() * (-t * 8.0 * (1.0 - tilt)).exp() * fd
            } else {
                (1.0 - (-t * 30.0).exp()) * (0.3 + ec * 0.5)
            };
            let py = mid_y + env * (h * 0.35) * (0.5 + tilt * 0.5);
            let px = x + pad + i as f32;
            if i == 0 { mir.move_to(px, py); } else { mir.line_to(px, py); }
        }
        canvas.stroke_path(&mir, &stroke(col_a(COL_AMBER_HI, 0.35), 1.0));

        // Coupling mode bars: d / f / p
        let bar_y = y + h - 14.0;
        let bar_w = 16.0;
        let bar_h = 6.0;
        let bar_gap = 4.0;
        let bar_x = x + pad;
        for (i, strength) in [fd, ff, fp].iter().enumerate() {
            let bx = bar_x + i as f32 * (bar_w + bar_gap);
            let mut bar = vg::Path::new();
            bar.rounded_rect(bx, bar_y, bar_w, bar_h, 2.0);
            let c = if *strength > 0.01 { COL_AMBER } else { COL_LINE };
            canvas.fill_path(&bar, &fill(c));
        }
    }
}

// ─── Vibrator vis ───────────────────────────────────────────────────────────

struct VibratorVis {
    params: Arc<MothParams>,
}

impl VibratorVis {
    pub fn new(cx: &mut Context, params: Arc<MothParams>) -> Handle<Self> {
        Self { params }.build(cx, |_| {})
    }
}

impl View for VibratorVis {
    fn element(&self) -> Option<&'static str> {
        Some("vibrator-vis")
    }

    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let bounds = cx.bounds();
        let x = bounds.x;
        let y = bounds.y;
        let w = bounds.w;
        let h = bounds.h;

        let mut bg = vg::Path::new();
        bg.rounded_rect(x, y, w, h, 6.0);
        canvas.fill_path(&bg, &fill(COL_BG));

        let damp = self.params.vib_damping.value();
        let disp = self.params.vib_dispersion.value();

        let pad = 12.0;
        let ew = w - pad * 2.0;
        let mid_y = y + h * 0.5;
        let partials = 10;
        let steps = (ew as usize).max(2);

        let mut cl = vg::Path::new();
        cl.move_to(x + pad, mid_y);
        cl.line_to(x + pad + ew, mid_y);
        canvas.stroke_path(&cl, &stroke(COL_LINE, 0.5));

        for p in 1..=partials {
            let pf = p as f32;
            let amp = 0.85_f32.powf(pf - 1.0) * (0.3 + damp * 0.7);
            let freq = pf * (1.0 + disp * disp * 0.02 * pf * pf);
            let alpha = (amp * 0.9).max(0.08);
            let c = if p <= 3 { COL_AMBER } else { COL_AMBER_HI };
            let lw = if p <= 3 { 1.5 } else { 0.7 };

            let mut path = vg::Path::new();
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let env = (-t * (3.0 + pf * 0.5) * (1.1 - damp)).exp();
                let val = (t * std::f32::consts::PI * 2.0 * freq * 2.0).sin()
                    * amp * env * (h * 0.35);
                let px = x + pad + i as f32;
                let py = mid_y - val;
                if i == 0 { path.move_to(px, py); } else { path.line_to(px, py); }
            }
            canvas.stroke_path(&path, &stroke(col_a(c, alpha), lw));
        }
    }
}

// ─── Body vis ───────────────────────────────────────────────────────────────

struct BodyVis {
    params: Arc<MothParams>,
}

impl BodyVis {
    pub fn new(cx: &mut Context, params: Arc<MothParams>) -> Handle<Self> {
        Self { params }.build(cx, |_| {})
    }
}

impl View for BodyVis {
    fn element(&self) -> Option<&'static str> {
        Some("body-vis")
    }

    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let bounds = cx.bounds();
        let bx = bounds.x;
        let by = bounds.y;
        let w = bounds.w;
        let h = bounds.h;

        let mut bg = vg::Path::new();
        bg.rounded_rect(bx, by, w, h, 6.0);
        canvas.fill_path(&bg, &fill(COL_BG));

        let geom = self.params.body_geometry.value();
        let size = self.params.body_size.value();

        let cx2 = bx + w * 0.5;
        let cy = by + h * 0.40;
        let sc = 0.5 + size * 0.5;
        let stiff = (geom - 0.25) * 4.0;

        let mut body_path = vg::Path::new();

        if stiff < -0.5 {
            // Tube: tall narrow ellipse
            body_path.ellipse(cx2, cy, 18.0 * sc + 8.0, 40.0 * sc + 12.0);
        } else if stiff < 0.3 {
            // Guitar: waisted body
            let bw = 28.0 * sc + 12.0;
            let bh = 38.0 * sc + 10.0;
            let waist = 0.55 + stiff * 0.3;

            body_path.move_to(cx2, cy - bh);
            body_path.bezier_to(
                cx2 + bw * 0.7, cy - bh, cx2 + bw, cy - bh * 0.5,
                cx2 + bw * 0.85, cy - bh * 0.15);
            body_path.bezier_to(
                cx2 + bw * waist, cy, cx2 + bw * waist, cy,
                cx2 + bw * 0.9, cy + bh * 0.2);
            body_path.bezier_to(
                cx2 + bw * 1.1, cy + bh * 0.6, cx2 + bw * 0.8, cy + bh,
                cx2, cy + bh);
            body_path.bezier_to(
                cx2 - bw * 0.8, cy + bh, cx2 - bw * 1.1, cy + bh * 0.6,
                cx2 - bw * 0.9, cy + bh * 0.2);
            body_path.bezier_to(
                cx2 - bw * waist, cy, cx2 - bw * waist, cy,
                cx2 - bw * 0.85, cy - bh * 0.15);
            body_path.bezier_to(
                cx2 - bw, cy - bh * 0.5, cx2 - bw * 0.7, cy - bh,
                cx2, cy - bh);
            body_path.close();
        } else if stiff < 0.7 {
            // Box / plate: rounded rectangle
            let bw = 32.0 * sc + 8.0;
            let bh = 22.0 * sc + 8.0;
            let sq = 0.2 + (stiff - 0.3) * 1.5;
            let r = bw.min(bh) * (1.0 - sq).max(0.05);
            body_path.rounded_rect(cx2 - bw, cy - bh, bw * 2.0, bh * 2.0, r);
        } else {
            // Bell: wide ellipse trending circular
            let br = 28.0 * sc + 8.0;
            let flat = (stiff - 0.7) * 3.0;
            body_path.ellipse(cx2, cy, br * (1.0 - flat * 0.3), br * (1.0 + flat * 0.5));
        }

        canvas.fill_path(&body_path, &fill(col_a(COL_AMBER, 0.06)));
        canvas.stroke_path(&body_path, &stroke(COL_AMBER, 1.5));

        // Sound hole (guitar-ish range only)
        if stiff > -0.3 && stiff < 0.5 {
            let mut hole = vg::Path::new();
            hole.ellipse(cx2, cy + 7.0 * sc, 5.0 * sc, 2.0 * sc);
            canvas.stroke_path(&hole, &stroke(COL_AMBER_MUT, 0.5));
        }

        // Mode spectrum bars at bottom
        let mode_y = by + h - 14.0;
        let mode_w = w - 20.0;
        let stiffness = (geom - 0.25) * 0.04;

        for i in 0..14 {
            let mf = (i + 1) as f32;
            let mut stretch = 1.0_f32;
            let mut acc = stiffness;
            for _ in 0..i {
                acc *= if stiffness < 0.0 { 0.93 } else { 0.98 };
                stretch += acc;
            }
            let pos = (mf * stretch) / 28.0;
            if pos > 1.0 { continue; }

            let gain = 1.0 / (1.0 + i as f32 * 0.3);
            let mx = bx + 10.0 + pos * mode_w;
            let c = if i < 3 { COL_AMBER } else { COL_AMBER_MUT };
            let lw = if i < 3 { 1.5 } else { 0.8 };

            let mut bar = vg::Path::new();
            bar.move_to(mx, mode_y);
            bar.line_to(mx, mode_y - gain * 16.0);
            canvas.stroke_path(&bar, &stroke(c, lw));
        }
    }
}

// ─── Nonlin vis ─────────────────────────────────────────────────────────────

struct NonlinVis {
    params: Arc<MothParams>,
}

impl NonlinVis {
    pub fn new(cx: &mut Context, params: Arc<MothParams>) -> Handle<Self> {
        Self { params }.build(cx, |_| {})
    }
}

impl View for NonlinVis {
    fn element(&self) -> Option<&'static str> {
        Some("nonlin-vis")
    }

    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let bounds = cx.bounds();
        let bx = bounds.x;
        let by = bounds.y;
        let w = bounds.w;
        let h = bounds.h;

        let mut bg = vg::Path::new();
        bg.rounded_rect(bx, by, w, h, 6.0);
        canvas.fill_path(&bg, &fill(COL_BG));

        let drive = self.params.nl_drive.value();
        let tape = self.params.nl_tape.value();
        let tube = self.params.nl_tube.value();
        let tt = if tape + tube > 0.001 { tape / (tape + tube) } else { 0.5 };
        let asym = 1.0 + (1.0 - tt) * 0.07;

        let pad = 16.0;
        let gw = w - pad * 2.0;
        let gh = h - pad * 2.0;
        let gc_x = bx + pad;
        let gc_y = by + pad;

        // Axes
        let mut axes = vg::Path::new();
        axes.move_to(gc_x, gc_y + gh * 0.5);
        axes.line_to(gc_x + gw, gc_y + gh * 0.5);
        axes.move_to(gc_x + gw * 0.5, gc_y);
        axes.line_to(gc_x + gw * 0.5, gc_y + gh);
        canvas.stroke_path(&axes, &stroke(COL_LINE, 0.5));

        // Unity line (dashed via segments)
        let mut unity = vg::Path::new();
        for d in 0..20 {
            let t0 = d as f32 / 20.0;
            let t1 = (d as f32 + 0.5) / 20.0;
            unity.move_to(gc_x + t0 * gw, gc_y + (1.0 - t0) * gh);
            unity.line_to(gc_x + t1 * gw, gc_y + (1.0 - t1) * gh);
        }
        canvas.stroke_path(&unity, &stroke(col_a(COL_LINE, 0.5), 0.5));

        let soft_sat = |v: f32| -> f32 { v * (27.0 + v * v) / (27.0 + 9.0 * v * v) };

        let steps = (gw as usize).max(2);

        // Ghost curve at higher drive
        {
            let mut path = vg::Path::new();
            let mut prev = 0.0_f32;
            let d = drive * 1.5;
            for i in 0..=steps {
                let inp = (i as f32 / steps as f32) * 2.0 - 1.0;
                let driven = inp * d;
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
                let out_c = out.clamp(-1.0, 1.0);
                let px = gc_x + i as f32;
                let py = gc_y + gh * 0.5 - out_c * gh * 0.5 * 0.9;
                if i == 0 { path.move_to(px, py); } else { path.line_to(px, py); }
            }
            canvas.stroke_path(&path, &stroke(col_a(COL_AMBER_HI, 0.25), 1.0));
        }

        // Main transfer curve
        {
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
                let out_c = out.clamp(-1.0, 1.0);
                let px = gc_x + i as f32;
                let py = gc_y + gh * 0.5 - out_c * gh * 0.5 * 0.9;
                if i == 0 { path.move_to(px, py); } else { path.line_to(px, py); }
            }
            canvas.stroke_path(&path, &stroke(COL_AMBER, 2.0));
        }
    }
}

// ─── Spatial vis ────────────────────────────────────────────────────────────

struct SpatialVis {
    params: Arc<MothParams>,
}

impl SpatialVis {
    pub fn new(cx: &mut Context, params: Arc<MothParams>) -> Handle<Self> {
        Self { params }.build(cx, |_| {})
    }
}

impl View for SpatialVis {
    fn element(&self) -> Option<&'static str> {
        Some("spatial-vis")
    }

    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let bounds = cx.bounds();
        let bx = bounds.x;
        let by = bounds.y;
        let w = bounds.w;
        let h = bounds.h;

        let mut bg = vg::Path::new();
        bg.rounded_rect(bx, by, w, h, 6.0);
        canvas.fill_path(&bg, &fill(COL_BG));

        let room = self.params.room_size.value();
        let rev = self.params.room_mix.value();

        let cx2 = bx + w * 0.5;
        let cy = by + h * 0.5;
        let max_r = (w.min(h) * 0.5) - 8.0;

        // Concentric decay circles
        for i in (0..=4).rev() {
            let r = (0.2 + i as f32 * 0.2) * room * max_r + 6.0;
            let alpha = 0.04 + i as f32 * 0.025 * (1.0 - rev * 0.5);
            let mut circle = vg::Path::new();
            circle.circle(cx2, cy, r);
            canvas.fill_path(&circle, &fill(col_a(COL_AMBER, alpha)));
        }

        // Centre dot
        let mut dot = vg::Path::new();
        dot.circle(cx2, cy, 3.0);
        canvas.fill_path(&dot, &fill(COL_AMBER));

        // FDN delay lines
        let delays = [1087.0_f32, 1283.0, 1429.0, 1597.0];
        let ray_alpha = 0.35 + rev * 0.5;

        for (i, &delay) in delays.iter().enumerate() {
            let angle = i as f32 * std::f32::consts::FRAC_PI_2
                + std::f32::consts::FRAC_PI_4;
            let r2 = (delay / 1597.0) * room * max_r * 0.8 + 4.0;
            let ex = cx2 + r2 * angle.cos();
            let ey = cy + r2 * angle.sin();

            let mut ray = vg::Path::new();
            ray.move_to(cx2 + 4.0 * angle.cos(), cy + 4.0 * angle.sin());
            ray.line_to(ex, ey);
            canvas.stroke_path(&ray, &stroke(col_a(COL_AMBER_MUT, ray_alpha), 0.5));

            let mut tip = vg::Path::new();
            tip.circle(ex, ey, 2.0);
            canvas.fill_path(&tip, &fill(col_a(COL_AMBER_MUT, ray_alpha)));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Editor creation
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Lens)]
struct Data {
    params: Arc<MothParams>,
}

impl Model for Data {}

pub(crate) fn default_state() -> Arc<ViziaState> {
    ViziaState::new(|| (EDITOR_WIDTH, EDITOR_HEIGHT))
}

pub(crate) fn create(
    params: Arc<MothParams>,
    editor_state: Arc<ViziaState>,
) -> Option<Box<dyn Editor>> {
    create_vizia_editor(editor_state, ViziaTheming::Custom, move |cx, _| {
        assets::register_noto_sans_light(cx);
        assets::register_noto_sans_thin(cx);

        cx.add_stylesheet(STYLE).expect("moth: failed to add stylesheet");

        let p = params.clone();

        Data { params: params.clone() }.build(cx);

        VStack::new(cx, |cx| {
            // ── Header ──
            HStack::new(cx, |cx| {
                Label::new(cx, "MOTH").class("title");
                Label::new(cx, "physical modelling synthesiser").class("subtitle");
                Label::new(cx, "RYO Modular").class("vendor");
            })
            .class("header");

            // ── Signal chain ──
            HStack::new(cx, |cx| {
                // ─── EXCITER ───
                VStack::new(cx, |cx| {
                    Label::new(cx, "EXCITER").class("section-header");
                    ExciterVis::new(cx, p.clone()).class("vis");
                    param_row(cx, "Morph", |p| &p.exciter_morph);
                    param_row(cx, "Tilt", |p| &p.spectral_tilt);
                    param_row(cx, "Stochastic", |p| &p.stochasticity);
                }).class("section");

                Element::new(cx).class("section-divider");

                // ─── VIBRATOR ───
                VStack::new(cx, |cx| {
                    Label::new(cx, "VIBRATOR").class("section-header");
                    VibratorVis::new(cx, p.clone()).class("vis");
                    param_row(cx, "Damping", |p| &p.vib_damping);
                    param_row(cx, "Brightness", |p| &p.vib_brightness);
                    param_row(cx, "Dispersion", |p| &p.vib_dispersion);
                    param_row(cx, "Position", |p| &p.position);
                }).class("section");

                Element::new(cx).class("section-divider");

                // ─── BODY ───
                VStack::new(cx, |cx| {
                    Label::new(cx, "BODY").class("section-header");
                    BodyVis::new(cx, p.clone()).class("vis-tall");
                    param_row(cx, "Geometry", |p| &p.body_geometry);
                    param_row(cx, "Brightness", |p| &p.body_brightness);
                    param_row(cx, "Damping", |p| &p.body_damping);
                    param_row(cx, "Size", |p| &p.body_size);
                }).class("section");

                Element::new(cx).class("section-divider");

                // ─── MIX ───
                VStack::new(cx, |cx| {
                    Label::new(cx, "MIX").class("section-header-muted");
                    param_row(cx, "Bleed", |p| &p.exciter_bleed);
                    param_row(cx, "Body Mix", |p| &p.body_mix);
                }).class("section-narrow");

                Element::new(cx).class("section-divider");

                // ─── CHARACTER ───
                VStack::new(cx, |cx| {
                    Label::new(cx, "CHARACTER").class("section-header");
                    NonlinVis::new(cx, p.clone()).class("vis");
                    param_row(cx, "Drive", |p| &p.nl_drive);
                    param_row(cx, "Tape", |p| &p.nl_tape);
                    param_row(cx, "Tube", |p| &p.nl_tube);
                    param_row(cx, "Warmth", |p| &p.nl_warmth);
                    param_row(cx, "Tone", |p| &p.nl_tone);
                }).class("section");

                Element::new(cx).class("section-divider");

                // ─── SPACE ───
                VStack::new(cx, |cx| {
                    Label::new(cx, "SPACE").class("section-header");
                    SpatialVis::new(cx, p.clone()).class("vis");
                    param_row(cx, "Room", |p| &p.room_size);
                    param_row(cx, "Reverb", |p| &p.room_mix);
                }).class("section-narrow");

                Element::new(cx).class("section-divider");

                // ─── MASTER ───
                VStack::new(cx, |cx| {
                    Label::new(cx, "OUT").class("section-header");
                    param_row(cx, "Master", |p| &p.master_gain);
                }).class("section-narrow");
            })
            .class("signal-chain");

            // ── Footer ──
            HStack::new(cx, |cx| {
                Label::new(cx, "exciter \u{2192} vibrator \u{2192} body \u{2192} nonlin \u{2192} spatial \u{2192} out")
                    .class("footer-text");
            }).class("footer");
        })
        .class("moth-root");

        ResizeHandle::new(cx);
    })
}

// ─── Helper ─────────────────────────────────────────────────────────────────

fn param_row<P, FMap>(cx: &mut Context, label: &str, params_to_param: FMap)
where
    P: Param + 'static,
    FMap: 'static + Fn(&Arc<MothParams>) -> &P + Copy,
{
    VStack::new(cx, move |cx| {
        Label::new(cx, label).class("param-label");
        ParamSlider::new(cx, Data::params, params_to_param)
            .set_style(ParamSliderStyle::FromLeft)
            .class("widget");
    })
    .class("param-row");
}
