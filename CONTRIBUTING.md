# Contributing

## Try local changes

Start Claude from your clone:

```sh
OVERVIEW_ME_FROM_SOURCE=1 claude --plugin-dir .
```

Claude then loads the skills from your clone instead of the installed plugin, and builds the `overview-me` CLI from your code at session start. No push needed.

## Test which skill fires

After changing a skill's `description`, run the trigger tests in `evals/` (see `evals/README.md`):

```sh
claude plugin eval . --ablation none --tag trigger --model sonnet --no-publish
```

## Security checks

The Security workflow runs on every PR, on `main` and weekly:

- `cargo deny check` checks crates for advisories, licenses and sources (rules in `deny.toml`). A new license fails it until you add it there.
- [zizmor](https://docs.zizmor.sh) checks the workflows. Pin every action to a commit SHA; Dependabot keeps the pins current.
- CodeQL checks the Rust code. Results are in the repo's Security tab.

## Publish a new version

Users only get new skills and a new CLI with a new version and GitHub release. [Release Please](https://github.com/googleapis/release-please) does both from the commit messages, so use conventional commits (`fix:` for a patch, `feat:` for a minor while below 1.0, `feat!:` for a breaking change).

1. Merge to `main`. Release Please opens or updates a `chore(main): release X.Y.Z` PR that bumps `Cargo.toml`, `Cargo.lock`, `.claude-plugin/plugin.json` and `package.json` and writes `CHANGELOG.md`.
2. Merge that PR when you want to release. Release Please tags `vX.Y.Z` and creates the GitHub release, then the Release workflow builds the CLI for macOS and Linux and attaches it.
3. Claude Code users run `/plugin marketplace update overview-me` (or turn on auto-update for the marketplace); their next session downloads the new CLI. pi users run `pi update --extensions`; the next skill run downloads the new CLI.

The Release workflow also runs when you push a `v*` tag by hand; the tag must match the versions in `Cargo.toml`, `.claude-plugin/plugin.json` and `package.json`.

## CLI

```sh
overview-me base [<pr>]                             # base and head of the PR or branch
overview-me extract --base <sha> [--spec spec.md]   # facts + draft map, with each node's diff
overview-me validate <map.json>                     # exit 1 on any problem
overview-me flow <map.json>                         # Before and After diagrams from the code
overview-me render <map.json> [--change --scope <text>] [--findings]  # overview page with slots to fill
overview-me findings <map.json> <review.json|.sarif>  # review findings placed on the map's nodes
overview-me page <file.md> [--out page.html] [--unpushed]  # overview as an HTML page, checked
overview-me snapshot                                # commit of HEAD + uncommitted files, branch untouched
overview-me path                                    # where the map for HEAD lives
```

## How it works

- **The CLI does the work, Claude writes the judgement.** The binary computes the structure from git and tree-sitter: the base, the draft map (groups, folded imports, edges, tour), both flow diagrams, and the page skeleton with every section the map gives. Claude writes criteria, captions and callouts in the map, and fills the page's `<!-- fill: … -->` slots. `page` refuses a page with a slot left, a `path:line` that points at no line, an unknown node id, or an open question that cites nothing. `validate` rejects any label that disagrees with git: every changed line, deleted file and move must belong to exactly one stop, and every test cited as proof must be a real test at that line, and every callout must point at a line inside its stop.
- Tests never become stops. A criterion counts as tested only when it names the tests that prove it.
- Maps live in `$(git rev-parse --git-common-dir)/overview-me/<head-sha>.json`: never committed, shared by all worktrees. Beside it: the flow file `<head-sha>.flow.json`, the PR overview `<head-sha>.review.md` the codebase overview `codebase.md` (one per repo) and feature overviews `features/<slug>.md`. `extract` deletes per-SHA store files older than 30 days, except the head it extracts; `codebase.*` and `features/` are kept. `overview-me page` writes each one's page as `.html` next to it, from the template `assets/overview.html` with the styles and scripts in `assets/` (see `assets/README.md`), compiled into the binary.
- Draft lanes come from the folders of the change; the Label step renames them to what that part of the system is and may regroup them. Test paths, store paths and risk keywords are in `rules.json` (override with `--rules`).
- Skills live in `skills/<name>/SKILL.md`. Text that more than one skill needs is in `references/` (the CLI, showing the page, `flow` blocks, filling the change map), so change it there once.

## Limits

- Call edges use name matching, not the language server. A flow arrow is drawn only for a call to a function, method, macro, constructor or constant, never for naming a type; a name with two definitions in scope gets no arrow.
- Risk (read / skim) is keyword-based.
- Needs git 2.30 or later.
