//! Path and content rules: which files are tests, which lane a file is in, how risky a hunk is.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::map::Risk;

const DEFAULT_RULES: &str = include_str!("../rules.json");

/// Longest lane name, in characters.
pub const LANE_MAX: usize = 24;
/// Lanes a flow draws; more get merged.
pub const MAX_LANES: usize = 5;
/// The lane whose files are tables, caches and queues: drawn as stores.
const STORE_LANE: &str = "db";
/// Rule lanes in their usual top-to-bottom order; other lanes come after them.
const LANE_ORDER: [&str; 6] = ["ui", "api", "services", "db", "engine", "jobs"];
/// Folders that say nothing about what the code does, skipped when naming a lane from a path.
const GENERIC_DIRS: [&str; 19] = [
    "src", "lib", "libs", "app", "apps", "internal", "pkg", "packages", "crates", "cmd", "source",
    "sources", "main", "java", "kotlin", "scala", "com", "org", "net",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Rules {
    test_globs: Vec<String>,
    lanes: Vec<LaneRule>,
    risk: RiskRules,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LaneRule {
    lane: String,
    globs: Vec<String>,
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

    /// The lane a file is drawn in: the first rule whose globs match, else a name from its path.
    pub fn lane(&self, file: &str) -> String {
        self.rule_lane(file)
            .map_or_else(|| lane_from_path(file), str::to_owned)
    }

    /// Whether a file holds tables, caches or queues: it falls in the `db` rule.
    pub fn is_store(&self, file: &str) -> bool {
        self.rule_lane(file) == Some(STORE_LANE)
    }

    fn rule_lane(&self, file: &str) -> Option<&str> {
        self.lanes
            .iter()
            .find(|l| any_glob(&l.globs, file))
            .map(|l| l.lane.as_str())
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

/// Where a lane sorts when the call graph does not decide: rule lanes in their usual order,
/// then the rest.
pub fn lane_rank(lane: &str) -> usize {
    LANE_ORDER
        .iter()
        .position(|l| *l == lane)
        .unwrap_or(LANE_ORDER.len())
}

/// A lane named from a path no rule matches: its first folder that is not a generic one such as
/// `src` or `packages`, else its file name, else `root` for a file at the top of the repo.
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
        assert_eq!(rules.lane("db/migrations/0001.sql"), "db");
        assert_eq!(rules.lane("apps/worker/src/send.ts"), "jobs");
        assert_eq!(rules.lane("src/engine/match.rs"), "engine");
        assert_eq!(rules.lane("apps/api/src/users.ts"), "api");
        assert_eq!(rules.lane("apps/web/src/main.ts"), "ui");
        assert_eq!(rules.lane("src/components/Button.tsx"), "ui");
        assert_eq!(rules.lane("packages/core/src/x.ts"), "services");
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
    fn unmatched_paths_name_their_lane() {
        let rules = Rules::load(None).expect("built-in rules parse");
        let cases = [
            ("scripts/release.sh", "scripts"),
            ("src/flow.rs", "flow"),
            ("apps/billing/src/invoice.ts", "billing"),
            ("internal/auth/token.go", "auth"),
            (".github/workflows/ci.yml", "github"),
            ("src/.env.example", "env"),
            ("Cargo.toml", "root"),
            ("src/Main.kt", "main"),
        ];
        for (path, want) in cases {
            assert_eq!(rules.lane(path), want, "{path}");
        }
        assert!(lane_rank("ui") < lane_rank("db"));
        assert_eq!(lane_rank("billing"), lane_rank("scripts"));
    }
}
