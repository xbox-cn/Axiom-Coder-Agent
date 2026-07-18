import { Activity, AlertTriangle, Check, CheckCircle2, CheckCheck, ChevronDown, ChevronRight, CircleCheck, CircleX, Clock3, Code2, Copy, FileText, GitBranch, LoaderCircle, MoreHorizontal, PanelLeft, PanelRight, Pause, Pencil, Pin, Play, RotateCcw, Sparkles, Square, TerminalSquare, Wrench } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { createPortal } from "react-dom";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { ApprovalRequest, GoalRecord, Message, RunRecord, RunStatus, ToolActivity } from "../lib/types";
import { statusIsRunning, useAppStore } from "../store/appStore";
import { Composer } from "./Composer";

const statusLabel: Record<RunStatus, string> = { idle: "空闲", queued: "排队中", reasoning: "思考中", streaming: "生成中", "tool-running": "执行工具", "awaiting-approval": "等待审批", completed: "已完成", failed: "失败", cancelled: "已取消" };
const modeLabel = { agent: "Agent", plan: "Plan", goal: "Goal" } as const;

type FeedItem =
  | { kind: "goal"; key: string; run: RunRecord; goal?: GoalRecord }
  | { kind: "message"; key: string; message: Message; run?: RunRecord }
  | { kind: "reasoning"; key: string; run: RunRecord }
  | { kind: "context"; key: string; summary: string }
  | { kind: "tools"; key: string; activities: ToolActivity[] }
  | { kind: "streaming"; key: string; content: string }
  | { kind: "thinking"; key: string; status: RunStatus }
  | { kind: "approval"; key: string; approval: ApprovalRequest }
  | { kind: "starter"; key: string; hasProvider: boolean };

export function Conversation() {
  const detail = useAppStore((state) => state.threadDetail);
  const data = useAppStore((state) => state.bootstrapData);
  const streaming = useAppStore((state) => state.streamingContent);
  const setInspectorTab = useAppStore((state) => state.setInspectorTab);
  const inspectorOpen = useAppStore((state) => state.inspectorOpen);
  const setInspectorOpen = useAppStore((state) => state.setInspectorOpen);
  const setSidebarOverlayOpen = useAppStore((state) => state.setSidebarOverlayOpen);
  const pendingApproval = useAppStore((state) => state.pendingApproval);
  const toolActivities = useAppStore((state) => state.toolActivities);
  const contextRecords = useAppStore((state) => state.contextRecords);
  const respondApproval = useAppStore((state) => state.respondApproval);
  const project = data?.projects.find((item) => item.id === detail?.thread.projectId);
  const scrollRef = useRef<HTMLDivElement>(null);
  const stickToBottomRef = useRef(true);
  const runs = useMemo(() => new Map(detail?.runs.map((run) => [run.id, run]) ?? []), [detail?.runs]);
  const goalRecord = detail?.goals.at(-1);
  const goalRun = goalRecord
    ? detail?.runs.find((run) => run.id === goalRecord.runId)
    : [...(detail?.runs ?? [])].reverse().find((run) => run.config.runMode === "goal");

  const feedItems = useMemo<FeedItem[]>(() => {
    if (!detail) return [];
    const items: FeedItem[] = [];
    if (goalRun) items.push({ kind: "goal", key: `goal-${goalRun.id}`, run: goalRun, goal: goalRecord });
    const reasoningRunIds = new Set<string>();
    for (const message of detail.messages) {
      const messageRun = message.runId ? runs.get(message.runId) : undefined;
      if (message.role === "assistant" && messageRun?.reasoningContent?.trim() && !reasoningRunIds.has(messageRun.id)) {
        items.push({ kind: "reasoning", key: `reasoning-${messageRun.id}`, run: messageRun });
        reasoningRunIds.add(messageRun.id);
      }
      items.push({ kind: "message", key: message.id, message, run: messageRun });
    }
    for (const run of detail.runs) {
      if (run.reasoningContent?.trim() && !reasoningRunIds.has(run.id)) {
        items.push({ kind: "reasoning", key: `reasoning-${run.id}`, run });
        reasoningRunIds.add(run.id);
      }
    }
    for (const record of contextRecords) items.push({ kind: "context", key: `context-${record.id}`, summary: record.summary });
    if (toolActivities.length > 0) items.push({ kind: "tools", key: "live-tools", activities: toolActivities });
    if (streaming) items.push({ kind: "streaming", key: "streaming", content: streaming });
    const hasLiveReasoning = detail.runs.some((run) => statusIsRunning(run.status) && Boolean(run.reasoningContent?.trim()));
    if (statusIsRunning(detail.thread.status) && detail.thread.status !== "awaiting-approval" && !streaming && !hasLiveReasoning) {
      items.push({ kind: "thinking", key: "thinking", status: detail.thread.status });
    }
    if (pendingApproval) items.push({ kind: "approval", key: pendingApproval.id, approval: pendingApproval });
    if (detail.messages.length === 0 && !statusIsRunning(detail.thread.status)) {
      items.push({ kind: "starter", key: "starter", hasProvider: Boolean(data?.providers.length) });
    }
    return items;
  }, [contextRecords, data?.providers.length, detail, goalRecord, goalRun, pendingApproval, runs, streaming, toolActivities]);

  const rowVirtualizer = useVirtualizer({
    count: feedItems.length,
    getScrollElement: () => scrollRef.current,
    getItemKey: (index) => feedItems[index]?.key ?? index,
    estimateSize: (index) => estimateFeedItemSize(feedItems[index]),
    initialRect: { width: 850, height: 800 },
    overscan: 8,
  });

  const measuredRows = rowVirtualizer.getVirtualItems();
  const visibleRows = measuredRows.length > 0 ? measuredRows : fallbackFeedRows(feedItems);
  const feedHeight = Math.max(
    rowVirtualizer.getTotalSize(),
    visibleRows.at(-1) ? visibleRows.at(-1)!.start + visibleRows.at(-1)!.size : 0,
  );

  useEffect(() => {
    if (!stickToBottomRef.current) return;
    const frame = requestAnimationFrame(() => {
      const node = scrollRef.current;
      if (node) node.scrollTop = node.scrollHeight;
    });
    return () => cancelAnimationFrame(frame);
  }, [feedItems.length, streaming]);

  if (!detail) {
    return <main className="conversation empty-conversation"><div className="empty-hero"><div className="axiom-mark hero"><i/><i/><i/></div><h1>让 Axiom 在你的代码中工作</h1><p>打开一个本地项目，创建任务，然后添加供应商与模型。</p></div><Composer disabled/></main>;
  }

  return <main className="conversation">
    <header className="conversation-header">
      <button className="mobile-sidebar-toggle" onClick={() => setSidebarOverlayOpen(true)} aria-label="打开侧栏" title="打开侧栏"><PanelLeft size={17}/></button>
      <div className="project-crumb"><span title={project?.path}>{project?.name ?? "项目"}</span>{project?.gitBranch ? <small><GitBranch size={13}/>{project.gitBranch}</small> : null}<span className="crumb-separator">/</span><strong title={detail.thread.title}>{detail.thread.title}</strong></div>
      <div className="header-meta"><span className={`status-pill ${detail.thread.status}`}><i/>{statusLabel[detail.thread.status]}</span><button className={`header-inspector-toggle ${inspectorOpen ? "active" : ""}`} onClick={() => setInspectorOpen(!inspectorOpen)} aria-label={inspectorOpen ? "关闭检查器" : "打开检查器"} title="切换检查器 (Ctrl+Shift+I)"><PanelRight size={17}/></button><button aria-label="更多操作"><MoreHorizontal size={18}/></button></div>
    </header>
    <div
      className="message-scroll"
      ref={scrollRef}
      onScroll={(event) => {
        const node = event.currentTarget;
        stickToBottomRef.current = node.scrollHeight - node.scrollTop - node.clientHeight < 140;
      }}
    >
      <div className="message-column">
        <div className="virtual-feed" style={{ height: feedHeight }}>
          {visibleRows.map((virtualRow) => {
            const item = feedItems[virtualRow.index];
            return (
              <div
                className="virtual-feed-row"
                data-index={virtualRow.index}
                key={virtualRow.key}
                ref={rowVirtualizer.measureElement}
                style={{ transform: `translateY(${virtualRow.start}px)` }}
              >
                <FeedRow item={item} onOpenChanges={() => setInspectorTab("changes")} onRespondApproval={respondApproval}/>
              </div>
            );
          })}
        </div>
      </div>
    </div>
    <Composer/>
  </main>;
}

function FeedRow({ item, onOpenChanges, onRespondApproval }: { item: FeedItem; onOpenChanges: () => void; onRespondApproval: (approved: boolean) => Promise<void> }) {
  switch (item.kind) {
    case "goal": return <GoalStatusCard run={item.run} goal={item.goal}/>;
    case "message": return <MessageView message={item.message} run={item.run} onOpenChanges={onOpenChanges}/>;
    case "reasoning": return <ReasoningBlock run={item.run}/>;
    case "context": return <ContextCompressionRecord summary={item.summary}/>;
    case "tools": return <ToolActivityList activities={item.activities}/>;
    case "streaming": return <article className="message assistant streaming-message"><div className="message-body"><Markdown content={item.content}/><span className="stream-caret"/></div></article>;
    case "thinking": return <ThinkingState status={item.status}/>;
    case "approval": return <ApprovalCard approval={item.approval} onRespond={onRespondApproval}/>;
    case "starter": return <StarterState hasProvider={item.hasProvider}/>;
  }
}

function fallbackFeedRows(items: FeedItem[]) {
  let start = 0;
  return items.slice(0, 24).map((item, index) => {
    const size = estimateFeedItemSize(item);
    const row = { index, key: item.key, start, size };
    start += size;
    return row;
  });
}

function estimateFeedItemSize(item?: FeedItem) {
  if (!item) return 96;
  if (item.kind === "starter") return 460;
  if (item.kind === "approval") return 150;
  if (item.kind === "goal") return 76;
  if (item.kind === "tools") {
    const expandedActivities = item.activities.filter((activity) => !isCompactToolActivity(activity));
    return Math.max(66, 58 + expandedActivities.length * 58);
  }
  if (item.kind === "reasoning") return 108;
  if (item.kind === "message") return item.message.role === "user" ? 82 : 150;
  return 72;
}

function MessageView({ message, run, onOpenChanges }: { message: Message; run?: RunRecord; onOpenChanges: () => void }) {
  const setDraft = useAppStore((state) => state.setDraft);
  const setRunMode = useAppStore((state) => state.setRunMode);
  const reuseMessage = useAppStore((state) => state.reuseMessage);
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    await navigator.clipboard.writeText(message.content);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1400);
  };
  if (message.role === "system") return <div className="system-message"><Activity size={13}/>{message.content}</div>;
  if (message.role === "user") return <article className="message user"><div className="user-bubble">{message.content && <p>{message.content}</p>}{message.attachments.length > 0 && <AttachmentList attachments={message.attachments}/>}</div><div className="message-actions user-actions"><button onClick={() => void copy()} aria-label="复制消息" title="复制消息">{copied?<CheckCheck size={14}/>:<Copy size={14}/>}</button><button onClick={() => reuseMessage(message)} aria-label="编辑并重新发送" title="编辑并重新发送"><Pencil size={14}/></button></div></article>;
  const executePlan = () => { setRunMode("agent"); setDraft(`请严格按以下计划执行，并在完成后验证结果：\n\n${message.content}`); };
  return <article className="message assistant"><div className="message-body"><div className="message-actions assistant-actions"><button onClick={() => void copy()} aria-label="复制消息" title="复制消息">{copied?<CheckCheck size={14}/>:<Copy size={14}/>}</button></div><Markdown content={message.content}/>{run && <>
    {run.config.runMode === "plan" && run.status === "completed" && <button className="execute-plan" onClick={executePlan}><Play size={14}/>按计划执行</button>}
    {run.config.runMode !== "plan" && <button className="change-summary" onClick={onOpenChanges}><CheckCircle2 size={15}/><span>查看本回合产生的代码变更</span><ChevronRight size={14}/></button>}
    <RunMeta run={run}/>
  </>}</div></article>;
}

function AttachmentList({ attachments }: { attachments: Message["attachments"] }) {
  return <div className="message-attachments">{attachments.map((attachment) => <span key={attachment.id}><FileText size={13}/><span title={attachment.name}>{attachment.name}</span><small>{formatBytes(attachment.size)}</small></span>)}</div>;
}

function Markdown({ content }: { content: string }) {
  return <ReactMarkdown remarkPlugins={[remarkGfm]} components={{
    code({ className, children, ...props }) { const inline = !className && !String(children).includes("\n"); return inline ? <code className="inline-code" {...props}>{children}</code> : <div className="code-block"><div className="code-toolbar"><span>{className?.replace("language-", "") || "code"}</span><button onClick={() => void navigator.clipboard.writeText(String(children))}>复制</button></div><pre><code className={className} {...props}>{children}</code></pre></div>; },
    a({ children, ...props }) { return <a {...props} target="_blank" rel="noreferrer">{children}</a>; },
  }}>{content}</ReactMarkdown>;
}

function ReasoningBlock({ run }: { run: RunRecord }) {
  const running = statusIsRunning(run.status);
  const [open, setOpen] = useState(running);
  useEffect(() => {
    if (running) setOpen(true);
  }, [running]);
  return <details className={`reasoning-block ${running ? "running" : "completed"}`} open={open} onToggle={(event) => setOpen(event.currentTarget.open)}>
    <summary><Sparkles size={14}/><span><strong>{running ? "正在思考" : "思考过程"}</strong><small>{run.usage.reasoningTokens != null ? `${formatNumber(run.usage.reasoningTokens)} reasoning tokens` : "可展开查看模型推理"}</small></span><ChevronRight size={14}/></summary>
    <div className="reasoning-content"><Markdown content={run.reasoningContent ?? ""}/></div>
  </details>;
}

function RunMeta({ run }: { run: RunRecord }) {
  const [open, setOpen] = useState(false); const usage = run.usage;
  return <div className="run-meta"><button className="run-meta-summary" onClick={() => setOpen((value) => !value)}>{open ? <ChevronDown size={13}/> : <ChevronRight size={13}/>}<span>{modeLabel[run.config.runMode]}</span><span>·</span><span>{run.config.providerId} / {run.config.modelId}</span><span>·</span><span>{thinkingText(run.config.thinkingLevel)}</span><span>·</span><span>{formatTokenTotal(usage.inputTokens, usage.outputTokens)} tokens</span>{usage.durationMs ? <><span>·</span><span>{formatDuration(usage.durationMs)}</span></> : null}<span className={usage.estimated ? "estimate-tag" : "exact-tag"}>{usage.estimated ? "估算" : "准确"}</span></button>
    {open && <div className="run-meta-grid"><Metric label="模式" value={modeLabel[run.config.runMode]}/><Metric label="权限" value={run.config.permissionMode}/><Metric label="输入" value={formatNumber(usage.inputTokens)}/><Metric label="输出" value={formatNumber(usage.outputTokens)}/><Metric label="缓存" value={formatNumber(usage.cachedTokens)}/><Metric label="推理" value={formatNumber(usage.reasoningTokens)}/><Metric label="上下文" value={`${formatNumber(usage.contextTokens)} / ${formatNumber(usage.contextLimit)}`}/><Metric label="累计" value={formatNumber(usage.cumulativeTokens)}/><Metric label="首 Token" value={usage.firstTokenMs ? `${usage.firstTokenMs}ms` : "—"}/><Metric label="费用" value={usage.estimatedCostUsd != null ? `$${usage.estimatedCostUsd.toFixed(4)}` : "—"}/></div>}
  </div>;
}
function Metric({ label, value }: { label: string; value: string }) { return <div><span>{label}</span><strong>{value}</strong></div>; }
function thinkingText(level: string) { return level === "off" ? "不思考" : level === "auto" ? "自动思考" : `${level.toUpperCase()} 思考`; }
function formatNumber(value?: number | null) { return value == null ? "—" : new Intl.NumberFormat("zh-CN", { notation: value > 9999 ? "compact" : "standard", maximumFractionDigits: 1 }).format(value); }
function formatDuration(ms: number) { return ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(1)}s`; }
function formatTokenTotal(input?: number | null, output?: number | null) { return input == null && output == null ? "—" : formatNumber((input ?? 0) + (output ?? 0)); }
function formatBytes(bytes: number) { return bytes < 1024 ? `${bytes} B` : bytes < 1024 * 1024 ? `${(bytes / 1024).toFixed(1)} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`; }

function GoalStatusCard({ run, goal }: { run: RunRecord; goal?: GoalRecord }) {
  const cancel = useAppStore((state) => state.cancel);
  const resumeGoal = useAppStore((state) => state.resumeGoal);
  const finishGoal = useAppStore((state) => state.finishGoal);
  const setRunMode = useAppStore((state) => state.setRunMode);
  const goalStatus = goal?.status ?? run.status;
  const running = statusIsRunning(run.status) || goalStatus === "running" || goalStatus === "awaiting-approval";
  const elapsed = run.usage.durationMs ?? Math.max(0, Date.now() - new Date(run.startedAt).getTime());
  const canResume = ["paused", "blocked", "failed"].includes(goalStatus);
  const resume = async () => { setRunMode("goal"); await resumeGoal(run.id); };
  return <section className={`goal-status-card ${goalStatus}`}><div className="goal-status-icon">{running ? <LoaderCircle className="spin" size={15}/> : <Sparkles size={15}/>}</div><div><strong>Goal · {goalStatusText(goalStatus)}</strong><span>{goal ? `${goal.turnCount} 回合 · ` : ""}{formatTokenTotal(run.usage.inputTokens, run.usage.outputTokens)} tokens · {formatDuration(elapsed)}{run.usage.estimatedCostUsd != null ? ` · $${run.usage.estimatedCostUsd.toFixed(4)}` : ""}</span></div><div className="goal-actions">{running ? <button onClick={() => void cancel()}><Pause size={14}/>暂停</button> : canResume ? <button onClick={() => void resume()}><Play size={14}/>继续</button> : null}<button onClick={() => { void finishGoal(run.id); setRunMode("agent"); }}><Square size={13}/>结束</button></div></section>;
}
function goalStatusText(status: string) { return status === "awaiting-approval" ? "等待审批" : status === "paused" ? "已暂停" : status === "blocked" ? "已阻塞" : status === "running" ? "运行中" : status === "completed" ? "已完成" : status === "failed" ? "失败" : statusLabel[status as RunStatus] ?? status; }
function ThinkingState({ status }: { status: RunStatus }) { return <article className="thinking-state"><LoaderCircle className="spin" size={14}/><div><strong>{status === "queued" ? "等待运行" : status === "tool-running" ? "正在执行工具" : "正在思考"}</strong><span>Agent 正在读取上下文并规划下一步</span></div></article>; }
function ApprovalCard({ approval, onRespond }: { approval: ApprovalRequest; onRespond: (approved: boolean) => Promise<void> }) {
  const respondQuestion = useAppStore((state) => state.respondQuestion);
  const [customAnswer, setCustomAnswer] = useState("");
  if (approval.toolName === "ask_user") {
    const rawOptions = Array.isArray(approval.arguments.options) ? approval.arguments.options : [];
    const options = rawOptions.flatMap((value) => {
      if (!value || typeof value !== "object") return [];
      const item = value as Record<string, unknown>;
      if (typeof item.id !== "string" || typeof item.label !== "string") return [];
      return [{ id: item.id, label: item.label, description: typeof item.description === "string" ? item.description : "" }];
    });
    return createPortal(<div className="question-dialog-backdrop" aria-hidden="false">
      <div className="question-card" role="dialog" aria-modal="true" aria-live="assertive" aria-label="Plan 需要你的选择">
        <div className="question-heading"><Sparkles size={17}/><div><strong>{approval.summary}</strong><span>选择一项，Axiom 会继续完成计划</span></div></div>
        <div className="question-options">{options.map((option, index) => <button autoFocus={index === 0} key={option.id} onClick={() => void respondQuestion(option.id)}><kbd>{index + 1}</kbd><span><strong>{option.label}</strong>{option.description && <small>{option.description}</small>}</span><ChevronRight size={15}/></button>)}</div>
        <div className="question-other"><input value={customAnswer} onChange={(event) => setCustomAnswer(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter" && customAnswer.trim()) void respondQuestion(customAnswer.trim()); }} placeholder="或输入其他答案"/><button disabled={!customAnswer.trim()} onClick={() => void respondQuestion(customAnswer.trim())}>提交</button></div>
      </div>
    </div>, document.body);
  }
  const argumentsText = JSON.stringify(approval.arguments, null, 2);
  return <div className="approval-card" role="alert" aria-live="assertive"><div className="approval-icon"><TerminalSquare size={18}/></div><div><strong>需要批准 {approval.toolName}</strong><code>{argumentsText}</code><p>{approval.summary}</p><small>仅允许这一次调用；敏感参数已在后端脱敏。</small></div><div className="approval-actions"><button className="ghost" onClick={() => void onRespond(false)}><RotateCcw size={14}/>拒绝</button><button className="accent" onClick={() => void onRespond(true)}><Check size={14}/>允许一次</button></div></div>;
}
const COMPACT_TOOL_NAMES = new Set(["list_files", "read_file", "search_files", "git_status", "git_diff"]);

function isCompactToolActivity(activity: ToolActivity) {
  return activity.status !== "failed" && COMPACT_TOOL_NAMES.has(activity.name);
}

function CompactToolActivityGroup({ activities }: { activities: ToolActivity[] }) {
  const running = activities.some((activity) => activity.status === "running");
  const [open, setOpen] = useState(running);
  useEffect(() => {
    if (running) setOpen(true);
  }, [running]);
  const completed = activities.filter((activity) => activity.status === "completed").length;
  return <details className={`tool-activity-group ${running ? "running" : "completed"}`} open={open} onToggle={(event) => setOpen(event.currentTarget.open)}>
    <summary><span className="tool-icon"><Wrench size={14}/></span><span><strong>{running ? "正在检查代码" : `已完成 ${completed} 项代码检查`}</strong><small>{activities.map((activity) => activity.name).join(" · ")}</small></span>{running ? <LoaderCircle className="spin" size={15}/> : <CircleCheck size={15}/>}<ChevronRight size={14}/></summary>
    <div className="tool-activity-group-rows">{activities.map((activity) => {
      const StatusIcon = activity.status === "running" ? LoaderCircle : CircleCheck;
      return <div className={`tool-activity-row ${activity.status}`} key={activity.id}><StatusIcon className={activity.status === "running" ? "spin" : ""} size={13}/><code>{activity.name}</code><span title={activity.summary}>{activity.summary}</span>{activity.durationMs != null && <time>{formatDuration(activity.durationMs)}</time>}</div>;
    })}</div>
  </details>;
}

function ToolActivityList({ activities }: { activities: ToolActivity[] }) {
  const compact = activities.filter(isCompactToolActivity);
  const detailed = activities.filter((activity) => !isCompactToolActivity(activity));
  return <div className="tool-activity-list" aria-label="工具活动">
    {compact.length > 0 ? <CompactToolActivityGroup activities={compact}/> : null}
    {detailed.map((activity) => { const StatusIcon = activity.status === "running" ? LoaderCircle : activity.status === "completed" ? CircleCheck : CircleX; return <details className={`tool-activity ${activity.status}`} key={activity.id} open={activity.status === "running"}><summary><span className="tool-icon"><Wrench size={14}/></span><span><strong>{activity.name}</strong><small>{activity.summary}</small></span><StatusIcon className={activity.status === "running" ? "spin" : ""} size={15}/>{activity.durationMs != null && <time>{formatDuration(activity.durationMs)}</time>}</summary>{activity.output && <pre>{activity.output}</pre>}</details>; })}
  </div>;
}
function ContextCompressionRecord({ summary }: { summary: string }) { return <details className="context-record"><summary><Activity size={13}/><span>上下文已透明压缩</span><ChevronRight size={13}/></summary><pre>{summary}</pre></details>; }
function StarterState({ hasProvider }: { hasProvider: boolean }) {
  const setModal = useAppStore((state) => state.setModal);
  const setDraft = useAppStore((state) => state.setDraft);
  return <div className="starter-state"><div className="axiom-mark hero"><i/><i/><i/></div><h2>你想在这个项目中完成什么？</h2><p>{hasProvider ? "描述任务，Axiom 会分析代码并在你的许可范围内执行。" : "先添加供应商与模型，然后开始第一个编程任务。"}</p>{!hasProvider ? <button className="starter-add-provider" onClick={() => setModal("providers")}>添加供应商</button> : <div className="starter-grid"><button onClick={() => setDraft("理解项目结构并说明主要模块") }><Code2 size={17}/><span>理解项目结构</span></button><button onClick={() => setDraft("查找并修复这个项目中最明显的问题") }><AlertTriangle size={17}/><span>查找并修复问题</span></button><button onClick={() => setDraft("分析并优化项目性能") }><Clock3 size={17}/><span>优化性能</span></button><button onClick={() => setDraft("解释我接下来引用的代码") }><Pin size={17}/><span>解释一段代码</span></button></div>}</div>;
}
