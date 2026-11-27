---
name: add-editor-panel
description: Add a dock panel to the crusty-gui editor — EditorTab variant, persisted tab id, layout profile placement, thin drawing fn, dispatch in CrustyGui::layout, View ▸ Panels entry, theme tokens + design lint. Use when adding or re-homing an editor panel.
---

What the panel should do and which editor it follows is decided in
`docs/EDITOR-UX.md` (`editor-ux` skill); this skill covers the mechanics.

Panels are thin drawing layers over state structs; logic stays in the state
module and systems. Exemplar: Anim Preview — `dock_layout.rs`,
`dock_crusty.rs`, `menu_bar_crusty.rs`, `anim_preview.rs`,
`anim_graph_preview.rs`, `anim_preview_crusty.rs`, `editor/mod.rs`,
`game_client/src/app.rs`. All editor files are under
`engine/src/engine/editor/`.

## Steps

1. **Tab variant.** Add to `EditorTab` in `dock_layout.rs`; extend
   `title_string`, `id_string`, `to_window_kind` (ordinary side panels return
   `None`).
2. **Persisted id.** `dock_crusty.rs`: `tab_id` and `parse_tab` (stable
   string — layouts on disk reference it), and a slot in the right
   `LayoutProfile` branch of `default_tree`. Side panels are not documents:
   leave `is_document` / `profile_of` alone. Persistence to
   `editor_layout_crusty.ron` (format v2, active tree + inactive profiles) is
   automatic through `CrustyDockLayout`.
3. **State** in its own module (`<panel>.rs`): durable editor state, no
   drawing. **Drawing** in `<panel>_crusty.rs`: a function taking
   `&mut Ui`, the dock `Rect` and a small context struct (see
   `graph_variables_panel` in `graph_dock_panels_crusty.rs`), exported from
   `editor/mod.rs`. It adapts state into widgets and returns actions; it does
   not reach into the world.
4. **Dispatch** the variant in `CrustyGui::layout`'s `DockArea::show_in` tab
   match in `game_client/src/app.rs`; mirror it in the floating-window match
   if the panel may float. `engine/src/engine/gui/crusty.rs` only supplies
   the root `Ui`.
5. **Menu.** `menu_bar_crusty::menu_bar_panel` — View ▸ Panels entry using
   `is_tab_open` / `open_tab`.
6. **Theme.** Colours and metrics come from `editor/theme/tokens.rs` through
   `ui.style()`; no raw hex outside the token module and no 2px edges except
   the reserved active-tab/type edge — `bash scripts/lint_design.sh` enforces
   both and runs in CI.
7. **Undo.** Mutations the panel exposes go through `CommandHistory` commands
   with verb/object labels (`editor/commands.rs`); a panel that writes the
   world directly has no undo.

## Verify

```bash
bash scripts/lint_design.sh
cargo test -p rust_engine --features editor dock_crusty
cargo check -p game_client --features editor
```

Then run the editor (`run-app` skill): open, close, re-dock and float the
panel; switch document profiles; restart and confirm placement restores.
No logger is installed — use `println!` for temporary diagnostics and remove
them before committing. Do not commit the rewritten `editor_layout_crusty.ron`
unless the default layout changed on purpose.

Done when the lint and tests pass and the panel survives a restart in the
profile it was added to.
