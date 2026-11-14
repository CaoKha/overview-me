---
name: pr-overview
description: Write a one-page overview of a PR or branch diff, with a change map checked against git and an explanation of how the feature works, from its entry points and the flow before and after to what to read first. Use when the user asks for a "PR overview", a "feature overview", a "change map", to "map this PR" or "map this diff", an "overview of this PR", the "structure of this feature", or to "explain this change". This is an overview, not a code review, so do not use it when the user asks to "review" a PR or diff, look for bugs, or approve a change, unless they also ask for an overview or map. For uncommitted work, use change-overview.
---

# PR overview

Two parts. The change map says *what changed*: every changed line belongs to one captioned stop, each claim names the tests that prove it, and `overview-me validate` rejects any label that disagrees with git. The overview says *how the feature works*: how it is triggered, what runs, what data it touches, and which existing code it leans on. The reader should understand the feature in five minutes without opening the codebase, then use the overview to pick what to read.

`<root>` is `${CLAUDE_SKILL_DIR}/../..`, the overview-me plugin root. The steps run its CLI, `overview-me`. Read `<root>/references/cli.md` first: it says where the CLI is, what to do if it is missing, and what each command does.

## Terms

- **base**, **head**: the two commits the change sits between. `base...head` is the diff.
- **`<store>`**: `$(dirname "$(overview-me path)")`, the folder that holds maps and overviews. It is never committed and is shared by all worktrees.
- **change map** (or map): the JSON file at `$(overview-me path)` that lists the criteria and nodes of one change.
- **criterion**: one thing the change claims to do, with its status and the tests that prove it.
- **node**: one changed place in the map, with an id like `N3`: one symbol, or the same edit at several sites. The page and the reader call a node a **stop**; the two words name the same thing.
- **site**: a range of changed lines in a node, on the head side. The first site is the node's main site.
- **tour**: every node id once, in the order a reader should go through them.
- **lane**: the layer a path belongs to (ui, api, services, db, engine, jobs, other), from `rules.json`.
- **risk**: `read` for a node a reviewer must read, `skim` for a mechanical one.
- **box**: one entry in a flow diagram's `nodes`. It is not a map node, but a box for changed code carries its map node id in `n`.

## Steps

1. **Find the base and head.** `<head>` is HEAD, unless the user names a PR that is not checked out.
   - For the checked-out PR, find its base branch with `gh pr view --json baseRefName`, then run `git merge-base origin/<base> HEAD`.
   - For a PR that is not checked out (the user gives a number or link that is not the current branch's PR), do not check it out: that would touch the user's files. Run `gh pr view <n> --json baseRefName,headRefOid`, then `git fetch origin <baseRefName> pull/<n>/head`. `<head>` is the `headRefOid`, and the base is `git merge-base origin/<baseRefName> <head>`.
   - For a stacked PR, the base is the PR below it.
   - With no PR (a plain branch, so `gh pr view` fails), find the default branch as in codebase-overview step 1: `git symbolic-ref --short refs/remotes/origin/HEAD`, else `origin/main`, `main`, then `master`. Run `git merge-base <default> HEAD`.
   - If `<head>` is HEAD, `git rev-list --count <base>..HEAD` prints 0 and `git status --porcelain` prints something, the work is not committed: tell the user, and follow `<root>/skills/change-overview/SKILL.md` instead.
   - If the base is still unclear, ask.
   - **If `<head>` is not HEAD**, use it wherever the steps below use HEAD: `overview-me path --head <head>` instead of `overview-me path`, `overview-me extract --base <base> --head <head>`, and `git show <head>:<path>` and `git grep -n <symbol> <head>` instead of reading the working tree.
2. **Read the codebase and folder overviews.**
   - If `<store>/codebase.md` exists, read it: modules, lanes, core concepts and main flows. Use its module names and terms in the map and the overview, and place the feature in it. If it is missing, go on, and suggest `/codebase-overview` at hand-off.
   - List `<store>/folders/`. Each `<slug>.md` there covers the folder you get by replacing each `--` in the slug with `/`. Read the ones whose folder holds a file from `git diff --name-only <base> <head>`, and use their submodule names and terms for changes inside that folder.
3. **Build the change map.** The map is `$(overview-me path)`, one JSON file per head commit that follows `<root>/schema/change-map.schema.json`. Git gives the facts; you add the labels (criteria, the tests that prove them, captions, groups and the tour); the validator checks your labels against git.
   - **Reuse.** If the map exists, its `base` is the base from step 1, and `overview-me validate "$(overview-me path)"` prints `ok`, keep it and go to step 4, unless the user asked to rebuild it.
   - **Extract.** Run `overview-me extract --base <sha> [--spec <spec file>]`. It prints the facts path, the map path and the draft nodes.
   - **Read.** Read the spec, the PR description (`gh pr view --json body`), and the facts file. In the facts file:
     - `units[].diff` holds the changes. Each unit is one changed hunk inside one symbol.
     - `testDeclarations` lists the tests in the changed test files.
     - Draft nodes with no sites and a `deletions` or `renames` entry stand for deleted or moved files.
   - **Fill.** Copy `draft` from the facts file to the map path, then fill in the labels as `<root>/references/change-map.md` says.
   - **Validate.** Run `overview-me validate "$(overview-me path)"`. Fix every problem and run it again until it prints `ok`. Do not write an overview from a map that does not validate.
4. **Read the map.** From the JSON:
   - `criteria`: what the feature claims to do, their status and tests.
   - `nodes`: lane, caption, risk and sites of every changed place.
   - `edges` and `tour`: how the changed places call each other, and the author's reading order.
   - `spec`: if set, read the spec sections that the criteria's `specRef` point to.
5. **Read the code, but only around the map.** Do not read the whole repo.
   - Read the main site (first site) of every node with `risk: read`, and the enclosing function of every site you cite.
   - **Entry points**: among the nodes, find the ones nothing in the map calls, mostly in `ui`, `api` and `jobs`. Find how each one is reached: the route, command, event, job schedule or UI action that triggers it. Search for its symbol to find the registration.
   - **Existing code**: for each changed symbol, find its callers and callees outside the diff, one hop only (`git grep -n <symbol>`). These are the unchanged pieces the feature depends on, and the existing callers whose behaviour may change.
   - **Data**: find the types, tables, migrations, config keys and messages that the feature adds or changes, and who writes and reads each one.
   - **Before**: read the same flow at the map's `base` commit, with `git show <base>:<path>` and `git grep -n <symbol> <base>`. Find how the entry points were reached, what the changed functions did, and which deleted code took part. This is the Before half of the Flow section. If the flow did not exist at base, there is no Before diagram.
6. **Write** `<head>.review.md` next to the map, in the format below.
7. **Show the page** as `<root>/references/show-page.md` says, for `<store>/<head>.review.md`.
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
- **Flow diagrams**: write each one as a `flow` block, as `<root>/references/flow-blocks.md` says, with the limits for a PR or change overview.
- **Read first**: 3 to 5 stops, the ones that hold the decisions. Point the rest to the tour.
- Keep the prose under about 150 lines, not counting the `flow` blocks. Shorter is better. If a section has nothing in it, write "None." instead of dropping it.
- Tests are not part of the flow. They appear only in Claims and proof.
- Once the overview is written from the map, do not change the map to fit the overview. If the map looks wrong, fix it in step 3 and validate again, or say so under Open questions.
