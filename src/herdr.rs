//! Thin wrapper around the `herdr` CLI via `$HERDR_BIN_PATH`.

use std::ffi::OsStr;
use std::process::{Command, Output, Stdio};

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

/// Fire-and-forget a herdr CLI command. Used for `agent start` after the
/// popup is already closed so a 30s readiness wait cannot hold the form.
pub fn spawn<I, S>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut cmd = Command::new(bin());
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().map_err(|e| format!("spawn herdr: {e}"))?;
    Ok(())
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
    let trimmed = stderr.trim();
    if trimmed.contains("popup already open") || trimmed.contains("ui_busy") {
        return true;
    }
    serde_json::from_str::<serde_json::Value>(trimmed)
        .ok()
        .and_then(|v| {
            let err = v.get("error")?;
            let code = err.get("code").and_then(|c| c.as_str()).unwrap_or("");
            let msg = err.get("message").and_then(|c| c.as_str()).unwrap_or("");
            Some(code == "ui_busy" || msg.contains("popup already open") || msg.contains("ui_busy"))
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
}
