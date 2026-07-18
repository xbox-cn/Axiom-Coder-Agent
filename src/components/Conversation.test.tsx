import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Conversation } from "./Conversation";
import { useAppStore } from "../store/appStore";
import { bootstrap, runConfig, runRecord, threadDetail, usage } from "../test/fixtures";
import * as api from "../lib/api";

function seedConversation() {
  const detail = threadDetail("awaiting-approval");
  const exactRun = runRecord("run-exact", "completed", runConfig("openai", "gpt-5.4"), usage(false));
  const estimatedRun = runRecord("run-estimated", "completed", runConfig("ollama", "qwen3-coder"), usage(true));
  detail.runs = [exactRun, estimatedRun];
  detail.messages = [
    { id: "m-user", threadId: "thread-a", role: "user", content: "检查项目", createdAt: "2026-01-01T00:00:00.000Z", pinned: false, attachments: [] },
    { id: "m-exact", threadId: "thread-a", role: "assistant", content: "已完成精确统计。", createdAt: "2026-01-01T00:00:01.000Z", runId: "run-exact", pinned: false, attachments: [] },
    { id: "m-estimated", threadId: "thread-a", role: "assistant", content: "本地估算统计。", createdAt: "2026-01-01T00:00:02.000Z", runId: "run-estimated", pinned: false, attachments: [] },
  ];
  useAppStore.setState({
    bootstrapData: bootstrap(),
    activeProjectId: "project-a",
    activeThreadId: "thread-a",
    threadDetail: detail,
    providerId: "provider-a",
    modelId: "model-a",
    thinkingLevel: "medium",
    permissionMode: "workspace-auto",
    draft: "",
    inspectorOpen: true,
    inspectorTab: "changes",
    streamingContent: "",
    pendingApproval: {
      id: "approval-a",
      toolName: "shell",
      summary: "运行 pnpm test",
      arguments: { command: "pnpm test" },
      createdAt: "2026-01-01T00:00:03.000Z",
    },
    toolActivities: [{ id: "tool-a", name: "read_file", status: "completed", summary: "已读取文件", output: "42 lines", durationMs: 418 }],
    contextRecords: [{ id: "context-a", summary: "保留最近关键工具结果。", createdAt: "2026-01-01T00:00:03.000Z" }],
    error: null,
  });
}

beforeEach(() => {
  vi.restoreAllMocks();
  seedConversation();
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: vi.fn().mockResolvedValue(undefined) },
  });
});

describe("Conversation", () => {
  it("明确区分准确和估算 Usage，并展开完整指标", () => {
    render(<Conversation />);
    expect(screen.getByText("准确")).toBeInTheDocument();
    expect(screen.getByText("估算")).toBeInTheDocument();
    fireEvent.click(screen.getByText(/openai \/ gpt-5.4/).closest("button")!);
    expect(screen.getByText("首 Token")).toBeInTheDocument();
    expect(screen.getByText("$0.0042")).toBeInTheDocument();
    expect(screen.getByText("7,200 / 12.8万")).toBeInTheDocument();
  });

  it("呈现审批、工具活动和上下文压缩记录", () => {
    render(<Conversation />);
    expect(screen.getByRole("alert")).toHaveTextContent("需要批准 shell");
    expect(screen.getByRole("alert")).toHaveTextContent("pnpm test");
    expect(screen.getByLabelText("工具活动")).toHaveTextContent("read_file");
    expect(screen.getByLabelText("工具活动")).toHaveTextContent("已读取文件");
    expect(screen.getByText("上下文已透明压缩")).toBeInTheDocument();
  });

  it("用户和助手消息都可以复制", async () => {
    render(<Conversation />);
    const copyButtons = screen.getAllByRole("button", { name: "复制消息" });
    fireEvent.click(copyButtons[0]);
    fireEvent.click(copyButtons[1]);
    await waitFor(() => expect(navigator.clipboard.writeText).toHaveBeenCalledTimes(2));
    expect(navigator.clipboard.writeText).toHaveBeenNthCalledWith(1, "检查项目");
    expect(navigator.clipboard.writeText).toHaveBeenNthCalledWith(2, "已完成精确统计。");
  });

  it("编辑并重新发送会恢复正文和附件快照", () => {
    const detail = useAppStore.getState().threadDetail!;
    detail.messages[0].attachments = [{
      id: "attachment-a",
      name: "notes.txt",
      mimeType: "text/plain",
      size: 12,
      sha256: "hash",
      snapshotPath: "D:/AxiomData/notes.txt",
      kind: "text",
    }];
    useAppStore.setState({ threadDetail: { ...detail } });
    render(<Conversation />);
    fireEvent.click(screen.getByRole("button", { name: "编辑并重新发送" }));
    expect(useAppStore.getState().draft).toBe("检查项目");
    expect(useAppStore.getState().attachments).toEqual([expect.objectContaining({ id: "attachment-a", name: "notes.txt" })]);
  });

  it("Plan 提问以选择卡提交选项", async () => {
    const respond = vi.spyOn(api, "respondUserQuestion").mockResolvedValue(undefined);
    useAppStore.setState({
      pendingApproval: {
        id: "question-a",
        toolName: "ask_user",
        summary: "选择实现方式",
        arguments: { options: [
          { id: "safe", label: "稳妥方案", description: "优先兼容性" },
          { id: "fast", label: "快速方案", description: "减少改动" },
        ] },
        createdAt: "2026-01-01T00:00:03.000Z",
      },
    });
    render(<Conversation />);
    expect(screen.getByRole("dialog", { name: "Plan 需要你的选择" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /稳妥方案/ }));
    await waitFor(() => expect(respond).toHaveBeenCalledWith("question-a", "safe"));
  });

  it("Plan 提问支持输入自定义答案", async () => {
    const respond = vi.spyOn(api, "respondUserQuestion").mockResolvedValue(undefined);
    useAppStore.setState({
      pendingApproval: {
        id: "question-custom",
        toolName: "ask_user",
        summary: "补充要求",
        arguments: { options: [] },
        createdAt: "2026-01-01T00:00:03.000Z",
      },
    });
    render(<Conversation />);
    fireEvent.change(screen.getByPlaceholderText("或输入其他答案"), { target: { value: "保留兼容层" } });
    fireEvent.click(screen.getByRole("button", { name: "提交" }));
    await waitFor(() => expect(respond).toHaveBeenCalledWith("question-custom", "保留兼容层"));
  });

  it("低风险读取工具合并为一个折叠组，失败和 Shell 保持独立", () => {
    useAppStore.setState({
      toolActivities: [
        { id: "read-1", name: "read_file", status: "completed", summary: "读取 A", durationMs: 10 },
        { id: "list-1", name: "list_files", status: "completed", summary: "列出 src", durationMs: 12 },
        { id: "search-1", name: "search_files", status: "failed", summary: "搜索失败", output: "error" },
        { id: "shell-1", name: "shell", status: "completed", summary: "pnpm test", output: "ok" },
      ],
    });
    const { container } = render(<Conversation />);
    expect(container.querySelectorAll(".tool-activity-group")).toHaveLength(1);
    expect(container.querySelector(".tool-activity-group")).toHaveTextContent("已完成 2 项代码检查");
    expect(container.querySelectorAll(".tool-activity")).toHaveLength(2);
    expect(screen.getByLabelText("工具活动")).toHaveTextContent("search_files");
    expect(screen.getByLabelText("工具活动")).toHaveTextContent("shell");
  });

  it("推理模型的思考过程以可折叠区块显示", () => {
    const detail = useAppStore.getState().threadDetail!;
    detail.runs[0] = { ...detail.runs[0], reasoningContent: "先阅读代码，再评估修复。", usage: { ...detail.runs[0].usage, reasoningTokens: 24 } };
    useAppStore.setState({ threadDetail: { ...detail } });
    const { container } = render(<Conversation />);
    const block = container.querySelector(".reasoning-block");
    expect(block).toHaveTextContent("思考过程");
    expect(block).toHaveTextContent("先阅读代码，再评估修复。");
    expect(block).toHaveTextContent("24 reasoning tokens");
  });

  it("10,000 条消息仅渲染可视窗口", () => {
    const detail = threadDetail("idle");
    detail.messages = Array.from({ length: 10_000 }, (_, index) => ({
      id: `message-${index}`,
      threadId: "thread-a",
      role: index % 2 === 0 ? "user" as const : "assistant" as const,
      content: `虚拟消息 ${index}`,
      createdAt: "2026-01-01T00:00:00.000Z",
      pinned: false,
      attachments: [],
    }));
    useAppStore.setState({ threadDetail: detail, pendingApproval: null, toolActivities: [], contextRecords: [] });
    const { container } = render(<Conversation />);
    expect(container.querySelectorAll(".virtual-feed-row").length).toBeLessThan(100);
    expect(container.querySelectorAll(".message").length).toBeLessThan(100);
  });

  it("头部按钮可折叠检查器，变更摘要可重新打开 Changes", () => {
    render(<Conversation />);
    fireEvent.click(screen.getByRole("button", { name: "关闭检查器" }));
    expect(useAppStore.getState().inspectorOpen).toBe(false);

    fireEvent.click(screen.getAllByRole("button", { name: /查看本回合产生的代码变更/ })[0]);
    expect(useAppStore.getState().inspectorOpen).toBe(true);
    expect(useAppStore.getState().inspectorTab).toBe("changes");
  });
});
