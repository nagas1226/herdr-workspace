//! Live directory completion for the workspace path field.

use std::fs;
use std::path::PathBuf;

const LIMIT: usize = 8;
const MAX_FUZZY_DIRS: usize = 20_000;

/// Build a bounded, one-time index of directories below the home directory.
///
/// The caller should cache this result. Skipping hidden and well-known build
/// directories keeps a normal development home responsive while still making
/// project directories discoverable without an external `fzf` dependency.
pub fn directory_index() -> Vec<String> {
    directory_index_from(&home_dir())
}

/// fzf-like ranking over a directory index. Every whitespace-separated term
/// must match either as a substring or as an ordered character subsequence.
pub fn fuzzy_suggestions(index: &[String], query: &str) -> Vec<String> {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|term| term.to_ascii_lowercase())
        .filter(|term| !term.is_empty())
        .collect();
    if terms.is_empty() {
        return Vec::new();
    }

    let mut matches: Vec<(u8, usize, &String)> = index
        .iter()
        .filter_map(|path| fuzzy_score(path, &terms).map(|score| (score, path.len(), path)))
        .collect();
    matches.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(b.2)));
    matches
        .into_iter()
        .take(LIMIT)
        .map(|(_, _, path)| path.clone())
        .collect()
}

pub fn expand_user(input: &str) -> PathBuf {
    if input == "~" {
        return home_dir();
    }
    if let Some(rest) = input.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    if input.is_empty() {
        return home_dir();
    }
    PathBuf::from(input)
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Next directory options for the current input.
///
/// Returned strings are the values that should replace `input` when accepted
/// (always a directory path, usually ending with `/`).
pub fn suggestions(input: &str) -> Vec<String> {
    let (parent, prefix, display_parent) = split_input(input);
    let Ok(entries) = fs::read_dir(&parent) else {
        return Vec::new();
    };
    let prefix_lower = prefix.to_ascii_lowercase();
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| {
            if name.starts_with('.') && !prefix.starts_with('.') {
                return false;
            }
            name.to_ascii_lowercase().starts_with(&prefix_lower)
        })
        .collect();
    names.sort_by(|a, b| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()));
    names.truncate(LIMIT);
    names
        .into_iter()
        .map(|name| join_display(&display_parent, &name))
        .collect()
}

/// Accept a suggestion into the input buffer (directory + trailing slash).
pub fn accept(suggestion: &str) -> String {
    if suggestion.ends_with('/') {
        suggestion.to_string()
    } else {
        format!("{suggestion}/")
    }
}

pub fn is_existing_dir(input: &str) -> bool {
    let path = expand_user(input.trim());
    path.is_dir()
}

pub fn default_name_from_dir(input: &str) -> String {
    let path = expand_user(input.trim().trim_end_matches('/'));
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

fn split_input(input: &str) -> (PathBuf, String, String) {
    if input.is_empty() || input == "~" {
        return (home_dir(), String::new(), String::from("~/"));
    }
    if input == "~/" {
        return (home_dir(), String::new(), String::from("~/"));
    }
    let ends_slash = input.ends_with('/');
    if ends_slash {
        let expanded = expand_user(input);
        return (expanded, String::new(), input.to_string());
    }
    let (display_parent, prefix) = match input.rsplit_once('/') {
        Some((parent, name)) => {
            let display = if parent.is_empty() {
                "/".to_string()
            } else {
                format!("{parent}/")
            };
            (display, name.to_string())
        }
        None => {
            // Relative fragment from cwd.
            return (
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                input.to_string(),
                String::new(),
            );
        }
    };
    (expand_user(&display_parent), prefix, display_parent)
}

fn join_display(display_parent: &str, name: &str) -> String {
    if display_parent.is_empty() {
        format!("{name}/")
    } else if display_parent.ends_with('/') {
        format!("{display_parent}{name}/")
    } else {
        format!("{display_parent}/{name}/")
    }
}

fn directory_index_from(root: &PathBuf) -> Vec<String> {
    let home = home_dir();
    let mut dirs = Vec::new();
    let mut pending = vec![root.clone()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if dirs.len() >= MAX_FUZZY_DIRS {
                return dirs;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if !kind.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if skip_recursive_dir(name) {
                continue;
            }
            let path = entry.path();
            dirs.push(display_path(&home, &path));
            pending.push(path);
        }
    }
    dirs
}

fn skip_recursive_dir(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            ".git" | "Library" | "node_modules" | "target" | "vendor" | "dist" | "build"
        )
}

fn display_path(home: &PathBuf, path: &PathBuf) -> String {
    match path.strip_prefix(home) {
        Ok(relative) if relative.as_os_str().is_empty() => "~/".into(),
        Ok(relative) => format!("~/{}", relative.display()),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

fn fuzzy_score(path: &str, terms: &[String]) -> Option<u8> {
    let lower_path = path.to_ascii_lowercase();
    let basename = lower_path.rsplit('/').next().unwrap_or(&lower_path);
    let mut score = 0;
    for term in terms {
        let rank = if basename == term {
            0
        } else if basename.starts_with(term) {
            1
        } else if basename.contains(term) {
            2
        } else if lower_path.contains(term) {
            3
        } else if is_subsequence(basename, term) {
            4
        } else if is_subsequence(&lower_path, term) {
            5
        } else {
            return None;
        };
        score = score.max(rank);
    }
    Some(score)
}

fn is_subsequence(haystack: &str, needle: &str) -> bool {
    let mut chars = haystack.chars();
    needle.chars().all(|needle| chars.any(|c| c == needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("alpha")).unwrap();
        fs::create_dir(dir.path().join("alpine")).unwrap();
        fs::create_dir(dir.path().join("beta")).unwrap();
        fs::write(dir.path().join("file.txt"), b"x").unwrap();
        fs::create_dir(dir.path().join(".hidden")).unwrap();
        dir
    }

    #[test]
    fn lists_child_dirs_after_slash() {
        let tree = setup_tree();
        let input = format!("{}/", tree.path().display());
        let got = suggestions(&input);
        assert!(got.iter().any(|s| s.ends_with("alpha/")), "{got:?}");
        assert!(got.iter().any(|s| s.ends_with("beta/")), "{got:?}");
        assert!(!got.iter().any(|s| s.contains("file.txt")), "{got:?}");
        assert!(!got.iter().any(|s| s.contains(".hidden")), "{got:?}");
    }

    #[test]
    fn filters_by_prefix() {
        let tree = setup_tree();
        let input = format!("{}/al", tree.path().display());
        let got = suggestions(&input);
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got.iter().all(|s| s.contains("al")), "{got:?}");
    }

    #[test]
    fn expand_tilde() {
        let home = home_dir();
        assert_eq!(expand_user("~"), home);
        assert_eq!(expand_user("~/src"), home.join("src"));
    }

    #[test]
    fn existing_dir_and_name() {
        let tree = setup_tree();
        let path = tree.path().join("alpha");
        assert!(is_existing_dir(&path.to_string_lossy()));
        assert_eq!(default_name_from_dir(&path.to_string_lossy()), "alpha");
        assert_eq!(
            default_name_from_dir(&format!("{}/", path.display())),
            "alpha"
        );
    }

    #[test]
    fn join_preserves_tilde_display() {
        assert_eq!(join_display("~/", "src"), "~/src/");
        assert_eq!(join_display("/tmp/", "x"), "/tmp/x/");
    }

    #[test]
    fn fuzzy_search_ranks_basename_and_subsequence_matches() {
        let index = vec![
            "~/Projects/herdr-workspace".into(),
            "~/Projects/herdr-config".into(),
            "~/Downloads/archive".into(),
        ];
        let got = fuzzy_suggestions(&index, "hws");
        assert_eq!(got, vec!["~/Projects/herdr-workspace"]);
        let got = fuzzy_suggestions(&index, "config");
        assert_eq!(got[0], "~/Projects/herdr-config");
    }
}
