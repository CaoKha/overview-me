//! Path and content rules: which files are tests, which lane a file is in, how risky a hunk is.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::map::{Lane, Risk};

const DEFAULT_RULES: &str = include_str!("../rules.json");

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
    lane: Lane,
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

    pub fn lane(&self, file: &str) -> Lane {
        self.lanes
            .iter()
            .find(|l| any_glob(&l.globs, file))
            .map_or(Lane::Other, |l| l.lane)
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
            ("**/*.test.*", "packages/perp/src/new.test.ts", true),
            ("**/*.test.*", "new.test.ts", true),
            (
                "**/__test__/**",
                "apps/al-gaib/__test__/test.trading.ts",
                true,
            ),
            ("apps/api*/**", "apps/api-rest-v3/src/x.ts", true),
            ("apps/al-gaib/**", "apps/al-gaib/.env.schema", true),
            ("packages/*/src/**", "packages/perp/src/a/b.ts", true),
            ("packages/*/src/**", "packages/perp/test/b.ts", false),
            ("**/*.test.*", "packages/perp/src/new.ts", false),
        ];
        for (pattern, path, want) in cases {
            assert_eq!(glob_match(pattern, path), want, "{pattern} vs {path}");
        }
    }

    #[test]
    fn default_rules_load_and_classify() {
        let rules = Rules::load(None).expect("built-in rules parse");
        assert!(rules.is_test("packages/perp/src/x.test.ts"));
        assert_eq!(rules.lane("apps/al-gaib/src/x.ts"), Lane::Engine);
        assert_eq!(rules.lane("packages/perp/src/x.ts"), Lane::Services);
        assert_eq!(
            rules.risk("a.ts", true, "fee").0,
            Risk::Skim,
            "comment-only beats keywords"
        );
        assert_eq!(
            rules.risk("a.ts", false, "const openingFee = 1").0,
            Risk::Read
        );
    }
}
