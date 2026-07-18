import type {
  AppBootstrap,
  McpServerConfig,
  ProviderProfile,
  RunConfigSnapshot,
  RunRecord,
  ThreadDetail,
  UsageRecord,
} from "../lib/types";

export const provider = (id = "provider-a", model = "model-a"): ProviderProfile => ({
  id,
  kind: "open-ai-compatible",
  name: id,
  baseUrl: "http://127.0.0.1:11434/v1",
  defaultModel: model,
  enabled: true,
  timeoutSeconds: 120,
  extraHeaders: {},
  hasCredential: false,
  createdAt: "2026-01-01T00:00:00.000Z",
  updatedAt: "2026-01-01T00:00:00.000Z",
  apiType: "chat-completions",
  models: [{ providerId: id, modelId: model, displayName: model, contextWindowTokens: 128000, source: "manual" }],
  legacy: false,
});

export const mcpServer = (id = "mcp-a"): McpServerConfig => ({
  id,
  name: id,
  scope: "global",
  projectId: null,
  transport: "stdio",
  command: "node",
  args: ["server.js"],
  cwd: null,
  url: null,
  env: {},
  headers: {},
  timeoutSeconds: 30,
  enabled: true,
  status: "stopped",
  lastError: null,
  discoveredTools: [],
  disabledTools: [],
  readOnlyTools: [],
  updatedAt: "2026-01-01T00:00:00.000Z",
});

export const usage = (estimated = false): UsageRecord => ({
  inputTokens: 1200,
  outputTokens: 300,
  cachedTokens: 200,
  reasoningTokens: 80,
  contextTokens: 7200,
  contextLimit: 128000,
  cumulativeTokens: 1500,
  estimated,
  durationMs: 1250,
  firstTokenMs: 320,
  estimatedCostUsd: 0.0042,
});

export const runConfig = (providerId = "provider-a", modelId = "model-a"): RunConfigSnapshot => ({
  providerId,
  modelId,
  thinkingLevel: "medium",
  permissionMode: "workspace-auto",
  runMode: "agent",
  createdAt: "2026-01-01T00:00:00.000Z",
});

export const runRecord = (
  id = "run-a",
  status: RunRecord["status"] = "completed",
  config = runConfig(),
  recordUsage = usage(false),
): RunRecord => ({
  id,
  threadId: "thread-a",
  status,
  config,
  usage: recordUsage,
  startedAt: "2026-01-01T00:00:00.000Z",
  completedAt: status === "completed" ? "2026-01-01T00:00:01.000Z" : null,
  error: null,
});

export const bootstrap = (): AppBootstrap => ({
  projects: [{
    id: "project-a",
    name: "Axiom",
    path: "D:\\Axiom",
    favorite: true,
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-01-01T00:00:00.000Z",
    gitBranch: "main",
  }],
  threads: [{
    id: "thread-a",
    projectId: "project-a",
    title: "测试任务",
    status: "idle",
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-01-01T00:00:00.000Z",
    unreadApproval: false,
    archived: false,
  }],
  providers: [provider(), provider("provider-b", "model-b")],
  mcpServers: [mcpServer()],
  settings: {
    theme: "system",
    sidebarCollapsed: false,
    sidebarWidth: 272,
    inspectorOpen: true,
    inspectorWidth: 420,
    defaultPermission: "workspace-auto",
    defaultProviderId: "provider-a",
    defaultModelId: "model-a",
    defaultThinkingLevel: "medium",
  },
});

export const threadDetail = (status: ThreadDetail["thread"]["status"] = "idle"): ThreadDetail => ({
  thread: { ...bootstrap().threads[0], status },
  messages: [],
  runs: [],
  contextSnapshots: [],
  goals: [],
});
