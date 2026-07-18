import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../lib/api";
import { useAppStore } from "../store/appStore";
import { bootstrap } from "../test/fixtures";
import { Sidebar } from "./Sidebar";

vi.mock("../lib/api", () => ({ saveSettings: vi.fn(), archiveThread: vi.fn(), deleteThread: vi.fn() }));
const saveSettings = vi.mocked(api.saveSettings);
const archiveThread = vi.mocked(api.archiveThread);
const deleteThread = vi.mocked(api.deleteThread);

beforeEach(() => {
  const data = bootstrap();
  data.settings.sidebarWidth = 300;
  useAppStore.setState({
    bootstrapData: data,
    activeProjectId: "project-a",
    activeThreadId: "thread-a",
    sidebarOverlayOpen: false,
  });
  saveSettings.mockResolvedValue(data.settings);
  archiveThread.mockResolvedValue(undefined);
  deleteThread.mockResolvedValue(undefined);
});

describe("Sidebar", () => {
  it("恢复持久化宽度，并在拖动结束后保存限制范围内的宽度", async () => {
    const { container } = render(<Sidebar />);
    const sidebar = container.querySelector(".sidebar") as HTMLElement;
    expect(sidebar.style.width).toBe("300px");

    fireEvent.pointerDown(screen.getByRole("separator", { name: "调整侧栏宽度" }), { clientX: 300 });
    fireEvent.pointerMove(window, { clientX: 360 });
    fireEvent.pointerUp(window, { clientX: 360 });

    await waitFor(() => expect(saveSettings).toHaveBeenCalledWith(expect.objectContaining({ sidebarWidth: 320 })));
    expect(sidebar.style.width).toBe("320px");
  });

  it("可以归档、查看并恢复任务", async () => {
    render(<Sidebar />);
    fireEvent.click(screen.getByRole("button", { name: "归档任务" }));
    await waitFor(() => expect(archiveThread).toHaveBeenCalledWith("thread-a", true));
    expect(useAppStore.getState().bootstrapData?.threads[0].archived).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: /查看已归档任务/ }));
    expect(screen.getByText("测试任务")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "恢复任务" }));
    await waitFor(() => expect(archiveThread).toHaveBeenLastCalledWith("thread-a", false));
    expect(useAppStore.getState().bootstrapData?.threads[0].archived).toBe(false);
  });

  it("确认后永久删除任务", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<Sidebar />);
    fireEvent.click(screen.getByRole("button", { name: "删除任务" }));
    await waitFor(() => expect(deleteThread).toHaveBeenCalledWith("thread-a"));
    expect(useAppStore.getState().bootstrapData?.threads).toHaveLength(0);
  });
});
