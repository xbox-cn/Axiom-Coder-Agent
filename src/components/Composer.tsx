import { ArrowUp, BrainCircuit, CircleStop, FileText, Gauge, Goal, Paperclip, Shield, Sparkles, X } from "lucide-react";
import { useEffect, useRef, type ReactNode } from "react";
import { onAttachmentDrop } from "../lib/api";
import { resolveContextLimit } from "../lib/context";
import type { PermissionMode, RunMode, ThinkingLevel } from "../lib/types";
import { statusIsRunning, useAppStore } from "../store/appStore";
import { Dropdown, type DropdownOption } from "./Dropdown";

const modeOptions: DropdownOption<RunMode>[] = [
  { value: "agent", label: "Agent", description: "读取、修改并验证代码" },
  { value: "plan", label: "Plan", description: "只读分析并生成实施计划" },
  { value: "goal", label: "Goal", description: "持续执行直到完成或阻塞" },
];
const permissionOptions: DropdownOption<PermissionMode>[] = [
  { value: "read-only", label: "只读", description: "仅允许读取和搜索" },
  { value: "workspace-auto", label: "工作区自动", description: "自动处理工作区内低风险操作" },
  { value: "full-access", label: "完全访问", description: "可执行任意本地命令" },
];
const thinkingOptions: DropdownOption<ThinkingLevel>[] = [
  { value: "off", label: "不思考" }, { value: "low", label: "低" }, { value: "medium", label: "中" },
  { value: "high", label: "高" }, { value: "xhigh", label: "极高" }, { value: "auto", label: "自动" },
];

export function Composer({ disabled = false, topSlot }: { disabled?: boolean; topSlot?: ReactNode }) {
  const draft = useAppStore((state) => state.draft);
  const setDraft = useAppStore((state) => state.setDraft);
  const send = useAppStore((state) => state.send);
  const cancel = useAppStore((state) => state.cancel);
  const detail = useAppStore((state) => state.threadDetail);
  const data = useAppStore((state) => state.bootstrapData);
  const providerId = useAppStore((state) => state.providerId);
  const modelId = useAppStore((state) => state.modelId);
  const thinking = useAppStore((state) => state.thinkingLevel);
  const permission = useAppStore((state) => state.permissionMode);
  const runMode = useAppStore((state) => state.runMode);
  const attachments = useAppStore((state) => state.attachments);
  const setProvider = useAppStore((state) => state.setProviderId);
  const setModel = useAppStore((state) => state.setModelId);
  const setThinking = useAppStore((state) => state.setThinkingLevel);
  const setPermission = useAppStore((state) => state.setPermissionMode);
  const setRunMode = useAppStore((state) => state.setRunMode);
  const addAttachments = useAppStore((state) => state.addAttachments);
  const removeAttachment = useAppStore((state) => state.removeAttachment);
  const setModal = useAppStore((state) => state.setModal);
  const textarea = useRef<HTMLTextAreaElement>(null);

  const running = statusIsRunning(detail?.thread.status);
  const latestRun = detail?.runs.at(-1);
  const context = latestRun?.usage.contextTokens ?? 0;
  const limit = resolveContextLimit(data?.providers ?? [], providerId, modelId, latestRun);
  const percent = limit > 0 ? Math.min(100, Math.round(context / limit * 100)) : 0;
  const providers = data?.providers.filter((provider) => provider.enabled) ?? [];
  const provider = providers.find((item) => item.id === providerId);
  const providerOptions = providers.map((item) => ({ value: item.id, label: item.name, description: `${item.models.length} 个模型` }));
  const modelOptions = (provider?.models ?? []).map((model) => ({ value: model.modelId, label: model.displayName || model.modelId, description: model.contextWindowTokens ? `${model.contextWindowTokens / 10_000} 万 Token` : undefined }));
  const validProvider = Boolean(provider);
  const validModel = Boolean(provider?.models.some((model) => model.modelId === modelId));
  const canSend = !disabled && !running && validProvider && validModel && Boolean(draft.trim() || attachments.length);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onAttachmentDrop((paths) => {
      if (!disposed) void addAttachments(paths);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    }, () => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [addAttachments]);

  useEffect(() => {
    const node = textarea.current;
    if (!node) return;
    node.style.height = "auto";
    node.style.height = `${Math.min(200, Math.max(72, node.scrollHeight))}px`;
  }, [draft]);

  useEffect(() => {
    const focus = () => textarea.current?.focus();
    window.addEventListener("axiom-focus-composer", focus);
    return () => window.removeEventListener("axiom-focus-composer", focus);
  }, []);

  const changePermission = (value: PermissionMode) => {
    if (value === "full-access" && !window.confirm("完全访问模式不提供 OS 级沙箱。Agent 可执行任意本地命令，确认启用？")) return;
    setPermission(value);
  };
  const changeMode = (value: RunMode) => {
    setRunMode(value);
    if (value === "plan") setPermission("read-only");
  };
  const onKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key !== "Enter" || event.shiftKey || event.nativeEvent.isComposing) return;
    event.preventDefault();
    if (canSend) void send();
  };
  const onDrop = (event: React.DragEvent) => {
    event.preventDefault();
    const paths = [...event.dataTransfer.files].map((file) => (file as File & { path?: string }).path).filter((path): path is string => Boolean(path));
    if (paths.length) void addAttachments(paths);
  };

  return <div className="composer-wrap">
    {topSlot ? <div className="composer-top-slot">{topSlot}</div> : null}
    <div className={`composer ${running ? "locked" : ""}`} onDragOver={(event) => event.preventDefault()} onDrop={onDrop}>
      <div className="composer-input-area">
        <textarea ref={textarea} value={draft} onChange={(event) => setDraft(event.target.value)} onKeyDown={onKeyDown} disabled={disabled} rows={1} placeholder={disabled ? "打开项目后即可开始" : "描述任务，使用 @ 引用文件…"}/>
        {attachments.length > 0 && <div className="attachment-chips">{attachments.map((attachment) => <span className="attachment-chip" key={attachment.id}><FileText size={13}/><span title={attachment.name}>{attachment.name}</span><small>{formatBytes(attachment.size)}</small><button onClick={() => removeAttachment(attachment.id)} aria-label={`移除附件 ${attachment.name}`}><X size={12}/></button></span>)}</div>}
      </div>
      <div className="composer-controls">
        <div className="composer-controls-left">
          <button className="attach-button" aria-label="添加文件" title="添加文件" onClick={() => void addAttachments()} disabled={disabled || running}><Paperclip size={16}/></button>
          <Dropdown ariaLabel="运行模式" className="mode" value={runMode} options={modeOptions} onChange={changeMode} disabled={running || disabled} icon={runMode === "goal" ? <Goal size={14}/> : runMode === "plan" ? <Gauge size={14}/> : <Sparkles size={14}/>}/>
          <Dropdown ariaLabel="权限模式" className={`permission ${permission}`} value={runMode === "plan" ? "read-only" : permission} options={permissionOptions} onChange={changePermission} disabled={running || disabled || runMode === "plan"} icon={<Shield size={14}/>}/>
        </div>
        <div className="composer-controls-center">
          {providers.length ? <Dropdown ariaLabel="供应商" className="provider" value={providerId} options={providerOptions} onChange={setProvider} disabled={running || disabled} placeholder="选择供应商"/> : <button className="add-provider-inline" onClick={() => setModal("providers")} disabled={running || disabled}>添加供应商</button>}
          <Dropdown ariaLabel="模型" className="model" value={modelId} options={modelOptions} onChange={setModel} disabled={running || disabled || !provider || !modelOptions.length} placeholder={provider ? "添加模型" : "选择模型"} icon={<Sparkles size={14}/>}/>
          <Dropdown ariaLabel="思考程度" className="thinking" value={thinking} options={thinkingOptions} onChange={setThinking} disabled={running || disabled} icon={<BrainCircuit size={14}/>}/>
        </div>
        <div className="send-group">
          <div className="context-ring" style={{ "--context": `${percent * 3.6}deg` } as React.CSSProperties} title={`当前上下文 ${context.toLocaleString()} / ${limit.toLocaleString()}`}><span>{percent}</span></div>
          {running ? <button className="stop-button" onClick={() => void cancel()} aria-label="停止"><CircleStop size={18}/></button> : <button className="send-button" onClick={() => void send()} disabled={!canSend} aria-label="发送"><ArrowUp size={18}/></button>}
        </div>
      </div>
    </div>
    <div className="composer-hint">{runMode === "plan" ? "Plan 模式由后端强制只读。" : runMode === "goal" ? "Goal 会持续执行，直到完成、阻塞、等待审批或你主动停止。" : "提交前请审查命令与代码变更。"} <kbd>Enter 发送</kbd><span> · </span><kbd>Shift Enter 换行</kbd></div>
  </div>;
}

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
