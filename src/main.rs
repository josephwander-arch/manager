#![recursion_limit = "512"]
//! Manager MCP Server v1.0
//! Multi-AI orchestrator: GPT (reasoning), Gemini CLI (coding), Claude Code (coding)
//! Submit Ã¢â€ â€™ Poll Ã¢â€ â€™ Retrieve pattern for long-running tasks
//!
//! Tools: submit_task, get_status, get_output, list_tasks, cancel_task, configure, retry_task
// NAV: TOC at line 5572 | 97 fn | 16 struct | 2026-04-08

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Read as IoRead, Write};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncReadExt;
use tokio::process::Command as TokioCommand;
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

// Dashboard
use axum::{extract::{Path as AxumPath, Query, State}, http::StatusCode, response::IntoResponse, routing::{get, post}, Json, Router};
use tower_http::cors::{Any, CorsLayer};

// ============================================================================
// Configuration
// ============================================================================

const MAX_HISTORY_ENTRIES: usize = 500;
const GPT_API_URL: &str = "https://api.openai.com/v1/chat/completions";
#[allow(dead_code)] const ROLLBACK_RETENTION_HOURS: i64 = 24;
const DEFAULT_GPT_MODEL: &str = "o3";

use once_cell::sync::Lazy;

fn default_data_dir() -> String {
    std::env::var("MANAGER_DATA_DIR")
        .unwrap_or_else(|_| {
            let local = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| r"C:\ProgramData".to_string());
            format!(r"{}\manager-mcp", local)
        })
}

macro_rules! lazy_path {
    ($name:ident, $env:expr, $default:expr) => {
        static $name: Lazy<String> = Lazy::new(|| std::env::var($env).unwrap_or_else(|_| $default));
    };
}

lazy_path!(_TASKS_DIR, "MANAGER_TASKS_DIR", format!(r"{}\tasks", default_data_dir()));
lazy_path!(_HISTORY_DIR, "MANAGER_HISTORY_DIR", default_data_dir());
lazy_path!(_WORKFLOW_PATTERNS_DIR, "MANAGER_WORKFLOW_DIR", format!(r"{}\workflow_patterns", default_data_dir()));
lazy_path!(_ROLLBACK_DIR, "MANAGER_ROLLBACK_DIR", format!(r"{}\rollback", default_data_dir()));
lazy_path!(_LEARNED_PATTERNS_DIR, "MANAGER_PATTERNS_DIR", format!(r"{}\learned_patterns", default_data_dir()));
lazy_path!(_DASHBOARD_PREFS_PATH, "MANAGER_DASHBOARD_PREFS", format!(r"{}\dashboard_prefs.json", default_data_dir()));
lazy_path!(_LOAVES_DIR, "MANAGER_LOAVES_DIR", format!(r"{}\loaves", default_data_dir()));

static _GEMINI_CMD: Lazy<String> = Lazy::new(|| {
    std::env::var("gemini_cmd()").unwrap_or_else(|_| {
        let npm_root = std::env::var("APPDATA").unwrap_or_default();
        let npm_path = format!(r"{}\npm\node_modules\@google\gemini-cli\dist\index.js", npm_root);
        if std::path::Path::new(&npm_path).exists() { npm_path } else { "gemini".to_string() }
    })
});
static _CLAUDE_CODE_CMD: Lazy<String> = Lazy::new(|| {
    std::env::var("claude_code_cmd()").unwrap_or_else(|_| {
        let home = std::env::var("USERPROFILE").unwrap_or_default();
        let local_path = format!(r"{}\.local\bin\claude.exe", home);
        if std::path::Path::new(&local_path).exists() { local_path } else { "claude".to_string() }
    })
});
static _CODEX_CMD: Lazy<String> = Lazy::new(|| {
    std::env::var("codex_cmd()").unwrap_or_else(|_| {
        let npm_root = std::env::var("APPDATA").unwrap_or_default();
        for arch in &["arm64", "x64"] {
            let p = format!(r"{}\npm\node_modules\@openai\codex\node_modules\@openai\codex-win32-{}\vendor\aarch64-pc-windows-msvc\codex\codex.exe", npm_root, arch);
            if std::path::Path::new(&p).exists() { return p; }
        }
        "codex".to_string()
    })
});
static _NODE_CMD: Lazy<String> = Lazy::new(|| {
    std::env::var("node_cmd()").unwrap_or_else(|_| {
        let pf = r"C:\Program Files\nodejs\node.exe";
        if std::path::Path::new(pf).exists() { pf.to_string() } else { "node".to_string() }
    })
});
static _LOAVES_ARCHIVE_DIR: Lazy<String> = Lazy::new(|| format!(r"{}\archive", &*_LOAVES_DIR));

// Re-export as &str for backwards compat with existing code
const fn _ignore() {}
#[allow(unused_macros)]
macro_rules! path_ref {
    ($static_name:ident) => { &*$static_name }
}

// Accessor functions so existing code compiles with minimal changes
fn tasks_dir() -> &'static str { &_TASKS_DIR }
fn history_dir() -> &'static str { &_HISTORY_DIR }
fn gemini_cmd() -> &'static str { &_GEMINI_CMD }
fn claude_code_cmd() -> &'static str { &_CLAUDE_CODE_CMD }
fn codex_cmd() -> &'static str { &_CODEX_CMD }
fn workflow_patterns_dir() -> &'static str { &_WORKFLOW_PATTERNS_DIR }
fn rollback_dir() -> &'static str { &_ROLLBACK_DIR }
fn learned_patterns_dir() -> &'static str { &_LEARNED_PATTERNS_DIR }
fn node_cmd() -> &'static str { &_NODE_CMD }
#[allow(dead_code)] fn dashboard_prefs_path() -> &'static str { &_DASHBOARD_PREFS_PATH }
fn loaves_dir() -> &'static str { &_LOAVES_DIR }
fn loaves_archive_dir() -> &'static str { &_LOAVES_ARCHIVE_DIR }


#[allow(dead_code)] fn load_terminal_visible() -> bool {
    std::fs::read_to_string(dashboard_prefs_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get("terminal_visible")?.as_bool())
        .unwrap_or(false)
}

/// Spawn a visible terminal window mirroring a background task's CLI command.
/// Fire-and-forget â€” errors are logged but don't affect the background task.
fn spawn_visible_terminal(title: &str, exe: &str, args: &[String], working_dir: &str) {
    // Write full command to temp .bat to avoid cmd/wt quoting hell
    let skip_args: std::collections::HashSet<&str> = ["--output-format", "stream-json"].iter().copied().collect();
    let mut cmd_parts: Vec<String> = vec![exe.to_string()];
    for a in args {
        if skip_args.contains(a.as_str()) { continue; }
        if a.contains(' ') || a.contains('"') || a.contains('\\') {
            cmd_parts.push(format!("\"{}\"", a.replace('"', "\\\"")));
        } else {
            cmd_parts.push(a.clone());
        }
    }
    let cmd_line = format!("@echo off\r\ncd /d {}\r\n{}", working_dir, cmd_parts.join(" "));

    let staging = format!("{}\\CPC\\staging",
        std::env::var("LOCALAPPDATA").unwrap_or_else(|_| r"C:\Users\Public".to_string()));
    let _ = std::fs::create_dir_all(&staging);
    let bat_name = format!("task_{}.bat",
        title.chars().filter(|c| c.is_alphanumeric()).take(20).collect::<String>());
    let bat_path = format!("{}\\{}", staging, bat_name);
    if std::fs::write(&bat_path, &cmd_line).is_err() { return; }

    // Try wt first, fallback to cmd start
    let wt = std::process::Command::new("wt")
        .args(["-w", "0", "new-tab", "--title", title, "cmd", "/K", &bat_path])
        .spawn();
    if wt.is_err() {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", &format!("\"{}\"", title), "cmd", "/K", &bat_path])
            .spawn();
    }
}

// ============================================================================
// MCP Protocol Types
// ============================================================================

#[derive(Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Value,
    method: String,
    params: Option<Value>,
}

#[derive(Serialize)]
struct JsonRpcSuccess {
    jsonrpc: String,
    id: Value,
    result: Value,
}

#[derive(Serialize)]
struct JsonRpcErrorResponse {
    jsonrpc: String,
    id: Value,
    error: JsonRpcError,
}

#[derive(Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

// ============================================================================
// Task Types
// ============================================================================

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Backend {
    Gpt,
    Gemini,
    ClaudeCode,
    Codex,
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Backend::Gpt => write!(f, "gpt"),
            Backend::Gemini => write!(f, "gemini"),
            Backend::ClaudeCode => write!(f, "claude_code"),
            Backend::Codex => write!(f, "codex"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum TaskStatus {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
    Paused,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskStatus::Queued => write!(f, "queued"),
            TaskStatus::Running => write!(f, "running"),
            TaskStatus::Done => write!(f, "done"),
            TaskStatus::Failed => write!(f, "failed"),
            TaskStatus::Cancelled => write!(f, "cancelled"),
            TaskStatus::Paused => write!(f, "paused"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ExtractionStatus {
    None,
    PendingSuccess,
    PendingFailure,
    Extracted,
    Dismissed,
    TooSimple,
}

impl Default for ExtractionStatus {
    fn default() -> Self { ExtractionStatus::None }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum TrustLevel {
    Low,      // 1-3: fire and forget
    Medium,   // 4-6: auto-backup before start
    High,     // 7-10: backup + require diff review
}

impl Default for TrustLevel {
    fn default() -> Self { TrustLevel::Low }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ValidationStatus {
    NotChecked,
    Passed,
    Failed,
    Skipped,
}

impl Default for ValidationStatus {
    fn default() -> Self { ValidationStatus::NotChecked }
}

fn default_max_retries() -> u32 { 2 }

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TaskStep {
    tool: String,
    timestamp: DateTime<Utc>,
    status: String, // "started", "completed", "error"
    summary: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Task {
    id: String,
    backend: Backend,
    prompt: String,
    system_prompt: Option<String>,
    model: Option<String>,
    working_dir: Option<String>,
    status: TaskStatus,
    output: String,
    error: Option<String>,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    progress_lines: usize,
    #[serde(default)]
    steps: Vec<TaskStep>,
    #[serde(default)]
    last_activity: Option<DateTime<Utc>>,
    #[serde(default)]
    stall_detected: bool,
    #[serde(default)]
    extraction_status: ExtractionStatus,
    #[serde(default)]
    pub trust_score: u8,
    #[serde(default)]
    pub trust_level: TrustLevel,
    #[serde(default)]
    pub rollback_path: Option<String>,
    #[serde(default)]
    pub validation_status: ValidationStatus,
    #[serde(default)]
    pub assertions: Vec<String>,
    #[serde(default)]
    pub backed_up_files: Vec<String>,
    #[serde(default)]
    pub retry_count: u32,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default)]
    pub retry_of: Option<String>,
    #[serde(default)]
    pub error_context: Option<String>,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cost_usd: f64,
    #[serde(default)]
    pub on_complete: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub save_artifact: bool,
    #[serde(default)]
    pub rerun_of: Option<String>,
    #[serde(default)]
    pub parent_task_id: Option<String>,
    #[serde(default)]
    pub forked_from: Option<String>,
    #[serde(default)]
    pub continuation_of: Option<String>,
    #[serde(default)]
    pub child_pid: Option<u32>,
    #[serde(default)]
    pub watchdog_observations: Vec<String>,
}

/// Item 16: Task routing intelligence — recommends the best backend for a prompt.
#[derive(Clone, Debug, Serialize)]
struct BackendRecommendation {
    recommended_backend: String,
    confidence: f32,
    reasoning: String,
    alternatives: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[allow(dead_code)]
struct WorkflowStep {
    id: String,
    backend: String,
    prompt: String,
    working_dir: Option<String>,
    on_success: Option<String>,
    #[serde(default)]
    max_retries: Option<u32>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    alternatives: Option<Vec<String>>,
    /// Item 17: IDs of steps that must complete before this step starts
    #[serde(default)]
    depends_on: Vec<String>,
    /// Item 17: Group tag — steps with same parallel_group and no unmet deps start together
    #[serde(default)]
    parallel_group: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
struct WorkflowTemplate {
    name: String,
    description: String,
    #[serde(default)]
    parameters: HashMap<String, String>,
    steps: Vec<TemplateStep>,
    #[serde(default = "default_backend")]
    backend: String,
    #[serde(default = "default_trust_tmpl")]
    trust_level: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    times_used: u32,
    #[serde(default)]
    last_used: String,
    #[serde(default = "default_success_rate")]
    success_rate: f64,
}

#[allow(dead_code)] fn default_backend() -> String { "claude_code".into() }
#[allow(dead_code)] fn default_trust_tmpl() -> String { "auto_with_backup".into() }
#[allow(dead_code)] fn default_success_rate() -> f64 { 1.0 }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
struct TemplateStep {
    id: String,
    prompt: String,
    #[serde(default)]
    backend: Option<String>,
}

// ============================================================================
// Server State
// ============================================================================

struct ServerConfig {
    openai_api_key: Option<String>,
    default_gpt_model: String,
    default_working_dir: String,
}

struct Server {
    tasks: Arc<RwLock<HashMap<String, Task>>>,
    config: Arc<RwLock<ServerConfig>>,
    runtime: tokio::runtime::Handle,
    stdout: Arc<Mutex<io::Stdout>>,
}

impl Server {
    fn new(runtime: tokio::runtime::Handle) -> Self {
        // Try to load OpenAI key from env
        let openai_key = std::env::var("OPENAI_API_KEY").ok();
        if openai_key.is_none() {
            warn!("OPENAI_API_KEY not set - GPT backend will fail until configured");
        }

        // Ensure tasks directory exists
        std::fs::create_dir_all(tasks_dir()).ok();
        std::fs::create_dir_all(history_dir()).ok();

        // Load any persisted tasks
        let mut tasks = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(tasks_dir()) {
            for entry in entries.flatten() {
                if entry.path().extension().map_or(false, |e| e == "json") {
                    if let Ok(data) = std::fs::read_to_string(entry.path()) {
                        if let Ok(task) = serde_json::from_str::<Task>(&data) {
                            // Observe — do NOT clobber Running/Queued tasks as Failed.
                            // The child process may still be alive even though manager restarted.
                            let mut task = task;
                            if task.status == TaskStatus::Running || task.status == TaskStatus::Queued {
                                let obs = format!(
                                    "[{}] Manager restarted — task was {} at load time. Child PID: {}",
                                    Utc::now().format("%H:%M:%S"),
                                    task.status,
                                    task.child_pid.map(|p| p.to_string()).unwrap_or_else(|| "unknown".into())
                                );
                                task.watchdog_observations.push(obs);
                                // Check if child PID is still alive (best-effort)
                                let child_alive = task.child_pid.map(|pid| {
                                    std::process::Command::new("tasklist")
                                        .args(["/FI", &format!("PID eq {}", pid), "/NH"])
                                        .output()
                                        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
                                        .unwrap_or(false)
                                }).unwrap_or(false);
                                if child_alive {
                                    let obs2 = format!(
                                        "[{}] Child PID {} still alive — keeping task status as {}",
                                        Utc::now().format("%H:%M:%S"),
                                        task.child_pid.unwrap(),
                                        task.status
                                    );
                                    task.watchdog_observations.push(obs2);
                                } else if task.child_pid.is_some() {
                                    // Child is confirmed dead with no result — now it's fair to mark failed
                                    let obs2 = format!(
                                        "[{}] Child PID {} is dead — marking task as failed",
                                        Utc::now().format("%H:%M:%S"),
                                        task.child_pid.unwrap()
                                    );
                                    task.watchdog_observations.push(obs2);
                                    task.status = TaskStatus::Failed;
                                    task.error = Some("Child process exited without reporting result (manager restarted)".into());
                                } else {
                                    // No child_pid stored — legacy task from before PID tracking.
                                    // Cannot verify liveness across manager restart, mark failed.
                                    let obs2 = format!(
                                        "[{}] Legacy task (no child_pid stored) — cannot verify liveness across manager restart, marking failed. Restore from DB if child actually completed.",
                                        Utc::now().format("%H:%M:%S"),
                                    );
                                    task.watchdog_observations.push(obs2);
                                    task.status = TaskStatus::Failed;
                                    task.error = Some("Legacy task without PID tracking — cannot verify liveness across manager restart".into());
                                }
                            }
                            tasks.insert(task.id.clone(), task);
                        }
                    }
                }
            }
        }

        Server {
            tasks: Arc::new(RwLock::new(tasks)),
            config: Arc::new(RwLock::new(ServerConfig {
                openai_api_key: openai_key,
                default_gpt_model: DEFAULT_GPT_MODEL.to_string(),
                default_working_dir: r"C:\Users\josep".to_string(),
            })),
            runtime,
            stdout: Arc::new(Mutex::new(io::stdout())),
        }
    }

    /// Write a JSON-RPC message to stdout (shared across threads).
    fn write_stdout(&self, msg: &str) {
        if let Ok(mut out) = self.stdout.lock() {
            let _ = writeln!(out, "{}", msg);
            let _ = out.flush();
        }
    }

    /// Send an MCP log notification (no id, no response expected).
    /// These show in Claude Code's status area, costing zero LLM tokens.
    fn send_notification(&self, level: &str, message: &str) {
        let notif = json!({
            "jsonrpc": "2.0",
            "method": "notifications/message",
            "params": {
                "level": level,
                "logger": "manager",
                "data": message
            }
        });
        self.write_stdout(&serde_json::to_string(&notif).unwrap_or_default());
    }

    fn persist_task(task: &Task) {
        let path = format!("{}\\{}.json", tasks_dir(), task.id);
        if let Ok(data) = serde_json::to_string_pretty(task) {
            std::fs::write(path, data).ok();
        }
    }

    fn save_to_history(task: &Task) {
        let history_path = format!("{}\\task_history.json", history_dir());
        let prompt_summary: String = if task.prompt.len() > 100 {
            safe_truncate(&task.prompt, 100)
        } else { task.prompt.clone() };
        let entry = json!({
            "task_id": task.id,
            "backend": task.backend.to_string(),
            "status": task.status.to_string(),
            "prompt_summary": prompt_summary,
            "step_count": task.steps.len(),
            "steps": task.steps.iter().map(|s| json!({"tool": s.tool, "status": s.status})).collect::<Vec<Value>>(),
            "output_preview": safe_truncate(&task.output, 500),
            "error": task.error,
            "created_at": task.created_at.to_rfc3339(),
            "started_at": task.started_at.map(|s| s.to_rfc3339()),
            "completed_at": task.completed_at.map(|s| s.to_rfc3339()),
            "duration_secs": task.started_at.and_then(|s| task.completed_at.map(|c| (c - s).num_seconds())),
        });

        let mut history: Vec<Value> = std::fs::read_to_string(&history_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        history.retain(|e| e.get("task_id").and_then(|v| v.as_str()) != Some(&task.id));
        history.push(entry);

        if history.len() > MAX_HISTORY_ENTRIES {
            let drain = history.len() - MAX_HISTORY_ENTRIES;
            history.drain(..drain);
        }

        if let Ok(data) = serde_json::to_string_pretty(&history) {
            std::fs::write(&history_path, data).ok();
        }
    }

    /// Item 13/14: Flag task for extraction review based on outcome
    fn flag_extraction(task: &mut Task) {
        match task.status {
            TaskStatus::Done if task.steps.len() >= 3 => {
                task.extraction_status = ExtractionStatus::PendingSuccess;
            }
            TaskStatus::Failed if task.steps.len() >= 1 => {
                task.extraction_status = ExtractionStatus::PendingFailure;
            }
            _ => {
                task.extraction_status = ExtractionStatus::TooSimple;
            }
        }
    }

    /// Calculate trust score from prompt content. Higher = riskier = more safeguards.
    fn calculate_trust(task: &mut Task) {
        let prompt_lower = task.prompt.to_lowercase();
        let mut score: u8 = 1;

        // File operations
        if prompt_lower.contains("delete") || prompt_lower.contains("remove") || prompt_lower.contains("rm ") { score += 3; }
        if prompt_lower.contains("overwrite") || prompt_lower.contains("replace") { score += 2; }
        if prompt_lower.contains("format") || prompt_lower.contains("drop") { score += 4; }

        // Git operations
        if prompt_lower.contains("git push") || prompt_lower.contains("force push") { score += 3; }
        if prompt_lower.contains("git reset --hard") { score += 4; }

        // System operations
        if prompt_lower.contains("registry") || prompt_lower.contains("regedit") { score += 5; }
        if prompt_lower.contains("install") || prompt_lower.contains("uninstall") { score += 2; }

        // Scope amplifiers
        if prompt_lower.contains("all files") || prompt_lower.contains("recursive") { score += 2; }
        if prompt_lower.contains("production") || prompt_lower.contains("deploy") { score += 1; }

        // Cap at 10
        score = score.min(10);

        task.trust_score = score;
        task.trust_level = match score {
            1..=3 => TrustLevel::Low,
            4..=6 => TrustLevel::Medium,
            7..=10 => TrustLevel::High,
            _ => TrustLevel::Low,
        };
    }

    /// Create rollback backups for files mentioned in the prompt.
    fn create_rollback(task: &mut Task) {
        if task.trust_level == TrustLevel::Low { return; }

        let rollback_dir = format!(r"{}\{}", rollback_dir(), task.id);
        let _ = std::fs::create_dir_all(&rollback_dir);

        // Extract file paths from prompt (Windows paths like C:\...\file.ext)
        let path_regex = match regex::Regex::new(r#"[A-Za-z]:\\[^\s,;'"]+\.\w+"#) {
            Ok(r) => r,
            Err(_) => return,
        };

        let mut backed_up = Vec::new();
        for m in path_regex.find_iter(&task.prompt) {
            let path = m.as_str();
            if std::path::Path::new(path).exists() {
                let filename = std::path::Path::new(path)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| "unknown".into());
                let backup_path = format!(r"{}\{}", rollback_dir, filename);
                if std::fs::copy(path, &backup_path).is_ok() {
                    backed_up.push(path.to_string());
                }
            }
        }

        if !backed_up.is_empty() {
            task.rollback_path = Some(rollback_dir);
            task.backed_up_files = backed_up;
        }
    }

    /// Restore backed up files on failure.
    fn rollback(task: &Task) -> Result<Vec<String>, String> {
        let rollback_dir = task.rollback_path.as_ref()
            .ok_or("No rollback data for this task")?;

        let mut restored = Vec::new();
        for original_path in &task.backed_up_files {
            let filename = std::path::Path::new(original_path)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();
            let backup_path = format!(r"{}\{}", rollback_dir, filename);
            if std::path::Path::new(&backup_path).exists() {
                match std::fs::copy(&backup_path, original_path) {
                    Ok(_) => restored.push(original_path.clone()),
                    Err(e) => return Err(format!("Failed to restore {}: {}", original_path, e)),
                }
            }
        }
        Ok(restored)
    }

    /// Infer assertions from the prompt and validate after task completion.
    fn validate_output(task: &mut Task) {
        let prompt_lower = task.prompt.to_lowercase();
        let mut assertions: Vec<String> = Vec::new();

        // Extract file paths that should exist after creation
        let path_regex = match regex::Regex::new(r#"[Cc]reate\s+(?:a\s+file\s+(?:at\s+)?)?([A-Za-z]:\\[^\s,;'"]+\.\w+)"#) {
            Ok(r) => r,
            Err(_) => { task.validation_status = ValidationStatus::Skipped; return; }
        };
        for cap in path_regex.captures_iter(&task.prompt) {
            if let Some(path) = cap.get(1) {
                assertions.push(format!("file_exists:{}", path.as_str()));
            }
        }

        // Check for build commands -> expect binary
        if prompt_lower.contains("cargo build") && prompt_lower.contains("--release") {
            let pkg_regex = regex::Regex::new(r"-p\s+(\S+)").ok();
            if let Some(re) = pkg_regex {
                if let Some(cap) = re.captures(&task.prompt) {
                    if let Some(pkg) = cap.get(1) {
                        assertions.push(format!(r"file_exists:C:\rust-mcp\target\release\{}.exe", pkg.as_str()));
                    }
                }
            }
        }

        // Run assertions
        if assertions.is_empty() {
            task.validation_status = ValidationStatus::Skipped;
            task.assertions = assertions;
            return;
        }

        let mut all_passed = true;
        let mut checked_assertions = Vec::new();
        for assertion in &assertions {
            if let Some(path) = assertion.strip_prefix("file_exists:") {
                let passed = std::path::Path::new(path).exists();
                checked_assertions.push(format!("{}:{}", assertion, if passed { "PASS" } else { "FAIL" }));
                if !passed { all_passed = false; }
            }
        }

        task.assertions = checked_assertions;
        task.validation_status = if all_passed { ValidationStatus::Passed } else { ValidationStatus::Failed };

        // Item 18: Clean up rollback backup when validation passes
        if all_passed {
            if let Some(ref rollback_dir) = task.rollback_path {
                if std::fs::remove_dir_all(rollback_dir).is_ok() {
                    info!("Backup cleaned: validation passed for {}", task.id);
                    task.rollback_path = None;
                    task.backed_up_files.clear();
                }
            }
        }
    }

        /// Item 3: Generate smart end report. Success = summary. Failure = step trail.
    fn generate_end_report(task: &Task) -> String {
        match task.status {
            TaskStatus::Done => {
                // Success: last 500 chars of output as summary
                let out = &task.output;
                if out.len() > 500 {
                    format!("âœ“ Task completed ({} steps)\n\n{}", task.steps.len(), &out[out.len()-500..])
                } else {
                    format!("âœ“ Task completed ({} steps)\n\n{}", task.steps.len(), out)
                }
            }
            TaskStatus::Failed => {
                // Failure: step trail + error
                let mut report = format!("âœ— Task failed after {} steps\n\n", task.steps.len());
                report.push_str("Step trail:\n");
                for (i, step) in task.steps.iter().enumerate() {
                    let mark = match step.status.as_str() {
                        "completed" => "âœ“",
                        "error" => "âœ—",
                        _ => "â†’",
                    };
                    report.push_str(&format!("  {} {}. {} ({})\n", mark, i+1, step.tool, step.status));
                }
                if let Some(ref err) = task.error {
                    report.push_str(&format!("\nError: {}\n", err));
                }
                // Last 300 chars of output for context
                let out = &task.output;
                if out.len() > 300 {
                    report.push_str(&format!("\nLast output:\n{}", &out[out.len()-300..]));
                } else if !out.is_empty() {
                    report.push_str(&format!("\nOutput:\n{}", out));
                }
                report
            }
            _ => task.output.clone(),
        }
    }

    /// Returns true if the most recent task step is still "started" (not yet
    /// "completed" or "error"). Used by the inline stall computation in
    /// handle_get_status to avoid false positives during long-running tool
    /// invocations like Write on large files.
    fn active_tool_running(task: &Task) -> bool {
        task.steps.last().map(|s| s.status == "started").unwrap_or(false)
    }

    /// Item 4: Read breadcrumb state for Gemini injection
    fn read_breadcrumb_state() -> Option<String> {
        let bc_dir = format!("{}\\Volumes\\.breadcrumb", 
            std::env::var("GOOGLE_DRIVE_PATH").unwrap_or_else(|_| r"C:\My Drive".to_string()));
        let bc_file = format!("{}\\current.json", bc_dir);
        std::fs::read_to_string(&bc_file).ok().and_then(|s| {
            let v: Value = serde_json::from_str(&s).ok()?;
            let op = v.get("operation")?.as_str()?;
            let steps: Vec<String> = v.get("steps")?.as_array()?.iter()
                .filter_map(|s| s.as_str().map(|x| x.to_string()))
                .collect();
            Some(format!("[CONTEXT: Current operation: {}. Steps completed: {}]", op, steps.join(", ")))
        })
    }

    /// Item 18: Build a retry task from a failed task. Returns the new Task to be inserted into the store.
    /// Updates the original task's output with a retry note.
    fn prepare_retry(task: &mut Task) -> Option<Task> {
        if task.status != TaskStatus::Failed || task.retry_count >= task.max_retries {
            return None;
        }

        let error_text = task.error.clone()
            .or_else(|| {
                let lines: Vec<&str> = task.output.lines().collect();
                let tail: Vec<&str> = lines.iter().rev().take(5).copied().collect();
                Some(tail.into_iter().rev().collect::<Vec<_>>().join("\n"))
            })
            .unwrap_or_else(|| "Unknown error".to_string());

        // Escalate backend if at max retries
        let new_backend = if task.retry_count + 1 >= task.max_retries {
            match task.backend {
                Backend::ClaudeCode => Backend::Codex,
                Backend::Codex => Backend::Gemini,
                Backend::Gemini => Backend::ClaudeCode,
                Backend::Gpt => Backend::Gpt, // no escalation for GPT
            }
        } else {
            task.backend.clone()
        };

        // Extract original prompt (strip any previous retry injection)
        let original_prompt = task.retry_of.as_ref()
            .and_then(|_| task.prompt.split("\n\n--- PREVIOUS ATTEMPT FAILED ---").next().map(String::from))
            .unwrap_or_else(|| task.prompt.clone());

        let new_prompt = format!(
            "{}\n\n--- PREVIOUS ATTEMPT FAILED ---\nError: {}\nAvoid the approach that caused this error. Try a different strategy.",
            original_prompt, error_text
        );

        let original_id = task.retry_of.clone().unwrap_or_else(|| task.id.clone());
        let new_task_id = Uuid::new_v4().to_string()[..8].to_string();

        let new_task = Task {
            id: new_task_id.clone(),
            backend: new_backend,
            prompt: new_prompt,
            system_prompt: task.system_prompt.clone(),
            model: task.model.clone(),
            working_dir: task.working_dir.clone(),
            status: TaskStatus::Queued,
            output: String::new(),
            error: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            progress_lines: 0, steps: Vec::new(), last_activity: None, stall_detected: false, extraction_status: ExtractionStatus::None,
            trust_score: 0, trust_level: TrustLevel::Low, rollback_path: None, validation_status: ValidationStatus::NotChecked, assertions: Vec::new(), backed_up_files: Vec::new(),
            retry_count: task.retry_count + 1,
            max_retries: task.max_retries,
            retry_of: Some(original_id),
            error_context: Some(error_text),
            input_tokens: 0, output_tokens: 0, cost_usd: 0.0,
            on_complete: task.on_complete.clone(),
            role: task.role.clone(),
            save_artifact: task.save_artifact,
            rerun_of: None,
            parent_task_id: None,
            forked_from: None,
            continuation_of: None,
            child_pid: None,
            watchdog_observations: Vec::new(),
        };

        // Note on original task
        task.output.push_str(&format!("\n[Retrying as {}]", new_task_id));
        info!("Retry {}/{} for task {} -> new task {}", task.retry_count + 1, task.max_retries, task.id, new_task_id);

        Some(new_task)
    }

    /// Item 18: Write learned pattern when a retry succeeds.
    fn learn_from_outcome(task: &Task) {
        let original_id = match &task.retry_of {
            Some(id) => id.clone(),
            None => return,
        };

        let original_error = task.error_context.clone().unwrap_or_else(|| "unknown".to_string());
        let output_summary: String = if task.output.len() > 500 {
            task.output[task.output.len()-500..].to_string()
        } else {
            task.output.clone()
        };
        let prompt_pattern: String = task.prompt.chars().take(200).collect();

        let pattern = json!({
            "type": "error_recovery",
            "original_error": original_error,
            "successful_approach": output_summary,
            "backend_original": original_id,
            "backend_successful": task.backend.to_string(),
            "prompt_pattern": prompt_pattern,
            "retry_count": task.retry_count,
            "learned_at": Utc::now().to_rfc3339(),
        });

        let _ = std::fs::create_dir_all(learned_patterns_dir());
        let filename = format!("{}\\{}_{}.json",
            learned_patterns_dir(),
            Utc::now().format("%Y%m%d_%H%M%S"),
            task.id
        );
        if let Ok(data) = serde_json::to_string_pretty(&pattern) {
            if std::fs::write(&filename, &data).is_ok() {
                info!("Learned pattern from retry success: {} -> {}", original_id, task.id);
            }
        }
    }

    /// Item 16: Analyze a prompt and recommend the best AI backend.
    fn recommend_backend(prompt: &str, working_dir: Option<&str>) -> BackendRecommendation {
        let prompt_lower = prompt.to_lowercase();

        // --- Keyword scoring ---
        struct KeywordRule { keywords: &'static [&'static str], backend: &'static str, weight: f32 }
        let rules = [
            KeywordRule { keywords: &["build", "cargo", "compile", "rust", "npm", "install"], backend: "claude_code", weight: 0.8 },
            KeywordRule { keywords: &["edit", "refactor", "complex", "multi-file", "debug"], backend: "claude_code", weight: 0.7 },
            KeywordRule { keywords: &["read", "report", "list", "check", "verify", "simple"], backend: "gemini", weight: 0.6 },
            KeywordRule { keywords: &["search", "google", "find online", "look up"], backend: "gemini", weight: 0.7 },
            KeywordRule { keywords: &["review", "audit", "sandbox", "safe"], backend: "codex", weight: 0.6 },
            KeywordRule { keywords: &["delete", "overwrite", "deploy", "push"], backend: "codex", weight: 0.7 },
            KeywordRule { keywords: &["reason", "analyze", "think", "strategy", "plan"], backend: "gpt", weight: 0.6 },
        ];

        let mut keyword_scores: HashMap<&str, f32> = HashMap::new();
        for rule in &rules {
            let hit_count = rule.keywords.iter().filter(|kw| prompt_lower.contains(**kw)).count();
            if hit_count > 0 {
                let score = rule.weight * (hit_count as f32 / rule.keywords.len() as f32);
                *keyword_scores.entry(rule.backend).or_insert(0.0) += score;
            }
        }

        // Working dir hints
        if let Some(wd) = working_dir {
            let wd_lower = wd.to_lowercase();
            if wd_lower.contains("rust-mcp") || wd_lower.contains("cargo") {
                *keyword_scores.entry("claude_code").or_insert(0.0) += 0.3;
            }
        }

        // --- Historical success rates ---
        let history_path = format!("{}\\task_history.json", history_dir());
        let mut backend_stats: HashMap<String, (u32, u32, u64)> = HashMap::new(); // (success, total, total_steps)

        if let Ok(data) = std::fs::read_to_string(&history_path) {
            if let Ok(history) = serde_json::from_str::<Vec<Value>>(&data) {
                for entry in &history {
                    let be = entry.get("backend").and_then(|v| v.as_str()).unwrap_or("");
                    let status = entry.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    let steps = entry.get("step_count").and_then(|v| v.as_u64()).unwrap_or(0);
                    let stat = backend_stats.entry(be.to_string()).or_insert((0, 0, 0));
                    stat.1 += 1;
                    stat.2 += steps;
                    if status == "done" { stat.0 += 1; }
                }
            }
        }

        // --- Learned error patterns ---
        let mut error_penalties: HashMap<String, f32> = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(learned_patterns_dir()) {
            for entry in entries.flatten() {
                if let Ok(data) = std::fs::read_to_string(entry.path()) {
                    if let Ok(pattern) = serde_json::from_str::<Value>(&data) {
                        let prompt_pattern = pattern.get("prompt_pattern").and_then(|v| v.as_str()).unwrap_or("");
                        // Check if this learned error is relevant to the current prompt
                        let pattern_words: Vec<&str> = prompt_pattern.split_whitespace().take(5).collect();
                        let overlap = pattern_words.iter().filter(|w| prompt_lower.contains(&w.to_lowercase())).count();
                        if overlap >= 2 {
                            let failed_backend = pattern.get("backend_original").and_then(|v| v.as_str()).unwrap_or("");
                            *error_penalties.entry(failed_backend.to_string()).or_insert(0.0) += 0.15;
                        }
                    }
                }
            }
        }

        // --- Combine scores: keyword 60%, history 30%, speed 10% ---
        let all_backends = ["claude_code", "gemini", "codex", "gpt"];
        let speed_scores: HashMap<&str, f32> = [
            ("gemini", 0.9), ("codex", 0.7), ("claude_code", 0.5), ("gpt", 0.4),
        ].into_iter().collect();

        let mut final_scores: Vec<(&str, f32)> = Vec::new();
        for be in &all_backends {
            let kw = keyword_scores.get(*be).copied().unwrap_or(0.0);
            let hist = backend_stats.get(*be).map(|(s, t, _)| {
                if *t == 0 { 0.5 } else { *s as f32 / *t as f32 }
            }).unwrap_or(0.5);
            let spd = speed_scores.get(*be).copied().unwrap_or(0.5);
            let penalty = error_penalties.get(*be).copied().unwrap_or(0.0);
            let score = (kw * 0.6) + (hist * 0.3) + (spd * 0.1) - penalty;
            final_scores.push((be, score));
        }

        final_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let best = final_scores[0];
        let alternatives: Vec<String> = final_scores[1..].iter().map(|(be, _)| be.to_string()).collect();

        // Confidence: how far ahead the winner is
        let confidence = if final_scores.len() >= 2 {
            let gap = best.1 - final_scores[1].1;
            (0.5 + gap).clamp(0.1, 1.0)
        } else {
            0.5
        };

        // Build reasoning
        let mut reasons = Vec::new();
        if let Some(&kw) = keyword_scores.get(best.0) {
            if kw > 0.0 { reasons.push(format!("keyword match score {:.2}", kw)); }
        }
        if let Some((s, t, avg_steps)) = backend_stats.get(best.0) {
            if *t > 0 {
                reasons.push(format!("historical: {}/{} success, avg {:.1} steps", s, t, *avg_steps as f32 / *t as f32));
            }
        }
        if error_penalties.values().any(|&v| v > 0.0) {
            reasons.push("learned error patterns applied".to_string());
        }

        let reasoning = if reasons.is_empty() {
            format!("{} selected as default (no strong signals)", best.0)
        } else {
            format!("{} selected: {}", best.0, reasons.join("; "))
        };

        BackendRecommendation {
            recommended_backend: best.0.to_string(),
            confidence,
            reasoning,
            alternatives,
        }
    }
}

/// Item 18: Spawn execution for a retry task based on its backend.
fn spawn_retry_execution(
    retry_task: &Task,
    tasks: Arc<RwLock<HashMap<String, Task>>>,
    config: Option<Arc<RwLock<ServerConfig>>>,
    handle: &tokio::runtime::Handle,
) {
    let tid = retry_task.id.clone();
    let prompt = retry_task.prompt.clone();
    let wd = retry_task.working_dir.clone().unwrap_or_else(|| r"C:\Users\josep".to_string());
    let model = retry_task.model.clone();

    match retry_task.backend {
        Backend::Gpt => {
            if let Some(cfg) = config {
                handle.spawn(run_gpt_task(cfg, tasks, tid));
            } else {
                info!("Cannot retry GPT task {} - no config available", tid);
            }
        }
        Backend::ClaudeCode => {
            let mut args = vec![
                "-p".to_string(), prompt,
                "--dangerously-skip-permissions".to_string(), "--verbose".to_string(),
                "--output-format".to_string(), "stream-json".to_string(),
                "--add-dir".to_string(), r"C:\temp".to_string(),
                "--add-dir".to_string(), r"C:\My Drive\Volumes".to_string(),
                "--add-dir".to_string(), r"C:\CPC".to_string(),
                "--add-dir".to_string(), r"C:\rust-mcp".to_string(),
                "--add-dir".to_string(), wd,
            ];
            if let Some(m) = model { args.push("--model".to_string()); args.push(m); }
            handle.spawn(run_cli_task(tasks, tid, claude_code_cmd(), args));
        }
        Backend::Codex => {
            let args = vec![
                "exec".into(), "--json".into(), "--skip-git-repo-check".into(),
                "--full-auto".into(), "--cd".into(), wd.clone(), prompt,
            ];
            handle.spawn(run_codex_task(tasks, tid, args, wd));
        }
        Backend::Gemini => {
            let mut args = vec![gemini_cmd().to_string(), "-p".into(), prompt, "--yolo".into()];
            if let Some(m) = model { args.push("--model".to_string()); args.push(m); }
            handle.spawn(run_cli_task(tasks, tid, node_cmd(), args));
        }
    }
}

/// Spawn an on_complete follow-up task when a task finishes successfully.
/// Call after the completion block while still holding access to the tasks arc.
fn spawn_on_complete(
    completed_task: &Task,
    tasks: Arc<RwLock<HashMap<String, Task>>>,
    config: Option<Arc<RwLock<ServerConfig>>>,
    handle: &tokio::runtime::Handle,
) {
    if completed_task.status != TaskStatus::Done { return; }
    let prompt = match completed_task.on_complete {
        Some(ref p) => p.clone(),
        None => return,
    };
    let parent_id = completed_task.id.clone();
    let backend = completed_task.backend.clone();
    let working_dir = completed_task.working_dir.clone();
    let model = completed_task.model.clone();

    let follow_up = Task {
        id: Uuid::new_v4().to_string()[..8].to_string(),
        backend: backend.clone(),
        prompt: format!("[ON_COMPLETE of task {}]\n{}", parent_id, prompt),
        system_prompt: None,
        model,
        working_dir,
        status: TaskStatus::Queued,
        output: String::new(),
        error: None,
        created_at: Utc::now(),
        started_at: None,
        completed_at: None,
        progress_lines: 0, steps: Vec::new(), last_activity: None, stall_detected: false, extraction_status: ExtractionStatus::None,
        trust_score: 0, trust_level: TrustLevel::Low, rollback_path: None, validation_status: ValidationStatus::NotChecked, assertions: Vec::new(), backed_up_files: Vec::new(), retry_count: 0, max_retries: 2, retry_of: None, error_context: None, input_tokens: 0, output_tokens: 0, cost_usd: 0.0,
        on_complete: None,
        role: None,
        save_artifact: false,
        rerun_of: None,
        parent_task_id: None,
        forked_from: None,
        continuation_of: None,
        child_pid: None,
        watchdog_observations: Vec::new(),
    };

    info!("on_complete: spawning follow-up task {} from completed task {}", follow_up.id, parent_id);
    Server::persist_task(&follow_up);
    let tasks_bg = tasks.clone();
    let follow_id = follow_up.id.clone();
    handle.block_on(async {
        let mut store = tasks_bg.write().await;
        store.insert(follow_id.clone(), follow_up.clone());
    });
    spawn_retry_execution(&follow_up, tasks, config, handle);
}

// ============================================================================
// Backend Execution
// ============================================================================

async fn run_gpt_task(
    config: Arc<RwLock<ServerConfig>>,
    tasks: Arc<RwLock<HashMap<String, Task>>>,
    task_id: String,
) {
    // Mark running
    {
        let mut store = tasks.write().await;
        if let Some(task) = store.get_mut(&task_id) {
            task.status = TaskStatus::Running;
            task.started_at = Some(Utc::now());
            Server::calculate_trust(task);
            Server::create_rollback(task);
            Server::persist_task(task);
        }
    }

    // Get task details + config
    let (prompt, system_prompt, model) = {
        let store = tasks.read().await;
        let task = store.get(&task_id).unwrap();
        (task.prompt.clone(), task.system_prompt.clone(), task.model.clone())
    };

    let (api_key, default_model) = {
        let cfg = config.read().await;
        (cfg.openai_api_key.clone(), cfg.default_gpt_model.clone())
    };

    let api_key = match api_key {
        Some(k) => k,
        None => {
            let mut store = tasks.write().await;
            let mut retry_task: Option<Task> = None;
            if let Some(task) = store.get_mut(&task_id) {
                task.status = TaskStatus::Failed;
                task.error = Some("OPENAI_API_KEY not configured. Use configure tool to set it.".into());
                task.completed_at = Some(Utc::now());
                Server::flag_extraction(task);
                // Item 18: retry/learn hooks
                retry_task = Server::prepare_retry(task);
                if task.status == TaskStatus::Done && task.retry_of.is_some() { Server::learn_from_outcome(task); }
                Server::persist_task(task);
                Server::save_to_history(task);
            }
            if let Some(ref rt) = retry_task {
                store.insert(rt.id.clone(), rt.clone());
                Server::persist_task(rt);
            }
            drop(store);
            if let Some(ref rt) = retry_task {
                spawn_retry_execution(rt, tasks.clone(), Some(config.clone()), &tokio::runtime::Handle::current());
            }
            return;
        }
    };

    let model = model.unwrap_or(default_model);

    // Build messages
    let mut messages = Vec::new();
    if let Some(sys) = system_prompt {
        messages.push(json!({"role": "system", "content": sys}));
    }
    messages.push(json!({"role": "user", "content": prompt}));

    let body = json!({
        "model": model,
        "messages": messages,
    });

    // Call OpenAI API
    let client = reqwest::Client::new();
    let result = client
        .post(GPT_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await;

    let mut store = tasks.write().await;
    let mut retry_task: Option<Task> = None;
    let mut completed_snap: Option<Task> = None;
    if let Some(task) = store.get_mut(&task_id) {
        match result {
            Ok(response) => {
                let status_code = response.status();
                match response.text().await {
                    Ok(text) => {
                        if status_code.is_success() {
                            // Parse response
                            if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                                let content = parsed["choices"][0]["message"]["content"]
                                    .as_str()
                                    .unwrap_or("(no content in response)");
                                let usage = &parsed["usage"];
                                let model_used = parsed["model"].as_str().unwrap_or(&model);
                                task.output = format!(
                                    "Model: {}\nTokens: prompt={}, completion={}, total={}\n\n{}",
                                    model_used,
                                    usage["prompt_tokens"].as_u64().unwrap_or(0),
                                    usage["completion_tokens"].as_u64().unwrap_or(0),
                                    usage["total_tokens"].as_u64().unwrap_or(0),
                                    content
                                );
                                task.status = TaskStatus::Done;
                            } else {
                                task.output = text;
                                task.status = TaskStatus::Done;
                            }
                        } else {
                            task.status = TaskStatus::Failed;
                            task.error = Some(format!("HTTP {}: {}", status_code, text));
                        }
                    }
                    Err(e) => {
                        task.status = TaskStatus::Failed;
                        task.error = Some(format!("Failed to read response: {}", e));
                    }
                }
            }
            Err(e) => {
                task.status = TaskStatus::Failed;
                task.error = Some(format!("Request failed: {}", e));
            }
        }
        task.completed_at = Some(Utc::now());
        if task.status == TaskStatus::Done { Server::validate_output(task); }
        Server::flag_extraction(task);
        // Item 18: retry/learn hooks
        if task.status == TaskStatus::Failed { retry_task = Server::prepare_retry(task); }
        if task.status == TaskStatus::Done && task.retry_of.is_some() { Server::learn_from_outcome(task); }
        Server::persist_task(task);
        Server::save_to_history(task);
        save_task_artifact(task);
        completed_snap = Some(task.clone());
    }
    if let Some(ref rt) = retry_task {
        store.insert(rt.id.clone(), rt.clone());
        Server::persist_task(rt);
    }
    drop(store);
    if let Some(ref rt) = retry_task {
        spawn_retry_execution(rt, tasks.clone(), Some(config.clone()), &tokio::runtime::Handle::current());
    }
    if let Some(ref ct) = completed_snap {
        spawn_on_complete(ct, tasks.clone(), Some(config.clone()), &tokio::runtime::Handle::current());
    }
}

/// Codex-specific runner: synchronous capture via spawn_blocking.
/// TokioCommand piped stdout doesn't work for codex.exe on Windows ARM.
async fn run_codex_task(
    tasks: Arc<RwLock<HashMap<String, Task>>>,
    task_id: String,
    args: Vec<String>,
    working_dir: String,
) {
    {
        let mut store = tasks.write().await;
        if let Some(task) = store.get_mut(&task_id) {
            task.status = TaskStatus::Running;
            task.started_at = Some(Utc::now());
            Server::calculate_trust(task);
            Server::create_rollback(task);
            Server::persist_task(task);
        }
    }

    let args_clone = args.clone();
    let wd = working_dir.clone();
    let result = tokio::task::spawn_blocking(move || {
        std::process::Command::new(codex_cmd())
            .args(&args_clone)
            .current_dir(&wd)
            .output()
    }).await;

    let mut store = tasks.write().await;
    let mut retry_task: Option<Task> = None;
    let mut completed_snap: Option<Task> = None;
    if let Some(task) = store.get_mut(&task_id) {
        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Parse JSONL events
                for line in stdout.lines() {
                    if let Ok(ev) = serde_json::from_str::<Value>(line) {
                        task.last_activity = Some(Utc::now());
                        let ev_type = ev.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        if ev_type == "item.completed" {
                            if let Some(item) = ev.get("item") {
                                match item.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                                    "agent_message" => {
                                        if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                                            if !text.is_empty() {
                                                if !task.output.is_empty() { task.output.push('\n'); }
                                                task.output.push_str(text);
                                                task.progress_lines += 1;
                                            }
                                        }
                                    }
                                    "mcp_tool_call" | "command_execution" => {
                                        let tool = item.get("tool").or_else(|| item.get("command"))
                                            .and_then(|v| v.as_str()).unwrap_or("unknown");
                                        let server = item.get("server").and_then(|v| v.as_str()).unwrap_or("");
                                        let tool_name = if server.is_empty() { tool.to_string() } else { format!("{}:{}", server, tool) };
                                        let has_error = item.get("error").map(|e| !e.is_null()).unwrap_or(false);
                                        let status = if has_error { "error" } else { "completed" };
                                        task.steps.push(TaskStep {
                                            tool: tool_name,
                                            timestamp: Utc::now(),
                                            status: status.to_string(),
                                            summary: item.get("arguments").or_else(|| item.get("command"))
                                                .map(|v| { let s = v.to_string(); { let s_ref: &str = &s; safe_truncate(s_ref, 120) } }),
                                        });
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                task.status = if output.status.success() { TaskStatus::Done } else { TaskStatus::Failed };
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    task.error = Some(format!("Exit {}: {}", output.status.code().unwrap_or(-1),
                        if stderr.len() > 500 { &stderr[stderr.len()-500..] } else { &stderr }));
                }
            }
            Ok(Err(e)) => {
                task.status = TaskStatus::Failed;
                task.error = Some(format!("Failed to run codex: {}", e));
            }
            Err(e) => {
                task.status = TaskStatus::Failed;
                task.error = Some(format!("Task panicked: {}", e));
            }
        }
        task.completed_at = Some(Utc::now());
        if task.status == TaskStatus::Done { Server::validate_output(task); }
        Server::flag_extraction(task);
        // Item 18: retry/learn hooks
        if task.status == TaskStatus::Failed { retry_task = Server::prepare_retry(task); }
        if task.status == TaskStatus::Done && task.retry_of.is_some() { Server::learn_from_outcome(task); }
        Server::persist_task(task);
        Server::save_to_history(task);
        save_task_artifact(task);
        completed_snap = Some(task.clone());
    }
    if let Some(ref rt) = retry_task {
        store.insert(rt.id.clone(), rt.clone());
        Server::persist_task(rt);
    }
    drop(store);
    if let Some(ref rt) = retry_task {
        spawn_retry_execution(rt, tasks.clone(), None, &tokio::runtime::Handle::current());
    }
    if let Some(ref ct) = completed_snap {
        spawn_on_complete(ct, tasks.clone(), None, &tokio::runtime::Handle::current());
    }
}
async fn run_cli_task(
    tasks: Arc<RwLock<HashMap<String, Task>>>,
    task_id: String,
    command: &str,
    args: Vec<String>,
) {
    // Mark running
    {
        let mut store = tasks.write().await;
        if let Some(task) = store.get_mut(&task_id) {
            task.status = TaskStatus::Running;
            task.started_at = Some(Utc::now());
            Server::calculate_trust(task);
            Server::create_rollback(task);
            Server::persist_task(task);
        }
    }

    // Get working dir
    let working_dir = {
        let store = tasks.read().await;
        store.get(&task_id)
            .and_then(|t| t.working_dir.clone())
            .unwrap_or_else(|| r"C:\Users\josep".to_string())
    };

    // Spawn process Ã¢â‚¬â€ .cmd files need cmd /C on Windows
    let (spawn_cmd, spawn_args) = if command.ends_with(".cmd") || command.ends_with(".bat") {
        let mut all_args = vec!["/C".to_string(), command.to_string()];
        all_args.extend(args.clone());
        ("cmd".to_string(), all_args)
    } else {
        (command.to_string(), args.clone())
    };

    // §12: Set CPC_AGENT_ROLE env var if task has a role
    let task_role = {
        let store = tasks.read().await;
        store.get(&task_id).and_then(|t| t.role.clone())
    };

    let mut cmd = TokioCommand::new(&spawn_cmd);
    cmd.args(&spawn_args)
        .current_dir(&working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    if let Some(ref role) = task_role {
        cmd.env("CPC_AGENT_ROLE", role);
    }

    let mut child = match cmd
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let mut store = tasks.write().await;
            let mut retry_task: Option<Task> = None;
            if let Some(task) = store.get_mut(&task_id) {
                task.status = TaskStatus::Failed;
                task.error = Some(format!("Failed to spawn {}: {}", command, e));
                task.completed_at = Some(Utc::now());
                Server::flag_extraction(task);
                // Item 18: retry/learn hooks
                retry_task = Server::prepare_retry(task);
                Server::persist_task(task);
                Server::save_to_history(task);
            }
            if let Some(ref rt) = retry_task {
                store.insert(rt.id.clone(), rt.clone());
                Server::persist_task(rt);
            }
            drop(store);
            if let Some(ref rt) = retry_task {
                spawn_retry_execution(rt, tasks.clone(), None, &tokio::runtime::Handle::current());
            }
            return;
        }
    };

    // Store child PID for lifecycle tracking
    if let Some(pid) = child.id() {
        let mut store = tasks.write().await;
        if let Some(task) = store.get_mut(&task_id) {
            task.child_pid = Some(pid);
            Server::persist_task(task);
        }
        drop(store);
    }

    // Take stdout/stderr handles before waiting
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();

    // Spawn stdout reader â€” raw byte reading, splits on both \n and \r
    let stdout_handle = if let Some(mut stdout) = stdout.take() {
        let tasks_c = tasks.clone();
        let tid_c = task_id.clone();
        Some(tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            let mut partial = String::new();
            let mut cr_seen = false;
            loop {
                match stdout.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = String::from_utf8_lossy(&buf[..n]);
                        let mut lines: Vec<String> = Vec::new();
                        for c in chunk.chars() {
                            if c == '\n' {
                                if cr_seen {
                                    // \r\n pair â€” \r already emitted a line
                                    cr_seen = false;
                                    continue;
                                }
                                lines.push(std::mem::take(&mut partial));
                            } else if c == '\r' {
                                cr_seen = true;
                                let line = std::mem::take(&mut partial);
                                if !line.is_empty() {
                                    lines.push(line);
                                }
                            } else {
                                cr_seen = false;
                                partial.push(c);
                            }
                        }
                        if !lines.is_empty() {
                            let mut store = tasks_c.write().await;
                            if let Some(task) = store.get_mut(&tid_c) {
                                if task.status == TaskStatus::Cancelled { return; }
                                for line in &lines {
                                    if line.is_empty() { continue; }
                                    // Try to parse as stream-json event
                                    if let Ok(ev) = serde_json::from_str::<Value>(line) {
                                        // Update activity timestamp for stall detection
                                        task.last_activity = Some(Utc::now());
                                        task.stall_detected = false;
                                        let ev_type = ev.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                        match ev_type {
                                            "assistant" => {
                                                // Claude Code nests tool_use AND text in message.content[]
                                                if let Some(contents) = ev.pointer("/message/content").and_then(|v| v.as_array()) {
                                                    for item in contents {
                                                        let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                                        match item_type {
                                                            "text" => {
                                                                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                                                                    if !text.is_empty() {
                                                                        if !task.output.is_empty() { task.output.push('\n'); }
                                                                        task.output.push_str(text);
                                                                        task.progress_lines += 1;
                                                                    }
                                                                }
                                                            }
                                                            "tool_use" => {
                                                                let tool_name = item.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                                                                task.steps.push(TaskStep {
                                                                    tool: tool_name,
                                                                    timestamp: Utc::now(),
                                                                    status: "started".to_string(),
                                                                    summary: item.get("input").map(|v| {
                                                                        let s = v.to_string();
                                                                        { let s_ref: &str = &s; safe_truncate(s_ref, 120) }
                                                                    }),
                                                                });
                                                            }
                                                            _ => {}
                                                        }
                                                    }
                                                }
                                            }
                                            "user" => {
                                                // Claude Code: tool_result is at .message.content[]
                                                if let Some(contents) = ev.pointer("/message/content").and_then(|v| v.as_array()) {
                                                    for item in contents {
                                                        if item.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                                                            if let Some(last) = task.steps.last_mut() {
                                                                let is_err = item.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
                                                                last.status = if is_err { "error".to_string() } else { "completed".to_string() };
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            "item.completed" => {
                                                // Codex: events have item.type (agent_message, mcp_tool_call)
                                                if let Some(item) = ev.get("item") {
                                                    let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                                    match item_type {
                                                        "agent_message" => {
                                                            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                                                                if !text.is_empty() {
                                                                    if !task.output.is_empty() { task.output.push('\n'); }
                                                                    task.output.push_str(text);
                                                                    task.progress_lines += 1;
                                                                }
                                                            }
                                                        }
                                                        "mcp_tool_call" => {
                                                            let tool = item.get("tool").and_then(|v| v.as_str()).unwrap_or("unknown");
                                                            let server = item.get("server").and_then(|v| v.as_str()).unwrap_or("");
                                                            let tool_name = if server.is_empty() { tool.to_string() } else { format!("{}:{}", server, tool) };
                                                            let status = item.get("status").and_then(|v| v.as_str()).unwrap_or("completed");
                                                            let has_error = item.get("error").is_some() && !item.get("error").unwrap().is_null();
                                                            task.steps.push(TaskStep {
                                                                tool: tool_name,
                                                                timestamp: Utc::now(),
                                                                status: if has_error { "error".to_string() } else { status.to_string() },
                                                                summary: item.get("arguments").map(|v| {
                                                                    let s = v.to_string();
                                                                    { let s_ref: &str = &s; safe_truncate(s_ref, 120) }
                                                                }),
                                                            });
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                            }
                                            "result" => {
                                                // Skip â€” Claude Code text already captured from assistant events
                                            }
                                            _ => {}
                                        }
                                    } else {
                                        // Not valid JSON â€” append raw (fallback)
                                        if !task.output.is_empty() { task.output.push('\n'); }
                                        task.output.push_str(line);
                                        task.progress_lines += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // Flush remaining partial data
            if !partial.is_empty() {
                let mut store = tasks_c.write().await;
                if let Some(task) = store.get_mut(&tid_c) {
                    // Try JSON parse â€” extract text from assistant content array
                    if let Ok(ev) = serde_json::from_str::<Value>(&partial) {
                        if ev.get("type").and_then(|t| t.as_str()) == Some("assistant") {
                            if let Some(contents) = ev.pointer("/message/content").and_then(|v| v.as_array()) {
                                for item in contents {
                                    if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                                        if !text.is_empty() {
                                            if !task.output.is_empty() { task.output.push('\n'); }
                                            task.output.push_str(text);
                                            task.progress_lines += 1;
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        if !task.output.is_empty() { task.output.push('\n'); }
                        task.output.push_str(&partial);
                        task.progress_lines += 1;
                    }
                }
            }
        }))
    } else {
        None
    };

    // Spawn stderr reader â€” same byte-level splitting with [STDERR] prefix
    let stderr_handle = if let Some(mut stderr) = stderr.take() {
        let tasks_c = tasks.clone();
        let tid_c = task_id.clone();
        Some(tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            let mut partial = String::new();
            let mut cr_seen = false;
            let mut stderr_buf = String::new();
            loop {
                match stderr.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = String::from_utf8_lossy(&buf[..n]);
                        let mut lines: Vec<String> = Vec::new();
                        for c in chunk.chars() {
                            if c == '\n' {
                                if cr_seen {
                                    cr_seen = false;
                                    continue;
                                }
                                lines.push(std::mem::take(&mut partial));
                            } else if c == '\r' {
                                cr_seen = true;
                                let line = std::mem::take(&mut partial);
                                if !line.is_empty() {
                                    lines.push(line);
                                }
                            } else {
                                cr_seen = false;
                                partial.push(c);
                            }
                        }
                        if !lines.is_empty() {
                            let mut store = tasks_c.write().await;
                            if let Some(task) = store.get_mut(&tid_c) {
                                if task.status == TaskStatus::Cancelled { return stderr_buf; }
                                for line in &lines {
                                    if !stderr_buf.is_empty() { stderr_buf.push('\n'); }
                                    stderr_buf.push_str(line);
                                    if !task.output.is_empty() { task.output.push('\n'); }
                                    task.output.push_str("[STDERR] ");
                                    task.output.push_str(line);
                                    task.progress_lines += 1;
                                }
                            }
                        }
                    }
                }
            }
            // Flush remaining partial data
            if !partial.is_empty() {
                if !stderr_buf.is_empty() { stderr_buf.push('\n'); }
                stderr_buf.push_str(&partial);
                let mut store = tasks_c.write().await;
                if let Some(task) = store.get_mut(&tid_c) {
                    if !task.output.is_empty() { task.output.push('\n'); }
                    task.output.push_str("[STDERR] ");
                    task.output.push_str(&partial);
                    task.progress_lines += 1;
                }
            }
            stderr_buf
        }))
    } else {
        None
    };

    // Wait for child exit and both readers concurrently
    let (exit_status, _, stderr_output) = tokio::join!(
        child.wait(),
        async {
            if let Some(h) = stdout_handle {
                let _ = h.await;
            }
        },
        async {
            if let Some(h) = stderr_handle {
                h.await.ok().unwrap_or_default()
            } else {
                String::new()
            }
        }
    );

    // Update final status
    let mut store = tasks.write().await;
    let mut retry_task: Option<Task> = None;
    let mut completed_snap: Option<Task> = None;
    if let Some(task) = store.get_mut(&task_id) {
        if task.status == TaskStatus::Cancelled {
            Server::persist_task(task);
            return;
        }

        // Item 7: Detect context limit before setting final status
        let ctx_limited = task.output.contains("context window")
            || task.output.contains("token limit")
            || task.output.contains("maximum context length")
            || task.output.contains("conversation is too long");

        match exit_status {
            Ok(status) if status.success() => {
                task.status = TaskStatus::Done;
            }
            Ok(status) => {
                task.status = TaskStatus::Failed;
                let stderr_msg = if stderr_output.len() > 500 {
                    format!("...{}", &stderr_output[stderr_output.len()-500..])
                } else { stderr_output };
                if ctx_limited {
                    let ctx_file = format!("{}\\context_resume_{}.json", history_dir(), task.id);
                    let resume = json!({
                        "task_id": task.id, "prompt": task.prompt,
                        "backend": task.backend.to_string(),
                        "steps_completed": task.steps.len(),
                        "last_steps": task.steps.iter().rev().take(10).rev()
                            .map(|s| json!({"tool": s.tool, "status": s.status}))
                            .collect::<Vec<Value>>(),
                        "output_tail": if task.output.len() > 1000 { &task.output[task.output.len()-1000..] } else { &task.output },
                        "saved_at": Utc::now().to_rfc3339(),
                    });
                    let _ = std::fs::write(&ctx_file, serde_json::to_string_pretty(&resume).unwrap_or_default());
                    task.error = Some(format!("Context limit after {} steps. Resume saved: {}", task.steps.len(), ctx_file));
                } else {
                    task.error = Some(format!("Exit code {}. Stderr: {}", status.code().unwrap_or(-1), stderr_msg));
                }
            }
            Err(e) => {
                task.status = TaskStatus::Failed;
                task.error = Some(format!("Process error: {}", e));
            }
        }
        task.completed_at = Some(Utc::now());
        if task.status == TaskStatus::Done { Server::validate_output(task); }
        Server::flag_extraction(task);
        // Item 18: retry/learn hooks
        if task.status == TaskStatus::Failed { retry_task = Server::prepare_retry(task); }
        if task.status == TaskStatus::Done && task.retry_of.is_some() { Server::learn_from_outcome(task); }
        Server::persist_task(task);
        Server::save_to_history(task);
        save_task_artifact(task);
        completed_snap = Some(task.clone());
    }
    if let Some(ref rt) = retry_task {
        store.insert(rt.id.clone(), rt.clone());
        Server::persist_task(rt);
    }
    drop(store);
    if let Some(ref rt) = retry_task {
        spawn_retry_execution(rt, tasks.clone(), None, &tokio::runtime::Handle::current());
    }
    if let Some(ref ct) = completed_snap {
        spawn_on_complete(ct, tasks.clone(), None, &tokio::runtime::Handle::current());
    }
}

// ============================================================================
// Tool Handlers
// ============================================================================

/// Safe UTF-8 string truncation — never panics on multi-byte characters
fn safe_truncate(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes { return s.to_string(); }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) { end -= 1; }
    format!("{}...", &s[..end])
}

const SAFETY_VALIDATION_BLOCK: &str = "[SAFETY VALIDATION REQUIRED]\nBefore executing any actions, review your plan and check:\n1. Does every action directly serve the stated goal?\n2. Are there steps that access credentials, financial sites, or system files unrelated to the goal?\n3. Would the user be surprised by any step?\nOutput [SAFETY CHECK: PASS] if all checks pass, or [SAFETY CHECK: REVIEW NEEDED: reason] if any check fails.\nDo not proceed with actions until you have completed this check.\n[END SAFETY VALIDATION]\n\n";

fn ensure_safety_validation(prompt: &str) -> String {
    if prompt.contains("[SAFETY VALIDATION REQUIRED]") {
        prompt.to_string()
    } else {
        format!("{}{}", SAFETY_VALIDATION_BLOCK, prompt)
    }
}

fn extract_safety_warning(output: &str) -> Option<String> {
    output
        .lines()
        .find(|line| line.contains("[SAFETY CHECK: REVIEW NEEDED"))
        .map(|line| line.trim().to_string())
}

fn handle_submit_task(server: &Server, params: Value) -> Result<Value, String> {
    let auto_route = params.get("auto_route").and_then(|v| v.as_bool()).unwrap_or(false);
    let prompt_for_route = params.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
    let wd_for_route = params.get("working_dir").and_then(|v| v.as_str());

    let backend_str_opt = params.get("backend").and_then(|v| v.as_str());

    // Item 16: If no backend specified and auto_route is true, use recommend_backend
    let (backend, routed) = if backend_str_opt.is_none() && auto_route {
        let rec = Server::recommend_backend(prompt_for_route, wd_for_route);
        let be = match rec.recommended_backend.as_str() {
            "gpt" => Backend::Gpt,
            "gemini" => Backend::Gemini,
            "claude_code" | "claude" => Backend::ClaudeCode,
            "codex" => Backend::Codex,
            _ => Backend::ClaudeCode,
        };
        (be, Some(rec))
    } else {
        let backend_str = backend_str_opt
            .ok_or("Missing 'backend' parameter (gpt, gemini, claude_code, codex). Or set auto_route: true.")?;
        let be = match backend_str {
            "gpt" => Backend::Gpt,
            "gemini" => Backend::Gemini,
            "claude_code" | "claude" => Backend::ClaudeCode,
            "codex" => Backend::Codex,
            _ => return Err(format!("Unknown backend '{}'. Use: gpt, gemini, claude_code, codex", backend_str)),
        };
        (be, None)
    };

    let raw_prompt = params.get("prompt")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'prompt' parameter")?
        .to_string();

    // CPC behavioral injection — prepend delegation rules to every task
    // Include active loaf context if one exists
    let loaf_context = find_active_loaf().map(|(id, loaf)| {
        let goal = loaf.get("goal").and_then(|g| g.as_str()).unwrap_or("?");
        let phase_idx = loaf.get("current_phase").and_then(|p| p.as_u64()).unwrap_or(0) as usize;
        let phase_name = loaf.get("phases").and_then(|p| p.as_array())
            .and_then(|p| p.get(phase_idx))
            .and_then(|p| p.get("name")).and_then(|n| n.as_str()).unwrap_or("main");
        format!(
            "You are working on: {}. Loaf: {}. Phase: {}. \
             Report: what you changed, what you decided, what you discovered.\n",
            goal, id, phase_name
        )
    }).unwrap_or_default();

    let prompt = ensure_safety_validation(&format!(
        "[CPC DELEGATION CONTEXT]\n\
         {}\
         - Track your progress: note what you're doing at each major step.\n\
         - When done, summarize: decisions made, files changed, patterns discovered.\n\
         - On failure, document what failed and why before exiting.\n\
         - If you discover something reusable (a fix, a pattern, a decision), call it out clearly.\n\n\
         [TASK]\n{}", loaf_context, raw_prompt
    ));

    // §12: Specialist role handling — custom YAML roles override built-in
    let role = params.get("role").and_then(|v| v.as_str()).map(String::from);
    let role_prompt_owned: Option<String> = role.as_deref().and_then(|r| {
        get_custom_role_prompt(r).or_else(|| get_role_prompt(r).map(String::from))
    });
    let role_prompt = role_prompt_owned.as_deref();

    let user_system_prompt = params.get("system_prompt")
        .and_then(|v| v.as_str())
        .map(String::from);

    let system_prompt = match (role_prompt, &user_system_prompt) {
        (Some(rp), Some(sp)) => Some(format!("{}\n\n{}", rp, sp)),
        (Some(rp), None) => Some(rp.to_string()),
        (None, Some(sp)) => Some(sp.clone()),
        (None, None) => None,
    };

    // §13: Auto-artifact saving (default true)
    let save_artifact = params.get("save_artifact").and_then(|v| v.as_bool()).unwrap_or(true);

    let model = params.get("model")
        .and_then(|v| v.as_str())
        .map(String::from);

    let working_dir = params.get("working_dir")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Per-task visibility override, falls back to dashboard prefs
    let visible = params.get("visible")
        .and_then(|v| v.as_bool())
        .unwrap_or(false); // MCP tasks default to background

    let task_id = Uuid::new_v4().to_string()[..8].to_string();

    let task = Task {
        id: task_id.clone(),
        backend: backend.clone(),
        prompt: prompt.clone(),
        system_prompt: system_prompt.clone(),
        model: model.clone(),
        working_dir: working_dir.clone(),
        status: TaskStatus::Queued,
        output: String::new(),
        error: None,
        created_at: Utc::now(),
        started_at: None,
        completed_at: None,
        progress_lines: 0, steps: Vec::new(), last_activity: None, stall_detected: false, extraction_status: ExtractionStatus::None,
        trust_score: 0, trust_level: TrustLevel::Low, rollback_path: None, validation_status: ValidationStatus::NotChecked, assertions: Vec::new(), backed_up_files: Vec::new(), retry_count: 0, max_retries: 2, retry_of: None, error_context: None, input_tokens: 0, output_tokens: 0, cost_usd: 0.0,
        on_complete: params.get("on_complete").and_then(|v| v.as_str()).map(String::from),
        role: role.clone(),
        save_artifact,
        rerun_of: None,
        parent_task_id: None,
        forked_from: None,
        continuation_of: None,
        child_pid: None,
        watchdog_observations: Vec::new(),
    };

    // Store task
    let tasks = server.tasks.clone();
    let config = server.config.clone();

    server.runtime.block_on(async {
        let mut store = tasks.write().await;
        store.insert(task_id.clone(), task.clone());
    });
    Server::persist_task(&task);

    // Spawn background execution
    let tasks_bg = server.tasks.clone();
    let tid = task_id.clone();

    // If visible, we only spawn the terminal (no background headless process)
    let run_background = !visible;

    // Track exe+args for optional visible terminal
    let mut vis_exe: Option<String> = None;
    let mut vis_args: Vec<String> = Vec::new();

    match backend {
        Backend::Gpt => {
            // GPT is API-only, no CLI to mirror
            server.runtime.spawn(run_gpt_task(config, tasks_bg, tid));
        }
        Backend::Gemini => {
            // Item 4: Inject breadcrumb context for Gemini continuity
            let gemini_prompt = if let Some(bc) = Server::read_breadcrumb_state() {
                format!("{}\n\n{}", bc, prompt)
            } else { prompt.clone() };
            let mut args = vec![
                gemini_cmd().to_string(),
                "-p".to_string(),
                gemini_prompt.clone(),
                "--yolo".to_string(),
        ];
            if let Some(m) = model {
                args.push("--model".to_string());
                args.push(m);
            }
            vis_exe = Some(node_cmd().to_string());
            vis_args = args.clone();
            if run_background {
                server.runtime.spawn(run_cli_task(
                    tasks_bg,
                    tid,
                    r"C:\Program Files\nodejs\node.exe",
                    args,
                ));
            }
        }
        Backend::ClaudeCode => {
            // CRITICAL: prompt must come immediately after -p
            let mut args = vec![
                "-p".to_string(),
                prompt.clone(),
                "--dangerously-skip-permissions".to_string(),
                "--verbose".to_string(),
                "--output-format".to_string(),
                "stream-json".to_string(),
                "--add-dir".to_string(),
                r"C:\temp".to_string(),
                "--add-dir".to_string(),
                r"C:\My Drive\Volumes".to_string(),
                "--add-dir".to_string(),
                r"C:\CPC".to_string(),
                "--add-dir".to_string(),
                r"C:\rust-mcp".to_string(),
        ];
            if let Some(m) = model {
                args.push("--model".to_string());
                args.push(m);
            }
            if let Some(ref wd) = working_dir {
                args.push("--add-dir".to_string());
                args.push(wd.clone());
            }
            vis_exe = Some(claude_code_cmd().to_string());
            vis_args = args.clone();
            if run_background { server.runtime.spawn(run_cli_task(tasks_bg, tid, claude_code_cmd(), args)); }
        }
        Backend::Codex => {
            let wd = working_dir.as_deref().unwrap_or(r"C:\rust-mcp");
            let args = vec![
                "exec".into(), "--json".into(), "--skip-git-repo-check".into(),
                "--full-auto".into(), "--cd".into(), wd.to_string(), prompt.clone(),
            ];
            vis_exe = Some(codex_cmd().to_string());
            vis_args = args.clone();
            if run_background { server.runtime.spawn(run_codex_task(tasks_bg, tid, args, wd.to_string())); }
        }
    }

    // When visible=true, SKIP background task (avoid double token cost)
    // Manager marks task as "visible" - no stream-json tracking
    if visible {
        if let Some(exe) = vis_exe {
            let title: String = prompt.chars().take(60).collect();
            let title = if prompt.len() > 60 { format!("{}...", title) } else { title };
            let wd = working_dir.as_deref().unwrap_or(r"C:\rust-mcp");
            spawn_visible_terminal(&title, &exe, &vis_args, wd);
        }
    }

    // If visible-only, mark task as done (terminal handles it, no tracking)
    if visible && !run_background {
        let tasks_done = server.tasks.clone();
        let tid_done = task_id.clone();
        server.runtime.block_on(async {
            let mut store = tasks_done.write().await;
            if let Some(t) = store.get_mut(&tid_done) {
                t.status = TaskStatus::Running;
                t.started_at = Some(Utc::now());
                t.output = "Running in visible terminal - check terminal window for output".to_string();
            }
        });
    }

    let mut result = json!({
        "task_id": task_id,
        "backend": backend.to_string(),
        "status": if visible { "visible" } else { "queued" },
        "visible": visible,
        "message": if visible { format!("Task opened in visible terminal. Watch the terminal window.") } else { format!("Task submitted to {}. Poll with get_status.", backend) }
    });

    // Item 16: Include routing info when auto_route was used
    if let Some(rec) = routed {
        result["auto_routed"] = json!(true);
        result["routing"] = json!({
            "confidence": rec.confidence,
            "reasoning": rec.reasoning,
            "alternatives": rec.alternatives,
        });
    }


    // v1.2.3: wait=true blocking removed. task_submit always returns immediately.
    // timeout_secs kept as estimated_secs for informational purposes only.
    if let Some(est) = params.get("timeout_secs").or(params.get("estimated_secs")).and_then(|v| v.as_u64()) {
        result["estimated_secs"] = json!(est);
    }

    Ok(result)
}

/// Watch multiple tasks until all complete. Polls internally (zero LLM turns).
/// Optionally sends MCP notifications for progress updates.
fn handle_watch_tasks(server: &Server, params: Value) -> Result<Value, String> {
    let task_ids: Vec<String> = params.get("task_ids")
        .and_then(|v| v.as_array())
        .ok_or("Missing 'task_ids' array")?
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();

    if task_ids.is_empty() {
        return Err("task_ids array is empty".into());
    }

    let timeout_secs = params.get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(600);

    let progress = params.get("progress")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let progress_interval_secs = params.get("progress_interval_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(10);

    // Validate all task IDs exist
    {
        let store = server.runtime.block_on(server.tasks.read());
        for tid in &task_ids {
            if !store.contains_key(tid) {
                return Err(format!("Task '{}' not found", tid));
            }
        }
    }

    let start = std::time::Instant::now();
    let mut last_progress = std::time::Instant::now() - std::time::Duration::from_secs(progress_interval_secs + 1);
    let mut last_steps: HashMap<String, usize> = HashMap::new();

    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));

        let store = server.runtime.block_on(server.tasks.read());

        // Check if all tasks are in terminal state
        let mut all_done = true;
        let mut results: Vec<Value> = Vec::new();

        for tid in &task_ids {
            if let Some(task) = store.get(tid) {
                let is_terminal = matches!(task.status, TaskStatus::Done | TaskStatus::Failed | TaskStatus::Cancelled);
                if !is_terminal {
                    all_done = false;
                }
                results.push(json!({
                    "task_id": task.id,
                    "backend": task.backend.to_string(),
                    "status": task.status.to_string(),
                    "elapsed": task.started_at.map(|s| {
                        let end = task.completed_at.unwrap_or_else(Utc::now);
                        format!("{}s", (end - s).num_seconds())
                    }),
                    "step_count": task.steps.len(),
                    "error": task.error.clone(),
                    "output_preview": if is_terminal { Server::generate_end_report(task) } else { String::new() },
                    "input_tokens": task.input_tokens,
                    "output_tokens": task.output_tokens,
                    "cost_usd": task.cost_usd,
                }));
            }
        }

        // Send progress notification if enabled and interval elapsed
        if progress && !all_done && last_progress.elapsed().as_secs() >= progress_interval_secs {
            let mut updates: Vec<String> = Vec::new();
            for tid in &task_ids {
                if let Some(task) = store.get(tid) {
                    let prev_steps = last_steps.get(tid).copied().unwrap_or(0);
                    let cur_steps = task.steps.len();
                    let status_str = match task.status {
                        TaskStatus::Running => {
                            if cur_steps > prev_steps {
                                format!("{} ({}): step {}", tid, task.backend, cur_steps)
                            } else {
                                format!("{} ({}): running ({}s)", tid, task.backend,
                                    task.started_at.map(|s| (Utc::now() - s).num_seconds()).unwrap_or(0))
                            }
                        }
                        TaskStatus::Done => format!("{} ({}): done", tid, task.backend),
                        TaskStatus::Failed => format!("{} ({}): FAILED", tid, task.backend),
                        _ => format!("{} ({}): {}", tid, task.backend, task.status),
                    };
                    updates.push(status_str);
                    last_steps.insert(tid.clone(), cur_steps);
                }
            }
            drop(store); // release lock before writing to stdout
            server.send_notification("info", &format!("[watch] {}", updates.join(" | ")));
            last_progress = std::time::Instant::now();
            continue; // re-check after notification
        }

        drop(store);

        if all_done {
            return Ok(json!({
                "status": "all_complete",
                "elapsed": format!("{}s", start.elapsed().as_secs()),
                "tasks": results
            }));
        }

        if start.elapsed().as_secs() > timeout_secs {
            return Ok(json!({
                "status": "timeout",
                "elapsed": format!("{}s", start.elapsed().as_secs()),
                "message": format!("Timed out after {}s. Some tasks still running.", timeout_secs),
                "tasks": results
            }));
        }
    }
}

fn handle_get_status(server: &Server, params: Value) -> Result<Value, String> {
    let task_id = params.get("task_id")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'task_id' parameter")?;

    let store = server.runtime.block_on(server.tasks.read());
    let task = store.get(task_id).ok_or(format!("Task '{}' not found", task_id))?;

    let elapsed = task.started_at.map(|s| {
        let end = task.completed_at.unwrap_or_else(Utc::now);
        let dur = end - s;
        format!("{}s", dur.num_seconds())
    });
    // Item 5: Compute stall status inline.
    // A task is "stalled" only if it's Running, no tool is mid-flight, AND
    // >90s have passed since last activity. active_tool_running takes precedence
    // because long Write/Edit tools can legitimately run 60-90s silently.
    let tool_running = task.steps.last().map(|s| s.status == "started").unwrap_or(false);
    let stalled = if task.status == TaskStatus::Running && !tool_running {
        task.last_activity.map(|la| Utc::now().signed_duration_since(la).num_seconds() > 90).unwrap_or(false)
    } else { false };

    // Human-friendly health enum — additive to stall_detected, more expressive.
    // Values: "done", "failed", "queued", "cancelled", "running_long_tool",
    // "stalled", "idle", "running".
    let health = match task.status {
        TaskStatus::Done => "done",
        TaskStatus::Failed => "failed",
        TaskStatus::Queued => "queued",
        TaskStatus::Cancelled => "cancelled",
        TaskStatus::Paused => "paused",
        TaskStatus::Running => {
            if tool_running { "running_long_tool" }
            else if stalled { "stalled" }
            else if task.last_activity.map(|la| Utc::now().signed_duration_since(la).num_seconds() > 30).unwrap_or(false) { "idle" }
            else { "running" }
        }
    };

    // Item 2: Recent steps summary
    let recent_steps: Vec<Value> = task.steps.iter().rev().take(5).rev().map(|s| {
        json!({"tool": s.tool, "status": s.status, "ts": s.timestamp.to_rfc3339()})
    }).collect();

    // Item 3: Smart report for terminal states
    let output_preview = match task.status {
        TaskStatus::Done | TaskStatus::Failed => Server::generate_end_report(task),
        _ => if task.output.len() > 300 {
            format!("{}...\n\n[{} total chars, use get_output for full]", safe_truncate(&task.output, 300), task.output.len())
        } else {
            task.output.clone()
        }
    };
    let warning = extract_safety_warning(&task.output);

    Ok(json!({
        "task_id": task.id,
        "backend": task.backend,
        "status": task.status.to_string(),
        "progress_lines": task.progress_lines,
        "step_count": task.steps.len(),
        "recent_steps": recent_steps,
        "stall_detected": stalled,
        "active_tool_running": Server::active_tool_running(task),
        "health": health,
            "input_tokens": task.input_tokens,
            "output_tokens": task.output_tokens,
            "cost_usd": task.cost_usd,
        "elapsed": elapsed,
        "created_at": task.created_at.to_rfc3339(),
        "started_at": task.started_at.map(|t| t.to_rfc3339()),
        "completed_at": task.completed_at.map(|t| t.to_rfc3339()),
        "error": task.error,
        "output_preview": output_preview,
        "warning": warning,
        "watchdog_observations": task.watchdog_observations,
        "child_pid": task.child_pid
    }))
}

fn handle_get_output(server: &Server, params: Value) -> Result<Value, String> {
    let task_id = params.get("task_id")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'task_id' parameter")?;

    let tail = params.get("tail")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);

    let store = server.runtime.block_on(server.tasks.read());
    let task = store.get(task_id).ok_or(format!("Task '{}' not found", task_id))?;

    let output = if let Some(n) = tail {
        let lines: Vec<&str> = task.output.lines().collect();
        if lines.len() > n {
            lines[lines.len()-n..].join("\n")
        } else {
            task.output.clone()
        }
    } else {
        task.output.clone()
    };
    let warning = extract_safety_warning(&output);

    Ok(json!({
        "task_id": task.id,
        "status": task.status.to_string(),
        "total_lines": task.output.lines().count(),
        "output": output,
        "error": task.error,
        "warning": warning
    }))
}

fn handle_list_tasks(server: &Server, params: Value) -> Result<Value, String> {
    let status_filter = params.get("status")
        .and_then(|v| v.as_str());

    let backend_filter = params.get("backend")
        .and_then(|v| v.as_str());

    let limit = params.get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(20) as usize;

    let store = server.runtime.block_on(server.tasks.read());

    let mut tasks: Vec<&Task> = store.values()
        .filter(|t| {
            if let Some(sf) = status_filter {
                if t.status.to_string() != sf { return false; }
            }
            if let Some(bf) = backend_filter {
                if t.backend.to_string() != bf { return false; }
            }
            true
        })
        .collect();

    tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    tasks.truncate(limit);

    let summary: Vec<Value> = tasks.iter().map(|t| {
        json!({
            "task_id": t.id,
            "backend": t.backend,
            "status": t.status.to_string(),
            "prompt_preview": if t.prompt.len() > 80 {
                safe_truncate(&t.prompt, 80)
            } else {
                t.prompt.clone()
            },
            "created_at": t.created_at.to_rfc3339(),
            "elapsed": t.started_at.map(|s| {
                let end = t.completed_at.unwrap_or_else(Utc::now);
                format!("{}s", (end - s).num_seconds())
            }),
        })
    }).collect();

    Ok(json!({
        "total": store.len(),
        "showing": summary.len(),
        "tasks": summary
    }))
}

fn handle_cancel_task(server: &Server, params: Value) -> Result<Value, String> {
    let task_id = params.get("task_id")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'task_id' parameter")?;

    let mut store = server.runtime.block_on(server.tasks.write());
    let task = store.get_mut(task_id).ok_or(format!("Task '{}' not found", task_id))?;

    if task.status != TaskStatus::Running && task.status != TaskStatus::Queued {
        return Err(format!("Task '{}' is already {} - cannot cancel", task_id, task.status));
    }

    // v1.2.3: Kill the child process tree before marking cancelled
    let killed_tree = if let Some(root_pid) = task.child_pid {
        kill_process_tree(root_pid)
    } else {
        vec![]
    };

    task.status = TaskStatus::Cancelled;
    task.completed_at = Some(Utc::now());
    task.error = Some("Cancelled by user".into());
    if !killed_tree.is_empty() {
        task.watchdog_observations.push(format!(
            "[{}] Cancel killed process tree: {:?}", Utc::now().format("%H:%M:%S"), killed_tree
        ));
    }
    Server::flag_extraction(task);
    // Item 18: no retry for cancelled tasks
    Server::persist_task(task);
    Server::save_to_history(task);

    Ok(json!({
        "task_id": task_id,
        "status": "cancelled",
        "killed_tree": killed_tree,
        "message": if killed_tree.is_empty() {
            "Task cancelled (no child process to kill).".to_string()
        } else {
            format!("Task cancelled. Killed {} processes.", killed_tree.len())
        }
    }))
}

/// Walk the process tree rooted at `root_pid`, kill descendants bottom-up, then kill root.
/// Returns list of PIDs that were successfully terminated.
fn kill_process_tree(root_pid: u32) -> Vec<u32> {
    use sysinfo::{System, Pid, ProcessesToUpdate};

    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    // Build parent→children map
    let mut children_map: HashMap<u32, Vec<u32>> = HashMap::new();
    for (pid, proc) in sys.processes() {
        if let Some(parent) = proc.parent() {
            children_map.entry(parent.as_u32()).or_default().push(pid.as_u32());
        }
    }

    // BFS to collect all descendants
    let mut to_kill = Vec::new();
    let mut queue = vec![root_pid];
    while let Some(pid) = queue.pop() {
        to_kill.push(pid);
        if let Some(kids) = children_map.get(&pid) {
            queue.extend(kids.iter());
        }
    }

    // Kill in reverse order (descendants first, root last)
    to_kill.reverse();
    let mut killed = Vec::new();
    for pid in &to_kill {
        if let Some(proc) = sys.process(Pid::from_u32(*pid)) {
            if proc.kill() {
                killed.push(*pid);
            }
        }
    }
    killed
}

/// v1.2.3: task_poll — returns tasks completed since a timestamp, plus still-running tasks and status_bar.
fn handle_task_poll(server: &Server, params: Value) -> Result<Value, String> {
    let since_str = params.get("since").and_then(|v| v.as_str());
    let since: DateTime<Utc> = if let Some(s) = since_str {
        DateTime::parse_from_rfc3339(s)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now() - chrono::Duration::hours(1))
    } else {
        // Default: 1 hour ago
        Utc::now() - chrono::Duration::hours(1)
    };

    let store = server.runtime.block_on(server.tasks.read());

    let completed_since: Vec<Value> = store.values()
        .filter(|t| {
            matches!(t.status, TaskStatus::Done | TaskStatus::Failed | TaskStatus::Cancelled)
                && t.completed_at.map_or(false, |c| c > since)
        })
        .map(|t| json!({
            "task_id": t.id,
            "backend": t.backend.to_string(),
            "status": t.status.to_string(),
            "prompt_preview": safe_truncate(&t.prompt, 80),
            "completed_at": t.completed_at.map(|c| c.to_rfc3339()),
            "error": t.error,
        }))
        .collect();

    let still_running: Vec<Value> = store.values()
        .filter(|t| matches!(t.status, TaskStatus::Running | TaskStatus::Queued))
        .map(|t| json!({
            "task_id": t.id,
            "backend": t.backend.to_string(),
            "status": t.status.to_string(),
            "prompt_preview": safe_truncate(&t.prompt, 80),
            "elapsed": t.started_at.map(|s| format!("{}s", (Utc::now() - s).num_seconds())),
            "child_pid": t.child_pid,
        }))
        .collect();

    let status_bar = build_status_bar(&store);

    Ok(json!({
        "completed_since": completed_since,
        "still_running": still_running,
        "status_bar": status_bar,
        "polled_at": Utc::now().to_rfc3339(),
    }))
}

/// Build a status_bar summary from task state + external state files.
fn build_status_bar(store: &HashMap<String, Task>) -> Value {
    let running = store.values().filter(|t| t.status == TaskStatus::Running).count();
    let queued = store.values().filter(|t| t.status == TaskStatus::Queued).count();
    let unclaimed = 0usize; // reserved for future queue system

    let manager_line = format!("{} running, {} queued, {} unclaimed", running, queued, unclaimed);

    // Query autonomous breadcrumb state
    let autonomous_data = std::env::var("AUTONOMOUS_DATA_DIR").unwrap_or_else(|_| {
        let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
        format!(r"{}\autonomous", local)
    });
    let breadcrumb_line = read_state_file(&format!(r"{}\logs\breadcrumb.jsonl", autonomous_data))
        .unwrap_or_else(|| "unavailable".to_string());

    // Query local server state
    let loaf_line = read_active_loaf_summary();

    let formatted = format!("mgr: {} | bc: {} | loaf: {}", manager_line, breadcrumb_line, loaf_line);

    json!({
        "manager": manager_line,
        "breadcrumb": breadcrumb_line,
        "loaf": loaf_line,
        "formatted": formatted,
    })
}

/// Read last line of a state file to get latest status. Returns None if file unreadable.
fn read_state_file(path: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let last_line = content.lines().last()?;
    // Try to extract a summary from JSONL
    if let Ok(v) = serde_json::from_str::<Value>(last_line) {
        let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("?");
        let step = v.get("current_step").and_then(|s| s.as_u64()).unwrap_or(0);
        let total = v.get("total_steps").and_then(|s| s.as_u64()).unwrap_or(0);
        Some(format!("{} ({}/{})", name, step, total))
    } else {
        Some(safe_truncate(last_line, 60))
    }
}

/// Read active loaf summary for status_bar
fn read_active_loaf_summary() -> String {
    match find_active_loaf() {
        Some((id, loaf)) => {
            let goal = loaf.get("goal").and_then(|g| g.as_str()).unwrap_or("?");
            format!("{}: {}", id, safe_truncate(goal, 40))
        }
        None => "none".to_string(),
    }
}

fn handle_pause_task(server: &Server, params: Value) -> Result<Value, String> {
    let task_id = params.get("task_id")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'task_id' parameter")?;

    let mut store = server.runtime.block_on(server.tasks.write());
    let task = store.get_mut(task_id).ok_or(format!("Task '{}' not found", task_id))?;

    if task.status != TaskStatus::Running && task.status != TaskStatus::Queued {
        return Err(format!("Task '{}' is {} - can only pause running or queued tasks", task_id, task.status));
    }

    task.status = TaskStatus::Paused;
    Server::persist_task(task);

    Ok(json!({
        "task_id": task_id,
        "status": "paused",
        "message": "Task paused. Background process may still be running but status is marked paused. Use resume_task to re-queue."
    }))
}

fn handle_resume_task(server: &Server, params: Value) -> Result<Value, String> {
    let task_id = params.get("task_id")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'task_id' parameter")?;

    let tasks = server.tasks.clone();
    let config = server.config.clone();
    let mut store = server.runtime.block_on(tasks.write());
    let task = store.get_mut(task_id).ok_or(format!("Task '{}' not found", task_id))?;

    if task.status != TaskStatus::Paused {
        return Err(format!("Task '{}' is {} - can only resume paused tasks", task_id, task.status));
    }

    task.status = TaskStatus::Queued;
    task.started_at = None;
    Server::persist_task(task);

    let task_snap = task.clone();
    drop(store);

    spawn_retry_execution(&task_snap, tasks.clone(), Some(config), &server.runtime);

    Ok(json!({
        "task_id": task_id,
        "status": "queued",
        "message": "Task resumed and re-queued for execution."
    }))
}

fn handle_configure(server: &Server, params: Value) -> Result<Value, String> {
    let mut config = server.runtime.block_on(server.config.write());
    let mut changes = Vec::new();

    if let Some(key) = params.get("openai_api_key").and_then(|v| v.as_str()) {
        config.openai_api_key = Some(key.to_string());
        changes.push("openai_api_key set");
    }

    if let Some(model) = params.get("default_gpt_model").and_then(|v| v.as_str()) {
        config.default_gpt_model = model.to_string();
        changes.push("default_gpt_model updated");
    }

    if let Some(dir) = params.get("default_working_dir").and_then(|v| v.as_str()) {
        config.default_working_dir = dir.to_string();
        changes.push("default_working_dir updated");
    }

    if changes.is_empty() {
        // Just show current config
        Ok(json!({
            "openai_api_key": config.openai_api_key.as_ref().map(|k| format!("{}...{}", &k[..8.min(k.len())], &k[k.len().saturating_sub(4)..])),
            "default_gpt_model": config.default_gpt_model,
            "default_working_dir": config.default_working_dir,
            "gemini_cmd": gemini_cmd(),
            "claude_code_cmd": claude_code_cmd(),
        }))
    } else {
        Ok(json!({
            "changes": changes,
            "message": "Configuration updated"
        }))
    }
}

fn handle_cleanup(server: &Server, params: Value) -> Result<Value, String> {
    let before_days = params.get("older_than_days")
        .and_then(|v| v.as_u64())
        .unwrap_or(7);

    let cutoff = Utc::now() - chrono::Duration::days(before_days as i64);

    let mut store = server.runtime.block_on(server.tasks.write());
    let to_remove: Vec<String> = store.iter()
        .filter(|(_, t)| {
            t.completed_at.map_or(false, |c| c < cutoff)
                && (t.status == TaskStatus::Done || t.status == TaskStatus::Failed || t.status == TaskStatus::Cancelled)
        })
        .map(|(id, _)| id.clone())
        .collect();

    let count = to_remove.len();
    for id in &to_remove {
        store.remove(id);
        let path = format!("{}\\{}.json", tasks_dir(), id);
        std::fs::remove_file(path).ok();
    }

    Ok(json!({
        "removed": count,
        "remaining": store.len(),
        "message": format!("Cleaned up {} completed tasks older than {} days", count, before_days)
    }))
}

// ============================================================================
// Workflow Execution
// ============================================================================

fn run_workflow_step(backend: &str, prompt: &str, working_dir: &str, timeout_secs: u64) -> Result<String, String> {
    let (cmd, args): (&str, Vec<String>) = match backend {
        "claude_code" => {
            let a = vec![
                "-p".to_string(), prompt.to_string(),
                "--dangerously-skip-permissions".to_string(),
                "--verbose".to_string(),
                "--output-format".to_string(), "stream-json".to_string(),
                "--add-dir".to_string(), working_dir.to_string(),
            ];
            (claude_code_cmd(), a)
        }
        "codex" => {
            let a = vec![
                "exec".to_string(),
                "--json".to_string(),
                "--skip-git-repo-check".to_string(),
                "--dangerously-bypass-approvals-and-sandbox".to_string(),
                "--cd".to_string(), working_dir.to_string(),
                prompt.to_string(),
            ];
            (codex_cmd(), a)
        }
        "gemini" => {
            let a = vec![
                gemini_cmd().to_string(),
                "--yolo".to_string(),
                "-p".to_string(), prompt.to_string(),
            ];
            (node_cmd(), a)
        }
        _ => return Err(format!("Unknown backend: '{}'. Use: claude_code, codex, gemini", backend)),
    };

    let mut child = std::process::Command::new(cmd)
        .args(&args)
        .current_dir(working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to spawn {} for backend '{}': {}", cmd, backend, e))?;

    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout_buf = String::new();
                let mut stderr_buf = String::new();
                if let Some(mut s) = child.stdout.take() {
                    IoRead::read_to_string(&mut s, &mut stdout_buf).ok();
                }
                if let Some(mut s) = child.stderr.take() {
                    IoRead::read_to_string(&mut s, &mut stderr_buf).ok();
                }
                if status.success() {
                    return Ok(stdout_buf);
                } else {
                    let err_tail = if stderr_buf.len() > 500 {
                        &stderr_buf[stderr_buf.len()-500..]
                    } else {
                        &stderr_buf
                    };
                    return Err(format!("Exit code {}. Stderr: {}",
                        status.code().unwrap_or(-1), err_tail));
                }
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("Timed out after {}s", timeout_secs));
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            Err(e) => return Err(format!("Error waiting for process: {}", e)),
        }
    }
}

fn handle_run_workflow(_server: &Server, args: Value) -> Result<Value, String> {
    let name = args.get("name").and_then(|v| v.as_str())
        .ok_or("Missing 'name' parameter")?
        .to_string();

    let steps: Vec<WorkflowStep> = serde_json::from_value(
        args.get("steps").cloned().ok_or("Missing 'steps' parameter")?
    ).map_err(|e| format!("Invalid steps: {}", e))?;

    if steps.is_empty() {
        return Err("Workflow must have at least one step".into());
    }

    let max_total = args.get("max_total_attempts")
        .and_then(|v| v.as_u64())
        .unwrap_or(15) as u32;

    let log_results = args.get("log_results")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Build step lookup
    let step_map: HashMap<String, usize> = steps.iter().enumerate()
        .map(|(i, s)| (s.id.clone(), i))
        .collect();

    let mut results: Vec<Value> = Vec::new();
    let mut previous_output = String::new();
    let mut total_attempts: u32 = 0;
    let mut current_step_idx: usize = 0;
    let mut steps_completed: u32 = 0;

    loop {
        if current_step_idx >= steps.len() {
            break; // All steps done
        }

        if total_attempts >= max_total {
            return Ok(json!({
                "workflow": name,
                "status": "aborted",
                "steps_completed": steps_completed,
                "steps_total": steps.len(),
                "results": results,
                "error": format!("Global attempt limit ({}) reached", max_total)
            }));
        }

        let step = &steps[current_step_idx];
        let max_retries = step.max_retries.unwrap_or(2);
        let timeout_secs = step.timeout_secs.unwrap_or(300);
        let working_dir = step.working_dir.as_deref().unwrap_or(r"C:\Users\josep");

        // Replace {{previous_output}} in prompt
        let prompt = step.prompt.replace("{{previous_output}}", &previous_output);

        let mut success = false;
        let mut attempts: u32 = 0;
        let mut last_error = String::new();
        let mut output = String::new();
        let mut used_backend = step.backend.clone();

        // Try primary backend with retries
        for attempt in 0..=max_retries {
            if total_attempts >= max_total { break; }
            total_attempts += 1;
            attempts += 1;

            let attempt_prompt = if attempt == 0 {
                prompt.clone()
            } else {
                format!("{}\n\n[RETRY {}/{}] Previous attempt failed: {}", prompt, attempt, max_retries, last_error)
            };

            match run_workflow_step(&step.backend, &attempt_prompt, working_dir, timeout_secs) {
                Ok(out) => {
                    output = out;
                    success = true;
                    break;
                }
                Err(e) => {
                    last_error = e;
                }
            }
        }

        // Try alternative backends if primary failed
        if !success {
            if let Some(alts) = &step.alternatives {
                for alt_backend in alts {
                    if success || total_attempts >= max_total { break; }

                    for alt_attempt in 0..2u32 {
                        if total_attempts >= max_total { break; }
                        total_attempts += 1;
                        attempts += 1;

                        let alt_prompt = if alt_attempt == 0 {
                            format!("{}\n\n[ESCALATED from {}] Previous attempts failed: {}", prompt, step.backend, last_error)
                        } else {
                            format!("{}\n\n[ESCALATED from {}, RETRY] Previous error: {}", prompt, step.backend, last_error)
                        };

                        match run_workflow_step(alt_backend, &alt_prompt, working_dir, timeout_secs) {
                            Ok(out) => {
                                output = out;
                                used_backend = alt_backend.clone();
                                success = true;
                                break;
                            }
                            Err(e) => {
                                last_error = e;
                            }
                        }
                    }
                }
            }
        }

        // Record step result
        let output_preview = if output.len() > 500 {
            format!("{}...[{} chars total]", safe_truncate(&output, 500), output.len())
        } else {
            output.clone()
        };

        results.push(json!({
            "step_id": step.id,
            "backend": used_backend,
            "status": if success { "done" } else { "failed" },
            "attempts": attempts,
            "output_preview": output_preview,
            "error": if success { None } else { Some(&last_error) }
        }));

        if !success {
            return Ok(json!({
                "workflow": name,
                "status": "failed",
                "steps_completed": steps_completed,
                "steps_total": steps.len(),
                "results": results,
                "error": format!("Step '{}' failed after {} attempts: {}", step.id, attempts, last_error)
            }));
        }

        steps_completed += 1;
        previous_output = output;

        // Determine next step
        if let Some(next_id) = &step.on_success {
            match step_map.get(next_id) {
                Some(&idx) => current_step_idx = idx,
                None => {
                    return Ok(json!({
                        "workflow": name,
                        "status": "failed",
                        "steps_completed": steps_completed,
                        "steps_total": steps.len(),
                        "results": results,
                        "error": format!("on_success references unknown step '{}'", next_id)
                    }));
                }
            }
        } else {
            current_step_idx += 1;
        }
    }

    // Log to inbox if requested
    if log_results {
        let inbox_path = r"C:\My Drive\Volumes\multi_ai_coordination\inbox.md";
        if let Ok(mut content) = std::fs::read_to_string(inbox_path) {
            let entry = format!(
                "\n### [{date}] Workflow '{name}' completed\n**Source:** Manager MCP\n**For:** All backends\n**Detail:** {done}/{total} steps completed successfully.\n",
                date = Utc::now().format("%Y-%m-%d %H:%M"),
                name = name,
                done = steps_completed,
                total = steps.len()
            );
            content.push_str(&entry);
            let _ = std::fs::write(inbox_path, content);
        }
    }

    Ok(json!({
        "workflow": name,
        "status": "completed",
        "steps_completed": steps_completed,
        "steps_total": steps.len(),
        "results": results,
        "error": null
    }))
}

// ============================================================================
// Item 17: Parallel Workflow with Dependency Gates
// ============================================================================

/// Per-step status tracked during parallel workflow execution.
#[derive(Clone, Debug, Serialize)]
struct ParallelStepResult {
    step_id: String,
    status: String, // "pending", "running", "done", "failed", "skipped"
    backend: String,
    output: String,
    error: Option<String>,
    attempts: u32,
}

fn handle_run_parallel(server: &Server, args: Value) -> Result<Value, String> {
    let name = args.get("name").and_then(|v| v.as_str())
        .ok_or("Missing 'name' parameter")?
        .to_string();

    let steps: Vec<WorkflowStep> = serde_json::from_value(
        args.get("steps").cloned().ok_or("Missing 'steps' parameter")?
    ).map_err(|e| format!("Invalid steps: {}", e))?;

    if steps.is_empty() {
        return Err("Workflow must have at least one step".into());
    }

    let max_concurrent = args.get("max_concurrent")
        .and_then(|v| v.as_u64())
        .unwrap_or(3) as usize;

    let fail_fast = args.get("fail_fast")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Validate: all depends_on references exist
    let step_ids: std::collections::HashSet<&str> = steps.iter().map(|s| s.id.as_str()).collect();
    for step in &steps {
        for dep in &step.depends_on {
            if !step_ids.contains(dep.as_str()) {
                return Err(format!("Step '{}' depends_on unknown step '{}'", step.id, dep));
            }
        }
    }

    // Detect cycles with simple visited/in-stack DFS
    {
        let adj: HashMap<&str, &[String]> = steps.iter()
            .map(|s| (s.id.as_str(), s.depends_on.as_slice()))
            .collect();
        let mut visited = std::collections::HashSet::new();
        let mut stack = std::collections::HashSet::new();
        fn dfs<'a>(
            node: &'a str,
            adj: &HashMap<&'a str, &'a [String]>,
            visited: &mut std::collections::HashSet<&'a str>,
            stack: &mut std::collections::HashSet<&'a str>,
        ) -> bool {
            if stack.contains(node) { return true; } // cycle
            if visited.contains(node) { return false; }
            visited.insert(node);
            stack.insert(node);
            if let Some(deps) = adj.get(node) {
                for dep in *deps {
                    if dfs(dep.as_str(), adj, visited, stack) { return true; }
                }
            }
            stack.remove(node);
            false
        }
        for s in &steps {
            if dfs(s.id.as_str(), &adj, &mut visited, &mut stack) {
                return Err(format!("Dependency cycle detected involving step '{}'", s.id));
            }
        }
    }

    // Build shared state
    let step_results: Arc<RwLock<HashMap<String, ParallelStepResult>>> = Arc::new(RwLock::new(
        steps.iter().map(|s| (s.id.clone(), ParallelStepResult {
            step_id: s.id.clone(),
            status: "pending".into(),
            backend: s.backend.clone(),
            output: String::new(),
            error: None,
            attempts: 0,
        })).collect()
    ));

    let failed_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let steps_arc = Arc::new(steps.clone());

    // Async: spawn workflow, return immediately with workflow_id
    let rt = server.runtime.clone();
    let wf_id = format!("wf_{}", Uuid::new_v4().to_string()[..8].to_string());

    let wf_task = Task {
        id: wf_id.clone(), backend: Backend::ClaudeCode,
        prompt: format!("Parallel workflow: {}", name), system_prompt: None,
        model: None, working_dir: None, status: TaskStatus::Running,
        output: String::new(), error: None, created_at: Utc::now(),
        started_at: Some(Utc::now()), completed_at: None, progress_lines: 0,
        steps: Vec::new(), last_activity: Some(Utc::now()), stall_detected: false,
        extraction_status: ExtractionStatus::None, trust_score: 0,
        trust_level: TrustLevel::Low, rollback_path: None,
        validation_status: ValidationStatus::NotChecked, assertions: Vec::new(),
        backed_up_files: Vec::new(), retry_count: 0, max_retries: 0,
        retry_of: None, error_context: None, input_tokens: 0, output_tokens: 0, cost_usd: 0.0,
        on_complete: None,
        role: None,
        save_artifact: false,
        rerun_of: None,
        parent_task_id: None,
        forked_from: None,
        continuation_of: None,
        child_pid: None,
        watchdog_observations: Vec::new(),
    };
    rt.block_on(async { server.tasks.write().await.insert(wf_id.clone(), wf_task); });

    let tasks_ref = server.tasks.clone();
    let wf_id_bg = wf_id.clone();
    let steps_for_result = steps.clone();
    rt.spawn(async move {
        let result = run_parallel_workflow(
            steps_arc, step_results.clone(), failed_flag.clone(), max_concurrent, fail_fast,
        ).await;

        let final_results = step_results.read().await;
        let mut parts: Vec<String> = Vec::new();
        let (mut done_c, mut fail_c, mut skip_c) = (0u32, 0u32, 0u32);
        for step in &steps_for_result {
            if let Some(r) = final_results.get(&step.id) {
                match r.status.as_str() {
                    "done" => done_c += 1, "failed" => fail_c += 1, "skipped" => skip_c += 1, _ => {}
                }
                let preview = safe_truncate(&r.output, 300);
                parts.push(format!("[{}] {} ({}): {}", r.status, r.step_id, r.backend, preview));
            }
        }
        let overall = if fail_c == 0 && skip_c == 0 { "done" } else if done_c > 0 { "partial" } else { "failed" };
        let summary = format!("{}/{} done, {} failed, {} skipped\n{}", done_c, steps_for_result.len(), fail_c, skip_c, parts.join("\n"));

        let mut tasks = tasks_ref.write().await;
        if let Some(t) = tasks.get_mut(&wf_id_bg) {
            t.status = if overall == "done" { TaskStatus::Done } else { TaskStatus::Failed };
            t.output = summary;
            t.completed_at = Some(Utc::now());
            t.last_activity = Some(Utc::now());
            if let Some(err) = result.err() { t.error = Some(err); }
        }
    });

    return Ok(json!({
        "workflow_id": wf_id,
        "status": "running",
        "steps_total": steps.len(),
        "note": "Workflow running in background. Poll with get_status using workflow_id."
    }));

    // Dead code below (kept for reference, compiler may warn)
}

/// Async orchestrator: runs steps in parallel respecting dependency gates and concurrency limit.
async fn run_parallel_workflow(
    steps: Arc<Vec<WorkflowStep>>,
    results: Arc<RwLock<HashMap<String, ParallelStepResult>>>,
    failed_flag: Arc<std::sync::atomic::AtomicBool>,
    max_concurrent: usize,
    fail_fast: bool,
) -> Result<(), String> {
    let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent));
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(steps.len().max(1));

    // Seed: launch all steps with no dependencies
    let mut pending_count = steps.len();
    for step in steps.iter() {
        if step.depends_on.is_empty() {
            launch_step(
                step.clone(), steps.clone(), results.clone(),
                failed_flag.clone(), semaphore.clone(), tx.clone(), fail_fast,
            );
        }
    }

    // As each step completes, check what it unblocks
    while let Some(completed_id) = rx.recv().await {
        pending_count = pending_count.saturating_sub(1);
        if pending_count == 0 { break; }

        // Find steps that depended on the completed one
        for step in steps.iter() {
            if !step.depends_on.contains(&completed_id) {
                continue;
            }
            // Check if ALL deps are now done
            let store = results.read().await;
            let all_deps_met = step.depends_on.iter().all(|dep_id| {
                store.get(dep_id).map_or(false, |r| r.status == "done")
            });
            let any_dep_failed = step.depends_on.iter().any(|dep_id| {
                store.get(dep_id).map_or(false, |r| r.status == "failed" || r.status == "skipped")
            });
            let still_pending = store.get(&step.id).map_or(false, |r| r.status == "pending");
            drop(store);

            if !still_pending {
                continue;
            }

            if any_dep_failed {
                // Skip this step — a dependency failed
                let mut store = results.write().await;
                if let Some(r) = store.get_mut(&step.id) {
                    r.status = "skipped".into();
                    r.error = Some("Dependency failed".into());
                }
                drop(store);
                // Notify so downstream of this skipped step also get processed
                let _ = tx.send(step.id.clone()).await;
            } else if all_deps_met {
                launch_step(
                    step.clone(), steps.clone(), results.clone(),
                    failed_flag.clone(), semaphore.clone(), tx.clone(), fail_fast,
                );
            }
        }
    }

    Ok(())
}

/// Spawn a single workflow step on a background task.
fn launch_step(
    step: WorkflowStep,
    _all_steps: Arc<Vec<WorkflowStep>>,
    results: Arc<RwLock<HashMap<String, ParallelStepResult>>>,
    failed_flag: Arc<std::sync::atomic::AtomicBool>,
    semaphore: Arc<tokio::sync::Semaphore>,
    tx: tokio::sync::mpsc::Sender<String>,
    fail_fast: bool,
) {
    tokio::spawn(async move {
        // Acquire concurrency permit
        let _permit = semaphore.acquire().await;

        // Check fail_fast before starting
        if fail_fast && failed_flag.load(std::sync::atomic::Ordering::Relaxed) {
            let mut store = results.write().await;
            if let Some(r) = store.get_mut(&step.id) {
                r.status = "skipped".into();
                r.error = Some("Skipped due to fail_fast".into());
            }
            drop(store);
            let _ = tx.send(step.id.clone()).await;
            return;
        }

        // Mark running
        {
            let mut store = results.write().await;
            if let Some(r) = store.get_mut(&step.id) {
                r.status = "running".into();
            }
        }

        // Build prompt with {{step_id.output}} template substitution
        let prompt = {
            let store = results.read().await;
            let mut p = step.prompt.clone();
            // Replace {{previous_output}} with empty for parallel (no single predecessor)
            p = p.replace("{{previous_output}}", "");
            // Replace {{step_id.output}} references
            for (sid, sr) in store.iter() {
                let placeholder = format!("{{{{{}.output}}}}", sid);
                if p.contains(&placeholder) {
                    p = p.replace(&placeholder, &sr.output);
                }
            }
            p
        };

        let working_dir = step.working_dir.as_deref().unwrap_or(r"C:\Users\josep");
        let timeout_secs = step.timeout_secs.unwrap_or(300);
        let max_retries = step.max_retries.unwrap_or(2);

        let mut success = false;
        let mut attempts: u32 = 0;
        let mut last_error = String::new();
        let mut output = String::new();
        let mut used_backend = step.backend.clone();

        // Try primary backend with retries
        for attempt in 0..=max_retries {
            attempts += 1;
            let attempt_prompt = if attempt == 0 {
                prompt.clone()
            } else {
                format!("{}\n\n[RETRY {}/{}] Previous attempt failed: {}", prompt, attempt, max_retries, last_error)
            };

            // run_workflow_step is sync/blocking — run on blocking thread pool
            let backend = step.backend.clone();
            let wd = working_dir.to_string();
            let ap = attempt_prompt.clone();
            match tokio::task::spawn_blocking(move || {
                run_workflow_step(&backend, &ap, &wd, timeout_secs)
            }).await {
                Ok(Ok(out)) => { output = out; success = true; break; }
                Ok(Err(e)) => { last_error = e; }
                Err(e) => { last_error = format!("Join error: {}", e); break; }
            }
        }

        // Try alternative backends if primary failed
        if !success {
            if let Some(alts) = &step.alternatives {
                for alt_backend in alts {
                    if success { break; }
                    for alt_attempt in 0..2u32 {
                        attempts += 1;
                        let alt_prompt = if alt_attempt == 0 {
                            format!("{}\n\n[ESCALATED from {}] Previous attempts failed: {}", prompt, step.backend, last_error)
                        } else {
                            format!("{}\n\n[ESCALATED from {}, RETRY] Previous error: {}", prompt, step.backend, last_error)
                        };
                        let ab = alt_backend.clone();
                        let wd = working_dir.to_string();
                        let ap = alt_prompt.clone();
                        let ts = timeout_secs;
                        match tokio::task::spawn_blocking(move || {
                            run_workflow_step(&ab, &ap, &wd, ts)
                        }).await {
                            Ok(Ok(out)) => { output = out; used_backend = alt_backend.clone(); success = true; break; }
                            Ok(Err(e)) => { last_error = e; }
                            Err(e) => { last_error = format!("Join error: {}", e); break; }
                        }
                    }
                }
            }
        }

        // Update result
        {
            let mut store = results.write().await;
            if let Some(r) = store.get_mut(&step.id) {
                r.status = if success { "done".into() } else { "failed".into() };
                r.backend = used_backend;
                r.output = output;
                r.error = if success { None } else { Some(last_error) };
                r.attempts = attempts;
            }
        }

        if !success {
            failed_flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }

        // Notify orchestrator this step finished
        let _ = tx.send(step.id.clone()).await;
    });
}

// ============================================================================
// Product Layer: Decompose / Templates / Explain
// ============================================================================

fn handle_decompose_task(args: Value) -> Result<Value, String> {
    let prompt = args.get("prompt").and_then(|v| v.as_str())
        .ok_or("Missing 'prompt'")?;
    let working_dir = args.get("working_dir").and_then(|v| v.as_str());

    // Try numbered steps first: "1. do X  2. do Y"
    let mut raw_steps: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut found_numbered = false;
    for line in prompt.lines() {
        let trimmed = line.trim();
        if trimmed.len() > 2
            && trimmed.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
            && (trimmed.contains(". ") || trimmed.contains(") "))
        {
            if !buf.is_empty() {
                raw_steps.push(buf.trim().to_string());
                buf.clear();
            }
            let content = trimmed.splitn(2, |c: char| c == '.' || c == ')')
                .nth(1).unwrap_or(trimmed).trim();
            buf = content.to_string();
            found_numbered = true;
        } else {
            if !buf.is_empty() { buf.push(' '); }
            buf.push_str(trimmed);
        }
    }
    if !buf.is_empty() { raw_steps.push(buf.trim().to_string()); }

    // Fall back to connector splitting
    if !found_numbered || raw_steps.len() <= 1 {
        raw_steps.clear();
        let lower = prompt.to_lowercase();
        let connectors = [" then ", " and then ", " after that, ", " next, ", " finally "];
        let mut splits: Vec<(usize, usize)> = Vec::new();
        for conn in &connectors {
            let mut from = 0;
            while let Some(pos) = lower[from..].find(conn) {
                splits.push((from + pos, conn.len()));
                from = from + pos + conn.len();
            }
        }
        splits.sort_by_key(|s| s.0);
        if splits.is_empty() {
            raw_steps.push(prompt.to_string());
        } else {
            let mut last = 0;
            for (pos, len) in &splits {
                let chunk = prompt[last..*pos].trim();
                if !chunk.is_empty() { raw_steps.push(chunk.to_string()); }
                last = pos + len;
            }
            let tail = prompt[last..].trim();
            if !tail.is_empty() { raw_steps.push(tail.to_string()); }
        }
    }

    let mut steps = Vec::new();
    for (i, step_prompt) in raw_steps.iter().enumerate() {
        let rec = Server::recommend_backend(step_prompt, working_dir);
        steps.push(json!({
            "id": format!("step_{}", i + 1),
            "prompt": step_prompt,
            "recommended_backend": rec.recommended_backend,
            "confidence": rec.confidence,
            "reason": rec.reasoning,
            "depends_on": if i > 0 { vec![format!("step_{}", i)] } else { vec![] as Vec<String> },
        }));
    }

    Ok(json!({
        "original_prompt": prompt,
        "steps": steps,
        "total_steps": steps.len(),
        "note": if steps.len() == 1 { "Single-step task. Use submit_task directly." }
                else { "Multi-step task decomposed. Feed to run_parallel or run_workflow." }
    }))
}

fn handle_save_template(args: Value) -> Result<Value, String> {
    let name = args.get("name").and_then(|v| v.as_str()).ok_or("Missing 'name'")?;
    let description = args.get("description").and_then(|v| v.as_str()).ok_or("Missing 'description'")?;
    let steps: Vec<Value> = serde_json::from_value(
        args.get("steps").cloned().ok_or("Missing 'steps'")?
    ).map_err(|e| format!("Invalid steps: {}", e))?;
    let backend = args.get("backend").and_then(|v| v.as_str()).unwrap_or("claude_code");
    let params: HashMap<String, String> = args.get("parameters")
        .and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();

    let template = json!({
        "name": name, "description": description, "parameters": params,
        "steps": steps, "backend": backend, "trust_level": "auto_with_backup",
        "source": "manual", "times_used": 0, "last_used": "", "success_rate": 1.0,
    });

    std::fs::create_dir_all(workflow_patterns_dir()).map_err(|e| format!("mkdir: {}", e))?;
    let path = format!("{}\\{}.json", workflow_patterns_dir(), name.replace(' ', "_"));
    let content = serde_json::to_string_pretty(&template).map_err(|e| format!("json: {}", e))?;
    std::fs::write(&path, &content).map_err(|e| format!("write: {}", e))?;

    Ok(json!({ "saved": path, "name": name, "steps": steps.len(), "parameters": params.keys().collect::<Vec<_>>() }))
}

fn handle_list_templates(_args: Value) -> Result<Value, String> {
    if !std::path::Path::new(workflow_patterns_dir()).exists() {
        return Ok(json!({ "templates": [], "count": 0 }));
    }
    let mut templates = Vec::new();
    if let Ok(entries) = std::fs::read_dir(workflow_patterns_dir()) {
        for entry in entries.flatten() {
            if entry.path().extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(c) = std::fs::read_to_string(entry.path()) {
                    if let Ok(t) = serde_json::from_str::<Value>(&c) {
                        templates.push(json!({
                            "name": t.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                            "description": t.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                            "backend": t.get("backend").and_then(|v| v.as_str()).unwrap_or("?"),
                            "parameters": t.get("parameters"),
                            "times_used": t.get("times_used").and_then(|v| v.as_u64()).unwrap_or(0),
                            "success_rate": t.get("success_rate").and_then(|v| v.as_f64()).unwrap_or(1.0),
                            "file": entry.file_name().to_string_lossy().to_string(),
                        }));
                    }
                }
            }
        }
    }
    let count = templates.len();
    Ok(json!({ "templates": templates, "count": count }))
}

fn handle_run_template(server: &Server, args: Value) -> Result<Value, String> {
    let name = args.get("name").and_then(|v| v.as_str()).ok_or("Missing 'name'")?;
    let params: HashMap<String, String> = args.get("parameters")
        .and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let working_dir = args.get("working_dir").and_then(|v| v.as_str());

    let path = format!("{}\\{}.json", workflow_patterns_dir(), name.replace(' ', "_"));
    let file_content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Template '{}' not found: {}", name, e))?;
    let mut tmpl: Value = serde_json::from_str(&file_content)
        .map_err(|e| format!("Invalid template: {}", e))?;

    let steps = tmpl.get("steps").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let backend = tmpl.get("backend").and_then(|v| v.as_str()).unwrap_or("claude_code").to_string();

    let mut combined = String::new();
    for step in &steps {
        if let Some(p) = step.get("prompt").and_then(|v| v.as_str()) {
            let mut expanded = p.to_string();
            for (k, v) in &params { expanded = expanded.replace(&format!("{{{{{}}}}}", k), v); }
            if !combined.is_empty() { combined.push_str("\n\n"); }
            if let Some(id) = step.get("id").and_then(|v| v.as_str()) {
                combined.push_str(&format!("Step {}: ", id));
            }
            combined.push_str(&expanded);
        }
    }

    let wd = working_dir.map(String::from)
        .or_else(|| tmpl.get("working_dir").and_then(|v| v.as_str()).map(String::from))
        .unwrap_or_else(|| r"C:\rust-mcp".to_string());
    let submit_args = json!({ "prompt": combined, "backend": backend, "working_dir": wd });
    let result = handle_submit_task(server, submit_args)?;

    if let Some(used) = tmpl.get("times_used").and_then(|v| v.as_u64()) {
        tmpl["times_used"] = json!(used + 1);
    }
    tmpl["last_used"] = json!(chrono::Utc::now().to_rfc3339());
    if let Ok(updated) = serde_json::to_string_pretty(&tmpl) {
        let _ = std::fs::write(&path, updated);
    }

    Ok(json!({
        "template": name, "task_id": result.get("task_id"),
        "backend": backend, "parameters_applied": params, "steps_count": steps.len(),
    }))
}

fn handle_explain_task(server: &Server, args: Value) -> Result<Value, String> {
    let task_id = args.get("task_id").and_then(|v| v.as_str());
    let last_n = args.get("last").and_then(|v| v.as_u64()).unwrap_or(5) as usize;

    if let Some(tid) = task_id {
        let tasks = server.runtime.block_on(server.tasks.read());
        if let Some(task) = tasks.get(tid) {
            let elapsed = task.created_at.signed_duration_since(chrono::Utc::now()).num_seconds().unsigned_abs();
            let step_trail: Vec<String> = task.steps.iter()
                .map(|s| format!("{} ({})", s.tool, s.status)).collect();
            let explanation = format!(
                "Task {}: You asked to {}. Backend: {}. Status: {}. Duration: {}s.{}",
                tid,
                safe_truncate(&task.prompt, 120),
                task.backend, task.status, elapsed,
                if step_trail.is_empty() { String::new() }
                else { format!(" Steps: {}", step_trail.join(" -> ")) }
            );
            return Ok(json!({
                "task_id": tid, "explanation": explanation,
                "status": task.status.to_string(), "backend": task.backend.to_string(),
                "duration_secs": elapsed, "steps": task.steps.len(),
            }));
        }
        return Err(format!("Task '{}' not found in active tasks", tid));
    }

    // No task_id: summarize recent history
    let history_path = format!("{}\\task_history.json", history_dir());
    if let Ok(c) = std::fs::read_to_string(&history_path) {
        if let Ok(entries) = serde_json::from_str::<Vec<Value>>(&c) {
            let recent: Vec<&Value> = entries.iter().rev().take(last_n).collect();
            let summaries: Vec<String> = recent.iter().map(|e| {
                format!("{} - {} via {} ({} steps, {})",
                    e.get("task_id").and_then(|v| v.as_str()).unwrap_or("?"),
                    e.get("prompt_summary").and_then(|v| v.as_str()).unwrap_or("?"),
                    e.get("backend").and_then(|v| v.as_str()).unwrap_or("?"),
                    e.get("step_count").and_then(|v| v.as_u64()).unwrap_or(0),
                    e.get("status").and_then(|v| v.as_str()).unwrap_or("?"))
            }).collect();
            return Ok(json!({ "recent_tasks": last_n, "total_in_history": entries.len(), "summary": summaries.join("\n") }));
        }
    }
    Ok(json!({ "summary": "No task history found.", "recent_tasks": 0 }))
}

// ============================================================================
// Project Loaf — Multi-Task Coordination
// ============================================================================

fn loaf_path(loaf_id: &str) -> String {
    format!("{}\\{}.json", loaves_dir(), loaf_id)
}

/// Find the most recent active loaf in the loaves directory
fn find_active_loaf() -> Option<(String, Value)> {
    let dir = std::fs::read_dir(loaves_dir()).ok()?;
    let mut best: Option<(String, Value, String)> = None; // (id, value, created)
    for entry in dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") { continue; }
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(v) = serde_json::from_str::<Value>(&content) {
                if v.get("status").and_then(|s| s.as_str()) == Some("active") {
                    let created = v.get("created").and_then(|c| c.as_str()).unwrap_or("").to_string();
                    if best.as_ref().map_or(true, |(_, _, bc)| created > *bc) {
                        let id = v.get("loaf_id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                        best = Some((id, v, created));
                    }
                }
            }
        }
    }
    best.map(|(id, v, _)| (id, v))
}

fn handle_loaf_create(_server: &Server, params: Value) -> Result<Value, String> {
    let goal = params.get("goal").and_then(|v| v.as_str())
        .ok_or("Missing 'goal' parameter")?;
    let project_name = params.get("project_name").and_then(|v| v.as_str())
        .ok_or("Missing 'project_name' parameter")?;

    let _ = std::fs::create_dir_all(loaves_dir());

    let phases: Vec<Value> = if let Some(arr) = params.get("phases").and_then(|v| v.as_array()) {
        arr.iter().map(|p| {
            let name = p.as_str().unwrap_or("unnamed");
            json!({"name": name, "status": if arr.first() == Some(p) { "active" } else { "pending" }, "tasks": []})
        }).collect()
    } else {
        vec![json!({"name": "main", "status": "active", "tasks": []})]
    };

    let loaf_id = format!("{}_Loaf", project_name);
    let now = Utc::now().to_rfc3339();
    let loaf = json!({
        "loaf_id": loaf_id,
        "goal": goal,
        "created": now,
        "status": "active",
        "current_phase": 0,
        "phases": phases,
        "decisions": [],
        "discoveries": [],
        "next_actions": [],
        "breadcrumbs": [{"timestamp": now, "event": "Loaf created"}],
        "metadata": {"total_tasks": 0, "completed_tasks": 0, "total_cost_usd": 0.0}
    });

    let path = loaf_path(&loaf_id);
    std::fs::write(&path, serde_json::to_string_pretty(&loaf).unwrap())
        .map_err(|e| format!("Failed to write loaf: {}", e))?;

    Ok(json!({"loaf_id": loaf_id, "path": path, "status": "created"}))
}

fn handle_loaf_update(_server: &Server, params: Value) -> Result<Value, String> {
    let loaf_id = params.get("loaf_id").and_then(|v| v.as_str())
        .ok_or("Missing 'loaf_id' parameter")?;

    let path = loaf_path(loaf_id);
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read loaf '{}': {}", loaf_id, e))?;
    let mut loaf: Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse loaf: {}", e))?;

    let now = Utc::now().to_rfc3339();
    // Collect breadcrumb events, push them all at the end to avoid borrow conflicts
    let mut new_breadcrumbs: Vec<Value> = Vec::new();

    // Task update
    if let Some(task_update) = params.get("task_update") {
        let task_id = task_update.get("task_id").and_then(|t| t.as_str()).unwrap_or("").to_string();
        let status = task_update.get("status").and_then(|s| s.as_str()).unwrap_or("unknown").to_string();

        let phase_idx = loaf.get("current_phase").and_then(|p| p.as_u64()).unwrap_or(0) as usize;
        if let Some(phases) = loaf.get_mut("phases").and_then(|p| p.as_array_mut()) {
            if let Some(phase) = phases.get_mut(phase_idx) {
                if let Some(tasks) = phase.get_mut("tasks").and_then(|t| t.as_array_mut()) {
                    // Find existing task or add new one
                    let existing = tasks.iter_mut().find(|t| t.get("task_id").and_then(|i| i.as_str()) == Some(&task_id));
                    if let Some(t) = existing {
                        if let Some(s) = task_update.get("status") { t["status"] = s.clone(); }
                        if let Some(s) = task_update.get("output_summary") { t["output_summary"] = s.clone(); }
                        if let Some(s) = task_update.get("files_changed") { t["files_changed"] = s.clone(); }
                    } else {
                        tasks.push(task_update.clone());
                    }

                    // Update metadata counts
                    let total = tasks.len();
                    let completed = tasks.iter().filter(|t| t.get("status").and_then(|s| s.as_str()) == Some("done")).count();
                    loaf["metadata"]["total_tasks"] = json!(total);
                    loaf["metadata"]["completed_tasks"] = json!(completed);
                }
            }
        }
        new_breadcrumbs.push(json!({"timestamp": &now, "event": format!("Task {} -> {}", task_id, status)}));

        // Capture decisions from task
        if let Some(decisions) = task_update.get("decisions_made").and_then(|d| d.as_array()) {
            if let Some(loaf_decisions) = loaf.get_mut("decisions").and_then(|d| d.as_array_mut()) {
                for d in decisions {
                    loaf_decisions.push(json!({"what": d, "who": "delegated_task", "when": &now}));
                }
            }
        }
        // Capture discoveries from task
        if let Some(discoveries) = task_update.get("discoveries").and_then(|d| d.as_array()) {
            if let Some(loaf_discoveries) = loaf.get_mut("discoveries").and_then(|d| d.as_array_mut()) {
                for d in discoveries {
                    loaf_discoveries.push(json!({"what": d, "when": &now}));
                }
            }
        }
    }

    // Direct decision
    if let Some(decision) = params.get("decision") {
        let what = decision.get("what").and_then(|w| w.as_str()).unwrap_or("?").to_string();
        if let Some(decisions) = loaf.get_mut("decisions").and_then(|d| d.as_array_mut()) {
            decisions.push(json!({
                "what": decision.get("what"),
                "why": decision.get("why"),
                "who": decision.get("who").and_then(|w| w.as_str()).unwrap_or("manager"),
                "when": &now
            }));
        }
        new_breadcrumbs.push(json!({"timestamp": &now, "event": format!("Decision: {}", what)}));
    }

    // Direct discovery
    if let Some(discovery) = params.get("discovery") {
        let what = discovery.get("what").and_then(|w| w.as_str()).unwrap_or("?").to_string();
        if let Some(discoveries) = loaf.get_mut("discoveries").and_then(|d| d.as_array_mut()) {
            discoveries.push(json!({
                "what": discovery.get("what"),
                "impact": discovery.get("impact"),
                "when": &now
            }));
        }
        new_breadcrumbs.push(json!({"timestamp": &now, "event": format!("Discovery: {}", what)}));
    }

    // Replace next_actions
    if let Some(actions) = params.get("next_actions") {
        loaf["next_actions"] = actions.clone();
        new_breadcrumbs.push(json!({"timestamp": &now, "event": "Next actions updated"}));
    }

    // Advance phase
    if let Some(ps) = params.get("phase_status").and_then(|s| s.as_str()) {
        if ps == "done" {
            let phase_idx = loaf.get("current_phase").and_then(|p| p.as_u64()).unwrap_or(0) as usize;
            if let Some(phases) = loaf.get_mut("phases").and_then(|p| p.as_array_mut()) {
                if let Some(phase) = phases.get_mut(phase_idx) {
                    phase["status"] = json!("done");
                }
                let next = phase_idx + 1;
                if next < phases.len() {
                    phases[next]["status"] = json!("active");
                    let name = phases[next].get("name").and_then(|n| n.as_str()).unwrap_or("?").to_string();
                    new_breadcrumbs.push(json!({"timestamp": &now, "event": format!("Phase advanced to: {}", name)}));
                }
            }
            // Set current_phase outside the phases borrow
            let phase_idx = loaf.get("current_phase").and_then(|p| p.as_u64()).unwrap_or(0) as usize;
            let total_phases = loaf.get("phases").and_then(|p| p.as_array()).map(|p| p.len()).unwrap_or(0);
            if phase_idx + 1 < total_phases {
                loaf["current_phase"] = json!(phase_idx + 1);
            }
        }
    }

    // Now push all collected breadcrumbs at once
    if let Some(bc) = loaf.get_mut("breadcrumbs").and_then(|b| b.as_array_mut()) {
        bc.extend(new_breadcrumbs);
    }

    std::fs::write(&path, serde_json::to_string_pretty(&loaf).unwrap())
        .map_err(|e| format!("Failed to write loaf: {}", e))?;

    let phase_name = loaf.get("phases").and_then(|p| p.as_array())
        .and_then(|p| p.get(loaf.get("current_phase").and_then(|i| i.as_u64()).unwrap_or(0) as usize))
        .and_then(|p| p.get("name")).and_then(|n| n.as_str()).unwrap_or("?");

    Ok(json!({
        "loaf_id": loaf_id,
        "status": "updated",
        "current_phase": phase_name,
        "metadata": loaf.get("metadata")
    }))
}

fn handle_loaf_status(_server: &Server, params: Value) -> Result<Value, String> {
    let (loaf_id, loaf) = if let Some(id) = params.get("loaf_id").and_then(|v| v.as_str()) {
        let path = loaf_path(id);
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read loaf '{}': {}", id, e))?;
        let v: Value = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse loaf: {}", e))?;
        (id.to_string(), v)
    } else {
        find_active_loaf().ok_or("No active loaf found")?
    };

    let phase_idx = loaf.get("current_phase").and_then(|p| p.as_u64()).unwrap_or(0) as usize;
    let phase_name = loaf.get("phases").and_then(|p| p.as_array())
        .and_then(|p| p.get(phase_idx))
        .and_then(|p| p.get("name")).and_then(|n| n.as_str()).unwrap_or("?");

    let total = loaf.get("metadata").and_then(|m| m.get("total_tasks")).and_then(|t| t.as_u64()).unwrap_or(0);
    let completed = loaf.get("metadata").and_then(|m| m.get("completed_tasks")).and_then(|t| t.as_u64()).unwrap_or(0);

    let breadcrumbs = loaf.get("breadcrumbs").and_then(|b| b.as_array())
        .map(|b| {
            let skip = if b.len() > 5 { b.len() - 5 } else { 0 };
            b.iter().skip(skip).cloned().collect::<Vec<_>>()
        }).unwrap_or_default();

    Ok(json!({
        "loaf_id": loaf_id,
        "goal": loaf.get("goal"),
        "status": loaf.get("status"),
        "current_phase": phase_name,
        "total_tasks": total,
        "completed_tasks": completed,
        "pending_tasks": total - completed,
        "last_breadcrumbs": breadcrumbs,
        "next_actions": loaf.get("next_actions"),
        "decisions_count": loaf.get("decisions").and_then(|d| d.as_array()).map(|d| d.len()).unwrap_or(0),
        "discoveries_count": loaf.get("discoveries").and_then(|d| d.as_array()).map(|d| d.len()).unwrap_or(0)
    }))
}

fn handle_loaf_close(_server: &Server, params: Value) -> Result<Value, String> {
    let loaf_id = params.get("loaf_id").and_then(|v| v.as_str())
        .ok_or("Missing 'loaf_id' parameter")?;

    let path = loaf_path(loaf_id);
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read loaf '{}': {}", loaf_id, e))?;
    let mut loaf: Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse loaf: {}", e))?;

    let now = Utc::now().to_rfc3339();
    loaf["status"] = json!("completed");
    if let Some(bc) = loaf.get_mut("breadcrumbs").and_then(|b| b.as_array_mut()) {
        bc.push(json!({"timestamp": now, "event": "Loaf completed and archived"}));
    }

    let _ = std::fs::create_dir_all(loaves_archive_dir());
    let archive_path = format!("{}\\{}.json", loaves_archive_dir(), loaf_id);
    std::fs::write(&archive_path, serde_json::to_string_pretty(&loaf).unwrap())
        .map_err(|e| format!("Failed to write archive: {}", e))?;
    let _ = std::fs::remove_file(&path);

    let total = loaf.get("metadata").and_then(|m| m.get("total_tasks")).and_then(|t| t.as_u64()).unwrap_or(0);
    let completed = loaf.get("metadata").and_then(|m| m.get("completed_tasks")).and_then(|t| t.as_u64()).unwrap_or(0);

    Ok(json!({
        "loaf_id": loaf_id,
        "status": "archived",
        "archive_path": archive_path,
        "goal": loaf.get("goal"),
        "total_tasks": total,
        "completed_tasks": completed,
        "decisions": loaf.get("decisions").and_then(|d| d.as_array()).map(|d| d.len()).unwrap_or(0),
        "discoveries": loaf.get("discoveries").and_then(|d| d.as_array()).map(|d| d.len()).unwrap_or(0)
    }))
}

// ============================================================================
// List Sessions + Analytics
// ============================================================================

fn handle_list_sessions(_args: Value) -> Result<Value, String> {
    let session_dir = std::path::Path::new(SESSION_DIR);
    if !session_dir.exists() {
        return Ok(json!({"sessions": [], "count": 0}));
    }

    let mut sessions = Vec::new();
    for entry in std::fs::read_dir(session_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.path().is_dir() { continue; }
        let meta_path = entry.path().join("meta.json");
        if meta_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&meta_path) {
                if let Ok(meta) = serde_json::from_str::<Value>(&content) {
                    let session_id = entry.file_name().to_string_lossy().to_string();
                    let pid = meta.get("pid").and_then(|v| v.as_u64());
                    let alive = pid.map(|p| {
                        std::process::Command::new("tasklist")
                            .args(["/FI", &format!("PID eq {}", p), "/NH"])
                            .output()
                            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&p.to_string()))
                            .unwrap_or(false)
                    }).unwrap_or(false);

                    sessions.push(json!({
                        "session_id": session_id,
                        "alive": alive,
                        "pid": pid,
                        "model": meta.get("model"),
                        "working_dir": meta.get("working_dir"),
                        "started_at": meta.get("started_at"),
                        "prompt_preview": meta.get("prompt").and_then(|v| v.as_str()).map(|s| {
                            if s.len() > 100 { format!("{}...", &s[..100]) } else { s.to_string() }
                        }),
                    }));
                }
            }
        }
    }

    let count = sessions.len();
    Ok(json!({"sessions": sessions, "count": count}))
}

fn handle_get_analytics(server: &Server, args: Value) -> Result<Value, String> {
    let store = server.runtime.block_on(server.tasks.read());
    let backend_filter = args.get("backend").and_then(|v| v.as_str()).map(|s| s.to_string());
    let since = args.get("since").and_then(|v| v.as_str()).map(|s| s.to_string());

    // (total, success, cost, total_duration_ms)
    let mut by_backend: std::collections::HashMap<String, (u32, u32, f64, u64)> = std::collections::HashMap::new();
    let mut total_tasks = 0u32;
    let mut total_success = 0u32;
    let mut total_cost = 0.0f64;
    let mut total_tokens_in = 0u64;
    let mut total_tokens_out = 0u64;
    let mut recent_failures: Vec<Value> = Vec::new();

    for task in store.values() {
        let backend_str = task.backend.to_string();

        if let Some(ref bf) = backend_filter {
            if backend_str != *bf { continue; }
        }
        if let Some(ref since_str) = since {
            if let Ok(since_dt) = chrono::DateTime::parse_from_rfc3339(since_str) {
                if task.created_at < since_dt { continue; }
            }
        }

        total_tasks += 1;
        total_cost += task.cost_usd;
        total_tokens_in += task.input_tokens;
        total_tokens_out += task.output_tokens;

        let is_success = task.status == TaskStatus::Done;
        if is_success { total_success += 1; }

        let entry = by_backend.entry(backend_str.clone()).or_insert((0, 0, 0.0, 0));
        entry.0 += 1;
        if is_success { entry.1 += 1; }
        entry.2 += task.cost_usd;

        // Calculate duration from started_at -> completed_at
        if let (Some(started), Some(completed)) = (&task.started_at, &task.completed_at) {
            let duration_ms = (*completed - *started).num_milliseconds().max(0) as u64;
            entry.3 += duration_ms;
        }

        if task.status == TaskStatus::Failed && recent_failures.len() < 5 {
            recent_failures.push(json!({
                "task_id": task.id,
                "backend": backend_str,
                "error_preview": task.error.as_deref().map(|s| {
                    if s.len() > 150 { format!("{}...", &s[..150]) } else { s.to_string() }
                }),
            }));
        }
    }

    let backend_stats: Vec<Value> = by_backend.iter().map(|(backend, (total, success, cost, duration_ms))| {
        json!({
            "backend": backend,
            "total_tasks": total,
            "successful": success,
            "success_rate": if *total > 0 { format!("{:.0}%", (*success as f64 / *total as f64) * 100.0) } else { "N/A".to_string() },
            "total_cost_usd": format!("{:.4}", cost),
            "avg_duration_secs": if *total > 0 { format!("{:.1}", (*duration_ms as f64 / *total as f64) / 1000.0) } else { "N/A".to_string() },
        })
    }).collect();

    Ok(json!({
        "total_tasks": total_tasks,
        "total_successful": total_success,
        "overall_success_rate": if total_tasks > 0 { format!("{:.0}%", (total_success as f64 / total_tasks as f64) * 100.0) } else { "N/A".to_string() },
        "total_cost_usd": format!("{:.4}", total_cost),
        "total_tokens": {"input": total_tokens_in, "output": total_tokens_out},
        "by_backend": backend_stats,
        "recent_failures": recent_failures,
    }))
}

// ============================================================================
// §12: Specialist Role Prompts
// ============================================================================

fn get_role_prompt(role: &str) -> Option<&'static str> {
    match role {
        "architect" => Some(
            "You are a software architect. Focus on system design, component boundaries, \
             data flow, and API contracts. Evaluate trade-offs between approaches. \
             Produce diagrams or pseudocode, not full implementations. \
             Flag coupling risks and scalability concerns."
        ),
        "implementer" => Some(
            "You are an implementer. Write production-quality code that follows existing \
             patterns in the codebase. Keep changes minimal and focused. \
             Run builds and tests after changes. Report what files you modified."
        ),
        "tester" => Some(
            "You are a test engineer. Write thorough tests covering happy paths, edge cases, \
             and error conditions. Prefer integration tests over mocks. \
             Report coverage gaps and suggest additional test scenarios."
        ),
        "reviewer" => Some(
            "You are a code reviewer. Read the code carefully and identify bugs, security \
             issues, performance problems, and style violations. Be specific about line \
             numbers and suggest concrete fixes. Prioritize findings by severity."
        ),
        "documenter" => Some(
            "You are a technical writer. Write clear, concise documentation for the code \
             and systems you examine. Produce READMEs, inline comments, API docs, and \
             architecture decision records as appropriate. Target the intended audience."
        ),
        "debugger" => Some(
            "You are a debugger. Systematically narrow down the root cause of issues. \
             Add logging, inspect state, form hypotheses, and test them. \
             Document the investigation path so others can follow your reasoning."
        ),
        "security" => Some(
            "You are a security analyst. Review code for OWASP top 10 vulnerabilities, \
             injection risks, authentication/authorization flaws, and data exposure. \
             Check dependencies for known CVEs. Report findings with severity ratings."
        ),
        _ => None,
    }
}

fn list_roles() -> Vec<Value> {
    vec![
        json!({"name": "architect", "description": "System design, component boundaries, trade-off analysis"}),
        json!({"name": "implementer", "description": "Production-quality code following existing patterns"}),
        json!({"name": "tester", "description": "Thorough test coverage: happy paths, edge cases, error conditions"}),
        json!({"name": "reviewer", "description": "Code review: bugs, security, performance, style"}),
        json!({"name": "documenter", "description": "Technical writing: READMEs, API docs, ADRs"}),
        json!({"name": "debugger", "description": "Systematic root cause analysis and investigation"}),
        json!({"name": "security", "description": "Security audit: OWASP, injection, auth, CVEs"}),
    ]
}

// Custom YAML role support
#[derive(Deserialize)]
struct CustomRole {
    name: String,
    prompt: String,
    #[serde(default)]
    expertise: Vec<String>,
}

fn custom_roles_dir() -> std::path::PathBuf {
    let volumes = std::env::var("VOLUMES_PATH")
        .unwrap_or_else(|_| r"C:\My Drive\Volumes".to_string());
    std::path::PathBuf::from(volumes).join("scripts").join("roles")
}

fn load_custom_roles() -> Vec<CustomRole> {
    let dir = custom_roles_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else { return vec![] };
    entries.filter_map(|e| {
        let e = e.ok()?;
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("yaml") { return None; }
        let content = std::fs::read_to_string(&path).ok()?;
        serde_yaml::from_str::<CustomRole>(&content).ok()
    }).collect()
}

fn get_custom_role_prompt(role: &str) -> Option<String> {
    let path = custom_roles_dir().join(format!("{}.yaml", role));
    let content = std::fs::read_to_string(&path).ok()?;
    let cr: CustomRole = serde_yaml::from_str(&content).ok()?;
    Some(cr.prompt)
}

fn handle_role_list(_args: Value) -> Result<Value, String> {
    let mut roles = list_roles();
    let custom = load_custom_roles();
    let custom_count = custom.len();
    for cr in custom {
        // Custom roles override built-in with same name
        roles.retain(|r| r.get("name").and_then(|n| n.as_str()) != Some(&cr.name));
        roles.push(json!({
            "name": cr.name,
            "description": cr.expertise.join(", "),
            "custom": true,
        }));
    }
    let count = roles.len();
    Ok(json!({ "roles": roles, "count": count, "built_in": 7, "custom": custom_count, "note": "Pass role name to task_submit's 'role' parameter" }))
}

fn handle_role_create(args: Value) -> Result<Value, String> {
    let name = args.get("name").and_then(|v| v.as_str())
        .ok_or("Missing required param: name")?;
    let prompt = args.get("prompt").and_then(|v| v.as_str())
        .ok_or("Missing required param: prompt")?;
    let expertise: Vec<String> = args.get("expertise")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    // Validate name (alphanumeric + underscore only)
    if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err("Role name must be alphanumeric/underscore only".into());
    }

    let dir = custom_roles_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create roles dir: {}", e))?;

    let yaml_content = format!(
        "name: {}\nprompt: |\n{}\nexpertise:\n{}",
        name,
        prompt.lines().map(|l| format!("  {}", l)).collect::<Vec<_>>().join("\n"),
        expertise.iter().map(|e| format!("  - {}", e)).collect::<Vec<_>>().join("\n"),
    );

    let path = dir.join(format!("{}.yaml", name));
    std::fs::write(&path, &yaml_content).map_err(|e| format!("Failed to write role: {}", e))?;
    info!("Custom role created: {}", name);
    Ok(json!({ "created": name, "path": path.display().to_string() }))
}

fn handle_role_delete(args: Value) -> Result<Value, String> {
    let name = args.get("name").and_then(|v| v.as_str())
        .ok_or("Missing required param: name")?;
    let path = custom_roles_dir().join(format!("{}.yaml", name));
    if !path.exists() {
        return Err(format!("Custom role '{}' not found", name));
    }
    std::fs::remove_file(&path).map_err(|e| format!("Failed to delete: {}", e))?;
    info!("Custom role deleted: {}", name);
    Ok(json!({ "deleted": name }))
}

// ============================================================================
// §13: Auto-Artifact Saving
// ============================================================================

fn save_task_artifact(task: &Task) {
    if !task.save_artifact { return; }
    if task.status != TaskStatus::Done { return; }

    let volumes_path = std::env::var("VOLUMES_PATH")
        .unwrap_or_else(|_| r"C:\My Drive\Volumes".to_string());
    let artifacts_dir = std::path::Path::new(&volumes_path).join("artifacts");
    if std::fs::create_dir_all(&artifacts_dir).is_err() { return; }

    let date = Utc::now().format("%Y-%m-%d").to_string();
    let role_tag = task.role.as_deref().unwrap_or("none");
    let filename = format!("{}_{}_{}_{}.md", date, task.id, task.backend, role_tag);
    let path = artifacts_dir.join(&filename);

    let prompt_preview: String = task.prompt.chars().take(200).collect();
    let content = format!(
        "# Task Artifact: {}\n\
         - Backend: {}\n\
         - Role: {}\n\
         - Date: {}\n\
         - Prompt: {}\n\
         ---\n\n{}\n",
        task.id,
        task.backend,
        role_tag,
        date,
        prompt_preview,
        task.output
    );

    let _ = std::fs::write(&path, &content);
    info!("Artifact saved: {}", path.display());
}

// ============================================================================
// Tool Dispatch
// ============================================================================

fn handle_tool_call(server: &Server, tool: &str, args: Value) -> Result<Value, String> {
    match tool {
        "task_submit" | "submit_task" => handle_submit_task(server, args),
        "task_status" | "get_status" => handle_get_status(server, args),
        "task_watch" | "watch_tasks" => handle_watch_tasks(server, args),
        "task_output" | "get_output" => handle_get_output(server, args),
        "task_list" | "list_tasks" => handle_list_tasks(server, args),
        "task_cancel" | "cancel_task" => handle_cancel_task(server, args),
        "task_poll" => handle_task_poll(server, args),
        "pause_task" => handle_pause_task(server, args),
        "resume_task" => handle_resume_task(server, args),
        "configure" => handle_configure(server, args),
        "task_cleanup" | "cleanup" => handle_cleanup(server, args),
        "session_start" | "start_session" => handle_start_session(server, args),
        "session_send" | "send_to_session" => handle_send_to_session(server, args),
        "open_terminal" => handle_open_terminal(args),
        "gemini_direct" => handle_gemini_direct(args),
        "codex_exec" => handle_codex_exec(args),
        "codex_review" => handle_codex_review(args),
        "workflow_run" | "run_workflow" => handle_run_workflow(server, args),
        "task_run_parallel" | "run_parallel" => handle_run_parallel(server, args),
        "review_extractions" => handle_review_extractions(server, args),
        "extract_workflow" => handle_extract_workflow(server, args),
        "dismiss_extraction" => handle_dismiss_extraction(server, args),
        "task_rollback" | "rollback_task" => handle_rollback_task(server, args),
        "task_retry" | "retry_task" => handle_retry_task(server, args),
        "task_rerun" | "rerun_task" => handle_task_rerun(server, args),
        "task_route" | "route_task" => handle_route_task(args),
        "task_decompose" | "decompose_task" => handle_decompose_task(args),
        "template_save" | "save_template" => handle_save_template(args),
        "template_list" | "list_templates" => handle_list_templates(args),
        "template_run" | "run_template" => handle_run_template(server, args),
        "task_explain" | "explain_task" => handle_explain_task(server, args),
        "create_loaf" => handle_loaf_create(server, args),
        "loaf_update" => handle_loaf_update(server, args),
        "loaf_status" => handle_loaf_status(server, args),
        "loaf_close" => handle_loaf_close(server, args),
        "session_list" | "list_sessions" => handle_list_sessions(args),
        "get_analytics" => handle_get_analytics(server, args),
        "role_list" | "list_roles" => handle_role_list(args),
        "role_create" | "create_role" => handle_role_create(args),
        "role_delete" | "delete_role" => handle_role_delete(args),
        _ => Err(format!("Unknown tool: {}", tool)),
    }
}

// ============================================================================
// Item 13/14: Extraction Tools
// ============================================================================

fn handle_review_extractions(server: &Server, _args: Value) -> Result<Value, String> {
    let store = server.runtime.block_on(server.tasks.read());
    let pending: Vec<Value> = store.values()
        .filter(|t| t.extraction_status == ExtractionStatus::PendingSuccess || t.extraction_status == ExtractionStatus::PendingFailure)
        .map(|t| {
            let prompt_summary: String = safe_truncate(&t.prompt, 200);
            let steps_detail: Vec<Value> = t.steps.iter().map(|s| json!({"tool": s.tool, "status": s.status, "summary": s.summary.as_deref().unwrap_or("")})).collect();
            json!({
                "task_id": t.id, "backend": t.backend,
                "extraction_type": if t.extraction_status == ExtractionStatus::PendingSuccess { "success_pattern" } else { "failure_anti_pattern" },
                "prompt": prompt_summary,
                "output_preview": safe_truncate(&t.output, 300),
                "error": t.error, "step_count": t.steps.len(), "steps": steps_detail,
                "duration_secs": t.started_at.and_then(|s| t.completed_at.map(|c| (c - s).num_seconds())),
            })
        }).collect();
    Ok(json!({"pending_count": pending.len(), "candidates": pending, "instructions": "For each: 3Q check (Reusable? Specific? New?). Yes=extract_workflow. No=dismiss_extraction."}))
}

fn handle_extract_workflow(server: &Server, args: Value) -> Result<Value, String> {
    let task_id = args.get("task_id").and_then(|v| v.as_str()).ok_or("Missing 'task_id'")?;
    let name = args.get("name").and_then(|v| v.as_str()).ok_or("Missing 'name'")?;
    let description = args.get("description").and_then(|v| v.as_str()).ok_or("Missing 'description'")?;
    let pattern_type = args.get("pattern_type").and_then(|v| v.as_str()).unwrap_or("workflow");
    let mut store = server.runtime.block_on(server.tasks.write());
    let task = store.get_mut(task_id).ok_or(format!("Task '{}' not found", task_id))?;
    let steps_summary: Vec<String> = task.steps.iter().map(|s| {
        let summary = s.summary.as_deref().unwrap_or("");
        format!("{}: {}", s.tool, safe_truncate(summary, 100))
    }).collect();
    let pattern = json!({"name": name, "description": description, "pattern_type": pattern_type, "steps": steps_summary, "backend": task.backend, "source_task_id": task.id, "original_prompt": task.prompt, "error": task.error, "extracted_at": Utc::now().to_rfc3339(), "times_used": 0, "success_rate": if task.status == TaskStatus::Done { 1.0_f64 } else { 0.0_f64 }});
    let _ = std::fs::create_dir_all(workflow_patterns_dir());
    let pattern_path = format!("{}\\{}.json", workflow_patterns_dir(), name);
    let data = serde_json::to_string_pretty(&pattern).map_err(|e| e.to_string())?;
    std::fs::write(&pattern_path, &data).map_err(|e| format!("Failed to write: {}", e))?;
    task.extraction_status = ExtractionStatus::Extracted;
    Server::persist_task(task);
    Ok(json!({"status": "extracted", "pattern_name": name, "pattern_path": pattern_path, "pattern_type": pattern_type}))
}

fn handle_dismiss_extraction(server: &Server, args: Value) -> Result<Value, String> {
    let task_id = args.get("task_id").and_then(|v| v.as_str()).ok_or("Missing 'task_id'")?;
    let reason = args.get("reason").and_then(|v| v.as_str()).unwrap_or("not extractable");
    let mut store = server.runtime.block_on(server.tasks.write());
    let task = store.get_mut(task_id).ok_or(format!("Task '{}' not found", task_id))?;
    task.extraction_status = ExtractionStatus::Dismissed;
    Server::persist_task(task);
    Ok(json!({"status": "dismissed", "task_id": task_id, "reason": reason}))
}

fn handle_rollback_task(server: &Server, args: Value) -> Result<Value, String> {
    let task_id = args.get("task_id").and_then(|v| v.as_str()).ok_or("Missing task_id")?;
    let store = server.runtime.block_on(server.tasks.read());
    let task = store.get(task_id).ok_or("Task not found")?;
    let restored = Server::rollback(task)?;
    Ok(json!({"rolled_back": true, "files_restored": restored, "task_id": task_id}))
}

/// Item 18: Manually trigger a retry for a failed task.
fn handle_retry_task(server: &Server, args: Value) -> Result<Value, String> {
    let task_id = args.get("task_id").and_then(|v| v.as_str()).ok_or("Missing 'task_id'")?;
    let inject_context = args.get("inject_context").and_then(|v| v.as_str());

    let mut store = server.runtime.block_on(server.tasks.write());
    let task = store.get_mut(task_id).ok_or(format!("Task '{}' not found", task_id))?;

    if task.status != TaskStatus::Failed {
        return Err(format!("Task '{}' is {} - can only retry failed tasks", task_id, task.status));
    }

    // Override max_retries to allow manual retry even if limit reached
    task.max_retries = task.retry_count + 1;

    // Inject extra context if provided
    if let Some(ctx) = inject_context {
        let current_error = task.error.clone().unwrap_or_default();
        task.error = Some(format!("{}\n\nAdditional context: {}", current_error, ctx));
    }

    let retry = Server::prepare_retry(task)
        .ok_or("Failed to prepare retry")?;

    let retry_id = retry.id.clone();
    let retry_backend = retry.backend.to_string();
    Server::persist_task(task);

    store.insert(retry_id.clone(), retry.clone());
    Server::persist_task(&retry);
    drop(store);

    spawn_retry_execution(&retry, server.tasks.clone(), Some(server.config.clone()), &server.runtime);

    Ok(json!({
        "retry_task_id": retry_id,
        "original_task_id": task_id,
        "backend": retry_backend,
        "status": "queued",
        "message": format!("Retry task {} created from failed task {}", retry_id, task_id)
    }))
}

// ============================================================================
// Task Rerun — re-submit a completed task with optional new context
// ============================================================================

fn handle_task_rerun(server: &Server, args: Value) -> Result<Value, String> {
    let task_id = args.get("task_id").and_then(|v| v.as_str())
        .ok_or("Missing required 'task_id'")?;
    let additional_context = args.get("additional_context").and_then(|v| v.as_str());
    let backend_override = args.get("backend_override").and_then(|v| v.as_str());
    let include_files: Vec<String> = args.get("include_files")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    // Look up original task
    let store = server.runtime.block_on(server.tasks.read());
    let original = store.get(task_id)
        .ok_or(format!("Task '{}' not found", task_id))?;

    if original.status != TaskStatus::Done {
        return Err(format!(
            "Task '{}' is {} — task_rerun requires a completed (done) task. Use task_retry for failed tasks.",
            task_id, original.status
        ));
    }

    // Capture what we need from the original before dropping the lock
    let original_prompt = original.prompt.clone();
    let original_backend = if let Some(ovr) = backend_override {
        match ovr {
            "gpt" => Backend::Gpt,
            "gemini" => Backend::Gemini,
            "claude_code" | "claude" => Backend::ClaudeCode,
            "codex" => Backend::Codex,
            _ => return Err(format!("Unknown backend_override '{}'. Use: gpt, gemini, claude_code, codex", ovr)),
        }
    } else {
        original.backend.clone()
    };
    let original_system_prompt = original.system_prompt.clone();
    let original_model = original.model.clone();
    let original_working_dir = original.working_dir.clone();
    let original_role = original.role.clone();
    let original_on_complete = original.on_complete.clone();
    let original_task_id = original.id.clone();
    drop(store);

    // Build file contents section
    let mut file_section = String::new();
    let mut files_loaded: usize = 0;
    let mut files_skipped: Vec<String> = Vec::new();
    for path in &include_files {
        match std::fs::read_to_string(path) {
            Ok(contents) => {
                file_section.push_str(&format!(
                    "\n\n--- Current state of {} ---\n{}\n--- End {} ---",
                    path, contents, path
                ));
                files_loaded += 1;
            }
            Err(e) => {
                eprintln!("task_rerun: skipping missing file {}: {}", path, e);
                files_skipped.push(path.clone());
            }
        }
    }

    // Construct new prompt
    let mut new_prompt = original_prompt;
    if !file_section.is_empty() {
        new_prompt.push_str(&file_section);
    }
    if let Some(ctx) = additional_context {
        new_prompt.push_str(&format!("\n\n[Additional context for rerun]\n{}", ctx));
    }

    // Create new task via internal submit
    let new_task_id = Uuid::new_v4().to_string()[..8].to_string();
    let new_task = Task {
        id: new_task_id.clone(),
        backend: original_backend.clone(),
        prompt: new_prompt,
        system_prompt: original_system_prompt,
        model: original_model,
        working_dir: original_working_dir.clone(),
        status: TaskStatus::Queued,
        output: String::new(),
        error: None,
        created_at: Utc::now(),
        started_at: None,
        completed_at: None,
        progress_lines: 0, steps: Vec::new(), last_activity: None, stall_detected: false,
        extraction_status: ExtractionStatus::None,
        trust_score: 0, trust_level: TrustLevel::Low, rollback_path: None,
        validation_status: ValidationStatus::NotChecked, assertions: Vec::new(),
        backed_up_files: Vec::new(), retry_count: 0, max_retries: 2,
        retry_of: None, error_context: None,
        input_tokens: 0, output_tokens: 0, cost_usd: 0.0,
        on_complete: original_on_complete,
        role: original_role,
        save_artifact: true,
        rerun_of: Some(original_task_id.clone()),
        parent_task_id: Some(original_task_id.clone()),
        forked_from: None,
        continuation_of: None,
        child_pid: None,
        watchdog_observations: Vec::new(),
    };

    // Store and persist
    let tasks = server.tasks.clone();
    let config = server.config.clone();
    server.runtime.block_on(async {
        let mut store = tasks.write().await;
        store.insert(new_task_id.clone(), new_task.clone());
    });
    Server::persist_task(&new_task);

    // Spawn execution (same pattern as task_submit)
    let tasks_bg = server.tasks.clone();
    let tid = new_task_id.clone();
    let be = original_backend.clone();
    let prompt_for_spawn = new_task.prompt.clone();
    let model_for_spawn = new_task.model.clone();
    match be {
        Backend::Gpt => {
            server.runtime.spawn(run_gpt_task(config, tasks_bg, tid));
        }
        Backend::ClaudeCode => {
            let mut args = vec![
                "-p".to_string(),
                prompt_for_spawn,
                "--dangerously-skip-permissions".to_string(),
                "--verbose".to_string(),
                "--output-format".to_string(), "stream-json".to_string(),
                "--add-dir".to_string(), r"C:\temp".to_string(),
                "--add-dir".to_string(), r"C:\My Drive\Volumes".to_string(),
                "--add-dir".to_string(), r"C:\CPC".to_string(),
                "--add-dir".to_string(), r"C:\rust-mcp".to_string(),
            ];
            if let Some(m) = model_for_spawn {
                args.push("--model".to_string());
                args.push(m);
            }
            if let Some(ref wd) = original_working_dir {
                args.push("--add-dir".to_string());
                args.push(wd.clone());
            }
            server.runtime.spawn(run_cli_task(tasks_bg, tid, claude_code_cmd(), args));
        }
        Backend::Gemini => {
            let mut args = vec![
                gemini_cmd().to_string(),
                "-p".to_string(),
                prompt_for_spawn,
                "--yolo".to_string(),
            ];
            if let Some(m) = model_for_spawn {
                args.push("--model".to_string());
                args.push(m);
            }
            server.runtime.spawn(run_cli_task(tasks_bg, tid, r"C:\Program Files\nodejs\node.exe", args));
        }
        Backend::Codex => {
            let wd = original_working_dir.unwrap_or_else(|| r"C:\rust-mcp".to_string());
            let args = vec![
                "exec".into(), "--json".into(), "--skip-git-repo-check".into(),
                "--full-auto".into(), "--cd".into(), wd.clone(), prompt_for_spawn,
            ];
            server.runtime.spawn(run_codex_task(tasks_bg, tid, args, wd));
        }
    }

    Ok(json!({
        "new_task_id": new_task_id,
        "rerun_of": original_task_id,
        "backend": original_backend.to_string(),
        "status": "queued",
        "include_files_loaded": files_loaded,
        "include_files_skipped": files_skipped,
        "has_additional_context": additional_context.is_some(),
        "message": format!("Rerun task {} created from completed task {}", new_task_id, original_task_id)
    }))
}

// ============================================================================
// Item 16: Task Routing
// ============================================================================

fn handle_route_task(args: Value) -> Result<Value, String> {
    let prompt = args.get("prompt").and_then(|v| v.as_str()).ok_or("Missing 'prompt'")?;
    let working_dir = args.get("working_dir").and_then(|v| v.as_str());
    let rec = Server::recommend_backend(prompt, working_dir);
    Ok(json!({
        "recommended_backend": rec.recommended_backend,
        "confidence": rec.confidence,
        "reasoning": rec.reasoning,
        "alternatives": rec.alternatives,
    }))
}

// ============================================================================
// Tools List
// ============================================================================

fn get_tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "task_submit",
                "description": "Submit a task to GPT (reasoning), Gemini CLI (coding), Claude Code (coding), or Codex (coding with safety controls). Returns task_id immediately. The task runs in background - poll with get_status. Set auto_route: true to automatically select the best backend.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "backend": {
                            "type": "string",
                            "enum": ["gpt", "gemini", "claude_code", "codex"],
                            "description": "Which AI backend: 'gpt' for OpenAI reasoning, 'gemini' for Gemini CLI, 'claude_code' for Claude Code, 'codex' for OpenAI Codex"
                        },
                        "prompt": {
                            "type": "string",
                            "description": "The task prompt / instructions"
                        },
                        "system_prompt": {
                            "type": "string",
                            "description": "Optional system prompt (GPT only)"
                        },
                        "model": {
                            "type": "string",
                            "description": "Model override. GPT: o3, gpt-4o, etc. Gemini: gemini-2.5-pro, etc. Claude Code: uses default"
                        },
                        "working_dir": {
                            "type": "string",
                            "description": "Working directory for CLI backends (Gemini, Claude Code)"
                        },
                        "visible": {
                            "type": "boolean",
                            "description": "Override terminal visibility. true = also spawn visible terminal, false = background only. Defaults to dashboard_prefs.json setting."
                        },
                        "auto_route": {
                            "type": "boolean",
                            "description": "If true and no backend specified, automatically select the best backend based on prompt analysis, history, and learned patterns."
                        },
                        "estimated_secs": {
                            "type": "integer",
                            "description": "Estimated duration in seconds (informational only, no enforcement). Surfaced in task_poll status_bar."
                        },
                        "on_complete": {
                            "type": "string",
                            "description": "Prompt for a follow-up task to auto-submit when this task completes successfully. The new task inherits backend, working_dir, and model."
                        },
                        "role": {
                            "type": "string",
                            "enum": ["architect", "implementer", "tester", "reviewer", "documenter", "debugger", "security"],
                            "description": "Specialist role: injects a role-specific system prompt and sets CPC_AGENT_ROLE env var for attribution."
                        },
                        "save_artifact": {
                            "type": "boolean",
                            "description": "Save task output as a markdown artifact in Volumes/artifacts/ on completion. Default: true."
                        }
                    },
                    "required": ["prompt"]
                }
            },
            {
                "name": "task_status",
                "description": "Check status of a task. Returns status (queued/running/done/failed/cancelled/paused), progress, elapsed time, and output preview.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "task_id": {
                            "type": "string",
                            "description": "Task ID from submit_task"
                        }
                    },
                    "required": ["task_id"]
                }
            },
            {
                "name": "task_output",
                "description": "Get full output from a task. Use 'tail' to get only last N lines.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "task_id": {
                            "type": "string",
                            "description": "Task ID"
                        },
                        "tail": {
                            "type": "integer",
                            "description": "Only return last N lines (optional)"
                        }
                    },
                    "required": ["task_id"]
                }
            },
            {
                "name": "task_list",
                "description": "List all tasks. Filter by status or backend.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "status": {
                            "type": "string",
                            "enum": ["queued", "running", "done", "failed", "cancelled"],
                            "description": "Filter by status"
                        },
                        "backend": {
                            "type": "string",
                            "enum": ["gpt", "gemini", "claude_code", "codex"],
                            "description": "Filter by backend"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Max tasks to return (default 20)"
                        }
                    }
                }
            },
            {
                "name": "task_cancel",
                "description": "Cancel a running or queued task.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "task_id": {
                            "type": "string",
                            "description": "Task ID to cancel"
                        }
                    },
                    "required": ["task_id"]
                }
            },
            {
                "name": "task_poll",
                "description": "Poll for task completions and running status. Returns tasks completed since a timestamp, still-running tasks, and a status_bar summary. Use instead of blocking wait.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "since": {
                            "type": "string",
                            "description": "RFC3339 timestamp. Returns tasks completed after this time. Defaults to 1 hour ago."
                        }
                    }
                }
            },
            {
                "name": "pause_task",
                "description": "Pause a running or queued task. Marks status as paused. Background process may still run but will be ignored. Use resume_task to re-queue.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "task_id": {
                            "type": "string",
                            "description": "Task ID to pause"
                        }
                    },
                    "required": ["task_id"]
                }
            },
            {
                "name": "resume_task",
                "description": "Resume a paused task. Sets status back to queued and re-spawns execution. For claude_code/codex backends, re-spawns the CLI process.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "task_id": {
                            "type": "string",
                            "description": "Task ID to resume"
                        }
                    },
                    "required": ["task_id"]
                }
            },
            {
                "name": "configure",
                "description": "View or update Manager configuration. Call with no params to see current config.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "openai_api_key": {
                            "type": "string",
                            "description": "Set OpenAI API key for GPT backend"
                        },
                        "default_gpt_model": {
                            "type": "string",
                            "description": "Default GPT model (e.g., o3, gpt-4o)"
                        },
                        "default_working_dir": {
                            "type": "string",
                            "description": "Default working directory for CLI backends"
                        }
                    }
                }
            },
            {
                "name": "task_cleanup",
                "description": "Remove completed/failed/cancelled tasks older than N days.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "older_than_days": {
                            "type": "integer",
                            "description": "Remove tasks older than this many days (default: 7)"
                        }
                    }
                }
            }
            ,{
                "name": "session_start",
                "description": "Start a persistent Claude Code session. Returns session_id. Use send_to_session for follow-ups. More control than submit_task.",
                "inputSchema": {"type": "object", "properties": {"prompt": {"type": "string", "description": "Initial prompt"}, "working_dir": {"type": "string", "description": "Working directory"}, "model": {"type": "string", "description": "Model: sonnet, opus, haiku"}}, "required": ["prompt"]}
            },
            {
                "name": "session_send",
                "description": "Send follow-up to an existing Claude Code session. Continues the conversation.",
                "inputSchema": {"type": "object", "properties": {"session_id": {"type": "string"}, "message": {"type": "string"}}, "required": ["session_id", "message"]}
            },
            {
                "name": "open_terminal",
                "description": "Open Claude Code in a visible terminal window for interactive use. Fire-and-forget.",
                "inputSchema": {"type": "object", "properties": {"prompt": {"type": "string", "description": "Optional initial prompt"}, "working_dir": {"type": "string"}}}
            },
            {
                "name": "gemini_direct",
                "description": "Send a one-shot query to Gemini CLI. No task queue - returns result directly.",
                "inputSchema": {"type": "object", "properties": {"prompt": {"type": "string"}, "model": {"type": "string", "description": "Model: gemini-2.5-pro, gemini-2.5-flash, etc."}, "working_dir": {"type": "string"}}, "required": ["prompt"]}
            },
            {
                "name": "codex_exec",
                "description": "Run OpenAI Codex non-interactively. Supports sandbox modes and model selection. Returns structured output.",
                "inputSchema": {"type": "object", "properties": {"prompt": {"type": "string", "description": "Task instructions for Codex"}, "model": {"type": "string", "description": "Model: o3, o4-mini, etc."}, "sandbox": {"type": "string", "description": "Sandbox mode: read-only, workspace-write, danger-full-access"}, "working_dir": {"type": "string"}, "full_auto": {"type": "boolean", "description": "Low-friction automatic execution (sandbox: workspace-write)"}, "skip_approvals": {"type": "boolean", "description": "DANGEROUS: Skip all approvals and sandbox. Use only in safe environments."}}, "required": ["prompt"]}
            },
            {
                "name": "codex_review",
                "description": "Run OpenAI Codex code review. Reviews uncommitted changes or changes against a base branch.",
                "inputSchema": {"type": "object", "properties": {"prompt": {"type": "string", "description": "Custom review instructions"}, "base": {"type": "string", "description": "Review changes against this base branch"}, "uncommitted": {"type": "boolean", "description": "Review staged, unstaged, and untracked changes"}, "commit": {"type": "string", "description": "Review changes introduced by this commit SHA"}, "working_dir": {"type": "string"}}}
            },
            {
                "name": "workflow_run",
                "description": "Execute a multi-step workflow with task chaining, retry logic, and backend escalation. Each step runs on a specified AI backend. Output from one step feeds into the next. Failed steps retry then escalate to alternative backends.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string", "description": "Workflow name for tracking"},
                        "steps": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": {"type": "string"},
                                    "backend": {"type": "string", "enum": ["claude_code", "codex", "gemini"]},
                                    "prompt": {"type": "string", "description": "Task prompt. Use {{previous_output}} to reference prior step output"},
                                    "working_dir": {"type": "string"},
                                    "on_success": {"type": "string", "description": "Next step ID on success"},
                                    "max_retries": {"type": "integer", "description": "Max retries for this step (default 2)"},
                                    "timeout_secs": {"type": "integer", "description": "Timeout per attempt in seconds (default 300)"},
                                    "alternatives": {"type": "array", "items": {"type": "string"}, "description": "Alternative backend order if primary fails"}
                                },
                                "required": ["id", "backend", "prompt"]
                            }
                        },
                        "max_total_attempts": {"type": "integer", "description": "Global attempt limit across all steps (default 15)"},
                        "log_results": {"type": "boolean", "description": "Write results to Volumes inbox"}
                    },
                    "required": ["name", "steps"]
                }
            },
            {
                "name": "task_run_parallel",
                "description": "Run multiple tasks in parallel with dependency gates. Steps with depends_on wait for their dependencies. Steps in the same parallel_group start together. Use {{step_id.output}} to reference output from completed dependencies.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string", "description": "Workflow name for tracking"},
                        "steps": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": {"type": "string", "description": "Unique step identifier"},
                                    "backend": {"type": "string", "enum": ["claude_code", "codex", "gemini"], "description": "AI backend to run this step"},
                                    "prompt": {"type": "string", "description": "Task prompt. Use {{step_id.output}} to reference output from a dependency"},
                                    "working_dir": {"type": "string"},
                                    "depends_on": {"type": "array", "items": {"type": "string"}, "description": "Step IDs that must complete before this step starts"},
                                    "parallel_group": {"type": "string", "description": "Group tag — steps with same group and no unmet deps start together"},
                                    "max_retries": {"type": "integer", "description": "Max retries for this step (default 2)"},
                                    "timeout_secs": {"type": "integer", "description": "Timeout per attempt in seconds (default 300)"},
                                    "alternatives": {"type": "array", "items": {"type": "string"}, "description": "Alternative backend order if primary fails"}
                                },
                                "required": ["id", "backend", "prompt"]
                            }
                        },
                        "max_concurrent": {"type": "integer", "description": "Max simultaneous tasks (default 3)"},
                        "fail_fast": {"type": "boolean", "description": "If true, cancel remaining steps on first failure (default false)"}
                    },
                    "required": ["name", "steps"]
                }
            },
            {
                "name": "review_extractions",
                "description": "List tasks pending extraction review. Returns step trails for completed/failed tasks that may contain reusable workflow patterns. Run 3Q check on each: Reusable? Specific? New? Then call extract_workflow or dismiss_extraction.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "status_filter": {"type": "string", "description": "Filter: 'pending_success', 'pending_failure', or 'all' (default: 'all')"}
                    }
                }
            },
            {
                "name": "extract_workflow",
                "description": "Extract a reusable workflow pattern from a completed task. Saves to Volumes/manager/workflow_patterns/{name}.json. Use after review_extractions confirms the pattern passes the 3Q check.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "task_id": {"type": "string", "description": "Task ID to extract pattern from"},
                        "name": {"type": "string", "description": "Short name for the workflow pattern (e.g. 'rust-build-deploy')"},
                        "description": {"type": "string", "description": "What this workflow does and when to use it"}
                    },
                    "required": ["task_id", "name", "description"]
                }
            },
            {
                "name": "dismiss_extraction",
                "description": "Mark a task as not worth extracting. Use after review_extractions when the pattern fails the 3Q check (not reusable, not specific, or not new).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "task_id": {"type": "string", "description": "Task ID to dismiss"},
                        "reason": {"type": "string", "description": "Why this wasn't extracted (e.g. 'too simple', 'duplicate of X')"}
                    },
                    "required": ["task_id"]
                }
            },
            {
                "name": "task_rollback",
                "description": "Restore files to their pre-task state. Use when a task failed and modified files need to be reverted. Only works for tasks with trust_level Medium or High that had backups created.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "task_id": {"type": "string", "description": "Task ID to rollback"}
                    },
                    "required": ["task_id"]
                }
            },
            {
                "name": "task_retry",
                "description": "Manually retry a failed task. Creates a new task with the error context injected into the prompt so the backend avoids the same mistake. If max retries exhausted, escalates to a different backend. Optionally inject additional context.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "task_id": {"type": "string", "description": "Failed task ID to retry"},
                        "inject_context": {"type": "string", "description": "Optional extra context to inject into the retry prompt (e.g. hints, corrections)"}
                    },
                    "required": ["task_id"]
                }
            },
            {
                "name": "task_route",
                "description": "Get an AI backend recommendation for a task. Analyzes the prompt, historical performance, and learned patterns to suggest the best backend.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "prompt": {"type": "string", "description": "The task prompt to analyze for backend routing"},
                        "working_dir": {"type": "string", "description": "Optional working directory for context hints"}
                    },
                    "required": ["prompt"]
                }
            },
            {
                "name": "task_decompose",
                "description": "Break a natural language request into structured steps with backend recommendations.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "prompt": {"type": "string", "description": "Natural language task to decompose"},
                        "working_dir": {"type": "string", "description": "Optional working directory"}
                    },
                    "required": ["prompt"]
                }
            },
            {
                "name": "template_save",
                "description": "Save a reusable workflow template with parameter placeholders.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "description": {"type": "string"},
                        "steps": {"type": "array", "items": {"type": "object"}},
                        "backend": {"type": "string"},
                        "parameters": {"type": "object"}
                    },
                    "required": ["name", "description", "steps"]
                }
            },
            {
                "name": "template_list",
                "description": "List available workflow templates.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "template_run",
                "description": "Run a saved template with parameter substitution.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "parameters": {"type": "object"},
                        "working_dir": {"type": "string"}
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "task_explain",
                "description": "Plain English explanation of a task or recent history.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "task_id": {"type": "string"},
                        "last": {"type": "integer"}
                    }
                }
            },
            {
                "name": "create_loaf",
                "description": "Create a new Project Loaf — a persistent JSON file that tracks multi-task coordination. Tracks goal, phases, tasks, decisions, discoveries, and breadcrumbs.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "goal": {"type": "string", "description": "What this project aims to accomplish"},
                        "project_name": {"type": "string", "description": "Short name (used in filename: {project_name}_Loaf.json)"},
                        "phases": {"type": "array", "items": {"type": "string"}, "description": "Optional phase names. Defaults to single 'main' phase."}
                    },
                    "required": ["goal", "project_name"]
                }
            },
            {
                "name": "loaf_update",
                "description": "Update an active Project Loaf with task results, decisions, discoveries, next actions, or phase advancement.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "loaf_id": {"type": "string", "description": "Loaf ID (e.g. 'MyProject_Loaf')"},
                        "task_update": {"type": "object", "description": "Task update: {task_id, status, output_summary, files_changed, decisions_made, discoveries}"},
                        "decision": {"type": "object", "description": "Decision record: {what, why, who}"},
                        "discovery": {"type": "object", "description": "Discovery record: {what, impact}"},
                        "next_actions": {"type": "array", "items": {"type": "string"}, "description": "Replace current next_actions list"},
                        "phase_status": {"type": "string", "description": "Set to 'done' to complete current phase and advance to next"}
                    },
                    "required": ["loaf_id"]
                }
            },
            {
                "name": "loaf_status",
                "description": "Get current state of a Project Loaf. If no loaf_id given, finds the most recent active loaf.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "loaf_id": {"type": "string", "description": "Loaf ID. Optional — omit to find most recent active loaf."}
                    }
                }
            },
            {
                "name": "loaf_close",
                "description": "Mark a Project Loaf as complete and archive it.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "loaf_id": {"type": "string", "description": "Loaf ID to close"}
                    },
                    "required": ["loaf_id"]
                }
            },
            {
                "name": "task_watch",
                "description": "Watch multiple tasks until all complete. Blocks server-side (zero LLM polling turns). Optionally sends MCP progress notifications at configurable intervals. Use instead of repeated get_status calls to save tokens.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "task_ids": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "List of task IDs to watch until completion"
                        },
                        "timeout_secs": {
                            "type": "integer",
                            "description": "Max seconds to wait before returning partial results (default: 600)"
                        },
                        "progress": {
                            "type": "boolean",
                            "description": "Send MCP notifications with progress updates (default: true). Set false to disable if notifications interfere with context limits."
                        },
                        "progress_interval_secs": {
                            "type": "integer",
                            "description": "Seconds between progress notifications (default: 10). Higher = fewer notifications."
                        }
                    },
                    "required": ["task_ids"]
                }
            }
        ,
            json!({
                "name": "session_list",
                "description": "List active Claude Code sessions with their status, working directory, and whether the process is still alive.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            }),
            json!({
                "name": "get_analytics",
                "description": "Get task performance analytics: success rates by backend, total cost, average duration, recent failures. Filter by backend or time range.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "backend": {
                            "type": "string",
                            "description": "Filter by backend: claude_code, codex, gemini, gpt"
                        },
                        "since": {
                            "type": "string",
                            "description": "Only include tasks created after this RFC3339 timestamp"
                        }
                    }
                }
            }),
            {
                "name": "role_list",
                "description": "List available specialist roles (built-in + custom YAML) for task_submit. Each role injects a system prompt tailored to that specialty.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "role_create",
                "description": "Create a custom specialist role as a YAML file. Custom roles can override built-in roles.",
                "inputSchema": {
                    "type": "object",
                    "required": ["name", "prompt"],
                    "properties": {
                        "name": { "type": "string", "description": "Role name (alphanumeric/underscore). Used as filename and role param value." },
                        "prompt": { "type": "string", "description": "System prompt injected when this role is used." },
                        "expertise": { "type": "array", "items": { "type": "string" }, "description": "List of expertise areas for this role." }
                    }
                }
            },
            {
                "name": "task_rerun",
                "description": "Re-submit a completed (done) task using its original prompt, with optional extra context, file injection, or backend override. Returns a new task_id. The new task records rerun_of pointing to the original.",
                "inputSchema": {
                    "type": "object",
                    "required": ["task_id"],
                    "properties": {
                        "task_id": { "type": "string", "description": "ID of a previously completed task to rerun" },
                        "additional_context": { "type": "string", "description": "Extra context appended to the original prompt" },
                        "backend_override": { "type": "string", "enum": ["gpt", "gemini", "claude_code", "codex"], "description": "Use a different backend than the original task" },
                        "include_files": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Array of file paths whose current contents are injected into the prompt"
                        }
                    }
                }
            },
            {
                "name": "role_delete",
                "description": "Delete a custom specialist role YAML file.",
                "inputSchema": {
                    "type": "object",
                    "required": ["name"],
                    "properties": {
                        "name": { "type": "string", "description": "Role name to delete." }
                    }
                }
            }
        ]
    })
}

// ============================================================================
// HTTP Dashboard
// ============================================================================

#[derive(Clone)]
struct DashboardState {
    tasks: Arc<RwLock<HashMap<String, Task>>>,
    config: Arc<RwLock<ServerConfig>>,
}

async fn dash_status(State(st): State<DashboardState>) -> Json<Value> {
    let store = st.tasks.read().await;
    let running = store.values().filter(|t| t.status == TaskStatus::Running).count();
    let completed = store.values().filter(|t| t.status == TaskStatus::Done).count();
    let mut tasks: Vec<&Task> = store.values().collect();
    tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let tasks_json: Vec<Value> = tasks.iter().map(|t| json!({
        "id": t.id,
        "backend": t.backend,
        "status": t.status.to_string(),
        "prompt_preview": safe_truncate(&t.prompt, 100),
        "created_at": t.created_at.to_rfc3339(),
        "progress_lines": t.progress_lines,
    })).collect();
    Json(json!({ "tasks": tasks_json, "running": running, "completed": completed, "total": store.len() }))
}

async fn dash_status_by_id(State(st): State<DashboardState>, AxumPath(id): AxumPath<String>) -> (StatusCode, Json<Value>) {
    let store = st.tasks.read().await;
    match store.get(&id) {
        Some(t) => (StatusCode::OK, Json(json!({
            "id": t.id, "backend": t.backend, "status": t.status.to_string(),
            "prompt": t.prompt, "output": t.output, "error": t.error,
            "created_at": t.created_at.to_rfc3339(),
            "started_at": t.started_at.map(|s| s.to_rfc3339()),
            "completed_at": t.completed_at.map(|s| s.to_rfc3339()),
            "progress_lines": t.progress_lines,
        }))),
        None => (StatusCode::NOT_FOUND, Json(json!({"error": format!("Task '{}' not found", id)}))),
    }
}

async fn dash_health() -> Json<Value> {
    let servers = ["utonomous", "echo", "atlas", "local", "browser-mcp", "manager", "stocks"];
    let output = TokioCommand::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase())
        .unwrap_or_default();
    let mut result = serde_json::Map::new();
    for name in &servers {
        let alive = output.contains(&format!("{}.exe", name));
        result.insert(name.to_string(), json!(if alive { "alive" } else { "dead" }));
    }
    Json(json!({"servers": Value::Object(result)}))
}

async fn dash_inbox() -> Json<Value> {
    let inbox_path = r"C:\My Drive\Volumes\multi_ai_coordination\inbox.md";
    let content = match std::fs::read_to_string(inbox_path) {
        Ok(c) => c,
        Err(e) => return Json(json!({"error": format!("Cannot read inbox: {}", e)})),
    };
    let mut pending: Vec<String> = Vec::new();
    let mut processed: Vec<String> = Vec::new();
    let mut section = "";
    let mut entry = String::new();
    for line in content.lines() {
        if line.starts_with("## Pending") { flush_entry(&mut entry, section, &mut pending, &mut processed); section = "pending"; continue; }
        if line.starts_with("## Processed") { flush_entry(&mut entry, section, &mut pending, &mut processed); section = "processed"; continue; }
        if line.starts_with("## ") { flush_entry(&mut entry, section, &mut pending, &mut processed); section = ""; continue; }
        if line.starts_with("### ") { flush_entry(&mut entry, section, &mut pending, &mut processed); entry = line.to_string(); }
        else if !entry.is_empty() { entry.push('\n'); entry.push_str(line); }
    }
    flush_entry(&mut entry, section, &mut pending, &mut processed);
    Json(json!({"pending": pending, "processed": processed}))
}

fn flush_entry(entry: &mut String, section: &str, pending: &mut Vec<String>, processed: &mut Vec<String>) {
    if entry.is_empty() { return; }
    match section {
        "pending" => pending.push(std::mem::take(entry)),
        "processed" => processed.push(std::mem::take(entry)),
        _ => { entry.clear(); }
    }
}

async fn dash_get_prefs() -> Json<Value> {
    let prefs_path = format!(r"{}\CPC\config\dashboard_prefs.json",
        std::env::var("LOCALAPPDATA").unwrap_or_else(|_| r"C:\Users\josep\AppData\Local".into()));
    match std::fs::read_to_string(&prefs_path) {
        Ok(c) => Json(serde_json::from_str::<Value>(&c).unwrap_or(json!({}))),
        Err(_) => Json(json!({})),
    }
}

async fn dash_post_prefs(Json(body): Json<Value>) -> Json<Value> {
    let prefs_dir = format!(r"{}\CPC\config",
        std::env::var("LOCALAPPDATA").unwrap_or_else(|_| r"C:\Users\josep\AppData\Local".into()));
    let _ = std::fs::create_dir_all(&prefs_dir);
    let prefs_path = format!(r"{}\dashboard_prefs.json", prefs_dir);
    match std::fs::write(&prefs_path, serde_json::to_string_pretty(&body).unwrap_or_default()) {
        Ok(_) => Json(json!({"saved": true})),
        Err(e) => Json(json!({"error": format!("Failed to write prefs: {}", e)})),
    }
}

async fn dash_post_task(State(st): State<DashboardState>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
    let backend_str = match body.get("backend").and_then(|v| v.as_str()) {
        Some(b) => b,
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error": "Missing 'backend'"}))),
    };
    let prompt = match body.get("prompt").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error": "Missing 'prompt'"}))),
    };
    let working_dir = body.get("working_dir").and_then(|v| v.as_str()).map(String::from);
    let backend = match backend_str {
        "gpt" => Backend::Gpt,
        "gemini" => Backend::Gemini,
        "claude_code" | "claude" => Backend::ClaudeCode,
        "codex" => Backend::Codex,
        _ => return (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Unknown backend: {}", backend_str)}))),
    };
    let task_id = Uuid::new_v4().to_string()[..8].to_string();
    let task = Task {
        id: task_id.clone(), backend: backend.clone(), prompt: prompt.clone(),
        system_prompt: None, model: None, working_dir: working_dir.clone(),
        status: TaskStatus::Queued, output: String::new(), error: None,
        created_at: Utc::now(), started_at: None, completed_at: None, progress_lines: 0, steps: Vec::new(), last_activity: None, stall_detected: false, extraction_status: ExtractionStatus::None,
        trust_score: 0, trust_level: TrustLevel::Low, rollback_path: None, validation_status: ValidationStatus::NotChecked, assertions: Vec::new(), backed_up_files: Vec::new(), retry_count: 0, max_retries: 2, retry_of: None, error_context: None, input_tokens: 0, output_tokens: 0, cost_usd: 0.0,
        on_complete: None,
        role: None,
        save_artifact: false,
        rerun_of: None,
        parent_task_id: None,
        forked_from: None,
        continuation_of: None,
        child_pid: None,
        watchdog_observations: Vec::new(),
    };
    { let mut store = st.tasks.write().await; store.insert(task_id.clone(), task.clone()); }
    Server::persist_task(&task);
    let tasks_bg = st.tasks.clone();
    let tid = task_id.clone();
    match backend {
        Backend::Gpt => { tokio::spawn(run_gpt_task(st.config.clone(), tasks_bg, tid)); }
        Backend::Gemini => {
            let args = vec![gemini_cmd().to_string(), "-p".into(), prompt, "--yolo".into()];
            tokio::spawn(run_cli_task(tasks_bg, tid, node_cmd(), args));
        }
        Backend::ClaudeCode => {
            let mut args = vec![
                "-p".into(), prompt,
                "--dangerously-skip-permissions".into(), "--verbose".into(),
                "--output-format".into(), "stream-json".into(),
                "--add-dir".into(), r"C:\temp".into(),
                "--add-dir".into(), r"C:\My Drive\Volumes".into(),
                "--add-dir".into(), r"C:\CPC".into(),
                "--add-dir".into(), r"C:\rust-mcp".into(),
            ];
            if let Some(ref wd) = working_dir { args.push("--add-dir".into()); args.push(wd.clone()); }
            tokio::spawn(run_cli_task(tasks_bg, tid, claude_code_cmd(), args));
        }
        Backend::Codex => {
            let wd = working_dir.unwrap_or_else(|| r"C:\rust-mcp".to_string());
            let args = vec![
                "exec".into(), "--json".into(), "--skip-git-repo-check".into(),
                "--full-auto".into(), "--cd".into(), wd.clone(), prompt,
            ];
            tokio::spawn(run_codex_task(tasks_bg, tid, args, wd));
        }
    }
    (StatusCode::OK, Json(json!({"task_id": task_id, "status": "queued"})))
}

async fn dash_cancel(State(st): State<DashboardState>, AxumPath(id): AxumPath<String>) -> (StatusCode, Json<Value>) {
    let mut store = st.tasks.write().await;
    match store.get_mut(&id) {
        Some(task) if task.status == TaskStatus::Running || task.status == TaskStatus::Queued => {
            task.status = TaskStatus::Cancelled;
            task.completed_at = Some(Utc::now());
            task.error = Some("Cancelled via dashboard".into());
            Server::flag_extraction(task);
            // Item 18: no retry for cancelled tasks
            Server::persist_task(task);
            Server::save_to_history(task);
            (StatusCode::OK, Json(json!({"task_id": id, "status": "cancelled"})))
        }
        Some(task) => (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Task {} is {} - cannot cancel", id, task.status)}))),
        None => (StatusCode::NOT_FOUND, Json(json!({"error": format!("Task {} not found", id)}))),
    }
}

async fn dash_knowledge() -> Json<Value> {
    let volumes_path = r"C:\My Drive\Volumes";
    let mut topics: Vec<Value> = Vec::new();
    let mut recent: Vec<(String, std::time::SystemTime)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(volumes_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() { continue; }
            let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            let op_file = path.join(format!("Operating_{}.md", name));
            let modified = entry.metadata().ok().and_then(|m| m.modified().ok());
            topics.push(json!({
                "name": name,
                "has_operating_file": op_file.exists(),
                "modified": modified.map(|t| { let dt: DateTime<Utc> = t.into(); dt.to_rfc3339() }),
            }));
            if let Some(t) = modified { recent.push((name, t)); }
        }
    }
    recent.sort_by(|a, b| b.1.cmp(&a.1));
    recent.truncate(10);
    let recent_json: Vec<Value> = recent.iter().map(|(n, t)| {
        let dt: DateTime<Utc> = (*t).into();
        json!({"topic": n, "modified": dt.to_rfc3339()})
    }).collect();
    Json(json!({"topics": topics, "recent_changes": recent_json}))
}

async fn dash_git() -> Json<Value> {
    let repo = r"C:\rust-mcp";
    let (status, branch, log) = tokio::join!(
        TokioCommand::new("git").args(["status", "--porcelain"]).current_dir(repo).output(),
        TokioCommand::new("git").args(["branch", "--show-current"]).current_dir(repo).output(),
        TokioCommand::new("git").args(["log", "--oneline", "-5"]).current_dir(repo).output()
    );
    Json(json!({
        "branch": branch.ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()),
        "status": status.ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()),
        "log": log.ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()),
    }))
}

async fn dash_system() -> Json<Value> {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu_usage();
    // CPU needs two samples for accurate reading
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    sys.refresh_cpu_usage();
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let total_disk: u64 = disks.iter().map(|d| d.total_space()).sum();
    let avail_disk: u64 = disks.iter().map(|d| d.available_space()).sum();
    Json(json!({
        "cpu": { "usage_percent": sys.global_cpu_usage(), "cores": sys.cpus().len() },
        "ram": {
            "total_gb": format!("{:.1}", sys.total_memory() as f64 / 1_073_741_824.0),
            "used_gb": format!("{:.1}", sys.used_memory() as f64 / 1_073_741_824.0),
            "available_gb": format!("{:.1}", (sys.total_memory() - sys.used_memory()) as f64 / 1_073_741_824.0),
        },
        "disk": {
            "total_gb": format!("{:.1}", total_disk as f64 / 1_073_741_824.0),
            "available_gb": format!("{:.1}", avail_disk as f64 / 1_073_741_824.0),
        },
    }))
}

async fn dash_history() -> Json<Value> {
    let history_path = format!("{}\\task_history.json", history_dir());
    match std::fs::read_to_string(&history_path) {
        Ok(data) => {
            let entries: Vec<Value> = serde_json::from_str(&data).unwrap_or_default();
            Json(json!({"entries": entries, "count": entries.len()}))
        }
        Err(_) => Json(json!({"entries": [], "count": 0})),
    }
}

fn volumes_base_path() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("VOLUMES_PATH").unwrap_or_else(|_| r"C:\My Drive\Volumes".to_string()),
    )
}

/// Validate that a requested path is under the Volumes base directory.
fn validate_volumes_path(requested: &str) -> Result<std::path::PathBuf, (StatusCode, Json<Value>)> {
    let base = volumes_base_path();
    let candidate = base.join(requested);
    // Canonicalize base (must exist)
    let canon_base = base.canonicalize().map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Volumes base path not accessible"})))
    })?;
    // Canonicalize candidate
    let canon = candidate.canonicalize().map_err(|_| {
        (StatusCode::NOT_FOUND, Json(json!({"error": "File not found"})))
    })?;
    if !canon.starts_with(&canon_base) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "Access denied: path outside Volumes"}))));
    }
    Ok(canon)
}

#[derive(Deserialize)]
struct PathQuery {
    path: Option<String>,
}

async fn api_read_file(Query(q): Query<PathQuery>) -> impl IntoResponse {
    let rel_path = match q.path {
        Some(p) if !p.is_empty() => p,
        _ => return (StatusCode::BAD_REQUEST, Json(json!({"error": "Missing 'path' parameter"}))).into_response(),
    };
    let canon = match validate_volumes_path(&rel_path) {
        Ok(p) => p,
        Err((status, body)) => return (status, body).into_response(),
    };
    if !canon.is_file() {
        return (StatusCode::NOT_FOUND, Json(json!({"error": "File not found"}))).into_response();
    }
    match tokio::fs::read_to_string(&canon).await {
        Ok(contents) => (
            StatusCode::OK,
            [("content-type", "text/plain; charset=utf-8"), ("access-control-allow-origin", "*")],
            contents,
        ).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to read file"}))).into_response(),
    }
}

async fn api_list_dir(Query(q): Query<PathQuery>) -> impl IntoResponse {
    let rel_path = match q.path {
        Some(p) if !p.is_empty() => p,
        _ => return (StatusCode::BAD_REQUEST, Json(json!({"error": "Missing 'path' parameter"}))).into_response(),
    };
    let canon = match validate_volumes_path(&rel_path) {
        Ok(p) => p,
        Err((status, body)) => return (status, body).into_response(),
    };
    if !canon.is_dir() {
        return (StatusCode::NOT_FOUND, Json(json!({"error": "Directory not found"}))).into_response();
    }
    let mut files: Vec<Value> = Vec::new();
    match std::fs::read_dir(&canon) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let meta = entry.metadata().ok();
                files.push(json!({
                    "name": entry.file_name().to_string_lossy(),
                    "is_dir": meta.as_ref().map(|m| m.is_dir()).unwrap_or(false),
                    "size": meta.as_ref().map(|m| m.len()).unwrap_or(0),
                }));
            }
        }
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to read directory"}))).into_response(),
    }
    (
        StatusCode::OK,
        [("access-control-allow-origin", "*")],
        Json(json!({"files": files})),
    ).into_response()
}

async fn start_dashboard(state: DashboardState) {
    let port: u16 = std::env::var("CPC_MANAGER_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(9876);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/status", get(dash_status))
        .route("/status/:id", get(dash_status_by_id))
        .route("/health", get(dash_health))
        .route("/inbox", get(dash_inbox))
        .route("/prefs", get(dash_get_prefs).post(dash_post_prefs))
        .route("/task", post(dash_post_task))
        .route("/cancel/:id", post(dash_cancel))
        .route("/knowledge", get(dash_knowledge))
        .route("/history", get(dash_history))
        .route("/git", get(dash_git))
        .route("/system", get(dash_system))
        .route("/api/read-file", get(api_read_file))
        .route("/api/list-dir", get(api_list_dir))
        .layer(cors)
        .with_state(state);

    let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await {
        Ok(l) => l,
        Err(e) => {
            warn!("Dashboard failed to bind port {}: {}", port, e);
            return;
        }
    };
    info!("Dashboard HTTP server on port {}", port);
    axum::serve(listener, app).await.ok();
}

// ============================================================================
// Singleton Lock + Named Pipe
// ============================================================================

const PIPE_NAME: &str = r"\\.\pipe\cpc-manager";

fn lock_path() -> String {
    format!(r"{}\manager.lock", default_data_dir())
}

/// Try to acquire exclusive lock. Returns the lock file handle on success.
/// The lock is held as long as the file handle is open.
fn try_acquire_lock() -> Option<std::fs::File> {
    use std::os::windows::io::AsRawHandle;
    std::fs::create_dir_all(&default_data_dir()).ok();
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(lock_path())
        .ok()?;

    // Use Windows LockFileEx for exclusive non-blocking lock
    let handle = file.as_raw_handle();
    let result = unsafe {
        use std::os::windows::io::RawHandle;
        #[allow(non_snake_case)]
        #[repr(C)]
        struct OVERLAPPED {
            Internal: usize,
            InternalHigh: usize,
            Offset: u32,
            OffsetHigh: u32,
            hEvent: RawHandle,
        }
        extern "system" {
            fn LockFileEx(
                hFile: RawHandle,
                dwFlags: u32,
                dwReserved: u32,
                nNumberOfBytesToLockLow: u32,
                nNumberOfBytesToLockHigh: u32,
                lpOverlapped: *mut OVERLAPPED,
            ) -> i32;
        }
        const LOCKFILE_EXCLUSIVE_LOCK: u32 = 0x02;
        const LOCKFILE_FAIL_IMMEDIATELY: u32 = 0x01;
        let mut ov = OVERLAPPED {
            Internal: 0, InternalHigh: 0, Offset: 0, OffsetHigh: 0, hEvent: std::ptr::null_mut(),
        };
        LockFileEx(
            handle,
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0, 1, 0,
            &mut ov,
        )
    };

    if result != 0 {
        // Write our PID to the lock file
        use std::io::Write as _;
        let mut f = &file;
        let _ = writeln!(f, "{}", std::process::id());
        Some(file)
    } else {
        None // Lock busy — another instance is primary
    }
}

/// Run as a pipe proxy: forward stdin to the primary instance's named pipe, forward responses to stdout.
fn run_as_proxy() -> ! {
    use std::io::{BufRead, Write as _};
    info!("Running as proxy — forwarding to primary manager via named pipe");

    // Connect to the named pipe (retry briefly in case primary is still starting)
    let pipe = {
        let mut attempts = 0;
        loop {
            match std::fs::OpenOptions::new().read(true).write(true).open(PIPE_NAME) {
                Ok(f) => break f,
                Err(e) => {
                    attempts += 1;
                    if attempts > 10 {
                        eprintln!("Failed to connect to primary manager pipe: {}", e);
                        std::process::exit(1);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            }
        }
    };

    let pipe_reader = std::io::BufReader::new(pipe.try_clone().expect("pipe clone"));
    let mut pipe_writer = pipe.try_clone().expect("pipe clone write");

    // Spawn thread to read pipe responses and write to stdout
    let stdout_thread = std::thread::spawn(move || {
        let mut stdout = io::stdout();
        for line in pipe_reader.lines() {
            match line {
                Ok(l) => {
                    let _ = writeln!(stdout, "{}", l);
                    let _ = stdout.flush();
                }
                Err(_) => break,
            }
        }
    });

    // Read stdin and forward to pipe
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        match line {
            Ok(l) => {
                if let Err(_) = writeln!(pipe_writer, "{}", l) {
                    break; // Pipe broken
                }
                let _ = pipe_writer.flush();
            }
            Err(_) => break,
        }
    }

    // stdin closed — we're done
    drop(pipe_writer);
    let _ = stdout_thread.join();
    std::process::exit(0);
}

/// Start named pipe server — accepts connections from proxy instances.
/// Each connection gets its own handler thread that processes JSON-RPC requests.
fn start_pipe_server(server_tasks: Arc<RwLock<HashMap<String, Task>>>, server_config: Arc<RwLock<ServerConfig>>, runtime_handle: tokio::runtime::Handle) {
    std::thread::spawn(move || {
        use std::os::windows::io::FromRawHandle;
        info!("Named pipe server starting at {}", PIPE_NAME);

        loop {
            // Create a named pipe instance
            let pipe_handle = unsafe {
                extern "system" {
                    fn CreateNamedPipeA(
                        lpName: *const u8,
                        dwOpenMode: u32,
                        dwPipeMode: u32,
                        nMaxInstances: u32,
                        nOutBufferSize: u32,
                        nInBufferSize: u32,
                        nDefaultTimeOut: u32,
                        lpSecurityAttributes: *const u8,
                    ) -> isize;
                    fn ConnectNamedPipe(hNamedPipe: isize, lpOverlapped: *mut u8) -> i32;
                }
                const PIPE_ACCESS_DUPLEX: u32 = 0x00000003;
                const PIPE_TYPE_BYTE: u32 = 0x00000000;
                const PIPE_READMODE_BYTE: u32 = 0x00000000;
                const PIPE_WAIT: u32 = 0x00000000;
                const PIPE_UNLIMITED_INSTANCES: u32 = 255;

                let name = format!("{}\0", PIPE_NAME);
                let handle = CreateNamedPipeA(
                    name.as_ptr(),
                    PIPE_ACCESS_DUPLEX,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                    PIPE_UNLIMITED_INSTANCES,
                    65536, 65536, 0,
                    std::ptr::null(),
                );
                if handle == -1 {
                    warn!("Failed to create named pipe, retrying in 5s");
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    continue;
                }
                // Wait for a client
                ConnectNamedPipe(handle, std::ptr::null_mut());
                handle
            };

            // Wrap handle as a File for reading/writing
            let pipe_file = unsafe { std::fs::File::from_raw_handle(pipe_handle as *mut std::ffi::c_void) };
            let tasks_c = server_tasks.clone();
            let config_c = server_config.clone();
            let rt = runtime_handle.clone();

            std::thread::spawn(move || {
                let reader = std::io::BufReader::new(&pipe_file);
                let mut writer = std::io::BufWriter::new(&pipe_file);

                // Create a temporary Server-like handler for this pipe connection
                let proxy_server = Server {
                    tasks: tasks_c,
                    config: config_c,
                    runtime: rt,
                    stdout: Arc::new(Mutex::new(io::stdout())), // not used for pipe responses
                };

                for line in reader.lines() {
                    let line = match line {
                        Ok(l) if !l.trim().is_empty() => l,
                        Ok(_) => continue,
                        Err(_) => break,
                    };

                    let request: JsonRpcRequest = match serde_json::from_str(&line) {
                        Ok(r) => r,
                        Err(e) => {
                            let err_resp = json!({
                                "jsonrpc": "2.0", "id": null,
                                "error": {"code": -32700, "message": format!("Parse error: {}", e)}
                            });
                            let _ = writeln!(writer, "{}", serde_json::to_string(&err_resp).unwrap());
                            let _ = writer.flush();
                            continue;
                        }
                    };

                    let response = match request.method.as_str() {
                        "initialize" => json!({
                            "jsonrpc": "2.0", "id": request.id,
                            "result": {
                                "protocolVersion": "2024-11-05",
                                "serverInfo": {"name": "manager", "version": "1.0.0"},
                                "capabilities": {"tools": {}}
                            }
                        }),
                        "notifications/initialized" => continue,
                        "tools/list" => json!({
                            "jsonrpc": "2.0", "id": request.id,
                            "result": get_tools_list()
                        }),
                        "tools/call" => {
                            let params = request.params.unwrap_or(json!({}));
                            let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            let tool_args = params.get("arguments").cloned().unwrap_or(json!({}));
                            match handle_tool_call(&proxy_server, tool_name, tool_args) {
                                Ok(result) => json!({
                                    "jsonrpc": "2.0", "id": request.id,
                                    "result": {
                                        "content": [{"type": "text", "text": serde_json::to_string_pretty(&result).unwrap()}],
                                        "isError": false
                                    }
                                }),
                                Err(e) => json!({
                                    "jsonrpc": "2.0", "id": request.id,
                                    "result": {
                                        "content": [{"type": "text", "text": format!("Error: {}", e)}],
                                        "isError": true
                                    }
                                }),
                            }
                        }
                        _ => json!({"jsonrpc": "2.0", "id": request.id, "result": {}}),
                    };

                    let _ = writeln!(writer, "{}", serde_json::to_string(&response).unwrap());
                    let _ = writer.flush();
                }
                info!("Pipe client disconnected");
            });
        }
    });
}

// ============================================================================
// Main Loop
// ============================================================================

fn main() {
    // Set up tracing
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::new("manager=info"))
        .init();

    info!("Manager MCP v1.0 starting...");

    // === Fix 3: Singleton via lock file + named pipe ===
    // Try to acquire exclusive lock. If another instance holds it,
    // run as a proxy that forwards MCP requests via named pipe.
    let _lock_guard = match try_acquire_lock() {
        Some(lock) => {
            info!("Acquired singleton lock — running as primary instance (PID {})", std::process::id());
            lock
        }
        None => {
            info!("Lock busy — running as proxy to primary instance");
            run_as_proxy(); // never returns
        }
    };

    // === Fix 4: Zombie reaper ===
    // Kill orphan manager.exe instances that aren't responding via named pipe.
    // Only the startup manager does this, and only to stale/orphaned instances.
    {
        let my_pid = std::process::id();
        let output = std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq manager.exe", "/FO", "CSV", "/NH"])
            .output();
        if let Ok(out) = output {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                // CSV format: "manager.exe","1234","Console","1","12,345 K"
                let parts: Vec<&str> = line.split(',').collect();
                if parts.len() >= 2 {
                    let pid_str = parts[1].trim().trim_matches('"');
                    if let Ok(pid) = pid_str.parse::<u32>() {
                        if pid != my_pid {
                            // Try to connect to pipe — if it responds, this is a live instance (shouldn't happen since we hold the lock)
                            let is_orphan = std::fs::OpenOptions::new()
                                .read(true).write(true)
                                .open(PIPE_NAME)
                                .is_err();
                            if is_orphan {
                                info!("Killing orphan manager.exe PID {} (no pipe response)", pid);
                                let _ = std::process::Command::new("taskkill")
                                    .args(["/PID", &pid.to_string(), "/F"])
                                    .output();
                            }
                        }
                    }
                }
            }
        }
    }

    // Create tokio runtime for async operations
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");

    let server = Server::new(runtime.handle().clone());

    // Start named pipe server for proxy instances
    start_pipe_server(server.tasks.clone(), server.config.clone(), runtime.handle().clone());

    // Spawn HTTP dashboard alongside MCP stdio
    runtime.spawn(start_dashboard(DashboardState {
        tasks: server.tasks.clone(),
        config: server.config.clone(),
    }));

    let stdin = io::stdin();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        if line.trim().is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let error_response = JsonRpcErrorResponse {
                    jsonrpc: "2.0".to_string(),
                    id: Value::Null,
                    error: JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {}", e),
                    },
                };
                server.write_stdout(&serde_json::to_string(&error_response).unwrap());
                continue;
            }
        };

        if request.jsonrpc != "2.0" {
            let error_response = JsonRpcErrorResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id.clone(),
                error: JsonRpcError {
                    code: -32600,
                    message: format!("Invalid JSON-RPC version"),
                },
            };
            server.write_stdout(&serde_json::to_string(&error_response).unwrap());
            continue;
        }

        let response = match request.method.as_str() {
            "initialize" => {
                JsonRpcSuccess {
                    jsonrpc: "2.0".to_string(),
                    id: request.id,
                    result: json!({
                        "protocolVersion": "2024-11-05",
                        "serverInfo": {"name": "manager", "version": "1.0.0"},
                        "capabilities": {"tools": {}}
                    }),
                }
            }
            "notifications/initialized" => continue,
            "tools/list" => {
                JsonRpcSuccess {
                    jsonrpc: "2.0".to_string(),
                    id: request.id,
                    result: get_tools_list(),
                }
            }
            "tools/call" => {
                let params = request.params.unwrap_or(json!({}));
                let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let tool_args = params.get("arguments").cloned().unwrap_or(json!({}));

                match handle_tool_call(&server, tool_name, tool_args) {
                    Ok(result) => JsonRpcSuccess {
                        jsonrpc: "2.0".to_string(),
                        id: request.id,
                        result: json!({
                            "content": [{"type": "text", "text": serde_json::to_string_pretty(&result).unwrap()}],
                            "isError": false
                        }),
                    },
                    Err(e) => JsonRpcSuccess {
                        jsonrpc: "2.0".to_string(),
                        id: request.id,
                        result: json!({
                            "content": [{"type": "text", "text": format!("Error: {}", e)}],
                            "isError": true
                        }),
                    },
                }
            }
            _ => {
                JsonRpcSuccess {
                    jsonrpc: "2.0".to_string(),
                    id: request.id,
                    result: json!({}),
                }
            }
        };

        server.write_stdout(&serde_json::to_string(&response).unwrap());
    }
}


// ============================================================================
// Session Management (absorbed from claude-bridge/claude-runner)
// ============================================================================

const SESSION_DIR: &str = r"C:\temp\manager-sessions";

fn handle_start_session(server: &Server, args: Value) -> Result<Value, String> {
    let prompt = args.get("prompt").and_then(|v| v.as_str())
        .ok_or("Missing 'prompt'")?;
    let working_dir = args.get("working_dir").and_then(|v| v.as_str())
        .unwrap_or(r"C:\");
    let model = args.get("model").and_then(|v| v.as_str());
    
    let session_id = format!("ses_{}", &uuid::Uuid::new_v4().to_string()[..8]);
    
    // Create session directory
    let session_path = format!("{}\\{}", SESSION_DIR, session_id);
    let _ = std::fs::create_dir_all(&session_path);
    
    // Build args - use -p for first prompt, store session for continuation
    let mut args = vec![
        "-p".to_string(), prompt.to_string(),
        "--dangerously-skip-permissions".to_string(),
        "--verbose".to_string(),
        "--output-format".to_string(), "stream-json".to_string(),
        "--add-dir".to_string(), working_dir.to_string(),
        "--add-dir".to_string(), r"C:\temp".to_string(),
        "--add-dir".to_string(), r"C:\My Drive\Volumes".to_string(),
        "--add-dir".to_string(), r"C:\rust-mcp".to_string(),
    ];
    if let Some(m) = model {
        args.push("--model".to_string());
        args.push(m.to_string());
    }
    
    // Submit as managed task so we can track it
    let task_id = session_id.clone();
    let tasks_bg = server.tasks.clone();
    let tid = task_id.clone();
    
    // Create task entry
    {
        let mut store = server.runtime.block_on(server.tasks.write());
        store.insert(task_id.clone(), Task {
            id: task_id.clone(),
            backend: Backend::ClaudeCode,
            prompt: prompt.to_string(),
            status: TaskStatus::Running,
            output: String::new(),
            error: None,
            system_prompt: None,
            model: None,
            working_dir: None,
            created_at: chrono::Utc::now(),
            started_at: Some(chrono::Utc::now()),
            completed_at: None,
            progress_lines: 0, steps: Vec::new(), last_activity: None, stall_detected: false, extraction_status: ExtractionStatus::None,
            trust_score: 0, trust_level: TrustLevel::Low, rollback_path: None, validation_status: ValidationStatus::NotChecked, assertions: Vec::new(), backed_up_files: Vec::new(), retry_count: 0, max_retries: 2, retry_of: None, error_context: None, input_tokens: 0, output_tokens: 0, cost_usd: 0.0,
            on_complete: None,
            role: None,
            save_artifact: false,
            rerun_of: None,
            parent_task_id: None,
            forked_from: None,
            continuation_of: None,
            child_pid: None,
            watchdog_observations: Vec::new(),
        });
    }

    server.runtime.spawn(run_cli_task(tasks_bg, tid, claude_code_cmd(), args));

    // Save session metadata
    let meta = serde_json::json!({
        "session_id": session_id,
        "working_dir": working_dir,
        "model": model,
        "created_at": chrono::Utc::now().to_rfc3339()
    });
    let _ = std::fs::write(
        format!("{}\\meta.json", session_path),
        serde_json::to_string_pretty(&meta).unwrap_or_default()
    );
    
    Ok(json!({
        "session_id": session_id,
        "status": "running",
        "message": "Session started. Use get_status/get_output to check. Use send_to_session for follow-ups."
    }))
}

fn handle_send_to_session(server: &Server, args: Value) -> Result<Value, String> {
    let session_id = args.get("session_id").and_then(|v| v.as_str())
        .ok_or("Missing 'session_id'")?;
    let message = args.get("message").and_then(|v| v.as_str())
        .ok_or("Missing 'message'")?;
    
    // Read session metadata for working_dir and model
    let meta_path = format!("{}\\{}\\meta.json", SESSION_DIR, session_id);
    let meta: Value = std::fs::read_to_string(&meta_path)
        .map(|s| serde_json::from_str(&s).unwrap_or(json!({})))
        .unwrap_or(json!({}));
    
    let working_dir = meta.get("working_dir").and_then(|v| v.as_str()).unwrap_or(r"C:\");
    let model = meta.get("model").and_then(|v| v.as_str());
    
    // Item 6: Route by backend stored in session meta
    let backend_str = meta.get("backend").and_then(|v| v.as_str()).unwrap_or("claude_code");
    let (exe, cli_args, backend_enum): (&str, Vec<String>, Backend) = match backend_str {
        "codex" => {
            let mut a = vec!["exec".to_string(), "resume".to_string(), "--last".to_string()];
            if let Some(m) = model { a.push("--model".to_string()); a.push(m.to_string()); }
            a.push("--json".to_string());
            a.push("--cd".to_string()); a.push(working_dir.to_string());
            a.push("--skip-git-repo-check".to_string());
            a.push("--full-auto".to_string());
            a.push(message.to_string());
            (codex_cmd(), a, Backend::Codex)
        }
        "gemini" => {
            let gp = if let Some(bc) = Server::read_breadcrumb_state() {
                format!("{}\n\n{}", bc, message)
            } else { message.to_string() };
            let mut a = vec![gemini_cmd().to_string(), "-p".to_string(), gp, "--yolo".to_string()];
            if let Some(m) = model { a.push("--model".to_string()); a.push(m.to_string()); }
            (node_cmd(), a, Backend::Gemini)
        }
        _ => {
            let mut a = vec![
                "-p".to_string(), message.to_string(),
                "--continue".to_string(),
                "--dangerously-skip-permissions".to_string(),
                "--verbose".to_string(),
                "--output-format".to_string(), "stream-json".to_string(),
                "--add-dir".to_string(), working_dir.to_string(),
                "--add-dir".to_string(), r"C:\temp".to_string(),
            ];
            if let Some(m) = model { a.push("--model".to_string()); a.push(m.to_string()); }
            (claude_code_cmd(), a, Backend::ClaudeCode)
        }
    };
    
    let task_id = format!("{}_turn_{}", session_id, chrono::Utc::now().timestamp());
    let tasks_bg = server.tasks.clone();
    let tid = task_id.clone();
    
    {
        let mut store = server.runtime.block_on(server.tasks.write());
        store.insert(task_id.clone(), Task {
            id: task_id.clone(),
            backend: backend_enum.clone(),
            prompt: message.to_string(),
            status: TaskStatus::Running,
            output: String::new(),
            error: None,
            system_prompt: None,
            model: None,
            working_dir: None,
            created_at: chrono::Utc::now(),
            started_at: Some(chrono::Utc::now()),
            completed_at: None,
            progress_lines: 0, steps: Vec::new(), last_activity: None, stall_detected: false, extraction_status: ExtractionStatus::None,
            trust_score: 0, trust_level: TrustLevel::Low, rollback_path: None, validation_status: ValidationStatus::NotChecked, assertions: Vec::new(), backed_up_files: Vec::new(), retry_count: 0, max_retries: 2, retry_of: None, error_context: None, input_tokens: 0, output_tokens: 0, cost_usd: 0.0,
            on_complete: None,
            role: None,
            save_artifact: false,
            rerun_of: None,
            parent_task_id: None,
            forked_from: None,
            continuation_of: None,
            child_pid: None,
            watchdog_observations: Vec::new(),
        });
    }

    match backend_enum {
        Backend::Codex => { server.runtime.spawn(run_codex_task(tasks_bg, tid, cli_args, working_dir.to_string())); }
        _ => { server.runtime.spawn(run_cli_task(tasks_bg, tid, exe, cli_args)); }
    }
    
    Ok(json!({
        "task_id": task_id,
        "session_id": session_id,
        "status": "running",
        "message": "Follow-up sent. Use get_status/get_output with task_id to check."
    }))
}

fn handle_open_terminal(args: Value) -> Result<Value, String> {
    let prompt = args.get("prompt").and_then(|v| v.as_str());
    let working_dir = args.get("working_dir").and_then(|v| v.as_str()).unwrap_or(r"C:\");

    // Build the claude command that runs inside the terminal
    let mut claude_args = format!("\"{}\"", claude_code_cmd());
    if let Some(p) = prompt {
        // Escape double quotes in prompt for cmd
        let escaped = p.replace('"', "\\\"");
        claude_args.push_str(&format!(" -p \"{}\"", escaped));
    }
    claude_args.push_str(" --dangerously-skip-permissions");
    // cmd /K keeps terminal open after claude exits
    let inner_cmd = format!("cmd /K {}", claude_args);

    // Title: task name preview (first 60 chars of prompt, or "Claude Code")
    let title = prompt
        .map(|p| {
            let trimmed: String = p.chars().take(60).collect();
            if p.len() > 60 { format!("{}...", trimmed) } else { trimmed }
        })
        .unwrap_or_else(|| "Claude Code".to_string());

    // Try Windows Terminal first, fall back to cmd start
    let (method, result) = {
        let wt_result = std::process::Command::new("wt")
            .args(["-w", "0", "new-tab", "--title", &title, "cmd", "/K", &claude_args])
            .current_dir(working_dir)
            .spawn();
        match wt_result {
            Ok(_) => ("wt", Ok(())),
            Err(_) => {
                // Fallback: cmd /C start with title
                let fallback = std::process::Command::new("cmd")
                    .args(["/C", "start", &format!("\"{}\"", title)])
                    .arg(&inner_cmd)
                    .current_dir(working_dir)
                    .spawn();
                match fallback {
                    Ok(_) => ("cmd", Ok(())),
                    Err(e) => ("cmd", Err(format!("Failed to open terminal: {}", e))),
                }
            }
        }
    };

    match result {
        Ok(()) => Ok(json!({
            "success": true,
            "message": format!("Claude Code terminal opened via {}", method),
            "working_dir": working_dir,
            "method": method
        })),
        Err(e) => Err(e),
    }
}

fn handle_gemini_direct(args: Value) -> Result<Value, String> {
    let prompt = args.get("prompt").and_then(|v| v.as_str())
        .ok_or("Missing 'prompt'")?;
    let working_dir = args.get("working_dir").and_then(|v| v.as_str()).unwrap_or(r"C:\");
    
    let output = std::process::Command::new(r"C:\Program Files\nodejs\node.exe")
        .args([gemini_cmd(), "--yolo", "-p", prompt])
        .current_dir(working_dir)
        .output()
        .map_err(|e| format!("Gemini CLI failed: {}", e))?;
    
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    
    Ok(json!({
        "success": output.status.success(),
        "output": stdout.trim(),
        "stderr": if stderr.is_empty() { None } else { Some(stderr.trim().to_string()) }
    }))
}

fn handle_codex_exec(args: Value) -> Result<Value, String> {
    let prompt = args.get("prompt").and_then(|v| v.as_str())
        .ok_or("Missing 'prompt'")?;
    let working_dir = args.get("working_dir").and_then(|v| v.as_str()).unwrap_or(r"C:\rust-mcp");
    let model = args.get("model").and_then(|v| v.as_str());
    let sandbox = args.get("sandbox").and_then(|v| v.as_str());
    let full_auto = args.get("full_auto").and_then(|v| v.as_bool()).unwrap_or(false);
    let skip_approvals = args.get("skip_approvals").and_then(|v| v.as_bool()).unwrap_or(false);
    
    let mut cmd = std::process::Command::new(codex_cmd());
    cmd.arg("exec");
    
    if let Some(m) = model {
        cmd.args(["--model", m]);
    }
    if let Some(s) = sandbox {
        cmd.args(["--sandbox", s]);
    }
    if full_auto {
        cmd.arg("--full-auto");
    }
    if skip_approvals {
        cmd.arg("--dangerously-bypass-approvals-and-sandbox");
    }
    cmd.args(["--cd", working_dir]);
    cmd.arg("--json");
    cmd.arg("--skip-git-repo-check");
    cmd.arg(prompt);
    
    let output = cmd
        .current_dir(working_dir)
        .output()
        .map_err(|e| format!("Codex exec failed: {}", e))?;
    
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    
    Ok(json!({
        "success": output.status.success(),
        "output": stdout.trim(),
        "stderr": if stderr.is_empty() { None } else { Some(stderr.trim().to_string()) },
        "exit_code": output.status.code()
    }))
}

fn handle_codex_review(args: Value) -> Result<Value, String> {
    let prompt = args.get("prompt").and_then(|v| v.as_str());
    let working_dir = args.get("working_dir").and_then(|v| v.as_str()).unwrap_or(r"C:\rust-mcp");
    let base = args.get("base").and_then(|v| v.as_str());
    let uncommitted = args.get("uncommitted").and_then(|v| v.as_bool()).unwrap_or(false);
    let commit = args.get("commit").and_then(|v| v.as_str());
    
    let mut cmd = std::process::Command::new(codex_cmd());
    cmd.arg("review");
    
    if let Some(b) = base {
        cmd.args(["--base", b]);
    }
    if uncommitted {
        cmd.arg("--uncommitted");
    }
    if let Some(sha) = commit {
        cmd.args(["--commit", sha]);
    }
    if let Some(p) = prompt {
        cmd.arg(p);
    }
    
    let output = cmd
        .current_dir(working_dir)
        .output()
        .map_err(|e| format!("Codex review failed: {}", e))?;
    
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    
    Ok(json!({
        "success": output.status.success(),
        "review": stdout.trim(),
        "stderr": if stderr.is_empty() { None } else { Some(stderr.trim().to_string()) },
        "exit_code": output.status.code()
    }))
}

// === FILE NAVIGATION ===
// Generated: 2026-04-08T14:12:33
// Total: 5569 lines | 97 functions | 16 structs | 12 constants
//
// IMPORTS: axum, chrono, once_cell, serde, serde_json, std, sysinfo, tokio, tower_http, tracing, uuid
//
// CONSTANTS:
//   const MAX_HISTORY_ENTRIES: 29
//   const GPT_API_URL: 30
//   const ROLLBACK_RETENTION_HOURS: 31
//   const DEFAULT_GPT_MODEL: 32
//   static _GEMINI_CMD: 58
//   static _CLAUDE_CODE_CMD: 65
//   static _CODEX_CMD: 72
//   static _NODE_CMD: 82
//   static _LOAVES_ARCHIVE_DIR: 88
//   const fn: 91
//   const SAFETY_VALIDATION_BLOCK: 1797
//   const SESSION_DIR: 5237
//
// STRUCTS:
//   JsonRpcRequest: 159-165
//   JsonRpcSuccess: 168-172
//   JsonRpcErrorResponse: 175-179
//   JsonRpcError: 182-185
//   TaskStep: 278-283
//   Task: 286-340
//   BackendRecommendation: 344-349
//   WorkflowStep: 352-370
//   WorkflowTemplate: 373-391
//   TemplateStep: 398-403
//   ServerConfig: 409-413
//   Server: 415-420
//   ParallelStepResult: 2807-2814
//   CustomRole: 3861-3866
//   DashboardState: 4722-4725
//   PathQuery: 5009-5011
//
// ENUMS:
//   Backend: 193-198
//   TaskStatus: 213-220
//   ExtractionStatus: 237-244
//   TrustLevel: 252-256
//   ValidationStatus: 264-269
//
// IMPL BLOCKS:
//   impl std::fmt::Display for Backend: 200-209
//   impl std::fmt::Display for TaskStatus: 222-233
//   impl Default for ExtractionStatus: 246-248
//   impl Default for TrustLevel: 258-260
//   impl Default for ValidationStatus: 271-273
//   impl Server: 422-1004
//
// FUNCTIONS:
//   default_data_dir: 36-42
//   tasks_dir: 97-97
//   history_dir: 98-98
//   gemini_cmd: 99-99
//   claude_code_cmd: 100-100
//   codex_cmd: 101-101
//   workflow_patterns_dir: 102-102
//   rollback_dir: 103-103
//   learned_patterns_dir: 104-104
//   node_cmd: 105-105
//   dashboard_prefs_path: 106-106
//   loaves_dir: 107-107
//   loaves_archive_dir: 108-108
//   load_terminal_visible: 111-117
//   spawn_visible_terminal: 121-152
//   default_max_retries: 275-275
//   default_backend: 393-393
//   default_trust_tmpl: 394-394
//   default_success_rate: 395-395
//   spawn_retry_execution: 1007-1053
//   spawn_on_complete: 1057-1102
//   run_gpt_task: 1108-1257 [LARGE]
//   run_codex_task: 1261-1370 [LARGE]
//   run_cli_task: 1371-1783 [LARGE]
//   safe_truncate: 1790-1795
//   ensure_safety_validation: 1799-1805
//   extract_safety_warning: 1807-1812
//   handle_submit_task: 1814-2107 [LARGE]
//   handle_watch_tasks: 2111-2231 [LARGE]
//   handle_get_status: 2233-2286 [med]
//   handle_get_output: 2288-2320
//   handle_list_tasks: 2322-2373 [med]
//   handle_cancel_task: 2375-2400
//   handle_pause_task: 2402-2422
//   handle_resume_task: 2424-2452
//   handle_configure: 2454-2488
//   handle_cleanup: 2490-2518
//   run_workflow_step: 2524-2604 [med]
//   handle_run_workflow: 2606-2799 [LARGE]
//   handle_run_parallel: 2816-2957 [LARGE]
//   run_parallel_workflow: 2960-3026 [med]
//   launch_step: 3029-3157 [LARGE]
//   handle_decompose_task: 3163-3241 [med]
//   handle_save_template: 3243-3265
//   handle_list_templates: 3267-3293
//   handle_run_template: 3295-3341
//   handle_explain_task: 3343-3387
//   loaf_path: 3393-3395
//   find_active_loaf: 3398-3417
//   handle_loaf_create: 3419-3457
//   handle_loaf_update: 3459-3595 [LARGE]
//   handle_loaf_status: 3597-3636
//   handle_loaf_close: 3638-3673
//   handle_list_sessions: 3679-3721
//   handle_get_analytics: 3723-3799 [med]
//   get_role_prompt: 3805-3845
//   list_roles: 3847-3857
//   custom_roles_dir: 3868-3872
//   load_custom_roles: 3874-3884
//   get_custom_role_prompt: 3886-3891
//   handle_role_list: 3893-3908
//   handle_role_create: 3910-3939
//   handle_role_delete: 3941-3951
//   save_task_artifact: 3957-3989
//   handle_tool_call: 3995-4037
//   handle_review_extractions: 4043-4060
//   handle_extract_workflow: 4062-4081
//   handle_dismiss_extraction: 4083-4091
//   handle_rollback_task: 4093-4099
//   handle_retry_task: 4102-4142
//   handle_route_task: 4148-4158
//   get_tools_list: 4164-4715 [LARGE]
//   dash_status: 4727-4742
//   dash_status_by_id: 4744-4757
//   dash_health: 4759-4773
//   dash_inbox: 4775-4794
//   flush_entry: 4796-4803
//   dash_get_prefs: 4805-4812
//   dash_post_prefs: 4814-4823
//   dash_post_task: 4825-4886 [med]
//   dash_cancel: 4888-4904
//   dash_knowledge: 4906-4932
//   dash_git: 4934-4946
//   dash_system: 4948-4971
//   dash_history: 4973-4982
//   volumes_base_path: 4984-4988
//   validate_volumes_path: 4991-5006
//   api_read_file: 5013-5033
//   api_list_dir: 5035-5066
//   start_dashboard: 5068-5105
//   main: 5111-5230 [LARGE]
//   handle_start_session: 5239-5316 [med]
//   handle_send_to_session: 5318-5407 [med]
//   handle_open_terminal: 5409-5464 [med]
//   handle_gemini_direct: 5466-5485
//   handle_codex_exec: 5487-5530
//   handle_codex_review: 5532-5569
//
// === END FILE NAVIGATION ===