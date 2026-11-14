# The overview-me CLI

The skills call the plugin root `<root>`. The CLI is `<root>/bin/overview-me`, written `overview-me` in the skills. The plugin installs it when a session starts.

- If it is missing, run `bash "<root>/scripts/ensure-cli.sh"` once. It downloads the release for this machine or builds it from source.
- If it is still missing, tell the user what the script printed and stop.
- Run it from the root of the repo under review.

| Command | What it does |
| --- | --- |
| `overview-me path [--head <sha>]` | Prints where the map for a head commit lives. Its folder is `<store>`. |
| `overview-me extract --base <sha> [--head <sha>] [--spec <file>]` | Writes the facts and a draft map for `base...head`, and prints their paths. |
| `overview-me validate <map.json>` | Checks a map against the schema and git. Prints `ok`, or exits 1 with the problems. |
| `overview-me snapshot` | Prints a commit holding HEAD plus every uncommitted and untracked file, without touching the branch, the index or the files. |
| `overview-me page <file.md> [--unpushed]` | Writes the overview as an HTML page next to it. Exits 1 on an invalid `flow` block. |
