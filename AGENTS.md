# wee-events.rs — Agent Instructions

## Workflow First

This repository uses the Superpowers workflow. Agents should explicitly use the relevant skills before acting.

Key skills for this repo:

- `using-superpowers` for every new conversation.
- `brainstorming` before designing or changing APIs, architecture, or behavior.
- `writing-plans` after design approval and before implementation on multi-step work.
- `test-driven-development` for feature and bugfix implementation.
- `systematic-debugging` when investigating failures or unexpected behavior.
- `verification-before-completion` before claiming work is done.
- `requesting-code-review` before handing off substantial implementation work.
- `receiving-code-review` before applying review feedback.

If a `jj` or `jujutsu` skill is available in the current harness, use it for repository workflow guidance. If not, use the `jj` CLI directly and follow the rules below.

## Version Control — jj First

This repository is managed with `jj`. Prefer `jj` for status, history, diff, and commit operations.

Common commands:

```bash
jj status
jj diff --git
jj log -n 10
jj show
jj workspace list
```

Rules:

- Prefer `jj` over `git` for normal repository workflow.
- Use `git` only for compatibility gaps where `jj` is not the right tool.
- Avoid interactive commands that open editors or pagers.
- For isolated parallel work, prefer `jj workspace add` over git worktrees.
- Check the working copy state before making changes and again before finishing.

## Build And Test

Tooling is managed through `mise`. Prefer `mise exec --` for Rust commands.

```bash
mise exec -- just fmt
mise exec -- just check
mise exec -- cargo test --workspace --all-features
```

Before finishing substantial work, run the relevant verification commands and report what was actually executed.

## Project Notes

- Keep core interfaces transport-agnostic. Transport concerns like JSON or Restate-specific wiring belong in adapters, not in the core service abstractions.
- Follow existing crate boundaries unless the task explicitly requires restructuring them.
- Prefer focused changes over broad refactors.
