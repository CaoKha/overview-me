//! Thin wrapper over the `git` CLI.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};
use std::sync::{Mutex, PoisonError};

use anyhow::{Context, Result, anyhow, bail};

/// Settings that change output we parse; forced on every call.
const IMMUNE_CONFIG: [&str; 6] = [
    "core.quotePath=false",
    "diff.noprefix=false",
    "diff.mnemonicPrefix=false",
    "diff.interHunkContext=0",
    "diff.relative=false",
    "color.ui=false",
];

fn failed(args: &[&str], out: &Output) -> anyhow::Error {
    anyhow!(
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr).trim()
    )
}

pub struct Git {
    repo: PathBuf,
    /// One `git cat-file --batch` for every file read, started on first use: a `git show` per
    /// file costs a process start, which dominates on a large repo.
    blobs: Mutex<Option<Blobs>>,
}

/// A running `git cat-file --batch`, read one object at a time.
struct Blobs {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Blobs {
    /// The text of the blob `spec` (`rev:path`), or `None` when it is missing or not a file.
    fn read(&mut self, spec: &str) -> std::io::Result<Option<String>> {
        writeln!(self.stdin, "{spec}")?;
        self.stdin.flush()?;
        let mut header = String::new();
        if self.stdout.read_line(&mut header)? == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        // `<sha> <type> <size>` when found; otherwise `<spec> missing` and the like, where the
        // spec may hold spaces.
        let parts: Vec<&str> = header.trim_end().split(' ').collect();
        let found = parts.len() == 3 && parts[0].bytes().all(|b| b.is_ascii_hexdigit());
        let Some(size) = parts
            .get(2)
            .filter(|_| found)
            .and_then(|s| s.parse::<usize>().ok())
        else {
            return Ok(None);
        };
        let mut body = vec![0; size + 1];
        self.stdout.read_exact(&mut body)?;
        body.pop();
        Ok((parts[1] == "blob").then(|| String::from_utf8_lossy(&body).into_owned()))
    }
}

impl Drop for Blobs {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Git {
    pub fn new(repo: &Path) -> Self {
        Self {
            repo: repo.to_path_buf(),
            blobs: Mutex::new(None),
        }
    }

    /// The text of `path` at `rev`, or `None` when it is missing or not a file.
    pub fn show(&self, rev: &str, path: &str) -> Option<String> {
        let spec = format!("{rev}:{path}");
        let fallback = || self.run(&["show", "--end-of-options", &spec]).ok();
        // The batch protocol is line based.
        if spec.contains('\n') {
            return fallback();
        }
        let mut blobs = self.blobs.lock().unwrap_or_else(PoisonError::into_inner);
        if blobs.is_none() {
            *blobs = self.start_blobs().ok();
        }
        match blobs.as_mut().map(|b| b.read(&spec)) {
            Some(Ok(text)) => text,
            // Not started, or it died: drop it and read this file the slow way.
            _ => {
                *blobs = None;
                fallback()
            }
        }
    }

    fn start_blobs(&self) -> Result<Blobs> {
        let mut child = self
            .command(&["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("running git cat-file")?;
        let stdin = child.stdin.take().context("git stdin")?;
        let stdout = BufReader::new(child.stdout.take().context("git stdout")?);
        Ok(Blobs {
            child,
            stdin,
            stdout,
        })
    }

    /// The repository folder git runs in.
    pub fn repo(&self) -> &Path {
        &self.repo
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
            return Err(failed(args, &out));
        }
        Ok(out.stdout)
    }

    /// Runs git with extra environment variables and returns stdout as text.
    ///
    /// # Errors
    /// See [`Git::run_bytes`].
    pub fn run_env(&self, envs: &[(&str, &str)], args: &[&str]) -> Result<String> {
        let out = self
            .command(args)
            .envs(envs.iter().copied())
            .output()
            .context("running git")?;
        if !out.status.success() {
            return Err(failed(args, &out));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
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
            _ => Err(failed(args, &out)),
        }
    }

    /// Runs a git command whose exit code 1 means "nothing found" (like `grep`): returns empty
    /// output then.
    ///
    /// # Errors
    /// When git cannot start or exits with a code other than 0 or 1.
    pub fn run_or_empty(&self, args: &[&str]) -> Result<String> {
        let out = self.command(args).output().context("running git")?;
        match out.status.code() {
            Some(0) => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
            Some(1) => Ok(String::new()),
            _ => Err(failed(args, &out)),
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

    pub(crate) fn merge_base(&self, a: &str, b: &str) -> Result<String> {
        Ok(self
            .run(&["merge-base", "--end-of-options", a, b])?
            .trim()
            .to_owned())
    }

    /// The text of each of `paths` at `rev`, in one `git cat-file --batch`; `None` for a path
    /// that is missing or not a file.
    ///
    /// # Errors
    /// When git cannot start or exits non-zero.
    pub fn read_files(&self, rev: &str, paths: &[&str]) -> Result<Vec<Option<String>>> {
        let mut child = self
            .command(&["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("running git")?;
        let input: String = paths.iter().map(|p| format!("{rev}:{p}\n")).collect();
        let mut stdin = child.stdin.take().context("git stdin")?;
        // Write from another thread: git's output can fill the pipe before it reads all input.
        let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
        let out = child.wait_with_output().context("running git")?;
        writer
            .join()
            .map_err(|_| anyhow::anyhow!("writing to git cat-file"))?
            .context("writing to git cat-file")?;
        if !out.status.success() {
            bail!(
                "git cat-file --batch failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        let mut rest = out.stdout.as_slice();
        let mut files = Vec::with_capacity(paths.len());
        for _ in paths {
            let Some(eol) = rest.iter().position(|&b| b == b'\n') else {
                bail!("git cat-file --batch: output ends early");
            };
            let header = String::from_utf8_lossy(&rest[..eol]).into_owned();
            rest = &rest[eol + 1..];
            // `<sha> <type> <size>`, or `<object> missing` / `ambiguous`.
            let mut parts = header.split(' ');
            let (kind, size) = (
                parts.nth(1),
                parts.next().and_then(|s| s.parse::<usize>().ok()),
            );
            let Some(size) = size else {
                files.push(None);
                continue;
            };
            if rest.len() < size + 1 {
                bail!("git cat-file --batch: output ends early");
            }
            let body = &rest[..size];
            rest = &rest[size + 1..];
            files.push((kind == Some("blob")).then(|| String::from_utf8_lossy(body).into_owned()));
        }
        Ok(files)
    }

    /// Where maps live: shared by every worktree of the repo.
    ///
    /// # Errors
    /// When the repo is not a git repository.
    pub fn store(&self) -> Result<PathBuf> {
        let common = self.run(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
        Ok(Path::new(common.trim()).join("overview-me"))
    }

    /// Path of the map for `head`.
    ///
    /// # Errors
    /// See [`Git::store`] and [`Git::rev`].
    pub fn map_path(&self, head: &str) -> Result<PathBuf> {
        Ok(self.store()?.join(format!("{}.json", self.rev(head)?)))
    }
}

#[cfg(test)]
mod tests {
    use super::Git;
    use crate::testutil::TempRepo;

    #[test]
    fn show_reads_files_through_one_batch() {
        let repo = TempRepo::new("show");
        repo.write("a.txt", "one\ntwo\n");
        repo.write("dir/with space.txt", "spaced");
        repo.write("empty.txt", "");
        let head = repo.commit("files");
        let git = Git::new(repo.path());
        assert_eq!(git.show(&head, "a.txt").as_deref(), Some("one\ntwo\n"));
        assert_eq!(git.show(&head, "missing.txt"), None);
        assert_eq!(git.show(&head, "dir"), None, "a folder is not a file");
        assert_eq!(
            git.show(&head, "dir/with space.txt").as_deref(),
            Some("spaced")
        );
        assert_eq!(git.show(&head, "missing 12 3.txt"), None);
        assert_eq!(git.show(&head, "empty.txt").as_deref(), Some(""));
        // The same process keeps answering after a miss.
        assert_eq!(git.show(&head, "a.txt").as_deref(), Some("one\ntwo\n"));
    }
}
