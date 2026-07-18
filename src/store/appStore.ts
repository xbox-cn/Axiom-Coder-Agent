import { create } from "zustand";
import * as api from "../lib/api";
import type {
  AgentEvent,
  AttachmentSnapshot,
  AppBootstrap,
  ApprovalRequest,
  FileEntry,
  GitSummary,
  InspectorTab,
  McpServerConfig,
  Message,
  PermissionMode,
  ProviderProfile,
  ProviderProfileInput,
  RunConfigSnapshot,
  RunMode,
  SearchMatch,
  ShellResult,
  ThinkingLevel,
  ThreadDetail,
  ThreadRunPreferences,
  ToolActivity,
} from "../lib/types";

type ModalName = "providers" | "mcp" | "settings" | "search" | null;
interface ContextRecord { id: string; summary: string; createdAt: string }

interface AppStore {
  bootstrapData: AppBootstrap | null;
  activeProjectId: string | null;
  activeThreadId: string | null;
  threadDetail: ThreadDetail | null;
  inspectorTab: InspectorTab;
  inspectorOpen: boolean;
  sidebarOverlayOpen: boolean;
  modal: ModalName;
  loading: boolean;
  error: string | null;
  draft: string;
  providerId: string;
  modelId: string;
  thinkingLevel: ThinkingLevel;
  permissionMode: PermissionMode;
  runMode: RunMode;
  attachments: AttachmentSnapshot[];
  recentModelByProvider: Record<string, string>;
  activeRunId: string | null;
  streamingContent: string;
  pendingApproval: ApprovalRequest | null;
  toolActivities: ToolActivity[];
  contextRecords: ContextRecord[];
  lastEventSequence: Record<string, number>;
  files: FileEntry[];
  selectedFile: FileEntry | null;
  selectedFileContent: string;
  searchMatches: SearchMatch[];
  searchingFiles: boolean;
  git: GitSummary | null;
  shellHistory: ShellResult[];
  restoringRunId: string | null;
  restoringFilePath: string | null;
  restoreMessage: string | null;
  initialize: () => Promise<void>;
  selectThread: (id: string) => Promise<void>;
  addProject: () => Promise<void>;
  createThread: () => Promise<void>;
  archiveThread: (id: string, archived: boolean) => Promise<void>;
  deleteThread: (id: string) => Promise<void>;
  reuseMessage: (message: Message) => void;
  send: () => Promise<void>;
  cancel: () => Promise<void>;
  resumeGoal: (runId: string) => Promise<void>;
  finishGoal: (runId: string) => Promise<void>;
  handleAgentEvent: (event: AgentEvent) => void;
  respondApproval: (approved: boolean) => Promise<void>;
  respondQuestion: (answer: string) => Promise<void>;
  restoreLatestRun: () => Promise<void>;
  restoreFileChange: (path: string) => Promise<void>;
  openFileExternal: (path: string) => Promise<void>;
  restoreContextSnapshot: (snapshotId: string) => Promise<void>;
  refreshInspector: () => Promise<void>;
  selectFile: (file: FileEntry) => Promise<void>;
  searchFiles: (query: string) => Promise<void>;
  runShell: (command: string, approved?: boolean) => Promise<ShellResult>;
  saveProvider: (input: ProviderProfileInput) => Promise<ProviderProfile>;
  deleteProvider: (providerId: string) => Promise<void>;
  saveMcp: (input: McpServerConfig) => Promise<McpServerConfig>;
  deleteMcp: (serverId: string) => Promise<void>;
  setDraft: (draft: string) => void;
  setInspectorTab: (tab: InspectorTab) => void;
  setInspectorOpen: (open: boolean, persist?: boolean) => void;
  setSidebarOverlayOpen: (open: boolean) => void;
  setModal: (modal: ModalName) => void;
  setProviderId: (id: string) => void;
  setModelId: (id: string) => void;
  setThinkingLevel: (level: ThinkingLevel) => void;
  setPermissionMode: (mode: PermissionMode) => void;
  setRunMode: (mode: RunMode) => void;
  addAttachments: (paths?: string[]) => Promise<void>;
  removeAttachment: (id: string) => void;
  clearAttachments: () => void;
  clearError: () => void;
}

export const statusIsRunning = (status?: string) =>
  ["queued", "reasoning", "streaming", "tool-running", "awaiting-approval"].includes(status ?? "");

const terminalStatus = (status: string) => ["completed", "failed", "cancelled"].includes(status);

let preferenceSaveQueue: Promise<unknown> = Promise.resolve();

function persistActiveThreadPreferences() {
  const state = useAppStore.getState();
  const threadId = state.activeThreadId;
  if (!threadId) return;
  const preferences: ThreadRunPreferences = {
    providerId: state.providerId,
    modelId: state.modelId,
    thinkingLevel: state.thinkingLevel,
    permissionMode: state.runMode === "plan" ? "read-only" : state.permissionMode,
    runMode: state.runMode,
  };
  preferenceSaveQueue = preferenceSaveQueue
    .then(() => api.saveThreadRunPreferences(threadId, preferences))
    .catch((error) => {
      if (useAppStore.getState().activeThreadId === threadId) useAppStore.setState({ error: String(error) });
    });
}

function resolveThreadPreferences(detail: ThreadDetail, data: AppBootstrap, current: ThreadRunPreferences): ThreadRunPreferences {
  const activeRun = [...detail.runs].reverse().find((item) => statusIsRunning(item.status));
  const latestRun = detail.runs.at(-1);
  const source = activeRun?.config ?? detail.runPreferences ?? latestRun?.config ?? current;
  const globalProviderId = data.settings.defaultProviderId ?? data.providers[0]?.id ?? "";
  const provider = data.providers.find((item) => item.id === source.providerId)
    ?? data.providers.find((item) => item.id === globalProviderId)
    ?? data.providers[0];
  const requestedModel = provider?.models.find((model) => model.modelId === source.modelId)?.modelId;
  const globalModel = provider?.models.find((model) => model.modelId === data.settings.defaultModelId)?.modelId;
  const runMode = source.runMode ?? "agent";
  return {
    providerId: provider?.id ?? "",
    modelId: requestedModel ?? globalModel ?? provider?.models[0]?.modelId ?? "",
    thinkingLevel: source.thinkingLevel ?? data.settings.defaultThinkingLevel,
    permissionMode: runMode === "plan" ? "read-only" : source.permissionMode ?? data.settings.defaultPermission,
    runMode,
  };
}

const initialTransientState = {
  activeRunId: null,
  streamingContent: "",
  pendingApproval: null,
  toolActivities: [] as ToolActivity[],
  contextRecords: [] as ContextRecord[],
  lastEventSequence: {} as Record<string, number>,
  searchMatches: [] as SearchMatch[],
  searchingFiles: false,
  restoringRunId: null,
  restoringFilePath: null,
  restoreMessage: null,
};

export const useAppStore = create<AppStore>((set, get) => ({
  bootstrapData: null,
  activeProjectId: null,
  activeThreadId: null,
  threadDetail: null,
  inspectorTab: "changes",
  inspectorOpen: true,
  sidebarOverlayOpen: false,
  modal: null,
  loading: true,
  error: null,
  draft: "",
  providerId: "",
  modelId: "",
  thinkingLevel: "medium",
  permissionMode: "workspace-auto",
  runMode: "agent",
  attachments: [],
  recentModelByProvider: {},
  files: [],
  selectedFile: null,
  selectedFileContent: "",
  git: null,
  shellHistory: [],
  ...initialTransientState,

  initialize: async () => {
    try {
      const data = await api.getBootstrap();
      const projectId = data.projects[0]?.id ?? null;
      const threadId = data.threads.find((thread) => thread.projectId === projectId)?.id ?? data.threads[0]?.id ?? null;
      const providerId = data.settings.defaultProviderId ?? data.providers[0]?.id ?? "";
      const provider = data.providers.find((item) => item.id === providerId) ?? data.providers[0];
      set({
        bootstrapData: data,
        activeProjectId: projectId,
        activeThreadId: threadId,
        providerId: provider?.id ?? "",
        modelId: provider?.models.some((model) => model.modelId === data.settings.defaultModelId) ? data.settings.defaultModelId! : provider?.models[0]?.modelId ?? "",
        thinkingLevel: data.settings.defaultThinkingLevel,
        permissionMode: data.settings.defaultPermission,
        inspectorOpen: data.settings.inspectorOpen && window.innerWidth >= 1120,
        loading: false,
      });
      if (threadId) await get().selectThread(threadId);
    } catch (error) {
      set({ loading: false, error: String(error) });
    }
  },

  selectThread: async (id) => {
    set({
      activeThreadId: id,
      loading: true,
      error: null,
      streamingContent: "",
      activeRunId: null,
      pendingApproval: null,
      toolActivities: [],
      contextRecords: [],
      lastEventSequence: {},
      searchMatches: [],
      restoreMessage: null,
    });
    try {
      const detail = await api.getThread(id);
      const run = [...detail.runs].reverse().find((item) => statusIsRunning(item.status));
      const state = get();
      const data = state.bootstrapData;
      const preferences = data
        ? resolveThreadPreferences(detail, data, {
            providerId: state.providerId,
            modelId: state.modelId,
            thinkingLevel: state.thinkingLevel,
            permissionMode: state.permissionMode,
            runMode: state.runMode,
          })
        : null;
      set({
        threadDetail: detail,
        activeProjectId: detail.thread.projectId,
        contextRecords: detail.contextSnapshots.map((snapshot) => ({ id: snapshot.id, summary: snapshot.summary, createdAt: snapshot.createdAt })),
        loading: false,
        activeRunId: run?.id ?? null,
        ...(preferences ?? {}),
        attachments: [],
      });
      await get().refreshInspector();
    } catch (error) {
      set({ loading: false, error: String(error) });
    }
  },

  addProject: async () => {
    const path = await api.pickProjectDirectory();
    if (!path) return;
    try {
      const project = await api.addProject(path);
      const thread = await api.createThread(project.id, "新任务");
      set((state) => ({
        bootstrapData: state.bootstrapData
          ? {
              ...state.bootstrapData,
              projects: [project, ...state.bootstrapData.projects.filter((item) => item.id !== project.id)],
              threads: [thread, ...state.bootstrapData.threads],
            }
          : state.bootstrapData,
        activeProjectId: project.id,
      }));
      await get().selectThread(thread.id);
    } catch (error) {
      set({ error: String(error) });
    }
  },

  createThread: async () => {
    const projectId = get().activeProjectId;
    if (!projectId) {
      await get().addProject();
      return;
    }
    try {
      const thread = await api.createThread(projectId, "新任务");
      set((state) => ({
        bootstrapData: state.bootstrapData
          ? { ...state.bootstrapData, threads: [thread, ...state.bootstrapData.threads] }
          : state.bootstrapData,
      }));
      await get().selectThread(thread.id);
    } catch (error) {
      set({ error: String(error) });
    }
  },

  archiveThread: async (id, archived) => {
    try {
      await api.archiveThread(id, archived);
      const state = get();
      const current = state.bootstrapData?.threads.find((thread) => thread.id === id);
      set((value) => ({
        bootstrapData: value.bootstrapData ? {
          ...value.bootstrapData,
          threads: value.bootstrapData.threads.map((thread) => thread.id === id ? { ...thread, archived } : thread),
        } : null,
      }));
      if (state.activeThreadId === id) {
        const fallback = state.bootstrapData?.threads.find((thread) => thread.id !== id && thread.projectId === current?.projectId && !thread.archived);
        if (fallback) await get().selectThread(fallback.id);
        else set({ activeThreadId: null, threadDetail: null });
      }
    } catch (error) { set({ error: String(error) }); }
  },

  deleteThread: async (id) => {
    try {
      await api.deleteThread(id);
      const state = get();
      const removed = state.bootstrapData?.threads.find((thread) => thread.id === id);
      const fallback = state.bootstrapData?.threads.find((thread) => thread.id !== id && thread.projectId === removed?.projectId && !thread.archived);
      set((value) => ({
        bootstrapData: value.bootstrapData ? { ...value.bootstrapData, threads: value.bootstrapData.threads.filter((thread) => thread.id !== id) } : null,
        ...(value.activeThreadId === id ? { activeThreadId: null, threadDetail: null } : {}),
      }));
      if (state.activeThreadId === id && fallback) await get().selectThread(fallback.id);
    } catch (error) { set({ error: String(error) }); }
  },

  reuseMessage: (message) => {
    set({ draft: message.content, attachments: message.attachments.map((attachment) => ({ ...attachment })) });
    window.dispatchEvent(new Event("axiom-focus-composer"));
  },

  send: async () => {
    const { draft, attachments, activeThreadId, providerId, modelId, thinkingLevel, permissionMode, runMode, threadDetail, bootstrapData } = get();
    const provider = bootstrapData?.providers.find((item) => item.id === providerId);
    const modelIsValid = provider?.models.some((model) => model.modelId === modelId) ?? false;
    if (!activeThreadId || (!draft.trim() && attachments.length === 0) || !provider || !modelIsValid || statusIsRunning(threadDetail?.thread.status)) return;
    const createdAt = new Date().toISOString();
    const attachmentSnapshot = attachments.map((item) => ({ ...item }));
    const userMessage: Message = {
      id: crypto.randomUUID(),
      threadId: activeThreadId,
      role: "user",
      content: draft.trim(),
      createdAt,
      pinned: false,
      attachments: attachmentSnapshot,
    };
    const config: RunConfigSnapshot = { providerId, modelId, thinkingLevel, permissionMode, runMode, createdAt };
    set((state) => ({
      draft: "",
      attachments: [],
      streamingContent: "",
      pendingApproval: null,
      toolActivities: [],
      restoreMessage: null,
      threadDetail: state.threadDetail
        ? { ...state.threadDetail, thread: { ...state.threadDetail.thread, status: "queued" }, messages: [...state.threadDetail.messages, userMessage] }
        : null,
      error: null,
    }));
    try {
      const run = await api.startAgentRun(activeThreadId, userMessage.content, config, attachmentSnapshot);
      set((state) => ({
        activeRunId: run.id,
        recentModelByProvider: { ...state.recentModelByProvider, [providerId]: modelId },
        threadDetail: state.threadDetail ? { ...state.threadDetail, runs: [...state.threadDetail.runs, run] } : null,
      }));
    } catch (error) {
      set((state) => ({
        error: String(error),
        draft: userMessage.content,
        attachments: attachmentSnapshot,
        threadDetail: state.threadDetail ? { ...state.threadDetail, thread: { ...state.threadDetail.thread, status: "failed" } } : null,
      }));
    }
  },

  cancel: async () => {
    const runId = get().activeRunId;
    if (!runId) return;
    try {
      await api.cancelAgentRun(runId);
    } catch (error) {
      set({ error: String(error) });
    }
  },

  resumeGoal: async (runId) => {
    try {
      const run = await api.resumeGoal(runId);
      set((state) => ({
        activeRunId: run.id,
        runMode: "goal",
        streamingContent: "",
        pendingApproval: null,
        toolActivities: [],
        error: null,
        threadDetail: state.threadDetail ? {
          ...state.threadDetail,
          thread: { ...state.threadDetail.thread, status: "queued", unreadApproval: false },
          runs: state.threadDetail.runs.map((item) => item.id === run.id ? run : item),
          goals: state.threadDetail.goals.map((goal) => goal.runId === runId ? {
            ...goal,
            status: "running",
            updatedAt: new Date().toISOString(),
            completedAt: null,
          } : goal),
        } : null,
      }));
    } catch (error) {
      set({ error: String(error) });
    }
  },

  finishGoal: async (runId) => {
    try {
      await api.finishGoal(runId);
      set((state) => ({
        threadDetail: state.threadDetail ? {
          ...state.threadDetail,
          goals: state.threadDetail.goals.map((goal) => goal.runId === runId ? { ...goal, status: "completed", updatedAt: new Date().toISOString(), completedAt: new Date().toISOString() } : goal),
        } : null,
      }));
    } catch (error) {
      set({ error: String(error) });
    }
  },

  handleAgentEvent: (event) => {
    if (event.threadId !== get().activeThreadId) return;
    const previousSequence = get().lastEventSequence[event.runId] ?? 0;
    if (event.sequence <= previousSequence) return;
    const eventRun = get().threadDetail?.runs.find((run) => run.id === event.runId);
    const isGoalEvent = eventRun?.config.runMode === "goal";

    set((state) => {
      const detail = state.threadDetail;
      if (!detail) return { lastEventSequence: { ...state.lastEventSequence, [event.runId]: event.sequence } };

      const runs = detail.runs.map((run) =>
        run.id === event.runId
          ? {
              ...run,
              status: event.status,
              usage: event.usage ?? run.usage,
              reasoningContent: event.kind === "reasoning-delta"
                ? `${run.reasoningContent ?? ""}${event.content ?? ""}`
                : run.reasoningContent,
              error: event.error ?? run.error,
              completedAt: terminalStatus(event.status) ? event.createdAt : run.completedAt,
            }
          : run,
      );
      const messages = event.message
        ? [...detail.messages.filter((message) => message.id !== event.message?.id), event.message]
        : detail.messages;
      let toolActivities = state.toolActivities;
      if (event.toolActivity) {
        const exists = toolActivities.some((activity) => activity.id === event.toolActivity?.id);
        toolActivities = exists
          ? toolActivities.map((activity) => activity.id === event.toolActivity?.id ? event.toolActivity! : activity)
          : [...toolActivities, event.toolActivity];
      }
      const contextRecords = event.kind === "context-compressed" && event.content
        ? [...state.contextRecords, { id: `${event.runId}-${event.sequence}`, summary: event.content, createdAt: event.createdAt }]
        : state.contextRecords;
      const pendingApproval = event.kind === "approval-requested"
        ? event.approval ?? null
        : terminalStatus(event.status)
          ? null
          : state.pendingApproval;
      const bootstrapData = state.bootstrapData
        ? {
            ...state.bootstrapData,
            threads: state.bootstrapData.threads.map((thread) =>
              thread.id === event.threadId
                ? { ...thread, status: event.status, unreadApproval: event.status === "awaiting-approval", updatedAt: event.createdAt }
                : thread,
            ),
          }
        : null;

      return {
        bootstrapData,
        threadDetail: {
          ...detail,
          thread: { ...detail.thread, status: event.status, unreadApproval: event.status === "awaiting-approval" },
          runs,
          messages,
          goals: isGoalEvent && event.status === "awaiting-approval"
            ? detail.goals.map((goal) => goal.runId === event.runId ? { ...goal, status: "awaiting-approval", updatedAt: event.createdAt } : goal)
            : detail.goals,
        },
        streamingContent: event.kind === "text-delta"
          ? state.streamingContent + (event.content ?? "")
          : event.kind === "message-completed"
            ? ""
            : state.streamingContent,
        activeRunId: statusIsRunning(event.status) ? event.runId : null,
        pendingApproval,
        toolActivities,
        contextRecords,
        lastEventSequence: { ...state.lastEventSequence, [event.runId]: event.sequence },
        error: event.error ?? state.error,
      };
    });

    if (event.status === "completed") void get().refreshInspector();
    if (terminalStatus(event.status) || (isGoalEvent && event.kind === "message-completed")) {
      void api.getThread(event.threadId).then((detail) => {
        if (get().activeThreadId === event.threadId) set({ threadDetail: detail });
      }).catch((error) => set({ error: String(error) }));
    }
  },

  respondApproval: async (approved) => {
    const approval = get().pendingApproval;
    if (!approval) return;
    try {
      await api.respondApproval(approval.id, approved);
      set((state) => ({
        pendingApproval: null,
        threadDetail: state.threadDetail
          ? {
              ...state.threadDetail,
              thread: { ...state.threadDetail.thread, status: "tool-running", unreadApproval: false },
              goals: state.threadDetail.goals.map((goal) => goal.runId === state.activeRunId
                ? { ...goal, status: "running", updatedAt: new Date().toISOString() }
                : goal),
            }
          : null,
      }));
    } catch (error) {
      set({ error: String(error) });
    }
  },

  respondQuestion: async (answer) => {
    const question = get().pendingApproval;
    if (!question || question.toolName !== "ask_user") return;
    try {
      await api.respondUserQuestion(question.id, answer);
      set((state) => ({
        pendingApproval: null,
        threadDetail: state.threadDetail ? {
          ...state.threadDetail,
          thread: { ...state.threadDetail.thread, status: "tool-running", unreadApproval: false },
        } : null,
      }));
    } catch (error) { set({ error: String(error) }); }
  },

  restoreLatestRun: async () => {
    const run = [...(get().threadDetail?.runs ?? [])].reverse().find((item) => terminalStatus(item.status));
    if (!run || get().restoringRunId) return;
    if (!window.confirm("撤销此回合由内置文件工具产生的全部变更？Shell 的外部副作用不会被撤销。")) return;
    set({ restoringRunId: run.id, restoreMessage: null });
    try {
      const restored = await api.restoreRunChanges(run.id);
      await get().refreshInspector();
      set({ restoreMessage: restored > 0 ? `已恢复 ${restored} 个文件检查点` : "此回合没有可恢复的内置文件变更" });
    } catch (error) {
      set({ error: String(error) });
    } finally {
      set({ restoringRunId: null });
    }
  },

  restoreFileChange: async (path) => {
    const state = get();
    const run = [...(state.threadDetail?.runs ?? [])].reverse().find((item) => terminalStatus(item.status));
    if (!run || state.restoringRunId || state.restoringFilePath) return;
    if (!window.confirm(`撤销“${path}”在最近已结束回合中的内置文件变更？Shell 的外部副作用不会被撤销。`)) return;
    set({ restoringFilePath: path, restoreMessage: null });
    try {
      const restored = await api.restoreRunFileChanges(run.id, path);
      await get().refreshInspector();
      set({ restoreMessage: restored > 0 ? `已恢复 ${path} 的 ${restored} 个检查点` : `没有找到 ${path} 的可恢复检查点` });
    } catch (error) {
      set({ error: String(error) });
    } finally {
      set({ restoringFilePath: null });
    }
  },

  openFileExternal: async (path) => {
    const threadId = get().activeThreadId;
    if (!threadId) return;
    try {
      await api.openProjectFileExternal(threadId, path);
    } catch (error) {
      set({ error: String(error) });
    }
  },

  restoreContextSnapshot: async (snapshotId) => {
    const threadId = get().activeThreadId;
    if (!threadId) return;
    try {
      await api.restoreContextSnapshot(snapshotId);
      const detail = await api.getThread(threadId);
      set({
        threadDetail: detail,
        contextRecords: detail.contextSnapshots.map((snapshot) => ({ id: snapshot.id, summary: snapshot.summary, createdAt: snapshot.createdAt })),
      });
    } catch (error) {
      set({ error: String(error) });
    }
  },

  refreshInspector: async () => {
    const threadId = get().activeThreadId;
    if (!threadId) return;
    const [files, git] = await Promise.all([
      api.listProjectFiles(threadId).catch(() => []),
      api.getGitSummary(threadId).catch(() => null),
    ]);
    set({ files, git });
  },

  selectFile: async (file) => {
    const threadId = get().activeThreadId;
    if (!threadId) return;
    if (file.isDirectory) {
      const files = await api.listProjectFiles(threadId, file.path).catch(() => []);
      set({ files, selectedFile: null, selectedFileContent: "", searchMatches: [] });
      return;
    }
    set({ selectedFile: file, selectedFileContent: "正在读取…" });
    try {
      set({ selectedFileContent: await api.readProjectFile(threadId, file.path) });
    } catch (error) {
      set({ selectedFileContent: String(error) });
    }
  },

  searchFiles: async (query) => {
    const threadId = get().activeThreadId;
    if (!threadId || query.trim().length < 2) {
      set({ searchMatches: [], searchingFiles: false });
      return;
    }
    set({ searchingFiles: true });
    try {
      const matches = await api.searchProjectFiles(threadId, query.trim());
      if (get().activeThreadId === threadId) set({ searchMatches: matches });
    } catch (error) {
      set({ error: String(error), searchMatches: [] });
    } finally {
      set({ searchingFiles: false });
    }
  },

  runShell: async (command, approved = false) => {
    const threadId = get().activeThreadId;
    if (!threadId) throw new Error("请先选择项目线程");
    const result = await api.executeShell(threadId, command, get().permissionMode, approved);
    set((state) => ({ shellHistory: [...state.shellHistory, result] }));
    return result;
  },

  saveProvider: async (input) => {
    const provider = await api.saveProvider(input);
    set((state) => ({
      bootstrapData: state.bootstrapData
        ? { ...state.bootstrapData, providers: [provider, ...state.bootstrapData.providers.filter((item) => item.id !== provider.id)] }
        : null,
      providerId: provider.id,
      modelId: provider.models[0]?.modelId ?? "",
    }));
    persistActiveThreadPreferences();
    return provider;
  },

  deleteProvider: async (providerId) => {
    await api.deleteProvider(providerId);
    set((state) => {
      const providers = state.bootstrapData?.providers.filter((item) => item.id !== providerId) ?? [];
      const fallback = providers[0];
      return {
        bootstrapData: state.bootstrapData ? { ...state.bootstrapData, providers } : null,
        providerId: state.providerId === providerId ? fallback?.id ?? "" : state.providerId,
        modelId: state.providerId === providerId ? fallback?.models[0]?.modelId ?? "" : state.modelId,
      };
    });
    persistActiveThreadPreferences();
  },

  saveMcp: async (input) => {
    const server = await api.saveMcpServer(input);
    set((state) => ({
      bootstrapData: state.bootstrapData
        ? { ...state.bootstrapData, mcpServers: [server, ...state.bootstrapData.mcpServers.filter((item) => item.id !== server.id)] }
        : null,
    }));
    return server;
  },

  deleteMcp: async (serverId) => {
    await api.deleteMcpServer(serverId);
    set((state) => ({
      bootstrapData: state.bootstrapData
        ? { ...state.bootstrapData, mcpServers: state.bootstrapData.mcpServers.filter((item) => item.id !== serverId) }
        : null,
    }));
  },

  setDraft: (draft) => set({ draft }),
  setInspectorTab: (inspectorTab) => set({ inspectorTab, inspectorOpen: true }),
  setInspectorOpen: (inspectorOpen, persist = true) => {
    const data = get().bootstrapData;
    set({ inspectorOpen });
    if (persist && data) {
      const settings = { ...data.settings, inspectorOpen };
      set({ bootstrapData: { ...data, settings } });
      void api.saveSettings(settings).catch((error) => set({ error: String(error) }));
    }
  },
  setSidebarOverlayOpen: (sidebarOverlayOpen) => set({ sidebarOverlayOpen }),
  setModal: (modal) => set({ modal }),
  setProviderId: (id) => {
    const state = get();
    const provider = state.bootstrapData?.providers.find((item) => item.id === id);
    const recent = state.recentModelByProvider[id];
    const modelId = provider?.models.some((model) => model.modelId === recent) ? recent : provider?.models[0]?.modelId ?? "";
    set({ providerId: id, modelId });
    persistActiveThreadPreferences();
  },
  setModelId: (modelId) => {
    set((state) => ({ modelId, recentModelByProvider: state.providerId ? { ...state.recentModelByProvider, [state.providerId]: modelId } : state.recentModelByProvider }));
    persistActiveThreadPreferences();
  },
  setThinkingLevel: (thinkingLevel) => { set({ thinkingLevel }); persistActiveThreadPreferences(); },
  setPermissionMode: (permissionMode) => { set({ permissionMode }); persistActiveThreadPreferences(); },
  setRunMode: (runMode) => {
    set((state) => ({ runMode, permissionMode: runMode === "plan" ? "read-only" : state.permissionMode }));
    persistActiveThreadPreferences();
  },
  addAttachments: async (paths) => {
    try {
      const selected = paths ?? await api.pickAttachmentFiles();
      if (!selected.length) return;
      const snapshots = await api.prepareAttachments(selected);
      set((state) => {
        const merged = [...state.attachments];
        for (const attachment of snapshots) if (!merged.some((item) => item.sha256 === attachment.sha256 && item.name === attachment.name)) merged.push(attachment);
        if (merged.length > 10) throw new Error("附件最多 10 个");
        return { attachments: merged };
      });
    } catch (error) { set({ error: String(error) }); }
  },
  removeAttachment: (id) => set((state) => ({ attachments: state.attachments.filter((item) => item.id !== id) })),
  clearAttachments: () => set({ attachments: [] }),
  clearError: () => set({ error: null }),
}));
