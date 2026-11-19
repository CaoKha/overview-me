# The overview-me CLI

The skills call the plugin root `<root>`. The CLI is `<root>/bin/overview-me`, written `overview-me` in the skills. In Claude Code the plugin installs it when a session starts; in other agents it is installed the first time a skill runs, as below.

- Before the first CLI call of a run, run `bash "<root>/scripts/ensure-cli.sh"`. It returns at once when the binary matches the plugin version; otherwise it downloads the release for this machine or builds it from source.
- If it is still missing, tell the user what the script printed and stop.
- Run it from the root of the repo under review.

| Command | What it does |
| --- | --- |
| `overview-me base [<pr>]` | Prints `base`, `head`, `mode` (`pr`, `branch` or `uncommitted`) and `pr`: the named PR (fetched, not checked out), else the checked-out branch's PR, else the branch against the default branch. |
| `overview-me path [--head <sha>]` | Prints where the map for a head commit lives. Its folder is `<store>`. |
| `overview-me extract --base <sha> [--head <sha>] [--spec <file>] [--fresh]` | Writes the facts and the draft map for `base...head` (keeps a map for the same base unless `--fresh`). Prints the overviews that cover the change, each draft node with its diff, and the tests in changed test files. |
| `overview-me validate <map.json>` | Checks a map against the schema and git. Prints `ok`, or exits 1 with the problems. |
| `overview-me flow <map.json>` | Draws the Before and After flow diagrams from the code into `<store>/<head>.flow.json`, and prints entry points, unchanged callers and callees, and removed symbols still referenced. |
| `overview-me render <map.json> [--change --scope <text>] [--fresh]` | Validates the map, runs `flow`, and writes the overview page `<store>/<head>.review.md` with `<!-- fill: … -->` slots. Refuses to overwrite a page unless `--fresh`. |
| `overview-me snapshot` | Prints a commit holding HEAD plus every uncommitted and untracked file, without touching the branch, the index or the files. |
| `overview-me page <file.md> [--unpushed]` | Writes the overview as an HTML page next to it. Fills ```` ```flow ```` blocks holding `@before` or `@after` from the flow file. Exits 1 when a `flow` block is invalid, a slot is left, a `path:line` points at no line, a node id is not in the map, or an open question cites nothing. |
