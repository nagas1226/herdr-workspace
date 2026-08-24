//! Shared popup lifecycle: run a TUI, then start agents after close.

use crate::herdr;
use std::process::{Command, Stdio};

pub fn run(inner: impl FnOnce(&mut Vec<Vec<String>>) -> Result<i32, String>) -> i32 {
    let mut agent_starts = Vec::new();
    let code = match inner(&mut agent_starts) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("herdr-workspace: {e}");
            1
        }
    };
    // Closing the popup kills this process. Hand agent starts to a detached
    // child first so later tabs (build, review, …) still launch.
    if !agent_starts.is_empty() {
        if let Err(e) = spawn_start_agents(&agent_starts) {
            eprintln!("herdr-workspace: {e}");
        }
    }
    close_popup();
    code
}

fn spawn_start_agents(jobs: &[Vec<String>]) -> Result<(), String> {
    let path = std::env::temp_dir().join(format!(
        "herdr-workspace-agents-{}.json",
        std::process::id()
    ));
    let payload = serde_json::to_vec(jobs).map_err(|e| format!("encode agent jobs: {e}"))?;
    std::fs::write(&path, payload).map_err(|e| format!("write {}: {e}", path.display()))?;
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let log = std::env::temp_dir().join("herdr-workspace-agents.log");
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .map_err(|e| format!("open {}: {e}", log.display()))?;
    // Ignore SIGHUP in this process so the child is not killed in the
    // window between fork and setsid when the popup closes.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
    }
    let mut cmd = Command::new(&exe);
    cmd.arg("start-agents")
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(log_file.try_clone().map_err(|e| e.to_string())?)
        .stderr(log_file);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::signal(libc::SIGHUP, libc::SIG_IGN);
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    cmd.spawn()
        .map_err(|e| format!("spawn start-agents: {e}"))?;
    Ok(())
}

fn close_popup() {
    let pane = std::env::var("HERDR_PANE_ID").unwrap_or_default();
    if !pane.is_empty() {
        let _ = herdr::run(["plugin", "pane", "close", pane.as_str()]);
    }
}
