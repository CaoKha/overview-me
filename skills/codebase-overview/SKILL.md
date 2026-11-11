---
name: codebase-overview
description: Write a map of the whole codebase at the default branch (main), covering what the system is, its modules and lanes, entry points, core concepts, main flows, conventions and where to find things. /change-map and /feature-overview use it so a reviewer can place a PR in the system. Use when the user asks for a "codebase overview", an "architecture overview", to "map the repo", or "explain this codebase".
---

# Codebase overview

A change map shows one PR. Without the rest of the system around it, the reader gets lost: they do not know what a module is for, what a term means, or what normally calls the changed code. The codebase overview is that context. It is written once per repo, kept up to date as main moves, and shared by every change map in the repo.

`${CLAUDE_SKILL_DIR}/../..` is the change-map checkout. Binary: `${CLAUDE_SKILL_DIR}/../../target/release/change-map` (called `change-map` below). If it is missing, run `cargo build --release` in `${CLAUDE_SKILL_DIR}/../..` once. Run it from the root of the repo under review.

The overview lives at `<store>/codebase.md`, where `<store>` is `$(dirname "$(change-map path)")`. Like the maps, it is never committed and is shared by all worktrees.

## Steps

1. **Pick the ref.** Use the default branch, not the checked-out branch: the reader may be on a PR branch.
   - `git fetch origin` (skip it if offline), then `git symbolic-ref --short refs/remotes/origin/HEAD`, which gives e.g. `origin/main`. If that fails, use `origin/main`, then `main`, then `master`. Ask if none exist.
   - `REF=<that ref>`, `SHA=$(git rev-parse $REF)`.
   - Read everything at `$REF` without checking it out: `git ls-tree -r --name-only $REF`, `git show $REF:<path>`, `git grep -n <pattern> $REF`.
2. **Decide: new or update.**
   - If `<store>/codebase.md` exists, read the `sha` on its second line.
   - Same sha: tell the user it is current and stop, unless they asked to rebuild.
   - Older sha that is an ancestor of `$SHA` (`git merge-base --is-ancestor <old> $SHA`): run `git diff --stat <old> $SHA` and update only the sections those files touch, then bump the sha. If more than about a third of the modules changed, rebuild.
   - Otherwise build it new.
3. **Survey** (new build). Breadth first. Do not read every file.
   - The README, docs index, and contributor or agent notes (`CLAUDE.md`, `AGENTS.md`, `CONTRIBUTING.md`, `docs/`).
   - The manifests and workspace files (`package.json`, `pnpm-workspace.yaml`, `Cargo.toml`, `go.mod`, `pyproject.toml`, …) for the module list, the scripts and the main dependencies.
   - The directory tree to depth 2 or 3 (`git ls-tree -d`), and file counts per top-level folder to see where the weight is.
   - Lanes: read `${CLAUDE_SKILL_DIR}/../../rules.json`. It maps paths to the lanes the change map uses (ui, api, services, db, engine, jobs, other). Put each module in a lane by those globs, so the overview and every change map speak the same words. If the globs do not match this repo, say so under Gaps.
   - For a repo with more than about 6 modules, you may survey modules in parallel with subagents, one per module, each returning: purpose, entry points, key types, what it imports from other modules. Keep the writing to yourself.
4. **Dig** into what a reviewer needs, per module:
   - **Entry points**: routes, CLI commands, job schedules, queue consumers, UI roots, exported public APIs. Find where they are registered.
   - **Core concepts**: the 5–15 domain types, tables or messages the system is built around, where each is defined, and one line on what it means.
   - **Flows**: the 2–4 most common paths through the system, e.g. "a user request → api → service → db", with the real function at each hop.
   - **Dependencies between modules**: who imports whom (`git grep` on import paths).
   - **Conventions**: where tests live and how to run them, config and env, error handling, naming, generated code that should not be edited.
5. **Write** `<store>/codebase.md` in the format below.
6. **Hand off.** Give the path and the one-paragraph summary. Tell the user that in nvim `<leader>rc` (`:ChangeMapCodebase`) opens it in the browser with diagrams and links to the code, `:ChangeMapCodebase!` opens it in a split, and that `/change-map` and `/feature-overview` now use it.

## Format

```markdown
# <Repo name> · codebase overview
<REF> · sha <SHA full> · <date> · <N> files

## In one paragraph
What the system does, for whom, and its shape (monorepo of X apps, a CLI, a service and a worker, …).

## Run it
| Task | Command |
| --- | --- |
| install / dev / test / lint / build | `...` |

## Modules
| Module | Path | Lane | What it does | Depends on |
| --- | --- | --- | --- | --- |

## Architecture
<mermaid flowchart, see rules>

## Entry points
- <kind: route, command, job, UI root> `<name>` → `path:line` (<module>)

## Core concepts
| Term | What it means | Defined at |
| --- | --- | --- |

## Main flows
### <Flow name>
1. <hop> `path:line` (<module>)

## Conventions
- Tests: ...
- Config: ...
- Do not edit: <generated paths>

## Where to find
| If you need… | Look in |
| --- | --- |
| a new API route | `path/` |

## Gaps
- <what you could not confirm, docs that disagree with code, lanes that do not fit>
```

## Rules

- Use simple technical English. No marketing words.
- Every claim about code cites a repo-relative `path:line` at `$SHA`, so `gF` in nvim can open it. Lines may drift on other branches; that is fine.
- Only state what the code or docs show. Put guesses under Gaps.
- **Architecture diagram**: `flowchart LR`, one subgraph per lane in lane order (ui, api, services, db, engine, jobs, other), one box per module, arrows for imports or runtime calls between modules. At most 20 boxes; group small modules. Quote labels and use no `#` in them.
- Name modules the same way everywhere: the Modules table is the vocabulary that change maps and feature overviews reuse.
- Aim for under about 250 lines. A reader should get the shape in ten minutes. Depth belongs in the code; the overview says where to look.
- Do not write the overview into the repo, and do not edit `rules.json`. If the lanes do not fit, suggest a `--rules` file under Gaps.
