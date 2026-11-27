---
name: editor-ux
description: Design or change an editor surface the way this project has decided to (Unreal for most surfaces, Godot for scene composition and in-editor docs, Unity for the game UI designer and the command palette). Use before designing, building or reviewing any editor panel, tool, window or workflow.
---

The decisions are in `docs/EDITOR-UX.md`. Read it first: it names, for every
editor surface, the reference engine and the one pattern to copy, and where
that surface stands today.

## Steps

1. **Find the surface** in the table in `docs/EDITOR-UX.md`. If the work adds
   a surface that is not listed, add a row (reference and pattern) in the same
   PR and say so in the description.
2. **State the pattern** you are copying in the PR plan, in one sentence, and
   what you leave out of the reference on purpose.
3. **Build it** with the `add-editor-panel` skill for the mechanics (tab
   variant, persisted id, drawing function, menu entry, theme tokens).
4. **Check the three rules that apply to every surface:**
   - every mutation goes through `CommandHistory` with a verb and object
     label;
   - every interactive widget has a stable id;
   - colours and metrics come from theme tokens (`bash scripts/lint_design.sh`).
5. **Verify.** A replay script and a golden image through `cargo xtask verify`
   once the harness exists (roadmap M11). Before that, run the editor
   (`run-app` skill), capture a screenshot, and have it reviewed as
   `CLAUDE.md` ▸ Visual Changes describes.

Done when the surface behaves like its reference for the named pattern, the
three rules hold, and the PR shows the evidence.
