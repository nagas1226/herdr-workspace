//! herdr-workspace — create a Herdr workspace and apply a layout profile.
//!
//! Modes, driven by the manifest:
//!   `open` — action `herdr-workspace.open`. No TTY; opens the `form` popup.
//!   `open-worktree` — action `herdr-workspace.worktree`. Opens `worktree`.
//!   `form` — pane `herdr-workspace.form`: directory, name, profile cards.
//!   `worktree` — pane `herdr-workspace.worktree`: branch, base ref, profiles.

mod apply;
mod complete;
mod config;
mod form;
mod git;
mod herdr;
mod popup;
mod theme;
mod worktree_form;

use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

fn main() -> ExitCode {
    let mode = std::env::args().nth(1).unwrap_or_default();
    match mode.as_str() {
        "open" => open_action("form"),
        "open-worktree" => open_action("worktree"),
        "wait-open" => wait_open(),
        "form" => ExitCode::from(form::run() as u8),
        "worktree" => ExitCode::from(worktree_form::run() as u8),
        "start-agents" => start_agents(),
        _ => {
            eprintln!("herdr-workspace: usage: herdr-workspace (open|open-worktree|form|worktree)");
            ExitCode::from(2)
        }
    }
}

/// Herdr allows one session popup at a time. Telescope (and similar pickers)
/// invoke this action *while their own popup is still open* and only close
/// after the action exits. Opening our form immediately then fails with
/// `popup already open`. Return success right away and retry in a detached
/// child after the caller popup is gone.
fn open_action(entrypoint: &str) -> ExitCode {
    match try_open_form(entrypoint) {
        OpenResult::Opened => ExitCode::SUCCESS,
        OpenResult::Busy => {
            if let Err(e) = spawn_wait_open(entrypoint) {
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
    let entrypoint = std::env::args().nth(2).unwrap_or_else(|| "form".into());
    // ~8s: telescope polls the action log then closes itself.
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(200));
        match try_open_form(&entrypoint) {
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

fn try_open_form(entrypoint: &str) -> OpenResult {
    let args = open_form_args(entrypoint);
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

fn open_form_args(entrypoint: &str) -> Vec<String> {
    let cwd = origin_cwd();
    let workspace_id = origin_workspace_id();
    let mut args = vec![
        "plugin".into(),
        "pane".into(),
        "open".into(),
        "--plugin".into(),
        "herdr-workspace".into(),
        "--entrypoint".into(),
        entrypoint.to_string(),
        "--focus".into(),
    ];
    if !cwd.is_empty() {
        let key = if entrypoint == "worktree" {
            "WORKTREE_FORM_CWD"
        } else {
            "WORKSPACE_FORM_CWD"
        };
        args.push("--env".into());
        args.push(format!("{key}={cwd}"));
    }
    if entrypoint == "worktree" && !workspace_id.is_empty() {
        args.push("--env".into());
        args.push(format!("WORKTREE_FORM_WORKSPACE_ID={workspace_id}"));
    }
    args
}

fn spawn_wait_open(entrypoint: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let mut cmd = Command::new(exe);
    cmd.arg("wait-open")
        .arg(entrypoint)
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

/// Detached after the form popup closes. `agent start` waits until each
/// agent is ready, so this must not run inside the popup process.
fn start_agents() -> ExitCode {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
    }
    let Some(path) = std::env::args().nth(2) else {
        eprintln!("herdr-workspace: usage: herdr-workspace start-agents <jobs.json>");
        return ExitCode::from(2);
    };
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("herdr-workspace: read {path}: {e}");
            return ExitCode::from(1);
        }
    };
    let _ = std::fs::remove_file(&path);
    let jobs: Vec<Vec<String>> = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("herdr-workspace: parse agent jobs: {e}");
            return ExitCode::from(1);
        }
    };
    let handles: Vec<_> = jobs
        .into_iter()
        .map(|args| std::thread::spawn(move || start_one_agent(&args)))
        .collect();
    for handle in handles {
        match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => eprintln!("herdr-workspace: agent start failed: {e}"),
            Err(_) => eprintln!("herdr-workspace: agent start thread panicked"),
        }
    }
    ExitCode::SUCCESS
}

fn start_one_agent(args: &[String]) -> Result<(), String> {
    // Wait until the form popup is gone, then give Codex time to come up.
    // Agent names are session-global; if the profile name is taken in another
    // workspace, try `name-2`, `name-3`, … so this pane still starts.
    let base = args.get(2).cloned().unwrap_or_default();
    let mut names = agent_name_candidates(&base);
    let mut current = names.next().unwrap_or(base.clone());
    let mut last = String::new();
    for i in 0..90 {
        let mut argv = args.to_vec();
        if argv.len() > 2 {
            argv[2] = current.clone();
        }
        match herdr::json(&argv) {
            Ok(_) => {
                if current != base {
                    eprintln!(
                        "herdr-workspace: started `{current}` because `{base}` was already used"
                    );
                }
                return Ok(());
            }
            Err(e) if herdr::agent_name_taken(&e) => match names.next() {
                Some(next) => {
                    eprintln!("herdr-workspace: `{current}` taken, retrying as `{next}`");
                    current = next;
                }
                None => return Err(e),
            },
            Err(e) if herdr::popup_busy(&e) || retryable_start(&e) => {
                last = e;
                let ms = if i < 10 { 200 } else { 1000 };
                std::thread::sleep(Duration::from_millis(ms));
            }
            Err(e) => return Err(e),
        }
    }
    Err(if last.is_empty() {
        "timed out starting agent".into()
    } else {
        last
    })
}

/// Profile name first, then `name-2` … `name-99`, truncated to 32 chars.
fn agent_name_candidates(base: &str) -> impl Iterator<Item = String> {
    let base = base.to_string();
    std::iter::once(base.clone()).chain((2u32..=99).map(move |n| {
        let suffix = format!("-{n}");
        let keep = 32usize.saturating_sub(suffix.len());
        let mut stem = base.chars().take(keep).collect::<String>();
        while stem.ends_with('-') {
            stem.pop();
        }
        if stem.is_empty() {
            format!("a{suffix}")
        } else {
            format!("{stem}{suffix}")
        }
    }))
}

fn retryable_start(err: &str) -> bool {
    let t = err.to_ascii_lowercase();
    t.contains("timed out")
        || t.contains("timeout")
        || t.contains("not ready")
        || t.contains("shell prompt")
        || t.contains("available")
}

fn origin_cwd() -> String {
    let raw = std::env::var("HERDR_PLUGIN_CONTEXT_JSON").unwrap_or_default();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
        if let Some(cwd) = herdr::str_of(&v, &["focused_pane_cwd"])
            .or_else(|| herdr::str_of(&v, &["workspace_cwd"]))
            .or_else(|| herdr::str_of(&v, &["worktree", "checkout_path"]))
        {
            return cwd;
        }
    }
    std::env::var("HERDR_WORKSPACE_CWD").unwrap_or_default()
}

fn origin_workspace_id() -> String {
    let raw = std::env::var("HERDR_PLUGIN_CONTEXT_JSON").unwrap_or_default();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
        if let Some(id) = herdr::str_of(&v, &["workspace_id"]) {
            return id;
        }
    }
    std::env::var("HERDR_WORKSPACE_ID").unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::agent_name_candidates;

    #[test]
    fn agent_name_candidates_keep_base_then_suffix() {
        let names: Vec<_> = agent_name_candidates("reviewer").take(3).collect();
        assert_eq!(names, ["reviewer", "reviewer-2", "reviewer-3"]);
    }

    #[test]
    fn agent_name_candidates_stay_within_32_chars() {
        let long = "abcdefghijklmnopqrstuvwxyz012345";
        assert_eq!(long.len(), 32);
        let names: Vec<_> = agent_name_candidates(long).take(2).collect();
        assert_eq!(names[0], long);
        assert_eq!(names[1].len(), 32);
        assert!(names[1].ends_with("-2"));
        assert!(names[1].chars().next().unwrap().is_ascii_lowercase());
    }
}
