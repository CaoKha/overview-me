//! Deterministic facts about `base...head`: changed hunks outside tests, split at symbol boundaries.

use std::collections::HashSet;

use anyhow::Result;
use serde::Serialize;

use crate::git::Git;
use crate::map::{Lane, Rename, Risk, TestRef};
use crate::rules::Rules;
use crate::symbols::{self, Parsed, Symbol};

/// One changed hunk inside one enclosing symbol. Lines are head-side, 1-based, inclusive.
/// A deletion-only hunk is anchored on the line before the removed lines.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Unit {
    pub file: String,
    pub start: u32,
    pub end: u32,
    pub symbol: String,
    #[serde(skip)]
    pub ident: Option<String>,
    #[serde(skip)]
    pub span: Option<(u32, u32)>,
    #[serde(skip)]
    pub exported: bool,
    #[serde(skip)]
    pub lane: Lane,
    pub risk: Risk,
    pub risk_why: String,
    pub comment_only: bool,
    /// Added head lines; empty for a deletion-only unit.
    #[serde(skip)]
    pub added: Vec<u32>,
    pub diff: Vec<String>,
}

impl Unit {
    pub fn contains(&self, line: u32) -> bool {
        self.start <= line && line <= self.end
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Skipped {
    pub file: String,
    pub reason: &'static str,
}

/// A changed, parsed, non-test file.
#[derive(Debug)]
pub struct ChangedFile {
    pub path: String,
    pub parsed: Option<Parsed>,
}

#[derive(Debug, Default)]
pub struct Changes {
    pub units: Vec<Unit>,
    pub test_files: Vec<String>,
    pub skipped: Vec<Skipped>,
    /// Deleted non-test files: each must be in one node's `deletions`.
    pub deleted: Vec<String>,
    /// Pure renames touching non-test paths, and moves of non-test files into test paths:
    /// each must be in one node's `renames`.
    pub renames: Vec<Rename>,
    pub files: Vec<ChangedFile>,
}

struct FileDiff {
    path: String,
    old_path: String,
    deleted: bool,
    binary: bool,
    /// A path that is not valid UTF-8; the file is skipped rather than renamed.
    bad_path: bool,
    renamed: bool,
    /// A gitlink (submodule pointer, mode 160000): there is no file to show.
    submodule: bool,
    hunks: Vec<Hunk>,
}

struct Hunk {
    old_start: u32,
    new_start: u32,
    new_count: u32,
    lines: Vec<String>,
}

/// Decodes a C-quoted git path (`"a/\303\251\tx"`) and returns it with the text after the
/// closing quote.
fn unquote(s: &str) -> Option<(Vec<u8>, &str)> {
    let mut bytes = s.strip_prefix('"')?.bytes().enumerate();
    let mut out = Vec::new();
    while let Some((i, b)) = bytes.next() {
        match b {
            b'"' => return Some((out, &s[i + 2..])),
            b'\\' => {
                let (_, e) = bytes.next()?;
                let decoded = match e {
                    b'n' => b'\n',
                    b't' => b'\t',
                    b'r' => b'\r',
                    b'a' => 0x07,
                    b'b' => 0x08,
                    b'f' => 0x0c,
                    b'v' => 0x0b,
                    b'0'..=b'3' => {
                        let mut value = e - b'0';
                        for _ in 0..2 {
                            let (_, d) = bytes.next().filter(|(_, d)| (b'0'..=b'7').contains(d))?;
                            value = value * 8 + (d - b'0');
                        }
                        value
                    }
                    other => other,
                };
                out.push(decoded);
            }
            other => out.push(other),
        }
    }
    None
}

/// A path after a header keyword: quoted or plain, minus `prefix` (`a/`, `b/` or none).
fn header_path(s: &str, prefix: &str) -> Option<Vec<u8>> {
    let raw = if s.starts_with('"') {
        unquote(s)?.0
    } else {
        // git ends `---`/`+++` paths that contain a space with a tab; a real tab would be quoted.
        s.strip_suffix('\t').unwrap_or(s).as_bytes().to_vec()
    };
    raw.strip_prefix(prefix.as_bytes()).map(<[u8]>::to_vec)
}

/// `diff --git <a> <b>`: the head-side path. Unquoted names may hold spaces, so a plain
/// `a/X b/X` header is split in the middle.
fn git_header_path(rest: &str) -> Option<Vec<u8>> {
    if rest.starts_with('"') {
        let (_, after) = unquote(rest)?;
        return header_path(after.strip_prefix(' ')?, "b/");
    }
    let n = rest.len().checked_sub(5)? / 2;
    if rest.len() == 2 * n + 5 && rest.get(2..2 + n) == rest.get(n + 5..) {
        return rest.get(n + 5..).map(|p| p.as_bytes().to_vec());
    }
    rest.split_once(" b/").map(|(_, b)| b.as_bytes().to_vec())
}

fn parse_diff(raw: &[u8]) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut in_hunk = false;
    for bytes in raw
        .strip_suffix(b"\n")
        .unwrap_or(raw)
        .split(|&b| b == b'\n')
    {
        if bytes.starts_with(b"diff --git ") {
            files.push(FileDiff {
                path: String::new(),
                old_path: String::new(),
                deleted: false,
                binary: false,
                bad_path: false,
                renamed: false,
                submodule: false,
                hunks: Vec::new(),
            });
            in_hunk = false;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if in_hunk && matches!(bytes.first(), None | Some(b'+' | b'-' | b' ' | b'\\')) {
            // Content: decode lossily; one bad byte must not lose the file.
            if let Some(h) = file.hunks.last_mut() {
                h.lines.push(String::from_utf8_lossy(bytes).into_owned());
            }
            continue;
        }
        in_hunk = false;
        let Ok(line) = std::str::from_utf8(bytes) else {
            file.bad_path = true;
            continue;
        };
        let mut set_path = |p: Option<Vec<u8>>, old: bool| match p.map(String::from_utf8) {
            Some(Ok(p)) if old => file.old_path = p,
            Some(Ok(p)) => file.path = p,
            Some(Err(_)) => file.bad_path = true,
            None => {}
        };
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let p = git_header_path(rest);
            set_path(p.clone(), false);
            set_path(p, true);
        } else if let Some(rest) = line.strip_prefix("rename from ") {
            file.renamed = true;
            set_path(header_path(rest, ""), true);
        } else if let Some(rest) = line.strip_prefix("rename to ") {
            set_path(header_path(rest, ""), false);
        } else if let Some(rest) = line.strip_prefix("--- ") {
            if rest != "/dev/null" {
                set_path(header_path(rest, "a/"), true);
            }
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            if rest != "/dev/null" {
                set_path(header_path(rest, "b/"), false);
            }
        } else if let Some((old_start, new_start, new_count)) = hunk_header(line) {
            file.hunks.push(Hunk {
                old_start,
                new_start,
                new_count,
                lines: Vec::new(),
            });
            in_hunk = true;
        } else if line.starts_with("deleted file mode") {
            file.deleted = true;
            file.submodule |= line.ends_with(" 160000");
        } else if line.starts_with("Binary files") {
            file.binary = true;
        } else if line.starts_with("index ") || line.starts_with("new file mode") {
            file.submodule |= line.ends_with(" 160000");
        }
    }
    files
}

/// `@@ -a,b +c,d @@` → `(a, c, d)`; a missing count is 1.
fn hunk_header(line: &str) -> Option<(u32, u32, u32)> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(" +")?;
    let new = rest.split_once(" @@")?.0;
    let start = |r: &str| r.split(',').next().and_then(|n| n.parse().ok());
    let count = new
        .split_once(',')
        .map_or(Some(1), |(_, c)| c.parse().ok())?;
    Some((start(old)?, start(new)?, count))
}

struct Piece<'p> {
    sym: Option<&'p Symbol>,
    start: u32,
    end: u32,
    diff: Vec<String>,
    /// Added lines (head side).
    added: Vec<u32>,
    /// Removed lines (base side).
    removed: Vec<u32>,
}

/// Splits one hunk at symbol boundaries and drops lines inside inline test modules.
/// Blank added lines stay with the code before them; context lines end a piece.
fn pieces<'p>(hunk: &Hunk, parsed: Option<&'p Parsed>) -> Vec<Piece<'p>> {
    let at = |line: u32| parsed.and_then(|p| p.symbol_at(line as usize));
    let in_tests = |line: u32| parsed.is_some_and(|p| p.in_tests(line as usize));
    let mut out: Vec<Piece<'p>> = Vec::new();
    let mut new_line = hunk.new_start;
    let mut old_line = hunk.old_start;
    // The head line just above the next change: removed lines are anchored there.
    let mut above = if hunk.new_count == 0 {
        hunk.new_start
    } else {
        hunk.new_start.saturating_sub(1)
    };
    let mut dels: Vec<(u32, String)> = Vec::new();
    let mut open = false;
    let flush = |dels: &mut Vec<(u32, String)>, out: &mut Vec<Piece<'p>>, anchor: u32| {
        if dels.is_empty() || in_tests(anchor) {
            dels.clear();
            return;
        }
        let mut diff = vec![format!("  {anchor:>4} ^ (lines removed below)")];
        let removed = dels.iter().map(|d| d.0).collect();
        diff.extend(dels.drain(..).map(|d| d.1));
        out.push(Piece {
            sym: at(anchor),
            start: anchor,
            end: anchor,
            diff,
            added: Vec::new(),
            removed,
        });
    };
    for raw in &hunk.lines {
        let (sign, code) = raw.split_at(raw.len().min(1));
        match sign {
            "-" => {
                dels.push((old_line, format!("-      | {code}")));
                old_line += 1;
            }
            "+" => {
                if in_tests(new_line) {
                    dels.clear();
                } else {
                    let sym = match out.last() {
                        Some(last) if open && code.trim().is_empty() => last.sym,
                        _ => at(new_line),
                    };
                    let removed: Vec<u32> = dels.iter().map(|d| d.0).collect();
                    let mut text: Vec<String> = dels.drain(..).map(|d| d.1).collect();
                    text.push(format!("+ {new_line:>4} | {code}"));
                    match out.last_mut() {
                        Some(last) if open && last.sym.map(|s| s.start) == sym.map(|s| s.start) => {
                            last.end = new_line;
                            last.diff.extend(text);
                            last.added.push(new_line);
                            last.removed.extend(removed);
                        }
                        _ => out.push(Piece {
                            sym,
                            start: new_line,
                            end: new_line,
                            diff: text,
                            added: vec![new_line],
                            removed,
                        }),
                    }
                    open = true;
                }
                above = new_line;
                new_line += 1;
            }
            "\\" => {}
            _ => {
                // Context (also an empty line, under diff.suppressBlankEmpty).
                flush(&mut dels, &mut out, above.max(1));
                open = false;
                above = new_line;
                new_line += 1;
                old_line += 1;
            }
        }
    }
    match out.last_mut() {
        Some(last) if open && !dels.is_empty() => {
            last.removed.extend(dels.iter().map(|d| d.0));
            last.diff.extend(dels.drain(..).map(|d| d.1));
        }
        _ => flush(&mut dels, &mut out, above.max(1)),
    }
    out
}

/// A unit holds only comments when every added line is a comment line at head and every
/// removed line was one at the merge base.
fn comment_only(
    head: Option<&Parsed>,
    head_lines: &[&str],
    base: Option<&Parsed>,
    base_lines: &[&str],
    p: &Piece<'_>,
) -> bool {
    let all = |parsed: Option<&Parsed>, lines: &[&str], nums: &[u32]| {
        nums.iter().all(|&n| {
            let text = lines
                .get((n as usize).wrapping_sub(1))
                .copied()
                .unwrap_or_default();
            parsed.is_some_and(|ps| ps.is_comment_line(n as usize, text))
        })
    };
    head.is_some() && all(head, head_lines, &p.added) && all(base, base_lines, &p.removed)
}

/// Reads `base...head` and returns every changed non-test hunk, split per enclosing symbol.
///
/// # Errors
/// When a git command fails or a file cannot be parsed.
pub fn changes(git: &Git, rules: &Rules, base: &str, head: &str) -> Result<Changes> {
    let diff = git.run_bytes(&[
        "diff",
        "-U0",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--find-renames",
        "--src-prefix=a/",
        "--dst-prefix=b/",
        "--end-of-options",
        &format!("{base}...{head}"),
    ])?;
    let merge_base = git
        .run(&["merge-base", "--end-of-options", base, head])?
        .trim()
        .to_owned();
    let mut out = Changes::default();
    for mut f in parse_diff(&diff) {
        if f.bad_path || f.path.is_empty() {
            out.skipped.push(Skipped {
                file: f.path,
                reason: "path is not valid UTF-8",
            });
            continue;
        }
        if f.submodule {
            out.skipped.push(Skipped {
                file: f.path,
                reason: "submodule",
            });
            continue;
        }
        // A move across the test boundary is listed even with edits, and code moved out of a
        // test path is new production code: every line of it must be covered.
        let mut from_tests = false;
        if f.renamed {
            let (from_test, to_test) = (rules.is_test(&f.old_path), rules.is_test(&f.path));
            if (f.hunks.is_empty() && !(from_test && to_test)) || from_test != to_test {
                out.renames.push(Rename {
                    from: f.old_path.clone(),
                    to: f.path.clone(),
                });
            }
            from_tests = from_test && !to_test;
        }
        if rules.is_test(&f.path) {
            if !f.deleted {
                out.test_files.push(f.path);
            }
            continue;
        }
        if f.deleted {
            out.deleted.push(f.path);
            continue;
        }
        let generated = f.binary
            || git
                .run(&["check-attr", "diff", "--", &f.path])?
                .trim()
                .ends_with("diff: unset");
        if generated {
            out.skipped.push(Skipped {
                file: f.path,
                reason: "generated or binary",
            });
            continue;
        }
        let source = git.run(&["show", "--end-of-options", &format!("{head}:{}", f.path)])?;
        let parsed = symbols::parse(&f.path, &source)?;
        let lines: Vec<&str> = source.lines().collect();
        if from_tests {
            f.hunks = vec![Hunk {
                old_start: 0,
                new_start: 1,
                new_count: u32::try_from(lines.len()).unwrap_or(u32::MAX),
                lines: lines.iter().map(|l| format!("+{l}")).collect(),
            }];
        }
        let all_pieces: Vec<Piece<'_>> = f
            .hunks
            .iter()
            .flat_map(|h| pieces(h, parsed.as_ref()))
            .collect();
        // The base side is only needed to judge removed lines.
        let base_source = if all_pieces.iter().any(|p| !p.removed.is_empty()) {
            git.run(&[
                "show",
                "--end-of-options",
                &format!("{merge_base}:{}", f.old_path),
            ])
            .unwrap_or_default()
        } else {
            String::new()
        };
        let base_parsed = symbols::parse(&f.old_path, &base_source)?;
        let base_lines: Vec<&str> = base_source.lines().collect();
        let lane = rules.lane(&f.path);
        let code = symbols::is_code(&f.path);
        for p in all_pieces {
            let changed: Vec<&str> = p
                .diff
                .iter()
                .filter_map(|d| d.split_once("| ").map(|(_, c)| c.trim()))
                .collect();
            let comment_only = code
                && comment_only(
                    parsed.as_ref(),
                    &lines,
                    base_parsed.as_ref(),
                    &base_lines,
                    &p,
                );
            let body = match p.sym {
                Some(s) => {
                    lines
                        .get(s.start - 1..s.end.min(lines.len()))
                        .unwrap_or_default()
                        .join("\n")
                        + &changed.join("\n")
                }
                None => changed.join("\n"),
            };
            let (risk, risk_why) = rules.risk(&f.path, comment_only, &body);
            let fallback = if code { "(top-level)" } else { "(file)" };
            out.units.push(Unit {
                file: f.path.clone(),
                start: p.start,
                end: p.end,
                symbol: p
                    .sym
                    .map_or_else(|| fallback.to_owned(), |s| s.name.clone()),
                ident: p.sym.map(|s| s.ident.clone()),
                span: p.sym.map(|s| (to_u32(s.start), to_u32(s.end))),
                exported: p.sym.is_some_and(|s| s.exported),
                lane,
                risk,
                risk_why,
                comment_only,
                added: p.added,
                diff: p.diff,
            });
        }
        out.files.push(ChangedFile {
            path: f.path,
            parsed,
        });
    }
    Ok(out)
}

fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Test declarations in `source` (the content of `file`).
///
/// # Errors
/// When the file cannot be parsed.
pub fn declarations_in(source: &str, file: &str) -> Result<Vec<TestRef>> {
    let parsed = symbols::parse(file, source)?;
    Ok(parsed.map_or_else(Vec::new, |p| {
        p.tests
            .into_iter()
            .map(|t| TestRef {
                file: file.to_owned(),
                line: u32::try_from(t.line).unwrap_or(u32::MAX),
                name: t.name,
            })
            .collect()
    }))
}

/// Test declarations in the changed test files and in inline Rust test modules of changed files.
///
/// # Errors
/// When git fails or a file cannot be parsed.
pub fn test_declarations(git: &Git, head: &str, changes: &Changes) -> Result<Vec<TestRef>> {
    let mut out = Vec::new();
    for file in &changes.test_files {
        let source = git.run(&["show", "--end-of-options", &format!("{head}:{file}")])?;
        out.extend(declarations_in(&source, file)?);
    }
    for f in &changes.files {
        if let Some(p) = &f.parsed {
            out.extend(p.tests.iter().map(|t| TestRef {
                file: f.path.clone(),
                line: u32::try_from(t.line).unwrap_or(u32::MAX),
                name: t.name.clone(),
            }));
        }
    }
    Ok(out)
}

/// Exported identifiers that a changed test file references at `head` (`git grep -w`).
///
/// # Errors
/// When git grep fails for a reason other than "no match".
pub fn tested_names(
    git: &Git,
    head: &str,
    test_files: &[String],
    names: &HashSet<&str>,
) -> Result<HashSet<String>> {
    let mut found = HashSet::new();
    if test_files.is_empty() {
        return Ok(found);
    }
    for name in names {
        let mut args = vec![
            "grep",
            "-q",
            "-w",
            "-F",
            "-e",
            name,
            "--end-of-options",
            head,
            "--",
        ];
        args.extend(test_files.iter().map(String::as_str));
        if git.check(&args)? {
            found.insert((*name).to_owned());
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hunk(old_start: u32, new_start: u32, new_count: u32, lines: &[&str]) -> Hunk {
        Hunk {
            old_start,
            new_start,
            new_count,
            lines: lines.iter().map(|l| (*l).to_owned()).collect(),
        }
    }

    fn ranges(ps: &[Piece<'_>]) -> Vec<(u32, u32, Vec<u32>)> {
        ps.iter()
            .map(|p| (p.start, p.end, p.added.clone()))
            .collect()
    }

    #[test]
    fn diff_headers_and_hunks_parse() {
        let diff = "diff --git a/x.ts b/x.ts\nindex 1..2 100644\n--- a/x.ts\n+++ b/x.ts\n@@ -3,0 +4,2 @@ ctx\n+a\n+b\n@@ -9 +11,0 @@\n--- removed line starting with dashes\n";
        let files = parse_diff(diff.as_bytes());
        assert_eq!(files.len(), 1);
        let f = &files[0];
        assert_eq!((f.path.as_str(), f.hunks.len()), ("x.ts", 2));
        assert_eq!((f.hunks[0].new_start, f.hunks[0].new_count), (4, 2));
        assert_eq!(
            f.hunks[1].lines,
            vec!["--- removed line starting with dashes"],
            "removed lines inside a hunk are content"
        );
    }

    #[test]
    fn quoted_and_spaced_paths_are_decoded() {
        let diff = concat!(
            "diff --git \"a/d\\303\\251j\\303\\240 vu.ts\" \"b/d\\303\\251j\\303\\240 vu.ts\"\n",
            "--- \"a/d\\303\\251j\\303\\240 vu.ts\"\n+++ \"b/d\\303\\251j\\303\\240 vu.ts\"\n@@ -1 +1 @@\n-a\n+b\n",
            "diff --git \"a/tab\\there.ts\" \"b/tab\\there.ts\"\nnew file mode 100644\n",
            "--- /dev/null\n+++ \"b/tab\\there.ts\"\n@@ -0,0 +1 @@\n+x\n",
            "diff --git a/q\"uote b/q\"uote\n",
            "diff --git a/my file.ts b/my file.ts\n--- a/my file.ts\t\n+++ b/my file.ts\t\n",
            "diff --git a/é.ts b/é.ts\n",
        );
        let paths: Vec<String> = parse_diff(diff.as_bytes())
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(
            paths,
            vec![
                "déjà vu.ts",
                "tab\there.ts",
                "q\"uote",
                "my file.ts",
                "é.ts"
            ]
        );
    }

    #[test]
    fn invalid_utf8_content_is_kept_lossily_and_bad_paths_are_flagged() {
        let mut diff =
            b"diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1 +1 @@\n-x\n+caf\xe9\n"
                .to_vec();
        diff.extend_from_slice(b"diff --git a/b\xff.ts b/b\xff.ts\n");
        diff.extend_from_slice(
            b"diff --git a/c.ts b/c.ts\n--- a/c.ts\n+++ b/c.ts\n@@ -1 +1 @@\n-y\n+z\n",
        );
        let files = parse_diff(&diff);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].hunks[0].lines[1], "+caf\u{fffd}");
        assert!(
            files[1].bad_path,
            "a non-UTF-8 path is skipped, not renamed"
        );
        assert_eq!(
            (files[2].path.as_str(), files[2].hunks[0].lines.len()),
            ("c.ts", 2)
        );
    }

    #[test]
    fn context_lines_advance_the_head_line_counter() {
        let h = hunk(1, 1, 6, &[" a", "-b", "+B", " c", "+d", " e", "-f", " g"]);
        let got = pieces(&h, None);
        assert_eq!(
            ranges(&got),
            vec![(2, 2, vec![2]), (4, 4, vec![4]), (5, 5, vec![])],
            "B is line 2, d is line 4, f was removed below e (line 5)"
        );
        assert_eq!(got[2].removed, vec![5]);
    }

    #[test]
    fn hunks_split_at_symbols_and_blank_lines_stay_put() {
        let src = "const a = 1\n\nconst b = 2\n";
        let parsed = symbols::parse("x.ts", src).expect("parses");
        let h = hunk(0, 1, 3, &["+const a = 1", "+", "+const b = 2"]);
        let got: Vec<_> = pieces(&h, parsed.as_ref())
            .iter()
            .map(|p| (p.sym.map(|s| s.name.clone()), p.start, p.end))
            .collect();
        assert_eq!(
            got,
            vec![(Some("a".into()), 1, 2), (Some("b".into()), 3, 3)]
        );
    }

    #[test]
    fn deletion_only_hunk_is_anchored() {
        let got = pieces(&hunk(8, 7, 0, &["-gone"]), None);
        assert_eq!(
            (got[0].start, got[0].end, got[0].removed.clone()),
            (7, 7, vec![8])
        );
    }

    #[test]
    fn lines_in_inline_test_modules_are_dropped() {
        let src = "fn prod() {}\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n";
        let parsed = symbols::parse("lib.rs", src).expect("parses");
        let h = hunk(
            1,
            1,
            5,
            &[
                "+fn prod() {}",
                "+#[cfg(test)]",
                "+mod tests {",
                "+    fn t() {}",
                "+}",
            ],
        );
        assert_eq!(ranges(&pieces(&h, parsed.as_ref())), vec![(1, 1, vec![1])]);
    }

    #[test]
    fn comment_only_uses_the_syntax_tree_on_both_sides() {
        let head = "fn f(balance: &mut i64) {\n    // new note\n    *balance += 1;\n}\n#[cfg(test)]\nmod m {}\n";
        let base = "fn f(balance: &mut i64) {\n    // old note\n}\n";
        let (hp, bp) = (
            symbols::parse("a.rs", head).expect("ok"),
            symbols::parse("a.rs", base).expect("ok"),
        );
        let (hl, bl): (Vec<&str>, Vec<&str>) = (head.lines().collect(), base.lines().collect());
        let piece = |added: Vec<u32>, removed: Vec<u32>| Piece {
            sym: None,
            start: 1,
            end: 1,
            diff: Vec::new(),
            added,
            removed,
        };
        let only = |p: Piece<'_>| comment_only(hp.as_ref(), &hl, bp.as_ref(), &bl, &p);
        assert!(only(piece(vec![2], vec![2])), "comment replaced by comment");
        assert!(!only(piece(vec![3], vec![])), "*balance += 1 is code");
        assert!(!only(piece(vec![5], vec![])), "#[cfg(test)] is code");
        assert!(
            !only(piece(vec![2], vec![1])),
            "removing code is not comment-only"
        );
    }

    /// A repo whose config tries every setting that changes diff output.
    #[test]
    fn changes_ignore_hostile_git_config() {
        let dir = std::env::temp_dir().join(format!("overview-me-hostile-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let sh = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .output()
                .expect("git runs")
                .status
                .success();
            assert!(ok, "git {args:?}");
        };
        sh(&["init", "-q"]);
        for (k, v) in [
            ("user.email", "t@t"),
            ("user.name", "t"),
            ("commit.gpgsign", "false"),
            ("diff.noprefix", "true"),
            ("diff.mnemonicPrefix", "true"),
            ("diff.interHunkContext", "10"),
            ("core.quotePath", "true"),
            ("color.ui", "always"),
            ("diff.external", "false"),
            ("diff.relative", "true"),
        ] {
            sh(&["config", k, v]);
        }
        let file = dir.join("é x.ts");
        let body: Vec<String> = (1..=12).map(|i| format!("const v{i} = {i}")).collect();
        std::fs::write(&file, body.join("\n") + "\n").expect("write");
        std::fs::write(dir.join("gone.ts"), "const g = 1\n").expect("write");
        std::fs::create_dir_all(dir.join("sub")).expect("sub dir");
        std::fs::write(dir.join("sub/keep.ts"), "const k = 1\n").expect("write");
        sh(&["add", "-A"]);
        sh(&["commit", "-q", "-m", "base"]);
        let mut changed = body.clone();
        changed[2] = "const v3 = 30".into();
        changed[6] = "const v7 = 70".into();
        std::fs::write(&file, changed.join("\n") + "\n").expect("write");
        std::fs::write(dir.join("bin.ts"), b"const s = 'caf\xe9'\n").expect("write");
        std::fs::remove_file(dir.join("gone.ts")).expect("rm");
        sh(&["add", "-A"]);
        sh(&["commit", "-q", "-m", "head"]);

        let rules = Rules::load(None).expect("rules");
        // Run from a subdirectory: with diff.relative, git would hide files outside it.
        let got = changes(&Git::new(&dir.join("sub")), &rules, "HEAD~1", "HEAD").expect("changes");
        let mut units: Vec<(String, u32, u32)> = got
            .units
            .iter()
            .map(|u| (u.file.clone(), u.start, u.end))
            .collect();
        units.sort();
        assert_eq!(
            units,
            vec![
                ("bin.ts".into(), 1, 1),
                ("é x.ts".into(), 3, 3),
                ("é x.ts".into(), 7, 7)
            ]
        );
        assert_eq!(got.deleted, vec!["gone.ts"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn submodule_pointer_changes_are_skipped() {
        let repo = crate::testutil::TempRepo::new("gitlink");
        repo.write("a.ts", "const a = 1\n");
        let base = repo.commit("base");
        repo.git(&[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{base},vendor/lib"),
        ]);
        repo.git(&["commit", "-q", "-m", "add submodule"]);
        let mid = repo.git(&["rev-parse", "HEAD"]);
        repo.git(&[
            "update-index",
            "--cacheinfo",
            &format!("160000,{mid},vendor/lib"),
        ]);
        repo.write("a.ts", "const a = 2\n");
        repo.git(&["add", "a.ts"]);
        repo.git(&["commit", "-q", "-m", "bump submodule"]);
        let rules = Rules::load(None).expect("rules");
        let got = changes(&Git::new(repo.path()), &rules, &base, "HEAD")
            .expect("a gitlink must not abort");
        let skipped: Vec<(&str, &str)> = got
            .skipped
            .iter()
            .map(|s| (s.file.as_str(), s.reason))
            .collect();
        assert_eq!(skipped, vec![("vendor/lib", "submodule")]);
        assert_eq!(got.units.len(), 1, "other files still count");
    }

    #[test]
    fn pure_moves_and_moves_into_test_paths_are_listed() {
        let repo = crate::testutil::TempRepo::new("renames");
        repo.write(
            "src/a.ts",
            "export const a = 1\nexport const b = 2\nexport const c = 3\n",
        );
        repo.write(
            "src/x.ts",
            "export const x = 1\nexport const y = 2\nexport const z = 3\n",
        );
        repo.write(
            "src/m.ts",
            "export const m = 1\nexport const n = 2\nexport const o = 3\nexport const p = 4\n",
        );
        let base = repo.commit("base");
        repo.git(&["mv", "src/a.ts", "src/b.ts"]);
        repo.git(&["mv", "src/x.ts", "src/x.test.ts"]);
        repo.git(&["mv", "src/m.ts", "src/n.ts"]);
        repo.write(
            "src/n.ts",
            "export const m = 1\nexport const n = 2\nexport const o = 3\nexport const p = 5\n",
        );
        repo.commit("moves");
        let rules = Rules::load(None).expect("rules");
        let got = changes(&Git::new(repo.path()), &rules, &base, "HEAD").expect("changes");
        let mut moves: Vec<(&str, &str)> = got
            .renames
            .iter()
            .map(|r| (r.from.as_str(), r.to.as_str()))
            .collect();
        moves.sort_unstable();
        assert_eq!(
            moves,
            vec![("src/a.ts", "src/b.ts"), ("src/x.ts", "src/x.test.ts")],
            "an edited move shows up as hunks, so it is not listed"
        );
    }

    #[test]
    fn code_moved_out_of_a_test_path_is_listed_and_fully_new() {
        let repo = crate::testutil::TempRepo::new("test-to-prod");
        let body: Vec<String> = (1..=20)
            .map(|i| format!("export const h{i} = {i}"))
            .collect();
        repo.write("src/__tests__/helper.ts", &(body.join("\n") + "\n"));
        repo.write("src/keep.ts", "export const k = 1\n");
        let base = repo.commit("base");
        std::fs::create_dir_all(repo.path().join("src/lib")).expect("lib dir");
        repo.git(&["mv", "src/__tests__/helper.ts", "src/lib/helper.ts"]);
        let mut edited = body.clone();
        edited[0] = "export const h1 = 100".into();
        repo.write("src/lib/helper.ts", &(edited.join("\n") + "\n"));
        repo.commit("move");
        let rules = Rules::load(None).expect("rules");
        let got = changes(&Git::new(repo.path()), &rules, &base, "HEAD").expect("changes");
        let moves: Vec<(&str, &str)> = got
            .renames
            .iter()
            .map(|r| (r.from.as_str(), r.to.as_str()))
            .collect();
        assert_eq!(
            moves,
            vec![("src/__tests__/helper.ts", "src/lib/helper.ts")]
        );
        let mut added: Vec<u32> = got
            .units
            .iter()
            .filter(|u| u.file == "src/lib/helper.ts")
            .flat_map(|u| u.added.clone())
            .collect();
        added.sort_unstable();
        assert_eq!(
            added,
            (1..=20).collect::<Vec<u32>>(),
            "every line of the moved file needs coverage"
        );
    }
}
