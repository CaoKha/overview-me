# Flow blocks

Every diagram in an overview is a ```` ```flow ```` block holding JSON. In a PR or change overview, `overview-me flow` draws both diagrams from the code and the page holds only `@before` and `@after`: do not write them by hand. To label an arrow or add a note to a box there, edit `<store>/<head>.flow.json` with the fields below. In a codebase or folder overview, you write the block. The page draws it as lanes stacked top to bottom, boxes in each lane, and curved arrows. Hovering or tabbing to a box highlights its arrows and the boxes at their other ends, so keep the graph honest and small enough to trace. `overview-me page` checks every block and lists what is wrong.

```json
{
  "nodes": {
    "<KEY A>": { "label": "<name>", "n": "<node id>", "kind": "added",
                 "note": "<one-line summary>", "paths": ["<path:start-end>"] },
    "<KEY B>": { "label": "<name>", "kind": "existing", "paths": ["<path:line>"] },
    "<KEY C>": { "label": "<table, cache or queue>", "kind": "existing store", "paths": ["<path:line>"] }
  },
  "tiers": [
    [{ "name": "<lane>", "ids": ["<KEY A>"] }],
    [{ "name": "<lane>", "ids": ["<KEY B>"] }],
    [{ "name": "<lane>", "ids": ["<KEY C>"] }]
  ],
  "edges": [["<KEY A>", "<KEY B>", "<label>", "added"], ["<KEY B>", "<KEY C>"]]
}
```

## Fields

- **`nodes`**: one box per key. The key is a short id used only inside the block.
  - `label`: a symbol or short name, at most about 25 characters, no paths. Required.
  - `n`: the change map node id of changed code, shown as a chip.
  - `kind`: space separated. `existing` (unchanged, dashed), `added` (new, green), `removed` (deleted, red and struck through), plus `store` for tables, caches and queues (rounded). No kind means changed code (solid).
  - `paths`: the files a reader opens for the box, as repo-relative `path:line`, `path:start-end` or `folder/` strings, main one first. Clicking the box lists them with copy buttons, so a developer can paste them into their editor's file search. Give every box its paths.
  - `note`: an optional one-line summary shown with the paths.
- **`tiers`**: rows, top to bottom in lane order (ui, api, services, db, engine, jobs, other). A row is a list of lanes `{ "name", "ids", "note"?, "flex"? }` side by side. `flex` is an optional CSS flex value, such as `"2"`, that makes one lane of a shared row wider. Put two lanes in a row only when two separate surfaces sit at the same level, such as two pages that start the flow. Every box sits in exactly one lane.
- **`edges`**: `[from, to, label?, style?]`. The label says in a few words what passes, what decides, or which transport carries it. `style` is `added` (new arrow, green), `removed` (arrow that goes away, red dashed) or `dashed` (async, fallback, cache or optional).

## By overview

| | PR or change overview | Codebase or folder overview |
| --- | --- | --- |
| A box is | one piece of code in the flow | one module (in a folder overview, one submodule or one outside module) |
| `kind` | `existing`, `added`, `removed`, each with or without `store` | `store` only |
| `n` | on changed code | never |
| `paths` | for changed code, the node's sites; in Before, at the base | the module's folder, then its entry points |
| Boxes | at most 3 per lane, since Before and After share the width | at most 4 per lane, 20 in all |
| Edges | in runtime order, at most about 15 | imports or runtime calls between modules |
| Edge `style` | `added`, `removed`, `dashed` | `dashed` only |

Over the limit, group boxes, or split a lane into two rows.
