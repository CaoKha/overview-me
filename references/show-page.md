# Show the page

The skill gives `<md>`, the overview markdown in `<store>`, and any extra flags for `overview-me page`. `<html>` is the same path with `.html` instead of `.md`.

1. Run `overview-me page <md> [flags]`. It writes `<html>`: a page with the look of a Claude artifact (cards, chips, light and dark themes), flow diagrams where hovering a box traces its links and clicking it lists its files with copy buttons, and every `path:line` linked to the forge. In a PR or change overview, the Before and After flows sit side by side on a wide screen.
2. If it exits 1, it lists what is wrong: an invalid `flow` block, a slot left to fill, a `path:line` that points at no line, a node id the map does not have, or an open question that cites nothing. Fix the markdown and run it again.
3. Check whether you run in Claude Code: you do if you have the Artifact tool.
   - **Claude Code:** publish `<html>` with the Artifact tool, icon `map`, and the summary's first sentence as `description`. Publish the file as it is: it already meets the artifact page rules (inline styles, light and dark tokens, Google Fonts, one pinned CDN script), so do not restyle or rewrite it. If you published it earlier in this session, publish the same path again so the link stays the same.
   - **Not Claude Code:** open `<html>` in the browser (`open` on macOS, `xdg-open` on Linux). It is the same page as the artifact. If the command is blocked or fails, give the user the exact command to run.
