//! `overview-me page`: a store markdown file (PR overview, codebase overview or review guide)
//! as a standalone HTML page, for the browser or a Claude artifact.
//!
//! The page itself is `assets/overview.html`, compiled in. It styles the markdown as a review
//! sheet, links `path:line` code spans to the forge, puts "### Before" and "### After" side by
//! side, and draws ```` ```flow ```` blocks as lane diagrams: hovering a box traces its edges,
//! clicking it shows its file paths with copy buttons.

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
    let errs = check_flows(&text);
    if !errs.is_empty() {
        bail!("{}:\n  {}", md.display(), errs.join("\n  "));
    }
    let name = md.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let title = text
        .lines()
        .find_map(|l| l.strip_prefix("# "))
        .map_or_else(|| name.to_owned(), str::to_owned);
    // `<head>.review.md`, `<head>.md`: references point at head; Before ones at the map's base.
    let sha = name
        .split('.')
        .next()
        .filter(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()));
    let base = sha.and_then(|s| {
        let map = md.with_file_name(format!("{s}.json"));
        let v: Value = serde_json::from_str(&std::fs::read_to_string(map).ok()?).ok()?;
        v.get("base")?.as_str().map(str::to_owned)
    });
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
