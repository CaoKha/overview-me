# Contributing

## Try local changes

Start Claude from your clone:

```sh
OVERVIEW_ME_FROM_SOURCE=1 claude --plugin-dir .
```

Claude then loads the skills from your clone instead of the installed plugin, and builds the `overview-me` CLI from your code at session start. No push needed.

## Publish a new version

Users only get a new CLI with a new GitHub release.

1. Set the same `version` in `Cargo.toml` and `.claude-plugin/plugin.json`, e.g. `0.2.0` (CI fails if they differ).
2. Push to `main`.
3. Tag it: `git tag v0.2.0 && git push origin v0.2.0`. The Release workflow builds the CLI for macOS and Linux and attaches it to the release.
4. Users run `/plugin marketplace update overview-me`; their next session downloads the new CLI.

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
