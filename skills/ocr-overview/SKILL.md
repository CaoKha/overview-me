---
name: ocr-overview
description: Run a code review with OpenCodeReview (OCR, the `ocr` CLI) on a PR, a branch or uncommitted work, and show each finding on the box of the overview flow that holds its code, on the same page pr-overview makes plus a Findings layer. Use when the user asks to "show the bugs on the flow", "put the review findings on the map", "plot the OCR findings on the overview", an "OCR overview", or to "review this PR with OpenCodeReview and show it on the flow". Use it only when the user asks for findings on an overview, flow or map, or names OpenCodeReview or OCR. A plain request to review a PR or find bugs is not a request for this skill.
---

# OCR overview

The overview page of pr-overview (or change-overview), with the findings of an OpenCodeReview run on top. OCR does the review. overview-me only checks where each finding points and shows it: a badge on the flow box that holds its code, the list in the box's panel with the lines highlighted, and a Findings section in the page.

`<root>` is the overview-me plugin root: `${CLAUDE_SKILL_DIR}/../..` in Claude Code, and in other agents the folder two levels above the folder holding this SKILL.md. The steps run its CLI, `overview-me`. Read `<root>/references/cli.md` first: it says where the CLI is, what to do if it is missing, and what each command does.

Read `<root>/skills/pr-overview/SKILL.md` first. Its terms and writing rules apply as written.

## Terms

- **finding**: one comment of the review: a file, lines at head, a severity (critical, high, medium, low), a category and a text.
- **review file**: `<store>/<head>.ocr.json`, the review as OCR (or you, in delegate mode) wrote it.
- **findings file**: `<store>/<head>.findings.json`, written by `overview-me findings`. Each finding is on a stop, outside the change, or rejected.

## Rules

- A finding goes on a stop only when its lines are inside that node's sites. One outside every site is listed as "outside this change". One whose file, lines or quoted code are not at head is rejected. The CLI decides this, not you.
- A review of any commit other than the map's head is refused. Always review `<base>...<head>` with the same two commits as the map.
- Findings are not part of the map. Do not copy them into callouts, captions or criteria, and do not edit the Findings section by hand: run `overview-me render <map> --findings` again.
- `overview-me` checks where a finding points, not whether it is right. Say so at hand-off.

## Steps

1. **Map the change.** Follow pr-overview steps 1 to 3: find the base and head, draft the map, label it, and run `overview-me validate <map>` until it prints `ok`. For uncommitted work, follow change-overview steps 1 and 2 instead of pr-overview step 1: `<head>` is the commit `overview-me snapshot` prints. If the map for this head already validates, reuse it.
2. **Pick the OCR mode.**
   - Run `ocr --version`. If `ocr` is missing, tell the user it installs with `npm install -g @alibaba-group/open-code-review`, and stop. Do not install it without asking.
   - Run `ocr llm test`. If it exits 0, OCR has a model provider: use **native** mode. Otherwise use **delegate** mode. Tell the user which mode you use and why.
3. **Review.** Write the review file `<store>/<head>.ocr.json`.
   - **Native.** Run `ocr review --from <base> --to <head> --format json --audience agent --output <store>/<head>.ocr.json --background "<one paragraph: what the change is for, from the PR description or what the user said>"`. It can take many minutes: run it in the background and wait for it to finish. If it fails, show the user its error and stop.
   - **Delegate.** OCR picks the files and the rules; you review with your own model.
     1. Run `ocr delegate preview --format json --from <base> --to <head>`. It lists `reviewable_files` and the `merge_base`.
     2. Run `ocr delegate rule --format json --from <base> --to <head> <every reviewable path>`. It gives the rule checklist of each group of files.
     3. Make a checklist of every reviewable file. For each one, read its diff with `git --no-pager diff --no-ext-diff --no-textconv --no-color <merge_base>..<head> -- <path>`, review it against its group's rules, and read more code with `git show <head>:<path>` where you need it. Review the code, not the map: a caption says what the change means to do, not that it does it. Mark each file reviewed, or skipped with the reason.
     4. Keep a finding only when the code shows the problem. Drop likely false positives. Severity: critical and high for bugs, security problems and data loss; medium for performance, error handling and maintainability; low for style and small suggestions.
     5. Write the review file as below. `start_line` and `end_line` are lines of the file at `<head>`. `existing_code` is those lines, copied exactly from `git show <head>:<path>`.

        ```json
        {
          "source": "OpenCodeReview (delegate)",
          "head": "<head>",
          "comments": [
            {
              "path": "src/fee.rs",
              "start_line": 3,
              "end_line": 3,
              "existing_code": "    base - discount(amount)",
              "content": "What is wrong, and when it happens.",
              "suggestion_code": "    (base - discount(amount)).max(0)",
              "category": "bug",
              "severity": "high"
            }
          ]
        }
        ```

        `category` is one of bug, security, performance, maintainability, test, style, documentation, other. `suggestion_code` is optional.
4. **Place the findings.** Run `overview-me findings <map> <store>/<head>.ocr.json --head <head>`. It prints how many findings are on stops, outside the change and rejected, and why each was rejected.
   - If it says the review is of another commit, review again at `<head>`.
   - **Native:** do not edit OCR's output to make a finding pass. Report the rejected ones at hand-off.
   - **Delegate:** a rejected finding is your own mistake. Fix its lines or `existing_code` from `git show <head>:<path>`, then run `overview-me findings` again.
5. **Render.** Run `overview-me render <map> --findings`, with `--change --scope "<scope>"` for uncommitted work as change-overview says.
   - If the page `<store>/<head>.review.md` already exists, only its Findings section is written. The text you filled in before is kept.
   - If the page is new, fill its slots and check it as pr-overview steps 5 to 7 say.
   - Either way, do pr-overview step 7 (check the diagrams).
6. **Show the page** as `<root>/references/show-page.md` says, for `<store>/<head>.review.md`. For uncommitted work, pass `--unpushed`.
7. **Hand off.** Give:
   - the artifact link (or the HTML path), and the paths of the map, the review file and the findings file;
   - the mode (native with OCR's provider and model from the review file's `llm`, or delegate);
   - the count of findings by severity, the stops with critical or high findings, and the findings outside the change;
   - the rejected findings with their reasons;
   - coverage: in native mode, `summary.files_reviewed` and any `warnings` from the review file; in delegate mode, how many files you reviewed and skipped, with the reasons;
   - that overview-me checked where each finding points, not whether it is right;
   - that a badge on a box counts its findings (red for critical and high), and clicking the box lists them beside the code.
