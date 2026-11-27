---
name: feature-overview
description: Write a one-page overview of one feature at the default branch, tracing its entry points through the code in a single flow. Use when the user asks for a "feature overview", "how does the X feature work", "map the X feature", or "explain feature X". Use pr-overview or change-overview for a PR, a diff or uncommitted work, and codebase-overview for the whole repo. This skill is not for a code review.
---

# Feature overview

Read `<root>/skills/codebase-overview/SKILL.md` first. Its ref, its store and its Rules apply, except the rules on the architecture diagram and modules: this page has one flow of stops instead.

`<root>` is the overview-me plugin root, `${CLAUDE_SKILL_DIR}/../..` in Claude Code and two folders above this SKILL.md in other agents.

## Steps

1. **Pick the ref.** Use the default branch and resolve `$REF` and `$SHA` as codebase-overview step 1 says.
2. **Pick the feature.** Use the slug of its row in `## Features` in `<store>/codebase.md`. If several rows match, ask, listing them. If none matches or there is no codebase overview, find the feature from the user's words and its entry points, and give it a kebab-case slug.
3. **Decide: new or update.** The page lives at `<store>/features/<slug>.md`. If it exists, decide from the `sha` on its second line as codebase-overview step 2 says; when current, go to step 7.
4. **Trace the feature.** Start from its entry points, checked at `$SHA`, and the row's Main flow if there is one, then follow calls through the lanes to the stores and outputs. Each stop is a function, handler or store.
5. **Write** `<store>/features/<slug>.md` in the format below.
6. **Check the gaps** as codebase-overview step 6 says.
7. **Show the page** as `<root>/references/show-page.md` says, for `<store>/features/<slug>.md`.
8. **Hand off.** Give the artifact link (or the HTML path), the markdown path and the one-paragraph summary. If there was no codebase overview, suggest `/codebase-overview`.

## Format

````markdown
# <Repo name> · <Feature> · feature overview
<REF> · sha <SHA full> · <date>

## In one paragraph
What the feature does and what the user gets.

## Entry points
- <kind: route, command, job, UI root> `<name>` → `<path:line>` (<lane>)

## Flow
```flow
<the stops on the feature's path, see rules>
```

## Walkthrough
1. <hop> `<path:line>` (<lane>)

## Core concepts
| Term | What it means | Defined at |
| --- | --- | --- |

## State
| Store | Reads | Writes | Defined at |
| --- | --- | --- | --- |

## Gaps
- <what you could not confirm>
````

## Rules

- Write one `flow` block as `<root>/references/flow-blocks.md` says for a feature overview. Put the numbered Walkthrough hops in the same order as the boxes.
- Reuse the lane names from the codebase overview. If there is no codebase overview, name the lanes as codebase-overview step 3 says.
- Keep Core concepts and State scoped to this feature.
