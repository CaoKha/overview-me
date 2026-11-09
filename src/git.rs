//! Thin wrapper over the `git` CLI.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Settings that change output we parse; forced on every call.
const IMMUNE_CONFIG: [&str; 6] = [
    "core.quotePath=false",
    "diff.noprefix=false",
    "diff.mnemonicPrefix=false",
    "diff.interHunkContext=0",
    "diff.relative=false",
    "color.ui=false",
];

pub struct Git {
    repo: PathBuf,
}

impl Git {
    pub fn new(repo: &Path) -> Self {
        Self {
            repo: repo.to_path_buf(),
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(&self.repo);
        // Output must not depend on the user's config: plain paths, a/ b/ prefixes, no merged hunks.
        for setting in IMMUNE_CONFIG {
            cmd.args(["-c", setting]);
        }
        cmd.args(args);
        cmd
    }

    /// Runs git and returns raw stdout.
    ///
    /// # Errors
    /// When git cannot start or exits non-zero; the error carries git's stderr.
    pub fn run_bytes(&self, args: &[&str]) -> Result<Vec<u8>> {
        let out = self.command(args).output().context("running git")?;
        if !out.status.success() {
            bail!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(out.stdout)
    }

    /// Runs git and returns stdout as text; invalid UTF-8 becomes U+FFFD instead of failing.
    ///
    /// # Errors
    /// See [`Git::run_bytes`].
    pub fn run(&self, args: &[&str]) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.run_bytes(args)?).into_owned())
    }

    /// Runs a git command whose exit code 1 means "no" (like `grep -q`).
    ///
    /// # Errors
    /// When git cannot start or exits with a code other than 0 or 1.
    pub fn check(&self, args: &[&str]) -> Result<bool> {
        let out = self.command(args).output().context("running git")?;
        match out.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => bail!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        }
    }

    /// Full sha of a revision.
    ///
    /// # Errors
    /// When the revision does not exist.
    pub fn rev(&self, rev: &str) -> Result<String> {
        Ok(self
            .run(&[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{rev}^{{commit}}"),
            ])?
            .trim()
            .to_owned())
    }

    /// Where maps live: shared by every worktree of the repo.
    ///
    /// # Errors
    /// When the repo is not a git repository.
    pub fn store(&self) -> Result<PathBuf> {
        let common = self.run(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
        Ok(Path::new(common.trim()).join("change-map"))
    }

    /// Path of the map for `head`.
    ///
    /// # Errors
    /// See [`Git::store`] and [`Git::rev`].
    pub fn map_path(&self, head: &str) -> Result<PathBuf> {
        Ok(self.store()?.join(format!("{}.json", self.rev(head)?)))
    }
}
