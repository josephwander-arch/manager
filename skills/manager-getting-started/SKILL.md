---
name: manager-getting-started
description: 'Getting started with Manager — the 48-tool multi-AI orchestrator. Use when: delegating code tasks to Claude Code/Codex/Gemini, running parallel AI executions, creating workflow templates, or routing tasks across AI backends.'
---

## What Manager Is

A single MCP server (manager.exe) with 44 tools for multi-AI orchestration. It routes tasks to Claude Code, Codex, Gemini CLI, and GPT, with parallel execution, workflow chaining, and learned routing patterns.

| Category | Key Tools | What It Does |
|----------|-----------|-------------|
| Task lifecycle | task_submit, task_status, task_output, task_poll, task_list | Submit, monitor, and retrieve AI task results |
| Routing | task_route, task_decompose, task_explain | Smart backend selection and task breakdown |
| Parallel | task_run_parallel | Concurrent multi-backend execution with dependencies |
| Workflows | workflow_run | Multi-step chains with retry and escalation |
| Templates | template_save, template_list, template_run | Reusable workflow patterns |
| Sessions | session_start, session_send, session_list | Interactive AI sessions |
| Direct access | codex_exec, codex_review, gemini_direct | Call specific backends directly |
| Analytics | get_analytics, run_analyzer | Execution stats and pattern analysis |
| Loaves | create_loaf, loaf_update, loaf_status, loaf_close | Multi-step tracked jobs |
| Notifications | notify | Desktop notifications for task events |

## Key Tools

| I want to... | Use |
|--------------|-----|
| Delegate a coding task | task_submit(backend="claude_code", prompt="...") |
| Let Manager pick the best AI | task_submit(auto_route=true, prompt="...") |
| Run tasks in parallel | task_run_parallel(steps=[...]) |
| Chain steps with retry | workflow_run(steps=[...]) |
| Save a reusable pattern | template_save(name="...", steps=[...]) |
| Check what backend to use | task_route(prompt="...") |
| Start interactive session | session_start(backend="claude_code") |
| Get execution stats | get_analytics |

## Common Patterns

**Quick delegation:**
manager:task_submit(backend="claude_code", prompt="Add error handling to src/main.rs", working_dir="C:/project")

**Poll for completion:**
manager:task_poll(task_id="abc-123", timeout=60)

**Parallel review:**
manager:task_run_parallel(steps=[
  {id: "frontend", backend: "claude_code", prompt: "Review src/ui/", parallel_group: "review"},
  {id: "backend", backend: "codex", prompt: "Review src/api/", parallel_group: "review"}
])

## Anti-Patterns

- Don't poll in a tight loop — use task_poll with a timeout instead of repeated task_status
- Don't hardcode backends — use auto_route or task_route to let Manager learn which backend works best
