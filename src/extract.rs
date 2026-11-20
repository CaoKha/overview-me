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
use crate::rules::{Rules, lane_rank};
use crate::scope::Scope;
use crate::symbols::{self, Symbol};

/// Folder drafts start above this many symbol, deletion and move nodes.
pub const LARGE_THRESHOLD: usize = 300;

#[derive(Debug, Default, clap::Args)]
pub struct Options {
    /// Draft folders explicitly, or keep symbols even for a large change.
    #[arg(long, value_parser = ["dir", "symbol"])]
    pub group_by: Option<String>,
    /// Folder depth (default: 2, e.g. apps/api or packages/core).
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u32).range(1..))]
    pub depth: u32,
    /// Group this prefix separately; repeat in priority order, first match wins.
    #[arg(long)]
    pub split: Vec<String>,
    /// Cover only this repo-relative file or folder.
    #[arg(long)]
    pub path: Option<String>,
    /// Draft map path instead of the shared store.
    #[arg(long)]
    pub map: Option<PathBuf>,
    /// Overwrite an existing map for this base.
    #[arg(long)]
    pub fresh: bool,
}

/// Path-component match: packages/a does not include packages/ab.
pub fn under(file: &str, prefix: &str) -> bool {
    let prefix = prefix.trim_end_matches('/');
    prefix.is_empty()
        || file == prefix
        || file
            .strip_prefix(prefix)
            .is_some_and(|s| s.starts_with('/'))
}

fn folder(file: &str, options: &Options) -> String {
    if let Some(prefix) = options.split.iter().find(|p| under(file, p)) {
        return prefix.trim_end_matches('/').to_owned();
    }
    let dir = file.rsplit_once('/').map_or("", |(dir, _)| dir);
    let depth = if options.depth == 0 {
        2
    } else {
        options.depth as usize
    };
    let name = dir.split('/').take(depth).collect::<Vec<_>>().join("/");
    if name.is_empty() {
        "(root)".into()
    } else {
        name
    }
}

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
    groups.sort_by_key(|us| {
        (
            lane_rank(&us[0].lane),
            us[0].lane.clone(),
            us[0].file.clone(),
            us[0].start,
        )
    });
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
                lane: first.lane.clone(),
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
    let mut spans: HashMap<&str, Vec<(u32, u32, &Draft<'_>)>> = HashMap::new();
    for d in drafts {
        for u in &d.units {
            if let Some((a, b)) = u.span {
                spans.entry(&u.file).or_default().push((a, b, d));
            }
        }
    }
    let enclosing = |file: &str, line: u32| {
        spans
            .get(file)?
            .iter()
            .find(|(a, b, _)| *a <= line && line <= *b)
            .map(|(_, _, d)| *d)
    };
    let parsed: HashMap<_, _> = changes
        .files
        .iter()
        .filter_map(|f| Some((f.path.as_str(), f.parsed.as_ref()?)))
        .collect();
    let parsed_at = |path: &str| parsed.get(path).copied();
    let mut refs: HashMap<&str, Vec<(&str, &crate::symbols::Parsed, &crate::symbols::Ident)>> =
        HashMap::new();
    for file in &changes.files {
        if let Some(p) = &file.parsed {
            for r in &p.identifiers {
                refs.entry(&r.name).or_default().push((&file.path, p, r));
            }
        }
    }
    let mut seen = HashSet::new();
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
            for &(path, parsed, r) in refs.get(sym.ident.as_str()).into_iter().flatten() {
                if !scope.reaches(path, parsed, r, def_file, sym) {
                    continue;
                }
                let Some(caller) = enclosing(path, u32::try_from(r.line).unwrap_or(u32::MAX))
                else {
                    continue;
                };
                if caller.node.id != callee.node.id
                    && seen.insert((&caller.node.id, &callee.node.id))
                {
                    edges.push(Edge {
                        from: caller.node.id.clone(),
                        to: callee.node.id.clone(),
                        kind: EdgeKind::Calls,
                        weight: None,
                    });
                }
            }
        }
    }
    edges
}

/// One folder owns each site, deletion and move. Split prefixes take priority over depth.
fn group_folders(
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    options: &Options,
    rules: &Rules,
) -> (Vec<Node>, Vec<Edge>) {
    let mut groups: BTreeMap<String, Node> = BTreeMap::new();
    let mut membership: HashMap<String, HashSet<String>> = HashMap::new();
    for node in nodes {
        let paths = node
            .sites
            .iter()
            .map(|s| s.file.as_str())
            .chain(node.deletions.iter().map(|d| d.file.as_str()))
            .chain(node.renames.iter().map(|r| r.to.as_str()));
        let names: HashSet<String> = paths.map(|p| folder(p, options)).collect();
        for name in &names {
            let group = groups.entry(name.clone()).or_insert_with(|| Node {
                id: String::new(),
                symbol: name.clone(),
                lane: String::new(),
                risk: Risk::Skim,
                caption: String::new(),
                criteria: Vec::new(),
                touched_by_test: false,
                sites: Vec::new(),
                deletions: Vec::new(),
                renames: Vec::new(),
            });
            group.sites.extend(
                node.sites
                    .iter()
                    .filter(|s| folder(&s.file, options) == *name)
                    .cloned(),
            );
            group.deletions.extend(
                node.deletions
                    .iter()
                    .filter(|d| folder(&d.file, options) == *name)
                    .cloned(),
            );
            group.renames.extend(
                node.renames
                    .iter()
                    .filter(|r| folder(&r.to, options) == *name)
                    .cloned(),
            );
            if node.risk == Risk::Read {
                group.risk = Risk::Read;
            }
            group.touched_by_test |= node.touched_by_test;
        }
        membership.insert(node.id, names);
    }
    let mut nodes: Vec<Node> = groups.into_values().collect();
    for (i, n) in nodes.iter_mut().enumerate() {
        n.id = format!("N{}", i + 1);
        let mut lanes: BTreeMap<String, usize> = BTreeMap::new();
        for file in n
            .sites
            .iter()
            .map(|s| &s.file)
            .chain(n.deletions.iter().map(|d| &d.file))
            .chain(n.renames.iter().map(|r| &r.to))
        {
            *lanes.entry(rules.lane(file)).or_default() += 1;
        }
        n.lane = lanes
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
            .map(|(lane, _)| lane)
            .unwrap_or_else(|| "services".into());
    }
    // Use the existing lane rules, merging only the least represented lanes past the limit.
    let mut lanes: BTreeMap<String, usize> = BTreeMap::new();
    for n in &nodes {
        *lanes.entry(n.lane.clone()).or_default() += 1;
    }
    let mut lanes: Vec<_> = lanes.into_iter().collect();
    lanes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    if let Some((largest, _)) = lanes.first() {
        for n in &mut nodes {
            if !lanes
                .iter()
                .take(crate::rules::MAX_LANES)
                .any(|(lane, _)| lane == &n.lane)
            {
                n.lane.clone_from(largest);
            }
        }
    }
    let ids: HashMap<&str, &str> = nodes
        .iter()
        .map(|n| (n.symbol.as_str(), n.id.as_str()))
        .collect();
    let mut weights: BTreeMap<(String, String), usize> = BTreeMap::new();
    for e in edges {
        for a in &membership[&e.from] {
            for b in &membership[&e.to] {
                if a != b {
                    *weights
                        .entry((ids[a.as_str()].into(), ids[b.as_str()].into()))
                        .or_default() += e.weight.unwrap_or(1);
                }
            }
        }
    }
    let mut edges: Vec<Edge> = weights
        .into_iter()
        .map(|((from, to), weight)| Edge {
            from,
            to,
            kind: EdgeKind::Calls,
            weight: Some(weight),
        })
        .collect();
    prune_edges(&mut edges);
    (nodes, edges)
}

/// The strongest module calls, at most two per caller and fifteen across the page.
fn prune_edges(edges: &mut Vec<Edge>) {
    edges.sort_by(|a, b| {
        b.weight
            .unwrap_or(1)
            .cmp(&a.weight.unwrap_or(1))
            .then(a.from.cmp(&b.from))
            .then(a.to.cmp(&b.to))
    });
    let mut outgoing: HashMap<String, usize> = HashMap::new();
    edges.retain(|e| {
        let count = outgoing.entry(e.from.clone()).or_default();
        *count += 1;
        *count <= 2
    });
    edges.truncate(15);
}

/// Largest body hunk first; import-only and error-definition sites are fallback choices.
fn main_first(node: &mut Node, changes: &Changes) {
    let best = node
        .sites
        .iter()
        .enumerate()
        .min_by_key(|(i, site)| {
            let imports = changes
                .units
                .iter()
                .filter(|u| u.file == site.file && site.start <= u.end && u.start <= site.end)
                .all(|u| import_only(u, changes));
            let stem = Path::new(&site.file)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let errors = matches!(stem, "error" | "errors")
                || stem.ends_with(".errors")
                || stem.ends_with("_errors");
            (
                imports,
                errors,
                std::cmp::Reverse(site.end - site.start),
                *i,
            )
        })
        .map(|(i, _)| i);
    if let Some(i) = best {
        let site = node.sites.remove(i);
        node.sites.insert(0, site);
    }
}

/// Print just one node's diffs, without expanding the whole map on stdout.
pub fn node(git: &Git, rules: &Rules, id: &str, path: Option<PathBuf>) -> Result<()> {
    let path = path.map_or_else(|| git.map_path("HEAD"), Ok)?;
    let map: ChangeMap = serde_json::from_str(
        &std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?,
    )?;
    let node = map
        .nodes
        .iter()
        .find(|n| n.id == id)
        .with_context(|| format!("no node {id} in {}", path.display()))?;
    let changes = facts::changes_in(git, rules, &map.base, &map.head, map.path.as_deref())?;
    println!(
        "{} {} {} [{}]",
        node.id,
        node.lane,
        node.symbol,
        node.risk.as_str()
    );
    for u in &changes.units {
        if node
            .sites
            .iter()
            .any(|s| s.file == u.file && s.start <= u.end && u.start <= s.end)
        {
            println!("  @ {}:{}", u.file, u.start);
            for line in &u.diff {
                println!("  {line}");
            }
        }
    }
    for d in &node.deletions {
        println!("  deleted {}", d.file);
    }
    for r in &node.renames {
        println!("  moved {} -> {}", r.from, r.to);
    }
    Ok(())
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
    options: &Options,
) -> Result<PathBuf> {
    let base = git.rev(base)?;
    let head = git.rev(head)?;
    let changes = facts::changes_in(git, rules, &base, &head, options.path.as_deref())?;
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
    let grouped = options.group_by.as_deref() == Some("dir")
        || (options.group_by.is_none() && nodes.len() > LARGE_THRESHOLD);
    let (nodes, edges) = if grouped {
        group_folders(nodes, edges, options, rules)
    } else {
        (nodes, edges)
    };
    let ids: Vec<String> = nodes.iter().map(|n| n.id.clone()).collect();
    let mut draft = ChangeMap {
        version: 1,
        grouped,
        path: options.path.clone(),
        base: base.clone(),
        head: head.clone(),
        spec: spec.map(|p| p.display().to_string()),
        criteria: Vec::new(),
        tour: tour(&ids, &edges),
        nodes,
        edges,
        callouts: Vec::new(),
    };
    let tested_folders: HashSet<String> = if grouped {
        changes
            .units
            .iter()
            .filter(|u| u.exported && u.ident.as_ref().is_some_and(|i| tested.contains(i)))
            .map(|u| folder(&u.file, options))
            .collect()
    } else {
        HashSet::new()
    };
    for n in &mut draft.nodes {
        if grouped {
            n.touched_by_test = tested_folders.contains(&n.symbol);
        }
        main_first(n, &changes);
    }
    let test_declarations = facts::test_declarations(git, &head, &changes)?;
    let owner: HashMap<&str, &str> = draft
        .nodes
        .iter()
        .flat_map(|n| {
            n.sites
                .iter()
                .map(move |s| (s.file.as_str(), n.id.as_str()))
        })
        .collect();
    let facts = Facts {
        test_declarations: &test_declarations,
        units: drafts
            .iter()
            .flat_map(|d| {
                d.units.iter().map(|u| FactUnit {
                    node: if grouped {
                        owner[u.file.as_str()]
                    } else {
                        &d.node.id
                    },
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
    let map_path = options
        .map
        .clone()
        .unwrap_or_else(|| store.join(format!("{head}.json")));
    if let Some(parent) = map_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let kept = !options.fresh
        && std::fs::read_to_string(&map_path)
            .ok()
            .and_then(|t| serde_json::from_str::<ChangeMap>(&t).ok())
            .is_some_and(|m| m.base == base && m.path == draft.path);
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
        if grouped {
            format!("{} (see facts)", skipped.len())
        } else if skipped.is_empty() {
            "none".to_owned()
        } else {
            skipped.join(", ")
        }
    );
    println!("tour: {}", draft.tour.join(" "));
    println!("{}", context(&store));
    if grouped {
        println!(
            "grouping: --group-by dir --depth {}{}; --split <prefix> (repeat, first match wins); --group-by symbol to disable",
            if options.depth == 0 { 2 } else { options.depth },
            options
                .split
                .iter()
                .map(|p| format!(" --split {p}"))
                .collect::<String>()
        );
        println!("largest folders (by changed hunks):");
        let mut counts: HashMap<String, usize> = HashMap::new();
        for u in &changes.units {
            *counts.entry(folder(&u.file, options)).or_default() += 1;
        }
        let mut counts: Vec<_> = counts.into_iter().collect();
        counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        for (name, count) in counts.iter().take(10) {
            println!("  {name}: {count} hunks");
        }
        println!(
            "node diffs: overview-me node <id> --map {}",
            map_path.display()
        );
        println!(
            "scope: --path <prefix>; facts include {} test declarations, {} skipped files",
            test_declarations.len(),
            changes.skipped.len()
        );
        return Ok(map_path);
    }
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
            n.lane,
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
            n.lane,
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
            weight: None,
        }
    }

    #[test]
    fn large_prefix_grouping_and_weights() {
        let options = Options {
            depth: 2,
            split: vec!["packages/a/src/auth".into(), "packages/a".into()],
            ..Default::default()
        };
        assert_eq!(
            folder("packages/a/src/auth/x.ts", &options),
            "packages/a/src/auth"
        );
        assert_eq!(folder("packages/ab/src/auth/x.ts", &options), "packages/ab");
        assert_eq!(folder("Cargo.toml", &options), "(root)");
        let repo = crate::testutil::TempRepo::new("large-groups");
        repo.write(
            "packages/a/src/a.ts",
            "export function a() { return 1 }\nexport function b() { return 2 }\n",
        );
        let base = repo.commit("base");
        repo.write(
            "packages/a/src/a.ts",
            "export function a() { return 3 }\nexport function b() { return 4 }\n",
        );
        repo.write(
            "packages/ab/src/c.ts",
            "import { a, b } from '../../a/src/a'\nexport function c() { return a() + b() }\n",
        );
        let head = repo.commit("head");
        let git = Git::new(repo.path());
        let rules = Rules::load(None).expect("rules");
        let options = Options {
            group_by: Some("dir".into()),
            depth: 2,
            fresh: true,
            ..Default::default()
        };
        let path = run(&git, &rules, &base, &head, None, None, &options).expect("extract");
        let mut map: ChangeMap =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("map");
        assert!(map.grouped);
        assert_eq!(map.nodes.len(), 2);
        assert_eq!(map.edges.len(), 1);
        assert_eq!(map.edges[0].weight, Some(2));
        for n in &mut map.nodes {
            n.caption = "module changed".into();
        }
        std::fs::write(&path, serde_json::to_string(&map).expect("json")).expect("write");
        assert!(crate::validate::run(&git, &rules, &path).expect("validate"));
        let options = Options {
            path: Some("packages/a".into()),
            ..options
        };
        let path = run(&git, &rules, &base, &head, None, None, &options).expect("scoped extract");
        let map: ChangeMap =
            serde_json::from_str(&std::fs::read_to_string(path).expect("read")).expect("map");
        assert_eq!(map.nodes.len(), 1);
        assert!(
            map.nodes[0]
                .sites
                .iter()
                .all(|s| under(&s.file, "packages/a"))
        );
    }

    #[test]
    fn large_edge_pruning_keeps_strongest_calls() {
        let mut edges: Vec<Edge> = (0..30)
            .map(|i| Edge {
                from: format!("N{}", i / 3),
                to: format!("N{}", i + 20),
                kind: EdgeKind::Calls,
                weight: Some(i),
            })
            .collect();
        prune_edges(&mut edges);
        assert_eq!(edges.len(), 15);
        assert_eq!(edges[0].weight, Some(29));
        for e in &edges {
            assert!(edges.iter().filter(|other| other.from == e.from).count() <= 2);
        }
    }

    #[test]
    fn large_main_site_prefers_body_over_imports_and_errors() {
        let repo = crate::testutil::TempRepo::new("large-main-site");
        let base = repo.commit("base");
        repo.write(
            "packages/a/src/errors.ts",
            "export const Failure = {\n a: 1,\n b: 2,\n c: 3,\n d: 4,\n e: 5,\n f: 6,\n}\n",
        );
        repo.write("packages/a/src/main.ts", "import {\n a,\n b,\n c,\n d,\n e,\n} from './other'\nexport function main() {\n return a()\n}\nexport function second() { return b() }\n");
        let head = repo.commit("head");
        let git = Git::new(repo.path());
        let rules = Rules::load(None).expect("rules");
        let changes = facts::changes(&git, &rules, &base, &head).expect("changes");
        let options = Options {
            group_by: Some("dir".into()),
            ..Default::default()
        };
        let path = run(&git, &rules, &base, &head, None, None, &options).expect("extract");
        let map: ChangeMap =
            serde_json::from_str(&std::fs::read_to_string(path).expect("read")).expect("map");
        let main = &map.nodes[0].sites[0];
        assert_eq!(main.file, "packages/a/src/main.ts");
        assert_eq!((main.start, main.end), (8, 10));
        assert!(
            changes
                .units
                .iter()
                .filter(|u| u.file == main.file && main.start <= u.end && u.start <= main.end)
                .all(|u| !import_only(u, &changes))
        );
    }

    #[test]
    fn large_auto_threshold_includes_deleted_files() {
        let repo = crate::testutil::TempRepo::new("large-threshold");
        for i in 0..=LARGE_THRESHOLD {
            repo.write(&format!("packages/a/{i}.txt"), "removed\n");
        }
        let base = repo.commit("base");
        std::fs::remove_dir_all(repo.path().join("packages")).expect("remove files");
        let head = repo.commit("head");
        let path = run(
            &Git::new(repo.path()),
            &Rules::load(None).expect("rules"),
            &base,
            &head,
            None,
            None,
            &Options::default(),
        )
        .expect("extract");
        let map: ChangeMap =
            serde_json::from_str(&std::fs::read_to_string(path).expect("read")).expect("map");
        assert!(map.grouped);
        assert_eq!(map.nodes.len(), 1);
        assert_eq!(map.nodes[0].deletions.len(), LARGE_THRESHOLD + 1);
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
        let map_path = run(
            &git,
            &rules,
            &base,
            &head,
            None,
            Some(out),
            &Options::default(),
        )
        .expect("extract");
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
        run(&git, &rules, &base, &head, None, None, &Options::default()).expect("extract again");
        let again: ChangeMap =
            serde_json::from_str(&std::fs::read_to_string(&map_path).expect("map")).expect("json");
        assert_eq!(again.nodes[0].caption, "returns y(2)");
    }
}
