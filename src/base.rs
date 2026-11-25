//! `overview-me base`: the two commits a change sits between, so the skill does not work them
//! out with `gh` and `git` by hand.
//!
//! - A PR number or link: its base branch and head commit. A PR that is not checked out is
//!   fetched, never checked out.
//! - No argument: the checked-out branch's PR, or else the branch against the default branch.
//! - Nothing committed on top of the base but files changed: the work is uncommitted.

use std::process::Command;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::git::Git;

#[derive(Debug, PartialEq, Eq)]
pub struct Change {
    pub base: String,
    pub head: String,
    /// `pr`, `branch` or `uncommitted`.
    pub mode: &'static str,
    pub pr: Option<u64>,
    /// The ref the base comes from, such as `origin/main`.
    pub against: String,
}

/// `gh pr view [<pr>]` as JSON, or `None` when `gh` is missing or finds no PR.
fn gh_pr(git: &Git, pr: Option<&str>) -> Option<Value> {
    let mut cmd = Command::new("gh");
    cmd.current_dir(git.repo())
        .env("GH_PROMPT_DISABLED", "1")
        .args(["pr", "view"]);
    if let Some(pr) = pr {
        cmd.arg(pr);
    }
    cmd.args(["--json", "number,baseRefName,headRefOid"]);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

/// `origin/<branch>` when the remote branch is known, else the local branch.
fn remote_or_local(git: &Git, branch: &str) -> Result<String> {
    let remote = format!("origin/{branch}");
    if git.rev(&remote).is_ok() {
        return Ok(remote);
    }
    if git.rev(branch).is_ok() {
        return Ok(branch.to_owned());
    }
    bail!("the base branch {branch} is neither on origin nor local; fetch it and run again")
}

/// The default branch: `origin/HEAD`, else `origin/main`, `main`, `master`.
fn default_branch(git: &Git) -> Result<String> {
    if let Ok(r) = git.run(&["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]) {
        let r = r.trim();
        if !r.is_empty() {
            return Ok(r.to_owned());
        }
    }
    for r in ["origin/main", "main", "master"] {
        if git.rev(r).is_ok() {
            return Ok(r.to_owned());
        }
    }
    bail!("no default branch found (origin/HEAD, origin/main, main, master): say which base to use")
}

/// Finds the change. `pr` is a PR number or link.
///
/// # Errors
/// When the PR cannot be read or fetched, no default branch exists, or there is no change.
pub fn resolve(git: &Git, pr: Option<&str>) -> Result<Change> {
    if pr.is_some_and(|p| p.starts_with('-')) {
        bail!("not a PR number or link: {}", pr.unwrap_or_default());
    }
    let head_now = git.rev("HEAD")?;
    let found = gh_pr(git, pr);
    if pr.is_some() && found.is_none() {
        bail!(
            "gh could not read PR {}: check `gh auth status` and the number",
            pr.unwrap_or_default()
        );
    }
    let (head, against, number) = match &found {
        Some(v) => {
            let base_ref = v["baseRefName"]
                .as_str()
                .context("gh gave no baseRefName")?;
            let head_oid = v["headRefOid"].as_str().unwrap_or_default().to_owned();
            let number = v["number"].as_u64();
            if pr.is_some() && head_oid != head_now {
                // Not checked out: fetch the PR head and its base, and leave the files alone.
                let n = number.context("gh gave no PR number")?;
                git.run(&[
                    "fetch",
                    "--quiet",
                    "origin",
                    "--end-of-options",
                    base_ref,
                    &format!("pull/{n}/head"),
                ])?;
                (git.rev(&head_oid)?, remote_or_local(git, base_ref)?, number)
            } else {
                let _ = git.run(&["fetch", "--quiet", "origin", "--end-of-options", base_ref]);
                (head_now.clone(), remote_or_local(git, base_ref)?, number)
            }
        }
        None => (head_now.clone(), default_branch(git)?, None),
    };
    let base = git.merge_base(&against, &head)?;
    let mut mode = if number.is_some() { "pr" } else { "branch" };
    if head == head_now {
        let ahead = git.run(&["rev-list", "--count", &format!("{base}..{head}")])?;
        if ahead.trim() == "0" {
            if git.run(&["status", "--porcelain"])?.trim().is_empty() {
                bail!("nothing to show: HEAD is at {against} and no file is changed");
            }
            mode = "uncommitted";
        }
    }
    Ok(Change {
        base,
        head,
        mode,
        pr: number,
        against,
    })
}

/// Prints the change as `key value` lines.
///
/// # Errors
/// See [`resolve`].
pub fn run(git: &Git, pr: Option<&str>) -> Result<()> {
    let c = resolve(git, pr)?;
    println!("base {}", c.base);
    println!("head {}", c.head);
    println!("mode {}", c.mode);
    println!("against {}", c.against);
    if let Some(n) = c.pr {
        println!("pr {n}");
    }
    if c.mode == "uncommitted" {
        println!(
            "note: nothing is committed on top of the base; the work is uncommitted, so write a change overview"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempRepo;

    #[test]
    fn branch_against_the_default_branch() {
        let repo = TempRepo::new("base-branch");
        repo.write("a.txt", "a\n");
        let first = repo.commit("first");
        repo.git(&["checkout", "-q", "-b", "feature"]);
        repo.write("a.txt", "b\n");
        let second = repo.commit("second");
        let git = Git::new(repo.path());
        let c = resolve(&git, None).expect("resolves");
        assert_eq!(
            (c.base.as_str(), c.head.as_str(), c.mode),
            (first.as_str(), second.as_str(), "branch")
        );
        assert!(
            ["main", "master"].contains(&c.against.as_str()),
            "{}",
            c.against
        );

        // On the default branch with only uncommitted edits: the work is uncommitted.
        repo.git(&["checkout", "-q", &c.against]);
        assert!(
            resolve(&git, None).is_err(),
            "a clean default branch has nothing to show"
        );
        repo.write("a.txt", "c\n");
        let c = resolve(&git, None).expect("resolves");
        assert_eq!((c.base.as_str(), c.mode), (first.as_str(), "uncommitted"));
    }

    #[test]
    fn option_like_pr_is_refused() {
        let repo = TempRepo::new("base-opt");
        repo.write("a.txt", "a\n");
        repo.commit("first");
        assert!(resolve(&Git::new(repo.path()), Some("--web")).is_err());
    }
}
