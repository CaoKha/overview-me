---
name: change-overview
description: Review your uncommitted changes on one page, meaning the staged, unstaged and untracked files that `git status` shows, against HEAD. Gives the same result as /pr-overview, a change map checked against git and then entry points, the flow before and after, data, where it plugs into existing code, and what to read first. Use when the user asks for a "change overview", to "review my changes", "review my local changes", "review uncommitted changes", "what did I change", or for an "overview before I commit".
---

# Change overview

`/pr-overview` for work that is not committed yet. The change is `<base>...<head>`, where `<base>` is HEAD and `<head>` is a snapshot of the files `git status` shows: staged, unstaged and untracked (not ignored). Commits are not part of it; use `/pr-overview` for those.

`${CLAUDE_SKILL_DIR}/../..` is the overview-me plugin root. The steps run its CLI, `${CLAUDE_SKILL_DIR}/../../bin/overview-me` (called `overview-me` below), which the plugin installs when a session starts. If it is missing, run `bash "${CLAUDE_SKILL_DIR}/../../scripts/ensure-cli.sh"` once: it downloads the release for this machine or builds it from source. If it is still missing, tell the user what the script printed and stop. Run the CLI from the root of the repo under review.

The steps are those of pr-overview. Read `${CLAUDE_SKILL_DIR}/../pr-overview/SKILL.md` first, and follow it with the changes below. Its format and rules apply as written.

## Steps

1. **Find the base.** This replaces pr-overview step 1.
   - The base is `git rev-parse HEAD`. If there is no commit yet, say so and stop.
   - Run `git status --porcelain --untracked-files=all`. If it is empty, say there is nothing to review and stop.
2. **Snapshot the working tree.** Run `overview-me snapshot`. It prints `<head>`, a commit that holds HEAD plus every uncommitted and untracked file. It does not touch the branch, the index or the files. The same files always give the same sha, so a second run with no new edits reuses the map.
3. **Do pr-overview steps 2 to 8**, with these changes:
   - **Head.** Use `<head>` wherever pr-overview uses HEAD: `overview-me path --head <head>` instead of `overview-me path`, and `overview-me extract --base <base> --head <head>`. Read files at `<head>` (`git show <head>:<path>`); it equals the working tree.
   - **No PR.** Skip `gh pr view`. Uncommitted work has no message: take the criteria from a spec if the user names one, from what the user said they are working on, and from the diff.
   - **Title and header.** Use `# <Feature name> · change overview`, and the header line `<base 10>...<head 10> · uncommitted files · <stops> stops · <M> criteria · spec: <spec or "none">`.
   - **Page.** Run `overview-me page <store>/<head>.review.md --unpushed`. The snapshot is not on the remote, so only `path:line` under Before link to the forge (at HEAD). If `git branch -r --contains HEAD` prints nothing, HEAD is not pushed either and those links will not resolve: say so at hand-off. Clicking a box still lists its files to copy.
   - **Hand off.** Also say how many files were included, and that the snapshot is not a commit on the branch: nothing was committed, staged or changed.
