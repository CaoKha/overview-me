//! `overview-me extract`: facts plus a draft map for the skill to fill.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::facts::{self, Changes, Skipped, Unit};
use crate::git::Git;
use crate::map::{ChangeMap, Deletion, Edge, EdgeKind, Node, Risk, Site, TestRef};
use crate::rules::Rules;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Facts<'a> {
    draft: ChangeMap,
    units: Vec<FactUnit<'a>>,
    test_files: &'a [String],
    /// Test declarations in changed test files: candidates for `criteria[].tests`.
    test_declarations: Vec<TestRef>,
    skipped: &'a [Skipped],
}

#[derive(Serialize)]
struct FactUnit<'a> {
    node: &'a str,
    #[serde(flatten)]
    unit: &'a Unit,
}

/// Draft nodes: one per enclosing symbol, ordered by lane, file and first changed line.
fn draft_nodes(units: &[Unit]) -> Vec<(Node, Vec<&Unit>)> {
    let mut groups: Vec<Vec<&Unit>> = Vec::new();
    let mut index: HashMap<(&str, u32), usize> = HashMap::new();
    for u in units {
        let key = (u.file.as_str(), u.span.map_or(0, |s| s.0));
        let i = *index.entry(key).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[i].push(u);
    }
    groups.sort_by_key(|us| (us[0].lane, us[0].file.clone(), us[0].start));
    groups
        .into_iter()
        .enumerate()
        .map(|(i, us)| {
            let first = us[0];
            let symbol = match first.ident {
                Some(_) => first.symbol.clone(),
                None => format!(
                    "{} {}",
                    first.symbol,
                    first.file.rsplit('/').next().unwrap_or(&first.file)
                ),
            };
            let node = Node {
                id: format!("N{}", i + 1),
                symbol,
                lane: first.lane,
                risk: if us.iter().any(|u| u.risk == Risk::Read) {
                    Risk::Read
                } else {
                    Risk::Skim
                },
                caption: String::new(),
                criteria: Vec::new(),
                touched_by_test: false,
                deletions: Vec::new(),
                renames: Vec::new(),
                sites: vec![Site {
                    file: first.file.clone(),
                    start: us.iter().map(|u| u.start).min().unwrap_or(first.start),
                    end: us.iter().map(|u| u.end).max().unwrap_or(first.end),
                }],
            };
            (node, us)
        })
        .collect()
}

/// `calls` edges: a changed symbol names a changed exported symbol that its file imports (or defines).
fn call_edges(changes: &Changes, drafts: &[(Node, Vec<&Unit>)]) -> Vec<Edge> {
    let mut edges: Vec<Edge> = Vec::new();
    let enclosing = |file: &str, line: u32| {
        drafts.iter().find(|(_, us)| {
            us[0].file == file && us[0].span.is_some_and(|(a, b)| a <= line && line <= b)
        })
    };
    for (callee, us) in drafts {
        let Some(name) = us
            .iter()
            .find(|u| u.exported)
            .and_then(|u| u.ident.as_deref())
        else {
            continue;
        };
        for file in &changes.files {
            let Some(parsed) = &file.parsed else {
                continue;
            };
            let same_file = file.path == us[0].file;
            if !same_file && !parsed.imports.contains(name) {
                continue;
            }
            for (ident, line) in &parsed.identifiers {
                let Some((caller, _)) = (ident == name)
                    .then(|| enclosing(&file.path, u32::try_from(*line).unwrap_or(u32::MAX)))
                    .flatten()
                else {
                    continue;
                };
                if caller.id != callee.id
                    && !edges
                        .iter()
                        .any(|e| e.from == caller.id && e.to == callee.id)
                {
                    edges.push(Edge {
                        from: caller.id.clone(),
                        to: callee.id.clone(),
                        kind: EdgeKind::Calls,
                    });
                }
            }
        }
    }
    edges
}

/// Writes `<store>/<head>.facts.json` (or `out`) and prints a summary of the draft nodes.
///
/// # Errors
/// When git fails or the facts file cannot be written.
pub fn run(
    git: &Git,
    rules: &Rules,
    base: &str,
    head: &str,
    spec: Option<&Path>,
    out: Option<PathBuf>,
) -> Result<()> {
    let base = git.rev(base)?;
    let head = git.rev(head)?;
    let changes = facts::changes(git, rules, &base, &head)?;
    let mut drafts = draft_nodes(&changes.units);

    let exported: HashSet<&str> = changes
        .units
        .iter()
        .filter(|u| u.exported)
        .filter_map(|u| u.ident.as_deref())
        .collect();
    let tested = facts::tested_names(git, &head, &changes.test_files, &exported)?;
    for (node, us) in &mut drafts {
        node.touched_by_test = us
            .iter()
            .any(|u| u.exported && u.ident.as_ref().is_some_and(|i| tested.contains(i)));
    }
    let edges = call_edges(&changes, &drafts);

    let spec = spec
        .map(|p| std::fs::canonicalize(p).with_context(|| format!("spec {}", p.display())))
        .transpose()?;
    // One draft node per deleted file; the skill folds them into the nodes they belong to.
    let mut nodes: Vec<Node> = drafts.iter().map(|(n, _)| n.clone()).collect();
    for file in &changes.deleted {
        nodes.push(Node {
            id: format!("N{}", nodes.len() + 1),
            symbol: format!("(deleted) {}", file.rsplit('/').next().unwrap_or(file)),
            lane: rules.lane(file),
            risk: Risk::Read,
            caption: String::new(),
            criteria: Vec::new(),
            touched_by_test: false,
            sites: Vec::new(),
            deletions: vec![Deletion { file: file.clone() }],
            renames: Vec::new(),
        });
    }
    // One draft node per listed move.
    for rename in &changes.renames {
        nodes.push(Node {
            id: format!("N{}", nodes.len() + 1),
            symbol: format!(
                "(moved) {}",
                rename.to.rsplit('/').next().unwrap_or(&rename.to)
            ),
            lane: rules.lane(&rename.to),
            risk: Risk::Skim,
            caption: String::new(),
            criteria: Vec::new(),
            touched_by_test: false,
            sites: Vec::new(),
            deletions: Vec::new(),
            renames: vec![rename.clone()],
        });
    }
    let draft = ChangeMap {
        version: 1,
        base: base.clone(),
        head: head.clone(),
        spec: spec.map(|p| p.display().to_string()),
        criteria: Vec::new(),
        nodes,
        edges,
        tour: Vec::new(),
    };
    let test_declarations = facts::test_declarations(git, &head, &changes)?;
    let facts = Facts {
        test_declarations,
        units: drafts
            .iter()
            .flat_map(|(n, us)| {
                us.iter().map(|u| FactUnit {
                    node: &n.id,
                    unit: u,
                })
            })
            .collect(),
        draft,
        test_files: &changes.test_files,
        skipped: &changes.skipped,
    };
    let store = git.store()?;
    let out = out.unwrap_or_else(|| store.join(format!("{head}.facts.json")));
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(&out, serde_json::to_string_pretty(&facts)?)
        .with_context(|| format!("writing {}", out.display()))?;

    let files: HashSet<&str> = changes.units.iter().map(|u| u.file.as_str()).collect();
    println!("facts: {}", out.display());
    println!(
        "map:   {}  (write the filled map here)",
        store.join(format!("{head}.json")).display()
    );
    let skipped: Vec<String> = changes
        .skipped
        .iter()
        .map(|s| format!("{} ({})", s.file, s.reason))
        .collect();
    println!(
        "{} hunks in {} files, {} deleted files, {} moves -> {} draft nodes, {} call edges; {} test files; skipped: {}",
        changes.units.len(),
        files.len(),
        changes.deleted.len(),
        changes.renames.len(),
        facts.draft.nodes.len(),
        facts.draft.edges.len(),
        changes.test_files.len(),
        if skipped.is_empty() {
            "none".to_owned()
        } else {
            skipped.join(", ")
        }
    );
    for (n, us) in &drafts {
        let mut flags = vec![n.risk.as_str()];
        if n.touched_by_test {
            flags.push("test-touched");
        }
        if us.iter().all(|u| u.comment_only) {
            flags.push("comments-only");
        }
        let site = &n.sites[0];
        println!(
            "{:<4} {:<8} {:<22} {}  {}:{}-{} ({} hunks)",
            n.id,
            n.lane.as_str(),
            flags.join(","),
            n.symbol,
            site.file,
            site.start,
            site.end,
            us.len()
        );
    }
    for n in facts.draft.nodes.iter().filter(|n| n.sites.is_empty()) {
        let what: Vec<String> = n
            .deletions
            .iter()
            .map(|d| format!("deleted {}", d.file))
            .chain(
                n.renames
                    .iter()
                    .map(|r| format!("moved {} -> {}", r.from, r.to)),
            )
            .collect();
        println!(
            "{:<4} {:<8} {:<22} {}",
            n.id,
            n.lane.as_str(),
            n.risk.as_str(),
            what.join(", ")
        );
    }
    Ok(())
}
