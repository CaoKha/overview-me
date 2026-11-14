# Fill the change map

The map follows `<root>/schema/change-map.schema.json`. Git gives the facts and `overview-me extract` writes a draft; you add the labels; `overview-me validate` checks them against git.

Copy `draft` from the facts file to the map path, then edit it.

## Criteria

- **criteria**: what this change claims to do, from the PR description and the spec parts it implements.
  - Use ids `REF-1`, `REF-2`, …
  - Add a `specRef` of `"<spec path>#<heading-slug>"` when a spec section covers the criterion.
- **status**: `implemented`; `partial` (some of the logic lives outside this diff: name the place in the criterion text); or `missing` (the change claims it, but no code does it). Only claim what the diff shows.
- **tests**: per criterion, the tests whose assertions prove it, as `{ file, line, name }` taken from `testDeclarations` (or from an existing test file).
  - Read the test before you list it.
  - A test that only calls the code does not count.
  - If no test checks the claim, leave `tests` empty. The criterion then shows as untested, which is the point.

## Nodes

- **groups**: merge nodes that make the same edit in several places into one node with several `sites`.
  - Fold import-only and type-only nodes into the sites of the function they serve.
  - Delete the merged nodes, and remap `edges`.
  - Aim for 8–15 nodes on a PR of about 20 files. Group repeated edits to reach that.
- **risk**: `read` (holds logic a reviewer must read) or `skim` (mechanical). `extract` sets it on each draft node from the rules. A merged node is `read` if any node merged into it was `read`. You may raise `skim` to `read` when the code holds a decision; never lower `read` to `skim`.
- **sites**: list the main site (the function body) first. It gets the caption, and the overview cites it as the node's main place. Every added line must sit inside exactly one node's sites.
- **renames**: every listed move (`{ from, to }`: a pure rename, or production code moved into a test path) goes in exactly one node's `renames`. Say in the caption why it moved. Treat a move into a test path as suspect.
- **deletions**: every deleted non-test file goes in exactly one node's `deletions`. Move it into the node whose change explains it, or keep its own node with a caption that says why it was removed.
- **caption**: one line per node, under 120 characters, in simple technical English with no marketing words. Say what changed and why. Do not start it with the symbol name.
- **node.criteria**: the criteria the node implements. Leave it empty for a node that only touches comments, and never link a criterion to such a node.
- Never set `touchedByTest` by hand. It is a computed hint, and the validator says what it must be.

## Edges and tour

- **edges**: keep the `calls` edges. Add `implements` or `uses` edges only when they help the reader.
- **tour**: every node id once, in reading order. Contract or shape first, then implementation, then callers.
