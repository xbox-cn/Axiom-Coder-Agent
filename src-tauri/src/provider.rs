use crate::{db::Database, models::*, secrets};
use base64::Engine;
use reqwest::{
    header::{
        HeaderMap, HeaderName, HeaderValue, ACCEPT, ACCEPT_ENCODING, AUTHORIZATION, CONTENT_TYPE,
    },
    Client, RequestBuilder, Response,
};
use serde_json::{json, Value};
use std::{
    error::Error as _,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
pub struct ProviderResponse {
    pub text: String,
    pub usage: UsageRecord,
    pub tool_call: Option<ProviderToolCall>,
}

#[derive(Debug, Clone)]
pub struct AllowedMcpTool {
    pub server_id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct ProviderToolCall {
    pub call_id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderStreamEvent {
    TextDelta(String),
    ReasoningDelta(String),
}

pub fn store_api_key(reference: &str, secret: &str) -> Result<(), String> {
    secrets::store(reference, secret)
}

pub fn load_api_key(reference: &str) -> Option<String> {
    secrets::load(reference).ok()
}

pub fn protect_extra_headers(provider_id: &str, headers: &Value) -> Result<Value, String> {
    let Some(object) = headers.as_object() else {
        return Ok(json!({}));
    };
    let mut protected = serde_json::Map::new();
    for (name, value) in object {
        let Some(value) = value.as_str() else {
            continue;
        };
        if value.is_empty() || secrets::reference(value).is_some() {
            protected.insert(name.clone(), Value::String(value.to_string()));
            continue;
        }
        let normalized: String = name
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                    ch.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .take(64)
            .collect();
        let reference = format!("provider:{provider_id}:header:{normalized}");
        secrets::store(&reference, value)?;
        protected.insert(name.clone(), Value::String(secrets::tagged(&reference)));
    }
    Ok(Value::Object(protected))
}

fn client(profile: &ProviderProfile) -> Result<Client, String> {
    Client::builder()
        // A total timeout also counts time spent streaming and can abort a healthy long
        // reasoning response. Use a generous inactivity timeout instead.
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(profile.timeout_seconds.max(600)))
        // Some compatible proxies advertise compressed bodies but send malformed frames.
        // Requesting identity avoids the opaque "error decoding response body" failure.
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .build()
        .map_err(|e| format!("Could not create HTTP client: {e}"))
}

const PROVIDER_REQUEST_RETRIES: usize = 5;
const PROVIDER_RETRY_STEP_SECONDS: u64 = 5;

fn provider_retry_delay(retry_number: usize) -> Duration {
    Duration::from_secs(PROVIDER_RETRY_STEP_SECONDS * retry_number as u64)
}

/// Retry only transport-level failures that happen before an HTTP response is
/// available. HTTP status errors keep their existing behavior and are never
/// retried silently.
async fn send_provider_request_with_retry(request: RequestBuilder) -> Result<Response, String> {
    for attempt in 0..=PROVIDER_REQUEST_RETRIES {
        let current = request.try_clone().ok_or_else(|| {
            "Provider request failed: request body could not be retried".to_string()
        })?;
        match current.send().await {
            Ok(response) => return Ok(response),
            Err(_error) if attempt < PROVIDER_REQUEST_RETRIES => {
                tokio::time::sleep(provider_retry_delay(attempt + 1)).await;
            }
            Err(error) => {
                return Err(format!(
                    "Provider request failed: {error} (retried {PROVIDER_REQUEST_RETRIES} times)"
                ));
            }
        }
    }
    unreachable!("provider request retry loop always returns")
}

fn provider_endpoint(base_url: &str, endpoint: &str) -> Result<String, String> {
    let mut base = base_url.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        return Err("Base URL is required".to_string());
    }
    loop {
        let lowered = base.to_ascii_lowercase();
        let suffix = ["/chat/completions", "/responses", "/models"]
            .into_iter()
            .find(|suffix| lowered.ends_with(suffix));
        let Some(suffix) = suffix else { break };
        base.truncate(base.len() - suffix.len());
        base = base.trim_end_matches('/').to_string();
    }
    if base.is_empty() {
        return Err("Base URL is invalid".to_string());
    }
    Ok(format!("{base}/{}", endpoint.trim_start_matches('/')))
}

fn redact_known_secret(mut message: String, secret: Option<&str>) -> String {
    if let Some(secret) = secret.filter(|value| !value.is_empty()) {
        message = message.replace(secret, "[REDACTED]");
    }
    message
}

fn credential(db: &Database, profile: &ProviderProfile) -> Result<Option<String>, String> {
    let reference = db.get_provider_credential_ref(&profile.id)?;
    Ok(reference.and_then(|value| load_api_key(&value)))
}

fn apply_headers(
    mut request: RequestBuilder,
    profile: &ProviderProfile,
) -> Result<RequestBuilder, String> {
    let mut headers = HeaderMap::new();
    if let Some(object) = profile.extra_headers.as_object() {
        for (name, value) in object {
            let Some(value) = value.as_str() else {
                continue;
            };
            let resolved;
            let value = if let Some(reference) = secrets::reference(value) {
                resolved = secrets::load(reference)?;
                resolved.as_str()
            } else {
                value
            };
            let name =
                HeaderName::from_str(name).map_err(|e| format!("Invalid header name: {e}"))?;
            let value =
                HeaderValue::from_str(value).map_err(|e| format!("Invalid header value: {e}"))?;
            headers.insert(name, value);
        }
    }
    request = request.headers(headers);
    Ok(request)
}

fn with_key(request: RequestBuilder, kind: ProviderKind, key: Option<&str>) -> RequestBuilder {
    let Some(key) = key else { return request };
    match kind {
        ProviderKind::Anthropic => request
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01"),
        ProviderKind::Gemini => request.header("x-goog-api-key", key),
        _ => request.header(AUTHORIZATION, format!("Bearer {key}")),
    }
}

pub async fn test_connection(
    db: Arc<Database>,
    provider_id: &str,
) -> Result<Vec<ModelDescriptor>, String> {
    discover_models(db, provider_id).await
}

pub async fn discover_models_draft(
    _api_type: ProviderApiType,
    base_url: &str,
    api_key: Option<&str>,
) -> Result<Vec<ModelDescriptor>, String> {
    let base_url = base_url.trim();
    if base_url.is_empty() {
        return Err("请填写 Base URL".to_string());
    }
    let models_url = provider_endpoint(base_url, "models")?;
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .build()
        .map_err(|error| format!("无法创建网络客户端: {error}"))?;
    let request = with_key(
        client.get(models_url),
        ProviderKind::OpenAiCompatible,
        api_key.filter(|value| !value.trim().is_empty()),
    );
    let response = request
        .send()
        .await
        .map_err(|error| format!("获取模型失败: {error}"))?;
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .map_err(|error| format!("上游返回了无效 JSON: {error}"))?;
    if !status.is_success() {
        return Err(redact_known_secret(
            provider_error(status.as_u16(), &body),
            api_key,
        ));
    }
    let levels = vec![
        ThinkingLevel::Off,
        ThinkingLevel::Low,
        ThinkingLevel::Medium,
        ThinkingLevel::High,
        ThinkingLevel::Xhigh,
        ThinkingLevel::Auto,
    ];
    let mut models: Vec<ModelDescriptor> = body
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?.trim().to_string();
            if id.is_empty() {
                return None;
            }
            let display_name = item
                .get("display_name")
                .or_else(|| item.get("name"))
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .to_string();
            let context_window = item
                .get("context_window_tokens")
                .or_else(|| item.get("context_window"))
                .or_else(|| item.get("context_length"))
                .and_then(Value::as_u64);
            Some(ModelDescriptor {
                id,
                display_name,
                context_window,
                max_output_tokens: None,
                capabilities: ModelCapabilities {
                    tools: true,
                    vision: true,
                    reasoning: true,
                    reasoning_levels: levels.clone(),
                    usage_reporting: true,
                },
            })
        })
        .collect();
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models.dedup_by(|left, right| left.id == right.id);
    Ok(models)
}

pub async fn test_model_draft(
    api_type: ProviderApiType,
    base_url: &str,
    api_key: Option<&str>,
    model_id: &str,
) -> Result<DraftModelTestResult, String> {
    let model_id = model_id.trim();
    if model_id.is_empty() {
        return Err("Model ID is required".to_string());
    }
    let endpoint = match api_type {
        ProviderApiType::Responses => "responses",
        ProviderApiType::ChatCompletions => "chat/completions",
    };
    let url = provider_endpoint(base_url, endpoint)?;
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(90))
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .build()
        .map_err(|error| format!("Could not create HTTP client: {error}"))?;
    let body = match api_type {
        ProviderApiType::Responses => json!({
            "model": model_id,
            "input": "Hello",
            "stream": true,
            "max_output_tokens": 256
        }),
        ProviderApiType::ChatCompletions => json!({
            "model": model_id,
            "messages": [{"role":"user", "content":"Hello"}],
            "stream": true,
            "max_tokens": 256
        }),
    };
    let started = Instant::now();
    let request = with_key(
        client
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream")
            .header(ACCEPT_ENCODING, "identity")
            .json(&body),
        ProviderKind::OpenAiCompatible,
        api_key.filter(|value| !value.trim().is_empty()),
    );
    let response = request.send().await.map_err(|error| {
        redact_known_secret(format!("Model test request failed: {error}"), api_key)
    })?;
    let response =
        checked_response_for_api(response, api_key, api_type == ProviderApiType::Responses).await?;
    let bytes = response.bytes().await.map_err(|error| {
        redact_known_secret(format!("Model test response failed: {error}"), api_key)
    })?;
    let parsed = parse_model_test_body(api_type, &bytes)
        .map_err(|error| redact_known_secret(error, api_key))?;
    if parsed.text.trim().is_empty() && parsed.reasoning.trim().is_empty() {
        return Err("The model test returned no text or reasoning content".to_string());
    }
    let usage = parsed.usage.as_ref().map(|raw| {
        let mut usage = normalize_usage(Some(raw), &parsed.text, &[]);
        usage.context_limit = builtin_context_limit(model_id);
        usage.duration_ms = Some(started.elapsed().as_millis() as u64);
        usage
    });
    let response_preview: String = if parsed.text.trim().is_empty() {
        format!(
            "服务响应成功（仅返回思考内容）：{}",
            parsed.reasoning.trim()
        )
        .chars()
        .take(240)
        .collect()
    } else {
        parsed.text.trim().chars().take(240).collect()
    };
    Ok(DraftModelTestResult {
        ok: true,
        latency_ms: started.elapsed().as_millis() as u64,
        response_preview,
        usage,
    })
}

#[derive(Debug, Default)]
struct ParsedModelTestBody {
    text: String,
    reasoning: String,
    usage: Option<Value>,
}

fn parse_model_test_body(
    api_type: ProviderApiType,
    body: &[u8],
) -> Result<ParsedModelTestBody, String> {
    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        return parse_model_test_values(api_type, std::iter::once(value));
    }

    let mut decoder = SseDecoder::default();
    let mut payloads = decoder.feed(body)?;
    payloads.extend(decoder.finish()?);
    if payloads.is_empty() {
        let text = std::str::from_utf8(body)
            .map_err(|_| "The model test response was not valid UTF-8".to_string())?;
        payloads = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("event:"))
            .map(|line| {
                line.strip_prefix("data:")
                    .unwrap_or(line)
                    .trim()
                    .to_string()
            })
            .collect();
    }
    let values = payloads
        .into_iter()
        .filter(|payload| payload != "[DONE]")
        .map(|payload| {
            serde_json::from_str::<Value>(&payload).map_err(|error| {
                format!("The provider returned an invalid streaming model-test event: {error}")
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if values.is_empty() {
        return Err("The model test returned an empty response body".to_string());
    }
    parse_model_test_values(api_type, values)
}

fn parse_model_test_values(
    api_type: ProviderApiType,
    values: impl IntoIterator<Item = Value>,
) -> Result<ParsedModelTestBody, String> {
    let mut parsed = ParsedModelTestBody::default();
    for value in values {
        if value.get("error").is_some() {
            return Err(provider_error(500, &value));
        }
        match api_type {
            ProviderApiType::ChatCompletions => {
                if let Some(delta) = openai_reasoning_delta(&value) {
                    parsed.reasoning.push_str(&delta);
                } else if parsed.reasoning.is_empty() {
                    parsed.reasoning.push_str(&openai_reasoning_text(&value));
                }
                if let Some(delta) = openai_delta(&value) {
                    parsed.text.push_str(&delta);
                } else if parsed.text.is_empty() {
                    parsed.text.push_str(&chat_json_text(&value));
                }
                if let Some(usage) = value.get("usage").filter(|usage| !usage.is_null()) {
                    parsed.usage = Some(usage.clone());
                }
            }
            ProviderApiType::Responses => {
                let event_type = value
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match event_type {
                    "response.output_text.delta" => {
                        if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                            parsed.text.push_str(delta);
                        }
                    }
                    "response.output_text.done" if parsed.text.is_empty() => {
                        if let Some(text) = value.get("text").and_then(Value::as_str) {
                            parsed.text.push_str(text);
                        }
                    }
                    "response.reasoning_summary_text.delta"
                    | "response.reasoning_text.delta"
                    | "response.reasoning.delta" => {
                        if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                            parsed.reasoning.push_str(delta);
                        }
                    }
                    "response.completed" => {
                        if let Some(response) = value.get("response") {
                            if parsed.text.is_empty() {
                                parsed.text.push_str(&responses_json_text(response));
                            }
                            if parsed.reasoning.is_empty() {
                                parsed
                                    .reasoning
                                    .push_str(&responses_json_reasoning(response));
                            }
                            parsed.usage = response.get("usage").cloned().or(parsed.usage);
                        }
                    }
                    "response.failed" | "response.incomplete" | "response.error" | "error" => {
                        return Err(provider_error(500, &value));
                    }
                    _ => {
                        if parsed.text.is_empty() {
                            parsed.text.push_str(&responses_json_text(&value));
                        }
                        if parsed.reasoning.is_empty() {
                            parsed.reasoning.push_str(&responses_json_reasoning(&value));
                        }
                    }
                }
                if let Some(usage) = value.get("usage").filter(|usage| !usage.is_null()) {
                    parsed.usage = Some(usage.clone());
                }
            }
        }
    }
    Ok(parsed)
}

pub async fn discover_models(
    db: Arc<Database>,
    provider_id: &str,
) -> Result<Vec<ModelDescriptor>, String> {
    let profile = db.get_provider(provider_id)?;
    let key = credential(&db, &profile)?;
    let client = client(&profile)?;
    let url = match profile.kind {
        ProviderKind::Ollama => format!("{}/api/tags", profile.base_url.trim_end_matches('/')),
        ProviderKind::Gemini => format!("{}/v1beta/models", profile.base_url.trim_end_matches('/')),
        _ => provider_endpoint(&profile.base_url, "models")?,
    };
    let request = apply_headers(
        with_key(client.get(url), profile.kind, key.as_deref()),
        &profile,
    )?;
    let response = request
        .send()
        .await
        .map_err(|e| format!("Provider connection failed: {e}"))?;
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .map_err(|e| format!("Provider returned invalid JSON: {e}"))?;
    if !status.is_success() {
        return Err(redact_known_secret(
            provider_error(status.as_u16(), &body),
            key.as_deref(),
        ));
    }
    let ids: Vec<(String, String)> = match profile.kind {
        ProviderKind::Ollama => body
            .get("models")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| {
                let id = item.get("name")?.as_str()?.to_string();
                Some((id.clone(), id))
            })
            .collect(),
        ProviderKind::Gemini => body
            .get("models")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| {
                let id = item
                    .get("name")?
                    .as_str()?
                    .trim_start_matches("models/")
                    .to_string();
                let name = item
                    .get("displayName")
                    .and_then(Value::as_str)
                    .unwrap_or(&id)
                    .to_string();
                Some((id, name))
            })
            .collect(),
        _ => body
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| {
                let id = item.get("id")?.as_str()?.to_string();
                Some((id.clone(), id))
            })
            .collect(),
    };
    let levels = vec![
        ThinkingLevel::Off,
        ThinkingLevel::Low,
        ThinkingLevel::Medium,
        ThinkingLevel::High,
        ThinkingLevel::Xhigh,
        ThinkingLevel::Auto,
    ];
    Ok(ids
        .into_iter()
        .map(|(id, display_name)| {
            let model_override = db.get_model_override(provider_id, &id).ok().flatten();
            ModelDescriptor {
                context_window: model_override
                    .as_ref()
                    .and_then(|value| value.context_window)
                    .or_else(|| Some(builtin_context_limit(&id))),
                max_output_tokens: model_override
                    .as_ref()
                    .and_then(|value| value.max_output_tokens),
                id,
                display_name,
                capabilities: model_override
                    .and_then(|value| value.capabilities)
                    .unwrap_or_else(|| ModelCapabilities {
                        tools: true,
                        vision: true,
                        reasoning: true,
                        reasoning_levels: levels.clone(),
                        usage_reporting: true,
                    }),
            }
        })
        .collect())
}

/// Streams provider text through `on_event` as the network response arrives. Dropping this
/// future drops the response body too, which is how the agent cancellation path aborts requests.
pub async fn generate_stream<F>(
    db: Arc<Database>,
    provider_id: &str,
    model_id: &str,
    messages: &[Message],
    config: &RunConfigSnapshot,
    plan_mcp_tools: &[AllowedMcpTool],
    mut on_event: F,
) -> Result<ProviderResponse, String>
where
    F: FnMut(ProviderStreamEvent),
{
    let profile = db.get_provider(provider_id)?;
    let key = credential(&db, &profile)?;
    let has_images = messages
        .iter()
        .any(|message| message.attachments.iter().any(|item| item.kind == "image"));
    if has_images {
        let vision_disabled = db
            .get_model_override(provider_id, model_id)?
            .and_then(|value| value.capabilities)
            .is_some_and(|capabilities| !capabilities.vision);
        if vision_disabled {
            return Err("当前模型不支持图片附件".to_string());
        }
        if matches!(
            profile.kind,
            ProviderKind::Anthropic | ProviderKind::Gemini | ProviderKind::Ollama
        ) {
            return Err(
                "此旧版兼容供应商适配器不支持图片附件，请改用 Responses API 或 Chat Completions 供应商"
                    .to_string(),
            );
        }
    }
    if !matches!(profile.kind, ProviderKind::Ollama) && key.is_none() {
        return Err(format!(
            "{} does not have an API key configured",
            profile.name
        ));
    }
    match profile.kind {
        ProviderKind::Anthropic => {
            generate_anthropic(
                &profile,
                key.as_deref(),
                model_id,
                messages,
                config,
                &mut on_event,
            )
            .await
        }
        ProviderKind::Gemini => {
            generate_gemini(
                &profile,
                key.as_deref(),
                model_id,
                messages,
                config,
                &mut on_event,
            )
            .await
        }
        ProviderKind::Ollama => {
            generate_ollama(&profile, model_id, messages, config, &mut on_event).await
        }
        _ if profile.api_type == ProviderApiType::Responses => {
            generate_responses(
                &profile,
                key.as_deref(),
                model_id,
                messages,
                config,
                plan_mcp_tools,
                &mut on_event,
            )
            .await
        }
        _ => {
            generate_openai_compatible(
                &profile,
                key.as_deref(),
                model_id,
                messages,
                config,
                plan_mcp_tools,
                &mut on_event,
            )
            .await
        }
    }
}

async fn checked_response(response: Response, secret: Option<&str>) -> Result<Response, String> {
    checked_response_for_api(response, secret, false).await
}

async fn checked_response_for_api(
    response: Response,
    secret: Option<&str>,
    responses_api: bool,
) -> Result<Response, String> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    let parsed = serde_json::from_str::<Value>(&body).unwrap_or_else(|_| json!({"message": body}));
    let lower = parsed.to_string().to_ascii_lowercase();
    if responses_api
        && (matches!(status.as_u16(), 404 | 405 | 501)
            || lower.contains("not implemented")
            || lower.contains("unsupported") && lower.contains("responses"))
    {
        return Err(
            "当前上游不支持 Responses API（/responses），请在供应商设置中切换为 Chat Completions。"
                .to_string(),
        );
    }
    Err(redact_known_secret(
        provider_error(status.as_u16(), &parsed),
        secret,
    ))
}

#[derive(Debug, Clone)]
struct StreamMetadata {
    content_type: String,
    content_encoding: String,
}

impl StreamMetadata {
    fn from_response(response: &Response) -> Self {
        let header = |name| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("not reported")
                .to_string()
        };
        Self {
            content_type: header(CONTENT_TYPE),
            content_encoding: header(reqwest::header::CONTENT_ENCODING),
        }
    }
}

fn body_looks_like_json(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
        .is_some_and(|byte| matches!(byte, b'{' | b'['))
}

async fn collect_response_body(
    mut response: Response,
    mut body: Vec<u8>,
    label: &str,
    metadata: &StreamMetadata,
) -> Result<Vec<u8>, String> {
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| stream_error(label, &error, metadata, false))?
    {
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn stream_error(
    label: &str,
    error: &reqwest::Error,
    metadata: &StreamMetadata,
    received_output: bool,
) -> String {
    let stage = if received_output {
        "在返回部分内容后"
    } else {
        "在首个文本到达前"
    };
    let reason = if error.is_timeout() {
        "上游长时间没有返回数据"
    } else if error.is_decode() {
        "上游或代理返回了无法解码的响应体"
    } else if error.is_body() {
        "上游响应体被提前中断"
    } else {
        "网络流被中断"
    };
    let source = error
        .source()
        .map(ToString::to_string)
        .filter(|value| value != &error.to_string())
        .map(|value| format!("；底层原因：{value}"))
        .unwrap_or_default();
    format!(
        "{label} {stage}中断：{reason}（Content-Type: {}，Content-Encoding: {}）：{error}{source}。Axiom 已请求 identity 编码；请重试，并检查上游服务或代理。",
        metadata.content_type, metadata.content_encoding
    )
}

async fn generate_openai_compatible<F>(
    profile: &ProviderProfile,
    key: Option<&str>,
    model: &str,
    messages: &[Message],
    config: &RunConfigSnapshot,
    plan_mcp_tools: &[AllowedMcpTool],
    on_event: &mut F,
) -> Result<ProviderResponse, String>
where
    F: FnMut(ProviderStreamEvent),
{
    let client = client(profile)?;
    let url = provider_endpoint(&profile.base_url, "chat/completions")?;
    let mapped: Vec<Value> = messages
        .iter()
        .map(chat_input_message)
        .collect::<Result<_, _>>()?;
    let mut body = json!({
        "model": model,
        "messages": mapped,
        "stream": true,
        "stream_options": {"include_usage": true},
        "tools": chat_tools(config.run_mode, plan_mcp_tools),
        "parallel_tool_calls": false
    });
    if let Some(max_tokens) = config.max_output_tokens {
        body["max_tokens"] = json!(max_tokens);
    }
    // New provider profiles are intentionally generic OpenAI-compatible profiles.
    // Forward the user's per-run thinking choice for every Chat Completions endpoint;
    // "off" remains omitted so non-reasoning models keep their default behavior.
    if let Some(level) = reasoning_effort(config.thinking_level) {
        body["reasoning_effort"] = json!(level);
    }
    let request = apply_headers(
        with_key(
            client
                .post(url)
                .header(CONTENT_TYPE, "application/json")
                .header(ACCEPT, "text/event-stream")
                .header(ACCEPT_ENCODING, "identity")
                .json(&body),
            profile.kind,
            key,
        ),
        profile,
    )?;
    let response = send_provider_request_with_retry(request).await?;
    let mut response = checked_response(response, key).await?;
    let stream_metadata = StreamMetadata::from_response(&response);
    let started = Instant::now();
    let mut first_token_ms = None;
    let mut text = String::new();
    let mut raw_usage: Option<Value> = None;
    let mut tool = ChatToolAccumulator::default();
    let mut decoder = SseDecoder::default();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        stream_error(
            "Provider stream",
            &error,
            &stream_metadata,
            !text.is_empty(),
        )
    })? {
        for data in decoder.feed(&chunk)? {
            if data == "[DONE]" {
                continue;
            }
            let value: Value = serde_json::from_str(&data)
                .map_err(|e| format!("Invalid provider SSE event: {e}"))?;
            if value.get("error").is_some() {
                return Err(redact_known_secret(provider_error(500, &value), key));
            }
            if value
                .pointer("/choices/0/finish_reason")
                .and_then(Value::as_str)
                == Some("content_filter")
            {
                return Err("Provider blocked the response through its content filter".to_string());
            }
            tool.observe(&value);
            if let Some(usage) = value.get("usage").filter(|v| !v.is_null()) {
                raw_usage = Some(usage.clone());
            }
            if let Some(delta) = openai_reasoning_delta(&value) {
                register_reasoning_delta(delta, on_event);
            }
            if let Some(delta) = openai_delta(&value) {
                register_delta(delta, &mut text, &mut first_token_ms, started, on_event);
            }
        }
    }
    for data in decoder.finish()? {
        if data != "[DONE]" {
            let value: Value = serde_json::from_str(&data)
                .map_err(|e| format!("Invalid provider SSE event: {e}"))?;
            if value.get("error").is_some() {
                return Err(redact_known_secret(provider_error(500, &value), key));
            }
            if value
                .pointer("/choices/0/finish_reason")
                .and_then(Value::as_str)
                == Some("content_filter")
            {
                return Err("Provider blocked the response through its content filter".to_string());
            }
            tool.observe(&value);
            if let Some(delta) = openai_reasoning_delta(&value) {
                register_reasoning_delta(delta, on_event);
            }
            if let Some(delta) = openai_delta(&value) {
                register_delta(delta, &mut text, &mut first_token_ms, started, on_event);
            }
            if let Some(usage) = value.get("usage").filter(|v| !v.is_null()) {
                raw_usage = Some(usage.clone());
            }
        }
    }
    finish_response_with_tool(
        text,
        raw_usage.as_ref(),
        messages,
        model,
        first_token_ms,
        tool.finish()?,
    )
}

fn responses_tools(run_mode: RunMode, plan_mcp_tools: &[AllowedMcpTool]) -> Vec<Value> {
    let mut tools = vec![
        json!({"type":"function","name":"list_files","description":"List files under a workspace-relative directory.","parameters":{"type":"object","properties":{"path":{"type":"string"}},"additionalProperties":false}}),
        json!({"type":"function","name":"read_file","description":"Read a UTF-8 workspace file.","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}}),
        json!({"type":"function","name":"search_files","description":"Search text in workspace files.","parameters":{"type":"object","properties":{"query":{"type":"string"},"path":{"type":"string"}},"required":["query"],"additionalProperties":false}}),
        json!({"type":"function","name":"git_status","description":"Read Git status for the workspace.","parameters":{"type":"object","properties":{},"additionalProperties":false}}),
        json!({"type":"function","name":"git_diff","description":"Read the current Git diff.","parameters":{"type":"object","properties":{},"additionalProperties":false}}),
    ];
    if run_mode == RunMode::Plan {
        tools.push(json!({
            "type":"function",
            "name":"ask_user",
            "description":"Ask the user one decision-critical clarification using a choice card. Use only when the answer materially changes the plan.",
            "parameters":{
                "type":"object",
                "properties":{
                    "question":{"type":"string","description":"One concise question."},
                    "options":{
                        "type":"array","minItems":2,"maxItems":3,
                        "items":{
                            "type":"object",
                            "properties":{
                                "id":{"type":"string"},
                                "label":{"type":"string"},
                                "description":{"type":"string"}
                            },
                            "required":["id","label","description"],
                            "additionalProperties":false
                        }
                    }
                },
                "required":["question","options"],
                "additionalProperties":false
            }
        }));
    }
    if run_mode != RunMode::Plan {
        tools.extend([
            json!({"type":"function","name":"write_file","description":"Write complete UTF-8 content to a workspace file.","parameters":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"],"additionalProperties":false}}),
            json!({"type":"function","name":"apply_patch","description":"Apply a unified patch to one workspace file.","parameters":{"type":"object","properties":{"path":{"type":"string"},"patch":{"type":"string"}},"required":["path","patch"],"additionalProperties":false}}),
            json!({"type":"function","name":"delete_file","description":"Delete a workspace file.","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}}),
            json!({"type":"function","name":"shell","description":"Run a project command under the active permission policy.","parameters":{"type":"object","properties":{"command":{"type":"string"}},"required":["command"],"additionalProperties":false}}),
        ]);
    }
    if run_mode != RunMode::Plan || !plan_mcp_tools.is_empty() {
        let pairs = plan_mcp_tools
            .iter()
            .map(|tool| format!("{}/{}", tool.server_id, tool.name))
            .collect::<Vec<_>>()
            .join(", ");
        let description = if run_mode == RunMode::Plan {
            format!("Call one MCP tool explicitly marked read-only and non-destructive. Allowed server/tool pairs: {pairs}")
        } else {
            "Call an enabled MCP tool.".to_string()
        };
        tools.push(json!({"type":"function","name":"mcp_call","description":description,"parameters":{"type":"object","properties":{"serverId":{"type":"string"},"tool":{"type":"string"},"arguments":{"type":"object"}},"required":["serverId","tool","arguments"],"additionalProperties":false}}));
    }
    tools
}

fn merge_tool_argument_delta(current: &mut String, incoming: &str) {
    if incoming.is_empty() || incoming == current {
        return;
    }
    let incoming_is_complete_object =
        serde_json::from_str::<Value>(incoming).is_ok_and(|value| value.is_object());
    let current_is_complete_object =
        serde_json::from_str::<Value>(current).is_ok_and(|value| value.is_object());

    // Compatible gateways sometimes repeat a complete cumulative arguments object in
    // a delta event. Treat a complete object as authoritative rather than producing
    // `{}{};` ordinary scalar/string fragments still append normally.
    if incoming_is_complete_object {
        current.clear();
        current.push_str(incoming);
    } else if !current_is_complete_object {
        current.push_str(incoming);
    }
}

#[derive(Debug, Default)]
struct ChatToolAccumulator {
    call_id: String,
    name: String,
    arguments: String,
    saw_full_message: bool,
}

impl ChatToolAccumulator {
    fn observe(&mut self, event: &Value) {
        // A few compatible gateways emit both streamed deltas and a final complete
        // message. The complete payload is authoritative; appending it would
        // duplicate the function name and JSON arguments.
        if let Some(tool) = event.pointer("/choices/0/message/tool_calls/0") {
            if let Some(value) = tool.get("id").and_then(Value::as_str) {
                self.call_id = value.to_string();
            }
            if let Some(value) = tool.pointer("/function/name").and_then(Value::as_str) {
                self.name = value.to_string();
            }
            if let Some(value) = tool.pointer("/function/arguments").and_then(Value::as_str) {
                self.arguments = value.to_string();
            }
            self.saw_full_message = true;
            return;
        }
        if self.saw_full_message {
            return;
        }
        let Some(tool) = event.pointer("/choices/0/delta/tool_calls/0") else {
            return;
        };
        if let Some(value) = tool.get("id").and_then(Value::as_str) {
            self.call_id = value.to_string();
        }
        if let Some(value) = tool.pointer("/function/name").and_then(Value::as_str) {
            self.name.push_str(value);
        }
        if let Some(value) = tool.pointer("/function/arguments").and_then(Value::as_str) {
            merge_tool_argument_delta(&mut self.arguments, value);
        }
    }

    fn finish(self) -> Result<Option<ProviderToolCall>, String> {
        if self.name.is_empty() {
            return Ok(None);
        }
        let arguments = if self.arguments.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&self.arguments).map_err(|error| {
                format!("Chat Completions function call returned invalid arguments: {error}")
            })?
        };
        Ok(Some(ProviderToolCall {
            call_id: if self.call_id.is_empty() {
                uuid::Uuid::new_v4().to_string()
            } else {
                self.call_id
            },
            name: self.name,
            arguments,
        }))
    }
}

fn chat_tools(run_mode: RunMode, plan_mcp_tools: &[AllowedMcpTool]) -> Vec<Value> {
    responses_tools(run_mode, plan_mcp_tools)
        .into_iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.get("name").cloned().unwrap_or(Value::Null),
                    "description": tool.get("description").cloned().unwrap_or(Value::Null),
                    "parameters": tool.get("parameters").cloned().unwrap_or_else(|| json!({"type":"object","properties":{}}))
                }
            })
        })
        .collect()
}

#[derive(Debug, Default)]
struct ResponsesToolAccumulator {
    call_id: String,
    name: String,
    arguments: String,
}

impl ResponsesToolAccumulator {
    fn observe(&mut self, event: &Value) {
        let event_type = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if matches!(
            event_type,
            "response.output_item.added" | "response.output_item.done"
        ) {
            if let Some(item) = event
                .get("item")
                .filter(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
            {
                if let Some(value) = item.get("call_id").and_then(Value::as_str) {
                    self.call_id = value.to_string();
                }
                if let Some(value) = item.get("name").and_then(Value::as_str) {
                    self.name = value.to_string();
                }
                if let Some(value) = item
                    .get("arguments")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                {
                    self.arguments = value.to_string();
                }
            }
        } else if event_type == "response.function_call_arguments.delta" {
            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                merge_tool_argument_delta(&mut self.arguments, delta);
            }
        } else if event_type == "response.function_call_arguments.done" {
            if let Some(value) = event.get("arguments").and_then(Value::as_str) {
                self.arguments = value.to_string();
            }
            if let Some(value) = event.get("call_id").and_then(Value::as_str) {
                self.call_id = value.to_string();
            }
            if let Some(value) = event.get("name").and_then(Value::as_str) {
                self.name = value.to_string();
            }
        }
    }

    fn finish(self) -> Result<Option<ProviderToolCall>, String> {
        if self.name.is_empty() {
            return Ok(None);
        }
        let arguments = if self.arguments.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&self.arguments).map_err(|error| {
                format!("Responses function call returned invalid arguments: {error}")
            })?
        };
        Ok(Some(ProviderToolCall {
            call_id: if self.call_id.is_empty() {
                uuid::Uuid::new_v4().to_string()
            } else {
                self.call_id
            },
            name: self.name,
            arguments,
        }))
    }
}

fn finish_responses_json<F>(
    body: &[u8],
    key: Option<&str>,
    messages: &[Message],
    model: &str,
    started: Instant,
    on_event: &mut F,
) -> Result<ProviderResponse, String>
where
    F: FnMut(ProviderStreamEvent),
{
    let value: Value = serde_json::from_slice(body)
        .map_err(|error| format!("Invalid Responses JSON response: {error}"))?;
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if value.get("error").is_some() || matches!(status, "failed" | "incomplete" | "cancelled") {
        return Err(redact_known_secret(
            provider_error(500, &json!({"response": value})),
            key,
        ));
    }

    let mut text = String::new();
    let mut first_token_ms = None;
    let reasoning = responses_json_reasoning(&value);
    if !reasoning.is_empty() {
        register_reasoning_delta(reasoning, on_event);
    }
    let output_text = responses_json_text(&value);
    if !output_text.is_empty() {
        register_delta(
            output_text,
            &mut text,
            &mut first_token_ms,
            started,
            on_event,
        );
    }
    let mut tool = ResponsesToolAccumulator::default();
    if let Some(items) = value.get("output").and_then(Value::as_array) {
        for item in items {
            tool.observe(&json!({"type":"response.output_item.done","item":item}));
        }
    }
    finish_response_with_tool(
        text,
        value.get("usage"),
        messages,
        model,
        first_token_ms,
        tool.finish()?,
    )
}

fn observe_responses_sse_event<F>(
    data: &str,
    key: Option<&str>,
    tool: &mut ResponsesToolAccumulator,
    text: &mut String,
    reasoning: &mut String,
    raw_usage: &mut Option<Value>,
    first_token_ms: &mut Option<u64>,
    started: Instant,
    on_event: &mut F,
) -> Result<(), String>
where
    F: FnMut(ProviderStreamEvent),
{
    if data == "[DONE]" {
        return Ok(());
    }
    let value: Value = serde_json::from_str(data)
        .map_err(|error| format!("Invalid Responses SSE event: {error}"))?;
    tool.observe(&value);
    match value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "response.output_text.delta" => {
            if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                register_delta(delta.to_string(), text, first_token_ms, started, on_event);
            }
        }
        "response.output_text.done" => {
            if text.is_empty() {
                if let Some(done) = value.get("text").and_then(Value::as_str) {
                    register_delta(done.to_string(), text, first_token_ms, started, on_event);
                }
            }
        }
        "response.reasoning_summary_text.delta"
        | "response.reasoning_text.delta"
        | "response.reasoning.delta" => {
            if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                reasoning.push_str(delta);
                register_reasoning_delta(delta.to_string(), on_event);
            }
        }
        "response.completed" => {
            *raw_usage = value.pointer("/response/usage").cloned();
            if text.is_empty() {
                let complete = value
                    .get("response")
                    .map(responses_json_text)
                    .unwrap_or_default();
                if !complete.is_empty() {
                    register_delta(complete, text, first_token_ms, started, on_event);
                }
            }
            if reasoning.is_empty() {
                let complete_reasoning = value
                    .get("response")
                    .map(responses_json_reasoning)
                    .unwrap_or_default();
                if !complete_reasoning.is_empty() {
                    reasoning.push_str(&complete_reasoning);
                    register_reasoning_delta(complete_reasoning, on_event);
                }
            }
        }
        "response.failed" | "response.incomplete" | "response.error" | "error" => {
            return Err(redact_known_secret(provider_error(500, &value), key));
        }
        _ => {}
    }
    Ok(())
}

async fn generate_responses<F>(
    profile: &ProviderProfile,
    key: Option<&str>,
    model: &str,
    messages: &[Message],
    config: &RunConfigSnapshot,
    plan_mcp_tools: &[AllowedMcpTool],
    on_event: &mut F,
) -> Result<ProviderResponse, String>
where
    F: FnMut(ProviderStreamEvent),
{
    let client = client(profile)?;
    let url = provider_endpoint(&profile.base_url, "responses")?;
    let input: Vec<Value> = messages
        .iter()
        .map(responses_input_item)
        .collect::<Result<_, _>>()?;
    let mut body = json!({
        "model": model,
        "input": input,
        "stream": true,
        "tools": responses_tools(config.run_mode, plan_mcp_tools),
        "parallel_tool_calls": false
    });
    if let Some(max_tokens) = config.max_output_tokens {
        body["max_output_tokens"] = json!(max_tokens);
    }
    if let Some(level) = reasoning_effort(config.thinking_level) {
        body["reasoning"] = json!({"effort": level});
    }
    let request = apply_headers(
        with_key(
            client
                .post(url)
                .header(CONTENT_TYPE, "application/json")
                .header(ACCEPT, "text/event-stream")
                .header(ACCEPT_ENCODING, "identity")
                .json(&body),
            profile.kind,
            key,
        ),
        profile,
    )?;
    let response = send_provider_request_with_retry(request).await?;
    let mut response = checked_response_for_api(response, key, true).await?;
    let stream_metadata = StreamMetadata::from_response(&response);
    let is_json = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("application/json"));
    let started = Instant::now();

    if is_json {
        let body = collect_response_body(
            response,
            Vec::new(),
            "Responses response body",
            &stream_metadata,
        )
        .await?;
        return finish_responses_json(&body, key, messages, model, started, on_event);
    }

    // Some OpenAI-compatible gateways ignore `Accept: text/event-stream` and return a
    // complete JSON object with a missing or `text/plain` Content-Type. Buffer only the
    // leading whitespace needed to sniff the body, then preserve normal token streaming.
    let mut initial = Vec::new();
    loop {
        let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| stream_error("Responses stream", &error, &stream_metadata, false))?
        else {
            break;
        };
        initial.extend_from_slice(&chunk);
        if initial.iter().any(|byte| !byte.is_ascii_whitespace()) {
            break;
        }
    }
    if body_looks_like_json(&initial) {
        let body = collect_response_body(
            response,
            initial,
            "Responses response body",
            &stream_metadata,
        )
        .await?;
        return finish_responses_json(&body, key, messages, model, started, on_event);
    }

    let mut first_token_ms = None;
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut raw_usage: Option<Value> = None;
    let mut tool = ResponsesToolAccumulator::default();
    let mut decoder = SseDecoder::default();
    for data in decoder.feed(&initial)? {
        observe_responses_sse_event(
            &data,
            key,
            &mut tool,
            &mut text,
            &mut reasoning,
            &mut raw_usage,
            &mut first_token_ms,
            started,
            on_event,
        )?;
    }
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        stream_error(
            "Responses stream",
            &error,
            &stream_metadata,
            !text.is_empty(),
        )
    })? {
        for data in decoder.feed(&chunk)? {
            observe_responses_sse_event(
                &data,
                key,
                &mut tool,
                &mut text,
                &mut reasoning,
                &mut raw_usage,
                &mut first_token_ms,
                started,
                on_event,
            )?;
        }
    }
    for data in decoder.finish()? {
        observe_responses_sse_event(
            &data,
            key,
            &mut tool,
            &mut text,
            &mut reasoning,
            &mut raw_usage,
            &mut first_token_ms,
            started,
            on_event,
        )?;
    }
    finish_response_with_tool(
        text,
        raw_usage.as_ref(),
        messages,
        model,
        first_token_ms,
        tool.finish()?,
    )
}

async fn generate_anthropic<F>(
    profile: &ProviderProfile,
    key: Option<&str>,
    model: &str,
    messages: &[Message],
    config: &RunConfigSnapshot,
    on_event: &mut F,
) -> Result<ProviderResponse, String>
where
    F: FnMut(ProviderStreamEvent),
{
    let client = client(profile)?;
    let url = format!("{}/v1/messages", profile.base_url.trim_end_matches('/'));
    let system = messages
        .iter()
        .filter(|m| m.role == MessageRole::System)
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let mapped: Vec<Value> = messages
        .iter()
        .filter(|m| m.role != MessageRole::System)
        .map(|m| {
            let role = if m.role == MessageRole::Assistant {
                "assistant"
            } else {
                "user"
            };
            json!({"role": role, "content": m.content})
        })
        .collect();
    let mut body = json!({
        "model": model,
        "messages": mapped,
        "max_tokens": config.max_output_tokens.unwrap_or(8192),
        "stream": true
    });
    if !system.is_empty() {
        body["system"] = Value::String(system);
    }
    if let Some(budget) = anthropic_thinking_budget(
        config.thinking_level,
        config.max_output_tokens.unwrap_or(8192),
    ) {
        body["thinking"] = json!({"type":"enabled", "budget_tokens": budget});
    }
    let request = apply_headers(
        with_key(client.post(url).json(&body), profile.kind, key),
        profile,
    )?;
    let response = request
        .send()
        .await
        .map_err(|e| format!("Anthropic request failed: {e}"))?;
    let mut response = checked_response(response, key).await?;
    let started = Instant::now();
    let mut first_token_ms = None;
    let mut text = String::new();
    let mut usage = Value::Object(Default::default());
    let mut decoder = SseDecoder::default();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Anthropic stream interrupted: {e}"))?
    {
        for data in decoder.feed(&chunk)? {
            let value: Value = serde_json::from_str(&data)
                .map_err(|e| format!("Invalid Anthropic SSE event: {e}"))?;
            merge_anthropic_usage(&mut usage, &value);
            if let Some(delta) = value.pointer("/delta/thinking").and_then(Value::as_str) {
                register_reasoning_delta(delta.to_string(), on_event);
            }
            if let Some(delta) = value.pointer("/delta/text").and_then(Value::as_str) {
                register_delta(
                    delta.to_string(),
                    &mut text,
                    &mut first_token_ms,
                    started,
                    on_event,
                );
            }
        }
    }
    for data in decoder.finish()? {
        let value: Value =
            serde_json::from_str(&data).map_err(|e| format!("Invalid Anthropic SSE event: {e}"))?;
        merge_anthropic_usage(&mut usage, &value);
        if let Some(delta) = value.pointer("/delta/thinking").and_then(Value::as_str) {
            register_reasoning_delta(delta.to_string(), on_event);
        }
        if let Some(delta) = value.pointer("/delta/text").and_then(Value::as_str) {
            register_delta(
                delta.to_string(),
                &mut text,
                &mut first_token_ms,
                started,
                on_event,
            );
        }
    }
    let mut response = finish_response(text, Some(&usage), messages, model, first_token_ms)?;
    response.usage.cached_tokens = usage.get("cache_read_input_tokens").and_then(Value::as_u64);
    Ok(response)
}

async fn generate_gemini<F>(
    profile: &ProviderProfile,
    key: Option<&str>,
    model: &str,
    messages: &[Message],
    config: &RunConfigSnapshot,
    on_event: &mut F,
) -> Result<ProviderResponse, String>
where
    F: FnMut(ProviderStreamEvent),
{
    let key = key.ok_or_else(|| "Google Gemini does not have an API key configured".to_string())?;
    let client = client(profile)?;
    let url = format!(
        "{}/v1beta/models/{}:streamGenerateContent?alt=sse",
        profile.base_url.trim_end_matches('/'),
        model
    );
    let contents: Vec<Value> = messages
        .iter()
        .filter(|m| m.role != MessageRole::System)
        .map(|m| {
            let role = if m.role == MessageRole::Assistant {
                "model"
            } else {
                "user"
            };
            json!({"role": role, "parts": [{"text": m.content}]})
        })
        .collect();
    let system_text = messages
        .iter()
        .filter(|m| m.role == MessageRole::System)
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut body = json!({"contents": contents});
    if !system_text.is_empty() {
        body["systemInstruction"] = json!({"parts": [{"text": system_text}]});
    }
    if let Some(max_tokens) = config.max_output_tokens {
        body["generationConfig"]["maxOutputTokens"] = json!(max_tokens);
    }
    let request = apply_headers(
        with_key(client.post(url).json(&body), profile.kind, Some(key)),
        profile,
    )?;
    let response = request
        .send()
        .await
        .map_err(|e| format!("Gemini request failed: {e}"))?;
    let mut response = checked_response(response, Some(key)).await?;
    let started = Instant::now();
    let mut first_token_ms = None;
    let mut text = String::new();
    let mut raw_usage: Option<Value> = None;
    let mut decoder = SseDecoder::default();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Gemini stream interrupted: {e}"))?
    {
        for data in decoder.feed(&chunk)? {
            let value: Value = serde_json::from_str(&data)
                .map_err(|e| format!("Invalid Gemini SSE event: {e}"))?;
            if let Some(current) = value.get("usageMetadata") {
                raw_usage = Some(current.clone());
            }
            for delta in gemini_reasoning_deltas(&value) {
                register_reasoning_delta(delta, on_event);
            }
            for delta in gemini_deltas(&value) {
                register_delta(delta, &mut text, &mut first_token_ms, started, on_event);
            }
        }
    }
    for data in decoder.finish()? {
        let value: Value =
            serde_json::from_str(&data).map_err(|e| format!("Invalid Gemini SSE event: {e}"))?;
        if let Some(current) = value.get("usageMetadata") {
            raw_usage = Some(current.clone());
        }
        for delta in gemini_reasoning_deltas(&value) {
            register_reasoning_delta(delta, on_event);
        }
        for delta in gemini_deltas(&value) {
            register_delta(delta, &mut text, &mut first_token_ms, started, on_event);
        }
    }
    finish_response(text, raw_usage.as_ref(), messages, model, first_token_ms)
}

async fn generate_ollama<F>(
    profile: &ProviderProfile,
    model: &str,
    messages: &[Message],
    config: &RunConfigSnapshot,
    on_event: &mut F,
) -> Result<ProviderResponse, String>
where
    F: FnMut(ProviderStreamEvent),
{
    let client = client(profile)?;
    let url = format!("{}/api/chat", profile.base_url.trim_end_matches('/'));
    let mapped: Vec<Value> = messages
        .iter()
        .map(|m| json!({"role": role_name(m.role), "content": m.content}))
        .collect();
    let mut options = json!({});
    if let Some(max_tokens) = config.max_output_tokens {
        options["num_predict"] = json!(max_tokens);
    }
    let response = apply_headers(
        client.post(url).json(&json!({
            "model": model, "messages": mapped, "stream": true, "options": options
        })),
        profile,
    )?
    .send()
    .await
    .map_err(|e| format!("Ollama request failed: {e}"))?;
    let mut response = checked_response(response, None).await?;
    let started = Instant::now();
    let mut first_token_ms = None;
    let mut text = String::new();
    let mut final_value: Option<Value> = None;
    let mut decoder = NdjsonDecoder::default();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Ollama stream interrupted: {e}"))?
    {
        for line in decoder.feed(&chunk)? {
            let value: Value = serde_json::from_str(&line)
                .map_err(|e| format!("Invalid Ollama stream event: {e}"))?;
            if let Some(delta) = value
                .pointer("/message/thinking")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                register_reasoning_delta(delta.to_string(), on_event);
            }
            if let Some(delta) = value
                .pointer("/message/content")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                register_delta(
                    delta.to_string(),
                    &mut text,
                    &mut first_token_ms,
                    started,
                    on_event,
                );
            }
            if value.get("done").and_then(Value::as_bool) == Some(true) {
                final_value = Some(value);
            }
        }
    }
    for line in decoder.finish()? {
        let value: Value =
            serde_json::from_str(&line).map_err(|e| format!("Invalid Ollama stream event: {e}"))?;
        if let Some(delta) = value
            .pointer("/message/thinking")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            register_reasoning_delta(delta.to_string(), on_event);
        }
        if let Some(delta) = value
            .pointer("/message/content")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            register_delta(
                delta.to_string(),
                &mut text,
                &mut first_token_ms,
                started,
                on_event,
            );
        }
        if value.get("done").and_then(Value::as_bool) == Some(true) {
            final_value = Some(value);
        }
    }
    let mut result = finish_response(text, None, messages, model, first_token_ms)?;
    if let Some(raw) = final_value {
        let input = raw.get("prompt_eval_count").and_then(Value::as_u64);
        let output = raw.get("eval_count").and_then(Value::as_u64);
        if input.is_some() {
            result.usage.input_tokens = input;
        }
        if output.is_some() {
            result.usage.output_tokens = output;
        }
        result.usage.estimated = input.is_none() || output.is_none();
    }
    Ok(result)
}

fn register_reasoning_delta<F>(delta: String, on_event: &mut F)
where
    F: FnMut(ProviderStreamEvent),
{
    if !delta.is_empty() {
        on_event(ProviderStreamEvent::ReasoningDelta(delta));
    }
}

fn register_delta<F>(
    delta: String,
    text: &mut String,
    first_token_ms: &mut Option<u64>,
    started: Instant,
    on_event: &mut F,
) where
    F: FnMut(ProviderStreamEvent),
{
    if delta.is_empty() {
        return;
    }
    if first_token_ms.is_none() {
        *first_token_ms = Some(started.elapsed().as_millis() as u64);
    }
    text.push_str(&delta);
    on_event(ProviderStreamEvent::TextDelta(delta));
}

const RESPONSES_CALL_PREFIX: &str = "__AXIOM_RESPONSES_FUNCTION_CALL__";
const RESPONSES_OUTPUT_PREFIX: &str = "__AXIOM_RESPONSES_FUNCTION_OUTPUT__";

pub fn responses_call_message(call: &ProviderToolCall) -> String {
    format!(
        "{RESPONSES_CALL_PREFIX}{}",
        json!({"call_id":call.call_id,"name":call.name,"arguments":call.arguments.to_string()})
    )
}

pub fn responses_output_message(call_id: &str, output: &str) -> String {
    format!(
        "{RESPONSES_OUTPUT_PREFIX}{}",
        json!({"call_id":call_id,"output":output})
    )
}

fn chat_input_message(message: &Message) -> Result<Value, String> {
    if let Some(payload) = message
        .content
        .strip_prefix(RESPONSES_CALL_PREFIX)
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
    {
        return Ok(json!({
            "role": "assistant",
            "content": Value::Null,
            "tool_calls": [{
                "id": payload.get("call_id").and_then(Value::as_str).unwrap_or_default(),
                "type": "function",
                "function": {
                    "name": payload.get("name").and_then(Value::as_str).unwrap_or_default(),
                    "arguments": payload.get("arguments").and_then(Value::as_str).unwrap_or("{}")
                }
            }]
        }));
    }
    if let Some(payload) = message
        .content
        .strip_prefix(RESPONSES_OUTPUT_PREFIX)
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
    {
        return Ok(json!({
            "role": "tool",
            "tool_call_id": payload.get("call_id").and_then(Value::as_str).unwrap_or_default(),
            "content": payload.get("output").and_then(Value::as_str).unwrap_or_default()
        }));
    }
    message_content(message, false)
        .map(|content| json!({"role": role_name(message.role), "content": content}))
}

fn responses_input_item(message: &Message) -> Result<Value, String> {
    if let Some(payload) = message
        .content
        .strip_prefix(RESPONSES_CALL_PREFIX)
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
    {
        return Ok(
            json!({"type":"function_call","call_id":payload.get("call_id").and_then(Value::as_str).unwrap_or_default(),"name":payload.get("name").and_then(Value::as_str).unwrap_or_default(),"arguments":payload.get("arguments").and_then(Value::as_str).unwrap_or("{}")}),
        );
    }
    if let Some(payload) = message
        .content
        .strip_prefix(RESPONSES_OUTPUT_PREFIX)
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
    {
        return Ok(
            json!({"type":"function_call_output","call_id":payload.get("call_id").and_then(Value::as_str).unwrap_or_default(),"output":payload.get("output").and_then(Value::as_str).unwrap_or_default()}),
        );
    }
    message_content(message, true)
        .map(|content| json!({"role": role_name(message.role), "content": content}))
}

fn chat_json_text(response: &Value) -> String {
    if let Some(text) = response
        .pointer("/choices/0/text")
        .or_else(|| response.get("response"))
        .or_else(|| response.get("text"))
        .and_then(Value::as_str)
    {
        return text.to_string();
    }
    response
        .pointer("/choices/0/message/content")
        .and_then(value_text)
        .unwrap_or_default()
}

fn responses_json_reasoning(response: &Value) -> String {
    response
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("reasoning"))
        .flat_map(|item| {
            item.get("summary")
                .or_else(|| item.get("content"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|part| {
            part.get("text")
                .and_then(Value::as_str)
                .or_else(|| part.get("content").and_then(Value::as_str))
        })
        .collect::<Vec<_>>()
        .join("")
}

fn responses_json_text(response: &Value) -> String {
    if let Some(text) = response
        .get("output_text")
        .or_else(|| response.get("response"))
        .or_else(|| response.get("text"))
        .and_then(Value::as_str)
    {
        return text.to_string();
    }
    response
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|item| {
            item.get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("output_text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("")
}

fn finish_response(
    text: String,
    raw_usage: Option<&Value>,
    messages: &[Message],
    model: &str,
    first_token_ms: Option<u64>,
) -> Result<ProviderResponse, String> {
    finish_response_with_tool(text, raw_usage, messages, model, first_token_ms, None)
}

fn finish_response_with_tool(
    text: String,
    raw_usage: Option<&Value>,
    messages: &[Message],
    model: &str,
    first_token_ms: Option<u64>,
    tool_call: Option<ProviderToolCall>,
) -> Result<ProviderResponse, String> {
    if text.is_empty() && tool_call.is_none() {
        return Err("Provider returned an empty response".to_string());
    }
    let mut usage = normalize_usage(raw_usage, &text, messages);
    usage.context_limit = builtin_context_limit(model);
    usage.first_token_ms = first_token_ms;
    Ok(ProviderResponse {
        text,
        usage,
        tool_call,
    })
}

fn message_content(message: &Message, responses: bool) -> Result<Value, String> {
    let images: Vec<&AttachmentSnapshot> = message
        .attachments
        .iter()
        .filter(|attachment| attachment.kind == "image")
        .collect();
    if images.is_empty() {
        return Ok(if responses {
            json!([{"type": "input_text", "text": message.content}])
        } else {
            Value::String(message.content.clone())
        });
    }
    let mut parts = vec![if responses {
        json!({"type": "input_text", "text": message.content})
    } else {
        json!({"type": "text", "text": message.content})
    }];
    for attachment in images {
        if !matches!(
            attachment.mime_type.as_str(),
            "image/png" | "image/jpeg" | "image/webp" | "image/gif"
        ) {
            return Err("图片附件格式不受支持，请重新添加附件".to_string());
        }
        let bytes = std::fs::read(&attachment.snapshot_path)
            .map_err(|_| "无法读取图片附件快照，请重新添加附件".to_string())?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        let data_url = format!("data:{};base64,{}", attachment.mime_type, encoded);
        parts.push(if responses {
            json!({"type": "input_image", "image_url": data_url})
        } else {
            json!({"type": "image_url", "image_url": {"url": data_url}})
        });
    }
    Ok(Value::Array(parts))
}

fn openai_reasoning_delta(value: &Value) -> Option<String> {
    ["reasoning_content", "reasoning", "thinking"]
        .into_iter()
        .find_map(|field| value.pointer(&format!("/choices/0/delta/{field}")))
        .and_then(value_text)
        .filter(|text| !text.is_empty())
}

fn openai_reasoning_text(value: &Value) -> String {
    ["reasoning_content", "reasoning", "thinking"]
        .into_iter()
        .find_map(|field| value.pointer(&format!("/choices/0/message/{field}")))
        .and_then(value_text)
        .unwrap_or_default()
}

fn value_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    value.as_array().map(|parts| {
        parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| part.get("content").and_then(Value::as_str))
            })
            .collect::<String>()
    })
}

fn openai_delta(value: &Value) -> Option<String> {
    let content = value.pointer("/choices/0/delta/content")?;
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }
    content
        .as_array()
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<String>()
        })
        .filter(|s| !s.is_empty())
}

fn gemini_deltas(value: &Value) -> Vec<String> {
    value
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|part| part.get("thought").and_then(Value::as_bool) != Some(true))
        .filter_map(|part| part.get("text").and_then(Value::as_str).map(str::to_string))
        .collect()
}

fn gemini_reasoning_deltas(value: &Value) -> Vec<String> {
    value
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|part| part.get("thought").and_then(Value::as_bool) == Some(true))
        .filter_map(|part| part.get("text").and_then(Value::as_str).map(str::to_string))
        .collect()
}

fn merge_anthropic_usage(target: &mut Value, event: &Value) {
    let source = event
        .pointer("/message/usage")
        .or_else(|| event.get("usage"));
    let Some(source) = source.and_then(Value::as_object) else {
        return;
    };
    let Some(target) = target.as_object_mut() else {
        return;
    };
    for (key, value) in source {
        target.insert(key.clone(), value.clone());
    }
}

fn normalize_usage(raw: Option<&Value>, text: &str, messages: &[Message]) -> UsageRecord {
    let estimated_input = messages
        .iter()
        .map(|message| estimate_tokens(&message.content))
        .sum();
    let estimated_output = estimate_tokens(text);
    let mut usage = UsageRecord {
        input_tokens: Some(estimated_input),
        output_tokens: Some(estimated_output),
        context_tokens: estimated_input,
        estimated: true,
        ..Default::default()
    };
    if let Some(raw) = raw {
        let input = raw
            .get("prompt_tokens")
            .or_else(|| raw.get("input_tokens"))
            .or_else(|| raw.get("promptTokenCount"))
            .and_then(Value::as_u64);
        let output = raw
            .get("completion_tokens")
            .or_else(|| raw.get("output_tokens"))
            .or_else(|| raw.get("candidatesTokenCount"))
            .and_then(Value::as_u64);
        usage.cached_tokens = raw
            .pointer("/prompt_tokens_details/cached_tokens")
            .or_else(|| raw.pointer("/input_tokens_details/cached_tokens"))
            .or_else(|| raw.get("cached_tokens"))
            .or_else(|| raw.get("cache_read_input_tokens"))
            .or_else(|| raw.get("cachedContentTokenCount"))
            .and_then(Value::as_u64);
        usage.reasoning_tokens = raw
            .pointer("/completion_tokens_details/reasoning_tokens")
            .or_else(|| raw.pointer("/output_tokens_details/reasoning_tokens"))
            .or_else(|| raw.get("reasoning_tokens"))
            .or_else(|| raw.get("thoughtsTokenCount"))
            .and_then(Value::as_u64);
        if input.is_some() {
            usage.input_tokens = input;
            usage.context_tokens = input.unwrap_or_default();
        }
        if output.is_some() {
            usage.output_tokens = output;
        }
        usage.estimated = input.is_none() || output.is_none();
    }
    usage
}

fn estimate_tokens(text: &str) -> u64 {
    ((text.chars().count() as f64) / 3.8).ceil() as u64
}

pub fn builtin_context_limit(model: &str) -> u64 {
    let model = model.to_ascii_lowercase();
    if model.contains("gemini-2.5") || model.contains("gemini-3") {
        1_048_576
    } else if model.contains("claude") {
        200_000
    } else if model.contains("qwen") {
        262_144
    } else if model.contains("gpt-4.1") {
        1_047_576
    } else if model.contains("gpt-5") || model.contains("o3") || model.contains("o4") {
        400_000
    } else {
        128_000
    }
}

fn reasoning_effort(level: ThinkingLevel) -> Option<&'static str> {
    match level {
        ThinkingLevel::Off => None,
        ThinkingLevel::Low => Some("low"),
        ThinkingLevel::Medium => Some("medium"),
        ThinkingLevel::High => Some("high"),
        ThinkingLevel::Xhigh => Some("xhigh"),
        ThinkingLevel::Auto => None,
    }
}

fn anthropic_thinking_budget(level: ThinkingLevel, max_output: u64) -> Option<u64> {
    let desired = match level {
        ThinkingLevel::Off | ThinkingLevel::Auto => return None,
        ThinkingLevel::Low => 1_024,
        ThinkingLevel::Medium => 4_096,
        ThinkingLevel::High => 8_192,
        ThinkingLevel::Xhigh => 16_384,
    };
    Some(desired.min(max_output.saturating_sub(1).max(1_024)))
}

fn role_name(role: MessageRole) -> &'static str {
    match role {
        MessageRole::System => "system",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
        MessageRole::User => "user",
    }
}

fn provider_error(status: u16, body: &Value) -> String {
    let message = body
        .pointer("/error/message")
        .or_else(|| body.pointer("/response/error/message"))
        .or_else(|| body.pointer("/response/incomplete_details/reason"))
        .or_else(|| body.pointer("/error/status"))
        .or_else(|| body.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("Unknown provider error");
    format!("Provider HTTP {status}: {message}")
}

#[derive(Default)]
struct SseDecoder {
    buffer: Vec<u8>,
    data_lines: Vec<String>,
}

impl SseDecoder {
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<String>, String> {
        self.buffer.extend_from_slice(bytes);
        let mut events = Vec::new();
        while let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let mut line = self.buffer.drain(..=position).collect::<Vec<_>>();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line = String::from_utf8(line)
                .map_err(|_| "Provider SSE was not valid UTF-8".to_string())?;
            if line.is_empty() {
                if !self.data_lines.is_empty() {
                    events.push(self.data_lines.drain(..).collect::<Vec<_>>().join("\n"));
                }
            } else if let Some(data) = line.strip_prefix("data:") {
                self.data_lines
                    .push(data.strip_prefix(' ').unwrap_or(data).to_string());
            }
        }
        Ok(events)
    }

    fn finish(&mut self) -> Result<Vec<String>, String> {
        if !self.buffer.is_empty() {
            let tail = String::from_utf8(std::mem::take(&mut self.buffer))
                .map_err(|_| "Provider SSE was not valid UTF-8".to_string())?;
            if let Some(data) = tail.trim_end_matches(['\r', '\n']).strip_prefix("data:") {
                self.data_lines
                    .push(data.strip_prefix(' ').unwrap_or(data).to_string());
            }
        }
        Ok(if self.data_lines.is_empty() {
            Vec::new()
        } else {
            vec![self.data_lines.drain(..).collect::<Vec<_>>().join("\n")]
        })
    }
}

#[derive(Default)]
struct NdjsonDecoder {
    buffer: Vec<u8>,
}

impl NdjsonDecoder {
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<String>, String> {
        self.buffer.extend_from_slice(bytes);
        let mut lines = Vec::new();
        while let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let mut line = self.buffer.drain(..=position).collect::<Vec<_>>();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if !line.is_empty() {
                lines.push(
                    String::from_utf8(line)
                        .map_err(|_| "Ollama stream was not valid UTF-8".to_string())?,
                );
            }
        }
        Ok(lines)
    }

    fn finish(&mut self) -> Result<Vec<String>, String> {
        if self.buffer.is_empty() {
            return Ok(Vec::new());
        }
        let line = String::from_utf8(std::mem::take(&mut self.buffer))
            .map_err(|_| "Ollama stream was not valid UTF-8".to_string())?;
        Ok(if line.trim().is_empty() {
            Vec::new()
        } else {
            vec![line]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_retry_backoff_increases_by_five_seconds() {
        assert_eq!(provider_retry_delay(1), Duration::from_secs(5));
        assert_eq!(provider_retry_delay(2), Duration::from_secs(10));
        assert_eq!(provider_retry_delay(5), Duration::from_secs(25));
    }

    #[test]
    fn sse_decoder_handles_split_chunks_and_multiline_events() {
        let mut decoder = SseDecoder::default();
        assert!(decoder
            .feed(b"event: message\ndata: {\"a\":")
            .unwrap()
            .is_empty());
        let events = decoder.feed(b"1}\ndata: second\n\n").unwrap();
        assert_eq!(events, vec!["{\"a\":1}\nsecond"]);
    }

    #[test]
    fn ndjson_decoder_handles_partial_lines() {
        let mut decoder = NdjsonDecoder::default();
        assert_eq!(decoder.feed(b"{\"a\":1").unwrap(), Vec::<String>::new());
        assert_eq!(
            decoder.feed(b"}\n{\"b\":2}\n").unwrap(),
            vec!["{\"a\":1}", "{\"b\":2}"]
        );
    }

    #[test]
    fn provider_endpoint_removes_known_terminal_paths_without_dropping_v1() {
        assert_eq!(
            provider_endpoint("https://api.example.test/v1/", "models").unwrap(),
            "https://api.example.test/v1/models"
        );
        assert_eq!(
            provider_endpoint("https://api.example.test/v1/models", "responses").unwrap(),
            "https://api.example.test/v1/responses"
        );
        assert_eq!(
            provider_endpoint(
                "https://api.example.test/v1/chat/completions",
                "chat/completions"
            )
            .unwrap(),
            "https://api.example.test/v1/chat/completions"
        );
        assert_eq!(
            provider_endpoint("https://api.example.test/v1/responses/responses", "models").unwrap(),
            "https://api.example.test/v1/models"
        );
    }

    #[test]
    fn provider_errors_redact_the_active_api_key() {
        let secret = "sk-private-value";
        let error = redact_known_secret(
            provider_error(
                401,
                &json!({"error":{"message":format!("invalid {secret}")}}),
            ),
            Some(secret),
        );
        assert!(!error.contains(secret));
        assert!(error.contains("[REDACTED]"));
    }

    #[test]
    fn chat_tool_arguments_support_streamed_deltas() {
        let mut accumulator = ChatToolAccumulator::default();
        accumulator.observe(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-chat","type":"function","function":{"name":"read_file","arguments":"{\"path\":"}}]}}]}));
        accumulator.observe(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"src/main.rs\"}"}}]}}]}));
        let call = accumulator.finish().unwrap().unwrap();
        assert_eq!(call.call_id, "call-chat");
        assert_eq!(call.name, "read_file");
        assert_eq!(call.arguments, json!({"path":"src/main.rs"}));
    }

    #[test]
    fn chat_repeated_complete_argument_deltas_do_not_create_trailing_json() {
        let mut accumulator = ChatToolAccumulator::default();
        accumulator.observe(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-chat","function":{"name":"list_files","arguments":"{}"}}]}}]}));
        accumulator.observe(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{}"}}]}}]}));
        let call = accumulator.finish().unwrap().unwrap();
        assert_eq!(call.arguments, json!({}));

        let mut cumulative = ChatToolAccumulator::default();
        cumulative.observe(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-chat-2","function":{"name":"read_file","arguments":"{\"path\":\"README.md\"}"}}]}}]}));
        cumulative.observe(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\":\"README.md\"}"}}]}}]}));
        assert_eq!(
            cumulative.finish().unwrap().unwrap().arguments,
            json!({"path":"README.md"})
        );
    }

    #[test]
    fn chat_invalid_arguments_still_fail() {
        let mut accumulator = ChatToolAccumulator::default();
        accumulator.observe(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"read_file","arguments":"{broken"}}]}}]}));
        assert!(accumulator
            .finish()
            .unwrap_err()
            .contains("invalid arguments"));
    }

    #[test]
    fn chat_complete_tool_payload_replaces_streamed_fragments() {
        let mut accumulator = ChatToolAccumulator::default();
        accumulator.observe(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-chat","type":"function","function":{"name":"read_","arguments":"{\"path\":"}}]}}]}));
        accumulator.observe(&json!({"choices":[{"message":{"tool_calls":[{"index":0,"id":"call-chat","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"README.md\"}"}}]}}]}));
        let call = accumulator.finish().unwrap().unwrap();
        assert_eq!(call.name, "read_file");
        assert_eq!(call.arguments, json!({"path":"README.md"}));
    }

    #[test]
    fn chat_function_messages_and_plan_tools_use_native_protocol_shapes() {
        let call = ProviderToolCall {
            call_id: "call-chat".into(),
            name: "read_file".into(),
            arguments: json!({"path":"README.md"}),
        };
        let call_message = Message {
            id: "call".into(),
            thread_id: "thread".into(),
            role: MessageRole::Assistant,
            content: responses_call_message(&call),
            created_at: "now".into(),
            run_id: None,
            pinned: false,
            attachments: Vec::new(),
        };
        let output_message = Message {
            id: "output".into(),
            thread_id: "thread".into(),
            role: MessageRole::User,
            content: responses_output_message("call-chat", "ok"),
            created_at: "now".into(),
            run_id: None,
            pinned: false,
            attachments: Vec::new(),
        };
        let mapped_call = chat_input_message(&call_message).unwrap();
        let mapped_output = chat_input_message(&output_message).unwrap();
        assert_eq!(mapped_call["role"], "assistant");
        assert_eq!(
            mapped_call["tool_calls"][0]["function"]["name"],
            "read_file"
        );
        assert_eq!(mapped_output["role"], "tool");
        assert_eq!(mapped_output["tool_call_id"], "call-chat");
        let plan_tools = chat_tools(RunMode::Plan, &[]);
        assert!(!plan_tools
            .iter()
            .any(|tool| tool["function"]["name"] == "shell"));
        assert!(plan_tools.iter().all(|tool| tool["type"] == "function"));
    }

    #[test]
    fn responses_tool_arguments_support_deltas_and_full_done_payloads() {
        let mut accumulator = ResponsesToolAccumulator::default();
        accumulator.observe(&json!({"type":"response.output_item.added","item":{"type":"function_call","call_id":"call-1","name":"read_file","arguments":""}}));
        accumulator.observe(
            &json!({"type":"response.function_call_arguments.delta","delta":"{\"path\":"}),
        );
        accumulator.observe(
            &json!({"type":"response.function_call_arguments.delta","delta":"\"src/main.rs\"}"}),
        );
        let call = accumulator.finish().unwrap().unwrap();
        assert_eq!(call.call_id, "call-1");
        assert_eq!(call.name, "read_file");
        assert_eq!(call.arguments, json!({"path":"src/main.rs"}));

        let mut full = ResponsesToolAccumulator::default();
        full.observe(&json!({"type":"response.output_item.done","item":{"type":"function_call","call_id":"call-2","name":"search_files","arguments":"{\"query\":\"Goal\"}"}}));
        assert_eq!(
            full.finish().unwrap().unwrap().arguments,
            json!({"query":"Goal"})
        );
    }

    #[test]
    fn responses_tool_arguments_reject_invalid_json() {
        let mut accumulator = ResponsesToolAccumulator::default();
        accumulator.observe(&json!({"type":"response.output_item.done","item":{"type":"function_call","name":"read_file","arguments":"{"}}));
        assert!(accumulator
            .finish()
            .unwrap_err()
            .contains("invalid arguments"));
    }

    #[test]
    fn responses_function_items_round_trip_without_visible_protocol_text() {
        let call = ProviderToolCall {
            call_id: "call-1".into(),
            name: "read_file".into(),
            arguments: json!({"path":"README.md"}),
        };
        let call_message = Message {
            id: "call".into(),
            thread_id: "thread".into(),
            role: MessageRole::Assistant,
            content: responses_call_message(&call),
            created_at: "now".into(),
            run_id: None,
            pinned: false,
            attachments: Vec::new(),
        };
        let output_message = Message {
            id: "output".into(),
            thread_id: "thread".into(),
            role: MessageRole::User,
            content: responses_output_message("call-1", "ok"),
            created_at: "now".into(),
            run_id: None,
            pinned: false,
            attachments: Vec::new(),
        };
        assert_eq!(
            responses_input_item(&call_message).unwrap()["type"],
            "function_call"
        );
        assert_eq!(
            responses_input_item(&call_message).unwrap()["arguments"],
            "{\"path\":\"README.md\"}"
        );
        assert_eq!(
            responses_input_item(&output_message).unwrap()["type"],
            "function_call_output"
        );
        assert_eq!(
            responses_input_item(&output_message).unwrap()["output"],
            "ok"
        );
    }

    #[test]
    fn responses_accepts_tool_only_results_and_filters_plan_mcp_tools() {
        let call = ProviderToolCall {
            call_id: "call".into(),
            name: "read_file".into(),
            arguments: json!({}),
        };
        let response =
            finish_response_with_tool(String::new(), None, &[], "model", None, Some(call));
        assert!(response.is_ok());
        assert!(!responses_tools(RunMode::Plan, &[])
            .iter()
            .any(|tool| tool["name"] == "mcp_call"));
        let allowed = [AllowedMcpTool {
            server_id: "docs".into(),
            name: "search".into(),
        }];
        let tools = responses_tools(RunMode::Plan, &allowed);
        let mcp = tools
            .iter()
            .find(|tool| tool["name"] == "mcp_call")
            .unwrap();
        assert!(mcp["description"].as_str().unwrap().contains("docs/search"));
        assert!(!tools.iter().any(|tool| tool["name"] == "shell"));
    }

    #[test]
    fn image_attachments_are_encoded_for_chat_and_responses() {
        let path =
            std::env::temp_dir().join(format!("axiom-provider-image-{}.png", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"image-bytes").unwrap();
        let message = Message {
            id: "image".into(),
            thread_id: "thread".into(),
            role: MessageRole::User,
            content: "inspect this image".into(),
            created_at: "now".into(),
            run_id: None,
            pinned: false,
            attachments: vec![AttachmentSnapshot {
                id: "attachment".into(),
                name: "preview.png".into(),
                mime_type: "image/png".into(),
                size: 11,
                sha256: "hash".into(),
                snapshot_path: path.to_string_lossy().to_string(),
                kind: "image".into(),
            }],
        };

        let chat = message_content(&message, false).unwrap();
        assert_eq!(chat[0]["type"], "text");
        assert!(chat[1]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));

        let responses = responses_input_item(&message).unwrap();
        assert_eq!(responses["content"][0]["type"], "input_text");
        assert!(responses["content"][1]["image_url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn missing_image_snapshot_returns_a_redacted_error() {
        let missing = std::env::temp_dir().join(format!(
            "axiom-provider-missing-{}.png",
            uuid::Uuid::new_v4()
        ));
        let message = Message {
            id: "image".into(),
            thread_id: "thread".into(),
            role: MessageRole::User,
            content: "inspect this image".into(),
            created_at: "now".into(),
            run_id: None,
            pinned: false,
            attachments: vec![AttachmentSnapshot {
                id: "attachment".into(),
                name: "private-name.png".into(),
                mime_type: "image/png".into(),
                size: 1,
                sha256: "hash".into(),
                snapshot_path: missing.to_string_lossy().to_string(),
                kind: "image".into(),
            }],
        };

        let error = responses_input_item(&message).unwrap_err();
        assert!(error.contains("无法读取图片附件快照"));
        assert!(!error.contains(missing.to_string_lossy().as_ref()));
        assert!(!error.contains("private-name.png"));
    }

    #[test]
    fn usage_normalization_marks_partial_usage_as_estimated() {
        let messages = vec![Message {
            id: "1".into(),
            thread_id: "t".into(),
            role: MessageRole::User,
            content: "hello".into(),
            created_at: "now".into(),
            run_id: None,
            pinned: false,
            attachments: Vec::new(),
        }];
        let usage = normalize_usage(Some(&json!({"prompt_tokens": 12})), "answer", &messages);
        assert_eq!(usage.input_tokens, Some(12));
        assert!(usage.output_tokens.is_some());
        assert!(usage.estimated);
        assert_eq!(usage.cached_tokens, None);
    }

    #[test]
    fn sse_decoder_handles_crlf_and_final_event_without_blank_line() {
        let mut decoder = SseDecoder::default();
        assert_eq!(
            decoder.feed(b"data: {\"delta\":\"a\"}\r\n\r\n").unwrap(),
            vec!["{\"delta\":\"a\"}"]
        );
        assert!(decoder.feed(b"data: {\"delta\":\"b\"}").unwrap().is_empty());
        assert_eq!(decoder.finish().unwrap(), vec!["{\"delta\":\"b\"}"]);
    }

    #[test]
    fn response_body_sniffing_accepts_json_without_content_type() {
        assert!(body_looks_like_json(b"  \r\n {\"output_text\":\"hello\"}"));
        assert!(body_looks_like_json(b"[1, 2, 3]"));
        assert!(!body_looks_like_json(
            b"data: {\"type\":\"response.completed\"}"
        ));
        assert!(!body_looks_like_json(b"   \r\n"));
    }

    #[test]
    fn complete_json_text_is_extracted_for_chat_and_responses() {
        assert_eq!(
            chat_json_text(&json!({"choices":[{"message":{"content":"hello"}}]})),
            "hello"
        );
        assert_eq!(
            chat_json_text(&json!({"choices":[{"message":{"content":[
                {"type":"text","text":"hel"},
                {"type":"text","content":"lo"}
            ]}}]})),
            "hello"
        );
        assert_eq!(
            responses_json_text(&json!({"output_text":"hello"})),
            "hello"
        );
        assert_eq!(
            responses_json_text(&json!({"output":[{"type":"message","content":[
                {"type":"output_text","text":"hel"},
                {"type":"output_text","text":"lo"}
            ]}]})),
            "hello"
        );
    }

    #[test]
    fn model_test_parser_accepts_json_sse_and_reasoning_only_responses() {
        let chat = parse_model_test_body(
            ProviderApiType::ChatCompletions,
            br#"{"choices":[{"message":{"content":"hello"}}],"usage":{"prompt_tokens":1,"completion_tokens":1}}"#,
        )
        .unwrap();
        assert_eq!(chat.text, "hello");
        assert!(chat.usage.is_some());

        let streamed_chat = parse_model_test_body(
            ProviderApiType::ChatCompletions,
            b"data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\ndata: [DONE]\n\n",
        )
        .unwrap();
        assert_eq!(streamed_chat.text, "hello");

        let reasoning_only = parse_model_test_body(
            ProviderApiType::ChatCompletions,
            br#"{"choices":[{"message":{"content":null,"reasoning_content":"thinking"}}]}"#,
        )
        .unwrap();
        assert!(reasoning_only.text.is_empty());
        assert_eq!(reasoning_only.reasoning, "thinking");

        let responses = parse_model_test_body(
            ProviderApiType::Responses,
            b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\n",
        )
        .unwrap();
        assert_eq!(responses.text, "hello");
        assert!(responses.usage.is_some());
    }

    #[test]
    fn reasoning_extractors_keep_gemini_thoughts_out_of_answer_text() {
        let chat = json!({"choices":[{"delta":{"reasoning_content":"step"}}]});
        assert_eq!(openai_reasoning_delta(&chat).as_deref(), Some("step"));

        let gemini = json!({"candidates":[{"content":{"parts":[
            {"thought":true,"text":"private reasoning"},
            {"text":"public answer"}
        ]}}]});
        assert_eq!(gemini_reasoning_deltas(&gemini), vec!["private reasoning"]);
        assert_eq!(gemini_deltas(&gemini), vec!["public answer"]);
    }

    #[test]
    fn plan_protocols_both_expose_ask_user_and_exclude_shell() {
        let responses = responses_tools(RunMode::Plan, &[]);
        assert!(responses.iter().any(|tool| tool["name"] == "ask_user"));
        assert!(!responses.iter().any(|tool| tool["name"] == "shell"));

        let chat = chat_tools(RunMode::Plan, &[]);
        assert!(chat
            .iter()
            .any(|tool| tool["function"]["name"] == "ask_user"));
        assert!(!chat.iter().any(|tool| tool["function"]["name"] == "shell"));
    }
}
