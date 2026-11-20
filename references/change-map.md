# Label the change map

The map follows `<root>/schema/change-map.schema.json`. `overview-me extract` writes the draft to the map path and prints each node with its diff; you add the labels in that file; `overview-me validate` checks them against git.

The draft already has the structure: one node per changed symbol, import-only hunks folded into the function in their file, the same edit at several places grouped into one node, `calls` edges, risk, `touchedByTest`, and a tour with callees before callers. Change the structure only where the draft got it wrong.

Above 300 draft nodes, the draft groups by folder (default depth 2: `apps/x`, `packages/x`). Keep these module groups for a modules-only page. Use `--depth N`, `--split <prefix>` (repeat, first match wins), or `--path <prefix> --map <scoped.json>` to change scope. `--path` requires `--map`; facts go beside the custom map unless `--out` is set. Labels are kept for the same base, head, path and grouping; changed grouping settings write a new draft. `overview-me node N3 --map <map>` prints one group’s hunks; tests remain in the facts file. The main site is the largest body hunk, with imports and error definitions used only as fallbacks.

## Criteria

- **criteria**: what this change claims to do, from the PR description and the spec parts it implements.
  - Use ids `REF-1`, `REF-2`, …
  - Add a `specRef` of `"<spec path>#<heading-slug>"` when a spec section covers the criterion.
- **status**: `implemented`; `partial` (some of the logic lives outside this diff: name the place in the criterion text); or `missing` (the change claims it, but no code does it). Only claim what the diff shows.
- **tests**: per criterion, the tests whose assertions prove it, as `{ file, line, name }` from the tests `extract` printed (or from an existing test file).
  - Read the test before you list it.
  - A test that only calls the code does not count.
  - If no test checks the claim, leave `tests` empty. The criterion then shows as untested, which is the point.

## Nodes

- **caption**: one line per node, under 120 characters, in simple technical English with no marketing words. Say what changed and why. Do not start it with the symbol name.
- **node.criteria**: the criteria the node implements. Leave it empty for a node that only touches comments, and never link a criterion to such a node.
- **groups**: merge nodes that belong together but the draft kept apart, such as a type-only change and the function it serves. Move the sites into one node, delete the others, and remap `edges` and `tour`. Aim for 8–15 nodes on a PR of about 20 files.
- **lane**: the flow section the node is drawn in. The draft names it from `<root>/rules.json` (ui, api, services, db, engine, jobs) or, where no rule matches, from the path: the first folder that is not generic such as `src` or `packages`, else the file name. Keep a drafted name that already says what that part of the system is. Rename one that does not, such as `flow` and `render` that together are the page renderer, so a reader sees the parts of the system the change crosses. Use at most 5 lanes, each 1–24 characters, the same name for every node in that part, and the module names of the codebase overview when one exists. The flow draws lanes callers above callees, and `db` files as stores whatever their lane is called.
- **risk**: `read` (holds logic a reviewer must read) or `skim` (mechanical). A merged node is `read` if any node merged into it was `read`. You may raise `skim` to `read` when the code holds a decision; never lower `read` to `skim`.
- **sites**: the main site (the function body) stays first. Every added line must sit inside exactly one node's sites.
- **renames**, **deletions**: every listed move and every deleted non-test file sits in exactly one node. Fold it into the node whose change explains it, or keep its own node with a caption that says why. Treat a move into a test path as suspect.
- Never set `touchedByTest` by hand. It is a computed hint, and the validator says what it must be.

## Edges

- **Module edges** have a `weight`: the number of reference sites reaching declarations in the target module. The draft keeps the strongest two outgoing calls per module, at most 15 total; After uses these calls; Before keeps base-side calls between modules and drops external boxes. `grouped: true` keeps removed symbols inside their module and caps After prose steps at 20, reporting omitted steps. A scoped map records `path`; validation covers only that path.
- **edges**: the draft links a node to the nodes whose symbols it names, following imports to the file that declares them. `at` lists the first references behind an edge as `path:line` at head: read them to check the edge. `validate` warns when none of them can name the target's code (a property, key or local of the same name). Between two nodes, the After flow draws an arrow only when `edges` has it. Delete an edge the code does not make, for example two unrelated symbols that share a name; `flow` and `render` keep the arrow out on every run. Adding an edge does not draw an arrow the code does not show.

## Tour

- **tour**: every node id once, in reading order. The draft puts callees before callers. Reorder only when the contract or shape the reader needs first is elsewhere.

## Callouts

- **callouts**: code that behaves in a way a careful reader could miss, as `{ tag, node, file, line, note }`.
  - `tag`: `breaking` (changes a contract an existing caller, user or stored data relies on), `security` (touches auth, input checks, secrets or permissions), `migration` (needs a deploy order, a backfill or a data change), `race` (depends on timing or ordering between concurrent work), `perf` (changes the cost of a hot path, a query or a loop), or `subtle` (anything else that is easy to misread).
  - `node`, `file`, `line`: the node it is about, and a line inside one of that node's sites.
  - `note`: one sentence that says what a reader could miss and when it matters.
- At most 5, most important first. Fewer is better: callouts lose their value when there are many. Leave the list empty when nothing qualifies.
- Each callout must be confirmed in the code. If you are not sure, put it under Open questions in the page instead.
- A callout explains behaviour. It is not a bug report or a verdict on the change.
