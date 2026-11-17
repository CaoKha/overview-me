# overview-me

Understand a code change in five minutes instead of reading the diff line by line.

overview-me is a Claude Code plugin. Ask Claude about a PR, your uncommitted changes or a whole codebase, and it writes a one-page overview: what the change does, how the code flows before and after it, which tests prove each claim, and which few places to read first. Every claim points to `path:line`, and the change map behind it is checked against git.

## Why not just ask Claude?

You can ask any agent to "explain this PR", with or without a skill. The answer changes from run to run: the agent may read different files, skip a changed file it never opened, cite a line that moved, or say a test proves something it doesn't. A skill is only instructions, and the model follows them more or less closely each time.

Most of that work doesn't need an agent anyway. Listing changed lines, tracking moves, finding callers and checking that a test exists are mechanical jobs. A program does them the same way every time, and it is never wrong about what git says. So overview-me follows one rule: **if a tool can do the job, a tool does it. The agent only does what needs judgment.**

- **The tool gets the facts.** A small CLI reads git and parses the code with tree-sitter. It finds the changed lines, the moved and deleted files, and which functions call which. The same diff always gives the same map.
- **The agent writes the labels.** Claude adds what a program can't: the criteria, the captions, the groups and the reading order.
- **The tool checks the agent.** `overview-me validate` rejects the map if a changed line, deleted file or move doesn't belong to exactly one stop. It also rejects a test cited as proof that isn't a real test at that line. Claude must fix the map until it passes, and only then writes the overview.
- **Fixed rules decide the grouping.** Lanes, test paths and risk keywords come from `rules.json`, so a file lands in the same place on every run.
- **One map per commit.** The map is saved under the commit's SHA and reused, so asking again about the same commit gives the same structure.

The wording still changes between runs, but the facts don't. Nothing in the diff is skipped without a word, and every `path:line` points to real code.

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
