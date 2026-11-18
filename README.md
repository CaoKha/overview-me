# overview-me

Review a PR with the code and its flow side by side.

overview-me is a Claude Code plugin. Ask Claude about a PR, your uncommitted changes or a whole codebase, and it writes a one-page overview you can click through.

![Click a box in the flow to read its code; click an eye icon to find that code in the flow](docs/images/demo.gif)

## Why I made this

Reviewing code is hard. It is harder when you don't know the codebase.

**The code is far from the explanation.** Ask Claude to explain a PR and you get paths to the code, not the code. To read each one, you open another browser tab, find the line, then come back. overview-me puts the code next to the explanation: click a box in the flow, or the eye icon next to any `path:line`, and the code opens in a side panel. No tab switching.

**A big PR doesn't fit in your head.** Halfway through, you forget where you came from and how the parts connect. The flow keeps the whole change on screen: how the code ran before, how it runs now, and which box the code you are reading belongs to.

**The facts must be right.** A CLI reads git and parses the code, so the same diff always gives the same map. Claude only writes the labels, and the CLI rejects the map until every changed line is in it and every cited test exists.

The [tutorial](docs/tutorial.md) walks through every feature with screenshots.

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
