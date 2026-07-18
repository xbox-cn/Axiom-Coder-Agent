mod db;
mod mcp;
mod models;
mod provider;
mod secrets;
mod tools;

use crate::{db::Database, models::*};
use chrono::Utc;
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, State};
use tokio::sync::{oneshot, watch};

struct RunningRun {
    thread_id: String,
    workspace_path: String,
    writable: bool,
    cancel: watch::Sender<bool>,
}

struct AppState {
    db: Arc<Database>,
    running: Arc<Mutex<HashMap<String, RunningRun>>>,
    pending_approvals: Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>,
}

const MAX_ATTACHMENTS: usize = 10;
const MAX_TEXT_ATTACHMENT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_IMAGE_ATTACHMENT_BYTES: u64 = 10 * 1024 * 1024;
const MAX_TOTAL_ATTACHMENT_BYTES: u64 = 20 * 1024 * 1024;

fn attachments_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("attachments"))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn bootstrap(state: State<'_, AppState>) -> Result<AppBootstrap, String> {
    state.db.bootstrap()
}

#[tauri::command]
fn add_project(state: State<'_, AppState>, path: String) -> Result<Project, String> {
    state.db.add_project(Path::new(&path))
}

#[tauri::command]
fn create_thread(
    state: State<'_, AppState>,
    project_id: String,
    title: Option<String>,
) -> Result<ThreadSummary, String> {
    state.db.create_thread(&project_id, title.as_deref())
}

#[tauri::command]
fn get_thread(state: State<'_, AppState>, thread_id: String) -> Result<ThreadDetail, String> {
    state.db.get_thread(&thread_id)
}

#[tauri::command]
fn archive_thread(
    state: State<'_, AppState>,
    thread_id: String,
    archived: bool,
) -> Result<(), String> {
    if state
        .running
        .lock()
        .values()
        .any(|run| run.thread_id == thread_id)
    {
        return Err("运行中的任务不能归档，请先停止运行".to_string());
    }
    state.db.archive_thread(&thread_id, archived)
}

#[tauri::command]
fn delete_thread(state: State<'_, AppState>, thread_id: String) -> Result<(), String> {
    if state
        .running
        .lock()
        .values()
        .any(|run| run.thread_id == thread_id)
    {
        return Err("运行中的任务不能删除，请先停止运行".to_string());
    }
    state.db.delete_thread(&thread_id)
}

#[tauri::command]
fn restore_context_snapshot(state: State<'_, AppState>, snapshot_id: String) -> Result<(), String> {
    state.db.restore_context_snapshot(&snapshot_id)
}

#[tauri::command]
fn save_provider(
    state: State<'_, AppState>,
    input: ProviderProfileInput,
) -> Result<ProviderProfile, String> {
    let id = input
        .id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let credential_ref =
        if let Some(secret) = input.api_key.as_deref().filter(|v| !v.trim().is_empty()) {
            let reference = format!("provider:{id}");
            provider::store_api_key(&reference, secret)?;
            Some(reference)
        } else {
            None
        };
    let mut normalized = input;
    normalized.extra_headers = provider::protect_extra_headers(&id, &normalized.extra_headers)?;
    normalized.id = Some(id);
    state
        .db
        .save_provider(&normalized, credential_ref.as_deref())
}

#[tauri::command]
fn delete_provider(state: State<'_, AppState>, provider_id: String) -> Result<(), String> {
    let profile = state.db.get_provider(&provider_id)?;
    let api_credential = state.db.get_provider_credential_ref(&provider_id)?;
    state.db.delete_provider(&provider_id)?;
    if let Some(reference) = api_credential {
        let _ = secrets::delete(&reference);
    }
    for reference in secrets::references_in(&profile.extra_headers) {
        let _ = secrets::delete(&reference);
    }
    Ok(())
}

#[tauri::command]
fn get_model_override(
    state: State<'_, AppState>,
    provider_id: String,
    model_id: String,
) -> Result<Option<ModelOverride>, String> {
    state.db.get_model_override(&provider_id, &model_id)
}

#[tauri::command]
fn save_model_override(
    state: State<'_, AppState>,
    value: ModelOverride,
) -> Result<ModelOverride, String> {
    state.db.save_model_override(&value)
}

#[tauri::command]
async fn discover_models(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<ModelDescriptor>, String> {
    provider::discover_models(state.db.clone(), &provider_id).await
}

#[tauri::command]
async fn discover_provider_models_draft(
    api_type: ProviderApiType,
    base_url: String,
    api_key: Option<String>,
) -> Result<Vec<ModelDescriptor>, String> {
    provider::discover_models_draft(api_type, &base_url, api_key.as_deref()).await
}

#[tauri::command]
async fn test_provider_model_draft(
    state: State<'_, AppState>,
    provider_id: Option<String>,
    api_type: ProviderApiType,
    base_url: String,
    api_key: Option<String>,
    model_id: String,
) -> Result<DraftModelTestResult, String> {
    // A draft key lives only for this command. When editing an existing provider, an empty
    // key reuses its Credential Manager entry without exposing it to the frontend.
    let draft_key = api_key.filter(|value| !value.trim().is_empty());
    let stored_key = if draft_key.is_none() {
        provider_id
            .as_deref()
            .and_then(|id| state.db.get_provider_credential_ref(id).ok().flatten())
            .and_then(|reference| provider::load_api_key(&reference))
    } else {
        None
    };
    provider::test_model_draft(
        api_type,
        &base_url,
        draft_key.as_deref().or(stored_key.as_deref()),
        &model_id,
    )
    .await
}

#[tauri::command]
fn prepare_attachments(
    app: tauri::AppHandle,
    paths: Vec<String>,
) -> Result<Vec<AttachmentSnapshot>, String> {
    prepare_attachment_paths(&attachments_root(&app)?, paths)
}

fn prepare_attachment_paths(
    root: &Path,
    paths: Vec<String>,
) -> Result<Vec<AttachmentSnapshot>, String> {
    if paths.len() > MAX_ATTACHMENTS {
        return Err(format!("最多只能添加 {MAX_ATTACHMENTS} 个附件"));
    }
    fs::create_dir_all(root).map_err(|error| format!("附件操作失败: {error}"))?;
    let mut total = 0u64;
    let mut snapshots = Vec::new();
    for value in paths {
        let path = PathBuf::from(&value);
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("无法读取附件信息 {}: {error}", path.display()))?;
        if !metadata.is_file() {
            return Err(format!("附件不是普通文件: {}", path.display()));
        }
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "附件名称不是有效的 Unicode".to_string())?
            .to_string();
        if metadata.len() > MAX_IMAGE_ATTACHMENT_BYTES {
            return Err(format!("附件 {name} 超过单文件大小限制"));
        }
        let bytes = fs::read(&path).map_err(|error| format!("读取附件 {name} 失败: {error}"))?;
        let (kind, mime_type, limit) = classify_attachment_bytes(&bytes)
            .ok_or_else(|| format!("不支持的二进制文件或无法解码的文本: {name}"))?;
        if metadata.len() > limit {
            return Err(format!("附件 {name} 超过单文件大小限制"));
        }
        total = total.saturating_add(metadata.len());
        if total > MAX_TOTAL_ATTACHMENT_BYTES {
            return Err("附件总大小不能超过 20MB".to_string());
        }
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let snapshot_name = if extension.is_empty() {
            sha256.clone()
        } else {
            format!("{sha256}.{extension}")
        };
        let snapshot_path = root.join(snapshot_name);
        if !snapshot_path.exists() {
            fs::write(&snapshot_path, &bytes).map_err(|error| format!("附件操作失败: {error}"))?;
        }
        snapshots.push(AttachmentSnapshot {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            mime_type: mime_type.to_string(),
            size: metadata.len(),
            sha256,
            snapshot_path: snapshot_path.to_string_lossy().to_string(),
            kind: kind.to_string(),
        });
    }
    Ok(snapshots)
}

fn detected_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

fn classify_attachment_bytes(bytes: &[u8]) -> Option<(&'static str, &'static str, u64)> {
    if let Some(mime_type) = detected_image_mime(bytes) {
        Some(("image", mime_type, MAX_IMAGE_ATTACHMENT_BYTES))
    } else if decode_text_attachment(bytes).is_some() {
        Some(("text", "text/plain", MAX_TEXT_ATTACHMENT_BYTES))
    } else {
        None
    }
}

/// Revalidates every field supplied over IPC. The UI only receives opaque attachment metadata;
/// it is not trusted to choose an arbitrary file or to relabel binary content as text/image.
fn validate_attachment_snapshots(
    attachment_root: &Path,
    attachments: Vec<AttachmentSnapshot>,
) -> Result<Vec<AttachmentSnapshot>, String> {
    if attachments.len() > MAX_ATTACHMENTS {
        return Err(format!("最多只能添加 {MAX_ATTACHMENTS} 个附件"));
    }
    if attachments.is_empty() {
        return Ok(Vec::new());
    }

    let root = attachment_root
        .canonicalize()
        .map_err(|_| "附件存储目录不可用，请重新添加附件".to_string())?;
    let mut total = 0u64;
    let mut validated = Vec::with_capacity(attachments.len());

    for attachment in attachments {
        let original_path = PathBuf::from(&attachment.snapshot_path);
        let link_metadata = fs::symlink_metadata(&original_path)
            .map_err(|_| "附件快照不存在，请重新添加附件".to_string())?;
        if link_metadata.file_type().is_symlink() {
            return Err("附件快照路径不安全，请重新添加附件".to_string());
        }
        let path = tools::guard_path(&root, &original_path)
            .map_err(|_| "附件快照路径不安全，请重新添加附件".to_string())?;
        let metadata =
            fs::metadata(&path).map_err(|_| "附件快照无法读取，请重新添加附件".to_string())?;
        if !metadata.is_file() {
            return Err("附件快照不是普通文件，请重新添加附件".to_string());
        }

        let name = attachment.name.trim();
        if name.is_empty() || name.contains(['/', '\\']) {
            return Err("附件名称无效，请重新添加附件".to_string());
        }
        let bytes = fs::read(&path).map_err(|_| "附件快照无法读取，请重新添加附件".to_string())?;
        let actual_sha256 = format!("{:x}", Sha256::digest(&bytes));
        if !attachment.sha256.eq_ignore_ascii_case(&actual_sha256) {
            return Err(format!("附件 {name} 的内容校验失败，请重新添加"));
        }
        let stored_stem = path.file_stem().and_then(|value| value.to_str());
        if stored_stem != Some(actual_sha256.as_str()) {
            return Err("附件快照文件名校验失败，请重新添加附件".to_string());
        }

        let (kind, mime_type, limit) = classify_attachment_bytes(&bytes)
            .ok_or_else(|| format!("不支持的二进制文件或无法解码的文本: {name}"))?;
        if metadata.len() > limit {
            return Err(format!("附件 {name} 超过单文件大小限制"));
        }
        total = total.saturating_add(metadata.len());
        if total > MAX_TOTAL_ATTACHMENT_BYTES {
            return Err("附件总大小不能超过 20MB".to_string());
        }

        validated.push(AttachmentSnapshot {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            mime_type: mime_type.to_string(),
            size: metadata.len(),
            sha256: actual_sha256,
            snapshot_path: path.to_string_lossy().to_string(),
            kind: kind.to_string(),
        });
    }
    Ok(validated)
}

fn decode_text_attachment(bytes: &[u8]) -> Option<String> {
    if let Ok(value) = std::str::from_utf8(bytes) {
        if let Some(value) =
            normalize_text_attachment(value.trim_start_matches('\u{feff}').to_string())
        {
            return Some(value);
        }
    }
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let little_endian = bytes.starts_with(&[0xff, 0xfe]);
        return decode_utf16_attachment(&bytes[2..], little_endian);
    }
    detect_bomless_utf16(bytes)
        .and_then(|little_endian| decode_utf16_attachment(bytes, little_endian))
}

fn decode_utf16_attachment(bytes: &[u8], little_endian: bool) -> Option<String> {
    if bytes.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| {
            if little_endian {
                u16::from_le_bytes([chunk[0], chunk[1]])
            } else {
                u16::from_be_bytes([chunk[0], chunk[1]])
            }
        })
        .collect();
    String::from_utf16(&units)
        .ok()
        .and_then(normalize_text_attachment)
}

fn detect_bomless_utf16(bytes: &[u8]) -> Option<bool> {
    if bytes.len() < 4 || bytes.len() % 2 != 0 {
        return None;
    }
    let pairs = bytes.len() / 2;
    let even_nuls = bytes.iter().step_by(2).filter(|byte| **byte == 0).count();
    let odd_nuls = bytes
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|byte| **byte == 0)
        .count();
    if odd_nuls * 2 >= pairs && even_nuls * 4 <= pairs {
        Some(true)
    } else if even_nuls * 2 >= pairs && odd_nuls * 4 <= pairs {
        Some(false)
    } else {
        None
    }
}

fn normalize_text_attachment(value: String) -> Option<String> {
    if value.chars().any(|character| {
        character == '\0' || (character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    }) {
        None
    } else {
        Some(value)
    }
}

#[tauri::command]
async fn test_provider(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<ModelDescriptor>, String> {
    provider::test_connection(state.db.clone(), &provider_id).await
}

#[tauri::command]
fn save_mcp_server(
    state: State<'_, AppState>,
    input: McpServerConfig,
) -> Result<McpServerConfig, String> {
    let protected = mcp::protect_credentials(input)?;
    state.db.save_mcp_server(&protected)
}

#[tauri::command]
fn delete_mcp_server(state: State<'_, AppState>, server_id: String) -> Result<(), String> {
    let server = state.db.get_mcp_server_any(&server_id)?;
    state.db.delete_mcp_server(&server_id)?;
    for reference in secrets::references_in(&server.env)
        .into_iter()
        .chain(secrets::references_in(&server.headers))
    {
        let _ = secrets::delete(&reference);
    }
    Ok(())
}

#[tauri::command]
async fn test_mcp_server(
    state: State<'_, AppState>,
    input: McpServerConfig,
) -> Result<McpTestResult, String> {
    let id = input.id.clone();
    let result = mcp::test_server(&input).await;
    if !id.trim().is_empty() {
        match &result {
            Ok(value) => {
                let _ = state.db.update_mcp_health(&id, "healthy", None);
                let _ = state
                    .db
                    .save_mcp_discovery(&id, &value.tools, &value.read_only_tools);
            }
            Err(error) => {
                let _ = state.db.update_mcp_health(&id, "error", Some(error));
            }
        }
    }
    result
}

#[tauri::command]
fn save_settings(
    state: State<'_, AppState>,
    mut settings: AppSettings,
) -> Result<AppSettings, String> {
    settings.sidebar_width = settings.sidebar_width.clamp(228, 320);
    settings.inspector_width = settings.inspector_width.clamp(340, 620);
    state.db.save_settings(&settings)
}

#[tauri::command]
fn list_project_files(
    state: State<'_, AppState>,
    thread_id: String,
    path: Option<String>,
) -> Result<Vec<FileEntry>, String> {
    let root = state.db.project_path_for_thread(&thread_id)?;
    tools::list_files(Path::new(&root), path.as_deref())
}

#[tauri::command]
fn read_project_file(
    state: State<'_, AppState>,
    thread_id: String,
    path: String,
) -> Result<String, String> {
    let root = state.db.project_path_for_thread(&thread_id)?;
    tools::read_file(Path::new(&root), &path)
}

#[tauri::command]
async fn get_git_summary(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<GitSummary, String> {
    let root = state.db.project_path_for_thread(&thread_id)?;
    tools::git_summary(Path::new(&root)).await
}

#[tauri::command]
async fn execute_shell(
    state: State<'_, AppState>,
    thread_id: String,
    command: String,
    permission_mode: PermissionMode,
    approved: bool,
) -> Result<ShellResult, String> {
    let root = state.db.project_path_for_thread(&thread_id)?;
    tools::execute_shell(Path::new(&root), &command, permission_mode, approved).await
}

#[tauri::command]
fn search_project_files(
    state: State<'_, AppState>,
    thread_id: String,
    query: String,
    path: Option<String>,
) -> Result<Vec<SearchMatch>, String> {
    let root = state.db.project_path_for_thread(&thread_id)?;
    tools::search_files(Path::new(&root), &query, path.as_deref())
}

#[tauri::command]
fn write_project_file(
    state: State<'_, AppState>,
    thread_id: String,
    path: String,
    content: String,
    permission_mode: PermissionMode,
) -> Result<FileMutation, String> {
    let root = state.db.project_path_for_thread(&thread_id)?;
    tools::write_file(Path::new(&root), &path, &content, permission_mode)
}

#[tauri::command]
fn apply_project_patch(
    state: State<'_, AppState>,
    thread_id: String,
    path: String,
    patch: String,
    permission_mode: PermissionMode,
) -> Result<FileMutation, String> {
    let root = state.db.project_path_for_thread(&thread_id)?;
    tools::apply_patch(Path::new(&root), &path, &patch, permission_mode)
}

#[tauri::command]
fn delete_project_file(
    state: State<'_, AppState>,
    thread_id: String,
    path: String,
    permission_mode: PermissionMode,
) -> Result<FileMutation, String> {
    let root = state.db.project_path_for_thread(&thread_id)?;
    tools::delete_file(Path::new(&root), &path, permission_mode)
}

#[tauri::command]
fn restore_run_changes(state: State<'_, AppState>, run_id: String) -> Result<u64, String> {
    if state.running.lock().contains_key(&run_id) {
        return Err("Run must finish before its changes can be restored".to_string());
    }
    let thread_id = state.db.thread_id_for_run(&run_id)?;
    let root = state.db.project_path_for_thread(&thread_id)?;
    let mutations = state.db.change_checkpoints(&run_id)?;
    let mut restored = 0u64;
    for mutation in mutations {
        tools::restore_mutation(Path::new(&root), &mutation)?;
        restored += 1;
    }
    if restored > 0 {
        state.db.clear_change_checkpoints(&run_id)?;
    }
    Ok(restored)
}

#[tauri::command]
fn restore_run_file_changes(
    state: State<'_, AppState>,
    run_id: String,
    path: String,
) -> Result<u64, String> {
    if state.running.lock().contains_key(&run_id) {
        return Err("Run must finish before its changes can be restored".to_string());
    }
    let thread_id = state.db.thread_id_for_run(&run_id)?;
    let root = state.db.project_path_for_thread(&thread_id)?;
    let target = tools::guard_path(Path::new(&root), Path::new(&path))?;
    let target = target.to_string_lossy().to_string();
    let entries = state.db.change_checkpoint_entries(&run_id)?;
    let mut restored = 0u64;
    for (checkpoint_id, mutation) in entries {
        if !paths_equal(&mutation.path, &target) {
            continue;
        }
        tools::restore_mutation(Path::new(&root), &mutation)?;
        state.db.delete_change_checkpoint(&checkpoint_id)?;
        restored += 1;
    }
    Ok(restored)
}

#[tauri::command]
fn open_project_file_external(
    state: State<'_, AppState>,
    thread_id: String,
    path: String,
) -> Result<(), String> {
    let root = state.db.project_path_for_thread(&thread_id)?;
    let target = tools::guard_path(Path::new(&root), Path::new(&path))?;
    if !target.is_file() {
        return Err("Only regular files can be opened externally".to_string());
    }
    tauri_plugin_opener::open_path(target, None::<&str>).map_err(|error| error.to_string())
}

#[tauri::command]
fn respond_approval(
    state: State<'_, AppState>,
    approval_id: String,
    approved: bool,
) -> Result<(), String> {
    let sender = state
        .pending_approvals
        .lock()
        .remove(&approval_id)
        .ok_or_else(|| "Approval request is no longer pending".to_string())?;
    sender
        .send(if approved { "approved" } else { "denied" }.to_string())
        .map_err(|_| "Agent stopped before the approval decision was delivered".to_string())
}

#[tauri::command]
fn respond_user_question(
    state: State<'_, AppState>,
    question_id: String,
    answer: String,
) -> Result<(), String> {
    let answer = answer.trim();
    if answer.is_empty() || answer.len() > 2_000 {
        return Err("Answer must contain between 1 and 2000 characters".to_string());
    }
    let sender = state
        .pending_approvals
        .lock()
        .remove(&question_id)
        .ok_or_else(|| "Question is no longer pending".to_string())?;
    sender
        .send(answer.to_string())
        .map_err(|_| "Agent stopped before the answer was delivered".to_string())
}

fn paths_equal(left: &str, right: &str) -> bool {
    #[cfg(windows)]
    {
        left.replace('/', "\\")
            .eq_ignore_ascii_case(&right.replace('/', "\\"))
    }
    #[cfg(not(windows))]
    {
        Path::new(left) == Path::new(right)
    }
}

fn run_conflicts(
    running: &RunningRun,
    thread_id: &str,
    workspace_path: &str,
    writable: bool,
) -> bool {
    running.thread_id == thread_id
        || (writable && running.writable && paths_equal(&running.workspace_path, workspace_path))
}

fn validate_run_config(db: &Database, config: &RunConfigSnapshot) -> Result<(), String> {
    if config.provider_id.trim().is_empty() || config.provider_id.trim() != config.provider_id {
        return Err("Select a valid provider".to_string());
    }
    if config.model_id.trim().is_empty() || config.model_id.trim() != config.model_id {
        return Err("Select a valid model".to_string());
    }
    let provider = db.get_provider(&config.provider_id)?;
    if !provider.enabled {
        return Err("The selected provider is disabled".to_string());
    }
    if !provider
        .models
        .iter()
        .any(|model| model.model_id == config.model_id)
    {
        return Err("The selected model does not belong to this provider".to_string());
    }
    Ok(())
}

fn plan_mcp_tools(
    db: &Database,
    project_id: &str,
) -> Result<Vec<provider::AllowedMcpTool>, String> {
    let mut allowed = Vec::new();
    for server in db.list_mcp_servers()? {
        let applies = server.enabled
            && (server.scope == McpScope::Global
                || (server.scope == McpScope::Project
                    && server.project_id.as_deref() == Some(project_id)));
        if !applies {
            continue;
        }
        for name in &server.read_only_tools {
            if server.discovered_tools.contains(name) && !server.disabled_tools.contains(name) {
                allowed.push(provider::AllowedMcpTool {
                    server_id: server.id.clone(),
                    name: name.clone(),
                });
            }
        }
    }
    Ok(allowed)
}

#[tauri::command]
fn start_agent_run(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    thread_id: String,
    prompt: String,
    mut config: RunConfigSnapshot,
    attachments: Option<Vec<AttachmentSnapshot>>,
) -> Result<RunRecord, String> {
    let attachments = attachments.unwrap_or_default();
    if prompt.trim().is_empty() && attachments.is_empty() {
        return Err("A prompt or at least one attachment is required".to_string());
    }
    validate_run_config(&state.db, &config)?;
    let attachments = validate_attachment_snapshots(&attachments_root(&app)?, attachments)?;
    let workspace_path = state.db.project_path_for_thread(&thread_id)?;
    if config.run_mode == RunMode::Plan {
        config.permission_mode = PermissionMode::ReadOnly;
    }
    let writable = config.permission_mode != PermissionMode::ReadOnly;
    let mut active = state.running.lock();
    if active
        .values()
        .any(|running| run_conflicts(running, &thread_id, &workspace_path, writable))
    {
        return Err("A writable Agent is already running in this workspace".to_string());
    }
    config.created_at = Utc::now().to_rfc3339();
    let context_limit = state
        .db
        .get_model_override(&config.provider_id, &config.model_id)?
        .and_then(|model| model.context_window)
        .filter(|limit| *limit > 0)
        .unwrap_or_else(|| provider::builtin_context_limit(&config.model_id));
    state.db.add_message_with_attachments(
        &thread_id,
        MessageRole::User,
        prompt.trim(),
        None,
        attachments,
    )?;
    let run = state
        .db
        .create_run_with_context(&thread_id, &config, context_limit)?;
    if config.run_mode == RunMode::Goal {
        state.db.create_goal(&run)?;
    }
    let (cancel_tx, cancel_rx) = watch::channel(false);
    active.insert(
        run.id.clone(),
        RunningRun {
            thread_id: thread_id.clone(),
            workspace_path,
            writable,
            cancel: cancel_tx,
        },
    );
    drop(active);

    let db = state.db.clone();
    let running = state.running.clone();
    let pending_approvals = state.pending_approvals.clone();
    let run_for_task = run.clone();
    tauri::async_runtime::spawn(async move {
        run_agent(app, db, running, pending_approvals, run_for_task, cancel_rx).await;
    });
    Ok(run)
}

#[tauri::command]
fn cancel_agent_run(state: State<'_, AppState>, run_id: String) -> Result<(), String> {
    if let Some(run) = state.running.lock().get(&run_id) {
        run.cancel.send(true).map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err("Run is not active".to_string())
    }
}

#[tauri::command]
fn resume_goal(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    run_id: String,
) -> Result<RunRecord, String> {
    let existing = state.db.get_run(&run_id)?;
    if existing.config.run_mode != RunMode::Goal {
        return Err("Only Goal runs can be resumed".to_string());
    }
    let workspace_path = state.db.project_path_for_thread(&existing.thread_id)?;
    let writable = existing.config.permission_mode != PermissionMode::ReadOnly;
    let mut active = state.running.lock();
    if active
        .values()
        .any(|running| run_conflicts(running, &existing.thread_id, &workspace_path, writable))
    {
        return Err("A writable Agent is already running in this workspace".to_string());
    }
    let run = state.db.resume_goal_run(&run_id)?;
    let (cancel_tx, cancel_rx) = watch::channel(false);
    active.insert(
        run.id.clone(),
        RunningRun {
            thread_id: run.thread_id.clone(),
            workspace_path,
            writable,
            cancel: cancel_tx,
        },
    );
    drop(active);

    let db = state.db.clone();
    let running = state.running.clone();
    let pending_approvals = state.pending_approvals.clone();
    let run_for_task = run.clone();
    tauri::async_runtime::spawn(async move {
        run_agent(app, db, running, pending_approvals, run_for_task, cancel_rx).await;
    });
    Ok(run)
}

#[tauri::command]
fn finish_goal(state: State<'_, AppState>, run_id: String) -> Result<(), String> {
    let run = state.db.get_run(&run_id)?;
    if run.config.run_mode != RunMode::Goal {
        return Err("Only Goal runs can be finished".to_string());
    }
    state.db.update_goal_status(&run_id, "completed")?;
    let _ = state.db.update_goal_turn_status(&run_id, "completed");
    if let Some(active) = state.running.lock().get(&run_id) {
        active
            .cancel
            .send(true)
            .map_err(|error| error.to_string())?;
    } else if !matches!(run.status, RunStatus::Completed | RunStatus::Failed) {
        state.db.update_run(
            &run.id,
            &run.thread_id,
            RunStatus::Completed,
            Some(&run.usage),
            None,
        )?;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ProtocolToolCall {
    name: String,
    #[serde(default)]
    arguments: Value,
}

const TOOL_PROTOCOL: &str = r#"
You can operate on the current workspace with Axiom tools. When a tool is required, output exactly one tool request and no final answer in this format:
```axiom-tool
{"name":"read_file","arguments":{"path":"src/main.rs"}}
```
Available names and arguments:
- list_files {"path":"optional relative directory"}
- read_file {"path":"relative file"}
- search_files {"query":"text","path":"optional relative directory"}
- write_file {"path":"relative file","content":"complete UTF-8 content"}
- apply_patch {"path":"relative file","patch":"unified patch for that file"}
- delete_file {"path":"relative file"}
- git_status {}
- git_diff {}
- shell {"command":"project command"}
- mcp_call {"serverId":"configured MCP id","tool":"tool name","arguments":{}}
Use relative paths. Inspect before editing. Prefer apply_patch over full rewrites. After a tool result is returned, continue reasoning and either call the next tool or provide the final answer. Never fabricate tool results.
"#;
const CANCELLED_SENTINEL: &str = "__AXIOM_CANCELLED__";
const MAX_TOOL_LOOPS: usize = 16;

async fn run_agent(
    app: tauri::AppHandle,
    db: Arc<Database>,
    running: Arc<Mutex<HashMap<String, RunningRun>>>,
    pending_approvals: Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>,
    run: RunRecord,
    mut cancel: watch::Receiver<bool>,
) {
    let started = Instant::now();
    let prior_duration_ms = run.usage.duration_ms.unwrap_or_default();
    let mut sequence = db.last_event_sequence(&run.id).unwrap_or_default();
    let root = match db.project_path_for_thread(&run.thread_id) {
        Ok(value) => value,
        Err(error) => {
            finish_error(&app, &db, &running, &run, &mut sequence, error).await;
            return;
        }
    };
    let project_id = match db.project_id_for_thread(&run.thread_id) {
        Ok(value) => value,
        Err(error) => {
            finish_error(&app, &db, &running, &run, &mut sequence, error).await;
            return;
        }
    };

    let allowed_plan_mcp = if run.config.run_mode == RunMode::Plan {
        match plan_mcp_tools(&db, &project_id) {
            Ok(value) => value,
            Err(error) => {
                finish_error(&app, &db, &running, &run, &mut sequence, error).await;
                return;
            }
        }
    } else {
        Vec::new()
    };

    let model_override = db
        .get_model_override(&run.config.provider_id, &run.config.model_id)
        .ok()
        .flatten();
    let context_limit = model_override
        .as_ref()
        .and_then(|value| value.context_window)
        .unwrap_or_else(|| provider::builtin_context_limit(&run.config.model_id));

    let _ = db.update_run(&run.id, &run.thread_id, RunStatus::Reasoning, None, None);
    emit_agent_event(
        &app,
        &db,
        &run,
        &mut sequence,
        AgentEventKind::Status,
        RunStatus::Reasoning,
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let mut messages = match db.messages_for_provider(&run.thread_id) {
        Ok(mut messages) => {
            messages.insert(
                0,
                Message {
                    id: "axiom-system".into(),
                    thread_id: run.thread_id.clone(),
                    role: MessageRole::System,
                    content: format!(
                        "You are Axiom, a precise local coding agent. Explain planned changes, preserve user work, and prefer small reviewable edits. Never claim a tool was used unless it actually ran. Current workspace: {}. Current permission mode: {:?}. Current run mode: {:?}.\n{}\n{}",
                        root,
                        run.config.permission_mode,
                        run.config.run_mode,
                        if run.config.run_mode == RunMode::Plan {
                            "Plan mode is enforced read-only. Inspect the project and return a decision-complete implementation plan. Do not attempt writes or shell commands. MCP calls are allowed only for tools explicitly marked read-only and non-destructive; all others are rejected by the permission layer. When an ambiguity materially affects the plan, call ask_user with one short question and 2-3 concrete options instead of asking in ordinary response text."
                        } else if run.config.run_mode == RunMode::Goal {
                            "Goal mode continues without a total turn or time limit. At the end of every non-tool response append exactly one control block: ```axiom-goal\n{\"action\":\"continue|complete|blocked\"}\n```. Use continue while meaningful work remains, complete only when verified, and blocked only when progress requires user input."
                        } else {
                            "Execute the requested coding task and verify the result."
                        },
                        TOOL_PROTOCOL
                    ),
                    created_at: Utc::now().to_rfc3339(),
                    run_id: None,
                    pinned: true,
                    attachments: Vec::new(),
                },
            );
            messages
        }
        Err(error) => {
            finish_error(&app, &db, &running, &run, &mut sequence, error).await;
            return;
        }
    };

    compress_context_if_needed(&app, &db, &run, &mut sequence, &mut messages, context_limit);
    let mut total_usage = run.usage.clone();
    total_usage.duration_ms = None;
    let mut request_index = 0usize;
    let mut consecutive_tool_loops = 0usize;
    let mut goal_turn_active = false;

    loop {
        if *cancel.borrow() {
            finish_cancelled(&app, &db, &running, &run, &mut sequence).await;
            return;
        }
        let status = if request_index == 0 {
            RunStatus::Streaming
        } else {
            RunStatus::Reasoning
        };
        let _ = db.update_run(&run.id, &run.thread_id, status, None, None);
        emit_agent_event(
            &app,
            &db,
            &run,
            &mut sequence,
            AgentEventKind::Status,
            status,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        if run.config.run_mode == RunMode::Goal && !goal_turn_active {
            if let Err(error) = db.add_goal_turn(&run.id) {
                finish_error(&app, &db, &running, &run, &mut sequence, error).await;
                return;
            }
            goal_turn_active = true;
        }
        let (delta_tx, mut delta_rx) = tokio::sync::mpsc::unbounded_channel();
        let provider_messages = messages.clone();
        let request_input_tokens = provider_messages
            .iter()
            .map(|message| estimate_tokens(&message.content))
            .sum();
        let initial_live_usage = realtime_usage_snapshot(
            &total_usage,
            request_input_tokens,
            "",
            "",
            context_limit,
            prior_duration_ms.saturating_add(started.elapsed().as_millis() as u64),
        );
        emit_agent_event(
            &app,
            &db,
            &run,
            &mut sequence,
            AgentEventKind::Usage,
            status,
            None,
            None,
            Some(initial_live_usage),
            None,
            None,
            None,
        );
        let provider_future = provider::generate_stream(
            db.clone(),
            &run.config.provider_id,
            &run.config.model_id,
            &provider_messages,
            &run.config,
            &allowed_plan_mcp,
            move |event| {
                let _ = delta_tx.send(event);
            },
        );
        tokio::pin!(provider_future);
        let mut stream_gate = StreamingTextGate::default();
        let mut live_output = String::new();
        let mut live_reasoning = String::new();
        let mut last_usage_emit = Instant::now();
        let mut pending_usage_chars = 0usize;
        let response = loop {
            tokio::select! {
                _ = cancel.changed() => {
                    finish_cancelled(&app, &db, &running, &run, &mut sequence).await;
                    return;
                }
                event = delta_rx.recv() => {
                    if let Some(event) = event {
                        handle_provider_stream_event(
                            event, &app, &db, &run, &mut sequence, &mut stream_gate,
                            &mut live_output, &mut live_reasoning, &total_usage,
                            request_input_tokens, context_limit,
                            prior_duration_ms.saturating_add(started.elapsed().as_millis() as u64),
                            &mut last_usage_emit, &mut pending_usage_chars,
                        );
                    }
                }
                result = &mut provider_future => {
                    while let Ok(event) = delta_rx.try_recv() {
                        handle_provider_stream_event(
                            event, &app, &db, &run, &mut sequence, &mut stream_gate,
                            &mut live_output, &mut live_reasoning, &total_usage,
                            request_input_tokens, context_limit,
                            prior_duration_ms.saturating_add(started.elapsed().as_millis() as u64),
                            &mut last_usage_emit, &mut pending_usage_chars,
                        );
                    }
                    break result;
                }
            }
        };
        let response = match response {
            Ok(value) => value,
            Err(error) => {
                finish_error(&app, &db, &running, &run, &mut sequence, error).await;
                return;
            }
        };
        if run.config.run_mode != RunMode::Goal {
            if let Some(visible) = stream_gate.finish() {
                emit_agent_event(
                    &app,
                    &db,
                    &run,
                    &mut sequence,
                    AgentEventKind::TextDelta,
                    RunStatus::Streaming,
                    Some(visible),
                    None,
                    None,
                    None,
                    None,
                    None,
                );
            }
        }
        accumulate_usage(&mut total_usage, &response.usage);
        total_usage.context_tokens = response.usage.context_tokens;
        total_usage.context_limit = context_limit;
        total_usage.cumulative_tokens = total_usage.input_tokens.unwrap_or_default()
            + total_usage.output_tokens.unwrap_or_default()
            + total_usage.reasoning_tokens.unwrap_or_default();
        total_usage.duration_ms =
            Some(prior_duration_ms.saturating_add(started.elapsed().as_millis() as u64));
        emit_agent_event(
            &app,
            &db,
            &run,
            &mut sequence,
            AgentEventKind::Usage,
            RunStatus::Reasoning,
            None,
            None,
            Some(total_usage.clone()),
            None,
            None,
            None,
        );
        request_index = request_index.saturating_add(1);

        let native_tool_call = response.tool_call.clone();
        let parsed_tool_call = native_tool_call
            .as_ref()
            .map(|call| ProtocolToolCall {
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            })
            .or_else(|| parse_tool_call(&response.text));
        if let Some(tool_call) = parsed_tool_call {
            consecutive_tool_loops = consecutive_tool_loops.saturating_add(1);
            if consecutive_tool_loops > MAX_TOOL_LOOPS {
                finish_error(
                    &app,
                    &db,
                    &running,
                    &run,
                    &mut sequence,
                    format!("Agent reached the consecutive tool loop limit ({MAX_TOOL_LOOPS})"),
                )
                .await;
                return;
            }
            let tool_id = uuid::Uuid::new_v4().to_string();
            let summary = tool_summary(&tool_call);
            let _ =
                db.save_tool_call_started(&tool_id, &run.id, &tool_call.name, &tool_call.arguments);
            let _ = db.update_run(&run.id, &run.thread_id, RunStatus::ToolRunning, None, None);
            emit_agent_event(
                &app,
                &db,
                &run,
                &mut sequence,
                AgentEventKind::ToolStarted,
                RunStatus::ToolRunning,
                None,
                None,
                None,
                None,
                None,
                Some(ToolActivity {
                    id: tool_id.clone(),
                    name: tool_call.name.clone(),
                    status: "running".into(),
                    summary: summary.clone(),
                    output: None,
                    duration_ms: None,
                }),
            );
            let tool_started = Instant::now();
            let execution = execute_agent_tool(
                &app,
                &db,
                &run,
                &root,
                &project_id,
                &pending_approvals,
                &mut cancel,
                &mut sequence,
                &tool_call,
            )
            .await;
            if matches!(&execution, Err(error) if error == CANCELLED_SENTINEL) {
                finish_cancelled(&app, &db, &running, &run, &mut sequence).await;
                return;
            }
            let (ok, result) = match execution {
                Ok(value) => (true, value),
                Err(error) => (false, json!({"ok":false,"error":error}).to_string()),
            };
            let result = truncate_for_context(result, 240_000);
            let _ = db.finish_tool_call(&tool_id, &result, ok);
            emit_agent_event(
                &app,
                &db,
                &run,
                &mut sequence,
                AgentEventKind::ToolCompleted,
                RunStatus::Reasoning,
                None,
                None,
                None,
                None,
                None,
                Some(ToolActivity {
                    id: tool_id,
                    name: tool_call.name.clone(),
                    status: if ok {
                        "completed".into()
                    } else {
                        "failed".into()
                    },
                    summary,
                    output: Some(result.clone()),
                    duration_ms: Some(tool_started.elapsed().as_millis() as u64),
                }),
            );
            if let Some(native) = native_tool_call {
                messages.push(transient_message(
                    &run.thread_id,
                    MessageRole::Assistant,
                    provider::responses_call_message(&native),
                ));
                messages.push(transient_message(
                    &run.thread_id,
                    MessageRole::User,
                    provider::responses_output_message(&native.call_id, &result),
                ));
            } else {
                messages.push(transient_message(
                    &run.thread_id,
                    MessageRole::Assistant,
                    response.text,
                ));
                messages.push(transient_message(
                    &run.thread_id,
                    MessageRole::User,
                    format!(
                        "Axiom tool result for `{}`:\n{}\nContinue from this verified result.",
                        tool_call.name, result
                    ),
                ));
            }
            continue;
        }

        consecutive_tool_loops = 0;
        let (visible_response, goal_action) = if run.config.run_mode == RunMode::Goal {
            match strip_goal_control(&response.text) {
                Ok(value) => value,
                Err(error) => {
                    finish_error(&app, &db, &running, &run, &mut sequence, error).await;
                    return;
                }
            }
        } else {
            (response.text.clone(), GoalAction::Complete)
        };
        if run.config.run_mode == RunMode::Goal && goal_action == GoalAction::Continue {
            let message = match db.add_message(
                &run.thread_id,
                MessageRole::Assistant,
                &visible_response,
                Some(&run.id),
            ) {
                Ok(message) => message,
                Err(error) => {
                    finish_error(&app, &db, &running, &run, &mut sequence, error).await;
                    return;
                }
            };
            messages.push(transient_message(
                &run.thread_id,
                MessageRole::Assistant,
                visible_response,
            ));
            messages.push(transient_message(
                &run.thread_id,
                MessageRole::User,
                "Continue pursuing the goal from the verified state. Do not repeat completed work."
                    .to_string(),
            ));
            total_usage.context_tokens = messages
                .iter()
                .map(|message| estimate_tokens(&message.content))
                .sum();
            total_usage.cumulative_tokens = total_usage.context_tokens
                + total_usage.output_tokens.unwrap_or_default()
                + total_usage.reasoning_tokens.unwrap_or_default();
            total_usage.estimated_cost_usd = model_override
                .as_ref()
                .and_then(|pricing| estimate_cost(&total_usage, pricing));
            total_usage.duration_ms =
                Some(prior_duration_ms.saturating_add(started.elapsed().as_millis() as u64));
            let _ = db.update_run(
                &run.id,
                &run.thread_id,
                RunStatus::Reasoning,
                Some(&total_usage),
                None,
            );
            emit_agent_event(
                &app,
                &db,
                &run,
                &mut sequence,
                AgentEventKind::MessageCompleted,
                RunStatus::Reasoning,
                None,
                Some(message),
                Some(total_usage.clone()),
                None,
                None,
                None,
            );
            let _ = db.update_goal_turn_status(&run.id, "completed");
            goal_turn_active = false;
            continue;
        }

        total_usage.context_tokens = messages
            .iter()
            .map(|message| estimate_tokens(&message.content))
            .sum();
        total_usage.context_limit = total_usage.context_limit.max(context_limit);
        total_usage.cumulative_tokens = total_usage.context_tokens
            + total_usage.output_tokens.unwrap_or_default()
            + total_usage.reasoning_tokens.unwrap_or_default();
        total_usage.estimated_cost_usd = model_override
            .as_ref()
            .and_then(|pricing| estimate_cost(&total_usage, pricing));
        total_usage.duration_ms =
            Some(prior_duration_ms.saturating_add(started.elapsed().as_millis() as u64));
        let message = match db.add_message(
            &run.thread_id,
            MessageRole::Assistant,
            &visible_response,
            Some(&run.id),
        ) {
            Ok(message) => message,
            Err(error) => {
                finish_error(&app, &db, &running, &run, &mut sequence, error).await;
                return;
            }
        };
        if run.config.run_mode == RunMode::Goal {
            let status = if goal_action == GoalAction::Blocked {
                "blocked"
            } else {
                "completed"
            };
            let _ = db.update_goal_turn_status(&run.id, status);
            let _ = db.update_goal_status(&run.id, status);
        }
        let _ = db.update_run(
            &run.id,
            &run.thread_id,
            RunStatus::Completed,
            Some(&total_usage),
            None,
        );
        emit_agent_event(
            &app,
            &db,
            &run,
            &mut sequence,
            AgentEventKind::MessageCompleted,
            RunStatus::Completed,
            None,
            Some(message),
            Some(total_usage),
            None,
            None,
            None,
        );
        running.lock().remove(&run.id);
        return;
    }
}

async fn execute_agent_tool(
    app: &tauri::AppHandle,
    db: &Arc<Database>,
    run: &RunRecord,
    root: &str,
    project_id: &str,
    pending_approvals: &Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>,
    cancel: &mut watch::Receiver<bool>,
    sequence: &mut u64,
    call: &ProtocolToolCall,
) -> Result<String, String> {
    let root_path = Path::new(root);
    if run.config.run_mode == RunMode::Plan
        && !matches!(
            call.name.as_str(),
            "list_files" | "read_file" | "search_files" | "git_status" | "git_diff" | "ask_user"
        )
    {
        return Err(format!(
            "Plan mode rejected non-read-only tool `{}`",
            call.name
        ));
    }
    match call.name.as_str() {
        "ask_user" => {
            if run.config.run_mode != RunMode::Plan {
                return Err("ask_user is only available in Plan mode".to_string());
            }
            request_user_question(app, db, run, pending_approvals, cancel, sequence, call).await
        }
        "list_files" => {
            let path = optional_arg(&call.arguments, "path");
            let files = tools::list_files(root_path, path)?;
            serde_json::to_string(&files).map_err(|e| e.to_string())
        }
        "read_file" => tools::read_file(root_path, required_arg(&call.arguments, "path")?),
        "search_files" => {
            let matches = tools::search_files(
                root_path,
                required_arg(&call.arguments, "query")?,
                optional_arg(&call.arguments, "path"),
            )?;
            serde_json::to_string(&matches).map_err(|e| e.to_string())
        }
        "write_file" => {
            let mutation = tools::write_file(
                root_path,
                required_arg(&call.arguments, "path")?,
                required_arg(&call.arguments, "content")?,
                run.config.permission_mode,
            )?;
            db.save_change_checkpoint(&run.id, project_id, &mutation)?;
            Ok(json!({"ok":true,"path":mutation.path,"operation":mutation.operation}).to_string())
        }
        "apply_patch" => {
            let mutation = tools::apply_patch(
                root_path,
                required_arg(&call.arguments, "path")?,
                required_arg(&call.arguments, "patch")?,
                run.config.permission_mode,
            )?;
            db.save_change_checkpoint(&run.id, project_id, &mutation)?;
            Ok(json!({"ok":true,"path":mutation.path,"operation":"patch"}).to_string())
        }
        "delete_file" => {
            let mutation = tools::delete_file(
                root_path,
                required_arg(&call.arguments, "path")?,
                run.config.permission_mode,
            )?;
            db.save_change_checkpoint(&run.id, project_id, &mutation)?;
            Ok(json!({"ok":true,"path":mutation.path,"operation":"delete"}).to_string())
        }
        "git_status" | "git_diff" => {
            let summary = tokio::select! {
                _ = cancel.changed() => return Err(CANCELLED_SENTINEL.to_string()),
                value = tools::git_summary(root_path) => value?,
            };
            if call.name == "git_diff" {
                Ok(summary.diff)
            } else {
                serde_json::to_string(&summary).map_err(|e| e.to_string())
            }
        }
        "shell" => {
            let command = required_arg(&call.arguments, "command")?;
            let needs_approval = run.config.permission_mode == PermissionMode::ReadOnly
                || (run.config.permission_mode == PermissionMode::WorkspaceAuto
                    && tools::shell_requires_approval(command));
            if needs_approval
                && !request_tool_approval(
                    app,
                    db,
                    run,
                    pending_approvals,
                    cancel,
                    sequence,
                    call,
                    format!(
                        "Run PowerShell: {}",
                        command.chars().take(180).collect::<String>()
                    ),
                )
                .await?
            {
                return Err("Shell command was denied".to_string());
            }
            let result = tokio::select! {
                _ = cancel.changed() => return Err(CANCELLED_SENTINEL.to_string()),
                value = tools::execute_shell(root_path, command, run.config.permission_mode, true) => value?,
            };
            serde_json::to_string(&result).map_err(|e| e.to_string())
        }
        "mcp_call" => {
            let server_id = required_arg(&call.arguments, "serverId")?;
            let tool_name = required_arg(&call.arguments, "tool")?;
            let server = db.get_mcp_server(server_id)?;
            let applies = server.scope == McpScope::Global
                || (server.scope == McpScope::Project
                    && server.project_id.as_deref() == Some(project_id));
            if !applies {
                return Err("MCP server is not available for this project".to_string());
            }
            let discovered = server.discovered_tools.iter().any(|tool| tool == tool_name);
            if (run.config.run_mode == RunMode::Plan || !server.discovered_tools.is_empty())
                && !discovered
            {
                return Err(format!(
                    "MCP tool was not discovered on this server: {tool_name}"
                ));
            }
            if server.disabled_tools.iter().any(|tool| tool == tool_name) {
                return Err(format!("MCP tool is disabled: {tool_name}"));
            }
            if run.config.run_mode == RunMode::Plan
                && !server.read_only_tools.iter().any(|tool| tool == tool_name)
            {
                return Err(format!(
                    "Plan mode only allows MCP tools explicitly marked read-only and non-destructive: {tool_name}"
                ));
            }
            if run.config.run_mode != RunMode::Plan
                && run.config.permission_mode == PermissionMode::ReadOnly
                && !request_tool_approval(
                    app,
                    db,
                    run,
                    pending_approvals,
                    cancel,
                    sequence,
                    call,
                    format!("Call MCP tool: {tool_name}"),
                )
                .await?
            {
                return Err("MCP call was denied".to_string());
            }
            let arguments = call
                .arguments
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let result = tokio::select! {
                _ = cancel.changed() => return Err(CANCELLED_SENTINEL.to_string()),
                value = mcp::call_tool(
                    &server,
                    tool_name,
                    arguments,
                ) => value?,
            };
            serde_json::to_string(&result).map_err(|e| e.to_string())
        }
        _ => Err(format!("Unknown tool: {}", call.name)),
    }
}

async fn request_tool_approval(
    app: &tauri::AppHandle,
    db: &Arc<Database>,
    run: &RunRecord,
    pending_approvals: &Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>,
    cancel: &mut watch::Receiver<bool>,
    sequence: &mut u64,
    call: &ProtocolToolCall,
    summary: String,
) -> Result<bool, String> {
    let approval_id = uuid::Uuid::new_v4().to_string();
    let request = ApprovalRequest {
        id: approval_id.clone(),
        tool_name: call.name.clone(),
        summary: summary.clone(),
        arguments: redact_tool_arguments(&call.arguments),
        created_at: Utc::now().to_rfc3339(),
    };
    let (sender, receiver) = oneshot::channel();
    pending_approvals.lock().insert(approval_id.clone(), sender);
    db.create_approval(
        &approval_id,
        &run.id,
        &call.name,
        &summary,
        &request.arguments,
    )?;
    let _ = db.update_run(
        &run.id,
        &run.thread_id,
        RunStatus::AwaitingApproval,
        None,
        None,
    );
    if run.config.run_mode == RunMode::Goal {
        let _ = db.update_goal_turn_status(&run.id, "awaiting-approval");
        let _ = db.update_goal_status(&run.id, "awaiting-approval");
    }
    emit_agent_event(
        app,
        db,
        run,
        sequence,
        AgentEventKind::ApprovalRequested,
        RunStatus::AwaitingApproval,
        None,
        None,
        None,
        None,
        Some(request),
        None,
    );
    let decision = tokio::select! {
        _ = cancel.changed() => {
            pending_approvals.lock().remove(&approval_id);
            return Err(CANCELLED_SENTINEL.to_string());
        }
        value = receiver => value.map_err(|_| "Approval channel closed".to_string())?,
    };
    let approved = decision == "approved";
    pending_approvals.lock().remove(&approval_id);
    db.decide_approval(&approval_id, approved)?;
    let _ = db.update_run(&run.id, &run.thread_id, RunStatus::ToolRunning, None, None);
    if run.config.run_mode == RunMode::Goal {
        let _ = db.update_goal_turn_status(&run.id, "running");
        let _ = db.update_goal_status(&run.id, "running");
    }
    Ok(approved)
}

async fn request_user_question(
    app: &tauri::AppHandle,
    db: &Arc<Database>,
    run: &RunRecord,
    pending_questions: &Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>,
    cancel: &mut watch::Receiver<bool>,
    sequence: &mut u64,
    call: &ProtocolToolCall,
) -> Result<String, String> {
    let question = required_arg(&call.arguments, "question")?.trim();
    let options = call
        .arguments
        .get("options")
        .and_then(Value::as_array)
        .ok_or_else(|| "ask_user.options must be an array".to_string())?;
    if !(2..=3).contains(&options.len()) {
        return Err("ask_user requires 2 or 3 options".to_string());
    }
    let mut ids = std::collections::HashSet::new();
    for option in options {
        let id = option
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let label = option
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if id.is_empty() || label.is_empty() || !ids.insert(id.to_string()) {
            return Err(
                "ask_user option ids and labels must be non-empty and ids must be unique"
                    .to_string(),
            );
        }
    }
    let question_id = uuid::Uuid::new_v4().to_string();
    let request = ApprovalRequest {
        id: question_id.clone(),
        tool_name: "ask_user".to_string(),
        summary: question.to_string(),
        arguments: call.arguments.clone(),
        created_at: Utc::now().to_rfc3339(),
    };
    let (sender, receiver) = oneshot::channel();
    pending_questions.lock().insert(question_id.clone(), sender);
    db.create_approval(&question_id, &run.id, "ask_user", question, &call.arguments)?;
    let _ = db.update_run(
        &run.id,
        &run.thread_id,
        RunStatus::AwaitingApproval,
        None,
        None,
    );
    emit_agent_event(
        app,
        db,
        run,
        sequence,
        AgentEventKind::ApprovalRequested,
        RunStatus::AwaitingApproval,
        None,
        None,
        None,
        None,
        Some(request),
        None,
    );
    let answer = tokio::select! {
        _ = cancel.changed() => {
            pending_questions.lock().remove(&question_id);
            return Err(CANCELLED_SENTINEL.to_string());
        }
        value = receiver => value.map_err(|_| "Question channel closed".to_string())?,
    };
    pending_questions.lock().remove(&question_id);
    db.decide_approval_value(&question_id, &answer)?;
    let _ = db.update_run(&run.id, &run.thread_id, RunStatus::ToolRunning, None, None);
    Ok(json!({"answer": answer}).to_string())
}

fn emit_agent_event(
    app: &tauri::AppHandle,
    db: &Arc<Database>,
    run: &RunRecord,
    sequence: &mut u64,
    kind: AgentEventKind,
    status: RunStatus,
    content: Option<String>,
    message: Option<Message>,
    usage: Option<UsageRecord>,
    error: Option<String>,
    approval: Option<ApprovalRequest>,
    tool_activity: Option<ToolActivity>,
) {
    *sequence += 1;
    if matches!(kind, AgentEventKind::ReasoningDelta) {
        if let Some(delta) = content.as_deref() {
            let _ = db.append_run_reasoning(&run.id, delta);
        }
    }
    if let Some(current_usage) = usage.as_ref() {
        let _ = db.update_run_usage(&run.id, current_usage);
    }
    let event = AgentEvent {
        sequence: *sequence,
        run_id: run.id.clone(),
        thread_id: run.thread_id.clone(),
        kind,
        status,
        content,
        message,
        usage,
        error,
        approval,
        tool_activity,
        created_at: Utc::now().to_rfc3339(),
    };
    let _ = db.save_event(&event);
    let _ = app.emit("agent-event", event);
}

async fn finish_error(
    app: &tauri::AppHandle,
    db: &Arc<Database>,
    running: &Arc<Mutex<HashMap<String, RunningRun>>>,
    run: &RunRecord,
    sequence: &mut u64,
    error: String,
) {
    if run.config.run_mode == RunMode::Goal {
        let _ = db.update_goal_turn_status(&run.id, "failed");
        let _ = db.update_goal_status(&run.id, "failed");
    }
    let _ = db.update_run(
        &run.id,
        &run.thread_id,
        RunStatus::Failed,
        None,
        Some(&error),
    );
    emit_agent_event(
        app,
        db,
        run,
        sequence,
        AgentEventKind::Error,
        RunStatus::Failed,
        None,
        None,
        None,
        Some(error),
        None,
        None,
    );
    running.lock().remove(&run.id);
}

async fn finish_cancelled(
    app: &tauri::AppHandle,
    db: &Arc<Database>,
    running: &Arc<Mutex<HashMap<String, RunningRun>>>,
    run: &RunRecord,
    sequence: &mut u64,
) {
    let user_finished_goal = run.config.run_mode == RunMode::Goal
        && matches!(db.goal_status(&run.id).as_deref(), Ok("completed"));
    if user_finished_goal {
        let _ = db.update_goal_turn_status(&run.id, "completed");
        let _ = db.update_run(&run.id, &run.thread_id, RunStatus::Completed, None, None);
        emit_agent_event(
            app,
            db,
            run,
            sequence,
            AgentEventKind::Status,
            RunStatus::Completed,
            Some("Goal ended by user".into()),
            None,
            None,
            None,
            None,
            None,
        );
    } else {
        if run.config.run_mode == RunMode::Goal {
            let _ = db.update_goal_turn_status(&run.id, "paused");
            let _ = db.pause_goal_if_active(&run.id);
        }
        let _ = db.update_run(&run.id, &run.thread_id, RunStatus::Cancelled, None, None);
        emit_agent_event(
            app,
            db,
            run,
            sequence,
            AgentEventKind::Status,
            RunStatus::Cancelled,
            Some("Run cancelled".into()),
            None,
            None,
            None,
            None,
            None,
        );
    }
    running.lock().remove(&run.id);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GoalAction {
    Continue,
    Complete,
    Blocked,
}

fn strip_goal_control(text: &str) -> Result<(String, GoalAction), String> {
    const ERROR: &str = "Goal 响应缺少有效的控制协议，运行已安全停止";
    let marker = "```axiom-goal";
    let Some(start) = text.rfind(marker) else {
        return Err(ERROR.to_string());
    };
    let payload = &text[start + marker.len()..];
    let Some(end) = payload.find("```") else {
        return Err(ERROR.to_string());
    };
    if !payload[end + 3..].trim().is_empty() {
        return Err(ERROR.to_string());
    }
    let action = serde_json::from_str::<Value>(payload[..end].trim())
        .ok()
        .and_then(|value| value.get("action")?.as_str().map(str::to_string))
        .and_then(|value| match value.as_str() {
            "continue" => Some(GoalAction::Continue),
            "complete" => Some(GoalAction::Complete),
            "blocked" => Some(GoalAction::Blocked),
            _ => None,
        })
        .ok_or_else(|| ERROR.to_string())?;
    Ok((text[..start].trim_end().to_string(), action))
}

fn parse_tool_call(text: &str) -> Option<ProtocolToolCall> {
    let marker = "```axiom-tool";
    if let Some(start) = text.find(marker) {
        let after = &text[start + marker.len()..];
        let payload = after.trim_start_matches(['\r', '\n', ' ']);
        let end = payload.find("```")?;
        return serde_json::from_str(payload[..end].trim()).ok();
    }
    let open = "<axiom_tool>";
    let close = "</axiom_tool>";
    if let Some(start) = text.find(open) {
        let payload = &text[start + open.len()..];
        let end = payload.find(close)?;
        return serde_json::from_str(payload[..end].trim()).ok();
    }
    None
}

fn tool_summary(call: &ProtocolToolCall) -> String {
    let target = optional_arg(&call.arguments, "path")
        .or_else(|| optional_arg(&call.arguments, "command"))
        .or_else(|| optional_arg(&call.arguments, "tool"))
        .unwrap_or("");
    if target.is_empty() {
        call.name.clone()
    } else {
        format!(
            "{} ? {}",
            call.name,
            target.chars().take(120).collect::<String>()
        )
    }
}

fn redact_tool_arguments(arguments: &Value) -> Value {
    let mut value = arguments.clone();
    if let Some(object) = value.as_object_mut() {
        for key in ["content", "apiKey", "authorization", "token", "secret"] {
            if object.contains_key(key) {
                object.insert(key.to_string(), Value::String("[REDACTED]".into()));
            }
        }
    }
    value
}

fn required_arg<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("Missing required argument `{key}`"))
}

fn optional_arg<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn transient_message(thread_id: &str, role: MessageRole, content: String) -> Message {
    Message {
        id: uuid::Uuid::new_v4().to_string(),
        thread_id: thread_id.to_string(),
        role,
        content,
        created_at: Utc::now().to_rfc3339(),
        run_id: None,
        pinned: false,
        attachments: Vec::new(),
    }
}

fn compress_context_if_needed(
    app: &tauri::AppHandle,
    db: &Arc<Database>,
    run: &RunRecord,
    sequence: &mut u64,
    messages: &mut Vec<Message>,
    limit: u64,
) {
    let tokens: u64 = messages
        .iter()
        .map(|message| estimate_tokens(&message.content))
        .sum();
    if tokens < limit * 85 / 100 || messages.len() <= 14 {
        return;
    }

    let cutoff = messages.len().saturating_sub(12);
    let selected_indices: Vec<usize> = (1..cutoff)
        .filter(|index| {
            let message = &messages[*index];
            !message.pinned || message.id.starts_with("context-snapshot:")
        })
        .collect();
    if selected_indices.len() < 2 {
        return;
    }

    let compressed = selected_indices
        .iter()
        .map(|index| {
            let message = &messages[*index];
            let role = match message.role {
                MessageRole::User => "User",
                MessageRole::Assistant => "Axiom",
                MessageRole::System => "System",
                MessageRole::Tool => "Tool",
            };
            format!(
                "- {role}: {}",
                message.content.chars().take(500).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    let summary = format!(
        "Context was transparently compressed from approximately {tokens} tokens.
{compressed}"
    );

    let previous = db.active_context_snapshot(&run.thread_id).ok().flatten();
    let mut source_message_ids = Vec::new();
    for index in &selected_indices {
        let id = &messages[*index].id;
        if id.starts_with("context-snapshot:") {
            if let Some(snapshot) = &previous {
                source_message_ids.extend(snapshot.source_message_ids.iter().cloned());
            }
        } else {
            source_message_ids.push(id.clone());
        }
    }
    source_message_ids.sort();
    source_message_ids.dedup();

    let insert_at = *selected_indices.first().unwrap_or(&1);
    for index in selected_indices.iter().rev() {
        messages.remove(*index);
    }
    let snapshot = db.save_context_snapshot(
        &run.thread_id,
        &run.id,
        &summary,
        estimate_tokens(&summary),
        &source_message_ids,
    );
    let snapshot_id = snapshot
        .as_ref()
        .map(|snapshot| snapshot.id.clone())
        .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());
    messages.insert(
        insert_at.min(messages.len()),
        Message {
            id: format!("context-snapshot:{snapshot_id}"),
            thread_id: run.thread_id.clone(),
            role: MessageRole::System,
            content: summary.clone(),
            created_at: Utc::now().to_rfc3339(),
            run_id: Some(run.id.clone()),
            pinned: true,
            attachments: Vec::new(),
        },
    );
    emit_agent_event(
        app,
        db,
        run,
        sequence,
        AgentEventKind::ContextCompressed,
        RunStatus::Reasoning,
        Some(summary),
        None,
        None,
        None,
        None,
        None,
    );
}

fn realtime_usage_snapshot(
    base: &UsageRecord,
    request_input_tokens: u64,
    output: &str,
    reasoning: &str,
    context_limit: u64,
    duration_ms: u64,
) -> UsageRecord {
    let output_tokens = estimate_tokens(output);
    let reasoning_tokens = estimate_tokens(reasoning);
    let input_tokens = base
        .input_tokens
        .unwrap_or_default()
        .saturating_add(request_input_tokens);
    let total_output = base
        .output_tokens
        .unwrap_or_default()
        .saturating_add(output_tokens);
    let total_reasoning = base
        .reasoning_tokens
        .unwrap_or_default()
        .saturating_add(reasoning_tokens);
    UsageRecord {
        input_tokens: Some(input_tokens),
        output_tokens: Some(total_output),
        cached_tokens: base.cached_tokens,
        reasoning_tokens: Some(total_reasoning),
        context_tokens: request_input_tokens
            .saturating_add(output_tokens)
            .saturating_add(reasoning_tokens),
        context_limit,
        cumulative_tokens: input_tokens
            .saturating_add(total_output)
            .saturating_add(total_reasoning),
        estimated: true,
        duration_ms: Some(duration_ms),
        first_token_ms: base.first_token_ms,
        estimated_cost_usd: base.estimated_cost_usd,
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_provider_stream_event(
    event: provider::ProviderStreamEvent,
    app: &tauri::AppHandle,
    db: &Arc<Database>,
    run: &RunRecord,
    sequence: &mut u64,
    stream_gate: &mut StreamingTextGate,
    live_output: &mut String,
    live_reasoning: &mut String,
    base_usage: &UsageRecord,
    request_input_tokens: u64,
    context_limit: u64,
    duration_ms: u64,
    last_usage_emit: &mut Instant,
    pending_usage_chars: &mut usize,
) {
    match event {
        provider::ProviderStreamEvent::TextDelta(delta) => {
            *pending_usage_chars = pending_usage_chars.saturating_add(delta.chars().count());
            live_output.push_str(&delta);
            if run.config.run_mode != RunMode::Goal {
                if let Some(visible) = stream_gate.push(&delta) {
                    emit_agent_event(
                        app,
                        db,
                        run,
                        sequence,
                        AgentEventKind::TextDelta,
                        RunStatus::Streaming,
                        Some(visible),
                        None,
                        None,
                        None,
                        None,
                        None,
                    );
                }
            }
        }
        provider::ProviderStreamEvent::ReasoningDelta(delta) => {
            *pending_usage_chars = pending_usage_chars.saturating_add(delta.chars().count());
            live_reasoning.push_str(&delta);
            emit_agent_event(
                app,
                db,
                run,
                sequence,
                AgentEventKind::ReasoningDelta,
                RunStatus::Reasoning,
                Some(delta),
                None,
                None,
                None,
                None,
                None,
            );
        }
    }

    if *pending_usage_chars >= 32 || last_usage_emit.elapsed() >= Duration::from_millis(100) {
        let usage = realtime_usage_snapshot(
            base_usage,
            request_input_tokens,
            live_output,
            live_reasoning,
            context_limit,
            duration_ms,
        );
        emit_agent_event(
            app,
            db,
            run,
            sequence,
            AgentEventKind::Usage,
            RunStatus::Streaming,
            None,
            None,
            Some(usage),
            None,
            None,
            None,
        );
        *last_usage_emit = Instant::now();
        *pending_usage_chars = 0;
    }
}

fn accumulate_usage(total: &mut UsageRecord, current: &UsageRecord) {
    let had_usage = total.input_tokens.is_some()
        || total.output_tokens.is_some()
        || total.reasoning_tokens.is_some();
    add_optional(&mut total.input_tokens, current.input_tokens);
    add_optional(&mut total.output_tokens, current.output_tokens);
    add_optional(&mut total.cached_tokens, current.cached_tokens);
    add_optional(&mut total.reasoning_tokens, current.reasoning_tokens);
    total.context_limit = total.context_limit.max(current.context_limit);
    total.estimated = if had_usage {
        total.estimated || current.estimated
    } else {
        current.estimated
    };
    if total.first_token_ms.is_none() {
        total.first_token_ms = current.first_token_ms;
    }
    if let Some(cost) = current.estimated_cost_usd {
        total.estimated_cost_usd = Some(total.estimated_cost_usd.unwrap_or_default() + cost);
    }
}

fn add_optional(target: &mut Option<u64>, value: Option<u64>) {
    if let Some(value) = value {
        *target = Some(target.unwrap_or_default() + value);
    }
}

fn estimate_cost(usage: &UsageRecord, pricing: &ModelOverride) -> Option<f64> {
    let cached = usage.cached_tokens.unwrap_or_default();
    let input = usage
        .input_tokens
        .unwrap_or_default()
        .saturating_sub(cached);
    let output = usage.output_tokens.unwrap_or_default();
    let reasoning = usage.reasoning_tokens.unwrap_or_default();
    let mut has_price = false;
    let mut cost = 0.0;
    for (tokens, price) in [
        (input, pricing.input_price_per_million),
        (
            cached,
            pricing
                .cache_price_per_million
                .or(pricing.input_price_per_million),
        ),
        (output, pricing.output_price_per_million),
        (reasoning, pricing.reasoning_price_per_million),
    ] {
        if let Some(price) = price {
            has_price = true;
            cost += tokens as f64 * price / 1_000_000.0;
        }
    }
    has_price.then_some(cost)
}

fn truncate_for_context(mut value: String, max_bytes: usize) -> String {
    if value.len() > max_bytes {
        let mut end = max_bytes.min(value.len());
        while end > 0 && !value.is_char_boundary(end) {
            end -= 1;
        }
        value.truncate(end);
        value.push_str("\n... content truncated ...");
    }
    value
}

fn estimate_tokens(text: &str) -> u64 {
    ((text.chars().count() as f64) / 3.8).ceil() as u64
}

#[derive(Default)]
struct StreamingTextGate {
    pending: String,
    decision: Option<bool>,
}

impl StreamingTextGate {
    const TOOL_PREFIX: &'static str = "```axiom-tool";

    /// Holds the short ambiguous prefix so tool protocol JSON never flashes in the conversation.
    fn push(&mut self, delta: &str) -> Option<String> {
        match self.decision {
            Some(true) => return Some(delta.to_string()),
            Some(false) => return None,
            None => self.pending.push_str(delta),
        }
        let trimmed = self.pending.trim_start();
        if trimmed.starts_with(Self::TOOL_PREFIX) {
            self.decision = Some(false);
            self.pending.clear();
            return None;
        }
        if trimmed.is_empty() || Self::TOOL_PREFIX.starts_with(trimmed) {
            return None;
        }
        self.decision = Some(true);
        Some(std::mem::take(&mut self.pending))
    }

    fn finish(&mut self) -> Option<String> {
        match self.decision {
            Some(false) => None,
            _ if self.pending.is_empty() => None,
            _ => Some(std::mem::take(&mut self.pending)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AttachmentTestDir(PathBuf);

    impl AttachmentTestDir {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("axiom-attachment-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).expect("create attachment test directory");
            Self(path)
        }
    }

    impl Drop for AttachmentTestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn utf16_bytes(value: &str, little_endian: bool, include_bom: bool) -> Vec<u8> {
        let mut bytes = if include_bom {
            if little_endian {
                vec![0xff, 0xfe]
            } else {
                vec![0xfe, 0xff]
            }
        } else {
            Vec::new()
        };
        for unit in value.encode_utf16() {
            let encoded = if little_endian {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            };
            bytes.extend_from_slice(&encoded);
        }
        bytes
    }

    fn snapshot(root: &Path, name: &str, bytes: &[u8]) -> AttachmentSnapshot {
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        let path = root.join(format!("{sha256}.snapshot"));
        fs::write(&path, bytes).expect("write attachment snapshot");
        AttachmentSnapshot {
            id: "untrusted-client-id".to_string(),
            name: name.to_string(),
            mime_type: "application/octet-stream".to_string(),
            size: 1,
            sha256,
            snapshot_path: path.to_string_lossy().to_string(),
            kind: "image".to_string(),
        }
    }

    #[test]
    fn attachment_validation_recomputes_untrusted_metadata() {
        let root = AttachmentTestDir::new();
        let input = snapshot(&root.0, "notes.txt", b"verified text");
        let validated = validate_attachment_snapshots(&root.0, vec![input]).unwrap();
        assert_eq!(validated.len(), 1);
        assert_eq!(validated[0].kind, "text");
        assert_eq!(validated[0].mime_type, "text/plain");
        assert_eq!(validated[0].size, 13);
        assert_ne!(validated[0].id, "untrusted-client-id");
    }

    #[test]
    fn attachment_validation_recognizes_image_magic_bytes() {
        let root = AttachmentTestDir::new();
        let input = snapshot(&root.0, "pixel.png", b"\x89PNG\r\n\x1a\nimage-data");
        let validated = validate_attachment_snapshots(&root.0, vec![input]).unwrap();
        assert_eq!(validated[0].kind, "image");
        assert_eq!(validated[0].mime_type, "image/png");
    }

    #[test]
    fn attachment_text_decoder_supports_utf16_and_rejects_malformed_content() {
        for little_endian in [true, false] {
            assert_eq!(
                decode_text_attachment(&utf16_bytes("Hello UTF-16", little_endian, true))
                    .as_deref(),
                Some("Hello UTF-16")
            );
            assert_eq!(
                decode_text_attachment(&utf16_bytes("Plain text", little_endian, false)).as_deref(),
                Some("Plain text")
            );
        }

        let mut odd_utf16 = utf16_bytes("odd", true, true);
        odd_utf16.push(0);
        assert!(decode_text_attachment(&odd_utf16).is_none());
        assert!(decode_text_attachment(b"valid utf-8\0with a nul").is_none());
        assert!(decode_text_attachment(b"text\x01\x02controls").is_none());
    }

    #[test]
    fn attachment_preparation_uses_content_not_file_extension() {
        let root = AttachmentTestDir::new();
        let disguised_binary = root.0.join("disguised.png");
        fs::write(&disguised_binary, [0, 1, 2, 3, 4]).unwrap();
        let error = prepare_attachment_paths(
            &root.0,
            vec![disguised_binary.to_string_lossy().to_string()],
        )
        .unwrap_err();
        assert!(error.contains("不支持的二进制文件"));

        let extensionless_image = root.0.join("extensionless-image");
        fs::write(&extensionless_image, b"\x89PNG\r\n\x1a\nimage-data").unwrap();
        let prepared = prepare_attachment_paths(
            &root.0,
            vec![extensionless_image.to_string_lossy().to_string()],
        )
        .unwrap();
        assert_eq!(prepared.len(), 1);
        assert_eq!(prepared[0].kind, "image");
        assert_eq!(prepared[0].mime_type, "image/png");
    }

    #[test]
    fn attachment_validation_rejects_hash_and_filename_tampering() {
        let root = AttachmentTestDir::new();
        let mut wrong_hash = snapshot(&root.0, "notes.txt", b"verified text");
        wrong_hash.sha256 = "0".repeat(64);
        assert!(validate_attachment_snapshots(&root.0, vec![wrong_hash])
            .unwrap_err()
            .contains("内容校验失败"));

        let mut wrong_name = snapshot(&root.0, "notes.txt", b"another text");
        let renamed = root.0.join("unexpected.snapshot");
        fs::rename(&wrong_name.snapshot_path, &renamed).unwrap();
        wrong_name.snapshot_path = renamed.to_string_lossy().to_string();
        assert!(validate_attachment_snapshots(&root.0, vec![wrong_name])
            .unwrap_err()
            .contains("文件名校验失败"));
    }

    #[test]
    fn attachment_validation_rejects_paths_outside_snapshot_root() {
        let root = AttachmentTestDir::new();
        let outside = AttachmentTestDir::new();
        let input = snapshot(&outside.0, "outside.txt", b"outside");
        assert!(validate_attachment_snapshots(&root.0, vec![input])
            .unwrap_err()
            .contains("路径不安全"));
    }

    #[test]
    fn workspace_paths_compare_with_platform_semantics() {
        #[cfg(windows)]
        {
            assert!(paths_equal("D:/Axiom/src", "d:\\axiom\\src"));
            assert!(!paths_equal("D:/Axiom/src", "D:/Other/src"));
        }
        #[cfg(not(windows))]
        {
            assert!(paths_equal("/tmp/axiom", "/tmp/axiom"));
            assert!(!paths_equal("/tmp/Axiom", "/tmp/axiom"));
        }
    }

    #[test]
    fn strips_valid_goal_control_without_exposing_protocol_text() {
        let (visible, action) =
            strip_goal_control("Verified work.\n```axiom-goal\n{\"action\":\"continue\"}\n```")
                .unwrap();
        assert_eq!(visible, "Verified work.");
        assert_eq!(action, GoalAction::Continue);
    }

    #[test]
    fn rejects_missing_or_invalid_goal_control() {
        for response in [
            "Useful text without control",
            "Useful text\n```axiom-goal\n{\"action\":\"unknown\"}\n```",
            "Useful text\n```axiom-goal\nnot-json\n```",
            "Useful text\n```axiom-goal\n{\"action\":\"complete\"}",
            "Useful text\n```axiom-goal\n{\"action\":\"complete\"}\n``` trailing",
        ] {
            let error = strip_goal_control(response).unwrap_err();
            assert!(error.contains("Goal 响应缺少有效的控制协议"));
            assert!(!error.contains(response));
        }
    }

    #[test]
    fn parses_axiom_tool_protocol() {
        let value = parse_tool_call(
            "```axiom-tool\n{\"name\":\"read_file\",\"arguments\":{\"path\":\"src/main.rs\"}}\n```",
        )
        .expect("tool call");
        assert_eq!(value.name, "read_file");
        assert_eq!(
            required_arg(&value.arguments, "path").unwrap(),
            "src/main.rs"
        );
    }

    #[test]
    fn redacts_sensitive_tool_arguments() {
        let value = redact_tool_arguments(&json!({"content":"secret source","path":"a.txt"}));
        assert_eq!(value["content"], "[REDACTED]");
        assert_eq!(value["path"], "a.txt");
    }

    #[test]
    fn serializes_writable_runs_per_workspace_but_allows_read_only_browsing() {
        let (cancel, _receiver) = watch::channel(false);
        let existing = RunningRun {
            thread_id: "thread-a".to_string(),
            workspace_path: "D:\\Axiom".to_string(),
            writable: true,
            cancel,
        };
        assert!(run_conflicts(&existing, "thread-a", "D:\\Axiom", false));
        assert!(run_conflicts(&existing, "thread-b", "D:\\Axiom", true));
        assert!(!run_conflicts(&existing, "thread-b", "D:\\Axiom", false));
        assert!(!run_conflicts(&existing, "thread-b", "D:\\Other", true));
    }
}

fn migrate_secret_fields(db: &Database) -> Result<(), String> {
    // MCP credentials still need one-time protection. Provider credentials are intentionally
    // left untouched here: re-saving a legacy native profile would convert it into a new
    // OpenAI-compatible profile and could change historical behavior.
    for server in db.list_mcp_servers()? {
        let protected = mcp::protect_credentials(server)?;
        db.save_mcp_server(&protected)?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
            let db = Database::open(&app_data)?;
            migrate_secret_fields(&db)?;
            app.manage(AppState {
                db: Arc::new(db),
                running: Arc::new(Mutex::new(HashMap::new())),
                pending_approvals: Arc::new(Mutex::new(HashMap::new())),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            add_project,
            create_thread,
            get_thread,
            archive_thread,
            delete_thread,
            restore_context_snapshot,
            save_provider,
            delete_provider,
            get_model_override,
            save_model_override,
            discover_models,
            discover_provider_models_draft,
            test_provider_model_draft,
            prepare_attachments,
            test_provider,
            save_mcp_server,
            delete_mcp_server,
            test_mcp_server,
            save_settings,
            list_project_files,
            read_project_file,
            search_project_files,
            write_project_file,
            apply_project_patch,
            delete_project_file,
            restore_run_changes,
            restore_run_file_changes,
            open_project_file_external,
            get_git_summary,
            execute_shell,
            start_agent_run,
            cancel_agent_run,
            resume_goal,
            finish_goal,
            respond_approval,
            respond_user_question
        ])
        .run(tauri::generate_context!())
        .expect("error while running Axiom");
}
