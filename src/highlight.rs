//! Syntax colors for embedded code previews, from tree-sitter highlight queries.
//!
//! Each line becomes a flat list of `[column, length, class]` triples, where columns and
//! lengths count UTF-16 code units (as JavaScript strings do) and `class` is one letter that
//! `assets/styles/code.css` colors as `.tok-<class>`. Files in other languages keep highlight.js.

use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::Language;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use crate::symbols::Lang;

/// Capture names the queries use, and the class each one draws with. A capture also matches
/// its dotted children: `function` covers `function.method` and `function.macro`.
const CLASSES: &[(&str, &str)] = &[
    ("keyword", "k"),
    ("variable.builtin", "k"),
    ("string", "s"),
    ("tag", "s"),
    ("comment", "c"),
    ("number", "n"),
    ("constant", "n"),
    ("boolean", "n"),
    ("escape", "n"),
    ("label", "n"),
    ("type", "t"),
    ("constructor", "t"),
    ("function", "f"),
    ("attribute", "a"),
    ("string.special.key", "p"),
    // Keys in data files only: code leaves field names plain, as GitHub does.
    ("property", "p"),
];

/// The JSON grammar's own query captures every `(string)` as `@string` ahead of a pair's key,
/// so keys would read as values; this one tells them apart.
const JSON_HIGHLIGHTS: &str = r#"
(pair key: (string) @string.special.key)
(pair value: (string) @string)
(array (string) @string)
(document (string) @string)
(number) @number
[(null) (true) (false)] @constant.builtin
(escape_sequence) @escape
(comment) @comment
"#;

/// The TOML grammar's own query captures a whole `key = value` pair as `@property`; this one
/// colors keys, and table headers as types.
const TOML_HIGHLIGHTS: &str = r#"
(table [(bare_key) (quoted_key)] @type)
(table (dotted_key [(bare_key) (quoted_key)] @type))
(table_array_element [(bare_key) (quoted_key)] @type)
(table_array_element (dotted_key [(bare_key) (quoted_key)] @type))
(pair [(bare_key) (quoted_key)] @property)
(pair (dotted_key [(bare_key) (quoted_key)] @property))
(boolean) @boolean
(comment) @comment
(string) @string
[(integer) (float)] @number
[(offset_date_time) (local_date_time) (local_date) (local_time)] @string.special
"#;

/// A file kind with a tree-sitter grammar: one the symbol index reads, or a data or script format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Grammar {
    Code(Lang),
    Bash,
    Java,
    Json,
    Toml,
    Yaml,
}

impl Grammar {
    fn of(path: &str) -> Option<Self> {
        if let Some(lang) = Lang::of(path) {
            return Some(Self::Code(lang));
        }
        let file = path.rsplit('/').next().unwrap_or(path);
        match file.rsplit_once('.')?.1 {
            "sh" | "bash" => Some(Self::Bash),
            "java" => Some(Self::Java),
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "yaml" | "yml" => Some(Self::Yaml),
            _ => None,
        }
    }
}

/// Highlights files, building each language's queries once.
pub struct Highlights {
    highlighter: Highlighter,
    configs: HashMap<Grammar, Option<HighlightConfiguration>>,
}

impl Highlights {
    pub fn new() -> Self {
        Self {
            highlighter: Highlighter::new(),
            configs: HashMap::new(),
        }
    }

    /// The classed spans of each line of `text` (as `str::lines` splits it), or `None` when
    /// tree-sitter has no grammar for `path` or fails on it.
    pub fn lines(&mut self, path: &str, text: &str) -> Option<Vec<Value>> {
        let grammar = Grammar::of(path)?;
        let config = self
            .configs
            .entry(grammar)
            .or_insert_with(|| config(grammar))
            .as_ref()?;
        let events = self
            .highlighter
            .highlight(config, text.as_bytes(), None, None, |_| None)
            .ok()?;
        let mut lines = vec![Vec::new(); text.lines().count()];
        let (mut line, mut col) = (0, 0);
        let mut open: Vec<&str> = Vec::new();
        for event in events {
            match event.ok()? {
                HighlightEvent::HighlightStart(h) => open.push(CLASSES[h.0].1),
                HighlightEvent::HighlightEnd => {
                    open.pop();
                }
                HighlightEvent::Source { start, end } => {
                    let class = open.last().copied();
                    let mut from = col;
                    for c in text[start..end].chars() {
                        if c == '\n' {
                            push(&mut lines, line, from, col, class);
                            (line, col, from) = (line + 1, 0, 0);
                        } else {
                            col += c.len_utf16();
                        }
                    }
                    push(&mut lines, line, from, col, class);
                }
            }
        }
        Some(lines.into_iter().map(Value::from).collect())
    }
}

fn push(lines: &mut [Vec<Value>], line: usize, from: usize, to: usize, class: Option<&str>) {
    if let (Some(class), Some(spans)) = (class, lines.get_mut(line))
        && to > from
    {
        spans.extend([from.into(), (to - from).into(), class.into()]);
    }
}

fn config(grammar: Grammar) -> Option<HighlightConfiguration> {
    let lang = match grammar {
        Grammar::Code(lang) => lang,
        Grammar::Bash => {
            return data(
                tree_sitter_bash::LANGUAGE.into(),
                tree_sitter_bash::HIGHLIGHT_QUERY,
                false,
            );
        }
        Grammar::Java => {
            return data(
                tree_sitter_java::LANGUAGE.into(),
                tree_sitter_java::HIGHLIGHTS_QUERY,
                false,
            );
        }
        Grammar::Json => {
            return data(tree_sitter_json::LANGUAGE.into(), JSON_HIGHLIGHTS, true);
        }
        Grammar::Toml => {
            return data(tree_sitter_toml_ng::LANGUAGE.into(), TOML_HIGHLIGHTS, true);
        }
        Grammar::Yaml => {
            return data(
                tree_sitter_yaml::LANGUAGE.into(),
                tree_sitter_yaml::HIGHLIGHTS_QUERY,
                true,
            );
        }
    };
    // TypeScript's queries add to JavaScript's, and TSX adds JSX's; the more specific come first.
    let (highlights, locals) = match lang {
        Lang::TypeScript => (
            [
                tree_sitter_typescript::HIGHLIGHTS_QUERY,
                tree_sitter_javascript::HIGHLIGHT_QUERY,
            ]
            .join("\n"),
            [
                tree_sitter_typescript::LOCALS_QUERY,
                tree_sitter_javascript::LOCALS_QUERY,
            ]
            .join("\n"),
        ),
        Lang::Tsx => (
            [
                tree_sitter_typescript::HIGHLIGHTS_QUERY,
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
                tree_sitter_javascript::HIGHLIGHT_QUERY,
            ]
            .join("\n"),
            [
                tree_sitter_typescript::LOCALS_QUERY,
                tree_sitter_javascript::LOCALS_QUERY,
            ]
            .join("\n"),
        ),
        Lang::Rust => (tree_sitter_rust::HIGHLIGHTS_QUERY.to_owned(), String::new()),
        Lang::Python => (
            tree_sitter_python::HIGHLIGHTS_QUERY.to_owned(),
            String::new(),
        ),
        Lang::Go => (tree_sitter_go::HIGHLIGHTS_QUERY.to_owned(), String::new()),
    };
    configured(lang.language(), &highlights, &locals, false)
}

fn data(language: Language, highlights: &str, keys: bool) -> Option<HighlightConfiguration> {
    configured(language, highlights, "", keys)
}

/// `keys` colors `property` captures, which `CLASSES` lists last.
fn configured(
    language: Language,
    highlights: &str,
    locals: &str,
    keys: bool,
) -> Option<HighlightConfiguration> {
    let mut config = HighlightConfiguration::new(language, "", highlights, "", locals).ok()?;
    let names = &CLASSES[..CLASSES.len() - usize::from(!keys)];
    config.configure(&names.iter().map(|(name, _)| *name).collect::<Vec<_>>());
    Some(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes(path: &str, text: &str) -> Vec<Vec<(String, &'static str)>> {
        let lines: Vec<_> = text.lines().collect();
        Highlights::new()
            .lines(path, text)
            .expect("highlighted")
            .iter()
            .zip(&lines)
            .map(|(spans, line)| {
                let units: Vec<u16> = line.encode_utf16().collect();
                spans
                    .as_array()
                    .expect("spans")
                    .chunks(3)
                    .map(|s| {
                        let (from, len) = (
                            s[0].as_u64().expect("col") as usize,
                            s[1].as_u64().expect("len") as usize,
                        );
                        let class = CLASSES
                            .iter()
                            .find(|(_, c)| *c == s[2].as_str().expect("class"))
                            .expect("known class")
                            .1;
                        (String::from_utf16_lossy(&units[from..from + len]), class)
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn rust_names_in_a_use_list_take_their_conventions() {
        let lines = classes(
            "app.rs",
            "use topcoat::{\n    Result,\n    asset::{Asset, asset_config},\n};\nfn main() { view!(x); let é = \"s\"; }\n",
        );
        assert_eq!(lines[0], [("use".to_owned(), "k")]);
        assert_eq!(lines[1], [("Result".to_owned(), "t")]);
        assert_eq!(lines[2], [("Asset".to_owned(), "t")]);
        assert!(lines[4].contains(&("main".to_owned(), "f")));
        assert!(
            lines[4].contains(&("view!".to_owned(), "f"))
                || lines[4].contains(&("view".to_owned(), "f"))
        );
        assert!(lines[4].contains(&("\"s\"".to_owned(), "s")));
    }

    #[test]
    fn every_language_configures() {
        for (path, text) in [
            ("a.ts", "const x: Foo = bar(1); // c\n"),
            ("a.tsx", "const x = <Foo a=\"b\" />;\n"),
            ("a.js", "function f() { return 'a'; }\n"),
            ("a.py", "def f(x):\n    return \"a\"\n"),
            ("a.go", "package main\nfunc f() int { return 1 }\n"),
            ("a.sh", "f() { echo \"$1\"; }\n"),
            ("A.java", "class A { void f() { g(1); } }\n"),
        ] {
            let lines = classes(path, text);
            assert!(lines.iter().any(|l| !l.is_empty()), "{path}: {lines:?}");
        }
        assert!(Highlights::new().lines("a.txt", "a = 1\n").is_none());
    }

    #[test]
    fn data_files_color_keys_apart_from_values() {
        let p = |s: &str| (s.to_owned(), "p");
        assert_eq!(
            classes("a.json", "{\"a\": \"b\"}\n")[0],
            [p("\"a\""), ("\"b\"".to_owned(), "s")]
        );
        assert_eq!(
            classes("Cargo.toml", "[package]\nname = \"x\"\n")[0],
            [("package".to_owned(), "t")]
        );
        assert_eq!(
            classes("Cargo.toml", "[package]\nname = \"x\"\n")[1],
            [p("name"), ("\"x\"".to_owned(), "s")]
        );
        assert_eq!(
            classes(".github/ci.yml", "on: true\n")[0],
            [p("on"), ("true".to_owned(), "n")]
        );
        // Field names in code stay plain.
        assert!(!classes("a.rs", "fn f(s: S) { s.field; }\n")[0].contains(&p("field")));
    }

    #[test]
    fn spans_split_at_line_ends_and_count_utf16() {
        let lines = classes("a.rs", "/* é\n🙂 */ let\n");
        assert_eq!(lines[0], [("/* é".to_owned(), "c")]);
        assert_eq!(
            lines[1],
            [("🙂 */".to_owned(), "c"), ("let".to_owned(), "k")]
        );
    }
}
