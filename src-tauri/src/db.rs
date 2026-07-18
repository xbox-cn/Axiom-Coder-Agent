use crate::models::*;
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use std::{fs, path::Path};
use uuid::Uuid;

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(app_data_dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(app_data_dir).map_err(|e| e.to_string())?;
        let path = app_data_dir.join("axiom.db");
        if path.exists() {
            let backup = app_data_dir.join("axiom.db.bak");
            let _ = fs::copy(&path, backup);
        }
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| e.to_string())?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| e.to_string())?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        db.recover_interrupted_runs()?;
        db.initialize_defaults()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL UNIQUE,
                favorite INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS threads (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                title TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'idle',
                unread_approval INTEGER NOT NULL DEFAULT 0,
                archived INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_threads_project_updated ON threads(project_id, updated_at DESC);
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                run_id TEXT,
                pinned INTEGER NOT NULL DEFAULT 0,
                attachments_json TEXT NOT NULL DEFAULT '[]'
            );
            CREATE INDEX IF NOT EXISTS idx_messages_thread_created ON messages(thread_id, created_at);
            CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(message_id UNINDEXED, thread_id UNINDEXED, content);
            CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
              INSERT INTO messages_fts(message_id, thread_id, content) VALUES (new.id, new.thread_id, new.content);
            END;
            CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
              DELETE FROM messages_fts WHERE message_id = old.id;
            END;
            CREATE TABLE IF NOT EXISTS runs (
                id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
                status TEXT NOT NULL,
                config_json TEXT NOT NULL,
                usage_json TEXT NOT NULL,
                reasoning_content TEXT NOT NULL DEFAULT '',
                error TEXT,
                started_at TEXT NOT NULL,
                completed_at TEXT
            );
            CREATE TABLE IF NOT EXISTS goals (
                id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL UNIQUE REFERENCES runs(id) ON DELETE CASCADE,
                thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
                status TEXT NOT NULL,
                turn_count INTEGER NOT NULL DEFAULT 0,
                started_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                completed_at TEXT
            );
            CREATE TABLE IF NOT EXISTS goal_turns (
                id TEXT PRIMARY KEY,
                goal_id TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
                turn_number INTEGER NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT,
                completed_at TEXT,
                UNIQUE(goal_id, turn_number)
            );
            CREATE TABLE IF NOT EXISTS run_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
                sequence INTEGER NOT NULL,
                event_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(run_id, sequence)
            );
            CREATE TABLE IF NOT EXISTS provider_profiles (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                name TEXT NOT NULL,
                base_url TEXT NOT NULL,
                default_model TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                timeout_seconds INTEGER NOT NULL DEFAULT 120,
                extra_headers_json TEXT NOT NULL DEFAULT '{}',
                credential_ref TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                api_type TEXT NOT NULL DEFAULT 'chat-completions'
            );
            CREATE TABLE IF NOT EXISTS provider_models (
                provider_id TEXT NOT NULL REFERENCES provider_profiles(id) ON DELETE CASCADE,
                model_id TEXT NOT NULL,
                display_name TEXT NOT NULL,
                context_window_tokens INTEGER,
                source TEXT NOT NULL DEFAULT 'manual',
                sort_order INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY(provider_id, model_id)
            );
            CREATE TABLE IF NOT EXISTS model_overrides (
                provider_id TEXT NOT NULL,
                model_id TEXT NOT NULL,
                descriptor_json TEXT NOT NULL,
                PRIMARY KEY(provider_id, model_id)
            );
            CREATE TABLE IF NOT EXISTS mcp_servers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                scope TEXT NOT NULL,
                project_id TEXT,
                transport TEXT NOT NULL,
                config_json TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                status TEXT NOT NULL DEFAULT 'stopped',
                last_error TEXT,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS tool_calls (
                id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                arguments_json TEXT NOT NULL,
                result_text TEXT,
                status TEXT NOT NULL,
                started_at TEXT NOT NULL,
                completed_at TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_tool_calls_run ON tool_calls(run_id, started_at);
            CREATE TABLE IF NOT EXISTS approvals (
                id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
                tool_name TEXT NOT NULL,
                summary TEXT NOT NULL,
                arguments_json TEXT NOT NULL,
                decision TEXT,
                created_at TEXT NOT NULL,
                decided_at TEXT
            );
            CREATE TABLE IF NOT EXISTS usage_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
                usage_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS context_snapshots (
                id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
                run_id TEXT,
                summary TEXT NOT NULL,
                token_count INTEGER NOT NULL,
                start_message_id TEXT,
                end_message_id TEXT,
                source_message_ids_json TEXT NOT NULL DEFAULT '[]',
                active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS change_checkpoints (
                id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
                project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                manifest_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS app_settings (
                key TEXT PRIMARY KEY,
                value_json TEXT NOT NULL
            );
            PRAGMA user_version = 7;
            "#,
        )
        .map_err(|e| e.to_string())?;
        for (column, definition) in [
            ("run_id", "TEXT"),
            ("start_message_id", "TEXT"),
            ("end_message_id", "TEXT"),
            ("source_message_ids_json", "TEXT NOT NULL DEFAULT '[]'"),
            ("active", "INTEGER NOT NULL DEFAULT 1"),
        ] {
            let exists = {
                let mut statement = conn
                    .prepare("PRAGMA table_info(context_snapshots)")
                    .map_err(|e| e.to_string())?;
                let found = statement
                    .query_map([], |row| row.get::<_, String>(1))
                    .map_err(|e| e.to_string())?
                    .filter_map(Result::ok)
                    .any(|name| name == column);
                found
            };
            if !exists {
                conn.execute(
                    &format!("ALTER TABLE context_snapshots ADD COLUMN {column} {definition}"),
                    [],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        for (table, column, definition) in [
            ("messages", "attachments_json", "TEXT NOT NULL DEFAULT '[]'"),
            (
                "provider_profiles",
                "api_type",
                "TEXT NOT NULL DEFAULT 'chat-completions'",
            ),
            ("goal_turns", "updated_at", "TEXT"),
            ("goal_turns", "completed_at", "TEXT"),
            ("threads", "archived", "INTEGER NOT NULL DEFAULT 0"),
            ("runs", "reasoning_content", "TEXT NOT NULL DEFAULT ''"),
        ] {
            let exists = {
                let mut statement = conn
                    .prepare(&format!("PRAGMA table_info({table})"))
                    .map_err(|e| e.to_string())?;
                let mut rows = statement
                    .query_map([], |row| row.get::<_, String>(1))
                    .map_err(|e| e.to_string())?;
                let found = rows.any(|row| row.is_ok_and(|name| name == column));
                found
            };
            if !exists {
                conn.execute(
                    &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
                    [],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    fn recover_interrupted_runs(&self) -> Result<(), String> {
        let mut conn = self.conn.lock();
        let transaction = conn.transaction().map_err(|error| error.to_string())?;
        let runs: Vec<(String, String)> = {
            let mut statement = transaction
                .prepare(
                    "SELECT id, thread_id FROM runs WHERE status IN ('queued','reasoning','streaming','tool-running','awaiting-approval')",
                )
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(|error| error.to_string())?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| error.to_string())?
        };
        if runs.is_empty() {
            transaction.commit().map_err(|error| error.to_string())?;
            return Ok(());
        }

        let now = Utc::now().to_rfc3339();
        for (run_id, thread_id) in runs {
            let partial = {
                let mut statement = transaction
                    .prepare("SELECT event_json FROM run_events WHERE run_id=?1 ORDER BY sequence")
                    .map_err(|error| error.to_string())?;
                let rows = statement
                    .query_map(params![run_id], |row| row.get::<_, String>(0))
                    .map_err(|error| error.to_string())?;
                let mut output = String::new();
                for row in rows {
                    let raw = row.map_err(|error| error.to_string())?;
                    let Ok(event) = serde_json::from_str::<AgentEvent>(&raw) else {
                        continue;
                    };
                    if matches!(event.kind, AgentEventKind::TextDelta) {
                        if let Some(content) = event.content {
                            output.push_str(&content);
                        }
                    }
                }
                output
            };
            let already_materialized: bool = transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM messages WHERE run_id=?1 AND role='assistant')",
                    params![run_id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            if !partial.trim().is_empty() && !already_materialized {
                let content = format!(
                    "{}\n\n> Axiom recovered this partial response after an interrupted run.",
                    partial.trim_end()
                );
                transaction
                    .execute(
                        "INSERT INTO messages (id, thread_id, role, content, created_at, run_id, pinned) VALUES (?1, ?2, 'assistant', ?3, ?4, ?5, 0)",
                        params![Uuid::new_v4().to_string(), thread_id, content, now, run_id],
                    )
                    .map_err(|error| error.to_string())?;
            }
            transaction
                .execute(
                    "UPDATE runs SET status='failed', error='Application exited before the run completed', completed_at=?2 WHERE id=?1",
                    params![run_id, now],
                )
                .map_err(|error| error.to_string())?;
            transaction
                .execute(
                    "UPDATE threads SET status='failed', unread_approval=0, updated_at=?2 WHERE id=?1",
                    params![thread_id, now],
                )
                .map_err(|error| error.to_string())?;
            transaction
                .execute(
                    "UPDATE approvals SET decision='interrupted', decided_at=?2 WHERE run_id=?1 AND decision IS NULL",
                    params![run_id, now],
                )
                .map_err(|error| error.to_string())?;
            transaction
                .execute(
                    "UPDATE tool_calls SET status='failed', result_text='Interrupted by application exit', completed_at=?2 WHERE run_id=?1 AND status='running'",
                    params![run_id, now],
                )
                .map_err(|error| error.to_string())?;
            transaction
                .execute(
                    "UPDATE goals SET status='paused', updated_at=?2, completed_at=NULL WHERE run_id=?1 AND status IN ('running','awaiting-approval')",
                    params![run_id, now],
                )
                .map_err(|error| error.to_string())?;
        }
        transaction.commit().map_err(|error| error.to_string())
    }

    fn initialize_defaults(&self) -> Result<(), String> {
        let settings = serde_json::to_string(&AppSettings::default()).map_err(|e| e.to_string())?;
        let conn = self.conn.lock();
        conn.execute(
            "INSERT OR IGNORE INTO app_settings (key, value_json) VALUES ('main', ?1)",
            params![settings],
        )
        .map_err(|e| e.to_string())?;

        // Remove only untouched, credential-free records created by earlier Axiom builds.
        let defaults = [
            (
                "openai",
                "open-ai",
                "OpenAI",
                "https://api.openai.com/v1",
                "gpt-5.4",
            ),
            (
                "anthropic",
                "anthropic",
                "Anthropic",
                "https://api.anthropic.com",
                "claude-sonnet-4-5",
            ),
            (
                "gemini",
                "gemini",
                "Google Gemini",
                "https://generativelanguage.googleapis.com",
                "gemini-2.5-pro",
            ),
            (
                "openrouter",
                "open-router",
                "OpenRouter",
                "https://openrouter.ai/api/v1",
                "openai/gpt-5.4",
            ),
            (
                "ollama-local",
                "ollama",
                "Ollama (Local)",
                "http://127.0.0.1:11434",
                "qwen3-coder",
            ),
            (
                "compatible",
                "open-ai-compatible",
                "OpenAI Compatible",
                "http://127.0.0.1:8000/v1",
                "local-model",
            ),
        ];
        for (id, kind, name, base_url, model) in defaults {
            conn.execute(
                "DELETE FROM provider_profiles WHERE id=?1 AND kind=?2 AND name=?3 AND base_url=?4 AND default_model=?5 AND enabled=1 AND timeout_seconds=120 AND extra_headers_json='{}' AND credential_ref IS NULL AND api_type='chat-completions' AND NOT EXISTS(SELECT 1 FROM model_overrides WHERE provider_id=?1) AND NOT EXISTS(SELECT 1 FROM provider_models WHERE provider_id=?1)",
                params![id, kind, name, base_url, model],
            ).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn bootstrap(&self) -> Result<AppBootstrap, String> {
        Ok(AppBootstrap {
            projects: self.list_projects()?,
            threads: self.list_threads(None)?,
            providers: self.list_providers()?,
            mcp_servers: self.list_mcp_servers()?,
            settings: self.get_settings()?,
        })
    }

    pub fn add_project(&self, path: &Path) -> Result<Project, String> {
        let canonical = path
            .canonicalize()
            .map_err(|e| format!("无法打开项目路径: {e}"))?;
        if !canonical.is_dir() {
            return Err("项目路径必须是目录".to_string());
        }
        let path_text = canonical.to_string_lossy().to_string();
        if let Some(existing) = self.find_project_by_path(&path_text)? {
            return Ok(existing);
        }
        let id = Uuid::new_v4().to_string();
        let name = canonical
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| path_text.clone());
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO projects (id, name, path, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
            params![id, name, path_text, now],
        )
        .map_err(|e| e.to_string())?;
        drop(conn);
        self.find_project_by_path(&path_text)?
            .ok_or_else(|| "项目创建失败".to_string())
    }

    fn find_project_by_path(&self, path: &str) -> Result<Option<Project>, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT id, name, path, favorite, created_at, updated_at FROM projects WHERE path = ?1",
            params![path],
            |row| Ok(project_from_row(row)),
        )
        .optional()
        .map_err(|e| e.to_string())
    }

    pub fn list_projects(&self) -> Result<Vec<Project>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT id, name, path, favorite, created_at, updated_at FROM projects ORDER BY favorite DESC, updated_at DESC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| Ok(project_from_row(row)))
            .map_err(|e| e.to_string())?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row.map_err(|e| e.to_string())?);
        }
        Ok(items)
    }

    pub fn create_thread(
        &self,
        project_id: &str,
        title: Option<&str>,
    ) -> Result<ThreadSummary, String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let title = title.unwrap_or("新任务").trim();
        let title = if title.is_empty() { "新任务" } else { title };
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO threads (id, project_id, title, status, created_at, updated_at) VALUES (?1, ?2, ?3, 'idle', ?4, ?4)",
            params![id, project_id, title, now],
        ).map_err(|e| e.to_string())?;
        drop(conn);
        self.get_thread_summary(&id)
    }

    pub fn list_threads(&self, project_id: Option<&str>) -> Result<Vec<ThreadSummary>, String> {
        let conn = self.conn.lock();
        let sql = if project_id.is_some() {
            "SELECT id, project_id, title, status, unread_approval, archived, created_at, updated_at FROM threads WHERE project_id = ?1 ORDER BY updated_at DESC"
        } else {
            "SELECT id, project_id, title, status, unread_approval, archived, created_at, updated_at FROM threads ORDER BY updated_at DESC"
        };
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let mapper = |row: &rusqlite::Row<'_>| -> rusqlite::Result<ThreadSummary> {
            Ok(ThreadSummary {
                id: row.get(0)?,
                project_id: row.get(1)?,
                title: row.get(2)?,
                status: parse_status(&row.get::<_, String>(3)?),
                unread_approval: row.get::<_, i64>(4)? != 0,
                archived: row.get::<_, i64>(5)? != 0,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        };
        let mut items = Vec::new();
        if let Some(id) = project_id {
            let rows = stmt
                .query_map(params![id], mapper)
                .map_err(|e| e.to_string())?;
            for row in rows {
                items.push(row.map_err(|e| e.to_string())?);
            }
        } else {
            let rows = stmt.query_map([], mapper).map_err(|e| e.to_string())?;
            for row in rows {
                items.push(row.map_err(|e| e.to_string())?);
            }
        }
        Ok(items)
    }

    fn get_thread_summary(&self, id: &str) -> Result<ThreadSummary, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT id, project_id, title, status, unread_approval, archived, created_at, updated_at FROM threads WHERE id = ?1",
            params![id],
            |row| Ok(ThreadSummary {
                id: row.get(0)?, project_id: row.get(1)?, title: row.get(2)?,
                status: parse_status(&row.get::<_, String>(3)?), unread_approval: row.get::<_, i64>(4)? != 0,
                archived: row.get::<_, i64>(5)? != 0,
                created_at: row.get(6)?, updated_at: row.get(7)?,
            }),
        ).map_err(|e| e.to_string())
    }

    pub fn archive_thread(&self, id: &str, archived: bool) -> Result<(), String> {
        let changed = self
            .conn
            .lock()
            .execute(
                "UPDATE threads SET archived=?2, updated_at=?3 WHERE id=?1",
                params![id, if archived { 1 } else { 0 }, Utc::now().to_rfc3339()],
            )
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err("任务不存在".to_string());
        }
        Ok(())
    }

    pub fn delete_thread(&self, id: &str) -> Result<(), String> {
        let changed = self
            .conn
            .lock()
            .execute("DELETE FROM threads WHERE id=?1", params![id])
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err("任务不存在".to_string());
        }
        Ok(())
    }

    pub fn get_thread(&self, id: &str) -> Result<ThreadDetail, String> {
        let thread = self.get_thread_summary(id)?;
        let conn = self.conn.lock();
        let mut msg_stmt = conn.prepare("SELECT id, thread_id, role, content, created_at, run_id, pinned, attachments_json FROM messages WHERE thread_id = ?1 ORDER BY created_at").map_err(|e| e.to_string())?;
        let msg_rows = msg_stmt
            .query_map(params![id], |row| {
                Ok(Message {
                    id: row.get(0)?,
                    thread_id: row.get(1)?,
                    role: parse_role(&row.get::<_, String>(2)?),
                    content: row.get(3)?,
                    created_at: row.get(4)?,
                    run_id: row.get(5)?,
                    pinned: row.get::<_, i64>(6)? != 0,
                    attachments: serde_json::from_str(&row.get::<_, String>(7)?)
                        .unwrap_or_default(),
                })
            })
            .map_err(|e| e.to_string())?;
        let mut messages = Vec::new();
        for row in msg_rows {
            messages.push(row.map_err(|e| e.to_string())?);
        }
        let mut run_stmt = conn.prepare("SELECT id, thread_id, status, config_json, usage_json, reasoning_content, error, started_at, completed_at FROM runs WHERE thread_id = ?1 ORDER BY started_at").map_err(|e| e.to_string())?;
        let run_rows = run_stmt
            .query_map(params![id], |row| {
                let config_json: String = row.get(3)?;
                let usage_json: String = row.get(4)?;
                Ok(RunRecord {
                    id: row.get(0)?,
                    thread_id: row.get(1)?,
                    status: parse_status(&row.get::<_, String>(2)?),
                    config: serde_json::from_str(&config_json)
                        .unwrap_or_else(|_| default_run_config()),
                    usage: serde_json::from_str(&usage_json).unwrap_or_default(),
                    reasoning_content: row.get(5)?,
                    error: row.get(6)?,
                    started_at: row.get(7)?,
                    completed_at: row.get(8)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut runs = Vec::new();
        for row in run_rows {
            runs.push(row.map_err(|e| e.to_string())?);
        }
        let mut snapshot_stmt = conn.prepare(
            "SELECT id, thread_id, run_id, summary, token_count, start_message_id, end_message_id, source_message_ids_json, active, created_at FROM context_snapshots WHERE thread_id=?1 ORDER BY created_at DESC"
        ).map_err(|e| e.to_string())?;
        let snapshot_rows = snapshot_stmt
            .query_map(params![id], context_snapshot_from_row)
            .map_err(|e| e.to_string())?;
        let mut context_snapshots = Vec::new();
        for row in snapshot_rows {
            context_snapshots.push(row.map_err(|e| e.to_string())?);
        }
        let mut goal_stmt = conn.prepare(
            "SELECT id, run_id, thread_id, status, turn_count, started_at, updated_at, completed_at FROM goals WHERE thread_id=?1 ORDER BY started_at"
        ).map_err(|e| e.to_string())?;
        let goal_rows = goal_stmt
            .query_map(params![id], |row| {
                Ok(GoalRecord {
                    id: row.get(0)?,
                    run_id: row.get(1)?,
                    thread_id: row.get(2)?,
                    status: row.get(3)?,
                    turn_count: row.get::<_, i64>(4)?.max(0) as u64,
                    started_at: row.get(5)?,
                    updated_at: row.get(6)?,
                    completed_at: row.get(7)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut goals = Vec::new();
        for row in goal_rows {
            goals.push(row.map_err(|e| e.to_string())?);
        }
        Ok(ThreadDetail {
            thread,
            messages,
            runs,
            context_snapshots,
            goals,
        })
    }

    pub fn add_message(
        &self,
        thread_id: &str,
        role: MessageRole,
        content: &str,
        run_id: Option<&str>,
    ) -> Result<Message, String> {
        self.add_message_with_attachments(thread_id, role, content, run_id, Vec::new())
    }

    pub fn add_message_with_attachments(
        &self,
        thread_id: &str,
        role: MessageRole,
        content: &str,
        run_id: Option<&str>,
        attachments: Vec<AttachmentSnapshot>,
    ) -> Result<Message, String> {
        let message = Message {
            id: Uuid::new_v4().to_string(),
            thread_id: thread_id.to_string(),
            role,
            content: content.to_string(),
            created_at: Utc::now().to_rfc3339(),
            run_id: run_id.map(str::to_string),
            pinned: false,
            attachments,
        };
        let attachments_json =
            serde_json::to_string(&message.attachments).map_err(|e| e.to_string())?;
        let conn = self.conn.lock();
        conn.execute("INSERT INTO messages (id, thread_id, role, content, created_at, run_id, pinned, attachments_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7)",
            params![message.id, message.thread_id, role_string(message.role), message.content, message.created_at, message.run_id, attachments_json])
            .map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE threads SET updated_at = ?2 WHERE id = ?1",
            params![thread_id, message.created_at],
        )
        .map_err(|e| e.to_string())?;
        Ok(message)
    }

    pub fn create_run(
        &self,
        thread_id: &str,
        config: &RunConfigSnapshot,
    ) -> Result<RunRecord, String> {
        self.create_run_with_context(thread_id, config, 128_000)
    }

    pub fn create_run_with_context(
        &self,
        thread_id: &str,
        config: &RunConfigSnapshot,
        context_limit: u64,
    ) -> Result<RunRecord, String> {
        let run = RunRecord {
            id: Uuid::new_v4().to_string(),
            thread_id: thread_id.to_string(),
            status: RunStatus::Queued,
            config: config.clone(),
            usage: UsageRecord {
                context_limit,
                estimated: true,
                ..Default::default()
            },
            reasoning_content: String::new(),
            error: None,
            started_at: Utc::now().to_rfc3339(),
            completed_at: None,
        };
        let conn = self.conn.lock();
        conn.execute("INSERT INTO runs (id, thread_id, status, config_json, usage_json, started_at) VALUES (?1, ?2, 'queued', ?3, ?4, ?5)",
            params![run.id, run.thread_id, serde_json::to_string(&run.config).unwrap(), serde_json::to_string(&run.usage).unwrap(), run.started_at])
            .map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE threads SET status = 'queued', updated_at = ?2 WHERE id = ?1",
            params![thread_id, run.started_at],
        )
        .map_err(|e| e.to_string())?;
        Ok(run)
    }

    pub fn get_run(&self, run_id: &str) -> Result<RunRecord, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT id, thread_id, status, config_json, usage_json, reasoning_content, error, started_at, completed_at FROM runs WHERE id=?1",
            params![run_id],
            |row| {
                let config_json: String = row.get(3)?;
                let usage_json: String = row.get(4)?;
                Ok(RunRecord {
                    id: row.get(0)?,
                    thread_id: row.get(1)?,
                    status: parse_status(&row.get::<_, String>(2)?),
                    config: serde_json::from_str(&config_json)
                        .unwrap_or_else(|_| default_run_config()),
                    usage: serde_json::from_str(&usage_json).unwrap_or_default(),
                    reasoning_content: row.get(5)?,
                    error: row.get(6)?,
                    started_at: row.get(7)?,
                    completed_at: row.get(8)?,
                })
            },
        )
        .map_err(|_| "Run does not exist".to_string())
    }

    pub fn create_goal(&self, run: &RunRecord) -> Result<(), String> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "INSERT OR IGNORE INTO goals (id, run_id, thread_id, status, turn_count, started_at, updated_at) VALUES (?1, ?1, ?2, 'running', 0, ?3, ?3)",
            params![run.id, run.thread_id, now],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn resume_goal_run(&self, run_id: &str) -> Result<RunRecord, String> {
        let now = Utc::now().to_rfc3339();
        let mut conn = self.conn.lock();
        let transaction = conn.transaction().map_err(|error| error.to_string())?;
        let (thread_id, config_json, usage_json, reasoning_content, started_at, goal_status): (
            String,
            String,
            String,
            String,
            String,
            String,
        ) = transaction
            .query_row(
                "SELECT r.thread_id, r.config_json, r.usage_json, r.reasoning_content, r.started_at, g.status FROM runs r JOIN goals g ON g.run_id=r.id WHERE r.id=?1",
                params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
            )
            .map_err(|_| "Goal does not exist".to_string())?;
        let config: RunConfigSnapshot =
            serde_json::from_str(&config_json).map_err(|error| error.to_string())?;
        if config.run_mode != RunMode::Goal {
            return Err("Only Goal runs can be resumed".to_string());
        }
        if !matches!(goal_status.as_str(), "paused" | "blocked" | "failed") {
            return Err(format!("Goal cannot be resumed from status: {goal_status}"));
        }
        let usage: UsageRecord = serde_json::from_str(&usage_json).unwrap_or_default();
        transaction
            .execute(
                "UPDATE runs SET status='queued', error=NULL, completed_at=NULL WHERE id=?1",
                params![run_id],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "UPDATE goals SET status='running', updated_at=?2, completed_at=NULL WHERE run_id=?1",
                params![run_id, now],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "UPDATE threads SET status='queued', unread_approval=0, updated_at=?2 WHERE id=?1",
                params![thread_id, now],
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(RunRecord {
            id: run_id.to_string(),
            thread_id,
            status: RunStatus::Queued,
            config,
            usage,
            reasoning_content,
            error: None,
            started_at,
            completed_at: None,
        })
    }

    pub fn last_event_sequence(&self, run_id: &str) -> Result<u64, String> {
        let conn = self.conn.lock();
        let sequence: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(sequence), 0) FROM run_events WHERE run_id=?1",
                params![run_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        Ok(sequence.max(0) as u64)
    }

    pub fn add_goal_turn(&self, run_id: &str) -> Result<String, String> {
        let now = Utc::now().to_rfc3339();
        let turn_id = Uuid::new_v4().to_string();
        let mut conn = self.conn.lock();
        let transaction = conn.transaction().map_err(|error| error.to_string())?;
        let turn: i64 = transaction
            .query_row(
                "SELECT turn_count + 1 FROM goals WHERE run_id=?1",
                params![run_id],
                |row| row.get(0),
            )
            .map_err(|_| "Goal state is missing".to_string())?;
        transaction
            .execute(
                "UPDATE goals SET turn_count=?2, status='running', updated_at=?3 WHERE run_id=?1",
                params![run_id, turn, now],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO goal_turns (id, goal_id, turn_number, status, created_at, updated_at) VALUES (?1, ?2, ?3, 'running', ?4, ?4)",
                params![turn_id, run_id, turn, now],
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(turn_id)
    }

    pub fn update_goal_turn_status(&self, run_id: &str, status: &str) -> Result<(), String> {
        if !matches!(
            status,
            "running" | "awaiting-approval" | "paused" | "completed" | "failed" | "blocked"
        ) {
            return Err(format!("Invalid Goal turn status: {status}"));
        }
        let now = Utc::now().to_rfc3339();
        let terminal = matches!(status, "paused" | "completed" | "failed" | "blocked");
        let conn = self.conn.lock();
        let changed = conn
            .execute(
                "UPDATE goal_turns SET status=?2, updated_at=?3, completed_at=CASE WHEN ?4 THEN ?3 ELSE NULL END WHERE id=(SELECT id FROM goal_turns WHERE goal_id=?1 ORDER BY turn_number DESC LIMIT 1)",
                params![run_id, status, now, terminal],
            )
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err("Goal turn does not exist".to_string());
        }
        Ok(())
    }

    pub fn goal_status(&self, run_id: &str) -> Result<String, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT status FROM goals WHERE run_id=?1",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(|_| "Goal does not exist".to_string())
    }

    pub fn update_goal_status(&self, run_id: &str, status: &str) -> Result<(), String> {
        let now = Utc::now().to_rfc3339();
        let terminal = matches!(status, "completed" | "failed" | "blocked");
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE goals SET status=?2, updated_at=?3, completed_at=CASE WHEN ?4 THEN ?3 ELSE NULL END WHERE run_id=?1",
            params![run_id, status, now, terminal],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn pause_goal_if_active(&self, run_id: &str) -> Result<(), String> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE goals SET status='paused', updated_at=?2, completed_at=NULL WHERE run_id=?1 AND status IN ('running', 'awaiting-approval')",
            params![run_id, now],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn update_run(
        &self,
        run_id: &str,
        thread_id: &str,
        status: RunStatus,
        usage: Option<&UsageRecord>,
        error: Option<&str>,
    ) -> Result<(), String> {
        let now = Utc::now().to_rfc3339();
        let terminal = matches!(
            status,
            RunStatus::Completed | RunStatus::Failed | RunStatus::Cancelled
        );
        let status_text = status_string(status);
        let conn = self.conn.lock();
        if let Some(usage) = usage {
            conn.execute("UPDATE runs SET status = ?2, usage_json = ?3, error = ?4, completed_at = CASE WHEN ?5 THEN ?6 ELSE completed_at END WHERE id = ?1",
                params![run_id, status_text, serde_json::to_string(usage).unwrap(), error, terminal, now]).map_err(|e| e.to_string())?;
        } else {
            conn.execute("UPDATE runs SET status = ?2, error = ?3, completed_at = CASE WHEN ?4 THEN ?5 ELSE completed_at END WHERE id = ?1",
                params![run_id, status_text, error, terminal, now]).map_err(|e| e.to_string())?;
        }
        conn.execute(
            "UPDATE threads SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![thread_id, status_text, now],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn update_run_usage(&self, run_id: &str, usage: &UsageRecord) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE runs SET usage_json=?2 WHERE id=?1",
            params![
                run_id,
                serde_json::to_string(usage).map_err(|error| error.to_string())?
            ],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn append_run_reasoning(&self, run_id: &str, delta: &str) -> Result<(), String> {
        if delta.is_empty() {
            return Ok(());
        }
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE runs SET reasoning_content=reasoning_content || ?2 WHERE id=?1",
            params![run_id, delta],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn save_event(&self, event: &AgentEvent) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute("INSERT INTO run_events (run_id, sequence, event_json, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![event.run_id, event.sequence as i64, serde_json::to_string(event).unwrap(), event.created_at]).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list_providers(&self) -> Result<Vec<ProviderProfile>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, kind, name, base_url, default_model, enabled, timeout_seconds, extra_headers_json, credential_ref, created_at, updated_at, api_type FROM provider_profiles ORDER BY updated_at DESC, name").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                let headers: String = row.get(7)?;
                let credential_ref: Option<String> = row.get(8)?;
                let kind_text: String = row.get(1)?;
                let api_text: String = row.get(11)?;
                Ok(ProviderProfile {
                    id: row.get(0)?,
                    kind: parse_provider_kind(&kind_text),
                    name: row.get(2)?,
                    base_url: row.get(3)?,
                    default_model: row.get(4)?,
                    enabled: row.get::<_, i64>(5)? != 0,
                    timeout_seconds: row.get::<_, i64>(6)?.max(10) as u64,
                    extra_headers: serde_json::from_str(&headers)
                        .unwrap_or_else(|_| serde_json::json!({})),
                    has_credential: credential_ref.is_some(),
                    created_at: row.get(9)?,
                    updated_at: row.get(10)?,
                    api_type: parse_provider_api_type(&api_text),
                    models: Vec::new(),
                    legacy: kind_text != "open-ai-compatible",
                })
            })
            .map_err(|e| e.to_string())?;
        let mut items = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())?;
        drop(stmt);
        for item in &mut items {
            let mut model_stmt = conn.prepare("SELECT model_id, display_name, context_window_tokens, source FROM provider_models WHERE provider_id=?1 ORDER BY sort_order, model_id").map_err(|e| e.to_string())?;
            let model_rows = model_stmt
                .query_map(params![item.id], |row| {
                    Ok(ProviderModel {
                        provider_id: item.id.clone(),
                        model_id: row.get(0)?,
                        display_name: row.get(1)?,
                        context_window_tokens: row
                            .get::<_, Option<i64>>(2)?
                            .map(|v| v.max(0) as u64),
                        source: row.get(3)?,
                    })
                })
                .map_err(|e| e.to_string())?;
            item.models = model_rows
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|e| e.to_string())?;
            if item.models.is_empty() && !item.default_model.trim().is_empty() {
                let context_window_tokens = conn
                    .query_row(
                        "SELECT descriptor_json FROM model_overrides WHERE provider_id=?1 AND model_id=?2",
                        params![item.id, item.default_model],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(|e| e.to_string())?
                    .and_then(|value| serde_json::from_str::<ModelOverride>(&value).ok())
                    .and_then(|value| value.context_window);
                item.models.push(ProviderModel {
                    provider_id: item.id.clone(),
                    model_id: item.default_model.clone(),
                    display_name: item.default_model.clone(),
                    context_window_tokens,
                    source: "legacy".into(),
                });
            }
        }
        Ok(items)
    }

    pub fn get_provider(&self, id: &str) -> Result<ProviderProfile, String> {
        self.list_providers()?
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| "供应商不存在".to_string())
    }

    pub fn save_provider(
        &self,
        input: &ProviderProfileInput,
        credential_ref: Option<&str>,
    ) -> Result<ProviderProfile, String> {
        let id = input
            .id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let name = input.name.trim();
        let base_url = input.base_url.trim().trim_end_matches('/');
        if name.is_empty() {
            return Err("请填写供应商显示名称".into());
        }
        if base_url.is_empty() {
            return Err("请填写 Base URL".into());
        }
        let mut seen = std::collections::HashSet::new();
        let models: Vec<_> = input
            .models
            .iter()
            .filter_map(|model| {
                let model_id = model.model_id.trim();
                if model_id.is_empty() || !seen.insert(model_id.to_string()) {
                    return None;
                }
                Some((
                    model_id.to_string(),
                    model
                        .display_name
                        .clone()
                        .filter(|v| !v.trim().is_empty())
                        .unwrap_or_else(|| model_id.to_string()),
                    model.context_window_tokens,
                    if model.source == "upstream" {
                        "upstream"
                    } else {
                        "manual"
                    },
                ))
            })
            .collect();
        if models.is_empty() {
            return Err("请至少添加一个模型".into());
        }
        let now = Utc::now().to_rfc3339();
        let default_model = models.first().map(|v| v.0.clone()).unwrap_or_default();
        let mut conn = self.conn.lock();
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO provider_profiles (id, kind, name, base_url, default_model, enabled, timeout_seconds, extra_headers_json, credential_ref, created_at, updated_at, api_type) VALUES (?1, 'open-ai-compatible', ?2, ?3, ?4, 1, 120, '{}', ?5, ?6, ?6, ?7) ON CONFLICT(id) DO UPDATE SET name=excluded.name, base_url=excluded.base_url, default_model=excluded.default_model, credential_ref=COALESCE(excluded.credential_ref, provider_profiles.credential_ref), updated_at=excluded.updated_at, api_type=excluded.api_type",
            params![id, name, base_url, default_model, credential_ref, now, provider_api_type_string(input.api_type)],
        ).map_err(|e| e.to_string())?;
        let existing_override_ids: Vec<String> = {
            let mut statement = tx
                .prepare("SELECT model_id FROM model_overrides WHERE provider_id=?1")
                .map_err(|e| e.to_string())?;
            let rows = statement
                .query_map(params![id], |row| row.get(0))
                .map_err(|e| e.to_string())?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|e| e.to_string())?
        };
        tx.execute(
            "DELETE FROM provider_models WHERE provider_id=?1",
            params![id],
        )
        .map_err(|e| e.to_string())?;
        for (index, (model_id, display_name, context, source)) in models.iter().enumerate() {
            tx.execute("INSERT INTO provider_models (provider_id, model_id, display_name, context_window_tokens, source, sort_order, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?7)", params![id, model_id, display_name, context.map(|v| v as i64), source, index as i64, now]).map_err(|e| e.to_string())?;
            let existing_override = tx
                .query_row(
                    "SELECT descriptor_json FROM model_overrides WHERE provider_id=?1 AND model_id=?2",
                    params![id, model_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|e| e.to_string())?
                .and_then(|value| serde_json::from_str::<ModelOverride>(&value).ok());
            if existing_override.is_some() || context.is_some() {
                let mut override_value = existing_override.unwrap_or_else(|| ModelOverride {
                    provider_id: id.clone(),
                    model_id: model_id.clone(),
                    ..Default::default()
                });
                override_value.provider_id = id.clone();
                override_value.model_id = model_id.clone();
                override_value.context_window = *context;
                tx.execute("INSERT INTO model_overrides (provider_id, model_id, descriptor_json) VALUES (?1,?2,?3) ON CONFLICT(provider_id, model_id) DO UPDATE SET descriptor_json=excluded.descriptor_json", params![id, model_id, serde_json::to_string(&override_value).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
            }
        }
        for model_id in existing_override_ids {
            if !models
                .iter()
                .any(|model| model.0.as_str() == model_id.as_str())
            {
                tx.execute(
                    "DELETE FROM model_overrides WHERE provider_id=?1 AND model_id=?2",
                    params![id, model_id],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        drop(conn);
        self.get_provider(&id)
    }

    pub fn get_provider_credential_ref(&self, id: &str) -> Result<Option<String>, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT credential_ref FROM provider_profiles WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())
        .map(|v| v.flatten())
    }

    pub fn delete_provider(&self, id: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        let transaction = conn
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "DELETE FROM model_overrides WHERE provider_id=?1",
                params![id],
            )
            .map_err(|error| error.to_string())?;
        let changed = transaction
            .execute("DELETE FROM provider_profiles WHERE id=?1", params![id])
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err("供应商不存在".to_string());
        }
        transaction.commit().map_err(|error| error.to_string())
    }

    pub fn get_model_override(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Option<ModelOverride>, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT descriptor_json FROM model_overrides WHERE provider_id=?1 AND model_id=?2",
            params![provider_id, model_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .map(|value| serde_json::from_str(&value).map_err(|e| e.to_string()))
        .transpose()
    }

    pub fn save_model_override(&self, value: &ModelOverride) -> Result<ModelOverride, String> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO model_overrides (provider_id, model_id, descriptor_json) VALUES (?1, ?2, ?3) ON CONFLICT(provider_id, model_id) DO UPDATE SET descriptor_json=excluded.descriptor_json",
            params![value.provider_id, value.model_id, serde_json::to_string(value).map_err(|e| e.to_string())?],
        ).map_err(|e| e.to_string())?;
        Ok(value.clone())
    }

    pub fn list_mcp_servers(&self) -> Result<Vec<McpServerConfig>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT config_json, status, last_error FROM mcp_servers ORDER BY name")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                let json: String = row.get(0)?;
                let mut item: McpServerConfig = serde_json::from_str(&json).unwrap();
                item.status = row.get(1)?;
                item.last_error = row.get(2)?;
                Ok(item)
            })
            .map_err(|e| e.to_string())?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row.map_err(|e| e.to_string())?);
        }
        Ok(items)
    }

    pub fn save_mcp_server(&self, input: &McpServerConfig) -> Result<McpServerConfig, String> {
        let mut item = input.clone();
        if item.id.trim().is_empty() {
            item.id = Uuid::new_v4().to_string();
        }
        item.updated_at = Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute("INSERT INTO mcp_servers (id, name, scope, project_id, transport, config_json, enabled, status, last_error, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) ON CONFLICT(id) DO UPDATE SET name=excluded.name, scope=excluded.scope, project_id=excluded.project_id, transport=excluded.transport, config_json=excluded.config_json, enabled=excluded.enabled, status=excluded.status, last_error=excluded.last_error, updated_at=excluded.updated_at",
            params![item.id, item.name, enum_json(item.scope), item.project_id, enum_json(item.transport), serde_json::to_string(&item).unwrap(), item.enabled, item.status, item.last_error, item.updated_at]).map_err(|e| e.to_string())?;
        Ok(item)
    }

    pub fn delete_mcp_server(&self, id: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        let changed = conn
            .execute("DELETE FROM mcp_servers WHERE id=?1", params![id])
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err("MCP 服务已禁用".to_string());
        }
        Ok(())
    }

    pub fn update_mcp_health(
        &self,
        id: &str,
        status: &str,
        last_error: Option<&str>,
    ) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE mcp_servers SET status=?2, last_error=?3, updated_at=?4 WHERE id=?1",
            params![id, status, last_error, Utc::now().to_rfc3339()],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn save_mcp_discovery(
        &self,
        id: &str,
        tools: &[String],
        read_only_tools: &[String],
    ) -> Result<(), String> {
        let conn = self.conn.lock();
        let config_json: Option<String> = conn
            .query_row(
                "SELECT config_json FROM mcp_servers WHERE id=?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some(config_json) = config_json else {
            return Ok(());
        };
        let mut config: McpServerConfig =
            serde_json::from_str(&config_json).map_err(|error| error.to_string())?;
        config.discovered_tools = tools.to_vec();
        config.read_only_tools = read_only_tools
            .iter()
            .filter(|tool| tools.contains(tool))
            .cloned()
            .collect();
        config.status = "healthy".to_string();
        config.last_error = None;
        config.updated_at = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE mcp_servers SET config_json=?2, status='healthy', last_error=NULL, updated_at=?3 WHERE id=?1",
            params![id, serde_json::to_string(&config).map_err(|error| error.to_string())?, config.updated_at],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn get_settings(&self) -> Result<AppSettings, String> {
        let conn = self.conn.lock();
        let json: Option<String> = conn
            .query_row(
                "SELECT value_json FROM app_settings WHERE key='main'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        Ok(json
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_default())
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<AppSettings, String> {
        let conn = self.conn.lock();
        conn.execute("INSERT INTO app_settings (key, value_json) VALUES ('main', ?1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json", params![serde_json::to_string(settings).unwrap()]).map_err(|e| e.to_string())?;
        Ok(settings.clone())
    }

    pub fn thread_id_for_run(&self, run_id: &str) -> Result<String, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT thread_id FROM runs WHERE id=?1",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())
    }

    pub fn project_path_for_thread(&self, thread_id: &str) -> Result<String, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT p.path FROM projects p JOIN threads t ON t.project_id=p.id WHERE t.id=?1",
            params![thread_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())
    }

    pub fn project_id_for_thread(&self, thread_id: &str) -> Result<String, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT project_id FROM threads WHERE id=?1",
            params![thread_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())
    }

    pub fn get_mcp_server_any(&self, id: &str) -> Result<McpServerConfig, String> {
        let conn = self.conn.lock();
        let json: String = conn
            .query_row(
                "SELECT config_json FROM mcp_servers WHERE id=?1",
                params![id],
                |row| row.get(0),
            )
            .map_err(|_| "MCP 服务不存在".to_string())?;
        serde_json::from_str(&json).map_err(|error| error.to_string())
    }

    pub fn get_mcp_server(&self, id: &str) -> Result<McpServerConfig, String> {
        let server = self.get_mcp_server_any(id)?;
        if !server.enabled {
            return Err("MCP 服务已禁用".to_string());
        }
        Ok(server)
    }

    pub fn save_tool_call_started(
        &self,
        id: &str,
        run_id: &str,
        name: &str,
        arguments: &serde_json::Value,
    ) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO tool_calls (id, run_id, name, arguments_json, status, started_at) VALUES (?1, ?2, ?3, ?4, 'running', ?5)",
            params![id, run_id, name, arguments.to_string(), Utc::now().to_rfc3339()],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn finish_tool_call(&self, id: &str, result: &str, ok: bool) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE tool_calls SET result_text=?2, status=?3, completed_at=?4 WHERE id=?1",
            params![
                id,
                result,
                if ok { "completed" } else { "failed" },
                Utc::now().to_rfc3339()
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn create_approval(
        &self,
        id: &str,
        run_id: &str,
        tool_name: &str,
        summary: &str,
        arguments: &serde_json::Value,
    ) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO approvals (id, run_id, tool_name, summary, arguments_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, run_id, tool_name, summary, arguments.to_string(), Utc::now().to_rfc3339()],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE threads SET unread_approval=1, status='awaiting-approval' WHERE id=(SELECT thread_id FROM runs WHERE id=?1)",
            params![run_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn decide_approval(&self, id: &str, approved: bool) -> Result<(), String> {
        self.decide_approval_value(id, if approved { "approved" } else { "denied" })
    }

    pub fn decide_approval_value(&self, id: &str, decision: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE approvals SET decision=?2, decided_at=?3 WHERE id=?1 AND decision IS NULL",
            params![id, decision, Utc::now().to_rfc3339()],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE threads SET unread_approval=0 WHERE id=(SELECT r.thread_id FROM runs r JOIN approvals a ON a.run_id=r.id WHERE a.id=?1)",
            params![id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn save_change_checkpoint(
        &self,
        run_id: &str,
        project_id: &str,
        mutation: &FileMutation,
    ) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO change_checkpoints (id, run_id, project_id, manifest_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![Uuid::new_v4().to_string(), run_id, project_id, serde_json::to_string(mutation).map_err(|e| e.to_string())?, Utc::now().to_rfc3339()],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn save_context_snapshot(
        &self,
        thread_id: &str,
        run_id: &str,
        summary: &str,
        token_count: u64,
        source_message_ids: &[String],
    ) -> Result<ContextSnapshot, String> {
        let snapshot = ContextSnapshot {
            id: Uuid::new_v4().to_string(),
            thread_id: thread_id.to_string(),
            run_id: Some(run_id.to_string()),
            summary: summary.to_string(),
            token_count,
            start_message_id: source_message_ids.first().cloned(),
            end_message_id: source_message_ids.last().cloned(),
            source_message_ids: source_message_ids.to_vec(),
            active: true,
            created_at: Utc::now().to_rfc3339(),
        };
        let mut conn = self.conn.lock();
        let transaction = conn.transaction().map_err(|e| e.to_string())?;
        transaction
            .execute(
                "UPDATE context_snapshots SET active=0 WHERE thread_id=?1 AND active=1",
                params![thread_id],
            )
            .map_err(|e| e.to_string())?;
        transaction.execute(
            "INSERT INTO context_snapshots (id, thread_id, run_id, summary, token_count, start_message_id, end_message_id, source_message_ids_json, active, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9)",
            params![
                snapshot.id,
                snapshot.thread_id,
                snapshot.run_id,
                snapshot.summary,
                snapshot.token_count.min(i64::MAX as u64) as i64,
                snapshot.start_message_id,
                snapshot.end_message_id,
                serde_json::to_string(&snapshot.source_message_ids).map_err(|e| e.to_string())?,
                snapshot.created_at,
            ],
        ).map_err(|e| e.to_string())?;
        transaction.commit().map_err(|e| e.to_string())?;
        Ok(snapshot)
    }

    pub fn active_context_snapshot(
        &self,
        thread_id: &str,
    ) -> Result<Option<ContextSnapshot>, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT id, thread_id, run_id, summary, token_count, start_message_id, end_message_id, source_message_ids_json, active, created_at FROM context_snapshots WHERE thread_id=?1 AND active=1 ORDER BY created_at DESC LIMIT 1",
            params![thread_id],
            context_snapshot_from_row,
        ).optional().map_err(|e| e.to_string())
    }

    pub fn restore_context_snapshot(&self, snapshot_id: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        let changed = conn
            .execute(
                "UPDATE context_snapshots SET active=0 WHERE id=?1 AND active=1",
                params![snapshot_id],
            )
            .map_err(|e| e.to_string())?;
        if changed == 0 {
            return Err("Context checkpoint is already restored or does not exist".to_string());
        }
        Ok(())
    }

    pub fn change_checkpoint_entries(
        &self,
        run_id: &str,
    ) -> Result<Vec<(String, FileMutation)>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, manifest_json FROM change_checkpoints WHERE run_id=?1 ORDER BY rowid DESC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![run_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        let mut entries = Vec::new();
        for row in rows {
            let (id, json) = row.map_err(|e| e.to_string())?;
            entries.push((id, serde_json::from_str(&json).map_err(|e| e.to_string())?));
        }
        Ok(entries)
    }

    pub fn change_checkpoints(&self, run_id: &str) -> Result<Vec<FileMutation>, String> {
        self.change_checkpoint_entries(run_id)
            .map(|entries| entries.into_iter().map(|(_, mutation)| mutation).collect())
    }

    pub fn delete_change_checkpoint(&self, checkpoint_id: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM change_checkpoints WHERE id = ?1",
            params![checkpoint_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn clear_change_checkpoints(&self, run_id: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM change_checkpoints WHERE run_id = ?1",
            params![run_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn messages_for_provider(&self, thread_id: &str) -> Result<Vec<Message>, String> {
        let mut messages: Vec<Message> = self
            .get_thread(thread_id)?
            .messages
            .into_iter()
            .filter(|m| {
                matches!(
                    m.role,
                    MessageRole::User | MessageRole::Assistant | MessageRole::System
                )
            })
            .collect();
        for message in &mut messages {
            for attachment in message
                .attachments
                .iter()
                .filter(|item| item.kind == "text")
            {
                let bytes = fs::read(&attachment.snapshot_path)
                    .map_err(|error| format!("无法读取附件 {}: {error}", attachment.name))?;
                let text = decode_attachment_text(&bytes)
                    .ok_or_else(|| format!("无法解码文本附件: {}", attachment.name))?;
                message
                    .content
                    .push_str(&format!("\n\n[附件: {}]\n{}", attachment.name, text));
            }
        }
        let Some(snapshot) = self.active_context_snapshot(thread_id)? else {
            return Ok(messages);
        };
        let source_ids: std::collections::HashSet<&str> = snapshot
            .source_message_ids
            .iter()
            .map(String::as_str)
            .collect();
        let insert_at = messages
            .iter()
            .position(|message| source_ids.contains(message.id.as_str()));
        messages.retain(|message| !source_ids.contains(message.id.as_str()) || message.pinned);
        if let Some(index) = insert_at {
            messages.insert(
                index.min(messages.len()),
                Message {
                    id: format!("context-snapshot:{}", snapshot.id),
                    thread_id: thread_id.to_string(),
                    role: MessageRole::System,
                    content: snapshot.summary,
                    created_at: snapshot.created_at,
                    run_id: snapshot.run_id,
                    pinned: true,
                    attachments: Vec::new(),
                },
            );
        }
        Ok(messages)
    }
}

fn decode_attachment_text(bytes: &[u8]) -> Option<String> {
    crate::decode_text_attachment(bytes)
}

fn context_snapshot_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContextSnapshot> {
    let source_json: String = row.get(7)?;
    Ok(ContextSnapshot {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        run_id: row.get(2)?,
        summary: row.get(3)?,
        token_count: row.get::<_, i64>(4)?.max(0) as u64,
        start_message_id: row.get(5)?,
        end_message_id: row.get(6)?,
        source_message_ids: serde_json::from_str(&source_json).unwrap_or_default(),
        active: row.get::<_, i64>(8)? != 0,
        created_at: row.get(9)?,
    })
}

fn project_from_row(row: &rusqlite::Row<'_>) -> Project {
    let path: String = row.get(2).unwrap_or_default();
    Project {
        id: row.get(0).unwrap_or_default(),
        name: row.get(1).unwrap_or_default(),
        path: path.clone(),
        favorite: row.get::<_, i64>(3).unwrap_or_default() != 0,
        created_at: row.get(4).unwrap_or_default(),
        updated_at: row.get(5).unwrap_or_default(),
        git_branch: git_branch(Path::new(&path)),
    }
}

fn git_branch(path: &Path) -> Option<String> {
    let mut command = std::process::Command::new("git");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .args(["-C", &path.to_string_lossy(), "branch", "--show-current"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .filter(|v| !v.is_empty())
            } else {
                None
            }
        })
}

fn enum_json<T: serde::Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}
fn status_string(value: RunStatus) -> &'static str {
    match value {
        RunStatus::Idle => "idle",
        RunStatus::Queued => "queued",
        RunStatus::Reasoning => "reasoning",
        RunStatus::Streaming => "streaming",
        RunStatus::ToolRunning => "tool-running",
        RunStatus::AwaitingApproval => "awaiting-approval",
        RunStatus::Completed => "completed",
        RunStatus::Failed => "failed",
        RunStatus::Cancelled => "cancelled",
    }
}
fn parse_status(value: &str) -> RunStatus {
    match value {
        "queued" => RunStatus::Queued,
        "reasoning" => RunStatus::Reasoning,
        "streaming" => RunStatus::Streaming,
        "tool-running" => RunStatus::ToolRunning,
        "awaiting-approval" => RunStatus::AwaitingApproval,
        "completed" => RunStatus::Completed,
        "failed" => RunStatus::Failed,
        "cancelled" => RunStatus::Cancelled,
        _ => RunStatus::Idle,
    }
}
fn role_string(value: MessageRole) -> &'static str {
    match value {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    }
}
fn parse_role(value: &str) -> MessageRole {
    match value {
        "system" => MessageRole::System,
        "assistant" => MessageRole::Assistant,
        "tool" => MessageRole::Tool,
        _ => MessageRole::User,
    }
}
fn provider_api_type_string(value: ProviderApiType) -> &'static str {
    match value {
        ProviderApiType::Responses => "responses",
        ProviderApiType::ChatCompletions => "chat-completions",
    }
}
fn parse_provider_api_type(value: &str) -> ProviderApiType {
    if value == "responses" {
        ProviderApiType::Responses
    } else {
        ProviderApiType::ChatCompletions
    }
}
fn parse_provider_kind(value: &str) -> ProviderKind {
    match value {
        "open-ai" => ProviderKind::OpenAi,
        "anthropic" => ProviderKind::Anthropic,
        "gemini" => ProviderKind::Gemini,
        "open-router" => ProviderKind::OpenRouter,
        "ollama" => ProviderKind::Ollama,
        _ => ProviderKind::OpenAiCompatible,
    }
}
fn default_run_config() -> RunConfigSnapshot {
    RunConfigSnapshot {
        provider_id: String::new(),
        model_id: String::new(),
        thinking_level: ThinkingLevel::Medium,
        permission_mode: PermissionMode::WorkspaceAuto,
        max_output_tokens: None,
        created_at: Utc::now().to_rfc3339(),
        run_mode: RunMode::Agent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(prefix: &str) -> Self {
            let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn setup_db() -> (TestDir, Database, Project, ThreadSummary) {
        let base = TestDir::new("axiom-db-test");
        let workspace = base.0.join("workspace");
        let data = base.0.join("data");
        fs::create_dir_all(&workspace).unwrap();
        let db = Database::open(&data).unwrap();
        let project = db.add_project(&workspace).unwrap();
        let thread = db.create_thread(&project.id, Some("Test thread")).unwrap();
        (base, db, project, thread)
    }

    fn seed_legacy_provider(
        db: &Database,
        id: &str,
        kind: &str,
        name: &str,
        base_url: &str,
        default_model: &str,
        credential_ref: Option<&str>,
        api_type: &str,
    ) {
        let now = Utc::now().to_rfc3339();
        db.conn
            .lock()
            .execute(
                "INSERT INTO provider_profiles (id, kind, name, base_url, default_model, enabled, timeout_seconds, extra_headers_json, credential_ref, created_at, updated_at, api_type) VALUES (?1, ?2, ?3, ?4, ?5, 1, 120, '{}', ?6, ?7, ?7, ?8)",
                params![id, kind, name, base_url, default_model, credential_ref, now, api_type],
            )
            .unwrap();
    }

    fn config(provider: &str, model: &str) -> RunConfigSnapshot {
        RunConfigSnapshot {
            provider_id: provider.to_string(),
            model_id: model.to_string(),
            thinking_level: ThinkingLevel::Medium,
            permission_mode: PermissionMode::WorkspaceAuto,
            max_output_tokens: Some(4096),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            run_mode: RunMode::Agent,
        }
    }

    #[test]
    fn legacy_template_cleanup_removes_only_untouched_credential_free_records() {
        let base = TestDir::new("axiom-provider-migration-test");
        let data = base.0.join("data");
        let db = Database::open(&data).unwrap();
        seed_legacy_provider(
            &db,
            "openai",
            "open-ai",
            "OpenAI",
            "https://api.openai.com/v1",
            "gpt-5.4",
            None,
            "chat-completions",
        );
        seed_legacy_provider(
            &db,
            "anthropic",
            "anthropic",
            "Anthropic",
            "https://api.anthropic.com",
            "claude-sonnet-4-5",
            Some("provider:anthropic"),
            "chat-completions",
        );
        seed_legacy_provider(
            &db,
            "gemini",
            "gemini",
            "Google Gemini",
            "https://generativelanguage.googleapis.com",
            "gemini-2.5-pro",
            None,
            "chat-completions",
        );
        seed_legacy_provider(
            &db,
            "openrouter",
            "open-router",
            "OpenRouter",
            "https://openrouter.ai/api/v1",
            "openai/gpt-5.4",
            None,
            "chat-completions",
        );
        seed_legacy_provider(
            &db,
            "ollama-local",
            "ollama",
            "My local Ollama",
            "http://127.0.0.1:11434",
            "qwen3-coder",
            None,
            "chat-completions",
        );
        seed_legacy_provider(
            &db,
            "compatible",
            "open-ai-compatible",
            "OpenAI Compatible",
            "http://127.0.0.1:9000/v1",
            "local-model",
            None,
            "chat-completions",
        );
        let now = Utc::now().to_rfc3339();
        db.conn
            .lock()
            .execute(
                "INSERT INTO provider_models (provider_id, model_id, display_name, context_window_tokens, source, sort_order, created_at, updated_at) VALUES ('gemini', 'user-model', 'User model', 128000, 'manual', 0, ?1, ?1)",
                params![now],
            )
            .unwrap();
        db.conn
            .lock()
            .execute(
                "INSERT INTO model_overrides (provider_id, model_id, descriptor_json) VALUES ('openrouter', 'openai/gpt-5.4', ?1)",
                params![serde_json::json!({
                    "providerId": "openrouter",
                    "modelId": "openai/gpt-5.4",
                    "contextWindow": 256000
                }).to_string()],
            )
            .unwrap();
        drop(db);

        let reopened = Database::open(&data).unwrap();
        let providers = reopened.list_providers().unwrap();
        assert!(!providers.iter().any(|provider| provider.id == "openai"));
        for id in [
            "anthropic",
            "gemini",
            "openrouter",
            "ollama-local",
            "compatible",
        ] {
            assert!(
                providers.iter().any(|provider| provider.id == id),
                "{id} should be retained"
            );
        }
        let anthropic = providers
            .iter()
            .find(|provider| provider.id == "anthropic")
            .unwrap();
        assert!(anthropic.has_credential);
        assert!(anthropic.legacy);
        assert_eq!(
            reopened
                .get_provider_credential_ref("anthropic")
                .unwrap()
                .as_deref(),
            Some("provider:anthropic")
        );
        let gemini = providers
            .iter()
            .find(|provider| provider.id == "gemini")
            .unwrap();
        assert!(gemini
            .models
            .iter()
            .any(|model| model.model_id == "user-model"));
        assert!(
            providers
                .iter()
                .find(|provider| provider.id == "openrouter")
                .unwrap()
                .legacy
        );
    }

    #[test]
    fn legacy_template_cleanup_preserves_default_model_and_api_type_edits() {
        for (suffix, default_model, api_type) in [
            ("model", "user-selected-model", "chat-completions"),
            ("api", "gpt-5.4", "responses"),
        ] {
            let base = TestDir::new(&format!("axiom-provider-{suffix}-migration-test"));
            let data = base.0.join("data");
            let db = Database::open(&data).unwrap();
            seed_legacy_provider(
                &db,
                "openai",
                "open-ai",
                "OpenAI",
                "https://api.openai.com/v1",
                default_model,
                None,
                api_type,
            );
            drop(db);

            let reopened = Database::open(&data).unwrap();
            let provider = reopened.get_provider("openai").unwrap();
            assert!(provider.legacy);
            assert_eq!(provider.default_model, default_model);
            assert_eq!(
                provider.api_type,
                if api_type == "responses" {
                    ProviderApiType::Responses
                } else {
                    ProviderApiType::ChatCompletions
                }
            );
        }
    }

    #[test]
    fn provider_context_decodes_bomless_utf16_attachment_snapshots() {
        let (base, db, _project, thread) = setup_db();
        let path = base.0.join("utf16-snapshot");
        let mut bytes = Vec::new();
        for unit in "Attached plan".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        fs::write(&path, &bytes).unwrap();
        db.add_message_with_attachments(
            &thread.id,
            MessageRole::User,
            "Review this file",
            None,
            vec![AttachmentSnapshot {
                id: Uuid::new_v4().to_string(),
                name: "plan.txt".to_string(),
                mime_type: "text/plain".to_string(),
                size: bytes.len() as u64,
                sha256: "test-only".to_string(),
                snapshot_path: path.to_string_lossy().to_string(),
                kind: "text".to_string(),
            }],
        )
        .unwrap();

        let messages = db.messages_for_provider(&thread.id).unwrap();
        assert!(messages[0].content.contains("Attached plan"));
    }

    #[test]
    fn context_snapshot_replaces_only_unpinned_sources_and_can_be_restored() {
        let (_base, db, _project, thread) = setup_db();
        let first = db
            .add_message(&thread.id, MessageRole::User, "first", None)
            .unwrap();
        let pinned = db
            .add_message(&thread.id, MessageRole::Assistant, "keep me", None)
            .unwrap();
        let last = db
            .add_message(&thread.id, MessageRole::User, "last", None)
            .unwrap();
        db.conn
            .lock()
            .execute(
                "UPDATE messages SET pinned=1 WHERE id=?1",
                params![pinned.id],
            )
            .unwrap();
        let run = db.create_run(&thread.id, &config("p", "m")).unwrap();
        let source_ids = vec![first.id.clone(), pinned.id.clone(), last.id.clone()];
        let snapshot = db
            .save_context_snapshot(&thread.id, &run.id, "summary", 123, &source_ids)
            .unwrap();

        let provider_messages = db.messages_for_provider(&thread.id).unwrap();
        assert!(provider_messages
            .iter()
            .any(|message| message.id == format!("context-snapshot:{}", snapshot.id)));
        assert!(provider_messages
            .iter()
            .any(|message| message.id == pinned.id));
        assert!(!provider_messages
            .iter()
            .any(|message| message.id == first.id));
        assert!(!provider_messages
            .iter()
            .any(|message| message.id == last.id));

        db.restore_context_snapshot(&snapshot.id).unwrap();
        let restored = db.messages_for_provider(&thread.id).unwrap();
        assert!(restored.iter().any(|message| message.id == first.id));
        assert!(restored.iter().any(|message| message.id == pinned.id));
        assert!(restored.iter().any(|message| message.id == last.id));
        assert!(db.active_context_snapshot(&thread.id).unwrap().is_none());
    }

    #[test]
    fn only_latest_context_snapshot_is_active() {
        let (_base, db, _project, thread) = setup_db();
        let run = db.create_run(&thread.id, &config("p", "m")).unwrap();
        let first = db
            .save_context_snapshot(&thread.id, &run.id, "first", 10, &["a".into()])
            .unwrap();
        let second = db
            .save_context_snapshot(&thread.id, &run.id, "second", 20, &["b".into()])
            .unwrap();
        let detail = db.get_thread(&thread.id).unwrap();
        assert_eq!(
            detail
                .context_snapshots
                .iter()
                .filter(|item| item.active)
                .count(),
            1
        );
        assert!(
            !detail
                .context_snapshots
                .iter()
                .find(|item| item.id == first.id)
                .unwrap()
                .active
        );
        assert_eq!(
            db.active_context_snapshot(&thread.id).unwrap().unwrap().id,
            second.id
        );
    }

    #[test]
    fn run_configuration_snapshot_remains_immutable_after_updates() {
        let (_base, db, _project, thread) = setup_db();
        let mut original = config("provider-a", "model-a");
        let run = db.create_run(&thread.id, &original).unwrap();
        original.provider_id = "provider-b".to_string();
        original.model_id = "model-b".to_string();
        db.update_run(&run.id, &thread.id, RunStatus::Completed, None, None)
            .unwrap();
        let saved = db
            .get_thread(&thread.id)
            .unwrap()
            .runs
            .into_iter()
            .find(|item| item.id == run.id)
            .unwrap();
        assert_eq!(saved.config.provider_id, "provider-a");
        assert_eq!(saved.config.model_id, "model-a");
        assert_eq!(saved.config.thinking_level, ThinkingLevel::Medium);
        assert_eq!(saved.status, RunStatus::Completed);
    }

    #[test]
    fn provider_override_and_provider_are_deleted_together() {
        let (_base, db, _project, _thread) = setup_db();
        let input = ProviderProfileInput {
            id: Some("custom-provider".to_string()),
            kind: ProviderKind::OpenAiCompatible,
            name: "Custom".to_string(),
            base_url: "http://localhost/v1".to_string(),
            default_model: "custom-model".to_string(),
            enabled: true,
            timeout_seconds: 30,
            extra_headers: serde_json::json!({}),
            api_key: None,
            api_type: ProviderApiType::ChatCompletions,
            models: vec![ProviderModelInput {
                model_id: "custom-model".to_string(),
                display_name: None,
                context_window_tokens: Some(777_000),
                source: "manual".to_string(),
            }],
        };
        db.save_provider(&input, Some("provider:custom-provider"))
            .unwrap();
        db.save_model_override(&ModelOverride {
            provider_id: "custom-provider".to_string(),
            model_id: "custom-model".to_string(),
            context_window: Some(777_000),
            max_output_tokens: Some(2048),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            db.get_model_override("custom-provider", "custom-model")
                .unwrap()
                .unwrap()
                .context_window,
            Some(777_000)
        );

        db.delete_provider("custom-provider").unwrap();
        assert!(db.get_provider("custom-provider").is_err());
        assert!(db
            .get_model_override("custom-provider", "custom-model")
            .unwrap()
            .is_none());
        assert!(db.delete_provider("custom-provider").is_err());
    }

    #[test]
    fn provider_model_edits_preserve_hidden_overrides_and_remove_deleted_models() {
        let (_base, db, _project, _thread) = setup_db();
        let mut input = ProviderProfileInput {
            id: Some("override-provider".to_string()),
            kind: ProviderKind::OpenAiCompatible,
            name: "Override Provider".to_string(),
            base_url: "http://localhost/v1".to_string(),
            default_model: "model-a".to_string(),
            enabled: true,
            timeout_seconds: 120,
            extra_headers: serde_json::json!({}),
            api_key: None,
            api_type: ProviderApiType::Responses,
            models: vec![ProviderModelInput {
                model_id: "model-a".to_string(),
                display_name: None,
                context_window_tokens: Some(128_000),
                source: "manual".to_string(),
            }],
        };
        db.save_provider(&input, None).unwrap();
        db.save_model_override(&ModelOverride {
            provider_id: "override-provider".to_string(),
            model_id: "model-a".to_string(),
            context_window: Some(128_000),
            max_output_tokens: Some(4096),
            input_price_per_million: Some(1.5),
            ..Default::default()
        })
        .unwrap();

        input.models[0].context_window_tokens = None;
        db.save_provider(&input, None).unwrap();
        let preserved = db
            .get_model_override("override-provider", "model-a")
            .unwrap()
            .unwrap();
        assert_eq!(preserved.context_window, None);
        assert_eq!(preserved.max_output_tokens, Some(4096));
        assert_eq!(preserved.input_price_per_million, Some(1.5));

        input.models = vec![ProviderModelInput {
            model_id: "model-b".to_string(),
            display_name: None,
            context_window_tokens: None,
            source: "manual".to_string(),
        }];
        db.save_provider(&input, None).unwrap();
        assert!(db
            .get_model_override("override-provider", "model-a")
            .unwrap()
            .is_none());
    }

    #[test]
    fn mcp_discovery_health_disable_and_delete_are_persisted() {
        let (_base, db, _project, _thread) = setup_db();
        let mut server = McpServerConfig {
            id: "mcp-test".to_string(),
            name: "MCP Test".to_string(),
            scope: McpScope::Global,
            project_id: None,
            transport: McpTransport::Stdio,
            command: Some("node".to_string()),
            args: vec!["server.js".to_string()],
            cwd: None,
            url: None,
            env: serde_json::json!({}),
            headers: serde_json::json!({}),
            timeout_seconds: 30,
            enabled: true,
            status: "stopped".to_string(),
            last_error: None,
            discovered_tools: Vec::new(),
            disabled_tools: Vec::new(),
            read_only_tools: Vec::new(),
            updated_at: String::new(),
        };
        db.save_mcp_server(&server).unwrap();
        db.save_mcp_discovery(
            &server.id,
            &["read".to_string(), "search".to_string()],
            &["read".to_string()],
        )
        .unwrap();
        let discovered = db.get_mcp_server(&server.id).unwrap();
        assert_eq!(discovered.status, "healthy");
        assert_eq!(discovered.discovered_tools, vec!["read", "search"]);
        assert_eq!(discovered.read_only_tools, vec!["read"]);

        server = discovered;
        server.enabled = false;
        db.save_mcp_server(&server).unwrap();
        assert!(db.get_mcp_server(&server.id).is_err());
        assert!(!db.get_mcp_server_any(&server.id).unwrap().enabled);

        db.delete_mcp_server(&server.id).unwrap();
        assert!(db.get_mcp_server_any(&server.id).is_err());
        assert!(db.delete_mcp_server(&server.id).is_err());
    }

    #[test]
    fn paused_goal_resumes_same_run_and_preserves_turns_and_event_sequence() {
        let (_base, db, _project, thread) = setup_db();
        let mut goal_config = config("provider", "model");
        goal_config.run_mode = RunMode::Goal;
        let run = db.create_run(&thread.id, &goal_config).unwrap();
        db.create_goal(&run).unwrap();
        db.add_goal_turn(&run.id).unwrap();
        let mut accumulated_usage = UsageRecord::default();
        accumulated_usage.input_tokens = Some(321);
        accumulated_usage.output_tokens = Some(123);
        accumulated_usage.duration_ms = Some(4_567);
        db.update_run(
            &run.id,
            &thread.id,
            RunStatus::Cancelled,
            Some(&accumulated_usage),
            None,
        )
        .unwrap();
        db.update_goal_status(&run.id, "paused").unwrap();
        db.save_event(&AgentEvent {
            sequence: 7,
            run_id: run.id.clone(),
            thread_id: thread.id.clone(),
            kind: AgentEventKind::Status,
            status: RunStatus::Cancelled,
            content: None,
            message: None,
            usage: None,
            error: None,
            approval: None,
            tool_activity: None,
            created_at: Utc::now().to_rfc3339(),
        })
        .unwrap();

        let resumed = db.resume_goal_run(&run.id).unwrap();
        assert_eq!(resumed.id, run.id);
        assert_eq!(resumed.status, RunStatus::Queued);
        assert_eq!(resumed.usage.input_tokens, Some(321));
        assert_eq!(resumed.usage.output_tokens, Some(123));
        assert_eq!(resumed.usage.duration_ms, Some(4_567));
        assert_eq!(db.last_event_sequence(&run.id).unwrap(), 7);
        let detail = db.get_thread(&thread.id).unwrap();
        assert_eq!(detail.runs.len(), 1);
        assert_eq!(detail.goals[0].status, "running");
        assert_eq!(detail.goals[0].turn_count, 1);

        db.update_goal_status(&run.id, "completed").unwrap();
        assert!(db.resume_goal_run(&run.id).is_err());
    }

    #[test]
    fn goal_turn_status_tracks_approval_completion_and_blocking() {
        let (_base, db, _project, thread) = setup_db();
        let mut goal_config = config("provider", "model");
        goal_config.run_mode = RunMode::Goal;
        let run = db.create_run(&thread.id, &goal_config).unwrap();
        db.create_goal(&run).unwrap();
        let first_id = db.add_goal_turn(&run.id).unwrap();
        db.update_goal_turn_status(&run.id, "awaiting-approval")
            .unwrap();
        db.update_goal_turn_status(&run.id, "running").unwrap();
        db.update_goal_turn_status(&run.id, "completed").unwrap();
        let second_id = db.add_goal_turn(&run.id).unwrap();
        db.update_goal_turn_status(&run.id, "blocked").unwrap();

        let conn = db.conn.lock();
        let first: (String, Option<String>) = conn
            .query_row(
                "SELECT status, completed_at FROM goal_turns WHERE id=?1",
                params![first_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let second: (String, Option<String>) = conn
            .query_row(
                "SELECT status, completed_at FROM goal_turns WHERE id=?1",
                params![second_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(first.0, "completed");
        assert!(first.1.is_some());
        assert_eq!(second.0, "blocked");
        assert!(second.1.is_some());
    }

    #[test]
    fn checkpoint_entries_are_newest_first_and_individually_deletable() {
        let (_base, db, project, thread) = setup_db();
        let run = db
            .create_run(&thread.id, &config("provider", "model"))
            .unwrap();
        for path in ["src/first.rs", "src/second.rs"] {
            db.save_change_checkpoint(
                &run.id,
                &project.id,
                &FileMutation {
                    path: path.to_string(),
                    before: Some(format!("before {path}")),
                    operation: "write".to_string(),
                },
            )
            .unwrap();
        }
        let entries = db.change_checkpoint_entries(&run.id).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].1.path, "src/second.rs");
        db.delete_change_checkpoint(&entries[0].0).unwrap();
        let remaining = db.change_checkpoint_entries(&run.id).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].1.path, "src/first.rs");
    }

    #[test]
    fn run_events_reject_duplicate_sequences_instead_of_replacing_history() {
        let (_base, db, _project, thread) = setup_db();
        let run = db
            .create_run(&thread.id, &config("provider", "model"))
            .unwrap();
        let event = AgentEvent {
            sequence: 1,
            run_id: run.id.clone(),
            thread_id: thread.id.clone(),
            kind: AgentEventKind::TextDelta,
            status: RunStatus::Streaming,
            content: Some("original".to_string()),
            message: None,
            usage: None,
            error: None,
            approval: None,
            tool_activity: None,
            created_at: Utc::now().to_rfc3339(),
        };
        db.save_event(&event).unwrap();
        let mut duplicate = event;
        duplicate.content = Some("replacement".to_string());
        assert!(db.save_event(&duplicate).is_err());

        let conn = db.conn.lock();
        let stored: String = conn
            .query_row(
                "SELECT event_json FROM run_events WHERE run_id=?1 AND sequence=1",
                params![run.id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(stored.contains("original"));
        assert!(!stored.contains("replacement"));
    }

    #[test]
    fn interrupted_run_recovers_persisted_partial_output_once() {
        let base = TestDir::new("axiom-db-recovery-test");
        let workspace = base.0.join("workspace");
        let data = base.0.join("data");
        fs::create_dir_all(&workspace).unwrap();
        let db = Database::open(&data).unwrap();
        let project = db.add_project(&workspace).unwrap();
        let thread = db.create_thread(&project.id, Some("Recovery")).unwrap();
        let run = db
            .create_run(&thread.id, &config("provider", "model"))
            .unwrap();
        for (sequence, content) in [(1, "partial "), (2, "output")] {
            db.save_event(&AgentEvent {
                sequence,
                run_id: run.id.clone(),
                thread_id: thread.id.clone(),
                kind: AgentEventKind::TextDelta,
                status: RunStatus::Streaming,
                content: Some(content.to_string()),
                message: None,
                usage: None,
                error: None,
                approval: None,
                tool_activity: None,
                created_at: Utc::now().to_rfc3339(),
            })
            .unwrap();
        }
        drop(db);

        let recovered = Database::open(&data).unwrap();
        let detail = recovered.get_thread(&thread.id).unwrap();
        assert_eq!(detail.thread.status, RunStatus::Failed);
        let recovered_messages: Vec<_> = detail
            .messages
            .iter()
            .filter(|message| message.run_id.as_deref() == Some(run.id.as_str()))
            .collect();
        assert_eq!(recovered_messages.len(), 1);
        assert!(recovered_messages[0].content.contains("partial output"));
        assert!(recovered_messages[0].content.contains("interrupted run"));
        drop(recovered);

        let reopened = Database::open(&data).unwrap();
        let count = reopened
            .get_thread(&thread.id)
            .unwrap()
            .messages
            .iter()
            .filter(|message| message.run_id.as_deref() == Some(run.id.as_str()))
            .count();
        assert_eq!(count, 1);
    }
}
