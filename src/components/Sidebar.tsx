import { Archive, ArchiveRestore, ChevronDown, ChevronRight, CircleDot, Folder, FolderGit2, PanelLeftClose, PanelLeftOpen, Plus, Search, Settings, SlidersHorizontal, Star, Trash2, Waypoints } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useAppStore } from "../store/appStore";
import { saveSettings } from "../lib/api";
import type { Project, ThreadSummary } from "../lib/types";

const statusClass=(status:string)=>status==="awaiting-approval"?"approval":(["queued","reasoning","streaming","tool-running"].includes(status)?"running":status);

export function Sidebar(){
  const data=useAppStore(s=>s.bootstrapData);
  const activeProjectId=useAppStore(s=>s.activeProjectId);
  const activeThreadId=useAppStore(s=>s.activeThreadId);
  const selectThread=useAppStore(s=>s.selectThread);
  const addProject=useAppStore(s=>s.addProject);
  const createThread=useAppStore(s=>s.createThread);
  const archiveThread=useAppStore(s=>s.archiveThread);
  const deleteThread=useAppStore(s=>s.deleteThread);
  const setModal=useAppStore(s=>s.setModal);
  const setSidebarOverlayOpen=useAppStore(s=>s.setSidebarOverlayOpen);
  const settings=useAppStore(s=>s.bootstrapData?.settings);
  const [width,setWidth]=useState(()=>Math.max(228,Math.min(320,settings?.sidebarWidth??272)));
  const collapsed=settings?.sidebarCollapsed??false;
  useEffect(()=>{setWidth(Math.max(228,Math.min(320,settings?.sidebarWidth??272)))},[settings?.sidebarWidth]);
  const persistSettings=(patch:Partial<NonNullable<typeof settings>>)=>{const state=useAppStore.getState();const data=state.bootstrapData;if(!data)return;const nextSettings={...data.settings,...patch};useAppStore.setState({bootstrapData:{...data,settings:nextSettings}});void saveSettings(nextSettings);};
  const persistCollapsed=(next:boolean)=>persistSettings({sidebarCollapsed:next});
  const startResize=(event:React.PointerEvent)=>{event.preventDefault();const startX=event.clientX,startWidth=width;let nextWidth=startWidth;const move=(e:PointerEvent)=>{nextWidth=Math.max(228,Math.min(320,startWidth+e.clientX-startX));setWidth(nextWidth)};const up=()=>{window.removeEventListener("pointermove",move);window.removeEventListener("pointerup",up);const rounded=Math.round(nextWidth);setWidth(rounded);persistSettings({sidebarWidth:rounded})};window.addEventListener("pointermove",move);window.addEventListener("pointerup",up,{once:true})};
  const [expanded,setExpanded]=useState<Record<string,boolean>>({});
  const [showArchived,setShowArchived]=useState<Record<string,boolean>>({});
  const grouped=useMemo(()=>new Map((data?.projects??[]).map(project=>[project,(data?.threads??[]).filter(t=>t.projectId===project.id)])),[data]);
  return <aside className={`sidebar ${collapsed?"collapsed":""}`} style={collapsed?undefined:{width,minWidth:width}}>
    <div className="brand-row"><div className="axiom-mark"><i/><i/><i/></div><strong>Axiom</strong><button className="sidebar-collapse" onClick={()=>window.innerWidth<900?setSidebarOverlayOpen(false):persistCollapsed(!collapsed)} aria-label={collapsed?"展开侧栏":"折叠侧栏"} title={collapsed?"展开侧栏":"折叠侧栏"}>{collapsed?<PanelLeftOpen size={16}/>:<PanelLeftClose size={16}/>}</button></div>
    <div className="sidebar-actions">
      <button className="primary-nav" onClick={()=>{setSidebarOverlayOpen(false);void createThread();}}><Plus size={17}/><span>新建任务</span><kbd>Ctrl N</kbd></button>
      <button onClick={()=>{setSidebarOverlayOpen(false);setModal("search");}}><Search size={17}/><span>搜索</span><kbd>Ctrl K</kbd></button>
    </div>
    <div className="sidebar-section-title"><span>项目</span><button onClick={()=>{setSidebarOverlayOpen(false);void addProject();}} aria-label="添加项目"><Plus size={15}/></button></div>
    <div className="project-list">
      {[...grouped.entries()].map(([project,threads])=><ProjectGroup key={project.id} project={project} threads={threads} activeProjectId={activeProjectId} activeThreadId={activeThreadId} expanded={expanded[project.id]??project.id===activeProjectId} showArchived={showArchived[project.id]??false} onToggle={()=>setExpanded(v=>({...v,[project.id]:!(v[project.id]??project.id===activeProjectId)}))} onToggleArchived={()=>setShowArchived(v=>({...v,[project.id]:!v[project.id]}))} onSelect={id=>{setSidebarOverlayOpen(false);void selectThread(id)}} onArchive={(id,archived)=>void archiveThread(id,archived)} onDelete={id=>{if(window.confirm("永久删除这个任务及其消息、运行记录和附件关联？此操作无法撤销。"))void deleteThread(id)}}/>) }
      {!data?.projects.length&&<button className="empty-project" onClick={()=>void addProject()}><FolderGit2 size={22}/><span>打开一个代码项目</span><small>选择本地文件夹开始</small></button>}
    </div>
    <div className="sidebar-footer">
      <button onClick={()=>{setSidebarOverlayOpen(false);setModal("providers");}}><SlidersHorizontal size={17}/><span>供应商与模型</span><span className="footer-count">{data?.providers.filter(p=>p.enabled).length??0}</span></button>
      <button onClick={()=>{setSidebarOverlayOpen(false);setModal("mcp");}}><Waypoints size={17}/><span>MCP 服务</span><span className="health-dot"/></button>
      <button onClick={()=>{setSidebarOverlayOpen(false);setModal("settings");}}><Settings size={17}/><span>设置</span></button>
    </div>
    {!collapsed&&<div className="sidebar-resize-handle" role="separator" aria-orientation="vertical" aria-label="调整侧栏宽度" onPointerDown={startResize}/>}
  </aside>;
}

function ProjectGroup({project,threads,activeProjectId,activeThreadId,expanded,showArchived,onToggle,onToggleArchived,onSelect,onArchive,onDelete}:{project:Project;threads:ThreadSummary[];activeProjectId:string|null;activeThreadId:string|null;expanded:boolean;showArchived:boolean;onToggle:()=>void;onToggleArchived:()=>void;onSelect:(id:string)=>void;onArchive:(id:string,archived:boolean)=>void;onDelete:(id:string)=>void}){
  const archivedCount=threads.filter(thread=>thread.archived).length;
  const visible=threads.filter(thread=>Boolean(thread.archived)===showArchived);
  return <div className={`project-group ${project.id===activeProjectId?"active-project":""}`}>
    <button className="project-row" onClick={onToggle}>{expanded?<ChevronDown size={14}/>:<ChevronRight size={14}/>}<Folder size={16}/><span>{project.name}</span>{project.favorite&&<Star size={12} fill="currentColor"/>}</button>
    {expanded&&<div className="thread-list">
      {visible.map(thread=><div className={`thread-item ${thread.id===activeThreadId?"active":""}`} key={thread.id}>
        <button className="thread-select" onClick={()=>onSelect(thread.id)} title={thread.title}><span className={`run-dot ${statusClass(thread.status)}`}>{thread.status==="awaiting-approval"?<CircleDot size={11}/>:null}</span><span className="thread-title">{thread.title}</span>{thread.unreadApproval&&<span className="unread-dot"/>}</button>
        <div className="thread-actions"><button aria-label={thread.archived?"恢复任务":"归档任务"} title={thread.archived?"恢复任务":"归档任务"} onClick={()=>onArchive(thread.id,!thread.archived)}>{thread.archived?<ArchiveRestore size={13}/>:<Archive size={13}/>}</button><button className="delete" aria-label="删除任务" title="删除任务" onClick={()=>onDelete(thread.id)}><Trash2 size={13}/></button></div>
      </div>)}
      {showArchived&&!visible.length&&<span className="archived-empty">没有已归档任务</span>}
      {archivedCount>0&&<button className="muted-thread" onClick={onToggleArchived}>{showArchived?<ChevronRight size={13}/>:<Archive size={13}/>}<span>{showArchived?"返回当前任务":`查看已归档任务 (${archivedCount})`}</span></button>}
    </div>}
  </div>;
}
