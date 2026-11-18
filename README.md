# overview-me

Understand a code change in five minutes instead of reading the diff line by line.

overview-me is a Claude Code plugin. Ask Claude about a PR, your uncommitted changes or a whole codebase, and it writes a one-page overview you can click through.

![A PR overview: the header facts, the On this page list and the first sections](docs/images/page-dark.png)

## What you get

- **The flow, before and after.** Diagrams of how the code ran before the change and how it runs now. Hover a box to trace its links, click it for its files.
- **Every claim points to code.** Each `path:line` has a preview that opens the file at those lines and rings the boxes that hold them in the flow.
- **Claims checked against tests.** Each criterion shows the code that implements it and the tests that prove it, or says that none do.
- **Where to start.** The few places to read first, and what is easy to miss: breaking changes, races, migrations and the like.
- **A map of the repo.** A codebase overview draws the modules and how they call each other. PR overviews use it to say where a change sits.

The [tutorial](docs/tutorial.md) walks through a real page, step by step, with screenshots.

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
| "PR overview", "explain PR 3389" | `/overview-me:pr-overview` | the overview of the PR checked out |
| "overview of my changes" | `/overview-me:change-overview` | the same, for your uncommitted files |
| "explain this codebase" | `/overview-me:codebase-overview` | a map of the repo; PR overviews use it to place a change |

In Claude Code the overview opens as a private Claude artifact; elsewhere it opens as an HTML file in your browser.

## Why not just ask Claude?

You can ask any agent to "explain this PR". The answer changes from run to run: the agent may read different files, skip a changed file it never opened, cite a line that moved, or say a test proves something it doesn't.

Most of that work doesn't need an agent. Listing changed lines, tracking moves, finding callers and checking that a test exists are mechanical jobs, and a program does them the same way every time. So overview-me follows one rule: **if a tool can do the job, a tool does it. The agent only does what needs judgment.**

- **The tool gets the facts.** A small CLI reads git and parses the code with tree-sitter. It finds the changed lines, the moved and deleted files, and which functions call which. The same diff always gives the same map.
- **The agent writes the labels.** Claude adds what a program can't: the criteria, the captions, the groups and the reading order.
- **The tool checks the agent.** `overview-me validate` rejects the map if a changed line, deleted file or move doesn't belong to exactly one stop, or if a test cited as proof isn't a real test at that line. Claude fixes the map until it passes, and only then writes the overview.
- **Fixed rules decide the grouping.** Lanes, test paths and risk keywords come from `rules.json`, so a file lands in the same place on every run.
- **One map per commit.** The map is saved under the commit's SHA and reused, so asking again about the same commit gives the same structure.

The wording still changes between runs, but the facts don't. Nothing in the diff is skipped without a word, and every `path:line` points to real code.

## Update

In a terminal:

```sh
claude plugin marketplace update overview-me
claude plugin update overview-me@overview-me
```

The first command fetches the latest list of versions; the second installs the newest one. Then start a new Claude Code session: the new skills load, and the session downloads the CLI for the new version.

To update automatically, run `/plugin` in Claude Code, open the overview-me marketplace, and turn on auto-update.

## Contribute

Working on overview-me itself? See [CONTRIBUTING.md](CONTRIBUTING.md).
