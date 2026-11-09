//! A throwaway git repository for tests.

use std::path::{Path, PathBuf};
use std::process::Command;

pub struct TempRepo {
    pub dir: PathBuf,
}

impl TempRepo {
    pub fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("change-map-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp repo dir");
        let repo = Self { dir };
        repo.git(&["init", "-q"]);
        for (k, v) in [
            ("user.email", "t@t"),
            ("user.name", "t"),
            ("commit.gpgsign", "false"),
        ] {
            repo.git(&["config", k, v]);
        }
        repo
    }

    /// Runs git in the repo and returns stdout; panics with stderr on failure.
    pub fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.dir)
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    pub fn write(&self, path: &str, content: &str) {
        let file = self.dir.join(path);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(file, content).expect("write file");
    }

    /// Commits everything and returns the new head sha.
    pub fn commit(&self, message: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--allow-empty", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
