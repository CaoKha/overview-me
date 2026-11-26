//! `overview-me flow`: the Before and After flow diagrams of a change, drawn from its map and the
//! code at base and head, so the skill neither explores the code nor writes diagram JSON.
//!
//! A box is a map node (changed code), a symbol the change removes, or unchanged code one call
//! away from changed code. An arrow is a call found as `extract` finds `calls` edges: a reference
//! to a symbol's identifier whose binding can be that symbol (see [`crate::scope`]). Between two
//! map nodes in After, the map's `edges` have the last word. Grouped Before keeps base-side calls.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::facts;
use crate::git::Git;
use crate::map::{ChangeMap, MAX_AT, Node};
use crate::rules::{LANE_MAX, MAX_LANES, Rules};
use crate::scope::Scope;
use crate::symbols::{self, Ident, Parsed, Symbol};

/// Boxes per row of a lane: Before and After share the page width.
const PER_ROW: usize = 3;
/// Arrows per diagram that a reader can still trace.
const MAX_EDGES: usize = 15;
/// Unchanged boxes per diagram set (Before and After together).
const MAX_EXISTING: usize = 6;
/// Names and files searched for callers and callees, per side.
const MAX_NAMES: usize = 300;
const MAX_FILES: usize = 400;
/// Longest box label, in characters.
const LABEL_MAX: usize = 25;
/// Callers and callees listed in the summary.
const MAX_LINKS: usize = 12;

/// The file `overview-me flow` writes, read by `overview-me page` for `@before` / `@after`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowFile {
    /// The flow at base; `None` when the change adds a new flow.
    pub before: Option<Value>,
    pub after: Value,
    /// Changed code at head that nothing in the repo is found to call.
    pub entry_points: Vec<Link>,
    /// Unchanged code that calls changed code at head.
    pub callers: Vec<Link>,
    /// Unchanged code that changed code calls at head.
    pub depends_on: Vec<Link>,
    /// References at head to a symbol the change removes, with no new definition of that name.
    pub still_referenced: Vec<Link>,
    /// Map nodes left out of the diagrams, with the reason.
    pub not_drawn: Vec<String>,
    /// The label and note `flow` wrote for each map node's box, by side: a later run keeps a
    /// box's text when it differs from these, since a reviewer edited it.
    pub generated: Value,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Link {
    pub symbol: String,
    pub at: String,
    /// Map node ids on the other end (empty for entry points).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<String>,
}

/// One box's code at one side.
#[derive(Debug, Default, Clone)]
struct Code {
    /// Line ranges of the box's symbols: references inside them are its calls.
    spans: Vec<(String, usize, usize)>,
    /// The symbols other code calls it by.
    decls: Vec<(String, Symbol)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Changed,
    Added,
    Removed,
    Existing,
}

#[derive(Debug)]
struct Item {
    /// Internal key: a node id, `rm:<file>#<name>` or `ex:<file>#<name>`.
    key: String,
    label: String,
    /// The Before label when the base side lacks `label`'s symbol: its own code's names.
    base_label: Option<String>,
    n: Option<String>,
    note: Option<String>,
    lane: String,
    /// A table, cache or queue: drawn rounded.
    store: bool,
    /// Layout order inside a lane: group (node, removed, existing), tour position, key.
    order: (u8, usize),
    kind: Kind,
    head: Option<Code>,
    base: Option<Code>,
    head_paths: Vec<String>,
    base_paths: Vec<String>,
}

/// Calls found at one side.
#[derive(Debug, Default)]
struct Graph {
    edges: BTreeSet<(String, String)>,
    /// The first references behind each edge, as `path:line`.
    at: BTreeMap<(String, String), Vec<String>>,
    /// Unchanged symbols on the other end of an edge, by internal key.
    existing: BTreeMap<String, (String, Symbol)>,
}

impl Graph {
    fn add(&mut self, from: String, to: String, path: &str, line: usize) {
        let at = self.at.entry((from.clone(), to.clone())).or_default();
        let site = format!("{path}:{line}");
        if at.len() < MAX_AT && !at.contains(&site) {
            at.push(site);
        }
        self.edges.insert((from, to));
    }
}

enum Owner {
    Box(String),
    Existing(String, Symbol),
}

/// Reads the map, writes `<store>/<head>.flow.json` (or `out`), prints a summary, and returns
/// the path, the map and the flow file's content.
///
/// # Errors
/// When the map cannot be read or parsed, its shas are not commits, or git fails.
pub fn run(
    git: &Git,
    rules: &Rules,
    map_path: &Path,
    out: Option<PathBuf>,
) -> Result<(PathBuf, ChangeMap, FlowFile)> {
    let text = std::fs::read_to_string(map_path)
        .with_context(|| format!("reading {}", map_path.display()))?;
    let map: ChangeMap =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", map_path.display()))?;
    for (name, sha) in [("base", &map.base), ("head", &map.head)] {
        if !crate::validate::is_sha(sha) || git.rev(sha).ok().as_ref() != Some(sha) {
            bail!("{name} must be the full sha of a commit, got {sha:?}");
        }
    }
    let mut flow = build(git, rules, &map)?;
    let out = out.unwrap_or_else(|| map_path.with_file_name(format!("{}.flow.json", map.head)));
    let previous: Option<Value> = std::fs::read_to_string(&out)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let kept = previous.map_or(0, |p| keep_edits(&mut flow, &p));
    std::fs::write(&out, serde_json::to_string_pretty(&flow)? + "\n")
        .with_context(|| format!("writing {}", out.display()))?;
    print_summary(&out, &flow);
    if kept > 0 {
        println!("kept {kept} hand edits from the previous flow file");
    }
    Ok((out, map, flow))
}

/// Each map node box's label and note, by side.
fn generated(before: Option<&Value>, after: &Value) -> Value {
    let side = |spec: Option<&Value>| -> Value {
        let mut out = Map::new();
        for (k, b) in spec
            .and_then(|s| s["nodes"].as_object())
            .into_iter()
            .flatten()
        {
            if b.get("n").is_some() {
                out.insert(k.clone(), json!({ "label": b["label"], "note": b["note"] }));
            }
        }
        Value::Object(out)
    };
    json!({ "before": side(before), "after": side(Some(after)) })
}

/// Carries a reviewer's edits from the previous flow file into `flow`: a map node box's label or
/// note that differs from what `flow` generated then, when the box is on the same side again,
/// and arrow labels on arrows that are still drawn. Returns how many it kept.
fn keep_edits(flow: &mut FlowFile, previous: &Value) -> usize {
    let mut kept = 0;
    for (name, spec) in [
        ("before", flow.before.as_mut()),
        ("after", Some(&mut flow.after)),
    ] {
        let (Some(spec), Some(old)) = (spec, previous.get(name).filter(|v| v.is_object())) else {
            continue;
        };
        let generated = &previous["generated"][name];
        if let Some(nodes) = spec["nodes"].as_object_mut() {
            for (k, b) in nodes.iter_mut() {
                let was = &old["nodes"][k];
                if b.get("n").is_none() || was["n"] != b["n"] || generated.get(k).is_none() {
                    continue;
                }
                for field in ["label", "note"] {
                    if was[field] != generated[k][field] && was[field] != b[field] {
                        if was[field].is_null() {
                            b.as_object_mut().map(|o| o.remove(field));
                        } else {
                            b[field] = was[field].clone();
                        }
                        kept += 1;
                    }
                }
            }
        }
        let old_edges = old["edges"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        for e in spec["edges"].as_array_mut().into_iter().flatten() {
            let Some(label) = old_edges
                .iter()
                .find(|o| o[0] == e[0] && o[1] == e[1])
                .and_then(|o| o.get(2))
                .filter(|l| l.is_string())
            else {
                continue;
            };
            if let Some(arr) = e.as_array_mut() {
                arr.resize(arr.len().max(3), Value::Null);
                if &arr[2] != label {
                    arr[2] = label.clone();
                    kept += 1;
                }
            }
        }
    }
    kept
}

fn print_summary(out: &Path, flow: &FlowFile) {
    let count = |spec: &Value, k: &str| {
        spec.get(k).map_or(0, |v| match v {
            Value::Object(m) => m.len(),
            Value::Array(a) => a.len(),
            _ => 0,
        })
    };
    println!("flow: {}", out.display());
    match &flow.before {
        Some(b) => println!(
            "before: {} boxes, {} arrows",
            count(b, "nodes"),
            count(b, "edges")
        ),
        None => println!("before: none (the flow is new)"),
    }
    println!(
        "after:  {} boxes, {} arrows",
        count(&flow.after, "nodes"),
        count(&flow.after, "edges")
    );
    let section = |title: &str, links: &[Link], arrow: &str| {
        if links.is_empty() {
            return;
        }
        println!("{title}:");
        for l in links {
            let ends = if l.nodes.is_empty() {
                String::new()
            } else {
                format!(" {arrow} {}", l.nodes.join(", "))
            };
            println!("  {} `{}`{ends}", l.symbol, l.at);
        }
    };
    section("entry points (no caller found)", &flow.entry_points, "");
    section("callers (unchanged)", &flow.callers, "→");
    section("depends on (unchanged)", &flow.depends_on, "←");
    section("removed but still referenced", &flow.still_referenced, "");
    for n in &flow.not_drawn {
        println!("not drawn: {n}");
    }
}

/// `file:start-end`, or `file:line` when both ends are the same line.
pub(crate) fn span_path(file: &str, a: usize, b: usize) -> String {
    if a == b {
        format!("{file}:{a}")
    } else {
        format!("{file}:{a}-{b}")
    }
}

/// Keeps the end of a long name: `Class.method` reads better than `Class.me…`.
fn label(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= LABEL_MAX {
        return name.to_owned();
    }
    let tail: String = chars[chars.len() - (LABEL_MAX - 1)..].iter().collect();
    format!("…{tail}")
}

/// What a node's Before box is called when its symbol is not at base: the base symbols it
/// edits (whole names that fit, then `+N` for the rest), else the stem of its first base
/// file. `None` keeps the node's symbol.
fn base_label(symbol: &str, base: Option<&Code>, base_paths: &[String]) -> Option<String> {
    let mut names: Vec<&str> = Vec::new();
    for (_, s) in base.map(|c| c.decls.as_slice()).unwrap_or_default() {
        if !names.contains(&s.name.as_str()) {
            names.push(&s.name);
        }
    }
    if names.contains(&symbol) {
        return None;
    }
    if !names.is_empty() {
        let with_rest = |k: usize| match names.len() - k {
            0 => names[..k].join(", "),
            more => format!("{} +{more}", names[..k].join(", ")),
        };
        let fits = (1..=names.len())
            .rev()
            .map(with_rest)
            .find(|t| t.chars().count() <= LABEL_MAX);
        return Some(fits.unwrap_or_else(|| label(&with_rest(1))));
    }
    let first = base_paths.first()?;
    let file = first.split_once(':').map_or(first.as_str(), |(f, _)| f);
    let name = Path::new(file).file_name()?.to_str()?;
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    Some(label(if stem.is_empty() { name } else { stem }))
}

fn parse_at(git: &Git, rev: &str, path: &str) -> Result<Option<Parsed>> {
    match git.show(rev, path) {
        Some(src) => symbols::parse(path, &src),
        None => Ok(None),
    }
}

fn load_naming<'a>(
    git: &Git,
    rules: &Rules,
    rev: &str,
    names: impl Iterator<Item = &'a str>,
    files: &mut BTreeMap<String, Parsed>,
) -> Result<()> {
    let mut names = names.take(MAX_NAMES).peekable();
    if names.peek().is_none() {
        return Ok(());
    }
    let mut args = vec!["grep", "-l", "-w", "-F"];
    for name in names {
        args.extend(["-e", name]);
    }
    args.extend(["--end-of-options", rev, "--"]);
    let listed = git.run_or_empty(&args)?;
    let prefix = format!("{rev}:");
    let mut paths: Vec<&str> = listed
        .lines()
        .filter_map(|l| l.strip_prefix(&prefix))
        .filter(|p| symbols::is_code(p) && !rules.is_test(p) && !files.contains_key(*p))
        .collect();
    paths.sort_unstable();
    paths.truncate(MAX_FILES);
    for p in paths {
        if let Some(parsed) = parse_at(git, rev, p)? {
            files.insert(p.to_owned(), parsed);
        }
    }
    Ok(())
}

type References<'a> = HashMap<&'a str, Vec<(&'a str, &'a Parsed, &'a Ident)>>;

fn index_references(files: &BTreeMap<String, Parsed>) -> References<'_> {
    let mut refs = References::new();
    for (path, parsed) in files {
        for ident in &parsed.identifiers {
            refs.entry(ident.name.as_str())
                .or_default()
                .push((path, parsed, ident));
        }
    }
    refs
}

/// Whether a reference to `sym` is a step in a flow: a call to a function, method, macro or
/// constructor, or a read of a constant. Naming a type, trait, impl block or module is not.
fn callable(sym: &Symbol) -> bool {
    !matches!(sym.kind, "type" | "trait" | "impl" | "mod")
}

/// Whether the identifier `name` at `line` is the declaration of a symbol, not a reference.
fn is_declaration(p: &Parsed, name: &str, line: usize) -> bool {
    p.symbols.iter().any(|s| {
        s.ident == name
            && s.start <= line
            && line <= s.end
            && p.identifiers
                .iter()
                .find(|i| i.name == name && i.line >= s.start)
                .is_some_and(|i| i.line == line)
    })
}

/// Calls into and out of the boxes at `rev`. Loads the files that may hold callers or callees
/// into `files`.
fn side(
    git: &Git,
    rules: &Rules,
    rev: &str,
    scope: &Scope<'_>,
    boxes: &[(String, Code)],
    files: &mut BTreeMap<String, Parsed>,
) -> Result<Graph> {
    // Names to search: what the boxes are called by, and what they call.
    let mut names: BTreeSet<String> = BTreeSet::new();
    let mut local_names = HashMap::new();
    for (_, code) in boxes {
        for (_, d) in &code.decls {
            names.insert(d.ident.clone());
        }
        for (file, a, b) in &code.spans {
            let Some(p) = files.get(file) else { continue };
            let local = local_names.entry(file.as_str()).or_insert_with(|| {
                p.symbols
                    .iter()
                    .map(|s| s.ident.as_str())
                    .collect::<HashSet<_>>()
            });
            for i in &p.identifiers {
                if (a..=b).contains(&&i.line)
                    && (p.imports.contains(&i.name) || local.contains(i.name.as_str()))
                {
                    names.insert(i.name.clone());
                }
            }
        }
    }
    load_naming(git, rules, rev, names.iter().map(String::as_str), files)?;
    let files = &*files;

    let mut spans: HashMap<&str, Vec<(usize, usize, &str)>> = HashMap::new();
    for (key, code) in boxes {
        for (file, a, b) in &code.spans {
            spans.entry(file).or_default().push((*a, *b, key));
        }
    }
    let in_box = |file: &str, a: usize, b: usize| {
        spans
            .get(file)
            .is_some_and(|spans| spans.iter().any(|(sa, sb, _)| *sa <= b && a <= *sb))
    };
    let owner = |file: &str, line: usize| -> Option<Owner> {
        let smallest = spans
            .get(file)
            .into_iter()
            .flatten()
            .filter(|(a, b, _)| *a <= line && line <= *b)
            .map(|(a, b, key)| (b - a, *key))
            .min();
        if let Some((_, k)) = smallest {
            return Some(Owner::Box(k.to_owned()));
        }
        let p = files.get(file)?;
        if p.in_tests(line) {
            return None;
        }
        let s = p.symbol_at(line)?;
        (!in_box(file, s.start, s.end)).then(|| Owner::Existing(file.to_owned(), s.clone()))
    };

    let refs = index_references(files);
    let mut decls: HashMap<&str, Vec<(&str, &Symbol)>> = HashMap::new();
    for (path, p) in files {
        for s in &p.symbols {
            decls
                .entry(s.ident.as_str())
                .or_default()
                .push((path.as_str(), s));
        }
    }

    let mut g = Graph::default();
    // Into the boxes: every in-scope reference to a box's symbols.
    for (key, code) in boxes {
        for (dfile, d) in code.decls.iter().filter(|(_, d)| callable(d)) {
            for &(path, p, r) in refs.get(d.ident.as_str()).into_iter().flatten() {
                if !scope.reaches(path, p, r, dfile, d) || is_declaration(p, &d.ident, r.line) {
                    continue;
                }
                match owner(path, r.line) {
                    Some(Owner::Box(k)) if &k != key => {
                        g.add(k, key.clone(), path, r.line);
                    }
                    Some(Owner::Existing(f, s)) => {
                        let ek = format!("ex:{f}#{}", s.name);
                        g.add(ek.clone(), key.clone(), path, r.line);
                        g.existing.entry(ek).or_insert((f, s));
                    }
                    _ => {}
                }
            }
        }
    }
    // Out of the boxes: references inside a box to one unchanged symbol in scope.
    for (key, code) in boxes {
        let own: HashSet<&str> = code.decls.iter().map(|(_, d)| d.ident.as_str()).collect();
        for (file, a, b) in &code.spans {
            let Some(p) = files.get(file) else { continue };
            for r in &p.identifiers {
                if !(a..=b).contains(&&r.line) || own.contains(r.name.as_str()) {
                    continue;
                }
                let targets: Vec<(&str, &Symbol)> = decls
                    .get(r.name.as_str())
                    .into_iter()
                    .flatten()
                    .filter(|(df, d)| {
                        callable(d)
                            && scope.reaches(file, p, r, df, d)
                            && !in_box(df, d.start, d.end)
                    })
                    .copied()
                    .collect();
                // Two definitions in scope: the name alone cannot say which one is called.
                if let [(df, d)] = targets.as_slice() {
                    let ek = format!("ex:{df}#{}", d.name);
                    g.add(key.clone(), ek.clone(), file, r.line);
                    g.existing
                        .entry(ek)
                        .or_insert(((*df).to_owned(), (*d).clone()));
                }
            }
        }
    }
    Ok(g)
}

/// References at `rev` to removed symbols: in scope, not their own declaration, and with no
/// symbol of that name still declared (a move or a rename keeps its callers working).
fn dangling(
    git: &Git,
    rules: &Rules,
    rev: &str,
    scope: &Scope<'_>,
    removed: &[(String, Symbol)],
    files: &mut BTreeMap<String, Parsed>,
) -> Result<Vec<Link>> {
    if removed.is_empty() {
        return Ok(Vec::new());
    }
    let names: BTreeSet<&str> = removed.iter().map(|(_, s)| s.ident.as_str()).collect();
    load_naming(git, rules, rev, names.into_iter(), files)?;
    let declared: HashSet<&str> = files
        .values()
        .flat_map(|p| p.symbols.iter().map(|s| s.ident.as_str()))
        .collect();
    let refs = index_references(files);
    let mut out: Vec<Link> = Vec::new();
    for (file, sym) in removed {
        if declared.contains(sym.ident.as_str()) {
            continue;
        }
        for &(path, p, r) in refs.get(sym.ident.as_str()).into_iter().flatten() {
            if !p.in_tests(r.line)
                && !is_declaration(p, &r.name, r.line)
                && scope.reaches(path, p, r, file, sym)
            {
                out.push(Link {
                    symbol: sym.name.clone(),
                    at: format!("{path}:{}", r.line),
                    nodes: Vec::new(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.at.cmp(&b.at));
    out.dedup();
    out.truncate(MAX_LINKS);
    Ok(out)
}

/// The flow diagrams for `map`.
///
/// # Errors
/// When git fails.
pub fn build(git: &Git, rules: &Rules, map: &ChangeMap) -> Result<FlowFile> {
    let changes = facts::changes_in(git, rules, &map.base, &map.head, map.path.as_deref())?;
    let merge_base = changes.merge_base.as_str();
    let old_of: HashMap<String, String> = changes
        .files
        .iter()
        .map(|f| (f.path.clone(), f.old_path.clone()))
        .collect();
    let old = |f: &str| old_of.get(f).cloned().unwrap_or_else(|| f.to_owned());
    let mut head_files: BTreeMap<String, Parsed> = BTreeMap::new();
    for f in changes.files {
        if let Some(p) = f.parsed {
            head_files.insert(f.path, p);
        }
    }
    // Which old paths exist at base, and their parsed code.
    let mut wanted: BTreeSet<String> = old_of.values().cloned().collect();
    wanted.extend(changes.deleted.iter().cloned());
    let mut args = vec![
        "ls-tree",
        "-r",
        "--name-only",
        "--end-of-options",
        merge_base,
        "--",
    ];
    args.extend(wanted.iter().map(String::as_str));
    let at_base: HashSet<String> = if wanted.is_empty() {
        HashSet::new()
    } else {
        git.run(&args)?.lines().map(str::to_owned).collect()
    };
    let mut base_files: BTreeMap<String, Parsed> = BTreeMap::new();
    for p in &wanted {
        if at_base.contains(p)
            && symbols::is_code(p)
            && let Some(parsed) = parse_at(git, merge_base, p)?
        {
            base_files.insert(p.clone(), parsed);
        }
    }

    let tour: HashMap<&str, usize> = map
        .tour
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();
    let node_item = |node: &Node| Item {
        key: node.id.clone(),
        label: label(&node.symbol),
        base_label: None,
        n: Some(node.id.clone()),
        note: Some(node.caption.clone()).filter(|c| !c.trim().is_empty()),
        lane: node.lane.clone(),
        store: false,
        order: (0, tour.get(node.id.as_str()).copied().unwrap_or(usize::MAX)),
        kind: Kind::Changed,
        head: None,
        base: None,
        head_paths: Vec::new(),
        base_paths: Vec::new(),
    };
    let mut items: Vec<Item> = Vec::new();
    let mut not_drawn: Vec<String> = Vec::new();
    // Symbols the change removes, with the head path their callers would import them from.
    let mut removed: Vec<(String, Symbol)> = Vec::new();
    for node in &map.nodes {
        let db = node
            .sites
            .iter()
            .map(|s| &s.file)
            .chain(node.deletions.iter().map(|d| &d.file))
            .any(|f| rules.is_store(f));
        let mut head = Code::default();
        let mut base = Code::default();
        let (mut head_paths, mut base_paths) = (Vec::new(), Vec::new());
        let (mut code_sites, mut top_level) = (0, 0);
        for s in &node.sites {
            let (start, end) = (s.start as usize, s.end as usize);
            head_paths.push(span_path(&s.file, start, end));
            let old_file = old(&s.file);
            let Some(p) = head_files.get(&s.file) else {
                if db && at_base.contains(&old_file) && !base_paths.contains(&old_file) {
                    base_paths.push(old_file);
                }
                continue;
            };
            code_sites += 1;
            let mut syms: Vec<&Symbol> = Vec::new();
            for l in [start, end] {
                if let Some(sym) = p.symbol_at(l)
                    && !syms.contains(&sym)
                {
                    syms.push(sym);
                }
            }
            if syms.is_empty() {
                top_level += 1;
                if at_base.contains(&old_file) && !base_paths.contains(&old_file) {
                    base_paths.push(old_file.clone());
                }
            }
            for sym in syms {
                head.spans.push((s.file.clone(), sym.start, sym.end));
                head.decls.push((s.file.clone(), sym.clone()));
                if let Some(b) = base_files.get(&old_file).and_then(|bp| {
                    bp.symbols
                        .iter()
                        .find(|b| b.name == sym.name && b.kind == sym.kind)
                }) {
                    base.spans.push((old_file.clone(), b.start, b.end));
                    base.decls.push((old_file.clone(), b.clone()));
                    base_paths.push(span_path(&old_file, b.start, b.end));
                }
            }
        }
        for d in &node.deletions {
            if let Some(bp) = base_files.get(&d.file) {
                for s in bp.symbols.iter().filter(|s| s.name == s.ident) {
                    base.spans.push((d.file.clone(), s.start, s.end));
                    base.decls.push((d.file.clone(), s.clone()));
                    removed.push((d.file.clone(), s.clone()));
                }
                base_paths.push(d.file.clone());
            } else if db {
                base_paths.push(d.file.clone());
            }
        }
        let top_level_only = code_sites > 0 && code_sites == top_level && !db;
        if top_level_only {
            not_drawn.push(format!("{} (top-level code only)", node.id));
            if !map.grouped || base.decls.is_empty() {
                continue;
            }
        }
        let on_head = code_sites > 0 || (db && !node.sites.is_empty());
        let on_base = !base.decls.is_empty() || !base_paths.is_empty();
        let kind = match (on_head, on_base) {
            (true, true) => Kind::Changed,
            (true, false) => Kind::Added,
            (false, true) => Kind::Removed,
            (false, false) => {
                if !node.renames.is_empty() || !node.sites.is_empty() {
                    not_drawn.push(format!("{} (no code in a parsed language)", node.id));
                }
                continue;
            }
        };
        items.push(Item {
            store: db,
            kind,
            head: (on_head && !top_level_only).then_some(head),
            base: on_base.then_some(base),
            head_paths,
            base_paths,
            ..node_item(node)
        });
    }

    // Code around the map's nodes sits in the lane of the node in its file, so a lane the
    // reviewer renamed stays one lane.
    let mut file_lane: HashMap<&str, &str> = HashMap::new();
    for node in &map.nodes {
        for f in node.sites.iter().map(|s| &s.file) {
            file_lane.entry(f).or_insert(&node.lane);
        }
    }
    let lane_of = |f: &str| {
        file_lane
            .get(f)
            .map_or_else(|| changes.lanes.lane(f), |l| (*l).to_owned())
    };

    // Symbols the change removes from files it keeps: outermost ones only.
    for (path, p) in &head_files {
        let old_file = old(path);
        let Some(bp) = base_files.get(&old_file) else {
            continue;
        };
        let gone: Vec<&Symbol> = bp
            .symbols
            .iter()
            .filter(|b| !bp.in_tests(b.start) && !p.symbols.iter().any(|s| s.name == b.name))
            .collect();
        let owners: BTreeSet<&str> = map
            .nodes
            .iter()
            .filter(|n| n.sites.iter().any(|s| &s.file == path))
            .map(|n| n.id.as_str())
            .collect();
        for s in &gone {
            let nested = gone.iter().any(|o| {
                o.start <= s.start && s.end <= o.end && (o.start, o.end) != (s.start, s.end)
            });
            if nested {
                continue;
            }
            removed.push((path.clone(), (*s).clone()));
            if map.grouped {
                if !items.iter().any(|i| owners.contains(i.key.as_str()))
                    && let Some(node) = map.nodes.iter().find(|n| owners.contains(n.id.as_str()))
                {
                    items.push(Item {
                        store: rules.is_store(&old_file),
                        ..node_item(node)
                    });
                }
                if let Some(item) = items.iter_mut().find(|i| owners.contains(i.key.as_str())) {
                    let code = item.base.get_or_insert_with(Code::default);
                    code.spans.push((old_file.clone(), s.start, s.end));
                    code.decls.push((old_file.clone(), (*s).clone()));
                    item.base_paths.push(span_path(&old_file, s.start, s.end));
                    item.kind = Kind::Changed;
                } else {
                    not_drawn.push(format!(
                        "{}#{} (removed symbol has no module)",
                        old_file, s.name
                    ));
                }
                continue;
            }
            items.push(Item {
                key: format!("rm:{old_file}#{}", s.name),
                label: label(&s.name),
                base_label: None,
                n: (owners.len() == 1)
                    .then(|| owners.iter().next().map(|s| (*s).to_owned()))
                    .flatten(),
                note: None,
                lane: lane_of(path),
                store: rules.is_store(&old_file),
                order: (1, s.start),
                kind: Kind::Removed,
                head: None,
                base: Some(Code {
                    spans: vec![(old_file.clone(), s.start, s.end)],
                    decls: vec![(old_file.clone(), (*s).clone())],
                }),
                head_paths: Vec::new(),
                base_paths: vec![span_path(&old_file, s.start, s.end)],
            });
        }
    }

    for item in &mut items {
        // Before sorting, so the names keep the map's site order, as the box's paths do.
        if let Some(node) = map.nodes.iter().find(|n| n.id == item.key) {
            item.base_label = base_label(&node.symbol, item.base.as_ref(), &item.base_paths);
        }
        for code in [&mut item.head, &mut item.base].into_iter().flatten() {
            code.spans.sort();
            code.spans.dedup();
            code.decls
                .sort_by(|a, b| (&a.0, a.1.start, &a.1.name).cmp(&(&b.0, b.1.start, &b.1.name)));
            code.decls.dedup();
        }
    }
    let boxes_at = |head_side: bool| -> Vec<(String, Code)> {
        items
            .iter()
            .filter_map(|i| {
                let code = if head_side { &i.head } else { &i.base };
                code.as_ref().map(|c| (i.key.clone(), c.clone()))
            })
            .collect()
    };
    let head_scope = Scope::load(git, &map.head)?;
    let base_scope = Scope::load(git, merge_base)?;
    let mut head_graph = side(
        git,
        rules,
        &map.head,
        &head_scope,
        &boxes_at(true),
        &mut head_files,
    )?;
    let still_referenced = dangling(
        git,
        rules,
        &map.head,
        &head_scope,
        &removed,
        &mut head_files,
    )?;
    let mut base_graph = side(
        git,
        rules,
        merge_base,
        &base_scope,
        &boxes_at(false),
        &mut base_files,
    )?;
    // Pruning changes what is drawn, not whether a base call still exists at head.
    let unpruned_head = map.grouped.then(|| Graph {
        edges: head_graph.edges.clone(),
        ..Graph::default()
    });
    // In After, an arrow between two map nodes needs an edge in the map: deleting a wrong
    // edge from the map removes its arrow for good.
    let node_ids: HashSet<&str> = map.nodes.iter().map(|n| n.id.as_str()).collect();
    let mapped: HashSet<(&str, &str)> = map
        .edges
        .iter()
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect();
    head_graph.edges.retain(|(a, b)| {
        !(node_ids.contains(a.as_str()) && node_ids.contains(b.as_str()))
            || mapped.contains(&(a.as_str(), b.as_str()))
    });

    if map.grouped {
        base_graph
            .edges
            .retain(|(a, b)| !a.starts_with("ex:") && !b.starts_with("ex:"));
        head_graph
            .edges
            .retain(|(a, b)| mapped.contains(&(a.as_str(), b.as_str())));
    }

    // Unchanged boxes: the most connected ones, while both diagrams stay traceable.
    let mut score: BTreeMap<&str, usize> = BTreeMap::new();
    for (a, b) in head_graph.edges.iter().chain(&base_graph.edges) {
        for k in [a, b] {
            if k.starts_with("ex:") {
                *score.entry(k).or_default() += 1;
            }
        }
    }
    let mut ranked: Vec<(&str, usize)> = score.into_iter().collect();
    ranked.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(y.0)));
    let mut selected: Vec<&str> = ranked.iter().take(MAX_EXISTING).map(|r| r.0).collect();
    let known: HashSet<&str> = items.iter().map(|i| i.key.as_str()).collect();
    let drawn = |g: &Graph, sel: &[&str]| {
        g.edges
            .iter()
            .filter(|(a, b)| {
                [a, b]
                    .iter()
                    .all(|k| known.contains(k.as_str()) || sel.contains(&k.as_str()))
            })
            .count()
    };
    while !selected.is_empty()
        && (drawn(&head_graph, &selected) > MAX_EDGES || drawn(&base_graph, &selected) > MAX_EDGES)
    {
        selected.pop();
    }
    let touches = |g: &Graph, k: &str| g.edges.iter().any(|(a, b)| a == k || b == k);
    for k in &selected {
        let (file, sym) = head_graph
            .existing
            .get(*k)
            .or_else(|| base_graph.existing.get(*k))
            .cloned()
            .unwrap_or_default();
        let paths = |g: &Graph| {
            g.existing
                .get(*k)
                .map(|(f, s)| vec![span_path(f, s.start, s.end)])
                .unwrap_or_default()
        };
        let on_head = touches(&head_graph, k);
        let on_base = touches(&base_graph, k);
        items.push(Item {
            key: (*k).to_owned(),
            label: label(&sym.name),
            base_label: None,
            n: None,
            note: None,
            lane: lane_of(&file),
            store: rules.is_store(&file),
            order: (2, 0),
            kind: Kind::Existing,
            head: on_head.then(Code::default),
            base: on_base.then(Code::default),
            head_paths: paths(&head_graph),
            base_paths: paths(&base_graph),
        });
    }

    let lane_index = arrange_lanes(&mut items, [&head_graph, &base_graph]);
    items.sort_by(|a, b| {
        (lane_index[&a.lane], a.order, &a.key).cmp(&(lane_index[&b.lane], b.order, &b.key))
    });
    // Short keys: node ids stay, removed symbols are r1…, unchanged code x1….
    let mut short: HashMap<String, String> = HashMap::new();
    let (mut r, mut x) = (0, 0);
    for i in &items {
        let k = match i.kind {
            Kind::Existing => {
                x += 1;
                format!("x{x}")
            }
            _ if i.key.starts_with("rm:") => {
                r += 1;
                format!("r{r}")
            }
            _ => i.key.clone(),
        };
        short.insert(i.key.clone(), k);
    }

    let before_has_change = items
        .iter()
        .any(|i| i.base.is_some() && i.kind != Kind::Existing);
    let mut before = None;
    if before_has_change {
        let (spec, dropped) = diagram(
            &items,
            &short,
            &base_graph,
            unpruned_head.as_ref().unwrap_or(&head_graph),
            false,
        );
        if dropped > 0 {
            not_drawn.push(format!("{dropped} arrows in Before (over {MAX_EDGES})"));
        }
        before = Some(spec);
    }
    let (after, dropped) = diagram(&items, &short, &head_graph, &base_graph, true);
    if dropped > 0 {
        not_drawn.push(format!("{dropped} arrows in After (over {MAX_EDGES})"));
    }

    let mut entry_points = Vec::new();
    for i in items
        .iter()
        .filter(|i| i.head.is_some() && node_ids.contains(i.key.as_str()))
    {
        if !head_graph.edges.iter().any(|(_, b)| b == &i.key) {
            entry_points.push(Link {
                symbol: format!("{} {}", i.key, i.label),
                at: i.head_paths.first().cloned().unwrap_or_default(),
                nodes: Vec::new(),
            });
        }
    }
    let links = |into: bool| -> Vec<Link> {
        let mut by: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for (a, b) in &head_graph.edges {
            let (ex, node) = if into { (a, b) } else { (b, a) };
            if ex.starts_with("ex:") && node_ids.contains(node.as_str()) {
                by.entry(ex).or_default().insert(node);
            }
        }
        let mut out: Vec<Link> = by
            .into_iter()
            .filter_map(|(ex, nodes)| {
                let (f, s) = head_graph.existing.get(ex)?;
                Some(Link {
                    symbol: s.name.clone(),
                    at: span_path(f, s.start, s.end),
                    nodes: nodes.into_iter().map(str::to_owned).collect(),
                })
            })
            .collect();
        out.sort_by(|a, b| b.nodes.len().cmp(&a.nodes.len()).then(a.at.cmp(&b.at)));
        out.truncate(MAX_LINKS);
        out
    };
    Ok(FlowFile {
        generated: generated(before.as_ref(), &after),
        before,
        after,
        entry_points,
        callers: links(true),
        depends_on: links(false),
        still_referenced,
        not_drawn,
    })
}

/// Merges lanes past [`MAX_LANES`] and orders them top to bottom, returning each lane's
/// position. A lane sits above the lanes it calls; where calls do not decide (none, or a
/// cycle), the lane met first in the tour goes first.
/// The smallest lane merges into the lane it calls or is called by most, else the biggest.
fn arrange_lanes(items: &mut [Item], graphs: [&Graph; 2]) -> HashMap<String, usize> {
    let at: HashMap<&str, usize> = items
        .iter()
        .enumerate()
        .map(|(n, i)| (i.key.as_str(), n))
        .collect();
    let calls: Vec<(usize, usize)> = graphs
        .iter()
        .flat_map(|g| &g.edges)
        .filter_map(|(a, b)| Some((*at.get(a.as_str())?, *at.get(b.as_str())?)))
        .collect();
    // Lanes in their usual order, with their box count.
    let usual = |items: &[Item]| -> Vec<(String, usize)> {
        // Per lane: its first box in the tour, and its box count.
        let mut lanes: BTreeMap<&str, ((u8, usize), usize)> = BTreeMap::new();
        for i in items {
            let e = lanes.entry(&i.lane).or_insert((i.order, 0));
            e.0 = e.0.min(i.order);
            e.1 += 1;
        }
        let mut v: Vec<_> = lanes.into_iter().collect();
        v.sort_by_key(|(l, (first, _))| (*first, *l));
        v.into_iter().map(|(l, (_, n))| (l.to_owned(), n)).collect()
    };
    let between = |items: &[Item], x: &str, y: &str| {
        calls
            .iter()
            .filter(|(a, b)| {
                let (la, lb) = (items[*a].lane.as_str(), items[*b].lane.as_str());
                (la == x && lb == y) || (la == y && lb == x)
            })
            .count()
    };
    loop {
        let lanes = usual(items);
        if lanes.len() <= MAX_LANES {
            break;
        }
        // The smallest lane, the last in order on a tie.
        let Some((small, _)) = lanes.iter().rev().min_by_key(|(_, n)| *n) else {
            break;
        };
        let Some((into, _)) = lanes
            .iter()
            .filter(|(l, _)| l != small)
            .rev()
            .max_by_key(|(l, n)| (between(items, small, l), *n))
        else {
            break;
        };
        let joined = format!("{into} · {small}");
        let name = if joined.chars().count() <= LANE_MAX {
            joined
        } else {
            into.clone()
        };
        for i in items.iter_mut() {
            if &i.lane == small || &i.lane == into {
                i.lane.clone_from(&name);
            }
        }
    }
    let mut left: Vec<String> = usual(items).into_iter().map(|(l, _)| l).collect();
    let mut ordered: Vec<String> = Vec::new();
    while !left.is_empty() {
        let called = |lane: &str| {
            calls.iter().any(|(a, b)| {
                let caller = items[*a].lane.as_str();
                items[*b].lane == lane && caller != lane && left.iter().any(|l| l == caller)
            })
        };
        let next = left.iter().position(|l| !called(l)).unwrap_or(0);
        ordered.push(left.remove(next));
    }
    ordered
        .into_iter()
        .enumerate()
        .map(|(n, l)| (l, n))
        .collect()
}

/// One `flow` block: the boxes present at this side, in lanes of rows of `PER_ROW`, and the
/// arrows between them, styled against the other side's arrows.
fn diagram(
    items: &[Item],
    short: &HashMap<String, String>,
    graph: &Graph,
    other: &Graph,
    after: bool,
) -> (Value, usize) {
    let present: Vec<&Item> = items
        .iter()
        .filter(|i| {
            if after {
                i.head.is_some()
            } else {
                i.base.is_some()
            }
        })
        .collect();
    let mut nodes = Map::new();
    for i in &present {
        let mut kind: Vec<&str> = Vec::new();
        match i.kind {
            Kind::Added if after => kind.push("added"),
            Kind::Removed => kind.push("removed"),
            Kind::Existing => kind.push("existing"),
            _ => {}
        }
        if i.store {
            kind.push("store");
        }
        let mut box_ = Map::new();
        let label = if after { None } else { i.base_label.as_ref() };
        box_.insert("label".into(), json!(label.unwrap_or(&i.label)));
        if let Some(n) = &i.n {
            box_.insert("n".into(), json!(n));
        }
        if !kind.is_empty() {
            box_.insert("kind".into(), json!(kind.join(" ")));
        }
        // The caption says what the change does: Before shows it only when no After box does.
        if let Some(note) = i.note.as_ref().filter(|_| after || i.head.is_none()) {
            box_.insert("note".into(), json!(note));
        }
        let paths = if after { &i.head_paths } else { &i.base_paths };
        box_.insert("paths".into(), json!(paths));
        nodes.insert(short[&i.key].clone(), Value::Object(box_));
    }
    let mut tiers: Vec<Value> = Vec::new();
    let mut row_of: HashMap<&str, usize> = HashMap::new();
    let mut lanes: Vec<(&str, Vec<&Item>)> = Vec::new();
    for i in &present {
        match lanes.last_mut() {
            Some((l, v)) if *l == i.lane => v.push(i),
            _ => lanes.push((&i.lane, vec![i])),
        }
    }
    for (lane, members) in lanes {
        for chunk in members.chunks(PER_ROW) {
            for i in chunk {
                row_of.insert(i.key.as_str(), tiers.len());
            }
            let ids: Vec<&str> = chunk.iter().map(|i| short[&i.key].as_str()).collect();
            tiers.push(json!([{ "name": lane, "ids": ids }]));
        }
    }
    let mut edges: Vec<(usize, &String, &String, Option<&str>)> = graph
        .edges
        .iter()
        .filter(|(a, b)| row_of.contains_key(a.as_str()) && row_of.contains_key(b.as_str()))
        .map(|(a, b)| {
            let style = (!other.edges.contains(&(a.clone(), b.clone()))).then_some(if after {
                "added"
            } else {
                "removed"
            });
            (row_of[a.as_str()], a, b, style)
        })
        .collect();
    // New and removed arrows first: they are the change. Then the rest, top to bottom.
    edges.sort_by_key(|e| (e.3.is_none(), e.0, e.1, e.2));
    let dropped = edges.len().saturating_sub(MAX_EDGES);
    edges.truncate(MAX_EDGES);
    edges.sort();
    let edges: Vec<Value> = edges
        .into_iter()
        .map(
            |(_, a, b, style)| match (style, graph.at.get(&(a.clone(), b.clone()))) {
                (style, Some(at)) if !at.is_empty() => json!([short[a], short[b], null, style, at]),
                (Some(s), _) => json!([short[a], short[b], null, s]),
                (None, _) => json!([short[a], short[b]]),
            },
        )
        .collect();
    (
        json!({ "nodes": nodes, "tiers": tiers, "edges": edges }),
        dropped,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempRepo;

    const FEE_BASE: &str = "export function fee(x: number) {\n  return x * 2\n}\n\nexport function oldFee(x: number) {\n  return x\n}\n";
    const FEE_HEAD: &str = "export function fee(x: number) {\n  return x * 3\n}\n\nexport function discount(x: number) {\n  return x - 1\n}\n";
    const HANDLER_BASE: &str = "import { fee, oldFee } from '../services/fee'\nimport { save } from '../db/store'\n\nexport function handle(x: number) {\n  save(fee(x))\n  return oldFee(x)\n}\n";
    const HANDLER_HEAD: &str = "import { fee, discount } from '../services/fee'\nimport { save } from '../db/store'\n\nexport function handle(x: number) {\n  save(fee(x))\n  return discount(x)\n}\n";
    const STORE: &str = "export function save(v: number) {\n  return v\n}\n";

    /// The draft map `extract` writes.
    fn draft(repo: &TempRepo, base: &str, head: &str) -> ChangeMap {
        let git = Git::new(repo.path());
        let rules = Rules::load(None).expect("rules");
        let out = repo.path().join("facts.json");
        let path = crate::extract::run(
            &git,
            &rules,
            base,
            head,
            None,
            Some(out),
            &crate::extract::Options {
                fresh: true,
                ..Default::default()
            },
        )
        .expect("extract");
        serde_json::from_str(&std::fs::read_to_string(path).expect("map")).expect("json")
    }

    fn node_id(map: &ChangeMap, symbol: &str) -> String {
        map.nodes
            .iter()
            .find(|n| n.symbol == symbol)
            .map(|n| n.id.clone())
            .unwrap_or_else(|| panic!("no node {symbol}: {:?}", map.nodes))
    }

    #[test]
    fn lanes_merge_to_the_limit_and_callers_sit_above() {
        let lanes = ["api", "billing", "db", "x1", "x2", "x3", "x4"];
        let mut items: Vec<Item> = lanes
            .iter()
            .enumerate()
            .map(|(n, lane)| Item {
                key: (*lane).to_owned(),
                label: (*lane).to_owned(),
                base_label: None,
                n: None,
                note: None,
                lane: (*lane).to_owned(),
                store: false,
                order: (0, n),
                kind: Kind::Changed,
                head: None,
                base: None,
                head_paths: Vec::new(),
                base_paths: Vec::new(),
            })
            .collect();
        let independent = arrange_lanes(&mut items[..3], [&Graph::default(), &Graph::default()]);
        assert!(
            independent["billing"] < independent["db"],
            "tour order breaks ties"
        );
        let graph = Graph {
            edges: [("api", "billing"), ("billing", "db"), ("api", "x4")]
                .into_iter()
                .map(|(a, b)| (a.to_owned(), b.to_owned()))
                .collect(),
            ..Graph::default()
        };
        let index = arrange_lanes(&mut items, [&graph, &Graph::default()]);
        let mut got: Vec<(&String, &usize)> = index.iter().collect();
        got.sort_by_key(|(_, n)| **n);
        let got: Vec<&str> = got.into_iter().map(|(l, _)| l.as_str()).collect();
        // x4 joins the lane that calls it; x3 calls nothing and joins the biggest lane.
        assert_eq!(got, ["api · x4 · x3", "billing", "db", "x1", "x2"]);
        assert_eq!(items[6].lane, "api · x4 · x3");
    }

    fn edges(spec: &Value) -> Vec<String> {
        spec["edges"]
            .as_array()
            .expect("edges")
            .iter()
            .map(|e| {
                let style = e.get(3).and_then(Value::as_str).unwrap_or("");
                format!(
                    "{}>{} {style}",
                    e[0].as_str().unwrap_or(""),
                    e[1].as_str().unwrap_or("")
                )
                .trim()
                .to_owned()
            })
            .collect()
    }

    #[test]
    fn before_and_after_from_the_code() {
        let repo = TempRepo::new("flow");
        repo.write("src/services/fee.ts", FEE_BASE);
        repo.write("src/api/handler.ts", HANDLER_BASE);
        repo.write("src/db/store.ts", STORE);
        let base = repo.commit("base");
        repo.write("src/services/fee.ts", FEE_HEAD);
        repo.write("src/api/handler.ts", HANDLER_HEAD);
        let head = repo.commit("head");
        let map = draft(&repo, &base, &head);
        let (handle, fee, discount) = (
            node_id(&map, "handle"),
            node_id(&map, "fee"),
            node_id(&map, "discount"),
        );

        let flow = build(
            &Git::new(repo.path()),
            &Rules::load(None).expect("rules"),
            &map,
        )
        .expect("flow");
        let after = &flow.after;
        assert_eq!(after["nodes"][&discount]["kind"], "added");
        assert!(
            after["nodes"][&fee].get("kind").is_none(),
            "fee is changed: {after}"
        );
        assert_eq!(after["nodes"]["x1"]["label"], "save");
        assert_eq!(after["nodes"]["x1"]["kind"], "existing store");
        assert_eq!(
            after["nodes"]["x1"]["paths"],
            json!(["src/db/store.ts:1-3"])
        );
        let mut got = edges(after);
        got.sort();
        let mut want = vec![
            format!("{handle}>{discount} added"),
            format!("{handle}>{fee}"),
            format!("{handle}>x1"),
        ];
        want.sort();
        assert_eq!(got, want);
        // Lanes top to bottom: api, then services, then db.
        let lanes: Vec<&str> = after["tiers"]
            .as_array()
            .expect("tiers")
            .iter()
            .map(|t| t[0]["name"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(lanes, ["api", "services", "db"]);

        let before = flow.before.as_ref().expect("the flow existed at base");
        assert_eq!(before["nodes"]["r1"]["label"], "oldFee");
        assert_eq!(before["nodes"]["r1"]["kind"], "removed");
        assert_eq!(
            before["nodes"]["r1"]["paths"],
            json!(["src/services/fee.ts:5-7"])
        );
        assert!(before["nodes"].get(&discount).is_none());
        let mut got = edges(before);
        got.sort();
        let mut want = vec![
            format!("{handle}>r1 removed"),
            format!("{handle}>{fee}"),
            format!("{handle}>x1"),
        ];
        want.sort();
        assert_eq!(got, want);

        assert_eq!(flow.entry_points.len(), 1, "{:?}", flow.entry_points);
        assert!(flow.entry_points[0].symbol.starts_with(&handle));
        assert_eq!(
            flow.depends_on,
            vec![Link {
                symbol: "save".into(),
                at: "src/db/store.ts:1-3".into(),
                nodes: vec![handle.clone()],
            }]
        );
        // The diagrams pass the page's own checks.
        let md = format!(
            "```flow\n{}\n```\n```flow\n{}\n```\n",
            flow.after,
            flow.before.expect("before")
        );
        assert_eq!(crate::page::check_flows(&md), Vec::<String>::new());
    }

    #[test]
    fn a_new_flow_has_no_before() {
        let repo = TempRepo::new("flow-new");
        repo.write("README.md", "x\n");
        let base = repo.commit("base");
        repo.write("src/services/fee.ts", FEE_HEAD);
        let head = repo.commit("head");
        let map = draft(&repo, &base, &head);
        let flow = build(
            &Git::new(repo.path()),
            &Rules::load(None).expect("rules"),
            &map,
        )
        .expect("flow");
        assert!(flow.before.is_none());
        assert_eq!(flow.after["nodes"].as_object().map(Map::len), Some(2));
    }

    #[test]
    fn removed_symbols_still_called_are_reported() {
        let repo = TempRepo::new("flow-dangling");
        repo.write("src/services/fee.ts", FEE_BASE);
        repo.write("src/api/handler.ts", HANDLER_BASE);
        repo.write("src/db/store.ts", STORE);
        let base = repo.commit("base");
        repo.write("src/services/fee.ts", FEE_HEAD);
        let head = repo.commit("head");
        let map = draft(&repo, &base, &head);
        let flow = build(
            &Git::new(repo.path()),
            &Rules::load(None).expect("rules"),
            &map,
        )
        .expect("flow");
        let at: Vec<&str> = flow
            .still_referenced
            .iter()
            .map(|l| l.at.as_str())
            .collect();
        assert_eq!(at, ["src/api/handler.ts:1", "src/api/handler.ts:6"]);
        assert!(flow.still_referenced.iter().all(|l| l.symbol == "oldFee"));
    }

    #[test]
    fn large_dangling_ignores_local_bindings() {
        let repo = TempRepo::new("large-dangling-local");
        repo.write("src/a.ts", "export const description = 'old'\nexport const generator = 1\nexport const ipConnectionCount = new Map()\n");
        let base = repo.commit("base");
        repo.write("src/a.ts", "export function run() {\n const description = 'new'\n const generator = 2\n const ipConnectionCount = new Map()\n return [description, generator, ipConnectionCount]\n}\nexport function parameter({ description }: { description: string }) { return description }\n");
        let head = repo.commit("head");
        let map = draft(&repo, &base, &head);
        let flow = flow_of(&repo, &map);
        assert!(
            flow.still_referenced.is_empty(),
            "{:?}",
            flow.still_referenced
        );
        let mut modules = map;
        modules.grouped = true;
        let flow = flow_of(&repo, &modules);
        assert!(
            flow.before.as_ref().expect("before")["nodes"]
                .as_object()
                .expect("nodes")
                .keys()
                .all(|key| key.starts_with('N'))
        );
    }

    #[test]
    fn dangling_defaults_loops_same_line_and_members() {
        for (i, body) in [
            "export function f({ r = RATE }) { return r }",
            "export function f({ key: r = RATE }) { return r }",
            "export function f([r = RATE]) { return r }",
            "export function f() { for (let RATE = 0; RATE < 2; RATE++) {} return RATE }",
            "export function f() { for (let RATE of [1]) {} return RATE }",
            "export function f() { for (let RATE in {}) {} return RATE }",
            "export function f(RATE) { return RATE } export function g() { return RATE }",
            "export function f() { let RATE = 1; return RATE } export function g() { return RATE }",
        ]
        .iter()
        .enumerate()
        {
            let repo = TempRepo::new(&format!("dangling-regression-{i}"));
            repo.write("a.ts", "export const RATE = 1\n");
            let base = repo.commit("base");
            repo.write("a.ts", &format!("{body}\n"));
            let head = repo.commit("head");
            let flow = flow_of(&repo, &draft(&repo, &base, &head));
            assert!(!flow.still_referenced.is_empty(), "missing RATE: {body}");
        }
        let repo = TempRepo::new("dangling-member-regression");
        repo.write("a.ts", "export function removed() { return 1 }\n");
        repo.write(
            "b.ts",
            "import * as ns from './a'\nexport function f(removed) { return ns.removed() }\n",
        );
        let base = repo.commit("base");
        repo.write("a.ts", "export function replacement() { return 2 }\n");
        let head = repo.commit("head");
        let flow = flow_of(&repo, &draft(&repo, &base, &head));
        assert!(
            flow.still_referenced
                .iter()
                .any(|link| link.symbol == "removed" && link.at == "b.ts:2")
        );
    }

    #[test]
    fn dangling_keeps_local_patterns_and_loop_bindings() {
        for (i, body) in [
            "export function f({ key: RATE }, ...rest) { return RATE + rest.length }",
            "export function f([RATE, ...rest]) { return RATE + rest.length }",
            "export function f() { for (let RATE of [1]) { console.log(RATE) } }",
            "export function f() { for (var RATE of [1]) {} return RATE }",
            "export function f() { for (var RATE = 0; RATE < 1; RATE++) {} return RATE }",
        ]
        .iter()
        .enumerate()
        {
            let repo = TempRepo::new(&format!("dangling-local-pattern-{i}"));
            repo.write("a.ts", "export const RATE = 1\nexport const rest = 2\n");
            let base = repo.commit("base");
            repo.write("a.ts", &format!("{body}\n"));
            let head = repo.commit("head");
            let flow = flow_of(&repo, &draft(&repo, &base, &head));
            assert!(
                flow.still_referenced.is_empty(),
                "{body}: {:?}",
                flow.still_referenced
            );
        }
    }

    #[test]
    fn grouped_removals_survive_skipped_and_added_owners() {
        for (i, body, expected) in [
            (0, "console.log('new')\n", "changed"),
            (1, "export function replacement() { return 2 }\n", "changed"),
        ] {
            let repo = TempRepo::new(&format!("grouped-removal-{i}"));
            repo.write("src/a.ts", "export function gone() { return 1 }\n");
            let base = repo.commit("base");
            repo.write("src/a.ts", body);
            let head = repo.commit("head");
            let mut map = draft(&repo, &base, &head);
            map.grouped = true;
            let flow = flow_of(&repo, &map);
            let before = flow.before.expect("removed module has Before");
            assert_eq!(
                before["nodes"][&map.nodes[0].id]["kind"].as_str(),
                (expected == "removed").then_some("removed")
            );
            assert!(flow.after["nodes"][&map.nodes[0].id]["kind"].is_null());
            if i == 0 {
                assert!(
                    flow.not_drawn
                        .iter()
                        .any(|n| n.ends_with("(top-level code only)"))
                );
                assert!(flow.after["nodes"].get(&map.nodes[0].id).is_none());
            }
        }
    }

    #[test]
    fn grouped_before_keeps_removed_calls() {
        let repo = TempRepo::new("grouped-before-calls");
        repo.write("src/a.ts", "export function a() { return 1 }\n");
        repo.write(
            "src/b.ts",
            "import { a } from './a'\nexport function b() { return a() }\n",
        );
        let base = repo.commit("base");
        repo.write("src/a.ts", "export function a() { return 2 }\n");
        repo.write(
            "src/b.ts",
            "import { a } from './a'\nexport function b() { return 3 }\n",
        );
        let head = repo.commit("head");
        let mut map = draft(&repo, &base, &head);
        map.grouped = true;
        assert!(map.edges.is_empty());
        let flow = flow_of(&repo, &map);
        let before = flow.before.expect("before");
        assert_eq!(before["edges"].as_array().expect("edges").len(), 1);
        assert_eq!(before["edges"][0][3], "removed");
        assert!(flow.after["edges"].as_array().expect("edges").is_empty());
    }

    #[test]
    fn grouped_before_does_not_mark_pruned_calls_removed() {
        let repo = TempRepo::new("pruned-before");
        for n in ["a", "b", "c"] {
            repo.write(
                &format!("packages/{n}/{n}.ts"),
                &format!("export function {n}() {{ return 1 }}\n"),
            );
        }
        let caller = "import { a } from '../../packages/a/a'\nimport { b } from '../../packages/b/b'\nimport { c } from '../../packages/c/c'\nexport function x() { return a() + b() + c()";
        repo.write("apps/x/x.ts", &format!("{caller} + 1 }}\n"));
        let base = repo.commit("base");
        for n in ["a", "b", "c"] {
            repo.write(
                &format!("packages/{n}/{n}.ts"),
                &format!("export function {n}() {{ return 2 }}\n"),
            );
        }
        repo.write("apps/x/x.ts", &format!("{caller} + 2 }}\n"));
        let head = repo.commit("head");
        let mut map = draft(&repo, &base, &head);
        map.grouped = true;
        assert_eq!(map.edges.len(), 3);
        map.edges.pop();
        let flow = flow_of(&repo, &map);
        let before = flow.before.expect("before");
        assert_eq!(before["edges"].as_array().expect("edges").len(), 3);
        assert!(
            before["edges"]
                .as_array()
                .expect("edges")
                .iter()
                .all(|e| e.get(3).is_none_or(Value::is_null))
        );
        assert_eq!(flow.after["edges"].as_array().expect("edges").len(), 2);
    }

    #[test]
    fn grouped_mixed_deletion_and_top_level_keeps_before() {
        let repo = TempRepo::new("mixed-top-level");
        repo.write("src/gone.ts", "export function gone() { return 1 }\n");
        repo.write("src/keep.ts", "console.log(1)\n");
        let base = repo.commit("base");
        std::fs::remove_file(repo.path().join("src/gone.ts")).expect("remove");
        repo.write("src/keep.ts", "console.log(2)\n");
        let head = repo.commit("head");
        let path = crate::extract::run(
            &Git::new(repo.path()),
            &Rules::load(None).expect("rules"),
            &base,
            &head,
            None,
            None,
            &crate::extract::Options {
                group_by: Some(crate::extract::GroupBy::Dir),
                ..Default::default()
            },
        )
        .expect("extract");
        let map: ChangeMap =
            serde_json::from_str(&std::fs::read_to_string(path).expect("read")).expect("map");
        assert_eq!(map.nodes.len(), 1);
        let flow = flow_of(&repo, &map);
        let before = flow.before.expect("deleted declaration keeps Before");
        assert!(before["nodes"][&map.nodes[0].id]["kind"].is_null());
        assert!(
            flow.not_drawn
                .iter()
                .any(|n| n.ends_with("(top-level code only)"))
        );
    }

    fn flow_of(repo: &TempRepo, map: &ChangeMap) -> FlowFile {
        build(
            &Git::new(repo.path()),
            &Rules::load(None).expect("rules"),
            map,
        )
        .expect("flow")
    }

    /// Moves the sites of the node at `file` into the node `into`, as an agent grouping them would.
    fn group_into(map: &mut ChangeMap, into: &str, file: &str) {
        let at = map
            .nodes
            .iter()
            .position(|n| n.id != into && n.sites.iter().any(|s| s.file == file))
            .unwrap_or_else(|| panic!("no node at {file}: {:?}", map.nodes));
        let from = map.nodes.remove(at);
        map.edges.retain(|e| e.from != from.id && e.to != from.id);
        map.tour.retain(|id| id != &from.id);
        let node = map.nodes.iter_mut().find(|n| n.id == into).expect("into");
        node.sites.extend(from.sites);
        node.caption = "Adds newFn and calls it".to_owned();
    }

    #[test]
    fn before_label_names_base_code_not_new_symbol() {
        let repo = TempRepo::new("before-label-base");
        repo.write("src/old.ts", "export function oldFn() {\n  return 1\n}\n");
        let base = repo.commit("base");
        repo.write("src/old.ts", "export function oldFn() {\n  return 2\n}\n");
        repo.write("src/fresh.ts", "export function newFn() {\n  return 3\n}\n");
        let head = repo.commit("head");
        let mut map = draft(&repo, &base, &head);
        let id = node_id(&map, "newFn");
        group_into(&mut map, &id, "src/old.ts");
        let flow = flow_of(&repo, &map);
        let before = flow.before.expect("before");
        assert_eq!(before["nodes"][&id]["label"], "oldFn", "{before}");
        assert!(before["nodes"][&id].get("note").is_none(), "{before}");
        assert_eq!(flow.after["nodes"][&id]["label"], "newFn");
        assert_eq!(flow.after["nodes"][&id]["note"], "Adds newFn and calls it");
    }

    #[test]
    fn before_label_falls_back_to_the_file_stem() {
        let repo = TempRepo::new("before-label-stem");
        repo.write("src/router.ts", "console.log(1)\n");
        let base = repo.commit("base");
        repo.write("src/router.ts", "console.log(2)\n");
        repo.write("src/fresh.ts", "export function newFn() {\n  return 3\n}\n");
        let head = repo.commit("head");
        let mut map = draft(&repo, &base, &head);
        let id = node_id(&map, "newFn");
        group_into(&mut map, &id, "src/router.ts");
        let flow = flow_of(&repo, &map);
        let before = flow.before.expect("before");
        assert_eq!(before["nodes"][&id]["label"], "router", "{before}");
        assert_eq!(flow.after["nodes"][&id]["label"], "newFn");
    }

    #[test]
    fn before_label_keeps_a_symbol_changed_in_place() {
        let repo = TempRepo::new("before-label-same");
        repo.write("src/services/fee.ts", FEE_BASE);
        let base = repo.commit("base");
        repo.write("src/services/fee.ts", FEE_HEAD);
        let head = repo.commit("head");
        let map = draft(&repo, &base, &head);
        let fee = node_id(&map, "fee");
        let flow = flow_of(&repo, &map);
        assert_eq!(flow.before.expect("before")["nodes"][&fee]["label"], "fee");
        assert_eq!(flow.after["nodes"][&fee]["label"], "fee");
    }

    #[test]
    fn base_label_cuts_a_name_list_at_whole_names() {
        let code = |names: &[&str]| Code {
            spans: Vec::new(),
            decls: names
                .iter()
                .map(|n| {
                    let sym = Symbol {
                        name: (*n).to_owned(),
                        ..Symbol::default()
                    };
                    ("a.ts".to_owned(), sym)
                })
                .collect(),
        };
        let got = |names: &[&str]| base_label("newFn", Some(&code(names)), &[]);
        assert_eq!(
            got(&["Route", "Route", "useKyc"]).as_deref(),
            Some("Route, useKyc")
        );
        assert_eq!(
            got(&["Route", "RouteComponent", "useSubscribeToKyc"]).as_deref(),
            Some("Route, RouteComponent +1")
        );
        assert_eq!(
            got(&["aVeryLongSymbolNameIndeed", "b"]).as_deref(),
            Some("…yLongSymbolNameIndeed +1")
        );
        assert_eq!(got(&["Route", "newFn"]), None);
        let paths = ["src/drizzle.config.ts".to_owned()];
        assert_eq!(
            base_label("x", None, &paths).as_deref(),
            Some("drizzle.config")
        );
    }

    #[test]
    fn grouped_removal_box_gets_the_base_label() {
        let repo = TempRepo::new("before-label-grouped");
        repo.write("src/a.ts", "export function gone() { return 1 }\n");
        let base = repo.commit("base");
        repo.write("src/a.ts", "console.log('new')\n");
        let head = repo.commit("head");
        let mut map = draft(&repo, &base, &head);
        map.grouped = true;
        map.nodes[0].symbol = "newThing".to_owned();
        map.nodes[0].caption = "Replaces gone with a log".to_owned();
        let id = map.nodes[0].id.clone();
        let flow = flow_of(&repo, &map);
        let before = flow.before.expect("before");
        assert_eq!(before["nodes"][&id]["label"], "gone", "{before}");
        // No After box shows the caption, so Before keeps it.
        assert_eq!(before["nodes"][&id]["note"], "Replaces gone with a log");
    }

    #[test]
    fn same_name_in_two_packages_is_not_a_call() {
        let repo = TempRepo::new("flow-same-name");
        repo.write("packages/db/package.json", r#"{"name":"@t/db"}"#);
        repo.write(
            "packages/db/src/schema.ts",
            "function pgSchema(n: string) {\n  return { table: (t: string, c: object) => ({ n, t, c }) }\n}\n\nexport const referral = pgSchema('referral')\n",
        );
        repo.write(
            "apps/api/package.json",
            r#"{"name":"@t/api","dependencies":{"@t/db":"workspace:*"}}"#,
        );
        repo.write(
            "apps/api/src/routers/index.ts",
            "export { referral } from './referral.js'\n",
        );
        let referees = |cols: &str| {
            format!(
                "import {{ referral }} from './schema.js'\n\nexport const referees = referral.table('referees', {cols})\n"
            )
        };
        let router = |extra: &str| {
            format!("const get = () => 1\n\nexport const referral = {{\n  get,{extra}\n}}\n")
        };
        let user = |import: &str, body: &str| {
            format!("import {import}\n\nexport function use() {{\n  return {body}\n}}\n")
        };
        let files = |v: &str| {
            repo.write(
                "packages/db/src/referees.ts",
                &referees(if v == "base" { "{}" } else { "{ id: 1 }" }),
            );
            repo.write(
                "apps/api/src/routers/referral.ts",
                &router(if v == "base" { "" } else { "\n  open: true," }),
            );
            // A different edit in each file, so each gets its own node.
            let wrap = |b: &str, n: u8| {
                if v == "base" {
                    b.to_owned()
                } else {
                    format!("[{b}, {n}]")
                }
            };
            repo.write(
                "apps/api/src/direct.ts",
                &user(
                    "{ referral } from './routers/referral.js'",
                    &wrap("referral", 1),
                ),
            );
            repo.write(
                "apps/api/src/barrel.ts",
                &user(
                    "{ referral } from './routers/index.js'",
                    &wrap("referral", 2),
                ),
            );
            repo.write(
                "apps/api/src/db.ts",
                &user("{ referees } from '@t/db/referees'", &wrap("referees", 3)),
            );
        };
        files("base");
        let base = repo.commit("base");
        files("head");
        let head = repo.commit("head");
        let map = draft(&repo, &base, &head);
        let id = |file: &str| {
            map.nodes
                .iter()
                .find(|n| n.sites.iter().any(|s| s.file == file))
                .map(|n| n.id.clone())
                .unwrap_or_else(|| panic!("no node in {file}: {:?}", map.nodes))
        };
        let (table, router) = (
            id("packages/db/src/referees.ts"),
            id("apps/api/src/routers/referral.ts"),
        );
        let (direct, barrel, db) = (
            id("apps/api/src/direct.ts"),
            id("apps/api/src/barrel.ts"),
            id("apps/api/src/db.ts"),
        );

        let mapped: Vec<String> = map
            .edges
            .iter()
            .map(|e| format!("{}>{}", e.from, e.to))
            .collect();
        for e in [format!("{table}>{router}"), format!("{router}>{table}")] {
            assert!(!mapped.contains(&e), "draft edge {e}: {mapped:?}");
        }
        for e in [
            format!("{direct}>{router}"),
            format!("{barrel}>{router}"),
            format!("{db}>{table}"),
        ] {
            assert!(mapped.contains(&e), "draft misses {e}: {mapped:?}");
        }

        let flow = flow_of(&repo, &map);
        let after = edges(&flow.after);
        for e in &after {
            let ends: Vec<&str> = e.split([' ', '>']).collect();
            assert!(
                !(ends[..2].contains(&table.as_str()) && ends[..2].contains(&router.as_str())),
                "arrow {e} links two different `referral`s: {after:?}"
            );
        }
        for e in [
            format!("{direct}>{router}"),
            format!("{barrel}>{router}"),
            format!("{db}>{table}"),
        ] {
            assert!(after.contains(&e), "missing {e}: {after:?}");
        }
        // The table file uses the schema's `referral`; nothing else does.
        let schema: Vec<_> = flow
            .depends_on
            .iter()
            .filter(|l| l.at.starts_with("packages/db/src/schema.ts"))
            .map(|l| (l.symbol.as_str(), l.nodes.clone()))
            .collect();
        assert_eq!(schema, vec![("referral", vec![table.clone()])]);
        assert!(flow.callers.is_empty(), "{:?}", flow.callers);
    }

    #[test]
    fn property_and_parameter_of_the_same_name_are_not_calls() {
        let repo = TempRepo::new("flow-shadow");
        repo.write("packages/db/package.json", r#"{"name":"@t/db"}"#);
        repo.write(
            "apps/web/package.json",
            r#"{"name":"@t/web","dependencies":{"@t/db":"workspace:*"}}"#,
        );
        let files = |n: u8| {
            repo.write(
                "packages/db/src/schema.ts",
                &format!("export const referral = pgSchema('referral', {n})\n"),
            );
            // `api` comes through a path alias, which cannot be followed.
            repo.write(
                "apps/web/src/kyc.ts",
                &format!("import {{ api }} from '~/lib/api'\n\nexport const key = () => api.referral.get.key({n})\n"),
            );
            repo.write(
                "apps/web/src/guard.ts",
                &format!("import {{ api }} from '~/lib/api'\nimport {{ match }} from 'ts-pattern'\n\nexport const options = () => api.referral.get.queryOptions()\n\nexport const open = (referral: Referral) => match(referral, {n})\n"),
            );
            // Real uses: a namespace import through an alias, and a named import.
            repo.write(
                "apps/web/src/ns.ts",
                &format!(
                    "import * as db from '~/db'\n\nexport const ns = () => db.referral.table({n})\n"
                ),
            );
            repo.write(
                "apps/web/src/named.ts",
                &format!("import {{ referral }} from '@t/db/schema'\n\nexport const named = () => referral.table({n})\n"),
            );
        };
        files(1);
        let base = repo.commit("base");
        files(2);
        let head = repo.commit("head");
        let map = draft(&repo, &base, &head);
        let schema = node_id(&map, "referral");
        let mut callers: Vec<_> = map
            .edges
            .iter()
            .filter(|e| e.to == schema)
            .map(|e| symbol_of(&map, &e.from))
            .collect();
        callers.sort_unstable();
        assert_eq!(callers, ["named", "ns"]);
        let ns = map
            .edges
            .iter()
            .find(|e| e.to == schema && symbol_of(&map, &e.from) == "ns");
        assert_eq!(ns.expect("ns edge").at, ["apps/web/src/ns.ts:3"]);
        // The arrow carries the same reference into the page.
        let flow = flow_of(&repo, &map);
        let arrow = flow.after["edges"]
            .as_array()
            .expect("edges")
            .iter()
            .find(|e| e[0] == ns.expect("ns edge").from.as_str())
            .cloned()
            .expect("ns arrow");
        assert_eq!(arrow[4], json!(["apps/web/src/ns.ts:3"]), "{arrow}");
    }

    fn symbol_of<'m>(map: &'m ChangeMap, id: &str) -> &'m str {
        &map.nodes.iter().find(|n| n.id == id).expect("node").symbol
    }

    #[test]
    fn member_property_is_not_a_call() {
        let repo = TempRepo::new("flow-member");
        let src = |n: u8| {
            format!(
                "const proc = {{ referral: {{ get: 1 }} }}\n\nexport const get = proc.referral.get + {n}\n\nexport const referral = {{ get, n: {n} }}\n"
            )
        };
        repo.write("src/r.ts", &src(1));
        let base = repo.commit("base");
        repo.write("src/r.ts", &src(2));
        let head = repo.commit("head");
        let map = draft(&repo, &base, &head);
        let (get, referral) = (node_id(&map, "get"), node_id(&map, "referral"));
        let flow = flow_of(&repo, &map);
        let after = edges(&flow.after);
        assert!(after.contains(&format!("{referral}>{get}")), "{after:?}");
        assert!(
            !after
                .iter()
                .any(|e| e.starts_with(&format!("{get}>{referral}"))),
            "{after:?}"
        );
        assert!(
            flow.callers.is_empty(),
            "keys are not calls: {:?}",
            flow.callers
        );
    }

    #[test]
    fn rerunning_flow_keeps_hand_edits() {
        let repo = TempRepo::new("flow-keep-edits");
        repo.write("src/services/fee.ts", FEE_BASE);
        repo.write("src/api/handler.ts", HANDLER_BASE);
        repo.write("src/db/store.ts", STORE);
        let base = repo.commit("base");
        repo.write("src/services/fee.ts", FEE_HEAD);
        repo.write("src/api/handler.ts", HANDLER_HEAD);
        let head = repo.commit("head");
        let mut map = draft(&repo, &base, &head);
        let (handle, fee) = (node_id(&map, "handle"), node_id(&map, "fee"));
        let git = Git::new(repo.path());
        let rules = Rules::load(None).expect("rules");
        let map_path = repo.path().join("map.json");
        let out = repo.path().join("map.flow.json");
        let run = |map: &ChangeMap| {
            std::fs::write(&map_path, serde_json::to_string(map).expect("map")).expect("write");
            super::run(&git, &rules, &map_path, Some(out.clone())).expect("flow");
            serde_json::from_str::<Value>(&std::fs::read_to_string(&out).expect("read"))
                .expect("json")
        };
        let mut edited = run(&map);
        edited["after"]["nodes"][&handle]["label"] = json!("hand label");
        edited["after"]["nodes"][&handle]
            .as_object_mut()
            .expect("box")
            .remove("note");
        edited["after"]["edges"][0][2] = json!("reads");
        let arrow = (
            edited["after"]["edges"][0][0].clone(),
            edited["after"]["edges"][0][1].clone(),
        );
        std::fs::write(&out, edited.to_string()).expect("edit");

        map.nodes
            .iter_mut()
            .find(|n| n.id == fee)
            .expect("fee")
            .caption = "new caption".into();
        let again = run(&map);
        let b = &again["after"]["nodes"][&handle];
        assert_eq!(b["label"], "hand label", "{again}");
        assert!(b.get("note").is_none(), "a removed note stays removed: {b}");
        assert_eq!(
            again["after"]["nodes"][&fee]["note"], "new caption",
            "unedited boxes follow the map"
        );
        let e = again["after"]["edges"]
            .as_array()
            .expect("edges")
            .iter()
            .find(|e| (e[0].clone(), e[1].clone()) == arrow)
            .expect("arrow");
        assert_eq!(e[2], "reads");
    }

    #[test]
    fn map_edges_decide_arrows_between_nodes() {
        let repo = TempRepo::new("flow-map-edges");
        repo.write("src/services/fee.ts", FEE_BASE);
        repo.write("src/api/handler.ts", HANDLER_BASE);
        repo.write("src/db/store.ts", STORE);
        let base = repo.commit("base");
        repo.write("src/services/fee.ts", FEE_HEAD);
        repo.write("src/api/handler.ts", HANDLER_HEAD);
        let head = repo.commit("head");
        let mut map = draft(&repo, &base, &head);
        let (handle, fee, discount) = (
            node_id(&map, "handle"),
            node_id(&map, "fee"),
            node_id(&map, "discount"),
        );
        map.edges.retain(|e| !(e.from == handle && e.to == fee));
        let mut got = edges(&flow_of(&repo, &map).after);
        got.sort();
        let mut want = vec![format!("{handle}>{discount} added"), format!("{handle}>x1")];
        want.sort();
        assert_eq!(got, want, "the reviewer removed {handle}>{fee}");
    }

    #[test]
    fn labels_keep_the_end_of_long_names() {
        assert_eq!(label("short"), "short");
        let long = label("VeryLongContainerName.someMethodName");
        assert_eq!(long.chars().count(), LABEL_MAX);
        assert!(long.starts_with('…') && long.ends_with("someMethodName"));
    }
}
