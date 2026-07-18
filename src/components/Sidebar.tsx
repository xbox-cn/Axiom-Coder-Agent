import { Archive, ChevronDown, ChevronRight, CircleDot, Folder, FolderGit2, PanelLeftClose, PanelLeftOpen, Plus, Search, Settings, SlidersHorizontal, Star, Waypoints } from "lucide-react";
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
  const grouped=useMemo(()=>new Map((data?.projects??[]).map(project=>[project,(data?.threads??[]).filter(t=>t.projectId===project.id)])),[data]);
  return <aside className={`sidebar ${collapsed?"collapsed":""}`} style={collapsed?undefined:{width,minWidth:width}}>
    <div className="brand-row"><div className="axiom-mark"><i/><i/><i/></div><strong>Axiom</strong><button className="sidebar-collapse" onClick={()=>window.innerWidth<900?setSidebarOverlayOpen(false):persistCollapsed(!collapsed)} aria-label={collapsed?"展开侧栏":"折叠侧栏"} title={collapsed?"展开侧栏":"折叠侧栏"}>{collapsed?<PanelLeftOpen size={16}/>:<PanelLeftClose size={16}/>}</button></div>
    <div className="sidebar-actions">
      <button className="primary-nav" onClick={()=>{setSidebarOverlayOpen(false);void createThread();}}><Plus size={17}/><span>新建任务</span><kbd>Ctrl N</kbd></button>
      <button onClick={()=>{setSidebarOverlayOpen(false);setModal("search");}}><Search size={17}/><span>搜索</span><kbd>Ctrl K</kbd></button>
    </div>
    <div className="sidebar-section-title"><span>项目</span><button onClick={()=>{setSidebarOverlayOpen(false);void addProject();}} aria-label="添加项目"><Plus size={15}/></button></div>
    <div className="project-list">
      {[...grouped.entries()].map(([project,threads])=><ProjectGroup key={project.id} project={project} threads={threads} activeProjectId={activeProjectId} activeThreadId={activeThreadId} expanded={expanded[project.id]??project.id===activeProjectId} onToggle={()=>setExpanded(v=>({...v,[project.id]:!(v[project.id]??project.id===activeProjectId)}))} onSelect={id=>{setSidebarOverlayOpen(false);void selectThread(id)}}/>) }
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

function ProjectGroup({project,threads,activeProjectId,activeThreadId,expanded,onToggle,onSelect}:{project:Project;threads:ThreadSummary[];activeProjectId:string|null;activeThreadId:string|null;expanded:boolean;onToggle:()=>void;onSelect:(id:string)=>void}){
  return <div className={`project-group ${project.id===activeProjectId?"active-project":""}`}>
    <button className="project-row" onClick={onToggle}>{expanded?<ChevronDown size={14}/>:<ChevronRight size={14}/>}<Folder size={16}/><span>{project.name}</span>{project.favorite&&<Star size={12} fill="currentColor"/>}</button>
    {expanded&&<div className="thread-list">
      {threads.map(thread=><button key={thread.id} className={thread.id===activeThreadId?"active":""} onClick={()=>onSelect(thread.id)}><span className={`run-dot ${statusClass(thread.status)}`}>{thread.status==="awaiting-approval"?<CircleDot size={11}/>:null}</span><span className="thread-title">{thread.title}</span>{thread.unreadApproval&&<span className="unread-dot"/>}</button>)}
      <button className="muted-thread"><Archive size={13}/><span>查看已归档任务</span></button>
    </div>}
  </div>;
}
