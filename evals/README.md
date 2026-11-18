# Trigger tests

Each case is one prompt and one check: which overview-me skill Claude picks, if any. They do not test what a skill writes.

- `pr-overview-*`, `change-overview-*`, `codebase-overview`: the named skill must be invoked.
- `no-*`: a review request ("review this PR", "find bugs in this diff"). No overview-me skill may be invoked.

Run them from the repo root:

```sh
claude plugin eval . --ablation none --tag trigger --model sonnet --no-publish
```

`--ablation none` matters: with the default no-plugin baseline arm, skill-call checks are reported but not scored. Each case runs 3 times and stops after 3 turns, so a skill that starts is cut off early; the check only needs the Skill call. A full run costs about $1–2.
