//! Folder-tree lanes and rules for tests, stores and hunk risk.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::map::Risk;
use crate::scope::under;

const DEFAULT_RULES: &str = include_str!("../rules.json");

/// Longest lane name, in characters.
pub const LANE_MAX: usize = 24;
/// Lanes a flow draws; more get merged.
pub const MAX_LANES: usize = 5;
/// Folders that say nothing about what the code does, skipped when naming a lane from a path.
const GENERIC_DIRS: [&str; 19] = [
    "src", "lib", "libs", "app", "apps", "internal", "pkg", "packages", "crates", "cmd", "source",
    "sources", "main", "java", "kotlin", "scala", "com", "org", "net",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Rules {
    test_globs: Vec<String>,
    store_globs: Vec<String>,
    risk: RiskRules,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RiskRules {
    skim_globs: Vec<String>,
    read_globs: Vec<String>,
    /// Lowercase substrings that make code worth a careful read.
    read_keywords: Vec<String>,
}

impl Rules {
    /// Loads rules from `path`, or the built-in `rules.json`.
    ///
    /// # Errors
    /// When the file cannot be read or does not match the rules format.
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let text = match path {
            Some(p) => {
                std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?
            }
            None => DEFAULT_RULES.to_owned(),
        };
        serde_json::from_str(&text).context("parsing rules")
    }

    pub fn is_test(&self, file: &str) -> bool {
        any_glob(&self.test_globs, file)
    }

    /// Whether a file holds tables, caches or queues.
    pub fn is_store(&self, file: &str) -> bool {
        any_glob(&self.store_globs, file)
    }

    /// Risk of a changed hunk, with the reason. `body` is the code around the change.
    pub fn risk(&self, file: &str, comment_only: bool, body: &str) -> (Risk, String) {
        if any_glob(&self.risk.skim_globs, file) {
            return (Risk::Skim, "doc path".to_owned());
        }
        if comment_only {
            return (Risk::Skim, "comments only".to_owned());
        }
        if any_glob(&self.risk.read_globs, file) {
            return (Risk::Read, "path rule".to_owned());
        }
        let lower = body.to_lowercase();
        match self
            .risk
            .read_keywords
            .iter()
            .find(|k| lower.contains(k.as_str()))
        {
            Some(k) => (Risk::Read, format!("mentions \"{k}\"")),
            None => (Risk::Skim, "no risk rule matched".to_owned()),
        }
    }
}

/// A partition of the changed non-test files into folder-tree lanes.
#[derive(Debug, Default)]
pub struct Lanes(Vec<(String, String)>);

impl Lanes {
    pub fn new<'a>(files: impl IntoIterator<Item = &'a str>) -> Self {
        fn children<'a>(files: &[&'a str], root: &str) -> BTreeMap<&'a str, Vec<&'a str>> {
            let mut groups: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
            for &file in files {
                let relative = file[root.len()..].trim_start_matches('/');
                let child = relative
                    .find('/')
                    .map_or(file, |n| &file[..file.len() - relative.len() + n]);
                groups.entry(child).or_default().push(file);
            }
            groups
        }

        let mut files: Vec<&str> = files.into_iter().collect();
        files.sort_unstable();
        files.dedup();
        if files.is_empty() {
            return Self::default();
        }
        let mut root = "";
        let mut groups = children(&files, root);
        while groups.len() == 1 {
            let Some((&child, files)) = groups.first_key_value() else {
                break;
            };
            if files.contains(&child) {
                break;
            }
            root = child;
            groups = children(files, root);
        }
        // Files beside folders at the cut root share one lane, so a README or Cargo.toml does
        // not take a lane each. Only files there: each file is a lane.
        let (loose, folders): (Vec<_>, Vec<_>) = groups
            .into_iter()
            .partition(|(path, files)| files.contains(path));
        let (loose, mut lanes): (Vec<&str>, _) = if folders.is_empty() {
            (Vec::new(), loose)
        } else {
            (loose.into_iter().map(|(path, _)| path).collect(), folders)
        };
        let budget = MAX_LANES - usize::from(!loose.is_empty());
        // Compress folders that only wrap another folder, without changing the lane count.
        loop {
            for (path, files) in &mut lanes {
                while !files.contains(path) {
                    let next = children(files, path);
                    let Some((&child, _)) = next.first_key_value() else {
                        break;
                    };
                    if next.len() != 1 || files.contains(&child) {
                        break;
                    }
                    *path = child;
                }
            }
            let split = lanes
                .iter()
                .enumerate()
                .filter(|(_, (path, files))| !files.contains(path))
                .map(|(i, (path, files))| (i, files.len(), children(files, path)))
                .filter(|(_, _, kids)| kids.len() >= 2 && lanes.len() - 1 + kids.len() <= budget)
                .max_by_key(|(i, count, _)| (*count, std::cmp::Reverse(*i)));
            let Some((i, _, kids)) = split else {
                break;
            };
            lanes.splice(i..=i, kids);
        }
        let single = lanes.len() == 1 && loose.is_empty() && !root.is_empty();
        let loose_name: String = match root.rsplit('/').next() {
            Some(dir) if !dir.is_empty() => {
                let chars: Vec<char> = dir.to_lowercase().chars().collect();
                chars[chars.len().saturating_sub(LANE_MAX)..]
                    .iter()
                    .collect()
            }
            _ => "root".to_owned(),
        };
        Self(
            lanes
                .into_iter()
                .map(|(path, files)| {
                    let is_file = files.contains(&path);
                    let relative = if single {
                        root.rsplit('/').next().unwrap_or(root)
                    } else {
                        path[root.len()..].trim_start_matches('/')
                    };
                    let name = if is_file && !single {
                        let (dir, file) = relative.rsplit_once('/').unwrap_or(("", relative));
                        let stem = Path::new(file)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or(file);
                        if dir.is_empty() {
                            stem.to_owned()
                        } else {
                            format!("{dir}/{stem}")
                        }
                    } else {
                        relative.to_owned()
                    };
                    let parts: Vec<_> = name
                        .split('/')
                        .filter(|p| !GENERIC_DIRS.contains(&p.to_lowercase().as_str()))
                        .collect();
                    let name = if parts.is_empty() {
                        name.rsplit('/').next().unwrap_or(&name).to_owned()
                    } else {
                        parts.join("/")
                    };
                    let chars: Vec<_> = name.to_lowercase().chars().collect();
                    let name = chars[chars.len().saturating_sub(LANE_MAX)..]
                        .iter()
                        .collect();
                    let prefix = if single {
                        format!("{root}/")
                    } else if is_file {
                        path.to_owned()
                    } else {
                        format!("{path}/")
                    };
                    (prefix, name)
                })
                .chain(
                    loose
                        .iter()
                        .map(|file| ((*file).to_owned(), loose_name.clone())),
                )
                .collect(),
        )
    }

    /// Unchanged files use the longest folder prefix, then the old path-based fallback.
    pub fn lane(&self, file: &str) -> String {
        self.0
            .iter()
            .filter(|(path, _)| file == path || (path.ends_with('/') && under(file, path)))
            .max_by_key(|(path, _)| path.len())
            .map_or_else(|| lane_from_path(file), |(_, name)| name.clone())
    }
}

/// A fallback lane: the first non-generic folder, else the file name or `root`.
fn lane_from_path(file: &str) -> String {
    let parts: Vec<&str> = file.split('/').collect();
    let (name, dirs) = parts.split_last().unwrap_or((&"", &[]));
    let lane = match dirs
        .iter()
        .map(|d| d.trim_start_matches('.'))
        .find(|d| !d.is_empty() && !GENERIC_DIRS.contains(&d.to_lowercase().as_str()))
    {
        Some(dir) => dir.to_owned(),
        None if dirs.is_empty() => "root".to_owned(),
        None => name
            .trim_start_matches('.')
            .split('.')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(name)
            .to_owned(),
    };
    lane.to_lowercase().chars().take(LANE_MAX).collect()
}

fn any_glob(globs: &[String], file: &str) -> bool {
    globs.iter().any(|g| glob_match(g, file))
}

/// Matches `/`-separated globs: `**` spans any number of segments, `*` any run inside one.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pat: Vec<&str> = pattern.split('/').collect();
    let segs: Vec<&str> = path.split('/').collect();
    segments_match(&pat, &segs)
}

fn segments_match(pat: &[&str], segs: &[&str]) -> bool {
    match pat.split_first() {
        None => segs.is_empty(),
        Some((&"**", rest)) => (0..=segs.len()).any(|i| segments_match(rest, &segs[i..])),
        Some((first, rest)) => segs.split_first().is_some_and(|(head, tail)| {
            star_match(first.as_bytes(), head.as_bytes()) && segments_match(rest, tail)
        }),
    }
}

fn star_match(pat: &[u8], s: &[u8]) -> bool {
    match pat.split_first() {
        None => s.is_empty(),
        Some((b'*', rest)) => (0..=s.len()).any(|i| star_match(rest, &s[i..])),
        Some((c, rest)) => s
            .split_first()
            .is_some_and(|(h, t)| h == c && star_match(rest, t)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_match_like_the_rules_expect() {
        let cases = [
            ("**/*.test.*", "packages/core/src/new.test.ts", true),
            ("**/*.test.*", "new.test.ts", true),
            ("**/__test__/**", "apps/engine/__test__/test.run.ts", true),
            ("apps/api*/**", "apps/api-v2/src/x.ts", true),
            ("apps/engine/**", "apps/engine/.env.schema", true),
            ("packages/*/src/**", "packages/core/src/a/b.ts", true),
            ("packages/*/src/**", "packages/core/test/b.ts", false),
            ("**/*.test.*", "packages/core/src/new.ts", false),
        ];
        for (pattern, path, want) in cases {
            assert_eq!(glob_match(pattern, path), want, "{pattern} vs {path}");
        }
    }

    #[test]
    fn default_rules_load_and_classify() {
        let rules = Rules::load(None).expect("built-in rules parse");
        assert!(rules.is_test("packages/core/src/x.test.ts"));
        assert!(rules.is_test("pkg/store/store_test.go"));
        assert!(rules.is_test("apps/web/src/banner.stories.tsx"));
        assert!(rules.is_store("db/migrations/0001.sql"));
        assert!(!rules.is_store("apps/api/src/users.ts"));
        assert_eq!(
            rules.risk("a.ts", true, "password").0,
            Risk::Skim,
            "comment-only beats keywords"
        );
        assert_eq!(
            rules
                .risk("a.ts", false, "const passwordHash = hash(input)")
                .0,
            Risk::Read
        );
        assert_eq!(rules.risk("pnpm-lock.yaml", false, "").0, Risk::Skim);
    }

    #[test]
    fn folder_tree_cut_and_refinement() {
        let flat = Lanes::new(["src/flow.rs", "src/rules.rs", "src/map.rs"]);
        assert_eq!(flat.lane("src/flow.rs"), "flow");
        assert_eq!(flat.lane("src/rules.rs"), "rules");
        assert_eq!(flat.lane("src/map.rs"), "map");
        let paths: Vec<_> = ["apps/web/src", "apps/api/src", "packages/core/src"]
            .into_iter()
            .flat_map(|dir| ["a", "b", "c", "d"].map(|file| format!("{dir}/{file}.ts")))
            .collect();
        let mono = Lanes::new(paths.iter().map(String::as_str));
        assert_eq!(mono.lane("apps/web/src/a.ts"), "web");
        assert_eq!(mono.lane("apps/api/src/a.ts"), "api");
        assert_eq!(mono.lane("packages/core/src/a.ts"), "core");
        assert_eq!(mono.lane("apps/web/src/unchanged.ts"), "web");
        let repo = Lanes::new(["README.md", "Cargo.toml", "src/a.rs", "skills/x/SKILL.md"]);
        assert_eq!(repo.lane("README.md"), "root");
        assert_eq!(repo.lane("Cargo.toml"), "root");
        assert_eq!(repo.lane("src/a.rs"), "src");
        assert_eq!(repo.lane("skills/x/SKILL.md"), "skills/x");
    }

    #[test]
    fn single_folder_and_outside_files() {
        let single = Lanes::new(["src/billing/invoice.rs"]);
        assert_eq!(single.lane("src/billing/invoice.rs"), "billing");
        assert_eq!(single.lane("src/billing/other.rs"), "billing");
        assert_eq!(single.lane("src/billing-v2/other.rs"), "billing-v2");
        assert_eq!(single.lane("internal/auth/token.go"), "auth");
        assert_eq!(Lanes::new([]).lane("Cargo.toml"), "root");
    }

    #[test]
    fn lane_names_keep_the_tail_and_file_stems() {
        let long = "src/AnExtremelyLongFolderName/AnotherLongFolderName/a.rs";
        let lanes = Lanes::new([long, "src/b.rs"]);
        assert_eq!(lanes.lane(long), "me/anotherlongfoldername");
        assert_eq!(
            lanes.lane("src/b.rs"),
            "src",
            "a file beside folders joins the root lane"
        );
    }

    #[test]
    fn store_globs_are_independent_of_lanes() {
        let mut json: serde_json::Value = serde_json::from_str(DEFAULT_RULES).expect("json");
        json["storeGlobs"] = serde_json::json!(["**/records/**"]);
        let rules: Rules = serde_json::from_value(json.clone()).expect("custom rules");
        assert!(rules.is_store("src/records/user.ts"));
        assert!(!rules.is_store("db/users.sql"));
        json["lanes"] = serde_json::json!([]);
        assert!(
            serde_json::from_value::<Rules>(json)
                .expect_err("old lanes key")
                .to_string()
                .contains("unknown field `lanes`")
        );
    }
}
