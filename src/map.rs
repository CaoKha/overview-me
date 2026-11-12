//! The change map convention (version 1). These types are the schema: `schema/change-map.schema.json`
//! is generated from them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A spec-linked map of one change, stored at `<git-common-dir>/overview-me/<head-sha>.json`.
/// Tests never become nodes: a criterion names the tests that prove it, and `touchedByTest` is a computed hint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[schemars(title = "Change map v1")]
pub struct ChangeMap {
    /// Always 1.
    #[schemars(range(min = 1, max = 1))]
    pub version: u32,
    /// Full sha of the base commit; the change is `base...head`.
    #[schemars(pattern(r"^[0-9a-f]{40}$"))]
    pub base: String,
    /// Full sha of the head commit.
    #[schemars(pattern(r"^[0-9a-f]{40}$"))]
    pub head: String,
    /// Path of the spec the criteria come from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    pub criteria: Vec<Criterion>,
    #[schemars(length(min = 1))]
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// Every node id once, in reading order.
    pub tour: Vec<String>,
}

/// One thing the change claims to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Criterion {
    #[schemars(pattern(r"^REF-[0-9]+$"))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub text: String,
    /// `<spec file>#<heading-slug>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(pattern(r"^.+#[a-z0-9-]+$"))]
    pub spec_ref: Option<String>,
    pub status: Status,
    /// The tests that prove this criterion. Empty means untested.
    pub tests: Vec<TestRef>,
}

impl Criterion {
    pub fn tested(&self) -> bool {
        !self.tests.is_empty()
    }
}

/// A test declaration at head: the line of `test(`/`it(`/`describe(` or `#[test]`, and its name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TestRef {
    /// Path relative to the repo root.
    pub file: String,
    #[schemars(range(min = 1))]
    pub line: u32,
    /// The test's name: the string passed to `test(`, or the function name.
    #[schemars(length(min = 1))]
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Implemented,
    /// Some of the logic lives outside this diff.
    Partial,
    /// Claimed, but no code in the diff does it.
    Missing,
}

/// A group of changed code that reads as one stop: one symbol, or the same edit at several sites.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Node {
    #[schemars(pattern(r"^N[0-9]+$"))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub symbol: String,
    pub lane: Lane,
    pub risk: Risk,
    /// One line: what changed and why.
    #[schemars(length(min = 1))]
    pub caption: String,
    /// Criterion ids this node implements.
    pub criteria: Vec<String>,
    /// Computed hint: a changed test file references the node's exported symbol. Not proof.
    pub touched_by_test: bool,
    /// The main site first. Empty only for a node that just deletes or moves files.
    pub sites: Vec<Site>,
    /// Deleted non-test files this node explains; each deleted file is in exactly one node.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deletions: Vec<Deletion>,
    /// Pure moves (no content change) this node explains; each listed move is in exactly one node.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub renames: Vec<Rename>,
}

/// A file moved without content changes (or moved into a test path).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rename {
    /// Path at the base.
    pub from: String,
    /// Path at head.
    pub to: String,
}

/// A file removed by the change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Deletion {
    /// Path relative to the repo root, as it was at the base.
    pub file: String,
}

/// A range of changed lines on the head side; start and end are changed lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Site {
    /// Path relative to the repo root.
    pub file: String,
    #[schemars(range(min = 1))]
    pub start: u32,
    #[schemars(range(min = 1))]
    pub end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EdgeKind {
    Calls,
    Implements,
    Uses,
}

/// Architecture lane, in display order.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Lane {
    Ui,
    Api,
    Services,
    Db,
    Engine,
    Jobs,
    Other,
}

impl Lane {
    pub const ALL: [Self; 7] = [
        Self::Ui,
        Self::Api,
        Self::Services,
        Self::Db,
        Self::Engine,
        Self::Jobs,
        Self::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ui => "ui",
            Self::Api => "api",
            Self::Services => "services",
            Self::Db => "db",
            Self::Engine => "engine",
            Self::Jobs => "jobs",
            Self::Other => "other",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Ui => "UI",
            Self::Api => "API",
            Self::Services => "Services",
            Self::Db => "Database",
            Self::Engine => "Engine",
            Self::Jobs => "Jobs",
            Self::Other => "Other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Read,
    Skim,
}

impl Risk {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Skim => "skim",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/schema/change-map.schema.json");

    /// The checked-in schema must match the types. `UPDATE_SCHEMA=1 cargo test` rewrites it.
    #[test]
    fn schema_file_is_up_to_date() {
        let generated = serde_json::to_string_pretty(&schemars::schema_for!(ChangeMap))
            .expect("schema serializes")
            + "\n";
        if std::env::var_os("UPDATE_SCHEMA").is_some() {
            std::fs::write(SCHEMA_PATH, &generated).expect("write schema");
        }
        let on_disk = std::fs::read_to_string(SCHEMA_PATH).unwrap_or_default();
        assert_eq!(
            on_disk, generated,
            "schema is stale: run UPDATE_SCHEMA=1 cargo test"
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let json = r#"{"version":1,"base":"a","head":"b","criteria":[],"nodes":[],"edges":[],"tour":[],"extra":1}"#;
        let err = serde_json::from_str::<ChangeMap>(json).expect_err("extra key must fail");
        assert!(err.to_string().contains("unknown field `extra`"), "{err}");
    }
}
