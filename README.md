# Axiom

Axiom 是一个 **Windows 优先、本地优先、多供应商**的开源 AI Coding Agent 桌面应用。它采用项目/任务、对话执行、检查器三栏信息架构，并使用独立的 Axiom 品牌与设计令牌。

> 当前版本：`1.0.2`。默认不收集遥测，不包含账号、云同步、自建云后端或插件市场。

## 下载

从 [GitHub Releases](https://github.com/xbox-cn/Axiom-Coder-Agent/releases/latest) 下载对应平台的安装包：

| 平台 | 架构 | 安装包 |
| --- | --- | --- |
| Windows | x64 | MSI、NSIS EXE |
| Linux | x64 | DEB、AppImage、RPM |
| macOS | Intel x64 | DMG |
| macOS | Apple Silicon | DMG |

Windows 是当前优先支持与重点测试的平台。macOS 与 Linux 构建由 GitHub Actions 自动生成，欢迎提交兼容性问题。

## 核心能力

### Codex 风格桌面工作区

- Tauri 2 + Rust + React 19 + TypeScript + Vite。
- 项目/任务侧栏、中央消息区和 `Changes / Files / Terminal / Context` 检查器三栏布局。
- Codex 风格的轻量消息流、悬浮 Composer、自定义 Dropdown 与响应式侧栏。
- 浅色、深色、跟随系统主题；统一使用 Segoe UI Variable 与 Cascadia Code 字体栈。
- Markdown/GFM、代码块、工具活动、审批卡片、Token 与上下文指标。
- 消息列表虚拟化，流式增量批量提交，降低长会话渲染开销。

### 供应商与模型

- 新安装不创建示例供应商，默认供应商和模型保持为空。
- 新建供应商支持 `Responses API` 和 `Chat Completions` 两种 OpenAI-compatible 协议。
- 填写 Base URL 与 API Key 后可从上游获取模型，也可手动添加和删除模型。
- 每个模型可设置以“万 Token”为单位的上下文长度。
- 旧版 OpenAI、Anthropic、Gemini、OpenRouter 与 Ollama 原生配置会作为兼容配置保留。
- 每回合保存不可变的供应商、模型、思考等级、权限和运行模式快照。
- 运行期间锁定配置选择器，不静默降级或切换供应商。

### Agent / Plan / Goal

- **Agent**：执行完整的文件、Git、Shell 与 MCP 工具循环。
- **Plan**：由 Rust 权限层强制只读，输出可执行计划，并支持“按计划执行”。
- **Goal**：持续多回合执行，直到完成、阻塞、等待审批、失败或用户暂停。
- 内置文件列表、读取、搜索、写入、补丁、删除、恢复、Git 与 Shell 工具。
- 只读、工作区自动、完全访问三级权限。
- 同一工作区只允许一个可写运行，取消时终止 Agent 创建的进程树。
- 文件写入前保存前镜像，支持按运行恢复内置文件工具产生的改动。

### 附件、Context 与 Usage

- 支持文件选择、多文件拖放、文本与常见图片附件。
- 发送时为附件创建不可变快照，原文件后续变化不会影响历史消息。
- Context Builder 组合项目指令、对话、压缩摘要、固定消息、附件与工具定义。
- 75% 占用预警，85% 时在下一回合前创建透明压缩检查点。
- 显示输入、输出、缓存、推理 Token、当前上下文、模型上限、耗时和费用估算。
- 优先使用上游 Usage；缺失数据明确标为“估算”或“不可用”，不伪造为零。

### MCP 与本地数据

- 支持 MCP `stdio` 与 Streamable HTTP。
- 支持全局/项目范围、工具发现、逐工具启用、健康检查与脱敏错误信息。
- SQLite 使用 WAL、FTS5 和版本化迁移；不可逆迁移前自动备份数据库。
- API Key、MCP Header 和环境变量敏感值写入系统凭据存储，SQLite 只保存引用。
- 只有用户配置的供应商和远程 MCP 会产生网络请求。

## 架构

```text
React / Zustand
      │ typed Tauri IPC + sequenced events
      ▼
Rust application core
 ├─ Agent state machine / Context Builder
 ├─ Provider adapters + streaming normalization
 ├─ Permission-aware file, Git and Shell tools
 ├─ MCP stdio / Streamable HTTP client
 ├─ SQLite + WAL + FTS5 + recovery
 └─ OS credential store / process-tree cancellation
```

SQLite/Rust 是项目、任务、消息、运行和配置的事实来源；Zustand 只保存草稿、选择器、面板和流式增量等瞬时状态。

## 本地开发

### 通用依赖

- Node.js 20+（CI 使用 Node.js 22）
- pnpm 10+
- Rust stable
- 对应平台的 Tauri 2 系统依赖

Windows 还需要 WebView2 Runtime、Visual Studio 2022 Build Tools、MSVC linker 与 Windows SDK。

```powershell
pnpm install
pnpm dev          # 浏览器 Mock 预览
pnpm tauri dev    # 原生桌面应用
```

### 验证

```powershell
pnpm build
pnpm test
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

### 打包与发布

本地可使用：

```powershell
pnpm tauri build
```

推送 `v*` 标签会触发 [release workflow](.github/workflows/release.yml)，并行构建：

- Windows x64：MSI、NSIS EXE
- Linux x64：DEB、AppImage、RPM
- macOS Intel：DMG
- macOS Apple Silicon：DMG

所有构建成功后，工作流会生成 `SHA256SUMS.txt` 并发布正式 GitHub Release。CI 产物目前未进行商业代码签名，系统可能显示未知发布者提示。

## 权限说明

- `read-only`：自动读取和搜索工作区，拒绝写入。
- `workspace-auto`：允许工作区内置文件操作和低风险项目命令；越界、联网、系统或破坏性操作需要审批。
- `full-access`：减少逐次审批，启用前显示风险确认，并在 Composer 中持续标识。

这些规则是 **应用层审批机制，不是 OS 级安全沙箱**。Shell 命令可能造成无法自动撤销的外部副作用。

## 快捷键

- `Ctrl+N`：新建任务
- `Ctrl+K`：全局搜索
- `Ctrl+Shift+I`：显示/隐藏检查器
- `Ctrl+Enter`：发送
- `Esc`：关闭弹层或停止当前运行

## 当前限制

- Windows 是优先平台；macOS 与 Linux 仍需要更多真机验证。
- 远程 MCP 仅支持固定 Header/Bearer 凭据，不支持浏览器 OAuth。
- Terminal 是当前任务的命令与输出视图，不是完整 IDE 终端工作区。
- Shell 权限策略不等价于容器、虚拟机或 OS 沙箱，外部副作用不承诺可逆。
- 不包含完整编辑器、LSP、调试器、账号/云同步、多人协作、插件市场、并行 Agent、自动 worktree 和远程主机。

## 许可证

Apache-2.0。见 [`LICENSE`](LICENSE)。
