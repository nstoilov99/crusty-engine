# Crusty Editor — DESIGN.md

Source of truth: **Crusty Design System.dc.html** (visual mockup, kept local in
`rust-engine/docs/mockup/`) + **theme.rs / theme.json** (values, same folder;
the live copy is the engine's `editor/theme.rs`).
When adding a new menu or widget: find the nearest component on the reference page, reuse its
tokens, and if nothing fits, extend this doc first.

## Core rules

1. **Flat and opaque.** Depth comes from a lighter fill + a 1px border. No shadows, no gradients
   on base widgets. The only alpha in the system: `accent_soft` (~15%), `popover_alpha` (0.96,
   transient surfaces only), the modal scrim (0.45), and status tints (8–15%).
2. **Semantic tokens only.** Components never reference raw hex or preset names. If you type a
   hex value in widget code, it's a bug.
3. **Presets swap surfaces + accents only.** `selection`, `status`, `danger`, `axis`, and
   `type_colors` are pinned across Steel / Tidepool / Graphite.

## Accent budget

Accent (`accent_active`) marks *state*, not decoration. In any one toolbar/panel view, the lit
elements should be countable on one hand — typically: the active tool, the enabled snap
toggle(s), the active tab underline. If a screen feels like it needs more accent, it needs
better layout instead.

- DO: 2px active-tab top edge; toggled tool button (`accent_soft` fill + accent border + accent
  icon); checked checkbox/toggle; slider & progress fill; live profiler chips.
- DON'T: menu hover, selected rows, panel headers, filter chips at rest, window frame, scrollbars.

## Selection

- `selection.fill` (#3C4654) + `selection.text` (#FFFFFF), fully opaque, in every preset.
- Applies to: hierarchy rows, asset tiles, dropdown current value, mode-selector current mode,
  command-palette highlighted row.
- Pointer hover is always the *neutral* `hover` surface. Selected ≠ hovered — different colors,
  never combined into one.
- Multi-select: same fill on all rows; the last-clicked row adds a 1px `focus_ring` inset.
- Keyboard focus on a selected row: 1px `focus_ring` inset border, fill unchanged.
- Drag-and-drop target: 2px `accent_active` line between rows (insertion) or 1px accent border
  (drop-into). Never repaint the row fill during drag.

## State ladder (all interactive widgets)

| State    | Change                                                        |
|----------|---------------------------------------------------------------|
| default  | base fill + `stroke` border                                    |
| hover    | fill one step lighter + `stroke_strong` border                 |
| pressed  | fill one step darker                                           |
| focused  | border → `focus_ring`; nothing else changes                    |
| disabled | fill `header`, border `stroke` @ ~50%, text `text.disabled`; icons tint chrome-disabled |

One step = adjacent surface in the stack (panel → hover → active).

## Elevation

| Level | Surface        | Border          | Notes                                  |
|-------|----------------|-----------------|----------------------------------------|
| E0    | `window`       | —               | window bg, tab strips, status bar      |
| E1    | `panel`        | `stroke` 1px    | panel bodies                           |
| E2    | `header`       | `stroke` 1px    | section headers, toolbars-in-panels    |
| E3    | `elevated` × `popover_alpha` | `stroke_strong` 1px | menus, dropdowns, tooltips, palette |
| E4    | `elevated` opaque | `stroke_strong` 1px + scrim 45% | modals — always opaque      |

Translucency is allowed ONLY at E3, is simple alpha (no blur), and is globally tunable /
disableable via `popover_alpha`.

## Status vs danger

- `status.error` colors *information*: log tags, count chips, failed progress, toast bars.
- `status.danger` colors *actions*: Remove/Delete buttons. Filled danger buttons live in confirm
  dialogs; inline destructive buttons (e.g. Remove Component) use the quiet danger-outline style.
- Count chips are filters: active = status tint fill (13%) + status border + dot; muted =
  neutral outline + gray dot. Counts stay visible either way.

## Axis fields

X/Y/Z numeric fields carry a 3px axis-colored inset edge + tinted axis letter. The axis color
never moves to the border (that would collide with focus/hover), never fills the field.
R-reset buttons are standard buttons, one per row.

## Asset tiles

- Thumbnail area never tints — selection wraps it (`selection.fill` card + `focus_ring` border).
- 2px bottom type edge in the asset's `type_colors` entry + small mono type label.
- One tooltip, single instance, structured: name / type · size / mono path. Tooltips never stack.

## Asset reference fields

One layout for every asset-typed property (mesh, material slot, animation clip, audio, script):

- **Thumb** — 44px, `window` fill + 1px `stroke` frame, 2px bottom edge in the asset's
  `type_colors` entry. Live preview render when available, typed icon otherwise. Never tints.
- **Name dropdown** — the picker trigger. Value in text-primary; "None" in secondary;
  missing reference in `status.error` + error dot on the thumb (field chrome stays neutral).
- **Utility strip** — 18px standard buttons: eyedropper = use asset selected in browser ·
  folder = locate in browser · R = reset to None.
- **Slot chip** — read-only mono chip (`header` fill) when the mesh names the slot; same spot
  carries clip/state/bus names for animation & audio.
- Multi-slot lists: section sub-header with mono count + add-circle; elements mono-indexed.

**Picker popover** is E3 (`popover_alpha`, `stroke_strong` border): search auto-focused,
list pre-filtered to the property's type (locked type chip), current value = `selection.fill`
row, hover neutral. Current-asset actions (New / Edit / Copy / Paste / Clear) grouped below
the list; footer = mono count + key hints.

## Edit menu & settings windows

Edit menu: History (undo/redo label the verb + object, Undo History…) · Edit (clipboard) ·
Configuration (Editor Preferences… ⌘, / Keyboard Shortcuts… / Project Settings… ⇧⌘, / Plugins…).
No in-menu search — the command palette (status-bar `>` field) is the searchable command surface.
Unavailable items disable, never hide.

One settings-window shell, two documents, told apart only by the title-bar scope chip:
- **Editor Preferences** — user-local `editor_prefs.ron`; applies live, autosaves (no OK/Apply).
- **Project Settings** — checked-in `project.ron`; dirties the project, saves with ⌘S.

Anatomy: 36px title bar (title + scope chip + search ⌘F) · 158px flat sidebar (categories,
current = `selection.fill`) · content = 26px E2 section bars + 28px rows (170px label column
in text-secondary, control left-aligned) · footer bar (mono file path + save state).

- Modified-from-default: `status.overridden` dot in the label gutter, label promoted to
  text-primary, mono "default …" hint, standard R reset at row end; dot accumulates on the
  sidebar category.
- Restart-required rows carry a mono warning-tint RESTART chip; footer offers "Restart now"
  while any is modified.
- Search filters rows across all categories; non-matching sidebar entries dim.
- Reset All… = quiet danger-outline in the footer; filled danger stays in the confirm dialog.
- Asset-typed settings (default scenes, …) reuse the asset reference field unchanged.
- Help text: 10px caption under the label, only where genuinely needed.

## Typography

IBM Plex Sans (UI) 18 / 14 / 12 / 10.5 · JetBrains Mono 12 for values, paths, shortcuts, logs,
counts. Mono everywhere a number can change width — it stops layout jitter.

## Iconography

White-source icons tinted at draw time (existing pipeline): chrome states
default #C0C4CC / hover #E6E8EC / active #FFFFFF / disabled #60646C / accent = `accent_active`.
Typed icons + tile edges share `type_colors`. The logo renders in chrome as a flat single-tint
glyph; full-color crab only on splash/about.

## Do / Don't

- DO put every editable field on `input` (darkest) — the inset look needs no shadow.
- DO use mono for any value a user might compare or scrub.
- DON'T introduce a second accent hue inside one preset.
- DON'T use `selection.fill` for anything that isn't a current selection/value.
- DON'T round anything beyond `radius_panel` (6). No pills except count chips (h/2).
- DON'T add borders thicker than 1px — 2px is reserved for the active-tab edge and tile type edge.
- DON'T let transient UI (tooltips/menus) own state: they render from tokens and die on dismiss.
