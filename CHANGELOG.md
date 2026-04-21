# Changelog

All notable changes to the Manager MCP Server are documented here.

## [Unreleased]

## [1.4.3] - 2026-04-20

### Added

- `reconnect_orphaned_tasks()` runs at startup and reconnects task records to per-task log files at `%LOCALAPPDATA%\manager-mcp\tasks\{id}\child.log`.

### Fixed

- Task subprocesses that survive a Claude Desktop restart are now reconnected to their persistent log or properly finalized instead of silently stalling. This addresses 5 observed manager-restart orphan incidents from Apr 17-19.

### Migration

- Old task records without `log_path` are marked failed with reason `pre-v1.4.3-no-logfile` on first startup because their orphaned stdout/stderr streams cannot be reconstructed.

## [1.4.2] - 2026-04-20

### Changed

- Default preferred dashboard port changed from `9200` to `9218` to move further above the
  MCP server range (local 9101, hands 9102, workflow 9103, auto 9104) and avoid drift
  observed on setups where 9200 was occasionally occupied.

### Added

- Dashboard URL is now written to `%LOCALAPPDATA%\manager-mcp\dashboard_url.txt` on startup.
  Users who lose the URL can read that file directly without calling MCP tools, starting a
  second Claude session, or searching logs.

### Migration

No action required. If you set `CPC_DASHBOARD_PORT=9218` as a workaround for port drift in
earlier versions, you can remove that setting — the new default does the same thing. Any
explicit value still takes precedence over the default.

### How to find your dashboard URL

Three options, in order of convenience:
1. Read `%LOCALAPPDATA%\manager-mcp\dashboard_url.txt`
2. Call the `manager:dashboard_status` MCP tool — returns `{port, running, url}`
3. Call the `manager:dashboard_open` MCP tool — opens the dashboard in the default browser

## [1.4.1] - 2026-04-20

### Fixed

- **Path migration (portability + PII cleanup)** — `src/main.rs` had 35 hardcoded paths leaking user-home, private workspace, and Google Drive locations. Any user who isn't the original developer got broken build/deploy/git features.
  - `C:\Users\josep\*` (7 occurrences) — resolved via `USERPROFILE` env var with `C:\Users\Default` fallback
  - `C:\rust-mcp\*` (14 occurrences) — resolved via `CPC_WORKSPACE_ROOT` env var with `C:\rust-mcp` fallback
  - `C:\My Drive\Volumes\*` (14 occurrences) — resolved via `cpc_paths::volumes_path()` with hardcoded fallback (same pattern as local v1.2.15)
- **Dashboard: progress bars frozen on done/failed sessions** — Tool-step progress bars now only render animated bars for `running`/`queued` sessions. Completed sessions show a static summary (e.g. `✓ 94 tool steps`); failed sessions show `✗ N tool steps`. Previously, done/failed sessions displayed frozen mid-run progress bars.
- **Dashboard: LOAFS panel always empty** — Panel now falls back to showing active breadcrumb count when no Project Loaf is running, sourced from manager, local, and autonomous server data.
- **Dashboard: COMPLETED TODAY panel always empty** — Fixed data source wiring: now checks `completed` breadcrumb arrays and `archive_today_count` field instead of filtering active-only breadcrumbs (which are by definition incomplete).

### Changed

- **`cpc-paths` dependency bumped v0.1.0 → v0.1.1** — picks up backups path derivation fix.

## [1.4.0] - 2026-04-20

### Fixed

- **JSON-RPC notification envelope** — Notifications (`notifications/cancelled`, `notifications/progress`, `notifications/roots/list_changed`, etc.) no longer receive stray success responses. Previously, only `notifications/initialized` was explicitly handled; all other notification methods fell through to the catch-all arm which returned a `JsonRpcSuccess` with `id: null`. JSON-RPC 2.0 spec requires that notifications MUST NOT receive responses. Claude Desktop's Zod validator rejected these stray responses, producing recurring "Invalid response" toasts. Fix: all `notifications/*` methods now `continue` without responding, plus a belt-and-suspenders guard skips any response where `id` is null.
- **Dashboard port bind retry** — Retry range expanded from 6 ports (preferred..preferred+5) to 100 ports (preferred..preferred+99), with random jitter on the starting offset to avoid collisions when multiple manager instances start concurrently. Makes the dashboard immune to leaked sockets from crashed previous instances. On exhaustion, logs at ERROR level with a `CPC_DASHBOARD_PORT` env var override hint instead of silently returning.

### Changed

- **Upgrade notes** — Existing installs can run the latest installer again or manually swap `manager.exe` from the GitHub release.

## [1.3.9] - 2026-04-19

### Fixed

- **Track 2: codex `--` defensive separator** — All 6 codex argument-building sites now insert `"--"` between CLI flags and the user prompt. Prevents prompts starting with `-` from being parsed as flags by codex CLI, which could cause silent argument mis-routing or exec failures.
- **Option C: embedded-only dashboard** — Removed disk-override path from `dash_root()`. Dashboard HTML is now served exclusively from the compile-time `include_str!` embed. Eliminates the runtime divergence bug where a stale `C:\CPC\dashboard\dashboard.html` override could silently shadow the repo source.
- **Dashboard counter: archive_today_count** — Breadcrumb scorecard now reads `archive_today_count` directly from the status payload instead of re-deriving it client-side (which could undercount).
- **Dashboard counter: em-dash sentinel** — Extractions count shows `—` (em-dash) when the extraction endpoint is unreachable, instead of displaying a stale or zero count that could be mistaken for real data.

## [1.3.8] - 2026-04-18

### Added

- **Active Operations tap panel** -- Dashboard Zone 2 ("Active Operations") now aggregates breadcrumbs from all CPC servers via `active.index.json`, enriched with step details from project JSONL files. Each entry shows a source-server tag, progress bar, elapsed time, and owner. Click/tap any card to expand the full steps list with per-step status (done/current/pending). Deduplicates across sources and falls back to polled server data when available.

## [1.3.7] - 2026-04-17

### Added

- **Live step counter** -- Task cards now show a `[N/M]` tool-step counter parsed from each task's `steps[]` array, replacing the static progress bar for tasks without `[STEP n/N]` output markers. Updates on every dashboard poll tick.
- **Cross-server last-5-tools widget** -- Bottom strip Zone 4 now merges manager's ring buffer with log-tailed `mcp_activity.jsonl` entries plus polled `recent_tool_calls` from all other servers. Shows newest 5 entries in `HH:MM:SS | server | tool_name` format. Purely read-only log tailing, zero latency impact on tool calls.
- **Pending-exe-swap counter** -- Scorecard widget counts `.new` files in the servers directory. Shows "Pending Swaps: N" so you know how many deploy swaps are waiting.
- **GitHub Actions release workflow** -- `v*` tag push builds x64 (windows-latest) + ARM64 (windows-11-arm native) binaries, attaches to draft release as `manager-vX.Y.Z-x64.exe` / `manager-vX.Y.Z-aarch64.exe`.
- **SECURITY.md** -- security policy and reporting instructions.
- **Platform-split install docs** -- README install section split into self-contained Windows x64 and ARM64 sub-sections.

## [1.3.6] - 2026-04-17

### Changed

- **Clippy cleanup** — removed blanket `#![allow(clippy::all)]` suppression from crate root. Replaced with targeted fixes across `main.rs` and `analyzer.rs`: `sort_by` closures rewritten to `sort_by_key` for Rust 1.95+ compatibility, plus additional lint-targeted fixes throughout.

## [1.3.5] - 2026-04-17

### Fixed

- **Less-liberal restart recovery** — Dead-child tasks whose file was written within 10 minutes are now marked `Orphaned` (not `Failed`), avoiding false failure notifications after clean manager restarts.
- **Notification label override scoped to success** — `notify_title`/`notify_body` overrides now only apply to `Completed` notifications. `Failed`/`Destroyed` always use default labels so users aren't misled by a success-oriented custom message.
- **Per-reason notification icons** — Failed tasks now show `[Error]` and cancelled tasks show `[Warning]` instead of the generic `[Info]` icon. New `notify_with_icon` trait method with backward-compatible default.
- **Recovery notify no longer blocks MCP init** — PowerShell toast subprocesses during restart recovery are now spawned in a background thread, preventing Claude Desktop from killing the manager during the init handshake.
- **Restart-recovery persists status to disk** — Previously, when restart-recovery marked a task as Orphaned or Failed, only the in-memory state was updated; the JSON file on disk still showed "running". On next restart, manager would re-read "running", redo the transition, and re-fire the notify toast, producing a notify-storm on every restart for any failed task. Now `persist_task()` is called immediately after each recovery status transition so the on-disk state matches memory.

## [1.3.4] - 2026-04-17

### Added

- **Loaf auto-advance on task complete** — When a task transitions to `Done` and its prompt contains the injected `Phase: {name}.` marker matching the active loaf's current phase, the phase is automatically marked `done` (with `completed_at` + `completed_by_task`) and `current_phase` increments. If the completed phase was the last one, the whole loaf is marked `completed`. Append-only breadcrumb events record every auto-advance for audit trail. New helper: `auto_advance_loaf_on_task_complete(task)`. Wired at all 6 task-completion sites (GPT normal/early-exit, Codex normal, CLI normal, CLI spawn-fail, recovery). Only fires on `Done` — failures never advance phases.

### Changed

- **Dashboard default port moved from 9100 to 9200** — Other MCP servers (local=9101, hands=9102, workflow=9103, and other CPC servers on 9104) were stealing ports 9101-9104, causing manager to fall through to 9105 and confusing dashboard bookmarks. 9200 is clean and manager-dedicated. `CPC_DASHBOARD_PORT` and `CPC_MANAGER_PORT` env vars still override.
- **Dashboard: "Active Breadcrumb" widget renamed to "Active Breadcrumbs"** — The widget already iterated over all active breadcrumbs (via `.map(b => ...)`), but the singular label made it look like a single-item widget. Also added explicit newest-first sort by `started_at`/`created_at`.

### Fixed

- None (v1.3.3 held up cleanly overnight; this release is features + cleanup)

## [1.3.3] - 2026-04-17

### Added

- **Restart-recovery task notify (Opus review B1)** — When `Server::new()` marks a task Failed during startup recovery (child PID dead, or legacy task with no PID tracking), the task's `notify_on_fail` flag is now respected. Task IDs marked Failed during recovery are collected into a Vec; after Server construction, the fire loop iterates and calls `check_and_fire_task_notify` using `RealNotifier` directly. Sessions are excluded — session notify still flows through `check_and_fire_session_notify` per the existing session recovery path. Closes the edge case flagged by Opus: manager restart while a notify-flagged task was running no longer produces silent failure.

## [1.3.2] - 2026-04-17

### Fixed

- **CRITICAL: UTF-8 panic in `generate_end_report`** — `task_status` panicked when a task's output contained a multi-byte UTF-8 character (em-dash, arrow, curly quote) at byte offset `len - 500`. Raw byte slice `&out[out.len() - 500..]` at `src/main.rs:1040` could land mid-codepoint. Replaced with a `safe_tail()` helper that walks char boundaries via `char_indices().nth(skip)`. `task_status` on any task with a 500+ character output containing common Unicode characters near the tail now works correctly.

- **BUG (Opus review): Missing notify on GPT early-exit path** — When `OPENAI_API_KEY` is missing and a GPT task fails during its initialization, the task was correctly marked Failed but `check_and_fire_task_notify` was never called. A user submitting `notify_on_fail: true` on a GPT task with no API key configured would get silent failure. Added the missing notify call in `run_gpt_task` after persist+save_to_history.

### Changed

- **Cosmetic (Opus review): serde `skip_serializing_if` on task notify bool fields** — The three `Option<bool>` notify fields in the Task struct (`notify_on_complete`, `notify_on_fail`, `notify_on_destroy`) now use `skip_serializing_if = "Option::is_none"` to match the `Option<String>` notify fields. Tasks without notify flags no longer serialize `"notify_on_complete": null` noise to persisted JSON and dashboard API.

## [1.3.1] - 2026-04-17

### Added

- **Notification hooks on `task_submit`** — The five notify flags (`notify_on_complete`, `notify_on_fail`, `notify_on_destroy`, `notify_title`, `notify_body`) that were previously only available on `session_start` are now also accepted by `task_submit`. When set, a Windows toast notification fires when the task transitions to Done or Failed. Matches the session notification pattern from v1.2.6, extended to background tasks.

### Changed

- **Backend-aware delegation prompt injection** — The CPC delegation context prepended to every task now adapts per backend. Claude Code tasks receive a shorter directive referencing TodoWrite (native) and explicit CPC breadcrumb calls, since Claude Code's native TodoWrite tool already tracks per-step progress. Other backends (GPT, Gemini, Codex) continue to receive the original full directive. Net effect: ~3-7% faster Claude Code task execution with no loss of downstream report quality or CPC breadcrumb coordination.

## [1.3.0] - 2026-04-16

### Fixed

- **Stdin handling fix (StdinMode v2)** — New `StdinMode` enum (`Null` / `Piped`) controls child process stdin. One-shot tasks get `Stdio::null()` (immediate EOF), sessions get `Stdio::piped()` for `send()` follow-ups. Resolves stdin reader block that caused task startup stalls on Windows when multiple backends competed for stdio.

- **Reader cleanup hardening (v2)** — Sequential `child.wait()` then 5-second timeout on reader drain. Prevents indefinite hang on Windows pipe close when child exits but stdout/stderr readers haven't finished consuming buffered data.

### Added

- **Stall watchdog** — `TaskStatus::Stalled` state for tasks with no output for `MANAGER_STALL_TIMEOUT_SECS`. Transitions back to `Running` automatically if output resumes. Surfaced in `task_poll` and dashboard.

- **`live_activity` field** — Per-task process tree snapshot (`ActivityEntry`: pid, name, cmd_preview, cpu_percent). Enables dashboard to show what a backend is actually doing.

- **`recent_tool_calls` ring buffer** — Last 50 tool calls with timestamps, session/task association, and duration. Credential-bearing tool names are redacted. Shared with dashboard via `/api/status`.

- **`label` field on TaskSubmitResult** — Optional human-readable label for tasks, shown in dashboard cards and `task_list`.

- **Step progress tracking** — `current_step`, `total_steps`, `current_step_desc` fields on task records. Powers breadcrumb progress bar in dashboard.

- **`effort` field** — Optional effort estimate on task submission, surfaced in dashboard.

- **Dashboard v1.3 patches** — Breadcrumb progress bar, voice health dot, Fix3–Fix6 consolidated (click-to-detail panel fix, 2 Hz render cap, aggregateToolCalls removal, pollServer guard for undefined intervals).

## [1.2.8] - 2026-04-15

### Added

- **CPC Operational Dashboard** — `GET /` serves a dark-theme single-file HTML dashboard. Polls all CPC servers (manager:9100, local:9101, hands:9102, workflow:9103, plus any additional CPC servers on the 9100-series) at configurable intervals (5s fast, 42s slow). Features: health strip, session/task cards with live elapsed timers, breadcrumb progress bars, today's scorecard, per-server service panels, quick action bar.

- **`GET /api/status`** — rich JSON endpoint: session counts (running/queued/done/failed/orphaned), last 20 task details, active loaf, `status_bar` output, version. Used by the dashboard frontend and `live_status.json` writer.

- **`GET /api/config`** — returns port assignments and poll intervals so the frontend self-configures without hardcoded values.

- **`live_status.json` writer** — every 30 seconds, polls all 5 servers and writes `{VOLUMES}/dashboard/live_status.json`. Consumed by React artifacts in Claude.ai via Google Drive for cross-device status visibility.

- **`dashboard_open` MCP tool** — opens the dashboard in the default browser; returns the URL.

- **`dashboard_stop` MCP tool** — aborts the dashboard tokio task and resets port tracking.

- **`dashboard_status` MCP tool** — returns `{running, port, url}` from atomic globals.

- **Port fallback** — if `CPC_DASHBOARD_PORT` (default 9100) is busy, tries 9100–9105 and logs which port was used. Binds `127.0.0.1` only.

### Changed

- `start_dashboard` now reads `CPC_DASHBOARD_PORT` env var (was `CPC_MANAGER_PORT`; backward-compat alias kept). Stores actual bound port in `DASHBOARD_PORT` atomic for tool introspection.
- **`docs/dashboard.md` rewrite** — expanded from 75 to 249 lines. Now covers runtime-decoupled architecture, axum migration, Cache-Control headers, click-to-detail panel, partial-install behavior, live_status.json, MCP tools, troubleshooting, and live-editing guide.

### Fixed

- **UTF-8 encoding** — repaired 19 mojibake characters in `src/main.rs` (corrupted em-dashes, checkmarks, arrows restored to correct Unicode codepoints). No logic changes.

## [1.2.7] - 2026-04-15

### Added

- **`status_bar` multi-breadcrumb format.** When multiple breadcrumbs are active, `status_bar` now shows count + per-project breakdown: `"3 active (batch1: 2, other: 1)"`. Single breadcrumb keeps existing `"active:<name>"` format. Reads from `C:\CPC\state\breadcrumbs\active.index.json` (written by CPC's MCP servers); falls back through legacy `active_operation.json` → legacy JSONL for older installs.

- **Session `orphaned` status.** When manager restarts and finds a session whose child process is still alive (pipes lost on restart), it now marks the session `orphaned` instead of keeping it `running`. `session_list` surfaces `"orphaned": true` for these entries. Orphaned sessions can be destroyed and restarted to recover.

- **`license = "Apache-2.0"` in Cargo.toml** — matches the LICENSE file (was missing the metadata field).

- **Two-Tier Storage docs** — `docs/per_machine_setup.md` now includes a Two-Tier Storage section explaining what belongs on Volumes vs local data, the do-not-sync rules, legacy path compatibility, and cross-machine setup.

### Changed

- `handle_list_sessions` response now includes `"orphaned": bool` field (always present, non-breaking).

### Changed
- Add legacy-fallback path resolution for session directory. Existing `C:\temp\manager-sessions\` (if present with session data) continues to be used; new installs use `cpc_paths::data_path("manager")` default.

### Added
- **`cpc-paths` dependency** (v0.1.0) -- portable path discovery library added as a git dep pinned to tag v0.1.0. No behavior change. Groundwork for future health/diagnostic integration.

## [1.2.6] - 2026-04-15 -- Session Notification Hooks

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

- **status_bar and Gemini breadcrumb injection now prefer local server's breadcrumb state** at `%LOCALAPPDATA%\CPC\state\active_operation.json`, falling back to legacy paths only if the local state directory doesn't exist. Fixes `"bc: unavailable"` for public distribution users running manager + local without additional CPC servers installed. Priority: local → legacy → unavailable.

## [1.2.3] - 2026-04-14

### Fixed

- **task_cancel now kills the child process tree.** Previously, cancellation only updated the task status in the database — the background child process (and any descendants it spawned) continued running. v1.2.3 uses sysinfo to walk the process tree via parent-child relationships, kills descendants bottom-up, then kills the root. Response includes `killed_tree: [pids]` and drops the "may still be running" disclaimer on success.

- **Removed task_submit blocking wait and timeout enforcement.** `wait=true` previously blocked the MCP handler thread polling every 500ms — dangerous for long tasks and a deadlock vector. `timeout_secs` killed tasks that were still working. Both removed. `task_submit` now always returns immediately. `timeout_secs` parameter kept as `estimated_secs` (informational only, no enforcement). Use `task_poll` or `task_watch` instead.

### Added

- **`task_poll` tool.** Returns `{ completed_since: [...], still_running: [...], status_bar: {...} }`. `since` parameter defaults to 1 hour ago. Replaces the `wait=true` polling pattern with an explicit, non-blocking poll.

- **`status_bar` tool.** One-line system summary: `{ manager: "N running, M queued, K unclaimed", breadcrumb: "...", loaf: "...", formatted: "one-line string" }`. Queries the CPC breadcrumb JSONL and active Project Loaf. Returns `"unavailable"` for unreachable sources — never errors.

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
