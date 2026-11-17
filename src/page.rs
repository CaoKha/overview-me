//! `overview-me page`: a store markdown file (PR or codebase overview)
//! as a standalone HTML page, for the browser or a Claude artifact.
//!
//! The page itself is `assets/overview.html`, compiled in. It styles the markdown as a review
//! sheet, links `path:line` code spans to the forge, puts "### Before" and "### After" side by
//! side, and draws ```` ```flow ```` blocks as lane diagrams: hovering a box traces its edges,
//! clicking it shows its file paths with copy buttons.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::git::Git;

const TEMPLATE: &str = include_str!("../assets/overview.html");

const NODE_KINDS: [&str; 4] = ["existing", "added", "removed", "store"];
const EDGE_STYLES: [&str; 3] = ["dashed", "added", "removed"];

/// Writes the page for `md` to `out` (default: `md` with an `.html` extension) and returns it.
/// With `unpushed`, the head is not on the remote, so only references under Before (at the
/// map's base) link to the forge.
///
/// # Errors
/// When a ```` ```flow ```` block is invalid (every problem is listed), or a file cannot be read
/// or written.
pub fn run(git: &Git, md: &Path, out: Option<PathBuf>, unpushed: bool) -> Result<PathBuf> {
    let text = std::fs::read_to_string(md).with_context(|| format!("reading {}", md.display()))?;
    let name = md.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let head = name
        .split('.')
        .next()
        .filter(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()));
    let flows: Option<Value> = head.and_then(|s| {
        let file = md.with_file_name(format!("{s}.flow.json"));
        serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()
    });
    let (text, mut errs) = expand_flows(&text, flows.as_ref());
    errs.extend(check_flows(&text));
    // `<head>.review.md`, `<head>.md`: references point at head; Before ones at the map's base.
    let sha = head;
    let map: Option<Value> = sha.and_then(|s| {
        let map = md.with_file_name(format!("{s}.json"));
        serde_json::from_str(&std::fs::read_to_string(map).ok()?).ok()
    });
    let base = map
        .as_ref()
        .and_then(|v| v.get("base")?.as_str().map(str::to_owned));
    let ids: Option<HashSet<String>> = map.as_ref().map(|v| {
        v["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| n["id"].as_str().map(str::to_owned))
            .collect()
    });
    // A codebase or folder overview names its commit in the header: "sha <40 hex>".
    let cited_at = sha.map(str::to_owned).or_else(|| {
        text.split_whitespace()
            .skip_while(|w| *w != "sha")
            .nth(1)
            .filter(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(str::to_owned)
    });
    let cache: RefCell<HashMap<(String, String), Option<usize>>> = RefCell::default();
    let lines_at = |at: &str, path: &str| {
        *cache
            .borrow_mut()
            .entry((at.to_owned(), path.to_owned()))
            .or_insert_with(|| {
                git.run(&["show", "--end-of-options", &format!("{at}:{path}")])
                    .ok()
                    .map(|t| t.lines().count())
            })
    };
    errs.extend(check_page(
        &text,
        ids.as_ref(),
        cited_at.as_deref(),
        base.as_deref(),
        &lines_at,
    ));
    if !errs.is_empty() {
        bail!("{}:\n  {}", md.display(), errs.join("\n  "));
    }
    let title = text
        .lines()
        .find_map(|l| l.strip_prefix("# "))
        .map_or_else(|| name.to_owned(), str::to_owned);
    let forge = git
        .run(&["remote", "get-url", "origin"])
        .ok()
        .and_then(|r| forge_url(r.trim()));
    let out = out.unwrap_or_else(|| md.with_extension("html"));
    let page = render(
        &title,
        &text,
        forge.as_deref(),
        sha.filter(|_| !unpushed),
        base.as_deref(),
    );
    std::fs::write(&out, page).with_context(|| format!("writing {}", out.display()))?;
    Ok(out)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `</script` or `<!--` in the markdown would end or break the data block; the page undoes this.
fn guard(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let mut rest = md;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let script = tail
            .get(1..8)
            .is_some_and(|t| t.eq_ignore_ascii_case("/script"));
        out.push_str(if script || tail.starts_with("<!--") {
            "<\\"
        } else {
            "<"
        });
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

/// The page for `md`. `sha` is the commit the `path:line` references point at (or `None` to read
/// "sha <40 hex>" from the header line); `base` is the commit for references under Before.
pub fn render(
    title: &str,
    md: &str,
    forge: Option<&str>,
    sha: Option<&str>,
    base: Option<&str>,
) -> String {
    let config = serde_json::json!({ "forge": forge, "sha": sha, "base": base }).to_string();
    let values = [
        ("TITLE", escape(title)),
        ("MD", guard(md)),
        ("CONFIG", config),
    ];
    // One pass over the template, so placeholder-like text inside the values is left alone.
    let mut out = String::with_capacity(TEMPLATE.len() + md.len());
    let mut rest = TEMPLATE;
    while let Some(i) = rest.find("{{") {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 2..];
        match tail.find("}}").and_then(|j| {
            values
                .iter()
                .find(|(k, _)| *k == &tail[..j])
                .map(|v| (j, v))
        }) {
            Some((j, (_, v))) => {
                out.push_str(v);
                rest = &tail[j + 2..];
            }
            None => {
                out.push_str("{{");
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `path:line` or `path:start-end` in a code span: the path and the last line cited.
fn cited(span: &str) -> Option<(&str, usize)> {
    let (path, lines) = span.rsplit_once(':')?;
    let ok_lines = lines
        .split('-')
        .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    let ok_path = !path.is_empty()
        && !path.contains(char::is_whitespace)
        && (path.contains('/') || path.contains('.'))
        && !path.contains("://");
    if !(ok_lines && ok_path) {
        return None;
    }
    Some((path, lines.rsplit('-').next()?.parse().ok()?))
}

/// The code spans of a line, and the line with them blanked out.
fn spans(line: &str) -> (Vec<&str>, String) {
    let mut found = Vec::new();
    let mut prose = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(i) = rest.find('`') {
        prose.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let Some(j) = after.find('`') else {
            prose.push_str(&rest[i..]);
            rest = "";
            break;
        };
        found.push(&after[..j]);
        prose.push(' ');
        rest = &after[j + 1..];
    }
    prose.push_str(rest);
    (found, prose)
}

/// What a reader would trip over on the page: slots left to fill, `path:line` references that
/// point at no line (at `base` under "### Before", else at `head`), node ids the map does not
/// have, and open questions that cite neither code nor a criterion. `lines_at(sha, path)` gives
/// a file's line count, or `None` when the file is not there.
pub fn check_page(
    md: &str,
    ids: Option<&HashSet<String>>,
    head: Option<&str>,
    base: Option<&str>,
    lines_at: &dyn Fn(&str, &str) -> Option<usize>,
) -> Vec<String> {
    let mut errs = Vec::new();
    let (mut fenced, mut in_before, mut section) = (false, false, String::new());
    for (i, line) in md.lines().enumerate() {
        let at = format!("line {}", i + 1);
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if let Some(h) = line.strip_prefix("## ") {
            section = h.trim().to_lowercase();
            in_before = false;
        } else if let Some(h) = line.strip_prefix("### ") {
            in_before = h.trim_start().to_lowercase().starts_with("before");
        }
        if let Some(j) = line.find(crate::render::SLOT) {
            let hint = line[j + crate::render::SLOT.len()..]
                .split("-->")
                .next()
                .unwrap_or_default()
                .trim();
            errs.push(format!("{at}: slot left to fill: {hint}"));
            continue;
        }
        let (code, prose) = spans(line);
        let sha = if in_before { base.or(head) } else { head };
        if let Some(sha) = sha {
            let short = &sha[..10.min(sha.len())];
            for span in &code {
                let Some((path, last)) = cited(span) else {
                    continue;
                };
                match lines_at(sha, path) {
                    None => errs.push(format!("{at}: `{span}`: no such file at {short}")),
                    Some(n) if last > n || last == 0 => {
                        errs.push(format!("{at}: `{span}`: the file has {n} lines at {short}"));
                    }
                    Some(_) => {}
                }
            }
        }
        if let Some(ids) = ids {
            for word in prose.split(|c: char| !c.is_ascii_alphanumeric()) {
                let is_id = word.len() > 1
                    && word.starts_with('N')
                    && word[1..].bytes().all(|b| b.is_ascii_digit());
                if is_id && !ids.contains(word) {
                    errs.push(format!("{at}: {word} is not a node in the map"));
                }
            }
        }
        if section == "open questions"
            && let Some(item) = line.strip_prefix("- ")
        {
            let cites_criterion = item
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                .any(|w| {
                    w.strip_prefix("REF-")
                        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                });
            if code.is_empty() && !cites_criterion && item.trim() != "None." {
                errs.push(format!(
                    "{at}: an open question must cite `path:line` or a criterion: {item}"
                ));
            }
        }
    }
    errs
}

/// Replaces each ```` ```flow ```` block that holds only `@before` or `@after` with that
/// diagram from the `overview-me flow` file, and lists the blocks it could not fill.
pub fn expand_flows(md: &str, flows: Option<&Value>) -> (String, Vec<String>) {
    let mut out = String::with_capacity(md.len());
    let mut errs = Vec::new();
    let lines: Vec<&str> = md.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let directive = lines
            .get(i + 1)
            .map(|l| l.trim())
            .filter(|l| matches!(*l, "@before" | "@after"));
        if line.trim_end() == "```flow"
            && let Some(which) = directive
            && lines.get(i + 2).is_some_and(|l| l.trim_end() == "```")
        {
            let side = &which[1..];
            match flows.map(|f| f.get(side)) {
                None => errs.push(format!(
                    "line {}: {which} needs the flow file: run `overview-me flow <map>` first",
                    i + 1
                )),
                Some(None | Some(Value::Null)) => errs.push(format!(
                    "line {}: the flow file has no {side} diagram (the flow is new)",
                    i + 1
                )),
                Some(Some(spec)) => {
                    out.push_str("```flow\n");
                    out.push_str(&spec.to_string());
                    out.push_str("\n```\n");
                    i += 3;
                    continue;
                }
            }
        }
        out.push_str(line);
        out.push('\n');
        i += 1;
    }
    (out, errs)
}

/// Problems in the ```` ```flow ```` blocks of `md` (empty when all are valid).
pub fn check_flows(md: &str) -> Vec<String> {
    let mut errs = Vec::new();
    let mut block: Option<(usize, Vec<&str>)> = None;
    for (i, line) in md.lines().enumerate() {
        match &mut block {
            Some((start, lines)) => {
                if line.trim_end() == "```" {
                    check_flow(&lines.join("\n"), *start, &mut errs);
                    block = None;
                } else {
                    lines.push(line);
                }
            }
            None if line.trim_end() == "```flow" => block = Some((i + 1, Vec::new())),
            None => {}
        }
    }
    if let Some((start, _)) = block {
        errs.push(format!("flow block at line {start}: not closed"));
    }
    errs
}

fn check_flow(text: &str, line: usize, errs: &mut Vec<String>) {
    let mut err = |msg: String| errs.push(format!("flow block at line {line}: {msg}"));
    let spec: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return err(format!("not valid JSON ({e})")),
    };
    let (Some(nodes), Some(tiers)) = (
        spec.get("nodes").and_then(Value::as_object),
        spec.get("tiers").and_then(Value::as_array),
    ) else {
        return err("needs \"nodes\" (object) and \"tiers\" (array)".into());
    };
    for (id, n) in nodes {
        if n.get("label").and_then(Value::as_str).is_none() {
            err(format!("node {id} needs a \"label\""));
        }
        for k in n
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .split_whitespace()
        {
            if !NODE_KINDS.contains(&k) {
                err(format!("node {id}: unknown kind \"{k}\""));
            }
        }
        if let Some(paths) = n.get("paths") {
            let ok = paths
                .as_array()
                .is_some_and(|a| a.iter().all(Value::is_string));
            if !ok {
                err(format!("node {id}: \"paths\" must be a list of strings"));
            }
        }
    }
    let mut placed: Vec<&str> = Vec::new();
    for tier in tiers {
        for lane in tier.as_array().map(Vec::as_slice).unwrap_or_default() {
            let (Some(name), Some(ids)) = (
                lane.get("name").and_then(Value::as_str),
                lane.get("ids").and_then(Value::as_array),
            ) else {
                err("each tier is a list of lanes {\"name\", \"ids\"}".into());
                continue;
            };
            for id in ids.iter().filter_map(Value::as_str) {
                if !nodes.contains_key(id) {
                    err(format!("lane {name}: no node {id}"));
                } else if placed.contains(&id) {
                    err(format!("node {id} is in more than one lane"));
                }
                placed.push(id);
            }
        }
    }
    for id in nodes.keys() {
        if !placed.contains(&id.as_str()) {
            err(format!("node {id} is in no lane"));
        }
    }
    let edges = spec.get("edges").and_then(Value::as_array);
    for e in edges.map(Vec::as_slice).unwrap_or_default() {
        let end = |i: usize| e.get(i).and_then(Value::as_str);
        match (end(0), end(1)) {
            (Some(a), Some(b)) if placed.contains(&a) && placed.contains(&b) => {
                if let Some(style) = e.get(3).filter(|s| !s.is_null())
                    && !style.as_str().is_some_and(|s| EDGE_STYLES.contains(&s))
                {
                    err(format!("edge {a} → {b}: unknown style {style}"));
                }
            }
            _ => err(format!("edge {e}: both ends must be placed nodes")),
        }
    }
}

/// https web URL of a remote, from ssh or https remote URLs; `None` when it is not one.
fn forge_url(remote: &str) -> Option<String> {
    let mut u = if let Some(rest) = remote.strip_prefix("git@") {
        format!("https://{}", rest.replacen(':', "/", 1))
    } else if let Some(rest) = remote.strip_prefix("ssh://git@") {
        let (host, path) = rest.split_once('/')?;
        format!("https://{}/{path}", host.split(':').next()?)
    } else {
        let rest = remote.strip_prefix("https://")?;
        // Drop credentials: https://user:token@host/…
        let rest = match rest.split_once('/') {
            Some((auth, path)) => format!("{}/{path}", auth.rsplit('@').next()?),
            None => rest.to_owned(),
        };
        format!("https://{rest}")
    };
    if let Some(s) = u.strip_suffix(".git") {
        u = s.to_owned();
    }
    let u = u.trim_end_matches('/').to_owned();
    let path = u.strip_prefix("https://")?.split_once('/')?.1;
    (!path.is_empty()).then_some(u)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forge_urls() {
        let cases = [
            ("git@github.com:o/r.git", Some("https://github.com/o/r")),
            (
                "ssh://git@gitlab.com:22/g/r.git",
                Some("https://gitlab.com/g/r"),
            ),
            (
                "https://tok@github.com/o/r/",
                Some("https://github.com/o/r"),
            ),
            ("https://github.com", None),
            ("/local/path", None),
        ];
        for (remote, want) in cases {
            assert_eq!(forge_url(remote).as_deref(), want, "{remote}");
        }
    }

    #[test]
    fn render_fills_once_and_guards_script() {
        let md = "# T\n</SCRIPT> <!-- x {{CONFIG}}";
        let page = render("a<b", md, Some("https://h/o/r"), None, None);
        assert!(page.contains("<title>a&lt;b</title>"));
        assert!(page.contains("<\\/SCRIPT> <\\!-- x {{CONFIG}}"));
        assert!(page.contains(r#""forge":"https://h/o/r""#));
        assert!(!page.contains("{{MD}}") && !page.contains("{{TITLE}}"));
    }

    #[test]
    fn expands_flow_directives() {
        let flows =
            serde_json::json!({"before": null, "after": {"nodes": {}, "tiers": [], "edges": []}});
        let (out, errs) = expand_flows("a\n```flow\n@after\n```\nb\n", Some(&flows));
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(
            out,
            "a\n```flow\n{\"edges\":[],\"nodes\":{},\"tiers\":[]}\n```\nb\n"
        );
        let (_, errs) = expand_flows("```flow\n@before\n```\n", Some(&flows));
        assert!(errs[0].contains("no before diagram"), "{errs:?}");
        let (_, errs) = expand_flows("```flow\n@after\n```\n", None);
        assert!(errs[0].contains("run `overview-me flow"), "{errs:?}");
    }

    #[test]
    fn page_checks() {
        let md = "# T\n\n## Flow\n### Before: x\n1. old `src/a.ts:9` (base)\n### After: y\n1. new `src/a.ts:9` (N1) and N7\n```flow\n{\"x\": \"N9 `src/zz.ts:1`\"}\n```\n## Open questions\n- REF-1 has no test.\n- does it scale?\n- `src/gone.ts:3` and `src/a.ts:0`\n- None.\n<!-- fill: feature name -->\n";
        let ids: HashSet<String> = ["N1".to_owned()].into();
        let lines_at = |sha: &str, path: &str| match (sha, path) {
            ("base", "src/a.ts") => Some(10),
            ("head", "src/a.ts") => Some(5),
            _ => None,
        };
        let errs = check_page(md, Some(&ids), Some("head"), Some("base"), &lines_at);
        assert_eq!(
            errs,
            vec![
                "line 7: `src/a.ts:9`: the file has 5 lines at head",
                "line 7: N7 is not a node in the map",
                "line 13: an open question must cite `path:line` or a criterion: does it scale?",
                "line 14: `src/gone.ts:3`: no such file at head",
                "line 14: `src/a.ts:0`: the file has 5 lines at head",
                "line 16: slot left to fill: feature name",
            ]
        );
        assert_eq!(cited("http://x.io:80"), None);
        assert_eq!(cited("a.rs:3-12"), Some(("a.rs", 12)));
        assert_eq!(cited("ratio:2"), None);
    }

    #[test]
    fn flow_checks() {
        let good = r#"```flow
{"nodes":{"a":{"label":"A","paths":["src/a.rs:1"]},"b":{"label":"B","kind":"existing store"}},
 "tiers":[[{"name":"ui","ids":["a"]}],[{"name":"db","ids":["b"]}]],
 "edges":[["a","b","write","added"]]}
```"#;
        assert!(check_flows(good).is_empty(), "{:?}", check_flows(good));
        let bad = r#"```flow
{"nodes":{"a":{"label":"A","kind":"weird","paths":"x"},"c":{}},
 "tiers":[[{"name":"ui","ids":["a","z"]}]],
 "edges":[["a","b"],["a","a","x","bold"]]}
```
```flow
{nope
```
```flow
{}"#;
        let errs = check_flows(bad).join("\n");
        for want in [
            "unknown kind \"weird\"",
            "\"paths\" must be a list",
            "node c needs a \"label\"",
            "lane ui: no node z",
            "node c is in no lane",
            "both ends must be placed",
            "unknown style \"bold\"",
            "line 6: not valid JSON",
            "line 9: not closed",
        ] {
            assert!(errs.contains(want), "missing {want:?} in\n{errs}");
        }
    }
}
