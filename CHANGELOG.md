# Changelog

All notable changes to the Manager MCP Server are documented here.

## [Unreleased]

### Added
- **`cpc-paths` dependency** (v0.1.0) — portable path discovery library added as a git dep pinned to tag v0.1.0. No behavior change. Groundwork for future health/diagnostic integration.

## [1.2.6] - 2026-04-15 — Session Notification Hooks

### Added

- **`session_start`: 5 new notification parameters** — `notify_on_complete`, `notify_on_fail`, `notify_on_destroy`, `notify_title`, `notify_body`. All default to false/null (non-breaking). Fully backwards-compatible — old callers see no change.

- **Heartbeat notify hook** — when the 30-second heartbeat detects a session has entered a terminal state, it reads the notification flags from `meta.json` and fires the appropriate toast (`notify_on_complete` for `Done`, `notify_on_fail` for `Failed` or `Cancelled`).

- **`session_destroy` notify hook** — fires `notify_on_destroy` toast *before* killing the process tree, so the notification has time to show.

- **Meta persistence** — all 5 notify fields are written to `{SESSION_DIR}/{id}/meta.json` at session creation. Flags survive manager restarts: if manager is killed while a session runs, the restart-time heartbeat still reads the flags correctly when it later detects session death.

- **`SessionNotifier` trait + `RealNotifier` / `TestNotifier`** — notification abstraction injected via `Server.notifier: Arc<dyn SessionNotifier>`. Enables unit-testing the notify paths without firing real Windows toasts.

- **`do_notify()` helper** — extracted from `handle_notify` as a reusable `fn do_notify(title, body, icon, duration_ms) -> Result<(), String>`. `handle_notify` now delegates to it.

- **6 unit tests** covering: normal-exit notify, crash notify, destroy notify, defaults-fire-nothing, custom title/body override, and meta persistence surviving a manager restart simulation.

## [1.2.5] - 2026-04-15 — Track B: Per-Server Learning Loop

### Added
- **`run_analyzer` tool** — Nightly task performance analyzer. Reads `task_history.json`, computes per-backend metrics (success rate, p50/p95 duration, avg cost, retry rate), detects inflection points, and writes promotion/demotion proposals to `Volumes/inbox/` for human review. Never auto-modifies routing logic — proposals only. Scheduled via Windows Task Scheduler at 03:45 daily.
- **`src/analyzer.rs`** — Standalone analyzer module with `BackendMetrics` struct, 7-day vs 14-day comparison windows, and inflection point detection.

## [1.2.1] - 2026-04-15 — Phase C Fix3

### Added
- **`notify` tool** — Windows toast notifications with title, body, icon (info/warning/error), and configurable duration. Use for background task completion alerts and status updates.

### Fixed
- **Watchdog scope fixes** — improved process-tree detection edge cases

## [1.2.4] - 2026-04-14

### Fixed

- **status_bar and Gemini breadcrumb injection now prefer local server's breadcrumb state** at `%LOCALAPPDATA%\CPC\state\active_operation.json`, falling back to autonomous paths only if the local state directory doesn't exist. Fixes `"bc: unavailable"` for public distribution users running manager + local without autonomous installed. Priority: local → autonomous → unavailable.

## [1.2.3] - 2026-04-14

### Fixed

- **task_cancel now kills the child process tree.** Previously, cancellation only updated the task status in the database — the background child process (and any descendants it spawned) continued running. v1.2.3 uses sysinfo to walk the process tree via parent-child relationships, kills descendants bottom-up, then kills the root. Response includes `killed_tree: [pids]` and drops the "may still be running" disclaimer on success.

- **Removed task_submit blocking wait and timeout enforcement.** `wait=true` previously blocked the MCP handler thread polling every 500ms — dangerous for long tasks and a deadlock vector. `timeout_secs` killed tasks that were still working. Both removed. `task_submit` now always returns immediately. `timeout_secs` parameter kept as `estimated_secs` (informational only, no enforcement). Use `task_poll` or `task_watch` instead.

### Added

- **`task_poll` tool.** Returns `{ completed_since: [...], still_running: [...], status_bar: {...} }`. `since` parameter defaults to 1 hour ago. Replaces the `wait=true` polling pattern with an explicit, non-blocking poll.

- **`status_bar` tool.** One-line system summary: `{ manager: "N running, M queued, K unclaimed", breadcrumb: "...", loaf: "...", formatted: "one-line string" }`. Queries autonomous breadcrumb JSONL and active Project Loaf. Returns `"unavailable"` for unreachable sources — never errors.

- **Fingerprint dedup with stalled override.** Before queuing, computes `fingerprint = hash(backend, prompt[:200], working_dir)`. If an active task matches: reject with `{ status: "duplicate", existing_task_id }` when last activity was within 120s. If match has no activity for 120s+, allow the new submission and mark the old task `superseded_by: <new_id>` (flagged for reap). New params: `allow_duplicate: bool` on `task_submit`, `include_stalled: bool` on `task_list`. New task fields: `fingerprint`, `superseded_by`.

- **`session_destroy` tool.** Kills the session's process tree (same as `task_cancel`), marks cancelled, updates `meta.json`. Returns `killed_tree: [pids]`.

- **Session fingerprint dedup.** `session_start` now computes the same fingerprint as `task_submit` and rejects healthy duplicates (active within 120s). Stalled sessions are superseded. `allow_duplicate: bool` param available.

- **Session heartbeat.** Async 30s loop syncs `child_pid` and `alive` from the task store into `meta.json`. `session_list` now returns authoritative `alive`/`pid`/`last_activity` fields from the task store instead of stale creation-time values. Fixes bug where `session_list` always reported `alive: false, pid: null` for live sessions.

- **`include_stalled` filter on `session_list`.** Same as `task_list` — when true, only returns sessions with no activity for 120s+.

- **Integration tests.** `test_kill_process_tree_spawns_and_kills` (spawns cmd/ping, kills, verifies via tasklist), `test_compute_fingerprint_deterministic`, `test_compute_fingerprint_truncates_prompt`.

---
## [1.2.0] - 2026-04-13

### Fixed

- **CRITICAL: Ghost tasks no longer survive manager restart.** Previously, restarting `manager.exe` (or a second instance starting) unconditionally marked all Running/Queued tasks as Failed with the message `"Server restarted while task was running"` — even when the child process was still alive. Tasks that *actually* completed also got stuck in "Running" forever across restarts. v1.2.0 replaces the blanket clobber with per-task PID tracking and smart liveness verification on startup. Tasks with live child PIDs keep running; dead PIDs are marked Failed with a specific observation; legacy tasks (no PID) are marked Failed with `"legacy task, no child_pid"`.

### Added

- **`child_pid` field on task records.** CLI tasks persist the spawned child process PID to disk. This enables startup recovery to verify task liveness rather than assuming everything is dead.

- **`watchdog_observations` field on task records.** Read-only telemetry array surfacing manager's observations about task state — restarts, PID liveness checks, singleton takeovers. These observations do NOT mutate task status. They only report what was seen.

- **Named-pipe singleton architecture.** Multiple Claude Desktop worker processes no longer cause competing manager instances. First manager acquires an exclusive lock at `~/AppData/Local/manager-mcp/manager.lock` and binds a named pipe server at `\\.\pipe\cpc-manager`. Subsequent spawns proxy stdio through the pipe and exit when stdio closes.

- **Zombie reaper on startup.** Detects stale `manager.exe` instances from previous Claude Desktop sessions and reaps them via named-pipe health check. Prevents accumulation of orphan manager processes.

### Changed

- **Task status transitions are now driven ONLY by child stdio results or explicit timeouts.** Previous behavior of auto-failing tasks based on "I noticed the manager restarted" has been removed entirely. Architectural principle: observation tools don't mutate state.

### Architecture

This release codifies the **observation-action separation**: watchdog observations are strictly read-only telemetry; only the child-exit code path or explicit cancellation can change `task.status`. This pattern should apply to any future observability features added to manager.

---
## [1.1.1] - 2026-04-11

### Added

- **`task_rerun` tool documented.** Re-submit a completed task with optional
  `additional_context`, `include_files` (array of file paths injected into
  the backend prompt), and `backend_override`. The new task record contains
  a `rerun_of` field pointing to the original, and the original's
  `parent_task_id` is set automatically on the new task. Use this instead
  of writing a new prompt from scratch when a completed task needs another
  pass with tweaks.

- **`health` enum on `task_status`.** New string field with 9 values:
  `done`, `failed`, `queued`, `cancelled`, `paused`, `running_long_tool`,
  `stalled`, `idle`, `running`. This replaces `stall_detected` as the
  field to read for behavior decisions. `stall_detected` remains for
  backward compatibility but `health` distinguishes `running_long_tool`
  (safe to wait) from `stalled` (actually stuck).

- **`active_tool_running` bool on `task_status`.** `true` when the
  backend's most recent step has a `"started"` event with no completion
  event yet. The stall detector reads this field to decide whether to skip.

- **Task lineage fields.** Three new fields on task records:
  `parent_task_id` (populated by `task_rerun`), `forked_from`, and
  `continuation_of`. Only `parent_task_id` is populated in this release.
  Fork and continuation handlers are planned for a follow-up release.

### Fixed

- **Stall detector false positives.** Threshold raised from 30 seconds to
  90 seconds. Detector now skips entirely when a tool is mid-flight
  (`active_tool_running == true`). A Write operation on a 12KB markdown
  file once took 99 seconds between visible step updates and was
  incorrectly flagged as stalled — this no longer happens.

## [1.1.0] - 2026-03-28

### Added

- Initial multi-backend orchestration (Claude Code, Codex, Gemini, GPT)
- Auto-route backend selection
- Project Loafs for durable coordination
- `task_run_parallel` with dependency gates
- Server-side `task_watch` blocking
- Archive-first file backups and `task_rollback`
- `get_analytics` for backend performance tracking
- Session tools for multi-turn interactions
- Workflow templates

## [1.0.0] - 2026-03-01

### Added

- Initial release with Claude Code backend support
- Basic task submission, status, and output retrieval
- Task cancellation and cleanup
