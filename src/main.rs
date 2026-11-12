//! overview-me: spec-linked maps of a change, for reviewing agent-written code.
//! Git gives the facts (`extract`), Claude writes the labels, `validate` checks them against git.

mod extract;
mod facts;
mod git;
mod map;
mod page;
mod render;
mod rules;
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
    /// Lane, test and risk rules; defaults to the built-in rules.json.
    #[arg(long, global = true)]
    rules: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Write facts and a draft map for base...head to <store>/<head>.facts.json.
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
    },
    /// Check a map against the schema and git; exits 1 on problems.
    Validate { map: PathBuf },
    /// Write <map>.md (criteria, tour, flow diagram) next to a map.
    Render { map: PathBuf },
    /// Write a store markdown file (review guide or overview) as an HTML page; exits 1 on an
    /// invalid flow block.
    Page {
        md: PathBuf,
        /// Page to write instead of <md> with an .html extension.
        #[arg(long)]
        out: Option<PathBuf>,
        /// The head is not on the remote: link only Before references (at the base commit).
        #[arg(long)]
        unpushed: bool,
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
        Command::Extract {
            base,
            head,
            spec,
            out,
        } => {
            let rules = Rules::load(cli.rules.as_deref())?;
            extract::run(&git, &rules, &base, &head, spec.as_deref(), out)?;
        }
        Command::Validate { map } => {
            let rules = Rules::load(cli.rules.as_deref())?;
            if !validate::run(&git, &rules, &map)? {
                return Ok(ExitCode::FAILURE);
            }
        }
        Command::Render { map } => println!("{}", render::run(&map)?.display()),
        Command::Page { md, out, unpushed } => {
            println!("{}", page::run(&git, &md, out, unpushed)?.display());
        }
        Command::Snapshot => println!("{}", snapshot::run(&git)?),
        Command::Path { head } => println!("{}", git.map_path(&head)?.display()),
    }
    Ok(ExitCode::SUCCESS)
}
