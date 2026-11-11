# change-map

Review agent-written code in Neovim without reading the diff line by line. A change map links your spec to the code, groups the change by layer, and gives you a short reading order with a caption on every changed place.

## Install

```sh
cd /path/to/change-map   # wherever you cloned it
cargo build --release
mkdir -p ~/.claude/skills
ln -sfn "$PWD/skills/change-map" ~/.claude/skills/change-map
ln -sfn "$PWD/skills/feature-overview" ~/.claude/skills/feature-overview
ln -sfn "$PWD/skills/codebase-overview" ~/.claude/skills/codebase-overview
```

LazyVim (`~/.config/nvim/lua/plugins/change-map.lua`):

```lua
return {
  {
    dir = "/path/to/change-map", -- your clone
    cmd = { "ChangeMap", "ChangeMapPanel", "ChangeMapDiff", "ChangeMapOverview", "ChangeMapCodebase" },
    keys = {
      { "<leader>rr", function() if require("change-map").load("") then require("change-map").panel() end end, desc = "Open change map (HEAD)" },
      { "<leader>rp", function() if require("change-map").ensure() then require("change-map").toggle_panel() end end, desc = "Toggle change map panel" },
      { "<leader>rd", function() if require("change-map").ensure() then require("change-map").diff() end end, desc = "Diff file against change base" },
      { "<leader>ro", function() require("change-map").overview() end, desc = "Open feature overview" },
      { "<leader>rc", function() require("change-map").codebase() end, desc = "Open codebase overview" },
      { "<leader>rs", function() if require("change-map").ensure() then require("change-map").panel({ focus = "input" }) end end, desc = "Search stops and criteria" },
      { "<leader>rn", function() require("change-map").stop(1) end, desc = "Next review stop" },
      { "<leader>rN", function() require("change-map").stop(-1) end, desc = "Previous review stop" },
    },
  },
  { "folke/which-key.nvim", optional = true, opts = { spec = { { "<leader>r", group = "review" } } } },
}
```

## Use

0. Once per repo, run **`/codebase-overview`**. It maps the whole codebase at the default branch (modules, lanes, entry points, core concepts, main flows, where to find things), so a PR has a place to sit. Run it again when main has moved; it only updates what changed.
1. In Claude Code, with the PR head checked out, run **`/change-map`**, ideally in the session that wrote the code. Add the spec and the PR if it can't guess them: `/change-map PR 3389, spec docs/spec.md`. Plain words work too: "make a change map for this PR". The skill builds the map and validates it.
2. Optional: run **`/feature-overview`** for a one-page picture of the feature: entry points, runtime flow, data, the existing code it plugs into, and what to read first. It needs the map from step 1.
3. In nvim, in the same repo, press **`<leader>rr`**. Press **`<leader>ro`** for the overview; `gF` on a `path:line` in it jumps to the code.

| Key | What |
| --- | --- |
| `<leader>rr` | Load the map for HEAD and open the panel |
| `<leader>rp` | Toggle the panel |
| `<leader>rs` | Search stops and criteria |
| `<leader>rn` / `<leader>rN` | Next / previous stop (same as `]q` / `[q`) |
| `]x` / `[x` | Next / previous place within the current stop |
| `<leader>rd` | Diff the current file against the PR base |
| `<leader>ro` | Open the feature overview |
| `<leader>rc` | Open the codebase overview |

**Panel:** Spec (criteria ✓ tested, ⚠ untested or partial, ✗ missing) → Lanes (ui, api, services, db, engine, jobs) → Unexplained code. `<CR>` jumps to code or opens a folder, `h` closes it, `P` toggles preview. Deleted files show as `✗ deleted`, moved files as `→ moved from`.

## CLI

```sh
change-map extract --base <sha> [--spec spec.md]   # facts + draft map
change-map validate <map.json>                     # exit 1 on any problem
change-map render <map.json>                       # markdown + mermaid
change-map path                                    # where the map for HEAD lives
```

## How it works

- **The map is facts, the labels are help.** The binary computes the structure from git and tree-sitter. Claude writes criteria, captions, groups and the tour. `validate` rejects any label that disagrees with git: every changed line, deleted file and move must belong to exactly one stop, and every test cited as proof must be a real test at that line.
- Tests never become stops. A criterion counts as tested only when it names the tests that prove it.
- Maps live in `$(git rev-parse --git-common-dir)/change-map/<head-sha>.json`: never committed, shared by all worktrees. The feature overview is `<head-sha>.overview.md` beside it, and the codebase overview is `codebase.md`, one per repo.
- Lanes, test paths and risk keywords are in `rules.json` (override with `--rules`).

## Limits

- Call edges use name matching, not the language server.
- Risk (read / skim) is keyword-based.
- Needs git 2.30 or later.
