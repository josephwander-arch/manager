# CPC Dashboard

Manager includes a built-in HTTP dashboard for monitoring task state,
breadcrumbs, session health, and service status across the CPC stack.

The dashboard is **runtime-decoupled** from the manager binary: HTML, CSS,
and JavaScript can be edited live without rebuilding. See
[Live editing the dashboard](#live-editing-the-dashboard) below.

---

## Opening the dashboard

With manager running, open a browser to:

```
http://localhost:9100
```

Or invoke the MCP tool from any connected client:

```json
{ "tool": "dashboard_open", "arguments": {} }
```

The dashboard auto-refreshes. No hard-refresh (Ctrl+Shift+R) is needed —
responses send `Cache-Control: no-store`, so the browser always fetches
the current state.

---


> **Don't use `file:///C:/CPC/dashboard/dashboard.html`** — that path points to the override file, but the dashboard is served by the manager process over HTTP. Opening it as a static file would show no data (the `/api/status` calls need manager running). Always use `http://localhost:9100`.

## Default ports

| Server   | Default port | Env override                                 |
|----------|--------------|----------------------------------------------|
| manager  | 9100         | `CPC_DASHBOARD_PORT` or `CPC_MANAGER_PORT`   |
| local    | 9101         | `CPC_DASHBOARD_PORT_LOCAL`                   |
| hands    | 9102         | `CPC_DASHBOARD_PORT_HANDS`                   |
| workflow | 9103         | `CPC_DASHBOARD_PORT_WORKFLOW`                |

Ports 9104+ are reserved for additional CPC servers outside the public
distribution; the dashboard discovers them dynamically and falls back
gracefully when they are not running.

Each server binds `127.0.0.1` only. If the primary port is taken, it
tries the next 5 consecutively. Port collisions between servers are
detected client-side via the `server` discriminator field in each
`/api/status` response.

All servers use **axum** for their HTTP endpoints (unified as of
2026-04-16). Previously local/hands/workflow used `tiny-http` which
had keep-alive connection exhaustion issues under browser load — the
axum migration eliminates that class of bug.

---

## Dashboard layout

The dashboard is organized in zones, top to bottom:

**Top strip — service health pills**
One pill per server (manager, local, hands, workflow, plus voice and any
additional CPC servers discovered at runtime). Green dot + version when
reachable. Grey italic "offline" when not. Voice is offline by design
unless voice mode is active.

**Active sessions and tasks**
Cards for each running session/task with:
- Prompt preview (hover shows full prompt)
- Backend (Claude Code / Codex / Gemini)
- Label chip (if task was submitted with a label)
- Step progress bar (parsed from `[STEP n/N]` output patterns)
- Live activity preview (child process name + CPU%, updated every 5s)
- Stalled indicator (amber pulse if no output for 60s+)
- Click any card — right panel swaps to full task detail
- Click another card to switch; click × or press Escape to clear

**Bottom 4-zone strip**
| Zone | Shows |
|------|-------|
| Active Loafs | Project Loafs in progress (multi-task coordination) |
| Active Breadcrumb | Current multi-step operation with step count |
| Completed Today | Summary of finished work |

**Service detail panels** (HANDS, WORKFLOW, and any other servers exposing `/api/status`)
Per-server stats pulled from each server's `/api/status`.

---

## What each panel shows

| Panel | Source | Data |
|-------|--------|------|
| Sessions & Tasks | `manager:9100/api/status` | Running/queued/done/failed tasks, backend, elapsed time, step progress, labels |
| Breadcrumbs | `local:9101/api/status` | Active breadcrumbs with progress bar, staleness, owner |
| Scorecard | manager + local | Running count, done today, failed, orphaned, extractions, .exe.old count |
| Hands | `hands:9102/api/status` | Browser status, current URL, tab count, contexts |
| Workflow | `workflow:9103/api/status` | Credential count, TOTP entries, flows, active watches |

---

## Live editing the dashboard

The dashboard HTML is loaded at request time, not compiled into the
manager binary. This means CSS, HTML, and JS changes take effect on
browser refresh — no rebuild needed.

**Live override file:**
```
C:\CPC\dashboard\dashboard.html
```

If this file exists, manager serves it. If it does not exist, manager
falls back to the embedded version compiled into the binary. Both
paths work — the override is purely for iteration.

**How to edit:**
1. Edit `C:\CPC\dashboard\dashboard.html` in your editor of choice.
2. Refresh the browser tab.
3. Done.

**How to reset to embedded default:**
```
Remove-Item C:\CPC\dashboard\dashboard.html
```
Manager will fall back to its embedded copy on next request.

**How to refresh the override from the embedded version** (after a
manager release that ships a new embedded dashboard):

```powershell
$embedded = Join-Path (Split-Path (Get-Command manager -ErrorAction SilentlyContinue).Source -Parent) "..\src\dashboard_ui.html"
# Or, if building from source:
$embedded = "C:\rust-mcp\manager-mcp\src\dashboard_ui.html"
Copy-Item $embedded C:\CPC\dashboard\dashboard.html -Force
```

---

## The decouple pattern

This runtime-load-with-embedded-fallback is a pattern CPC uses whenever
iteration on a resource is expected to be frequent but rebuilding the
host is expensive.

Applications in CPC:
- Dashboard HTML/CSS/JS (this doc)
- Skill content (reloadable without MCP server restart)
- Knowledge-base routing rules (disk-loaded at startup)
- Operating file templates

**When NOT to use the decouple pattern:** Config schemas, tool
definitions, or anything where drift between compiled and live state
would cause subtle bugs. For those, compile-time inclusion is correct.

---

## Partial install behavior

The dashboard auto-collapses any server that fails 2 consecutive polls
**and has never been seen**. After 10 seconds with only manager + local
running, panels for the missing servers disappear and a footer shows:

```
N additional servers not detected (hands, workflow, ...) — Show all
```

Clicking **Show all** forces all panels visible. If a collapsed server
comes online later, its panel re-appears automatically.

The health strip hides dots for collapsed servers. Only servers that
have responded at least once get a dot.

---

## live_status.json piggyback

Every 30 seconds, manager polls every discovered server endpoint and
writes a snapshot to:

```
C:\My Drive\Volumes\dashboard\live_status.json
```

This file is used by mobile status cards and external monitors that
cannot reach `localhost` directly. It contains the same data as the
browser dashboard, serialized as JSON with a `timestamp` field.

---

## Action bar

| Action | What it does |
|--------|-------------|
| Clean .exe.old | POSTs to `local:9101/api/action/clean_old` — deletes `*.exe.old` from the server install directory |
| GitHub ↗ | Opens the GitHub release page for any CPC server |
| Path copyable | Copies `C:\My Drive\Volumes` to clipboard |

---

## Views

| View | Shows |
|------|-------|
| Full | All panels visible (default) |
| Ops | Hides service panels, shows task/breadcrumb/scorecard only |
| Summary | Scorecard only |

Switch via the view selector in the top-right.

---

## Poll intervals

Manager and local poll every **5 seconds**.
Hands, workflow, and other secondary servers poll every **42 seconds**
(lower priority, heavier payloads).

---

## Troubleshooting

**Dashboard shows old content after a manager update**
Shouldn't happen — `Cache-Control: no-store` prevents browser caching.
If it does, clear `C:\CPC\dashboard\dashboard.html` to force fallback
to the embedded version:
```powershell
Remove-Item C:\CPC\dashboard\dashboard.html
```

**Local / hands / workflow pill shows "offline"**
That server process isn't running. Check `manager:status_bar` for the
server list. If the server is running but still shows offline, check
whether its port is bound by something else:
```powershell
Test-NetConnection localhost -Port 9101  # or 9102, 9103, 9104
```

**Dashboard loads but all data panels are empty**
Manager is up but other servers haven't registered yet. Wait 10
seconds for the first poll cycle to complete, or force a refresh.

**Browser shows a blank page at http://localhost:9100**
Manager isn't running. Verify with:
```powershell
Get-Process manager -ErrorAction SilentlyContinue
```
If not running, start it via your Claude Desktop config or manually.
