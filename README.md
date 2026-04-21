# Manager MCP Server

[![CI](https://github.com/josephwander-arch/manager/actions/workflows/ci.yml/badge.svg)](https://github.com/josephwander-arch/manager/actions/workflows/ci.yml)

Multi-vendor AI orchestration from inside any MCP client. Manager routes
coding, reasoning, and toolchain tasks to **Claude Code**, **OpenAI Codex**,
**Google Gemini CLI**, or **OpenAI GPT API** — based on task shape, historical
success rates, and explicit user choice.

One MCP server. Four backends. Server-side blocking. Durable coordination.

**48 MCP tools** across task orchestration, session management, loafs, templates, analytics, dashboard, and multi-backend coordination.

**Part of [CPC](https://github.com/josephwander-arch) (Copy Paste Compute)** — a multi-agent AI orchestration platform. Related repos: [local](https://github.com/josephwander-arch/local) · [hands](https://github.com/josephwander-arch/hands) · [workflow](https://github.com/josephwander-arch/workflow) · [cpc-paths](https://github.com/josephwander-arch/cpc-paths) · [cpc-breadcrumbs](https://github.com/josephwander-arch/cpc-breadcrumbs)

---

## About

CPC is developed by Joseph Wander, an independent builder exploring multi-agent AI workflows for daily professional use. It is not a funded company, not an incorporated product, and not a managed cloud service — it is a personal infrastructure project made public under Apache-2.0 so others can use, fork, or extend it.

What CPC solves: coordinating multiple AI coding backends (Claude Code, GPT, Codex, Gemini) from a single MCP-aware client so reasoning and implementation happen in separate sandboxes with durable state between them.

What CPC is not: it is not a replacement for Claude Desktop's native tooling, not a SaaS, and not an abstraction over vendor APIs — it is a local Rust binary that slots into any MCP client alongside whatever else you already run.

---

## What's New in v1.4.3

**Reconnect orphaned tasks on restart.** Task subprocesses that survive a Claude Desktop restart are now reconnected to their persistent log files instead of silently stalling. Startup calls `reconnect_orphaned_tasks()` to scan `%LOCALAPPDATA%\manager-mcp\tasks\` for running processes.

### Previous: v1.4.2 — Dashboard port stability

### Previous: v1.4.1 — Path migration + dashboard polish

See [CHANGELOG.md](CHANGELOG.md) for the full history (v1.0.0 through v1.4.1), or browse the [Releases page](https://github.com/josephwander-arch/manager/releases) for per-version binaries and notes.

---

## Dashboard URL

The dashboard runs on a local HTTP server at `http://127.0.0.1:{port}/`. Port is
chosen dynamically at startup (default `9218`, falls back through a 100-port range if busy).

**Three ways to find your dashboard URL:**

1. **File:** `%LOCALAPPDATA%\manager-mcp\dashboard_url.txt` (written on each startup)
2. **MCP tool:** `manager:dashboard_status` returns `{port, running, url}`
3. **MCP tool:** `manager:dashboard_open` launches the dashboard in your default browser

**To pin the port** across restarts:
- Add `CPC_DASHBOARD_PORT=<port>` to the manager server env in `claude_desktop_config.json`

---

## Overview

Manager exists for the **delegate-when-the-task-gets-long heuristic**: if the
implementation needs more than a few dozen lines of code, delegate it to a
coding agent rather than writing it inline in your main conversation. Claude's
context window is for reasoning and orchestration; coding agents have their
own sandboxes and token budgets — let them write code. The exact line count
at which delegation becomes cheaper varies with task complexity and your
per-task token budget; in practice, the threshold tends to sit somewhere in
the 30–40-line range, which is the rule of thumb you'll see repeated in CPC
skill files.

### The meta-agent pattern

What you get when manager is wired up isn't just a router — it's a concrete **meta-agent** architecture. Your primary chat (Claude Desktop, Claude Code, or any MCP client) becomes the orchestrator: it holds the goal-level context, decides what to delegate, when to parallelize, and how to synthesize results. Coding agents become disposable workers with their own sandboxes and token budgets. Manager sits between them as durable infrastructure — it persists task state to disk, tails child process logs, and reconnects surviving subprocesses across restarts (new in v1.4.3).

Practical consequences:

- Your conversation window is freed from implementation detail. You read summaries, not code diffs.
- Failed delegations don't cost orchestration context. Retry with `task_retry` and keep going.
- Long-running work survives client restarts. The meta-agent can step away and come back.
- Parallel subtasks become trivial via `task_run_parallel` — fan out, collect, synthesize.
- Multiple coding backends coexist. Pick Claude Code for multi-step toolchains, Codex for one-shot scripts, Gemini for large-context Q&A, all from one orchestration layer.

The pattern works equally well with Anthropic Cowork for scheduled/autonomous operation — Cowork fills the gap where chat-based meta-agents end (ephemeral context, no scheduling). Use chat for interactive orchestration; use Cowork for background jobs that need to run while you're not watching.

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

## Install

### Windows x64

1. Download `manager-v1.4.3-x64.exe` from the [latest release](https://github.com/josephwander-arch/manager/releases/latest).
2. Rename to `manager.exe` and place in `%LOCALAPPDATA%\CPC\servers\`.
3. Add to your `claude_desktop_config.json`:
   ```json
   {
     "mcpServers": {
       "manager": {
         "command": "%LOCALAPPDATA%\\CPC\\servers\\manager.exe"
       }
     }
   }
   ```
4. Restart Claude Desktop.

---

### Windows ARM64

1. Download `manager-v1.4.3-aarch64.exe` from the [latest release](https://github.com/josephwander-arch/manager/releases/latest).
2. Rename to `manager.exe` and place in `%LOCALAPPDATA%\CPC\servers\`.
3. Add to your `claude_desktop_config.json`:
   ```json
   {
     "mcpServers": {
       "manager": {
         "command": "%LOCALAPPDATA%\\CPC\\servers\\manager.exe"
       }
     }
   }
   ```
4. Restart Claude Desktop.

---

### Prerequisites

- At least one backend CLI installed:
  - **Claude Code**: `claude` CLI
  - **Codex**: `codex` CLI or `OPENAI_API_KEY`
  - **Gemini**: `gemini` CLI or `GEMINI_API_KEY`
  - **GPT**: `OPENAI_API_KEY`

For full per-machine setup (paths, backend CLI auth, toast notifications), see [`docs/per_machine_setup.md`](./docs/per_machine_setup.md).

### Build from Source

```bash
git clone https://github.com/josephwander-arch/manager.git
cd manager
cargo build --release
```

Binary appears at `target/release/manager.exe`. Requires Rust stable toolchain — nightly is not required.

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
| `gemini_direct` *(beta)* | One-shot query to Gemini CLI, no task queue |
| `codex_exec` *(beta)* | Run OpenAI Codex non-interactively with sandbox modes |
| `codex_review` *(beta)* | Run OpenAI Codex code review on uncommitted changes |
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

Manager works standalone — pair it with other CPC MCP servers when you want a larger toolkit. Manager handles delegation and coordination; the other servers handle the tools you're delegating over.

- Pair with [local](https://github.com/josephwander-arch/local) for filesystem, shell, and persistent-session tools on Windows.
- Pair with [hands](https://github.com/josephwander-arch/hands) when delegated tasks need browser or native-UI automation.
- Pair with [workflow](https://github.com/josephwander-arch/workflow) when delegated tasks hit stored APIs that you've already graduated from browser to HTTP.

Manager runs as a **Claude Desktop** MCP server. It is the orchestrator that delegates *to* Claude Code, Codex, and Gemini CLIs — manager should NOT be listed in those CLIs' own MCP configs. Putting manager in `~/.claude/mcp.json` or `~/.codex/config.toml` creates handle retention that prevents clean shutdown when Desktop restarts (symptom: orphaned manager processes requiring Task Manager force-kill). A client-specific example config (`claude_desktop_config.example.json`) ships in this repo. If your client supports Anthropic skill files, you can also load `skills/manager.md` directly for skill-only (no-server) use — handy for planning or read-only review flows.

### First-run tip for Claude clients

If you're running manager inside Claude Desktop, enable **tools always loaded** in that client's tool settings before your first call. Manager exposes a wide tool surface; clients that lazy-load tools sometimes fail to discover the full set on the first invocation. Turning on always-loaded is a one-time toggle that eliminates this class of first-run friction entirely.

### Three configs, three purposes — and why manager lives in only one

If you're using manager with delegation, you'll interact with up to three different MCP config files. They serve distinct purposes and **do not cross-contaminate**: editing one does not affect the others.

**Critical:** manager belongs only in **Claude Desktop's** config. Putting it in `~/.claude/mcp.json` or `~/.codex/config.toml` creates handle retention that prevents clean shutdown when Desktop restarts — the visible symptom is orphan `manager.exe` processes in Task Manager that have to be force-killed.

| File | Used by | Purpose | Put `manager` here? |
|------|---------|---------|---------------------|
| `%APPDATA%\Claude\claude_desktop_config.json` | Claude Desktop app | MCP servers loaded when you open Claude Desktop | ✅ Yes — this is manager's home |
| `~/.claude/mcp.json` (global) or `./.mcp.json` (per-project) | `claude` CLI / Claude Code | MCP servers loaded when Claude Code runs — including delegated sessions | ❌ No — manager delegates *to* this CLI, not from it |
| `~/.codex/config.toml` | `codex` CLI | MCP servers loaded when Codex runs — including delegated sessions | ❌ No — same reason |

**What to put in delegated-agent configs (the `.claude/mcp.json` and `.codex/config.toml` files):** tools the delegated session needs for *its own* work — e.g., `local` if you want shell/git/filesystem beyond the CLI's native tools. Not manager. The delegated session doesn't orchestrate anything; it executes the task manager handed to it.

### Bootstrap the rest of the stack via manager itself

Manager's own `task_submit` is a clean way to install its sibling servers. Once manager is running, delegate a Claude Code task:

> `task_submit with backend claude_code: install hands, local, and workflow from github.com/josephwander-arch/, register them in Claude Desktop config, and verify each one started cleanly.`

The delegated session downloads each binary, places it, edits the config, and verifies startup in its own sandbox. You monitor via `task_status` and collect the result when it reports `health: done`. Manual installs work just as well — use whichever is faster for your setup.

## Requirements

- Windows 10/11 (x64 or ARM64)
- At least one backend CLI installed and authenticated (Claude Code, Codex, Gemini, or GPT)
- Rust stable toolchain (build from source only)

## Failure modes

Manager's orchestration surface has a few predictable failure shapes. Knowing them up front makes debugging faster:

- **Backend CLI not authenticated** — `task_submit` returns quickly (~10s) with `health: auth_error`. Fix: run the backend CLI manually (`claude`, `codex`, `gemini`) and re-authenticate, then retry.
- **Backend CLI not on PATH** — dispatch fails with `health: backend_missing`. Fix: install the CLI and confirm `where <cli>` resolves before retrying.
- **Long-running task silent** — status stays at `running` past your expected window. Check `task_status` first, then inspect `C:\CPC\tasks\<task_id>\transcript.jsonl` for the raw backend output.
- **Breadcrumb orphaned by crashed session** — shows up in `breadcrumb_list` with no recent activity. Use `breadcrumb_adopt` to take it over or `breadcrumb_abort` to close it out.
- **Dashboard stuck on stale state** — refresh the browser; dashboard is view-only and recovers on reload.
- **Orphan manager processes surviving Desktop restart** — if you have to force-kill `manager.exe` in Task Manager after quitting Claude Desktop, check whether manager is listed in `~/.claude/mcp.json` or `~/.codex/config.toml`. If yes, remove it from those configs and restart. Manager belongs only in Claude Desktop's config. When a delegated CLI holds an MCP handle to one of its own dependencies, that dependency cannot exit cleanly on restart, producing orphan processes that accumulate until force-killed.

## Contributing

Issues welcome; PRs considered but this is primarily maintained as part of the CPC stack.

## License

Apache License 2.0. See [LICENSE](LICENSE).

---

## Contact

Joseph Wander
- GitHub: [github.com/josephwander-arch](https://github.com/josephwander-arch/)
- Email: josephwander@gmail.com
