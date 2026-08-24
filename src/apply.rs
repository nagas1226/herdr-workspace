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
        apply_tab(runner, tab, &tab_id, &root, cwd, name, &mut agent_starts)?;
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
    workspace_name: &str,
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
                prefixed_agent_name(workspace_name, agent),
                "--kind".into(),
                kind.to_string(),
                "--pane".into(),
                id.clone(),
            ];
            if !pane.args.is_empty() {
                args.push("--".into());
                args.extend(pane.args.iter().map(|a| unquote_arg(a)));
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

/// Live Herdr names are unique in the session, not per workspace.
/// Prefix the profile role with a slug of the workspace label.
/// `{slug}-{role}`, truncated to 32 chars, matching `[a-z][a-z0-9_-]{0,31}`.
fn prefixed_agent_name(workspace: &str, agent: &str) -> String {
    let slug = workspace_slug(workspace);
    if slug.is_empty() {
        return truncate_agent(agent, 32);
    }
    let max = 32usize;
    if slug.len() + 1 + agent.len() <= max {
        return format!("{slug}-{agent}");
    }
    let role = truncate_agent(agent, (max - 2).min(agent.len()).max(1));
    let room = max.saturating_sub(1 + role.len());
    let mut stem = slug.chars().take(room).collect::<String>();
    while stem.ends_with('-') {
        stem.pop();
    }
    if stem.is_empty() {
        stem.push('w');
    }
    format!("{stem}-{role}")
}

fn workspace_slug(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, 'w');
    }
    if out.is_empty() {
        return "ws".into();
    }
    if !out.chars().next().unwrap().is_ascii_lowercase() {
        out.insert(0, 'w');
    }
    out
}

fn truncate_agent(agent: &str, max: usize) -> String {
    let mut s: String = agent.chars().take(max).collect();
    while s.ends_with('-') {
        s.pop();
    }
    if s.is_empty() {
        "a".into()
    } else {
        s
    }
}

/// YAML `-c key="value"` must become CLI `key=value`. `trim_matches('"')`
/// also eats a trailing quote on `key="value"`, which is worse.
fn unquote_arg(raw: &str) -> String {
    let s = raw.trim();
    if let Some((k, v)) = s.split_once('=') {
        let v = unwrap_quotes(v.trim());
        return format!("{k}={v}");
    }
    unwrap_quotes(s).to_string()
}

fn unwrap_quotes(s: &str) -> &str {
    let b = s.as_bytes();
    if b.len() >= 2 && b[0] == b'"' && b[b.len() - 1] == b'"' {
        &s[1..s.len() - 1]
    } else {
        s
    }
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
            ["agent", "start", "demo-pair", "--kind", "claude", "--pane"]
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
                "agent start demo-worker --kind codex --pane w1:p1 -- -m gpt-5.6-luna -c model_reasoning_effort=medium"
            )),
            "{starts:?}"
        );
        assert!(
            starts
                .iter()
                .any(|c| c.contains("agent start demo-builder --kind grok --pane")),
            "{starts:?}"
        );
    }

    #[test]
    fn team_profile_queues_every_named_agent() {
        let dir = tempfile::tempdir().unwrap();
        let profile = parse_yaml(include_str!(
            "../tests/fixtures/team.yaml"
        ))
        .unwrap()
        .profiles
        .remove(0);
        let mut fake = Fake::new();
        let result = apply_with(&mut fake, dir.path().to_str().unwrap(), "demo", &profile).unwrap();
        let names: Vec<&str> = result
            .agent_starts
            .iter()
            .map(|c| c[2].as_str())
            .collect();
        assert_eq!(
            names,
            [
                "demo-leader",
                "demo-frontend-builder",
                "demo-background-builder",
                "demo-reviewer",
                "demo-research-left",
                "demo-research-right",
                "demo-qa",
            ]
        );
        let frontend = result
            .agent_starts
            .iter()
            .find(|c| c[2] == "demo-frontend-builder")
            .unwrap();
        assert!(frontend.iter().any(|a| a == "-m"));
        assert!(frontend.iter().any(|a| a == "gpt-5.6-luna"));
        assert!(frontend.iter().any(|a| a == "model_reasoning_effort=high"));
        assert!(!frontend.iter().any(|a| a.contains('"')));
        let qa = result
            .agent_starts
            .iter()
            .find(|c| c[2] == "demo-qa")
            .unwrap();
        assert!(qa.iter().any(|a| a == "gpt-5.6-luna"));
        assert!(qa.iter().any(|a| a == "model_reasoning_effort=medium"));
    }

    #[test]
    fn prefixed_agent_name_slugs_workspace_label() {
        assert_eq!(prefixed_agent_name("capehorn-next", "reviewer"), "capehorn-next-reviewer");
        assert_eq!(prefixed_agent_name("Capehorn Next", "frontend-builder"), "capehorn-next-frontend-builder");
        assert_eq!(prefixed_agent_name("telemetry-replay", "reviewer"), "telemetry-replay-reviewer");
        let long = prefixed_agent_name("telemetry-replay", "frontend-builder");
        assert!(long.len() <= 32, "{long}");
        assert!(long.starts_with("telemetry-"));
        assert!(long.contains("frontend"));
        assert!(long.chars().next().unwrap().is_ascii_lowercase());
        assert!(!long.ends_with('-'));
        for (ws, role) in [
            ("capehorn-next", "leader"),
            ("capehorn-next", "frontend-builder"),
            ("capehorn-next", "background-builder"),
            ("capehorn-next", "reviewer"),
            ("capehorn-next", "research-left"),
            ("capehorn-next", "research-right"),
            ("capehorn-next", "qa"),
        ] {
            let n = prefixed_agent_name(ws, role);
            assert!(n.len() <= 32, "{n}");
            assert!(n.starts_with("capehorn-next-"), "{n}");
            assert!(n.ends_with(role) || n.contains("frontend") || n.contains("background"), "{n}");
        }
        assert_eq!(
            prefixed_agent_name("capehorn-next", "reviewer"),
            "capehorn-next-reviewer"
        );
    }

    #[test]
    fn unquote_arg_strips_wrapped_value_not_trailing_quote() {
        assert_eq!(unquote_arg(r#"model_reasoning_effort="high""#), "model_reasoning_effort=high");
        assert_eq!(unquote_arg("-m"), "-m");
        assert_eq!(unquote_arg(r#""gpt-5.6-luna""#), "gpt-5.6-luna");
    }

    #[test]
    fn rejects_missing_dir() {
        let mut fake = Fake::new();
        let err = apply_with(&mut fake, "/no/such/dir", "x", &pair_profile()).unwrap_err();
        assert!(err.contains("does not exist"));
        assert!(fake.calls.is_empty());
    }
}
