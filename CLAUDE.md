# Project Instructions for AI Agents

This file provides instructions and context for AI coding agents working on this project.

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:6cd5cc61 -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md for details and anti-patterns.

## Agent Context Profiles

The managed Beads block is task-tracking guidance, not permission to override repository, user, or orchestrator instructions.

- **Conservative (default)**: Use `bd` for task tracking. Do not run git commits, git pushes, or Dolt remote sync unless explicitly asked. At handoff, report changed files, validation, and suggested next commands.
- **Minimal**: Keep tool instruction files as pointers to `bd prime`; use the same conservative git policy unless active instructions say otherwise.
- **Team-maintainer**: Only when the repository explicitly opts in, agents may close beads, run quality gates, commit, and push as part of session close. A current "do not commit" or "do not push" instruction still wins.

## Session Completion

This protocol applies when ending a Beads implementation workflow. It is subordinate to explicit user, repository, and orchestrator instructions.

1. **File issues for remaining work** - Create beads for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **Handle git/sync by active profile**:
   ```bash
   # Conservative/minimal/default: report status and proposed commands; wait for approval.
   git status

   # Team-maintainer opt-in only, unless current instructions forbid it:
   git pull --rebase
   git push
   git status
   ```
5. **Hand off** - Summarize changes, validation, issue status, and any blocked sync/commit/push step

**Critical rules:**
- Explicit user or orchestrator instructions override this Beads block.
- Do not commit or push without clear authority from the active profile or the current user request.
- If a required sync or push is blocked, stop and report the exact command and error.
<!-- END BEADS INTEGRATION -->


## Build & Test

```bash
cargo test
cargo build --release
mkdir -p bin && cp -f target/release/herdr-workspace bin/
herdr plugin link "$PWD"
```

Bind `herdr-workspace.open` and `herdr-workspace.worktree` in `~/.config/herdr/config.toml` (`type = "plugin_action"`), then `herdr server reload-config`.

## Architecture Overview

- `open` action (no TTY) opens the centered `form` popup pane.
- `worktree` action (no TTY) opens the centered `worktree` popup pane on the current git workspace.
- `form` is a ratatui wizard: directory (live completions) → name → profile cards → cancel/save.
- `worktree` form: branch → base ref (fuzzy git refs) → profile cards → cancel/save.
- Profiles live in `$HERDR_PLUGIN_CONFIG_DIR/config.yaml` (seeded from `config.example.yaml`). Schema and how to write them: `docs/profiles.md`.
- Save: `herdr workspace create` (or `herdr worktree create`), then tab/pane split/rename, `pane run` or `agent start` from the selected profile. Worktree Save also fast-forwards the new checkout onto the latest base.

## Conventions & Patterns

- Call Herdr through `$HERDR_BIN_PATH` (`src/herdr.rs`).
- Use `bd` for task tracking. Conventional commits; changelog via `scripts/changelog.sh` (git-cliff).
- Releases: push a `v*` tag; GitHub Actions uploads musl/macOS binaries; `herdr/install.sh` downloads them.
