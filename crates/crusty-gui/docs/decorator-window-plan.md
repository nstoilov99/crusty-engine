# Unreal-style detachable panels — implementation plan

Goal: make tab tear-off / re-dock feel like Unreal Engine's Slate docking —
a translucent "thumbnail" that follows the cursor during the drag, drop
compasses that light up in the target window, and on release either dock into
the target or materialize a real OS window — with no opaque box stuck on top
and no white/blank window.

## How Unreal/Slate does it

Slate owns *every* window and routes input centrally, which is why theirs is
seamless and our OS-capture approach is fragile:

1. **App-level input.** `FSlateApplication` processes all mouse input centrally
   and tracks the global cursor each tick. The drag doesn't depend on one
   window's OS mouse-capture surviving across windows.
2. **A dedicated cursor-decorator window.** On drag-start Slate spawns an
   `SWindow` of type `CursorDecorator`: per-pixel transparent, borderless,
   no-activate, always-on-top, click-through. It renders only a small thumbnail
   of the dragged tab and follows the cursor. Transparent + click-through means
   it never occludes drop zones and never steals input.
3. **Drop overlays render into the target**, not the decorator. The directional
   dock compass lights up inside whichever `SDockingTabStack` is under the cursor.
4. **On release:** over a valid target → insert the tab and destroy the
   decorator; over empty space → materialize a new real `SWindow` at the
   decorator's position via `FTabManager`, then destroy the decorator.

Slate's transparency is **DWM per-pixel composition with a premultiplied-alpha
swapchain** — not a layered (`WS_EX_LAYERED`) window.

## Why our prototypes failed

- We lean on the OS's *implicit* mouse capture + per-window `WM_MOUSEMOVE`,
  which is fragile across windows/apps.
- Our decorator was an *opaque* window: under the cursor it stole events; made
  click-through (`set_cursor_hittest`) it gained `WS_EX_LAYERED`, which our
  Vulkan swapchain cannot present to → the white square.
- Our swapchain is opaque (`composite_alpha` = first-supported), so a see-through
  card was never possible.

## The enabling facts (verified in winit 0.30.13 source)

- `WindowAttributes::with_transparent(true)` on Windows maps to
  `DwmEnableBlurBehindWindow` with an empty region = **per-pixel DWM alpha**
  (`window.rs:1228`). It does **not** set `WS_EX_LAYERED` — only
  `set_cursor_hittest` does (`window_state.rs:301`).
  → **A transparent winit window is DWM-composited, Vulkan-compatible.**
- If we clear the swapchain to `[0,0,0,0]` and draw with the existing
  `SrcAlpha / OneMinusSrcAlpha` blend, the output is already premultiplied
  (`rgb·a`, `a`) — exactly what `CompositeAlpha::PreMultiplied` expects.
  → **No shader/blend changes required.**

Consequence: we get a true see-through decorator *without* the layered-window
trap — provided we **avoid click-through** and keep the small cursor offset
(which the existing source-window-driven capture model already supports).

## Plan

### Phase A — de-risk spike (gate the rest on this)
Make the decorator window transparent and confirm on the real GPU:
- `with_transparent(true)` on the decorator's `WindowAttributes`.
- Build its `WindowRenderer` in a new "transparent" mode that picks a
  premultiplied-capable `composite_alpha` and clears the present pass to
  `[0,0,0,0]` (skipping the opaque scene-background blit).
- Verify: see-through background, crisp card, **no white box**, no flicker.
- If `PreMultiplied` isn't supported, try `PostMultiplied`, then `Inherit`.

### Phase B — transparent render path in `WindowRenderer`
- Add a `transparent: bool` to `WindowRenderer`; `new_transparent` constructor.
- Swapchain: choose `composite_alpha` accordingly (`src/window/mod.rs:241`).
- `render`: when transparent, clear scene pass and present pass to `[0,0,0,0]`
  and skip the scene-background blit so only the UI mesh lands on a transparent
  surface (`src/window/mod.rs:412,496,507`).

### Phase C — decorator as a transparent card
- `spawn_dock_host(decorator=true)`: add `.with_transparent(true)`, keep
  borderless + `AlwaysOnTop` + `with_active(false)`; build the renderer in
  transparent mode.
- Keep the `DECORATOR_CURSOR_GAP` offset so the cursor stays outside the card →
  preserves source-window capture, no click-through needed.
- (Later) shrink to a tab thumbnail rather than the full panel tree.

### Phase D — drop resolution (mostly already wired)
- Over a compass target → dock + `destroy_decorator()`.
- Over empty space → `spawn_dock_host(decorator=false)` (real titled window) at
  the decorator's spot, then destroy the decorator.

### Phase E — input robustness (Slate's real lesson)
- Safety net: if a tab-drag is active but we observe the button is up without
  having resolved (focus-lost / next press), resolve it then — so a lost release
  can't strand the drag over a foreign app.
- Optional: `set_cursor_grab` on the source window at drag-start to harden capture.

## Risks / fallback
- Main risk: `PreMultiplied`/`PostMultiplied` composite alpha unsupported on the
  driver → Phase A catches it; fallback is the proven windowless ghost
  (`DECORATOR_WINDOW = false`).
- All work stays behind `DECORATOR_WINDOW`, so current behavior is untouched
  until this is solid.
