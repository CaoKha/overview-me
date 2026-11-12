//! `overview-me validate`: the map must agree with git.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::facts::{self, Changes, Unit};
use crate::git::Git;
use crate::map::{ChangeMap, Status};
use crate::rules::Rules;

/// Reads, deserializes and checks a map. Prints every problem; returns whether it is valid.
///
/// # Errors
/// When the file cannot be read or git fails. Schema and semantic problems are not errors.
pub fn run(git: &Git, rules: &Rules, path: &Path) -> Result<bool> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let map: ChangeMap = match serde_json::from_str(&text) {
        Ok(map) => map,
        Err(err) => {
            eprintln!("{}: schema: {err}", path.display());
            return Ok(false);
        }
    };
    // base and head reach git: only full shas of real commits may pass, before any other call.
    for (name, sha) in [("base", &map.base), ("head", &map.head)] {
        if !is_sha(sha) || git.rev(sha).ok().as_ref() != Some(sha) {
            eprintln!(
                "{}: {name} must be the full sha of a commit, got {sha:?}",
                path.display()
            );
            return Ok(false);
        }
    }
    let changes = facts::changes(git, rules, &map.base, &map.head)?;
    let exported: HashSet<&str> = changes
        .units
        .iter()
        .filter(|u| u.exported)
        .filter_map(|u| u.ident.as_deref())
        .collect();
    let tested = facts::tested_names(git, &map.head, &changes.test_files, &exported)?;
    let read_spec = |p: &Path| std::fs::read_to_string(p).ok();
    let read_head = |file: &str| {
        git.run(&["show", "--end-of-options", &format!("{}:{file}", map.head)])
            .ok()
    };
    // Inline Rust test modules live in ordinary .rs files.
    let is_test_file = |file: &str| rules.is_test(file) || file.ends_with(".rs");
    let problems = check(
        &map,
        &changes,
        &tested,
        &read_spec,
        &read_head,
        &is_test_file,
    );
    if problems.is_empty() {
        println!(
            "ok: {} nodes, {} tour stops, {} criteria, {} hunks covered",
            map.nodes.len(),
            map.tour.len(),
            map.criteria.len(),
            changes.units.len()
        );
        return Ok(true);
    }
    eprintln!("{}: {} problem(s)", path.display(), problems.len());
    for p in &problems {
        eprintln!("  - {p}");
    }
    Ok(false)
}

fn is_numbered(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix)
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

fn is_sha(s: &str) -> bool {
    s.len() == 40
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Consecutive runs in sorted line numbers: `[1, 2, 5]` → `[(1, 2), (5, 5)]`.
fn runs(lines: &[u32]) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for &l in lines {
        match out.last_mut() {
            Some(last) if last.1 + 1 == l => last.1 = l,
            _ => out.push((l, l)),
        }
    }
    out
}

fn duplicates<'a>(ids: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut seen = HashSet::new();
    ids.filter(|id| !seen.insert(*id)).collect()
}

/// GitHub-style heading anchor: lowercase, spaces to `-`, other punctuation dropped.
pub fn slug(heading: &str) -> String {
    heading
        .trim()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            '-' => Some('-'),
            c if c.is_alphanumeric() => Some(c),
            _ => None,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

fn heading_slugs(markdown: &str) -> HashSet<String> {
    markdown
        .lines()
        .filter_map(|l| {
            let rest = l.trim_start_matches('#');
            (rest.len() < l.len() && l.len() - rest.len() <= 6 && rest.starts_with(' '))
                .then(|| slug(rest))
        })
        .collect()
}

fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest),
        _ => PathBuf::from(path),
    }
}

/// Semantic checks. `tested` holds the exported names that changed test files reference;
/// `read_spec` reads a spec file and `read_head` a repo file at the map's head.
pub fn check(
    map: &ChangeMap,
    changes: &Changes,
    tested: &HashSet<String>,
    read_spec: &dyn Fn(&Path) -> Option<String>,
    read_head: &dyn Fn(&str) -> Option<String>,
    is_test_file: &dyn Fn(&str) -> bool,
) -> Vec<String> {
    let mut problems: Vec<String> = Vec::new();
    if map.version != 1 {
        problems.push(format!("version must be 1, got {}", map.version));
    }
    for (name, sha) in [("base", &map.base), ("head", &map.head)] {
        if !is_sha(sha) {
            problems.push(format!(
                "{name} must be a full 40-character sha, got {sha:?}"
            ));
        }
    }
    for id in duplicates(map.nodes.iter().map(|n| n.id.as_str())) {
        problems.push(format!("duplicate node id {id}"));
    }
    for id in duplicates(map.criteria.iter().map(|c| c.id.as_str())) {
        problems.push(format!("duplicate criterion id {id}"));
    }
    let node_ids: HashSet<&str> = map.nodes.iter().map(|n| n.id.as_str()).collect();
    let criterion_ids: HashSet<&str> = map.criteria.iter().map(|c| c.id.as_str()).collect();

    // Sites: start and end are changed lines of one file.
    let in_unit = |file: &str, line: u32| {
        changes
            .units
            .iter()
            .any(|u| u.file == file && u.contains(line))
    };
    for n in &map.nodes {
        if !is_numbered(&n.id, "N") {
            problems.push(format!("node id {:?} must look like N1", n.id));
        }
        if n.caption.trim().is_empty() {
            problems.push(format!("{}: caption is empty", n.id));
        }
        if n.sites.is_empty() && n.deletions.is_empty() && n.renames.is_empty() {
            problems.push(format!(
                "{}: needs at least one site, deletion or rename",
                n.id
            ));
        }
        for r in n.renames.iter().filter(|r| !changes.renames.contains(r)) {
            problems.push(format!(
                "{}: rename {} -> {} is not a listed move in base...head",
                n.id, r.from, r.to
            ));
        }
        for d in n
            .deletions
            .iter()
            .filter(|d| !changes.deleted.contains(&d.file))
        {
            problems.push(format!(
                "{}: deletion {} is not a deleted non-test file in base...head",
                n.id, d.file
            ));
        }
        for s in &n.sites {
            let at = format!("{} site {}:{}-{}", n.id, s.file, s.start, s.end);
            if s.start > s.end {
                problems.push(format!("{at}: start is after end"));
            } else if !changes.units.iter().any(|u| u.file == s.file) {
                problems.push(format!(
                    "{at}: file has no changed non-test hunk in base...head"
                ));
            } else if !in_unit(&s.file, s.start) || !in_unit(&s.file, s.end) {
                problems.push(format!(
                    "{at}: start or end is not a changed line of any hunk"
                ));
            }
        }
    }

    // Coverage, per line: every added head line of a non-test unit (or the anchor of a
    // deletion-only unit) lies in exactly one node's sites.
    let mut owned: HashMap<&str, Vec<&Unit>> = HashMap::new();
    for u in &changes.units {
        let lines = if u.added.is_empty() {
            vec![u.start]
        } else {
            u.added.clone()
        };
        let mut uncovered: Vec<u32> = Vec::new();
        let mut twice: Vec<(u32, Vec<&str>)> = Vec::new();
        for line in lines {
            let owners: Vec<&str> = map
                .nodes
                .iter()
                .filter(|n| {
                    n.sites
                        .iter()
                        .any(|s| s.file == u.file && s.start <= line && line <= s.end)
                })
                .map(|n| n.id.as_str())
                .collect();
            match owners.as_slice() {
                [] => uncovered.push(line),
                [one] => {
                    let entry = owned.entry(one).or_default();
                    if !entry.iter().any(|o| std::ptr::eq(*o, u)) {
                        entry.push(u);
                    }
                }
                _ => twice.push((line, owners)),
            }
        }
        for (a, b) in runs(&uncovered) {
            problems.push(format!(
                "not covered: {}:{a}-{b} ({}) belongs to no node",
                u.file, u.symbol
            ));
        }
        let twice_lines: Vec<u32> = twice.iter().map(|t| t.0).collect();
        for (a, b) in runs(&twice_lines) {
            let ids = twice
                .iter()
                .find(|t| t.0 == a)
                .map(|t| t.1.join(", "))
                .unwrap_or_default();
            problems.push(format!(
                "covered twice: {}:{a}-{b} ({}) is in {ids}",
                u.file, u.symbol
            ));
        }
    }
    for r in &changes.renames {
        let owners: Vec<&str> = map
            .nodes
            .iter()
            .filter(|n| n.renames.contains(r))
            .map(|n| n.id.as_str())
            .collect();
        match owners.as_slice() {
            [] => problems.push(format!(
                "not covered: move {} -> {} belongs to no node",
                r.from, r.to
            )),
            [_] => {}
            many => problems.push(format!(
                "covered twice: move {} -> {} is in {}",
                r.from,
                r.to,
                many.join(", ")
            )),
        }
    }
    for file in &changes.deleted {
        let owners: Vec<&str> = map
            .nodes
            .iter()
            .filter(|n| n.deletions.iter().any(|d| &d.file == file))
            .map(|n| n.id.as_str())
            .collect();
        match owners.as_slice() {
            [] => problems.push(format!(
                "not covered: deleted file {file} belongs to no node"
            )),
            [_] => {}
            many => problems.push(format!(
                "covered twice: deleted file {file} is in {}",
                many.join(", ")
            )),
        }
    }

    for id in map.tour.iter().filter(|id| !node_ids.contains(id.as_str())) {
        problems.push(format!("tour: unknown node {id}"));
    }
    for id in duplicates(map.tour.iter().map(String::as_str)) {
        problems.push(format!("tour: {id} appears twice"));
    }
    for n in map.nodes.iter().filter(|n| !map.tour.contains(&n.id)) {
        problems.push(format!("tour: {} is missing", n.id));
    }
    for n in &map.nodes {
        for c in n
            .criteria
            .iter()
            .filter(|c| !criterion_ids.contains(c.as_str()))
        {
            problems.push(format!("{}: unknown criterion {c}", n.id));
        }
    }
    for e in &map.edges {
        for end in [&e.from, &e.to]
            .into_iter()
            .filter(|id| !node_ids.contains(id.as_str()))
        {
            problems.push(format!("edge {}->{}: unknown node {end}", e.from, e.to));
        }
    }

    // Tested flags are facts, and comment-only code proves nothing.
    let node_tested: HashMap<&str, bool> = map
        .nodes
        .iter()
        .map(|n| {
            let units = owned.get(n.id.as_str()).map_or(&[][..], Vec::as_slice);
            let t = units
                .iter()
                .any(|u| u.exported && u.ident.as_ref().is_some_and(|i| tested.contains(i)));
            (n.id.as_str(), t)
        })
        .collect();
    for n in &map.nodes {
        let want = node_tested.get(n.id.as_str()).copied().unwrap_or(false);
        if n.touched_by_test != want {
            problems.push(format!(
                "{}: touchedByTest must be {want} (from changed test files)",
                n.id
            ));
        }
        let units = owned.get(n.id.as_str()).map_or(&[][..], Vec::as_slice);
        if !n.criteria.is_empty() && !units.is_empty() && units.iter().all(|u| u.comment_only) {
            problems.push(format!(
                "{}: only touches comments, so it must not link a criterion",
                n.id
            ));
        }
    }

    for c in &map.criteria {
        if !is_numbered(&c.id, "REF-") {
            problems.push(format!("criterion id {:?} must look like REF-1", c.id));
        }
        if c.text.trim().is_empty() {
            problems.push(format!("{}: text is empty", c.id));
        }
        let linked: Vec<&str> = map
            .nodes
            .iter()
            .filter(|n| n.criteria.contains(&c.id))
            .map(|n| n.id.as_str())
            .collect();
        for t in &c.tests {
            let at = format!("{}: test {}:{}", c.id, t.file, t.line);
            if !is_test_file(&t.file) {
                problems.push(format!("{at}: not a test file"));
                continue;
            }
            let decls = read_head(&t.file).map(|src| facts::declarations_in(&src, &t.file));
            match decls {
                None => problems.push(format!("{at}: file not found at head")),
                Some(Err(err)) => problems.push(format!("{at}: cannot parse: {err}")),
                Some(Ok(decls)) => {
                    // Several tests can share a line; any of them may be the one meant.
                    let here: Vec<&str> = decls
                        .iter()
                        .filter(|d| d.line == t.line)
                        .map(|d| d.name.as_str())
                        .collect();
                    if here.is_empty() {
                        problems.push(format!(
                            "{at}: no test declaration (test(, it(, describe( or #[test]) on that line"
                        ));
                    } else if !here.contains(&t.name.as_str()) {
                        let names: Vec<String> = here.iter().map(|n| format!("{n:?}")).collect();
                        problems.push(format!(
                            "{at}: the test there is named {}, not {:?}",
                            names.join(" or "),
                            t.name
                        ));
                    }
                }
            }
        }
        match c.status {
            Status::Missing if !linked.is_empty() => {
                problems.push(format!(
                    "{}: status missing but {} link it",
                    c.id,
                    linked.join(", ")
                ));
            }
            Status::Implemented if linked.is_empty() => {
                problems.push(format!("{}: status implemented but no node links it", c.id));
            }
            _ => {}
        }
        if let Some(spec_ref) = &c.spec_ref {
            match spec_ref.rsplit_once('#') {
                None => problems.push(format!(
                    "{}: specRef {spec_ref:?} must be file#heading-slug",
                    c.id
                )),
                Some((file, anchor)) => {
                    let path = expand_home(file);
                    match read_spec(&path) {
                        None => problems.push(format!(
                            "{}: specRef file not found: {}",
                            c.id,
                            path.display()
                        )),
                        Some(md) if !heading_slugs(&md).contains(anchor) => {
                            let mut known: Vec<String> = heading_slugs(&md).into_iter().collect();
                            known.sort();
                            problems.push(format!(
                                "{}: specRef heading #{anchor} not in {} (headings: {})",
                                c.id,
                                path.display(),
                                known.join(", ")
                            ));
                        }
                        Some(_) => {}
                    }
                }
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Criterion, Deletion, Lane, Node, Rename, Risk, Site, TestRef};

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    const SPEC: &str = "# Spec\n\n## 01 The offer\n\ntext\n";
    const TEST_FILE: &str = "describe('fee', () => {\n  test('charges the fee', () => {})\n})\ntest('first', () => {}); test('second', () => {})\n";

    fn unit(file: &str, start: u32, end: u32, ident: &str, comment_only: bool) -> Unit {
        Unit {
            file: file.into(),
            start,
            end,
            symbol: ident.into(),
            ident: Some(ident.into()),
            span: Some((start, end)),
            exported: true,
            lane: Lane::Services,
            risk: Risk::Read,
            risk_why: String::new(),
            comment_only,
            added: (start..=end).collect(),
            diff: Vec::new(),
        }
    }

    fn node(
        id: &str,
        criteria: &[&str],
        touched_by_test: bool,
        sites: &[(&str, u32, u32)],
    ) -> Node {
        Node {
            id: id.into(),
            symbol: id.into(),
            lane: Lane::Services,
            risk: Risk::Read,
            caption: "does a thing".into(),
            criteria: criteria.iter().map(|&c| c.into()).collect(),
            touched_by_test,
            sites: sites
                .iter()
                .map(|&(file, start, end)| Site {
                    file: file.into(),
                    start,
                    end,
                })
                .collect(),
            deletions: Vec::new(),
            renames: Vec::new(),
        }
    }

    /// Two hunks in `a.ts` (lines 10-12, `fee` is tested) and one comment-only hunk in `b.ts`.
    fn fixture() -> (ChangeMap, Changes, HashSet<String>) {
        let changes = Changes {
            units: vec![
                unit("a.ts", 10, 12, "fee", false),
                unit("a.ts", 20, 20, "fee", false),
                unit("b.ts", 5, 6, "note", true),
            ],
            ..Changes::default()
        };
        let map = ChangeMap {
            version: 1,
            base: SHA.into(),
            head: SHA.into(),
            spec: Some("/spec.md".into()),
            criteria: vec![Criterion {
                id: "REF-1".into(),
                text: "charges the fee".into(),
                spec_ref: Some("/spec.md#01-the-offer".into()),
                status: Status::Implemented,
                tests: vec![TestRef {
                    file: "a.test.ts".into(),
                    line: 2,
                    name: "charges the fee".into(),
                }],
            }],
            nodes: vec![
                node(
                    "N1",
                    &["REF-1"],
                    true,
                    &[("a.ts", 10, 12), ("a.ts", 20, 20)],
                ),
                node("N2", &[], false, &[("b.ts", 5, 6)]),
            ],
            edges: Vec::new(),
            tour: vec!["N1".into(), "N2".into()],
        };
        (map, changes, HashSet::from(["fee".to_owned()]))
    }

    fn problems(map: &ChangeMap, changes: &Changes, tested: &HashSet<String>) -> Vec<String> {
        let read_spec = |p: &Path| (p == Path::new("/spec.md")).then(|| SPEC.to_owned());
        let read_head = |f: &str| (f == "a.test.ts").then(|| TEST_FILE.to_owned());
        let is_test_file = |f: &str| f.ends_with(".test.ts");
        check(map, changes, tested, &read_spec, &read_head, &is_test_file)
    }

    #[test]
    fn valid_map_has_no_problems() {
        let (map, changes, tested) = fixture();
        assert_eq!(problems(&map, &changes, &tested), Vec::<String>::new());
    }

    #[test]
    fn site_outside_any_hunk_fails() {
        let (mut map, changes, tested) = fixture();
        map.nodes[0].sites.push(Site {
            file: "a.ts".into(),
            start: 14,
            end: 15,
        });
        let got = problems(&map, &changes, &tested);
        assert_eq!(
            got,
            vec!["N1 site a.ts:14-15: start or end is not a changed line of any hunk"]
        );
    }

    #[test]
    fn site_in_unchanged_file_fails() {
        let (mut map, changes, tested) = fixture();
        map.nodes[1].sites.push(Site {
            file: "c.ts".into(),
            start: 1,
            end: 1,
        });
        assert!(problems(&map, &changes, &tested)[0].contains("file has no changed non-test hunk"));
    }

    #[test]
    fn uncovered_hunk_fails() {
        let (mut map, changes, tested) = fixture();
        map.nodes[0].sites.pop();
        assert_eq!(
            problems(&map, &changes, &tested),
            vec!["not covered: a.ts:20-20 (fee) belongs to no node"]
        );
    }

    #[test]
    fn hunk_in_two_nodes_fails() {
        let (mut map, changes, tested) = fixture();
        map.nodes[1].sites.push(Site {
            file: "a.ts".into(),
            start: 20,
            end: 20,
        });
        assert!(
            problems(&map, &changes, &tested)
                .iter()
                .any(|p| p == "covered twice: a.ts:20-20 (fee) is in N1, N2")
        );
    }

    #[test]
    fn tour_must_list_every_known_node_once() {
        let (mut map, changes, tested) = fixture();
        map.tour = vec!["N1".into(), "N1".into(), "N9".into()];
        let got = problems(&map, &changes, &tested);
        assert_eq!(
            got,
            vec![
                "tour: unknown node N9",
                "tour: N1 appears twice",
                "tour: N2 is missing"
            ]
        );
    }

    #[test]
    fn unknown_criterion_and_edge_ends_fail() {
        let (mut map, changes, tested) = fixture();
        map.nodes[0].criteria.push("REF-7".into());
        map.edges.push(crate::map::Edge {
            from: "N1".into(),
            to: "N5".into(),
            kind: crate::map::EdgeKind::Calls,
        });
        let got = problems(&map, &changes, &tested);
        assert_eq!(
            got,
            vec![
                "N1: unknown criterion REF-7",
                "edge N1->N5: unknown node N5"
            ]
        );
    }

    #[test]
    fn bad_spec_ref_heading_fails() {
        let (mut map, changes, tested) = fixture();
        map.criteria[0].spec_ref = Some("/spec.md#09-nowhere".into());
        let got = problems(&map, &changes, &tested);
        assert_eq!(
            got,
            vec![
                "REF-1: specRef heading #09-nowhere not in /spec.md (headings: 01-the-offer, spec)"
            ]
        );
    }

    #[test]
    fn missing_spec_file_fails() {
        let (mut map, changes, tested) = fixture();
        map.criteria[0].spec_ref = Some("/nope.md#01-the-offer".into());
        assert_eq!(
            problems(&map, &changes, &tested),
            vec!["REF-1: specRef file not found: /nope.md"]
        );
    }

    #[test]
    fn touched_by_test_must_match_the_facts() {
        let (mut map, changes, tested) = fixture();
        map.nodes[0].touched_by_test = false;
        assert_eq!(
            problems(&map, &changes, &tested),
            vec!["N1: touchedByTest must be true (from changed test files)"]
        );
    }

    #[test]
    fn criterion_tests_must_be_real_test_declarations() {
        let (mut map, changes, tested) = fixture();
        let bad = [
            ("a.test.ts", 1, "fee", ""),
            ("a.test.ts", 3, "charges the fee", "no test declaration"),
            (
                "a.test.ts",
                2,
                "other name",
                "is named \"charges the fee\", not \"other name\"",
            ),
            (
                "gone.test.ts",
                2,
                "charges the fee",
                "file not found at head",
            ),
            ("a.test.ts", 99, "charges the fee", "no test declaration"),
            ("a.ts", 10, "fee", "not a test file"),
        ];
        for (file, line, name, want) in bad {
            map.criteria[0].tests = vec![TestRef {
                file: file.into(),
                line,
                name: name.into(),
            }];
            let got = problems(&map, &changes, &tested);
            if want.is_empty() {
                assert_eq!(
                    got,
                    Vec::<String>::new(),
                    "describe('fee') at line 1 is a declaration"
                );
            } else {
                assert!(
                    got.len() == 1 && got[0].contains(want),
                    "{file}:{line} {name}: {got:?}"
                );
            }
        }
    }

    #[test]
    fn comment_only_node_cannot_link_a_criterion() {
        let (mut map, changes, tested) = fixture();
        map.nodes[1].criteria.push("REF-1".into());
        assert_eq!(
            problems(&map, &changes, &tested),
            vec!["N2: only touches comments, so it must not link a criterion"]
        );
    }

    #[test]
    fn status_must_agree_with_links() {
        let (mut map, changes, tested) = fixture();
        map.criteria[0].status = Status::Missing;
        assert_eq!(
            problems(&map, &changes, &tested),
            vec!["REF-1: status missing but N1 link it"]
        );
        map.criteria[0].status = Status::Implemented;
        map.nodes[0].criteria.clear();
        assert_eq!(
            problems(&map, &changes, &tested),
            vec!["REF-1: status implemented but no node links it"]
        );
    }

    #[test]
    fn ids_and_shas_are_checked() {
        let (mut map, changes, tested) = fixture();
        map.base = "abc".into();
        map.criteria[0].id = "R1".into();
        map.nodes[0].criteria = vec!["R1".into()];
        let got = problems(&map, &changes, &tested);
        assert!(
            got.iter().any(|p| p.starts_with("base must be a full")),
            "{got:?}"
        );
        assert!(
            got.iter().any(|p| p.starts_with("criterion id \"R1\"")),
            "{got:?}"
        );
    }

    #[test]
    fn coverage_is_per_changed_line() {
        let (mut map, changes, tested) = fixture();
        map.nodes[0].sites[0].end = 10;
        assert_eq!(
            problems(&map, &changes, &tested),
            vec!["not covered: a.ts:11-12 (fee) belongs to no node"],
            "a site touching one line of a hunk does not cover the rest"
        );
    }

    #[test]
    fn a_line_shared_by_two_tests_can_reference_either() {
        let (mut map, changes, tested) = fixture();
        for (name, ok) in [("second", true), ("first", true), ("third", false)] {
            map.criteria[0].tests = vec![TestRef {
                file: "a.test.ts".into(),
                line: 4,
                name: name.into(),
            }];
            let got = problems(&map, &changes, &tested);
            assert_eq!(got.is_empty(), ok, "{name}: {got:?}");
        }
    }

    #[test]
    fn deleted_files_need_exactly_one_node() {
        let (mut map, mut changes, tested) = fixture();
        changes.deleted = vec!["old.ts".into()];
        assert_eq!(
            problems(&map, &changes, &tested),
            vec!["not covered: deleted file old.ts belongs to no node"]
        );
        let mut gone = node("N3", &[], false, &[]);
        gone.deletions = vec![Deletion {
            file: "old.ts".into(),
        }];
        map.nodes.push(gone.clone());
        map.tour.push("N3".into());
        assert_eq!(problems(&map, &changes, &tested), Vec::<String>::new());
        map.nodes[1].deletions = vec![Deletion {
            file: "old.ts".into(),
        }];
        assert_eq!(
            problems(&map, &changes, &tested),
            vec!["covered twice: deleted file old.ts is in N2, N3"]
        );
        map.nodes[1].deletions = vec![Deletion {
            file: "kept.ts".into(),
        }];
        assert_eq!(
            problems(&map, &changes, &tested),
            vec!["N2: deletion kept.ts is not a deleted non-test file in base...head"]
        );
    }

    #[test]
    fn moves_need_exactly_one_node() {
        let (mut map, mut changes, tested) = fixture();
        let mv = Rename {
            from: "a.ts".into(),
            to: "b.ts".into(),
        };
        changes.renames = vec![mv.clone()];
        assert_eq!(
            problems(&map, &changes, &tested),
            vec!["not covered: move a.ts -> b.ts belongs to no node"]
        );
        map.nodes[1].renames = vec![mv.clone()];
        assert_eq!(problems(&map, &changes, &tested), Vec::<String>::new());
        map.nodes[0].renames = vec![mv];
        assert_eq!(
            problems(&map, &changes, &tested),
            vec!["covered twice: move a.ts -> b.ts is in N1, N2"]
        );
    }

    #[test]
    fn option_like_shas_never_reach_git() {
        let repo = crate::testutil::TempRepo::new("evil");
        repo.write("a.ts", "const a = 1\n");
        let head = repo.commit("base");
        let target = repo.path().join("pwned");
        for (base, head) in [
            (format!("--output={}", target.display()), head.clone()),
            (head.clone(), format!("--output={}", target.display())),
        ] {
            let map = format!(
                r#"{{"version":1,"base":"{base}","head":"{head}","criteria":[],"nodes":[],"edges":[],"tour":[]}}"#
            );
            let path = repo.path().join("evil.json");
            std::fs::write(&path, map).expect("write map");
            let rules = Rules::load(None).expect("rules");
            let valid = run(&Git::new(repo.path()), &rules, &path)
                .expect("rejected as a problem, not a git error");
            assert!(!valid, "an option-like sha must be rejected");
            assert!(!target.exists(), "git must not write {}", target.display());
        }
    }

    #[test]
    fn slugs_match_github_anchors() {
        assert_eq!(
            slug("03 Trade log and daily settlement"),
            "03-trade-log-and-daily-settlement"
        );
        assert_eq!(
            slug("Why the rate is flat, and not 25 / 30 / 35"),
            "why-the-rate-is-flat-and-not-25--30--35"
        );
        assert_eq!(slug("The referrer gets: 25%"), "the-referrer-gets-25");
    }
}
