import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../lib/api";
import { useAppStore } from "../store/appStore";
import { bootstrap } from "../test/fixtures";
import { Sidebar } from "./Sidebar";

vi.mock("../lib/api", () => ({ saveSettings: vi.fn() }));
const saveSettings = vi.mocked(api.saveSettings);

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
});
