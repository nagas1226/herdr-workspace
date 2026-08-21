//! Live directory completion for the workspace path field.

use std::fs;
use std::path::PathBuf;

const LIMIT: usize = 8;

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
}
