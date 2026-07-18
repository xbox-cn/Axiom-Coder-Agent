import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { demoBootstrap, demoRun, demoThread } from "./mock";
import type {
  AgentEvent,
  AppBootstrap,
  AppSettings,
  AttachmentSnapshot,
  FileEntry,
  GitSummary,
  McpServerConfig,
  McpTestResult,
  ModelDescriptor,
  ModelOverride,
  PermissionMode,
  Project,
  ProviderApiType,
  ProviderProfile,
  ProviderProfileInput,
  RunConfigSnapshot,
  RunRecord,
  SearchMatch,
  ShellResult,
  ThreadDetail,
  ThreadSummary,
} from "./types";

const isTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const listeners = new Set<(event: AgentEvent) => void>();
const cancelledMockRuns = new Set<string>();
const mockRuns = new Map<string, RunRecord>();
const mockApprovals = new Map<string, { runId: string; resolve: (approved: boolean) => void }>();

export async function getBootstrap(): Promise<AppBootstrap> {
  return isTauri() ? invoke("bootstrap") : structuredClone(demoBootstrap);
}

export async function pickProjectDirectory(): Promise<string | null> {
  if (!isTauri()) return "D:\\Projects\\new-project";
  const result = await open({ directory: true, multiple: false, title: "选择 Axiom 项目文件夹" });
  return typeof result === "string" ? result : null;
}

export async function addProject(path: string): Promise<Project> {
  if (isTauri()) return invoke("add_project", { path });
  const now = new Date().toISOString();
  return {
    id: crypto.randomUUID(),
    name: path.split(/[\\/]/).filter(Boolean).at(-1) ?? "project",
    path,
    favorite: false,
    createdAt: now,
    updatedAt: now,
    gitBranch: "main",
  };
}

export async function createThread(projectId: string, title?: string): Promise<ThreadSummary> {
  if (isTauri()) return invoke("create_thread", { projectId, title });
  const now = new Date().toISOString();
  return {
    id: crypto.randomUUID(),
    projectId,
    title: title || "新任务",
    status: "idle",
    createdAt: now,
    updatedAt: now,
    unreadApproval: false,
  };
}

export async function getThread(threadId: string): Promise<ThreadDetail> {
  if (isTauri()) return invoke("get_thread", { threadId });
  return {
    ...structuredClone(demoThread),
    thread: { ...demoThread.thread, id: threadId },
    messages: demoThread.messages.map((message) => ({ ...message, threadId })),
    runs: demoThread.runs.map((run) => ({ ...run, threadId })),
    contextSnapshots: [],
    goals: [],
  };
}

export async function restoreContextSnapshot(snapshotId: string): Promise<void> {
  if (isTauri()) return invoke("restore_context_snapshot", { snapshotId });
}

export async function saveProvider(input: ProviderProfileInput): Promise<ProviderProfile> {
  if (isTauri()) return invoke("save_provider", { input });
  const id = input.id ?? crypto.randomUUID();
  return {
    ...input, id, legacy: false, hasCredential: Boolean(input.apiKey),
    models: input.models.map((model) => ({ providerId: id, modelId: model.modelId, displayName: model.displayName || model.modelId, contextWindowTokens: model.contextWindowTokens ?? null, source: model.source })),
    createdAt: new Date().toISOString(), updatedAt: new Date().toISOString(),
  };
}

export async function deleteProvider(providerId: string): Promise<void> {
  if (isTauri()) return invoke("delete_provider", { providerId });
}

export async function getModelOverride(providerId: string, modelId: string): Promise<ModelOverride | null> {
  return isTauri() ? invoke("get_model_override", { providerId, modelId }) : null;
}

export async function saveModelOverride(value: ModelOverride): Promise<ModelOverride> {
  return isTauri() ? invoke("save_model_override", { value }) : value;
}

export async function discoverProviderModelsDraft(
  apiType: ProviderApiType,
  baseUrl: string,
  apiKey?: string,
): Promise<ModelDescriptor[]> {
  if (isTauri()) return invoke("discover_provider_models_draft", { apiType, baseUrl, apiKey: apiKey || null });
  await sleep(450);
  return ["axiom-coder", "axiom-coder-mini"].map((id, index) => ({
    id,
    displayName: id,
    contextWindow: index === 0 ? 128_000 : null,
    maxOutputTokens: null,
    capabilities: { tools: true, vision: true, reasoning: true, reasoningLevels: ["off", "low", "medium", "high", "xhigh", "auto"], usageReporting: true },
  }));
}

export async function discoverModels(providerId: string): Promise<ModelDescriptor[]> {
  if (isTauri()) return invoke("discover_models", { providerId });
  await sleep(600);
  return ["qwen3-coder", "gpt-5.4", "claude-sonnet-4-5"].map((id) => ({
    id,
    displayName: id,
    contextWindow: 128_000,
    maxOutputTokens: 16_384,
    capabilities: {
      tools: true,
      vision: true,
      reasoning: true,
      reasoningLevels: ["off", "low", "medium", "high", "auto"],
      usageReporting: true,
    },
  }));
}

export async function testProvider(providerId: string): Promise<ModelDescriptor[]> {
  return isTauri() ? invoke("test_provider", { providerId }) : discoverModels(providerId);
}

export async function saveMcpServer(input: McpServerConfig): Promise<McpServerConfig> {
  return isTauri()
    ? invoke("save_mcp_server", { input })
    : { ...input, id: input.id || crypto.randomUUID(), status: "stopped", updatedAt: new Date().toISOString() };
}

export async function deleteMcpServer(serverId: string): Promise<void> {
  if (isTauri()) return invoke("delete_mcp_server", { serverId });
}

export async function testMcpServer(input: McpServerConfig): Promise<McpTestResult> {
  if (isTauri()) return invoke("test_mcp_server", { input });
  await sleep(700);
  return {
    ok: true,
    serverName: input.name,
    protocolVersion: "2025-06-18",
    tools: ["search_code", "read_file", "create_issue"],
    readOnlyTools: ["search_code", "read_file"],
    latencyMs: 142,
    message: "MCP 服务连接正常",
  };
}

export async function saveSettings(settings: AppSettings): Promise<AppSettings> {
  return isTauri() ? invoke("save_settings", { settings }) : settings;
}

export async function listProjectFiles(threadId: string, path?: string): Promise<FileEntry[]> {
  if (isTauri()) return invoke("list_project_files", { threadId, path });
  return [
    { name: "src", path: "D:\\Projects\\axiom-demo\\src", isDirectory: true, size: 0 },
    { name: "package.json", path: "D:\\Projects\\axiom-demo\\package.json", isDirectory: false, size: 1840 },
    { name: "README.md", path: "D:\\Projects\\axiom-demo\\README.md", isDirectory: false, size: 4210 },
    { name: "tsconfig.json", path: "D:\\Projects\\axiom-demo\\tsconfig.json", isDirectory: false, size: 622 },
  ];
}

export async function searchProjectFiles(threadId: string, query: string, path?: string): Promise<SearchMatch[]> {
  if (isTauri()) return invoke("search_project_files", { threadId, query, path });
  await sleep(120);
  const normalized = query.toLowerCase();
  const samples: SearchMatch[] = [
    { path: "src/auth/session.ts", line: 18, column: 10, preview: "export function refreshSession() {" },
    { path: "src/middleware/auth.ts", line: 42, column: 7, preview: "await refreshSession();" },
  ];
  return samples.filter((item) => `${item.path} ${item.preview}`.toLowerCase().includes(normalized));
}

export async function readProjectFile(threadId: string, path: string): Promise<string> {
  if (isTauri()) return invoke("read_project_file", { threadId, path });
  return `// ${path}\n\nexport function refreshSession() {\n  return requestRefresh();\n}\n`;
}

export async function getGitSummary(threadId: string): Promise<GitSummary> {
  if (isTauri()) return invoke("get_git_summary", { threadId });
  return {
    branch: "main",
    changedFiles: [
      { status: "M", path: "src/auth/session.ts" },
      { status: "M", path: "src/middleware/auth.ts" },
      { status: "A", path: "src/auth/session.test.ts" },
    ],
    diff: `diff --git a/src/auth/session.ts b/src/auth/session.ts
index 3ce2..51bf 100644
--- a/src/auth/session.ts
+++ b/src/auth/session.ts
@@ -12,7 +12,14 @@
-let refreshInFlight = false;
+let refreshInFlight: Promise<Session> | null = null;

 export function refreshSession() {
-  return requestRefresh();
+  refreshInFlight ??= requestRefresh().finally(() => {
+    refreshInFlight = null;
+  });
+
+  return refreshInFlight;
 }`,
  };
}

export async function executeShell(
  threadId: string,
  command: string,
  permissionMode: PermissionMode,
  approved = false,
): Promise<ShellResult> {
  if (isTauri()) return invoke("execute_shell", { threadId, command, permissionMode, approved });
  await sleep(450);
  return {
    command,
    cwd: "D:\\Projects\\axiom-demo",
    exitCode: 0,
    stdout: `> ${command}\n✓ 18 tests passed in 1.42s`,
    stderr: "",
    durationMs: 1420,
  };
}

export async function pickAttachmentFiles(): Promise<string[]> {
  if (!isTauri()) return [];
  const result = await open({ multiple: true, directory: false, title: "选择附件" });
  if (!result) return [];
  return Array.isArray(result) ? result : [result];
}

export async function onAttachmentDrop(callback: (paths: string[]) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return getCurrentWindow().onDragDropEvent(({ payload }) => {
    if (payload.type === "drop" && payload.paths.length) callback(payload.paths);
  });
}

export async function prepareAttachments(paths: string[]): Promise<AttachmentSnapshot[]> {
  if (!paths.length) return [];
  if (isTauri()) return invoke("prepare_attachments", { paths });
  return paths.map((path) => ({ id: crypto.randomUUID(), name: path.split(/[\\/]/).filter(Boolean).at(-1) ?? path, mimeType: "text/plain", size: 0, sha256: "browser-preview", snapshotPath: path, kind: "text" as const }));
}

export async function startAgentRun(
  threadId: string,
  prompt: string,
  config: RunConfigSnapshot,
  attachments: AttachmentSnapshot[] = [],
): Promise<RunRecord> {
  if (isTauri()) return invoke("start_agent_run", { threadId, prompt, config, attachments });
  const run: RunRecord = {
    ...structuredClone(demoRun),
    id: crypto.randomUUID(),
    threadId,
    status: "queued",
    config: { ...config, createdAt: new Date().toISOString() },
    startedAt: new Date().toISOString(),
    completedAt: null,
  };
  mockRuns.set(run.id, run);
  void simulateRun(run, prompt);
  return run;
}

export async function cancelAgentRun(runId: string): Promise<void> {
  if (isTauri()) return invoke("cancel_agent_run", { runId });
  cancelledMockRuns.add(runId);
  for (const [approvalId, approval] of mockApprovals) {
    if (approval.runId === runId) {
      approval.resolve(false);
      mockApprovals.delete(approvalId);
    }
  }
  const run = mockRuns.get(runId);
  if (run) emitMockEvent(run, { kind: "status", status: "cancelled" });
}

export async function resumeGoal(runId: string): Promise<RunRecord> {
  if (isTauri()) return invoke("resume_goal", { runId });
  const existing = mockRuns.get(runId);
  if (!existing || existing.config.runMode !== "goal") throw new Error("Goal does not exist");
  cancelledMockRuns.delete(runId);
  const run = { ...existing, status: "queued" as const, error: null, completedAt: null };
  mockRuns.set(runId, run);
  void simulateRun(run, "Continue pursuing the existing goal from its persisted state.");
  return run;
}

export async function finishGoal(runId: string): Promise<void> {
  if (isTauri()) return invoke("finish_goal", { runId });
  cancelledMockRuns.add(runId);
  const run = mockRuns.get(runId);
  if (run) emitMockEvent(run, { kind: "status", status: "cancelled" });
}

export async function respondApproval(approvalId: string, approved: boolean): Promise<void> {
  if (isTauri()) return invoke("respond_approval", { approvalId, approved });
  const pending = mockApprovals.get(approvalId);
  if (!pending) throw new Error("审批请求已结束");
  mockApprovals.delete(approvalId);
  pending.resolve(approved);
}

export async function restoreRunChanges(runId: string): Promise<number> {
  if (isTauri()) return invoke("restore_run_changes", { runId });
  await sleep(220);
  return 3;
}

export async function restoreRunFileChanges(runId: string, path: string): Promise<number> {
  if (isTauri()) return invoke("restore_run_file_changes", { runId, path });
  await sleep(160);
  return 1;
}

export async function openProjectFileExternal(threadId: string, path: string): Promise<void> {
  if (isTauri()) return invoke("open_project_file_external", { threadId, path });
}

type BufferedListener = { push: (event: AgentEvent) => void; dispose: () => void };

function createBufferedListener(callback: (event: AgentEvent) => void): BufferedListener {
  let pending: AgentEvent | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;
  const flush = () => {
    if (timer) clearTimeout(timer);
    timer = null;
    if (pending) callback(pending);
    pending = null;
  };
  return {
    push(event) {
      if (event.kind !== "text-delta") {
        flush();
        callback(event);
        return;
      }
      if (pending && pending.runId === event.runId && pending.threadId === event.threadId) {
        pending = {
          ...event,
          content: `${pending.content ?? ""}${event.content ?? ""}`,
        };
      } else {
        flush();
        pending = event;
      }
      timer ??= setTimeout(flush, 24);
    },
    dispose() {
      if (timer) clearTimeout(timer);
      timer = null;
      pending = null;
    },
  };
}

export async function onAgentEvent(callback: (event: AgentEvent) => void): Promise<UnlistenFn> {
  const buffered = createBufferedListener(callback);
  if (isTauri()) {
    const unlisten = await listen<AgentEvent>("agent-event", (event) => buffered.push(event.payload));
    return () => {
      buffered.dispose();
      unlisten();
    };
  }
  listeners.add(buffered.push);
  return () => {
    listeners.delete(buffered.push);
    buffered.dispose();
  };
}

let mockSequence = 0;
function emitMockEvent(run: RunRecord, event: Partial<AgentEvent>) {
  const payload: AgentEvent = {
    sequence: ++mockSequence,
    runId: run.id,
    threadId: run.threadId,
    kind: "status",
    status: "reasoning",
    createdAt: new Date().toISOString(),
    ...event,
  };
  listeners.forEach((listener) => listener(payload));
}

async function simulateRun(run: RunRecord, prompt: string) {
  const emit = (event: Partial<AgentEvent>) => emitMockEvent(run, event);
  const stopped = () => cancelledMockRuns.has(run.id);

  await sleep(250);
  if (stopped()) return;
  emit({ status: "reasoning", content: "正在分析项目上下文" });
  await sleep(380);
  if (stopped()) return;

  const toolId = crypto.randomUUID();
  emit({
    kind: "tool-started",
    status: "tool-running",
    toolActivity: { id: toolId, name: "read_file", status: "running", summary: "读取 src/auth/session.ts" },
  });
  await sleep(420);
  if (stopped()) return;
  emit({
    kind: "tool-completed",
    status: "reasoning",
    toolActivity: {
      id: toolId,
      name: "read_file",
      status: "completed",
      summary: "已读取 src/auth/session.ts",
      output: "42 lines · 1.8 KB",
      durationMs: 418,
    },
  });

  let approvalDenied = false;
  if (/shell|命令|运行测试/i.test(prompt)) {
    const approvalId = crypto.randomUUID();
    const approved = new Promise<boolean>((resolve) => {
      mockApprovals.set(approvalId, { runId: run.id, resolve });
    });
    emit({
      kind: "approval-requested",
      status: "awaiting-approval",
      approval: {
        id: approvalId,
        toolName: "shell",
        summary: "运行 PowerShell：pnpm test",
        arguments: { command: "pnpm test" },
        createdAt: new Date().toISOString(),
      },
    });
    approvalDenied = !(await approved);
    if (stopped()) return;
    emit({ kind: "status", status: "reasoning", content: approvalDenied ? "命令已被拒绝，继续生成说明。" : "命令已批准。" });
  }

  await sleep(260);
  if (stopped()) return;
  emit({ status: "streaming" });
  const response = approvalDenied
    ? "Shell 命令已按你的选择拒绝，工作区未执行该命令。我会继续基于已有上下文提供修改建议。"
    : `我已经分析了你的请求：“${prompt.slice(0, 80)}”。\n\n下一步会先读取相关入口文件，再以最小 Diff 完成修改，并在完成后运行针对性测试。当前是浏览器预览模式；在 Tauri 桌面版本中，这里会连接你选择的真实供应商并执行本地工具。`;
  for (const piece of response.match(/.{1, 12}/gs) ?? []) {
    await sleep(12);
    if (stopped()) return;
    emit({ kind: "text-delta", status: "streaming", content: piece });
  }
  const message = {
    id: crypto.randomUUID(),
    threadId: run.threadId,
    role: "assistant" as const,
    content: response,
    createdAt: new Date().toISOString(),
    runId: run.id,
    pinned: false,
    attachments: [],
  };
  emit({
    kind: "message-completed",
    status: "completed",
    message,
    usage: {
      inputTokens: 4820,
      outputTokens: 316,
      cachedTokens: 1200,
      reasoningTokens: 88,
      contextTokens: 9120,
      contextLimit: 128_000,
      cumulativeTokens: 22_140,
      estimated: false,
      durationMs: 1940,
      firstTokenMs: 950,
      estimatedCostUsd: 0.0032,
    },
  });
  mockRuns.delete(run.id);
}
