import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../lib/api";
import { useAppStore } from "./appStore";
import { bootstrap, mcpServer, provider, runConfig, runRecord, threadDetail, usage } from "../test/fixtures";
import type { AgentEvent, RunConfigSnapshot } from "../lib/types";

vi.mock("../lib/api", () => ({
  createProjectDirectory: vi.fn(),
  createThread: vi.fn(),
  startAgentRun: vi.fn(),
  cancelAgentRun: vi.fn(),
  resumeGoal: vi.fn(),
  finishGoal: vi.fn(),
  respondApproval: vi.fn(),
  restoreContextSnapshot: vi.fn(),
  restoreRunChanges: vi.fn(),
  restoreRunFileChanges: vi.fn(),
  openProjectFileExternal: vi.fn(),
  getThread: vi.fn(),
  saveThreadRunPreferences: vi.fn(async (_threadId, preferences) => preferences),
  saveProvider: vi.fn(),
  deleteProvider: vi.fn(),
  saveMcpServer: vi.fn(),
  deleteMcpServer: vi.fn(),
  listProjectFiles: vi.fn(),
  getGitSummary: vi.fn(),
}));

const mocked = vi.mocked(api);

function seed() {
  useAppStore.setState({
    bootstrapData: bootstrap(),
    activeProjectId: "project-a",
    activeThreadId: "thread-a",
    threadDetail: threadDetail(),
    draft: "",
    providerId: "provider-a",
    modelId: "model-a",
    thinkingLevel: "medium",
    permissionMode: "workspace-auto",
    activeRunId: null,
    streamingContent: "",
    pendingApproval: null,
    toolActivities: [],
    contextRecords: [],
    lastEventSequence: {},
    files: [],
    git: null,
    error: null,
  });
}

function event(overrides: Partial<AgentEvent> = {}): AgentEvent {
  return {
    sequence: 1,
    runId: "run-a",
    threadId: "thread-a",
    kind: "status",
    status: "reasoning",
    createdAt: "2026-01-01T00:00:01.000Z",
    ...overrides,
  };
}

beforeEach(() => {
  seed();
  mocked.respondApproval.mockResolvedValue(undefined);
  mocked.finishGoal.mockResolvedValue(undefined);
  mocked.restoreContextSnapshot.mockResolvedValue(undefined);
  mocked.restoreRunChanges.mockResolvedValue(0);
  mocked.restoreRunFileChanges.mockResolvedValue(1);
  mocked.openProjectFileExternal.mockResolvedValue(undefined);
  mocked.deleteProvider.mockResolvedValue(undefined);
  mocked.deleteMcpServer.mockResolvedValue(undefined);
  mocked.listProjectFiles.mockResolvedValue([]);
  mocked.getGitSummary.mockResolvedValue({ branch: "main", changedFiles: [], diff: "" });
  mocked.saveThreadRunPreferences.mockImplementation(async (_threadId, preferences) => preferences);
});

describe("appStore", () => {
  it("creates a project directory and opens its first task", async () => {
    const project = {
      id: "project-new", name: "demo-project", path: "D:\\Projects\\demo-project", favorite: false,
      createdAt: "2026-01-01T00:00:00.000Z", updatedAt: "2026-01-01T00:00:00.000Z", gitBranch: null,
    };
    const thread = {
      id: "thread-new", projectId: project.id, title: "新任务", status: "idle" as const,
      createdAt: project.createdAt, updatedAt: project.updatedAt, unreadApproval: false, archived: false,
    };
    const detail = threadDetail();
    detail.thread = thread;
    mocked.createProjectDirectory.mockResolvedValue(project);
    mocked.createThread.mockResolvedValue(thread);
    mocked.getThread.mockResolvedValue(detail);

    const created = await useAppStore.getState().createProject("demo-project", "D:\\Projects");

    expect(created).toBe(true);
    expect(mocked.createProjectDirectory).toHaveBeenCalledWith("D:\\Projects", "demo-project");
    expect(mocked.createThread).toHaveBeenCalledWith(project.id, "新任务");
    expect(useAppStore.getState().activeProjectId).toBe(project.id);
    expect(useAppStore.getState().activeThreadId).toBe(thread.id);
    expect(useAppStore.getState().bootstrapData?.projects[0]).toEqual(project);
  });
  it("restores persisted per-thread run preferences", async () => {
    const detail = threadDetail();
    detail.runPreferences = {
      providerId: "provider-b",
      modelId: "model-b",
      thinkingLevel: "high",
      permissionMode: "full-access",
      runMode: "goal",
    };
    mocked.getThread.mockResolvedValue(detail);

    await useAppStore.getState().selectThread(detail.thread.id);

    expect(useAppStore.getState()).toMatchObject({
      providerId: "provider-b",
      modelId: "model-b",
      thinkingLevel: "high",
      permissionMode: "full-access",
      runMode: "goal",
    });
  });

  it("persists composer changes in order and forces Plan read-only", async () => {
    useAppStore.getState().setThinkingLevel("xhigh");
    useAppStore.getState().setRunMode("plan");

    await vi.waitFor(() => expect(mocked.saveThreadRunPreferences).toHaveBeenCalledTimes(2));
    expect(mocked.saveThreadRunPreferences).toHaveBeenLastCalledWith("thread-a", {
      providerId: "provider-a",
      modelId: "model-a",
      thinkingLevel: "xhigh",
      permissionMode: "read-only",
      runMode: "plan",
    });
  });

  it("相邻回合各自保存不可变的供应商、模型和思考配置", async () => {
    let index = 0;
    mocked.startAgentRun.mockImplementation(async (threadId, _prompt, config) => {
      index += 1;
      return { ...runRecord(`run-${index}`, "queued", { ...config }), threadId };
    });

    useAppStore.setState({ draft: "第一回合" });
    await useAppStore.getState().send();
    const firstConfig = structuredClone(mocked.startAgentRun.mock.calls[0][2]) as RunConfigSnapshot;

    const firstDetail = useAppStore.getState().threadDetail!;
    useAppStore.setState({
      threadDetail: { ...firstDetail, thread: { ...firstDetail.thread, status: "completed" } },
      draft: "第二回合",
      providerId: "provider-b",
      modelId: "model-b",
      thinkingLevel: "high",
    });
    await useAppStore.getState().send();

    expect(mocked.startAgentRun).toHaveBeenCalledTimes(2);
    expect(firstConfig).toMatchObject({ providerId: "provider-a", modelId: "model-a", thinkingLevel: "medium" });
    expect(mocked.startAgentRun.mock.calls[1][2]).toMatchObject({ providerId: "provider-b", modelId: "model-b", thinkingLevel: "high" });
    expect(firstConfig).toMatchObject({ providerId: "provider-a", modelId: "model-a", thinkingLevel: "medium" });
  });

  it("忽略重复和乱序事件，并用同一 ID 更新工具活动", () => {
    const detail = threadDetail("queued");
    detail.runs = [runRecord("run-a", "queued")];
    useAppStore.setState({ threadDetail: detail, activeRunId: "run-a" });

    useAppStore.getState().handleAgentEvent(event({
      sequence: 2,
      kind: "tool-started",
      status: "tool-running",
      toolActivity: { id: "tool-a", name: "read_file", status: "running", summary: "读取文件" },
    }));
    useAppStore.getState().handleAgentEvent(event({ sequence: 1, kind: "text-delta", content: "不应出现" }));
    useAppStore.getState().handleAgentEvent(event({
      sequence: 3,
      kind: "tool-completed",
      status: "reasoning",
      toolActivity: { id: "tool-a", name: "read_file", status: "completed", summary: "读取完成", output: "ok", durationMs: 12 },
    }));

    expect(useAppStore.getState().streamingContent).toBe("");
    expect(useAppStore.getState().lastEventSequence["run-a"]).toBe(3);
    expect(useAppStore.getState().toolActivities).toEqual([
      expect.objectContaining({ id: "tool-a", status: "completed", output: "ok" }),
    ]);
  });

  it("审批允许或拒绝后清除卡片并恢复工具运行状态", async () => {
    const detail = threadDetail("awaiting-approval");
    detail.runs = [runRecord("run-a", "awaiting-approval")];
    useAppStore.setState({ threadDetail: detail, activeRunId: "run-a" });
    useAppStore.getState().handleAgentEvent(event({
      kind: "approval-requested",
      status: "awaiting-approval",
      approval: { id: "approval-a", toolName: "shell", summary: "运行测试", arguments: { command: "pnpm test" }, createdAt: "2026-01-01T00:00:01.000Z" },
    }));

    expect(useAppStore.getState().pendingApproval?.id).toBe("approval-a");
    await useAppStore.getState().respondApproval(false);
    expect(mocked.respondApproval).toHaveBeenCalledWith("approval-a", false);
    expect(useAppStore.getState().pendingApproval).toBeNull();
    expect(useAppStore.getState().threadDetail?.thread.status).toBe("tool-running");
  });

  it("恢复上下文检查点后重新读取线程事实状态", async () => {
    const restored = threadDetail("completed");
    restored.contextSnapshots = [{
      id: "snapshot-a",
      threadId: "thread-a",
      runId: "run-a",
      summary: "压缩摘要",
      tokenCount: 420,
      startMessageId: "m1",
      endMessageId: "m2",
      sourceMessageIds: ["m1", "m2"],
      active: false,
      createdAt: "2026-01-01T00:00:01.000Z",
    }];
    mocked.getThread.mockResolvedValue(restored);

    await useAppStore.getState().restoreContextSnapshot("snapshot-a");
    expect(mocked.restoreContextSnapshot).toHaveBeenCalledWith("snapshot-a");
    expect(useAppStore.getState().threadDetail).toEqual(restored);
    expect(useAppStore.getState().contextRecords[0]).toMatchObject({ id: "snapshot-a", summary: "压缩摘要" });
  });

  it("保存和删除供应商时同步选择安全回退项", async () => {
    const created = provider("provider-c", "model-c");
    mocked.saveProvider.mockResolvedValue(created);
    await useAppStore.getState().saveProvider({
      kind: created.kind,
      name: created.name,
      baseUrl: created.baseUrl,
      defaultModel: created.defaultModel,
      enabled: true,
      timeoutSeconds: 120,
      extraHeaders: {},
      apiType: "chat-completions",
      models: [{ modelId: created.defaultModel, displayName: created.defaultModel, contextWindowTokens: 128000, source: "manual" }],
    });
    expect(useAppStore.getState().providerId).toBe("provider-c");
    expect(useAppStore.getState().bootstrapData?.providers[0].id).toBe("provider-c");

    await useAppStore.getState().deleteProvider("provider-c");
    expect(mocked.deleteProvider).toHaveBeenCalledWith("provider-c");
    expect(useAppStore.getState().providerId).toBe("provider-a");
    expect(useAppStore.getState().modelId).toBe("model-a");
  });

  it("保存和删除 MCP 服务时同步本地列表", async () => {
    const created = { ...mcpServer("mcp-b"), discoveredTools: ["search"] };
    mocked.saveMcpServer.mockResolvedValue(created);
    await useAppStore.getState().saveMcp(created);
    expect(useAppStore.getState().bootstrapData?.mcpServers[0]).toEqual(created);

    await useAppStore.getState().deleteMcp("mcp-b");
    expect(mocked.deleteMcpServer).toHaveBeenCalledWith("mcp-b");
    expect(useAppStore.getState().bootstrapData?.mcpServers.some((item) => item.id === "mcp-b")).toBe(false);
  });

  it("resumes a persisted Goal run instead of creating a replacement run", async () => {
    const goalRun = runRecord("goal-a", "cancelled", { ...runConfig("provider-a", "model-a"), runMode: "goal" });
    const resumed = { ...goalRun, status: "queued" as const, completedAt: null, error: null };
    const detail = threadDetail("cancelled");
    detail.runs = [goalRun];
    detail.goals = [{
      id: "goal-a",
      runId: "goal-a",
      threadId: detail.thread.id,
      status: "paused",
      turnCount: 2,
      startedAt: goalRun.startedAt,
      updatedAt: goalRun.startedAt,
      completedAt: null,
    }];
    useAppStore.setState({ threadDetail: detail, activeRunId: null, runMode: "agent" });
    mocked.resumeGoal.mockResolvedValue(resumed);

    await useAppStore.getState().resumeGoal("goal-a");

    expect(mocked.resumeGoal).toHaveBeenCalledWith("goal-a");
    expect(useAppStore.getState().activeRunId).toBe("goal-a");
    expect(useAppStore.getState().runMode).toBe("goal");
    expect(useAppStore.getState().threadDetail?.runs).toHaveLength(1);
    expect(useAppStore.getState().threadDetail?.goals[0].status).toBe("running");
  });

  it("Goal 每个逻辑回合完成后刷新持久化回合数和状态", async () => {
    const goalRun = runRecord("goal-live", "reasoning", { ...runConfig("provider-a", "model-a"), runMode: "goal" });
    const detail = threadDetail("reasoning");
    detail.runs = [goalRun];
    detail.goals = [{
      id: "goal-live",
      runId: "goal-live",
      threadId: detail.thread.id,
      status: "running",
      turnCount: 1,
      startedAt: goalRun.startedAt,
      updatedAt: goalRun.startedAt,
      completedAt: null,
    }];
    const refreshed = structuredClone(detail);
    refreshed.goals[0].turnCount = 2;
    refreshed.goals[0].updatedAt = "2026-01-01T00:00:02.000Z";
    mocked.getThread.mockResolvedValue(refreshed);
    useAppStore.setState({ threadDetail: detail, activeRunId: "goal-live" });

    useAppStore.getState().handleAgentEvent(event({
      runId: "goal-live",
      sequence: 7,
      kind: "message-completed",
      status: "reasoning",
    }));

    await vi.waitFor(() => expect(useAppStore.getState().threadDetail?.goals[0].turnCount).toBe(2));
    expect(mocked.getThread).toHaveBeenCalledWith("thread-a");
  });

  it("Goal 审批状态立即显示，并在响应后恢复运行", async () => {
    const goalRun = runRecord("goal-approval", "reasoning", { ...runConfig("provider-a", "model-a"), runMode: "goal" });
    const detail = threadDetail("reasoning");
    detail.runs = [goalRun];
    detail.goals = [{
      id: "goal-approval", runId: "goal-approval", threadId: detail.thread.id, status: "running", turnCount: 1,
      startedAt: goalRun.startedAt, updatedAt: goalRun.startedAt, completedAt: null,
    }];
    useAppStore.setState({ threadDetail: detail, activeRunId: "goal-approval" });

    useAppStore.getState().handleAgentEvent(event({
      runId: "goal-approval", sequence: 3, kind: "approval-requested", status: "awaiting-approval",
      approval: { id: "goal-approval-request", toolName: "shell", summary: "Run tests", arguments: {}, createdAt: "2026-01-01T00:00:01.000Z" },
    }));
    expect(useAppStore.getState().threadDetail?.goals[0].status).toBe("awaiting-approval");

    await useAppStore.getState().respondApproval(true);
    expect(useAppStore.getState().threadDetail?.goals[0].status).toBe("running");
  });

  it("Usage 事件替换运行统计而不改写运行配置", () => {
    const config = runConfig("provider-a", "model-a");
    const detail = threadDetail("streaming");
    detail.runs = [runRecord("run-a", "streaming", config, usage(true))];
    useAppStore.setState({ threadDetail: detail, activeRunId: "run-a" });
    const exactUsage = usage(false);
    useAppStore.getState().handleAgentEvent(event({ kind: "usage", status: "streaming", usage: exactUsage }));
    const updated = useAppStore.getState().threadDetail!.runs[0];
    expect(updated.usage).toEqual(exactUsage);
    expect(updated.config).toEqual(config);
  });

  it("思考增量实时追加到当前 Run", () => {
    const detail = threadDetail("reasoning");
    detail.runs = [runRecord("run-a", "reasoning")];
    useAppStore.setState({ threadDetail: detail, activeRunId: "run-a" });

    useAppStore.getState().handleAgentEvent(event({ sequence: 2, kind: "reasoning-delta", status: "reasoning", content: "先分析" }));
    useAppStore.getState().handleAgentEvent(event({ sequence: 3, kind: "reasoning-delta", status: "reasoning", content: "，再实现" }));

    expect(useAppStore.getState().threadDetail?.runs[0].reasoningContent).toBe("先分析，再实现");
  });

  it("逐文件撤销使用最近结束回合并刷新检查器", async () => {
    const detail = threadDetail("completed");
    detail.runs = [runRecord("run-finished", "completed")];
    useAppStore.setState({ threadDetail: detail, git: { branch: "main", changedFiles: [{ status: "M", path: "src/file.ts" }], diff: "" } });
    vi.spyOn(window, "confirm").mockReturnValue(true);

    await useAppStore.getState().restoreFileChange("src/file.ts");

    expect(mocked.restoreRunFileChanges).toHaveBeenCalledWith("run-finished", "src/file.ts");
    expect(mocked.listProjectFiles).toHaveBeenCalled();
    expect(mocked.getGitSummary).toHaveBeenCalled();
    expect(useAppStore.getState().restoringFilePath).toBeNull();
    expect(useAppStore.getState().restoreMessage).toContain("src/file.ts");
  });

  it("外部打开文件始终绑定当前任务", async () => {
    await useAppStore.getState().openFileExternal("README.md");
    expect(mocked.openProjectFileExternal).toHaveBeenCalledWith("thread-a", "README.md");
  });
});
