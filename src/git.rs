//! Read-only git probes that decide what tuicr should show.

use std::path::{Path, PathBuf};
use std::process::Command;

/// What the review covers.
#[derive(Debug, PartialEq, Eq)]
pub enum Scope {
    /// On the base branch (or no base exists): uncommitted changes only.
    Uncommitted,
    /// On a feature branch: commits since the fork point from `base`, plus
    /// uncommitted changes when the tree is dirty.
    Branch {
        base: String,
        merge_base: String,
        commits: bool,
        dirty: bool,
    },
    Nothing(String),
}

impl Scope {
    /// tuicr arguments selecting this scope; `None` for [`Scope::Nothing`].
    pub fn tuicr_args(&self) -> Option<Vec<String>> {
        match self {
            Scope::Uncommitted => Some(vec!["-w".into()]),
            Scope::Branch {
                merge_base,
                commits,
                dirty,
                ..
            } => {
                let mut args = Vec::new();
                if *commits {
                    args.extend(["-r".into(), format!("{merge_base}..HEAD")]);
                }
                if *dirty {
                    args.push("-w".into());
                }
                Some(args)
            }
            Scope::Nothing(_) => None,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Scope::Uncommitted => "uncommitted changes".into(),
            Scope::Branch {
                base,
                commits,
                dirty,
                ..
            } => match (commits, dirty) {
                (true, true) => format!("branch changes vs {base} + uncommitted changes"),
                (true, false) => format!("branch changes vs {base}"),
                _ => "uncommitted changes".into(),
            },
            Scope::Nothing(why) => why.clone(),
        }
    }
}

pub struct Repo {
    root: PathBuf,
}

impl Repo {
    /// Resolves the repository containing `dir`.
    pub fn discover(dir: &Path) -> Result<Self, String> {
        let root = git(dir, &["rev-parse", "--show-toplevel"])
            .map_err(|e| format!("{} is not inside a git repository ({e})", dir.display()))?;
        Ok(Self {
            root: PathBuf::from(root),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn current_branch(&self) -> Option<String> {
        self.git(&["branch", "--show-current"])
            .ok()
            .filter(|b| !b.is_empty())
    }

    pub fn scope(&self, configured_base: Option<&str>) -> Scope {
        if self
            .git(&["rev-parse", "--verify", "--quiet", "HEAD"])
            .is_err()
        {
            return Scope::Nothing("repository has no commits yet".into());
        }
        let dirty = self.is_dirty();
        let base = configured_base
            .map(str::to_string)
            .or_else(|| self.detect_base());
        let on_base = match (&base, self.current_branch()) {
            (None, _) => true,
            (Some(base), Some(branch)) => *base == branch,
            (Some(_), None) => false, // detached HEAD: compare against base
        };
        if on_base {
            return if dirty {
                Scope::Uncommitted
            } else {
                Scope::Nothing("no uncommitted changes".into())
            };
        }
        let base = base.expect("on_base is true when base is None");
        let Some(merge_base) = self.merge_base(&base) else {
            return if dirty {
                Scope::Uncommitted
            } else {
                Scope::Nothing(format!("no common history with {base}"))
            };
        };
        let head = self.git(&["rev-parse", "HEAD"]).unwrap_or_default();
        let commits = merge_base != head;
        if !commits && !dirty {
            return Scope::Nothing(format!("no changes vs {base}"));
        }
        Scope::Branch {
            base,
            merge_base,
            commits,
            dirty,
        }
    }

    fn is_dirty(&self) -> bool {
        self.git(&["status", "--porcelain", "--untracked-files=normal"])
            .map(|out| !out.is_empty())
            .unwrap_or(false)
    }

    /// `origin/HEAD`'s branch, then the first of `main`/`master` that exists
    /// locally or on `origin`.
    fn detect_base(&self) -> Option<String> {
        if let Ok(head) = self.git(&[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ]) && let Some(name) = head.strip_prefix("origin/")
        {
            return Some(name.to_string());
        }
        ["main", "master"]
            .into_iter()
            .find(|name| !self.base_refs(name).is_empty())
            .map(str::to_string)
    }

    /// The existing refs naming `base`: the local branch and `origin/<base>`.
    fn base_refs(&self, base: &str) -> Vec<String> {
        [
            format!("refs/heads/{base}"),
            format!("refs/remotes/origin/{base}"),
        ]
        .into_iter()
        .filter(|r| self.git(&["rev-parse", "--verify", "--quiet", r]).is_ok())
        .collect()
    }

    /// Fork point of HEAD from `base`. With both the local branch and
    /// `origin/<base>` present, git picks the newest common ancestor across
    /// both, so a stale local `main` does not drag in already-merged commits.
    fn merge_base(&self, base: &str) -> Option<String> {
        let mut refs = self.base_refs(base);
        if refs.is_empty() {
            // A configured base may be any revision (`develop`, `upstream/main`).
            refs.push(base.to_string());
        }
        let mut args = vec!["merge-base", "HEAD"];
        args.extend(refs.iter().map(String::as_str));
        self.git(&args).ok()
    }

    fn git(&self, args: &[&str]) -> Result<String, String> {
        git(&self.root, args)
    }
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempRepo(PathBuf);

    impl TempRepo {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("herdr-earmark-test-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let repo = Self(dir);
            repo.run(&["init", "-q", "-b", "main"]);
            repo.run(&["config", "user.email", "t@t"]);
            repo.run(&["config", "user.name", "t"]);
            repo.commit("a.txt", "one");
            repo
        }

        fn run(&self, args: &[&str]) -> String {
            git(&self.0, args).unwrap()
        }

        fn write(&self, file: &str, body: &str) {
            std::fs::write(self.0.join(file), body).unwrap();
        }

        fn commit(&self, file: &str, body: &str) {
            self.write(file, body);
            self.run(&["add", "."]);
            self.run(&["commit", "-q", "-m", file]);
        }

        fn scope(&self) -> Scope {
            Repo::discover(&self.0).unwrap().scope(None)
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn base_branch_reviews_only_uncommitted_changes() {
        let repo = TempRepo::new("main");
        assert!(matches!(repo.scope(), Scope::Nothing(_)));
        repo.commit("b.txt", "unpushed commit on main is not reviewed");
        assert!(matches!(repo.scope(), Scope::Nothing(_)));
        repo.write("a.txt", "edited");
        assert_eq!(repo.scope(), Scope::Uncommitted);
        assert_eq!(repo.scope().tuicr_args().unwrap(), ["-w"]);
    }

    #[test]
    fn feature_branch_reviews_commits_since_fork_and_uncommitted() {
        let repo = TempRepo::new("branch");
        let fork = repo.run(&["rev-parse", "HEAD"]);
        repo.run(&["switch", "-q", "-c", "feature"]);
        assert!(matches!(repo.scope(), Scope::Nothing(_)));

        repo.write("a.txt", "edited");
        assert_eq!(repo.scope().tuicr_args().unwrap(), ["-w"]);

        repo.commit("b.txt", "feature work");
        let range = format!("{fork}..HEAD");
        assert_eq!(repo.scope().tuicr_args().unwrap(), ["-r", &range]);

        repo.write("b.txt", "more");
        assert_eq!(repo.scope().tuicr_args().unwrap(), ["-r", &range, "-w"]);

        // main moving on after the fork does not change the fork point.
        repo.run(&["stash", "-q"]);
        repo.run(&["switch", "-q", "main"]);
        repo.commit("c.txt", "mainline");
        repo.run(&["switch", "-q", "feature"]);
        assert_eq!(repo.scope().tuicr_args().unwrap(), ["-r", &range]);
    }
}
