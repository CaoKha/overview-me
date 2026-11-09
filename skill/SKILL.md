---
name: change-map
description: Build a change map for a PR or diff, a spec-linked review guide that the change-map Neovim plugin opens with :ChangeMap. Use when the user asks for a "change map", to "map this PR" or "map this diff", or for a "review guide for the code you wrote".
---

# Change map

A change map is one JSON file that follows `/home/agent/dev/change-map/schema/change-map.schema.json`. Git gives the facts. You add the labels: criteria, the tests that prove them, captions, groups and the tour. The validator checks your labels against git.

Binary: `/home/agent/dev/change-map/target/release/change-map` (called `change-map` below). If it is missing, run `cargo build --release` in `/home/agent/dev/change-map` once. Run it from the repo root.

## Steps

1. **Find the base.**
   - For a PR, find its base branch with `gh pr view --json baseRefName`, then run `git merge-base origin/<base> HEAD`.
   - For a stacked PR, the base is the PR below it.
   - If the base is unclear, ask.
2. **Extract.** Run `change-map extract --base <sha> [--spec <spec file>]`. It prints the facts path, the map path (the same path that `change-map path` prints), and the draft nodes.
3. **Read.** Read the spec, the PR description (`gh pr view --json body`), and the facts file. In the facts file:
   - `units[].diff` holds the changes. Each unit is one changed hunk inside one symbol.
   - `testDeclarations` lists the tests in the changed test files.
   - Draft nodes with no sites and a `deletions` or `renames` entry stand for deleted or moved files.
4. **Fill.** Copy `draft` from the facts file to the map path, then edit it:
   - **criteria**: what this change claims to do, from the PR description and the spec parts it implements.
     - Use ids `REF-1`, `REF-2`, …
     - Add a `specRef` of `"<spec path>#<heading-slug>"` when a spec section covers the criterion.
   - **status**: `implemented`; `partial` (some of the logic lives outside this diff); or `missing` (the change claims it, but no code does it).
   - **tests**: per criterion, the tests that actually prove it, as `{ file, line, name }` taken from `testDeclarations` (or from an existing test file).
     - Read the test before you list it.
     - A test that only calls the code does not count.
     - If no test checks the claim, leave `tests` empty. The criterion then shows as untested, which is the point.
   - **groups**: merge nodes that make the same edit in several places into one node with several `sites`.
     - Fold import-only and type-only nodes into the sites of the function they serve.
     - Delete the merged nodes, and remap `edges`.
   - **sites**: list the main site (the function body) first. It gets the caption and the quickfix entry. Every added line must sit inside exactly one node's sites.
   - **renames**: every listed move (`{ from, to }`: a pure rename, or production code moved into a test path) goes in exactly one node's `renames`. Say in the caption why it moved. Treat a move into a test path as suspect.
   - **deletions**: every deleted non-test file goes in exactly one node's `deletions`. Move it into the node whose change explains it, or keep its own node with a caption that says why it was removed.
   - **captions**: one line per node, saying what changed and why.
   - **node.criteria**: the criteria the node implements. Leave it empty for a node that only touches comments.
   - **edges**: keep the `calls` edges. Add `implements` or `uses` edges only when they help the reader.
   - **tour**: every node id once, in reading order. Contract or shape first, then implementation, then callers.
   - Never set `touchedByTest` by hand. It is a computed hint, and the validator says what it must be.
5. **Validate.** Run `change-map validate "$(change-map path)"`. Fix every problem and run it again until it prints `ok`.
6. **Render.** Run `change-map render "$(change-map path)"`. It writes `<head>.md` next to the map.
7. **Hand off.**
   - Tell the user to open nvim in the repo and press `<leader>rr`, or run `:ChangeMap` and then `:ChangeMapPanel`.
   - Give the stop count and any `missing`, `partial` or untested criteria.

## Writing rules

- Use simple technical English. No marketing words.
- A caption is one line, under 120 characters, and says what changed and why. Do not start it with the symbol name.
- Aim for 8–15 tour stops on a PR of about 20 files. Group repeated edits to reach that.
- Never link a criterion to code that only touches comments.
- In `tests`, list only tests whose assertions prove the criterion. A test that merely calls the function does not count.
- Only claim what the diff shows. If the logic lives in another PR or file, use `partial` and name the place in the criterion text.
