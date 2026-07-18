import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { Dropdown } from "./Dropdown";

function Example({ onChange = () => undefined }: { onChange?: (value: string) => void }) {
  const [value, setValue] = useState("agent");
  return <Dropdown
    ariaLabel="运行模式"
    value={value}
    options={[
      { value: "agent", label: "Agent" },
      { value: "plan", label: "Plan" },
      { value: "goal", label: "Goal" },
    ]}
    onChange={(next) => { setValue(next); onChange(next); }}
  />;
}

describe("Dropdown", () => {
  it("打开后把焦点交给 listbox，并支持方向键选择", () => {
    const onChange = vi.fn();
    render(<Example onChange={onChange}/>);
    fireEvent.click(screen.getByLabelText("运行模式"));
    const listbox = screen.getByRole("listbox", { name: "运行模式" });
    listbox.focus();
    fireEvent.keyDown(listbox, { key: "ArrowDown" });
    fireEvent.keyDown(listbox, { key: "Enter" });
    expect(onChange).toHaveBeenCalledWith("plan");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("无有效值且首项禁用时聚焦并选择第一个可用项", () => {
    const onChange = vi.fn();
    render(<Dropdown
      ariaLabel="模型"
      value=""
      options={[
        { value: "unavailable", label: "不可用", disabled: true },
        { value: "model-a", label: "Model A" },
        { value: "model-b", label: "Model B" },
      ]}
      onChange={onChange}
    />);
    fireEvent.click(screen.getByLabelText("模型"));
    const listbox = screen.getByRole("listbox", { name: "模型" });
    const firstEnabled = screen.getByRole("option", { name: "Model A" });
    expect(listbox).toHaveAttribute("aria-activedescendant", firstEnabled.id);
    fireEvent.keyDown(listbox, { key: "Enter" });
    expect(onChange).toHaveBeenCalledWith("model-a");
  });

  it("Escape 关闭并恢复触发按钮焦点", () => {
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => { callback(0); return 1; });
    render(<Example/>);
    const trigger = screen.getByLabelText("运行模式");
    fireEvent.click(trigger);
    const listbox = screen.getByRole("listbox", { name: "运行模式" });
    listbox.focus();
    fireEvent.keyDown(listbox, { key: "Escape" });
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });
});
