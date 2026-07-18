import { useCallback, useEffect, useRef, useState } from "react";
import { Download, RefreshCw, RotateCcw, X } from "lucide-react";
import { check, type Update, type DownloadEvent } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

type UpdateState =
  | { kind: "hidden" }
  | { kind: "checking" }
  | { kind: "current" }
  | { kind: "available"; update: Update }
  | { kind: "downloading"; update: Update; progress: number | null }
  | { kind: "ready"; update: Update }
  | { kind: "error"; message: string };

const isTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export function UpdateBanner() {
  const [state, setState] = useState<UpdateState>({ kind: "hidden" });
  const active = useRef<Update | null>(null);
  const currentTimer = useRef<number | null>(null);

  const checkNow = useCallback(async (manual = false) => {
    if (!isTauri()) return;
    if (currentTimer.current != null) window.clearTimeout(currentTimer.current);
    setState({ kind: "checking" });
    try {
      const update = await check({ timeout: 20_000 });
      if (!update) {
        if (manual) {
          setState({ kind: "current" });
          currentTimer.current = window.setTimeout(() => setState({ kind: "hidden" }), 3200);
        } else {
          setState({ kind: "hidden" });
        }
        return;
      }
      active.current = update;
      setState({ kind: "available", update });
    } catch (error) {
      if (manual) setState({ kind: "error", message: String(error) });
      else setState({ kind: "hidden" });
    }
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    const timer = window.setTimeout(() => void checkNow(false), 1600);
    const manual = () => void checkNow(true);
    window.addEventListener("axiom-check-update", manual);
    return () => {
      window.clearTimeout(timer);
      if (currentTimer.current != null) window.clearTimeout(currentTimer.current);
      window.removeEventListener("axiom-check-update", manual);
      void active.current?.close();
    };
  }, [checkNow]);

  const install = async (update: Update) => {
    let received = 0;
    let total: number | undefined;
    setState({ kind: "downloading", update, progress: null });
    try {
      await update.downloadAndInstall((event: DownloadEvent) => {
        if (event.event === "Started") total = event.data.contentLength;
        if (event.event === "Progress") received += event.data.chunkLength;
        const progress = total && total > 0 ? Math.min(100, Math.round(received / total * 100)) : null;
        setState({ kind: "downloading", update, progress });
      });
      setState({ kind: "ready", update });
    } catch (error) {
      setState({ kind: "error", message: String(error) });
    }
  };

  if (state.kind === "hidden") return null;
  const dismiss = () => setState({ kind: "hidden" });
  return <aside className="update-banner" role="status" aria-live="polite">
    <div className="update-banner-icon">{state.kind === "available" ? <Download size={16}/> : state.kind === "ready" ? <RotateCcw size={16}/> : <RefreshCw className={state.kind === "checking" || state.kind === "downloading" ? "spin" : ""} size={16}/>}</div>
    <div className="update-banner-copy">
      {state.kind === "checking" && <><strong>正在检查更新</strong><span>正在连接 GitHub Releases…</span></>}
      {state.kind === "current" && <><strong>已是最新版本</strong><span>Axiom 1.0.1</span></>}
      {state.kind === "available" && <><strong>发现 Axiom {state.update.version}</strong><span>{state.update.body?.trim() || "新版本已可下载。"}</span></>}
      {state.kind === "downloading" && <><strong>正在下载 Axiom {state.update.version}</strong><span>{state.progress == null ? "正在接收更新包…" : String(state.progress) + "%"}</span></>}
      {state.kind === "ready" && <><strong>更新已安装</strong><span>重启 Axiom 以使用 {state.update.version}。</span></>}
      {state.kind === "error" && <><strong>更新检查失败</strong><span>{state.message}</span></>}
    </div>
    <div className="update-banner-actions">
      {state.kind === "available" && <button className="primary" onClick={() => void install(state.update)}>下载并安装</button>}
      {state.kind === "ready" && <button className="primary" onClick={() => void relaunch()}>立即重启</button>}
      {state.kind === "error" && <button className="secondary" onClick={() => void checkNow(true)}>重试</button>}
      {state.kind !== "checking" && state.kind !== "downloading" && <button className="icon-button" onClick={dismiss} aria-label="关闭更新提示"><X size={15}/></button>}
    </div>
    {state.kind === "downloading" && state.progress != null && <div className="update-progress"><i style={{ width: String(state.progress) + "%" }}/></div>}
  </aside>;
}
