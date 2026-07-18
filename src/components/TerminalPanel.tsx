import { Circle, Play, RefreshCw, Terminal } from "lucide-react";
import { useState } from "react";
import { useAppStore } from "../store/appStore";

export default function TerminalPanel() {
  const history = useAppStore((state) => state.shellHistory);
  const runShell = useAppStore((state) => state.runShell);
  const permission = useAppStore((state) => state.permissionMode);
  const [command, setCommand] = useState("pnpm test");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState("");

  const execute = async () => {
    if (!command.trim()) return;
    setRunning(true);
    setError("");
    try {
      await runShell(command);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className="terminal-panel">
      <div className="terminal-banner">
        <Circle size={8} fill="currentColor" />
        <span>PowerShell · {permissionLabel(permission)}</span>
      </div>
      <div className="terminal-output">
        {history.length === 0 ? (
          <div className="terminal-placeholder">
            <Terminal size={20} />
            <span>运行输出会保存在当前任务中</span>
          </div>
        ) : null}
        {history.map((item, index) => (
          <div className="terminal-entry" key={`${item.command}-${index}`}>
            <div><span className="prompt">PS ›</span> {item.command}</div>
            {item.stdout ? <pre>{item.stdout}</pre> : null}
            {item.stderr ? <pre className="stderr">{item.stderr}</pre> : null}
            <small>exit {item.exitCode ?? "?"} · {item.durationMs}ms</small>
          </div>
        ))}
        {error ? <pre className="terminal-error">{error}</pre> : null}
      </div>
      <div className="terminal-command">
        <span>PS ›</span>
        <input
          aria-label="PowerShell 命令"
          value={command}
          onChange={(event) => setCommand(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") void execute();
          }}
        />
        <button onClick={() => void execute()} disabled={running} aria-label={running ? "命令执行中" : "运行命令"}>
          {running ? <RefreshCw className="spin" size={15} /> : <Play size={15} />}
        </button>
      </div>
    </div>
  );
}

function permissionLabel(value: string) {
  return value === "read-only" ? "只读" : value === "full-access" ? "完全访问" : "工作区自动";
}
