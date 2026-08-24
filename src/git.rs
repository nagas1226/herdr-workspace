//! Git refs for the worktree form: list, fuzzy-filter, fetch, fast-forward.

use std::path::{Path, PathBuf};
use std::process::Command;

const LIMIT: usize = 8;

pub fn repo_root(dir: &str) -> Result<PathBuf, String> {
    let out = git_ok(&["-C", dir, "rev-parse", "--show-toplevel"])?;
    let root = out.trim();
    if root.is_empty() {
        return Err("not a git repository".into());
    }
    Ok(PathBuf::from(root))
}

pub fn list_refs(repo: &Path) -> Result<Vec<String>, String> {
    let mut refs = Vec::new();
    let raw = git_ok(&[
        "-C",
        repo_str(repo),
        "for-each-ref",
        "--format=%(refname:short)",
        "refs/heads",
        "refs/remotes",
        "refs/tags",
    ])?;
    for line in raw.lines() {
        let name = line.trim();
        if name.is_empty() || name.ends_with("/HEAD") {
            continue;
        }
        refs.push(name.to_string());
    }
    refs.sort();
    refs.dedup();
    Ok(refs)
}

/// Ranked shortlist for the base-ref field. Empty query prefers
/// `origin/main`, `main`, `master`, then remaining refs.
pub fn suggestions(query: &str, refs: &[String]) -> Vec<String> {
    let q = query.trim();
    let mut ranked: Vec<(u32, usize, &String)> = refs
        .iter()
        .filter_map(|r| score(q, r).map(|s| (s, r.len(), r)))
        .collect();
    ranked.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(b.2)));
    ranked
        .into_iter()
        .take(LIMIT)
        .map(|(_, _, r)| r.clone())
        .collect()
}

pub fn default_base(refs: &[String]) -> String {
    for preferred in ["origin/main", "origin/master", "main", "master"] {
        if refs.iter().any(|r| r == preferred) {
            return preferred.to_string();
        }
    }
    refs.first().cloned().unwrap_or_else(|| "HEAD".into())
}

pub fn valid_branch_name(name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("branch name is empty".into());
    }
    git_ok(&["check-ref-format", "--branch", name]).map(|_| ())
}

pub fn fetch_prune(repo: &Path) {
    let _ = git_run(&["-C", repo_str(repo), "fetch", "--prune", "--quiet"]);
}

/// Fetch remotes in `repo` (best-effort) and resolve `base` to the newest
/// equivalent: `origin/<name>` when that exists, otherwise the given ref.
#[cfg(test)]
pub fn latest_base(repo: &Path, base: &str) -> Result<String, String> {
    fetch_prune(repo);
    resolve_latest_base(repo, base)
}

pub fn resolve_latest_base(repo: &Path, base: &str) -> Result<String, String> {
    let base = base.trim();
    if base.is_empty() {
        return Err("base ref is empty".into());
    }
    if let Some(remote) = remote_counterpart(base) {
        if rev_exists(repo, &remote) {
            return Ok(remote);
        }
    }
    if rev_exists(repo, base) {
        return Ok(base.to_string());
    }
    Err(format!("unknown ref `{base}`"))
}

/// Fast-forward `worktree` to `base` after a fetch. No-op if already up to date.
pub fn fast_forward(worktree: &Path, base: &str) -> Result<(), String> {
    let target = resolve_latest_base(worktree, base)?;
    let status = git_run(&["-C", repo_str(worktree), "merge", "--ff-only", &target]);
    if !status.status.success() {
        let err = String::from_utf8_lossy(&status.stderr);
        let msg = err.trim();
        if msg.is_empty() {
            return Err(format!("failed to fast-forward onto `{target}`"));
        }
        return Err(msg.to_string());
    }
    Ok(())
}

fn remote_counterpart(base: &str) -> Option<String> {
    if base == "HEAD" {
        return Some("origin/HEAD".into());
    }
    if base.starts_with("origin/") || base.contains("://") {
        return None;
    }
    if let Some(rest) = base.strip_prefix("refs/heads/") {
        return Some(format!("origin/{rest}"));
    }
    if base.starts_with("refs/") {
        return None;
    }
    Some(format!("origin/{base}"))
}

fn rev_exists(repo: &Path, rev: &str) -> bool {
    git_run(&[
        "-C",
        repo_str(repo),
        "rev-parse",
        "--verify",
        "--quiet",
        rev,
    ])
    .status
    .success()
}

fn score(query: &str, candidate: &str) -> Option<u32> {
    if query.is_empty() {
        let prefer = match candidate {
            "origin/main" => 0,
            "origin/master" => 1,
            "main" => 2,
            "master" => 3,
            _ if candidate.starts_with("origin/") => 10,
            _ => 20,
        };
        return Some(prefer);
    }
    let q = query.to_ascii_lowercase();
    let c = candidate.to_ascii_lowercase();
    if c == q {
        return Some(0);
    }
    if c.starts_with(&q) {
        return Some(1);
    }
    if let Some(short) = c.strip_prefix("origin/") {
        if short == q {
            return Some(2);
        }
        if short.starts_with(&q) {
            return Some(3);
        }
    }
    if c.contains(&q) {
        return Some(4);
    }
    if subsequence(&c, &q) {
        return Some(5);
    }
    None
}

fn subsequence(hay: &str, needle: &str) -> bool {
    let mut it = hay.chars();
    for n in needle.chars() {
        if it.find(|c| *c == n).is_none() {
            return false;
        }
    }
    true
}

fn repo_str(repo: &Path) -> &str {
    repo.to_str().unwrap_or(".")
}

fn git_ok(args: &[&str]) -> Result<String, String> {
    let out = git_run(args);
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = err.trim();
        if msg.is_empty() {
            return Err(format!("git {} failed", args.join(" ")));
        }
        return Err(msg.to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn git_run(args: &[&str]) -> std::process::Output {
    Command::new("git")
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap_or_else(|e| panic!("herdr-workspace: failed to spawn git: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn git_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        run(dir.path(), &["git", "init", "-q", "-b", "main"]);
        run(dir.path(), &["git", "config", "user.email", "t@t.t"]);
        run(dir.path(), &["git", "config", "user.name", "t"]);
        fs::write(dir.path().join("a"), "a").unwrap();
        run(dir.path(), &["git", "add", "a"]);
        run(dir.path(), &["git", "commit", "-q", "-m", "init"]);
        dir
    }

    fn run(dir: &Path, args: &[&str]) {
        let st = Command::new(args[0])
            .args(&args[1..])
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(st.success(), "{args:?}");
    }

    #[test]
    fn lists_local_branches() {
        let repo = git_repo();
        run(repo.path(), &["git", "branch", "feature/foo"]);
        run(repo.path(), &["git", "tag", "v1"]);
        let refs = list_refs(repo.path()).unwrap();
        assert!(refs.contains(&"main".into()), "{refs:?}");
        assert!(refs.contains(&"feature/foo".into()), "{refs:?}");
        assert!(refs.contains(&"v1".into()), "{refs:?}");
    }

    #[test]
    fn fuzzy_matches_subsequence_and_prefix() {
        let refs = vec![
            "main".into(),
            "origin/main".into(),
            "feature/login".into(),
            "feat/logout".into(),
            "origin/feature/auth".into(),
        ];
        let got = suggestions("mai", &refs);
        assert_eq!(got[0], "main");
        assert!(got.iter().any(|s| s == "origin/main"), "{got:?}");
        let feat = suggestions("flog", &refs);
        assert!(feat.iter().any(|s| s == "feature/login"), "{feat:?}");
        let empty = suggestions("", &refs);
        assert_eq!(empty[0], "origin/main");
    }

    #[test]
    fn default_prefers_origin_main() {
        let refs = vec!["dev".into(), "origin/main".into(), "main".into()];
        assert_eq!(default_base(&refs), "origin/main");
        assert_eq!(default_base(&["topic".into()]), "topic");
    }

    #[test]
    fn rejects_bad_branch_name() {
        assert!(valid_branch_name("").is_err());
        assert!(valid_branch_name("ok/name").is_ok());
        assert!(valid_branch_name("..bad").is_err());
    }

    #[test]
    fn latest_base_keeps_existing_local() {
        let repo = git_repo();
        let got = latest_base(repo.path(), "main").unwrap();
        assert_eq!(got, "main");
    }
}
