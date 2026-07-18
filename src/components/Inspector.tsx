import { lazy, Suspense, useEffect, useState } from "react";
import {
  Braces,
  Check,
  ChevronRight,
  ExternalLink,
  File,
  FileCode2,
  Folder,
  GitCompareArrows,
  LoaderCircle,
  PanelRightClose,
  RefreshCw,
  Search,
  Terminal,
  TextSearch,
  Undo2,
} from "lucide-react";
import { saveSettings } from "../lib/api";
import type { FileEntry, InspectorTab } from "../lib/types";
import { useAppStore } from "../store/appStore";
import { resolveContextLimit } from "../lib/context";

const LazyDiffView = lazy(() => import("./DiffView"));
const LazyTerminalPanel = lazy(() => import("./TerminalPanel"));

const tabs: { id: InspectorTab; label: string; icon: typeof File }[] = [
  { id: "changes", label: "Changes", icon: GitCompareArrows },
  { id: "files", label: "Files", icon: FileCode2 },
  { id: "terminal", label: "Terminal", icon: Terminal },
  { id: "context", label: "Context", icon: Braces },
];

export function Inspector() {
  const detail = useAppStore((state) => state.threadDetail);
  const tab = useAppStore((state) => state.inspectorTab);
  const setTab = useAppStore((state) => state.setInspectorTab);
  const refresh = useAppStore((state) => state.refreshInspector);
  const open = useAppStore((state) => state.inspectorOpen);
  const setOpen = useAppStore((state) => state.setInspectorOpen);
  const settings = useAppStore((state) => state.bootstrapData?.settings);
  const changedCount = useAppStore((state) => state.git?.changedFiles.length ?? 0);
  const [width, setWidth] = useState(() => settings?.inspectorWidth ?? 420);

  useEffect(() => {
    if (settings?.inspectorWidth) setWidth(settings.inspectorWidth);
  }, [settings?.inspectorWidth]);

  const startResize = (event: React.PointerEvent) => {
    event.currentTarget.setPointerCapture?.(event.pointerId);
    const startX = event.clientX;
    const startWidth = width;
    let nextWidth = startWidth;
    const move = (pointerEvent: PointerEvent) => {
      nextWidth = Math.max(340, Math.min(620, startWidth + startX - pointerEvent.clientX));
      setWidth(nextWidth);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      const state = useAppStore.getState();
      const data = state.bootstrapData;
      if (data) {
        const next = { ...data.settings, inspectorWidth: Math.round(nextWidth) };
        useAppStore.setState({ bootstrapData: { ...data, settings: next } });
        void saveSettings(next);
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  if (!open) return null;
  if (!detail) {
    return <aside className="inspector empty-inspector" style={{ width }}><div className="inspector-empty"><PanelRightClose size={20} /><span>检查器</span></div></aside>;
  }

  return (
    <aside className="inspector" style={{ width }}>
      <div className="resize-handle" role="separator" aria-orientation="vertical" aria-label="调整检查器宽度" onPointerDown={startResize} />
      <div className="inspector-tabs">
        {tabs.map((item) => {
          const Icon = item.icon;
          return <button key={item.id} className={tab === item.id ? "active" : ""} onClick={() => setTab(item.id)}><Icon size={14} />{item.label}{item.id === "changes" ? <span>{changedCount}</span> : null}</button>;
        })}
        <button className="refresh-inspector" onClick={() => void refresh()} aria-label="刷新"><RefreshCw size={14} /></button>
        <button className="close-inspector" onClick={() => setOpen(false)} aria-label="关闭检查器"><PanelRightClose size={15} /></button>
      </div>
      <div className="inspector-body">
        {tab === "changes" ? <ChangesPanel /> : tab === "files" ? <FilesPanel /> : tab === "terminal" ? (
          <Suspense fallback={<div className="terminal-loading"><LoaderCircle className="spin" size={15} />正在载入终端…</div>}>
            <LazyTerminalPanel />
          </Suspense>
        ) : <ContextPanel />}
      </div>
    </aside>
  );
}

function fileEntry(path: string): FileEntry {
  return { name: path.split(/[\/]/).at(-1) ?? path, path, isDirectory: false, size: 0 };
}

function ChangesPanel() {
  const git = useAppStore((state) => state.git);
  const setTab = useAppStore((state) => state.setInspectorTab);
  const selectFile = useAppStore((state) => state.selectFile);
  const openExternal = useAppStore((state) => state.openFileExternal);
  const restoreAll = useAppStore((state) => state.restoreLatestRun);
  const restoreFile = useAppStore((state) => state.restoreFileChange);
  const restoring = useAppStore((state) => state.restoringRunId);
  const restoringPath = useAppStore((state) => state.restoringFilePath);
  const restoreMessage = useAppStore((state) => state.restoreMessage);
  const [reviewed, setReviewed] = useState(false);

  if (!git?.changedFiles.length) {
    return <div className="changes-panel empty-changes"><Empty icon={Check} title="工作区没有变更" text="Agent 生成的修改会显示在这里。" />{restoreMessage ? <div className="restore-message"><Check size={14} />{restoreMessage}</div> : null}</div>;
  }

  const preview = (path: string) => {
    setTab("files");
    void selectFile(fileEntry(path));
  };

  return (
    <div className="changes-panel">
      <div className="panel-summary">
        <div><strong>{git.changedFiles.length} 个文件已更改</strong><span>{git.branch ? `分支 ${git.branch}` : "非 Git 项目"}</span></div>
        <div>
          <button title="撤销最近回合的全部内置文件变更" aria-label="撤销最近回合的全部内置文件变更" onClick={() => void restoreAll()} disabled={Boolean(restoring || restoringPath)}>{restoring ? <LoaderCircle className="spin" size={14} /> : <Undo2 size={14} />}</button>
          <button className={`review-button ${reviewed ? "reviewed" : ""}`} onClick={() => setReviewed((value) => !value)}>{reviewed ? "已审查" : "审查"}</button>
        </div>
      </div>
      {restoreMessage ? <div className="restore-message"><Check size={14} />{restoreMessage}</div> : null}
      <div className="changed-file-list">
        {git.changedFiles.map((file) => (
          <div className="changed-file-row" key={file.path}>
            <button className="changed-file-main" onClick={() => preview(file.path)} title={file.path}>
              <span className={`git-status status-${file.status.toLowerCase()}`}>{file.status || "M"}</span>
              <span>{file.path}</span>
              <ChevronRight size={13} />
            </button>
            <button className="changed-file-action" onClick={() => void openExternal(file.path)} title="在外部编辑器中打开" aria-label={`在外部编辑器中打开 ${file.path}`}><ExternalLink size={13} /></button>
            <button className="changed-file-action" onClick={() => void restoreFile(file.path)} title="撤销此文件的最近回合变更" aria-label={`撤销 ${file.path} 的最近回合变更`} disabled={Boolean(restoring || restoringPath)}>{restoringPath === file.path ? <LoaderCircle className="spin" size={13} /> : <Undo2 size={13} />}</button>
          </div>
        ))}
      </div>
      <Suspense fallback={<div className="diff-loading"><LoaderCircle className="spin" size={15} />正在载入 Diff…</div>}>
        <LazyDiffView diff={git.diff} />
      </Suspense>
    </div>
  );
}

function FilesPanel() {
  const files = useAppStore((state) => state.files);
  const selected = useAppStore((state) => state.selectedFile);
  const content = useAppStore((state) => state.selectedFileContent);
  const selectFile = useAppStore((state) => state.selectFile);
  const openExternal = useAppStore((state) => state.openFileExternal);
  const searchFiles = useAppStore((state) => state.searchFiles);
  const matches = useAppStore((state) => state.searchMatches);
  const searching = useAppStore((state) => state.searchingFiles);
  const [query, setQuery] = useState("");
  const visible = files.filter((file) => file.name.toLowerCase().includes(query.toLowerCase()));

  useEffect(() => {
    const timer = window.setTimeout(() => void searchFiles(query), 180);
    return () => window.clearTimeout(timer);
  }, [query, searchFiles]);

  const openMatch = (path: string) => void selectFile(fileEntry(path));

  return (
    <div className="files-panel">
      <div className="file-search"><Search size={14} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索工作区文件内容" />{searching ? <LoaderCircle className="spin" size={13} /> : null}</div>
      {query.trim().length >= 2 ? (
        <div className="search-match-list">{matches.map((match) => <button key={`${match.path}:${match.line}:${match.column}`} onClick={() => openMatch(match.path)}><span><strong>{match.path}</strong><small>{match.line}:{match.column}</small></span><code>{match.preview}</code></button>)}{!searching && !matches.length ? <div className="no-search-matches">没有匹配结果</div> : null}</div>
      ) : (
        <div className="file-browser">{visible.map((file) => <button key={file.path} className={selected?.path === file.path ? "active" : ""} onClick={() => void selectFile(file)}>{file.isDirectory ? <Folder size={15} /> : <File size={15} />}<span>{file.name}</span>{file.isDirectory ? <ChevronRight size={13} /> : null}</button>)}</div>
      )}
      {selected && !selected.isDirectory ? (
        <div className="file-preview">
          <div className="file-preview-header"><strong>{selected.name}</strong><small>{selected.size ? formatBytes(selected.size) : "只读预览"}</small><button onClick={() => void openExternal(selected.path)} title="在外部编辑器中打开" aria-label={`在外部编辑器中打开 ${selected.name}`}><ExternalLink size={14} /></button></div>
          <pre><code>{content}</code></pre>
        </div>
      ) : null}
    </div>
  );
}

function ContextPanel(){
  const detail=useAppStore(s=>s.threadDetail);
  const providers=useAppStore(s=>s.bootstrapData?.providers??[]);
  const providerId=useAppStore(s=>s.providerId);
  const modelId=useAppStore(s=>s.modelId);
  const liveRecords=useAppStore(s=>s.contextRecords);
  const restoreSnapshot=useAppStore(s=>s.restoreContextSnapshot);
  const [restoringId,setRestoringId]=useState<string|null>(null);
  const latestRun=detail?.runs.at(-1);
  const latest=latestRun?.usage;
  const context=latest?.contextTokens??0;
  const limit=Math.max(1,resolveContextLimit(providers,providerId,modelId,latestRun));
  const pct=Math.min(100,Math.round(context/limit*100));
  const snapshots=detail?.contextSnapshots??[];
  const compressionCount=Math.max(liveRecords.length,snapshots.length);
  const segments=[
    {name:"指令与工具定义",tokens:Math.min(context,6240),color:"#6b5cff"},
    {name:"对话消息",tokens:Math.max(0,Math.round(context*.54)),color:"#369b73"},
    {name:"文件引用",tokens:Math.round(context*.2),color:"#d78b28"},
    {name:"工具结果",tokens:Math.round(context*.1),color:"#3f8ad8"},
    {name:"输出预留",tokens:Math.min(8192,limit),color:"#a0a0a0"},
  ];
  const restore=async(id:string)=>{setRestoringId(id);try{await restoreSnapshot(id)}finally{setRestoringId(null)}};
  return <div className="context-panel">
    <div className={`context-hero ${pct>=85?"critical":pct>=75?"warning":""}`}>
      <div className="large-context-ring" style={{"--context":`${pct*3.6}deg`} as React.CSSProperties}><strong>{pct}%</strong><span>占用</span></div>
      <div><strong>{context.toLocaleString()}</strong><span>/ {limit.toLocaleString()} tokens</span><small>{pct>=85?"下一回合将自动压缩":pct>=75?"接近上下文上限":"上下文空间充足"}</small></div>
    </div>
    <div className="context-list">{segments.map(item=><div key={item.name}><i style={{background:item.color}}/><span>{item.name}</span><strong>{item.tokens.toLocaleString()}</strong></div>)}</div>
    <small className="context-estimate-note">分类数据为本地估算；总占用优先使用供应商 Usage。</small>
    <div className="context-card"><TextSearch size={16}/><div><strong>透明压缩</strong><p>75% 开始预警；达到 85% 后，会在下一回合前保存可恢复的压缩检查点。</p>{compressionCount>0&&<small>已保存 {compressionCount} 个压缩检查点</small>}</div></div>
    {snapshots.length>0&&<section className="context-checkpoints" aria-label="上下文压缩检查点">
      <h3>压缩检查点</h3>
      {snapshots.map(snapshot=><details key={snapshot.id} className={`context-checkpoint ${snapshot.active?"active":""}`}>
        <summary><span><strong>{snapshot.active?"当前生效":"已恢复"}</strong><small>{new Date(snapshot.createdAt).toLocaleString("zh-CN")} · {snapshot.sourceMessageIds.length} 条消息</small></span><ChevronRight size={14}/></summary>
        <pre>{snapshot.summary}</pre>
        {snapshot.active&&<button onClick={()=>void restore(snapshot.id)} disabled={restoringId===snapshot.id}>{restoringId===snapshot.id?<LoaderCircle className="spin" size={13}/>:<Undo2 size={13}/>}恢复压缩前上下文</button>}
      </details>)}
    </section>}
    <div className="context-card muted"><Braces size={16}/><div><strong>累计消耗</strong><p>整个对话累计使用 {(latest?.cumulativeTokens??context).toLocaleString()} tokens。</p></div></div>
  </div>;
}

function Empty({icon:Icon,title,text}:{icon:typeof Check;title:string;text:string}){return <div className="panel-empty"><Icon size={22}/><strong>{title}</strong><span>{text}</span></div>}

function formatBytes(value:number){if(value<1024)return `${value} B`;if(value<1024*1024)return `${(value/1024).toFixed(1)} KB`;return `${(value/1024/1024).toFixed(1)} MB`}
