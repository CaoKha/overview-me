---
name: codebase-overview
description: Write a map of the whole codebase at the default branch (main), covering what the system is, its modules and lanes, entry points, core concepts, main flows, conventions and where to find things. /pr-overview uses it so a reviewer can place a PR in the system. Use when the user asks for a "codebase overview", an "architecture overview", to "map the repo", or "explain this codebase".
---

# Codebase overview

A change map shows one PR. Without the rest of the system around it, the reader gets lost: they do not know what a module is for, what a term means, or what normally calls the changed code. The codebase overview is that context. It is written once per repo, kept up to date as main moves, and shared by every change map in the repo.

`${CLAUDE_SKILL_DIR}/../..` is the overview-me plugin root. The steps run its CLI, `${CLAUDE_SKILL_DIR}/../../bin/overview-me` (called `overview-me` below), which the plugin installs when a session starts. If it is missing, run `bash "${CLAUDE_SKILL_DIR}/../../scripts/ensure-cli.sh"` once: it downloads the release for this machine or builds it from source. If it is still missing, tell the user what the script printed and stop. Run the CLI from the root of the repo under review.

The overview lives at `<store>/codebase.md`, where `<store>` is `$(dirname "$(overview-me path)")`. Like the maps, it is never committed and is shared by all worktrees.

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
6. **Show the page.** Run `overview-me page <store>/codebase.md`. It writes `<store>/codebase.html`: a page with the look of a Claude artifact (cards, chips, light and dark themes), the architecture diagram as flow diagrams where hovering a box traces its links and clicking it lists its files with copy buttons, and every `path:line` linked to the forge. If it exits 1, it lists what is wrong in the `flow` blocks: fix the markdown and run it again. Then check whether you run in Claude Code: you do if you have the Artifact tool.
   - **Claude Code:** publish `<store>/codebase.html` with the Artifact tool, icon `map`, and the summary's first sentence as `description`. Publish the file as it is: it already meets the artifact page rules (inline styles, light and dark tokens, Google Fonts, one pinned CDN script), so do not restyle or rewrite it. If you published it earlier in this session, publish the same path again so the link stays the same.
   - **Not Claude Code:** open `<store>/codebase.html` in the browser (`open` on macOS, `xdg-open` on Linux). It is the same page as the artifact. If the command is blocked or fails, give the user the exact command to run.
7. **Hand off.** Give the artifact link (or the HTML path), the markdown path and the one-paragraph summary. Tell the user that clicking a module in the architecture diagram lists its files to copy into their editor, and that `/pr-overview` now uses it.

## Format

````markdown
# <Repo name> · codebase overview
<REF> · sha <SHA full> · <date> · <N> files

## In one paragraph
What the system does, for whom, and its shape (monorepo of X apps, a CLI, a service and a worker, …).

## Run it
| Task | Command |
| --- | --- |
| <task: install, dev, test, lint, build> | `<command>` |

## Modules
| Module | Path | Lane | What it does | Depends on |
| --- | --- | --- | --- | --- |

## Architecture
```flow
<modules and how they call each other, see rules>
```

## Entry points
- <kind: route, command, job, UI root> `<name>` → `<path:line>` (<module>)

## Core concepts
| Term | What it means | Defined at |
| --- | --- | --- |

## Main flows
### <Flow name>
1. <hop> `<path:line>` (<module>)

## Conventions
- Tests: <where tests live, how they are named and run>
- Config: <where config lives and how it is loaded>
- Do not edit: <generated paths>

## Where to find
| If you need… | Look in |
| --- | --- |
| <a common task, e.g. adding an API route> | `<path/>` |

## Gaps
- <what you could not confirm, docs that disagree with code, lanes that do not fit>
````

## Rules

- Use simple technical English. No marketing words.
- Every claim about code cites a repo-relative `path:line` at `$SHA`, so a reader can find it in their editor or on the forge. Lines may drift on other branches; that is fine.
- Only state what the code or docs show. Put guesses under Gaps.
- **Architecture diagram**: a ```` ```flow ```` block of JSON, drawn on the page as lanes stacked top to bottom with one box per module and curved arrows; hovering or tabbing to a box highlights its arrows and the modules at their other ends.
  ```json
  {
    "nodes": {
      "<KEY A>": { "label": "<module>", "note": "<one-line summary>", "paths": ["<module folder/>", "<entry path:line>"] },
      "<KEY B>": { "label": "<module>", "paths": ["<module folder/>", "<entry path:line>"] },
      "<KEY C>": { "label": "<database, cache or queue>", "kind": "store", "paths": ["<schema folder/>"] }
    },
    "tiers": [
      [{ "name": "<lane>", "ids": ["<KEY A>"] }],
      [{ "name": "<lane>", "ids": ["<KEY B>"] }],
      [{ "name": "<lane>", "ids": ["<KEY C>"] }]
    ],
    "edges": [["<KEY A>", "<KEY B>", "<transport or call>"], ["<KEY B>", "<KEY C>", "<transport or call>"]]
  }
  ```
  - `nodes`: one per module, keyed by a short id. `label` is the module name from the Modules table, at most about 25 characters. `kind: "store"` for databases, caches and queues (rounded box). Use no `n` and no other kinds here: those mark changed code in PR overviews.
  - `paths` on every box: the module's folder, then its entry points as `path:line`. Clicking the box lists them with copy buttons for the reader's editor. `note` is an optional one-line summary shown with them.
  - `tiers`: rows, top to bottom in lane order (ui, api, services, db, engine, jobs, other), each a list of lanes `{ "name", "ids", "note"? }`. Every module sits in exactly one lane. At most 4 boxes per lane and 20 in all: group small modules.
  - `edges`: `[from, to, label?, style?]` for imports or runtime calls between modules. The label names the transport or the call in a few words ("<RPC or HTTP>", "<queue>", "<SQL>"). `style: "dashed"` for async or optional links.
  - The render step checks the block and lists what is wrong.
- Name modules the same way everywhere: the Modules table is the vocabulary that PR overviews reuse.
- Aim for under about 250 lines. A reader should get the shape in ten minutes. Depth belongs in the code; the overview says where to look.
- Do not write the overview into the repo, and do not edit `rules.json`. If the lanes do not fit, say which modules fall to `other` and why under Gaps. Do not suggest a `--rules` file: no skill passes one.
