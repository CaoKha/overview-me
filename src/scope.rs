//! Which declaration a name in a file points to. Two symbols can share a name: a reference is
//! linked to a symbol only when the name's binding can be that symbol.
//!
//! TypeScript and JavaScript imports are followed: a relative import to its file (through
//! `export … from` re-exports), a bare one to a workspace package or a third-party dependency.
//! Other languages, and imports that cannot be followed (path aliases), keep the name rule: the
//! file imports a name equal to the symbol's.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use anyhow::Result;
use serde_json::Value;

use crate::git::Git;
use crate::symbols::{self, Ident, Parsed, Role, Symbol};

/// `export … from` hops followed from an import to the declaration.
const MAX_HOPS: usize = 4;
/// Extensions an import specifier can leave out or write as `.js`.
const EXTENSIONS: [&str; 9] = [
    ".ts", ".tsx", ".mts", ".cts", ".d.ts", ".js", ".jsx", ".mjs", ".cjs",
];

/// What a name in a file is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    /// Declared at the top level of the file itself.
    Local,
    /// Imported from this file of the repo.
    File(String),
    /// Imported from the workspace package in this folder (`""` for the repo root).
    Package(String),
    /// Imported from a third-party package or a non-code file: never a symbol of the repo.
    External,
    /// Imported, from a source that could not be followed.
    Unresolved,
    /// Neither declared at the top level nor imported: a local, a parameter or a global.
    Free,
}

type Loader<'a> = Box<dyn Fn(&str) -> Option<Parsed> + 'a>;

/// The files of one revision, its workspace packages and dependencies, and the bindings
/// resolved so far.
pub struct Scope<'a> {
    tree: HashSet<String>,
    /// Workspace package name → its folder.
    packages: HashMap<String, String>,
    /// Package names listed as dependencies by any `package.json`.
    dependencies: HashSet<String>,
    load: Loader<'a>,
    parsed: RefCell<HashMap<String, Option<Rc<Parsed>>>>,
    bindings: RefCell<HashMap<(String, String), Binding>>,
}

/// The type a method is reached through: `Git` for `Git::new`, `C` for `C.m`; `None` for a
/// top-level symbol.
pub fn container<'n>(name: &'n str, ident: &str) -> Option<&'n str> {
    let rest = name.strip_suffix(ident)?.trim_end_matches([':', '.']);
    (!rest.is_empty()).then(|| rest.rsplit([':', '.']).next().unwrap_or(rest))
}

/// Whether `path` is in the folder `dir` (`""` is the repo root).
fn under(path: &str, dir: &str) -> bool {
    dir.is_empty()
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// A top-level declaration of `name` in `p`.
fn declares(p: &Parsed, name: &str) -> bool {
    p.symbols.iter().any(|s| s.ident == name && s.name == name)
}

/// The package a bare specifier names: `@scope/pkg` for `@scope/pkg/sub`, `pkg` for `pkg/sub`.
fn package_name(spec: &str) -> &str {
    let mut ends = spec.match_indices('/').map(|(i, _)| i);
    let end = if spec.starts_with('@') {
        ends.nth(1)
    } else {
        ends.next()
    };
    end.map_or(spec, |i| &spec[..i])
}

/// `dir/spec` with `.` and `..` folded; `None` when it leaves the repo.
fn join(dir: &str, spec: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for seg in spec.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

impl<'a> Scope<'a> {
    /// The scope of `rev`: its files and `package.json` files.
    ///
    /// # Errors
    /// When git fails to list the tree.
    pub fn load(git: &'a Git, rev: &str) -> Result<Self> {
        let listed = git.run(&["ls-tree", "-r", "--name-only", "--end-of-options", rev])?;
        let tree: HashSet<String> = listed.lines().map(str::to_owned).collect();
        let mut paths: Vec<&str> = tree
            .iter()
            .map(String::as_str)
            .filter(|p| {
                (*p == "package.json" || p.ends_with("/package.json"))
                    && !p.contains("node_modules/")
            })
            .collect();
        // Sorted: when two manifests share a name, the first one wins every run.
        paths.sort_unstable();
        let texts = git.read_files(rev, &paths)?;
        let manifests: Vec<(String, String)> = paths
            .iter()
            .zip(texts)
            .filter_map(|(path, text)| {
                let dir = path.strip_suffix("package.json").unwrap_or_default();
                Some((dir.trim_end_matches('/').to_owned(), text?))
            })
            .collect();
        let rev = rev.to_owned();
        let load: Loader<'a> = Box::new(move |path: &str| {
            let src = git
                .run(&["show", "--end-of-options", &format!("{rev}:{path}")])
                .ok()?;
            symbols::parse(path, &src).ok().flatten()
        });
        Ok(Self::new(tree, &manifests, load))
    }

    /// A scope over `tree`, with `manifests` as (folder, `package.json` text), loading parsed
    /// files with `load`.
    fn new(tree: HashSet<String>, manifests: &[(String, String)], load: Loader<'a>) -> Self {
        let mut packages = HashMap::new();
        let mut dependencies = HashSet::new();
        for (dir, text) in manifests {
            let Ok(json) = serde_json::from_str::<Value>(text) else {
                continue;
            };
            if let Some(name) = json.get("name").and_then(Value::as_str) {
                packages
                    .entry(name.to_owned())
                    .or_insert_with(|| dir.clone());
            }
            for field in [
                "dependencies",
                "devDependencies",
                "peerDependencies",
                "optionalDependencies",
            ] {
                if let Some(deps) = json.get(field).and_then(Value::as_object) {
                    dependencies.extend(deps.keys().cloned());
                }
            }
        }
        Self {
            tree,
            packages,
            dependencies,
            load,
            parsed: RefCell::default(),
            bindings: RefCell::default(),
        }
    }

    fn parsed(&self, path: &str) -> Option<Rc<Parsed>> {
        if let Some(p) = self.parsed.borrow().get(path) {
            return p.clone();
        }
        let p = (self.load)(path).map(Rc::new);
        self.parsed.borrow_mut().insert(path.to_owned(), p.clone());
        p
    }

    /// The repo file a relative specifier in `from` points to, with TypeScript's rules: `.js`
    /// stands for `.ts`, the extension can be left out, and a folder means its `index` file.
    fn resolve_path(&self, from: &str, spec: &str) -> Option<String> {
        let dir = from.rsplit_once('/').map_or("", |(d, _)| d);
        let joined = join(dir, spec)?;
        let mut candidates = vec![joined.clone()];
        for (js, ts) in [
            (".js", &[".ts", ".tsx"][..]),
            (".jsx", &[".tsx"][..]),
            (".mjs", &[".mts"][..]),
            (".cjs", &[".cts"][..]),
        ] {
            if let Some(stem) = joined.strip_suffix(js) {
                candidates.extend(ts.iter().map(|ext| format!("{stem}{ext}")));
            }
        }
        candidates.extend(EXTENSIONS.iter().map(|ext| format!("{joined}{ext}")));
        candidates.extend(EXTENSIONS.iter().map(|ext| format!("{joined}/index{ext}")));
        candidates.into_iter().find(|c| self.tree.contains(c))
    }

    /// What `name`, used in `file` (parsed as `p`), is bound to.
    pub fn binding(&self, file: &str, p: &Parsed, name: &str) -> Binding {
        let key = (file.to_owned(), name.to_owned());
        if let Some(b) = self.bindings.borrow().get(&key) {
            return b.clone();
        }
        let b = if !symbols::tracks_imports(file) {
            if p.imports.contains(name) {
                Binding::Unresolved
            } else {
                Binding::Free
            }
        } else if declares(p, name) {
            Binding::Local
        } else if let Some(import) = p.import_from.get(name) {
            self.follow(file, &import.module, import.name.as_deref(), 0)
        } else if p.imports.contains(name) {
            Binding::Unresolved
        } else {
            Binding::Free
        };
        self.bindings.borrow_mut().insert(key, b.clone());
        b
    }

    /// Where `name` (`None`: the whole module) imported from `spec` in `from` is declared.
    fn follow(&self, from: &str, spec: &str, name: Option<&str>, hops: usize) -> Binding {
        if spec.starts_with('.') {
            return match self.resolve_path(from, spec) {
                Some(target) if symbols::is_code(&target) => self.through(target, name, hops),
                Some(_) => Binding::External,
                None => Binding::Unresolved,
            };
        }
        let package = package_name(spec);
        if let Some(dir) = self.packages.get(package) {
            Binding::Package(dir.clone())
        } else if self.dependencies.contains(package) || spec.starts_with("node:") {
            Binding::External
        } else {
            Binding::Unresolved
        }
    }

    /// Follows `name` from the file `target` through its re-exports to its declaration.
    fn through(&self, target: String, name: Option<&str>, hops: usize) -> Binding {
        let Some(name) = name.filter(|n| *n != "default") else {
            return Binding::File(target);
        };
        if hops >= MAX_HOPS {
            return Binding::File(target);
        }
        let Some(p) = self.parsed(&target) else {
            return Binding::File(target);
        };
        if declares(&p, name) {
            return Binding::File(target);
        }
        if let Some(r) = p
            .reexports
            .iter()
            .find(|r| r.exported.as_deref() == Some(name))
        {
            return self.follow(&target, &r.module, r.name.as_deref(), hops + 1);
        }
        // `import { x } from './a'` then `export { x }`.
        if let Some(import) = p.import_from.get(name) {
            return self.follow(&target, &import.module, import.name.as_deref(), hops + 1);
        }
        for r in p.reexports.iter().filter(|r| r.exported.is_none()) {
            if let Binding::File(f) = self.follow(&target, &r.module, Some(name), hops + 1)
                && self.parsed(&f).is_some_and(|q| declares(&q, name))
            {
                return Binding::File(f);
            }
        }
        Binding::File(target)
    }

    /// Whether the reference `r` in `caller` (parsed as `p`) can name `sym`, declared in `callee`.
    pub fn reaches(&self, caller: &str, p: &Parsed, r: &Ident, callee: &str, sym: &Symbol) -> bool {
        let method = container(&sym.name, &sym.ident);
        // `Foo::new` brings `new` into scope too: a method also needs its type.
        let type_in_scope = || {
            method
                .is_none_or(|c| p.imports.contains(c) || p.identifiers.iter().any(|i| i.name == c))
        };
        // The name rule: the same file or Go package, or an import of an equal name.
        let by_name = || {
            caller == callee
                || symbols::same_go_package(caller, callee)
                || (sym.exported && p.imports.contains(&sym.ident) && type_in_scope())
        };
        let in_module = |b: Binding| match b {
            Binding::File(f) => f == callee && type_in_scope(),
            Binding::Package(dir) => under(callee, &dir) && sym.exported && type_in_scope(),
            Binding::Unresolved => by_name(),
            Binding::Local | Binding::External | Binding::Free => false,
        };
        match &r.role {
            Role::Key => false,
            // `x.m()` on a value: the type check decides, as for any method.
            Role::Member(_) if method.is_some() => by_name(),
            // `q.name` reaches a top-level symbol only through a namespace import of its module.
            Role::Member(q) => in_module(self.binding(caller, p, q)),
            Role::Name => {
                if caller == callee || symbols::same_go_package(caller, callee) {
                    return true;
                }
                match self.binding(caller, p, &r.name) {
                    Binding::Free => by_name(),
                    b => in_module(b),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope<'a>(files: &'a [(&'a str, &'a str)], manifests: &[(&str, &str)]) -> Scope<'a> {
        let tree = files.iter().map(|(p, _)| (*p).to_owned()).collect();
        let manifests: Vec<(String, String)> = manifests
            .iter()
            .map(|(d, t)| ((*d).to_owned(), (*t).to_owned()))
            .collect();
        Scope::new(
            tree,
            &manifests,
            Box::new(move |path: &str| {
                let (_, src) = files.iter().find(|(p, _)| *p == path)?;
                symbols::parse(path, src).ok().flatten()
            }),
        )
    }

    fn bind(s: &Scope<'_>, files: &[(&str, &str)], file: &str, name: &str) -> Binding {
        let (_, src) = files.iter().find(|(p, _)| *p == file).expect("file");
        let p = symbols::parse(file, src).expect("parses").expect("code");
        s.binding(file, &p, name)
    }

    #[test]
    fn relative_imports_resolve_like_typescript() {
        let files = [
            ("src/a.ts", "export const a = 1\n"),
            ("src/b.tsx", "export const b = 1\n"),
            ("src/lib/index.ts", "export const lib = 1\n"),
            ("src/data.json", "{}"),
            (
                "src/use.ts",
                "import { a } from './a.js'\nimport { b } from './b'\nimport { lib } from './lib'\nimport data from './data.json'\nimport { gone } from './gone'\nexport const own = 1\n",
            ),
        ];
        let s = scope(&files, &[]);
        let b = |n: &str| bind(&s, &files, "src/use.ts", n);
        assert_eq!(b("a"), Binding::File("src/a.ts".into()));
        assert_eq!(b("b"), Binding::File("src/b.tsx".into()));
        assert_eq!(b("lib"), Binding::File("src/lib/index.ts".into()));
        assert_eq!(b("data"), Binding::External);
        assert_eq!(b("gone"), Binding::Unresolved);
        assert_eq!(b("own"), Binding::Local);
        assert_eq!(b("nothing"), Binding::Free);
    }

    #[test]
    fn re_exports_are_followed_to_the_declaration() {
        let files = [
            ("src/r/referral.ts", "export const referral = {}\n"),
            ("src/r/other.ts", "export const other = 1\n"),
            (
                "src/r/index.ts",
                "export { referral as ref } from './referral.js'\nexport * from './other.js'\n",
            ),
            (
                "src/use.ts",
                "import { ref, other } from './r/index.js'\nimport * as r from './r'\n",
            ),
        ];
        let s = scope(&files, &[]);
        let b = |n: &str| bind(&s, &files, "src/use.ts", n);
        assert_eq!(b("ref"), Binding::File("src/r/referral.ts".into()));
        assert_eq!(b("other"), Binding::File("src/r/other.ts".into()));
        assert_eq!(b("r"), Binding::File("src/r/index.ts".into()));
    }

    #[test]
    fn bare_imports_are_workspace_packages_or_dependencies() {
        let files = [(
            "apps/api/src/use.ts",
            "import { table } from '@t/db/schema'\nimport { eq } from 'drizzle-orm'\nimport fs from 'node:fs'\nimport { x } from '~/alias'\n",
        )];
        let manifests = [
            ("packages/db", r#"{"name":"@t/db"}"#),
            (
                "apps/api",
                r#"{"name":"@t/api","dependencies":{"@t/db":"workspace:*","drizzle-orm":"1"}}"#,
            ),
        ];
        let s = scope(&files, &manifests);
        let b = |n: &str| bind(&s, &files, "apps/api/src/use.ts", n);
        assert_eq!(b("table"), Binding::Package("packages/db".into()));
        assert_eq!(b("eq"), Binding::External);
        assert_eq!(b("fs"), Binding::External);
        assert_eq!(b("x"), Binding::Unresolved);
    }

    #[test]
    fn helpers() {
        assert_eq!(package_name("@s/p/sub"), "@s/p");
        assert_eq!(package_name("pkg/sub"), "pkg");
        assert_eq!(package_name("pkg"), "pkg");
        assert_eq!(join("a/b", "../c/./d"), Some("a/c/d".into()));
        assert_eq!(join("a", "../../x"), None);
        assert!(under("packages/db/a.ts", "packages/db"));
        assert!(!under("packages/dbx/a.ts", "packages/db"));
        assert!(under("a.ts", ""));
    }

    #[test]
    fn methods_need_their_type_in_scope() {
        assert_eq!(container("Git::new", "new"), Some("Git"));
        assert_eq!(container("a::B::c", "c"), Some("B"));
        assert_eq!(container("C.m", "m"), Some("C"));
        assert_eq!(container("fee", "fee"), None);
    }
}
