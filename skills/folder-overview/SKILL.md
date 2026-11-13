---
name: folder-overview
description: Write a map of one folder or package at the default branch (main), covering its submodules and lanes, entry points, core concepts, main flows, conventions and dependencies across its boundary. Use when the user asks for a "folder overview", a "package overview", to "explain this folder", "explain this package", or "map this module".
argument-hint: <path/to/folder>
---

# Folder overview

The codebase overview for one folder or package inside the repo.

`${CLAUDE_SKILL_DIR}/../..` is the overview-me plugin root. The steps run its CLI, `${CLAUDE_SKILL_DIR}/../../bin/overview-me` (called `overview-me` below), which the plugin installs when a session starts. If it is missing, run `bash "${CLAUDE_SKILL_DIR}/../../scripts/ensure-cli.sh"` once: it downloads the release for this machine or builds it from source. If it is still missing, tell the user what the script printed and stop. Run the CLI from the root of the repo under review.

Read `${CLAUDE_SKILL_DIR}/../codebase-overview/SKILL.md` first, and follow it with the changes below. Its format and rules apply as written, except where changed here.

## Steps

1. **Pick the folder.** Use the path from the skill arguments: $ARGUMENTS.
   - Make it repo-relative, with no leading `./` and no trailing `/`. Call it `<folder>` below.
   - If no folder was given, list likely packages from workspace manifests and top-level folders with many files. Ask which one and wait.
   - Pick `$REF` and `$SHA` as in codebase-overview step 1. Check that `git cat-file -t "$REF:<folder>"` prints `tree`. If it does not, say the folder is not on the default branch and stop.
2. **Where it lives.** Use `<store>/folders/<slug>.md` instead of `<store>/codebase.md`. `<store>` is `$(dirname "$(overview-me path)")`. Replace each `/` in the folder with `--` for `<slug>`: `packages/api` becomes `packages--api`. Create `<store>/folders/` if needed. Never commit the overview.
3. **Decide: new or update.** This replaces codebase-overview step 2, using this folder's markdown file.
   - Read the old `sha` from its second line if it exists. Same sha: say it is current and stop, unless the user asked to rebuild.
   - If the old sha differs and `git diff --quiet <old> $SHA -- <folder>` exits 0, only bump the sha line, say it is current and stop, unless the user asked to rebuild.
   - If the old sha is an ancestor of `$SHA`, update from `git diff --stat <old> $SHA -- <folder>`. Rebuild if more than about a third of the folder's submodules changed.
   - Otherwise build it new, as in codebase-overview.
4. **Survey.** Scope codebase-overview step 3 to files under the folder: `git ls-tree -r --name-only $REF -- <folder>/`. Read its own README, manifest (`package.json`, `Cargo.toml`, `go.mod`, `pyproject.toml`, …) and local docs.
   - If `<store>/codebase.md` exists, read it. Name the folder and its neighbours with its Modules vocabulary, and take lanes from it. Otherwise use the lane rules from codebase-overview.
5. **Boundary.** After Survey, find what uses the folder and what it uses outside itself. Read code at `$REF` and cite `path:line` at `$SHA`, as in codebase-overview.
   - **Used by:** search the rest of the repo for imports of this folder: `git grep -n <package name or import path> $REF -- ':!<folder>/'`. Keep actual imports. Write one row per importing module, with one `path:line` each.
   - **Uses:** find imports inside the folder that point outside it. List other repo modules first, then the main external dependencies from its manifest.
6. **Dig and write.** Do codebase-overview steps 4 and 5 for this folder's submodules. Write `<store>/folders/<slug>.md`, with the format changes below.
7. **Page and hand off.** Run `overview-me page <store>/folders/<slug>.md`. It writes `<store>/folders/<slug>.html`. Publish and hand off as in codebase-overview steps 6 and 7, using these paths. If you published this folder's page earlier in the session, publish the same path again. Give the folder summary; omit the claim that `/pr-overview` uses it, since that skill reads `<store>/codebase.md`.

## Format changes

- Title: `# <folder> · folder overview`.
- Header line: `<REF> · sha <SHA full> · <folder>/ · <date> · <N> files`. Count only files under the folder.
- **Modules:** list the folder's own submodules: subfolders, or files for a flat folder. Paths stay relative to the repo root.
- Add `## Boundary` after Modules, before Architecture, with these two tables:

  ```markdown
  ## Boundary
  ### Used by
  | Module | Where | What for |
  | --- | --- | --- |

  ### Uses
  | Module or dependency | Where | What for |
  | --- | --- | --- |
  ```

- **Architecture:** put the folder's submodules in their lanes. Put outside modules from Boundary in one extra tier at the bottom, in a lane named `outside`. Use at most 4 boxes in that lane; group the rest. Follow the same flow-block rules as codebase-overview: no `n`, and `kind: "store"` is the only kind allowed.
- Aim for under about 200 lines.
