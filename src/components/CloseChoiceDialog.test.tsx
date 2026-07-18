import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as api from "../lib/api";
import { CloseChoiceDialog } from "./CloseChoiceDialog";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("CloseChoiceDialog", () => {
  it("offers tray, exit, and cancel choices for native close requests", async () => {
    const hide = vi.spyOn(api, "hideMainWindow").mockResolvedValue(undefined);
    render(<CloseChoiceDialog/>);
    window.dispatchEvent(new Event("axiom-close-requested"));
    const dialog = await screen.findByRole("dialog", { name: "关闭 Axiom？" });
    expect(dialog).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /隐藏到托盘/ })).toHaveFocus();
    fireEvent.click(screen.getByRole("button", { name: /隐藏到托盘/ }));
    await waitFor(() => expect(hide).toHaveBeenCalledOnce());
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("cancels with Escape without quitting", async () => {
    const quit = vi.spyOn(api, "quitApp").mockResolvedValue(undefined);
    render(<CloseChoiceDialog/>);
    window.dispatchEvent(new Event("axiom-close-requested"));
    await screen.findByRole("dialog");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(quit).not.toHaveBeenCalled();
  });
});
