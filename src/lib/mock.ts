import type { AppBootstrap, Message, RunRecord, ThreadDetail } from "./types";

const now = new Date().toISOString();
const projectId = "demo-project";
const threadId = "demo-thread";

export const demoRun: RunRecord = {
  id:"demo-run", threadId, status:"completed",
  config:{ providerId:"ollama-local", modelId:"qwen3-coder", thinkingLevel:"high", permissionMode:"workspace-auto", runMode:"agent", createdAt:now },
  usage:{ inputTokens:28420, outputTokens:1824, cachedTokens:6144, reasoningTokens:932, contextTokens:37320, contextLimit:128000, cumulativeTokens:92614, estimated:false, durationMs:12400, firstTokenMs:830, estimatedCostUsd:0 },
  startedAt:now, completedAt:now,
};

export const demoMessages: Message[] = [
  { id:"m1", threadId, role:"user", content:"检查这个项目的登录流程，找出会导致会话失效的原因，并给出最小修改方案。", createdAt:now, pinned:false, attachments:[] },
  { id:"m2", threadId, role:"assistant", runId:"demo-run", createdAt:now, pinned:false, attachments:[], content:"我沿着 `session → middleware → refresh` 的调用顺序检查了一遍。问题出在刷新请求并发时，每个请求都会覆盖前一个请求写入的 token。\n\n建议把刷新逻辑收敛为单例 Promise，并只在刷新完成后更新持久化状态：\n\n```ts\nlet refreshInFlight: Promise<Session> | null = null;\n\nexport function refreshSession() {\n  refreshInFlight ??= requestRefresh().finally(() => {\n    refreshInFlight = null;\n  });\n  return refreshInFlight;\n}\n```\n\n这项修改不改变现有 API，并能避免多个 401 同时触发刷新。右侧 **Changes** 中已经整理了建议 Diff。" },
];

export const demoBootstrap: AppBootstrap = {
  projects:[{ id:projectId, name:"axiom-demo", path:"D:\\Projects\\axiom-demo", favorite:true, createdAt:now, updatedAt:now, gitBranch:"main" }],
  threads:[
    { id:threadId, projectId, title:"修复登录会话并发刷新", status:"completed", createdAt:now, updatedAt:now, unreadApproval:false },
    { id:"t2", projectId, title:"优化构建与包体积", status:"idle", createdAt:now, updatedAt:now, unreadApproval:false },
    { id:"t3", projectId, title:"重构 Provider 适配层", status:"awaiting-approval", createdAt:now, updatedAt:now, unreadApproval:true },
  ],
  providers:[],
  mcpServers:[
    { id:"mcp1", name:"GitHub", scope:"global", transport:"stdio", command:"npx", args:["-y","@modelcontextprotocol/server-github"], env:{ GITHUB_TOKEN:"••••••••" }, headers:{}, timeoutSeconds:30, enabled:true, status:"healthy", discoveredTools:[], disabledTools:[], readOnlyTools:[], updatedAt:now },
    { id:"mcp2", name:"Filesystem", scope:"project", projectId, transport:"stdio", command:"npx", args:["-y","@modelcontextprotocol/server-filesystem"], env:{}, headers:{}, timeoutSeconds:30, enabled:true, status:"stopped", discoveredTools:[], disabledTools:[], readOnlyTools:[], updatedAt:now },
  ],
  settings:{ theme:"system", sidebarCollapsed:false, sidebarWidth:272, inspectorOpen:true, inspectorWidth:420, defaultPermission:"workspace-auto", defaultProviderId:null, defaultModelId:null, defaultThinkingLevel:"medium" },
};

export const demoThread: ThreadDetail = { thread:demoBootstrap.threads[0], messages:demoMessages, runs:[demoRun], contextSnapshots:[], goals:[] };
