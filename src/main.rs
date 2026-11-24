//! overview-me: spec-linked maps of a change, for reviewing agent-written code.
//! Git gives the facts (`extract`), Claude writes the labels, `validate` checks them against git.

mod base;
mod extract;
mod facts;
mod findings;
mod flow;
mod git;
mod map;
mod page;
mod render;
mod rules;
mod scope;
mod snapshot;
mod symbols;
#[cfg(test)]
mod testutil;
mod validate;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::git::Git;
use crate::rules::Rules;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Repository to read (any worktree).
    #[arg(long, global = true, default_value = ".")]
    repo: PathBuf,
    /// Test, store and risk rules; defaults to the built-in rules.json.
    #[arg(long, global = true)]
    rules: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the base and head of the change: a PR (by number or link, or the checked-out
    /// branch's), else the branch against the default branch.
    Base {
        /// PR number or link; default: the checked-out branch's PR.
        pr: Option<String>,
    },
    /// Write facts for base...head to <store>/<head>.facts.json and a draft map to <store>/<head>.json.
    Extract {
        #[arg(long)]
        base: String,
        #[arg(long, default_value = "HEAD")]
        head: String,
        /// Spec file the criteria come from.
        #[arg(long)]
        spec: Option<PathBuf>,
        /// Facts file to write instead of the default.
        #[arg(long)]
        out: Option<PathBuf>,
        #[command(flatten)]
        options: extract::Options,
    },
    /// Print the changed hunks owned by one node.
    Node {
        id: String,
        #[arg(long)]
        map: Option<PathBuf>,
    },
    /// Check a map against the schema and git; exits 1 on problems.
    Validate { map: PathBuf },
    /// Write the Before and After flow diagrams of a map to <store>/<head>.flow.json.
    Flow {
        map: PathBuf,
        /// Flow file to write instead of the default.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Validate a map, write its flow file, and write the PR or change overview page with
    /// slots to fill to <store>/<head>.review.md.
    Render {
        map: PathBuf,
        /// Write a change overview (uncommitted work) instead of a PR overview.
        #[arg(long)]
        change: bool,
        /// Scope shown in the header line, such as "uncommitted files".
        #[arg(long)]
        scope: Option<String>,
        /// Page to write instead of the default.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Overwrite the page if it exists (what was filled in is lost).
        #[arg(long)]
        fresh: bool,
        /// Add a Findings section from <store>/<head>.findings.json; on an existing page, only
        /// that section is written.
        #[arg(long)]
        findings: bool,
    },
    /// Check code-review findings (OpenCodeReview JSON or SARIF 2.1.0) at the map's head, place
    /// each on the node whose sites hold it, and write <store>/<head>.findings.json.
    Findings {
        map: PathBuf,
        /// The review: `ocr review --format json` output, the same comments written by a host
        /// agent, or a SARIF file.
        review: PathBuf,
        /// The commit the review read, when the file does not name it; must be the map's head.
        #[arg(long)]
        head: Option<String>,
        /// Findings file to write instead of the default.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Write a store markdown file (PR or codebase overview) as an HTML page; exits 1 on an
    /// invalid flow block.
    Page {
        md: PathBuf,
        /// Page to write instead of <md> with an .html extension.
        #[arg(long)]
        out: Option<PathBuf>,
        /// The head is not on the remote: link only Before references (at the base commit).
        #[arg(long)]
        unpushed: bool,
        /// A palette stylesheet to add to the page's palette picker; repeatable. See DESIGN.md.
        #[arg(long = "theme", value_name = "CSS")]
        themes: Vec<PathBuf>,
    },
    /// Print a commit holding HEAD plus all uncommitted and untracked files (HEAD when clean),
    /// made without touching the branch, the index or the files.
    Snapshot,
    /// Print where the map for HEAD lives: <git-common-dir>/overview-me/<head-sha>.json.
    Path {
        #[arg(long, default_value = "HEAD")]
        head: String,
    },
}

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    let git = Git::new(&cli.repo);
    match cli.command {
        Command::Base { pr } => base::run(&git, pr.as_deref())?,
        Command::Extract {
            base,
            head,
            spec,
            out,
            options,
        } => {
            let rules = Rules::load(cli.rules.as_deref())?;
            extract::run(&git, &rules, &base, &head, spec.as_deref(), out, &options)?;
        }
        Command::Node { id, map } => {
            let rules = Rules::load(cli.rules.as_deref())?;
            extract::node(&git, &rules, &id, map)?;
        }
        Command::Validate { map } => {
            let rules = Rules::load(cli.rules.as_deref())?;
            if !validate::run(&git, &rules, &map)? {
                return Ok(ExitCode::FAILURE);
            }
        }
        Command::Flow { map, out } => {
            let rules = Rules::load(cli.rules.as_deref())?;
            flow::run(&git, &rules, &map, out)?;
        }
        Command::Render {
            map,
            change,
            scope,
            out,
            fresh,
            findings,
        } => {
            let rules = Rules::load(cli.rules.as_deref())?;
            let kind = render::Kind {
                title: if change {
                    "change overview"
                } else {
                    "PR overview"
                },
                scope: scope.as_deref(),
            };
            render::run(&git, &rules, &map, &kind, out, fresh, findings)?;
        }
        Command::Findings {
            map,
            review,
            head,
            out,
        } => {
            findings::run(&git, &map, &review, head.as_deref(), out)?;
        }
        Command::Page {
            md,
            out,
            unpushed,
            themes,
        } => {
            println!(
                "{}",
                page::run(&git, &md, out, unpushed, &themes)?.display()
            );
        }
        Command::Snapshot => println!("{}", snapshot::run(&git)?),
        Command::Path { head } => println!("{}", git.map_path(&head)?.display()),
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn scoped_extract_requires_map_and_tuning_flags_parse() {
        assert!(
            Cli::try_parse_from(["overview-me", "extract", "--base", "HEAD", "--path", "src"])
                .is_err()
        );
        for value in ["dir", "symbol"] {
            assert!(
                Cli::try_parse_from([
                    "overview-me",
                    "extract",
                    "--base",
                    "HEAD",
                    "--group-by",
                    value
                ])
                .is_ok()
            );
        }
        assert!(
            Cli::try_parse_from([
                "overview-me",
                "extract",
                "--base",
                "HEAD",
                "--group-by",
                "other"
            ])
            .is_err()
        );
    }
}
