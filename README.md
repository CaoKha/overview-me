# overview-me

Understand a code change in five minutes instead of reading the diff line by line.

overview-me is a Claude Code plugin. Ask Claude about a PR, your uncommitted changes or a whole codebase, and it writes a one-page overview: what the change does, how the code flows before and after it, which tests prove each claim, which few places to read first, and what is easy to miss (breaking changes, races, migrations and the like). Every claim points to `path:line`, and the change map behind it is checked against git.

## Install

In Claude Code:

```
/plugin marketplace add CaoKha/overview-me
/plugin install overview-me@overview-me
```

## Update

### Manually

In a terminal:

```sh
claude plugin marketplace update overview-me
claude plugin update overview-me@overview-me
```

The first command fetches the latest list of versions; the second installs the newest one. Then start a new Claude Code session: the new skills load, and the session downloads the CLI for the new version.

### Automatically

Run `/plugin` in Claude Code, open the overview-me marketplace, and turn on auto-update. Claude Code then installs new versions for you; the next session after an update downloads the new CLI.

## Use

Ask in plain words, or run the command:

| Ask | Command | You get |
| --- | --- | --- |
| "PR overview", "explain PR 3389" | `/overview-me:pr-overview` | the overview of the PR checked out |
| "overview of my changes" | `/overview-me:change-overview` | the same, for your uncommitted files |
| "explain this codebase" | `/overview-me:codebase-overview` | a map of the repo; PR overviews use it to place a change |
| "explain packages/api" | `/overview-me:folder-overview <folder>` | the same map for one folder |

The result opens as a page. Hover a box in a flow diagram to trace its links, click it to copy its file paths into your editor, and follow any `path:line` to the code.

Working on overview-me itself? See [CONTRIBUTING.md](CONTRIBUTING.md).
