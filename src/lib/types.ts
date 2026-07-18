export type RunStatus = "idle" | "queued" | "reasoning" | "streaming" | "tool-running" | "awaiting-approval" | "completed" | "failed" | "cancelled";
export type RunMode = "agent" | "plan" | "goal";
export type ThinkingLevel = "off" | "low" | "medium" | "high" | "xhigh" | "auto";
export type PermissionMode = "read-only" | "workspace-auto" | "full-access";
export type ProviderKind = "open-ai" | "anthropic" | "gemini" | "open-router" | "ollama" | "open-ai-compatible";
export type ProviderApiType = "responses" | "chat-completions";
export type MessageRole = "system" | "user" | "assistant" | "tool";
export type InspectorTab = "changes" | "files" | "terminal" | "context";

export interface Project { id:string; name:string; path:string; favorite:boolean; createdAt:string; updatedAt:string; gitBranch?:string|null }
export interface ThreadSummary { id:string; projectId:string; title:string; status:RunStatus; createdAt:string; updatedAt:string; unreadApproval:boolean; archived:boolean }
export interface AttachmentSnapshot { id:string; name:string; mimeType:string; size:number; sha256:string; snapshotPath:string; kind:"text"|"image" }
export interface Message { id:string; threadId:string; role:MessageRole; content:string; createdAt:string; runId?:string|null; pinned:boolean; attachments:AttachmentSnapshot[] }
export interface ModelCapabilities { tools:boolean; vision:boolean; reasoning:boolean; reasoningLevels:ThinkingLevel[]; usageReporting:boolean }
export interface ModelDescriptor { id:string; displayName:string; contextWindow?:number|null; maxOutputTokens?:number|null; capabilities:ModelCapabilities }
export interface ModelOverride { providerId:string; modelId:string; contextWindow?:number|null; maxOutputTokens?:number|null; inputPricePerMillion?:number|null; outputPricePerMillion?:number|null; cachePricePerMillion?:number|null; reasoningPricePerMillion?:number|null; capabilities?:ModelCapabilities|null }
export interface ProviderModel { providerId:string; modelId:string; displayName:string; contextWindowTokens?:number|null; source:"upstream"|"manual"|"legacy" }
export interface ProviderModelInput { modelId:string; displayName?:string|null; contextWindowTokens?:number|null; source:"upstream"|"manual" }
export interface ThreadRunPreferences { providerId:string; modelId:string; thinkingLevel:ThinkingLevel; permissionMode:PermissionMode; runMode:RunMode }
export interface RunConfigSnapshot { providerId:string; modelId:string; thinkingLevel:ThinkingLevel; permissionMode:PermissionMode; runMode:RunMode; maxOutputTokens?:number|null; createdAt:string }
export interface UsageRecord { inputTokens?:number|null; outputTokens?:number|null; cachedTokens?:number|null; reasoningTokens?:number|null; contextTokens:number; contextLimit:number; cumulativeTokens:number; estimated:boolean; durationMs?:number|null; firstTokenMs?:number|null; estimatedCostUsd?:number|null }
export interface RunRecord { id:string; threadId:string; status:RunStatus; config:RunConfigSnapshot; usage:UsageRecord; reasoningContent?:string; error?:string|null; startedAt:string; completedAt?:string|null }
export interface ContextSnapshot { id:string; threadId:string; runId?:string|null; summary:string; tokenCount:number; startMessageId?:string|null; endMessageId?:string|null; sourceMessageIds:string[]; active:boolean; createdAt:string }
export interface GoalRecord { id:string; runId:string; threadId:string; status:"running"|"awaiting-approval"|"paused"|"completed"|"failed"|"blocked"; turnCount:number; startedAt:string; updatedAt:string; completedAt?:string|null }
export interface ThreadDetail { thread:ThreadSummary; runPreferences?:ThreadRunPreferences|null; messages:Message[]; runs:RunRecord[]; contextSnapshots:ContextSnapshot[]; goals:GoalRecord[] }
export interface ProviderProfile { id:string; kind:ProviderKind; name:string; baseUrl:string; defaultModel:string; enabled:boolean; timeoutSeconds:number; extraHeaders:Record<string,string>; hasCredential:boolean; createdAt:string; updatedAt:string; apiType:ProviderApiType; models:ProviderModel[]; legacy:boolean }
export interface ProviderProfileInput { id?:string; kind:ProviderKind; name:string; baseUrl:string; defaultModel:string; enabled:boolean; timeoutSeconds:number; extraHeaders:Record<string,string>; apiKey?:string; apiType:ProviderApiType; models:ProviderModelInput[] }
export interface McpServerConfig { id:string; name:string; scope:"global"|"project"; projectId?:string|null; transport:"stdio"|"streamable-http"; command?:string|null; args:string[]; cwd?:string|null; url?:string|null; env:Record<string,string>; headers:Record<string,string>; timeoutSeconds:number; enabled:boolean; status:string; lastError?:string|null; discoveredTools:string[]; disabledTools:string[]; readOnlyTools:string[]; updatedAt:string }
export interface DraftModelTestResult { ok:boolean; latencyMs:number; responsePreview:string; usage?:UsageRecord|null }
export interface McpTestResult { ok:boolean; serverName?:string|null; protocolVersion?:string|null; tools:string[]; readOnlyTools:string[]; latencyMs:number; message:string }
export interface AppSettings { theme:"system"|"light"|"dark"; sidebarCollapsed:boolean; sidebarWidth:number; inspectorOpen:boolean; inspectorWidth:number; defaultPermission:PermissionMode; defaultProviderId?:string|null; defaultModelId?:string|null; defaultThinkingLevel:ThinkingLevel }
export interface AppBootstrap { projects:Project[]; threads:ThreadSummary[]; providers:ProviderProfile[]; mcpServers:McpServerConfig[]; settings:AppSettings }
export interface ApprovalRequest { id:string; toolName:string; summary:string; arguments:Record<string,unknown>; createdAt:string }
export interface ToolActivity { id:string; name:string; status:"running"|"completed"|"failed"; summary:string; output?:string|null; durationMs?:number|null }
export interface AgentEvent { sequence:number; runId:string; threadId:string; kind:"status"|"text-delta"|"reasoning-delta"|"message-completed"|"usage"|"tool-started"|"tool-completed"|"approval-requested"|"context-compressed"|"error"; status:RunStatus; content?:string|null; message?:Message|null; usage?:UsageRecord|null; error?:string|null; approval?:ApprovalRequest|null; toolActivity?:ToolActivity|null; createdAt:string }
export interface FileEntry { name:string; path:string; isDirectory:boolean; size:number }
export interface GitFileChange { status:string; path:string }
export interface GitSummary { branch?:string|null; changedFiles:GitFileChange[]; diff:string }
export interface ShellResult { command:string; cwd:string; exitCode?:number|null; stdout:string; stderr:string; durationMs:number }
export interface SearchMatch { path:string; line:number; column:number; preview:string }
