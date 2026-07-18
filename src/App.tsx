import { useEffect } from "react";
import { onAgentEvent } from "./lib/api";
import { useAppStore } from "./store/appStore";
import { Sidebar } from "./components/Sidebar";
import { Conversation } from "./components/Conversation";
import { Inspector } from "./components/Inspector";
import { AppModal } from "./components/AppModal";
import { AlertCircle, X } from "lucide-react";
import "./App.css";

export default function App(){
  const initialize=useAppStore(s=>s.initialize);
  const handleAgentEvent=useAppStore(s=>s.handleAgentEvent);
  const loading=useAppStore(s=>s.loading);
  const error=useAppStore(s=>s.error);
  const clearError=useAppStore(s=>s.clearError);
  const settings=useAppStore(s=>s.bootstrapData?.settings);
  const createThread=useAppStore(s=>s.createThread);
  const setModal=useAppStore(s=>s.setModal);
  const cancel=useAppStore(s=>s.cancel);
  const inspectorOpen=useAppStore(s=>s.inspectorOpen);
  const setInspectorOpen=useAppStore(s=>s.setInspectorOpen);
  const sidebarOverlayOpen=useAppStore(s=>s.sidebarOverlayOpen);
  const setSidebarOverlayOpen=useAppStore(s=>s.setSidebarOverlayOpen);
  useEffect(()=>{
    let unlisten:(()=>void)|undefined;
    void initialize();
    void onAgentEvent(handleAgentEvent).then(fn=>{unlisten=fn;});
    return()=>unlisten?.();
  },[initialize,handleAgentEvent]);
  useEffect(()=>{
    const handler=(event:KeyboardEvent)=>{
      if((event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==="n"){event.preventDefault();void createThread();}
      if((event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==="k"){event.preventDefault();setModal("search");}
      if((event.ctrlKey||event.metaKey)&&event.shiftKey&&event.key.toLowerCase()==="i"){event.preventDefault();setInspectorOpen(!useAppStore.getState().inspectorOpen);}
      if(event.key==="Escape"){if(useAppStore.getState().sidebarOverlayOpen)setSidebarOverlayOpen(false);else if(useAppStore.getState().inspectorOpen&&window.innerWidth<1120)setInspectorOpen(false,false);else void cancel();}
    };
    window.addEventListener("keydown",handler);return()=>window.removeEventListener("keydown",handler);
  },[createThread,setModal,cancel,setInspectorOpen,setSidebarOverlayOpen]);
  useEffect(()=>{
    const onResize=()=>{if(window.innerWidth<1120&&useAppStore.getState().inspectorOpen)setInspectorOpen(false,false);if(window.innerWidth>=900&&useAppStore.getState().sidebarOverlayOpen)setSidebarOverlayOpen(false);};
    window.addEventListener("resize",onResize);return()=>window.removeEventListener("resize",onResize);
  },[setInspectorOpen,setSidebarOverlayOpen]);
  useEffect(()=>{
    const theme=settings?.theme??"system";
    document.documentElement.dataset.theme=theme;
  },[settings?.theme]);
  if(loading&&!useAppStore.getState().bootstrapData)return <div className="boot-screen"><div className="axiom-mark large"><i/><i/><i/></div><span>正在启动 Axiom</span></div>;
  return <div className={`app-shell ${sidebarOverlayOpen?"mobile-sidebar-open":""}`}>
    <Sidebar/>
    {sidebarOverlayOpen&&<button className="sidebar-overlay-backdrop" aria-label="关闭侧栏抽屉" onClick={()=>setSidebarOverlayOpen(false)}/>}
    <Conversation/>
    {inspectorOpen&&<button className="inspector-backdrop" aria-label="关闭检查器抽屉" onClick={()=>setInspectorOpen(false,false)}/>}
    <Inspector/>
    <AppModal/>
    {error&&<div className="error-toast" role="alert"><AlertCircle size={17}/><span>{error}</span><button onClick={clearError} aria-label="关闭错误"><X size={16}/></button></div>}
  </div>;
}
