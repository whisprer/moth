# Moth GUI — Integration Guide

## What's here

Three files that add a vizia-based GUI to Moth VST:

```
moth-vst-gui/
├── Cargo.toml        → replaces  C:\github\moth\moth-vst\Cargo.toml
└── src/
    ├── lib.rs        → replaces  C:\github\moth\moth-vst\src\lib.rs
    └── editor.rs     → NEW file: C:\github\moth\moth-vst\src\editor.rs
```

## Integration steps

1. **Back up** your current `moth-vst/` directory (just in case)

2. **Copy the three files** into place:
   - `Cargo.toml` → `C:\github\moth\moth-vst\Cargo.toml`
   - `src/lib.rs` → `C:\github\moth\moth-vst\src\lib.rs`
   - `src/editor.rs` → `C:\github\moth\moth-vst\src\editor.rs`

3. **Build**:
   ```
   cd C:\github\moth
   cargo run -p xtask -- bundle moth-vst --release
   ```

4. **Copy** `target/bundled/moth-vst.vst3` to `C:\Program Files\Common Files\VST3\`

5. **Rescan** in Ableton, load Moth — the GUI should appear when you open the plugin window.

## What changed vs the original lib.rs

Only three surgical additions (all DSP code is byte-identical):

1. Added `use nih_plug_vizia::ViziaState;` and `mod editor;`
2. Added `#[persist = "editor-state"] editor_state: Arc<ViziaState>` to `MothParams`
3. Added `fn editor()` to the `Plugin` impl (4 lines)

No DSP changes. No parameter changes. No behavioural changes.

## Potential issues & fixes

**If `ViziaState::new` signature doesn't match:**
The API for `ViziaState` changed over nih-plug versions. If your pinned nih-plug
uses the older `from_size(w, h)` API, change `editor.rs` line:
```rust
// New API:
ViziaState::new(|| (EDITOR_WIDTH, EDITOR_HEIGHT))
// Old API (if needed):
ViziaState::from_size(EDITOR_WIDTH, EDITOR_HEIGHT)
```

**If `cx.add_stylesheet(STYLE)` doesn't compile:**
Older vizia versions use `cx.add_theme(STYLE)` instead. Try:
```rust
cx.add_theme(STYLE);
```

**If `Element::new(cx)` isn't found:**
Replace section dividers with:
```rust
Label::new(cx, "").class("section-divider");
```

**If the theme looks wrong (white background, etc):**
Try `ViziaTheming::Default` instead of `ViziaTheming::Custom`. The custom dark
CSS should override either, but `Default` is more forgiving as a fallback.
