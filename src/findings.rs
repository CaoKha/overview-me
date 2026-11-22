//! `overview-me findings`: code-review findings checked against the map's head and placed on the
//! map's nodes, without an LLM.
//!
//! Reads OpenCodeReview JSON (`ocr review --format json`, or the same `comments` written by a
//! host agent in delegate mode) or SARIF 2.1.0 (OCR, Semgrep, CodeQL, clippy-sarif and others).
//! A finding whose line is inside a node's sites goes on that node; one outside every site is
//! kept as "outside this change"; one whose file or lines are not there at head, or whose quoted
//! code is not at its lines, is rejected. A file reviewed at another commit is refused whole.
//!
//! Findings are a layer over the map, not part of it: they live in `<store>/<head>.findings.json`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::git::Git;
use crate::map::ChangeMap;

/// Lines around a finding's range searched for its `existing_code`.
const SLACK: usize = 3;
/// Characters of a rejected finding's text kept to name it.
const REJECTED_TEXT: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

impl Severity {
    /// The word a page shows.
    pub fn label(self) -> &'static str {
        match self {
            Self::Critical => "Critical",
            Self::High => "High",
            Self::Medium => "Medium",
            Self::Low => "Low",
        }
    }

    /// OCR's words; anything else is low, as OCR does.
    fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "critical" | "blocker" => Self::Critical,
            "high" | "major" => Self::High,
            "medium" | "moderate" => Self::Medium,
            _ => Self::Low,
        }
    }

    /// A SARIF level: error is high, warning medium, note and none low.
    fn from_level(level: &str) -> Self {
        match level {
            "error" => Self::High,
            "warning" => Self::Medium,
            _ => Self::Low,
        }
    }
}

/// `<store>/<head>.findings.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FindingsFile {
    /// Always 1.
    pub version: u32,
    pub base: String,
    /// The commit every finding was checked against: the map's head.
    pub head: String,
    /// The tool that wrote the findings, such as `OpenCodeReview`.
    pub source: String,
    /// Most severe first; on a stop before outside the change.
    pub findings: Vec<Finding>,
    /// Findings that point at no line at head, with why.
    pub rejected: Vec<Rejected>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    /// `F1`, `F2`, … in the order of `findings`.
    pub id: String,
    /// The node whose sites hold the finding; none when it is outside this change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    pub file: String,
    /// The lines at head; none for a finding about the whole file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<u32>,
    pub severity: Severity,
    /// bug, security, performance, maintainability, test, style, documentation or other.
    pub category: String,
    /// The tool's rule id, for SARIF.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

impl Finding {
    /// `file:start-end`, `file:line`, or `file`.
    pub fn place(&self) -> String {
        match (self.start, self.end) {
            (Some(s), Some(e)) if e > s => format!("{}:{s}-{e}", self.file),
            (Some(s), _) => format!("{}:{s}", self.file),
            _ => self.file.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rejected {
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<u32>,
    pub severity: Severity,
    pub content: String,
    pub reason: String,
}

/// One finding as the tool wrote it.
#[derive(Debug, Clone, Default)]
struct Raw {
    file: String,
    start: Option<u32>,
    end: Option<u32>,
    severity: Option<Severity>,
    category: String,
    rule: Option<String>,
    content: String,
    suggestion: Option<String>,
    existing: Option<String>,
}

/// What a findings file holds.
struct Report {
    source: String,
    /// Commits the file says it reviewed.
    commits: Vec<String>,
    findings: Vec<Raw>,
}

const CATEGORIES: [&str; 8] = [
    "bug",
    "security",
    "performance",
    "maintainability",
    "test",
    "style",
    "documentation",
    "other",
];

fn category(s: &str) -> String {
    let s = s.trim().to_lowercase();
    if CATEGORIES.contains(&s.as_str()) {
        s
    } else {
        "other".to_owned()
    }
}

fn line(v: &Value) -> Option<u32> {
    v.as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|&n| n > 0)
}

fn text(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Reads OCR JSON (an object with `comments`, or a bare list of comments) or SARIF 2.1.0.
fn parse(v: &Value) -> Result<Report> {
    if let Some(runs) = v.get("runs").and_then(Value::as_array) {
        return Ok(parse_sarif(runs));
    }
    let (comments, top) = match v {
        Value::Array(list) => (list.as_slice(), None),
        Value::Object(_) => {
            if let Some(status) = v.get("status").and_then(Value::as_str)
                && status != "success"
            {
                let message = v["message"].as_str().unwrap_or_default();
                bail!("the review did not finish (status \"{status}\"): {message}");
            }
            let Some(list) = v.get("comments").and_then(Value::as_array) else {
                bail!("not OpenCodeReview JSON (no \"comments\") nor SARIF (no \"runs\")");
            };
            (list.as_slice(), Some(v))
        }
        _ => bail!("not OpenCodeReview JSON nor SARIF"),
    };
    let mut commits = Vec::new();
    if let Some(top) = top {
        let input = &top["manifest"]["input"];
        if input["mode"].as_str() == Some("workspace") {
            bail!(
                "this is a workspace review: it read files that are in no commit. Review \
                 `--from <base> --to <head>` instead (for uncommitted work, <head> is the \
                 commit `overview-me snapshot` prints)"
            );
        }
        commits.extend(text(&input["resolved_head"]));
        commits.extend(text(&top["head"]));
    }
    let source = top
        .and_then(|t| text(&t["source"]))
        .unwrap_or_else(|| "OpenCodeReview".to_owned());
    let findings = comments
        .iter()
        .map(|c| Raw {
            file: c["path"].as_str().unwrap_or_default().to_owned(),
            start: line(&c["start_line"]),
            end: line(&c["end_line"]),
            severity: c["severity"].as_str().map(Severity::parse),
            category: category(c["category"].as_str().unwrap_or_default()),
            rule: None,
            content: c["content"].as_str().unwrap_or_default().trim().to_owned(),
            suggestion: text(&c["suggestion_code"]),
            existing: text(&c["existing_code"]),
        })
        .collect();
    Ok(Report {
        source,
        commits,
        findings,
    })
}

fn parse_sarif(runs: &[Value]) -> Report {
    let mut tools: Vec<String> = Vec::new();
    let mut commits = Vec::new();
    let mut findings = Vec::new();
    for run in runs {
        if let Some(name) = text(&run["tool"]["driver"]["name"])
            && !tools.contains(&name)
        {
            tools.push(name);
        }
        for p in run["versionControlProvenance"]
            .as_array()
            .into_iter()
            .flatten()
        {
            commits.extend(text(&p["revisionId"]));
        }
        for r in run["results"].as_array().into_iter().flatten() {
            let loc = &r["locations"][0]["physicalLocation"];
            let region = &loc["region"];
            let props = &r["properties"];
            let rule = text(&r["ruleId"]);
            let severity = props["severity"]
                .as_str()
                .map(Severity::parse)
                .or_else(|| r["level"].as_str().map(Severity::from_level));
            let category = props["category"]
                .as_str()
                .or(rule.as_deref())
                .map(category)
                .unwrap_or_else(|| "other".to_owned());
            let suggestion = text(
                &r["fixes"][0]["artifactChanges"][0]["replacements"][0]["insertedContent"]["text"],
            );
            let content = text(&r["message"]["text"])
                .or_else(|| text(&r["message"]["markdown"]))
                .unwrap_or_default();
            findings.push(Raw {
                file: loc["artifactLocation"]["uri"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                start: line(&region["startLine"]),
                end: line(&region["endLine"]),
                // SARIF's default level is warning.
                severity: severity.or(Some(Severity::Medium)),
                category,
                rule,
                content,
                suggestion,
                existing: text(&region["snippet"]["text"]),
            });
        }
    }
    let source = if tools.is_empty() {
        "SARIF".to_owned()
    } else {
        tools.join(", ")
    };
    Report {
        source,
        commits,
        findings,
    }
}

/// `%xx` escapes in a URI path.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(b) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The repo-relative path of a finding's file, or why it has none.
fn repo_path(raw: &str, root: &Path) -> Result<String, String> {
    let mut p = raw.trim().to_owned();
    if let Some(rest) = p.strip_prefix("file://") {
        p = percent_decode(rest.strip_prefix("localhost").unwrap_or(rest));
    } else if !p.contains("://") {
        p = percent_decode(&p);
    }
    let root = root.to_string_lossy();
    let root = root.trim_end_matches('/');
    if let Some(rest) = p.strip_prefix(root).and_then(|r| r.strip_prefix('/')) {
        p = rest.to_owned();
    }
    while let Some(rest) = p.strip_prefix("./") {
        p = rest.to_owned();
    }
    if p.is_empty() {
        return Err("no file".into());
    }
    if p.starts_with('/') || p.contains("://") || p.split('/').any(|part| part == "..") {
        return Err(format!("{raw} is not a path inside the repo"));
    }
    Ok(p)
}

/// Lines trimmed, blank ones dropped: the code with its layout ignored.
fn normalized<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    lines.map(str::trim).filter(|l| !l.is_empty()).collect()
}

/// Checks a finding against its file at head: the lines exist, and its quoted code is at them.
fn check_lines(f: &Raw, text: &str, short: &str) -> Result<(), String> {
    let lines: Vec<&str> = text.lines().collect();
    let Some(start) = f.start else {
        return Ok(());
    };
    let end = f.end.unwrap_or(start);
    if end < start {
        return Err(format!("end line {end} is before start line {start}"));
    }
    if end as usize > lines.len() {
        return Err(format!("the file has {} lines at {short}", lines.len()));
    }
    let Some(existing) = &f.existing else {
        return Ok(());
    };
    let want = normalized(existing.lines());
    if want.is_empty() {
        return Ok(());
    }
    let from = (start as usize - 1).saturating_sub(SLACK);
    let to = (end as usize + SLACK).min(lines.len());
    let window = normalized(lines[from..to].iter().copied());
    let found = window.windows(want.len()).any(|w| w == want.as_slice())
        // One quoted line may be part of a longer line.
        || (want.len() == 1 && window.iter().any(|l| l.contains(want[0])));
    if found {
        Ok(())
    } else {
        Err(format!(
            "the code it quotes is not at lines {start}-{end} at {short}"
        ))
    }
}

/// The node whose sites hold the finding: the site holding its first line, else the first site
/// (in tour order) its lines overlap. A finding without lines goes on the one node with sites in
/// its file.
pub fn node_for(
    map: &ChangeMap,
    file: &str,
    start: Option<u32>,
    end: Option<u32>,
) -> Option<String> {
    let order = map
        .tour
        .iter()
        .filter_map(|id| map.nodes.iter().find(|n| &n.id == id))
        .chain(map.nodes.iter().filter(|n| !map.tour.contains(&n.id)));
    let nodes: Vec<_> = order.collect();
    let Some(start) = start else {
        let mut with_file = nodes
            .iter()
            .filter(|n| n.sites.iter().any(|s| s.file == file));
        let first = with_file.next()?;
        return with_file.next().is_none().then(|| first.id.clone());
    };
    let end = end.unwrap_or(start).max(start);
    let holds = |line: u32| {
        nodes.iter().find(|n| {
            n.sites
                .iter()
                .any(|s| s.file == file && s.start <= line && line <= s.end)
        })
    };
    holds(start)
        .or_else(|| {
            nodes.iter().find(|n| {
                n.sites
                    .iter()
                    .any(|s| s.file == file && s.start <= end && start <= s.end)
            })
        })
        .map(|n| n.id.clone())
}

fn clip(s: &str, max: usize) -> String {
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match one_line.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &one_line[..i]),
        None => one_line,
    }
}

/// Checks and places every finding of `report` for `map`. `read_head(path)` gives a file's text
/// at head, or `None` when it is not there.
fn place(
    map: &ChangeMap,
    report: Report,
    root: &Path,
    read_head: &dyn Fn(&str) -> Option<String>,
) -> FindingsFile {
    let short = &map.head[..10.min(map.head.len())];
    let mut kept: Vec<Finding> = Vec::new();
    let mut rejected = Vec::new();
    for mut raw in report.findings {
        let severity = raw.severity.unwrap_or(Severity::Low);
        let file = match repo_path(&raw.file, root) {
            Ok(p) => p,
            Err(reason) => {
                rejected.push(Rejected {
                    file: raw.file.clone(),
                    start: raw.start,
                    end: raw.end,
                    severity,
                    content: clip(&raw.content, REJECTED_TEXT),
                    reason,
                });
                continue;
            }
        };
        raw.file = file;
        if raw.start.is_none() {
            raw.end = None;
        } else if raw.end.is_none() {
            raw.end = raw.start;
        }
        let checked = read_head(&raw.file)
            .ok_or_else(|| format!("no such file at {short}"))
            .and_then(|text| check_lines(&raw, &text, short));
        if let Err(reason) = checked {
            rejected.push(Rejected {
                file: raw.file,
                start: raw.start,
                end: raw.end,
                severity,
                content: clip(&raw.content, REJECTED_TEXT),
                reason,
            });
            continue;
        }
        let finding = Finding {
            id: String::new(),
            node: node_for(map, &raw.file, raw.start, raw.end),
            file: raw.file,
            start: raw.start,
            end: raw.end,
            severity,
            category: raw.category,
            rule: raw.rule,
            content: raw.content,
            suggestion: raw.suggestion,
        };
        let same = |k: &Finding| {
            k.file == finding.file
                && k.start == finding.start
                && k.end == finding.end
                && k.content == finding.content
        };
        if !kept.iter().any(same) {
            kept.push(finding);
        }
    }
    let tour_index = |node: &Option<String>| {
        node.as_ref()
            .and_then(|id| map.tour.iter().position(|t| t == id))
            .unwrap_or(usize::MAX)
    };
    kept.sort_by(|a, b| {
        (
            a.node.is_none(),
            a.severity,
            tour_index(&a.node),
            &a.file,
            a.start,
        )
            .cmp(&(
                b.node.is_none(),
                b.severity,
                tour_index(&b.node),
                &b.file,
                b.start,
            ))
    });
    for (i, f) in kept.iter_mut().enumerate() {
        f.id = format!("F{}", i + 1);
    }
    FindingsFile {
        version: 1,
        base: map.base.clone(),
        head: map.head.clone(),
        source: report.source,
        findings: kept,
        rejected,
    }
}

/// The findings file for a map: `<head>.findings.json` beside it.
pub fn path_for(map_path: &Path, head: &str) -> PathBuf {
    map_path.with_file_name(format!("{head}.findings.json"))
}

/// Reads the findings file for `head` beside `map_path`.
///
/// # Errors
/// When it is missing, does not parse, or is for another head.
pub fn load(map_path: &Path, head: &str) -> Result<(PathBuf, FindingsFile)> {
    let path = path_for(map_path, head);
    let text = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "reading {}: run `overview-me findings <map> <review file>` first",
            path.display()
        )
    })?;
    let file: FindingsFile =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    if file.head != head {
        bail!("{} is for head {}, not {head}", path.display(), file.head);
    }
    Ok((path, file))
}

/// Reads the review file, checks every finding at the map's head, places it, writes
/// `<store>/<head>.findings.json` (or `out`) and prints a summary.
///
/// # Errors
/// When the map or the review file cannot be read, the review is of another commit or names no
/// commit (and `head` is not given), or git fails.
pub fn run(
    git: &Git,
    map_path: &Path,
    review: &Path,
    head: Option<&str>,
    out: Option<PathBuf>,
) -> Result<PathBuf> {
    let map: ChangeMap = serde_json::from_str(
        &std::fs::read_to_string(map_path)
            .with_context(|| format!("reading {}", map_path.display()))?,
    )
    .with_context(|| format!("parsing {}", map_path.display()))?;
    let value: Value = serde_json::from_str(
        &std::fs::read_to_string(review)
            .with_context(|| format!("reading {}", review.display()))?,
    )
    .with_context(|| format!("parsing {}", review.display()))?;
    let mut report = parse(&value).with_context(|| format!("reading {}", review.display()))?;
    report.commits.extend(head.map(str::to_owned));
    if report.commits.is_empty() {
        bail!(
            "{} does not say which commit it reviewed: pass --head <sha>, the commit the review read",
            review.display()
        );
    }
    for c in &report.commits {
        let sha = git
            .rev(c)
            .with_context(|| format!("{} names commit {c}", review.display()))?;
        if sha != map.head {
            bail!(
                "{} is a review of {}, but the map is for {}: review the map's head",
                review.display(),
                &sha[..10],
                &map.head[..10.min(map.head.len())]
            );
        }
    }
    let root = git.run(&["rev-parse", "--show-toplevel"])?;
    let file = place(&map, report, Path::new(root.trim()), &|p| {
        git.show(&map.head, p)
    });
    let out = out.unwrap_or_else(|| path_for(map_path, &map.head));
    std::fs::write(&out, serde_json::to_string_pretty(&file)? + "\n")
        .with_context(|| format!("writing {}", out.display()))?;
    print_summary(&out, &file);
    Ok(out)
}

fn print_summary(out: &Path, file: &FindingsFile) {
    println!("findings: {}", out.display());
    println!("source:   {}", file.source);
    let on: Vec<&Finding> = file.findings.iter().filter(|f| f.node.is_some()).collect();
    let mut per_node: Vec<(&str, usize)> = Vec::new();
    for f in &on {
        let id = f.node.as_deref().unwrap_or_default();
        match per_node.iter_mut().find(|(n, _)| *n == id) {
            Some((_, c)) => *c += 1,
            None => per_node.push((id, 1)),
        }
    }
    let list: Vec<String> = per_node.iter().map(|(n, c)| format!("{n} {c}")).collect();
    println!(
        "placed:   {} on {} stops{}",
        on.len(),
        per_node.len(),
        if list.is_empty() {
            String::new()
        } else {
            format!(" ({})", list.join(", "))
        }
    );
    println!("outside this change: {}", file.findings.len() - on.len());
    println!("rejected: {}", file.rejected.len());
    for r in &file.rejected {
        let at = match (r.start, r.end) {
            (Some(s), Some(e)) if e > s => format!("{}:{s}-{e}", r.file),
            (Some(s), _) => format!("{}:{s}", r.file),
            _ => r.file.clone(),
        };
        println!("  {at}: {}", r.reason);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Node, Risk, Site};
    use serde_json::json;

    const BASE: &str = "0123456789abcdef0123456789abcdef01234567";
    const HEAD: &str = "89abcdef0123456789abcdef0123456789abcdef";

    fn node(id: &str, sites: &[(&str, u32, u32)]) -> Node {
        Node {
            id: id.into(),
            symbol: id.into(),
            lane: "core".into(),
            risk: Risk::Read,
            caption: "c".into(),
            criteria: vec![],
            touched_by_test: false,
            sites: sites
                .iter()
                .map(|(f, s, e)| Site {
                    file: (*f).into(),
                    start: *s,
                    end: *e,
                })
                .collect(),
            deletions: vec![],
            renames: vec![],
        }
    }

    fn map() -> ChangeMap {
        ChangeMap {
            version: 1,
            grouped: false,
            grouping: None,
            path: None,
            base: BASE.into(),
            head: HEAD.into(),
            spec: None,
            criteria: vec![],
            nodes: vec![
                node("N1", &[("src/a.rs", 3, 5)]),
                node("N2", &[("src/a.rs", 10, 12), ("src/b.rs", 1, 2)]),
            ],
            edges: vec![],
            tour: vec!["N2".into(), "N1".into()],
            callouts: vec![],
        }
    }

    fn head_file(path: &str) -> Option<String> {
        match path {
            "src/a.rs" => Some((1..=20).map(|i| format!("    line {i}\n")).collect()),
            "src/b.rs" => Some("fn b() {}\nfn c() {}\n".into()),
            "src/c.rs" => Some("x\n".into()),
            _ => None,
        }
    }

    fn placed(review: &Value) -> FindingsFile {
        place(
            &map(),
            parse(review).expect("parses"),
            Path::new("/repo"),
            &head_file,
        )
    }

    #[test]
    fn ocr_comments_are_placed_on_nodes_outside_or_rejected() {
        let review = json!({
            "status": "success",
            "comments": [
                {"path": "src/a.rs", "start_line": 4, "end_line": 4, "content": "low in N1", "severity": "low", "category": "style"},
                {"path": "src/a.rs", "start_line": 11, "end_line": 13, "content": "high in N2", "severity": "high", "category": "bug", "existing_code": "line 11\n  line 12"},
                {"path": "src/a.rs", "start_line": 8, "end_line": 10, "content": "overlaps N2", "severity": "medium", "category": "nonsense"},
                {"path": "src/c.rs", "start_line": 1, "end_line": 1, "content": "outside", "severity": "critical", "category": "security"},
                {"path": "src/a.rs", "start_line": 30, "end_line": 31, "content": "past the end", "severity": "high"},
                {"path": "src/gone.rs", "start_line": 1, "content": "no file"},
                {"path": "src/a.rs", "start_line": 4, "end_line": 4, "content": "wrong code", "existing_code": "line 17"},
                {"path": "/elsewhere/x.rs", "start_line": 1, "content": "absolute"},
                {"path": "src/a.rs", "start_line": 4, "end_line": 4, "content": "low in N1", "severity": "low", "category": "style"}
            ]
        });
        let f = placed(&review);
        let got: Vec<(&str, Option<&str>, &str, &str)> = f
            .findings
            .iter()
            .map(|f| {
                (
                    f.id.as_str(),
                    f.node.as_deref(),
                    f.content.as_str(),
                    f.category.as_str(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                ("F1", Some("N2"), "high in N2", "bug"),
                ("F2", Some("N2"), "overlaps N2", "other"),
                ("F3", Some("N1"), "low in N1", "style"),
                ("F4", None, "outside", "security"),
            ]
        );
        let reasons: Vec<&str> = f.rejected.iter().map(|r| r.reason.as_str()).collect();
        assert_eq!(
            reasons,
            [
                "the file has 20 lines at 89abcdef01",
                "no such file at 89abcdef01",
                "the code it quotes is not at lines 4-4 at 89abcdef01",
                "/elsewhere/x.rs is not a path inside the repo",
            ]
        );
        assert_eq!(f.source, "OpenCodeReview");
    }

    #[test]
    fn a_finding_without_lines_goes_on_the_only_node_in_its_file() {
        let review = json!([
            {"path": "src/b.rs", "content": "whole file", "start_line": 0, "end_line": 0},
            {"path": "src/a.rs", "content": "two nodes have sites here"}
        ]);
        let f = placed(&review);
        let got: Vec<(Option<&str>, Option<u32>)> = f
            .findings
            .iter()
            .map(|f| (f.node.as_deref(), f.start))
            .collect();
        assert_eq!(got, [(Some("N2"), None), (None, None)]);
        assert_eq!(f.findings[0].place(), "src/b.rs");
    }

    #[test]
    fn sarif_results_are_read_with_levels_and_paths() {
        let review = json!({
            "version": "2.1.0",
            "runs": [{
                "tool": {"driver": {"name": "Semgrep"}},
                "versionControlProvenance": [{"revisionId": HEAD}],
                "results": [
                    {"ruleId": "rust.unsafe", "level": "error", "message": {"text": "unsafe here"},
                     "locations": [{"physicalLocation": {"artifactLocation": {"uri": "file:///repo/src/a.rs"}, "region": {"startLine": 3}}}]},
                    {"ruleId": "security", "message": {"text": "default level"},
                     "locations": [{"physicalLocation": {"artifactLocation": {"uri": "./src/b.rs"}, "region": {"startLine": 1, "endLine": 2}}}],
                     "fixes": [{"artifactChanges": [{"replacements": [{"insertedContent": {"text": "fn b() -> () {}"}}]}]}]}
                ]
            }]
        });
        let report = parse(&review).expect("sarif");
        assert_eq!(report.commits, [HEAD]);
        let f = place(&map(), report, Path::new("/repo"), &head_file);
        assert_eq!(f.source, "Semgrep");
        let a = &f.findings[0];
        assert_eq!(
            (
                a.file.as_str(),
                a.node.as_deref(),
                a.severity,
                a.category.as_str(),
                a.rule.as_deref()
            ),
            (
                "src/a.rs",
                Some("N1"),
                Severity::High,
                "other",
                Some("rust.unsafe")
            )
        );
        let b = &f.findings[1];
        assert_eq!(
            (
                b.node.as_deref(),
                b.severity,
                b.category.as_str(),
                b.place().as_str()
            ),
            (Some("N2"), Severity::Medium, "security", "src/b.rs:1-2")
        );
        assert_eq!(b.suggestion.as_deref(), Some("fn b() -> () {}"));
    }

    #[test]
    fn unfinished_and_workspace_reviews_are_refused() {
        let err = parse(&json!({"status": "error", "message": "no key", "comments": []}))
            .err()
            .expect("error status");
        assert!(err.to_string().contains("did not finish"), "{err}");
        let err = parse(&json!({"status": "success", "comments": [], "manifest": {"input": {"mode": "workspace"}}}))
            .err()
            .expect("workspace");
        assert!(err.to_string().contains("workspace review"), "{err}");
        let r = parse(&json!({"comments": [], "manifest": {"input": {"mode": "range", "resolved_head": HEAD}}}))
            .expect("range");
        assert_eq!(r.commits, [HEAD]);
    }

    #[test]
    fn run_refuses_a_review_of_another_commit() {
        use crate::testutil::TempRepo;
        let repo = TempRepo::new("findings");
        repo.write("a.txt", "one\n");
        let base = repo.commit("base");
        repo.write("a.txt", "one\ntwo\n");
        let head = repo.commit("head");
        let git = Git::new(repo.path());
        let mut m = map();
        m.base = base.clone();
        m.head = head.clone();
        m.nodes = vec![node("N1", &[("a.txt", 2, 2)])];
        m.tour = vec!["N1".into()];
        let map_path = repo.path().join("map.json");
        std::fs::write(&map_path, serde_json::to_string(&m).expect("map")).expect("write map");
        let review = repo.path().join("review.json");
        let comment = json!({"path": "a.txt", "start_line": 2, "end_line": 2, "content": "c", "severity": "high", "existing_code": "two"});

        std::fs::write(&review, json!({"comments": [comment]}).to_string()).expect("write");
        let err = run(&git, &map_path, &review, None, None).expect_err("no commit named");
        assert!(err.to_string().contains("--head"), "{err}");
        let err = run(&git, &map_path, &review, Some(&base), None).expect_err("other commit");
        assert!(err.to_string().contains("is a review of"), "{err}");

        let out = run(&git, &map_path, &review, Some(&head[..12]), None).expect("same head");
        let (_, file) = load(&map_path, &head).expect("load");
        assert_eq!(out, path_for(&map_path, &head));
        assert_eq!(file.findings.len(), 1);
        assert_eq!(file.findings[0].node.as_deref(), Some("N1"));
        assert!(load(&map_path, &base).is_err(), "another head has no file");
    }

    #[test]
    fn paths_are_made_repo_relative() {
        let root = Path::new("/repo");
        assert_eq!(repo_path("src/a%20b.rs", root).as_deref(), Ok("src/a b.rs"));
        assert_eq!(repo_path("file:///repo/x.rs", root).as_deref(), Ok("x.rs"));
        assert_eq!(repo_path("././x.rs", root).as_deref(), Ok("x.rs"));
        assert!(repo_path("../x.rs", root).is_err());
        assert!(repo_path("https://h/x.rs", root).is_err());
        assert!(repo_path("", root).is_err());
    }
}
