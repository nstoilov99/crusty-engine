---
name: roadmap-task
description: Claim, plan, implement and close out a roadmap task (M11.2, M13.1, …) or expand a milestone outline in this engine. There is no plan file per task — the plan goes in the PR description — and the close-out touches a fixed set of files. Use when asked to start, plan, continue or close out a task or milestone.
---

A roadmap task runs claim → plan → packages → close-out. Everything lives in
`docs/roadmap/ROADMAP.md`; do not create a file per task.

## 1. Orient and claim

1. Read `docs/roadmap/ROADMAP.md`: **How work runs**, then the task's
   section. Read the deferred ledgers it touches in
   `docs/roadmap/COMPLETED.md`.
2. Check the task is allowed to start: its **Depends on** is done, and its
   milestone may run beside whatever is already claimed (**What may run at
   the same time**).
3. Read the subsystem sections of `docs/ARCHITECTURE.md` and the matching
   *Gotchas* section of `docs/KNOWLEDGE.md`; skim `CONTEXT.md` for the terms.
4. Verify every claim you will build on against the code. The roadmap records
   what was true when it was written.
5. Claim the milestone: write your branch in the status board's **Claimed**
   column and name any hot-path PR you will need. This goes in your first PR.

**If the milestone is still an outline** (M14 and later), expand it first, in
`ROADMAP.md`, to the level of M11–M13: tasks with goal, scope, out of scope,
dependencies, acceptance and how to verify. Open that as a docs-only PR, run
the `council` skill on it, and merge it before any code PR. You may tighten a
gate; loosening or replacing one needs the owner's approval in that PR.

Done when you can name the files you will change and the tests that cover
them.

## 2. Plan

Write the plan in the PR description (open the PR as a draft early):

1. **Goal** and **Out of scope**, from the roadmap section.
2. **What exists** — file and type names you verified.
3. **Design** — numbered decisions, each with the alternative rejected and
   why.
4. **Packages** — one reviewable commit each; each compiles and passes
   alone; name the tests each adds.
5. **Acceptance checklist** — the roadmap's acceptance items, each to be
   ticked with its proof.
6. **Questions for the owner** — only choices that change the work
   materially. Stop and ask; do not start packages on open questions.

Ask for a read-only challenge of the plan (see *Cost routing* in `CLAUDE.md`)
and fold the findings in before coding.

## 3. Packages

- Branch `m<NN>-<task>-<slug>` from `main` (for example `m11-2-headless`).
  One commit per package, conventional-commit subject, no trailers.
- Stay inside your track's paths. A change to a hot path (`Cargo.lock`, the
  ECS API, `FramePacket`, `game_shared`, unowned paths) goes in its own small
  PR that merges first.
- Per package: tests first where a seam exists (`/tdd`), then
  `bash scripts/ci_local.sh --quick` (or `pwsh scripts/ci_local.ps1 -Quick`)
  before the commit.
- Visual, editor or server changes: run the harness for that kind of change
  (`cargo xtask verify`, `cargo xtask server-gate`, once they exist) and keep
  the output for the PR.
- Working notes and recordings go in `.scratch/<task>/` (gitignored).

Done when every package is committed and the full local gate
(`scripts/ci_local.sh` without `--quick`) is green.

## 4. Close-out — every file, every time

| File | Change |
|---|---|
| `docs/roadmap/ROADMAP.md` | Your milestone's status-board row and section only: mark the task done with the PR number and move its section out in this same PR (nothing is left for after the merge), update **Open follow-ups** if the task closed or created any |
| `docs/roadmap/COMPLETED.md` | Append the task section: status line with date, decisions, acceptance table (verified live / proven by tests / **not run**), review notes, **Deferred ledger** |
| `docs/ARCHITECTURE.md` | New or changed subsystem section |
| `docs/KNOWLEDGE.md` | `## <Area> Gotchas (<task id>)` — what the next agent would otherwise rediscover |
| `CONTEXT.md` | New canonical terms |
| `CLAUDE.md` ▸ Current Development Focus | The `Next:` line |

Then run `/code-review` against `main` and complete the PR evidence listed in
`ROADMAP.md` ▸ **Merge policy and PR evidence**. Who merges is decided by the
`Merge policy` line in `CLAUDE.md`: with `owner`, hand the PR over and stop;
with `agents`, merge once the evidence is complete. Never merge on red and
never use `--admin`.
