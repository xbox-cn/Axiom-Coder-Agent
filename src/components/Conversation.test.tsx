import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { Conversation } from "./Conversation";
import { useAppStore } from "../store/appStore";
import { bootstrap, runConfig, runRecord, threadDetail, usage } from "../test/fixtures";

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

beforeEach(seedConversation);

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
