---
name: roadmap-task
description: Plan, implement and close out a numbered roadmap task (Task 45, 54-A, …) in this engine — write the VULKANO plan doc, work in packages on a task branch, and update every status file at close-out. Use when asked to start, plan, continue or close out a task.
---

A roadmap task runs plan → packages → close-out. Each stage ends on a
checkable state; the close-out touches a fixed set of files.

## 1. Orient

1. Read `docs/roadmap/ROADMAP.md` ▸ the task's section and its **Now (2026-11)**
   note, then the deferred ledgers it names in `docs/roadmap/COMPLETED.md`.
2. Read the subsystem sections of `docs/ARCHITECTURE.md` and the matching
   *Gotchas* section of `docs/KNOWLEDGE.md`; skim `CONTEXT.md` for the terms.
3. Verify every claim you will build on against the code (`grep` the type or
   function). Plan docs record what was true at writing time.

Done when you can name the files you will change and the tests that cover them.

## 2. Plan document

Write `docs/roadmap/VULKANO-<N>-<SLUG>.md` with these sections (the shape of
`VULKANO-41.7-ANIM-PIPELINE-ROOT.md`):

1. **Goal** and **Non-goals** (the non-goals become the deferred ledger).
2. **What exists** — verified file/type names, with dates.
3. **Design** — numbered decisions `D1…` with the alternative rejected and why.
4. **Work packages** `P1…` — one reviewable commit each; each compiles and
   passes alone; name the tests each adds.
5. **Acceptance** — a table of checkable items, including the live check the
   user will do.
6. **Decisions for the user `[USER]`** — only choices that change the work
   materially. Stop here and ask; do not start packages on open `[USER]` items.

Ask Codex for a read-only challenge round of the plan (see *Cost routing* in
`CLAUDE.md`); fold the findings into v2 before coding.

## 3. Packages

- Branch `task-<N>-<slug>` from `main`. One commit per package, subject in
  conventional-commit form (`feat(anim): …`, `fix(editor): …`), no trailers.
- Per package: tests first where a seam exists (`/tdd`), then
  `pwsh scripts/ci_local.ps1 -Quick` (or `.sh --quick`) before the commit.
- Keep tickets, review transcripts and recordings in `.scratch/<task>/`
  (gitignored). The durable record is the plan doc + close-out, not `.scratch`.
- Visual or behavioural changes: run the app (`run-app` skill) and record what
  was verified live versus test-proven.

Done when every package is committed and the full gate
(`scripts/ci_local.ps1` without `-Quick`) is green.

## 4. Close-out — every file, every time

| File | Change |
|---|---|
| `docs/roadmap/ROADMAP.md` | Status board row → ✅ with PR number; remove the task section; update **Current focus**; update **Open follow-ups** if the task closed or created any |
| `docs/roadmap/COMPLETED.md` | Append the task section: status line with date, decisions, acceptance table (live-verified vs test-proven vs **not run**), review notes, **Deferred ledger** |
| `docs/ARCHITECTURE.md` | New or changed subsystem section |
| `docs/KNOWLEDGE.md` | `## <Area> Gotchas (Task <N>)` — things the next agent will otherwise rediscover |
| `CONTEXT.md` | New canonical terms |
| `CLAUDE.md` ▸ Current Development Focus | One line moved/added; the `Next:` line |
| Plan doc | Status header updated; `[USER]` decisions recorded as decided |

Then `/code-review` against `main`, open the PR (`task-<N>-<slug>` → `main`),
and report: what was verified live, what is test-proven only, and what was
deferred.
