//! Declarations, imports and identifiers of a source file, from tree-sitter.

use std::collections::HashSet;

use anyhow::{Context, Result};
use tree_sitter::{Language, Node, Parser};

/// A declaration that can enclose a hunk. Lines are 1-based and inclusive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// Display name, qualified by its container (`Class.method`, `Type::method`).
    pub name: String,
    /// The bare identifier, for name matching.
    pub ident: String,
    pub kind: &'static str,
    pub start: usize,
    pub end: usize,
    pub exported: bool,
}

#[derive(Debug, Default)]
pub struct Parsed {
    pub symbols: Vec<Symbol>,
    /// Line ranges of inline test modules (`mod tests`); hunks there are tests.
    pub test_ranges: Vec<(usize, usize)>,
    /// Names brought in by `import` / `use`.
    pub imports: HashSet<String>,
    /// Every identifier with its line.
    pub identifiers: Vec<(String, usize)>,
    /// Test declarations: `test(`/`it(`/`describe(` calls, or `#[test]` functions.
    pub tests: Vec<TestDecl>,
    /// Comment nodes as (start row, start byte, end row, end byte), 0-based, end exclusive.
    pub comments: Vec<(usize, usize, usize, usize)>,
}

/// A test declaration: its name and the line of `test(` or `#[test]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestDecl {
    pub name: String,
    pub line: usize,
}

impl Parsed {
    /// The innermost symbol that contains `line`.
    pub fn symbol_at(&self, line: usize) -> Option<&Symbol> {
        self.symbols
            .iter()
            .filter(|s| s.start <= line && line <= s.end)
            .min_by_key(|s| s.end - s.start)
    }

    /// Whether a line (1-based) holds only comment text: every non-blank byte lies inside a
    /// comment node. Blank lines count as comment-only.
    pub fn is_comment_line(&self, line: usize, text: &str) -> bool {
        let row = line.saturating_sub(1);
        let covered = |col: usize| {
            self.comments
                .iter()
                .any(|&(sr, sc, er, ec)| (sr, sc) <= (row, col) && (row, col) < (er, ec))
        };
        // Every non-blank byte must be comment text: `/* a */ run() /* b */` is code.
        text.char_indices()
            .filter(|(_, c)| !c.is_whitespace())
            .all(|(i, _)| covered(i))
    }

    pub fn in_tests(&self, line: usize) -> bool {
        self.test_ranges
            .iter()
            .any(|&(a, b)| a <= line && line <= b)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lang {
    TypeScript,
    Tsx,
    Rust,
}

impl Lang {
    fn of(path: &str) -> Option<Self> {
        let ext = path.rsplit_once('.').map(|(_, e)| e)?;
        match ext {
            "ts" | "mts" | "cts" | "js" | "mjs" | "cjs" => Some(Self::TypeScript),
            "tsx" | "jsx" => Some(Self::Tsx),
            "rs" => Some(Self::Rust),
            _ => None,
        }
    }

    fn language(self) -> Language {
        match self {
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
        }
    }

    fn separator(self) -> &'static str {
        if self == Self::Rust { "::" } else { "." }
    }
}

/// Whether `path` is in a language this tool can parse.
pub fn is_code(path: &str) -> bool {
    Lang::of(path).is_some()
}

/// Parses `source`; `None` for languages without a grammar here (they map to whole-file nodes).
///
/// # Errors
/// When the grammar cannot be loaded or tree-sitter returns no tree.
pub fn parse(path: &str, source: &str) -> Result<Option<Parsed>> {
    let Some(lang) = Lang::of(path) else {
        return Ok(None);
    };
    let mut parser = Parser::new();
    parser
        .set_language(&lang.language())
        .context("loading tree-sitter grammar")?;
    let tree = parser
        .parse(source, None)
        .with_context(|| format!("parsing {path}"))?;
    let src = source.as_bytes();
    let mut parsed = Parsed::default();
    collect_symbols(lang, tree.root_node(), src, "", &mut parsed);
    collect_identifiers(tree.root_node(), src, false, &mut parsed);
    collect_tests(lang, tree.root_node(), src, &mut parsed.tests);
    collect_comments(tree.root_node(), &mut parsed.comments);
    Ok(Some(parsed))
}

fn text(node: Node<'_>, src: &[u8]) -> String {
    node.utf8_text(src).unwrap_or_default().to_owned()
}

fn line(row: usize) -> usize {
    row + 1
}

/// A declaration's (name, kind, body to descend into).
fn declaration<'t>(
    lang: Lang,
    node: Node<'t>,
    src: &[u8],
) -> Option<(String, &'static str, Option<Node<'t>>)> {
    let name = |field: &str| node.child_by_field_name(field).map(|n| text(n, src));
    let body = node.child_by_field_name("body");
    let found = match (lang, node.kind()) {
        (Lang::Rust, "function_item" | "function_signature_item") => (name("name")?, "fn", None),
        (Lang::Rust, "struct_item" | "enum_item" | "union_item" | "type_item") => {
            (name("name")?, "type", None)
        }
        (Lang::Rust, "const_item" | "static_item") => (name("name")?, "const", None),
        (Lang::Rust, "macro_definition") => (name("name")?, "macro", None),
        (Lang::Rust, "trait_item") => (name("name")?, "trait", body),
        (Lang::Rust, "mod_item") => (name("name")?, "mod", body),
        (Lang::Rust, "impl_item") => (name("type")?, "impl", body),
        (Lang::Rust, _) => return None,
        (_, "function_declaration" | "generator_function_declaration") => {
            (name("name")?, "function", None)
        }
        (_, "class_declaration" | "abstract_class_declaration") => (name("name")?, "class", body),
        (_, "interface_declaration" | "type_alias_declaration" | "enum_declaration") => {
            (name("name")?, "type", None)
        }
        (_, "method_definition") => (name("name")?, "method", None),
        (_, "lexical_declaration" | "variable_declaration") => {
            let mut cursor = node.walk();
            let declarator = node
                .named_children(&mut cursor)
                .find(|c| c.kind() == "variable_declarator")?;
            (
                text(declarator.child_by_field_name("name")?, src),
                "const",
                None,
            )
        }
        _ => return None,
    };
    Some(found)
}

fn collect_symbols(lang: Lang, container: Node<'_>, src: &[u8], prefix: &str, out: &mut Parsed) {
    let mut cursor = container.walk();
    for child in container.named_children(&mut cursor) {
        let (decl, exported) = if child.kind() == "export_statement" {
            match child.child_by_field_name("declaration") {
                Some(d) => (d, true),
                None => continue,
            }
        } else {
            let mut c = child.walk();
            let public = child
                .children(&mut c)
                .any(|n| n.kind() == "visibility_modifier");
            (child, public)
        };
        let Some((ident, kind, body)) = declaration(lang, decl, src) else {
            continue;
        };
        let (start, end) = (
            line(child.start_position().row),
            line(child.end_position().row),
        );
        if lang == Lang::Rust && kind == "mod" && ident == "tests" {
            // The range starts at the first `#[cfg(test)]`-style attribute above the module.
            let mut first = child;
            while let Some(prev) = first
                .prev_named_sibling()
                .filter(|p| p.kind() == "attribute_item")
            {
                first = prev;
            }
            out.test_ranges
                .push((line(first.start_position().row), end));
            continue;
        }
        let name = format!("{prefix}{ident}");
        if let Some(body) = body {
            collect_symbols(lang, body, src, &format!("{name}{}", lang.separator()), out);
        }
        out.symbols.push(Symbol {
            name,
            ident,
            kind,
            start,
            end,
            exported,
        });
    }
}

const TEST_CALLS: [&str; 3] = ["test", "it", "describe"];

/// `#[test]`, `#[tokio::test]`, `#[rstest]`: the attribute path ends in `test`. `#[cfg(test)]` does not.
fn is_test_attribute(attr: &str) -> bool {
    let inner = attr.trim().trim_start_matches("#[").trim_end_matches(']');
    let path = inner
        .split(|c: char| c == '(' || c == '=' || c.is_whitespace())
        .next()
        .unwrap_or_default();
    path.ends_with("test")
}

fn member_root(member: Node<'_>) -> Option<Node<'_>> {
    member
        .child_by_field_name("object")
        .filter(|o| o.kind() == "identifier")
}

/// `test('name', …)`, `it.only('name', …)`, `describe('name', …)`, or a fn with a `#[…test]` attribute.
fn test_decl(lang: Lang, node: Node<'_>, src: &[u8]) -> Option<TestDecl> {
    if lang == Lang::Rust {
        if node.kind() != "function_item" {
            return None;
        }
        let mut attr = node.prev_named_sibling();
        while let Some(a) = attr.filter(|a| a.kind() == "attribute_item") {
            if is_test_attribute(&text(a, src)) {
                let name = text(node.child_by_field_name("name")?, src);
                return Some(TestDecl {
                    name,
                    line: line(a.start_position().row),
                });
            }
            attr = a.prev_named_sibling();
        }
        return None;
    }
    if node.kind() != "call_expression" {
        return None;
    }
    let callee = node.child_by_field_name("function")?;
    // `test(`, `it.only(`, or `test.for(cases)(` / `test.each(cases)(`: find the root identifier.
    let root = match callee.kind() {
        "identifier" => callee,
        "member_expression" => member_root(callee)?,
        "call_expression" => callee
            .child_by_field_name("function")
            .filter(|f| f.kind() == "member_expression")
            .and_then(member_root)?,
        _ => return None,
    };
    // `fooTest(` covers vitest fixtures made with `const fooTest = test.extend(…)`.
    let name = text(root, src);
    if !TEST_CALLS.contains(&name.as_str()) && !name.ends_with("Test") {
        return None;
    }
    let first = node.child_by_field_name("arguments")?.named_child(0)?;
    if !matches!(first.kind(), "string" | "template_string") {
        return None;
    }
    let raw = text(first, src);
    let name = raw
        .get(1..raw.len().saturating_sub(1))
        .unwrap_or_default()
        .to_owned();
    Some(TestDecl {
        name,
        line: line(node.start_position().row),
    })
}

fn collect_comments(node: Node<'_>, out: &mut Vec<(usize, usize, usize, usize)>) {
    if node.kind().ends_with("comment") {
        let (a, b) = (node.start_position(), node.end_position());
        out.push((a.row, a.column, b.row, b.column));
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_comments(child, out);
    }
}

fn collect_tests(lang: Lang, node: Node<'_>, src: &[u8], out: &mut Vec<TestDecl>) {
    if let Some(decl) = test_decl(lang, node, src) {
        out.push(decl);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_tests(lang, child, src, out);
    }
}

fn collect_identifiers(node: Node<'_>, src: &[u8], in_import: bool, out: &mut Parsed) {
    let in_import = in_import || matches!(node.kind(), "import_statement" | "use_declaration");
    if node.kind().ends_with("identifier") && node.named_child_count() == 0 {
        let name = text(node, src);
        if in_import {
            out.imports.insert(name.clone());
        }
        out.identifiers
            .push((name, line(node.start_position().row)));
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_identifiers(child, src, in_import, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(parsed: &Parsed) -> Vec<(&str, usize, usize, bool)> {
        let mut v: Vec<_> = parsed
            .symbols
            .iter()
            .map(|s| (s.name.as_str(), s.start, s.end, s.exported))
            .collect();
        v.sort_by_key(|s| s.1);
        v
    }

    #[test]
    fn typescript_top_level_declarations_and_methods() {
        let src = "import { a } from 'x'\n\nexport const f = (x: number) => {\n  return a(x)\n}\n\ntype T = {\n  a: 1\n}\n\nclass C {\n  m() {}\n}\n";
        let parsed = parse("x.ts", src)
            .expect("parses")
            .expect("typescript is supported");
        assert_eq!(
            names(&parsed),
            vec![
                ("f", 3, 5, true),
                ("T", 7, 9, false),
                ("C", 11, 13, false),
                ("C.m", 12, 12, false)
            ]
        );
        assert_eq!(
            parsed.symbol_at(12).map(|s| s.name.as_str()),
            Some("C.m"),
            "innermost wins"
        );
        assert!(parsed.imports.contains("a"));
    }

    #[test]
    fn rust_items_impl_methods_and_test_modules() {
        let src = "use std::fmt;\n\npub struct S;\n\nimpl S {\n    pub fn go(&self) {}\n}\n\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n";
        let parsed = parse("lib.rs", src)
            .expect("parses")
            .expect("rust is supported");
        assert_eq!(
            names(&parsed),
            vec![("S", 3, 3, true), ("S", 5, 7, false), ("S::go", 6, 6, true)]
        );
        assert!(parsed.in_tests(11));
        assert!(parsed.imports.contains("fmt"));
    }

    #[test]
    fn test_declarations_are_found() {
        let ts = "describe('outer', () => {\n  it.only(\"does x\", () => {})\n  test(`y`, () => {})\n  helper('z')\n  liquidationTest('w', () => {})\n  test.for([1])('v %i', () => {})\n})\n";
        let parsed = parse("a.test.ts", ts).expect("parses").expect("typescript");
        let got: Vec<_> = parsed
            .tests
            .iter()
            .map(|t| (t.name.as_str(), t.line))
            .collect();
        assert_eq!(
            got,
            vec![("outer", 1), ("does x", 2), ("y", 3), ("w", 5), ("v %i", 6)]
        );

        let rs = "mod tests {\n    #[test]\n    fn works() {}\n    fn helper() {}\n}\n";
        let parsed = parse("lib.rs", rs).expect("parses").expect("rust");
        let got: Vec<_> = parsed
            .tests
            .iter()
            .map(|t| (t.name.as_str(), t.line))
            .collect();
        assert_eq!(got, vec![("works", 2)]);
    }

    #[test]
    fn rust_test_attributes_need_a_test_path() {
        let rs = "#[cfg(test)]\nfn helper() {}\n#[tokio::test]\nasync fn a() {}\n#[rstest]\nfn b() {}\n#[test_case(1)]\nfn c() {}\n";
        let parsed = parse("lib.rs", rs).expect("parses").expect("rust");
        let got: Vec<_> = parsed.tests.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            got,
            vec!["a", "b"],
            "cfg(test) and test_case are not test attributes"
        );
    }

    #[test]
    fn inline_test_module_range_starts_at_its_attributes() {
        let rs =
            "fn prod() {}\n\n#[cfg(test)]\n#[allow(dead_code)]\nmod tests {\n    fn t() {}\n}\n";
        let parsed = parse("lib.rs", rs).expect("parses").expect("rust");
        assert_eq!(parsed.test_ranges, vec![(3, 7)]);
    }

    #[test]
    fn comment_lines_come_from_the_syntax_tree() {
        let rs = "fn f(balance: &mut i64) {\n    // note\n    *balance += 1; // trailing\n    /* a */ let x = 1;\n}\n#[cfg(test)]\nmod m {}\n";
        let parsed = parse("lib.rs", rs).expect("parses").expect("rust");
        let lines: Vec<&str> = rs.lines().collect();
        let got: Vec<bool> = (1..=lines.len())
            .map(|l| parsed.is_comment_line(l, lines[l - 1]))
            .collect();
        assert_eq!(
            got,
            vec![false, true, false, false, false, false, false],
            "{lines:?}"
        );
    }

    #[test]
    fn code_between_two_comments_is_code() {
        let ts = "/* a */ deleteAllUsers() /* b */\n/* a */ /* b */\n";
        let parsed = parse("x.ts", ts).expect("parses").expect("typescript");
        assert!(!parsed.is_comment_line(1, "/* a */ deleteAllUsers() /* b */"));
        assert!(parsed.is_comment_line(2, "/* a */ /* b */"));
    }

    #[test]
    fn unknown_languages_are_not_parsed() {
        assert!(parse("a.env", "X=1").expect("no error").is_none());
    }
}
