//! `overview-me render`: a Markdown review guide with a flow diagram of the map, next to the
//! JSON. `overview-me page` turns it into the HTML page.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::map::{ChangeMap, Criterion, Lane, Status};

/// Writes `<map>.md` and returns its path.
///
/// # Errors
/// When the map cannot be read or parsed, or the file cannot be written.
pub fn run(path: &Path) -> Result<PathBuf> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let map: ChangeMap =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let out = path.with_extension("md");
    std::fs::write(&out, markdown(&map)).with_context(|| format!("writing {}", out.display()))?;
    Ok(out)
}

fn mark(c: &Criterion) -> &'static str {
    match c.status {
        Status::Missing => "✗",
        Status::Implemented if c.tested() => "✓",
        _ => "⚠",
    }
}

fn short(sha: &str) -> &str {
    sha.get(..10).unwrap_or(sha)
}

/// `file:start` or `file:start-end`.
fn site_ref(file: &str, start: u32, end: u32) -> String {
    if end > start {
        format!("{file}:{start}-{end}")
    } else {
        format!("{file}:{start}")
    }
}

/// The map as a ```` ```flow ```` block: one lane per row in lane order, one box per stop; the
/// page shows a box's caption and files when it is clicked.
fn flow(map: &ChangeMap) -> String {
    let mut nodes = serde_json::Map::new();
    for n in &map.nodes {
        let paths: Vec<String> = n
            .sites
            .iter()
            .map(|s| site_ref(&s.file, s.start, s.end))
            .chain(n.renames.iter().map(|r| r.to.clone()))
            .chain(n.deletions.iter().map(|d| d.file.clone()))
            .collect();
        let mut node = json!({ "label": n.symbol, "n": n.id, "note": n.caption, "paths": paths });
        if n.sites.is_empty() && n.renames.is_empty() && !n.deletions.is_empty() {
            node["kind"] = json!("removed");
        }
        nodes.insert(n.id.clone(), node);
    }
    let tiers: Vec<Value> = Lane::ALL
        .iter()
        .filter_map(|&lane| {
            let ids: Vec<&str> = map
                .nodes
                .iter()
                .filter(|n| n.lane == lane)
                .map(|n| n.id.as_str())
                .collect();
            (!ids.is_empty()).then(|| json!([{ "name": lane.title(), "ids": ids }]))
        })
        .collect();
    let edges: Vec<Value> = map
        .edges
        .iter()
        .map(|e| {
            let kind = serde_json::to_value(e.kind).unwrap_or(Value::Null);
            json!([e.from, e.to, kind])
        })
        .collect();
    let spec = json!({ "nodes": nodes, "tiers": tiers, "edges": edges });
    // Serializing a Value cannot fail.
    serde_json::to_string_pretty(&spec).unwrap_or_default()
}

pub fn markdown(map: &ChangeMap) -> String {
    let by_id = |id: &str| map.nodes.iter().find(|n| n.id == id);
    let mut md = String::new();
    // Writing to a String cannot fail, so the results below are ignored.
    let _ = writeln!(md, "# Change map {} · review guide\n", short(&map.head));
    let spec = map
        .spec
        .as_ref()
        .map(|s| format!(" · spec: {s}"))
        .unwrap_or_default();
    let _ = writeln!(
        md,
        "{}...{} · {} stops · {} criteria{spec}\n",
        short(&map.base),
        short(&map.head),
        map.tour.len(),
        map.criteria.len()
    );

    md.push_str("## Criteria\n\n");
    for c in &map.criteria {
        let linked: Vec<String> = map
            .nodes
            .iter()
            .filter(|n| n.criteria.contains(&c.id))
            .map(|n| format!("{} {}", n.id, n.symbol))
            .collect();
        let status = match c.status {
            Status::Implemented => "implemented",
            Status::Partial => "partial",
            Status::Missing => "missing",
        };
        let tested = if c.tested() { "tested" } else { "untested" };
        let links = if linked.is_empty() {
            String::new()
        } else {
            format!(" → {}", linked.join("; "))
        };
        let spec = c
            .spec_ref
            .as_ref()
            .map(|r| format!(" · spec `{r}`"))
            .unwrap_or_default();
        let _ = writeln!(
            md,
            "- {} **{}** {} _({status}, {tested})_{links}{spec}",
            mark(c),
            c.id,
            c.text
        );
        for t in &c.tests {
            let _ = writeln!(md, "  - test `{}:{}` {}", t.file, t.line, t.name);
        }
    }

    let _ = writeln!(md, "\n## Map\n\n```flow\n{}\n```", flow(map));

    md.push_str("\n## Tour\n\n");
    for (i, n) in map.tour.iter().filter_map(|id| by_id(id)).enumerate() {
        let refs = if n.criteria.is_empty() {
            "no criterion".to_owned()
        } else {
            n.criteria.join(",")
        };
        let tested = if n.touched_by_test {
            "touched by tests"
        } else {
            "no test touches it"
        };
        let sites: Vec<String> = n
            .sites
            .iter()
            .map(|s| format!("`{}`", site_ref(&s.file, s.start, s.end)))
            .chain(n.deletions.iter().map(|d| format!("deleted `{}`", d.file)))
            .chain(
                n.renames
                    .iter()
                    .map(|r| format!("moved `{}` → `{}`", r.from, r.to)),
            )
            .collect();
        let _ = writeln!(
            md,
            "{}. {} **{}** ({} · {} · {refs} · {tested}): {}  \n   {}",
            i + 1,
            n.id,
            n.symbol,
            n.lane.as_str(),
            n.risk.as_str(),
            n.caption,
            sites.join(", ")
        );
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_guide_has_a_valid_flow_with_files() {
        let a = "a".repeat(40);
        let b = "b".repeat(40);
        let map: ChangeMap = serde_json::from_value(json!({
            "version": 1, "base": a, "head": b,
            "criteria": [{ "id": "REF-1", "text": "Refund", "status": "implemented", "tests": [] }],
            "nodes": [
                { "id": "N1", "symbol": "refund", "lane": "api", "risk": "read", "caption": "Adds refund",
                  "criteria": ["REF-1"], "touchedByTest": false,
                  "sites": [{ "file": "src/api.rs", "start": 3, "end": 9 }] },
                { "id": "N2", "symbol": "old.rs", "lane": "services", "risk": "skim", "caption": "Removed",
                  "criteria": [], "touchedByTest": false, "sites": [],
                  "deletions": [{ "file": "src/old.rs" }] }
            ],
            "edges": [{ "from": "N1", "to": "N2", "kind": "calls" }],
            "tour": ["N1", "N2"]
        }))
        .expect("map");
        let md = markdown(&map);
        assert!(
            crate::page::check_flows(&md).is_empty(),
            "{:?}",
            crate::page::check_flows(&md)
        );
        assert!(md.contains(r#""src/api.rs:3-9""#), "{md}");
        assert!(md.contains(r#""kind": "removed""#), "{md}");
        assert!(md.contains("1. N1 **refund**"), "{md}");
    }
}
