//! `overview-me render`: the PR or change overview page, written from the map and the flow, with
//! `<!-- fill: … -->` slots where only a reader of the code can write the text.
//!
//! Everything the map or the code already says is written here: the header, entry points, both
//! flow diagrams, the step list, where the change plugs in, claims and proof, what to read
//! first, the callouts and the open questions that facts raise. `overview-me page` refuses a
//! page with a slot left.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::facts::{self, Changes, Unit};
use crate::flow::{self, FlowFile, span_path};
use crate::git::Git;
use crate::map::{CalloutTag, ChangeMap, Node, Risk, Status};
use crate::rules::Rules;

/// Steps under Before: only the parts the change touches.
const MAX_BEFORE_STEPS: usize = 4;
/// Stops read in full under Read order.
const MAX_READ_FIRST: usize = 5;
/// Module pages explain a bounded set of steps; every group remains in the map.
const MAX_MODULE_STEPS: usize = 20;

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
/// the page path. Refuses to overwrite a page unless `fresh`. With `findings`, the page gets a
/// Findings section from `<store>/<head>.findings.json`; on an existing page (without `fresh`),
/// only that section is written and the filled slots are kept.
///
/// # Errors
/// When the map does not validate, the page exists, the findings file is missing, or git fails.
pub fn run(
    git: &Git,
    rules: &Rules,
    map_path: &Path,
    kind: &Kind<'_>,
    out: Option<PathBuf>,
    fresh: bool,
    findings: bool,
) -> Result<PathBuf> {
    if !crate::validate::run(git, rules, map_path)? {
        bail!("the map has problems (listed above): fix them before rendering");
    }
    let (_, map, flow) = flow::run(git, rules, map_path, None)?;
    let out = out.unwrap_or_else(|| map_path.with_file_name(format!("{}.review.md", map.head)));
    let section = if findings {
        let (_, file) = crate::findings::load(map_path, &map.head)?;
        Some(findings_section(&map, &file))
    } else {
        None
    };
    if out.exists() && !fresh {
        let Some(section) = section else {
            bail!(
                "{} exists: fill it in, or pass --fresh to write it again (what was filled in is lost)",
                out.display()
            );
        };
        let md =
            std::fs::read_to_string(&out).with_context(|| format!("reading {}", out.display()))?;
        let md = with_section(&md, FINDINGS, &section);
        std::fs::write(&out, &md).with_context(|| format!("writing {}", out.display()))?;
        println!(
            "page:  {} (Findings section written; the rest kept)",
            out.display()
        );
        println!(
            "slots: {} to fill (search for \"{SLOT}\")",
            md.matches(SLOT).count()
        );
        return Ok(out);
    }
    let changes = facts::changes_in(git, rules, &map.base, &map.head, map.path.as_deref())?;
    let mut md = page(&map, &flow, &changes, kind);
    if let Some(section) = section {
        md = with_section(&md, FINDINGS, &section);
    }
    std::fs::write(&out, &md).with_context(|| format!("writing {}", out.display()))?;
    println!("page:  {}", out.display());
    if map.grouped {
        println!(
            "module steps: at most {MAX_MODULE_STEPS} prose slots; remaining groups stay in the map and flows"
        );
    }
    println!(
        "slots: {} to fill (search for \"{SLOT}\")",
        md.matches(SLOT).count()
    );
    Ok(out)
}

/// The heading of the review findings section.
pub const FINDINGS: &str = "## Findings";
/// Sections the findings section goes before, in order of preference.
const FINDINGS_BEFORE: [&str; 2] = ["## Watch out", "## Open questions"];
/// What separates a finding's place from its text in the section; `page` checks only the place.
pub const FINDING_TEXT_MARK: &str = " — ";

/// `md` with the section under `heading` replaced by `section`, or `section` inserted before the
/// first of `FINDINGS_BEFORE` (else at the end).
fn with_section(md: &str, heading: &str, section: &str) -> String {
    let lines: Vec<&str> = md.lines().collect();
    let is_h2 = |l: &str| l.starts_with("## ");
    let (from, to) = match lines.iter().position(|l| l.trim_end() == heading) {
        Some(i) => {
            let end = lines[i + 1..]
                .iter()
                .position(|l| is_h2(l))
                .map_or(lines.len(), |j| i + 1 + j);
            (i, end)
        }
        None => {
            let at = FINDINGS_BEFORE
                .iter()
                .find_map(|h| lines.iter().position(|l| l.trim_end() == *h))
                .unwrap_or(lines.len());
            (at, at)
        }
    };
    let mut out: Vec<&str> = lines[..from].to_vec();
    if out.last().is_some_and(|l| !l.trim().is_empty()) {
        out.push("");
    }
    let section = section.trim_end();
    out.extend(section.lines());
    out.push("");
    out.extend(&lines[to..]);
    let mut text = out.join("\n");
    text.push('\n');
    text
}

/// One line of markdown from a finding's whole text, its paragraphs joined, with HTML outside
/// code spans escaped so tool output cannot add markup or slots to the page.
fn finding_text(s: &str) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.is_empty() {
        return "(no text)".to_owned();
    }
    escape_outside_code(&flat)
}

/// `s` with `&`, `<` and `>` escaped except inside code spans, found as CommonMark finds them:
/// a run of backticks not after a backslash, closed by the next run of the same length. A run
/// that nothing closes is plain text, so an odd backtick cannot leave the rest of `s` unescaped.
fn escape_outside_code(s: &str) -> String {
    let b = s.as_bytes();
    let run_at = |i: usize| b[i..].iter().take_while(|&&c| c == b'`').count();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            // `\<` meant a literal `<`: the entity written next says the same.
            b'\\' if matches!(b.get(i + 1), Some(b'&' | b'<' | b'>')) => i += 1,
            b'\\' if b.get(i + 1).is_some_and(u8::is_ascii_punctuation) => {
                out.push_str(&s[i..i + 2]);
                i += 2;
            }
            b'`' => {
                let n = run_at(i);
                let mut from = i + n;
                let close = loop {
                    let Some(k) = b[from..].iter().position(|&c| c == b'`') else {
                        break None;
                    };
                    let m = run_at(from + k);
                    if m == n {
                        break Some(from + k + n);
                    }
                    from += k + m;
                };
                let end = close.unwrap_or(i + n);
                out.push_str(&s[i..end]);
                i = end;
            }
            b'&' | b'<' | b'>' => {
                out.push_str(match b[i] {
                    b'&' => "&amp;",
                    b'<' => "&lt;",
                    _ => "&gt;",
                });
                i += 1;
            }
            _ => {
                let c = s[i..].chars().next().unwrap_or_default();
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
    out
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// The Findings section: a summary line, the findings on stops, those outside the change, and
/// how many were rejected.
pub fn findings_section(map: &ChangeMap, file: &crate::findings::FindingsFile) -> String {
    use crate::findings::Severity;
    let mut md = String::new();
    let w = &mut md;
    let _ = writeln!(w, "{FINDINGS}");
    let on: Vec<_> = file.findings.iter().filter(|f| f.node.is_some()).collect();
    let outside: Vec<_> = file.findings.iter().filter(|f| f.node.is_none()).collect();
    let stops: HashSet<&str> = on.iter().filter_map(|f| f.node.as_deref()).collect();
    let severe = file
        .findings
        .iter()
        .filter(|f| matches!(f.severity, Severity::Critical | Severity::High))
        .count();
    let _ = writeln!(
        w,
        "{} at {} · {} · {severe} critical or high · {} on {} · {} outside this change · {} rejected\n",
        finding_text(&file.source),
        &map.head[..10],
        plural(file.findings.len(), "finding"),
        on.len(),
        plural(stops.len(), "stop"),
        outside.len(),
        file.rejected.len()
    );
    let item = |w: &mut String, f: &crate::findings::Finding| {
        let node = f
            .node
            .as_deref()
            .map(|n| format!(" · {n}"))
            .unwrap_or_default();
        let _ = writeln!(
            w,
            "- **{}** {}{node} · `{}`{FINDING_TEXT_MARK}{}",
            f.severity.label(),
            f.category,
            f.place(),
            finding_text(&f.content)
        );
    };
    if file.findings.is_empty() {
        let _ = writeln!(w, "None.\n");
    }
    if !on.is_empty() {
        let _ = writeln!(w, "### On the stops");
        for f in &on {
            item(w, f);
        }
        let _ = writeln!(w);
    }
    if !outside.is_empty() {
        let _ = writeln!(w, "### Outside this change");
        for f in &outside {
            item(w, f);
        }
        let _ = writeln!(w);
    }
    if !file.rejected.is_empty() {
        let _ = writeln!(
            w,
            "{} rejected: the file, the lines or the code {} quotes are not there at this commit (reasons in `{}.findings.json`).\n",
            plural(file.rejected.len(), "finding"),
            if file.rejected.len() == 1 {
                "it"
            } else {
                "they"
            },
            map.head
        );
    }
    md
}

/// The node's main place: its main site, or the file it deletes or moves.
fn main_place(n: &Node) -> String {
    n.sites
        .first()
        .map(|s| span_path(&s.file, s.start as usize, s.end as usize))
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
        if map.grouped {
            let _ = writeln!(w, "- `{label}` `{}` ({id})", e.at);
        } else {
            let _ = writeln!(
                w,
                "- {} → `{label}` `{}` ({id})",
                slot("the route, command, job, event or UI action that reaches it"),
                e.at
            );
        }
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
    let step_limit = if map.grouped {
        MAX_MODULE_STEPS
    } else {
        usize::MAX
    };
    for (i, k) in after_order.iter().take(step_limit).enumerate() {
        let _ = writeln!(
            w,
            "{}. {} `{}` ({k})",
            i + 1,
            slot("what this step does"),
            first_path(&flow.after, k)
        );
    }
    let _ = writeln!(w);

    if after_order.len() > step_limit {
        let _ = writeln!(
            w,
            "{} more module steps are listed in the map; prose capped at {step_limit}.\n",
            after_order.len() - step_limit
        );
    }
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
    let tables = [
        (
            &flow.callers,
            "Affected by the change",
            "Calls",
            "What changes for it",
            "what is now different for it, or delete the row when nothing changes for it",
        ),
        (
            &flow.depends_on,
            "Used by the change",
            "Used by",
            "What for",
            "what the change uses it for, or delete the row when the change does not use it",
        ),
    ];
    for (index, (links, heading, column, third_column, hint)) in tables
        .into_iter()
        .filter(|(links, ..)| !links.is_empty())
        .enumerate()
    {
        if index > 0 {
            let _ = writeln!(w);
        }
        let _ = writeln!(w, "### {heading}");
        let _ = writeln!(w, "| Code | {column} | {third_column} |");
        let _ = writeln!(w, "| --- | --- | --- |");
        for l in links {
            let _ = writeln!(
                w,
                "| `{}` `{}` | {} | {} |",
                l.symbol.replace('|', "\\|"),
                l.at,
                l.nodes.join(", "),
                slot(hint)
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

    let _ = writeln!(w, "## Read order");
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
    for n in &map.nodes {
        // A callout or a criterion already tells the reader what to check in it.
        if n.risk != Risk::Read
            || n.touched_by_test
            || !n.criteria.is_empty()
            || map.callouts.iter().any(|c| c.node == n.id)
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
            .map(|u| match keyword_line(u) {
                Some(line) => format!(" ({} at `{}:{line}`)", u.risk_why, u.file),
                None => format!(" ({})", u.risk_why),
            })
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
    if !changes.skipped.is_empty() {
        let mut by_reason: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for s in &changes.skipped {
            by_reason.entry(s.reason).or_default().push(&s.file);
        }
        let groups: Vec<String> = by_reason
            .into_iter()
            .map(|(reason, files)| {
                let shown: Vec<String> = files
                    .iter()
                    .take(MAX_SKIPPED_SHOWN)
                    .map(|f| format!("`{f}`"))
                    .collect();
                let more = files.len().saturating_sub(MAX_SKIPPED_SHOWN);
                let more = if more > 0 {
                    format!(" and {more} more")
                } else {
                    String::new()
                };
                format!("{reason}: {}{more}", shown.join(", "))
            })
            .collect();
        out.push(format!(
            "Not covered: {} ({}).",
            plural(changes.skipped.len(), "skipped file"),
            groups.join("; ")
        ));
    }
    out
}

/// Skipped files named per reason in Open questions; the rest are counted.
const MAX_SKIPPED_SHOWN: usize = 4;

/// The first added line of `u` holding the keyword its risk names (`mentions "<keyword>"`).
fn keyword_line(u: &Unit) -> Option<u32> {
    let keyword = u
        .risk_why
        .strip_prefix("mentions \"")?
        .strip_suffix('"')?
        .to_lowercase();
    u.diff.iter().find_map(|l| {
        let (head, code) = l.strip_prefix('+')?.split_once("| ")?;
        code.to_lowercase()
            .contains(&keyword)
            .then(|| head.trim().parse().ok())
            .flatten()
    })
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
            sites: vec![Site {
                file: file.into(),
                start,
                end: start + 2,
            }],
            ..Node::default()
        }
    }

    fn fixture() -> (ChangeMap, FlowFile) {
        let map = ChangeMap {
            version: 1,
            grouped: false,
            grouping: None,
            path: None,
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
            generated: Value::Null,
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

    fn findings_file() -> crate::findings::FindingsFile {
        use crate::findings::{Finding, FindingsFile, Rejected, Severity};
        let finding = |id: &str,
                       node: Option<&str>,
                       file: &str,
                       start: Option<u32>,
                       severity,
                       content: &str| Finding {
            id: id.into(),
            node: node.map(Into::into),
            file: file.into(),
            start,
            end: start.map(|s| s + 1),
            severity,
            category: "bug".into(),
            rule: None,
            content: content.into(),
            suggestion: None,
        };
        FindingsFile {
            version: 1,
            base: BASE.into(),
            head: HEAD.into(),
            source: "OpenCodeReview".into(),
            findings: vec![
                finding(
                    "F1",
                    Some("N2"),
                    "src/services/d.ts",
                    Some(5),
                    Severity::High,
                    "Negative fee.\n\nMore detail.",
                ),
                finding(
                    "F2",
                    Some("N2"),
                    "src/services/d.ts",
                    Some(6),
                    Severity::Low,
                    "See `x.ts:999` <b>now</b> — N99",
                ),
                finding("F3", None, "src/db/s.ts", None, Severity::Medium, ""),
            ],
            rejected: vec![Rejected {
                file: "gone.ts".into(),
                start: Some(1),
                end: Some(1),
                severity: Severity::Low,
                content: "x".into(),
                reason: "no such file".into(),
            }],
        }
    }

    #[test]
    fn findings_section_lists_stops_then_outside() {
        let (map, _) = fixture();
        let md = findings_section(&map, &findings_file());
        assert_eq!(
            md,
            format!(
                "## Findings\n\
                 OpenCodeReview at 89abcdef01 · 3 findings · 1 critical or high · 2 on 1 stop · 1 outside this change · 1 rejected\n\n\
                 ### On the stops\n\
                 - **High** bug · N2 · `src/services/d.ts:5-6` — Negative fee. More detail.\n\
                 - **Low** bug · N2 · `src/services/d.ts:6-7` — See `x.ts:999` &lt;b&gt;now&lt;/b&gt; — N99\n\n\
                 ### Outside this change\n\
                 - **Medium** bug · `src/db/s.ts` — (no text)\n\n\
                 1 finding rejected: the file, the lines or the code it quotes are not there at this commit (reasons in `{HEAD}.findings.json`).\n\n"
            )
        );
    }

    #[test]
    fn finding_text_keeps_all_of_a_long_finding() {
        let long = format!(
            "{} the tail says how bad.\n\nSecond paragraph.",
            "word ".repeat(200)
        );
        let text = finding_text(&long);
        assert!(text.ends_with("the tail says how bad. Second paragraph."));
        assert!(!text.contains('…'));
    }

    #[test]
    fn finding_text_escapes_html_outside_code_spans_only() {
        for (input, want) in [
            ("`a<b>` <i>", "`a<b>` &lt;i&gt;"),
            ("``a ` <b> ``", "``a ` <b> ``"),
            // Unclosed runs are text: what follows them is escaped.
            ("`<img>``", "`&lt;img&gt;``"),
            ("a ` <img>", "a ` &lt;img&gt;"),
            // An escaped backtick opens nothing.
            ("\\`<img>`", "\\`&lt;img&gt;`"),
            ("\\<b> & `x`", "&lt;b&gt; &amp; `x`"),
            ("café `ü<1>` ö", "café `ü<1>` ö"),
        ] {
            assert_eq!(escape_outside_code(input), want, "{input}");
        }
    }

    #[test]
    fn findings_section_replaces_its_old_self_or_goes_before_watch_out() {
        let page =
            "# T\n\n## Read first\n1. a\n\n## Watch out\nNone.\n\n## Open questions\n- None.\n";
        let first = with_section(page, FINDINGS, "## Findings\nold\n");
        assert_eq!(
            first,
            "# T\n\n## Read first\n1. a\n\n## Findings\nold\n\n## Watch out\nNone.\n\n## Open questions\n- None.\n"
        );
        let second = with_section(&first, FINDINGS, "## Findings\nnew\n");
        assert_eq!(second, first.replace("old", "new"));
        let at_end = with_section("# T\n\n## Flow\nx\n", FINDINGS, "## Findings\ny\n");
        assert_eq!(at_end, "# T\n\n## Flow\nx\n\n## Findings\ny\n\n");
    }

    #[test]
    fn large_slots_are_capped_per_module() {
        let (mut map, mut flow) = fixture();
        let changes = Changes::default();
        map.grouped = true;
        let template = map.nodes[0].clone();
        map.nodes = (1..=55)
            .map(|i| {
                let mut n = template.clone();
                n.id = format!("N{i}");
                n
            })
            .collect();
        map.tour = map.nodes.iter().map(|n| n.id.clone()).collect();
        flow.after["nodes"] = serde_json::json!({});
        for n in &map.nodes {
            flow.after["nodes"][&n.id] = serde_json::json!({ "n": n.id, "paths": ["src/a.ts:1"] });
        }
        let md = page(
            &map,
            &flow,
            &changes,
            &Kind {
                title: "PR overview",
                scope: None,
            },
        );
        assert_eq!(
            md.matches("fill: what this step does").count(),
            MAX_MODULE_STEPS
        );
        assert!(md.contains("35 more module steps"));
        assert!(md.matches(SLOT).count() < 45);
    }

    #[test]
    fn fact_questions_cite_the_keyword_and_group_skipped_files() {
        let (mut map, flow) = fixture();
        map.nodes.push(node("N9", Risk::Read, &[], "seed.ts", 1));
        let mut changes = Changes::default();
        changes.units.push(Unit {
            file: "seed.ts".into(),
            start: 1,
            end: 3,
            symbol: "seed".into(),
            ident: None,
            span: None,
            exported: false,
            lane: "db".into(),
            risk: Risk::Read,
            risk_why: "mentions \"password\"".into(),
            comment_only: false,
            added: vec![1, 2],
            diff: vec![
                "+    1 | const user = 'a'".into(),
                "+    2 | const Password = 'x'".into(),
            ],
        });
        for (file, reason) in [
            ("env.d.ts", "generated or binary"),
            ("tree.gen.ts", "generated or binary"),
            ("a.webp", "generated or binary"),
            ("b.webp", "generated or binary"),
            ("c.webp", "generated or binary"),
            ("vendor/x", "submodule"),
        ] {
            changes.skipped.push(facts::Skipped {
                file: file.into(),
                reason,
            });
        }
        let qs = fact_questions(&map, &flow, &changes);
        let read: Vec<_> = qs.iter().filter(|q| q.contains("marked read")).collect();
        // N2 has a criterion and a callout: they already say what to check.
        assert_eq!(
            read,
            [
                "N9 `seed.ts:1-3` is marked read (mentions \"password\" at `seed.ts:2`), and no changed test references it."
            ]
        );
        let skipped: Vec<_> = qs.iter().filter(|q| q.contains("skipped")).collect();
        assert_eq!(
            skipped,
            [
                "Not covered: 6 skipped files (generated or binary: `env.d.ts`, `tree.gen.ts`, `a.webp`, `b.webp` and 1 more; submodule: `vendor/x`)."
            ]
        );
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

## Read order
1. N2 `src/services/d.ts:5-7`: <!-- fill: why this one first: the decision it holds -->
2. N1 `src/api/h.ts:4-6`: <!-- fill: why this one first: the decision it holds -->

Skim: N3.

## Watch out
- **Breaking** N2 `src/services/d.ts:6`: returns a negative fee for x < 1

## Open questions
- REF-2 (gives a discount) has no test that checks it. Its code: N2 `src/services/d.ts:5-7`.
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
