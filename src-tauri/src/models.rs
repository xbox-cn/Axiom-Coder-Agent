use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub favorite: bool,
    pub created_at: String,
    pub updated_at: String,
    pub git_branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub status: RunStatus,
    pub created_at: String,
    pub updated_at: String,
    pub unread_approval: bool,
    #[serde(default)]
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub thread_id: String,
    pub role: MessageRole,
    pub content: String,
    pub created_at: String,
    pub run_id: Option<String>,
    pub pinned: bool,
    #[serde(default)]
    pub attachments: Vec<AttachmentSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentSnapshot {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub size: u64,
    pub sha256: String,
    pub snapshot_path: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub id: String,
    pub thread_id: String,
    pub status: RunStatus,
    pub config: RunConfigSnapshot,
    pub usage: UsageRecord,
    pub error: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadDetail {
    pub thread: ThreadSummary,
    pub messages: Vec<Message>,
    pub runs: Vec<RunRecord>,
    pub context_snapshots: Vec<ContextSnapshot>,
    #[serde(default)]
    pub goals: Vec<GoalRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalRecord {
    pub id: String,
    pub run_id: String,
    pub thread_id: String,
    pub status: String,
    pub turn_count: u64,
    pub started_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSnapshot {
    pub id: String,
    pub thread_id: String,
    pub run_id: Option<String>,
    pub summary: String,
    pub token_count: u64,
    pub start_message_id: Option<String>,
    pub end_message_id: Option<String>,
    pub source_message_ids: Vec<String>,
    pub active: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProfile {
    pub id: String,
    pub kind: ProviderKind,
    pub name: String,
    pub base_url: String,
    pub default_model: String,
    pub enabled: bool,
    pub timeout_seconds: u64,
    pub extra_headers: serde_json::Value,
    pub has_credential: bool,
    pub created_at: String,
    pub updated_at: String,
    pub api_type: ProviderApiType,
    #[serde(default)]
    pub models: Vec<ProviderModel>,
    #[serde(default)]
    pub legacy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProfileInput {
    pub id: Option<String>,
    pub kind: ProviderKind,
    pub name: String,
    pub base_url: String,
    pub default_model: String,
    pub enabled: bool,
    pub timeout_seconds: u64,
    #[serde(default)]
    pub extra_headers: serde_json::Value,
    pub api_key: Option<String>,
    #[serde(default)]
    pub api_type: ProviderApiType,
    #[serde(default)]
    pub models: Vec<ProviderModelInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModel {
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub context_window_tokens: Option<u64>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelInput {
    pub model_id: String,
    pub display_name: Option<String>,
    pub context_window_tokens: Option<u64>,
    #[serde(default = "default_model_source")]
    pub source: String,
}

fn default_model_source() -> String {
    "manual".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDescriptor {
    pub id: String,
    pub display_name: String,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub capabilities: ModelCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilities {
    pub tools: bool,
    pub vision: bool,
    pub reasoning: bool,
    pub reasoning_levels: Vec<ThinkingLevel>,
    pub usage_reporting: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOverride {
    pub provider_id: String,
    pub model_id: String,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub input_price_per_million: Option<f64>,
    pub output_price_per_million: Option<f64>,
    pub cache_price_per_million: Option<f64>,
    pub reasoning_price_per_million: Option<f64>,
    pub capabilities: Option<ModelCapabilities>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunConfigSnapshot {
    pub provider_id: String,
    pub model_id: String,
    pub thinking_level: ThinkingLevel,
    pub permission_mode: PermissionMode,
    pub max_output_tokens: Option<u64>,
    pub created_at: String,
    #[serde(default)]
    pub run_mode: RunMode,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRecord {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub context_tokens: u64,
    pub context_limit: u64,
    pub cumulative_tokens: u64,
    pub estimated: bool,
    pub duration_ms: Option<u64>,
    pub first_token_ms: Option<u64>,
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftModelTestResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub response_preview: String,
    pub usage: Option<UsageRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    pub id: String,
    pub name: String,
    pub scope: McpScope,
    pub project_id: Option<String>,
    pub transport: McpTransport,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub url: Option<String>,
    pub env: serde_json::Value,
    pub headers: serde_json::Value,
    pub timeout_seconds: u64,
    pub enabled: bool,
    pub status: String,
    pub last_error: Option<String>,
    #[serde(default)]
    pub discovered_tools: Vec<String>,
    #[serde(default)]
    pub disabled_tools: Vec<String>,
    /// Tools whose MCP annotations explicitly set readOnlyHint=true and destructiveHint=false.
    #[serde(default)]
    pub read_only_tools: Vec<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBootstrap {
    pub projects: Vec<Project>,
    pub threads: Vec<ThreadSummary>,
    pub providers: Vec<ProviderProfile>,
    pub mcp_servers: Vec<McpServerConfig>,
    pub settings: AppSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub theme: String,
    pub sidebar_collapsed: bool,
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: u32,
    pub inspector_open: bool,
    pub inspector_width: u32,
    pub default_permission: PermissionMode,
    pub default_provider_id: Option<String>,
    pub default_model_id: Option<String>,
    pub default_thinking_level: ThinkingLevel,
}

fn default_sidebar_width() -> u32 {
    272
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "system".to_string(),
            sidebar_collapsed: false,
            sidebar_width: default_sidebar_width(),
            inspector_open: true,
            inspector_width: 420,
            default_permission: PermissionMode::WorkspaceAuto,
            default_provider_id: None,
            default_model_id: None,
            default_thinking_level: ThinkingLevel::Medium,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub id: String,
    pub tool_name: String,
    pub summary: String,
    pub arguments: serde_json::Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolActivity {
    pub id: String,
    pub name: String,
    pub status: String,
    pub summary: String,
    pub output: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEvent {
    pub sequence: u64,
    pub run_id: String,
    pub thread_id: String,
    pub kind: AgentEventKind,
    pub status: RunStatus,
    pub content: Option<String>,
    pub message: Option<Message>,
    pub usage: Option<UsageRecord>,
    pub error: Option<String>,
    pub approval: Option<ApprovalRequest>,
    pub tool_activity: Option<ToolActivity>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentEventKind {
    Status,
    TextDelta,
    MessageCompleted,
    Usage,
    ToolStarted,
    ToolCompleted,
    ApprovalRequested,
    ContextCompressed,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RunStatus {
    Idle,
    Queued,
    Reasoning,
    Streaming,
    ToolRunning,
    AwaitingApproval,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    OpenAi,
    Anthropic,
    Gemini,
    OpenRouter,
    Ollama,
    OpenAiCompatible,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderApiType {
    Responses,
    #[default]
    ChatCompletions,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RunMode {
    #[default]
    Agent,
    Plan,
    Goal,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ThinkingLevel {
    Off,
    Low,
    Medium,
    High,
    Xhigh,
    Auto,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionMode {
    ReadOnly,
    WorkspaceAuto,
    FullAccess,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum McpScope {
    Global,
    Project,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum McpTransport {
    Stdio,
    StreamableHttp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    pub path: String,
    pub line: u64,
    pub column: u64,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMutation {
    pub path: String,
    pub before: Option<String>,
    pub operation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSummary {
    pub branch: Option<String>,
    pub changed_files: Vec<GitFileChange>,
    pub diff: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileChange {
    pub status: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellResult {
    pub command: String,
    pub cwd: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTestResult {
    pub ok: bool,
    pub server_name: Option<String>,
    pub protocol_version: Option<String>,
    pub tools: Vec<String>,
    #[serde(default)]
    pub read_only_tools: Vec<String>,
    pub latency_ms: u64,
    pub message: String,
}

#[cfg(test)]
mod app_settings_tests {
    use super::AppSettings;

    #[test]
    fn app_settings_backfills_sidebar_width() {
        let value = serde_json::json!({
            "theme": "system",
            "sidebarCollapsed": false,
            "inspectorOpen": true,
            "inspectorWidth": 420,
            "defaultPermission": "workspace-auto",
            "defaultProviderId": null,
            "defaultModelId": null,
            "defaultThinkingLevel": "medium"
        });
        let settings: AppSettings = serde_json::from_value(value).unwrap();
        assert_eq!(settings.sidebar_width, 272);
    }
}
