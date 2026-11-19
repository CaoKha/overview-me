---
name: pr-overview
description: Write a one-page overview of a PR or branch diff, with a change map checked against git and an explanation of how the feature works, from its entry points and the flow before and after to what to read first. Use when the user asks for a "PR overview", a "feature overview", a "change map", to "map this PR" or "map this diff", an "overview of this PR", the "structure of this feature", or to "explain this change". This is an overview, not a code review, so do not use it when the user asks to "review" a PR or diff, look for bugs, or approve a change, unless they also ask for an overview or map. For uncommitted work, use change-overview.
---

# PR overview

Two parts. The change map says *what changed*: every changed line belongs to one captioned stop, each claim names the tests that prove it, and `overview-me validate` rejects any label that disagrees with git. The overview page says *how the feature works*: how it is triggered, what runs before and after, what data it touches, and what to read first.

The CLI does the mechanical work. It finds the base, drafts the map, draws the Before and After flows from the code, and writes the page with every section that the map and the code can give. You write only what needs judgement: the labels in the map, and the text in the page's `<!-- fill: … -->` slots.

`<root>` is the overview-me plugin root: `${CLAUDE_SKILL_DIR}/../..` in Claude Code, and in other agents the folder two levels above the folder holding this SKILL.md. The steps run its CLI, `overview-me`. Read `<root>/references/cli.md` first: it says where the CLI is, what to do if it is missing, and what each command does.

## Terms

- **base**, **head**: the two commits the change sits between. `base...head` is the diff.
- **`<store>`**: `$(dirname "$(overview-me path)")`, the folder that holds maps and pages. It is never committed and is shared by all worktrees.
- **change map** (or map): the JSON file `<store>/<head>.json` that lists the criteria and nodes of one change.
- **criterion**: one thing the change claims to do, with its status and the tests that prove it.
- **node**: one changed place in the map, with an id like `N3`. The page calls a node a **stop**.
- **site**: a range of changed lines in a node, on the head side. The first site is the main site.
- **tour**: every node id once, in reading order.
- **slot**: a `<!-- fill: <hint> -->` in the page. Replace it with the text the hint asks for.

## Steps

1. **Find the change.** Run `overview-me base [<PR number or link>]`. Pass a PR only when the user names one. It prints `base`, `head`, `mode` and, for a PR, `pr`. A PR that is not checked out is fetched, never checked out.
   - `mode uncommitted`: the work is not committed. Tell the user, and follow `<root>/skills/change-overview/SKILL.md` instead.
   - If it fails, or the user names another base (for example the PR below a stacked PR), use the base the user gives, or ask.
2. **Draft the map.** Run `overview-me extract --base <base> --head <head> [--spec <spec file>]`. It writes the draft map to `<store>/<head>.json` and prints:
   - the codebase overview, if one exists. Read it, and use its module names and terms;
   - every draft node with its diff, and the tests in the changed test files.
   
   The draft already folds import-only hunks into their function, groups the same edit at several places, and sets the tour. If it says it kept a map for this base, go to step 4, unless the user asked to rebuild it (`--fresh`).
3. **Label the map** as `<root>/references/change-map.md` says: criteria and their tests, a caption for every node, lane names, callouts, and any grouping the draft missed. Take the criteria from the PR description (`gh pr view <pr> --json body`) and the spec. Run `overview-me validate <map>` until it prints `ok`.
4. **Render.** Run `overview-me render <map>`. It validates the map, draws the flows into `<store>/<head>.flow.json`, and writes `<store>/<head>.review.md`. It prints the entry points, the unchanged code that calls or is called by the change, and what it could not draw.
5. **Fill the slots** in `<store>/<head>.review.md`. Read code only where a slot needs it:
   - **Entry points:** search for each symbol's registration to find what reaches it: a route, command, job, event or UI action.
   - **Steps:** read the cited lines. Before steps cite the base: read them with `git show <base>:<path>`.
   - **Data and state:** the types, tables, columns, config keys and messages that the nodes add, change or remove.
   - **Where it plugs in:** fill the last cell of each row. Under "Affected by the change", delete a row when nothing changes for that code, for example a caller that only calls new code. Under "Used by the change", delete a row when the change does not use it, for example a sibling the changed lines only list. Delete a table and its heading when you delete its last row. If both go, write "None." under the section heading.
   
   Leave the rest of the page as rendered. Do not redraw the flows, rewrite the tables, or repeat the map in prose. To label an arrow or add a note to a box, edit `<store>/<head>.flow.json`. To remove a wrong arrow between two stops, delete its edge from the map's `edges` and run `overview-me flow <map>` before you label arrows, since it rewrites the flow file.
6. **Check your open questions.** The questions the page already lists come from facts. Test each question you add against the code before you keep it:
   - Look for the answer: read the code it cites, `git grep -n` its symbols at `<head>` and at `<base>`, and read the tests, the PR description, the spec and the codebase overview.
   - If you find the answer, drop the question. If the answer helps the reader, put it in the section it belongs to.
   - If the question says the code looks wrong, read the cited code again with its callers and callees. Keep it only if the problem is still there, and say exactly what the code does.
   - Keep a question only when the code in this repo cannot answer it: a value set outside the repo, an intent the PR does not state, or behaviour only a run shows. Each one cites the `path:line` it is about.
7. **Show the page** as `<root>/references/show-page.md` says, for `<store>/<head>.review.md`.
8. **Hand off.** Give the artifact link (or the HTML path), the markdown and map paths, the stop count, the one-paragraph summary, the Watch out callouts, and the gaps (missing, partial or untested criteria, and open questions). Tell the user that clicking a box in a flow diagram lists its files to copy into their editor.

## Writing rules

- Use simple technical English. No marketing words.
- Only state what the code shows. If you could not confirm something, put it under Open questions instead of guessing.
- Cite repo-relative `path:line` for every claim about code, and node ids (`N3`) for changed code. Say "new", "changed" or "existing" when it is not obvious.
- A slot takes one phrase or sentence, except "In one paragraph" (3 to 5 sentences) and "Data and state" (a table, or "None.").
- Once the page is written from the map, do not change the map to fit the page. If the map looks wrong, fix it, validate it, and run `overview-me render <map> --fresh`.
