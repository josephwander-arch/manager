---
title: "Manager MCP Server — Multi-AI Orchestration for Claude Code, Codex, Gemini, and GPT"
description: "A Rust MCP server that routes tasks across multiple AI backends with parallel execution, workflow templates, and analytics. Build and deploy a custom MCP server for AI agent delegation and task management."
keywords:
  - MCP server
  - model context protocol server
  - AI orchestration
  - multi-AI orchestrator
  - Claude Code automation
  - Codex automation
  - task routing AI
  - AI task management
  - parallel AI execution
  - concurrent AI tasks
  - workflow automation AI
  - AI workflow templates
  - Claude Desktop MCP
  - Claude Code MCP
  - rust mcp server
  - build MCP server rust
  - AI agent delegation
  - multi-agent system
  - Gemini CLI integration
  - OpenAI integration
  - custom MCP server
  - MCP tool server
  - AI backend manager
  - AI task orchestrator
---

# Manager MCP Server: Getting Started Guide

Manager is a Rust MCP server that acts as a multi-AI orchestrator, routing tasks across Claude Code, Codex, Gemini CLI, and GPT through a single unified interface. It exposes 60+ tools over JSON-RPC (stdin/stdout) and handles task routing, parallel AI execution, workflow automation, and execution analytics. If you need AI agent delegation across multiple backends from Claude Desktop or Claude Code, Manager is the MCP tool server that ties them together.

## What Manager Does

Most AI workflows involve a single backend. Manager turns that into a multi-agent system where each backend handles what it does best:

- **Claude Code** and **Codex** handle coding tasks with file system access and safety controls.
- **Gemini CLI** provides an alternative coding backend with Google's models.
- **GPT** (via OpenAI API) handles reasoning, analysis, and text generation tasks.

Manager sits between your MCP client (Claude Desktop, Claude Code, or any model context protocol client) and these backends. When you submit a task, Manager can automatically route it to the best-suited AI backend based on prompt analysis, historical performance, and learned patterns. You can also run concurrent AI tasks in parallel, chain steps into workflows with retry and escalation logic, and save successful patterns as reusable templates.

## Installation and Setup

### Build from Source

Manager is a single Rust binary with no runtime dependencies beyond the AI backends themselves.

```bash
git clone https://github.com/your-org/manager-mcp.git
cd manager-mcp
cargo build --release
```

The compiled binary lands at `target/release/manager.exe` (Windows) or `target/release/manager` (Linux/macOS).

### Configure for Claude Desktop

Add Manager to your Claude Desktop MCP configuration at `%APPDATA%/Claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "manager": {
      "command": "C:/path/to/manager.exe",
      "args": []
    }
  }
}
```

### Configure for Claude Code

Register Manager as an MCP server in Claude Code with the CLI:

```bash
claude mcp add manager /path/to/manager.exe
```

Or add it directly to `~/.claude/settings.json`:

```json
{
  "mcpServers": {
    "manager": {
      "command": "/path/to/manager.exe",
      "args": []
    }
  }
}
```

### Environment Variables

Manager auto-detects installed backends on first run. Override any path with environment variables:

| Variable | Default | Purpose |
|----------|---------|---------|
| `MANAGER_DATA_DIR` | `%LOCALAPPDATA%\manager-mcp` | Base data directory for tasks, patterns, and state |
| `CLAUDE_CODE_CMD` | Auto-detect `~/.local/bin/claude.exe` | Path to Claude Code binary |
| `CODEX_CMD` | Auto-detect from npm global install | Path to Codex binary |
| `GEMINI_CMD` | Auto-detect from npm global install | Path to Gemini CLI entry point |
| `NODE_CMD` | Auto-detect `Program Files\nodejs\node.exe` | Node.js binary (for JS-based backends) |
| `MANAGER_TASKS_DIR` | `{data_dir}\tasks` | Task state storage |
| `MANAGER_WORKFLOW_DIR` | `{data_dir}\workflow_patterns` | Saved workflow templates |
| `MANAGER_PATTERNS_DIR` | `{data_dir}\learned_patterns` | Learned execution patterns |

## Usage Examples

All interaction happens through MCP tool calls. Below are JSON-RPC examples for the key operations.

### Submit a Task to a Specific Backend

Route a coding task directly to Claude Code automation:

```json
{
  "tool": "task_submit",
  "arguments": {
    "backend": "claude_code",
    "prompt": "Refactor the error handling in src/main.rs to use thiserror",
    "working_dir": "C:/projects/my-app",
    "role": "implementer"
  }
}
```

This returns a `task_id` immediately. The task executes asynchronously in the background.

### Auto-Route a Task

Let Manager decide the best AI backend based on prompt analysis and historical performance:

```json
{
  "tool": "task_submit",
  "arguments": {
    "prompt": "Analyze the trade-offs between async-std and tokio for this project",
    "auto_route": true
  }
}
```

You can also ask for a routing recommendation without executing:

```json
{
  "tool": "task_route",
  "arguments": {
    "prompt": "Write unit tests for the authentication module"
  }
}
```

### Poll for Results

Check task status and retrieve output once complete:

```json
{
  "tool": "task_status",
  "arguments": { "task_id": "abc-123" }
}
```

```json
{
  "tool": "task_output",
  "arguments": { "task_id": "abc-123", "tail": 50 }
}
```

### Run Parallel AI Execution

Execute concurrent AI tasks across multiple backends with dependency gates. Steps sharing a `parallel_group` start simultaneously, while `depends_on` enforces ordering:

```json
{
  "tool": "task_run_parallel",
  "arguments": {
    "name": "full-stack-review",
    "steps": [
      {
        "id": "frontend",
        "backend": "claude_code",
        "prompt": "Review src/components/ for accessibility issues",
        "parallel_group": "review"
      },
      {
        "id": "backend",
        "backend": "codex",
        "prompt": "Review src/api/ for security vulnerabilities",
        "parallel_group": "review"
      },
      {
        "id": "summary",
        "backend": "claude_code",
        "prompt": "Combine these reviews into a single report: {{frontend.output}} {{backend.output}}",
        "depends_on": ["frontend", "backend"]
      }
    ],
    "max_concurrent": 3,
    "fail_fast": false
  }
}
```

### Run a Multi-Step Workflow

Chain tasks with automatic retry and backend escalation. If a step fails on its primary backend, Manager retries and then escalates to alternatives:

```json
{
  "tool": "workflow_run",
  "arguments": {
    "name": "build-test-deploy",
    "steps": [
      {
        "id": "build",
        "backend": "claude_code",
        "prompt": "Run cargo build --release in the project",
        "working_dir": "C:/projects/my-app",
        "on_success": "test",
        "max_retries": 2,
        "alternatives": ["codex"]
      },
      {
        "id": "test",
        "backend": "claude_code",
        "prompt": "Run cargo test and report results. Previous build output: {{previous_output}}",
        "working_dir": "C:/projects/my-app"
      }
    ]
  }
}
```

### Save and Reuse Templates

Save a successful workflow as a reusable AI workflow template, then replay it later:

```json
{
  "tool": "template_save",
  "arguments": {
    "name": "rust-build-test",
    "description": "Build and test a Rust project",
    "steps": [
      { "id": "build", "backend": "claude_code", "prompt": "cargo build --release in {{project_dir}}" },
      { "id": "test", "backend": "claude_code", "prompt": "cargo test in {{project_dir}}" }
    ],
    "parameters": ["project_dir"]
  }
}
```

```json
{
  "tool": "template_run",
  "arguments": {
    "name": "rust-build-test",
    "params": { "project_dir": "C:/projects/my-app" }
  }
}
```

## Configuration Reference

Call `configure` with no arguments to view current settings, or pass fields to update them:

```json
{
  "tool": "configure",
  "arguments": {
    "openai_api_key": "sk-...",
    "default_backend": "claude_code",
    "gpt_model": "o3"
  }
}
```

**Supported backends:** `gpt`, `gemini`, `claude_code`, `codex`

**Task roles:** `architect`, `implementer`, `tester`, `reviewer`, `documenter`, `debugger`, `security` -- each injects a role-specific system prompt for more focused output.

## Architecture Overview

Manager runs as a single-process MCP server communicating over JSON-RPC on stdin/stdout. Internally it maintains an async task queue, spawning backend processes as needed:

```
Claude Desktop / Claude Code / Any MCP Client
        |
        | JSON-RPC (stdin/stdout)
        v
   manager.exe  (Rust, async Tokio runtime)
        |
   +----+--------+----------+--------+
   |             |          |        |
Claude Code   Codex    Gemini CLI   GPT
 (CLI)        (CLI)     (CLI)      (API)
```

Key internal components:

- **Task queue** -- tracks submitted tasks with status (queued, running, done, failed, cancelled, paused), output capture, and timing.
- **Router** -- analyzes prompts against keyword patterns, historical success rates, and learned patterns to recommend the optimal backend.
- **Parallel executor** -- manages concurrent AI tasks with dependency resolution, group scheduling, and configurable concurrency limits.
- **Workflow engine** -- chains steps with output forwarding (`{{previous_output}}`), retry logic, and backend escalation on failure.
- **Pattern extractor** -- reviews completed tasks for reusable workflow patterns and saves them for future use.
- **Web dashboard** -- optional built-in HTTP server for monitoring task status and backend health in a browser.

All task state, workflow templates, and learned patterns persist to disk under `MANAGER_DATA_DIR`, so nothing is lost across restarts. The server is stateless at the protocol level -- every tool call is self-contained, and clients can reconnect at any time.

## Additional Tools

Beyond the core task lifecycle, Manager provides tools for direct backend access (`codex_exec`, `codex_review`, `gemini_direct`), interactive sessions (`session_start`, `session_send`), execution analytics (`get_analytics`), multi-step tracked jobs called "loaves" (`create_loaf`, `loaf_update`, `loaf_close`), and pattern extraction from task history (`extract_workflow`, `review_extractions`). Run `task_list` to see all active and completed tasks, or `template_list` to browse saved AI workflow templates.

For the complete tool reference with input schemas, see the [README](../README.md).
