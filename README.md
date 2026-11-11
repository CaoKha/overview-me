# change-map

Review agent-written code in Neovim without reading the diff line by line. A change map links your spec to the code, groups the change by layer, and gives you a short reading order with a caption on every changed place.

## Install

```sh
cd /path/to/change-map   # wherever you cloned it
cargo build --release
mkdir -p ~/.claude/skills && ln -s "$PWD/skill" ~/.claude/skills/change-map
```

LazyVim (`~/.config/nvim/lua/plugins/change-map.lua`):

```lua
return {
  {
    dir = "/path/to/change-map", -- your clone
    cmd = { "ChangeMap", "ChangeMapPanel", "ChangeMapDiff" },
    keys = {
      { "<leader>rr", function() if require("change-map").load("") then require("change-map").panel() end end, desc = "Open change map (HEAD)" },
      { "<leader>rp", function() if require("change-map").ensure() then require("change-map").toggle_panel() end end, desc = "Toggle change map panel" },
      { "<leader>rd", function() if require("change-map").ensure() then require("change-map").diff() end end, desc = "Diff file against change base" },
      { "<leader>rs", function() if require("change-map").ensure() then require("change-map").panel({ focus = "input" }) end end, desc = "Search stops and criteria" },
      { "<leader>rn", function() require("change-map").stop(1) end, desc = "Next review stop" },
      { "<leader>rN", function() require("change-map").stop(-1) end, desc = "Previous review stop" },
    },
  },
  { "folke/which-key.nvim", optional = true, opts = { spec = { { "<leader>r", group = "review" } } } },
}
```

## Use

1. In Claude Code, with the PR head checked out, run **`/change-map`**, ideally in the session that wrote the code. Add the spec and the PR if it can't guess them: `/change-map PR 3389, spec docs/spec.md`. Plain words work too: "make a change map for this PR". The skill builds the map and validates it.
2. In nvim, in the same repo, press **`<leader>rr`**.

| Key | What |
| --- | --- |
| `<leader>rr` | Load the map for HEAD and open the panel |
| `<leader>rp` | Toggle the panel |
| `<leader>rs` | Search stops and criteria |
| `<leader>rn` / `<leader>rN` | Next / previous stop (same as `]q` / `[q`) |
| `]x` / `[x` | Next / previous place within the current stop |
| `<leader>rd` | Diff the current file against the PR base |

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
- Maps live in `$(git rev-parse --git-common-dir)/change-map/<head-sha>.json`: never committed, shared by all worktrees.
- Lanes, test paths and risk keywords are in `rules.json` (override with `--rules`).

## Limits

- Call edges use name matching, not the language server.
- Risk (read / skim) is keyword-based.
- Needs git 2.30 or later.
