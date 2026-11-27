# Issue tracker: GitHub

Issues, specs and PRs for this repo live on GitHub (`nstoilov99/crusty-engine`,
inferred from `git remote -v`). Use the `gh` CLI, or the host's pull-request
tools when the harness provides them.

## Conventions

- **Create an issue**: `gh issue create --title "..." --body "..."` (heredoc for multi-line bodies).
- **Read an issue**: `gh issue view <number> --comments`.
- **List issues**: `gh issue list --state open --json number,title,labels` with `--label` / `--state` filters.
- **Comment**: `gh issue comment <number> --body "..."`.
- **Labels**: `gh issue edit <number> --add-label "..."` / `--remove-label "..."`.
- **Close**: `gh issue close <number> --comment "..."`.

GitHub shares one number space across issues and PRs: a bare `#42` may be
either — try `gh pr view 42`, fall back to `gh issue view 42`.

## Where specs actually live

Most work here is not ticketed. The spec for a task is its section in
`docs/roadmap/ROADMAP.md` (goal, scope, acceptance) plus the plan in the PR
description; the `/code-review` Spec axis should read those when no issue is
referenced.
Ticket-sized breakdowns written during a task go in `.scratch/` (gitignored).

## Pull requests as a triage surface

**PRs as a request surface: no.** All PRs are the owner's own task branches.

## When a skill says "publish to the issue tracker"

Create a GitHub issue.

## When a skill says "fetch the relevant ticket"

Run `gh issue view <number> --comments`; if the work has no issue, read the
task's plan document instead.
