//! `change-map render`: a Markdown review guide with a mermaid map, next to the JSON.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::map::{ChangeMap, Criterion, Lane, Node, Status};

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

/// Mermaid label text: no double quotes, and `#` would start an entity.
fn label(s: &str) -> String {
    s.replace('"', "'").replace('#', "#35;")
}

fn mermaid_id(criterion: &str) -> String {
    criterion.replace('-', "_")
}

fn short(sha: &str) -> &str {
    sha.get(..10).unwrap_or(sha)
}

fn basename(file: &str) -> &str {
    file.rsplit('/').next().unwrap_or(file)
}

pub fn markdown(map: &ChangeMap) -> String {
    let by_id = |id: &str| map.nodes.iter().find(|n| n.id == id);
    let mut md = String::new();
    // Writing to a String cannot fail, so the results below are ignored.
    let _ = writeln!(md, "# Change map · {}\n", short(&map.head));
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
            .map(|r| format!(" · [spec]({r})"))
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
            .map(|s| format!("`{}:{}`", s.file, s.start))
            .chain(n.deletions.iter().map(|d| format!("deleted `{}`", d.file)))
            .chain(
                n.renames
                    .iter()
                    .map(|r| format!("moved `{}` → `{}`", r.from, r.to)),
            )
            .collect();
        let _ = writeln!(
            md,
            "{}. **{}** ({} · {} · {refs} · {tested}): {}  \n   {}",
            i + 1,
            n.symbol,
            n.lane.as_str(),
            n.risk.as_str(),
            n.caption,
            sites.join(", ")
        );
    }

    md.push_str("\n## Map\n\n```mermaid\nflowchart LR\n  subgraph SPEC[\"Spec\"]\n");
    for c in &map.criteria {
        let _ = writeln!(
            md,
            "    {}[\"{} {} {}\"]",
            mermaid_id(&c.id),
            mark(c),
            c.id,
            label(&c.text)
        );
    }
    md.push_str("  end\n");
    for lane in Lane::ALL {
        let nodes: Vec<&Node> = map.nodes.iter().filter(|n| n.lane == lane).collect();
        if nodes.is_empty() {
            continue;
        }
        let _ = writeln!(md, "  subgraph {}[\"{}\"]", lane.as_str(), lane.title());
        for n in nodes {
            let more = if n.sites.len() > 1 {
                format!(" +{}", n.sites.len() - 1)
            } else {
                String::new()
            };
            let file = n
                .sites
                .first()
                .map(|s| basename(&s.file).to_owned())
                .or_else(|| {
                    n.renames
                        .first()
                        .map(|r| format!("moved to {}", basename(&r.to)))
                })
                .or_else(|| {
                    n.deletions
                        .first()
                        .map(|d| format!("deleted {}", basename(&d.file)))
                })
                .unwrap_or_default();
            let _ = writeln!(
                md,
                "    {}[\"{}<br/>{file}{more}\"]",
                n.id,
                label(&n.symbol)
            );
        }
        md.push_str("  end\n");
    }
    for n in &map.nodes {
        for c in &n.criteria {
            let _ = writeln!(md, "  {} -.-> {}", mermaid_id(c), n.id);
        }
    }
    for e in &map.edges {
        let kind = serde_json::to_value(e.kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let _ = writeln!(md, "  {} -->|{kind}| {}", e.from, e.to);
    }
    md.push_str("```\n");
    md
}
