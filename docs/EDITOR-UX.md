# Editor UX reference

Which existing editor each surface of this editor follows, and the one
pattern to copy from it. Unreal is the default reference. Godot is the
reference for scene composition and in-editor documentation, and Unity for
the game UI designer and the command palette.

Read this before designing or changing an editor surface. Visual style
(tokens, states, spacing) is a separate matter: see
`crates/crusty-gui/docs/DESIGN.md` and the design lint
(`scripts/lint_design.sh`).

## Reference per surface

| Surface | Reference | Pattern to copy |
|---|---|---|
| Docking and layout | Unreal | A saved layout per asset-editor type |
| Details panel (Inspector) | Unreal | Search box, collapsible categories, a reset-to-default arrow that also marks overridden values |
| Content browser | Unreal | Sources tree on the left, tiles with an asset-type colour strip, type-filter chips |
| Outliner (Hierarchy) | Unreal | Filterable tree with folders, visibility and lock columns, prefab-instance badges |
| Viewport and gizmos | Unreal | W / E / R modes, world/local toggle, a snapping bar inside the viewport, right-mouse fly |
| Play controls | Unreal | A play-mode dropdown, plus Eject to detach from the player while the game runs |
| Prefab and scene composition | Godot | An instanced scene opens in its own tab; editable children; revert per property |
| Data assets and tables | Unreal | A typed row struct as the schema, a row grid above the selected row's details, CSV import and export |
| Landscape tools | Unreal | Sculpt and Paint modes, brush size / falloff / strength, weight layers |
| Game UI designer | Unity UI Builder | Hierarchy, a WYSIWYG canvas, a style inspector, anchors |
| Graph editors | Unreal Blueprint | Dragging off a pin opens a context palette with search |
| Command palette and search | Unity Search | One fuzzy palette across commands, assets and entities; every action shows its shortcut |
| In-editor documentation | Godot | Tooltips and F1 pages generated from the doc comments of the component schema |

## How to apply it

- **Copy the pattern, not the pixels.** The table names an interaction. The
  look comes from this editor's own design system.
- **How closely to match** is set in `CLAUDE.md` ▸ Following Visual
  References; it applies to the reference's behaviour as well as its look.
  When you depart from the reference on purpose, say why in the PR.
- **One reference per surface.** If a surface needs something its reference
  lacks, take the nearest equivalent from the same engine before mixing in
  another.
- **Every mutation is undoable** and labelled with a verb and an object, in
  Edit ▸ Undo. A surface that writes the world directly is not finished.
- **Every interactive widget has a stable id** (roadmap task M11.4), so the
  verification harness can drive the surface from a script.
- **Colours and metrics come from theme tokens**, checked by
  `bash scripts/lint_design.sh`.
- **Verify with the harness:** a replay script that exercises the surface and
  a golden image of it, once the harness exists. Until then, a screenshot
  reviewed as `CLAUDE.md` ▸ Visual Changes describes.

## Where each surface stands (2026-11)

Only what was checked in the code is listed. Check the current state again
when you start work on a surface.

| Surface | Verified today | Roadmap |
|---|---|---|
| Docking and layout | Per-document layout profiles exist | none |
| Details panel | Has a filter box; sections are hand-coded and edits bypass undo | M18.2 |
| Content browser | Folder tree, thumbnails and a type filter exist | M18 |
| Outliner | Not checked | M18 |
| Viewport and gizmos | Not checked | M18 |
| Play controls | Play, Play As Client and Listen Server exist | M18 |
| Prefab composition | Basic prefabs; an instance keeps no link to its source | M18 |
| Data assets and tables | Missing | M18 |
| Landscape tools | Missing | M18, after terrain in M16 |
| Game UI designer | Missing; the HUD is written in code | M19 |
| Graph editors | Dragging off a pin opens the palette | Polish is parked (old Task 45.5) |
| Command palette | A command palette exists; its coverage of assets and entities was not checked | M18 |
| In-editor documentation | Missing | M18, from the schema |

To add a panel, follow the `add-editor-panel` skill.
