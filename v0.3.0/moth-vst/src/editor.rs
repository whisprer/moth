//! Moth — vizia GUI editor with full-spectrum signal chain visualizations.
//!
//! Three-colour system:
//! - **Amber**  — primary: structure, energy envelope, main transfer curves
//! - **Purple** — secondary: spectral modifiers (brightness, warmth, stochasticity)
//! - **Teal**   — tertiary: position, damping Q, tone shaping
//!
//! Every parameter is visible on at least one visualization.

use nih_plug::prelude::*;
use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::vizia::vg;
use nih_plug_vizia::widgets::*;
use nih_plug_vizia::{assets, create_vizia_editor, ViziaState, ViziaTheming};
use std::sync::Arc;

use crate::MothParams;

// ─── Palette ────────────────────────────────────────────────────────────────

const AMBER: vg::Color = vg::Color::rgbf(0.831, 0.659, 0.333); // #D4A855
const AMBER_HI: vg::Color = vg::Color::rgbf(0.910, 0.753, 0.416); // #E8C06A
const AMBER_MUT: vg::Color = vg::Color::rgbf(0.541, 0.478, 0.396); // #8A7A65
const PURPLE: vg::Color = vg::Color::rgbf(0.608, 0.478, 0.847); // #9B7AD8
const PURPLE_MUT: vg::Color = vg::Color::rgbf(0.435, 0.369, 0.580); // #6F5E94
const TEAL: vg::Color = vg::Color::rgbf(0.365, 0.792, 0.647); // #5DCAA5
const TEAL_MUT: vg::Color = vg::Color::rgbf(0.290, 0.545, 0.463); // #4A8B76
const BG: vg::Color = vg::Color::rgbf(0.078, 0.071, 0.063); // #141210
const LINE: vg::Color = vg::Color::rgbf(0.165, 0.153, 0.141); // #2A2724

const EDITOR_WIDTH: u32 = 920;
const EDITOR_HEIGHT: u32 = 580;

// ─── CSS ────────────────────────────────────────────────────────────────────

const STYLE: &str = r#"
* { font-size: 13; }

.moth-root {
    background-color: #1a1816;
    child-space: 0px;
    width: 1s;
    height: 1s;
}

.header {
    height: auto; width: 1s;
    child-top: 8px; child-bottom: 8px;
    child-left: 16px; child-right: 16px;
    col-between: 8px;
    background-color: #1e1c19;
    border-color: #2a2724;
    border-width: 0px 0px 1px 0px;
}
.title { color: #d4a855; font-size: 20; width: auto; height: auto; }
.subtitle { color: #6b6560; font-size: 11; width: auto; height: auto; child-top: 1s; child-bottom: 0px; }
.vendor { color: #6b6560; font-size: 11; width: auto; height: auto; child-left: 1s; child-top: 1s; child-bottom: 0px; }

.signal-chain {
    width: 1s; height: 1s;
    child-space: 0px;
    child-top: 4px; child-bottom: 8px;
    child-left: 8px; child-right: 8px;
    col-between: 2px;
}

.section {
    width: 1s; height: 1s;
    child-space: 0px;
    child-left: 6px; child-right: 6px;
    child-top: 4px; row-between: 2px;
}
.section-narrow {
    width: 1s; max-width: 110px; height: 1s;
    child-space: 0px;
    child-left: 6px; child-right: 6px;
    child-top: 4px; row-between: 2px;
}
.section-header { color: #d4a855; font-size: 10; width: 1s; height: auto; child-left: 1s; child-right: 1s; child-bottom: 4px; }
.section-header-muted { color: #8a7a65; font-size: 10; width: 1s; height: auto; child-left: 1s; child-right: 1s; child-bottom: 4px; }
.section-divider { width: 1px; height: 1s; background-color: #2a2724; }

.vis { width: 1s; height: 100px; border-radius: 6px; child-space: 0px; }
.vis-tall { width: 1s; height: 120px; border-radius: 6px; child-space: 0px; }

.param-row { width: 1s; height: auto; row-between: 1px; child-bottom: 3px; }
.param-label { color: #8a8580; font-size: 10; width: 1s; height: auto; child-bottom: 1px; }
.param-row .widget { width: 1s; height: 18px; }

.footer {
    height: auto; width: 1s;
    child-left: 1s; child-right: 1s;
    child-top: 2px; child-bottom: 6px;
    border-color: #2a2724;
    border-width: 1px 0px 0px 0px;
    col-between: 4px;
}
.footer-text { color: #4a4640; font-size: 9; width: auto; height: auto; }

.legend {
    height: auto; width: 1s;
    child-left: 1s; child-right: 1s;
    child-bottom: 2px;
    col-between: 16px;
}
.legend-item { width: auto; height: auto; col-between: 4px; child-top: 1s; child-bottom: 1s; }
.legend-dot { width: 6px; height: 6px; border-radius: 3px; }
.dot-amber { background-color: #d4a855; }
.dot-purple { background-color: #9b7ad8; }
.dot-teal { background-color: #5dcaa5; }
.legend-label { font-size: 9; color: #6b6560; width: auto; height: auto; }

param-slider { background-color: #252320; border-radius: 3px; color: #d4a855; }
param-slider .fill { background-color: #4a4640; }
param-slider.sl-purple { color: #9b7ad8; }
param-slider.sl-teal { color: #5dcaa5; }
param-slider.sl-amber { color: #d4a855; }

.lbl-amber { color: #8a7a65; }
.lbl-purple { color: #6f5e94; }
.lbl-teal { color: #4a8b76; }
"#;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn col_a(c: vg::Color, a: f32) -> vg::Color {
    vg::Color::rgbaf(c.r, c.g, c.b, a)
}
fn pfill(c: vg::Color) -> vg::Paint {
    vg::Paint::color(c)
}
fn pstroke(c: vg::Color, w: f32) -> vg::Paint {
    let mut p = vg::Paint::color(c);
    p.set_line_width(w);
    p
}

// ═══════════════════════════════════════════════════════════════════════════
//  EXCITER VISUALIZATION
// ═══════════════════════════════════════════════════════════════════════════
// Amber:  energy envelope shape (morph + tilt)
// Purple: stochasticity noise band around envelope
// Teal:   coupling mode indicator bars (d/f/p)

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
        let b = cx.bounds();
        let (x, y, w, h) = (b.x, b.y, b.w, b.h);

        let mut bg = vg::Path::new();
        bg.rounded_rect(x, y, w, h, 6.0);
        canvas.fill_path(&bg, &pfill(BG));

        let morph = self.params.exciter_morph.value();
        let tilt = self.params.spectral_tilt.value();
        let stoch = self.params.stochasticity.value();

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
        let mid_y = y + h * 0.42;
        let steps = (ew as usize).max(2);

        // Centre line
        let mut cl = vg::Path::new();
        cl.move_to(x + pad, mid_y);
        cl.line_to(x + pad + ew, mid_y);
        canvas.stroke_path(&cl, &pstroke(LINE, 0.5));

        // Compute envelope values for reuse
        let env_at = |t: f32| -> f32 {
            if ec < 0.3 {
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
            }
        };

        let amp_scale = (h * 0.33) * (0.5 + tilt * 0.5);

        // ── Purple: stochasticity noise band ──
        if stoch > 0.01 {
            let noise_w = stoch * amp_scale * 0.6;
            let mut top_path = vg::Path::new();
            let mut bot_path = vg::Path::new();
            // Build closed polygon: top edge forward, bottom edge backward
            let mut top_pts: Vec<(f32, f32)> = Vec::with_capacity(steps + 1);
            let mut bot_pts: Vec<(f32, f32)> = Vec::with_capacity(steps + 1);
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let env = env_at(t);
                let base_y = mid_y - env * amp_scale;
                let band = noise_w * env.abs().max(0.05);
                top_pts.push((x + pad + i as f32, base_y - band));
                bot_pts.push((x + pad + i as f32, base_y + band));
            }
            let mut band_path = vg::Path::new();
            band_path.move_to(top_pts[0].0, top_pts[0].1);
            for &(px, py) in &top_pts[1..] {
                band_path.line_to(px, py);
            }
            for &(px, py) in bot_pts.iter().rev() {
                band_path.line_to(px, py);
            }
            band_path.close();
            canvas.fill_path(&band_path, &pfill(col_a(PURPLE, 0.12 + stoch * 0.15)));
        }

        // ── Amber: main envelope ──
        let mut env_path = vg::Path::new();
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let py = mid_y - env_at(t) * amp_scale;
            let px = x + pad + i as f32;
            if i == 0 {
                env_path.move_to(px, py);
            } else {
                env_path.line_to(px, py);
            }
        }
        canvas.stroke_path(&env_path, &pstroke(AMBER, 1.5));

        // ── Amber: mirror envelope (faint) ──
        let mut mir = vg::Path::new();
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let env = if ec < 0.3 {
                (-t * 20.0 * (1.0 - ec * 2.0)).exp() * (-t * 8.0 * (1.0 - tilt)).exp() * fd
            } else {
                (1.0 - (-t * 30.0).exp()) * (0.3 + ec * 0.5)
            };
            let py = mid_y + env * amp_scale;
            let px = x + pad + i as f32;
            if i == 0 {
                mir.move_to(px, py);
            } else {
                mir.line_to(px, py);
            }
        }
        canvas.stroke_path(&mir, &pstroke(col_a(AMBER_HI, 0.3), 1.0));

        // ── Teal: coupling mode bars (d / f / p) ──
        let bar_y = y + h - 14.0;
        let bar_w = 16.0;
        let bar_h = 6.0;
        let bar_gap = 4.0;
        let bar_x = x + pad;
        for (i, &strength) in [fd, ff, fp].iter().enumerate() {
            let bx = bar_x + i as f32 * (bar_w + bar_gap);
            let mut bar = vg::Path::new();
            bar.rounded_rect(bx, bar_y, bar_w, bar_h, 2.0);
            let c = if strength > 0.01 { TEAL } else { LINE };
            canvas.fill_path(&bar, &pfill(c));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  VIBRATOR VISUALIZATION
// ═══════════════════════════════════════════════════════════════════════════
// Amber:  decaying partial sinusoids (damping + dispersion)
// Purple: brightness spectral rolloff envelope
// Teal:   position comb tap marker

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
        let b = cx.bounds();
        let (x, y, w, h) = (b.x, b.y, b.w, b.h);

        let mut bg = vg::Path::new();
        bg.rounded_rect(x, y, w, h, 6.0);
        canvas.fill_path(&bg, &pfill(BG));

        let damp = self.params.vib_damping.value();
        let bright = self.params.vib_brightness.value();
        let disp = self.params.vib_dispersion.value();
        let pos = self.params.position.value();

        let pad = 12.0;
        let ew = w - pad * 2.0;
        let mid_y = y + h * 0.5;
        let partials = 10;
        let steps = (ew as usize).max(2);

        // Centre line
        let mut cl = vg::Path::new();
        cl.move_to(x + pad, mid_y);
        cl.line_to(x + pad + ew, mid_y);
        canvas.stroke_path(&cl, &pstroke(LINE, 0.5));

        // ── Teal: position comb tap marker ──
        // Clamped position: 0.5 - 0.98 * |pos - 0.5| (Elements formula)
        let clamped_pos = 0.5 - 0.98 * (pos - 0.5_f32).abs();
        let pos_x = x + pad + clamped_pos * ew;
        let mut pos_line = vg::Path::new();
        // Dashed vertical line
        let dash_count = 8;
        for d in 0..dash_count {
            let t0 = y + 4.0 + d as f32 * (h - 8.0) / dash_count as f32;
            let t1 = t0 + (h - 8.0) / dash_count as f32 * 0.5;
            pos_line.move_to(pos_x, t0);
            pos_line.line_to(pos_x, t1);
        }
        canvas.stroke_path(&pos_line, &pstroke(col_a(TEAL, 0.5), 1.0));
        // Small teal diamond at position
        let mut dia = vg::Path::new();
        dia.move_to(pos_x, y + h - 10.0);
        dia.line_to(pos_x + 4.0, y + h - 6.0);
        dia.line_to(pos_x, y + h - 2.0);
        dia.line_to(pos_x - 4.0, y + h - 6.0);
        dia.close();
        canvas.fill_path(&dia, &pfill(col_a(TEAL, 0.7)));

        // ── Purple: brightness spectral rolloff envelope ──
        // FIR g = (1 - brightness) * 0.45 — higher g = darker
        // Show as a decaying ceiling line across the partials
        {
            let mut rolloff = vg::Path::new();
            let rolloff_rate = (1.0 - bright) * 0.45;
            for p in 0..=partials {
                let pf = (p + 1) as f32;
                let atten = (-rolloff_rate * pf * 1.2).exp();
                let rx = x + pad + (pf / (partials as f32 + 1.0)) * ew;
                let ry = mid_y - atten * (h * 0.38);
                if p == 0 {
                    rolloff.move_to(rx, ry);
                } else {
                    rolloff.line_to(rx, ry);
                }
            }
            canvas.stroke_path(&rolloff, &pstroke(col_a(PURPLE, 0.6), 1.5));
            // Mirror below
            let mut rolloff_m = vg::Path::new();
            for p in 0..=partials {
                let pf = (p + 1) as f32;
                let atten = (-rolloff_rate * pf * 1.2).exp();
                let rx = x + pad + (pf / (partials as f32 + 1.0)) * ew;
                let ry = mid_y + atten * (h * 0.38);
                if p == 0 {
                    rolloff_m.move_to(rx, ry);
                } else {
                    rolloff_m.line_to(rx, ry);
                }
            }
            canvas.stroke_path(&rolloff_m, &pstroke(col_a(PURPLE, 0.25), 1.0));
        }

        // ── Amber: partial sinusoids ──
        for p in 1..=partials {
            let pf = p as f32;
            let amp = 0.85_f32.powf(pf - 1.0) * (0.3 + damp * 0.7);
            let freq = pf * (1.0 + disp * disp * 0.02 * pf * pf);
            let alpha = (amp * 0.9).max(0.08);
            let c = if p <= 3 { AMBER } else { AMBER_HI };
            let lw = if p <= 3 { 1.5 } else { 0.7 };

            let mut path = vg::Path::new();
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let env = (-t * (3.0 + pf * 0.5) * (1.1 - damp)).exp();
                let val =
                    (t * std::f32::consts::PI * 2.0 * freq * 2.0).sin() * amp * env * (h * 0.35);
                let px = x + pad + i as f32;
                let py = mid_y - val;
                if i == 0 {
                    path.move_to(px, py);
                } else {
                    path.line_to(px, py);
                }
            }
            canvas.stroke_path(&path, &pstroke(col_a(c, alpha), lw));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  BODY VISUALIZATION
// ═══════════════════════════════════════════════════════════════════════════
// Amber:  morphing body outline (geometry + size) + mode bars
// Purple: brightness rolloff shown as mode gain envelope
// Teal:   damping shown as mode Q width indicators

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
        let b = cx.bounds();
        let (bx, by, w, h) = (b.x, b.y, b.w, b.h);

        let mut bg = vg::Path::new();
        bg.rounded_rect(bx, by, w, h, 6.0);
        canvas.fill_path(&bg, &pfill(BG));

        let geom = self.params.body_geometry.value();
        let size = self.params.body_size.value();
        let bright = self.params.body_brightness.value();
        let damp = self.params.body_damping.value();

        let cx2 = bx + w * 0.5;
        let cy = by + h * 0.38;
        let sc = 0.5 + size * 0.5;
        let stiff = (geom - 0.25) * 4.0;

        // ── Amber: body outline ──
        let mut body_path = vg::Path::new();
        if stiff < -0.5 {
            body_path.ellipse(cx2, cy, 18.0 * sc + 8.0, 40.0 * sc + 12.0);
        } else if stiff < 0.3 {
            let bw = 28.0 * sc + 12.0;
            let bh = 38.0 * sc + 10.0;
            let waist = 0.55 + stiff * 0.3;
            body_path.move_to(cx2, cy - bh);
            body_path.bezier_to(
                cx2 + bw * 0.7,
                cy - bh,
                cx2 + bw,
                cy - bh * 0.5,
                cx2 + bw * 0.85,
                cy - bh * 0.15,
            );
            body_path.bezier_to(
                cx2 + bw * waist,
                cy,
                cx2 + bw * waist,
                cy,
                cx2 + bw * 0.9,
                cy + bh * 0.2,
            );
            body_path.bezier_to(
                cx2 + bw * 1.1,
                cy + bh * 0.6,
                cx2 + bw * 0.8,
                cy + bh,
                cx2,
                cy + bh,
            );
            body_path.bezier_to(
                cx2 - bw * 0.8,
                cy + bh,
                cx2 - bw * 1.1,
                cy + bh * 0.6,
                cx2 - bw * 0.9,
                cy + bh * 0.2,
            );
            body_path.bezier_to(
                cx2 - bw * waist,
                cy,
                cx2 - bw * waist,
                cy,
                cx2 - bw * 0.85,
                cy - bh * 0.15,
            );
            body_path.bezier_to(
                cx2 - bw,
                cy - bh * 0.5,
                cx2 - bw * 0.7,
                cy - bh,
                cx2,
                cy - bh,
            );
            body_path.close();
        } else if stiff < 0.7 {
            let bw = 32.0 * sc + 8.0;
            let bh = 22.0 * sc + 8.0;
            let sq = 0.2 + (stiff - 0.3) * 1.5;
            let r = bw.min(bh) * (1.0 - sq).max(0.05);
            body_path.rounded_rect(cx2 - bw, cy - bh, bw * 2.0, bh * 2.0, r);
        } else {
            let br = 28.0 * sc + 8.0;
            let flat = (stiff - 0.7) * 3.0;
            body_path.ellipse(cx2, cy, br * (1.0 - flat * 0.3), br * (1.0 + flat * 0.5));
        }
        canvas.fill_path(&body_path, &pfill(col_a(AMBER, 0.06)));
        canvas.stroke_path(&body_path, &pstroke(AMBER, 1.5));

        // Sound hole (guitar range)
        if stiff > -0.3 && stiff < 0.5 {
            let mut hole = vg::Path::new();
            hole.ellipse(cx2, cy + 7.0 * sc, 5.0 * sc, 2.0 * sc);
            canvas.stroke_path(&hole, &pstroke(AMBER_MUT, 0.5));
        }

        // ── Mode spectrum at bottom ──
        let modes = 14;
        let mode_y = by + h - 10.0;
        let mode_w = w - 20.0;
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
            let fpos = (mf * stretch) / 28.0;
            if fpos > 1.0 {
                continue;
            }

            // Amber: base mode gain
            let gain = 1.0 / (1.0 + i as f32 * rolloff);
            // Warmth emphasis on first 3 modes
            let boosted_gain = if i == 0 {
                gain * 1.3
            } else if i < 3 {
                gain * 1.15
            } else {
                gain
            };
            let mx = bx + 10.0 + fpos * mode_w;
            let bar_h = boosted_gain * 22.0;

            // ── Teal: Q width indicator (wider = lower Q = more damped) ──
            let mode_q = base_q / (1.0 + i as f32 * 0.2);
            let q_width = (3.0 / mode_q).max(0.5).min(4.0);
            if q_width > 1.0 {
                let mut q_bar = vg::Path::new();
                q_bar.rounded_rect(mx - q_width, mode_y - bar_h, q_width * 2.0, bar_h, 1.0);
                canvas.fill_path(&q_bar, &pfill(col_a(TEAL, 0.15)));
            }

            // ── Amber: mode bar ──
            let c = if i < 3 { AMBER } else { AMBER_MUT };
            let lw = if i < 3 { 2.0 } else { 1.0 };
            let mut bar = vg::Path::new();
            bar.move_to(mx, mode_y);
            bar.line_to(mx, mode_y - bar_h);
            canvas.stroke_path(&bar, &pstroke(c, lw));
        }

        // ── Purple: brightness rolloff envelope curve ──
        {
            let mut env = vg::Path::new();
            for i in 0..=modes {
                let mf = (i + 1) as f32;
                let gain_env = 1.0 / (1.0 + i as f32 * rolloff);
                let mx = bx + 10.0 + (mf / 28.0) * mode_w;
                let my = mode_y - gain_env * 22.0;
                if i == 0 {
                    env.move_to(mx, my);
                } else {
                    env.line_to(mx, my);
                }
            }
            canvas.stroke_path(&env, &pstroke(col_a(PURPLE, 0.6), 1.2));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  NONLIN VISUALIZATION
// ═══════════════════════════════════════════════════════════════════════════
// Amber:  transfer curve (drive + tape/tube blend)
// Purple: warmth pre-emphasis indicator (low-freq boost before saturation)
// Teal:   tone post-filter indicator (HF rolloff after saturation)

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
        let b = cx.bounds();
        let (bx, by, w, h) = (b.x, b.y, b.w, b.h);

        let mut bg = vg::Path::new();
        bg.rounded_rect(bx, by, w, h, 6.0);
        canvas.fill_path(&bg, &pfill(BG));

        let drive = self.params.nl_drive.value();
        let tape = self.params.nl_tape.value();
        let tube = self.params.nl_tube.value();
        let warmth = self.params.nl_warmth.value();
        let tone = self.params.nl_tone.value();

        let tt = if tape + tube > 0.001 {
            tape / (tape + tube)
        } else {
            0.5
        };
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
        canvas.stroke_path(&axes, &pstroke(LINE, 0.5));

        // Unity line (dashed)
        let mut unity = vg::Path::new();
        for d in 0..20 {
            let t0 = d as f32 / 20.0;
            let t1 = (d as f32 + 0.5) / 20.0;
            unity.move_to(gc_x + t0 * gw, gc_y + (1.0 - t0) * gh);
            unity.line_to(gc_x + t1 * gw, gc_y + (1.0 - t1) * gh);
        }
        canvas.stroke_path(&unity, &pstroke(col_a(LINE, 0.5), 0.5));

        let soft_sat = |v: f32| -> f32 { v * (27.0 + v * v) / (27.0 + 9.0 * v * v) };
        let steps = (gw as usize).max(2);

        // ── Purple: warmth pre-emphasis region ──
        // Warmth boosts low frequencies before saturation — show as a
        // filled region on the left (low amplitude) side of the curve
        if warmth > 0.01 {
            let warmth_w = gw * 0.35 * warmth;
            let warmth_h = gh * 0.12 * warmth;
            let mut wp = vg::Path::new();
            wp.move_to(gc_x, gc_y + gh * 0.5);
            wp.bezier_to(
                gc_x + warmth_w * 0.3,
                gc_y + gh * 0.5 - warmth_h,
                gc_x + warmth_w * 0.7,
                gc_y + gh * 0.5 - warmth_h * 0.8,
                gc_x + warmth_w,
                gc_y + gh * 0.5,
            );
            wp.line_to(gc_x, gc_y + gh * 0.5);
            wp.close();
            canvas.fill_path(&wp, &pfill(col_a(PURPLE, 0.2)));

            // Mirror below axis
            let mut wpm = vg::Path::new();
            wpm.move_to(gc_x + gw, gc_y + gh * 0.5);
            wpm.bezier_to(
                gc_x + gw - warmth_w * 0.3,
                gc_y + gh * 0.5 + warmth_h,
                gc_x + gw - warmth_w * 0.7,
                gc_y + gh * 0.5 + warmth_h * 0.8,
                gc_x + gw - warmth_w,
                gc_y + gh * 0.5,
            );
            wpm.line_to(gc_x + gw, gc_y + gh * 0.5);
            wpm.close();
            canvas.fill_path(&wpm, &pfill(col_a(PURPLE, 0.2)));
        }

        // ── Teal: tone post-filter ──
        // Tone is a LP after saturation — show as rolloff on the extremes
        if tone < 0.99 {
            let rolloff_strength = (1.0 - tone) * 0.25;
            // Top-right corner fade (high positive output attenuated)
            let mut tf = vg::Path::new();
            let fade_start = gc_x + gw * (0.6 + tone * 0.35);
            tf.move_to(fade_start, gc_y);
            tf.line_to(gc_x + gw, gc_y);
            tf.line_to(gc_x + gw, gc_y + gh * rolloff_strength);
            tf.bezier_to(
                gc_x + gw - (gw - fade_start) * 0.5,
                gc_y + gh * rolloff_strength * 0.3,
                fade_start + (gw + gc_x - fade_start) * 0.2,
                gc_y,
                fade_start,
                gc_y,
            );
            tf.close();
            canvas.fill_path(&tf, &pfill(col_a(TEAL, 0.12)));

            // Bottom-left corner fade (mirror)
            let mut bf = vg::Path::new();
            let fade_end = gc_x + gw * (0.4 - tone * 0.35);
            bf.move_to(fade_end, gc_y + gh);
            bf.line_to(gc_x, gc_y + gh);
            bf.line_to(gc_x, gc_y + gh - gh * rolloff_strength);
            bf.bezier_to(
                gc_x + (fade_end - gc_x) * 0.5,
                gc_y + gh - gh * rolloff_strength * 0.3,
                fade_end - (fade_end - gc_x) * 0.2,
                gc_y + gh,
                fade_end,
                gc_y + gh,
            );
            bf.close();
            canvas.fill_path(&bf, &pfill(col_a(TEAL, 0.12)));
        }

        // ── Amber: ghost curve at higher drive ──
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
                    prev = v;
                    v
                } else if driven >= 0.0 {
                    soft_sat(driven)
                } else {
                    soft_sat(driven * asym) / asym
                };
                let px = gc_x + i as f32;
                let py = gc_y + gh * 0.5 - out.clamp(-1.0, 1.0) * gh * 0.5 * 0.9;
                if i == 0 {
                    path.move_to(px, py);
                } else {
                    path.line_to(px, py);
                }
            }
            canvas.stroke_path(&path, &pstroke(col_a(AMBER_HI, 0.2), 1.0));
        }

        // ── Amber: main transfer curve ──
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
                    prev = v;
                    v
                } else if driven >= 0.0 {
                    soft_sat(driven)
                } else {
                    soft_sat(driven * asym) / asym
                };
                let px = gc_x + i as f32;
                let py = gc_y + gh * 0.5 - out.clamp(-1.0, 1.0) * gh * 0.5 * 0.9;
                if i == 0 {
                    path.move_to(px, py);
                } else {
                    path.line_to(px, py);
                }
            }
            canvas.stroke_path(&path, &pstroke(AMBER, 2.0));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  SPATIAL VISUALIZATION
// ═══════════════════════════════════════════════════════════════════════════
// Amber: FDN delay lines + concentric decay circles (room + reverb)

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
        let b = cx.bounds();
        let (bx, by, w, h) = (b.x, b.y, b.w, b.h);

        let mut bg = vg::Path::new();
        bg.rounded_rect(bx, by, w, h, 6.0);
        canvas.fill_path(&bg, &pfill(BG));

        let room = self.params.room_size.value();
        let rev = self.params.room_mix.value();

        let cx2 = bx + w * 0.5;
        let cy = by + h * 0.5;
        let max_r = (w.min(h) * 0.5) - 8.0;

        for i in (0..=4).rev() {
            let r = (0.2 + i as f32 * 0.2) * room * max_r + 6.0;
            let alpha = 0.04 + i as f32 * 0.025 * (1.0 - rev * 0.5);
            let mut circle = vg::Path::new();
            circle.circle(cx2, cy, r);
            canvas.fill_path(&circle, &pfill(col_a(AMBER, alpha)));
        }

        let mut dot = vg::Path::new();
        dot.circle(cx2, cy, 3.0);
        canvas.fill_path(&dot, &pfill(AMBER));

        let delays = [1087.0_f32, 1283.0, 1429.0, 1597.0];
        let ray_alpha = 0.35 + rev * 0.5;
        for (i, &delay) in delays.iter().enumerate() {
            let angle = i as f32 * std::f32::consts::FRAC_PI_2 + std::f32::consts::FRAC_PI_4;
            let r2 = (delay / 1597.0) * room * max_r * 0.8 + 4.0;
            let ex = cx2 + r2 * angle.cos();
            let ey = cy + r2 * angle.sin();

            let mut ray = vg::Path::new();
            ray.move_to(cx2 + 4.0 * angle.cos(), cy + 4.0 * angle.sin());
            ray.line_to(ex, ey);
            canvas.stroke_path(&ray, &pstroke(col_a(AMBER_MUT, ray_alpha), 0.5));

            let mut tip = vg::Path::new();
            tip.circle(ex, ey, 2.0);
            canvas.fill_path(&tip, &pfill(col_a(AMBER_MUT, ray_alpha)));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  EDITOR
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
        cx.add_stylesheet(STYLE).expect("moth: stylesheet");

        let p = params.clone();
        Data {
            params: params.clone(),
        }
        .build(cx);

        VStack::new(cx, |cx| {
            // Header
            HStack::new(cx, |cx| {
                Label::new(cx, "MOTH").class("title");
                Label::new(cx, "physical modelling synthesiser").class("subtitle");
                Label::new(cx, "RYO Modular").class("vendor");
            }).class("header");

            // Signal chain
            HStack::new(cx, |cx| {
                // EXCITER
                VStack::new(cx, |cx| {
                    Label::new(cx, "EXCITER").class("section-header");
                    ExciterVis::new(cx, p.clone()).class("vis");
                    param_row(cx, "Morph", |p| &p.exciter_morph);
                    param_row_c(cx, "Tilt", "purple", |p| &p.spectral_tilt);
                    param_row_c(cx, "Stochastic", "purple", |p| &p.stochasticity);
                }).class("section");

                Element::new(cx).class("section-divider");

                // VIBRATOR
                VStack::new(cx, |cx| {
                    Label::new(cx, "VIBRATOR").class("section-header");
                    VibratorVis::new(cx, p.clone()).class("vis");
                    param_row(cx, "Damping", |p| &p.vib_damping);
                    param_row_c(cx, "Brightness", "purple", |p| &p.vib_brightness);
                    param_row_c(cx, "Dispersion", "teal", |p| &p.vib_dispersion);
                    param_row_c(cx, "Position", "teal", |p| &p.position);
                }).class("section");

                Element::new(cx).class("section-divider");

                // BODY
                VStack::new(cx, |cx| {
                    Label::new(cx, "BODY").class("section-header");
                    BodyVis::new(cx, p.clone()).class("vis-tall");
                    param_row(cx, "Geometry", |p| &p.body_geometry);
                    param_row_c(cx, "Brightness", "purple", |p| &p.body_brightness);
                    param_row_c(cx, "Damping", "teal", |p| &p.body_damping);
                    param_row_c(cx, "Size", "teal", |p| &p.body_size);
                }).class("section");

                Element::new(cx).class("section-divider");

                // MIX
                VStack::new(cx, |cx| {
                    Label::new(cx, "MIX").class("section-header-muted");
                    param_row_c(cx, "Bleed", "teal", |p| &p.exciter_bleed);
                    param_row(cx, "Body Mix", |p| &p.body_mix);
                }).class("section-narrow");

                Element::new(cx).class("section-divider");

                // CHARACTER
                VStack::new(cx, |cx| {
                    Label::new(cx, "CHARACTER").class("section-header");
                    NonlinVis::new(cx, p.clone()).class("vis");
                    param_row(cx, "Drive", |p| &p.nl_drive);
                    param_row(cx, "Tape", |p| &p.nl_tape);
                    param_row(cx, "Tube", |p| &p.nl_tube);
                    param_row_c(cx, "Warmth", "purple", |p| &p.nl_warmth);
                    param_row_c(cx, "Tone", "teal", |p| &p.nl_tone);
                }).class("section");

                Element::new(cx).class("section-divider");

                // SPACE
                VStack::new(cx, |cx| {
                    Label::new(cx, "SPACE").class("section-header");
                    SpatialVis::new(cx, p.clone()).class("vis");
                    param_row(cx, "Room", |p| &p.room_size);
                    param_row_c(cx, "Reverb", "teal", |p| &p.room_mix);
                }).class("section-narrow");

                Element::new(cx).class("section-divider");

                // MASTER
                VStack::new(cx, |cx| {
                    Label::new(cx, "OUT").class("section-header");
                    param_row(cx, "Master", |p| &p.master_gain);
                }).class("section-narrow");
            }).class("signal-chain");

            // Legend
            HStack::new(cx, |cx| {
                HStack::new(cx, |cx| {
                    Element::new(cx).class("legend-dot").class("dot-amber");
                    Label::new(cx, "energy / structure").class("legend-label");
                }).class("legend-item");
                HStack::new(cx, |cx| {
                    Element::new(cx).class("legend-dot").class("dot-purple");
                    Label::new(cx, "spectral / brightness").class("legend-label");
                }).class("legend-item");
                HStack::new(cx, |cx| {
                    Element::new(cx).class("legend-dot").class("dot-teal");
                    Label::new(cx, "position / damping / tone").class("legend-label");
                }).class("legend-item");
            }).class("legend");

            // Footer
            HStack::new(cx, |cx| {
                Label::new(cx, "exciter \u{2192} vibrator \u{2192} body \u{2192} nonlin \u{2192} spatial \u{2192} out")
                    .class("footer-text");
            }).class("footer");
        }).class("moth-root");

        ResizeHandle::new(cx);
    })
}

fn param_row<P, FMap>(cx: &mut Context, label: &str, params_to_param: FMap)
where
    P: Param + 'static,
    FMap: 'static + Fn(&Arc<MothParams>) -> &P + Copy,
{
    param_row_c(cx, label, "amber", params_to_param);
}

fn param_row_c<P, FMap>(cx: &mut Context, label: &str, color: &'static str, params_to_param: FMap)
where
    P: Param + 'static,
    FMap: 'static + Fn(&Arc<MothParams>) -> &P + Copy,
{
    let lbl_class = match color {
        "purple" => "lbl-purple",
        "teal" => "lbl-teal",
        _ => "lbl-amber",
    };
    let sl_class = match color {
        "purple" => "sl-purple",
        "teal" => "sl-teal",
        _ => "sl-amber",
    };
    VStack::new(cx, move |cx| {
        Label::new(cx, label).class("param-label").class(lbl_class);
        ParamSlider::new(cx, Data::params, params_to_param)
            .set_style(ParamSliderStyle::FromLeft)
            .class("widget")
            .class(sl_class);
    })
    .class("param-row");
}
