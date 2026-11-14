---
name: change-overview
description: Write a one-page overview of uncommitted work, meaning the staged, unstaged and untracked files against HEAD (or, on request, together with the branch's commits), as the same page pr-overview makes. Use when the user asks for a "change overview", an "overview of my changes", to "map my changes", "what did I change", an "overview before I commit", or a guide to code you just wrote and have not committed. This is an overview, not a code review, so do not use it when the user asks to "review" their changes, look for bugs, or check them before committing, unless they also ask for an overview or map. For commits or a PR, use pr-overview.
---

# Change overview

`/pr-overview` for work that is not committed yet. The change is `<base>...<head>`, where `<base>` is HEAD and `<head>` is a snapshot of the files `git status` shows: staged, unstaged and untracked (not ignored). Commits are not part of it; use `/pr-overview` for those.

`<root>` is `${CLAUDE_SKILL_DIR}/../..`, the overview-me plugin root. The steps run its CLI, `overview-me`. Read `<root>/references/cli.md` first: it says where the CLI is, what to do if it is missing, and what each command does.

The steps are those of pr-overview. Read `<root>/skills/pr-overview/SKILL.md` first, and follow it with the changes below. Its format and rules apply as written.

## Steps

1. **Find the base.** This replaces pr-overview step 1.
   - By default the base is `git rev-parse HEAD`, so the page shows only uncommitted work. If there is no commit yet, say so and stop.
   - If the user asks to include the branch's commits too ("everything on this branch", "my commits and my changes"), the base is the merge-base with the default branch instead, found as in pr-overview step 1.
   - Run `git status --porcelain --untracked-files=all`. If it is empty, nothing is uncommitted: say so, suggest `/pr-overview` for the branch's commits, and stop.
2. **Snapshot the working tree.** Run `overview-me snapshot`. It prints `<head>`, a commit that holds HEAD plus every uncommitted and untracked file. It does not touch the branch, the index or the files. The same files always give the same sha, so a second run with no new edits reuses the map.
3. **Do pr-overview steps 2 to 8**, with these changes:
   - **Head.** Use `<head>` wherever pr-overview uses HEAD: `overview-me path --head <head>` instead of `overview-me path`, and `overview-me extract --base <base> --head <head>`. Read files at `<head>` (`git show <head>:<path>`); it equals the working tree.
   - **No PR.** Skip `gh pr view`. Uncommitted work has no message: take the criteria from a spec if the user names one, from what the user said they are working on, and from the diff.
   - **Title and header.** Use `# <Feature name> · change overview`, and the header line `<base 10>...<head 10> · <scope> · <stops> stops · <M> criteria · spec: <spec or "none">`, where `<scope>` is `uncommitted files`, or `branch and uncommitted files` when the base is the merge-base.
   - **Page.** Run `overview-me page <store>/<head>.review.md --unpushed`. The snapshot is not on the remote, so only `path:line` under Before link to the forge (at HEAD). If `git branch -r --contains HEAD` prints nothing, HEAD is not pushed either and those links will not resolve: say so at hand-off. Clicking a box still lists its files to copy.
   - **Hand off.** Also say how many files were included, and that the snapshot is not a commit on the branch: nothing was committed, staged or changed. If the base is HEAD and the branch has commits that are not on the default branch, say they were left out, and that asking to include the branch's commits puts them on the same page.
