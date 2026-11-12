//! `overview-me snapshot`: the working tree as a commit, so uncommitted work can be mapped like a
//! commit. The commit holds HEAD plus every staged, unstaged and untracked (not ignored) file.
//! It is built with a throwaway index and never touches the branch, the real index or the files.
//! Author, committer and dates are fixed, so the same HEAD and files always give the same sha and
//! a map built for it can be reused.

use anyhow::{Context, Result};

use crate::git::Git;

/// Returns the snapshot commit, or HEAD when nothing is uncommitted.
///
/// # Errors
/// When HEAD does not exist (no commit yet) or a git command fails.
pub fn run(git: &Git) -> Result<String> {
    let head = git.rev("HEAD")?;
    let status = git.run(&["status", "--porcelain", "--untracked-files=all"])?;
    if status.trim().is_empty() {
        return Ok(head);
    }
    let git_dir = git.run(&["rev-parse", "--path-format=absolute", "--git-dir"])?;
    let git_dir = std::path::Path::new(git_dir.trim());
    let index = git_dir.join(format!("overview-me-snapshot-{}.index", std::process::id()));
    // Start from the real index so unchanged files keep their cached stat data.
    let real = git_dir.join("index");
    if real.exists() {
        std::fs::copy(&real, &index).context("copying the index")?;
    }
    let result = commit(git, &head, &index.to_string_lossy());
    let _ = std::fs::remove_file(&index);
    result
}

fn commit(git: &Git, head: &str, index: &str) -> Result<String> {
    let only_index = [("GIT_INDEX_FILE", index)];
    git.run_env(&only_index, &["add", "-A"])?;
    let tree = git.run_env(&only_index, &["write-tree"])?;
    let date = git.run(&["log", "-1", "--format=%cI", head])?;
    let date = date.trim();
    let who = [
        ("GIT_AUTHOR_NAME", "overview-me"),
        ("GIT_AUTHOR_EMAIL", "overview-me@localhost"),
        ("GIT_AUTHOR_DATE", date),
        ("GIT_COMMITTER_NAME", "overview-me"),
        ("GIT_COMMITTER_EMAIL", "overview-me@localhost"),
        ("GIT_COMMITTER_DATE", date),
    ];
    let sha = git.run_env(
        &who,
        &[
            "commit-tree",
            "--no-gpg-sign",
            tree.trim(),
            "-p",
            head,
            "-m",
            "overview-me snapshot of the working tree",
        ],
    )?;
    Ok(sha.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempRepo;

    #[test]
    fn snapshot_holds_uncommitted_work_and_leaves_the_repo_alone() {
        let repo = TempRepo::new("snapshot");
        repo.write("a.txt", "one\n");
        repo.write(".gitignore", "ignored.txt\n");
        let head = repo.commit("base");
        let git = Git::new(repo.path());
        assert_eq!(run(&git).expect("clean"), head, "clean tree gives HEAD");

        repo.write("a.txt", "two\n");
        repo.write("staged.txt", "s\n");
        repo.git(&["add", "staged.txt"]);
        repo.write("new/untracked.txt", "u\n");
        repo.write("ignored.txt", "i\n");
        let status_before = repo.git(&["status", "--porcelain"]);

        let snap = run(&git).expect("snapshot");
        assert_ne!(snap, head);
        assert_eq!(repo.git(&["rev-parse", &format!("{snap}^")]), head);
        assert_eq!(repo.git(&["show", &format!("{snap}:a.txt")]), "two");
        assert_eq!(repo.git(&["show", &format!("{snap}:staged.txt")]), "s");
        assert_eq!(
            repo.git(&["show", &format!("{snap}:new/untracked.txt")]),
            "u"
        );
        let files = repo.git(&["ls-tree", "-r", "--name-only", &snap]);
        assert!(!files.contains("ignored.txt"), "{files}");

        assert_eq!(repo.git(&["rev-parse", "HEAD"]), head, "branch untouched");
        assert_eq!(
            repo.git(&["status", "--porcelain"]),
            status_before,
            "index untouched"
        );
        assert_eq!(run(&git).expect("again"), snap, "same files, same sha");
    }
}
