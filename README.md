# Manager MCP Server

Multi-vendor AI orchestration from inside any MCP client. Manager routes
coding, reasoning, and toolchain tasks to **Claude Code**, **OpenAI Codex**,
**Google Gemini CLI**, or **OpenAI GPT API** — based on task shape, historical
success rates, and explicit user choice.

One MCP server. Four backends. Server-side blocking. Durable coordination.

**Part of [CPC](https://github.com/josephwander-arch) (Cognitive Performance Computing)** — a multi-agent AI orchestration platform. Related repos: [local](https://github.com/josephwander-arch/local) · [hands](https://github.com/josephwander-arch/hands) · [workflow](https://github.com/josephwander-arch/workflow) · [cpc-paths](https://github.com/josephwander-arch/cpc-paths) · [cpc-breadcrumbs](https://github.com/josephwander-arch/cpc-breadcrumbs)

---

## What's New in v1.3.5

**StdinMode v2, stall watchdog, and Dashboard v1.3.**

### StdinMode v2 — Stdin Handling Fix

New `StdinMode` enum (`Null` / `Piped`) controls child process stdin. One-shot
tasks get `Stdio::null()` (immediate EOF), sessions get `Stdio::piped()` for
`send()` follow-ups. Resolves stdin reader block that caused task startup stalls
on Windows when multiple backends competed for stdio.

Reader cleanup hardened: sequential `child.wait()` then 5-second timeout on
reader drain prevents indefinite hang on Windows pipe close.

### Stall Watchdog

`TaskStatus::Stalled` state for tasks with no output for
`MANAGER_STALL_TIMEOUT_SECS`. Transitions back to `Running` automatically if
output resumes. Surfaced in `task_poll` and dashboard.

### Dashboard v1.3

- **Breadcrumb progress bar** — visual step tracking in the dashboard
- **Voice health dot** — at-a-glance server status indicator
- **Click-to-detail panel** — click any session/task card to see full prompt and output
- **2 Hz render cap** — smooth, efficient DOM updates
- **pollServer guard** — prevents runaway polling for unconfigured servers

### New Task Fields

| Field | Description |
|-------|-------------|
| `live_activity` | Per-task process tree snapshot (pid, name, cmd_preview, cpu_percent) |
| `recent_tool_calls` | Last 50 tool calls with timestamps and duration (credentials redacted) |
| `label` | Optional human-readable label for tasks, shown in dashboard and `task_list` |
| `current_step` / `total_steps` / `current_step_desc` | Step progress tracking |
| `effort` | Optional effort estimate on task submission |

See [CHANGELOG.md](CHANGELOG.md) for the full release history.

---

<details>
<summary>Older Releases</summary>

### v1.2.8 — Operational Dashboard

- `GET /` serves a dark-theme single-file HTML dashboard polling all servers
- `GET /api/status` — rich JSON endpoint with session counts, task details, loaf state
- `GET /api/config` — port assignments and poll intervals
- `live_status.json` writer for cross-device visibility
- `dashboard_open`, `dashboard_stop`, `dashboard_status` MCP tools
- Port fallback (9100–9105) with `127.0.0.1` binding

### v1.2.7 — Multi-Breadcrumb Status Bar + Session Orphan Detection

- `status_bar` shows count + per-project breakdown for multiple active breadcrumbs
- Session `orphaned` status when manager restarts with live child processes
- `license = "Apache-2.0"` metadata in Cargo.toml
- Two-Tier Storage docs in `per_machine_setup.md`

### v1.2.6 — Session Notification Hooks

Five new optional parameters on `session_start`: `notify_on_complete`,
`notify_on_fail`, `notify_on_destroy`, `notify_title`, `notify_body`. All
default to false. Flags persist to `meta.json` and survive manager restarts.

### v1.2.5 — Per-Server Learning Loop

- `run_analyzer` tool — nightly task performance analyzer with promotion/demotion proposals

### v1.2.3 — Cancel-Kill, Output-as-Timer, Status Bar, Fingerprint Dedup

- `task_cancel` and `session_destroy` now kill the full process tree
- Removed `wait=true` blocking and `timeout_secs` enforcement
- `task_poll` — non-blocking completion polling with status bar
- `status_bar` — one-line system summary
- Fingerprint dedup with stalled-session override
- Session heartbeat with live `alive`/`pid`/`last_activity` fields

### v1.2.1 — Notify + Watchdog Fixes

- `notify` tool — Windows toast notifications
- Watchdog scope fixes for process-tree detection

### v1.2.0 — Ghost-Task Fix

- Tasks with live child PIDs survive manager restart instead of being force-failed
- `child_pid` and `watchdog_observations` fields on task records
- Named-pipe singleton architecture
- Zombie reaper on startup

### v1.1.1 — task_rerun, Health Enum, Stall Fix

- `task_rerun` — re-submit completed tasks with modifications
- `health` enum (9 values) replaces `stall_detected`
- `active_tool_running` boolean on `task_status`
- Stall detector threshold raised to 90s, skips mid-flight tools
- Task lineage fields: `parent_task_id`, `forked_from`, `continuation_of`

### v1.1.0 — Initial Multi-Backend Release

- Multi-backend orchestration (Claude Code, Codex, Gemini, GPT)
- Auto-route, Project Loafs, `task_run_parallel`, `task_watch`
- Archive-first backups, `get_analytics`, session tools, workflow templates

### v1.0.0 — Initial Release

- Claude Code backend support, basic task lifecycle

</details>

---

## Overview

Manager exists because of the **33-line rule**: if a task requires writing
more than ~33 lines of code, delegate it. Claude's context window is for
reasoning and orchestration. Coding agents have their own sandboxes and token
budgets — let them write code.

### Backends

| Backend | Status | Best For |
|---------|--------|----------|
| **Claude Code** | Full support | Multi-step toolchains, iterative implementation, complex refactors — the primary backend |
| **GPT** | Full support | Pure reasoning chains, structured output, classification |
| **Codex** | Compatibility — beta | One-shot script generation. Full functionality planned for v2. |
| **Gemini CLI** | Compatibility — beta | One-shot Q&A, large-context analysis. Full functionality planned for v2. |

### Key Capabilities

- **Auto-routing** — `auto_route=true` picks the best backend per task
- **Server-side blocking** — `task_watch` holds the connection, zero polling
- **Project Loafs** — durable JSON coordination files that survive crashes
- **Archive-first** — file backups before every write, `task_rollback` to restore
- **Analytics** — `get_analytics` shows backend success rates over time
- **Task lineage** — `task_rerun` links new tasks to originals via `parent_task_id`

---

## Installation & Per-Machine Setup

This is a standalone Rust MCP server for Claude Desktop / Claude Code. Each machine that runs the server needs its own copy of the binary plus a few config tweaks.

**Quick install:**
1. Download the right binary from [Releases](https://github.com/josephwander-arch/manager/releases) — `_arm64.exe` for Windows ARM64, `_x64.exe` for x64.
2. Copy to `C:\CPC\servers\manager.exe`.
3. Edit `%APPDATA%\Claude\claude_desktop_config.json` — paste the snippet from [`claude_desktop_config.example.json`](./claude_desktop_config.example.json) into your `mcpServers` object.
4. Restart Claude Desktop.

For full per-machine setup (paths, backend CLI auth, toast notifications), see [`docs/per_machine_setup.md`](./docs/per_machine_setup.md).

A future `cpc-setup.exe` helper will automate this entire process.

### Prerequisites

- At least one backend CLI installed:
  - **Claude Code**: `claude` CLI
  - **Codex**: `codex` CLI or `OPENAI_API_KEY`
  - **Gemini**: `gemini` CLI or `GEMINI_API_KEY`
  - **GPT**: `OPENAI_API_KEY`

### Claude Desktop Configuration

Add to your `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "manager": {
      "command": "C:\\CPC\\servers\\manager.exe",
      "args": []
    }
  }
}
```

See `claude_desktop_config.example.json` for ARM64 + x64 paths.

### Download

Grab the latest binary from the [v1.3.0 release](https://github.com/josephwander-arch/manager/releases/tag/v1.3.0):

- `manager_v1.3.0_windows_x64.exe` — Windows x64
- `manager_v1.3.0_windows_arm64.exe` — Windows ARM64

Place the `.exe` in your MCP server directory and register its path in your client config.

### Build from Source

```bash
git clone https://github.com/josephwander-arch/manager.git
cd manager
cargo build --release
```

Binary appears at `target/release/manager.exe`. Requires Rust stable toolchain — nightly is not required.

### Verify Installation

Run the included doctor script:

```powershell
.\doctor.ps1
```

This checks the binary, backend availability, and task state directory.

---

## Quickstart

### Submit a task

```
task_submit(
  prompt="Write a pytest suite for utils.py covering edge cases",
  auto_route=true
)
```

`auto_route=true` picks the best backend automatically. To pick manually, pass
`backend="claude_code"` (or `codex`, `gemini`, `gpt`) instead.

`task_submit` always returns immediately — use `task_poll` or `task_watch` to
monitor progress.

### Poll for completions

```
task_poll(since="2026-04-14T10:00:00Z")
# Returns: { completed_since: [...], still_running: [...], status_bar: {...} }
```

### Re-run with tweaks

```
task_rerun(
  task_id="task_abc123",
  additional_context="Also handle the empty-array edge case",
  include_files=["tests/edge_cases.py"]
)
```

### Check task health

```
status = task_status(task_id="task_abc123")
# Read status.health — not stall_detected
# "running_long_tool" = backend is working, keep waiting
# "stalled" = actually stuck, consider cancelling
```

### Parallel workflow with dependencies

```
task_run_parallel(
  name="auth refactor",
  steps=[
    { id: "tests",    backend: "claude_code", prompt: "Write unit tests for auth.py" },
    { id: "docs",     backend: "claude_code", prompt: "Write docstrings for auth.py" },
    { id: "refactor", backend: "claude_code", prompt: "Refactor auth.py using new tests", depends_on: ["tests"] }
  ]
)
```

### Monitor with zero polling overhead

```
task_watch(task_ids=["task_1", "task_2"], timeout=600)
```

---

## Tool Reference

### Core Task Tools

| Tool | Purpose |
|------|---------|
| `task_submit` | Submit a one-shot task to a backend (always returns immediately) |
| `task_status` | Check task state, health, and active_tool_running |
| `task_watch` | Server-side block until tasks complete |
| `task_poll` | Poll completions since a timestamp + status_bar summary |
| `task_output` | Retrieve full output of a completed task |
| `task_cancel` | Cancel a running or pending task (kills process tree) |
| `task_retry` | Re-run a failed task with error context injected |
| `task_rerun` | Re-submit a completed task with modifications |
| `task_rollback` | Restore files from pre-task backup |
| `task_explain` | Human-readable summary of what a task did |
| `task_list` | List recent tasks with optional filtering |
| `task_cleanup` | Remove old task records |
| `task_decompose` | Break a complex prompt into a subtask DAG |
| `task_route` | Preview routing decision without submitting |
| `pause_task` | Pause a running or queued task |
| `resume_task` | Resume a paused task |

### Session Tools

| Tool | Purpose |
|------|---------|
| `session_start` | Start a persistent multi-turn session (fingerprint dedup, heartbeat, notify hooks) |
| `session_send` | Send a message to an active session |
| `session_list` | List active sessions with live alive/pid fields |
| `session_destroy` | Kill session process tree and mark cancelled |

### Direct Backend Tools

| Tool | Purpose |
|------|---------|
| `gemini_direct` | One-shot query to Gemini CLI, no task queue |
| `codex_exec` | Run OpenAI Codex non-interactively with sandbox modes |
| `codex_review` | Run OpenAI Codex code review on uncommitted changes |
| `open_terminal` | Open Claude Code in a visible terminal window |

### Project Loaf Tools

| Tool | Purpose |
|------|---------|
| `create_loaf` | Create a coordination file for related subtasks |
| `loaf_update` | Update loaf state |
| `loaf_status` | Read current loaf state |
| `loaf_close` | Finalize a completed loaf |

### Workflow & Template Tools

| Tool | Purpose |
|------|---------|
| `task_run_parallel` | Execute tasks with dependency gates |
| `workflow_run` | Execute a saved workflow template |
| `template_save` | Save a reusable workflow template |
| `template_list` | List saved templates |
| `template_run` | Run a saved template |

### Status & Analytics

| Tool | Purpose |
|------|---------|
| `status_bar` | One-line system summary: manager + breadcrumb + loaf |
| `notify` | Windows toast notification with title, body, icon, duration |
| `get_analytics` | Query historical task performance data |
| `configure` | Update manager settings at runtime |
| `role_create` | Define a named backend role |
| `role_delete` | Remove a role |
| `role_list` | List defined roles |

### Extraction Tools

| Tool | Purpose |
|------|---------|
| `review_extractions` | Review delegation output for patterns |
| `dismiss_extraction` | Dismiss a pending extraction |
| `extract_workflow` | Extract a workflow pattern from task history |

---

## Companion Skill: Manager + Local

If you also run the `local` MCP server, install the **manager-with-local**
skill for breadcrumb-tracked delegation chains. This wraps multi-step
manager orchestrations in local's breadcrumb system for crash recovery,
cross-context resumption, and audit trails.

See `skills/manager-with-local.md` for the full reference.

---

## Examples

- [`examples/delegate_a_coding_task.md`](examples/delegate_a_coding_task.md) — Single-task delegation walkthrough
- [`examples/task_rerun_workflow.md`](examples/task_rerun_workflow.md) — Re-running completed tasks with modifications
- [`examples/parallel_workflow.md`](examples/parallel_workflow.md) — DAG execution with dependency gates
- [`examples/health_enum_interpretation.md`](examples/health_enum_interpretation.md) — Reading the health enum correctly

---

### Prerequisites: log into your coding CLI first

Manager delegates to coding agents by shelling out to their command-line interfaces. **You must install and log into each CLI you want manager to use, before manager can call it.** Manager does not handle authentication — it assumes the CLI is already ready.

- **Claude Code** — run `claude` in PowerShell or your terminal, complete the login flow, confirm it works standalone. Requires an active Claude subscription; manager's usage counts against that subscription.
- **OpenAI Codex CLI** *(beta support)* — install `codex`, log in, verify. Requires an active OpenAI subscription.
- **Gemini CLI** *(beta support)* — install `gemini`, log in, verify. Requires an active Google AI subscription.

Each CLI must be authenticated in a real interactive terminal *before* manager's first delegation call. If you skip this step, manager's first `task_submit` will hang or fail with an auth error from the child process. This is the single most common first-run issue — check it before anything else.

## Compatible With

Works with any MCP client. Common install channels:

- **Claude Desktop** (the main chat app) — add to `claude_desktop_config.json`. See `claude_desktop_config.example.json` in this repo.
- **Claude Code** — add to `~/.claude/mcp.json`, or point your `CLAUDE.md` at `skills/manager.md` to load it as a skill instead.
- **OpenAI Codex CLI** — register via Codex's MCP config, or load the skill directly.
- **Gemini CLI** — register via Gemini's MCP config, or load the skill directly.

**Two install layouts:**

1. **Local folder** — clone or download this repo, then point your client at the local directory or the extracted `.exe` binary.
2. **Installed binary** — grab the `.exe` from the [Releases](https://github.com/josephwander-arch/manager/releases) page, place it wherever you keep your MCP binaries, then register its path in your client's config.

**Also ships as a skill** — if your client supports Anthropic skill files, load `skills/manager.md` directly. Skill-only mode gives you the behavioral guidance without running the server; useful for planning, review, or read-only workflows.

### First-run tip: enable "always-loaded tools"

For the smoothest experience, enable **tools always loaded** in your Claude client settings (Claude Desktop: Settings → Tools, or equivalent in Claude Code / Codex / Gemini). This ensures Claude recognizes the tool surface on first use without needing to re-discover it every session. Most users hit friction on day one because this is off by default.

### Bootstrap the rest of the toolkit *(optional convenience)*

`manager` is not a required install path — each of the other four MCP servers can be installed directly using the steps in Compatible With above. But if you already have `manager` running, you can skip the manual work for the rest.

Once `manager` is running, you can delegate the remaining four installs to a fresh Claude Code session. Ask Claude:

> `task_submit with backend claude_code: install hands, local, echo, and workflow from github.com/josephwander-arch/, register them in Claude Desktop config, and verify each one started cleanly.`

The delegated session handles download, placement, and config updates in its own context — you monitor via `task_status` and pick up the results when it reports `health: done`. Good for users who already have Claude Code installed and want the full stack without manual steps.

## Requirements

- Windows 10/11 (x64 or ARM64)
- At least one backend CLI installed and authenticated (Claude Code, Codex, Gemini, or GPT)
- Rust stable toolchain (build from source only)

## Contributing

Issues welcome; PRs considered but this is primarily maintained as part of the CPC stack.

## License

Apache License 2.0. See [LICENSE](LICENSE).

---

## Donations

If this project saves you time, consider supporting development:

**$NeverRemember** (Cash App)

---

## Contact

Joseph Wander
- GitHub: [github.com/josephwander-arch](https://github.com/josephwander-arch/)
- Email: protipsinc@gmail.com
