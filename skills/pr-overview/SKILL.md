---
name: pr-overview
description: Review a PR or diff on one page, mapping the change and explaining the feature it builds. First a spec-linked change map (criteria and the tests that prove them, one captioned stop per changed place, a reading order), checked against git; then the overview, covering entry points, the flow before and after the change, data, where it plugs into existing code, and what to read first. Use when the user asks for a "PR overview", a "PR review", to "review this PR", a "feature overview", a "change map", to "map this PR" or "map this diff", an "overview of this PR", a "review guide for the code you wrote", the "structure of this feature", or to "explain this change".
---

# PR overview

Two parts. The change map says *what changed*: every changed line belongs to one captioned stop, each claim names the tests that prove it, and `overview-me validate` rejects any label that disagrees with git. The overview says *how the feature works*: how it is triggered, what runs, what data it touches, and which existing code it leans on. The reader should understand the feature in five minutes without opening the codebase, then use the overview to pick what to read.

`${CLAUDE_SKILL_DIR}/../..` is the overview-me plugin root. The steps run its CLI, `${CLAUDE_SKILL_DIR}/../../bin/overview-me` (called `overview-me` below), which the plugin installs when a session starts. If it is missing, run `bash "${CLAUDE_SKILL_DIR}/../../scripts/ensure-cli.sh"` once: it downloads the release for this machine or builds it from source. If it is still missing, tell the user what the script printed and stop. Run the CLI from the root of the repo under review.

## Steps

1. **Find the base.**
   - For a PR, find its base branch with `gh pr view --json baseRefName`, then run `git merge-base origin/<base> HEAD`.
   - For a stacked PR, the base is the PR below it.
   - If the base is unclear, ask.
2. **Read the codebase overview.** `<store>` is `$(dirname "$(overview-me path)")`. If `<store>/codebase.md` exists, read it: modules, lanes, core concepts and main flows. Use its module names and terms in the map and the overview, and place the feature in it. If it is missing, go on, and suggest `/codebase-overview` at hand-off.
3. **Build the change map.** The map is `$(overview-me path)`, one JSON file per head commit that follows `${CLAUDE_SKILL_DIR}/../../schema/change-map.schema.json`. Git gives the facts; you add the labels (criteria, the tests that prove them, captions, groups and the tour); the validator checks your labels against git.
   - **Reuse.** If the map exists, its `base` is the base from step 1, and `overview-me validate "$(overview-me path)"` prints `ok`, keep it and go to step 4, unless the user asked to rebuild it.
   - **Extract.** Run `overview-me extract --base <sha> [--spec <spec file>]`. It prints the facts path, the map path and the draft nodes.
   - **Read.** Read the spec, the PR description (`gh pr view --json body`), and the facts file. In the facts file:
     - `units[].diff` holds the changes. Each unit is one changed hunk inside one symbol.
     - `testDeclarations` lists the tests in the changed test files.
     - Draft nodes with no sites and a `deletions` or `renames` entry stand for deleted or moved files.
   - **Fill.** Copy `draft` from the facts file to the map path, then edit it:
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
     - **sites**: list the main site (the function body) first. It gets the caption, and the overview cites it as the node's main place. Every added line must sit inside exactly one node's sites.
     - **renames**: every listed move (`{ from, to }`: a pure rename, or production code moved into a test path) goes in exactly one node's `renames`. Say in the caption why it moved. Treat a move into a test path as suspect.
     - **deletions**: every deleted non-test file goes in exactly one node's `deletions`. Move it into the node whose change explains it, or keep its own node with a caption that says why it was removed.
     - **captions**: one line per node, saying what changed and why.
     - **node.criteria**: the criteria the node implements. Leave it empty for a node that only touches comments.
     - **edges**: keep the `calls` edges. Add `implements` or `uses` edges only when they help the reader.
     - **tour**: every node id once, in reading order. Contract or shape first, then implementation, then callers.
     - Never set `touchedByTest` by hand. It is a computed hint, and the validator says what it must be.
   - **Validate.** Run `overview-me validate "$(overview-me path)"`. Fix every problem and run it again until it prints `ok`. Do not write an overview from a map that does not validate.
4. **Read the map.** From the JSON:
   - `criteria`: what the feature claims to do, their status and tests.
   - `nodes`: lane, caption, risk and sites of every changed place.
   - `edges` and `tour`: how the changed places call each other, and the author's reading order.
   - `spec`: if set, read the spec sections that the criteria's `specRef` point to.
5. **Read the code, but only around the map.** Do not read the whole repo.
   - Read the main site (first site) of every `read` node, and the enclosing function of every site you cite.
   - **Entry points**: among the nodes, find the ones nothing in the map calls, mostly in `ui`, `api` and `jobs`. Find how each one is reached: the route, command, event, job schedule or UI action that triggers it. Search for its symbol to find the registration.
   - **Existing code**: for each changed symbol, find its callers and callees outside the diff, one hop only (`git grep -n <symbol>`). These are the unchanged pieces the feature depends on, and the existing callers whose behaviour may change.
   - **Data**: find the types, tables, migrations, config keys and messages that the feature adds or changes, and who writes and reads each one.
   - **Before**: read the same flow at the map's `base` commit, with `git show <base>:<path>` and `git grep -n <symbol> <base>`. Find how the entry points were reached, what the changed functions did, and which deleted code took part. This is the Before half of the Flow section. If the flow did not exist at base, there is no Before diagram.
6. **Write** `<head>.review.md` next to the map, in the format below.
7. **Show the page.** Run `overview-me page <store>/<head>.review.md`. It writes `<store>/<head>.review.html`: a page with the look of a Claude artifact (cards, chips, light and dark themes), the Before and After flows side by side on a wide screen, flow diagrams where hovering a box traces its links and clicking it lists its files with copy buttons, and every `path:line` linked to the forge. If it exits 1, it lists what is wrong in the `flow` blocks: fix the markdown and run it again. Then check whether you run in Claude Code: you do if you have the Artifact tool.
   - **Claude Code:** publish `<store>/<head>.review.html` with the Artifact tool, icon `map`, and the summary's first sentence as `description`. Publish the file as it is: it already meets the artifact page rules (inline styles, light and dark tokens, Google Fonts, one pinned CDN script), so do not restyle or rewrite it. If you published it earlier in this session, publish the same path again so the link stays the same.
   - **Not Claude Code:** open `<store>/<head>.review.html` in the browser (`open` on macOS, `xdg-open` on Linux). It is the same page as the artifact. If the command is blocked or fails, give the user the exact command to run.
8. **Hand off.** Give the artifact link (or the HTML path), the markdown and map paths, the stop count, the one-paragraph summary, and the gaps (missing, partial or untested criteria, and open questions). Tell the user that clicking a box in a flow diagram lists its files to copy into their editor.

## Format

````markdown
# <Feature name> · PR overview

<base 10>...<head 10> · <N> stops · <M> criteria · spec: <spec or "none">

## In one paragraph
What the feature does for its user, and the approach the code takes. 3–5 sentences.

## Where it sits
Modules touched: <module (lane)>, … from the codebase overview. Which of its main flows this feature joins or adds to. "No codebase overview" if there is none.

## Entry points
- <trigger: route, command, job, UI action> → `<symbol>` `<path:line>` (<node id>)

## Flow
### Before: <how it worked at base, in one line>
```flow
<the flow at base, see rules>
```

1. <step at base, only what the change touches> `<path:line>` (base)
2. ...

### After: <how it works now, in one line>
```flow
<the flow at head, see rules>
```

1. <step in runtime order> `<path:line>` (<node id>)
2. ...

## Data and state
| Thing | Kind | Written by | Read by |
| --- | --- | --- | --- |
| `<name>` | <kind, e.g. column, cache key, env var> (<new / changed / removed>) | <node id> `<path:line>` | <node id> `<path:line>` |

## Where it plugs in
- **Depends on (unchanged):** `<symbol>` `<path:line>`: <what the feature uses it for>.
- **Changes behaviour of:** `<existing caller>` `<path:line>`: <what is now different for it>.

## Claims and proof
| Criterion | Status | Code | Tests |
| --- | --- | --- | --- |
| <criterion id> <short text> | <✓ / ⚠ / ✗> | <node ids> | <test count> |

## Read first
1. <node id> `<path:line>`: <why this one first>.
2. ...
Skim: <node ids that are mechanical, e.g. renames, wiring>.

## Open questions
- <what the code does not answer, or looks wrong>
````

## Map rules

- Use simple technical English. No marketing words.
- A caption is one line, under 120 characters, and says what changed and why. Do not start it with the symbol name.
- Aim for 8–15 tour stops on a PR of about 20 files. Group repeated edits to reach that.
- Never link a criterion to code that only touches comments.
- In `tests`, list only tests whose assertions prove the criterion. A test that merely calls the function does not count.
- Only claim what the diff shows. If the logic lives in another PR or file, use `partial` and name the place in the criterion text.

## Overview rules

- Use simple technical English. No marketing words.
- Every claim about code cites `path:line` with a repo-relative path, so a reader can find it in their editor or on the forge. Cite node ids (`N3`) for changed code.
- Only state what the code shows. If you could not confirm something, put it under Open questions instead of guessing.
- Keep changed and existing code apart: say "new", "changed" or "existing" when it is not obvious.
- **Before and After**: two diagrams of the same flow, at `base` and at `head`, drawn so the reader can compare them box by box: the same node keys, labels and lanes for the same code, in the same order. The difference should jump out: boxes that change kind, appear or disappear, and arrows whose label or style changes (for example "<old rule>" before and "<new rule>" after).
  - **Before**: unchanged code is `existing`; code this branch changes has no kind and its node id (its old version); code it deletes is `removed` with its node id; arrows that go away are `removed`. No `added`.
  - **After**: unchanged code is `existing`; changed code has no kind and its node id; new code is `added` with its node id; new arrows are `added`. No `removed`.
  - Before steps: 1 to 4 short steps, only the parts the change touches, with `path:line` at base (the page links them to the base commit). After steps: the full runtime order.
  - If the flow is new and did not exist at base, write "None. This flow is new." under the Before heading, and say what the feature attaches to.
- **Flow diagrams** are ```` ```flow ```` blocks holding JSON. The page draws them as lanes stacked top to bottom, boxes in each lane, and curved arrows; hovering or tabbing to a box highlights its arrows and the boxes at their other ends, so keep the graph honest and small enough to trace:
  ```json
  {
    "nodes": {
      "<KEY A>": { "label": "<new symbol>", "n": "<node id>", "kind": "added",
                   "note": "<one-line summary>", "paths": ["<path:start-end>"] },
      "<KEY B>": { "label": "<existing symbol>", "kind": "existing", "paths": ["<path:line>"] },
      "<KEY C>": { "label": "<table, cache or queue>", "kind": "existing store", "paths": ["<path:line>"] }
    },
    "tiers": [
      [{ "name": "<lane>", "ids": ["<KEY A>"] }],
      [{ "name": "<lane>", "ids": ["<KEY B>"] }],
      [{ "name": "<lane>", "ids": ["<KEY C>"] }]
    ],
    "edges": [["<KEY A>", "<KEY B>", "<what passes>", "added"], ["<KEY B>", "<KEY C>"]]
  }
  ```
  - `nodes`: one per box. `label` is a symbol or short name, at most about 25 characters, no paths. `n` is the node id of changed code (shown as a chip); leave it out for unchanged code. `kind`, space separated: `existing` (unchanged, dashed), `added` (new, green), `removed` (deleted, red and struck through), plus `store` for tables, caches and queues (rounded). No kind means changed code (solid).
  - `paths` on every box: the files a reader opens for it, as repo-relative `path:line` or `path:start-end` strings (main one first; for changed code, the node's `sites` from the map). Clicking the box lists them with copy buttons, so a developer can paste them into their editor's file search. `note` is an optional one-line summary shown with them. In Before, paths are at `base` (the page links them there).
  - `tiers`: rows, top to bottom in lane order (ui, api, services, db, engine, jobs, other). A row holds one or more lanes `{ "name", "ids", "note"?, "flex"? }` side by side; use two lanes in a row only when two separate surfaces start the flow (e.g. <page A> and <page B>). Every node sits in exactly one lane. At most 3 boxes per lane, because Before and After share the width on a wide screen: group the rest, or split the lane into two rows.
  - `edges`: `[from, to, label?, style?]` in runtime order. The label says what passes or what decides, in a few words ("<call name>", "<condition>", "<read or write target>"). `style`: `added` (new arrow, green), `removed` (arrow that goes away, red dashed), `dashed` (async, fallback or cache). At most about 15 edges.
  - The render step checks every block and lists what is wrong.
- **Read first**: 3 to 5 stops, the ones that hold the decisions. Point the rest to the tour.
- Keep the prose under about 150 lines, not counting the `flow` blocks. Shorter is better. If a section has nothing in it, write "None." instead of dropping it.
- Tests are not part of the flow. They appear only in Claims and proof.
- Once the overview is written from the map, do not change the map to fit the overview. If the map looks wrong, fix it in step 3 and validate again, or say so under Open questions.
