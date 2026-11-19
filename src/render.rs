//! `overview-me render`: the PR or change overview page, written from the map and the flow, with
//! `<!-- fill: … -->` slots where only a reader of the code can write the text.
//!
//! Everything the map or the code already says is written here: the header, entry points, both
//! flow diagrams, the step list, where the change plugs in, claims and proof, what to read
//! first, the callouts and the open questions that facts raise. `overview-me page` refuses a
//! page with a slot left.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::facts::{self, Changes};
use crate::flow::{self, FlowFile};
use crate::git::Git;
use crate::map::{CalloutTag, ChangeMap, Node, Risk, Status};
use crate::rules::Rules;

/// Steps under Before: only the parts the change touches.
const MAX_BEFORE_STEPS: usize = 4;
/// Stops under Read first.
const MAX_READ_FIRST: usize = 5;

/// The marker `overview-me page` looks for.
pub const SLOT: &str = "<!-- fill:";

fn slot(hint: &str) -> String {
    format!("{SLOT} {hint} -->")
}

/// Which page to write.
pub struct Kind<'a> {
    /// `PR overview` or `change overview`.
    pub title: &'a str,
    /// Shown in the header line after the shas, such as `uncommitted files`.
    pub scope: Option<&'a str>,
}

/// Validates the map, writes the flow file and `<store>/<head>.review.md` (or `out`), and returns
/// the page path. Refuses to overwrite a page unless `fresh`.
///
/// # Errors
/// When the map does not validate, the page exists, or git fails.
pub fn run(
    git: &Git,
    rules: &Rules,
    map_path: &Path,
    kind: &Kind<'_>,
    out: Option<PathBuf>,
    fresh: bool,
) -> Result<PathBuf> {
    if !crate::validate::run(git, rules, map_path)? {
        bail!("the map has problems (listed above): fix them before rendering");
    }
    let (_, map, flow) = flow::run_flow(git, rules, map_path, None)?;
    let out = out.unwrap_or_else(|| map_path.with_file_name(format!("{}.review.md", map.head)));
    if out.exists() && !fresh {
        bail!(
            "{} exists: fill it in, or pass --fresh to write it again (what was filled in is lost)",
            out.display()
        );
    }
    let changes = facts::changes(git, rules, &map.base, &map.head)?;
    let md = page(&map, &flow, &changes, kind);
    std::fs::write(&out, &md).with_context(|| format!("writing {}", out.display()))?;
    println!("page:  {}", out.display());
    println!(
        "slots: {} to fill (search for \"{SLOT}\")",
        md.matches(SLOT).count()
    );
    Ok(out)
}

/// `file:start-end`, or `file:line` when both ends are the same line.
fn site(file: &str, start: u32, end: u32) -> String {
    if start == end {
        format!("{file}:{start}")
    } else {
        format!("{file}:{start}-{end}")
    }
}

/// The node's main place: its main site, or the file it deletes or moves.
fn main_place(n: &Node) -> String {
    n.sites
        .first()
        .map(|s| site(&s.file, s.start, s.end))
        .or_else(|| n.deletions.first().map(|d| d.file.clone()))
        .or_else(|| n.renames.first().map(|r| r.to.clone()))
        .unwrap_or_default()
}

fn first_path(spec: &Value, key: &str) -> String {
    spec["nodes"][key]["paths"][0]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// Boxes for map nodes in runtime order: breadth first from the entry points along the arrows,
/// then the rest in tour order.
fn runtime_order(spec: &Value, entries: &[String], tour: &[String]) -> Vec<String> {
    let nodes = spec["nodes"].as_object();
    let is_node = |k: &str| nodes.is_some_and(|m| m.get(k).is_some_and(|b| b["n"] == k));
    let mut next: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in spec["edges"].as_array().into_iter().flatten() {
        if let (Some(a), Some(b)) = (e[0].as_str(), e[1].as_str()) {
            next.entry(a).or_default().push(b);
        }
    }
    let mut order: Vec<String> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut queue: VecDeque<&str> = entries.iter().map(String::as_str).collect();
    while let Some(k) = queue.pop_front() {
        if !seen.insert(k) {
            continue;
        }
        if is_node(k) {
            order.push(k.to_owned());
        }
        queue.extend(next.get(k).into_iter().flatten());
    }
    for id in tour {
        if is_node(id) && !order.contains(id) {
            order.push(id.clone());
        }
    }
    order
}

fn tag(t: CalloutTag) -> &'static str {
    match t {
        CalloutTag::Breaking => "Breaking",
        CalloutTag::Security => "Security",
        CalloutTag::Migration => "Migration",
        CalloutTag::Race => "Race",
        CalloutTag::Perf => "Perf",
        CalloutTag::Subtle => "Subtle",
    }
}

/// The page markdown.
pub fn page(map: &ChangeMap, flow: &FlowFile, changes: &Changes, kind: &Kind<'_>) -> String {
    let node = |id: &str| map.nodes.iter().find(|n| n.id == id);
    let mut md = String::new();
    let w = &mut md;

    let _ = writeln!(w, "# {} · {}\n", slot("feature name"), kind.title);
    let scope = kind.scope.map(|s| format!(" · {s}")).unwrap_or_default();
    let _ = writeln!(
        w,
        "{}...{}{scope} · {} stops · {} criteria · spec: {}\n",
        &map.base[..10],
        &map.head[..10],
        map.nodes.len(),
        map.criteria.len(),
        map.spec.as_deref().unwrap_or("none")
    );

    let _ = writeln!(w, "## In one paragraph");
    let _ = writeln!(
        w,
        "{}\n",
        slot(
            "what the feature does for its user, and the approach the code takes, in 3 to 5 sentences"
        )
    );
    let _ = writeln!(w, "## Where it sits");
    let _ = writeln!(
        w,
        "{}\n",
        slot(
            "modules touched, as `module (lane)` from the codebase overview, and which of its main flows this joins; or \"No codebase overview.\""
        )
    );

    let _ = writeln!(w, "## Entry points");
    let mut entries: Vec<String> = Vec::new();
    for e in &flow.entry_points {
        let (id, label) = e.symbol.split_once(' ').unwrap_or((&e.symbol, ""));
        entries.push(id.to_owned());
        let _ = writeln!(
            w,
            "- {} → `{label}` `{}` ({id})",
            slot("the route, command, job, event or UI action that reaches it"),
            e.at
        );
    }
    if flow.entry_points.is_empty() {
        let _ = writeln!(w, "None.");
    }
    let _ = writeln!(w);

    let _ = writeln!(w, "## Flow");
    let after_order = runtime_order(&flow.after, &entries, &map.tour);
    match &flow.before {
        Some(before) => {
            let _ = writeln!(
                w,
                "### Before: {}",
                slot("how it worked at base, in one line")
            );
            let _ = writeln!(w, "```flow\n@before\n```\n");
            let boxes = before["nodes"].as_object();
            let mut keys: Vec<String> = after_order
                .iter()
                .filter(|k| boxes.is_some_and(|m| m.contains_key(k.as_str())))
                .cloned()
                .collect();
            for (k, b) in boxes.into_iter().flatten() {
                let removed = b["kind"].as_str().is_some_and(|s| s.contains("removed"));
                if (removed || b["n"].is_string()) && !keys.contains(k) {
                    keys.push(k.clone());
                }
            }
            for (i, k) in keys.iter().take(MAX_BEFORE_STEPS).enumerate() {
                let b = &before["nodes"][k.as_str()];
                let who = b["n"]
                    .as_str()
                    .map_or_else(|| "base".to_owned(), |n| format!("base, {n}"));
                let path = first_path(before, k);
                let what = if b["kind"].as_str().is_some_and(|s| s.contains("removed")) {
                    format!(
                        "what `{}` did (removed by this change)",
                        b["label"].as_str().unwrap_or(k)
                    )
                } else {
                    "what this step did at base".to_owned()
                };
                let _ = writeln!(w, "{}. {} `{path}` ({who})", i + 1, slot(&what));
            }
            let _ = writeln!(w);
        }
        None => {
            let _ = writeln!(w, "### Before: none");
            let _ = writeln!(
                w,
                "None. This flow is new. {}\n",
                slot("what existing code it attaches to")
            );
        }
    }
    let _ = writeln!(w, "### After: {}", slot("how it works now, in one line"));
    let _ = writeln!(w, "```flow\n@after\n```\n");
    for (i, k) in after_order.iter().enumerate() {
        let _ = writeln!(
            w,
            "{}. {} `{}` ({k})",
            i + 1,
            slot("what this step does"),
            first_path(&flow.after, k)
        );
    }
    let _ = writeln!(w);

    let _ = writeln!(w, "## Data and state");
    let _ = writeln!(
        w,
        "{}\n",
        slot(
            "a table `| Thing | Kind | Written by | Read by |` with one row per type, table, column, cache key, env var or message the change adds, changes or removes (say which), citing node ids and `path:line`; or \"None.\""
        )
    );

    let _ = writeln!(w, "## Where it plugs in");
    if flow.callers.is_empty() && flow.depends_on.is_empty() {
        let _ = writeln!(w, "None.");
    }
    if !flow.callers.is_empty() {
        let _ = writeln!(w, "### Affected by the change");
        let _ = writeln!(w, "| Code | Calls | What changes for it |");
        let _ = writeln!(w, "| --- | --- | --- |");
        for l in &flow.callers {
            let _ = writeln!(
                w,
                "| `{}` `{}` | {} | {} |",
                l.symbol.replace('|', "\\|"),
                l.at,
                l.nodes.join(", "),
                slot("what is now different for it, or delete the row when nothing changes for it")
            );
        }
    }
    if !flow.depends_on.is_empty() {
        if !flow.callers.is_empty() {
            let _ = writeln!(w);
        }
        let _ = writeln!(w, "### Used by the change");
        let _ = writeln!(w, "| Code | Used by | What for |");
        let _ = writeln!(w, "| --- | --- | --- |");
        for l in &flow.depends_on {
            let _ = writeln!(
                w,
                "| `{}` `{}` | {} | {} |",
                l.symbol.replace('|', "\\|"),
                l.at,
                l.nodes.join(", "),
                slot(
                    "what the change uses it for, or delete the row when the change does not use it"
                )
            );
        }
    }
    let _ = writeln!(w);

    let _ = writeln!(w, "## Claims and proof");
    if map.criteria.is_empty() {
        let _ = writeln!(w, "None.\n");
    } else {
        let _ = writeln!(w, "| Criterion | Status | Code | Tests |");
        let _ = writeln!(w, "| --- | --- | --- | --- |");
        for c in &map.criteria {
            let status = match (c.status, c.tests.is_empty()) {
                (Status::Implemented, false) => "✓ implemented",
                (Status::Implemented, true) => "⚠ untested",
                (Status::Partial, _) => "⚠ partial",
                (Status::Missing, _) => "✗ missing",
            };
            let code: Vec<&str> = map
                .nodes
                .iter()
                .filter(|n| n.criteria.contains(&c.id))
                .map(|n| n.id.as_str())
                .collect();
            let code = if code.is_empty() {
                "none".to_owned()
            } else {
                code.join(", ")
            };
            let _ = writeln!(
                w,
                "| {} {} | {status} | {code} | {} |",
                c.id,
                c.text.replace('|', "\\|"),
                c.tests.len()
            );
        }
        let _ = writeln!(w);
    }

    let _ = writeln!(w, "## Read first");
    let mut first: Vec<&Node> = map
        .tour
        .iter()
        .filter_map(|id| node(id))
        .filter(|n| n.risk == Risk::Read)
        .take(MAX_READ_FIRST)
        .collect();
    if first.is_empty() {
        first = map.tour.iter().filter_map(|id| node(id)).take(3).collect();
    }
    for (i, n) in first.iter().enumerate() {
        let _ = writeln!(
            w,
            "{}. {} `{}`: {}",
            i + 1,
            n.id,
            main_place(n),
            slot("why this one first: the decision it holds")
        );
    }
    let skim: Vec<&str> = map
        .tour
        .iter()
        .filter(|id| !first.iter().any(|n| &n.id == *id))
        .map(String::as_str)
        .collect();
    if !skim.is_empty() {
        let _ = writeln!(w, "\nSkim: {}.", skim.join(", "));
    }
    let _ = writeln!(w);

    let _ = writeln!(w, "## Watch out");
    for c in &map.callouts {
        let _ = writeln!(
            w,
            "- **{}** {} `{}:{}`: {}",
            tag(c.tag),
            c.node,
            c.file,
            c.line,
            c.note
        );
    }
    if map.callouts.is_empty() {
        let _ = writeln!(w, "None.");
    }
    let _ = writeln!(w);

    let _ = writeln!(w, "## Open questions");
    let facts = fact_questions(map, flow, changes);
    for q in &facts {
        let _ = writeln!(w, "- {q}");
    }
    let none = if facts.is_empty() {
        "write \"None.\" if there is none"
    } else {
        "delete this line if there is none"
    };
    let _ = writeln!(
        w,
        "- {}",
        slot(&format!(
            "a question the code cannot answer (a value set outside the repo, intent the PR does not state, behaviour only a run shows), citing `path:line`; one line each; {none}"
        ))
    );
    md
}

/// Open questions that the map and the code raise by themselves: each is a fact, so it is never
/// wrong, and each cites where it comes from.
pub fn fact_questions(map: &ChangeMap, flow: &FlowFile, changes: &Changes) -> Vec<String> {
    let mut out = Vec::new();
    let linked = |id: &str| -> Vec<&Node> {
        map.nodes
            .iter()
            .filter(|n| n.criteria.iter().any(|c| c == id))
            .collect()
    };
    for c in &map.criteria {
        let code = linked(&c.id)
            .first()
            .map(|n| format!(" Its code: {} `{}`.", n.id, main_place(n)))
            .unwrap_or_default();
        match (c.status, c.tests.is_empty()) {
            (Status::Implemented, true) => out.push(format!(
                "{} ({}) has no test that checks it.{code}",
                c.id, c.text
            )),
            (Status::Partial, _) => out.push(format!(
                "{} ({}) is partial: part of its logic lives outside this diff.{code}",
                c.id, c.text
            )),
            (Status::Missing, _) => out.push(format!(
                "{} ({}) is claimed, but no code in the diff does it.",
                c.id, c.text
            )),
            _ => {}
        }
    }
    let proven: HashSet<&str> = map
        .criteria
        .iter()
        .filter(|c| !c.tests.is_empty())
        .map(|c| c.id.as_str())
        .collect();
    for n in &map.nodes {
        if n.risk != Risk::Read
            || n.touched_by_test
            || n.criteria.iter().any(|c| proven.contains(c.as_str()))
        {
            continue;
        }
        let why = changes
            .units
            .iter()
            .find(|u| {
                n.sites
                    .iter()
                    .any(|s| s.file == u.file && s.start <= u.start && u.end <= s.end)
                    && u.risk == Risk::Read
            })
            .map(|u| format!(" ({})", u.risk_why))
            .unwrap_or_default();
        out.push(format!(
            "{} `{}` is marked read{why}, and no changed test references it.",
            n.id,
            main_place(n)
        ));
    }
    for l in &flow.still_referenced {
        out.push(format!(
            "`{}` is removed, but `{}` still references it and no symbol of that name is left.",
            l.symbol, l.at
        ));
    }
    for s in &changes.skipped {
        out.push(format!(
            "`{}` was skipped ({}), so this overview does not cover it.",
            s.file, s.reason
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Callout, Criterion, Site, TestRef};
    use serde_json::json;

    const BASE: &str = "0123456789abcdef0123456789abcdef01234567";
    const HEAD: &str = "89abcdef0123456789abcdef0123456789abcdef";

    fn node(id: &str, risk: Risk, criteria: &[&str], file: &str, start: u32) -> Node {
        Node {
            id: id.into(),
            symbol: id.to_lowercase(),
            lane: "services".into(),
            risk,
            caption: "does it".into(),
            criteria: criteria.iter().map(|c| (*c).into()).collect(),
            touched_by_test: false,
            sites: vec![Site {
                file: file.into(),
                start,
                end: start + 2,
            }],
            deletions: Vec::new(),
            renames: Vec::new(),
        }
    }

    fn fixture() -> (ChangeMap, FlowFile) {
        let map = ChangeMap {
            version: 1,
            base: BASE.into(),
            head: HEAD.into(),
            spec: None,
            criteria: vec![
                Criterion {
                    id: "REF-1".into(),
                    text: "charges the fee".into(),
                    spec_ref: None,
                    status: Status::Implemented,
                    tests: vec![TestRef {
                        file: "a.test.ts".into(),
                        line: 2,
                        name: "fee".into(),
                    }],
                },
                Criterion {
                    id: "REF-2".into(),
                    text: "gives a discount".into(),
                    spec_ref: None,
                    status: Status::Implemented,
                    tests: Vec::new(),
                },
            ],
            nodes: vec![
                node("N1", Risk::Read, &["REF-1"], "src/api/h.ts", 4),
                node("N2", Risk::Read, &["REF-2"], "src/services/d.ts", 5),
                node("N3", Risk::Skim, &[], "src/services/f.ts", 1),
            ],
            edges: Vec::new(),
            tour: vec!["N3".into(), "N2".into(), "N1".into()],
            callouts: vec![Callout {
                tag: CalloutTag::Breaking,
                node: "N2".into(),
                file: "src/services/d.ts".into(),
                line: 6,
                note: "returns a negative fee for x < 1".into(),
            }],
        };
        let after = json!({
            "nodes": {
                "N1": {"label": "handle", "n": "N1", "paths": ["src/api/h.ts:4-6"]},
                "N2": {"label": "discount", "n": "N2", "kind": "added", "paths": ["src/services/d.ts:5-7"]},
                "N3": {"label": "fee", "n": "N3", "paths": ["src/services/f.ts:1-3"]},
                "x1": {"label": "save", "kind": "existing store", "paths": ["src/db/s.ts:1-3"]}
            },
            "tiers": [],
            "edges": [["N1", "N3"], ["N1", "N2", null, "added"], ["N1", "x1"]]
        });
        let before = json!({
            "nodes": {
                "N1": {"label": "handle", "n": "N1", "paths": ["src/api/h.ts:4-6"]},
                "N3": {"label": "fee", "n": "N3", "paths": ["src/services/f.ts:1-3"]},
                "r1": {"label": "oldFee", "kind": "removed", "paths": ["src/services/f.ts:5-7"]}
            },
            "tiers": [],
            "edges": []
        });
        let flow = FlowFile {
            before: Some(before),
            after,
            entry_points: vec![flow::Link {
                symbol: "N1 handle".into(),
                at: "src/api/h.ts:4-6".into(),
                nodes: Vec::new(),
            }],
            callers: vec![
                flow::Link {
                    symbol: "save".into(),
                    at: "src/db/s.ts:1-3".into(),
                    nodes: vec!["N3".into()],
                },
                flow::Link {
                    symbol: "cron".into(),
                    at: "src/jobs/cron.ts:2-4".into(),
                    nodes: vec!["N1".into()],
                },
            ],
            depends_on: vec![flow::Link {
                symbol: "save".into(),
                at: "src/db/s.ts:1-3".into(),
                nodes: vec!["N1".into()],
            }],
            still_referenced: vec![flow::Link {
                symbol: "oldFee".into(),
                at: "src/jobs/run.ts:9".into(),
                nodes: Vec::new(),
            }],
            not_drawn: Vec::new(),
        };
        (map, flow)
    }

    /// The whole page, so a change to the layout shows up as a diff here.
    #[test]
    fn page_snapshot() {
        let (map, flow) = fixture();
        let kind = Kind {
            title: "PR overview",
            scope: None,
        };
        let md = page(&map, &flow, &Changes::default(), &kind);
        let want = r#"# <!-- fill: feature name --> · PR overview

0123456789...89abcdef01 · 3 stops · 2 criteria · spec: none

## In one paragraph
<!-- fill: what the feature does for its user, and the approach the code takes, in 3 to 5 sentences -->

## Where it sits
<!-- fill: modules touched, as `module (lane)` from the codebase overview, and which of its main flows this joins; or "No codebase overview." -->

## Entry points
- <!-- fill: the route, command, job, event or UI action that reaches it --> → `handle` `src/api/h.ts:4-6` (N1)

## Flow
### Before: <!-- fill: how it worked at base, in one line -->
```flow
@before
```

1. <!-- fill: what this step did at base --> `src/api/h.ts:4-6` (base, N1)
2. <!-- fill: what this step did at base --> `src/services/f.ts:1-3` (base, N3)
3. <!-- fill: what `oldFee` did (removed by this change) --> `src/services/f.ts:5-7` (base)

### After: <!-- fill: how it works now, in one line -->
```flow
@after
```

1. <!-- fill: what this step does --> `src/api/h.ts:4-6` (N1)
2. <!-- fill: what this step does --> `src/services/f.ts:1-3` (N3)
3. <!-- fill: what this step does --> `src/services/d.ts:5-7` (N2)

## Data and state
<!-- fill: a table `| Thing | Kind | Written by | Read by |` with one row per type, table, column, cache key, env var or message the change adds, changes or removes (say which), citing node ids and `path:line`; or "None." -->

## Where it plugs in
### Affected by the change
| Code | Calls | What changes for it |
| --- | --- | --- |
| `save` `src/db/s.ts:1-3` | N3 | <!-- fill: what is now different for it, or delete the row when nothing changes for it --> |
| `cron` `src/jobs/cron.ts:2-4` | N1 | <!-- fill: what is now different for it, or delete the row when nothing changes for it --> |

### Used by the change
| Code | Used by | What for |
| --- | --- | --- |
| `save` `src/db/s.ts:1-3` | N1 | <!-- fill: what the change uses it for, or delete the row when the change does not use it --> |

## Claims and proof
| Criterion | Status | Code | Tests |
| --- | --- | --- | --- |
| REF-1 charges the fee | ✓ implemented | N1 | 1 |
| REF-2 gives a discount | ⚠ untested | N2 | 0 |

## Read first
1. N2 `src/services/d.ts:5-7`: <!-- fill: why this one first: the decision it holds -->
2. N1 `src/api/h.ts:4-6`: <!-- fill: why this one first: the decision it holds -->

Skim: N3.

## Watch out
- **Breaking** N2 `src/services/d.ts:6`: returns a negative fee for x < 1

## Open questions
- REF-2 (gives a discount) has no test that checks it. Its code: N2 `src/services/d.ts:5-7`.
- N2 `src/services/d.ts:5-7` is marked read, and no changed test references it.
- `oldFee` is removed, but `src/jobs/run.ts:9` still references it and no symbol of that name is left.
- <!-- fill: a question the code cannot answer (a value set outside the repo, intent the PR does not state, behaviour only a run shows), citing `path:line`; one line each; delete this line if there is none -->
"#;
        assert_eq!(md, want, "\n--- got ---\n{md}");
    }

    #[test]
    fn a_new_flow_says_so() {
        let (map, mut flow) = fixture();
        flow.before = None;
        let kind = Kind {
            title: "change overview",
            scope: Some("uncommitted files"),
        };
        let md = page(&map, &flow, &Changes::default(), &kind);
        assert!(md.contains("· change overview\n"));
        assert!(md.contains("89abcdef01 · uncommitted files · 3 stops"));
        assert!(md.contains("### Before: none\nNone. This flow is new."));
        assert!(!md.contains("@before"));
    }
}
