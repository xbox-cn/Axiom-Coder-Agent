import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Composer } from "./Composer";
import { useAppStore } from "../store/appStore";
import { bootstrap, runRecord, threadDetail } from "../test/fixtures";
import * as api from "../lib/api";
import type { RunStatus } from "../lib/types";

vi.mock("../lib/api", () => ({
  startAgentRun: vi.fn(),
  cancelAgentRun: vi.fn(),
  onAttachmentDrop: vi.fn(() => Promise.resolve(() => undefined)),
}));

const startAgentRun = vi.mocked(api.startAgentRun);
const cancelAgentRun = vi.mocked(api.cancelAgentRun);

function seed(status: RunStatus = "idle") {
  const detail = threadDetail(status);
  if (status !== "idle") detail.runs = [runRecord("run-a", status)];
  useAppStore.setState({
    bootstrapData: bootstrap(),
    activeProjectId: "project-a",
    activeThreadId: "thread-a",
    threadDetail: detail,
    draft: "",
    providerId: "provider-a",
    modelId: "model-a",
    thinkingLevel: "medium",
    permissionMode: "workspace-auto",
    activeRunId: status === "idle" ? null : "run-a",
    streamingContent: "",
    pendingApproval: null,
    toolActivities: [],
    contextRecords: [],
    lastEventSequence: {},
    error: null,
  });
}

beforeEach(() => {
  seed();
  startAgentRun.mockResolvedValue(runRecord("run-new", "queued"));
  cancelAgentRun.mockResolvedValue(undefined);
});

describe("Composer", () => {
  it.each<RunStatus>(["queued", "reasoning", "streaming", "tool-running", "awaiting-approval"])(
    "在 %s 状态锁定运行配置",
    (status) => {
      seed(status);
      render(<Composer />);
      expect(screen.getByLabelText("权限模式")).toBeDisabled();
      expect(screen.getByLabelText("供应商")).toBeDisabled();
      expect(screen.getByLabelText("模型")).toBeDisabled();
      expect(screen.getByLabelText("思考程度")).toBeDisabled();
      expect(screen.getByRole("button", { name: "停止" })).toBeEnabled();
    },
  );

  it.each<RunStatus>(["idle", "completed", "failed", "cancelled"])("在 %s 状态解锁运行配置", (status) => {
    seed(status);
    render(<Composer />);
    expect(screen.getByLabelText("权限模式")).toBeEnabled();
    expect(screen.getByLabelText("供应商")).toBeEnabled();
    expect(screen.getByLabelText("模型")).toBeEnabled();
    expect(screen.getByLabelText("思考程度")).toBeEnabled();
  });

  it("启用完全访问前显示风险确认，拒绝后不切换", () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<Composer />);
    fireEvent.click(screen.getByLabelText("权限模式"));
    fireEvent.click(screen.getByRole("option", { name: /完全访问/ }));
    expect(confirm).toHaveBeenCalledOnce();
    expect(useAppStore.getState().permissionMode).toBe("workspace-auto");
  });

  it("Ctrl+Enter 发送当前草稿和不可变运行配置", async () => {
    useAppStore.setState({ draft: "检查项目" });
    render(<Composer />);
    fireEvent.keyDown(screen.getByPlaceholderText(/描述任务/), { key: "Enter", ctrlKey: true });
    await waitFor(() => expect(startAgentRun).toHaveBeenCalledOnce());
    expect(startAgentRun).toHaveBeenCalledWith(
      "thread-a",
      "检查项目",
      expect.objectContaining({
        providerId: "provider-a",
        modelId: "model-a",
        thinkingLevel: "medium",
        permissionMode: "workspace-auto",
      }),
      [],
    );
    expect(useAppStore.getState().draft).toBe("");
  });

  it("运行中显示停止按钮并取消活动运行", async () => {
    seed("streaming");
    render(<Composer />);
    fireEvent.click(screen.getByRole("button", { name: "停止" }));
    await waitFor(() => expect(cancelAgentRun).toHaveBeenCalledWith("run-a"));
  });

  it("上下文环显示当前占用和模型上限", () => {
    const detail = threadDetail("completed");
    detail.runs = [runRecord("run-a", "completed")];
    useAppStore.setState({ threadDetail: detail });
    render(<Composer />);
    expect(screen.getByTitle("当前上下文 7,200 / 128,000")).toHaveTextContent("6");
  });

  it("状态切换后立即更新锁定状态", () => {
    const { rerender } = render(<Composer />);
    expect(screen.getByLabelText("供应商")).toBeEnabled();
    act(() => {
      useAppStore.setState({ threadDetail: threadDetail("queued") });
    });
    rerender(<Composer />);
    expect(screen.getByLabelText("供应商")).toBeDisabled();
  });
});
