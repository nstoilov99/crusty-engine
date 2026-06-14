# crusty-gui Roadmap

Goal: grow `crusty-gui` into a comprehensive GUI library that can replace
**egui** in [rust-engine](https://github.com/nstoilov99/rust-engine).

Both projects already share a stack — **vulkano 0.35**, **winit 0.30**,
**cosmic-text** — so integration is mostly additive.

## Current State

crusty-gui is a Phase 0–8 hybrid immediate/retained GUI with:

- ~15 widgets (Button, Checkbox, Radio, Slider, ProgressBar, TextEdit, Label,
  CollapsingHeader, ScrollArea, ComboBox, SelectableValue, Tooltip,
  ContextMenu, TreeNode)
- SDF rounded-rect rendering + glassmorphism (Kawase blur), WGSL→SPIR-V
- cosmic-text shaping + swash rasterization into an R8 glyph atlas
- CPU tessellator → vulkano backend, per-clip scissor ranges
- Hierarchical ID system, persistent widget `Memory`, exponential animation
- Basic docking (split/leaf tree, tab bars, splitters)
- 2-layer paint (regular + overlay)

### Engine integration targets (what must work after the swap)

The rust-engine editor uses egui + egui_dock 0.18 via a custom vulkano
integration (`engine/src/engine/gui/mod.rs`). Panels to port:

- Asset Browser (grid/list, thumbnails, drag-drop, search)
- Hierarchy Panel (scene tree)
- Inspector Panel (component editing — label/value grids)
- Profiler (flamegraph, sortable table, frame history)
- Console (filtered log viewer)
- Viewport (engine texture + transform gizmo overlay)
- Scene tabs, Mesh editor (secondary window), Build/Import dialogs
- Menu bar, status bar, toast notifications

---

## Phase 9 — Floating windows & layering *(blocker)*

The editor relies on dialogs and popups everywhere.

- [x] `Area` primitive: free-positioned UI region (foundation for windows,
      tooltips, menus)
- [x] `Window` widget: movable / resizable / collapsible, title bar
- [x] Real z-order: replace the 2-layer paint/overlay split with N ordered
      layers + bring-to-front on click
- [x] Modal support + click-outside-to-close

## Phase 10 — Texture protocol *(blocker)*

The viewport, asset thumbnails, and previews render engine images.

- [x] `TextureId` registry: user-registered vulkano `ImageView`s sampled in
      the UI shader (new textured fragment mode)
- [x] `Image` / `ImageButton` widgets with UV control
- [x] RGBA atlas page alongside R8 (also unblocks color emoji)

## Phase 11 — Layout engine upgrade

Cursor-based top-down/left-right is too weak for the inspector and profiler.

- [ ] Nested `horizontal()` / `vertical()` with alignment & justify
- [ ] `Grid` (label/value rows — inspector)
- [ ] `Table` with resizable, sortable, virtualized columns (profiler)
- [ ] Wrapping text, `available_size` negotiation, min/max constraints
- [ ] Typed per-widget memory store (egui `IdTypeMap` parity): replace the
      fixed `WidgetMemory` scratch fields with an `Id`+`TypeId` keyed any-map
      so widgets (and external/user widgets) persist their own typed state —
      table column widths, text selection ranges, etc.

## Phase 12 — Docking completion

- [ ] Tab drag-out / drag-in
- [ ] 5-way drop zones with overlay preview
- [ ] serde serialization of dock tree **and** `Memory` (egui_dock parity)

## Phase 13 — Editor-grade interaction

- [ ] Drag & drop payload system (asset browser → inspector slots)
- [ ] Clipboard (arboard) + full IME in TextEdit
- [ ] Keyboard focus traversal (Tab / Shift-Tab), shortcuts API
- [ ] Menu bar / nested menus, toasts

## Phase 14 — Painter API

- [ ] Public low-level painter: lines, polygons, beziers, text-at-pos
      (required for flamegraph, gizmo overlays, custom profiler drawing)
- [ ] Plot helpers (later — raw painter unblocks everything first)

## Phase 15 — Multi-window

- [ ] Multi-viewport: independent `Context`-per-window (or shared context
      with per-window output) — mesh editor's secondary window
- [ ] Promote the demo's winit/swapchain glue into the real `src/window/`
      module

## Phase 16 — Swap & hardening

- [ ] Define a `GuiBackend` trait in rust-engine mirroring the current
      `Gui` / `EguiRenderer` seam; implement for crusty-gui
- [ ] Port panels one at a time (start with **console** — simplest)
- [ ] Damage tracking / "needs repaint" to avoid 100% redraw, buffer reuse
- [ ] Snapshot tests (render-to-image compare) + unit tests for layout/input

---

# Beyond egui parity

Phases 9–16 reach parity. Everything after is about making crusty-gui
*better* than egui, not just equivalent.

## Phase 17 — Performance beyond egui

- [ ] Retained draw-command caching: skip tessellation for unchanged
      subtrees (egui re-tessellates everything every frame — biggest
      available win)
- [ ] Glyph atlas eviction / growth
- [ ] GPU buffer pooling
- [ ] Partial swapchain updates / damage rects

## Phase 18 — Polish that egui lacks

- [ ] Rich animation system: spring physics, layout transitions (windows /
      tabs animating into place), enter/exit animations — builds on the
      existing `approach()` foundation
- [ ] Hot-reloadable theming (style files, not code) — editor QoL
- [ ] Custom font fallback chains, color emoji, subpixel AA option

## Phase 19 — Accessibility & i18n

- [ ] AccessKit integration (screen readers, OS automation — egui has this)
- [ ] RTL text layout (cosmic-text supports it — mostly plumbing)
- [ ] Keyboard-only operation audit

## Phase 20 — Game-facing UI (not just editor)

- [ ] Worldspace UI: render to texture + raycast input mapping (HUDs,
      in-game menus — egui is weak here; the engine integration makes it
      natural)
- [ ] Gamepad / controller navigation (engine already has gamepad input)
- [ ] Data-driven UI definitions: load layouts from assets, hot-reload

## Phase 21 — Ecosystem (if open-sourced)

- [ ] Docs + examples gallery
- [ ] Crate split: `crusty-gui-winit` / `crusty-gui-vulkano` so backends are
      swappable
- [ ] Optional wgpu backend for wider adoption

## Phase 22 — Node canvas / pan-zoom *(visual scripting)*

Foundation for a Blueprint-style visual scripting editor in the engine.
Node positions live in the **graph asset** (app owns them for save/undo),
not GUI memory — the canvas just renders at given positions and reports
drags back. Depends on Phase 14 (bezier painter for wires) and leans on
Phase 17 (culling + retained tessellation) before it scales to big graphs.

- [ ] Pan/zoom canvas transform: a world↔screen coordinate space with input
      hit-tested in canvas space (the one genuinely missing primitive — all
      layout and `Area` are screen-space today)
- [ ] Free-positioned node + pin widgets, link-drag interaction on the canvas
- [ ] Bezier wire rendering between pins (uses the Phase 14 painter)
- [ ] Off-screen node culling + retained draw caching for large graphs
      (Phase 17), and glyph-atlas growth for multi-scale text under zoom

---

## Sequencing rationale

- **9 + 10** unblock porting the first real panels.
- **11** unblocks the inspector and profiler (the heaviest UIs).
- **12–13** reach editor parity with egui + egui_dock.
- **14–15** cover the long tail.
- **16** is the actual migration.
- **17 + 20** offer the highest post-parity returns: performance and
  in-game UI are egui's genuine weaknesses, and Phase 20 turns this from
  "editor UI" into the engine's only UI system.
- **22** (node canvas) is gated on **14** (bezier wires) and only scales
  with **17** (culling + retained tessellation), so it comes late despite
  building on the Phase 9 free-positioning foundation.

After Phase 11, the console and hierarchy panels should be portable.
