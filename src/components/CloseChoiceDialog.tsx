import { LogOut, Minus, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { hideMainWindow, onCloseRequested, quitApp } from "../lib/api";

export function CloseChoiceDialog() {
  const [open, setOpen] = useState(false);
  const hideButton = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onCloseRequested(() => setOpen(true)).then((fn) => { unlisten = fn; });
    return () => unlisten?.();
  }, []);

  useEffect(() => {
    if (!open) return;
    hideButton.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopImmediatePropagation();
      setOpen(false);
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [open]);

  if (!open) return null;
  return createPortal(
    <div className="close-choice-backdrop" role="presentation">
      <section className="close-choice-dialog" role="dialog" aria-modal="true" aria-labelledby="close-choice-title">
        <div className="close-choice-heading">
          <div className="close-choice-icon"><Minus size={18}/></div>
          <div><h2 id="close-choice-title">关闭 Axiom？</h2><p>你可以把窗口隐藏到系统托盘，正在进行的任务不会被中断。</p></div>
        </div>
        <div className="close-choice-actions">
          <button ref={hideButton} className="primary" onClick={() => void hideMainWindow().then(() => setOpen(false))}><Minus size={15}/>隐藏到托盘</button>
          <button className="danger-ghost" onClick={() => void quitApp()}><LogOut size={15}/>退出 Axiom</button>
          <button className="secondary" onClick={() => setOpen(false)}><X size={15}/>取消</button>
        </div>
      </section>
    </div>,
    document.body,
  );
}
