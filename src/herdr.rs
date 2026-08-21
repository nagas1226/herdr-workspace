//! Thin wrapper around the `herdr` CLI via `$HERDR_BIN_PATH`.

use std::ffi::OsStr;
use std::process::{Command, Output};

pub fn bin() -> String {
    std::env::var("HERDR_BIN_PATH").unwrap_or_else(|_| "herdr".to_string())
}

pub fn run<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(bin())
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("herdr-workspace: failed to spawn herdr CLI {}: {e}", bin()))
}

pub fn json<I, S>(args: I) -> Result<serde_json::Value, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let out = run(args);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        let msg = stderr.trim();
        if msg.is_empty() {
            return Err(format!("herdr exited {}", out.status));
        }
        return Err(msg.to_string());
    }
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(serde_json::json!({}));
    }
    match serde_json::from_str(trimmed) {
        Ok(v) => Ok(v),
        Err(_) => Ok(serde_json::json!({"stdout": trimmed})),
    }
}

pub fn popup_busy(stderr: &str) -> bool {
    matches_error(stderr, "ui_busy")
        || stderr.contains("popup already open")
        || stderr.contains("ui_busy")
}

pub fn agent_name_taken(stderr: &str) -> bool {
    matches_error(stderr, "agent_name_taken") || stderr.contains("agent_name_taken")
}

fn matches_error(stderr: &str, code: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(stderr.trim())
        .ok()
        .and_then(|v| {
            let err = v.get("error")?;
            err.get("code").and_then(|c| c.as_str()).map(|c| c == code)
        })
        .unwrap_or(false)
}

pub fn str_of(v: &serde_json::Value, keys: &[&str]) -> Option<String> {
    let mut cur = v;
    for k in keys {
        cur = cur.get(*k)?;
    }
    match cur {
        serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_popup_already_open_json() {
        let stderr = r#"{"error":{"code":"plugin_pane_open_failed","message":"popup already open"},"id":"cli:plugin"}"#;
        assert!(popup_busy(stderr));
    }

    #[test]
    fn detects_ui_busy() {
        assert!(popup_busy(r#"{"error":{"code":"ui_busy","message":"settings open"}}"#));
        assert!(!popup_busy(r#"{"error":{"code":"plugin_not_found","message":"nope"}}"#));
    }

    #[test]
    fn detects_agent_name_taken() {
        let stderr = r#"{"error":{"code":"agent_name_taken","message":"agent name reviewer is already used"},"id":"cli:agent:start"}"#;
        assert!(agent_name_taken(stderr));
        assert!(!agent_name_taken(r#"{"error":{"code":"ui_busy","message":"x"}}"#));
    }
}
