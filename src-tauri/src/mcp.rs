use crate::{
    models::{McpServerConfig, McpTestResult, McpTransport},
    secrets,
};
use reqwest::{Client, RequestBuilder};
use serde_json::{json, Map, Value};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::timeout,
};

const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

#[cfg(windows)]
fn hide_background_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.as_std_mut().creation_flags(0x08000000);
}

#[cfg(not(windows))]
fn hide_background_window(_command: &mut Command) {}

/// Converts every MCP environment/header value into an opaque Windows Credential
/// Manager reference before the configuration is persisted in SQLite.
pub fn protect_credentials(mut config: McpServerConfig) -> Result<McpServerConfig, String> {
    if config.id.trim().is_empty() {
        config.id = uuid::Uuid::new_v4().to_string();
    }
    config.env = protect_map(&config.id, "env", &config.env)?;
    config.headers = protect_map(&config.id, "header", &config.headers)?;
    Ok(config)
}

fn protect_map(server_id: &str, group: &str, value: &Value) -> Result<Value, String> {
    let Some(object) = value.as_object() else {
        return Ok(Value::Object(Map::new()));
    };
    let mut protected = Map::new();
    for (key, value) in object {
        let Some(value) = value.as_str() else {
            continue;
        };
        if value.is_empty() {
            protected.insert(key.clone(), Value::String(String::new()));
            continue;
        }
        if secrets::reference(value).is_some() {
            protected.insert(key.clone(), Value::String(value.to_string()));
            continue;
        }
        let reference = credential_name(server_id, group, key);
        secrets::store(&reference, value)?;
        protected.insert(key.clone(), Value::String(secrets::tagged(&reference)));
    }
    Ok(Value::Object(protected))
}

fn credential_name(server_id: &str, group: &str, key: &str) -> String {
    let slug: String = key
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .take(48)
        .collect();
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    format!(
        "mcp:{server_id}:{group}:{slug}:{:08x}",
        hasher.finish() as u32
    )
}

fn hydrate_credentials(config: &McpServerConfig) -> Result<McpServerConfig, String> {
    let mut hydrated = config.clone();
    hydrated.env = hydrate_map(&config.env)?;
    hydrated.headers = hydrate_map(&config.headers)?;
    Ok(hydrated)
}

fn hydrate_map(value: &Value) -> Result<Value, String> {
    let Some(object) = value.as_object() else {
        return Ok(Value::Object(Map::new()));
    };
    let mut hydrated = Map::new();
    for (key, value) in object {
        let Some(value) = value.as_str() else {
            continue;
        };
        let resolved = match secrets::reference(value) {
            Some(reference) => secrets::load(reference)?,
            None => value.to_string(),
        };
        hydrated.insert(key.clone(), Value::String(resolved));
    }
    Ok(Value::Object(hydrated))
}

pub async fn test_server(config: &McpServerConfig) -> Result<McpTestResult, String> {
    let config = hydrate_credentials(config)?;
    let started = Instant::now();
    let (server_name, protocol_version, tools, read_only_tools) = match config.transport {
        McpTransport::Stdio => test_stdio(&config).await?,
        McpTransport::StreamableHttp => test_http(&config).await?,
    };
    Ok(McpTestResult {
        ok: true,
        server_name,
        protocol_version,
        tools,
        read_only_tools,
        latency_ms: started.elapsed().as_millis() as u64,
        message: "MCP 服务连接正常".to_string(),
    })
}

struct StdioSession {
    child: Child,
    stdin: ChildStdin,
    reader: tokio::io::Lines<BufReader<ChildStdout>>,
    timeout_seconds: u64,
    #[cfg(windows)]
    _process_tree: crate::tools::process_tree::KillOnDropJob,
}

impl StdioSession {
    async fn start(config: &McpServerConfig) -> Result<Self, String> {
        let command = config
            .command
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "stdio MCP 缺少 command".to_string())?;
        let mut process = Command::new(command);
        hide_background_window(&mut process);
        process
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(cwd) = &config.cwd {
            process.current_dir(cwd);
        }
        apply_environment(&mut process, &config.env);
        let mut child = process
            .spawn()
            .map_err(|error| safe_error(config, "无法启动 MCP 服务", &error.to_string()))?;
        #[cfg(windows)]
        let process_tree = crate::tools::process_tree::KillOnDropJob::attach(
            child
                .id()
                .ok_or_else(|| "无法获取 MCP 进程 ID".to_string())?,
        )?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "无法打开 MCP stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "无法打开 MCP stdout".to_string())?;
        Ok(Self {
            child,
            stdin,
            reader: BufReader::new(stdout).lines(),
            timeout_seconds: config.timeout_seconds,
            #[cfg(windows)]
            _process_tree: process_tree,
        })
    }

    async fn send(&mut self, value: &Value) -> Result<(), String> {
        self.stdin
            .write_all(format!("{value}\n").as_bytes())
            .await
            .map_err(|error| format!("写入 MCP stdin 失败: {error}"))?;
        self.stdin
            .flush()
            .await
            .map_err(|error| format!("刷新 MCP stdin 失败: {error}"))
    }

    async fn request(&mut self, id: i64, method: &str, params: Value) -> Result<Value, String> {
        self.send(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        read_response(&mut self.reader, id, self.timeout_seconds).await
    }

    async fn initialize(&mut self) -> Result<Value, String> {
        let response = self
            .request(
                1,
                "initialize",
                json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {"name":"Axiom","version":"1.0.2"}
                }),
            )
            .await?;
        ensure_rpc_success(&response)?;
        self.send(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await?;
        Ok(response)
    }

    async fn close(mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}

fn apply_environment(command: &mut Command, env: &Value) {
    if let Some(env) = env.as_object() {
        for (key, value) in env {
            if let Some(value) = value.as_str() {
                command.env(key, value);
            }
        }
    }
}

async fn test_stdio(
    config: &McpServerConfig,
) -> Result<(Option<String>, Option<String>, Vec<String>, Vec<String>), String> {
    let mut session = StdioSession::start(config).await?;
    let initialize = session.initialize().await?;
    let tools = session
        .request(2, "tools/list", json!({}))
        .await
        .unwrap_or_else(|_| json!({"result":{"tools":[]}}));
    session.close().await;
    Ok(parse_results(&initialize, &tools))
}

async fn read_response(
    reader: &mut tokio::io::Lines<BufReader<ChildStdout>>,
    id: i64,
    seconds: u64,
) -> Result<Value, String> {
    timeout(Duration::from_secs(seconds.max(3)), async {
        while let Some(line) = reader
            .next_line()
            .await
            .map_err(|error| format!("读取 MCP 输出失败: {error}"))?
        {
            if line.trim().is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(&line)
                .map_err(|error| format!("MCP 返回了无效 JSON: {error}"))?;
            if value.get("id").and_then(Value::as_i64) == Some(id) {
                return Ok(value);
            }
        }
        Err("MCP 进程提前结束".to_string())
    })
    .await
    .map_err(|_| "MCP 请求超时".to_string())?
}

async fn test_http(
    config: &McpServerConfig,
) -> Result<(Option<String>, Option<String>, Vec<String>, Vec<String>), String> {
    let client = http_client(config)?;
    let (initialize, session) = http_initialize(&client, config).await?;
    let tools_payload = json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}});
    let tools = send_http(&client, config, &tools_payload, session.as_deref())
        .await
        .unwrap_or_else(|_| json!({"result":{"tools":[]}}));
    Ok(parse_results(&initialize, &tools))
}

fn http_client(config: &McpServerConfig) -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(config.timeout_seconds.max(3)))
        .build()
        .map_err(|error| format!("无法创建 MCP HTTP 客户端: {error}"))
}

async fn http_initialize(
    client: &Client,
    config: &McpServerConfig,
) -> Result<(Value, Option<String>), String> {
    let payload = json!({
        "jsonrpc":"2.0",
        "id":1,
        "method":"initialize",
        "params":{
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities":{},
            "clientInfo":{"name":"Axiom","version":"1.0.2"}
        }
    });
    let (value, session) = send_http_with_session(client, config, &payload, None).await?;
    ensure_rpc_success(&value)?;
    Ok((value, session))
}

async fn send_http(
    client: &Client,
    config: &McpServerConfig,
    payload: &Value,
    session: Option<&str>,
) -> Result<Value, String> {
    send_http_with_session(client, config, payload, session)
        .await
        .map(|(value, _)| value)
}

async fn send_http_with_session(
    client: &Client,
    config: &McpServerConfig,
    payload: &Value,
    session: Option<&str>,
) -> Result<(Value, Option<String>), String> {
    let url = config
        .url
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "HTTP MCP 缺少 URL".to_string())?;
    let mut request = client
        .post(url)
        .header("accept", "application/json, text/event-stream")
        .json(payload);
    if let Some(session) = session {
        request = request.header("Mcp-Session-Id", session);
    }
    request = apply_headers(request, &config.headers);
    let response = request
        .send()
        .await
        .map_err(|error| safe_error(config, "MCP HTTP 请求失败", &error.to_string()))?;
    let status = response.status();
    let response_session = response
        .headers()
        .get("Mcp-Session-Id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = response
        .text()
        .await
        .map_err(|error| safe_error(config, "读取 MCP HTTP 响应失败", &error.to_string()))?;
    if !status.is_success() {
        let excerpt: String = body.chars().take(240).collect();
        return Err(format!(
            "MCP HTTP {}: {}",
            status.as_u16(),
            redact_secrets(config, &excerpt)
        ));
    }
    let value = parse_http_payload(&body)?;
    Ok((value, response_session))
}

fn apply_headers(mut request: RequestBuilder, headers: &Value) -> RequestBuilder {
    if let Some(headers) = headers.as_object() {
        for (key, value) in headers {
            if let Some(value) = value.as_str() {
                request = request.header(key, value);
            }
        }
    }
    request
}

fn parse_http_payload(body: &str) -> Result<Value, String> {
    if let Ok(value) = serde_json::from_str(body) {
        return Ok(value);
    }
    for line in body.lines() {
        if let Some(data) = line.trim_start().strip_prefix("data:") {
            let data = data.trim();
            if data == "[DONE]" {
                continue;
            }
            if let Ok(value) = serde_json::from_str(data) {
                return Ok(value);
            }
        }
    }
    Err("无法解析 MCP HTTP 响应".to_string())
}

fn parse_results(
    initialize: &Value,
    tools: &Value,
) -> (Option<String>, Option<String>, Vec<String>, Vec<String>) {
    let server_name = initialize
        .pointer("/result/serverInfo/name")
        .and_then(Value::as_str)
        .map(str::to_string);
    let protocol = initialize
        .pointer("/result/protocolVersion")
        .and_then(Value::as_str)
        .map(str::to_string);
    let descriptors = tools
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let names = descriptors
        .iter()
        .filter_map(|value| {
            value
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();
    let read_only_tools = descriptors
        .iter()
        .filter(|value| {
            value
                .pointer("/annotations/readOnlyHint")
                .and_then(Value::as_bool)
                == Some(true)
                && value
                    .pointer("/annotations/destructiveHint")
                    .and_then(Value::as_bool)
                    == Some(false)
        })
        .filter_map(|value| {
            value
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();
    (server_name, protocol, names, read_only_tools)
}

fn ensure_rpc_success(response: &Value) -> Result<(), String> {
    if let Some(error) = response.get("error") {
        let code = error.get("code").and_then(Value::as_i64);
        return Err(match code {
            Some(code) => format!("MCP JSON-RPC 错误（code {code}）"),
            None => "MCP JSON-RPC 请求失败".to_string(),
        });
    }
    Ok(())
}

pub async fn call_tool(
    config: &McpServerConfig,
    name: &str,
    arguments: Value,
) -> Result<Value, String> {
    if config.disabled_tools.iter().any(|tool| tool == name) {
        return Err(format!("MCP 工具已禁用: {name}"));
    }
    let config = hydrate_credentials(config)?;
    match config.transport {
        McpTransport::Stdio => call_tool_stdio(&config, name, arguments).await,
        McpTransport::StreamableHttp => call_tool_http(&config, name, arguments).await,
    }
}

async fn call_tool_stdio(
    config: &McpServerConfig,
    name: &str,
    arguments: Value,
) -> Result<Value, String> {
    let mut session = StdioSession::start(config).await?;
    session.initialize().await?;
    let response = session
        .request(2, "tools/call", json!({"name":name,"arguments":arguments}))
        .await;
    session.close().await;
    let response = response?;
    ensure_rpc_success(&response)?;
    Ok(response.get("result").cloned().unwrap_or(Value::Null))
}

async fn call_tool_http(
    config: &McpServerConfig,
    name: &str,
    arguments: Value,
) -> Result<Value, String> {
    let client = http_client(config)?;
    let (_, session) = http_initialize(&client, config).await?;
    let payload = json!({
        "jsonrpc":"2.0",
        "id":2,
        "method":"tools/call",
        "params":{"name":name,"arguments":arguments}
    });
    let value = send_http(&client, config, &payload, session.as_deref()).await?;
    ensure_rpc_success(&value)?;
    Ok(value.get("result").cloned().unwrap_or(Value::Null))
}

fn redact_secrets(config: &McpServerConfig, input: &str) -> String {
    let mut redacted = input.to_string();
    if let Some(url) = &config.url {
        if let Ok(mut parsed) = url::Url::parse(url) {
            parsed.set_query(None);
            parsed.set_fragment(None);
            redacted = redacted.replace(url, parsed.as_str());
        }
    }
    for object in [config.env.as_object(), config.headers.as_object()]
        .into_iter()
        .flatten()
    {
        for value in object.values().filter_map(Value::as_str) {
            if value.len() >= 4 && !secrets::reference(value).is_some() {
                redacted = redacted.replace(value, "[REDACTED]");
            }
        }
    }
    redacted
}

fn safe_error(config: &McpServerConfig, context: &str, detail: &str) -> String {
    format!("{context}: {}", redact_secrets(config, detail))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::McpScope;

    fn config() -> McpServerConfig {
        McpServerConfig {
            id: "server-1".into(),
            name: "test".into(),
            scope: McpScope::Global,
            project_id: None,
            transport: McpTransport::StreamableHttp,
            command: None,
            args: vec![],
            cwd: None,
            url: Some("https://example.test/mcp?token=url-secret".into()),
            env: json!({"TOKEN":"environment-secret"}),
            headers: json!({"Authorization":"Bearer header-secret"}),
            timeout_seconds: 30,
            enabled: true,
            status: "stopped".into(),
            last_error: None,
            discovered_tools: vec![],
            disabled_tools: vec![],
            read_only_tools: vec![],
            updated_at: String::new(),
        }
    }

    #[test]
    fn parses_json_and_sse_payloads() {
        assert_eq!(
            parse_http_payload(r#"{"result":{"ok":true}}"#).unwrap()["result"]["ok"],
            true
        );
        assert_eq!(
            parse_http_payload("event: message\ndata: {\"result\":{\"ok\":true}}\n\n").unwrap()
                ["result"]["ok"],
            true
        );
    }

    #[test]
    fn extracts_server_and_tool_metadata() {
        let (name, protocol, tools, read_only_tools) = parse_results(
            &json!({"result":{"serverInfo":{"name":"demo"},"protocolVersion":"2025-06-18"}}),
            &json!({"result":{"tools":[
                {"name":"read","annotations":{"readOnlyHint":true,"destructiveHint":false}},
                {"name":"write","annotations":{"readOnlyHint":false,"destructiveHint":true}},
                {"name":"ambiguous","annotations":{"readOnlyHint":true}}
            ]}}),
        );
        assert_eq!(name.as_deref(), Some("demo"));
        assert_eq!(protocol.as_deref(), Some("2025-06-18"));
        assert_eq!(tools, vec!["read", "write", "ambiguous"]);
        assert_eq!(read_only_tools, vec!["read"]);
    }

    #[test]
    fn diagnostic_text_redacts_headers_environment_and_url_query() {
        let output = redact_secrets(
            &config(),
            "Bearer header-secret environment-secret https://example.test/mcp?token=url-secret",
        );
        assert!(!output.contains("header-secret"));
        assert!(!output.contains("environment-secret"));
        assert!(!output.contains("url-secret"));
        assert!(output.contains("[REDACTED]"));
    }

    #[test]
    fn credential_names_are_stable_and_do_not_embed_secret_values() {
        let first = credential_name("server", "header", "Authorization");
        let second = credential_name("server", "header", "Authorization");
        assert_eq!(first, second);
        assert!(first.starts_with("mcp:server:header:authorization:"));
    }
}
