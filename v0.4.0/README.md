# Moth Shadow Hills — Step 1 of 2

## What this step does

Replaces the dark green CSS panel with your actual faceplate PNG as the
background, and positions the parameter sliders precisely over the slots
painted into the image. **No animated knobs, no VU needle yet** — those
come in step 2.

This is deliberately incremental. You build it, see the faceplate render,
confirm the slider positions look right, THEN we add the rotating knobs
and VU needle on top. Less risk of a 1000-line file going badly wrong.

## Files to drop in

```
moth-shadow-hills/
├── Cargo.toml          → C:\github\moth\moth-vst\Cargo.toml  (REPLACES current)
├── assets/
│   ├── faceplate.png   → C:\github\moth\moth-vst\assets\faceplate.png  (NEW DIR)
│   ├── knob_large.png  → also copy these even though we don't use them yet
│   ├── knob_small.png
│   ├── led_amber.png
│   ├── led_off.png
│   ├── screw.png
│   ├── toggle_up.png
│   ├── toggle_down.png
│   └── tube_grid.png
└── src/
    └── editor.rs       → C:\github\moth\moth-vst\src\editor.rs  (REPLACES current)
```

You'll need to:
1. Create the `assets` folder inside `moth-vst`
2. Copy all 9 PNGs into it
3. Replace `Cargo.toml` and `src/editor.rs`
4. Keep your existing `src/lib.rs` as-is (no changes needed)

## Build

```
cd C:\github\moth
cargo run -p xtask -- bundle moth-vst --release
```

The first build will be slow because cargo needs to compile the `image` crate
and its PNG decoder. Subsequent builds will be normal speed.

## What you should see

- Open Moth in Ableton
- The plugin window opens at 1344×797 showing your faceplate as the background
- 20 thin amber/purple/teal fill bars sit precisely over the painted slots
  for each parameter
- Drag any fill bar — the parameter changes, the bar fills/empties
- The 4 painted knobs (Morph, Tilt, Size, Drive) are static images for now
- The VU meter has no needle for now

## What's NOT working yet

- Knob rotation (need step 2)
- VU needle (need step 2)
- Phosphor traces in the display windows (need step 2)
- Coupling LEDs (need step 2)
- Master gain has no visible control (it's still functional, accessible via
  Ableton's parameter unfold)

## If something breaks

**"image" crate not found:** `cargo update` then rebuild.

**Faceplate doesn't appear (just dark green):** the `include_bytes!` path
is wrong — verify `moth-vst/assets/faceplate.png` exists.

**Sliders are in wrong positions:** the faceplate may have been rendered at
a different size than 1344×797. Check `Image.size` of your faceplate and
update the `FW` and `FH` constants at the top of `editor.rs`.

**ParamSlider ::set_style not found:** different nih-plug version. Remove
the `.set_style(ParamSliderStyle::FromLeft)` line.

**`position_type` / `Pixels` not found:** use the older API:
```rust
.position_type(PositionType::SelfDirected)
```
becomes
```rust
.position_type("self-directed")
```
and `Pixels(...)` may need to be `Units::Pixels(...)`.

## Next step (after this works)

Once you confirm the faceplate shows with sliders in the right positions,
we add:
- A `Knob` view that loads `knob_large.png` and rotates based on parameter value
- A `VuMeter` view that draws a needle on top of the painted meter face
- Five `Display` views (one per signal-chain section) that draw phosphor
  traces in the windows: exciter envelope, vibrator partials, body modes,
  saturation curve, FDN diagram
- Coupling mode LEDs that brighten when each mode is active

Each addition is independently testable.
