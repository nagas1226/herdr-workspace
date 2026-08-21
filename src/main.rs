//! herdr-workspace — create a Herdr workspace and apply a layout profile.
//!
//! Two modes, driven by the manifest:
//!   `open` — action `herdr-workspace.open`. Runs on the herdr server with no
//!            TTY. Opens the centered `form` popup pane.
//!   `form` — pane `herdr-workspace.form`. Runs inside the popup with a real
//!            TTY: directory, name, tiled profile cards, cancel/save.

mod apply;
mod complete;
mod config;
mod form;
mod herdr;
mod theme;

use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

fn main() -> ExitCode {
    let mode = std::env::args().nth(1).unwrap_or_default();
    match mode.as_str() {
        "open" => open_action(),
        "wait-open" => wait_open(),
        "form" => ExitCode::from(form::run() as u8),
        _ => {
            eprintln!("herdr-workspace: usage: herdr-workspace (open|form)");
            ExitCode::from(2)
        }
    }
}

/// Action `herdr-workspace.open`. Herdr allows one session popup at a time.
/// Telescope (and similar pickers) invoke this action *while their own popup
/// is still open* and only close after the action exits. Opening our form
/// immediately then fails with `popup already open`. Return success right
/// away and retry in a detached child after the caller popup is gone.
fn open_action() -> ExitCode {
    match try_open_form() {
        OpenResult::Opened => ExitCode::SUCCESS,
        OpenResult::Busy => {
            if let Err(e) = spawn_wait_open() {
                eprintln!("herdr-workspace: {e}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        OpenResult::Failed(err) => {
            eprintln!("herdr-workspace: failed to open popup: {err}");
            ExitCode::from(1)
        }
    }
}

fn wait_open() -> ExitCode {
    // ~8s: telescope polls the action log then closes itself.
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(200));
        match try_open_form() {
            OpenResult::Opened => return ExitCode::SUCCESS,
            OpenResult::Busy => continue,
            OpenResult::Failed(err) => {
                eprintln!("herdr-workspace: failed to open popup: {err}");
                return ExitCode::from(1);
            }
        }
    }
    eprintln!("herdr-workspace: timed out waiting for the current popup to close");
    ExitCode::from(1)
}

enum OpenResult {
    Opened,
    Busy,
    Failed(String),
}

fn try_open_form() -> OpenResult {
    let args = open_form_args();
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let out = herdr::run(&arg_refs);
    if out.status.success() {
        return OpenResult::Opened;
    }
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if herdr::popup_busy(&err) {
        OpenResult::Busy
    } else {
        OpenResult::Failed(err)
    }
}

fn open_form_args() -> Vec<String> {
    let cwd = origin_cwd();
    let mut args = vec![
        "plugin".into(),
        "pane".into(),
        "open".into(),
        "--plugin".into(),
        "herdr-workspace".into(),
        "--entrypoint".into(),
        "form".into(),
        "--focus".into(),
    ];
    if !cwd.is_empty() {
        args.push("--env".into());
        args.push(format!("WORKSPACE_FORM_CWD={cwd}"));
    }
    args
}

fn spawn_wait_open() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let mut cmd = Command::new(exe);
    cmd.arg("wait-open")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().map_err(|e| format!("spawn wait-open: {e}"))?;
    Ok(())
}

fn origin_cwd() -> String {
    let raw = std::env::var("HERDR_PLUGIN_CONTEXT_JSON").unwrap_or_default();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
        if let Some(cwd) = herdr::str_of(&v, &["focused_pane_cwd"])
            .or_else(|| herdr::str_of(&v, &["workspace_cwd"]))
        {
            return cwd;
        }
    }
    std::env::var("HERDR_WORKSPACE_CWD").unwrap_or_default()
}
