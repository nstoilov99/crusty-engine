# AI review pipeline

Every pull request is reviewed by three models, a fixer agent triages their
comments, and a human merges from [Pyor](https://pyor.review) or GitHub.

| Role | Who | Where configured |
|---|---|---|
| Correctness review | Claude Fable 5.1 | `.github/workflows/ai-review.yml` |
| Design review | Claude Opus 5.5 | `.github/workflows/ai-review.yml` |
| General review | GPT (Codex code review) | ChatGPT > Codex settings; rules in `AGENTS.md` |
| Fixer | Claude Fable 5.1 | `.github/workflows/ai-fix.yml` |
| Human gate | you, in Pyor | branch ruleset on `main` |

## One-time setup (needs your browser)

1. **Install the Claude GitHub app** on this repository:
   https://github.com/apps/claude. Or run `/install-github-app` inside Claude Code.
2. **Create the subscription token** on your PC, in a terminal:
   `claude setup-token`. Copy the token it prints.
3. **Add the secret**: repository Settings > Secrets and variables > Actions >
   New repository secret, name `CLAUDE_CODE_OAUTH_TOKEN`, value = the token.
4. **Enable Codex review**: ChatGPT > Settings > Codex > connect GitHub, enable
   this repository, turn on Code review and Automatic reviews.
5. **Codex model and rules**: the review model is chosen by OpenAI and is not
   configurable per repo; what you control is the `## Code Review Rules`
   section of `AGENTS.md`, which Codex reads for every review. Codex posts as
   `chatgpt-codex-connector`; `ai-fix.yml` lists `chatgpt-codex-connector[bot]`.
   If the author shown on its first review differs, update `allowed_bots`.
6. **Install Pyor** (https://pyor.review/download) and connect this repository.
7. **Protect main**: Settings > Rules > Rulesets > New branch ruleset, target
   `main`, enable "Require a pull request before merging" (1 approval) and
   "Require status checks to pass" (Windows, Linux, Design lint, wasm32).

## How a PR flows

1. PR opened or pushed: Fable and Opus review within minutes; Codex reviews on
   its own schedule. Each reviewer prefixes comments with its name.
2. Each bot review triggers the fixer once. It groups duplicate points, fixes
   what is correct, pushes, replies to everything, and labels the PR:
   `bots-agree`, `auto-fixed`, `needs-human`.
3. The push re-triggers the reviewers. After two fixer rounds the PR is labeled
   `needs-human` and the loop stops.
4. In Pyor, sort by label: `needs-human` first, then `bots-agree` with open
   threads, then `auto-fixed` as a skim. Approve and merge there.

## Opt-outs and knobs

- Label a PR `no-ai-review` to skip the Claude reviewers, `no-ai-fix` to skip
  the fixer. Draft PRs are skipped automatically.
- `AI_FIX_MAX_ROUNDS` in `ai-fix.yml` caps fixer rounds (default 2).
- Reviewer briefs and models live in the matrix at the top of `ai-review.yml`.
- `ai-fix.yml` only receives review events once it is on the default branch.
