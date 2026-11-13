# overview-me

Understand a code change in five minutes instead of reading the diff line by line.

overview-me is a Claude Code plugin. Ask Claude about a PR, your uncommitted changes or a whole codebase, and it writes a one-page overview: what the change does, how the code flows before and after it, which tests prove each claim, and which few places to read first. Every claim points to `path:line`, and the change map behind it is checked against git.

## Install

In Claude Code:

```
/plugin marketplace add CaoKha/overview-me
/plugin install overview-me@overview-me
```

## Use

Ask in plain words, or run the command:

| Ask | Command | You get |
| --- | --- | --- |
| "review this PR", "explain PR 3389" | `/overview-me:pr-overview` | the overview of the PR checked out |
| "review my changes" | `/overview-me:change-overview` | the same, for your uncommitted files |
| "explain this codebase" | `/overview-me:codebase-overview` | a map of the repo; PR overviews use it to place a change |
| "explain packages/api" | `/overview-me:folder-overview <folder>` | the same map for one folder |

The result opens as a page. Hover a box in a flow diagram to trace its links, click it to copy its file paths into your editor, and follow any `path:line` to the code.

Working on overview-me itself? See [CONTRIBUTING.md](CONTRIBUTING.md).
