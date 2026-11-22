# Page assets

`overview-me page` builds one self-contained HTML page from these files. `src/page.rs` compiles them into the binary.

- `overview.html`: the page shell, with `{{PLACEHOLDER}}` slots that `src/page.rs` fills.
- `styles/`: the stylesheet, one file per part of the page. They are joined into one `<style>` in the order `STYLES` in `src/page.rs` lists.
- `scripts/`: the page script, one file per feature. They are joined into one `<script type="module">` in the order `SCRIPTS` in `src/page.rs` lists.

## Rules

- Script files share one scope. Do not use `import` or `export`. A file can use what an earlier file declares.
- Code that runs at load must come after the `const` and `let` values it reads. Functions can be anywhere.
- CSS order matters when two rules have the same specificity. A later file wins.
- Put a part's phone rules (`@media (max-width: 640px)`) in that part's file.
- To add a file, add it to `STYLES` or `SCRIPTS` in `src/page.rs`.
