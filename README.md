# overview-me

One-page overviews for reviewing agent-written code: a PR, your uncommitted changes, or the whole codebase. Each page links your spec to the code, groups the change by layer, draws the flow before and after, and gives you a short reading order with a caption on every changed place.

## Install

In Claude Code:

```
/plugin marketplace add CaoKha/overview-me
/plugin install overview-me@overview-me
```

That's all. When a session starts, the plugin installs the `overview-me` CLI its skills run into the plugin folder: it downloads the release built for your machine (macOS or Linux, arm64 or x86_64), or builds it with `cargo` if there is no release for it. It does this once per plugin version. While the repo is private, the download uses the `gh` CLI's GitHub login.

The skills are `/overview-me:pr-overview`, `/overview-me:change-overview` and `/overview-me:codebase-overview`; asking in plain words ("review this PR", "review my changes") works too. `/plugin marketplace update overview-me` pulls new versions, and the next session fetches the matching CLI.

## Develop

- `OVERVIEW_ME_FROM_SOURCE=1 claude --plugin-dir /path/to/overview-me` loads the plugin from your clone and builds the CLI from it at each session start.
- To release: bump `version` in `Cargo.toml` and `.claude-plugin/plugin.json` (CI checks they match), push, then push the tag `v<version>`. The Release workflow builds the four binaries and publishes them; installed plugins pick them up after `/plugin marketplace update`.

## Use

0. Once per repo, run **`/codebase-overview`**. It maps the whole codebase at the default branch (modules, lanes, entry points, core concepts, main flows, where to find things), so a PR has a place to sit. Run it again when main has moved; it only updates what changed.
1. With the PR head checked out, run **`/pr-overview`**, ideally in the session that wrote the code. Name the PR if it can't guess it: `/pr-overview PR 3389`. Plain words work too: "map this PR", "explain this change". It first builds the change map (criteria and the tests that prove them, one captioned stop per changed place, a reading order) and validates it against git, then writes the overview: entry points, before and after flows, data, the existing code it plugs into, and what to read first. Run it again on the same head and it reuses the map.
2. Before you commit, run **`/change-overview`** for the same page about your uncommitted work: the staged, unstaged and untracked files that `git status` shows, against HEAD. It maps a snapshot of the working tree and never commits, stages or edits anything.

Every skill ends by showing its result as a page: a Claude artifact in Claude Code, or the same page opened in the browser elsewhere. On the page:

- **Flow diagrams**: hover a box to trace its links; click it to pin it and list its files (`path:line`) with copy buttons, ready to paste into your editor's file search. Esc or a click on the background unpins it.
- **PR overview**: the before and after flows sit side by side on a wide screen. Solid boxes are changed code, green new, red removed, dashed unchanged.
- Every `path:line` links to the forge at the right commit (base for the Before flow).

## CLI

```sh
overview-me extract --base <sha> [--spec spec.md]   # facts + draft map
overview-me validate <map.json>                     # exit 1 on any problem
overview-me render <map.json>                       # plain review guide <head>.md (not used by the skills)
overview-me page <file.md> [--out page.html] [--unpushed]  # overview as an HTML page
overview-me snapshot                                # commit of HEAD + uncommitted files, branch untouched
overview-me path                                    # where the map for HEAD lives
```

## How it works

- **The map is facts, the labels are help.** The binary computes the structure from git and tree-sitter. Claude writes criteria, captions, groups and the tour. `validate` rejects any label that disagrees with git: every changed line, deleted file and move must belong to exactly one stop, and every test cited as proof must be a real test at that line.
- Tests never become stops. A criterion counts as tested only when it names the tests that prove it.
- Maps live in `$(git rev-parse --git-common-dir)/overview-me/<head-sha>.json`: never committed, shared by all worktrees. Beside it: the PR overview `<head-sha>.review.md` and the codebase overview `codebase.md` (one per repo). `overview-me page` writes each one's page as `.html` next to it, from the template `assets/overview.html` compiled into the binary.
- Lanes, test paths and risk keywords are in `rules.json` (override with `--rules`).

## Limits

- Call edges use name matching, not the language server.
- Risk (read / skim) is keyword-based.
- Needs git 2.30 or later.
