import {
  Check,
  ChevronRight,
  Cloud,
  Download,
  Eye,
  EyeOff,
  LoaderCircle,
  Monitor,
  Moon,
  Plus,
  Search,
  Server,
  Sun,
  TestTube2,
  Trash2,
  Waypoints,
  X,
  type LucideIcon,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { discoverProviderModelsDraft, saveSettings, testMcpServer, testProviderModelDraft } from "../lib/api";
import type {
  DraftModelTestResult,
  McpServerConfig,
  ProviderApiType,
  ProviderModelInput,
  ProviderProfile,
  ProviderProfileInput,
} from "../lib/types";
import { useAppStore } from "../store/appStore";
import { Dropdown } from "./Dropdown";

export function AppModal() {
  const modal = useAppStore((state) => state.modal);
  const setModal = useAppStore((state) => state.setModal);

  useEffect(() => {
    if (!modal) return;
    const close = (event: KeyboardEvent) => {
      if (event.key === "Escape") setModal(null);
    };
    window.addEventListener("keydown", close);
    return () => window.removeEventListener("keydown", close);
  }, [modal, setModal]);

  if (!modal) return null;
  return (
    <div
      className="modal-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) setModal(null);
      }}
    >
      <section className={`modal modal-${modal}`} role="dialog" aria-modal="true" aria-label={modalTitle[modal]}>
        <button className="modal-close" onClick={() => setModal(null)} aria-label="关闭">
          <X size={17} />
        </button>
        {modal === "providers" ? (
          <ProvidersModal />
        ) : modal === "mcp" ? (
          <McpModal />
        ) : modal === "settings" ? (
          <SettingsModal />
        ) : (
          <SearchModal />
        )}
      </section>
    </div>
  );
}

const modalTitle = {
  providers: "供应商管理",
  mcp: "MCP 服务",
  settings: "设置",
  search: "全局搜索",
};

function ModalHeader({ icon: Icon, title, subtitle }: { icon: LucideIcon; title: string; subtitle: string }) {
  return (
    <header className="modal-header">
      <div className="modal-icon"><Icon size={20} /></div>
      <div><h2>{title}</h2><p>{subtitle}</p></div>
    </header>
  );
}

function ProvidersModal() {
  const providers = useAppStore((state) => state.bootstrapData?.providers ?? []);
  const save = useAppStore((state) => state.saveProvider);
  const remove = useAppStore((state) => state.deleteProvider);
  const [selected, setSelected] = useState(providers[0]?.id ?? "");
  const existing = providers.find((provider) => provider.id === selected);
  const [draft, setDraft] = useState<ProviderProfileInput>(() => toInput(existing));
  const [showKey, setShowKey] = useState(false);
  const [fetching, setFetching] = useState(false);
  const [saving, setSaving] = useState(false);
  const [result, setResult] = useState("");
  const [manualModel, setManualModel] = useState("");
  const [testingModels, setTestingModels] = useState<Set<string>>(() => new Set());
  const [modelTests, setModelTests] = useState<Record<string, DraftModelTestResult | { ok: false; error: string }>>({});

  const choose = (id: string) => {
    const profile = providers.find((item) => item.id === id);
    setSelected(id); setDraft(toInput(profile)); setResult(""); setManualModel(""); setModelTests({}); setTestingModels(new Set());
  };
  const create = () => { setSelected(""); setDraft(toInput()); setResult(""); setManualModel(""); setModelTests({}); setTestingModels(new Set()); };

  const fetchModels = async () => {
    if (!draft.baseUrl.trim()) { setResult("请先填写 Base URL。"); return; }
    setFetching(true); setResult("");
    try {
      const upstream = await discoverProviderModelsDraft(draft.apiType, draft.baseUrl.trim(), draft.apiKey);
      setDraft((current) => {
        const existingById = new Map(current.models.map((model) => [model.modelId, model]));
        const merged = [...current.models];
        for (const model of upstream) {
          const previous = existingById.get(model.id);
          if (previous) {
            const index = merged.findIndex((item) => item.modelId === model.id);
            merged[index] = { ...previous, displayName: model.displayName, contextWindowTokens: previous.contextWindowTokens ?? model.contextWindow ?? null };
          } else {
            merged.push({ modelId: model.id, displayName: model.displayName, contextWindowTokens: model.contextWindow ?? null, source: "upstream" });
          }
        }
        return { ...current, models: merged, defaultModel: merged[0]?.modelId ?? "" };
      });
      setResult(`已从上游获取 ${upstream.length} 个模型；手动模型和上下文长度已保留。`);
    } catch (error) { setResult(`获取失败：${String(error)}。当前模型列表未改变，可继续手动添加。`); }
    finally { setFetching(false); }
  };

  const addModel = () => {
    const modelId = manualModel.trim();
    if (!modelId) return;
    if (draft.models.some((item) => item.modelId === modelId)) { setResult("该模型已存在。"); return; }
    const models: ProviderModelInput[] = [...draft.models, { modelId, displayName: modelId, contextWindowTokens: null, source: "manual" }];
    setDraft((current) => ({ ...current, models, defaultModel: current.defaultModel || modelId }));
    setManualModel(""); setResult("");
  };
  const updateContext = (modelId: string, value: string) => {
    const parsed = value.trim() === "" ? null : Number(value);
    if (parsed != null && (!Number.isFinite(parsed) || parsed <= 0 || !/^\d+(?:\.\d{0,4})?$/.test(value))) return;
    setDraft((current) => ({ ...current, models: current.models.map((model) => model.modelId === modelId ? { ...model, contextWindowTokens: parsed == null ? null : Math.round(parsed * 10_000) } : model) }));
  };
  const deleteModel = (modelId: string) => setDraft((current) => {
    const models = current.models.filter((model) => model.modelId !== modelId);
    return { ...current, models, defaultModel: current.defaultModel === modelId ? models[0]?.modelId ?? "" : current.defaultModel };
  });

  const testModel = async (modelId: string) => {
    if (!draft.baseUrl.trim()) { setResult("请先填写 Base URL。"); return; }
    setTestingModels((current) => new Set(current).add(modelId));
    setModelTests((current) => { const next = { ...current }; delete next[modelId]; return next; });
    try {
      const tested = await testProviderModelDraft(draft.id, draft.apiType, draft.baseUrl.trim(), draft.apiKey, modelId);
      setModelTests((current) => ({ ...current, [modelId]: tested }));
    } catch (error) {
      setModelTests((current) => ({ ...current, [modelId]: { ok: false, error: String(error) } }));
    } finally {
      setTestingModels((current) => { const next = new Set(current); next.delete(modelId); return next; });
    }
  };

  const persist = async () => {
    if (!draft.name.trim()) { setResult("请填写供应商显示名称。"); return; }
    if (!draft.baseUrl.trim()) { setResult("请填写 Base URL。"); return; }
    if (!draft.models.length) { setResult("请从上游获取或手动添加至少一个模型。"); return; }
    setSaving(true); setResult("");
    try {
      const normalized = { ...draft, name: draft.name.trim(), baseUrl: draft.baseUrl.trim().replace(/\/$/, ""), defaultModel: draft.models[0].modelId };
      const profile = await save(normalized); setSelected(profile.id); setDraft(toInput(profile)); setResult("供应商已保存。");
    } catch (error) { setResult(String(error)); } finally { setSaving(false); }
  };
  const destroy = async () => {
    if (!draft.id || !window.confirm(`删除供应商“${draft.name}”？关联凭据也会一并删除。`)) return;
    try { await remove(draft.id); const next = providers.find((item) => item.id !== draft.id); if (next) choose(next.id); else create(); setResult("供应商已删除。"); } catch (error) { setResult(String(error)); }
  };

  return <>
    <ModalHeader icon={Server} title="供应商与模型" subtitle="配置 Responses API 或 Chat Completions；密钥只存入系统凭据管理器。" />
    <div className="settings-layout provider-settings">
      <nav className="settings-nav" aria-label="供应商列表">
        <button className="add-provider" onClick={create}><Plus size={15}/>添加供应商</button>
        {!providers.length && <div className="settings-empty"><Server size={20}/><strong>尚无供应商</strong><span>添加后才能发送任务</span></div>}
        {providers.map((provider) => <button key={provider.id} className={selected === provider.id ? "active" : ""} onClick={() => choose(provider.id)}>
          <span className="provider-list-icon"><Server size={15}/></span><span><strong>{provider.name}</strong><small>{apiTypeLabel(provider.apiType)} · {provider.models.length} 个模型</small><small className="provider-url">{provider.baseUrl}</small></span><ChevronRight size={14}/>
        </button>)}
      </nav>
      <div className="settings-form provider-form">
        {existing?.legacy ? <div className="legacy-provider-note"><strong>旧版兼容配置</strong><p>此配置使用原生 {existing.kind} 适配器并保持只读，避免保存时改变现有请求行为或凭据。</p><dl><dt>Base URL</dt><dd>{existing.baseUrl}</dd><dt>模型</dt><dd>{existing.models.map((model) => model.modelId).join("、") || existing.defaultModel}</dd></dl></div> : <>
          <label>供应商显示名称<input value={draft.name} onChange={(event) => setDraft((value) => ({ ...value, name: event.target.value }))} placeholder="例如：公司网关" autoFocus={!draft.id}/></label>
          <div className="provider-api-type"><span>API 类型</span><div className="segmented" role="group" aria-label="API 类型">{(["responses", "chat-completions"] as ProviderApiType[]).map((type) => <button key={type} className={draft.apiType === type ? "active" : ""} onClick={() => setDraft((value) => ({ ...value, apiType: type }))}>{apiTypeLabel(type)}</button>)}</div></div>{draft.apiType === "responses" && <p className="api-type-note">Responses API 仅适用于明确支持 <code>/responses</code> 的服务；大多数 OpenAI 兼容服务请选 Chat Completions。</p>}
          <label>Base URL<input value={draft.baseUrl} onChange={(event) => setDraft((value) => ({ ...value, baseUrl: event.target.value }))} placeholder="https://api.example.com/v1" spellCheck={false}/></label>
          <label>API Key<div className="secret-input"><input type={showKey ? "text" : "password"} value={draft.apiKey ?? ""} onChange={(event) => setDraft((value) => ({ ...value, apiKey: event.target.value }))} placeholder={existing?.hasCredential ? "已保存；留空表示不修改" : "sk-..."} autoComplete="off"/><button onClick={() => setShowKey((value) => !value)} aria-label={showKey ? "隐藏密钥" : "显示密钥"}>{showKey ? <EyeOff size={16}/> : <Eye size={16}/>}</button></div></label>
          <section className="provider-model-section"><header><div><strong>模型列表</strong><span>上下文单位统一为万 Token</span></div><button className="secondary" onClick={() => void fetchModels()} disabled={fetching || !draft.baseUrl.trim()}>{fetching ? <LoaderCircle className="spin" size={14}/> : <Download size={14}/>}从上游获取模型</button></header>
            <div className="model-table"><div className="model-table-head"><span>模型 ID</span><span>上下文（万 Token）</span><span>可用性</span><span/></div>{draft.models.map((model) => { const tested = modelTests[model.modelId]; const testing = testingModels.has(model.modelId); return <div className="model-entry" key={model.modelId}><div className="model-row"><code title={model.modelId}>{model.modelId}</code><input aria-label={`${model.modelId} 上下文长度`} type="number" min="0.0001" step="0.0001" value={toWan(model.contextWindowTokens)} onChange={(event) => updateContext(model.modelId, event.target.value)} placeholder="未知"/><button className="model-test-button" onClick={() => void testModel(model.modelId)} disabled={testing || !draft.baseUrl.trim()} aria-label={`测试模型 ${model.modelId}`}>{testing ? <LoaderCircle className="spin" size={14}/> : <TestTube2 size={14}/>}<span>{testing ? "测试中" : "测试"}</span></button><button className="model-delete-button" onClick={() => deleteModel(model.modelId)} aria-label={`删除模型 ${model.modelId}`}><Trash2 size={15}/></button></div>{tested && <div className={`model-test-result ${tested.ok ? "success" : "failed"}`}>{tested.ok ? <>可用 · {tested.latencyMs}ms · {tested.responsePreview}</> : <>不可用 · {"error" in tested ? tested.error : "未知错误"}</>}</div>}</div>; })}{!draft.models.length && <div className="model-empty">尚无模型，请从上游获取或手动添加。</div>}</div>
            <div className="manual-model"><input value={manualModel} onChange={(event) => setManualModel(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") { event.preventDefault(); addModel(); } }} placeholder="手动输入模型 ID"/><button className="secondary" onClick={addModel}><Plus size={14}/>添加模型</button></div>
          </section>
        </>}
        {result && <div className="form-result" role="status">{result}</div>}
        <div className="form-actions sticky-actions">{draft.id && <button className="danger" onClick={() => void destroy()}><Trash2 size={14}/>删除</button>}<span/><button className="ghost" onClick={create}>新建草稿</button>{!existing?.legacy && <button className="accent" onClick={() => void persist()} disabled={saving}>{saving ? "保存中…" : "保存供应商"}</button>}</div>
      </div>
    </div>
  </>;
}

function McpModal() {
  const servers = useAppStore((state) => state.bootstrapData?.mcpServers ?? []);
  const activeProjectId = useAppStore((state) => state.activeProjectId);
  const save = useAppStore((state) => state.saveMcp);
  const remove = useAppStore((state) => state.deleteMcp);
  const [selected, setSelected] = useState(servers[0]?.id ?? "");
  const [draft, setDraft] = useState<McpServerConfig>(() => servers[0] ?? newMcp(activeProjectId));
  const [envText, setEnvText] = useState(() => prettyJson(servers[0]?.env ?? {}));
  const [headerText, setHeaderText] = useState(() => prettyJson(servers[0]?.headers ?? {}));
  const [result, setResult] = useState("");
  const [testing, setTesting] = useState(false);
  const [saving, setSaving] = useState(false);

  const choose = (id: string) => {
    const server = servers.find((item) => item.id === id);
    if (!server) return;
    setSelected(id); setDraft(server); setEnvText(prettyJson(server.env)); setHeaderText(prettyJson(server.headers)); setResult("");
  };

  const create = () => {
    setSelected(""); setDraft(newMcp(activeProjectId)); setEnvText("{}"); setHeaderText("{}"); setResult("");
  };

  const persist = async (): Promise<McpServerConfig | null> => {
    setSaving(true); setResult("");
    try {
      const input = {
        ...draft,
        projectId: draft.scope === "project" ? activeProjectId : null,
        env: parseStringMap(envText, "环境变量"),
        headers: parseStringMap(headerText, "HTTP Header"),
      };
      const server = await save(input);
      setSelected(server.id); setDraft(server); setEnvText(prettyJson(server.env)); setHeaderText(prettyJson(server.headers));
      setResult("MCP 配置已保存；敏感值已转换为系统凭据引用。");
      return server;
    } catch (error) {
      setResult(String(error));
      return null;
    } finally { setSaving(false); }
  };

  const test = async () => {
    setTesting(true); setResult("");
    try {
      const server = await persist();
      if (!server) return;
      const response = await testMcpServer(server);
      const updated = { ...server, status: "healthy", lastError: null, discoveredTools: response.tools, readOnlyTools: response.readOnlyTools };
      setDraft(updated);
      await save(updated);
      setResult(`${response.message} · ${response.latencyMs} ms · ${response.tools.length} 个工具`);
    } catch (error) {
      setResult(String(error));
      setDraft((value) => ({ ...value, status: "error", lastError: String(error) }));
    } finally { setTesting(false); }
  };

  const destroy = async () => {
    if (!draft.id || !window.confirm(`删除 MCP 服务“${draft.name}”？关联凭据也会一并移除。`)) return;
    try {
      await remove(draft.id);
      const next = servers.find((item) => item.id !== draft.id);
      if (next) choose(next.id); else create();
      setResult("MCP 服务已删除。");
    } catch (error) { setResult(String(error)); }
  };

  const toggleTool = (tool: string) => {
    setDraft((current) => ({
      ...current,
      disabledTools: current.disabledTools.includes(tool)
        ? current.disabledTools.filter((item) => item !== tool)
        : [...current.disabledTools, tool],
    }));
  };

  return (
    <>
      <ModalHeader icon={Waypoints} title="MCP 服务" subtitle="管理全局或项目级 tools、resources 和 prompts；MVP 支持 stdio 与 Streamable HTTP。" />
      <div className="settings-layout">
        <nav className="settings-nav" aria-label="MCP 服务列表">
          <button className="add-provider" onClick={create}><Plus size={15} />添加 MCP</button>
          {servers.map((server) => <button key={server.id} className={selected === server.id ? "active" : ""} onClick={() => choose(server.id)}><span className={`mcp-status ${server.status}`} /><span><strong>{server.name}</strong><small>{server.transport} · {server.scope === "global" ? "全局" : "项目"}</small></span><ChevronRight size={14} /></button>)}
        </nav>
        <div className="settings-form">
          <div className="form-row">
            <label>名称<input value={draft.name} onChange={(event) => setDraft((value) => ({ ...value, name: event.target.value }))} /></label>
            <div className="form-field">
              <span>范围</span>
              <Dropdown
                ariaLabel="MCP 生效范围"
                value={draft.scope}
                className="settings-dropdown"
                options={[
                  { value: "global", label: "全局" },
                  { value: "project", label: "当前项目" },
                ]}
                onChange={(scope) => setDraft((value) => ({ ...value, scope }))}
              />
            </div>
          </div>
          <div className="segmented"><button className={draft.transport === "stdio" ? "active" : ""} onClick={() => setDraft((value) => ({ ...value, transport: "stdio" }))}>stdio</button><button className={draft.transport === "streamable-http" ? "active" : ""} onClick={() => setDraft((value) => ({ ...value, transport: "streamable-http" }))}>Streamable HTTP</button></div>
          {draft.transport === "stdio" ? <>
            <label>Command<input value={draft.command ?? ""} onChange={(event) => setDraft((value) => ({ ...value, command: event.target.value }))} placeholder="npx" /></label>
            <label>Arguments<input value={draft.args.join(" ")} onChange={(event) => setDraft((value) => ({ ...value, args: event.target.value.split(/\s+/).filter(Boolean) }))} placeholder="-y @modelcontextprotocol/server-filesystem" /></label>
            <label>Working directory<input value={draft.cwd ?? ""} onChange={(event) => setDraft((value) => ({ ...value, cwd: event.target.value || null }))} /></label>
            <label>环境变量（JSON）<textarea className="json-editor" spellCheck={false} value={envText} onChange={(event) => setEnvText(event.target.value)} /></label>
          </> : <>
            <label>Endpoint URL<input value={draft.url ?? ""} onChange={(event) => setDraft((value) => ({ ...value, url: event.target.value }))} placeholder="https://mcp.example.com/mcp" /></label>
            <label>HTTP Header（JSON）<textarea className="json-editor" spellCheck={false} value={headerText} onChange={(event) => setHeaderText(event.target.value)} /></label>
          </>}
          <div className="form-row"><label>超时（秒）<input type="number" min="3" value={draft.timeoutSeconds} onChange={(event) => setDraft((value) => ({ ...value, timeoutSeconds: Number(event.target.value) }))} /></label><label className="compact-switch"><span>启用</span><input type="checkbox" checked={draft.enabled} onChange={(event) => setDraft((value) => ({ ...value, enabled: event.target.checked }))} /></label></div>
          {draft.discoveredTools.length ? <section className="mcp-tools"><header><strong>已发现工具</strong><small>{draft.discoveredTools.length - draft.disabledTools.length}/{draft.discoveredTools.length} 已启用</small></header>{draft.discoveredTools.map((tool) => <label key={tool}><code>{tool}</code>{(draft.readOnlyTools ?? []).includes(tool) ? <small>Plan 可用</small> : null}<input type="checkbox" checked={!draft.disabledTools.includes(tool)} onChange={() => toggleTool(tool)} /></label>)}</section> : null}
          {result ? <div className={`test-result ${result.includes("正常") || result.includes("已保存") ? "success" : ""}`}>{result}</div> : null}
          <div className="form-actions"><button className="danger-ghost" onClick={() => void destroy()} disabled={!draft.id}><Trash2 size={15} />删除</button><span className="spacer" /><button className="ghost" onClick={() => void test()} disabled={testing || saving}><TestTube2 size={15} />{testing ? "测试中" : "保存并测试"}</button><button className="accent" onClick={() => void persist()} disabled={saving}><Check size={15} />{saving ? "保存中" : "保存"}</button></div>
        </div>
      </div>
    </>
  );
}

function newMcp(projectId: string | null): McpServerConfig {
  return { id: "", name: "新 MCP 服务", scope: "global", projectId, transport: "stdio", command: "npx", args: [], cwd: null, url: null, env: {}, headers: {}, timeoutSeconds: 30, enabled: true, status: "stopped", lastError: null, discoveredTools: [], disabledTools: [], readOnlyTools: [], updatedAt: new Date().toISOString() };
}

function SettingsModal() {
  const settings = useAppStore((state) => state.bootstrapData?.settings);
  if (!settings) return null;
  const update = async (theme: "system" | "light" | "dark") => {
    const next = { ...settings, theme };
    await saveSettings(next);
    useAppStore.setState((state) => ({ bootstrapData: state.bootstrapData ? { ...state.bootstrapData, settings: next } : null }));
    document.documentElement.dataset.theme = theme;
  };
  return <><ModalHeader icon={Monitor} title="设置" subtitle="Axiom 默认不发送遥测，诊断数据仅保存在本机。" /><div className="simple-settings"><section><h3>外观</h3><div className="theme-picker"><button className={settings.theme === "system" ? "active" : ""} onClick={() => void update("system")}><Monitor size={18} /><span>跟随系统</span></button><button className={settings.theme === "light" ? "active" : ""} onClick={() => void update("light")}><Sun size={18} /><span>浅色</span></button><button className={settings.theme === "dark" ? "active" : ""} onClick={() => void update("dark")}><Moon size={18} /><span>深色</span></button></div></section><section><h3>隐私</h3><div className="privacy-card"><Cloud size={18} /><div><strong>完全本地的数据层</strong><p>项目、对话、运行事件和配置保存在本机 SQLite。只有你配置的供应商和远程 MCP 会产生网络请求。</p></div><span className="local-badge">LOCAL</span></div></section><section><h3>关于</h3><div className="about-row"><div className="axiom-mark"><i /><i /><i /></div><div><strong>Axiom 1.0.3</strong><span>Apache-2.0 · Windows 优先</span></div><button className="secondary check-update-button" onClick={() => window.dispatchEvent(new Event("axiom-check-update"))}>检查更新</button></div></section></div></>;
}

function SearchModal() {
  const threads = useAppStore((state) => state.bootstrapData?.threads ?? []);
  const projects = useAppStore((state) => state.bootstrapData?.projects ?? []);
  const select = useAppStore((state) => state.selectThread);
  const close = useAppStore((state) => state.setModal);
  const [query, setQuery] = useState("");
  const results = useMemo(() => threads.filter((thread) => thread.title.toLowerCase().includes(query.toLowerCase())), [threads, query]);
  return <div className="search-modal-content"><div className="global-search"><Search size={19} /><input autoFocus value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索任务和消息…" /><kbd>ESC</kbd></div><div className="search-results">{results.map((thread) => <button key={thread.id} onClick={() => { void select(thread.id); close(null); }}><span><strong>{thread.title}</strong><small>{projects.find((project) => project.id === thread.projectId)?.name}</small></span><ChevronRight size={15} /></button>)}{!results.length ? <div className="no-results">没有匹配的任务</div> : null}</div></div>;
}

function toInput(profile?: ProviderProfile): ProviderProfileInput {
  return profile ? {
    id: profile.id, kind: profile.kind, name: profile.name, baseUrl: profile.baseUrl, defaultModel: profile.models[0]?.modelId ?? profile.defaultModel,
    enabled: true, timeoutSeconds: 120, extraHeaders: {}, apiKey: "", apiType: profile.apiType,
    models: profile.models.filter((model) => model.source !== "legacy").map((model) => ({ modelId: model.modelId, displayName: model.displayName, contextWindowTokens: model.contextWindowTokens ?? null, source: model.source === "upstream" ? "upstream" : "manual" })),
  } : { kind: "open-ai-compatible", name: "", baseUrl: "", defaultModel: "", enabled: true, timeoutSeconds: 120, extraHeaders: {}, apiKey: "", apiType: "chat-completions", models: [] };
}

function apiTypeLabel(type: ProviderApiType): string { return type === "responses" ? "Responses API" : "Chat Completions"; }
function toWan(tokens?: number | null): string { return tokens == null ? "" : String(tokens / 10_000); }

function prettyJson(value: Record<string, string>): string {
  return JSON.stringify(value, null, 2);
}

function parseStringMap(value: string, label: string): Record<string, string> {
  let parsed: unknown;
  try { parsed = JSON.parse(value || "{}"); } catch { throw new Error(`${label} 必须是有效 JSON。`); }
  if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") throw new Error(`${label} 必须是 JSON 对象。`);
  for (const [key, item] of Object.entries(parsed)) {
    if (typeof item !== "string") throw new Error(`${label}.${key} 必须是字符串。`);
  }
  return parsed as Record<string, string>;
}
