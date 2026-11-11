---
name: feature-overview
description: Write a one-page overview of the feature a PR builds, from the change map that /change-map made, covering entry points, runtime flow, data, where it plugs into existing code, and what to read first. Use when the user asks for a "feature overview", an "overview of this PR", the "structure of this feature", or to "explain this change" after a change map exists.
---

# Feature overview

The change map says *what changed*. The overview says *how the feature works*: how it is triggered, what runs, what data it touches, and which existing code it leans on. The reader should understand the feature in five minutes without opening the codebase, then use the overview to pick what to read.

`${CLAUDE_SKILL_DIR}/../..` is the change-map checkout. Binary: `${CLAUDE_SKILL_DIR}/../../target/release/change-map` (called `change-map` below). If it is missing, run `cargo build --release` in `${CLAUDE_SKILL_DIR}/../..` once. Run it from the root of the repo under review.

## Steps

1. **Load the map.**
   - Run `change-map path`. The map is that file; the overview goes next to it as `<head>.overview.md`.
   - If the map does not exist, stop and tell the user to run `/change-map` first.
   - Run `change-map validate "$(change-map path)"`. If it fails, stop and say the map must be fixed first. Do not write an overview from a wrong map.
2. **Read the codebase overview.** If `<store>/codebase.md` exists (`<store>` is the folder of the map), read it: modules, lanes, core concepts and main flows. Use its module names and terms, and place the feature in it. If it is missing, go on, and suggest `/codebase-overview` at hand-off.
3. **Read the map.** From the JSON:
   - `criteria`: what the feature claims to do, their status and tests.
   - `nodes`: lane, caption, risk and sites of every changed place.
   - `edges` and `tour`: how the changed places call each other, and the author's reading order.
   - `spec`: if set, read the spec sections that the criteria's `specRef` point to.
4. **Read the code, but only around the map.** Do not read the whole repo.
   - Read the main site (first site) of every `read` node, and the enclosing function of every site you cite.
   - **Entry points**: among the nodes, find the ones nothing in the map calls, mostly in `ui`, `api` and `jobs`. Find how each one is reached: the route, command, event, job schedule or UI action that triggers it. Search for its symbol to find the registration.
   - **Existing code**: for each changed symbol, find its callers and callees outside the diff, one hop only (`git grep -n <symbol>`). These are the unchanged pieces the feature depends on, and the existing callers whose behaviour may change.
   - **Data**: find the types, tables, migrations, config keys and messages that the feature adds or changes, and who writes and reads each one.
5. **Write** `<head>.overview.md` next to the map, in the format below.
6. **Hand off.** Give the path, the one-paragraph summary, and the gaps (missing, partial or untested criteria, and open questions). Tell the user that in nvim `:ChangeMapOverview` (or `<leader>ro`) opens it, and `gF` on a `path:line` jumps to the code.

## Format

```markdown
# <Feature name> · overview

<base 10>...<head 10> · <N> stops · <M> criteria · spec: <spec or "none">

## In one paragraph
What the feature does for its user, and the approach the code takes. 3–5 sentences.

## Where it sits
Modules touched: <module (lane)>, … from the codebase overview. Which of its main flows this feature joins or adds to. "No codebase overview" if there is none.

## Entry points
- <trigger: route, command, job, UI action> → `<symbol>` `path:line` (N3)

## Flow
<mermaid flowchart, see rules>

1. <step in runtime order> `path:line` (N3)
2. ...

## Data and state
| Thing | Kind | Written by | Read by |
| --- | --- | --- | --- |
| `orders.refund_id` | column (new) | N5 `path:line` | N7 `path:line` |

## Where it plugs in
- **Depends on (unchanged):** `<symbol>` `path:line`: what the feature uses it for.
- **Changes behaviour of:** `<existing caller>` `path:line`: what is now different for it.

## Claims and proof
| Criterion | Status | Code | Tests |
| --- | --- | --- | --- |
| REF-1 <short text> | ✓ / ⚠ / ✗ | N3, N5 | 2 |

## Read first
1. N3 `path:line`: why this one first.
2. ...
Skim: <node ids that are mechanical, e.g. renames, wiring>.

## Open questions
- <what the code does not answer, or looks wrong>
```

## Rules

- Use simple technical English. No marketing words.
- Every claim about code cites `path:line` with a repo-relative path, so `gF` can open it. Cite node ids (`N3`) for changed code.
- Only state what the code shows. If you could not confirm something, put it under Open questions instead of guessing.
- Keep changed and existing code apart: say "new", "changed" or "existing" when it is not obvious.
- **Flow diagram**: `flowchart LR`, one subgraph per lane in lane order (ui, api, services, db, engine, jobs, other), at most 15 boxes. Changed nodes are solid boxes labelled `N3 symbol`; existing code the flow passes through is a dashed box (`classDef existing stroke-dasharray: 4 3`). Arrows follow runtime order, not the tour. Quote labels and use no `#` in them.
- **Read first**: 3 to 5 stops, the ones that hold the decisions. Point the rest to the tour.
- Keep the whole file under about 150 lines. Shorter is better. If a section has nothing in it, write "None." instead of dropping it.
- Tests are not part of the flow. They appear only in Claims and proof.
- Do not edit the map. If the map looks wrong, say so under Open questions.
