# Layout profiles

A **profile** is a reusable Herdr workspace layout: tabs, pane splits, pane
labels, optional shell commands, and optional named coding agents.

The New workspace and New worktree popups list one card per profile. Save
creates a workspace (or a Git worktree of the current workspace) and applies
that profile.

## Where the file lives

Profiles are **not** in the plugin checkout. They live in the plugin config
directory:

```bash
herdr plugin config-dir herdr-workspace
```

That is usually `~/.config/herdr/plugins/config/herdr-workspace/`. The file
is `config.yaml`.

On first run, if `config.yaml` is missing, it is copied from
[`config.example.yaml`](../config.example.yaml). After that, only the config
directory file is loaded. Edit that file and reopen the popup — no rebuild.

```
~/.config/herdr/plugins/config/herdr-workspace/config.yaml
```

## File shape

```yaml
profiles:
  - id: pair                 # unique, used internally
    name: Pair               # card title in the popup
    description: |           # card body; optional
      Shell on the left, coding agent on the right.
    tabs:
      - name: main           # Herdr tab label
        panes:
          - id: shell        # local id, unique within this tab
            name: shell      # Herdr pane label; optional
          - id: agent
            name: agent
            split_from: shell
            direction: right
            ratio: 0.5
            agent: pair      # live Herdr agent name
            kind: claude     # herdr agent start --kind
```

Top level is a list under `profiles`. At least one profile is required.
Profile `id` values must be unique in the file.

## Nested structure

```
config.yaml
└── profiles[]
    ├── id
    ├── name
    ├── description?
    └── tabs[]
        ├── name
        └── panes[]
            ├── id
            ├── name?
            ├── split_from?   # required on every pane except the first
            ├── direction?    # right | down  (required with split_from)
            ├── ratio?        # (0, 1)
            ├── command?      # XOR with agent
            ├── agent?        # XOR with command; requires kind
            ├── kind?
            └── args[]
```

### Profile

| Field | Required | Meaning |
| --- | --- | --- |
| `id` | yes | Stable identifier. Unique in the file. Not shown on the card. |
| `name` | yes | Card title. |
| `description` | no | Card subtitle. YAML `\|` is fine for several lines. |
| `tabs` | yes | At least one tab. Created in list order. |

The first tab reuses the workspace's initial tab (renamed). Later tabs are
created with `herdr tab create --no-focus`.

### Tab

| Field | Required | Meaning |
| --- | --- | --- |
| `name` | yes | Herdr tab label. |
| `panes` | yes | At least one pane. First pane is the tab root. |

### Pane

| Field | Required | Meaning |
| --- | --- | --- |
| `id` | yes | Local id, unique **within the tab**. Used as `split_from` target. Not sent to Herdr. |
| `name` | no | Herdr pane label (`herdr pane rename`). If omitted, Herdr keeps the default. |
| `split_from` | all but the first | Earlier pane `id` in the **same tab**. |
| `direction` | with `split_from` | `right` or `down`. |
| `ratio` | no | New pane's share of the split, exclusive `(0, 1)`. Example: `0.5` is half. |
| `command` | no | Text sent to the pane with Enter after layout is built. Mutually exclusive with `agent`. |
| `agent` | no | Live Herdr agent name. Requires `kind`. Mutually exclusive with `command`. |
| `kind` | with `agent` | `herdr agent start --kind` value. |
| `args` | no | Extra argv after `--` on `herdr agent start`. Only valid with `agent`. |

## How splits work

Each tab has exactly one **root pane**: the first entry, with no
`split_from`. That pane is the tab's existing root (from workspace or tab
create).

Every later pane must split from a pane **already listed above it** in the
same tab. Splits are sequential: think “carve this rectangle out of that
existing pane”, not a free-form grid.

```
editor (root)
 ├─ split right 0.45 → agent
 └─ split down  0.35 → logs   (from editor, after the right split)
```

```yaml
panes:
  - id: editor
    name: editor
  - id: agent
    name: agent
    split_from: editor
    direction: right
    ratio: 0.45
  - id: logs
    name: logs
    split_from: editor
    direction: down
    ratio: 0.35
```

Rules:

- `split_from` cannot point at a later pane, another tab, or a missing id.
- The first pane must **not** have `split_from`.
- `direction` is only `right` or `down`.
- `ratio` is the **new** pane's share. Omit it to let Herdr pick a default.

## `command` vs named agents

A pane is one of three things:

1. **Empty shell** — no `command`, no `agent`.
2. **Shell command** — `command` only. Sent with `herdr pane run` after
   splits and renames. Use YAML `|` for multiline text; a trailing newline
   is stripped.
3. **Named agent** — `agent` + `kind`, optional `args`. After the popup
   closes, the plugin runs:

   ```bash
   herdr agent start <workspace-slug>-<agent> --kind <kind> --pane <pane-id> -- <args...>
   ```

   Herdr agent names are unique among **live** agents in the session, not
   per workspace. The plugin prefixes the profile `agent` with a slug of
   the workspace name (`Capehorn Next` → `capehorn-next-reviewer`) so two
   Team workspaces can keep the same role names. Pane `name` is unchanged
   (sidebar label stays `reviewer`). The live target is the prefixed name:

   ```bash
   herdr agent prompt capehorn-next-reviewer "…"
   ```

   The slug is `[a-z0-9-]` from the workspace label; the full name is
   truncated to 32 characters. If that name is still taken, the plugin
   retries `…-2`, `…-3`, …

Do **not** set both `command` and `agent`. If you need flags for Codex or
Grok, put them in `args`, not in `command`.

Profile `agent` must match `[a-z][a-z0-9_-]{0,31}`. That is the role
name; the live Herdr name is `{workspace-slug}-{agent}`. `kind` must be a
Herdr-supported kind, for example:

`pi`, `claude`, `codex`, `gemini`, `cursor`, `devin`, `agy`, `cline`,
`omp`, `mastracode`, `opencode`, `copilot`, `kimi`, `kiro`, `droid`,
`amp`, `grok`, `hermes`, `kilo`, `qodercli`, `qwen`, `maki`.

Run `herdr agent start --help` for the current list.

`agent start` waits until Herdr detects the agent and considers it ready
(default 30s). The plugin starts agents **after** the popup closes so Save
does not hang the form.

## What Save does

For the chosen directory, workspace name, and profile:

1. `herdr workspace create --cwd <dir> --label <name> --focus`
2. For each tab, in order: rename the first tab, or `tab create` later ones
3. For each pane: split if needed, then `pane rename` when `name` is set
4. For each `command` pane: `herdr pane run`
5. Close the popup
6. For each named agent: spawn `herdr agent start …` in the background
7. `herdr workspace focus` on the new workspace

The worktree popup is the same from step 2, but step 1 is:

1. Resolve the latest `base` (`git fetch`, prefer `origin/<name>` when it exists)
2. `herdr worktree create --workspace <current> --branch <name> --base <ref> --label <name> --focus`
3. Fast-forward the new checkout onto that base (`git merge --ff-only`)

## Recipes

### One shell

```yaml
- id: solo
  name: Solo
  description: |
    One tab, one shell in the project directory.
  tabs:
    - name: main
      panes:
        - id: shell
          name: shell
```

### Shell + named agent

```yaml
- id: pair
  name: Pair
  tabs:
    - name: main
      panes:
        - id: shell
          name: shell
        - id: agent
          name: agent
          split_from: shell
          direction: right
          ratio: 0.5
          agent: pair
          kind: claude
```

### Two tabs, two named agents (with extra flags)

```yaml
- id: agents
  name: Agents
  description: |
    worker = Codex, builder = Grok.
  tabs:
    - name: worker
      panes:
        - id: worker
          name: worker
          agent: worker
          kind: codex
          args:
            - -m
            - gpt-5.6-luna
            - -c
            - model_reasoning_effort="medium"
    - name: builder
      panes:
        - id: builder
          name: builder
          agent: builder
          kind: grok
```

That last profile, saved as workspace `demo`, starts:

```bash
herdr agent start demo-worker --kind codex --pane <id> -- -m gpt-5.6-luna -c model_reasoning_effort="medium"
herdr agent start demo-builder --kind grok --pane <id>
```

### Shell command (not an agent)

```yaml
- id: logs
  name: logs
  split_from: editor
  direction: down
  ratio: 0.35
  command: |
    tail -f /tmp/app.log
```

## Validation

The plugin rejects the file (and the popup will not apply it) when:

- `profiles` is empty
- profile `id` or `name` is empty, or `id` is duplicated
- a profile has no tabs, or a tab has no panes / empty `name`
- a pane `id` is empty or duplicated in the same tab
- the first pane has `split_from`, or a later pane is missing it
- `split_from` is not an earlier pane in that tab
- `direction` is not `right` or `down`
- `ratio` is not strictly between 0 and 1
- `agent` is set without `kind`, or `kind` / `args` without `agent`
- `agent` and `command` are both set
- `agent` does not match `[a-z][a-z0-9_-]{0,31}`

## See also

- Seeded examples: [`config.example.yaml`](../config.example.yaml)
- Plugin overview: [`README.md`](../README.md)
