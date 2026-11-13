# Contributing

## Try local changes

Start Claude from your clone:

```sh
OVERVIEW_ME_FROM_SOURCE=1 claude --plugin-dir .
```

Claude then loads the skills from your clone instead of the installed plugin, and builds the `overview-me` CLI from your code at session start. No push needed.

## Publish a new version

Users only get new skills and a new CLI with a new version and GitHub release. [Release Please](https://github.com/googleapis/release-please) does both from the commit messages, so use conventional commits (`fix:` for a patch, `feat:` for a minor while below 1.0, `feat!:` for a breaking change).

1. Merge to `main`. Release Please opens or updates a `chore(main): release X.Y.Z` PR that bumps `Cargo.toml`, `Cargo.lock` and `.claude-plugin/plugin.json` and writes `CHANGELOG.md`.
2. Merge that PR when you want to release. Release Please tags `vX.Y.Z` and creates the GitHub release, then the Release workflow builds the CLI for macOS and Linux and attaches it.
3. Users run `/plugin marketplace update overview-me` (or turn on auto-update for the marketplace); their next session downloads the new CLI.

The Release workflow also runs when you push a `v*` tag by hand; the tag must match both versions.

## CLI

```sh
overview-me extract --base <sha> [--spec spec.md]   # facts + draft map
overview-me validate <map.json>                     # exit 1 on any problem
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
