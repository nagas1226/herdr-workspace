//! Create a workspace and apply a layout profile through the herdr CLI.

use crate::config::{PaneSpec, Profile, TabSpec};
use crate::herdr;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

pub trait Runner {
    fn json(&mut self, args: &[String]) -> Result<Value, String>;
}

pub struct HerdrCli;

impl Runner for HerdrCli {
    fn json(&mut self, args: &[String]) -> Result<Value, String> {
        herdr::json(args)
    }
}

/// Layout was created. `agent_starts` are `herdr agent start …` argv lists
/// that should run *after* the form popup closes — `agent start` waits until
/// the agent is ready and would otherwise keep the popup open.
#[derive(Debug, Clone, Default)]
pub struct ApplyResult {
    #[allow(dead_code)]
    pub workspace_id: String,
    pub agent_starts: Vec<Vec<String>>,
}

pub fn apply(cwd: &str, name: &str, profile: &Profile) -> Result<ApplyResult, String> {
    apply_with(&mut HerdrCli, cwd, name, profile)
}

pub fn apply_with<R: Runner>(
    runner: &mut R,
    cwd: &str,
    name: &str,
    profile: &Profile,
) -> Result<ApplyResult, String> {
    let cwd_path = Path::new(cwd);
    if !cwd_path.is_dir() {
        return Err(format!("directory does not exist: {cwd}"));
    }
    if name.trim().is_empty() {
        return Err("workspace name is empty".into());
    }

    let created = runner.json(&vec![
        "workspace".into(),
        "create".into(),
        "--cwd".into(),
        cwd.to_string(),
        "--label".into(),
        name.to_string(),
        "--focus".into(),
    ])?;
    let workspace_id = req(&created, &["result", "workspace", "workspace_id"])?;
    let first_tab = req(&created, &["result", "tab", "tab_id"])?;
    let first_root = req(&created, &["result", "root_pane", "pane_id"])?;
    let mut agent_starts = Vec::new();

    for (i, tab) in profile.tabs.iter().enumerate() {
        let (tab_id, root) = if i == 0 {
            runner.json(&vec![
                "tab".into(),
                "rename".into(),
                first_tab.clone(),
                tab.name.clone(),
            ])?;
            (first_tab.clone(), first_root.clone())
        } else {
            let created_tab = runner.json(&vec![
                "tab".into(),
                "create".into(),
                "--workspace".into(),
                workspace_id.clone(),
                "--cwd".into(),
                cwd.to_string(),
                "--label".into(),
                tab.name.clone(),
                "--no-focus".into(),
            ])?;
            (
                req(&created_tab, &["result", "tab", "tab_id"])?,
                req(&created_tab, &["result", "root_pane", "pane_id"])?,
            )
        };
        apply_tab(runner, tab, &tab_id, &root, cwd, &mut agent_starts)?;
    }

    runner.json(&vec!["workspace".into(), "focus".into(), workspace_id.clone()])?;
    Ok(ApplyResult {
        workspace_id,
        agent_starts,
    })
}

fn apply_tab<R: Runner>(
    runner: &mut R,
    tab: &TabSpec,
    _tab_id: &str,
    root: &str,
    cwd: &str,
    agent_starts: &mut Vec<Vec<String>>,
) -> Result<(), String> {
    let mut ids: HashMap<String, String> = HashMap::new();
    for pane in &tab.panes {
        let herdr_id = if pane.split_from.is_none() {
            root.to_string()
        } else {
            split_pane(runner, pane, &ids, cwd)?
        };
        if let Some(label) = pane.name.as_deref().filter(|s| !s.is_empty()) {
            runner.json(&vec![
                "pane".into(),
                "rename".into(),
                herdr_id.clone(),
                label.to_string(),
            ])?;
        }
        ids.insert(pane.id.clone(), herdr_id);
    }
    for pane in &tab.panes {
        let id = ids.get(&pane.id).ok_or_else(|| format!("missing pane {}", pane.id))?;
        if let Some(agent) = pane.agent.as_deref().filter(|s| !s.is_empty()) {
            let kind = pane
                .kind
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| format!("pane {} agent needs kind", pane.id))?;
            let mut args = vec![
                "agent".into(),
                "start".into(),
                agent.to_string(),
                "--kind".into(),
                kind.to_string(),
                "--pane".into(),
                id.clone(),
            ];
            if !pane.args.is_empty() {
                args.push("--".into());
                args.extend(pane.args.clone());
            }
            agent_starts.push(args);
        } else if let Some(cmd) = pane.command.as_deref().filter(|s| !s.is_empty()) {
            runner.json(&vec![
                "pane".into(),
                "run".into(),
                id.clone(),
                cmd.to_string(),
            ])?;
        }
    }
    Ok(())
}

fn split_pane<R: Runner>(
    runner: &mut R,
    pane: &PaneSpec,
    ids: &HashMap<String, String>,
    cwd: &str,
) -> Result<String, String> {
    let from = pane.split_from.as_deref().unwrap();
    let parent = ids
        .get(from)
        .ok_or_else(|| format!("split_from `{from}` not created yet"))?;
    let dir = pane.direction.as_deref().unwrap_or("right");
    let mut args = vec![
        "pane".into(),
        "split".into(),
        parent.clone(),
        "--direction".into(),
        dir.to_string(),
        "--cwd".into(),
        cwd.to_string(),
        "--no-focus".into(),
    ];
    if let Some(ratio) = pane.ratio {
        args.push("--ratio".into());
        args.push(format!("{ratio}"));
    }
    let created = runner.json(&args)?;
    req(&created, &["result", "pane", "pane_id"])
}

fn req(v: &Value, keys: &[&str]) -> Result<String, String> {
    herdr::str_of(v, keys).ok_or_else(|| format!("missing {}", keys.join(".")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_yaml;

    struct Fake {
        calls: Vec<Vec<String>>,
        splits: usize,
        tabs: usize,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                calls: Vec::new(),
                splits: 0,
                tabs: 0,
            }
        }
    }

    impl Runner for Fake {
        fn json(&mut self, args: &[String]) -> Result<Value, String> {
            self.calls.push(args.to_vec());
            let cmd = args.iter().map(|s| s.as_str()).collect::<Vec<_>>();
            match cmd.as_slice() {
                ["workspace", "create", ..] => Ok(json(
                    r#"{"result":{"workspace":{"workspace_id":"w1"},"tab":{"tab_id":"w1:t1"},"root_pane":{"pane_id":"w1:p1"}}}"#,
                )),
                ["workspace", "focus", "w1"] => Ok(json(r#"{"result":{}}"#)),
                ["tab", "rename", "w1:t1", _] => Ok(json(r#"{"result":{}}"#)),
                ["tab", "create", ..] => {
                    self.tabs += 1;
                    let n = self.tabs + 1;
                    Ok(json(&format!(
                        r#"{{"result":{{"tab":{{"tab_id":"w1:t{n}"}},"root_pane":{{"pane_id":"w1:p{n}0"}}}}}}"#
                    )))
                }
                ["pane", "split", ..] => {
                    self.splits += 1;
                    Ok(json(&format!(
                        r#"{{"result":{{"pane":{{"pane_id":"w1:p1{}"}}}}}}"#,
                        self.splits
                    )))
                }
                ["pane", "rename", ..] | ["pane", "run", ..] => Ok(json(r#"{"result":{}}"#)),
                ["agent", "start", ..] => Ok(json(r#"{"result":{}}"#)),
                other => Err(format!("unexpected {other:?}")),
            }
        }
    }

    fn json(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    fn pair_profile() -> Profile {
        parse_yaml(crate::config::EXAMPLE_YAML)
            .unwrap()
            .profiles
            .into_iter()
            .find(|p| p.id == "pair")
            .unwrap()
    }

    fn full_profile() -> Profile {
        parse_yaml(crate::config::EXAMPLE_YAML)
            .unwrap()
            .profiles
            .into_iter()
            .find(|p| p.id == "full")
            .unwrap()
    }

    #[test]
    fn pair_splits_once_and_runs_agent() {
        let dir = tempfile::tempdir().unwrap();
        let mut fake = Fake::new();
        let result = apply_with(&mut fake, dir.path().to_str().unwrap(), "demo", &pair_profile())
            .unwrap();
        assert_eq!(result.workspace_id, "w1");
        let joined: Vec<String> = fake
            .calls
            .iter()
            .map(|c| c.iter().cloned().collect::<Vec<_>>().join(" "))
            .collect();
        assert!(joined.iter().any(|c| c.starts_with("workspace create")));
        assert!(joined.iter().any(|c| c.contains("pane split w1:p1 --direction right")));
        assert!(!joined.iter().any(|c| c.contains("agent start")));
        assert_eq!(result.agent_starts.len(), 1);
        assert_eq!(
            result.agent_starts[0][..6],
            ["agent", "start", "pair", "--kind", "claude", "--pane"]
        );
        assert!(joined.iter().any(|c| c == "workspace focus w1"));
        assert_eq!(fake.splits, 1);
    }

    #[test]
    fn full_creates_second_tab_and_two_splits() {
        let dir = tempfile::tempdir().unwrap();
        let mut fake = Fake::new();
        apply_with(&mut fake, dir.path().to_str().unwrap(), "demo", &full_profile()).unwrap();
        assert_eq!(fake.splits, 2);
        assert_eq!(fake.tabs, 1);
        let joined: Vec<String> = fake
            .calls
            .iter()
            .map(|c| c.join(" "))
            .collect();
        assert!(joined.iter().any(|c| c.contains("tab create") && c.contains("ops")));
        assert!(joined.iter().any(|c| c.contains("tab rename w1:t1 code")));
    }

    #[test]
    fn named_agent_uses_agent_start() {
        let dir = tempfile::tempdir().unwrap();
        let profile = parse_yaml(
            r#"
profiles:
  - id: agents
    name: Agents
    tabs:
      - name: worker
        panes:
          - id: worker
            name: worker
            agent: worker
            kind: codex
            args: ["-m", "gpt-5.6-luna", "-c", "model_reasoning_effort=\"medium\""]
      - name: builder
        panes:
          - id: builder
            name: builder
            agent: builder
            kind: grok
"#,
        )
        .unwrap()
        .profiles
        .remove(0);
        let mut fake = Fake::new();
        let result = apply_with(&mut fake, dir.path().to_str().unwrap(), "demo", &profile).unwrap();
        let joined: Vec<String> = fake.calls.iter().map(|c| c.join(" ")).collect();
        assert!(!joined.iter().any(|c| c.contains("agent start")), "{joined:?}");
        assert!(!joined.iter().any(|c| c.contains("pane run")));
        assert_eq!(result.agent_starts.len(), 2);
        let starts: Vec<String> = result.agent_starts.iter().map(|c| c.join(" ")).collect();
        assert!(
            starts.iter().any(|c| c.contains(
                "agent start worker --kind codex --pane w1:p1 -- -m gpt-5.6-luna -c model_reasoning_effort=\"medium\""
            )),
            "{starts:?}"
        );
        assert!(
            starts
                .iter()
                .any(|c| c.contains("agent start builder --kind grok --pane")),
            "{starts:?}"
        );
    }

    #[test]
    fn rejects_missing_dir() {
        let mut fake = Fake::new();
        let err = apply_with(&mut fake, "/no/such/dir", "x", &pair_profile()).unwrap_err();
        assert!(err.contains("does not exist"));
        assert!(fake.calls.is_empty());
    }
}
