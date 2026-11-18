//! `overview-me extract`: facts plus a draft map for the skill to label.
//!
//! The draft already does the mechanical work: one node per changed symbol, import-only hunks
//! folded into the function in their file, the same edit at several places grouped into one
//! node, `calls` edges, and a reading order (callees before callers). The skill adds criteria,
//! captions, tests and callouts.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::facts::{self, Changes, Skipped, Unit};
use crate::git::Git;
use crate::map::{ChangeMap, Deletion, Edge, EdgeKind, Node, Risk, Site, TestRef};
use crate::rules::Rules;
use crate::scope::Scope;
use crate::symbols::{self, Symbol};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Facts<'a> {
    units: Vec<FactUnit<'a>>,
    test_files: &'a [String],
    /// Test declarations in changed test files: candidates for `criteria[].tests`.
    test_declarations: &'a [TestRef],
    skipped: &'a [Skipped],
}

#[derive(Serialize)]
struct FactUnit<'a> {
    node: &'a str,
    #[serde(flatten)]
    unit: &'a Unit,
}

/// A draft node and the hunks it holds.
struct Draft<'a> {
    node: Node,
    units: Vec<&'a Unit>,
}

/// Draft nodes: one per enclosing symbol, ordered by lane, file and first changed line.
fn draft_nodes(units: &[Unit]) -> Vec<Draft<'_>> {
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
        .map(|us| {
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
                id: String::new(),
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
                // Hunks inside one symbol share a site; top-level hunks are scattered across
                // the file, so each keeps its own and no site spans a function between them.
                sites: if first.span.is_some() {
                    vec![Site {
                        file: first.file.clone(),
                        start: us.iter().map(|u| u.start).min().unwrap_or(first.start),
                        end: us.iter().map(|u| u.end).max().unwrap_or(first.end),
                    }]
                } else {
                    us.iter()
                        .map(|u| Site {
                            file: u.file.clone(),
                            start: u.start,
                            end: u.end,
                        })
                        .collect()
                },
            };
            Draft { node, units: us }
        })
        .collect()
}

/// Moves `from` into `into`: its sites after the main site, its hunks, and a `read` risk.
fn merge<'a>(into: &mut Draft<'a>, from: Draft<'a>) {
    into.node.sites.extend(from.node.sites);
    into.units.extend(from.units);
    if from.node.risk == Risk::Read {
        into.node.risk = Risk::Read;
    }
}

/// Whether a top-level hunk only changes imports: every identifier on its added lines is an
/// imported name, or every removed line is an import statement.
fn import_only(unit: &Unit, changes: &Changes) -> bool {
    if unit.ident.is_some() || !symbols::is_code(&unit.file) {
        return false;
    }
    if unit.added.is_empty() {
        let removed: Vec<&str> = unit
            .diff
            .iter()
            .filter(|d| d.starts_with('-'))
            .filter_map(|d| d.split_once("| ").map(|(_, c)| c.trim()))
            .collect();
        return !removed.is_empty()
            && removed.iter().all(|c| {
                ["import ", "use ", "from ", "pub use "]
                    .iter()
                    .any(|k| c.starts_with(k))
                    || c.contains("require(")
                    || c.is_empty()
            });
    }
    let Some(parsed) = changes
        .files
        .iter()
        .find(|f| f.path == unit.file)
        .and_then(|f| f.parsed.as_ref())
    else {
        return false;
    };
    let names: Vec<&str> = parsed
        .identifiers
        .iter()
        .filter(|i| {
            unit.added
                .contains(&u32::try_from(i.line).unwrap_or(u32::MAX))
        })
        .map(|i| i.name.as_str())
        .collect();
    !names.is_empty() && names.iter().all(|n| parsed.imports.contains(*n))
}

/// Folds import-only nodes into the first symbol node of the same file.
fn fold_imports<'a>(drafts: Vec<Draft<'a>>, changes: &Changes) -> Vec<Draft<'a>> {
    let mut out: Vec<Draft<'a>> = Vec::new();
    let mut pending: Vec<Draft<'a>> = Vec::new();
    for d in drafts {
        if d.units.iter().all(|u| import_only(u, changes)) {
            pending.push(d);
        } else {
            out.push(d);
        }
    }
    for d in pending {
        let file = d.units[0].file.clone();
        match out
            .iter_mut()
            .find(|o| o.units.iter().any(|u| u.file == file && u.ident.is_some()))
        {
            Some(target) => merge(target, d),
            None => out.push(d),
        }
    }
    out
}

/// The changed text of a node, without line numbers: equal for the same edit at two places.
fn signature(d: &Draft<'_>) -> String {
    d.units
        .iter()
        .filter(|u| u.ident.is_some())
        .flat_map(|u| &u.diff)
        .filter(|l| l.starts_with('+') || l.starts_with('-'))
        .filter_map(|l| {
            let (head, code) = l.split_once("| ")?;
            Some(format!("{}{}", &head[..1], code.trim()))
        })
        .filter(|l| l.len() > 1)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Groups symbol nodes that make the same edit into the first of them.
fn group_same_edits<'a>(drafts: Vec<Draft<'a>>) -> Vec<Draft<'a>> {
    let mut out: Vec<Draft<'a>> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for d in drafts {
        let sig = signature(&d);
        let groupable = !sig.is_empty()
            && d.units.iter().all(|u| !u.comment_only)
            && d.units.iter().any(|u| u.ident.is_some());
        if groupable && let Some(&i) = seen.get(&sig) {
            merge(&mut out[i], d);
            continue;
        }
        if groupable {
            seen.insert(sig, out.len());
        }
        out.push(d);
    }
    out
}

/// `calls` edges: a changed symbol names another changed symbol, and the name's binding can be
/// that symbol (see [`crate::scope`]): the same file, its Go package, or an import of it.
fn call_edges(changes: &Changes, drafts: &[Draft<'_>], scope: &Scope<'_>) -> Vec<Edge> {
    let mut edges: Vec<Edge> = Vec::new();
    let enclosing = |file: &str, line: u32| {
        drafts.iter().find(|d| {
            d.units
                .iter()
                .any(|u| u.file == file && u.span.is_some_and(|(a, b)| a <= line && line <= b))
        })
    };
    let parsed_at = |path: &str| {
        changes
            .files
            .iter()
            .find(|f| f.path == path)
            .and_then(|f| f.parsed.as_ref())
    };
    for callee in drafts {
        let mut targets: Vec<(&str, &Symbol)> = callee
            .units
            .iter()
            .filter_map(|u| {
                let ident = u.ident.as_deref()?;
                let (a, b) = u.span?;
                let sym = parsed_at(&u.file)?.symbols.iter().find(|s| {
                    s.ident == ident
                        && u32::try_from(s.start).ok() == Some(a)
                        && u32::try_from(s.end).ok() == Some(b)
                })?;
                Some((u.file.as_str(), sym))
            })
            .collect();
        targets.sort_by(|x, y| (x.0, &x.1.name, x.1.start).cmp(&(y.0, &y.1.name, y.1.start)));
        targets.dedup();
        for (def_file, sym) in targets {
            for file in &changes.files {
                let Some(parsed) = &file.parsed else {
                    continue;
                };
                for r in parsed.identifiers.iter().filter(|r| r.name == sym.ident) {
                    if !scope.reaches(&file.path, parsed, r, def_file, sym) {
                        continue;
                    }
                    let Some(caller) =
                        enclosing(&file.path, u32::try_from(r.line).unwrap_or(u32::MAX))
                    else {
                        continue;
                    };
                    if caller.node.id != callee.node.id
                        && !edges
                            .iter()
                            .any(|e| e.from == caller.node.id && e.to == callee.node.id)
                    {
                        edges.push(Edge {
                            from: caller.node.id.clone(),
                            to: callee.node.id.clone(),
                            kind: EdgeKind::Calls,
                        });
                    }
                }
            }
        }
    }
    edges
}

/// Reading order: a node comes after every node it calls (contract before implementation
/// before callers); ties and cycles keep the draft order.
fn tour(ids: &[String], edges: &[Edge]) -> Vec<String> {
    let mut callees: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in edges {
        callees
            .entry(e.from.as_str())
            .or_default()
            .push(e.to.as_str());
    }
    let mut placed: Vec<String> = Vec::new();
    let mut left: Vec<&String> = ids.iter().collect();
    while !left.is_empty() {
        let ready = left
            .iter()
            .position(|id| {
                callees.get(id.as_str()).is_none_or(|cs| {
                    cs.iter()
                        .all(|c| *c == id.as_str() || placed.iter().any(|p| p == c))
                })
            })
            .unwrap_or(0);
        placed.push(left.remove(ready).clone());
    }
    placed
}

/// The line naming the codebase overview this change sits in, if one exists.
fn context(store: &Path) -> String {
    let codebase = store.join("codebase.md");
    match std::fs::read_to_string(&codebase) {
        Ok(text) => {
            let sha = text
                .split_whitespace()
                .skip_while(|w| *w != "sha")
                .nth(1)
                .map_or("?", |s| &s[..s.len().min(10)]);
            format!("codebase overview: {} (at {sha})", codebase.display())
        }
        Err(_) => "codebase overview: none (suggest /codebase-overview at hand-off)".into(),
    }
}

/// Writes `<store>/<head>.facts.json` (or `out`) and, unless a map for this base exists (or
/// `fresh`), the draft map; prints each draft node with its diff. Returns the map path.
///
/// # Errors
/// When git fails or a file cannot be written.
pub fn run(
    git: &Git,
    rules: &Rules,
    base: &str,
    head: &str,
    spec: Option<&Path>,
    out: Option<PathBuf>,
    fresh: bool,
) -> Result<PathBuf> {
    let base = git.rev(base)?;
    let head = git.rev(head)?;
    let changes = facts::changes(git, rules, &base, &head)?;
    let mut drafts = group_same_edits(fold_imports(draft_nodes(&changes.units), &changes));
    for (i, d) in drafts.iter_mut().enumerate() {
        d.node.id = format!("N{}", i + 1);
    }

    let exported: HashSet<&str> = changes
        .units
        .iter()
        .filter(|u| u.exported)
        .filter_map(|u| u.ident.as_deref())
        .collect();
    let tested = facts::tested_names(git, &head, &changes.test_files, &exported)?;
    for d in &mut drafts {
        d.node.touched_by_test = d
            .units
            .iter()
            .any(|u| u.exported && u.ident.as_ref().is_some_and(|i| tested.contains(i)));
    }
    let edges = call_edges(&changes, &drafts, &Scope::load(git, &head)?);

    let spec = spec
        .map(|p| std::fs::canonicalize(p).with_context(|| format!("spec {}", p.display())))
        .transpose()?;
    // One draft node per deleted file; the skill folds them into the nodes they belong to.
    let mut nodes: Vec<Node> = drafts.iter().map(|d| d.node.clone()).collect();
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
    let ids: Vec<String> = nodes.iter().map(|n| n.id.clone()).collect();
    let draft = ChangeMap {
        version: 1,
        base: base.clone(),
        head: head.clone(),
        spec: spec.map(|p| p.display().to_string()),
        criteria: Vec::new(),
        tour: tour(&ids, &edges),
        nodes,
        edges,
        callouts: Vec::new(),
    };
    let test_declarations = facts::test_declarations(git, &head, &changes)?;
    let facts = Facts {
        test_declarations: &test_declarations,
        units: drafts
            .iter()
            .flat_map(|d| {
                d.units.iter().map(|u| FactUnit {
                    node: &d.node.id,
                    unit: u,
                })
            })
            .collect(),
        test_files: &changes.test_files,
        skipped: &changes.skipped,
    };
    let store = git.store()?;
    let out = out.unwrap_or_else(|| store.join(format!("{head}.facts.json")));
    for dir in [out.parent(), Some(store.as_path())].into_iter().flatten() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(&out, serde_json::to_string_pretty(&facts)?)
        .with_context(|| format!("writing {}", out.display()))?;
    let map_path = store.join(format!("{head}.json"));
    let kept = !fresh
        && std::fs::read_to_string(&map_path)
            .ok()
            .and_then(|t| serde_json::from_str::<ChangeMap>(&t).ok())
            .is_some_and(|m| m.base == base);
    if !kept {
        std::fs::write(&map_path, serde_json::to_string_pretty(&draft)? + "\n")
            .with_context(|| format!("writing {}", map_path.display()))?;
    }

    let files: HashSet<&str> = changes.units.iter().map(|u| u.file.as_str()).collect();
    println!("facts: {}", out.display());
    println!(
        "map:   {}  ({})",
        map_path.display(),
        if kept {
            "kept: a map for this base exists; pass --fresh to start over"
        } else {
            "draft written: fill in its labels"
        }
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
        draft.nodes.len(),
        draft.edges.len(),
        changes.test_files.len(),
        if skipped.is_empty() {
            "none".to_owned()
        } else {
            skipped.join(", ")
        }
    );
    println!("tour: {}", draft.tour.join(" "));
    println!("{}", context(&store));
    for d in &drafts {
        let n = &d.node;
        let mut flags = vec![n.risk.as_str()];
        if n.touched_by_test {
            flags.push("test-touched");
        }
        if d.units.iter().all(|u| u.comment_only) {
            flags.push("comments-only");
        }
        let site = &n.sites[0];
        println!(
            "\n{} {} {} [{}] {}:{}-{}",
            n.id,
            n.lane.as_str(),
            n.symbol,
            flags.join(","),
            site.file,
            site.start,
            site.end,
        );
        let mut by_site: BTreeMap<(&str, u32), Vec<&Unit>> = BTreeMap::new();
        for u in &d.units {
            by_site
                .entry((u.file.as_str(), u.start))
                .or_default()
                .push(u);
        }
        let several = n.sites.len() > 1;
        for ((file, start), us) in by_site {
            if several {
                println!("  @ {file}:{start}");
            }
            for line in us.iter().flat_map(|u| &u.diff) {
                println!("  {line}");
            }
        }
    }
    for n in draft.nodes.iter().filter(|n| n.sites.is_empty()) {
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
            "\n{} {} {} [{}]",
            n.id,
            n.lane.as_str(),
            what.join(", "),
            n.risk.as_str()
        );
    }
    if !test_declarations.is_empty() {
        println!("\ntests in changed test files:");
        for t in &test_declarations {
            println!("  {}:{} {}", t.file, t.line, t.name);
        }
    }
    Ok(map_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(from: &str, to: &str) -> Edge {
        Edge {
            from: from.into(),
            to: to.into(),
            kind: EdgeKind::Calls,
        }
    }

    #[test]
    fn tour_puts_callees_first() {
        let ids: Vec<String> = ["N1", "N2", "N3", "N4"].map(String::from).to_vec();
        let edges = [edge("N1", "N2"), edge("N2", "N3"), edge("N4", "N4")];
        assert_eq!(tour(&ids, &edges), ["N3", "N2", "N1", "N4"]);
        // A cycle still lists every node once, after the nodes that are ready.
        let edges = [edge("N1", "N2"), edge("N2", "N1")];
        assert_eq!(tour(&ids, &edges), ["N3", "N4", "N1", "N2"]);
    }

    #[test]
    fn draft_folds_imports_and_groups_same_edits() {
        let repo = crate::testutil::TempRepo::new("extract-draft");
        repo.write(
            "src/a.ts",
            "import { x } from './x'\n\nexport function one() {\n  return 1\n}\n\nexport function two() {\n  return 1\n}\n",
        );
        let base = repo.commit("base");
        repo.write(
            "src/a.ts",
            "import { x, y } from './x'\n\nexport function one() {\n  return y(2)\n}\n\nexport function two() {\n  return y(2)\n}\n",
        );
        let head = repo.commit("head");
        let git = Git::new(repo.path());
        let rules = Rules::load(None).expect("rules");
        let out = repo.path().join("facts.json");
        let map_path = run(&git, &rules, &base, &head, None, Some(out), false).expect("extract");
        let map: ChangeMap =
            serde_json::from_str(&std::fs::read_to_string(&map_path).expect("map")).expect("json");
        assert_eq!(map.nodes.len(), 1, "{:#?}", map.nodes);
        let sites: Vec<(u32, u32)> = map.nodes[0]
            .sites
            .iter()
            .map(|s| (s.start, s.end))
            .collect();
        assert_eq!(
            sites,
            [(4, 4), (1, 1), (8, 8)],
            "main site, its import, then the same edit"
        );
        assert_eq!(map.tour, ["N1"]);
        // A second run keeps the labelled map.
        let mut labelled = map.clone();
        labelled.nodes[0].caption = "returns y(2)".into();
        std::fs::write(&map_path, serde_json::to_string(&labelled).expect("json")).expect("write");
        run(&git, &rules, &base, &head, None, None, false).expect("extract again");
        let again: ChangeMap =
            serde_json::from_str(&std::fs::read_to_string(&map_path).expect("map")).expect("json");
        assert_eq!(again.nodes[0].caption, "returns y(2)");
    }
}
